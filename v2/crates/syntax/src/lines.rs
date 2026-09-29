//! Splitting source into lines.
//!
//! Structure is built from lines: a line's indentation says which item or block
//! it belongs to, and the lexer reads everything after it. This module finds the
//! lines (with `memchr`), measures indentation, drops blank and comment-only
//! lines, and attaches each run of `///` lines to the line that follows it.

use axiom_core::{Diagnostic, FileId, Loc};
use memchr::memchr;

use crate::ast::Doc;

/// A line with something on it besides a comment.
#[derive(Clone, Copy)]
pub(crate) struct Line<'s> {
    /// Offset of the first byte of the line.
    pub start: usize,
    /// Offset of the first byte after the indentation.
    pub body: usize,
    /// Offset just past the last byte, before any `\r\n`.
    pub end: usize,
    /// Width of the indentation in columns. A tab counts as two, which is what
    /// the `tab-indent` fix replaces it with.
    pub indent: usize,
    pub doc: Option<DocBlock<'s>>,
}

/// The `///` block directly above a line.
#[derive(Clone, Copy)]
pub(crate) struct DocBlock<'s> {
    pub text: Doc<'s>,
    pub loc: Loc,
}

impl<'s> Line<'s> {
    /// Claims the documentation. Lines that cannot be documented leave it
    /// unclaimed, and the parser warns that it was ignored.
    pub fn take_doc(&mut self) -> Option<Doc<'s>> {
        self.doc.take().map(|block| block.text)
    }
}

/// Where a pending doc block sits, so the next `///` line can tell whether it
/// continues it.
struct Pending {
    start: usize,
    end: usize,
    /// Where the line after the block's last line starts.
    next_line: usize,
}

pub(crate) struct Lines<'s> {
    src: &'s str,
    file: FileId,
    pos: usize,
    peeked: Option<Line<'s>>,
    pending_doc: Option<Pending>,
}

impl<'s> Lines<'s> {
    pub fn new(src: &'s str, file: FileId) -> Lines<'s> {
        let bom = if src.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
        Lines { src, file, pos: bom, peeked: None, pending_doc: None }
    }

    pub fn next(&mut self, diags: &mut Vec<Diagnostic>) -> Option<Line<'s>> {
        self.peeked.take().or_else(|| self.scan(diags))
    }

    pub fn peek(&mut self, diags: &mut Vec<Diagnostic>) -> Option<&Line<'s>> {
        if self.peeked.is_none() {
            self.peeked = self.scan(diags);
        }
        self.peeked.as_ref()
    }

    fn scan(&mut self, diags: &mut Vec<Diagnostic>) -> Option<Line<'s>> {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() {
            let start = self.pos;
            let newline = memchr(b'\n', &bytes[start..]).map_or(bytes.len(), |i| start + i);
            self.pos = (newline + 1).min(bytes.len());
            let end = if newline > start && bytes[newline - 1] == b'\r' { newline - 1 } else { newline };
            let blanks = bytes[start..end].iter().take_while(|b| matches!(b, b' ' | b'\t')).count();
            let body = start + blanks;
            let text = &bytes[body..end];
            if text.is_empty() {
                continue;
            }
            if text.starts_with(b"///") && text.get(3) != Some(&b'/') {
                self.extend_doc(start, body, end);
                continue;
            }
            if text.starts_with(b"//") {
                continue;
            }
            let indent = self.measure_indent(start, body, diags);
            let doc = self.pending_doc.take().map(|pending| self.doc_block(&pending));
            return Some(Line { start, body, end, indent, doc });
        }
        self.warn_dangling_doc(diags);
        None
    }

    /// Adds the `///` line starting at `start` to the pending block, or begins
    /// a new block when something else separates it from the last doc line.
    fn extend_doc(&mut self, start: usize, body: usize, end: usize) {
        match &mut self.pending_doc {
            Some(pending) if pending.next_line == start => {
                pending.end = end;
                pending.next_line = self.pos;
            }
            _ => self.pending_doc = Some(Pending { start: body, end, next_line: self.pos }),
        }
    }

    fn doc_block(&self, pending: &Pending) -> DocBlock<'s> {
        let text = Doc(&self.src[pending.start..pending.end]);
        DocBlock { text, loc: Loc::new(self.file, pending.start as u32, pending.end as u32) }
    }

    fn measure_indent(&self, start: usize, body: usize, diags: &mut Vec<Diagnostic>) -> usize {
        let blanks = &self.src.as_bytes()[start..body];
        let Some(first_tab) = blanks.iter().position(|&b| b == b'\t') else {
            return blanks.len();
        };
        let columns: usize = blanks.iter().map(|&b| if b == b'\t' { 2 } else { 1 }).sum();
        let tab = Loc::new(self.file, (start + first_tab) as u32, (start + first_tab + 1) as u32);
        diags.push(
            Diagnostic::error("tab-indent", "tabs cannot indent a line")
                .label(tab, "this tab")
                .note("indentation is spaces, so a block's shape looks the same in every editor")
                .fix(
                    "indent with two spaces per tab",
                    Loc::new(self.file, start as u32, body as u32),
                    " ".repeat(columns),
                ),
        );
        columns
    }

    fn warn_dangling_doc(&mut self, diags: &mut Vec<Diagnostic>) {
        if let Some(pending) = self.pending_doc.take() {
            let loc = self.doc_block(&pending).loc;
            diags.push(unattached_doc(loc));
        }
    }
}

/// A `///` block that documents nothing.
fn unattached_doc(loc: Loc) -> Diagnostic {
    Diagnostic::warning("unattached-doc", "this doc comment documents nothing")
        .label(loc, "no item, leg or law follows it")
        .help("`///` documents the item, leg or law below it; use `//` for an ordinary comment")
}
