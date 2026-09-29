//! Contracts (LANGUAGE §5): `contract NAME [with PARTY]`, its schedule and the
//! template an occurrence overrides.
//!
//! A line is the schedule (it starts with `about`, an amount that a cadence
//! follows, `buy` or a cadence), a nested law, a property (`deposit 2_350
//! USD`, `from 2025-07-01 until 2026-06-30`: a declaration's line, which the
//! model reads with the others), an item, or else a leg. A contract keeps the
//! lines that parse when one does not, so that its occurrences in the journal
//! are not errors as well. The terms a schedule states are the terms a
//! statement restates (`07-01 flat 3_050 USD monthly`), so both are read here.

use axiom_core::{Diagnostic, Loc, Span};

use crate::ast::*;
use crate::dates::not_a_day;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

#[rustfmt::skip]
const CADENCES: [(&str, Span); 5] = [
    ("daily", Span::days(1)), ("weekly", Span::days(7)), ("monthly", Span::months(1)), ("quarterly", Span::months(3)),
    ("yearly", Span::months(12)),
];

const DIRECTIONS: [(&str, Direction); 2] = [("from", Direction::From), ("into", Direction::Into)];

const WEEKDAYS: [(&str, u8); 7] =
    [("monday", 0), ("tuesday", 1), ("wednesday", 2), ("thursday", 3), ("friday", 4), ("saturday", 5), ("sunday", 6)];

/// The properties a contract's lines may start with; any other line is a leg or an item.
const PROPERTIES: [&str; 8] = ["from", "until", "covers", "business", "deposit", "loan", "escrow", "match"];

