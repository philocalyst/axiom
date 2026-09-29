//! Statements (LANGUAGE §3): `DATE SUBJECT PREDICATE [until DATE] [STRING] CODE*`.
//!
//! One line says one thing about one thing on a day, and what it says is read
//! from the shape of what follows its subject: never from what the subject's
//! name means, which is the model's to know. `01 flat` is an occurrence
//! whether or not `flat` is a contract, and `flat 3_050 USD monthly` is new
//! terms because an amount is followed by a cadence.

use axiom_core::{Day, Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

const EVENT_STATES: [(&str, EventState); 3] =
    [("settled", EventState::Settled), ("void", EventState::Void), ("returned", EventState::Returned)];

const BUDGET_PERIODS: [(&str, Period); 2] = [("monthly", Period::Month), ("yearly", Period::Year)];

/// What the end of a statement adds to what it says, in any order.
#[derive(Default)]
struct Tail<'s> {
    until: Option<(Day, Loc)>,
    description: Option<(Text<'s>, Loc)>,
    due: Option<(Due, Loc)>,
    purpose: Option<(Purpose<'s>, Loc)>,
}

impl<'s> Parser<'s> {
    /// A dated line about `subject`, whose header and lines are read here. An
    /// `amount` already read after the subject is the start of the predicate.
    pub fn statement(
        &mut self,
        line: &mut Line<'s>,
        date: Day,
        subject: Subject<'s>,
        amount: Option<Amount<'s>>,
    ) -> Parse<()> {
        let mut statement = self.said(date, subject, amount)?;
        let header = self.end_header(line)?;
        let takes_lines = matches!(statement.predicate, Predicate::Occurrence { .. } | Predicate::Terms(_) | Predicate::Owes(_));
        // Lines under anything else are reported as belonging to nothing.
        if takes_lines {
            statement.body = self.body(line, Scope::Dated(date))?;
        }
        self.check_claim(&statement, header.loc)?;
        self.emit(&header, statement, ItemKind::Statement);
        Ok(())
    }

    /// The statement a header line says, without the lines under it.
    pub fn said(&mut self, date: Day, subject: Subject<'s>, amount: Option<Amount<'s>>) -> Parse<Statement<'s>> {
        let mut predicate = self.predicate(subject, amount)?;
        let codes = self.mark::<Code>();
        let tail = self.statement_tail(date, &predicate)?;
        if let Predicate::Owes(owes) = &mut predicate {
            (owes.due, owes.purpose) = (tail.due.map(|(due, _)| due), tail.purpose.map(|(purpose, _)| purpose));
        }
        Ok(Statement {
            date,
            subject,
            predicate,
            until: tail.until.map(|(day, _)| day),
            description: tail.description.map(|(text, _)| text),
            codes: self.since(codes),
            body: Body::default(),
        })
    }

    /// What follows the subject: an amount already read, or the words and
    /// tokens that say what the line is.
    fn predicate(&mut self, subject: Subject<'s>, amount: Option<Amount<'s>>) -> Parse<Predicate<'s>> {
        if let Some(amount) = amount {
            // An amount says what it is by what follows: a cadence makes it terms.
            return match self.at_cadence() {
                true => self.terms(Some(Payment::Fixed(amount)), false).map(|terms| Predicate::Terms(self.push(terms))),
                false => Ok(Predicate::Occurrence { amount: Some(amount) }),
            };
        }
        let token = self.peek();
        match (subject, token.tok) {
            (Subject::Budget(_), _) => self.allowance().map(Predicate::Budget),
            // A commodity's amount is its price.
            (Subject::Unit(_), Tok::Number(_)) => self.measured().map(Predicate::Price),
            (_, Tok::Punct(Punct::Eq)) => self.assertion(),
            (_, Tok::Name(word)) => self.word_predicate(word),
            (Subject::Name(_), Tok::Eol | Tok::Str(_) | Tok::Code(_)) => Ok(Predicate::Occurrence { amount: None }),
            (Subject::Unit(_), Tok::Eol | Tok::Str(_) | Tok::Code(_)) => {
                Err(self.expected("expected-amount", "what one unit cost, such as `280.14 USD`"))
            }
            _ => Err(self.expected("expected-predicate", "what the line says: `= AMOUNT`, `owes`, `ends`, or a property")),
        }
    }

    /// A predicate that starts with a word.
    fn word_predicate(&mut self, word: &str) -> Parse<Predicate<'s>> {
        match word {
            "owes" => self.owes(),
            "ends" => Ok(self.bump_as(Predicate::Ends)),
            "waived" => Ok(self.bump_as(Predicate::Waived)),
            // `basis zero` is a kind's property; `basis 12_000 USD` is an asset arriving.
            "basis" if matches!(self.lexer.peek_second().tok, Tok::Number(_) | Tok::Name("empty")) => self.basis(),
            "split" => self.split(),
            "settled" | "void" | "returned" => {
                self.choose(&EVENT_STATES, "unknown-event-state", "settlement state").map(|(state, _)| Predicate::Event(state))
            }
            "about" | "buy" => self.terms(None, false).map(|terms| Predicate::Terms(self.push(terms))),
            _ if self.at_cadence() => self.terms(None, false).map(|terms| Predicate::Terms(self.push(terms))),
            _ => self.prop(true).map(Predicate::Property),
        }
    }

    /// `= [-]AMOUNT [! [STRING] | via NAME]`
    fn assertion(&mut self) -> Parse<Predicate<'s>> {
        self.bump();
        let amount = self.signed_amount()?;
        let gap = match self.tok() {
            Tok::Punct(Punct::Bang) => Gap::Waived(self.waiver()?),
            Tok::Name("via") => {
                self.bump();
                Gap::Via(self.name("expected-name", "who the difference is with, like `market`")?)
            }
            _ => Gap::Refused,
        };
        Ok(Predicate::Assert(Assertion { amount, gap }))
    }

    /// `owes CREDITOR [AMOUNT]`, the rest of a claim being the statement's tail.
    fn owes(&mut self) -> Parse<Predicate<'s>> {
        self.bump();
        let creditor = self.name("expected-name", "the party or owner it is owed to")?;
        let amount = match self.tok() {
            Tok::Number(_) | Tok::Name("empty") => Some(self.amount()?),
            _ => None,
        };
        Ok(Predicate::Owes(Owes { creditor, amount, due: None, purpose: None }))
    }

    /// `basis AMOUNT [since DATE]`
    fn basis(&mut self) -> Parse<Predicate<'s>> {
        self.bump();
        let amount = self.amount()?;
        let since = match self.eat_word("since") {
            Some(_) => Some(self.date("the day the asset was acquired, like `2019-03-01`")?),
            None => None,
        };
        Ok(Predicate::Basis { amount, since })
    }

    /// `split N for M`
    fn split(&mut self) -> Parse<Predicate<'s>> {
        self.bump();
        let numerator = self.split_count("the new number of units, like `2` in `split 2 for 1`")?;
        self.expect_word("for", "expected-for", "`for` and the old number of units, like `split 2 for 1`")?;
        let denominator = self.split_count("the old number of units, like `1` in `split 2 for 1`")?;
        Ok(Predicate::Split { numerator, denominator })
    }

    /// A number of units in a split: a positive number.
    fn split_count(&mut self, what: &str) -> Parse<Dec> {
        let token = self.peek();
        let Tok::Number(count) = token.tok else { return Err(self.expected("expected-number", what)) };
        if count.is_zero() {
            let diag = Diagnostic::error("bad-split", "a split cannot have zero units")
                .label(token.loc, "zero units would destroy every holding");
            return self.fail(diag);
        }
        self.bump();
        Ok(count)
    }

    /// `LIMIT monthly|yearly [carries]`, where a limit is an amount or `N% of #PURPOSE`.
    pub fn allowance(&mut self) -> Parse<Allowance<'s>> {
        let limit = match self.tok() {
            Tok::Percent(percent) => {
                self.bump();
                self.expect_word("of", "expected-of", "`of` and the purpose it is a share of, like `10% of #income`")?;
                let Tok::Purpose(of) = self.tok() else {
                    return Err(self.expected("expected-purpose", "the purpose it is a share of, like `#income`"));
                };
                self.bump();
                Limit::Share { percent, of }
            }
            _ => Limit::Fixed(self.amount()?),
        };
        let (per, _) = self.choose(&BUDGET_PERIODS, "unknown-period", "budget period")?;
        Ok(Allowance { limit, per, carries: self.eat_word("carries").is_some() })
    }

    /// What ends a statement, in any order: `until DATE`, a description, codes,
    /// and for a claim `due WHEN` and a purpose. Codes go to their table as
    /// they come.
    fn statement_tail(&mut self, date: Day, predicate: &Predicate<'s>) -> Parse<Tail<'s>> {
        let mut tail = Tail::default();
        let claim = matches!(predicate, Predicate::Owes(_));
        loop {
            let token = self.peek();
            match token.tok {
                Tok::Name("until") => {
                    self.bump();
                    let day = self.date_from(Some(date), "the last day it holds, like `2026-05-31`")?;
                    let clause = self.loc_from(token.loc.start as usize);
                    if !changes_for_a_while(predicate) {
                        return self.fail(until_is_for_changes(clause, what_it_says(predicate)));
                    }
                    self.once(&mut tail.until, "`until` clause", clause, day)?;
                }
                Tok::Str(text) => {
                    self.bump();
                    self.once(&mut tail.description, "description", token.loc, Text(text))?;
                }
                Tok::Code(code) => {
                    self.bump();
                    self.push(code);
                }
                Tok::Name("due") if claim => {
                    self.bump();
                    let due = self.due(Scope::Dated(date))?;
                    self.once(&mut tail.due, "`due` clause", self.loc_from(token.loc.start as usize), due)?;
                }
                Tok::Purpose(name) if claim => {
                    let purpose = self.purpose(name)?;
                    self.once(&mut tail.purpose, "purpose", self.loc_from(token.loc.start as usize), purpose)?;
                }
                _ => return Ok(tail),
            }
        }
    }

    /// Keeps `value` as the one clause of its kind, or reports the second.
    fn once<T>(&mut self, slot: &mut Option<(T, Loc)>, what: &str, at: Loc, value: T) -> Parse<()> {
        match slot {
            Some((_, first)) => Err(self.duplicate(what, at, *first)),
            None => {
                *slot = Some((value, at));
                Ok(())
            }
        }
    }

    /// A claim states an amount, or has items that make one; and its lines are items.
    pub fn check_claim(&mut self, statement: &Statement<'s>, header: Loc) -> Parse<()> {
        let Predicate::Owes(owes) = &statement.predicate else { return Ok(()) };
        if let Some(leg) = self.slice(statement.body.legs).first() {
            return self.fail(claim_takes_items(leg.loc));
        }
        match owes.amount.is_none() && statement.body.items.is_empty() {
            true => self.fail(claim_without_amount(header)),
            false => Ok(()),
        }
    }
}

