//! Sync: how a book stays current without being typed (LANGUAGE §14).
//!
//! The model owns source declarations and their typed formats. This crate
//! binds those declarations to the engine's run, reads local files or runs
//! declared commands, and plans changes against an append-only source catalog.
//! Planning writes nothing: the caller decides whether to show a dry run or to
//! [`apply`] the changes it returned.
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
//! | `world`         | borrowed reconciliation state bound from the book and run      |
//! | `sink`          | Axiom output merged into the journal, a file or a param          |
//! | `write`         | the file a day belongs to, day order, short dates                |
//! | `apply`         | a plan's changes written into the project, confined to it        |
//! | `diff`          | what would be written, as a unified diff                         |
//! | `command`       | running commands, all at once                                    |
//! | `planner`       | model-native source selection, reads, reconciliation and changes |
//! | `unknown`       | memos nothing recognized, grouped, for `check`                   |
//!
//! # What binding to a book fills
//!
//! | the book says                                        | this crate takes                     |
//! |------------------------------------------------------|--------------------------------------|
//! | declared `sync` sources and formats                  | [`axiom_model::sync`]                |
//! | book and run flows                                   | borrowed reconciliation candidates   |
//! | local imports and command output                     | [`SourceRegistry`]                   |
//! | plan a selected source or all sources                | [`plan`]                             |
//! | pending changes and source diagnostics               | [`PlanOutcome`]                      |
//! | memos from a declared local `read` source             | [`read_memos`]                       |

mod amount;
mod apply;
mod binding;
mod cell;
mod command;
mod csv;
mod date;
mod diff;
mod format;
mod paths;
mod peg;
mod planner;
mod promise;
mod recognize;
mod reconcile;
mod sink;
mod tagged;
mod unknown;
mod world;
mod write;

use std::borrow::Cow;

use axiom_core::{Day, FileId, Loc, Qty};

pub use apply::apply;
pub use format::read_memos;
pub use paths::matching_paths;
pub use planner::{GeneratedSource, PlanOutcome, SourceFailure, SourceRegistry, SourceResult, plan};
pub use unknown::{Group, group as unrecognized};
pub use write::Change;

/// One line of a statement, in the terms of the account it is for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record<'t> {
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
pub(crate) struct Facts<'t> {
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
pub(crate) struct Original<'t> {
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
    #[cfg(test)]
    pub(crate) fn new(day: Day, qty: Qty, memo: impl Into<Cow<'t, str>>) -> Record<'t> {
        Record { day, qty, memo: memo.into(), balance: None, pending: false, at: Loc::default(), facts: None }
    }

    pub(crate) fn facts(&self) -> &Facts<'t> {
        self.facts.as_deref().unwrap_or(&NO_FACTS)
    }
}

/// The commodity an account is counted in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Unit<'a> {
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
pub(crate) struct Insert {
    pub path: String,
    pub day: Day,
    pub form: Form,
}

#[derive(Clone, Debug)]
pub(crate) enum Form {
    /// What follows the date of a journal line, and any lines under it.
    Item(String),
    /// A row of a param, as it is written under `param NAME`.
    Row { param: String, text: String },
}
