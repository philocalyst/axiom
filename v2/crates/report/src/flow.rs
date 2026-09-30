//! `flow`: the income statement, by month or by year.
//!
//! Income and expense places are read as the change in their balances over
//! each period, priced in the base currency on the day each flow happened.
//! A flow spread over a range is recognized a little each day, so a year's
//! premium lands in every month it covers.

use std::iter;

use axiom_core::{Day, Days, Id, Qty, Tree, spread};
use axiom_model::{Book, Period, Place};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::{Lens, Priced};
use crate::places::{Side, depth, leaf, path};
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// How many periods to show when the window is not given.
const DEFAULT_PERIODS: usize = 12;

mod purposes;

/// What the statement's rows are: the purpose tree, or the parties the money went to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Purpose,
    Party,
}

pub fn view<'s>(lens: Lens<'_, 's>, by: Period, group: Group, from: Option<Day>, to: Option<Day>) -> Report<'s> {
    let book = lens.book;
    let to = to.unwrap_or(lens.day);
    let lens = lens.on(to);
    let periods = match from {
        Some(from) => Periods::covering(by, from, to),
        None => {
            let first = book.flows.as_slice().first().map_or(to, |flow| flow.day.min(to));
            Periods::covering(by, first, to).last(DEFAULT_PERIODS)
        }
    };
    // v3 bridge: a v4 book has purposes, and no income or expense places.
    if lens.sides.is_v4() {
        return purposes::report(lens, periods, group, to);
    }
    let statement = Statement::compile(lens, periods, to);
    Report::new("Income and spending").with(statement.section(lens))
}

/// A quantity per place per period, stored flat so that a subtree's figures
/// are one contiguous slice.
struct Grid {
    columns: usize,
    cells: Vec<Qty>,
}

impl Grid {
    fn new(rows: usize, columns: usize) -> Grid {
        Grid { columns, cells: vec![Qty::ZERO; rows * columns] }
    }

    fn add<T>(&mut self, row: Id<T>, column: usize, qty: Qty) {
        self.cells[row.index() * self.columns + column] += qty;
    }

    /// Per-period totals for `row` and everything beneath it.
    fn subtree<T>(&self, tree: &Tree<T>, row: Id<T>) -> Vec<Qty> {
        let rows = row.index() * self.columns..tree.end(row).index() * self.columns;
        let mut totals = vec![Qty::ZERO; self.columns];
        for row in self.cells[rows].chunks(self.columns) {
            add_into(&mut totals, row);
        }
        totals
    }
}

fn add_into(totals: &mut [Qty], values: &[Qty]) {
    for (total, &value) in totals.iter_mut().zip(values) {
        *total += value;
    }
}

fn is_zero(values: &[Qty]) -> bool {
    values.iter().all(|value| value.is_zero())
}

/// Income and spending per period, ready to lay out.
struct Statement {
    periods: Periods,
    /// Recognition stops here: nothing after the window's end has happened.
    cutoff: Day,
    /// Positive for both income and spending; see [`statement_sign`].
    grid: Grid,
    /// Realized gains per period, derived from the run's parcels.
    gains: Vec<Qty>,
    spread_seen: bool,
    /// The flows with no price on their day, which the statement leaves out.
    unpriced: Priced,
}

/// How a place's balance change reads in the statement: income and spending
/// both come out positive. Value that fell into `?` is unexplained spending.
fn statement_sign(lens: Lens, place: Id<Place>) -> Option<i64> {
    if place == lens.book.roots.unknown {
        return Some(1);
    }
    match lens.sides.side(place) {
        Some(Side::Income) => Some(-1),
        Some(Side::Spending) => Some(1),
        None => None,
    }
}

