//! `budget`: what each purpose has spent against its dated allowance.

use axiom_core::{Day, Days, Id, Qty, Ratio};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Budget, Limit, Period};

use crate::calendar::Periods;
use crate::lens::Lens;
use crate::{Cell, Column, Report, Row, Section, Style};

pub(crate) fn view_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: Option<Day>, by: Period) -> Report<'s> {
    let at = at.unwrap_or(run.today);
    purpose_budgets(lens.on(at), run, at, by)
}

fn purpose_budgets<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: Day, by: Period) -> Report<'s> {
    let book = lens.book();
    let query = Periods::covering(by, at, at).window(0).days();
    let end = query.last().min(run.horizon);
    let mut table = Section::new(
        ["Purpose", "Owner", "Window"]
            .map(Column::left)
            .into_iter()
            .chain(["Spent", "Limit", "Left", "Used"].map(Column::right)),
    );
    if !book.budgets.is_empty() && query.first() > end {
        table.note("The requested budget window is beyond the run horizon.");
        return Report::new(format!("Budgets for {}", Periods::covering(by, at, at).title(0))).with(table);
    }
    let mut unpriced = 0;

    let mut budgets: Vec<_> = book.budgets.iter().map(|(_, budget)| budget).collect();
    budgets.sort_by_key(|budget| book.name(book.purposes[budget.purpose].name));
    for budget in budgets {
        if budget.starts > end {
            continue;
        }
        let name = book.name(book.purposes[budget.purpose].name);
        let terms = *budget.terms.at(at.max(budget.starts).min(run.horizon));
        let owners = budget_owners(lens, run, budget);

        if terms.period == Period::Year {
            let window = Periods::covering(Period::Year, at.max(budget.starts), end).window(0).days();
            for owner in owners.iter().copied() {
                let reading = matching_reading(run, budget, owner, window);
                let Some((spent, limit)) = values(&terms.limit, reading) else {
                    continue;
                };
                push_row(book, &mut table, name, owner_name(book, owner), window_label(window), spent, limit, 0);
            }
            continue;
        }

        let first = budget.starts.max(query.first());
        let months = Periods::covering(Period::Month, first, end);
        for owner in owners.iter().copied() {
            let mut month_rows = Vec::new();
            for index in 0..months.len() {
                let window = months.window(index).days();
                let start = window.first().max(budget.starts);
                if start > end || start > window.last() {
                    continue;
                }
                let active = *budget.terms.at(start);
                if active.period != Period::Month {
                    continue;
                }
                let reading = matching_reading(run, budget, owner, window);
                let Some((spent, limit)) = values(&active.limit, reading) else {
                    continue;
                };
                month_rows.push((window, spent, limit, active.carries));
            }

            if by == Period::Year {
                if let Some((spent, limit)) = sum_periods(lens, &month_rows) {
                    push_row(book, &mut table, name, owner_name(book, owner), window_label(query), spent, limit, 0);
                } else {
                    unpriced += 1;
                }
                for (window, spent, limit, _) in month_rows {
                    push_row(book, &mut table, "", "", window_label(window), spent, limit, 1);
                }
            } else {
                for (window, spent, limit, _) in month_rows {
                    push_row(book, &mut table, name, owner_name(book, owner), window_label(window), spent, limit, 0);
                }
            }
        }
    }

    if table.rows.is_empty() {
        table.note(if book.budgets.is_empty() {
            "No budgets are declared in this book."
        } else {
            "No budget headroom was recorded for this window."
        });
    }
    table.unpriced(unpriced, "budget total");
    Report::new(format!("Budgets for {}", Periods::covering(by, at, at).title(0))).with(table)
}

/// Preserve owners recorded by the engine, but still show a wholly unused
/// typed budget when it has not needed an owner-specific comparison yet.
fn budget_owners(lens: Lens<'_, '_, '_, '_>, run: &Run, budget: &Budget) -> Vec<Option<Id<axiom_model::Entity>>> {
    let mut owners: Vec<_> = run
        .headroom
        .iter()
        .filter(|reading| reading.law == budget.law && lens.owns_entity(reading.owner))
        .map(|reading| Some(reading.owner))
        .collect();
    owners.sort_unstable();
    if let Some(selected) = lens.whose.owners() {
        owners.extend(selected.iter().copied().map(Some));
    }
    owners.sort_unstable();
    owners.dedup();
    if owners.is_empty() {
        owners.push(None);
    }
    owners
}

fn matching_reading<'a>(
    run: &'a Run,
    budget: &Budget,
    owner: Option<Id<axiom_model::Entity>>,
    days: Days,
) -> Option<&'a Headroom> {
    run.headroom
        .iter()
        .filter(|reading| {
            reading.law == budget.law && owner.is_none_or(|owner| reading.owner == owner) && reading.days.overlaps(days)
        })
        .max_by_key(|reading| reading.day)
}

fn values(limit: &Limit, reading: Option<&Headroom>) -> Option<(Amount, Amount)> {
    if let Some(reading) = reading {
        return Some((reading.counted, reading.limit));
    }
    let Limit::Amount(limit) = *limit else {
        return None;
    };
    Some((Amount::zero(limit.unit), limit))
}

fn sum_periods(lens: Lens<'_, '_, '_, '_>, rows: &[(Days, Amount, Amount, bool)]) -> Option<(Amount, Amount)> {
    if rows.iter().any(|row| row.3) {
        let last = rows.last().copied().expect("a nonempty month series");
        return Some((last.1, last.2));
    }
    let spent = rows.iter().try_fold(Qty::ZERO, |sum, row| Some(sum + lens.value(row.1)?))?;
    let limit = rows.iter().try_fold(Qty::ZERO, |sum, row| Some(sum + lens.value(row.2)?))?;
    let unit = lens.book().base;
    Some((Amount::new(spent, unit), Amount::new(limit, unit)))
}

fn owner_name<'s>(book: &'s Book<'_>, owner: Option<Id<axiom_model::Entity>>) -> &'s str {
    owner.map_or("budget", |owner| book.name(book.entities[owner].path))
}

fn window_label(days: Days) -> String {
    match axiom_core::calendar::Window::exactly(days) {
        Some(window) => window.to_string(),
        None => format!("{}..{}", days.first(), days.last()),
    }
}

fn push_row<'s>(
    book: &'s Book<'_>,
    table: &mut Section<'s>,
    purpose: &'s str,
    owner: &'s str,
    window: String,
    spent: Amount,
    limit: Amount,
    depth: usize,
) {
    let left = Amount::new(room_amount(limit, spent), limit.unit);
    let ratio = used_amount(spent, limit);
    let purpose = if purpose.is_empty() { Cell::Blank } else { Cell::Purpose(purpose) };
    let row = Row::new([
        purpose,
        Cell::Name(owner),
        Cell::text(window),
        Cell::amount(book, spent),
        Cell::amount(book, limit),
        Cell::amount(book, left),
        ratio.map_or(Cell::Blank, Cell::Percent),
    ])
    .depth(depth)
    .style(if left.qty.is_negative() {
        Style::Alert
    } else {
        if depth == 0 { Style::Normal } else { Style::Muted }
    });
    table.push(row);
}

fn room_amount(limit: Amount, spent: Amount) -> Qty {
    limit.qty - spent.qty
}

fn used_amount(spent: Amount, limit: Amount) -> Option<Ratio> {
    (spent.unit == limit.unit).then(|| Ratio::new(spent.qty.0.into(), limit.qty.0.into())).flatten()
}
