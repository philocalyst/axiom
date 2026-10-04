//! `balance`: what every place holds, as a tree, on one day or at each month end.

use std::collections::BTreeSet;
use std::iter;

use axiom_core::glob::glob;
use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Class, Commodity, Period, Place};

use crate::balances::Balances;
use crate::calendar::Periods;
use crate::lens::{Basket, Lens, Valued, on_balance_sheet};
use crate::places::{depth, leaf, names, path};
use crate::resolve;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// How many month-end columns `--monthly` shows.
const MONTHLY_COLUMNS: usize = 12;

/// What a balance is shown in: each commodity as it is, or all that has a price as the base currency's worth.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Worth {
    Native,
    Market,
}

/// Builds a balance view with names resolved by a shared report context.
pub(crate) fn view_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    globs: &[&str],
    worth: Worth,
    monthly: bool,
) -> Result<Report<'s>, Diagnostic> {
    let (book, at) = (lens.book(), lens.day);
    let selection = Selection::new(book, globs)?;
    let days = column_days(book, at, monthly);
    let snapshots = Balances::of(lens, run, &days);

    let mut table = Section::new(iter::once(Column::left("Place")).chain(amount_columns(book, &snapshots, worth)));
    let mut unpriced_holdings = 0;
    for place in book.listed_places() {
        match selection.mark(place) {
            Mark::Hidden => {}
            Mark::Context => table.push(context_row(book, place, snapshots.days().len())),
            Mark::Chosen if on_balance_sheet(book.places[place].class) => {
                unpriced_holdings += push_place(&mut table, lens, place, &snapshots, worth)
            }
            Mark::Chosen => {}
        }
    }
    if table.rows.is_empty() {
        table.note(format!("Nothing is held on {at}."));
    }
    if unpriced_holdings > 0 {
        table.note("Holdings without a price are muted, in their own commodity, and left out of every total.");
    }

    let title = if monthly { format!("Balances by month to {at}") } else { format!("Balances at {at}") };
    let mut report = Report::new(title).with(table);
    if globs.is_empty() {
        report = report.with(net_worth_section(lens, &snapshots));
    }
    Ok(report)
}

/// Assets and liabilities in the base currency, each priced as a whole: what
/// someone would call their net worth.
#[derive(Default, Debug)]
pub struct NetWorth {
    pub assets: Qty,
    /// Negative: liabilities are naturally credit balances.
    pub liabilities: Qty,
    /// Commodities with no price path, left out.
    pub unpriced: usize,
}

impl NetWorth {
    /// The books on `snapshots`' `column`, at the lens's prices.
    pub fn of(lens: Lens, snapshots: &Balances, column: usize) -> NetWorth {
        let book = lens.book();
        let side = |class: Class| -> Valued {
            let mut basket = Basket::default();
            for root in book.places.roots().filter(|&root| book.places[root].class == class) {
                basket.merge(&snapshots.subtree(book, column, root));
            }
            basket.value(lens)
        };
        let (assets, liabilities) = (side(Class::Asset), side(Class::Debt));
        NetWorth {
            assets: assets.total,
            liabilities: liabilities.total,
            unpriced: assets.unpriced.len() + liabilities.unpriced.len(),
        }
    }

    pub fn total(&self) -> Qty {
        self.assets + self.liabilities
    }
}

/// One column for a plain balance; a column per month end for `--monthly`,
/// the last of them being `at` itself.
fn column_days(book: &Book, at: Day, monthly: bool) -> Vec<Day> {
    if !monthly {
        return vec![at];
    }
    let first = book.flows.as_slice().first().map_or(at, |flow| flow.day.min(at));
    Periods::covering(Period::Month, first, at).last(MONTHLY_COLUMNS).ends().map(|end| end.min(at)).collect()
}

fn amount_columns<'s>(book: &'s Book<'_>, snapshots: &Balances, worth: Worth) -> Vec<Column<'s>> {
    if let [_] = snapshots.days() {
        let title = match worth {
            Worth::Market => format!("Value ({})", base_symbol(book)),
            Worth::Native => "Balance".to_string(),
        };
        return vec![Column::right(title)];
    }
    snapshots.days().iter().map(|day| Column::right(day.to_string())).collect()
}

fn base_symbol<'s>(book: &'s Book<'_>) -> &'s str {
    book.name(book.commodities[book.base].symbol)
}

