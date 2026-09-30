//! `budget`: every envelope, spent against its limit, for a month or a year.
//!
//! An envelope is a `budget` law, or a `warn` over a month or a year on an
//! expense place, which is what `budget 650 USD monthly` writes. This view is
//! the headroom of those laws in the period asked for. A year lists each
//! envelope's year, with its months beneath.

use std::collections::BTreeMap;

use axiom_core::calendar::Window;
use axiom_core::{Day, Days, Id, Ratio};
use axiom_engine::Headroom;
use axiom_model::{Amount, Book, Law, Period, Place, Subject};

use crate::calendar::Periods;
use crate::headroom::current;
use crate::lens::Lens;
use crate::places::{Side, path};
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

pub fn view<'s>(lens: Lens<'_, 's>, at: Option<Day>, by: Period) -> Report<'s> {
    let (book, whose, today) = (lens.book, lens.whose, lens.day);
    let at = at.unwrap_or(today);
    let periods = Periods::covering(by, at, at);
    let window = periods.window(0).days();
    // Every window of the period so far is read, those no flow reached included.
    let all = &current(lens, window.first(), window.last().min(today).max(at));
    let mut envelopes: BTreeMap<(Id<Place>, Id<Law>, u32), Vec<&Headroom>> = BTreeMap::new();
    for reading in all.iter().filter(|reading| whose.includes(reading.owner) && reading.days.overlaps(window)) {
        if let Some(place) = envelope(lens, reading) {
            envelopes.entry((place, reading.law, reading.step)).or_default().push(reading);
        }
    }

    let columns = ["Place", "Law", "Window"].map(Column::left).into_iter();
    let mut table = Section::new(columns.chain(["Spent", "Limit", "Left", "Used"].map(Column::right)));
    for ((place, law, _), mut months) in envelopes {
        months.sort_by_key(|reading| reading.days.first());
        let label = |window: Days| {
            [Cell::Name(path(book, place)), Cell::Name(book.name(book.laws[law].name)), Cell::Period(window)]
        };
        for reading in &months {
            let (of, owner) = (Some(path(book, place)), book.name(book.entities[reading.owner].path));
            table.fact("counted", of, owner, When::During(reading.days), Money::of(book, reading.counted));
            table.fact("limit", of, owner, When::During(reading.days), Money::of(book, reading.limit));
        }
        if by == Period::Year && months.len() > 1 {
            // A year of months adds up to one row, with the months beneath it.
            let sum = |pick: fn(&Headroom) -> Amount| {
                Amount::new(months.iter().map(|month| pick(month).qty).sum(), months[0].limit.unit)
            };
            table.push(line(book, label(window), sum(|month| month.counted), sum(|month| month.limit), Style::Normal));
            for month in months {
                let blank = [Cell::Blank, Cell::Blank, Cell::Period(month.days)];
                table.push(line(book, blank, month.counted, month.limit, Style::Muted).depth(1));
            }
        } else {
            for reading in months {
                table.push(line(book, label(reading.days), reading.counted, reading.limit, Style::Normal));
            }
        }
    }
    if table.rows.is_empty() {
        table.note("No budgets. A line like `budget 500 USD monthly` under an account makes one.");
    }
    Report::new(["Budgets for".into(), Cell::Period(window)]).with(table)
}

/// The place an envelope watches, if the reading is one.
fn envelope(lens: Lens, reading: &Headroom) -> Option<Id<Place>> {
    let Subject::Place(place) = reading.subject else { return None };
    let on_expenses = Window::exactly(reading.days).is_some() && lens.sides.side(place) == Some(Side::Spending);
    (reading.warn && (lens.is_budget(reading.law) || on_expenses)).then_some(place)
}

/// One window of an envelope. Past the limit it is an alert, whatever its style.
fn line<'s>(book: &Book<'s>, label: [Cell<'s>; 3], spent: Amount, limit: Amount, style: Style) -> Row<'s> {
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
