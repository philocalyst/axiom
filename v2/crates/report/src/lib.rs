//! Views over a run. A report is data (typed cells in sections); the command
//! line decides how to draw it.

use std::borrow::Cow;

use axiom_core::{Day, Diagnostic, Qty, Ratio};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Period};

/// What to show. Built by the command line from its arguments.
#[derive(Clone, Debug)]
pub enum Query<'a> {
    /// Balances per place and commodity, optionally at market value, optionally
    /// with a column per month.
    Balance { globs: Vec<&'a str>, at: Option<Day>, value: bool, monthly: bool },
    /// A place's flows with a running balance.
    Register { place: &'a str, from: Option<Day>, to: Option<Day> },
    /// Income and spending by period; spread flows recognized per day.
    Flow { by: Period, from: Option<Day>, to: Option<Day> },
    /// What can be spent now, and what drawing on each other place would net.
    Available { at: Option<Day> },
    /// Each budget (a `warn` law over a window total): spent against limit.
    Budget { month: Option<Day> },
    /// Tallies and obligations per system for a year and entity.
    Tax { year: Option<i32>, entity: Option<&'a str> },
    /// Parcels with basis and unrealized gain.
    Lots { place: Option<&'a str> },
    /// Plans, inferred recurrences, obligations and growth, run forward
    /// through the laws, with bands from bootstrapped spending.
    Forecast { until: Option<Day>, paths: u32 },
    /// Explains a place, `#code`, law, tax line, or `file:line`.
    Why { target: &'a str },
}

pub struct Report<'s> {
    pub title: String,
    pub sections: Vec<Section<'s>>,
}

pub struct Section<'s> {
    pub heading: Option<String>,
    pub columns: Vec<Column>,
    pub rows: Vec<Row<'s>>,
    pub notes: Vec<String>,
}

pub struct Column {
    pub title: Cow<'static, str>,
    pub align: Align,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
}

pub struct Row<'s> {
    /// Indentation for tree-shaped tables.
    pub depth: u8,
    pub style: Style,
    pub cells: Vec<Cell<'s>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Normal,
    /// A subtotal or total line.
    Total,
    /// Derived, estimated, or not yet real.
    Muted,
    /// Over a limit, overdrawn, or violated.
    Alert,
}

pub enum Cell<'s> {
    Blank,
    Text(Cow<'s, str>),
    /// Quanta, the commodity's decimal places, and its symbol.
    Amount { qty: Qty, scale: u8, unit: &'s str },
    Day(Day),
    Percent(Ratio),
}

/// Builds the view `query` asks for.
pub fn report<'s>(book: &Book<'s>, run: &Run, query: &Query) -> Result<Report<'s>, Diagnostic> {
    let _ = (book, run, query);
    todo!("lane F")
}

/// The one-line account of a healthy book that `axiom check` ends with.
pub struct Summary {
    pub flows: usize,
    pub places: usize,
    /// Laws that ran at least once past their filters.
    pub laws: usize,
    /// Assets and liabilities valued in the base currency at the run's day.
    pub net_worth: Amount,
    /// Holdings left out of `net_worth` for lack of a price.
    pub unpriced: usize,
}

pub fn summary(book: &Book, run: &Run) -> Summary {
    let _ = (book, run);
    todo!("lane F")
}
