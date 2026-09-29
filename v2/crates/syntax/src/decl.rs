//! Declarations and the other block items: `account`, `entity`, `commodity`,
//! `kind`, `code`, `param`, and `sync`.
//!
//! A declaration keeps its good lines when one line is bad: dropping it would
//! turn every later use of what it declares into an error of its own.

use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::Tok;
use crate::lines::Line;
use crate::parser::{Parse, Parser};

impl<'s> Parser<'s> {
    /// `account PATH [as ALIAS] [: KIND]`, `entity NAME[, NAME…] [: KIND]`,
    /// `commodity SYMBOL [: KIND]` or `kind NAME [: PARENT]`, with its indented
    /// properties and nested laws. Several entities on a line are one
    /// declaration each, all sharing what is written under them.
    pub fn decl(&mut self, line: &mut Line<'s>, what: DeclKind) -> Parse<()> {
        let mut names = vec![match what {
            DeclKind::Commodity => self.unit("expected-commodity", "a commodity symbol such as `USD`")?,
            _ => self.name_like("expected-name", "a name")?,
        }];
        while what == DeclKind::Entity && self.eat(",").is_some() {
            names.push(self.name_like("expected-name", "another entity name")?);
        }
        let alias = match what == DeclKind::Account && self.eat_word("as").is_some() {
            true => Some(self.name("expected-name", "a short name for the account, like `biz`")?),
            false => None,
        };
        let kind = self.eat(":").and_then(|_| self.name_like("expected-kind", "a kind after `:`").ok());
        let header = self.keep_header(line);
        let (props, laws) = (self.mark::<Prop>(), self.mark::<Law>());
        let _ = self.children(line, |parser, child| match parser.eat_word("law") {
            Some(_) => parser.law(child).map(drop),
            None => parser.property(child),
        });
        let (props, laws) = (self.since(props), self.since(laws));
        for name in names {
            let id = self.push(Decl { what, name, alias, kind, props, laws });
            self.emit(&header, ItemKind::Decl(id));
        }
        Ok(())
    }

    /// `NAME ARG*`: arguments are primary expressions, commas optional.
    fn property(&mut self, line: &Line<'s>) -> Parse<()> {
        let name = self.name("expected-property", "a property name")?;
        let start = self.roots.len();
        while !self.at_eol() {
            if self.eat(",").is_none() {
                let arg = self.primary()?;
                self.roots.push(arg);
            }
        }
        let args = self.roots_since(start);
        let loc = self.loc_from(line.body);
        self.push(Prop { name, args, loc });
        Ok(())
    }

    /// `code GLOB [GLOB…]` with `on PLACE-GLOB | KIND …` lines.
    pub fn code_rule(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let mut patterns = vec![self.pattern()?];
        while !self.at_eol() {
            patterns.push(self.pattern()?);
        }
        let header = self.end_header(line)?;
        let mark = self.mark::<Name>();
        let _ = self.children(line, |parser, _| {
            parser.expect_word("on", "expected-on", "`on` and where the code may be used")?;
            loop {
                let place = parser.pattern()?;
                parser.t.names.push(place);
                parser.eat("|");
                if parser.at_eol() {
                    return Ok(());
                }
            }
        });
        let on = self.since(mark);
        for pattern in patterns {
            let id = self.push(CodeRule { pattern, on });
            self.emit(&header, ItemKind::Code(id));
        }
        Ok(())
    }

