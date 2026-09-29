//! Views over a run. A report is data (typed cells in sections); the command
//! line decides how to draw it.
//!
//! Every view is a pure function of the book and its run. What several views
//! need lives in one place: [`history`] answers "what happened, as posted",
//! [`value`] prices it in the base currency, [`calendar`] cuts time into
//! periods, and [`table`] builds sections so the views stay declarative.

mod apportion;
mod available;
mod balance;
mod budget;
mod calendar;
mod flow;
mod forecast;
mod history;
mod lots;
mod places;
mod register;
mod resolve;
mod synth;
mod table;
mod tax;
mod value;
mod why;

#[cfg(test)]
mod tests;

use std::borrow::Cow;

use axiom_core::{Day, Diagnostic, Loc, Qty, Ratio};
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
    /// Explains a place, `#code`, law, or tax line.
    Why { target: &'a str },
    /// Explains what is written on one source line and everything it caused.
    /// The command line turns `file:line` into the line's byte range, because
    /// only it holds the source text.
    Line { loc: Loc },
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
    Amount {
        qty: Qty,
        scale: u8,
        unit: &'s str,
    },
    Day(Day),
    Percent(Ratio),
    /// Where in the sources a line comes from. The command line prints it as
    /// `file:line`, which `why` accepts, so every figure can be traced.
    Source(Loc),
}

/// Builds the view `query` asks for.
pub fn report<'s>(book: &Book<'s>, run: &Run, query: &Query) -> Result<Report<'s>, Diagnostic> {
    match query {
        Query::Balance { globs, at, value, monthly } => balance::view(book, run, globs, *at, *value, *monthly),
        Query::Register { place, from, to } => register::view(book, run, place, *from, *to),
        Query::Flow { by, from, to } => Ok(flow::view(book, run, *by, *from, *to)),
        Query::Available { at } => Ok(available::view(book, run, *at)),
        Query::Budget { month } => Ok(budget::view(book, run, *month)),
        Query::Tax { year, entity } => tax::view(book, run, *year, *entity),
        Query::Lots { place } => lots::view(book, run, *place),
        Query::Forecast { until, paths } => Ok(forecast::view(book, run, *until, *paths)),
        Query::Why { target } => why::target(book, run, target),
        Query::Line { loc } => Ok(why::line(book, run, *loc)),
    }
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
    let balances = history::Balances::at(book, run, run.today);
    let worth = balance::NetWorth::of(book, &balances, run.today);
    Summary {
        flows: book.flows.len(),
        // Class roots (`assets`, `expenses`, …) group places; nobody declared them.
        places: book.places.ids().filter(|&place| !places::is_class_root(book, place)).count(),
        laws: run.checks.iter().filter(|&&ran| ran > 0).count(),
        net_worth: Amount::new(worth.total(), book.base),
        unpriced: worth.unpriced,
    }
}
