//! Calendar segments for dated purpose budgets.
//!
//! A period change inside a calendar period closes the old segment the day
//! before the change and starts the new cadence on the change date. A change
//! to the limit alone keeps the open segment; its effective limit is read at
//! that segment's end (or at today's date while it is open).

use axiom_core::{Day, Days, Period};
use axiom_model::{Budget, BudgetTerms, Window};

pub(crate) fn window(period: Period) -> Window {
    match period {
        Period::Month => Window::Month,
        Period::Year => Window::Year,
    }
}

/// The first day of the active calendar segment containing `day`.
pub(crate) fn segment_start(budget: &Budget, lower_bound: Day, day: Day) -> Day {
    let initial = lower_bound.max(window(budget.terms.at(day).period).around(day).first());
    let mut start = initial;
    let mut previous = *budget.terms.at(initial);
    for (change, next) in budget.terms.changes().take_while(|(change, _)| *change <= day) {
        if change <= initial {
            previous = *next;
            continue;
        }
        if structural_change(previous, *next) {
            start = change;
        }
        previous = *next;
    }
    start
}

/// The end of the active segment containing `day`, for report/window identity.
pub(crate) fn segment_days(budget: &Budget, day: Day) -> Days {
    let first = segment_start(budget, budget.starts, day);
    let last = window(budget.terms.at(day).period).around(day).last();
    Days::new(first, last).unwrap_or_else(|| Days::on(day))
}

/// The last day before a period or carry-policy change, bounded by `through`.
pub(crate) fn segment_end(budget: &Budget, start: Day, through: Day) -> Day {
    let terms = *budget.terms.at(start);
    let natural_end = window(terms.period).around(start).last().min(through);
    budget
        .terms
        .changes()
        .take_while(|(change, _)| *change <= natural_end)
        .find(|(change, next)| *change > start && structural_change(terms, **next))
        .map_or(natural_end, |(change, _)| change.add_days(-1))
}

/// The first day of the currently continuous carrying run.
pub(crate) fn carry_start(budget: &Budget, start: Day, through: Day) -> Option<Day> {
    let mut previous = *budget.terms.at(start);
    let mut active = previous.carries.then_some(start);
    for (change, next) in budget.terms.changes().take_while(|(change, _)| *change <= through) {
        if change <= start {
            previous = *next;
            active = previous.carries.then_some(start);
            continue;
        }
        if !previous.carries && next.carries {
            active = Some(change);
        } else if previous.carries && !next.carries {
            active = None;
        }
        previous = *next;
    }
    active
}

fn structural_change(previous: BudgetTerms, next: BudgetTerms) -> bool {
    previous.period != next.period || previous.carries != next.carries
}