impl<'s> Parser<'s> {
    /// The rest of `contract NAME [with PARTY]`, after the keyword, and its body.
    pub fn contract(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "a contract name")?;
        let party = match self.eat_word("with") {
            Some(_) => Some(self.name("expected-name", "the party it is with, such as `lumen`")?),
            None => None,
        };
        if let (None, Tok::Name(_)) = (party, self.tok()) {
            let diag = self
                .unexpected(self.peek(), "expected-with", "`with` and the party it is with, or the end of the line")
                .fix("insert `with`", self.point(self.peek().loc.start), "with ");
            return self.fail(diag);
        }
        let header = self.keep_header(line);
        let (props, laws) = (self.mark::<Prop>(), self.mark::<Law>());
        let (legs, items) = (self.mark::<Leg>(), self.mark::<LineItem>());
        let mut schedule: Option<Schedule> = None;
        let body = self.block(line, true, |parser, child| match parser.tok() {
            Tok::Name("law") => parser.then(|parser| parser.law(child)).map(drop),
            Tok::Name(word) if PROPERTIES.contains(&word) => parser.property(child),
            _ if parser.at_schedule() => {
                let read = parser.schedule(child)?;
                match schedule.replace(read) {
                    Some(first) => Err(parser.duplicate("schedule line", read.at, first.at)),
                    None => Ok(()),
                }
            }
            _ if parser.at_item() => parser.line_item(child, Scope::Undated).map(drop),
            _ => parser.leg(child, Scope::Undated).map(drop),
        });
        if schedule.is_none() && body.is_ok() {
            self.report(missing_schedule(header.loc));
        }
        let body_lines = Body { legs: self.since(legs), items: self.since(items) };
        let (props, laws) = (self.since(props), self.since(laws));
        let contract = Contract { name, party, schedule, props, body: body_lines, laws, damaged: body.is_err() };
        self.emit(&header, contract, ItemKind::Contract);
        Ok(())
    }

    /// Whether the line is the schedule: it starts with `about`, `buy` or a
    /// cadence, or with an amount that a word follows (an item's amount is
    /// followed by nothing, or by a clause of its tail).
    fn at_schedule(&self) -> bool {
        match self.tok() {
            Tok::Name("about" | "buy") => true,
            Tok::Name(_) => self.at_cadence(),
            Tok::Number(_) => {
                let mut ahead = self.lexer.clone();
                ahead.bump();
                ahead.bump();
                matches!(ahead.peek().tok, Tok::Name(word) if !matches!(word, "via" | "for" | "due" | "basis"))
            }
            _ => false,
        }
    }

    /// Whether the next token starts a cadence.
    pub fn at_cadence(&self) -> bool {
        matches!(self.tok(), Tok::Name(word) if is_cadence(word))
    }

    /// `TERMS ["description"]` on a contract's own line.
    fn schedule(&mut self, line: &Line<'s>) -> Parse<Schedule<'s>> {
        let terms = self.terms(None, true)?;
        let description = match self.tok() {
            Tok::Str(text) => Some(Text(self.bump_as(text))),
            _ => None,
        };
        self.expect_eol()?;
        Ok(Schedule { at: self.loc_from(line.body), terms, description })
    }

    /// `[about] [AMOUNT | buy UNIT for AMOUNT] CADENCE [on DAY, …] [(from | into) NAME]
    /// [#purpose [of NAME]]`. A `payment` already read stands in for the amount.
    /// A contract's schedule needs `from` or `into`; terms restating it need not.
    pub fn terms(&mut self, payment: Option<Payment<'s>>, needs_holding: bool) -> Parse<Terms<'s>> {
        let about = payment.is_none() && self.eat_word("about").is_some();
        let payment = match (payment, self.tok()) {
            (Some(payment), _) => Some(payment),
            (None, Tok::Number(_)) => Some(Payment::Fixed(self.amount()?)),
            (None, Tok::Name("buy")) => {
                self.bump();
                let unit = self.unit("expected-commodity", "what it buys, such as `VTI`")?;
                self.keyword("for")?;
                Some(Payment::Buy { unit, spend: self.amount()? })
            }
            (None, _) => None,
        };
        let cadence = self.cadence()?;
        let on = if self.eat_word("on").is_some() { self.days_of_period()? } else { Many::EMPTY };
        let holding = match (self.tok(), needs_holding) {
            (Tok::Name("from" | "into"), _) | (_, true) => {
                let (direction, _) = self.choose(&DIRECTIONS, "unknown-direction", "direction")?;
                let name = self.name("expected-name", "the account it is paid from or into, such as `checking`")?;
                Some(Holding { direction, name })
            }
            _ => None,
        };
        let purpose = match self.tok() {
            Tok::Purpose(name) => Some(self.purpose(name)?),
            _ => None,
        };
        Ok(Terms { about, payment, cadence, on, holding, purpose })
    }

    /// `daily`, `weekly`, `monthly`, `quarterly`, `yearly`, `twice monthly` or `every SPAN`.
    fn cadence(&mut self) -> Parse<Cadence> {
        match self.tok() {
            Tok::Name("twice") => {
                self.bump();
                self.keyword("monthly").map(|()| Cadence::TwiceMonthly)
            }
            Tok::Name("every") => {
                self.bump();
                let span = |tok| if let Tok::Span(span) = tok { Some(Cadence::Every(span)) } else { None };
                self.take(span, "expected-span", "a span such as `2w`")
            }
            _ => self.choose(&CADENCES, "unknown-cadence", "cadence").map(|(span, _)| Cadence::Every(span)),
        }
    }

    /// After `on`: `15`, `last`, `04-15` or a weekday, and more of them after commas.
    fn days_of_period(&mut self) -> Parse<Many<On>> {
        let mark = self.mark::<On>();
        loop {
            let token = self.peek();
            let day = match token.tok {
                Tok::Number(_) => match self.day_of_month(token).filter(|day| (1..=31).contains(day)) {
                    Some(day) => self.bump_as(On::MonthDay(day)),
                    None => return self.fail(not_a_day(token.loc, self.text(token.loc))),
                },
                Tok::MonthDay(..) => self.month_day().map(|(month, day)| On::YearDay { month, day })?,
                Tok::Name("last") => self.bump_as(On::Last),
                Tok::Name(_) => self.choose(&WEEKDAYS, "unknown-day", "weekday").map(|(day, _)| On::Weekday(day))?,
                _ => return Err(self.expected("expected-day", "a day: `15`, `last`, `04-15` or a weekday")),
            };
            self.push(day);
            if self.eat(Punct::Comma).is_none() {
                return Ok(self.since(mark));
            }
        }
    }
}

/// Whether `word` starts a cadence: `monthly`, `twice monthly`, `every 2w`.
fn is_cadence(word: &str) -> bool {
    matches!(word, "twice" | "every") || CADENCES.iter().any(|(cadence, _)| *cadence == word)
}

fn missing_schedule(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-schedule", "a contract says how often, and for how much")
        .label(header, "this contract has no schedule")
        .help("add an indented line: `45 USD monthly on 8 from visa`")
}