// ─── Which places ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Hidden,
    /// Not asked for, but above something that was: shown to keep the tree.
    Context,
    Chosen,
}

/// The places a list of globs picks: each match with its whole subtree, and
/// the ancestors that hold the tree together.
struct Selection {
    marks: Vec<Mark>,
}

impl Selection {
    fn new(book: &Book, globs: &[&str]) -> Result<Selection, Diagnostic> {
        if let Some(stray) =
            globs.iter().find(|&&pattern| !book.places.values().any(|place| matches(pattern, book.name(place.path))))
        {
            return Err(resolve::nothing_named("place matching", stray, names(book)));
        }
        if globs.is_empty() {
            let mut marks = vec![Mark::Hidden; book.places.len()];
            for (id, place) in book.places.iter() {
                if on_balance_sheet(place.class) {
                    marks[id.index()] = Mark::Chosen;
                }
            }
            for index in (0..marks.len()).rev() {
                if marks[index] != Mark::Hidden
                    && let Some(parent) = book.places.parent(Id::new(index as u32))
                    && marks[parent.index()] == Mark::Hidden
                {
                    marks[parent.index()] = Mark::Context;
                }
            }
            return Ok(Selection { marks });
        }
        let mut marks = vec![Mark::Hidden; book.places.len()];
        // Pre-order: a parent is marked before its children.
        for (id, place) in book.places.iter() {
            let inherited = book.places.parent(id).is_some_and(|parent| marks[parent.index()] == Mark::Chosen);
            if inherited || globs.iter().any(|pattern| matches(pattern, book.name(place.path))) {
                marks[id.index()] = Mark::Chosen;
            }
        }
        // Children first, so context climbs to the root.
        for index in (0..marks.len()).rev() {
            if marks[index] != Mark::Hidden
                && let Some(parent) = book.places.parent(Id::new(index as u32))
                && marks[parent.index()] == Mark::Hidden
            {
                marks[parent.index()] = Mark::Context;
            }
        }
        Ok(Selection { marks })
    }

    fn mark(&self, place: Id<Place>) -> Mark {
        self.marks[place.index()]
    }
}

/// `checking` picks `assets/bank/checking`: a pattern may match the whole
/// path or any trailing run of its segments.
fn matches(pattern: &str, path: &str) -> bool {
    glob(pattern, path) || path.match_indices('/').any(|(slash, _)| glob(pattern, &path[slash + 1..]))
}

// ─── Rows ───────────────────────────────────────────────────────────────────

/// One line of a place's balance: a commodity, with a cell per column.
struct Line<'s> {
    style: Style,
    cells: Vec<Cell<'s>>,
}

/// Adds the place's lines to the table; returns how many were unpriced.
fn push_place<'s>(
    table: &mut Section<'s>,
    lens: Lens<'s, '_, '_, '_>,
    place: Id<Place>,
    snapshots: &Balances,
    worth: Worth,
) -> usize {
    let book = lens.book();
    let baskets: Vec<Basket> =
        (0..snapshots.days().len()).map(|column| snapshots.subtree(book, column, place)).collect();
    let sign = lens.display_sign(place);
    let lines = match worth {
        Worth::Market => market_lines(lens, sign, snapshots.days(), &baskets),
        Worth::Native => native_lines(book, sign, &baskets),
    };
    for (column, basket) in baskets.iter().enumerate() {
        let day = snapshots.days()[column];
        if worth == Worth::Market {
            let valued = basket.value(lens.on(day));
            if valued.priced > 0 {
                table.fact(
                    "balance",
                    Some(path(book, place)),
                    lens.whose.label(book),
                    When::Instant(day),
                    Money::base(book, Qty(valued.total.0 * sign)),
                );
            }
            for amount in valued.unpriced {
                table.fact(
                    "balance",
                    Some(path(book, place)),
                    lens.whose.label(book),
                    When::Instant(day),
                    Money::of(book, Amount::new(Qty(amount.qty.0 * sign), amount.unit)),
                );
            }
        } else {
            for amount in basket.amounts() {
                table.fact(
                    "balance",
                    Some(path(book, place)),
                    lens.whose.label(book),
                    When::Instant(day),
                    Money::of(book, Amount::new(Qty(amount.qty.0 * sign), amount.unit)),
                );
            }
        }
    }
    let unpriced = lines.iter().filter(|line| line.style == Style::Muted).count();
    let is_root = depth(book, place) == 0;
    for (index, line) in lines.into_iter().enumerate() {
        let label = if index == 0 { Cell::Name(leaf(book, place)) } else { Cell::Blank };
        let style = if is_root && line.style == Style::Normal { Style::Total } else { line.style };
        table.push(Row::new(iter::once(label).chain(line.cells)).depth(depth(book, place)).style(style));
    }
    unpriced
}

