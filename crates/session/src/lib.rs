//! A loaded project as a value: the texts it was read from, the book built from them, and (to come) the run of its
//! fold and the edits that change them. What an MCP server and a GUI are written against; the command line is its
//! first client.
//!
//! | module    | job                                                                                  |
//! |-----------|--------------------------------------------------------------------------------------|
//! | `texts`   | the owner of every text, which grows through a shared reference and never moves one |
//! | `sources` | the files by `FileId`, as a table of borrowed texts; what a `Loc` points into        |
//!
//! # Why the owner is separate from the session
//!
//! A `Book<'s>` borrows its names from the text it was built from, and a `Plan<'b, 's>` borrows the whole `Book`, so
//! no value can own a text, a book built from it and a plan over that book. The text therefore lives in a [`Texts`]
//! the client owns, and everything built from it borrows that.

#![forbid(unsafe_code)]
// `Diagnostic` is 128 bytes and every crate of the workspace returns it by value in an `Err`; a smaller or interned one
// (STATUS, "What K0b found") would remove the lint, and this is the one place this crate says it knows.
#![allow(clippy::result_large_err)]

mod sources;
mod texts;

pub use sources::{SourceFile, Sources};
pub use texts::Texts;
