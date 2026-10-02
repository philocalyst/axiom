//! The parser's state, and the small operations every grammar rule shares.
//!
//! Grammar rules are methods on [`Parser`], spread over the modules that own
//! each part of the language. They return [`Parse`]: `Ok` with what they built,
//! or `Err(Reported)` after recording a diagnostic, so `?` unwinds to the line
//! or item that can recover.
//!
//! A parser builds one [`Piece`] of a file, appending to its tables as it goes,
//! and knows nothing about other parsers: a large file is cut into pieces, each
//! parsed by its own `Parser`, and the pieces are joined afterwards (see
//! `parse`). Indices it hands out say which piece they are in.

use std::ops::Range;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, FileId, Loc};

use crate::ast::*;
use crate::ast::{Piece, locate};
use crate::lex::{Lexer, Punct, Tok, Token};
use crate::lines::{Line, Lines};
use crate::malformed::{clip, diagnose};

/// The diagnostic for this failure is already in [`Parser::diags`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reported;

pub(crate) type Parse<T> = Result<T, Reported>;

/// Where a leg, an item or a tail is written: what it may add to the common
/// clauses, the day a short `due` or `until` date counts forward from, and
/// whether its amounts are the journal's or a declaration's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Scope {
    /// A flow, or the lines under one, on this day.
    Dated(Day),
    /// A statement, or the lines under one, on this day: its tail may say `until`.
    Statement(Day),
    /// The lines of an `opening` on this day, which say `since` when their
    /// parcels were acquired, and which an asset has with a `basis` and no
    /// amount.
    Opening(Day),
    /// A declaration's line (a contract's template, an `also`): no day, so no
    /// short dates, and amounts that are any expression of the law grammar.
    Undated,
}

impl Scope {
    /// The day it is written on, if it has one.
    pub fn day(self) -> Option<Day> {
        match self {
            Scope::Dated(day) | Scope::Statement(day) | Scope::Opening(day) => Some(day),
            Scope::Undated => None,
        }
    }

    /// Whether it is the lines of an `opening`.
    pub fn is_opening(self) -> bool {
        matches!(self, Scope::Opening(_))
    }
}

/// A parsed header line: where the item is, and what documents it.
pub(crate) struct Header<'s> {
    pub loc: Loc,
    pub doc: Option<Doc<'s>>,
}

pub(crate) struct Parser<'s> {
    pub src: &'s str,
    pub id: FileId,
    /// What its dates may leave out, from the file's folder and the headings above.
    pub folder: Folder,
    /// What has been parsed: the items, the expressions, and the tables.
    pub items: Vec<Item<'s>>,
    pub exprs: Vec<Expr<'s>>,
    pub t: Tables<'s>,
    /// The piece's number, which every index it makes says.
    piece: usize,
    pub lines: Lines<'s>,
    /// Tokens of the line being parsed.
    pub lexer: Lexer<'s>,
    /// How many expressions deep the parser is, so nesting can be limited.
    pub depth: u32,
    pub diags: Vec<Diagnostic>,
    /// The expression roots of the lists being read. Lists nest (a call inside
    /// an argument), so each is collected here and moved to its table whole.
    pub roots: Vec<ExprId>,
    /// The commodities the file writes, for the amount that names none. Found
    /// when first needed.
    pub units: Option<Vec<&'s str>>,
}

impl<'s> Parser<'s> {
    /// A parser for `src[range]`, which starts at the start of a line and is
    /// piece number `piece` of the file, whose dates start out as `folder` says.
    pub fn new(id: FileId, src: &'s str, range: Range<usize>, piece: usize, folder: Folder) -> Parser<'s> {
        Parser {
            src,
            id,
            folder,
            items: Vec::new(),
            exprs: Vec::new(),
            t: Tables::default(),
            piece,
            lines: Lines::new(src, id, range),
            lexer: Lexer::new(src, id),
            depth: 0,
            diags: Vec::new(),
            roots: Vec::new(),
            units: None,
        }
    }

    /// What the parser built, and what it found wrong.
    pub fn finish(self) -> (Piece<'s>, Vec<Diagnostic>, Lines<'s>) {
        (Piece { items: self.items, exprs: self.exprs, tables: self.t }, self.diags, self.lines)
    }

    /// Starts reading the tokens of `line`.
    pub fn begin_line(&mut self, line: &Line<'s>) {
        self.lexer.load(line.body, line.end);
    }

    // ─── Positions ──────────────────────────────────────────────────────────

    pub fn text(&self, loc: Loc) -> &'s str {
        &self.src[loc.range()]
    }

