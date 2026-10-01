//! Views over a run. A report is data (typed cells in sections); each client
//! supplies a renderer and source catalog, whether it is a terminal, editor, or
//! GUI.
//!
//! Every view is a pure function of the book and its run. What several views
//! need lives in one place: [`history`] answers "what happened, as posted",
//! [`lens`] says whose it is, what it is worth and how liquid it is,
//! [`headroom`] what every limit has counted, [`calendar`] cuts time into
//! periods, and [`table`] builds sections so the views stay declarative.

mod available;
mod balance;
mod budget;
mod calendar;
mod claims;
mod closings;
mod context;
mod flow;
mod forecast;
mod gains;
mod headroom;
mod history;
mod lens;
mod limits;
mod lots;
mod places;
mod register;
mod resolve;
mod synth;
mod table;
mod tax;
mod why;
pub mod json;

#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

use axiom_core::{Day, Diagnostic, Id, Loc, Qty, Ratio};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Period, Place};

use crate::history::Snapshots;
use crate::lens::{Lens, Whose};

/// What to show. A client builds this from its own input surface.
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
    /// Each budget (a `warn` law over a window total): spent against limit,
    /// for the month or the year containing `at` (default: today).
    Budget { at: Option<Day>, by: Period },
    /// Every cap and budget a person lives under, from the run's headroom:
    /// counted, limit, room left, share used.
    Limits { year: Option<i32> },
    /// What others owe and what is owed to them: open claims with their
    /// counterparty, age and due day.
    Claims { at: Option<Day> },
    /// Tallies and obligations per system for a year.
    Tax { year: Option<i32> },
    /// Every disposal in a year: acquired, sold, proceeds, basis, gain, term.
    Gains { year: Option<i32> },
    /// Parcels with basis and unrealized gain, as of a day (default: today).
    Lots { place: Option<&'a str>, at: Option<Day> },
    /// Plans, inferred recurrences, obligations and growth, run forward
    /// through the laws, with bands from bootstrapped spending.
    Forecast { until: Option<Day>, paths: u32 },
    /// Explains a place, `#code`, law, tax line, or a source path and line.
    Why { target: &'a str },
    /// Explains what is written at one resolved source location and everything
    /// it caused. A client uses its [`SourceProvider`] to resolve `why FILE:LINE`.
    Line { loc: Loc },
}

/// A report view with its title and ordered sections.
pub struct Report<'s> {
    pub title: String,
    pub sections: Vec<Section<'s>>,
}

/// One headed table or note group in a report.
pub struct Section<'s> {
    pub heading: Option<String>,
    pub columns: Vec<Column>,
    pub rows: Vec<Row<'s>>,
    pub notes: Vec<String>,
}

/// A heading and alignment for one table column.
pub struct Column {
    pub title: Cow<'static, str>,
    pub align: Align,
}

/// Horizontal alignment for a table column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
}

/// One row of typed cells in a section.
pub struct Row<'s> {
    /// Indentation for tree-shaped tables.
    pub depth: u8,
    pub style: Style,
    pub cells: Vec<Cell<'s>>,
}

/// How a row should be emphasized by a renderer.
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

/// A report value before a client chooses how to draw it.
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
    /// Where in the sources a line comes from, so any client can offer a trace.
    Source(Loc),
}

pub use context::Context;
pub use table::percent;

/// A source position as a client can display it. Lines and columns are
/// one-based; columns count Unicode scalar values (a tab is one column). The
/// path is borrowed from the client that owns the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition<'a> {
    pub path: &'a str,
    pub line: usize,
    pub column: usize,
}

/// Resolves source positions without making the report crate own file storage.
/// A terminal, editor, or GUI can provide its own source catalog.
pub trait SourceProvider {
    /// The location covered by `line` of `path`, where lines are one-based.
    fn locate(&self, path: &str, line: usize) -> Option<Loc>;

    /// A borrowed display position for a source range.
    fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>>;
}