/// Whether what the predicate says is a change that can hold for a while.
fn changes_for_a_while(predicate: &Predicate<'_>) -> bool {
    matches!(predicate, Predicate::Terms(_) | Predicate::Waived | Predicate::Property(_) | Predicate::Budget(_))
}

/// What the predicate is, in a sentence: "a balance" for `= 5 USD`.
fn what_it_says(predicate: &Predicate<'_>) -> &'static str {
    match predicate {
        Predicate::Occurrence { .. } => "an occurrence",
        Predicate::Terms(_) => "new terms",
        Predicate::Waived => "a waiver",
        Predicate::Ends => "an ending",
        Predicate::Assert(_) => "a balance",
        Predicate::Owes(_) => "a claim",
        Predicate::Event(_) => "a settlement",
        Predicate::Price(_) => "a price",
        Predicate::Split { .. } => "a split",
        Predicate::Property(_) => "a property",
        Predicate::Basis { .. } => "a basis",
        Predicate::Budget(_) => "a budget",
    }
}

fn until_is_for_changes(until: Loc, says: &str) -> Diagnostic {
    Diagnostic::error("until-without-change", format!("`until` says how long a change holds, and {says} is not one"))
        .label(until, "nothing here holds for a while")
        .note("terms, a waiver, a property and a budget hold from their day; `until` names the last day")
        .fix("remove it", until, "")
}

fn claim_takes_items(leg: Loc) -> Diagnostic {
    Diagnostic::error("claim-takes-items", "a claim is broken down in items, not legs")
        .label(leg, "this line names an end")
        .note("the claim is between its two names; items say what it is for: `3_000 USD #design \"brand refresh\"`")
}

fn claim_without_amount(header: Loc) -> Diagnostic {
    Diagnostic::error("expected-amount", "a claim needs an amount, or items that make one")
        .label(header, "no amount here, and no items under it")
        .help("write the amount after the creditor: `owes studio 3_800 USD`, or indent items below")
}
