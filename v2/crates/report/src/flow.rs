//! `flow`: the income statement, by month or by year.
//!
//! Income and expense places are read as the change in their balances over
//! each period, priced in the base currency on the day each flow happened.
//! A flow spread over a range is recognized a little each day, so a year's
//! premium lands in every month it covers.

use std::collections::HashMap;
use std::iter;

use axiom_core::{Day, Days, Id, Qty, spread};
use axiom_engine::Run;
use axiom_model::{Book, Class, Entity, Period, Place, PurposeRoot};

use crate::calendar::Periods;
use crate::history::{Posting, postings};
use crate::lens::{Lens, Whose};
use crate::places::{Side, depth, leaf, path, v3_side};
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

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

/// The same statement grouped by the other end of each flow.
pub fn view_by_party<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    from: Option<Day>,
    to: Option<Day>,
) -> Report<'s> {
    let cutoff = to.unwrap_or(run.today);
    view_by_party_with_lens(Lens::new(book, whose, cutoff), run, from, cutoff)
}

pub(crate) fn view_by_party_with_lens<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    from: Option<Day>,
    cutoff: Day,
) -> Report<'s> {
    let book = lens.book;
    let periods = match from {
        Some(from) => Periods::covering(Period::Month, from, cutoff),
        None => {
            let first = book
                .flows
                .as_slice()
                .first()
                .map_or(cutoff, |flow| flow.day.min(cutoff));
            Periods::covering(Period::Month, first, cutoff).last(DEFAULT_PERIODS)
        }
    };
    let mut values: HashMap<(PurposeRoot, Party), Vec<Qty>> = HashMap::new();
    let mut unpriced = 0;
    for posting in postings(book, run).filter(|posting| posting.is_real_on(cutoff)) {
        let flow = posting.flow;
        if !lens.whose.includes(flow.owner) || !flow.moves_quantity(axiom_model::End::From) {
            continue;
        }
        let from_outside = book.places[flow.from].class == Class::Outside;
        let to_outside = book.places[flow.to].class == Class::Outside;
        let inbound = from_outside && !to_outside;
        let outbound = to_outside && !from_outside;
        let root = flow
            .purpose
            .map(|purpose| book.purposes[purpose.purpose].root)
            .or_else(|| {
                if inbound {
                    Some(PurposeRoot::Income)
                } else if outbound {
                    Some(PurposeRoot::Spending)
                } else {
                    None
                }
            });
        let Some(root) = root else { continue };
        let amount = if inbound {
            posting.arrive_in_base(lens)
        } else {
            posting.out_in_base(lens)
        };
        let Some(amount) = amount else {
            unpriced += 1;
            continue;
        };
        // Income is positive when it comes in; spending and capital are
        // positive when they leave. Transfers are shown as gross movement.
        let reverses = (root == PurposeRoot::Income && !inbound)
            || ((root == PurposeRoot::Spending || root == PurposeRoot::Capital) && inbound);
        let amount = if reverses { -amount } else { amount };
        let other = if inbound { flow.from } else { flow.to };
        let party = flow.payee.map_or(Party::Place(other), Party::Entity);
        for period in periods.overlapping(flow.recognized.first(), flow.recognized.last()) {
            let window = periods.window(period).days();
            let Some(happened) = Days::new(window.first(), window.last().min(cutoff)) else {
                continue;
            };
            let part = spread(amount, flow.recognized, happened);
            values
                .entry((root, party))
                .or_insert_with(|| vec![Qty::ZERO; periods.len()])[period] += part;
        }
    }

    let mut section = table("Party", &periods);
    let roots = [
        PurposeRoot::Income,
        PurposeRoot::Spending,
        PurposeRoot::Capital,
        PurposeRoot::Transfer,
    ];
    let mut income = vec![Qty::ZERO; periods.len()];
    let mut spending = vec![Qty::ZERO; periods.len()];
    for root in roots {
        let mut parties: Vec<_> = values
            .iter()
            .filter(|((found, _), _)| *found == root)
            .collect();
        parties.sort_by_key(|((_, party), amounts)| {
            let magnitude = amounts
                .iter()
                .map(|qty| i128::from(qty.0).abs())
                .sum::<i128>();
            (-magnitude, party.label(book))
        });
        let total =
            parties
                .iter()
                .fold(vec![Qty::ZERO; periods.len()], |mut total, (_, amounts)| {
                    add_into(&mut total, amounts);
                    total
                });
        if is_zero(&total) {
            continue;
        }
        let heading = match root {
            PurposeRoot::Income => "Income",
            PurposeRoot::Spending => "Spending",
            PurposeRoot::Capital => "Capital",
            PurposeRoot::Transfer => "Transfer",
        };
        section.push(row(book, Cell::Word(heading), 0, &total, Style::Total));
        for ((_, party), amounts) in parties {
            let name = party.label(book);
            for (index, &amount) in amounts
                .iter()
                .enumerate()
                .filter(|(_, amount)| !amount.is_zero())
            {
                section.fact(
                    match root {
                        PurposeRoot::Income => "income",
                        PurposeRoot::Spending => "spending",
                        PurposeRoot::Capital => "capital",
                        PurposeRoot::Transfer => "transfer",
                    },
                    Some(name),
                    lens.whose.label(book),
                    When::During(periods.window(index).days()),
                    Money::base(book, amount),
                );
            }
            section.push(row(book, Cell::Name(name), 1, amounts, Style::Normal));
        }
        match root {
            PurposeRoot::Income => add_into(&mut income, &total),
            PurposeRoot::Spending => add_into(&mut spending, &total),
            _ => {}
        }
    }
    let net = income
        .iter()
        .zip(&spending)
        .map(|(&earned, &spent)| earned - spent)
        .collect::<Vec<_>>();
    if !is_zero(&net) {
        section.push(net_row(book, &net));
    }
    section.unpriced(unpriced, "flow");
    Report::new("Income and spending").with(section)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Party {
    Entity(Id<Entity>),
    Place(Id<Place>),
}