/// A client renderer for a report. Renderers receive complete report data and
/// borrowed source positions, with no dependency on a terminal or filesystem.
pub trait ReportRenderer {
    type Output;

    fn render<'s>(&self, report: &Report<'s>, sources: &dyn SourceProvider) -> Self::Output;
}

/// Builds the view `query` asks for, about the money of `whose` (`--for`: an
/// entity, a household including its members; default everything).
pub fn report<'s>(book: &Book<'s>, run: &Run, query: &Query, whose: Option<&str>) -> Result<Report<'s>, Diagnostic> {
    views(book, run, &Whose::resolve(book, whose)?, query)
}

/// Builds a report with source-aware query resolution while borrowing source
/// names and texts from the client's provider.
pub fn report_with_sources<'s>(
    book: &Book<'s>,
    run: &Run,
    query: &Query,
    whose: Option<&str>,
    sources: &dyn SourceProvider,
) -> Result<Report<'s>, Diagnostic> {
    if let Some(loc) = resolve_source_line(query, sources) {
        let query = Query::Line { loc };
        report(book, run, &query, whose)
    } else {
        report(book, run, query, whose)
    }
}

/// Turns a source target into a line query when the provider recognizes it.
/// Unknown paths remain ordinary `why` targets so existing name diagnostics
/// keep their useful suggestions.
pub fn resolve_source_line(query: &Query<'_>, sources: &dyn SourceProvider) -> Option<Loc> {
    let Query::Why { target } = query else { return None };
    let Some((path, number)) = target.rsplit_once(':') else { return None };
    let Ok(line) = number.parse::<usize>() else { return None };
    if path.is_empty() || line == 0 {
        return None;
    }
    sources.locate(path, line)
}

/// The view `query` asks for, about the money of `whose`.
fn views<'s>(book: &Book<'s>, run: &Run, whose: &Whose, query: &Query) -> Result<Report<'s>, Diagnostic> {
    match query {
        Query::Balance { globs, at, value, monthly } => balance::view(book, run, whose, globs, *at, *value, *monthly),
        Query::Register { place, from, to } => register::view(book, run, whose, place, *from, *to),
        Query::Flow { by, from, to } => Ok(flow::view(book, run, whose, *by, *from, *to)),
        Query::Available { at } => Ok(available::view(book, run, whose, *at)),
        Query::Budget { at, by } => Ok(budget::view(book, run, whose, *at, *by)),
        Query::Limits { year } => Ok(limits::view(book, run, whose, *year)),
        Query::Claims { at } => Ok(claims::view(book, run, whose, *at)),
        Query::Tax { year } => Ok(tax::view(book, run, whose, *year)),
        Query::Gains { year } => Ok(gains::view(book, run, whose, *year)),
        Query::Lots { place, at } => lots::view(book, run, whose, *place, *at),
        Query::Forecast { until, paths } => Ok(forecast::view(book, run, whose, *until, *paths)),
        Query::Why { target } => why::target(book, run, whose, target),
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
    let (everyone, today) = (Whose::default(), run.today);
    let lens = Lens::new(book, &everyone, today);
    let worth = balance::NetWorth::of(lens, &Snapshots::of(lens, run, &[today], false), 0);
    // Class roots (`assets`, `expenses`, …) group places, and the built-in
    // places exist in every book: only what was declared or used counts.
    let used = |place: Id<Place>| {
        let held = run.holdings.partition_point(|holding| holding.place < place);
        book.places[place].loc.is_some()
            || !book.touching[place].is_empty()
            || run.holdings.get(held).is_some_and(|holding| holding.place == place)
    };
    Summary {
        flows: book.flows.len(),
        places: book.places.ids().filter(|&place| !places::is_class_root(book, place) && used(place)).count(),
        laws: run.checks.iter().filter(|&&ran| ran > 0).count(),
        net_worth: Amount::new(worth.total(), book.base),
        unpriced: worth.unpriced,
    }
}
