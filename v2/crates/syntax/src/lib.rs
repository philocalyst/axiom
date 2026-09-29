//! Source text to a borrowed syntax tree.
//!
//! Parsing is line-oriented: a column-0 line starts an item and indented lines
//! belong to it, so each item parses (and recovers) on its own. Lines are found
//! with `memchr`; dates and digit runs are read eight bytes at a time.
//!
//! | module      | job                                                        |
//! |-------------|------------------------------------------------------------|
//! | `lines`     | lines, indentation, comments, `///` blocks                 |
//! | `lex`       | one line's tokens, lazily                                  |
//! | `cursor`    | two-token lookahead and raw reads over a line              |
//! | `parser`    | parser state and the helpers every rule shares             |
//! | `block`     | indentation blocks and per-line error recovery             |
//! | `item`      | top-level dispatch and one-line directives                 |
//! | `journal`   | dated entries and plans                                    |
//! | `flow`      | flow headers, sides, legs, tails                           |
//! | `amount`, `select` | amounts and lot selectors                           |
//! | `decl`, `law` | declarations, params, syncs, laws                        |
//! | `expr`      | the expression grammar                                     |
//! | `errors`, `malformed` | diagnostics for what was found instead           |

pub mod ast;

mod amount;
mod block;
mod cursor;
mod decl;
mod errors;
mod expr;
mod flow;
mod item;
mod journal;
mod law;
mod lex;
mod lines;
mod malformed;
mod parser;
mod select;

#[cfg(test)]
mod tests;

pub use ast::*;

use axiom_core::{Diagnostic, FileId, Loc};

use crate::parser::Parser;

/// Parses one file. Every item that parses is kept; each one that does not
/// produces a diagnostic and is skipped. Diagnostics come in source order.
pub fn parse(file: FileId, src: &str) -> (File<'_>, Vec<Diagnostic>) {
    // Locations are `u32` offsets.
    if u32::try_from(src.len()).is_err() {
        let diag = Diagnostic::error("file-too-large", "source files are limited to 4 GiB")
            .label(Loc::new(file, 0, 0), "this file is larger");
        return (File { id: file, items: Vec::new(), exprs: Exprs::default() }, vec![diag]);
    }
    let mut parser = Parser::new(file, src);
    let items = parser.items();
    let Parser { exprs, mut diags, .. } = parser;
    diags.sort_by_key(|diag| diag.anchor().map(|loc| loc.start));
    (File { id: file, items, exprs }, diags)
}
