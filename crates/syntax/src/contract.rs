//! Contracts (LANGUAGE §7): `contract NAME [with PARTY]`, its schedule and the
//! template an occurrence overrides.
//!
//! A line is a schedule (it starts with `about`, `buy`, a cadence, or an amount
//! that a word follows), a nested law, an `also`, a `due`, a property (`deposit
//! 2_350 USD`, `share 120 SQFT for studio`: a declaration's line, which the
//! model reads with the others), a purpose or a description, an item, or else a
//! leg. A contract keeps the lines that parse when one does not, so that its
//! occurrences in the journal are not errors as well. The terms a schedule
//! states are the terms a change restates (`07-01 flat now 3_050 USD
//! monthly`), so both are read here.

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

/// The properties a contract's lines may start with; any other line is a leg
/// or an item. (`due` and `also` have lines of their own.)
#[rustfmt::skip]
const PROPERTIES: [&str; 13] = [
    "from", "until", "grace", "for", "covers", "prorated", "rising", "indexed", "share", "input", "deposit", "area",
    "loan",
];

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
        let (props, laws, alsos) = (self.mark::<Prop>(), self.mark::<Law>(), self.mark::<Also>());
        let (legs, items) = (self.mark::<Leg>(), self.mark::<LineItem>());
        let mut found = Found::default();
        let body = self.block(line, true, |parser, child| parser.contract_line(child, &mut found));
        if found.schedule.is_none() && found.standing.is_none() && body.is_ok() {
            self.report(missing_schedule(header.loc));
        }
        let template = Body { legs: self.since(legs), items: self.since(items) };
        let contract = Contract {
            name,
            party,
            schedule: found.schedule,
            standing: found.standing,
            purpose: found.purpose.map(|(purpose, _)| purpose),
            description: found.description.map(|(text, _)| text),
            deadline: found.deadline.map(|(deadline, _)| deadline),
            alsos: self.since(alsos),
            props: self.since(props),
            body: template,
            laws: self.since(laws),
            damaged: body.is_err(),
        };
        self.emit(&header, contract, ItemKind::Contract);
        Ok(())
    }

    /// One line of a contract's body.
    fn contract_line(&mut self, line: &mut Line<'s>, found: &mut Found<'s>) -> Parse<()> {
        let scope = Scope::Undated;
        match self.tok() {
            Tok::Name("law") => self.then(|parser| parser.law(line, None)).map(drop),
            Tok::Name("also") => self.also(line).map(drop),
            Tok::Name("due") => {
                let deadline = self.deadline()?;
                self.expect_eol()?;
                let at = self.line_loc(line);
                match found.deadline.replace((deadline, at)) {
                    Some((_, first)) => Err(self.duplicate("`due` line", at, first)),
                    None => Ok(()),
                }
            }
            Tok::Name(word) if PROPERTIES.contains(&word) => self.property(line, scope),
            _ if self.at_schedule() => {
                let read = self.schedule(line, found)?;
                let slot = match read.terms.payment {
                    Some(Payment::Buy { .. }) => &mut found.standing,
                    _ => &mut found.schedule,
                };
                match slot.replace(read) {
                    Some(first) => Err(self.duplicate("schedule line", read.at, first.at)),
                    None => Ok(()),
                }
            }
            Tok::Purpose(_) | Tok::Str(_) => {
                self.purpose_and_description(found)?;
                self.expect_eol()
            }
            _ if self.at_item() => self.line_item(line, scope).map(drop),
            _ => self.leg(line, scope).map(drop),
        }
    }

    /// Whether the line is a schedule: it starts with `about`, `buy` or a
    /// cadence, or with an amount that a word follows (an item's amount is
    /// followed by nothing, or by a clause of its tail).
    fn at_schedule(&self) -> bool {
        match self.tok() {
            Tok::Name("about" | "buy") => true,
            Tok::Name(_) => {
                if self.at_cadence() {
                    return true;
                }
                let mut ahead = self.lexer.clone();
                ahead.bump();
                matches!(ahead.peek().tok, Tok::Name(word) if is_cadence(word))
            }
            Tok::Code(_) => {
                let mut ahead = self.lexer.clone();
                ahead.bump();
                matches!(ahead.peek().tok, Tok::Name(word) if is_cadence(word))
            }
            Tok::Percent(_) | Tok::Fraction(..) | Tok::Punct(Punct::LParen) => {
                let mut ahead = self.lexer.clone();
                ahead.bump();
                !matches!(ahead.peek().tok, Tok::Eol | Tok::Str(_) | Tok::Code(_) | Tok::Purpose(_))
            }
            Tok::Number(_) => {
                let mut ahead = self.lexer.clone();
                ahead.bump();
                ahead.bump();
                matches!(ahead.peek().tok, Tok::Name(word) if !matches!(word, "via" | "for" | "due" | "basis" | "against"))
            }
            _ => false,
        }
    }

    /// Whether the next token starts a cadence.
    pub fn at_cadence(&self) -> bool {
        matches!(self.tok(), Tok::Name(word) if is_cadence(word))
    }

    /// A name followed by a cadence begins a computed terms amount, as in
    /// `now base monthly`.
    pub fn cadence_follows_name(&self) -> bool {
        let mut ahead = self.lexer.clone();
        ahead.bump();
        matches!(ahead.peek().tok, Tok::Name(word) if is_cadence(word))
    }

    /// `TERMS [#purpose [of NAME]] ["description"]` on a contract's own line.
    /// The purpose and description belong to the contract, whichever line says them.
    fn schedule(&mut self, line: &Line<'s>, found: &mut Found<'s>) -> Parse<Schedule<'s>> {
        let terms = self.terms(Scope::Undated, true)?;
        let at = self.loc_from(line.body);
        self.purpose_and_description(found)?;
        self.expect_eol()?;
        Ok(Schedule { at, terms })
    }

    /// A purpose and a description, as a contract says them.
    fn purpose_and_description(&mut self, found: &mut Found<'s>) -> Parse<()> {
        loop {
            let token = self.peek();
            match token.tok {
                Tok::Purpose(name) => {
                    let purpose = self.purpose(name)?;
                    let at = self.loc_from(token.loc.start as usize);
                    if let Some((_, first)) = found.purpose.replace((purpose, at)) {
                        return Err(self.duplicate("purpose", at, first));
                    }
                }
                Tok::Str(text) => {
                    self.bump();
                    if let Some((_, first)) = found.description.replace((Text(text), token.loc)) {
                        return Err(self.duplicate("description", token.loc, first));
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// `[about] AMOUNT CADENCE [on DAY, …] [(from | into) NAME]`, or `buy UNIT
    /// for AMOUNT CADENCE …`. A contract's schedule needs `from` or `into`;
    /// terms restating it need not.
    pub fn terms(&mut self, scope: Scope, needs_holding: bool) -> Parse<Terms<'s>> {
        let about = self.eat_word("about").is_some();
        let payment = match self.tok() {
            Tok::Number(_) => Some(Payment::Fixed(self.amount(scope)?)),
            Tok::Name("buy") => {
                self.bump();
                let unit = self.unit("expected-commodity", "what it buys, such as `VTI`")?;
                self.keyword("for")?;
                Some(Payment::Buy { unit, spend: self.amount(scope)? })
            }
            Tok::Percent(_) | Tok::Fraction(..) | Tok::Code(_) | Tok::Punct(Punct::LParen) => {
                Some(Payment::Fixed(self.amount(scope)?))
            }
            Tok::Name(_) if !self.at_cadence() => Some(Payment::Fixed(self.amount(scope)?)),
            _ => None,
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
        Ok(Terms { about, payment, cadence, on, holding })
    }

    /// `due SPAN [else ITEM]`
    fn deadline(&mut self) -> Parse<Deadline<'s>> {
        self.bump();
        let span = |tok| if let Tok::Span(span) = tok { Some(span) } else { None };
        let span = self.take(span, "expected-span", "a span such as `5d`")?;
        let otherwise = match self.eat_word("else") {
            Some(_) => Some(self.item_body(None, self.peek().loc.start as usize, Scope::Undated)?),
            None => None,
        };
        Ok(Deadline { span, otherwise })
    }

    /// `also ITEM | FLOW [when EXPR]`
    pub fn also(&mut self, line: &Line<'s>) -> Parse<Ref<Also<'s>>> {
        self.bump();
        let scope = Scope::Undated;
        if self.at_eol() {
            return Err(self.expected(
                "expected-also",
                "an item or a flow to add: `+ 2% of amount #fee` or `-> escrow 410 USD #escrow`",
            ));
        }
        let added = if self.at_item() {
            AlsoLine::Item(self.item_body(None, self.peek().loc.start as usize, scope)?)
        } else {
            let clauses = self.mark::<Clause>();
            let from = self.side(scope)?;
            AlsoLine::Flow(self.flow_head(from, scope, clauses)?.0)
        };
        let when = match self.eat_word("when") {
            Some(_) => Some(self.expression()?),
            None => None,
        };
        self.expect_eol()?;
        Ok(self.push(Also { line: added, when, loc: self.loc_from(line.body) }))
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

/// What a contract's lines have said so far.
#[derive(Default)]
struct Found<'s> {
    schedule: Option<Schedule<'s>>,
    standing: Option<Schedule<'s>>,
    purpose: Option<(Purpose<'s>, Loc)>,
    description: Option<(Text<'s>, Loc)>,
    deadline: Option<(Deadline<'s>, Loc)>,
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
