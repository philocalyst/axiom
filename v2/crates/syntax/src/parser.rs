//! The parser's state, and the small operations every grammar rule shares.
//!
//! Grammar rules are methods on [`Parser`], spread over the modules that own
//! each part of the language. They return [`Parse`]: `Ok` with what they built,
//! or `Err(Reported)` after recording a diagnostic, so `?` unwinds to the line
//! or item that can recover.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, FileId, Loc};

use crate::ast::{Exprs, Name};
use crate::cursor::Cursor;
use crate::lex::{Tok, Token};
use crate::lines::{Line, Lines};

/// The diagnostic for this failure is already in [`Parser::diags`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reported;

pub(crate) type Parse<T> = Result<T, Reported>;

pub(crate) struct Parser<'s> {
    pub src: &'s str,
    pub file: FileId,
    pub lines: Lines<'s>,
    /// Tokens of the line being parsed.
    pub cursor: Cursor<'s>,
    pub exprs: Exprs<'s>,
    /// How many expressions deep the parser is, so nesting can be limited.
    pub depth: u32,
    pub diags: Vec<Diagnostic>,
}

impl<'s> Parser<'s> {
    pub fn new(file: FileId, src: &'s str) -> Parser<'s> {
        Parser {
            src,
            file,
            lines: Lines::new(src, file),
            cursor: Cursor::new(src),
            exprs: Exprs::default(),
            depth: 0,
            diags: Vec::new(),
        }
    }

    /// Starts reading the tokens of `line`.
    pub fn begin_line(&mut self, line: &Line<'s>) {
        self.cursor.load(self.file, line.body, line.end);
    }

    pub fn text(&self, loc: Loc) -> &'s str {
        &self.src[loc.range()]
    }

    /// From `start` to the end of the last token consumed.
    pub fn loc_from(&self, start: usize) -> Loc {
        Loc::new(self.file, start as u32, self.cursor.prev_end())
    }

    /// The whole line, trailing comment included.
    pub fn line_loc(&self, line: &Line<'s>) -> Loc {
        Loc::new(self.file, line.body as u32, line.end as u32)
    }

    pub fn report(&mut self, diag: Diagnostic) -> Reported {
        self.diags.push(diag);
        Reported
    }

    pub fn fail<T>(&mut self, diag: Diagnostic) -> Parse<T> {
        Err(self.report(diag))
    }

    /// Whether the next token is the name `word`.
    pub fn at_word(&self, word: &str) -> bool {
        matches!(self.cursor.peek().tok, Tok::Name(text) if text == word)
    }

    pub fn eat_word(&mut self, word: &str) -> Option<Loc> {
        self.at_word(word).then(|| self.cursor.bump().loc)
    }

    pub fn name(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Name(text) => {
                self.cursor.bump();
                Ok(Name { text, loc: token.loc })
            }
            _ => Err(self.expected(code, what)),
        }
    }

    /// A name, where a plain integer also counts: `529` is a number by shape,
    /// yet it is also what the 529 plan's kind is called.
    pub fn name_like(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        let token = self.cursor.peek();
        match self.integer_name(token) {
            Some(name) => {
                self.cursor.bump();
                Ok(name)
            }
            None => self.name(code, what),
        }
    }

    pub fn unit(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        let token = self.cursor.peek();
        match token.tok {
            Tok::Unit(text) => {
                self.cursor.bump();
                Ok(Name { text, loc: token.loc })
            }
            _ => Err(self.expected(code, what)),
        }
    }

    /// The token as a name, if it is an integer written without separators.
    pub fn integer_name(&self, token: Token<'s>) -> Option<Name<'s>> {
        let text = self.text(token.loc);
        let integer = matches!(token.tok, Tok::Number(_)) && text.bytes().all(|b| b.is_ascii_digit());
        integer.then_some(Name { text, loc: token.loc })
    }

    /// The token as a year: exactly four digits.
    pub fn year(&self, token: Token<'s>) -> Option<i32> {
        let integer = self.integer_name(token)?;
        if integer.text.len() != 4 {
            return None;
        }
        integer.text.parse().ok()
    }

    /// One word of a small fixed vocabulary, with a spelling suggestion when
    /// the word is close to one of them.
    pub fn choose<T: Copy>(&mut self, table: &[(&str, T)], code: &'static str, what: &str) -> Parse<(T, Loc)> {
        let token = self.cursor.peek();
        let Tok::Name(word) = token.tok else {
            return Err(self.expected(code, &format!("a {what}")));
        };
        if let Some(&(_, value)) = table.iter().find(|(known, _)| *known == word) {
            self.cursor.bump();
            return Ok((value, token.loc));
        }
        let mut diag =
            Diagnostic::error(code, format!("unknown {what} `{word}`")).label(token.loc, format!("not a known {what}"));
        match closest(word, table.iter().map(|(known, _)| *known)) {
            Some(near) => diag = diag.fix(format!("did you mean `{near}`?"), token.loc, near),
            None => diag = diag.note(format!("the {what}s are {}", list_words(table))),
        }
        self.fail(diag)
    }
}

/// `a`, `b` and `c`: the words of a vocabulary, for a diagnostic.
pub(crate) fn list_words<T>(table: &[(&str, T)]) -> String {
    let words: Vec<String> = table.iter().map(|(word, _)| format!("`{word}`")).collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => words.concat(),
    }
}
