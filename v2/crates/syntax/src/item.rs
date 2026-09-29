//! Top-level items: reading a file's column-0 lines and dispatching each one on
//! its first word.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};

use crate::ast::{DeclKind, Item, ItemKind, Setting};
use crate::lex::{Tok, Token};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported};

#[rustfmt::skip]
const KEYWORDS: [&str; 14] = [
    "account", "entity", "commodity", "kind", "code", "param", "law", "every", "sync", "system", "use", "base",
    "relaxed", "layout",
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

impl<'s> Parser<'s> {
    /// Every item of the file. An item that fails is skipped along with its
    /// block, and parsing resumes at the next column-0 line.
    pub fn items(&mut self) -> Vec<Item<'s>> {
        let mut items = Vec::new();
        let mut first = true;
        while let Some(mut line) = self.lines.next(&mut self.diags) {
            if line.indent > 0 {
                self.report_orphan(&line);
                continue;
            }
            let parsed = self.item(&mut line, first);
            first = false;
            self.discard_block(&line, parsed.is_ok());
            if let Ok(item) = parsed {
                items.push(item);
            }
        }
        items
    }

    fn item(&mut self, line: &mut Line<'s>, first: bool) -> Parse<Item<'s>> {
        self.begin_line(line);
        let token = self.cursor.peek();
        match token.tok {
            Tok::Date(date) => {
                self.cursor.bump();
                self.journal_entry(line, date)
            }
            Tok::Name(word) => {
                self.cursor.bump();
                self.keyword_item(line, token, word, first)
            }
            _ => Err(self.expected("expected-item", "a date or a keyword")),
        }
    }

    fn keyword_item(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<Item<'s>> {
        match word {
            "account" => self.decl(line, DeclKind::Account),
            "entity" => self.decl(line, DeclKind::Entity),
            "commodity" => self.decl(line, DeclKind::Commodity),
            "kind" => self.decl(line, DeclKind::Kind),
            "code" => self.code_rule(line),
            "param" => self.param(line),
            "law" => self.law_item(line),
            "every" => self.plan(line),
            "sync" => self.sync(line),
            "system" | "use" | "base" | "relaxed" | "layout" => self.setting(line, keyword, word, first),
            _ => Err(self.unknown_keyword(line, keyword, word)),
        }
    }

    /// One-line directives.
    fn setting(&mut self, line: &mut Line<'s>, keyword: Token<'s>, word: &str, first: bool) -> Parse<Item<'s>> {
        let setting = match word {
            "system" if !first => return self.fail(system_not_first(keyword.loc)),
            "system" => Setting::System(self.name("expected-path", "a system path such as `us/401k`")?),
            "use" => Setting::Use(self.name("expected-path", "a system path such as `us/401k`")?),
            "base" => Setting::Base(self.unit("expected-commodity", "the base commodity, such as `USD`")?),
            "relaxed" => Setting::Relaxed(keyword.loc),
            _ => {
                let free = self.expect_word("free", "expected-layout", "`free`")?;
                Setting::LayoutFree(keyword.loc.to(free))
            }
        };
        let header = self.end_header(line)?;
        Ok(header.item(ItemKind::Setting(setting)))
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
        let indentation = Loc::new(self.file, line.start as u32, line.body as u32);
        let diag = Diagnostic::error("unexpected-indent", "this line is indented but nothing above it takes a block")
            .label(self.line_loc(line), "items start in column 0")
            .fix("remove the indentation", indentation, "");
        self.diags.push(diag);
        self.skip_block(0);
    }

    /// A first word that is no keyword. Close spellings get a fix; otherwise
    /// the rest of the line says what the author probably meant.
    fn unknown_keyword(&mut self, line: &Line<'s>, keyword: Token<'s>, word: &str) -> Reported {
        let diag = Diagnostic::error("unknown-keyword", format!("unknown keyword `{word}`"))
            .label(keyword.loc, "a line starts with a date or a keyword");
        let indent = Loc::new(self.file, line.start as u32, line.start as u32);
        let looks_like_leg =
            matches!(self.cursor.peek().tok, Tok::Number(_) | Tok::Ellipsis | Tok::Eq | Tok::LParen | Tok::Question);
        let diag = if let Some(near) = closest(word, KEYWORDS) {
            diag.fix(format!("did you mean `{near}`?"), keyword.loc, near)
        } else if let Some(&(_, owner)) = BLOCK_WORDS.iter().find(|(known, _)| *known == word) {
            diag.fix(format!("`{word}` is a line of a `{owner}`: indent it under one"), indent, "  ")
        } else if let Some(iso) = slash_date(word) {
            diag.fix(format!("dates are written `{iso}`"), keyword.loc, iso)
        } else if matches!(self.cursor.peek().tok, Tok::Arrow) {
            diag.help(format!("a transaction starts with its date: `2026-01-15 {word} -> …`"))
        } else if looks_like_leg {
            diag.fix("if this is a leg of the item above, indent it", indent, "  ")
        } else {
            diag.note(format!("the keywords are `{}`", KEYWORDS.join("`, `")))
        };
        self.report(diag)
    }
}

/// `2026/01/15` as `2026-01-15`: the date written with slashes.
fn slash_date(word: &str) -> Option<String> {
    let mut parts = word.split('/');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    let numbers = [year, month, day].iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    (numbers && year.len() == 4 && parts.next().is_none()).then(|| format!("{year}-{month:0>2}-{day:0>2}"))
}

fn system_not_first(loc: Loc) -> Diagnostic {
    Diagnostic::error("system-not-first", "`system` must be the first item of the file")
        .label(loc, "a file is either a system or a project file")
        .help("move this line to the top, or remove it if this file is not a system")
}
