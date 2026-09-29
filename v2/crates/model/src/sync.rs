//! What a book says about sync (LANGUAGE §14): where facts from outside come
//! from, how their records read, and the patterns that recognize a memo.
//!
//! These live in their own module, not among the book's things, because their
//! names are the law compiler's too: [`Op`] and [`Field`] here are not
//! [`crate::law::Op`] and [`crate::law::Field`], and [`Source`] is not the
//! parsed [`crate::Source`] that `build` takes. Reach them as `sync::Source`.

use axiom_core::{Id, Loc, Sym};

use crate::book::{Param, Place, Purpose, System};

/// `sync NAME`: a place facts come from, and where what it recognizes goes.
#[derive(Clone, Debug)]
pub struct Source {
    pub name: Sym,
    pub fetch: Fetch,
    /// How its records read, if it reads records.
    pub format: Option<Id<Format>>,
    pub sink: Sink,
    /// The system that declared it, if one did.
    pub system: Option<Id<System>>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// How a source gets its text. (Not `Origin`, which is a flow's.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fetch {
    /// `read "imports/chase-*.csv"`.
    Read(Sym),
    /// `run COMMAND`, with `{since}`, `{today}`, `{units}` and `{year}` unexpanded.
    Run(Sym),
}

/// Where a source's facts go.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Sink {
    /// A sync named after an account: its records, reconciled into the
    /// journal. `csv: None` means the source prints Axiom.
    Feed { account: Id<Place>, csv: Option<Csv> },
    /// `into PATH`: Axiom text, merged into that file (`{year}` splits it).
    File(Sym),
    /// `into param NAME`: rows merged into that param.
    Param(Id<Param>),
    /// Neither: Axiom statements (invoices, bills) into the journal.
    Journal,
}

/// `format NAME`, or a `format csv` block inline in a sync.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Format {
    pub name: Sym,
    pub shape: Shape,
    pub loc: Loc,
}

/// What a record looks like.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Shape {
    Csv(Csv),
    /// OFX, ISO 20022: named records, fields by path (`BookgDt/Dt`).
    Tagged {
        records: Sym,
        fields: Box<[(Field, Box<[Sym]>)]>,
    },
}

/// How the columns of an export read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Csv {
    /// Which column carries what: `date "Posting Date"`, `amount 4`.
    pub columns: Box<[(Field, Column)]>,
    /// `"MM/DD/YYYY"`; ISO when absent.
    pub date_format: Option<Sym>,
    /// `amount 4 flipped`: money into the account is negative in the export.
    pub flipped: bool,
    /// `category "Groceries" is #groceries`.
    pub categories: Box<[(Sym, Id<Purpose>)]>,
}

/// A column of an export.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Header(Sym),
    /// 1-based, as written.
    Index(u16),
}

/// What a column or a tagged field is of a record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Date,
    Amount,
    Debit,
    Credit,
    Memo,
    Balance,
    Pending,
    Code,
    Id,
    Party,
    Gross,
    Fee,
    Currency,
    Category,
    Object,
    Route,
    Via,
}

/// A compiled parsing expression (LANGUAGE §14): matched anywhere in a memo, in
/// any case. `known_as` on an entity, account or code rule holds these.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pattern {
    pub name: Option<Sym>,
    /// A program for a small matching machine: literals, classes, sequence,
    /// ordered choice, repetition, captures. Built from the syntax once.
    pub program: Box<[Op]>,
    pub loc: Loc,
}

/// One step of a pattern's program.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// Case-insensitive literal (stored uppercased).
    Literal(Sym),
    Class(CharClass),
    /// Try the next `len` ops; on failure jump past them to the alternative.
    Choice {
        len: u16,
    },
    Repeat {
        min: u8,
        max: Option<u8>,
        len: u16,
    },
    Capture {
        name: Capture,
        len: u16,
    },
    Call(Id<Pattern>),
}

/// `digit letter space alnum any rest start end`. (Not `Class`, which is a
/// place's.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharClass {
    Digit,
    Letter,
    Space,
    Alnum,
    Any,
    Rest,
    Start,
    End,
}

/// What a capture fills of a record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capture {
    Payee,
    Code,
    Amount,
    Date,
    Named(Sym),
}
