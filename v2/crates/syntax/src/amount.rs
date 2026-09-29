//! Amounts: `84.20 USD`, `empty`, and the mistakes people make writing them.

use axiom_core::{Dec, Diagnostic, Loc};

use crate::ast::Amount;
use crate::lex::{Tok, Token};
use crate::parser::{Parse, Parser};

/// How much of a file is read to learn which commodities it writes.
const SURVEY: usize = 1 << 20;

impl<'s> Parser<'s> {
    /// `NUMBER COMMODITY`, or `empty`.
    #[inline(always)]
    pub fn amount(&mut self) -> Parse<Amount<'s>> {
        let token = self.peek();
        match token.tok {
            Tok::Name("empty") => Ok(self.bump_as(Amount(self.text(token.loc)))),
            _ => self.measured(),
        }
    }

    /// An amount that may be negative, as after `=` in an assertion: an
    /// overdrawn account is `-50 USD`.
    pub fn signed_amount(&mut self) -> Parse<Amount<'s>> {
        let Some(minus) = self.eat("-") else { return self.amount() };
        let amount = self.measured()?;
        Ok(Amount(self.text(minus.to(self.loc_of(&amount)))))
    }

    /// `NUMBER COMMODITY`: a quantity of something. Prices are always this.
    #[inline(always)]
    pub fn measured(&mut self) -> Parse<Amount<'s>> {
        let token = self.peek();
        let Tok::Number(num) = token.tok else {
            let refund = self.at("-") && matches!(self.lexer.peek_second().tok, Tok::Number(_));
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
            return Ok(Amount(self.text(token.loc.to(next.loc))));
        }
        let diag = self.missing_commodity(token, num, next);
        self.fail(diag)
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

/// `-50 USD`: amounts have no sign; the arrow says which way the money goes.
fn negative_amount(minus: Token<'_>) -> Diagnostic {
    Diagnostic::error("negative-amount", "amounts carry no sign")
        .label(minus.loc, "a refund goes the other way")
        .note("the arrow gives the direction: to record money coming back, swap the two places")
        .help("write a refund as its own flow, from where the money came back to where it left")
}
