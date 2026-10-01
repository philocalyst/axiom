//! `flow`: the income statement, by month or by year.
//!
//! Income and expense places are read as the change in their balances over
//! each period, priced in the base currency on the day each flow happened.
//! A flow spread over a range is recognized a little each day, so a year's
//! premium lands in every month it covers.

use std::iter;

use axiom_core::{Day, Days, Id, Qty, spread};
use axiom_engine::Run;
use axiom_model::{Book, Period, Place};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::{Lens, Whose};
use crate::places::{Side, depth, leaf, v3_side};
use crate::{Cell, Column, Report, Row, Section, Style};

/// How many periods to show when the window is not given.
const DEFAULT_PERIODS: usize = 12;

pub fn view<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    by: Period,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let to = to.unwrap_or(run.today);
    view_with_lens(Lens::new(book, whose, to), run, by, from)
}

/// Builds an income statement with names resolved by a shared report context.
pub(crate) fn view_with_lens<'s>(lens: Lens<'_, 's>, run: &Run, by: Period, from: Option<Day>) -> Report<'s> {
    let (book, to) = (lens.book, lens.day);
    let periods = match from {
        Some(from) => Periods::covering(by, from, to),
        None => {
            let first = book.flows.as_slice().first().map_or(to, |flow| flow.day.min(to));
            Periods::covering(by, first, to).last(DEFAULT_PERIODS)
        }
    };
    let statement = Statement::compile(lens, run, periods, to);
    Report::new("Income and spending").with(statement.section(book))
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

    fn add(&mut self, place: Id<Place>, column: usize, qty: Qty) {
        self.cells[place.index() * self.columns + column] += qty;
    }

    /// Per-period totals for `place` and everything beneath it.
    fn subtree(&self, book: &Book, place: Id<Place>) -> Vec<Qty> {
        let rows = place.index() * self.columns..book.places.end(place).index() * self.columns;
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
    unpriced: usize,
}

/// How a place's balance change reads in the statement: income and spending
/// both come out positive. Value that fell into `?` is unexplained spending.
fn statement_sign(book: &Book, place: Id<Place>) -> Option<i64> {
    if place == book.roots.unknown {
        return Some(1);
    }
    match v3_side(book, place) {
        Some(Side::Income) => Some(-1),
        Some(Side::Spending) => Some(1),
        None => None,
    }
}

impl Statement {
    fn compile(lens: Lens, run: &Run, periods: Periods, cutoff: Day) -> Statement {
        let book = lens.book;
        let mut statement = Statement {
            periods,
            cutoff,
            grid: Grid::new(book.places.len(), periods.len()),
            gains: vec![Qty::ZERO; periods.len()],
            spread_seen: false,
            unpriced: 0,
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
        let Some(sign) = statement_sign(lens.book, place).filter(|_| lens.owns(place)) else { return };
        let Some(change) = change else {
            self.unpriced += 1;
            return;
        };
        let recognized = Qty(change.0 * sign);
        for period in self.periods.overlapping(over.first(), over.last()) {
            let window = self.periods.window(period).days();
            if let Some(happened) = Days::new(window.first(), window.last().min(self.cutoff)) {
                self.grid.add(place, period, spread(recognized, over, happened));
            }
        }
    }

    fn section<'s>(&self, book: &Book<'s>) -> Section<'s> {
        let periods = (0..self.periods.len()).map(|period| Column::right(self.periods.title(period)));
        let total = (self.periods.len() > 1).then(|| Column::right("Total"));
        let mut section = Section::new(iter::once(Column::left("Place")).chain(periods).chain(total));

        let gains = Derived { label: "realized gains ≈", side: "income", values: &self.gains, style: Style::Muted };
        let unexplained = self.grid.subtree(book, book.roots.unknown);
        let unexplained =
            Derived { label: "unexplained (?)", side: "spending", values: &unexplained, style: Style::Normal };
        let income = self.side_rows(book, &mut section, Side::Income, &gains);
        let spending = self.side_rows(book, &mut section, Side::Spending, &unexplained);
        // Spending that vanished into `?` is still spending.
        let net: Vec<Qty> = income.iter().zip(&spending).map(|(&earned, &spent)| earned - spent).collect();
        let cells = net
            .iter()
            .map(|&qty| Cell::base(book, qty))
            .chain((self.periods.len() > 1).then(|| Cell::base(book, net.iter().copied().sum())));
        section.push(Row::new(iter::once(Cell::text("Net")).chain(cells)).style(Style::Total));

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
        if self.unpriced > 0 {
            section.note(format!("{} flows have no price on their day and are left out.", self.unpriced));
        }
        section
    }

    /// The rows of one side's tree, then its derived line; returns the side's total.
    fn side_rows<'s>(&self, book: &Book<'s>, section: &mut Section<'s>, side: Side, derived: &Derived) -> Vec<Qty> {
        let has_derived = !is_zero(derived.values);
        let mut total = derived.values.to_vec();
        for root in book.places.roots().filter(|&root| v3_side(book, root) == Some(side)) {
            for place in book.places.subtree(root) {
                let values = self.grid.subtree(book, place);
                if place == root {
                    add_into(&mut total, &values);
                }
                if is_zero(&values) {
                    continue;
                }
                let style = if place == root && !has_derived { Style::Total } else { Style::Normal };
                section.push(self.row(book, Cell::text(leaf(book, place)), depth(book, place), &values, style));
            }
        }
        if has_derived {
            section.push(self.row(book, Cell::text(derived.label), 1, derived.values, derived.style));
            section.push(self.row(book, Cell::text(format!("Total {}", derived.side)), 0, &total, Style::Total));
        }
        total
    }

    fn row<'s>(&self, book: &Book<'s>, label: Cell<'s>, depth: usize, values: &[Qty], style: Style) -> Row<'s> {
        let cells = values.iter().map(|&qty| Cell::base_or_blank(book, qty));
        let total = (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
        Row::new(iter::once(label).chain(cells).chain(total)).depth(depth).style(style)
    }
}

/// A line the journal never wrote, added to a side: gains, or unexplained value.
struct Derived<'a> {
    label: &'static str,
    /// What the side is called in its total: `Total income`.
    side: &'static str,
    values: &'a [Qty],
    style: Style,
}
