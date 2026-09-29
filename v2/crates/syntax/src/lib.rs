//! Source text to a borrowed syntax tree.
//!
//! Parsing is line-oriented: a column-0 line starts an item and indented lines
//! belong to it, so each item parses (and recovers) on its own. Lines are found
//! with `memchr`; dates, digit runs and names are read eight bytes at a time.
//! That independence is also what lets a large file be cut at item boundaries
//! and its pieces parsed on every core (see [`parse`]).
//!
//! | module      | job                                                        |
//! |-------------|------------------------------------------------------------|
//! | `ast`       | the tree: small items, tables, typed ranges                |
//! | `lines`     | lines, indentation, comments, `///` blocks                 |
//! | `lex`       | one line's tokens                                          |
//! | `cursor`    | two-token lookahead and raw reads over a line              |
//! | `parser`    | parser state and the helpers every rule shares             |
//! | `block`     | indentation blocks and per-line error recovery             |
//! | `item`      | top-level dispatch and one-line directives                 |
//! | `journal`   | dated entries, openings and plans                          |
//! | `flow`      | flow headers, ends, legs, tails                            |
//! | `amount`, `select` | amounts and lot selectors                           |
//! | `decl`, `law` | declarations, params, syncs, laws                        |
//! | `expr`      | the expression grammar                                     |
//! | `errors`, `malformed` | diagnostics for what was found instead           |

pub mod ast;

mod amount;
mod decl;
mod expr;
mod flow;
mod journal;
mod law;
mod lex;
mod lines;
mod malformed;
mod parser;
mod structure;

#[cfg(test)]
mod tests;

pub use ast::*;

use std::ops::Range;

use axiom_core::{Diagnostic, FileId, Loc, par};
use memchr::{memchr, memchr_iter, memrchr};

use crate::ast::Piece;
use crate::lines::{Tabs, unattached_doc};
use crate::parser::Parser;

/// A file smaller than this is parsed by one thread: cutting and joining would
/// cost more than it saves.
const PARALLEL_MIN: usize = 1 << 20;

/// Pieces to cut a large file into, per core: enough that pieces of uneven
/// difficulty even out.
const PIECES_PER_CORE: usize = 4;

/// The size pieces are cut to, at most, so that their tables cannot outgrow
/// what an index can say (2^24 of a kind, and a node takes a byte of source).
const PIECE_TARGET: usize = 1 << 23;

/// Files larger than this cannot be cut into the 256 pieces an index can name.
const FILE_MAX: usize = 256 * PIECE_TARGET;

/// Parses one file. Every item that parses is kept; each one that does not
/// produces a diagnostic and is skipped. Diagnostics come in source order.
///
/// A large file is cut at item boundaries and its pieces are parsed on every
/// core. They keep their tables, and the file is their items in order.
pub fn parse(file: FileId, src: &str) -> (File<'_>, Vec<Diagnostic>) {
    if src.len() > FILE_MAX {
        let diag = Diagnostic::error("file-too-large", "source files are limited to 2 GiB")
            .label(Loc::new(file, 0, 0), "this file is larger");
        return (File::new(file, src, Vec::new()), vec![diag]);
    }
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    let pieces = match src.len() < PARALLEL_MIN {
        true => 1,
        false => (cores * PIECES_PER_CORE).max(src.len().div_ceil(PIECE_TARGET)).min(256),
    };
    parse_in(file, src, pieces)
}

/// [`parse`] with the file cut into about `pieces` pieces.
pub(crate) fn parse_in(file: FileId, src: &str, pieces: usize) -> (File<'_>, Vec<Diagnostic>) {
    let ranges: Vec<(usize, Range<usize>)> = cut(src, pieces).into_iter().enumerate().collect();
    let parsed = par::map_each(&ranges, |(number, range)| parse_piece(file, src, range.clone(), *number));
    let (mut pieces, mut diags, mut tabs) = (Vec::new(), Vec::new(), Tabs::default());
    for (piece, more, more_tabs) in parsed {
        pieces.push(piece);
        diags.extend(more);
        tabs.merge(more_tabs);
    }
    diags.extend(tabs.diagnostic());
    diags.sort_by_key(|diag| diag.anchor().map(|loc| loc.start));
    (File::new(file, src, pieces), diags)
}

/// Piece number `number` of the file, `src[range]`, parsed.
fn parse_piece(file: FileId, src: &str, range: Range<usize>, number: usize) -> (Piece<'_>, Vec<Diagnostic>, Tabs) {
    let mut parser = Parser::new(file, src, range.clone(), number);
    let (items, dated) = count_items(&src.as_bytes()[range]);
    parser.items.reserve(items);
    parser.t.txns.reserve(dated);
    parser.items(number == 0);
    let (piece, mut diags, lines) = parser.finish();
    diags.extend(lines.stray_doc.map(unattached_doc));
    (piece, diags, lines.tabs)
}

/// How many lines of `text` start an item, and how many of those start with a
/// date: room for the tables, so they never grow.
fn count_items(text: &[u8]) -> (usize, usize) {
    let mut counts = (1, usize::from(text.first().is_some_and(u8::is_ascii_digit)));
    for newline in memchr_iter(b'\n', text) {
        match text.get(newline + 1) {
            Some(b'0'..=b'9') => counts = (counts.0 + 1, counts.1 + 1),
            Some(b' ' | b'\t' | b'\r' | b'\n') | None => {}
            Some(_) => counts.0 += 1,
        }
    }
    counts
}

/// Cuts `src` into about `pieces` ranges, each starting where an item does.
fn cut(src: &str, pieces: usize) -> Vec<Range<usize>> {
    let bytes = src.as_bytes();
    let mut ranges = Vec::with_capacity(pieces);
    let mut start = 0;
    for piece in 1..pieces {
        let target = (bytes.len() / pieces * piece).max(start);
        if let Some(at) = item_boundary(bytes, target).filter(|&at| at > start) {
            ranges.push(start..at);
            start = at;
        }
    }
    ranges.push(start..bytes.len());
    ranges
}

/// The start of the first item after `from`. Comments and blank lines that
/// come right before it are counted as its own, so a `///` block is never
/// parted from what it documents.
pub(crate) fn item_boundary(bytes: &[u8], from: usize) -> Option<usize> {
    let mut line = memchr(b'\n', &bytes[from..])? + from + 1;
    loop {
        // Items start in column 0, and comments there are not items.
        if bytes.get(line).is_none_or(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/')) {
            break;
        }
        line += memchr(b'\n', &bytes[line..])? + 1;
    }
    (line < bytes.len()).then(|| comments_before(bytes, line))
}

/// The start of the run of blank and comment lines that ends just before `at`.
fn comments_before(bytes: &[u8], mut at: usize) -> usize {
    while at > 0 {
        let start = memrchr(b'\n', &bytes[..at - 1]).map_or(0, |newline| newline + 1);
        let text = bytes[start..at - 1].trim_ascii_start();
        if !(text.is_empty() || text.starts_with(b"//")) {
            break;
        }
        at = start;
    }
    at
}
