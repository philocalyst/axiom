//! `balance`: what every place holds, as a tree, on one day or at each month end.

use std::collections::BTreeSet;
use std::iter;

use axiom_core::glob::glob;
use axiom_core::{Day, Diagnostic, Id, Qty, par};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Class, Commodity, Period, Place};

use crate::calendar::Periods;
use crate::history::Balances;
use crate::places::{depth, leaf, names};
use crate::resolve;
use crate::value::{Basket, Valued, Valuer};
use crate::{Cell, Column, Report, Row, Section, Style};

/// How many month-end columns `--monthly` shows.
const MONTHLY_COLUMNS: usize = 12;

/// The books on one day: a column of the report.
struct Snapshot {
    day: Day,
    balances: Balances,
}

pub fn view<'s>(
    book: &Book<'s>,
    run: &Run,
    globs: &[&str],
    at: Option<Day>,
    value: bool,
    monthly: bool,
) -> Result<Report<'s>, Diagnostic> {
    let at = at.unwrap_or(run.today);
    let selection = Selection::new(book, globs)?;
    let days = column_days(book, at, monthly);
    let snapshots = par::map(&days, |&day| Snapshot { day, balances: Balances::at(book, run, day) });

    let mut table = Section::new(iter::once(Column::left("Place")).chain(amount_columns(book, &snapshots, value)));
    let mut unpriced = 0;
    for place in book.places.ids() {
        match selection.mark(place) {
            Mark::Hidden => {}
            Mark::Context => table.push(context_row(book, place, snapshots.len())),
            Mark::Chosen => unpriced += push_place(&mut table, book, place, &snapshots, value),
        }
    }
    if table.rows.is_empty() {
        table.note(format!("Nothing is held on {at}."));
    }
    if unpriced > 0 {
        table.note("Holdings without a price are muted, in their own commodity, and left out of every total.");
    }

    let title = if monthly { format!("Balances by month to {at}") } else { format!("Balances at {at}") };
    let mut report = Report::new(title).with(table);
    if globs.is_empty() {
        report = report.with(net_worth_section(book, &snapshots));
    }
    Ok(report)
}

/// Assets and liabilities in the base currency: what someone would call
/// their net worth.
#[derive(Default, Debug)]
pub struct NetWorth {
    pub assets: Qty,
    /// Negative: liabilities are naturally credit balances.
    pub liabilities: Qty,
    /// Holdings with no price path, left out.
    pub unpriced: usize,
}

impl NetWorth {
    pub fn of(book: &Book, balances: &Balances, day: Day) -> NetWorth {
        let valuer = Valuer::new(book, day);
        let mut worth = NetWorth::default();
        for (place, amount) in balances.holdings() {
            let class = book.places[place].class;
            if !matches!(class, Class::Asset | Class::Liability) {
                continue;
            }
            match valuer.qty(amount) {
                Some(qty) if class == Class::Asset => worth.assets += qty,
                Some(qty) => worth.liabilities += qty,
                None => worth.unpriced += 1,
            }
        }
        worth
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

fn amount_columns(book: &Book, snapshots: &[Snapshot], value: bool) -> Vec<Column> {
    if snapshots.len() == 1 {
        let title = if value { format!("Value ({})", base_symbol(book)) } else { "Balance".to_string() };
        return vec![Column::right(title)];
    }
    snapshots.iter().map(|snapshot| Column::right(snapshot.day.to_string())).collect()
}

fn base_symbol<'s>(book: &Book<'s>) -> &'s str {
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
            return Ok(Selection { marks: vec![Mark::Chosen; book.places.len()] });
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
    book: &Book<'s>,
    place: Id<Place>,
    snapshots: &[Snapshot],
    value: bool,
) -> usize {
    let baskets: Vec<Basket> = snapshots.iter().map(|snapshot| snapshot.balances.subtree(book, place)).collect();
    let sign = book.places[place].class.display_sign();
    let lines = if value { market_lines(book, snapshots, &baskets, sign) } else { native_lines(book, &baskets, sign) };
    let unpriced = lines.iter().filter(|line| line.style == Style::Muted).count();
    let is_root = depth(book, place) == 0;
    for (index, line) in lines.into_iter().enumerate() {
        let label = if index == 0 { Cell::text(leaf(book, place)) } else { Cell::Blank };
        let style = if is_root && line.style == Style::Normal { Style::Total } else { line.style };
        table.push(Row::new(iter::once(label).chain(line.cells)).depth(depth(book, place)).style(style));
    }
    unpriced
}

fn context_row<'s>(book: &Book<'s>, place: Id<Place>, columns: usize) -> Row<'s> {
    Row::new(iter::once(Cell::text(leaf(book, place))).chain((0..columns).map(|_| Cell::Blank)))
        .depth(depth(book, place))
        .style(Style::Muted)
}

/// One line per commodity held anywhere in the subtree, at its own units.
fn native_lines<'s>(book: &Book<'s>, baskets: &[Basket], sign: i64) -> Vec<Line<'s>> {
    let units: BTreeSet<Id<Commodity>> =
        baskets.iter().flat_map(|basket| basket.iter().map(|held| held.unit)).collect();
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
fn market_lines<'s>(book: &Book<'s>, snapshots: &[Snapshot], baskets: &[Basket], sign: i64) -> Vec<Line<'s>> {
    let valued: Vec<Valued> = baskets
        .iter()
        .zip(snapshots)
        .map(|(basket, snapshot)| basket.value(&Valuer::new(book, snapshot.day)))
        .collect();
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

fn amount_cell<'s>(book: &Book<'s>, qty: Qty, unit: Id<Commodity>, sign: i64) -> Cell<'s> {
    if qty.is_zero() { Cell::Blank } else { Cell::amount(book, Amount::new(Qty(qty.0 * sign), unit)) }
}

// ─── Net worth ──────────────────────────────────────────────────────────────

fn net_worth_section<'s>(book: &Book<'s>, snapshots: &[Snapshot]) -> Section<'s> {
    let worths: Vec<NetWorth> =
        snapshots.iter().map(|snapshot| NetWorth::of(book, &snapshot.balances, snapshot.day)).collect();
    let columns = amount_columns(book, snapshots, true);
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
    if let Some(unpriced) = worths.iter().map(|worth| worth.unpriced).max().filter(|&n| n > 0) {
        section.note(format!("{unpriced} holdings have no price and are not counted."));
    }
    section
}
