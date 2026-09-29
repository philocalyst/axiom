//! Indentation: which lines belong to the line above.
//!
//! An item is a column-0 line plus every deeper line after it. A block's lines
//! must share one indentation, fixed by its first line; deeper lines belong to
//! the block's own sub-blocks. [`Parser::children`] walks a block, parses each
//! line on its own, and keeps going after a bad one so every error is reported.

use axiom_core::{Diagnostic, Loc};

use crate::ast::{Doc, Item, ItemKind};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported};

/// A parsed header line: where the item is, and what documents it.
pub(crate) struct Header<'s> {
    pub loc: Loc,
    pub doc: Option<Doc<'s>>,
}

impl<'s> Header<'s> {
    pub fn item(self, kind: ItemKind<'s>) -> Item<'s> {
        Item { doc: self.doc, loc: self.loc, kind }
    }
}

/// The indentation a block has settled on, and the line before, for context.
struct Block {
    indent: Option<usize>,
    previous: Loc,
}

impl<'s> Parser<'s> {
    /// Ends a header line: nothing may follow what was parsed. The header's
    /// location stops at its last token, so it excludes a trailing comment and
    /// the block below.
    pub fn end_header(&mut self, line: &mut Line<'s>) -> Parse<Header<'s>> {
        self.expect_eol()?;
        Ok(Header { loc: self.loc_from(line.body), doc: line.take_doc() })
    }

    /// Parses each line of the block under `parent` with `each`.
    ///
    /// A line that fails does not stop the block: its diagnostic is recorded,
    /// the remaining lines are still parsed, and the block as a whole then
    /// reports failure so the item is dropped rather than kept half-built.
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
            intact &= each(self, &mut line).is_ok();
            self.warn_ignored_doc(&line);
            block.previous = self.line_loc(&line);
        }
        if intact { Ok(()) } else { Err(Reported) }
    }

    /// The next line if it is indented deeper than `parent_indent`.
    pub fn next_child(&mut self, parent_indent: usize) -> Option<Line<'s>> {
        let indent = self.lines.peek(&mut self.diags)?.indent;
        if indent <= parent_indent {
            return None;
        }
        self.lines.next(&mut self.diags)
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
        let diag = if line.indent > expected {
            self.skip_block(expected);
            self.over_indented(line, expected, block.previous)
        } else {
            self.under_indented(line, expected)
        };
        self.diags.push(diag);
        false
    }

    fn over_indented(&self, line: &Line<'s>, expected: usize, previous: Loc) -> Diagnostic {
        Diagnostic::error("unexpected-indent", "this line is indented further than the lines above it")
            .label(self.line_loc(line), format!("indented {} spaces, where the block uses {expected}", line.indent))
            .context(previous, "this line takes no indented block")
            .fix("align it with the block", self.indentation(line), " ".repeat(expected))
    }

    fn under_indented(&self, line: &Line<'s>, expected: usize) -> Diagnostic {
        Diagnostic::error("inconsistent-indent", "this line's indentation matches no block above it")
            .label(self.line_loc(line), format!("indented {} spaces, where the block uses {expected}", line.indent))
            .note("the lines of a block share one indentation, and belong to the nearest line indented less")
            .fix("align it with the block", self.indentation(line), " ".repeat(expected))
    }

    fn indentation(&self, line: &Line<'s>) -> Loc {
        Loc::new(self.file, line.start as u32, line.body as u32)
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
