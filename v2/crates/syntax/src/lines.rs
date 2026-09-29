//! Splitting source into lines.
//!
//! Structure is built from lines: a line's indentation says which item or block
//! it belongs to, and the lexer reads everything after it. This module finds the
//! lines (with `memchr`), measures indentation, drops blank and comment-only
//! lines, and attaches each run of `///` lines to the line that follows it.

use std::ops::Range;

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

/// The lines indented with a tab, which are reported once for the file.
#[derive(Clone, Copy, Default)]
pub(crate) struct Tabs {
    pub count: u32,
    /// The first such line's indentation, and how many columns it is.
    pub first: Option<(Loc, usize)>,
}

impl Tabs {
    /// Adds the tabs of the piece that follows.
    pub fn merge(&mut self, later: Tabs) {
        self.count += later.count;
        self.first = self.first.or(later.first);
    }

    /// One error for the whole file, whatever number of lines use tabs.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        let (indentation, columns) = self.first?;
        let (message, label) = match self.count {
            1 => ("a line is indented with a tab".to_string(), "this indentation has a tab".to_string()),
            n => (format!("{n} lines are indented with a tab"), "the first, of all of them".to_string()),
        };
        let diag = Diagnostic::error("tab-indent", message)
            .label(indentation, label)
            .note("indentation is spaces, so a block's shape looks the same in every editor");
        Some(diag.fix("indent with two spaces per tab", indentation, " ".repeat(columns)))
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
    end: usize,
    peeked: Option<Line<'s>>,
    pending_doc: Option<Pending>,
    pub tabs: Tabs,
    /// A `///` block that documents nothing, found at the end.
    pub stray_doc: Option<Loc>,
}

impl<'s> Lines<'s> {
    /// The lines of `src[range]`, which starts at a line start.
    pub fn new(src: &'s str, file: FileId, range: Range<usize>) -> Lines<'s> {
        let bom = if range.start == 0 && src.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
        Lines {
            src,
            file,
            pos: range.start + bom,
            end: range.end,
            peeked: None,
            pending_doc: None,
            tabs: Tabs::default(),
            stray_doc: None,
        }
    }

    pub fn next(&mut self) -> Option<Line<'s>> {
        self.peeked.take().or_else(|| self.scan())
    }

    pub fn peek(&mut self) -> Option<&Line<'s>> {
        if self.peeked.is_none() {
            self.peeked = self.scan();
        }
        self.peeked.as_ref()
    }

    fn scan(&mut self) -> Option<Line<'s>> {
        let bytes = &self.src.as_bytes()[..self.end];
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
            let indent = self.measure_indent(start, body);
            let doc = self.pending_doc.take().map(|pending| self.doc_block(&pending));
            return Some(Line { start, body, end, indent, doc });
        }
        if let Some(pending) = self.pending_doc.take() {
            self.stray_doc = Some(self.doc_block(&pending).loc);
        }
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

    /// The width of the indentation. A tab counts as two columns and is
    /// counted, so the whole file is reported once, however many lines use one.
    fn measure_indent(&mut self, start: usize, body: usize) -> usize {
        let blanks = &self.src.as_bytes()[start..body];
        let columns = blanks.iter().map(|&b| if b == b'\t' { 2 } else { 1 }).sum();
        if blanks.contains(&b'\t') {
            self.tabs.count += 1;
            let indentation = Loc::new(self.file, start as u32, body as u32);
            self.tabs.first.get_or_insert((indentation, columns));
        }
        columns
    }
}

/// A `///` block that documents nothing.
pub(crate) fn unattached_doc(loc: Loc) -> Diagnostic {
    Diagnostic::warning("unattached-doc", "this doc comment documents nothing")
        .label(loc, "no item, leg or law follows it")
        .help("`///` documents the item, leg or law below it; use `//` for an ordinary comment")
}
