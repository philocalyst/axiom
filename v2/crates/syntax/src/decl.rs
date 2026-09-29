//! Declarations and the other block items: `account`, `entity`, `commodity`,
//! `kind`, `code`, `param`, and `sync`.

use axiom_core::{Diagnostic, Loc};

use crate::ast::{CodeRule, Decl, DeclKind, ExprId, ExprKind, Item, ItemKind, Key, Name, Param, ParamRow, Prop, Sync};
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser};

impl<'s> Parser<'s> {
    /// `account|entity|commodity|kind NAME [: KIND]`, with its indented
    /// properties and nested laws.
    pub fn decl(&mut self, line: &mut Line<'s>, what: DeclKind) -> Parse<Item<'s>> {
        let name = match what {
            DeclKind::Commodity => self.unit("expected-commodity", "a commodity symbol such as `USD`")?,
            _ => self.name_like("expected-name", "a name")?,
        };
        let kind = match self.cursor.eat(Tok::Colon) {
            Some(_) => Some(self.name_like("expected-kind", "a kind after `:`")?),
            None => None,
        };
        let header = self.end_header(line)?;
        let (mut props, mut laws) = (Vec::new(), Vec::new());
        self.children(line, |parser, child| {
            if parser.eat_word("law").is_some() {
                laws.push(parser.law(child)?);
            } else {
                props.push(parser.property(child)?);
            }
            Ok(())
        })?;
        Ok(header.item(ItemKind::Decl(Decl { what, name, kind, props, laws })))
    }

    /// `NAME ARG*`: arguments are primary expressions, commas optional.
    fn property(&mut self, line: &Line<'s>) -> Parse<Prop<'s>> {
        let name = self.name("expected-property", "a property name")?;
        let mut args = Vec::new();
        while !matches!(self.cursor.peek().tok, Tok::Eol) {
            if self.cursor.eat(Tok::Comma).is_none() {
                args.push(self.primary()?);
            }
        }
        Ok(Prop { name, args, loc: self.loc_from(line.body) })
    }

    /// `code GLOB` with `on PLACE-GLOB | KIND` lines.
    pub fn code_rule(&mut self, line: &mut Line<'s>) -> Parse<Item<'s>> {
        let pattern = self.pattern()?;
        let header = self.end_header(line)?;
        let mut on = Vec::new();
        self.children(line, |parser, _| {
            parser.expect_word("on", "expected-on", "`on` and where the code may be used")?;
            loop {
                on.push(parser.pattern()?);
                if parser.cursor.eat(Tok::Bar).is_none() {
                    return parser.expect_eol();
                }
            }
        })?;
        Ok(header.item(ItemKind::Code(CodeRule { pattern, on })))
    }

    /// A glob over places, kinds or codes; a lone `*` is the pattern for all.
    fn pattern(&mut self) -> Parse<Name<'s>> {
        let token = self.cursor.peek();
        if matches!(token.tok, Tok::Star) {
            self.cursor.bump();
            return Ok(Name { text: self.text(token.loc), loc: token.loc });
        }
        if let Tok::Code(text) = token.tok {
            return self.fail(hash_in_pattern(token.loc, text));
        }
        self.name("expected-pattern", "a name or glob such as `trip-*`")
    }

    /// `param NAME` with `KEY+ VALUE` rows.
    pub fn param(&mut self, line: &mut Line<'s>) -> Parse<Item<'s>> {
        let name = self.name("expected-name", "a parameter name")?;
        let header = self.end_header(line)?;
        let mut rows = Vec::new();
        self.children(line, |parser, row| {
            rows.push(parser.param_row(row)?);
            Ok(())
        })?;
        Ok(header.item(ItemKind::Param(Param { name, rows })))
    }

    fn param_row(&mut self, line: &Line<'s>) -> Parse<ParamRow<'s>> {
        let mut keys = Vec::new();
        while let Some(key) = self.key()? {
            keys.push(key);
        }
        if keys.is_empty() {
            let diag = Diagnostic::error("missing-key", "this row has no key")
                .label(self.line_loc(line), "a row is `KEY VALUE`, and this has only a value")
                .help("start the row with a year, date or name to look it up by: `2026 single 24_500 USD`");
            return self.fail(diag);
        }
        let value = self.param_value()?;
        self.expect_eol()?;
        Ok(ParamRow { keys, value, loc: self.loc_from(line.body) })
    }

    /// The next key of a row, if the next token is one. The last token of a
    /// line is always the value, and a number followed by a commodity is a
    /// value too, not a year.
    fn key(&mut self) -> Parse<Option<Key<'s>>> {
        let (token, second) = (self.cursor.peek(), self.cursor.peek_second());
        if matches!(second.tok, Tok::Eol) {
            return Ok(None);
        }
        let key = match token.tok {
            Tok::Date(day) => Key::Date(day, token.loc),
            Tok::Name(text) => Key::Name(Name { text, loc: token.loc }),
            Tok::Number(_) if !matches!(second.tok, Tok::Unit(_)) => match self.year(token) {
                Some(year) => Key::Year(year, token.loc),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        self.cursor.bump();
        Ok(Some(key))
    }

    /// An expression, or a schedule of brackets: `0 USD 10% | 12_400 USD 12%`.
    fn param_value(&mut self) -> Parse<ExprId> {
        let first_threshold = self.expression()?;
        if !matches!(self.cursor.peek().tok, Tok::Percent(_)) {
            return Ok(first_threshold);
        }
        let first = self.exprs[first_threshold].first;
        let start = self.exprs[first_threshold].loc.start as usize;
        let mut brackets = vec![(first_threshold, self.rate()?)];
        while self.cursor.eat(Tok::Bar).is_some() {
            if let Some(dots) = self.cursor.eat(Tok::Ellipsis) {
                return self.fail(abbreviated_schedule(dots.loc));
            }
            let threshold = self.expression()?;
            brackets.push((threshold, self.rate()?));
        }
        let kind = ExprKind::Schedule(brackets.into_boxed_slice());
        Ok(self.exprs.push(kind, self.loc_from(start), first))
    }

    fn rate(&mut self) -> Parse<ExprId> {
        let token = self.cursor.peek();
        let Tok::Percent(num) = token.tok else {
            return Err(self.expected("expected-rate", "a rate such as `12%` after the threshold"));
        };
        let first = self.exprs.next();
        self.cursor.bump();
        Ok(self.exprs.push(ExprKind::Pct(num), token.loc, first))
    }

    /// `sync FILE` with a `run COMMAND…` line. Both are raw text, not tokens:
    /// a file name has dots and a command has anything.
    pub fn sync(&mut self, line: &mut Line<'s>) -> Parse<Item<'s>> {
        let Some(file) = self.cursor.raw_word() else {
            return Err(self.expected("expected-file", "the file to write, like `prices/2026.ax`"));
        };
        let header = self.end_header(line)?;
        let mut run = None;
        self.children(line, |parser, _| {
            parser.expect_word("run", "expected-run", "`run` and a command")?;
            let Some(command) = parser.cursor.raw_rest() else {
                return Err(parser.expected("expected-command", "the command to run"));
            };
            match run.replace(command) {
                Some(_) => parser.fail(only_one_run(command.loc)),
                None => Ok(()),
            }
        })?;
        match run {
            Some(run) => Ok(header.item(ItemKind::Sync(Sync { file, run }))),
            None => self.fail(missing_run(header.loc)),
        }
    }
}

fn abbreviated_schedule(loc: Loc) -> Diagnostic {
    Diagnostic::error("abbreviated-schedule", "a schedule cannot be abbreviated with `...`")
        .label(loc, "write out the remaining brackets")
        .help("every bracket is a threshold and a rate: `105_700 USD 24% | 201_775 USD 32%`")
}

fn hash_in_pattern(loc: Loc, text: &str) -> Diagnostic {
    Diagnostic::error("hash-in-pattern", "code patterns are written without `#`").label(loc, "remove the `#`").fix(
        format!("write `{text}`"),
        loc,
        text,
    )
}

fn only_one_run(loc: Loc) -> Diagnostic {
    Diagnostic::error("duplicate-run", "a sync has one `run` command").label(loc, "second command")
}

fn missing_run(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-run", "this sync has no command to run")
        .label(header, "nothing below says how to produce this file")
        .help("add an indented line: `run python3 fetch_prices.py`")
}
