//! Reporting what the parser found instead of what it needed.

use axiom_core::{Diagnostic, Loc};

use crate::lex::{Tok, Token};
use crate::malformed;
use crate::parser::{Parse, Parser, Reported};

/// A bracket pair, so an unclosed one can point back at where it opened.
#[derive(Clone, Copy)]
pub(crate) enum Delim {
    Paren,
    Bracket,
}

impl Delim {
    fn closer(self) -> Tok<'static> {
        match self {
            Delim::Paren => Tok::RParen,
            Delim::Bracket => Tok::RBracket,
        }
    }

    fn symbols(self) -> (&'static str, &'static str) {
        match self {
            Delim::Paren => ("(", ")"),
            Delim::Bracket => ("[", "]"),
        }
    }
}

impl<'s> Parser<'s> {
    /// Reports that the next token is not `what`. A token that is not a token
    /// at all (a bad date, an open string) is reported as what it is instead.
    pub fn expected(&mut self, code: &'static str, what: &str) -> Reported {
        let token = self.cursor.peek();
        let diag = self.unexpected(token, code, what);
        self.report(diag)
    }

    /// The diagnostic for finding `token` where `what` was needed, for callers
    /// that want to add help before reporting it.
    pub fn unexpected(&self, token: Token<'s>, code: &'static str, what: &str) -> Diagnostic {
        if let Tok::Invalid(kind) = token.tok {
            return malformed::diagnose(kind, token.loc, self.text(token.loc));
        }
        let diag = Diagnostic::error(code, format!("expected {what}, found {}", self.describe(token)))
            .label(token.loc, format!("expected {what}"));
        if self.is_doubled_slash(token) {
            return diag.note("`//` starts a comment only after whitespace, and a path separator is a single `/`");
        }
        diag
    }

    /// Whether `token` is one `/` of a `//` written without a space before it.
    fn is_doubled_slash(&self, token: Token<'s>) -> bool {
        let bytes = self.src.as_bytes();
        let touches = |at: Option<usize>| at.is_some_and(|at| bytes.get(at) == Some(&b'/'));
        matches!(token.tok, Tok::Slash)
            && (touches(Some(token.loc.end as usize)) || touches((token.loc.start as usize).checked_sub(1)))
    }

    fn describe(&self, token: Token<'s>) -> String {
        const SHOWN: usize = 24;
        if matches!(token.tok, Tok::Eol) {
            return "the end of the line".to_string();
        }
        let text = self.text(token.loc);
        match text.char_indices().nth(SHOWN) {
            Some((cut, _)) => format!("`{}…`", &text[..cut]),
            None => format!("`{text}`"),
        }
    }

    /// Consumes `tok`, or reports that `what` was expected.
    pub fn expect(&mut self, tok: Tok<'static>, code: &'static str, what: &str) -> Parse<Loc> {
        match self.cursor.eat(tok) {
            Some(token) => Ok(token.loc),
            None => Err(self.expected(code, what)),
        }
    }

    pub fn expect_word(&mut self, word: &str, code: &'static str, what: &str) -> Parse<Loc> {
        match self.eat_word(word) {
            Some(loc) => Ok(loc),
            None => Err(self.expected(code, what)),
        }
    }

    /// A line must end where its grammar does.
    pub fn expect_eol(&mut self) -> Parse<()> {
        if matches!(self.cursor.peek().tok, Tok::Eol) {
            return Ok(());
        }
        Err(self.expected("expected-end-of-line", "the end of the line"))
    }

    /// A clause that may appear once, written again at `second`.
    pub fn duplicate(&mut self, what: &str, second: Loc, first: Loc) -> Reported {
        self.report(
            Diagnostic::error("duplicate-clause", format!("only one {what} is allowed"))
                .label(second, format!("second {what}"))
                .context(first, format!("first {what}")),
        )
    }

    /// Consumes the closer of a bracket opened at `open`.
    pub fn close(&mut self, open: Loc, delim: Delim) -> Parse<Loc> {
        if let Some(token) = self.cursor.eat(delim.closer()) {
            return Ok(token.loc);
        }
        let (opener, closer) = delim.symbols();
        let token = self.cursor.peek();
        let diag = self
            .unexpected(token, "unclosed-delimiter", &format!("`{closer}`"))
            .context(open, format!("this `{opener}` is never closed"));
        self.fail(diag)
    }
}
