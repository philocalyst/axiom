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
use crate::lex::{Tok, Token};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported};

#[rustfmt::skip]
const KEYWORDS: [&str; 18] = [
    "account", "entity", "asset", "purpose", "commodity", "kind", "contract", "budget", "code", "param", "law", "sync",
    "system", "use", "base", "relaxed", "layout", "opening",
];

/// Words that start a line inside a block, and what owns such a block. Written
/// at column 0 they are a block's line whose block was forgotten.
const BLOCK_WORDS: [(&str, &str); 9] = [
    ("when", "law"),
    ("let", "law"),
    ("require", "law"),
    ("warn", "law"),
    ("owe", "law"),
    ("count", "law"),
    ("always", "law"),
    ("each", "law"),
    ("run", "sync"),
];

/// The indentation a block has settled on, and the line before, for context.
struct Block {
    indent: Option<usize>,
    previous: Loc,
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
            Tok::Date(_) | Tok::MonthDay(..) | Tok::Number(_) => {
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
        match word {
            "account" => self.decl(line, DeclKind::Account),
            "entity" => self.decl(line, DeclKind::Entity),
            "asset" => self.decl(line, DeclKind::Asset),
            "purpose" => self.decl(line, DeclKind::Purpose),
            "commodity" => self.decl(line, DeclKind::Commodity),
            "kind" => self.decl(line, DeclKind::Kind),
            "budget" => self.budget(line),
            "code" => self.code_rule(line),
            "param" => self.param(line),
            "law" => self.law_item(line),
            "contract" => self.contract(line),
            "every" | "plan" => self.fail(plan_is_a_contract(keyword.loc, word)),
            "opening" => self.opening(line),
            "sync" => self.sync(line),
            "system" | "use" | "base" | "relaxed" | "layout" => self.setting(line, keyword, word, first),
            _ => self.unknown_keyword(line, keyword, word, first),
        }
    }

    /// One-line directives.
    fn setting(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<()> {
        let path = "a system path such as `us/401k`";
        let setting = match word {
            "system" if !first => return self.fail(system_not_first(keyword.loc)),
            "system" => Setting::System(self.name("expected-path", path)?),
            "use" => Setting::Use(self.name("expected-path", path)?),
            "base" => Setting::Base(self.unit("expected-commodity", "the base commodity, such as `USD`")?),
            "relaxed" => Setting::Relaxed,
            _ => {
                self.expect_word("free", "expected-layout", "`free`")?;
                Setting::LayoutFree
            }
        };
        let header = self.end_header(line)?;
        self.emit(&header, setting, ItemKind::Setting);
        Ok(())
    }

    /// A first word that is no keyword. A close spelling gets a fix, and the
    /// line is read as that keyword, so a typo does not make every later use of
    /// what it declares an error too; otherwise the rest of the line says what
    /// the author probably meant.
    fn unknown_keyword(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<()> {
        let diag = Diagnostic::error("unknown-keyword", format!("unknown keyword `{word}`"))
            .label(keyword.loc, "a line starts with a date or a keyword");
        let indent = self.point(line.start as u32);
        let looks_like_leg = matches!(self.tok(), Tok::Number(_) | Tok::Punct("..." | "=" | "(" | "?"));
        let near = closest(word, KEYWORDS);
        let diag = if let Some(near) = near {
            diag.fix(format!("did you mean `{near}`?"), keyword.loc, near)
        } else if let Some(&(_, owner)) = BLOCK_WORDS.iter().find(|(known, _)| *known == word) {
            diag.fix(format!("`{word}` is a line of a `{owner}`: indent it under one"), indent, "  ")
        } else if self.at("->") {
            diag.help(format!("a transaction starts with its date: `2026-01-15 {word} -> …`"))
        } else if looks_like_leg {
            diag.fix("if this is a leg of the item above, indent it", indent, "  ")
        } else {
            diag.note(format!("the keywords are `{}`", KEYWORDS.join("`, `")))
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
        mut each: impl FnMut(&mut Self, &mut Line<'s>) -> Parse<()>,
    ) -> Parse<()> {
        let mut block = Block { indent: None, previous: self.line_loc(parent) };
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
        if line.indent == expected {
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
    fn warn_ignored_doc(&mut self, line: &Line<'s>) {
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
        .help("write `contract NAME with PARTY` and, indented, `45 USD monthly on 8 from visa`; then `08 NAME` kept")
}

fn system_not_first(loc: Loc) -> Diagnostic {
    Diagnostic::error("system-not-first", "`system` must be the first item of the file")
        .label(loc, "a file is either a system or a project file")
        .help("move this line to the top, or remove it if this file is not a system")
}
