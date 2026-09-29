//! Source text to a borrowed syntax tree.
//!
//! Parsing is line-oriented: a column-0 line starts an item and indented lines
//! belong to it, so each item parses (and recovers) on its own. Lines are found
//! with `memchr`; dates and digit runs are read eight bytes at a time.

pub mod ast;

pub use ast::*;

use axiom_core::{Diagnostic, FileId};

/// Parses one file. Every item that parses is kept; each one that does not
/// produces a diagnostic and is skipped.
pub fn parse(file: FileId, src: &str) -> (File<'_>, Vec<Diagnostic>) {
    let _ = (file, src);
    todo!("lane A")
}
