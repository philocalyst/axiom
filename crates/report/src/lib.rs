//! Views over a run. A report is data (typed cells in sections); each client
//! supplies a renderer and source catalog, whether it is a terminal, editor, or
//! GUI.
//!
//! Every view is a pure function of the book and its run. What several views
//! need lives in one place: [`history`] answers "what happened, as posted",
//! [`view`] is the context of one view (a day, whose money, the plan and the run) and says what it is worth and how
//! liquid it is,
//! [`headroom`] what every limit has counted, [`calendar`] cuts time into
//! periods, and [`table`] builds sections so the views stay declarative.

mod available;
mod balance;
mod balances;
mod budget;
mod calendar;
mod claims;
mod closings;
mod context;
mod contracts;
mod flow;
mod forecast;
mod gains;
mod headroom;
mod history;
pub mod json;
mod limits;
mod lots;
mod pivot;
mod places;
mod register;
mod resolve;
mod synth;
mod table;
mod tax;
mod view;
mod why;

#[cfg(test)]
mod offspring_tests;
#[cfg(test)]
mod recognition_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

use axiom_core::{Day, Days, Id, Loc, Qty, Ratio, Span};
use axiom_engine::{Plan, Run};
use axiom_model::{Amount, Book, Period, Place, Trigger};

use crate::balances::Balances;
use crate::view::{View, Whose};

/// What to show. A client builds this from its own input surface.
#[derive(Clone, Debug)]
pub enum Query<'a> {
    /// Balances per place and commodity, optionally at market value, optionally
    /// with a column per month.
    Balance { globs: Vec<&'a str>, at: Option<Day>, value: bool, monthly: bool },
    /// A place, `entity:NAME`, asset or contract's register.
    Register { place: &'a str, from: Option<Day>, to: Option<Day> },
    /// Income and spending by period; spread flows recognized per day.
    Flow { by: FlowBy, from: Option<Day>, to: Option<Day> },
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
    /// Current promises, terms and the next due day.
    Contracts,
    /// Tallies and obligations per system for a year.
    Tax { year: Option<i32> },
    /// Every disposal in a year: acquired, sold, proceeds, basis, gain, term.
    Gains { year: Option<i32> },
    /// Parcels with basis and unrealized gain, as of a day (default: today).
    Lots { place: Option<&'a str>, at: Option<Day> },
    /// Plans, inferred recurrences, obligations and growth, run forward
    /// through the laws, with bands from bootstrapped spending.
    Forecast { until: Option<Day>, paths: u32 },
    /// Explains a place, `entity:NAME`, `^code`, `#purpose`, asset, contract, law or source line.
    Why { target: &'a str },
    /// Explains what is written at one resolved source location and everything
    /// it caused. A client uses its [`SourceProvider`] to resolve `why FILE:LINE`.
    Line { loc: Loc },
}

/// How flows are grouped in a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowBy {
    Period(Period),
    Party,
}

/// A report view with its title and ordered sections.
///
/// The lifetime is the borrow of the model book used to build the view. Cells
/// can borrow decoded model text from that book without cloning it per row.
pub struct Report<'s> {
    pub title: Cell<'s>,
    pub sections: Vec<Section<'s>>,
}

/// One headed table or note group in a report.
pub struct Section<'s> {
    pub heading: Option<Cell<'s>>,
    pub columns: Vec<Column<'s>>,
    pub rows: Vec<Row<'s>>,
    pub notes: Vec<Cell<'s>>,
    /// Machine-readable measures shown by this section.
    pub facts: Vec<Fact<'s>>,
}

/// A heading and alignment for one table column.
pub struct Column<'s> {
    pub title: Cell<'s>,
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
#[derive(Clone, Debug)]
pub enum Cell<'s> {
    Blank,
    /// Fixed wording supplied by a view.
    Word(&'static str),
    /// Prose borrowed from the book or owned by a view-specific sentence.
    Text(Cow<'s, str>),
    /// A declared place, entity, law, asset, or contract name.
    Name(&'s str),
    /// A code without its written sigil.
    Code(&'s str),
    /// A purpose without its written sigil.
    Purpose(&'s str),
    /// Externally supplied or diagnostic wording.
    Said(Cow<'s, str>),
    /// Quanta, the commodity's decimal places, and its symbol.
    Amount {
        qty: Qty,
        scale: u8,
        unit: &'s str,
    },
    Day(Day),
    Span(Span),
    Period(Days),
    Percent(Ratio),
    Number(Ratio),
    Count(usize, &'static str),
    Trigger(Trigger),
    /// Where in the sources a line comes from, so any client can offer a trace.
    Source(Loc),
    /// A structured sentence or list whose pieces remain individually typed.
    Join(&'static str, Vec<Cell<'s>>),
}

/// A quantity with its commodity, recorded without display punctuation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Money<'s> {
    pub qty: Qty,
    pub scale: u8,
    pub unit: &'s str,
}

/// When a fact holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    Instant(Day),
    During(Days),
}

/// A measure from a report, in a stable concept/entity/period/unit/value shape.
#[derive(Clone, Copy, Debug)]
pub struct Fact<'s> {
    pub concept: &'s str,
    pub of: Option<&'s str>,
    pub entity: &'s str,
    pub when: When,
    pub value: Money<'s>,
}

pub use context::{Context, Folded};
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

/// Turns a source target into a line query when the provider recognizes it.
/// Unknown paths remain ordinary `why` targets so existing name diagnostics
/// keep their useful suggestions.
pub fn resolve_source_line(query: &Query<'_>, sources: &dyn SourceProvider) -> Option<Loc> {
    let Query::Why { target } = query else {
        return None;
    };
    let Some((path, number)) = target.rsplit_once(':') else {
        return None;
    };
    let Ok(line) = number.parse::<usize>() else {
        return None;
    };
    if path.is_empty() || line == 0 {
        return None;
    }
    sources.locate(path, line)
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
    summary_of(&Plan::new(book), run)
}

/// [`summary`], with the plan the run was folded with: a client that has one does not build another.
pub fn summary_of(plan: &Plan<'_, '_>, run: &Run) -> Summary {
    let (everyone, today, book) = (Whose::default(), run.today, plan.book());
    let view = View::new(plan, &everyone, run, today);
    let worth = balance::NetWorth::of(view, &Balances::of(view, &[today]), 0);
    // Built-in place rows exist in every book: only declared or used places
    // contribute to the summary.
    let used = |place: Id<Place>| {
        let held = run.holdings.partition_point(|holding| holding.place < place);
        book.places[place].loc.is_some()
            || !book.touching[place].is_empty()
            || run.holdings.get(held).is_some_and(|holding| holding.place == place)
    };
    Summary {
        flows: book.flows.len(),
        places: book.places.ids().filter(|&place| used(place)).count(),
        laws: run.checks.iter().filter(|&&ran| ran > 0).count(),
        net_worth: Amount::new(worth.total(), book.base),
        unpriced: worth.unpriced,
    }
}