impl Statement {
    fn compile(lens: Lens, periods: Periods, cutoff: Day) -> Statement {
        let (book, run) = (lens.book, lens.run);
        let mut statement = Statement {
            periods,
            cutoff,
            grid: Grid::new(book.places.len(), periods.len()),
            gains: vec![Qty::ZERO; periods.len()],
            spread_seen: false,
            unpriced: Priced::default(),
        };
        for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
            statement.record(lens, &posting);
        }
        for gain in run.gains.iter().filter(|gain| gain.day <= cutoff && lens.owns(gain.from)) {
            for period in periods.overlapping(gain.day, gain.day) {
                statement.gains[period] += gain.gain();
            }
        }
        statement
    }

    /// The source pays on the flow's day; the arrival is recognized over its
    /// whole range.
    fn record(&mut self, lens: Lens, posting: &Posting) {
        let flow = posting.flow;
        self.spread_seen |= flow.recognized.last() > flow.day;
        self.recognize(lens, flow.from, posting.out_in_base(lens).map(|qty| -qty), Days::on(flow.day));
        self.recognize(lens, flow.to, posting.arrive_in_base(lens), flow.recognized);
    }

    fn recognize(&mut self, lens: Lens, place: Id<Place>, change: Option<Qty>, over: Days) {
        let Some(sign) = statement_sign(lens, place).filter(|_| lens.owns(place)) else { return };
        let Some(change) = self.unpriced.add(change) else { return };
        let recognized = Qty(change.0 * sign);
        for period in self.periods.overlapping(over.first(), over.last()) {
            let window = self.periods.window(period).days();
            if let Some(happened) = Days::new(window.first(), window.last().min(self.cutoff)) {
                self.grid.add(place, period, spread(recognized, over, happened));
            }
        }
    }

    fn section<'s>(&self, lens: Lens<'_, 's>) -> Section<'s> {
        let book = lens.book;
        let mut section = table("Place", &self.periods);

        let gains = Derived { label: "realized gains ≈", side: "income", values: &self.gains, style: Style::Muted };
        let unexplained = self.grid.subtree(&book.places, book.roots.unknown);
        let unexplained =
            Derived { label: "unexplained (?)", side: "spending", values: &unexplained, style: Style::Normal };
        let income = self.side_rows(lens, &mut section, Side::Income, &gains);
        let spending = self.side_rows(lens, &mut section, Side::Spending, &unexplained);
        // Spending that vanished into `?` is still spending.
        let net: Vec<Qty> = income.iter().zip(&spending).map(|(&earned, &spent)| earned - spent).collect();
        section.push(net_row(book, &net));

        if !is_zero(&self.gains) {
            section.note(
                "≈ Realized gains are derived from the basis of the parcels sold; the journal does not state them.",
            );
        }
        if self.spread_seen {
            section.note(
                "Flows written over a date range are recognized a little each day across the periods they cover.",
            );
        }
        section.unpriced(self.unpriced.missing(), "flow");
        section
    }

    /// The rows of one side's tree, then its derived line; returns the side's total.
    fn side_rows<'s>(&self, lens: Lens<'_, 's>, section: &mut Section<'s>, side: Side, derived: &Derived) -> Vec<Qty> {
        let (book, has_derived) = (lens.book, !is_zero(derived.values));
        let mut total = derived.values.to_vec();
        for root in book.places.roots().filter(|&root| lens.sides.side(root) == Some(side)) {
            for place in book.places.subtree(root) {
                let values = self.grid.subtree(&book.places, place);
                if place == root {
                    add_into(&mut total, &values);
                }
                if is_zero(&values) {
                    continue;
                }
                let style = if place == root && !has_derived { Style::Total } else { Style::Normal };
                for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
                    let during = When::During(self.periods.window(index).days());
                    let concept = if side == Side::Income { "income" } else { "spending" };
                    section.fact(
                        concept,
                        Some(path(book, place)),
                        lens.whose.label(book),
                        during,
                        Money::base(book, qty),
                    );
                }
                section.push(row(book, Cell::Name(leaf(book, place)), depth(book, place), &values, style));
            }
        }
        if has_derived {
            section.push(row(book, derived.label.into(), 1, derived.values, derived.style));
            section.push(row(book, ["Total".into(), derived.side.into()].into(), 0, &total, Style::Total));
        }
        total
    }
}

/// An empty statement: a column for the rows' names, one for each period, and a total.
fn table<'s>(title: &'static str, periods: &Periods) -> Section<'s> {
    let columns = (0..periods.len()).map(|period| Column::right(Cell::Period(periods.window(period).days())));
    let total = (periods.len() > 1).then(|| Column::right("Total"));
    Section::new(iter::once(Column::left(title)).chain(columns).chain(total))
}

/// The bottom line: every period's figure, shown even when it is nothing.
fn net_row<'s>(book: &Book<'s>, net: &[Qty]) -> Row<'s> {
    let cells = net.iter().map(|&qty| Cell::base(book, qty));
    let total = (net.len() > 1).then(|| Cell::base(book, net.iter().copied().sum()));
    Row::new(iter::once("Net".into()).chain(cells).chain(total)).style(Style::Total)
}

/// A statement's row: what it is about, its figure in each period, and their total.
fn row<'s>(book: &Book<'s>, label: Cell<'s>, depth: usize, values: &[Qty], style: Style) -> Row<'s> {
    let cells = values.iter().map(|&qty| Cell::base_or_blank(book, qty));
    let total = (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
    Row::new(iter::once(label).chain(cells).chain(total)).depth(depth).style(style)
}

/// A line the journal never wrote, added to a side: gains, or unexplained value.
struct Derived<'a> {
    label: &'static str,
    /// What the side is called in its total: `Total income`.
    side: &'static str,
    values: &'a [Qty],
    style: Style,
}
