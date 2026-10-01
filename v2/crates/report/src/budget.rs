//! `budget`: every envelope, spent against its limit, for a month or a year.
//!
//! Budgets are typed declarations on purposes. This view shows their engine
//! headroom in the requested month or year.

use axiom_core::{Day, Id, Ratio};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Period};

use crate::calendar::Periods;
use crate::headroom::window_words;
use crate::lens::Lens;
use crate::{Cell, Column, Report, Row, Section, Style};

pub(crate) fn view_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: Option<Day>, by: Period) -> Report<'s> {
    let at = at.unwrap_or(run.today);
    purpose_budgets(lens.on(at), run, at, by)
}

/// Budgets are typed declarations on purposes. Their law id ties the report to
/// the exact headroom readings the engine produced, including the owner scope.
fn purpose_budgets<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: Day, by: Period) -> Report<'s> {
    let book = lens.book();
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
                    && lens.whose.includes(reading.owner)
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
        table.note(if book.budgets.is_empty() {
            "No budgets are declared in this book."
        } else {
            "No budget headroom was recorded for this window."
        });
    }
    Report::new(format!("Budgets for {}", periods.title(0))).with(table)
}
