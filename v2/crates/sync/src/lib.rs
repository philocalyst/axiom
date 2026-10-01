//! Sync: how a book stays current without being typed (LANGUAGE §14).
//!
//! Axiom never touches the network. A source is a folder of files it reads, or
//! a command whose output it reads; what it reads is recognized, reconciled
//! with what the book already says, and written back as the lines a person
//! would have typed. Everything here works on this crate's own small types, so
//! it is testable without a book; the `axiom` binary binds them to one.
//!
//! | module          | job                                                              |
//! |-----------------|------------------------------------------------------------------|
//! | `format`        | declared columns and tags to records, or a diagnostic at the cell |
//! | `csv`, `tagged` | rows of cells; OFX and XML records of cells                      |
//! | `date`, `amount`| dates and amounts as banks write them                            |
//! | `peg`           | the patterns the ledger writes: a small PEG                      |
//! | `recognize`     | `known-as` patterns and names, compiled once; `via`; codes       |
//! | `reconcile`     | records already written: same amount, within three days          |
//! | `promise`       | records that keep a contract's occurrence                        |
//! | `world`         | one feed: recognize, reconcile, keep, write                      |
//! | `sink`          | Axiom output merged into the journal, a file or a param          |
//! | `write`         | the file a day belongs to, day order, short dates                |
//! | `diff`          | what would be written, as a unified diff                         |
//! | `command`       | running commands, all at once                                    |
//! | `session`       | all of it, source by source, documents before bank lines         |
//! | `unknown`       | memos nothing recognized, grouped, for `check`                   |
//!
//! # What binding to a book fills
//!
//! | the book says                                        | this crate takes                     |
//! |------------------------------------------------------|--------------------------------------|
//! | entities, accounts and their `known-as` (never `me`) | [`Known`], `account` for a place     |
//! | `pattern NAME = …`                                   | [`Patterns`]                         |
//! | `code NAME` with `known-as`                          | the `codes` of [`Recognizer::new`]   |
//! | flows on an account, each leg of a split, derived flows | one [`Existing`] each, in its unit |
//! | a batch of flows sharing a code                      | its members and its total, [`Batch`] |
//! | occurrences due and unwritten (`Contract::due_days`) | [`Due`]                              |
//! | open claims that carry a code                        | `World::claims`: code to party       |
//! | the units the book has                               | `World::units`                       |
//! | `sync NAME`, its `read` or `run`, its `format`, `into` | [`Source`], [`Feed`], [`Format`], [`Sink`] |

mod amount;
mod binding;
mod cell;
mod command;
mod csv;
mod date;
mod diff;
mod format;
mod peg;
mod paths;
mod planner;
mod promise;
mod recognize;
mod reconcile;
mod session;
mod sink;
mod tagged;
mod unknown;
mod world;
mod write;

use std::borrow::Cow;

use axiom_core::{Day, FileId, Loc, Qty};

pub use axiom_model::sync::{Column, Field, Format, Rule, Shape, Spec};
pub use command::{Failed, substitute};
pub use format::read_memos;
pub use peg::Patterns;
pub use planner::{GeneratedSource, PlanOutcome, SourceFailure, SourceResult, plan};
pub use promise::Due;
pub use paths::matching_paths;
pub use recognize::{KnownId, Reading, Recognized, Recognizer, Scratch, Tie, Who};
pub use reconcile::{Batch, Existing, WINDOW};
pub use session::{Env, Failure, Input, Kind, Outcome, Source, sync};
pub use sink::Sink;
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
    /// What else the export says of it, if it says anything.
    pub facts: Option<Box<Facts<'t>>>,
}

/// What a format may say of a record besides its day, amount and memo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts<'t> {
    /// A code, as the language writes one (`check-1041`, lowercase).
    pub code: Option<Cow<'t, str>>,
    /// Records that share one are one flow.
    pub id: Option<Cow<'t, str>>,
    pub party: Option<Cow<'t, str>>,
    pub via: Option<Cow<'t, str>>,
    /// The unit the amount is in, when it is not the account's, uppercase.
    pub currency: Option<Cow<'t, str>>,
    /// The other side of a bank's FX amount, captured as amount and currency.
    /// It is a reconciliation candidate only; it never creates another flow.
    pub original: Option<Original<'t>>,
    pub category: Option<Cow<'t, str>>,
    pub object: Option<Cow<'t, str>>,
    /// The account it belongs to, as the export names it.
    pub route: Option<Cow<'t, str>>,
    /// What a payout was before its fee, and the fee; both positive.
    pub gross: Option<Qty>,
    pub fee: Option<Qty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original<'t> {
    pub qty: Qty,
    pub unit: Cow<'t, str>,
}

static NO_FACTS: Facts<'static> = Facts {
    code: None,
    id: None,
    party: None,
    via: None,
    currency: None,
    original: None,
    category: None,
    object: None,
    route: None,
    gross: None,
    fee: None,
};

impl<'t> Record<'t> {
    /// A record that says nothing but its day, amount and memo.
    pub fn new(day: Day, qty: Qty, memo: impl Into<Cow<'t, str>>) -> Record<'t> {
        Record {
            day,
            qty,
            memo: memo.into(),
            balance: None,
            pending: false,
            at: Loc::default(),
            facts: None,
        }
    }

    pub fn facts(&self) -> &Facts<'t> {
        self.facts.as_deref().unwrap_or(&NO_FACTS)
    }
}

/// The commodity an account is counted in.
#[derive(Clone, Copy, Debug)]
pub struct Unit<'a> {
    pub name: &'a str,
    /// Decimal places.
    pub scale: u8,
}

/// A range of the text a source gave, for a label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
