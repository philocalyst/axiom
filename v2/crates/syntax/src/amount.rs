//! Amounts: `84.20 USD`, `empty`, and the mistakes people make writing them.

use axiom_core::{Dec, Diagnostic};

use crate::ast::{Amount, Name};
use crate::lex::{Tok, Token};
use crate::parser::{Parse, Parser};

impl<'s> Parser<'s> {
    /// `NUMBER COMMODITY`, or `empty`.
    pub fn amount(&mut self) -> Parse<Amount<'s>> {
        let token = self.cursor.peek();
        if !matches!(token.tok, Tok::Name("empty")) {
            return self.measured();
        }
        self.cursor.bump();
        Ok(Amount { num: Dec::ZERO, unit: None, loc: token.loc })
    }

    /// `NUMBER COMMODITY`: a quantity of something. Prices are always this.
    pub fn measured(&mut self) -> Parse<Amount<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Number(num) => {
                self.cursor.bump();
                self.commodity_after(token, num)
            }
            Tok::Minus if matches!(self.cursor.peek_second().tok, Tok::Number(_)) => self.fail(negative_amount(token)),
            _ => Err(self.expected("expected-amount", "an amount such as `50 USD`")),
        }
    }

    fn commodity_after(&mut self, number: Token<'s>, num: Dec) -> Parse<Amount<'s>> {
        let next = self.cursor.peek();
        if let Tok::Unit(text) = next.tok {
            self.cursor.bump();
            return Ok(Amount { num, unit: Some(Name { text, loc: next.loc }), loc: number.loc.to(next.loc) });
        }
        let diag = self.missing_commodity(number, num, next);
        self.fail(diag)
    }

    /// Explains a number that is not followed by a commodity, guessing what
    /// was meant from what follows it.
    fn missing_commodity(&self, number: Token<'s>, num: Dec, next: Token<'s>) -> Diagnostic {
        let written = self.text(number.loc);
        lowercase_commodity(written, next)
            .or_else(|| bare_zero(number, num, next))
            .or_else(|| self.thousands_comma(written, next))
            .unwrap_or_else(|| {
                self.unexpected(next, "expected-commodity", "a commodity such as `USD`")
                    .help(format!("an amount is a number and its commodity, like `{written} USD`"))
            })
    }

    /// `1,000 USD`: a comma glued between digits where `_` belongs.
    fn thousands_comma(&self, written: &str, comma: Token<'s>) -> Option<Diagnostic> {
        let digits_after = self.cursor.peek_second();
        let glued = matches!(digits_after.tok, Tok::Number(_)) && digits_after.loc.start == comma.loc.end;
        if !matches!(comma.tok, Tok::Comma) || !glued {
            return None;
        }
        let diag = Diagnostic::error(
            "thousands-comma",
            format!("`{written},…` is not a number: thousands are separated with `_`"),
        )
        .label(comma.loc, "use `_` here")
        .fix("write `_` between digit groups: `1_000`", comma.loc, "_");
        Some(diag)
    }
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
fn bare_zero(number: Token<'_>, num: Dec, next: Token<'_>) -> Option<Diagnostic> {
    if !num.is_zero() || matches!(next.tok, Tok::Invalid(_)) {
        return None;
    }
    let diag = Diagnostic::error("bare-zero", "a bare `0` has no commodity; write `empty`")
        .label(number.loc, "zero of what?")
        .note("`empty` is the zero of every commodity, so it needs no unit")
        .fix("write `empty`", number.loc, "empty");
    Some(diag)
}

/// `-50 USD`: amounts have no sign; the arrow says which way the money goes.
fn negative_amount(minus: Token<'_>) -> Diagnostic {
    Diagnostic::error("negative-amount", "amounts carry no sign")
        .label(minus.loc, "remove this `-`")
        .note("the arrow gives the direction: money moves from the left of `->` to the right")
        .fix("remove the sign", minus.loc, "")
}
