//! Contracts (LANGUAGE §5): `contract NAME with PARTY`, its schedule and the
//! template legs an occurrence overrides.
//!
//! A line is the schedule (it starts with an amount or a cadence), a nested law,
//! a property (`deposit 2_350 USD`, `from 2025-07-01 until 2026-06-30`: a
//! declaration's line, which the model reads with the others), or else a leg. A
//! contract keeps the lines that parse when one does not, so that its
//! occurrences in the journal are not errors as well.

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

/// The properties a contract's lines may start with; any other line is a leg.
const PROPERTIES: [&str; 8] = ["from", "until", "covers", "business", "deposit", "loan", "escrow", "match"];

impl<'s> Parser<'s> {
    /// The rest of `contract NAME with PARTY`, after the keyword, and its body.
    pub fn contract(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "a contract name")?;
        self.keyword("with")?;
        let party = self.name("expected-name", "the party it is with, such as `lumen`")?;
        let header = self.keep_header(line);
        let (props, legs, laws) = (self.mark::<Prop>(), self.mark::<Leg>(), self.mark::<Law>());
        let mut schedule: Option<Schedule> = None;
        let body = self.children(line, |parser, child| match parser.tok() {
            Tok::Name("law") => parser.then(|parser| parser.law(child)).map(drop),
            Tok::Name(word) if PROPERTIES.contains(&word) => parser.property(child),
            _ if parser.at_schedule() => {
                let read = parser.schedule(child)?;
                match schedule.replace(read) {
                    Some(first) => Err(parser.duplicate("schedule line", read.at, first.at)),
                    None => Ok(()),
                }
            }
            _ => parser.leg(child, Scope::Undated).map(drop),
        });
        if schedule.is_none() && body.is_ok() {
            self.report(missing_schedule(header.loc));
        }
        let (props, legs, laws) = (self.since(props), self.since(legs), self.since(laws));
        let contract = Contract { name, party, schedule, props, legs, laws, damaged: body.is_err() };
        self.emit(&header, contract, ItemKind::Contract);
        Ok(())
    }

    /// Whether the line is the schedule: it starts with an amount or a cadence.
    fn at_schedule(&self) -> bool {
        let is_cadence = |word: &str| CADENCES.iter().any(|(cadence, _)| *cadence == word);
        match self.tok() {
            Tok::Number(_) => true,
            Tok::Name(word) => matches!(word, "buy" | "twice" | "every") || is_cadence(word),
            _ => false,
        }
    }

    /// `[AMOUNT | buy UNIT for AMOUNT] CADENCE [on DAY, …] (from | into) NAME
    /// [#purpose [of NAME]] ["description"]`
    fn schedule(&mut self, line: &Line<'s>) -> Parse<Schedule<'s>> {
        let payment = match self.tok() {
            Tok::Number(_) => Some(Payment::Fixed(self.amount()?)),
            Tok::Name("buy") => {
                self.bump();
                let unit = self.unit("expected-commodity", "what it buys, such as `VTI`")?;
                self.keyword("for")?;
                Some(Payment::Buy { unit, spend: self.amount()? })
            }
            _ => None,
        };
        let cadence = self.cadence()?;
        let on = if self.eat_word("on").is_some() { self.days_of_period()? } else { Many::EMPTY };
        let (direction, _) = self.choose(&DIRECTIONS, "unknown-direction", "direction")?;
        let holding = self.name("expected-name", "the account it is paid from or into, such as `checking`")?;
        let tail = self.tail(Scope::Schedule, self.mark::<Clause>())?;
        self.expect_eol()?;
        Ok(Schedule { at: self.loc_from(line.body), payment, cadence, on, direction, holding, tail })
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

fn missing_schedule(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-schedule", "a contract says how often, and for how much")
        .label(header, "this contract has no schedule")
        .help("add an indented line: `45 USD monthly on 8 from visa`")
}
