//! Declarations and the other block items: `account`, `entity`, `asset`,
//! `purpose`, `commodity`, `kind`, `budget`, `code`, `param`, and `sync`.
//!
//! A declaration keeps its good lines when one line is bad: dropping it would
//! turn every later use of what it declares into an error of its own.

use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser};

/// Where v3's chart put the accounts that v4 has no accounts for.
const CHART_ROOTS: [&str; 3] = ["income/", "expenses/", "equity/"];

impl<'s> Parser<'s> {
    /// `account NAME [: KIND [at NAME]]`, `entity NAME[, NAME…] [: KIND] [#PURPOSE]`, `asset`,
    /// `purpose`, `commodity SYMBOL [: KIND]` or `kind NAME [: PARENT]`, with its
    /// indented properties and nested laws. Several entities on a line are one
    /// declaration each, all sharing what is written under them.
    pub fn decl(&mut self, line: &mut Line<'s>, what: DeclKind) -> Parse<()> {
        let mut names = vec![match what {
            DeclKind::Commodity => self.unit("expected-commodity", "a commodity symbol such as `USD`")?,
            _ => self.name_like("expected-name", "a name")?,
        }];
        while what == DeclKind::Entity && self.eat(Punct::Comma).is_some() {
            names.push(self.name_like("expected-name", "another entity name")?);
        }
        if what == DeclKind::Account && CHART_ROOTS.iter().any(|root| names[0].starts_with(root)) {
            let note = chart_account(self.loc_of(&names[0]), names[0].0);
            self.diags.push(note);
        }
        let kind = self.eat(Punct::Colon).and_then(|_| self.name_like("expected-kind", "a kind after `:`").ok());
        let at = if what == DeclKind::Account { self.eat_word("at") } else { None };
        let at = at.and_then(|_| self.name("expected-name", "the institution it is with, such as `chase`").ok());
        let purpose = match (what, self.tok()) {
            (DeclKind::Entity, Tok::Purpose(purpose)) => Some(self.bump_as(purpose)),
            _ => None,
        };
        let header = self.keep_header(line);
        let (props, laws) = (self.mark::<Prop>(), self.mark::<Law>());
        let _ = self.children(line, |parser, child| match parser.eat_word("law") {
            Some(_) => parser.law(child).map(drop),
            None => parser.property(child),
        });
        let (props, laws) = (self.since(props), self.since(laws));
        for name in names {
            self.emit(&header, Decl { what, name, kind, at, purpose, props, laws }, ItemKind::Decl);
        }
        Ok(())
    }

    /// `budget PURPOSE LIMIT monthly|yearly [carries]`
    pub fn budget(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let purpose = self.name("expected-name", "the purpose it is for, such as `food`")?;
        let allowance = self.allowance()?;
        let header = self.end_header(line)?;
        self.emit(&header, Budget { purpose, allowance }, ItemKind::Budget);
        Ok(())
    }

    /// A property line of a declaration: `NAME ARG*`.
    pub fn property(&mut self, line: &Line<'s>) -> Parse<()> {
        let prop = self.prop(false)?;
        debug_assert_eq!(prop.loc.start as usize, line.body);
        self.push(prop);
        Ok(())
    }

