//! Declarations and the other block items: `account`, `entity`, `asset`,
//! `purpose`, `commodity`, `kind`, `budget`, `code` and `param`. (The sources
//! that feed a book, `sync`, `format` and `pattern`, are in `source`.)
//!
//! A declaration keeps its good lines when one line is bad: dropping it would
//! turn every later use of what it declares into an error of its own.

use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Scope};

/// Where v3's chart put the accounts that v4 has no accounts for.
const CHART_ROOTS: [&str; 3] = ["income/", "expenses/", "equity/"];

/// What a declaration's lines have said so far.
#[derive(Default)]
struct Found<'s> {
    budget: Option<(Ref<Allowance<'s>>, Loc)>,
}

impl<'s> Parser<'s> {
    /// `account NAME [: KIND [at NAME]]`, `entity NAME[, NAME…] [: KIND] [#PURPOSE]`, `asset`,
    /// `purpose`, `commodity SYMBOL [: KIND]` or `kind NAME [: PARENT]`, with its
    /// indented properties, `also` lines, `known-as` lines and nested laws.
    /// Several entities on a line are one declaration each, all sharing what is
    /// written under them.
    pub fn decl(&mut self, line: &mut Line<'s>, what: DeclKind) -> Parse<()> {
        let mut names = vec![match what {
            DeclKind::Commodity => self.unit("expected-commodity", "a commodity symbol such as `USD`")?,
            _ => self.name_like("expected-name", "a name")?,
        }];
        while what == DeclKind::Entity && self.eat(Punct::Comma).is_some() {
            names.push(self.name_like("expected-name", "another entity name")?);
        }
        if what == DeclKind::Account && CHART_ROOTS.iter().any(|root| names[0].starts_with(root)) {
            return self.fail(chart_account(self.loc_of(&names[0]), names[0].0));
        }
        let kind = self.eat(Punct::Colon).and_then(|_| self.name_like("expected-kind", "a kind after `:`").ok());
        let at = if what == DeclKind::Account { self.eat_word("at") } else { None };
        let at = at.and_then(|_| self.name("expected-name", "the institution it is with, such as `chase`").ok());
        let purpose = match (what, self.tok()) {
            (DeclKind::Entity, Tok::Purpose(purpose)) => Some(self.bump_as(purpose)),
            _ => None,
        };
        let header = self.keep_header(line);
        let (props, laws, alsos, patterns) =
            (self.mark::<Prop>(), self.mark::<Law>(), self.mark::<Also>(), self.mark::<Pattern>());
        let mut found = Found::default();
        let _ = self.children(line, |parser, child| parser.decl_line(child, what, &mut found));
        let (props, laws, alsos, known_as) =
            (self.since(props), self.since(laws), self.since(alsos), self.since(patterns));
        let budget = found.budget.map(|(allowance, _)| allowance);
        for name in names {
            let decl = Decl { what, name, kind, at, purpose, budget, known_as, alsos, props, laws };
            self.emit(&header, decl, ItemKind::Decl);
        }
        Ok(())
    }

    /// One line of a declaration's body.
    fn decl_line(&mut self, line: &mut Line<'s>, what: DeclKind, found: &mut Found<'s>) -> Parse<()> {
        match self.tok() {
            Tok::Name("law") => {
                let implied = (what == DeclKind::Purpose).then_some(Trigger::Flow);
                self.then(|parser| parser.law(line, implied)).map(drop)
            }
            Tok::Name("also") => self.also(line).map(drop),
            Tok::Name("known-as") => self.known_as(),
            Tok::Name("budget") if what == DeclKind::Purpose => {
                self.bump();
                let allowance = self.allowance(Scope::Undated)?;
                self.expect_eol()?;
                let at = self.loc_from(line.body);
                match found.budget.replace((self.push(allowance), at)) {
                    Some((_, first)) => Err(self.duplicate("`budget` line", at, first)),
                    None => Ok(()),
                }
            }
            _ => self.property(line, Scope::Undated),
        }
    }

    /// `budget PURPOSE LIMIT monthly|yearly [carries] [funded from H into H]`
    pub fn budget(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let purpose = self.name("expected-name", "the purpose it is for, such as `food`")?;
        let allowance = self.allowance(Scope::Undated)?;
        let header = self.end_header(line)?;
        self.emit(&header, Budget { purpose, allowance }, ItemKind::Budget);
        Ok(())
    }

    /// A property line of a declaration, and the lines under it: `NAME ARG*`.
    /// The property is kept when a line under it is bad.
    pub fn property(&mut self, line: &Line<'s>, scope: Scope) -> Parse<()> {
        let mut prop = self.prop(scope)?;
        self.expect_eol()?;
        debug_assert_eq!(prop.loc.start as usize, line.body);
        let nested = self.mark::<Nested>();
        let lines = self.children(line, |parser, _| {
            let inner = parser.prop(scope)?;
            parser.expect_eol()?;
            parser.push(Nested(inner));
            Ok(())
        });
        prop.lines = self.since(nested);
        self.push(prop);
        lines
    }