    /// A glob over places, kinds or codes; a lone `*` is the pattern for all.
    fn pattern(&mut self) -> Parse<Name<'s>> {
        let token = self.peek();
        match token.tok {
            Tok::Punct("*") => {
                self.bump();
                Ok(Name(self.text(token.loc)))
            }
            Tok::Code(code) => {
                let name = code.name();
                let diag = Diagnostic::error("hash-in-pattern", "code patterns are written without `#`")
                    .label(token.loc, "remove the `#`")
                    .fix(format!("write `{name}`"), token.loc, name);
                self.fail(diag)
            }
            _ => self.name("expected-pattern", "a name or glob such as `trip-*`"),
        }
    }

    /// `param NAME` with `KEY+ VALUE` rows.
    pub fn param(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "a parameter name")?;
        let header = self.keep_header(line);
        let mark = self.mark::<ParamRow>();
        let _ = self.children(line, |parser, row| parser.param_row(row));
        let rows = self.since(mark);
        let id = self.push(Param { name, rows });
        self.emit(&header, ItemKind::Param(id));
        Ok(())
    }

    fn param_row(&mut self, line: &Line<'s>) -> Parse<()> {
        let mark = self.mark::<Key>();
        while let Some(key) = self.key() {
            self.t.keys.push(key);
        }
        let keys = self.since(mark);
        if keys.is_empty() {
            let diag = Diagnostic::error("missing-key", "this row has no key")
                .label(self.line_loc(line), "a row is `KEY VALUE`, and this has only a value")
                .help("start the row with a year, date or name to look it up by: `2026 single 24_500 USD`");
            return self.fail(diag);
        }
        let value = self.param_value()?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        self.push(ParamRow { keys, value, loc });
        Ok(())
    }

    /// The next key of a row, if the next token is one. The last token of a
    /// line is always the value, and a number followed by a commodity is a
    /// value too, not a year.
    fn key(&mut self) -> Option<Key<'s>> {
        let (token, second) = (self.peek(), self.lexer.peek_second());
        let key = match token.tok {
            _ if matches!(second.tok, Tok::Eol) => return None,
            Tok::Date(day) => Key::Date(day, token.loc),
            Tok::Name(text) => Key::Name(Name(text)),
            Tok::Number(_) if !matches!(second.tok, Tok::Unit(_)) => Key::Year(self.year(token)?, token.loc),
            _ => return None,
        };
        self.bump();
        Some(key)
    }

    /// An expression, or a schedule of brackets: `0 USD 10% | 12_400 USD 12%`.
    fn param_value(&mut self) -> Parse<ExprId> {
        let first_threshold = self.expression()?;
        if !matches!(self.tok(), Tok::Percent(_)) {
            return Ok(first_threshold);
        }
        let (first, start) = (self.expr(first_threshold).first, self.expr(first_threshold).loc.start as usize);
        let mark = self.mark::<Bracket>();
        let mut threshold = first_threshold;
        loop {
            let rate = self.rate()?;
            self.t.brackets.push(Bracket { threshold, rate });
            if self.eat("|").is_none() {
                break;
            }
            if let Some(dots) = self.eat("...") {
                return self.fail(abbreviated_schedule(dots));
            }
            threshold = self.expression()?;
        }
        let schedule = ExprKind::Schedule(self.since(mark));
        Ok(self.node(schedule, self.loc_from(start), first))
    }

    fn rate(&mut self) -> Parse<ExprId> {
        let token = self.peek();
        let Tok::Percent(num) = token.tok else {
            return Err(self.expected("expected-rate", "a rate such as `12%` after the threshold"));
        };
        let first = self.next_expr();
        self.bump();
        Ok(self.node(ExprKind::Pct(num), token.loc, first))
    }

    /// `sync FILE` with a `run COMMAND…` line. Both are raw text, not tokens:
    /// a file name has dots and a command has anything.
    pub fn sync(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let Some(file) = self.lexer.raw_word() else {
            return Err(self.expected("expected-file", "the file to write, like `prices/2026.ax`"));
        };
        let header = self.end_header(line)?;
        let mut run = None;
        self.children(line, |parser, _| {
            parser.expect_word("run", "expected-run", "`run` and a command")?;
            let Some(command) = parser.lexer.raw_rest() else {
                return Err(parser.expected("expected-command", "the command to run"));
            };
            match run.replace(command) {
                Some(_) => {
                    let diag = Diagnostic::error("duplicate-run", "a sync has one `run` command")
                        .label(parser.loc_of(&command), "second command");
                    parser.fail(diag)
                }
                None => Ok(()),
            }
        })?;
        let Some(run) = run else { return self.fail(missing_run(header.loc)) };
        let id = self.push(Sync { file, run });
        self.emit(&header, ItemKind::Sync(id));
        Ok(())
    }
}

fn abbreviated_schedule(loc: Loc) -> Diagnostic {
    Diagnostic::error("abbreviated-schedule", "a schedule cannot be abbreviated with `...`")
        .label(loc, "write out the remaining brackets")
        .help("every bracket is a threshold and a rate: `105_700 USD 24% | 201_775 USD 32%`")
}

fn missing_run(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-run", "this sync has no command to run")
        .label(header, "nothing below says how to produce this file")
        .help("add an indented line: `run python3 fetch_prices.py`")
}
