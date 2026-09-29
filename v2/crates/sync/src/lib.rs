//! Sync: how a book stays current without being typed (LANGUAGE §13).
//!
//! Axiom never touches the network. A source is a folder of files it reads, or
//! a command whose output it reads; what it reads is recognized, reconciled
//! with what the book already says, and written back as the lines a person
//! would have typed. Everything here works on this crate's own small types, so
//! it is testable without a book; the `axiom` binary binds them to one.
//!
//! | module      | job                                                        |
//! |-------------|------------------------------------------------------------|
//! | `csv`, `ofx`| a bank's export to records, or to a diagnostic at the cell |
//! | `amount`    | amounts as banks write them                                |
//! | `peg`       | the patterns the ledger writes: a small PEG                |
//! | `recognize` | `known-as` patterns, compiled once; `via`; codes           |
//! | `reconcile` | records already written: same amount, within three days    |
//! | `promise`   | records that keep a contract's occurrence                  |
//! | `world`     | one feed: recognize, reconcile, keep, write                |
//! | `sink`      | Axiom output merged into the journal, a file or a param    |
//! | `write`     | the file a day belongs to, day order, short dates          |
//! | `diff`      | what would be written, as a unified diff                   |
//! | `command`   | running commands, all at once                              |
//! | `session`   | all of it, source by source                                |
//! | `unknown`   | memos nothing recognized, grouped, for `check`             |

mod amount;
mod command;
mod csv;
mod diff;
mod ofx;
mod peg;
mod promise;
mod recognize;
mod reconcile;
mod session;
mod sink;
mod statement;
mod unknown;
mod world;
mod write;

use std::borrow::Cow;

use axiom_core::{Day, FileId, Loc, Qty};

pub use command::{Failed, substitute};
pub use csv::{Amounts, Column, Csv, DateFormat};
pub use peg::{Pattern, PatternError, Patterns};
pub use promise::Due;
pub use recognize::{BadPattern, Known, Recognizer, Scratch};
pub use reconcile::{Existing, WINDOW};
pub use session::{Env, Failure, Input, Kind, Outcome, Source, sync};
pub use sink::Sink;
pub use statement::{Format, Statement};
pub use unknown::{Group, group as unrecognized};
pub use world::{Account, Feed, World, money};
pub use write::{Change, Layout};

/// One line of a statement, in the terms of the account it is for.
#[derive(Clone, Debug, PartialEq)]
pub struct Record<'t> {
    pub day: Day,
    /// Money into the account is positive.
    pub qty: Qty,
    pub memo: Cow<'t, str>,
    /// What the account held after it, as the statement shows it, if the
    /// export says.
    pub balance: Option<Qty>,
    /// Not yet posted: it may still change, or vanish.
    pub pending: bool,
    /// Where the memo is in the export.
    pub at: Loc,
}

/// The commodity an account is counted in.
#[derive(Clone, Copy, Debug)]
pub struct Unit<'a> {
    pub name: &'a str,
    /// Decimal places.
    pub scale: u8,
}

/// A range of the text a source gave, for a label.
#[derive(Clone, Copy)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// The range in `file`, at least a byte wide so that it can be drawn.
    pub fn loc(self, file: FileId) -> Loc {
        Loc::new(file, self.start as u32, self.end.max(self.start + 1) as u32)
    }
}

/// A line to add to the journal, or a row to a param: where, and on which day.
#[derive(Clone, Debug)]
pub struct Insert {
    pub path: String,
    pub day: Day,
    pub form: Form,
}

#[derive(Clone, Debug)]
pub enum Form {
    /// What follows the date of a journal line, and any lines under it.
    Item(String),
    /// A row of a param, as it is written under `param NAME`.
    Row { param: String, text: String },
}
