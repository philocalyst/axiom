//! Source text to a borrowed syntax tree.
//!
//! Parsing is line-oriented: a column-0 line starts an item and indented lines
//! belong to it, so each item parses (and recovers) on its own. Lines are found
//! with `memchr`, and dates are read eight bytes at a time.
//! That independence is also what lets a large file be cut at item boundaries
//! and its pieces parsed on every core (see [`parse`]).
//!
//! | module      | job                                                        |
//! |-------------|------------------------------------------------------------|
//! | `ast`       | the tree: small items, per-piece tables, typed ranges      |
//! | `lines`     | lines, indentation, comments, `///` blocks                 |
//! | `lex`       | one line's tokens, with two of lookahead                   |
//! | `parser`    | parser state and the helpers every rule shares             |
//! | `structure` | item dispatch, one-line directives, indented blocks        |
//! | `journal`   | dated lines: flows and what statements are about, openings |
//! | `statement` | statements: subject, predicate, `until`, description, codes |
//! | `refs`      | typed indices: which piece of the file, and where in it    |
//! | `dates`     | dates in full or short of what the folder or heading gives |
//! | `flow`      | flow headers, ends, legs, tails, lot selectors             |
//! | `contract`  | contracts: schedule, properties, template legs             |
//! | `amount`    | amounts, and what is said about the ones that are wrong    |
//! | `decl`, `law` | declarations, params, syncs, laws                        |
//! | `expr`      | the expression grammar                                     |
//! | `malformed` | diagnostics for words that are not tokens                  |

pub mod ast;

mod amount;
mod contract;
mod dates;
mod decl;
mod expr;
mod flow;
mod journal;
mod law;
mod lex;
mod lines;
mod malformed;
mod parser;
mod refs;
mod statement;
mod structure;

#[cfg(test)]
mod tests;

pub use ast::*;
pub use dates::MONTHS;

use std::ops::Range;

use axiom_core::{Diagnostic, FileId, Loc, par};
use memchr::{memchr, memchr_iter, memrchr};

use crate::ast::Piece;
use crate::dates::heading;
use crate::lines::{Tabs, unattached_doc};
use crate::parser::Parser;
use crate::refs::MAX_PIECES;

/// A file smaller than this is parsed by one thread: cutting and joining would
/// cost more than it saves.
const PARALLEL_MIN: usize = 1 << 20;

/// Pieces to cut a large file into, per core: enough that pieces of uneven
/// difficulty even out.
const PIECES_PER_CORE: usize = 4;

/// The size pieces are cut to, at most, so that their tables cannot outgrow
/// what an index can say (2^24 of a kind, and a node takes a byte of source).
const PIECE_TARGET: usize = 1 << 23;

/// Files larger than this cannot be cut into the pieces an index can name.
const FILE_MAX: usize = MAX_PIECES * PIECE_TARGET;

/// Parses one file, which is in `folder`. Every item that parses is kept; each
/// one that does not produces a diagnostic and is skipped. Diagnostics come in
/// source order.
///
/// A date short of what `folder` (or a heading above it) gives is completed
/// here. Nothing checks that a whole date agrees with either: dates are a
/// convention of where files are kept, not a law (§10).
///
/// A large file is cut at item boundaries and its pieces are parsed on every
/// core. They keep their tables, and the file is their items in order.
pub fn parse(file: FileId, src: &str, folder: Folder) -> (File<'_>, Vec<Diagnostic>) {
    if src.len() > FILE_MAX {
        let diag = Diagnostic::error("file-too-large", "source files are limited to 2 GiB")
            .label(Loc::new(file, 0, 0), "this file is larger");
        return (File::new(file, src, Vec::new()), vec![diag]);
    }
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    let pieces = match src.len() < PARALLEL_MIN {
        true => 1,
        false => (cores * PIECES_PER_CORE).max(src.len().div_ceil(PIECE_TARGET)).min(MAX_PIECES),
    };
    parse_in(file, src, folder, pieces)
}

/// [`parse`] with the file cut into about `pieces` pieces.
pub(crate) fn parse_in(file: FileId, src: &str, folder: Folder, pieces: usize) -> (File<'_>, Vec<Diagnostic>) {
    let ranges = cut(src, pieces);
    // What each piece holds is read first, because what a piece's dates leave
    // out depends on the last heading before it, in whichever piece that was.
    let scans = par::map_each(&ranges, |range| Scan::of(&src.as_bytes()[range.clone()]));
    let mut context = folder;
    let jobs: Vec<Job> = ranges
        .into_iter()
        .zip(scans)
        .enumerate()
        .map(|(number, (range, scan))| {
            let job = Job { number, range, folder: context, scan };
            context = job.scan.heading.unwrap_or(context);
            job
        })
        .collect();
    let parsed = par::map_each(&jobs, |job| parse_piece(file, src, job));
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

/// A parsed piece, what it found wrong, and its lines that were indented with tabs.
type Parsed<'s> = (Piece<'s>, Vec<Diagnostic>, Tabs);

/// One piece of the file to parse: which, where, what its dates start out as,
/// and what a first look at it found.
struct Job {
    number: usize,
    range: Range<usize>,
    folder: Folder,
    scan: Scan,
}

/// The piece `job` describes, parsed.
fn parse_piece<'s>(file: FileId, src: &'s str, job: &Job) -> Parsed<'s> {
    let mut parser = Parser::new(file, src, job.range.clone(), job.number, job.folder);
    parser.items.reserve(job.scan.items);
    parser.reserve::<Txn>(job.scan.dated);
    parser.items(job.number == 0);
    let (piece, mut diags, lines) = parser.finish();
    diags.extend(lines.stray_doc.map(unattached_doc));
    (piece, diags, lines.tabs)
}

/// What one pass over a piece's lines finds: room for the tables, so they
/// never grow, and the last heading, which the pieces after it start from.
struct Scan {
    /// How many lines start an item.
    items: usize,
    /// How many of those start with a date (or a number).
    dated: usize,
    /// The context of the last heading line, if the piece has one.
    heading: Option<Folder>,
}

impl Scan {
    fn of(text: &[u8]) -> Scan {
        let first_is_digit = text.first().is_some_and(u8::is_ascii_digit);
        let mut scan = Scan { items: 1, dated: usize::from(first_is_digit), heading: None };
        scan.heading = text.first().filter(|_| first_is_digit).and_then(|_| heading(text));
        for newline in memchr_iter(b'\n', text) {
            match text.get(newline + 1) {
                Some(b'0'..=b'9') => {
                    (scan.items, scan.dated) = (scan.items + 1, scan.dated + 1);
                    // A whole date is not a heading, and is what nearly every dated line starts with.
                    let line = &text[newline + 1..];
                    if !(line.get(4) == Some(&b'-') && line.get(7) == Some(&b'-')) {
                        scan.heading = heading(line).or(scan.heading);
                    }
                }
                Some(b' ' | b'\t' | b'\r' | b'\n') | None => {}
                Some(_) => scan.items += 1,
            }
        }
        scan
    }
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
