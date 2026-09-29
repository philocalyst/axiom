//! Journal items: everything that starts with a date, and `opening` blocks.

use axiom_core::{Day, Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::dates::empty_range;
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

const EVENT_STATES: [(&str, EventState); 3] =
    [("settled", EventState::Settled), ("void", EventState::Void), ("returned", EventState::Returned)];

impl<'s> Parser<'s> {
    /// A line that began with a date. What follows it says which kind of entry
    /// it is: a `^code` is an event, a commodity a price or split, anything
    /// else a flow, an assertion, a claim, or a contract's occurrence or end.
    pub fn journal_entry(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        match self.tok() {
            Tok::Code(code) => {
                self.bump();
                let (state, state_loc) = self.choose(&EVENT_STATES, "unknown-event-state", "settlement state")?;
                let header = self.end_header(line)?;
                self.emit(&header, Event { date, code, state, state_loc }, ItemKind::Event);
                Ok(())
            }
            // A commodity that starts a flow is a party: `VTI -> fidelity 198.12 USD`.
            Tok::Unit(_) if self.lexer.peek_second().tok != Tok::Punct("->") => self.price_or_split(line, date),
            _ => self.transaction(line, date),
        }
    }

    /// `[..DATE] FROM`, then what follows it: `=` makes an assertion, `owes` a
    /// claim, `ends` the end of a contract, the end of the line an occurrence of
    /// one, anything else a flow. Reading the first end before deciding is what
    /// lets one pass tell them apart.
    fn transaction(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let clauses = self.mark::<Clause>();
        let spread = self.spread(line, date)?;
        let from = self.side()?;
        // What is left of a contract's name: no amount to speak of, and no lots.
        if let (false, Some(end), None) = (spread, from.end, from.amount) {
            match self.tok() {
                Tok::Punct("=") => return self.assertion(line, date, end),
                Tok::Name("owes") if end.select.is_empty() => return self.claim_item(line, date, end.name),
                Tok::Name("ends") if end.select.is_empty() => return self.ending(line, date, end.name),
                _ => {}
            }
        }
        let amount = match from.amount {
            None => Some(None),
            Some(Quantity::Fixed(amount)) => Some(Some(amount)),
            Some(_) => None,
        };
        if let (false, true, Some(end), Some(amount)) = (spread, self.at_eol(), &from.end, amount) {
            if end.select.is_empty() {
                return self.occurrence(line, date, end.name, amount);
            }
        }
        let (mut flow, arrow) = self.flow_head(from, clauses)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow, arrow)?;
        self.emit(&header, Txn { date, flow }, ItemKind::Txn);
        Ok(())
    }

    /// `..DATE` after a transaction's date: it is paid that day and recognized
    /// over the range, which is what the clause `for DATE..DATE` says. Adds
    /// that clause.
    fn spread(&mut self, line: &Line<'s>, date: Day) -> Parse<bool> {
        let Some(dots) = self.eat("..") else { return Ok(false) };
        let last = self.date("the last day of the range, like `2026-12-31`")?;
        let range = self.loc_from(line.body);
        if last < date {
            return self.fail(empty_range(range, self.text(range), date, last));
        }
        self.t.clauses.push(Clause { at: dots.to(range), kind: ClauseKind::For(For::Period(date, last)) });
        Ok(true)
    }

    /// `DATE END = [-]AMOUNT [! [STRING] | via NAME]`.
    fn assertion(&mut self, line: &mut Line<'s>, date: Day, end: End<'s>) -> Parse<()> {
        self.bump();
        let amount = self.signed_amount()?;
        let gap = match self.tok() {
            Tok::Punct("!") => Gap::Waived(self.waiver()?),
            Tok::Name("via") => {
                self.bump();
                Gap::Via(self.name("expected-name", "who the difference is with, like `market`")?)
            }
            _ => Gap::Refused,
        };
        let header = self.end_header(line)?;
        self.emit(&header, Assert { date, end, amount, gap }, ItemKind::Assert);
        Ok(())
    }

    /// `DATE DEBTOR owes CREDITOR AMOUNT TAIL`, the debtor already read.
    fn claim_item(&mut self, line: &mut Line<'s>, date: Day, debtor: Name<'s>) -> Parse<()> {
        let claim = self.claim(date, debtor)?;
        let header = self.end_header(line)?;
        self.emit(&header, claim, ItemKind::Claim);
        Ok(())
    }

    /// `owes CREDITOR AMOUNT TAIL`, which is also a line of an opening.
    fn claim(&mut self, date: Day, debtor: Name<'s>) -> Parse<Claim<'s>> {
        self.bump();
        let creditor = self.name("expected-name", "the party or owner it is owed to")?;
        let amount = self.amount()?;
        Ok(Claim { date, debtor, creditor, amount, tail: self.tail(self.mark::<Clause>())? })
    }

    /// `DATE CONTRACT [AMOUNT]` with override legs below.
    fn occurrence(&mut self, line: &mut Line<'s>, date: Day, name: Name<'s>, amount: Option<Amount<'s>>) -> Parse<()> {
        let header = self.end_header(line)?;
        let legs = self.legs(line, |parser, leg_line| parser.leg(leg_line).map(drop))?;
        self.emit(&header, Occurrence { date, contract: name, amount, legs }, ItemKind::Occurrence);
        Ok(())
    }

    /// `DATE CONTRACT ends`
    fn ending(&mut self, line: &mut Line<'s>, date: Day, contract: Name<'s>) -> Parse<()> {
        self.bump();
        let header = self.end_header(line)?;
        self.emit(&header, Ending { date, contract }, ItemKind::Ending);
        Ok(())
    }

    /// `opening DATE` and its lines `END [SELECTOR] AMOUNT [basis AMOUNT] [since DATE]`,
    /// `ASSET basis AMOUNT [since DATE]` and `DEBTOR owes CREDITOR AMOUNT TAIL`.
    pub fn opening(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let date = self.item_date("the day the balances are stated, like `2024-12-31`")?;
        let header = self.end_header(line)?;
        let claims = self.mark::<Claim>();
        let lines = self.in_scope(Scope::Opening, |parser| {
            parser.legs(line, |parser, opening_line| {
                if let (Tok::Name(debtor), Tok::Name("owes")) = (parser.tok(), parser.lexer.peek_second().tok) {
                    parser.bump();
                    let claim = parser.claim(date, Name(debtor))?;
                    parser.expect_eol()?;
                    parser.push(claim);
                    return Ok(());
                }
                let leg = parser.leg(opening_line)?;
                match parser.get(leg).amount {
                    Quantity::Fixed(_) | Quantity::Whole => Ok(()),
                    _ => parser.fail(opening_needs_amount(parser.get(leg).loc)),
                }
            })
        });
        let claims = self.since(claims);
        self.emit(&header, Opening { date, lines: lines?, claims }, ItemKind::Opening);
        Ok(())
    }

    /// `DATE UNIT PRICE`, or `DATE UNIT split N for M`.
    fn price_or_split(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let unit = self.unit("expected-commodity", "a commodity such as `VTI`")?;
        if self.eat_word("split").is_some() {
            let numerator = self.split_count("the new number of units, like `2` in `split 2 for 1`")?;
            self.expect_word("for", "expected-for", "`for` and the old number of units, like `split 2 for 1`")?;
            let denominator = self.split_count("the old number of units, like `1` in `split 2 for 1`")?;
            let header = self.end_header(line)?;
            self.emit(&header, Split { date, unit, numerator, denominator }, ItemKind::Split);
            return Ok(());
        }
        let price = self.measured()?;
        let header = self.end_header(line)?;
        self.emit(&header, Price { date, unit, price }, ItemKind::Price);
        Ok(())
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
}

fn opening_needs_amount(leg: Loc) -> Diagnostic {
    Diagnostic::error("opening-amount", "an opening line says how much a place holds")
        .label(leg, "no amount here")
        .help("write the balance the statement shows: `checking 10_000 USD`")
}