    /// `NAME ARG*`: arguments are primary expressions, commas optional. In a
    /// statement they stop at what ends one: a string, a code or `until`.
    pub fn prop(&mut self, in_statement: bool) -> Parse<Prop<'s>> {
        let start = self.peek().loc.start as usize;
        let name = self.name("expected-property", "a property name")?;
        let roots = self.roots.len();
        while !self.at_eol() {
            if in_statement && matches!(self.tok(), Tok::Str(_) | Tok::Code(_) | Tok::Name("until")) {
                break;
            }
            if self.eat(Punct::Comma).is_none() {
                let arg = self.primary()?;
                self.roots.push(arg);
            }
        }
        let args = self.roots_since(roots);
        Ok(Prop { name, args, loc: self.loc_from(start) })
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
                parser.push(place);
                parser.eat(Punct::Pipe);
                if parser.at_eol() {
                    return Ok(());
                }
            }
        });
        let on = self.since(mark);
        for pattern in patterns {
            self.emit(&header, CodeRule { pattern, on }, ItemKind::Code);
        }
        Ok(())
    }

    /// A glob over places, kinds or codes; a lone `*` is the pattern for all.
    fn pattern(&mut self) -> Parse<Name<'s>> {
        let token = self.peek();
        match token.tok {
            Tok::Punct(Punct::Star) => Ok(self.bump_as(Name(self.text(token.loc)))),
            Tok::Code(_) | Tok::Purpose(_) => {
                let (mark, name) = self.text(token.loc).split_at(1);
                let diag = Diagnostic::error("mark-in-pattern", format!("code patterns are written without `{mark}`"))
                    .label(token.loc, format!("remove the `{mark}`"))
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
        self.emit(&header, Param { name, rows }, ItemKind::Param);
        Ok(())
    }

    fn param_row(&mut self, line: &Line<'s>) -> Parse<()> {
        let mark = self.mark::<Key>();
        while let Some(key) = self.key() {
            self.push(key);
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
            self.push(Bracket { threshold, rate });
            if self.eat(Punct::Pipe).is_none() {
                break;
            }
            if let Some(dots) = self.eat(Punct::Ellipsis) {
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

    /// `sync NAME` with a `run COMMAND…` line, and perhaps `into PATH`, which
    /// are raw text (a command has anything in it, and a path has dots and
    /// braces), and any other lines, which are properties.
    pub fn sync(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "what it feeds, such as `checking`")?;
        if self.at(Punct::Dot) && self.peek().loc.start == self.lexer.prev_end() {
            return self.fail(self.sync_file(name));
        }
        let header = self.end_header(line)?;
        let (mut run, mut into) = (None, None);
        let props = self.mark::<Prop>();
        self.children(line, |parser, child| match parser.tok() {
            Tok::Name("run") => parser.raw_line("run", "the command to run", &mut run),
            Tok::Name("into") => parser.raw_line("into", "where to write, like `prices/{year}.ax`", &mut into),
            _ => parser.property(child),
        })?;
        let Some(run) = run else { return self.fail(missing_run(header.loc)) };
        let props = self.since(props);
        self.emit(&header, Sync { name, run: run.0, into: into.map(|(text, _)| text), props }, ItemKind::Sync);
        Ok(())
    }

    /// A `sync` line that is a word and the raw text after it, once.
    fn raw_line(&mut self, word: &str, what: &str, slot: &mut Option<(Text<'s>, Loc)>) -> Parse<()> {
        let keyword = self.bump().loc;
        let Some(text) = self.lexer.raw_rest() else {
            return Err(self.expected(if word == "run" { "expected-command" } else { "expected-path" }, what));
        };
        let at = keyword.to(self.loc_of(&text));
        match slot.replace((Text(text.0), at)) {
            Some((_, first)) => Err(self.duplicate(&format!("`{word}` line"), at, first)),
            None => Ok(()),
        }
    }

    /// v3's `sync prices/2026.ax`, which named the file it wrote. A sync is
    /// named for what it feeds now, and says where it writes with `into`.
    fn sync_file(&self, name: Name<'s>) -> Diagnostic {
        let start = self.loc_of(&name);
        let len = self.src[start.start as usize..].find([' ', '\t', '\r', '\n']).unwrap_or(self.src.len() - start.start as usize);
        let file = Loc::new(self.id, start.start, start.start + len as u32);
        let written = self.text(file);
        let feeds = name.split('/').next().unwrap_or(&name);
        Diagnostic::error("sync-file", format!("a sync is named for what it feeds, not for the file `{written}` it writes"))
            .label(file, "a file is `into`, on a line of its own")
            .note("`sync prices` names the source, and its lines say `run` a command and `into` a file")
            .fix(format!("name it `{feeds}` and say where it writes"), file, format!("{feeds}\n  into {written}"))
    }
}

/// An account under one of [`CHART_ROOTS`]. Whether it is a chart account is for the
/// model to judge, so this is only a note.
fn chart_account(loc: Loc, name: &str) -> Diagnostic {
    let message = format!("`{name}` is a v3 chart account: v4 has no income, expense or equity accounts");
    Diagnostic::info("chart-account", message)
        .label(loc, "what a flow is for is its purpose, and whom it is with is its party")
        .help("declare the party (`entity lumen : employer`), and write `#purpose` where its kind does not say")
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
