//! A token cursor over one line.
//!
//! A line is tokenized in one go into a buffer that is reused for every line,
//! so no line allocates once the buffer has grown. Lookahead is then just
//! indexing, and reading raw text (a `run` command) means skipping the tokens
//! that fell inside it.

use axiom_core::{FileId, Loc};

use crate::ast::Name;
use crate::lex::{Lexer, Tok, Token};

pub(crate) struct Cursor<'s> {
    src: &'s str,
    /// The current line's tokens. The last one is always `Eol`.
    tokens: Vec<Token<'s>>,
    next: usize,
    /// The end of the code on the line, before any trailing blanks or comment.
    line_end: usize,
    /// The end of the last consumed token.
    prev_end: u32,
}

impl<'s> Cursor<'s> {
    /// A cursor over an empty line.
    pub fn new(src: &'s str) -> Cursor<'s> {
        let end_of_line = Token { tok: Tok::Eol, loc: Loc::default() };
        Cursor { src, tokens: vec![end_of_line], next: 0, line_end: 0, prev_end: 0 }
    }

    /// Replaces the tokens with those of `src[body..end]`.
    pub fn load(&mut self, file: FileId, body: usize, end: usize) {
        self.tokens.clear();
        let mut lexer = Lexer::new(self.src, file, body, end);
        loop {
            let token = lexer.next_token();
            self.tokens.push(token);
            if matches!(token.tok, Tok::Eol) {
                break;
            }
        }
        (self.next, self.line_end, self.prev_end) = (0, end, body as u32);
    }

    #[inline]
    pub fn peek(&self) -> Token<'s> {
        self.tokens[self.next]
    }

    /// The token after the next one. Two tokens are enough to tell `? USD` (an
    /// unknown amount) from `?` (a place), and a year from the number after it.
    #[inline]
    pub fn peek_second(&self) -> Token<'s> {
        self.tokens[(self.next + 1).min(self.tokens.len() - 1)]
    }

    /// Consumes the next token. The line's end is never consumed: asking for
    /// more keeps returning it.
    #[inline]
    pub fn bump(&mut self) -> Token<'s> {
        let token = self.tokens[self.next];
        if !matches!(token.tok, Tok::Eol) {
            self.next += 1;
            self.prev_end = token.loc.end;
        }
        token
    }

    /// Consumes the next token if it is the punctuation `tok`. Only the kind is
    /// compared, which is one tag comparison instead of a walk over every
    /// variant's payload, so `tok` should carry none.
    #[inline]
    pub fn eat(&mut self, tok: Tok<'static>) -> Option<Token<'s>> {
        let same_kind = std::mem::discriminant(&self.peek().tok) == std::mem::discriminant(&tok);
        same_kind.then(|| self.bump())
    }

    /// Where the last consumed token ended.
    pub fn prev_end(&self) -> u32 {
        self.prev_end
    }

    /// The next non-blank run of characters, raw; what it means is the caller's
    /// business.
    pub fn raw_word(&mut self) -> Option<Name<'s>> {
        let start = self.raw_start()?;
        let len = self.src[start..self.line_end].find([' ', '\t']).unwrap_or(self.line_end - start);
        Some(self.consume_raw(start, start + len))
    }

    /// The rest of the line, raw, comments included.
    pub fn raw_rest(&mut self) -> Option<Name<'s>> {
        let start = self.raw_start()?;
        let end = start + self.src[start..self.line_end].trim_end().len();
        Some(self.consume_raw(start, end))
    }

    /// Where raw text starts: the next token, unless the line is over.
    fn raw_start(&self) -> Option<usize> {
        let token = self.peek();
        (!matches!(token.tok, Tok::Eol)).then_some(token.loc.start as usize)
    }

    /// Skips the tokens the raw text swallowed.
    fn consume_raw(&mut self, start: usize, end: usize) -> Name<'s> {
        let file = self.peek().loc.file;
        while !matches!(self.peek().tok, Tok::Eol) && (self.peek().loc.start as usize) < end {
            self.next += 1;
        }
        self.prev_end = end as u32;
        Name { text: &self.src[start..end], loc: Loc::new(file, start as u32, end as u32) }
    }
}