    /// Where a slice of the source was written.
    pub fn loc_of(&self, text: &str) -> Loc {
        locate(self.id, self.src, text)
    }

    /// From `start` to the end of the last token consumed.
    pub fn loc_from(&self, start: usize) -> Loc {
        Loc::new(self.id, start as u32, self.lexer.prev_end())
    }

    /// The whole line, trailing comment included.
    pub fn line_loc(&self, line: &Line<'s>) -> Loc {
        Loc::new(self.id, line.body as u32, line.end as u32)
    }

    /// The empty place `at` bytes into the file.
    pub fn point(&self, at: u32) -> Loc {
        Loc::new(self.id, at, at)
    }

    // ─── Nodes ──────────────────────────────────────────────────────────────

    /// Adds a node to its table. Every write to the tables goes through here.
    pub fn push<T: Stored<'s>>(&mut self, node: T) -> Ref<T> {
        let table = T::table_mut(&mut self.t);
        table.push(node);
        Ref::new(self.piece, table.len() - 1)
    }

    /// Makes room for `more` nodes of type `T`.
    pub fn reserve<T: Stored<'s>>(&mut self, more: usize) {
        T::table_mut(&mut self.t).reserve(more);
    }

    /// Where the next node of type `T` will go: the start of a run to close
    /// with [`Parser::since`].
    pub fn mark<T: Stored<'s>>(&self) -> usize {
        T::table(&self.t).len()
    }

    /// The nodes of type `T` added since `mark`.
    pub fn since<T: Stored<'s>>(&self, mark: usize) -> Many<T> {
        Many::new(Ref::new(self.piece, mark), T::table(&self.t).len() - mark)
    }

    /// A node this parser has added.
    pub fn get<T: Stored<'s>>(&self, id: Ref<T>) -> &T {
        &T::table(&self.t)[id.local()]
    }

    /// A run of nodes this parser has added.
    pub fn slice<T: Stored<'s>>(&self, many: Many<T>) -> &[T] {
        &T::table(&self.t)[many.range()]
    }

    /// Adds `node` to its table, and the item that is it.
    pub fn emit<T: Stored<'s>>(&mut self, header: &Header<'s>, node: T, kind: fn(Ref<T>) -> ItemKind<'s>) {
        let kind = kind(self.push(node));
        self.items.push(Item { doc: header.doc, loc: header.loc, kind });
    }

    /// Adds an expression node whose children were all added since `first`.
    pub fn node(&mut self, kind: ExprKind<'s>, loc: Loc, first: ExprId) -> ExprId {
        let id = self.next_expr();
        debug_assert!(first <= id, "a subtree starts at or before its root");
        self.exprs.push(Expr { kind, loc, first });
        id
    }

    /// The id the next expression node will get: where a new subtree starts.
    pub fn next_expr(&self) -> ExprId {
        ExprId::new(self.piece, self.exprs.len())
    }

    /// An expression node this parser has added.
    pub fn expr(&self, id: ExprId) -> &Expr<'s> {
        &self.exprs[id.local()]
    }

    // ─── Tokens ─────────────────────────────────────────────────────────────

    pub fn peek(&self) -> Token<'s> {
        self.lexer.peek()
    }

    pub fn tok(&self) -> Tok<'s> {
        self.lexer.peek().tok
    }

    pub fn bump(&mut self) -> Token<'s> {
        self.lexer.bump()
    }

    /// Consumes the token, which meant `value`.
    pub fn bump_as<T>(&mut self, value: T) -> T {
        self.bump();
        value
    }

    /// Consumes the token, then reads what it introduces.
    pub fn then<T>(&mut self, read: impl FnOnce(&mut Self) -> Parse<T>) -> Parse<T> {
        self.bump();
        read(self)
    }

    pub fn at_eol(&self) -> bool {
        matches!(self.tok(), Tok::Eol)
    }

    /// Whether the next token is the punctuation `punct`.
    pub fn at(&self, punct: Punct) -> bool {
        matches!(self.tok(), Tok::Punct(next) if next == punct)
    }

    pub fn eat(&mut self, punct: Punct) -> Option<Loc> {
        self.at(punct).then(|| self.bump().loc)
    }

    /// Whether the next token is the name `word`. Keywords are names, and count
    /// only where the grammar looks for them.
    pub fn at_word(&self, word: &str) -> bool {
        matches!(self.tok(), Tok::Name(next) if next == word)
    }

    pub fn eat_word(&mut self, word: &str) -> Option<Loc> {
        self.at_word(word).then(|| self.bump().loc)
    }

    /// Consumes the next token if `pick` makes something of it; otherwise
    /// reports that `what` was expected.
    pub fn take<T>(&mut self, pick: impl FnOnce(Tok<'s>) -> Option<T>, code: &'static str, what: &str) -> Parse<T> {
        let Some(value) = pick(self.tok()) else { return Err(self.expected(code, what)) };
        self.bump();
        Ok(value)
    }

    pub fn code(&mut self, code: &'static str, what: &str) -> Parse<Code<'s>> {
        self.take(|tok| if let Tok::Code(text) = tok { Some(text) } else { None }, code, what)
    }

    pub fn name(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        self.take(|tok| if let Tok::Name(text) = tok { Some(Name(text)) } else { None }, code, what)
    }

    pub fn unit(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        self.take(|tok| if let Tok::Unit(text) = tok { Some(Name(text)) } else { None }, code, what)
    }

    /// The word `word`, which a line's grammar puts between its parts.
    pub fn keyword(&mut self, word: &str) -> Parse<()> {
        self.expect_word(word, "expected-keyword", &format!("`{word}`")).map(drop)
    }

    /// A name, where a plain integer also counts: `529` is a number by shape,
    /// yet it is also what the 529 plan's kind is called.
    pub fn name_like(&mut self, code: &'static str, what: &str) -> Parse<Name<'s>> {
        match self.integer_name(self.peek()) {
            Some(name) => {
                self.bump();
                Ok(name)
            }
            None => self.name(code, what),
        }
    }

    /// The token as a name, if it is an integer written without separators.
    pub fn integer_name(&self, token: Token<'s>) -> Option<Name<'s>> {
        let text = self.text(token.loc);
        (matches!(token.tok, Tok::Number(_)) && text.bytes().all(|b| b.is_ascii_digit())).then_some(Name(text))
    }

    /// The token as a year: exactly four digits.
    pub fn year(&self, token: Token<'s>) -> Option<i32> {
        self.integer_name(token).filter(|integer| integer.len() == 4)?.parse().ok()
    }

    /// One word of a small fixed vocabulary, with a spelling suggestion when
    /// the word is close to one of them.
    pub fn choose<T: Copy>(&mut self, table: &[(&str, T)], code: &'static str, what: &str) -> Parse<(T, Loc)> {
        let token = self.peek();
        let Tok::Name(word) = token.tok else { return Err(self.expected(code, &format!("a {what}"))) };
        if let Some(&(_, value)) = table.iter().find(|(known, _)| *known == word) {
            self.bump();
            return Ok((value, token.loc));
        }
        let diag =
            Diagnostic::error(code, format!("unknown {what} `{word}`")).label(token.loc, format!("not a known {what}"));
        self.fail(match closest(word, table.iter().map(|(known, _)| *known)) {
            Some(near) => diag.fix(format!("did you mean `{near}`?"), token.loc, near),
            None => diag.note(format!("the {what}s are {}", list_words(table))),
        })
    }

    // ─── What was found instead ─────────────────────────────────────────────

    pub fn report(&mut self, diag: Diagnostic) -> Reported {
        self.diags.push(diag);
        Reported
    }

    pub fn fail<T>(&mut self, diag: Diagnostic) -> Parse<T> {
        Err(self.report(diag))
    }

    /// Reports that the next token is not `what`. A token that is not a token
    /// at all (a bad date, an open string) is reported as what it is instead.
    pub fn expected(&mut self, code: &'static str, what: &str) -> Reported {
        let diag = self.unexpected(self.peek(), code, what);
        self.report(diag)
    }

    /// The diagnostic for finding `token` where `what` was needed, for callers
    /// that want to add help before reporting it.
    pub fn unexpected(&self, token: Token<'s>, code: &'static str, what: &str) -> Diagnostic {
        if let Tok::Invalid(kind) = token.tok {
            return diagnose(kind, token.loc, self.text(token.loc));
        }
        let found = match token.tok {
            Tok::Eol => "the end of the line".to_string(),
            _ => format!("`{}`", clip(self.text(token.loc), 24)),
        };
        let diag = Diagnostic::error(code, format!("expected {what}, found {found}"))
            .label(token.loc, format!("expected {what}"));
        // A `/` touching another is `//` written where it is no comment.
        let touching = |at: Option<usize>| at.is_some_and(|at| self.src.as_bytes().get(at) == Some(&b'/'));
        let doubled = matches!(token.tok, Tok::Punct(Punct::Slash))
            && (touching(Some(token.loc.end as usize)) || touching((token.loc.start as usize).checked_sub(1)));
        match doubled {
            true => diag.note("`//` starts a comment only after whitespace, and a path separator is a single `/`"),
            false => diag,
        }
    }

    /// Consumes `punct`, or reports that `what` was expected.
    pub fn expect(&mut self, punct: Punct, code: &'static str, what: &str) -> Parse<Loc> {
        self.eat(punct).ok_or_else(|| self.expected(code, what))
    }

    pub fn expect_word(&mut self, word: &str, code: &'static str, what: &str) -> Parse<Loc> {
        self.eat_word(word).ok_or_else(|| self.expected(code, what))
    }

    /// A line must end where its grammar does. A closing bracket found there
    /// closes nothing: that is the mistake, not the end of the line.
    pub fn expect_eol(&mut self) -> Parse<()> {
        let token = self.peek();
        match token.tok {
            Tok::Eol => Ok(()),
            Tok::Punct(closer @ (Punct::RParen | Punct::RBracket)) => {
                let diag =
                    Diagnostic::error("unbalanced-delimiter", format!("this `{}` closes nothing", closer.spelling()))
                        .label(token.loc, "nothing is open here")
                        .fix("remove it", token.loc, "");
                Err(self.report(diag))
            }
            _ => Err(self.expected("expected-end-of-line", "the end of the line")),
        }
    }

    /// A clause that may appear once, written again at `second`.
    pub fn duplicate(&mut self, what: &str, second: Loc, first: Loc) -> Reported {
        let diag = Diagnostic::error("duplicate-clause", format!("only one {what} is allowed"))
            .label(second, format!("second {what}"))
            .context(first, format!("first {what}"));
        self.report(diag)
    }

    /// Consumes the `closer` of the bracket opened at `open`.
    pub fn close(&mut self, open: Loc, closer: Punct) -> Parse<Loc> {
        if let Some(loc) = self.eat(closer) {
            return Ok(loc);
        }
        let opener = self.text(open);
        let diag = self
            .unexpected(self.peek(), "unclosed-delimiter", &format!("`{}`", closer.spelling()))
            .context(open, format!("this `{opener}` is never closed"));
        self.fail(diag)
    }

    // ─── Where a header ends ────────────────────────────────────────────────

    /// Ends a header line: nothing may follow what was parsed. The header's
    /// location stops at its last token, so it excludes a trailing comment and
    /// the block below.
    pub fn end_header(&mut self, line: &mut Line<'s>) -> Parse<Header<'s>> {
        self.expect_eol()?;
        Ok(self.header(line))
    }

    /// Ends the header of an item worth keeping even when the line has junk at
    /// its end: the junk is reported, and the item stays, so the names it
    /// declares do not turn every later use of them into an error.
    pub fn keep_header(&mut self, line: &mut Line<'s>) -> Header<'s> {
        let _ = self.expect_eol();
        self.header(line)
    }

    fn header(&mut self, line: &mut Line<'s>) -> Header<'s> {
        Header { loc: self.loc_from(line.body), doc: line.take_doc() }
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
