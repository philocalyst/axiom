//! A file's structure: items in column 0, dispatched on their first word, and
//! the indented blocks under them.
//!
//! An item is a column-0 line plus every deeper line after it. A block's lines
//! must share one indentation, fixed by its first line; deeper lines belong to
//! the block's own sub-blocks. [`Parser::children`] walks a block, parses each
//! line on its own, and keeps going after a bad one so every error is reported.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok, Token};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported, list_words};

/// What a column-0 line that does not start with a date can be, by its first
/// word. The table below is the only list of them: the dispatch, the
/// suggestion for a misspelling and the list in a message all read it.
#[derive(Clone, Copy)]
enum Keyword {
    Account,
    Entity,
    Asset,
    Purpose,
    Commodity,
    Kind,
    Contract,
    Budget,
    Code,
    Param,
    Law,
    Sync,
    Opening,
    System,
    Use,
    Base,
    Relaxed,
    Currency,
    Rates,
    Pattern,
    Format,
}

const KEYWORDS: [(&str, Keyword); 21] = [
    ("account", Keyword::Account),
    ("entity", Keyword::Entity),
    ("asset", Keyword::Asset),
    ("purpose", Keyword::Purpose),
    ("commodity", Keyword::Commodity),
    ("kind", Keyword::Kind),
    ("contract", Keyword::Contract),
    ("budget", Keyword::Budget),
    ("code", Keyword::Code),
    ("param", Keyword::Param),
    ("law", Keyword::Law),
    ("sync", Keyword::Sync),
    ("opening", Keyword::Opening),
    ("system", Keyword::System),
    ("use", Keyword::Use),
    ("base", Keyword::Base),
    ("relaxed", Keyword::Relaxed),
    ("currency", Keyword::Currency),
    ("rates", Keyword::Rates),
    ("pattern", Keyword::Pattern),
    ("format", Keyword::Format),
];

/// Words that start a line inside a block, and what owns such a block. Written
/// at column 0 they are a block's line whose block was forgotten.
const BLOCK_WORDS: [(&str, &str); 16] = [
    ("when", "law"),
    ("unless", "law"),
    ("let", "law"),
    ("require", "law"),
    ("warn", "law"),
    ("owe", "law"),
    ("count", "law"),
    ("consume", "law"),
    ("carry", "law"),
    ("always", "law"),
    ("each", "law"),
    ("run", "sync"),
    ("read", "sync"),
    ("into", "sync"),
    ("known-as", "declaration"),
    ("also", "declaration"),
];

