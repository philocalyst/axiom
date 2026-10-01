//! `budget`: every envelope, spent against its limit, for a month or a year.
//!
//! An envelope is a `budget` law, or a `warn` over a month or a year on an
//! expense place, which is what `budget 650 USD monthly` writes. This view is
//! the headroom of those laws in the period asked for. A year lists each
//! envelope's year, with its months beneath.

use std::collections::BTreeMap;

use axiom_core::calendar::Window;
use axiom_core::{Day, Id, Ratio};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Budget, Law, Period, Place, Subject};

use crate::calendar::Periods;
use crate::headroom::{current, window_words};
use crate::lens::Whose;
use crate::places::{Side, path, v3_side};
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &'s Book<'_>, run: &Run, whose: &Whose, at: Option<Day>, by: Period) -> Report<'s> {
    let at = at.unwrap_or(run.today);
    if !book.budgets.is_empty() {
        return purpose_budgets(book, run, whose, at, by);
    }
    // v3 bridge: v3 books have no typed budgets; retain their place-based caps
    // until the v3 model is removed.
    place_budgets(book, run, whose, at, by)
}

/// Budgets are typed declarations on purposes. Their law id ties the report to
/// the exact headroom readings the engine produced, including the owner scope.
fn purpose_budgets<'s>(book: &'s Book<'_>, run: &Run, whose: &Whose, at: Day, by: Period) -> Report<'s> {
    let periods = Periods::covering(by, at, at);
    let window = periods.window(0).days();
    let mut table = Section::new(
        ["Purpose", "Owner", "Window"].map(Column::left)
            .into_iter()
            .chain(["Spent", "Limit", "Left", "Used"].map(Column::right)),
    );

    for (_, budget) in book.budgets.iter() {
        let purpose = book.name(book.purposes[budget.purpose].name);
        let mut readings: Vec<_> = run
            .headroom
            .iter()
            .filter(|reading| {
                reading.law == budget.law
                    && whose.includes(reading.owner)
                    && reading.days.overlaps(window)
            })
            .collect();
        readings.sort_by_key(|reading| (reading.owner, reading.days.first()));
        for reading in readings {
            let left = Amount::new(reading.limit.qty - reading.counted.qty, reading.limit.unit);
            let used = Ratio::new(reading.counted.qty.0.into(), reading.limit.qty.0.into());
            table.push(
                Row::new([
                    Cell::Purpose(purpose),
                    Cell::Name(book.name(book.entities[reading.owner].path)),
                    Cell::text(window_words(reading)),
                    Cell::amount(book, reading.counted),
                    Cell::amount(book, reading.limit),
                    Cell::amount(book, left),
                    used.map_or(Cell::Blank, Cell::Percent),
                ])
                .style(if left.qty.is_negative() { Style::Alert } else { Style::Normal }),
            );
        }
    }
    if table.rows.is_empty() {
        table.note("No budget headroom was recorded for this window.");
    }
    Report::new(format!("Budgets for {}", periods.title(0))).with(table)
}

// v3 bridge: place-based `warn` caps were the only budget representation.
fn place_budgets<'s>(book: &'s Book<'_>, run: &Run, whose: &Whose, at: Day, by: Period) -> Report<'s> {
    let periods = Periods::covering(by, at, at);
    let window = periods.window(0).days();
    // Every window of the period so far is read, those no flow reached included.
    let all = &current(book, run, window.first(), window.last().min(run.today).max(at));
    let mut envelopes: BTreeMap<(Id<Place>, Id<Law>, u32), Vec<&Headroom>> = BTreeMap::new();
    for reading in all.iter().filter(|reading| whose.includes(reading.owner) && reading.days.overlaps(window)) {
        if let Some(place) = envelope(book, reading) {
            envelopes.entry((place, reading.law, reading.step)).or_default().push(reading);
        }
    }

    let columns = ["Place", "Law", "Window"].map(Column::left).into_iter();
    let mut table = Section::new(columns.chain(["Spent", "Limit", "Left", "Used"].map(Column::right)));
    for ((place, law, _), mut months) in envelopes {
        months.sort_by_key(|reading| reading.days.first());
        let label = |window: String| {
            [Cell::text(path(book, place)), Cell::text(book.name(book.laws[law].name)), Cell::text(window)]
        };
        if by == Period::Year && months.len() > 1 {
            // A year of months adds up to one row, with the months beneath it.
            let sum = |pick: fn(&Headroom) -> Amount| {
                Amount::new(months.iter().map(|month| pick(month).qty).sum(), months[0].limit.unit)
            };
            table.push(line(
                book,
                label(periods.title(0)),
                sum(|month| month.counted),
                sum(|month| month.limit),
                Style::Normal,
            ));
            for month in months {
                let blank = [Cell::Blank, Cell::Blank, Cell::text(window_words(month))];
                table.push(line(book, blank, month.counted, month.limit, Style::Muted).depth(1));
            }
        } else {
            for reading in months {
                table.push(line(book, label(window_words(reading)), reading.counted, reading.limit, Style::Normal));
            }
        }
    }
    if table.rows.is_empty() {
        table.note("No budgets. A line like `budget 500 USD monthly` under an account makes one.");
    }
    Report::new(format!("Budgets for {}", periods.title(0))).with(table)
}

/// The place an envelope watches, if the reading is one.
fn envelope(book: &Book, reading: &Headroom) -> Option<Id<Place>> {
    let Subject::Place(place) = reading.subject else { return None };
    let is_budget = book.name(book.laws[reading.law].name) == "budget";
    let on_expenses = Window::exactly(reading.days).is_some() && v3_side(book, place) == Some(Side::Spending);
    (reading.warn && (is_budget || on_expenses)).then_some(place)
}

/// One window of an envelope. Past the limit it is an alert, whatever its style.
fn line<'s>(book: &'s Book<'_>, label: [Cell<'s>; 3], spent: Amount, limit: Amount, style: Style) -> Row<'s> {
    let left = Amount::new(limit.qty - spent.qty, limit.unit);
    let used = Ratio::new(spent.qty.0.into(), limit.qty.0.into());
    let cells = [
        Cell::amount(book, spent),
        Cell::amount(book, limit),
        Cell::amount(book, left),
        used.map_or(Cell::Blank, Cell::Percent),
    ];
    Row::new(label.into_iter().chain(cells)).style(if spent.qty > limit.qty { Style::Alert } else { style })
}
