//! Amounts: `84.20 USD`, `empty`, and the mistakes people make writing them.

use axiom_core::{Dec, Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok, Token};
use crate::parser::{Parse, Parser, Scope};

/// How much of a file is read to learn which commodities it writes.
const SURVEY: usize = 1 << 20;

impl<'s> Parser<'s> {
    /// An amount of §4, in the journal or, in a declaration, as any expression.
    /// A `@ PRICE` is not part of it: a flow's header and legs say their price
    /// as a clause. See [`Parser::priced_amount`] for the lines that do not.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    pub fn amount(&mut self, scope: Scope) -> Parse<Amount<'s>> {
        match scope {
            Scope::Undated => self.declared_amount(),
            _ => self.journal_amount(false),
        }
    }

    /// An amount that may say what it is worth at a price: `^inv-12[HR] @ 150 USD/HR`.
    pub fn priced_amount(&mut self, scope: Scope) -> Parse<Amount<'s>> {
        match scope {
            Scope::Undated => self.declared_amount(),
            _ => self.journal_amount(true),
        }
    }

    /// `NUMBER UNIT`, or `empty`.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    pub fn literal(&mut self) -> Parse<Literal<'s>> {
        let token = self.peek();
        match token.tok {
            Tok::Name("empty") => Ok(self.bump_as(Literal(self.text(token.loc)))),
            _ => self.measured(),
        }
    }

    /// A literal that may be negative, as after `=` in a value: an overdrawn
    /// account is `-50 USD`.
    pub fn signed_literal(&mut self) -> Parse<Literal<'s>> {
        let Some(minus) = self.eat(Punct::Minus) else { return self.literal() };
        let amount = self.measured()?;
        Ok(Literal(self.text(minus.to(self.loc_of(&amount)))))
    }

    /// `NUMBER UNIT`: a quantity of something. Prices are always this.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    pub fn measured(&mut self) -> Parse<Literal<'s>> {
        let token = self.peek();
        let Tok::Number(num) = token.tok else {
            let refund = self.at(Punct::Minus) && matches!(self.lexer.peek_second().tok, Tok::Number(_));
            return Err(if refund {
                self.report(negative_amount(token))
            } else {
                self.expected("expected-amount", "an amount such as `50 USD`")
            });
        };
        self.bump();
        let next = self.peek();
        if let Tok::Unit(_) = next.tok {
            self.bump();
            return Ok(Literal(self.text(token.loc.to(next.loc))));
        }
        let diag = self.missing_commodity(token, num, next);
        self.fail(diag)
    }

    /// In a declaration an amount is any expression of the law grammar; a lone
    /// literal or share is still what it says, so a declaration and a journal
    /// give the same amount the same form.
    fn declared_amount(&mut self) -> Parse<Amount<'s>> {
        if self.at_eol() {
            return Err(self.expected("expected-amount", "an amount such as `50 USD`, or an expression"));
        }
        let start = self.peek();
        let root = self.expression()?;
        let last = root.local() + 1 == self.exprs.len();
        // A number alone is no amount: it says which commodity is missing.
        if let (ExprKind::Num(num), true) = (&self.expr(root).kind, last) {
            let diag = self.missing_commodity(start, *num, self.peek());
            return self.fail(diag);
        }
        let simple = match (&self.expr(root).kind, last) {
            (ExprKind::Amount(literal), true) => Some(Amount::Literal(*literal)),
            _ => None,
        };
        Ok(simple.inspect(|_| drop(self.exprs.pop())).unwrap_or(Amount::Computed(root)))
    }

    /// The journal's amounts: `TERM [up to TERM]…`.
    // Inlined: the commonest amount by far is a literal, which needs no more than this.
    #[inline(always)]
    fn journal_amount(&mut self, priced: bool) -> Parse<Amount<'s>> {
        if let Tok::Number(_) = self.tok() {
            let literal = self.measured()?;
            if !matches!(self.tok(), Tok::Name("up") | Tok::Punct(Punct::At)) {
                return Ok(Amount::Literal(literal));
            }
            return self.more_amount(Amount::Literal(literal), priced);
        }
        let start = self.peek().loc;
        let (first, loc) = self.term(priced)?;
        self.more_of(first, loc, start.start as usize, priced)
    }

    /// What follows a literal that `up to` or `@` may continue.
    #[inline(never)]
    fn more_amount(&mut self, first: Amount<'s>, priced: bool) -> Parse<Amount<'s>> {
        let Amount::Literal(literal) = first else { unreachable!("a literal") };
        let loc = self.loc_of(&literal);
        let (amount, loc) = match priced && self.at(Punct::At) {
            true => (self.at_price(literal)?, self.loc_from(loc.start as usize)),
            false => (first, loc),
        };
        self.more_of(amount, loc, loc.start as usize, priced)
    }

    /// `[up to TERM]…` after a first term.
    fn more_of(&mut self, first: Amount<'s>, loc: Loc, start: usize, priced: bool) -> Parse<Amount<'s>> {
        if !self.at_up_to() {
            return Ok(first);
        }
        let mut left = self.node_of(first, loc);
        while self.at_up_to() {
            let first_node = self.expr(left).first;
            // `up to` is two words.
            self.bump();
            self.bump();
            let (right, right_loc) = self.term(priced)?;
            let right = self.node_of(right, right_loc);
            left = self.node(ExprKind::Binary(BinOp::UpTo, left, right), self.loc_from(start), first_node);
        }
        Ok(Amount::Computed(left))
    }

    /// `up to`, which is two words.
    fn at_up_to(&mut self) -> bool {
        matches!(self.tok(), Tok::Name("up")) && matches!(self.lexer.peek_second().tok, Tok::Name("to"))
    }

    /// One term of a journal amount: a literal, a share (`12%`, alone or `of`
    /// something), or a reference (`^bldg-water`); and, when a price is wanted, `@ PRICE`.
    // Inlined: what it returns is built where it is wanted, not copied up out of a call.
    #[inline(always)]
    fn term(&mut self, priced: bool) -> Parse<(Amount<'s>, Loc)> {
        let token = self.peek();
        let amount = match token.tok {
            Tok::Percent(percent) => Amount::Computed(self.share(token, ExprKind::Pct(percent))?.0),
            Tok::Fraction(top, bottom) => {
                if bottom == 0 {
                    return self.fail(zero_fraction(token.loc));
                }
                let of = "`of` and what it is a share of, like `1/3 of ^pge-jan`";
                match self.share(token, ExprKind::Fraction(top, bottom))? {
                    (root, true) => Amount::Computed(root),
                    (_, false) => return Err(self.expected("expected-of", of)),
                }
            }
            Tok::Code(_) => Amount::Computed(self.primary()?),
            _ => Amount::Literal(self.literal()?),
        };
        let loc = self.loc_from(token.loc.start as usize);
        if priced && self.at(Punct::At) {
            let quantity = self.node_of(amount, loc);
            let at = self.price(quantity, token.loc.start)?;
            return Ok((at, self.loc_from(token.loc.start as usize)));
        }
        Ok((amount, loc))
    }

    /// `QTY @ PRICE`, after the quantity that starts at `start` was read as a
    /// literal: the same amount at a price, for the statements that read the
    /// quantity as the header of a flow first.
    pub fn at_price(&mut self, quantity: Literal<'s>) -> Parse<Amount<'s>> {
        let loc = self.loc_of(&quantity);
        let node = self.node_at(ExprKind::Amount(quantity), loc);
        self.price(node, loc.start)
    }

    /// `@ PRICE` after `quantity`, a node that starts at byte `start`.
    fn price(&mut self, quantity: ExprId, start: u32) -> Parse<Amount<'s>> {
        let first = self.expr(quantity).first;
        self.bump();
        let price = self.measured()?;
        let price_node = self.node_at(ExprKind::Amount(price), self.loc_of(&price));
        Ok(Amount::Computed(self.node(ExprKind::At(quantity, price_node), self.loc_from(start as usize), first)))
    }

    /// After a percent or fraction: `of REF` makes it a share of that; nothing
    /// after it leaves it a share of what the amount is for, a lone node. The
    /// root, and whether there was an `of`.
    fn share(&mut self, token: Token<'s>, share: ExprKind<'s>) -> Parse<(ExprId, bool)> {
        let first = self.next_expr();
        self.bump();
        let share = self.node(share, token.loc, first);
        if !self.at_word("of") {
            return Ok((share, false));
        }
        self.bump();
        let what = "what it is a share of: a code such as `^bldg-water`, a name, or an amount";
        if !matches!(self.tok(), Tok::Code(_) | Tok::Name(_) | Tok::Number(_)) {
            return Err(self.expected("expected-reference", what));
        }
        // A number is the share's amount: it needs its commodity.
        let of = match self.tok() {
            Tok::Number(_) => {
                let literal = self.measured()?;
                self.node_at(ExprKind::Amount(literal), self.loc_of(&literal))
            }
            _ => self.primary()?,
        };
        Ok((self.node(ExprKind::Of(share, of), self.loc_from(token.loc.start as usize), first), true))
    }

    /// The amount as a node of the expression arena, for an operator to hold.
    /// `loc` is where the amount was written.
    fn node_of(&mut self, amount: Amount<'s>, loc: Loc) -> ExprId {
        let kind = match amount {
            Amount::Computed(root) => return root,
            Amount::Literal(literal) => ExprKind::Amount(literal),
        };
        self.node_at(kind, loc)
    }

    /// A node with no children.
    fn node_at(&mut self, kind: ExprKind<'s>, loc: Loc) -> ExprId {
        let first = self.next_expr();
        self.node(kind, loc, first)
    }

    /// Explains a number that is not followed by a commodity, guessing what
    /// was meant from what follows it.
    fn missing_commodity(&mut self, number: Token<'s>, num: Dec, next: Token<'s>) -> Diagnostic {
        let written = self.text(number.loc);
        if matches!(next.tok, Tok::Invalid(_)) {
            return self.unexpected(next, "expected-commodity", "a commodity such as `USD`");
        }
        lowercase_commodity(written, next)
            .or_else(|| bare_zero(number, num))
            .or_else(|| self.regrouped(number))
            .unwrap_or_else(|| self.which_commodity(number))
    }

    /// A number with no commodity at all: which of the file's would it be?
    fn which_commodity(&mut self, number: Token<'s>) -> Diagnostic {
        let written = self.text(number.loc);
        let end = self.point(number.loc.end);
        let units = self.units.get_or_insert_with(|| survey_units(self.src));
        let diag = Diagnostic::error("expected-commodity", format!("`{written}` has no commodity"))
            .label(number.loc, "which commodity?");
        if units.is_empty() {
            return diag.help(format!("an amount is a number and its commodity, like `{written} USD`"));
        }
        let diag = diag.note(format!("the amounts in this file are written in {}", units.join(", ")));
        units.iter().fold(diag, |diag, unit| diag.fix(format!("write `{written} {unit}`"), end, format!(" {unit}")))
    }

    /// `1,234.56` and `1.234,56`: digits grouped the way other places write
    /// them. The fix is the same number in Axiom's spelling, never another.
    fn regrouped(&self, number: Token<'s>) -> Option<Diagnostic> {
        let rest = &self.src[number.loc.start as usize..];
        let len = rest.bytes().position(|b| !matches!(b, b'0'..=b'9' | b'.' | b',' | b'_')).unwrap_or(rest.len());
        let run = rest[..len].trim_end_matches(['.', ',']);
        let (spelled, decimal_comma) = regroup(run).filter(|_| run.contains(',') || run.matches('.').count() > 1)?;
        let loc = Loc::new(self.id, number.loc.start, number.loc.start + run.len() as u32);
        let diag = if decimal_comma {
            Diagnostic::error("european-number", format!("`{run}` looks like a European amount: a comma for decimals"))
                .note("Axiom writes numbers with `.` for decimals and `_` between groups of digits")
        } else {
            let diag = Diagnostic::error(
                "thousands-comma",
                format!("`{run}` is not a number: thousands are separated with `_`"),
            );
            match run.matches(',').count() == 1 && !run.contains('.') {
                true => diag.note(format!("if `,` was a decimal separator, write `{}`", run.replace(',', "."))),
                false => diag,
            }
        };
        Some(diag.label(loc, format!("read as {spelled}")).fix(format!("write `{spelled}`"), loc, spelled))
    }
}

/// `run` spelled with `.` for decimals and `_` between groups, and whether its
/// decimal separator was a comma. The later of `.` and `,` is the decimal
/// separator; a lone comma is one unless three digits follow it.
fn regroup(run: &str) -> Option<(String, bool)> {
    let decimal = match (run.rfind('.'), run.rfind(',')) {
        (Some(dot), Some(comma)) => Some(dot.max(comma)),
        (None, Some(comma)) if run.matches(',').count() == 1 && run.len() - comma - 1 != 3 => Some(comma),
        _ => None,
    };
    let spelled: String = run
        .char_indices()
        .map(|(at, c)| match c {
            '.' | ',' if Some(at) == decimal => '.',
            '.' | ',' => '_',
            c => c,
        })
        .collect();
    Dec::parse(spelled.as_bytes())?;
    Some((spelled, decimal.is_some_and(|at| run.as_bytes()[at] == b',')))
}

/// The commodities a file writes, first three seen, from the first part of it.
fn survey_units(src: &str) -> Vec<&str> {
    let mut units: Vec<&str> = Vec::new();
    let mut previous: &[u8] = &[];
    for word in src.as_bytes()[..src.len().min(SURVEY)].split(u8::is_ascii_whitespace).filter(|word| !word.is_empty()) {
        let len = word.iter().take_while(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.')).count();
        let unit = std::str::from_utf8(&word[..len]).unwrap_or_default();
        if previous.last().is_some_and(u8::is_ascii_digit) && word[0].is_ascii_uppercase() && !units.contains(&unit) {
            units.push(unit);
        }
        previous = word;
        if units.len() == 3 {
            break;
        }
    }
    units
}

/// `50 usd`: a commodity written in lowercase.
fn lowercase_commodity(written: &str, word: Token<'_>) -> Option<Diagnostic> {
    let Tok::Name(text) = word.tok else { return None };
    if !text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return None;
    }
    let upper = text.to_ascii_uppercase();
    let diag = Diagnostic::error("lowercase-commodity", format!("commodities are written in capitals, not `{text}`"))
        .label(word.loc, "a commodity is uppercase")
        .fix(format!("write `{written} {upper}`"), word.loc, upper);
    Some(diag)
}

/// `0`: zero of nothing in particular, which is what `empty` is for.
fn bare_zero(number: Token<'_>, num: Dec) -> Option<Diagnostic> {
    let diag = Diagnostic::error("bare-zero", "a bare `0` has no commodity; write `empty`")
        .label(number.loc, "zero of what?")
        .note("`empty` is the zero of every commodity, so it needs no unit")
        .fix("write `empty`", number.loc, "empty");
    num.is_zero().then_some(diag)
}

/// `1/0`: a fraction of nothing.
pub(crate) fn zero_fraction(loc: Loc) -> Diagnostic {
    Diagnostic::error("zero-fraction", "a fraction cannot have zero as its denominator")
        .label(loc, "nothing is divided into zero parts")
}

/// `-50 USD`: amounts have no sign; the arrow says which way the money goes.
fn negative_amount(minus: Token<'_>) -> Diagnostic {
    Diagnostic::error("negative-amount", "amounts carry no sign")
        .label(minus.loc, "a refund goes the other way")
        .note("the arrow gives the direction: to record money coming back, swap the two places")
        .help("write a refund as its own flow, from where the money came back to where it left")
}