fn context_row<'s>(book: &'s Book<'_>, place: Id<Place>, columns: usize) -> Row<'s> {
    Row::new(iter::once(Cell::Name(leaf(book, place))).chain((0..columns).map(|_| Cell::Blank)))
        .depth(depth(book, place))
        .style(Style::Muted)
}

/// One line per commodity held anywhere in the subtree, at its own units.
fn native_lines<'s>(book: &'s Book<'_>, sign: i64, baskets: &[Basket]) -> Vec<Line<'s>> {
    let units: BTreeSet<Id<Commodity>> =
        baskets.iter().flat_map(|basket| basket.amounts().map(|held| held.unit)).collect();
    units
        .into_iter()
        .map(|unit| Line {
            style: Style::Normal,
            cells: baskets.iter().map(|basket| amount_cell(book, basket.get(unit), unit, sign)).collect(),
        })
        .collect()
}

/// One line valuing everything priceable in the base currency, then a muted
/// line for each commodity that has no price.
fn market_lines<'s>(lens: Lens<'s, '_, '_, '_>, sign: i64, days: &[Day], baskets: &[Basket]) -> Vec<Line<'s>> {
    let book = lens.book();
    let valued: Vec<Valued> = baskets.iter().zip(days).map(|(basket, &day)| basket.value(lens.on(day))).collect();
    let mut lines = Vec::new();
    if valued.iter().any(|column| column.priced > 0) {
        let cells = valued
            .iter()
            .map(|column| if column.priced == 0 { Cell::Blank } else { Cell::base(book, Qty(column.total.0 * sign)) });
        lines.push(Line { style: Style::Normal, cells: cells.collect() });
    }
    let unpriced: BTreeSet<Id<Commodity>> =
        valued.iter().flat_map(|column| column.unpriced.iter().map(|held| held.unit)).collect();
    for unit in unpriced {
        let cells = valued.iter().map(|column| {
            let held = column.unpriced.iter().find(|held| held.unit == unit);
            amount_cell(book, held.map_or(Qty::ZERO, |held| held.qty), unit, sign)
        });
        lines.push(Line { style: Style::Muted, cells: cells.collect() });
    }
    lines
}

fn amount_cell<'s>(book: &'s Book<'_>, qty: Qty, unit: Id<Commodity>, sign: i64) -> Cell<'s> {
    if qty.is_zero() { Cell::Blank } else { Cell::amount(book, Amount::new(Qty(qty.0 * sign), unit)) }
}

// ─── Net worth ──────────────────────────────────────────────────────────────

fn net_worth_section<'s>(lens: Lens<'s, '_, '_, '_>, snapshots: &Balances) -> Section<'s> {
    let book = lens.book();
    let worths: Vec<NetWorth> = snapshots
        .days()
        .iter()
        .enumerate()
        .map(|(column, &day)| NetWorth::of(lens.on(day), snapshots, column))
        .collect();
    let columns = amount_columns(book, snapshots, Worth::Market);
    let mut section = Section::new(iter::once(Column::left("Net worth")).chain(columns));
    let rows: [(&str, Style, fn(&NetWorth) -> Qty); 3] = [
        ("Assets", Style::Normal, |worth| worth.assets),
        ("Liabilities", Style::Normal, |worth| -worth.liabilities),
        ("Net worth", Style::Total, |worth| worth.total()),
    ];
    for (label, style, pick) in rows {
        let cells = worths.iter().map(|worth| Cell::base(book, pick(worth)));
        section.push(Row::new(iter::once(Cell::text(label)).chain(cells)).style(style));
    }
    for (day, worth) in snapshots.days().iter().copied().zip(&worths) {
        for (concept, amount) in
            [("assets", worth.assets), ("liabilities", -worth.liabilities), ("net_worth", worth.total())]
        {
            section.fact(concept, None, lens.whose.label(book), When::Instant(day), Money::base(book, amount));
        }
    }
    if let Some(unpriced) = worths.iter().map(|worth| worth.unpriced).max().filter(|&n| n > 0) {
        section.note(format!("{unpriced} holdings have no price and are not counted."));
    }
    section
}