impl Party {
    fn label<'s>(self, book: &Book<'s>) -> &'s str {
        match self {
            Party::Entity(entity) => book.name(book.entities[entity].path),
            Party::Place(place) => path(book, place),
        }
    }
}

/// Builds an income statement with names resolved by a shared report context.
pub(crate) fn view_with_lens<'s>(
    lens: Lens<'_, 's>,
    run: &Run,
    by: Period,
    from: Option<Day>,
) -> Report<'s> {
    let (book, to) = (lens.book, lens.day);
    let periods = match from {
        Some(from) => Periods::covering(by, from, to),
        None => {
            let first = book
                .flows
                .as_slice()
                .first()
                .map_or(to, |flow| flow.day.min(to));
            Periods::covering(by, first, to).last(DEFAULT_PERIODS)
        }
    };
    let statement = Statement::compile(lens, run, periods, to);
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
        Grid {
            columns,
            cells: vec![Qty::ZERO; rows * columns],
        }
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
        for gain in run
            .gains
            .iter()
            .filter(|gain| gain.day <= cutoff && lens.owns(gain.from))
        {
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
        self.recognize(
            lens,
            flow.from,
            posting.out_in_base(lens).map(|qty| -qty),
            Days::on(flow.day),
        );
        self.recognize(lens, flow.to, posting.arrive_in_base(lens), flow.recognized);
    }

    fn recognize(&mut self, lens: Lens, place: Id<Place>, change: Option<Qty>, over: Days) {
        let Some(sign) = statement_sign(lens.book, place).filter(|_| lens.owns(place)) else {
            return;
        };
        let Some(change) = change else {
            self.unpriced += 1;
            return;
        };
        let recognized = Qty(change.0 * sign);
        for period in self.periods.overlapping(over.first(), over.last()) {
            let window = self.periods.window(period).days();
            if let Some(happened) = Days::new(window.first(), window.last().min(self.cutoff)) {
                self.grid
                    .add(place, period, spread(recognized, over, happened));
            }
        }
    }

    fn section<'s>(&self, lens: Lens<'_, 's>) -> Section<'s> {
        let book = lens.book;
        let periods =
            (0..self.periods.len()).map(|period| Column::right(self.periods.title(period)));
        let total = (self.periods.len() > 1).then(|| Column::right("Total"));
        let mut section = Section::new(
            iter::once(Column::left("Place"))
                .chain(periods)
                .chain(total),
        );

        let gains = Derived {
            label: "realized gains ≈",
            side: "income",
            values: &self.gains,
            style: Style::Muted,
        };
        let unexplained = self.grid.subtree(book, book.roots.unknown);
        let unexplained = Derived {
            label: "unexplained (?)",
            side: "spending",
            values: &unexplained,
            style: Style::Normal,
        };
        let income = self.side_rows(lens, &mut section, Side::Income, &gains);
        let spending = self.side_rows(lens, &mut section, Side::Spending, &unexplained);
        // Spending that vanished into `?` is still spending.
        let net: Vec<Qty> = income
            .iter()
            .zip(&spending)
            .map(|(&earned, &spent)| earned - spent)
            .collect();
        let cells = net
            .iter()
            .map(|&qty| Cell::base(book, qty))
            .chain((self.periods.len() > 1).then(|| Cell::base(book, net.iter().copied().sum())));
        section.push(Row::new(iter::once(Cell::Word("Net")).chain(cells)).style(Style::Total));
        for (index, &qty) in net.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
            section.fact(
                "net_income",
                None,
                lens.whose.label(book),
                When::During(self.periods.window(index).days()),
                Money::base(book, qty),
            );
        }

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
        section.unpriced(self.unpriced, "flow");
        section
    }

    /// The rows of one side's tree, then its derived line; returns the side's total.
    fn side_rows<'s>(
        &self,
        lens: Lens<'_, 's>,
        section: &mut Section<'s>,
        side: Side,
        derived: &Derived,
    ) -> Vec<Qty> {
        let book = lens.book;
        let has_derived = !is_zero(derived.values);
        let mut total = derived.values.to_vec();
        for root in book
            .places
            .roots()
            .filter(|&root| v3_side(book, root) == Some(side))
        {
            for place in book.places.subtree(root) {
                let values = self.grid.subtree(book, place);
                if place == root {
                    add_into(&mut total, &values);
                }
                if is_zero(&values) {
                    continue;
                }
                let style = if place == root && !has_derived {
                    Style::Total
                } else {
                    Style::Normal
                };
                for (index, &qty) in values.iter().enumerate().filter(|(_, qty)| !qty.is_zero()) {
                    section.fact(
                        if side == Side::Income { "income" } else { "spending" },
                        Some(path(book, place)),
                        lens.whose.label(book),
                        When::During(self.periods.window(index).days()),
                        Money::base(book, qty),
                    );
                }
                section.push(self.row(
                    book,
                    Cell::Name(leaf(book, place)),
                    depth(book, place),
                    &values,
                    style,
                ));
            }
        }
        if has_derived {
            section.push(self.row(
                book,
                Cell::Word(derived.label),
                1,
                derived.values,
                derived.style,
            ));
            section.push(self.row(
                book,
                Cell::list(" ", [Cell::Word("Total"), Cell::Word(derived.side)]),
                0,
                &total,
                Style::Total,
            ));
        }
        total
    }

    fn row<'s>(
        &self,
        book: &Book<'s>,
        label: Cell<'s>,
        depth: usize,
        values: &[Qty],
        style: Style,
    ) -> Row<'s> {
        let cells = values.iter().map(|&qty| Cell::base_or_blank(book, qty));
        let total =
            (values.len() > 1).then(|| Cell::base_or_blank(book, values.iter().copied().sum()));
        Row::new(iter::once(label).chain(cells).chain(total))
            .depth(depth)
            .style(style)
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