    /// `NAME ARG*`: arguments are expressions, commas optional. In a statement
    /// they stop at what ends one: a string, a code or `until`, and the short
    /// dates of `due` and `until` count forward from the statement's day.
    pub fn prop(&mut self, scope: Scope) -> Parse<Prop<'s>> {
        let start = self.peek().loc.start as usize;
        let name = self.name("expected-property", "a property name")?;
        let in_statement = matches!(scope, Scope::Statement(_));
        let forward = in_statement && matches!(name.0, "due" | "until");
        let roots = self.roots.len();
        while !self.at_eol() {
            if in_statement && matches!(self.tok(), Tok::Str(_) | Tok::Code(_) | Tok::Name("until")) {
                break;
            }
            if self.eat(Punct::Comma).is_some() {
                continue;
            }
            let token = self.peek();
            let arg = match (self.tok(), forward) {
                (Tok::MonthDay(..), true) => {
                    let first = self.next_expr();
                    let day = self.date_from(scope.day(), "a date")?;
                    self.node(ExprKind::Date(day), token.loc, first)
                }
                _ => self.expression()?,
            };
            self.roots.push(arg);
        }
        let args = self.roots_since(roots);
        Ok(Prop { name, args, lines: Many::EMPTY, loc: self.loc_from(start) })
    }

    /// `code GLOB [GLOB…]` with `on PLACE-GLOB | KIND …` and `known-as PATTERN` lines.
    pub fn code_rule(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let mut globs = vec![self.glob()?];
        while !self.at_eol() {
            globs.push(self.glob()?);
        }
        let header = self.end_header(line)?;
        let (on, patterns) = (self.mark::<Name>(), self.mark::<Pattern>());
        let _ = self.children(line, |parser, _| match parser.tok() {
            Tok::Name("known-as") => parser.known_as(),
            _ => {
                parser.expect_word("on", "expected-on", "`on` and where the code may be used, or `known-as`")?;
                loop {
                    let place = parser.glob()?;
                    parser.push(place);
                    parser.eat(Punct::Pipe);
                    if parser.at_eol() {
                        return Ok(());
                    }
                }
            }
        });
        let (on, known_as) = (self.since(on), self.since(patterns));
        for pattern in globs {
            self.emit(&header, CodeRule { pattern, on, known_as }, ItemKind::Code);
        }
        Ok(())
    }

    /// A glob over places, kinds or codes; a lone `*` is the glob for all.
    fn glob(&mut self) -> Parse<Name<'s>> {
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

    /// `param NAME [UNIT]` with `KEY+ VALUE` rows.
    pub fn param(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let name = self.name("expected-name", "a parameter name")?;
        let unit = match self.tok() {
            Tok::Unit(unit) => Some(self.bump_as(Name(unit))),
            _ => None,
        };
        let header = self.keep_header(line);
        let mark = self.mark::<ParamRow>();
        let _ = self.children(line, |parser, row| parser.param_row(row));
        let rows = self.since(mark);
        self.emit(&header, Param { name, unit, rows }, ItemKind::Param);
        Ok(())
    }

    fn param_row(&mut self, line: &Line<'s>) -> Parse<()> {
        let mark = self.mark::<Key>();
        let mut first = true;
        while let Some(key) = self.key(first) {
            first = false;
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
    /// value except when it is the row's first (year) key.
    fn key(&mut self, first: bool) -> Option<Key<'s>> {
        let (token, second) = (self.peek(), self.lexer.peek_second());
        let key = match token.tok {
            _ if matches!(second.tok, Tok::Eol) => return None,
            Tok::Date(day) => Key::Date(day, token.loc),
            Tok::Name(text) => Key::Name(Name(text)),
            Tok::Unit(text) => Key::Name(Name(text)),
            Tok::Number(_) if first || !matches!(second.tok, Tok::Unit(_)) => {
                Key::Year(self.year(token)?, token.loc)
            }
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
}

/// An account under one of [`CHART_ROOTS`], which v4 does not keep.
fn chart_account(loc: Loc, name: &str) -> Diagnostic {
    let message = format!("`{name}` is a v3 chart account: v4 has no income, expense or equity accounts");
    Diagnostic::error("chart-account", message)
        .label(loc, "what a flow is for is its purpose, and whom it is with is its party")
        .help("declare the party (`entity lumen : employer`) and use `#purpose` for what each flow is for; declare institution positions as `account checking : bank at chase`")
}

fn abbreviated_schedule(loc: Loc) -> Diagnostic {
    Diagnostic::error("abbreviated-schedule", "a schedule cannot be abbreviated with `...`")
        .label(loc, "write out the remaining brackets")
        .help("every bracket is a threshold and a rate: `105_700 USD 24% | 201_775 USD 32%`")
}