/// The ways a system converts that are one word; `param NAME` is the other.
const RATES: [(&str, Rates<'static>); 1] = [("spot", Rates::Spot)];

/// The indentation a block has settled on, and the line before, for context.
struct Block {
    indent: Option<usize>,
    previous: Loc,
    /// Whether a line that starts with an amount may be indented further than
    /// the rest, to line its digits up with those above it.
    ragged: bool,
}

impl<'s> Parser<'s> {
    /// Every item of the file, added as it parses. An item that fails is
    /// skipped along with its block, and parsing resumes at the next column-0
    /// line. `first` says whether the first line is the file's first.
    pub fn items(&mut self, mut first: bool) {
        while let Some(mut line) = self.lines.next() {
            if line.indent > 0 {
                self.report_orphan(&line);
                continue;
            }
            let parsed = self.item(&mut line, first);
            first = false;
            self.discard_block(&line, parsed.is_ok());
        }
    }

    fn item(&mut self, line: &mut Line<'s>, first: bool) -> Parse<()> {
        self.begin_line(line);
        let token = self.peek();
        match token.tok {
            Tok::Date(date) => {
                self.bump();
                self.journal_entry(line, date)
            }
            Tok::Month(_) | Tok::Number(_) | Tok::MonthDay(..) => {
                // A line of only a year or a month says what the lines below it are in.
                if let Some(folder) = Folder::heading(&self.src.as_bytes()[line.body..line.end]) {
                    self.warn_ignored_doc(line);
                    self.folder = folder;
                    return Ok(());
                }
                if let Tok::Month(_) = token.tok {
                    return Err(self.expected("expected-item", "a date, a keyword, or a heading on a line of its own"));
                }
                let date = self.item_date("a date or a keyword")?;
                self.journal_entry(line, date)
            }
            Tok::Name(word) => {
                self.bump();
                self.keyword_item(line, token, word, first)
            }
            _ => Err(self.expected("expected-item", "a date or a keyword")),
        }
    }

    fn keyword_item(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<()> {
        let Some(&(_, kind)) = KEYWORDS.iter().find(|(known, _)| *known == word) else {
            return self.unknown_keyword(line, keyword, word, first);
        };
        match kind {
            Keyword::Account => self.decl(line, DeclKind::Account),
            Keyword::Entity => self.decl(line, DeclKind::Entity),
            Keyword::Asset => self.decl(line, DeclKind::Asset),
            Keyword::Purpose => self.decl(line, DeclKind::Purpose),
            Keyword::Commodity => self.decl(line, DeclKind::Commodity),
            Keyword::Kind => self.decl(line, DeclKind::Kind),
            Keyword::Budget => self.budget(line),
            Keyword::Code => self.code_rule(line),
            Keyword::Param => self.param(line),
            Keyword::Law => self.law_item(line),
            Keyword::Contract => self.contract(line),
            Keyword::Opening => self.opening(line),
            Keyword::Sync => self.sync(line),
            Keyword::Pattern => self.named_pattern(line),
            Keyword::Format => self.format(line),
            Keyword::System if !first => self.fail(system_not_first(keyword.loc)),
            Keyword::System | Keyword::Use => {
                let path = self.name("expected-path", "a system path such as `us/401k`")?;
                self.setting(
                    line,
                    if matches!(kind, Keyword::System) { Setting::System(path) } else { Setting::Use(path) },
                )
            }
            Keyword::Base => {
                let unit = self.unit("expected-commodity", "the base commodity, such as `USD`")?;
                self.setting(line, Setting::Base(unit))
            }
            Keyword::Relaxed => self.setting(line, Setting::Relaxed),
            Keyword::Currency => {
                let unit = self.unit("expected-commodity", "the commodity its laws count in, such as `USD`")?;
                self.setting(line, Setting::Currency(unit))
            }
            Keyword::Rates => {
                let rates = match self.tok() {
                    Tok::Name("param") => {
                        self.bump();
                        Rates::Param(self.name("expected-name", "the param that holds the rates, such as `irs-rates`")?)
                    }
                    _ => self.choose(&RATES, "unknown-rates", "way to convert").map(|(rates, _)| rates)?,
                };
                self.setting(line, Setting::Rates(rates))
            }
        }
    }

    /// A one-line directive, once what it names is read.
    fn setting(&mut self, line: &mut Line<'s>, setting: Setting<'s>) -> Parse<()> {
        let header = self.end_header(line)?;
        self.emit(&header, setting, ItemKind::Setting);
        Ok(())
    }

    /// A first word that is no keyword. A close spelling gets a fix, and the
    /// line is read as that keyword, so a typo does not make every later use of
    /// what it declares an error too; otherwise the rest of the line says what
    /// the author probably meant.
    fn unknown_keyword(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<()> {
        match word {
            "every" | "plan" => return self.fail(plan_is_a_contract(keyword.loc, word)),
            "layout" => return self.fail(layout_is_gone(self.line_loc(line))),
            _ => {}
        }
        let diag = Diagnostic::error("unknown-keyword", format!("unknown keyword `{word}`"))
            .label(keyword.loc, "a line starts with a date or a keyword");
        let indent = self.point(line.start as u32);
        let looks_like_leg = matches!(
            self.tok(),
            Tok::Number(_)
                | Tok::Percent(_)
                | Tok::Punct(Punct::Ellipsis | Punct::Eq | Punct::LParen | Punct::Question)
        );
        let near = closest(word, KEYWORDS.iter().map(|(known, _)| *known));
        let diag = if let Some(near) = near {
            diag.fix(format!("did you mean `{near}`?"), keyword.loc, near)
        } else if let Some(&(_, owner)) = BLOCK_WORDS.iter().find(|(known, _)| *known == word) {
            diag.fix(format!("`{word}` is a line of a `{owner}`: indent it under one"), indent, "  ")
        } else if self.at(Punct::Arrow) {
            diag.help(format!("a transaction starts with its date: `2026-01-15 {word} -> …`"))
        } else if looks_like_leg {
            diag.fix("if this is a leg of the item above, indent it", indent, "  ")
        } else {
            diag.note(format!("the keywords are {}", list_words(&KEYWORDS)))
        };
        let reported = self.report(diag);
        match near {
            Some(near) => self.keyword_item(line, keyword, near, first),
            None => Err(reported),
        }
    }

    /// Indented lines that no item claimed. Under an item that parsed, they are
    /// a mistake worth naming; under one that failed they are just skipped.
    fn discard_block(&mut self, item: &Line<'s>, item_parsed: bool) {
        let Some(first) = self.next_child(item.indent) else { return };
        if item_parsed {
            let diag = Diagnostic::error("unexpected-indent", "this item takes no indented lines")
                .label(self.line_loc(&first), "indented line with nothing to belong to")
                .context(self.line_loc(item), "this line is complete on its own");
            self.diags.push(diag);
        }
        self.skip_block(item.indent);
    }

    fn report_orphan(&mut self, line: &Line<'s>) {
        let diag = Diagnostic::error("unexpected-indent", "this line is indented but nothing above it takes a block")
            .label(self.line_loc(line), "items start in column 0")
            .fix("remove the indentation", self.indentation(line), "");
        self.diags.push(diag);
        self.skip_block(0);
    }

    // ─── Blocks ─────────────────────────────────────────────────────────────

    /// Parses each line of the block under `parent` with `each`.
    ///
    /// A line that fails does not stop the block: its diagnostic is recorded
    /// and the remaining lines are still parsed. The block then reports
    /// failure, so an item that needs every line drops itself; one that can do
    /// without the bad line (a declaration) ignores the result and keeps the
    /// rest.
    pub fn children(
        &mut self,
        parent: &Line<'s>,
        each: impl FnMut(&mut Self, &mut Line<'s>) -> Parse<()>,
    ) -> Parse<()> {
        self.block(parent, false, each)
    }

    /// [`Parser::children`], where with `ragged` a line that starts with an
    /// amount (an item) may be indented further than the others.
    pub fn block(
        &mut self,
        parent: &Line<'s>,
        ragged: bool,
        mut each: impl FnMut(&mut Self, &mut Line<'s>) -> Parse<()>,
    ) -> Parse<()> {
        let mut block = Block { indent: None, previous: self.line_loc(parent), ragged };
        let mut intact = true;
        while let Some(mut line) = self.next_child(parent.indent) {
            if !self.is_aligned(&mut block, &line) {
                intact = false;
                continue;
            }
            self.begin_line(&line);
            let roots = self.roots.len();
            if each(self, &mut line).is_err() {
                self.roots.truncate(roots);
                intact = false;
            }
            self.warn_ignored_doc(&line);
            block.previous = self.line_loc(&line);
        }
        if intact { Ok(()) } else { Err(Reported) }
    }

    /// The next line if it is indented deeper than `parent_indent`.
    pub fn next_child(&mut self, parent_indent: usize) -> Option<Line<'s>> {
        let indent = self.lines.peek()?.indent;
        if indent <= parent_indent {
            return None;
        }
        self.lines.next()
    }

    /// Discards the rest of a block without looking at it.
    pub fn skip_block(&mut self, parent_indent: usize) {
        while self.next_child(parent_indent).is_some() {}
    }

    /// Whether `line` sits at its block's indentation. A line indented further
    /// drags its own deeper lines with it, which are skipped so one mistake is
    /// reported once.
    fn is_aligned(&mut self, block: &mut Block, line: &Line<'s>) -> bool {
        let expected = *block.indent.get_or_insert(line.indent);
        let amount_first = matches!(self.src.as_bytes()[line.body], b'0'..=b'9' | b'+' | b'-');
        if line.indent == expected || (block.ragged && line.indent > expected && amount_first) {
            return true;
        }
        let there = format!("indented {} spaces, where the block uses {expected}", line.indent);
        let diag = if line.indent > expected {
            self.skip_block(expected);
            Diagnostic::error("unexpected-indent", "this line is indented further than the lines above it")
                .label(self.line_loc(line), there)
                .context(block.previous, "this line takes no indented block")
        } else {
            Diagnostic::error("inconsistent-indent", "this line's indentation matches no block above it")
                .label(self.line_loc(line), there)
                .note("the lines of a block share one indentation, and belong to the nearest line indented less")
        };
        self.diags.push(diag.fix("align it with the block", self.indentation(line), " ".repeat(expected)));
        false
    }

    pub fn indentation(&self, line: &Line<'s>) -> Loc {
        Loc::new(self.id, line.start as u32, line.body as u32)
    }

    /// Lines that cannot be documented (properties, steps, rows) still tell the
    /// author when a `///` block above them is being ignored.
    pub fn warn_ignored_doc(&mut self, line: &Line<'s>) {
        if let Some(doc) = line.doc {
            let diag = Diagnostic::warning("misplaced-doc", "this doc comment is ignored")
                .label(doc.loc, "nothing here takes documentation")
                .help("`///` documents items, legs and laws; use `//` for an ordinary comment");
            self.diags.push(diag);
        }
    }
}

/// v3's `every …` and `plan NAME every …`.
fn plan_is_a_contract(loc: Loc, word: &str) -> Diagnostic {
    Diagnostic::error("plan-is-a-contract", format!("`{word}` is gone: what repeats is a contract"))
        .label(loc, "a promise of flows, with a name and a party")
        .note("a contract states its schedule once, and the journal records each time it is kept")
        .help("write `contract NAME with PARTY` and, indented, `45 USD monthly on 8 from visa`; then `08 NAME` says it")
}

/// v3's `layout free`, which turned off a rule that no longer exists.
fn layout_is_gone(line: Loc) -> Diagnostic {
    Diagnostic::error("layout-is-gone", "`layout` is gone: where a file is kept never limits its dates")
        .label(line, "nothing checks a file's dates against its place")
        .note("a short date takes its year and month from the nearest heading above it, else from the file's folder")
        .fix("remove it", line, "")
}

fn system_not_first(loc: Loc) -> Diagnostic {
    Diagnostic::error("system-not-first", "`system` must be the first item of the file")
        .label(loc, "a file is either a system or a project file")
        .help("move this line to the top, or remove it if this file is not a system")
}
