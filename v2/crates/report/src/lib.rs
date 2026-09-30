//! Views over a run. A report is data (typed cells in sections); the command
//! line decides how to draw it.
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
mod contracts;
mod flow;
mod forecast;
mod gains;
mod headroom;
mod history;
mod lens;
mod limits;
mod lots;
mod places;
mod promises;
mod register;
mod resolve;
mod synth;
mod table;
mod tax;
mod why;

#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod v4_tests;

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Qty, Ratio, Span};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Period, Place, Trigger};

pub use crate::flow::Group;
use crate::history::Snapshots;
use crate::lens::{Context, Lens, Whose};

/// What to show. Built by the command line from its arguments.
#[derive(Clone, Debug)]
pub enum Query<'a> {
    /// Balances per place and commodity, optionally at market value, optionally
    /// with a column per month.
    Balance { globs: Vec<&'a str>, at: Option<Day>, value: bool, monthly: bool },
    /// A place's flows with a running balance.
    Register { place: &'a str, from: Option<Day>, to: Option<Day> },
    /// Income and spending by period; spread flows recognized per day.
    Flow { by: Period, group: Group, from: Option<Day>, to: Option<Day> },
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
    /// Every promise: its terms today, what falls due next, how many were kept,
    /// what is late and whom it blames, and what a loan still owes.
    Contracts { at: Option<Day> },
    /// Tallies and obligations per system for a year.
    Tax { year: Option<i32> },
    /// Every disposal in a year: acquired, sold, proceeds, basis, gain, term.
    Gains { year: Option<i32> },
    /// Parcels with basis and unrealized gain, as of a day (default: today).
    Lots { place: Option<&'a str>, at: Option<Day> },
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

/// What a view says: a title, and sections of typed cells. Nothing here is
/// laid out or worded for a reader; the command line draws it as text or JSON,
/// and an editor or a GUI could draw it as anything else.
#[derive(Clone, Debug)]
pub struct Report<'s> {
    pub title: Cell<'s>,
    pub sections: Vec<Section<'s>>,
}

#[derive(Clone, Debug)]
pub struct Section<'s> {
    pub heading: Option<&'static str>,
    pub columns: Vec<Column<'s>>,
    pub rows: Vec<Row<'s>>,
    pub notes: Vec<Cell<'s>>,
    /// The figures the rows show, one by one, in the shape of an XBRL fact:
    /// what was measured, of whom, when, in what unit, and its value.
    pub facts: Vec<Fact<'s>>,
}

#[derive(Clone, Debug)]
pub struct Column<'s> {
    pub title: Cell<'s>,
    pub align: Align,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Debug)]
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

/// One thing a table, a title or a note says. A view builds these from the
/// book and the run and never formats a string: what is fixed wording is
/// `Word`, and everything else is typed, so a renderer can word, align, colour
/// or link each kind as it likes.
#[derive(Clone, Debug)]
pub enum Cell<'s> {
    Blank,
    /// Fixed wording: a label, or the words of a sentence.
    Word(&'static str),
    /// Words the book wrote: a description, a doc.
    Text(&'s str),
    /// Words someone else said: a diagnostic's message, or what the reader asked about.
    Said(String),
    /// A declared thing, by name: a place, an entity, a law.
    Name(&'s str),
    /// The code a fact goes by, without its sigil.
    Code(&'s str),
    Amount(Money<'s>),
    Day(Day),
    /// A calendar length: an age, a liquidity.
    Span(Span),
    /// A month, a year, one day, all time, or the days between two.
    Period(Days),
    Percent(Ratio),
    /// A plain number: a rate, a ratio.
    Number(Ratio),
    /// How many of a noun there are: `3 flows`.
    Count(usize, &'static str),
    /// When a law fires.
    Trigger(Trigger),
    /// Where in the sources a line comes from. The command line prints it as
    /// `file:line`, which `why` accepts, so every figure can be traced.
    Source(Loc),
    /// Parts of one thing, or of a sentence, put together with this between them.
    Join(&'static str, Vec<Cell<'s>>),
}

/// A quantity with its commodity: quanta, the commodity's decimal places, and
/// its symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Money<'s> {
    pub qty: Qty,
    pub scale: u8,
    pub unit: &'s str,
}

/// A measurement: `concept` (of something, when it is a kind of measure) of
/// `entity` over `when`, worth `value`.
#[derive(Clone, Copy, Debug)]
pub struct Fact<'s> {
    pub concept: &'s str,
    /// What it measures, when the concept is a kind of measure: `balance` of a place.
    pub of: Option<&'s str>,
    pub entity: &'s str,
    pub when: When,
    pub value: Money<'s>,
}

/// When a fact holds: at the end of a day, or over a period.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    Instant(Day),
    During(Days),
}

/// Builds the view `query` asks for, about the money of `whose` (`--for`: an
/// entity, a household including its members; default everything).
pub fn report<'s>(book: &Book<'s>, run: &Run, query: &Query, whose: Option<&str>) -> Result<Report<'s>, Diagnostic> {
    let (cx, whose) = (Context::new(book, run), Whose::resolve(book, whose)?);
    views(Lens::new(&cx, &whose, run.today), query)
}

/// The view `query` asks for, through a lens on the run's day.
fn views<'s>(lens: Lens<'_, 's>, query: &Query) -> Result<Report<'s>, Diagnostic> {
    match query {
        Query::Balance { globs, at, value, monthly } => balance::view(lens, globs, *at, *value, *monthly),
        Query::Register { place, from, to } => register::view(lens, place, *from, *to),
        Query::Flow { by, group, from, to } => Ok(flow::view(lens, *by, *group, *from, *to)),
        Query::Available { at } => Ok(available::view(lens, *at)),
        Query::Budget { at, by } => Ok(budget::view(lens, *at, *by)),
        Query::Limits { year } => Ok(limits::view(lens, *year)),
        Query::Claims { at } => Ok(claims::view(lens, *at)),
        Query::Contracts { at } => Ok(contracts::view(lens, *at)),
        Query::Tax { year } => Ok(tax::view(lens, *year)),
        Query::Gains { year } => Ok(gains::view(lens, *year)),
        Query::Lots { place, at } => lots::view(lens, *place, *at),
        Query::Forecast { until, paths } => Ok(forecast::view(lens, *until, *paths)),
        Query::Why { target } => why::target(lens, target),
        Query::Line { loc } => Ok(why::line(lens, *loc)),
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
    let (cx, everyone) = (Context::new(book, run), Whose::default());
    let lens = Lens::new(&cx, &everyone, run.today);
    let worth = balance::NetWorth::of(lens, &Snapshots::of(lens, &[run.today], false), 0);
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
        places: book.places.ids().filter(|&place| !cx.sides.is_root(place) && used(place)).count(),
        laws: run.checks.iter().filter(|&&ran| ran > 0).count(),
        net_worth: Amount::new(worth.total(), book.base),
        unpriced: worth.unpriced,
    }
}
