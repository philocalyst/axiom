//! Dated lines: a flow, or a statement about its first end, and `opening` blocks.

use axiom_core::{Day, Diagnostic, Loc};

use crate::ast::*;
use crate::dates::empty_range;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported, Scope};

impl<'s> Parser<'s> {
    /// A line that began with a date. What follows it says which it is: a
    /// `^code`, a `#purpose` or a commodity is what a statement is about, and
    /// anything else is a flow unless its first end is not followed by an arrow.
    pub fn journal_entry(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        match self.tok() {
            Tok::Code(code) => {
                self.bump();
                self.statement(line, date, Subject::Code(code), None)
            }
            // v3 wrote `DATE #code settled`; a purpose is the subject of a change.
            Tok::Purpose(_) if matches!(self.lexer.peek_second().tok, Tok::Name("settled" | "void" | "returned")) => {
                Err(self.hash_code(self.peek().loc))
            }
            Tok::Purpose(purpose) => {
                self.bump();
                self.statement(line, date, Subject::Purpose(purpose), None)
            }
            // A commodity that starts a flow is a party: `VTI -> fidelity 198.12 USD`.
            Tok::Unit(unit) if !matches!(self.lexer.peek_second().tok, Tok::Punct(Punct::Arrow)) => {
                self.bump();
                if let Tok::Number(_) = self.tok() {
                    return Err(self.price_needs_equals());
                }
                self.statement(line, date, Subject::Unit(Name(unit)), None)
            }
            _ => self.transaction(line, date),
        }
    }

    /// `[..DATE] FROM`, then what follows it: an arrow makes a flow, and
    /// anything else is what a statement says of the first end. Reading the
    /// first end before deciding is what lets one pass tell them apart.
    fn transaction(&mut self, line: &mut Line<'s>, date: Day) -> Parse<()> {
        let clauses = self.mark::<Clause>();
        let scope = Scope::Dated(date);
        let spread = self.spread(line, date)?;
        let from = self.side(scope)?;
        if let (false, Some(end), false) = (spread, &from.end, self.at(Punct::Arrow)) {
            let amount = match from.amount {
                None => Some(None),
                Some(Quantity::Amount(amount)) => Some(Some(amount)),
                Some(_) => None,
            };
            // A lot selector belongs to a flow, and is reported as a missing arrow.
            if let (true, Some(amount)) = (end.select.is_empty(), amount) {
                return self.statement(line, date, Subject::Name(end.name), amount);
            }
        }
        let (mut flow, arrow) = self.flow_head(from, scope, clauses)?;
        let header = self.end_header(line)?;
        self.flow_legs(line, &mut flow, scope, arrow)?;
        self.emit(&header, Txn { date, flow }, ItemKind::Txn);
        Ok(())
    }

    /// v4's first form of a price, `DATE VTI 280.14 USD`: a price is a value now.
    fn price_needs_equals(&mut self) -> Reported {
        let number = self.peek().loc;
        let diag = Diagnostic::error("price-needs-equals", "a price is a value: `VTI = 280.14 USD`")
            .label(number, "a commodity's price is written after `=`")
            .fix("insert `=`", self.point(number.start), "= ");
        self.report(diag)
    }

    /// `..DATE` after a transaction's date: it is paid that day and recognized
    /// over the range, which is what the clause `for DATE..DATE` says. Adds
    /// that clause.
    fn spread(&mut self, line: &Line<'s>, date: Day) -> Parse<bool> {
        let Some(dots) = self.eat(Punct::DotDot) else { return Ok(false) };
        let last = self.date("the last day of the range, like `2026-12-31`")?;
        let range = self.loc_from(line.body);
        if last < date {
            return self.fail(empty_range(range, self.text(range), date, last));
        }
        self.push(Clause { at: dots.to(range), kind: ClauseKind::For(For::Period(date, last)) });
        Ok(true)
    }

    /// `opening DATE` and its lines `END [SELECTOR] AMOUNT [basis AMOUNT] [since DATE]`,
    /// `ASSET basis AMOUNT [since DATE]` and `DEBTOR owes CREDITOR AMOUNT TAIL`.
    pub fn opening(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let date = self.item_date("the day the balances are stated, like `2024-12-31`")?;
        let header = self.end_header(line)?;
        let claims = self.mark::<Statement>();
        let lines = self.legs(line, |parser, opening_line| {
            if let (Tok::Name(debtor), Tok::Name("owes")) = (parser.tok(), parser.lexer.peek_second().tok) {
                parser.bump();
                let claim = parser.said(date, Subject::Name(Name(debtor)), None)?;
                parser.check_claim(&claim, parser.line_loc(opening_line))?;
                parser.expect_eol()?;
                parser.push(claim);
                return Ok(());
            }
            let leg = parser.leg(opening_line, Scope::Opening(date))?;
            match parser.get(leg).amount {
                Quantity::Amount(_) | Quantity::Whole => Ok(()),
                _ => parser.fail(opening_needs_amount(parser.get(leg).loc)),
            }
        });
        let claims = self.since(claims);
        self.emit(&header, Opening { date, lines: lines?, claims }, ItemKind::Opening);
        Ok(())
    }
}

fn opening_needs_amount(leg: Loc) -> Diagnostic {
    Diagnostic::error("opening-amount", "an opening line says how much a place holds")
        .label(leg, "no amount here")
        .help("write the balance the statement shows: `checking 10_000 USD`")
}
