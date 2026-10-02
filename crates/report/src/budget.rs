//! `budget`: what each purpose has spent against its dated allowance.

use axiom_core::{Day, Days, Id, Qty, Ratio};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Budget, Entity, Limit, Period};

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
    let title = format!("Budgets for {}", Periods::covering(by, at, at).title(0));
    let mut table = Table::new(lens, run, View { query, end, by });
    if !book.budgets.is_empty() && query.first() > end {
        table.section.note("The requested budget window is beyond the run horizon.");
        return Report::new(title).with(table.section);
    }
    let mut budgets: Vec<_> = book.budgets.iter().map(|(_, budget)| budget).collect();
    budgets.sort_by_key(|budget| book.name(book.purposes[budget.purpose].name));
    for budget in budgets.into_iter().filter(|budget| budget.starts <= end) {
        let name = book.name(book.purposes[budget.purpose].name);
        let terms = *budget.terms.at(at.max(budget.starts).min(run.horizon));
        let owners = budget_owners(lens, run, budget);
        if terms.period == Period::Year {
            let window = Periods::covering(Period::Year, at.max(budget.starts), end).window(0).days();
            table.yearly(budget, name, &terms.limit, &owners, window);
        } else {
            table.monthly(budget, name, &owners);
        }
    }
    if table.section.rows.is_empty() {
        table.section.note(if book.budgets.is_empty() {
            "No budgets are declared in this book."
        } else {
            "No budget headroom was recorded for this window."
        });
    }
    table.section.unpriced(table.unpriced, "budget total");
    Report::new(title).with(table.section)
}

/// What a budget spent in one window, against its limit.
struct Month {
    window: Days,
    spent: Amount,
    limit: Amount,
    /// Whether the budget is judged on the total since it began.
    carries: bool,
}

/// The window a budget view is about, and how it groups what it shows.
#[derive(Clone, Copy)]
struct View {
    /// The days the view is of: the month or the year asked for.
    query: Days,
    /// Where it stops: its last day, or the end of what the run covers.
    end: Day,
    by: Period,
}

/// The budget table as it fills: its rows, and how many budget totals could not be priced.
struct Table<'s, 'b, 'w, 'p, 'r> {
    lens: Lens<'s, 'b, 'w, 'p>,
    run: &'r Run,
    view: View,
    section: Section<'s>,
    unpriced: usize,
}

impl<'s, 'b, 'w, 'p, 'r> Table<'s, 'b, 'w, 'p, 'r> {
    fn new(lens: Lens<'s, 'b, 'w, 'p>, run: &'r Run, view: View) -> Table<'s, 'b, 'w, 'p, 'r> {
        let columns = ["Purpose", "Owner", "Window"]
            .map(Column::left)
            .into_iter()
            .chain(["Spent", "Limit", "Left", "Used"].map(Column::right));
        Table { lens, run, view, section: Section::new(columns), unpriced: 0 }
    }

    /// A budget that allows a limit for each year: one row for each owner, if it has one for the year.
    fn yearly(&mut self, budget: &Budget, name: &'s str, limit: &Limit, owners: &[Option<Id<Entity>>], window: Days) {
        let book = self.lens.book();
        for &owner in owners {
            let reading = matching_reading(self.run, budget, owner, window);
            if let Some(values) = values(limit, reading) {
                self.push(name, owner_name(book, owner), window_label(window), values, 0);
            }
        }
    }

    /// A budget that allows a limit for each month: for each owner, a row for each month, and when the view is
    /// of a year, the year's total above them.
    fn monthly(&mut self, budget: &Budget, name: &'s str, owners: &[Option<Id<Entity>>]) {
        let book = self.lens.book();
        let View { query, end, by } = self.view;
        let months = Periods::covering(Period::Month, budget.starts.max(query.first()), end);
        for &owner in owners {
            let rows = self.months(budget, owner, months);
            if by == Period::Year {
                match sum_periods(self.lens, &rows) {
                    Some(total) => self.push(name, owner_name(book, owner), window_label(query), total, 0),
                    None => self.unpriced += 1,
                }
                for month in rows {
                    self.push("", "", window_label(month.window), (month.spent, month.limit), 1);
                }
            } else {
                for month in rows {
                    self.push(name, owner_name(book, owner), window_label(month.window), (month.spent, month.limit), 0);
                }
            }
        }
    }

    /// What `owner` spent in each of `months` the budget allows a limit for each month and is in force.
    fn months(&self, budget: &Budget, owner: Option<Id<Entity>>, months: Periods) -> Vec<Month> {
        let mut rows = Vec::new();
        for index in 0..months.len() {
            let window = months.window(index).days();
            let start = window.first().max(budget.starts);
            if start > self.view.end || start > window.last() {
                continue;
            }
            let active = *budget.terms.at(start);
            if active.period != Period::Month {
                continue;
            }
            let reading = matching_reading(self.run, budget, owner, window);
            if let Some((spent, limit)) = values(&active.limit, reading) {
                rows.push(Month { window, spent, limit, carries: active.carries });
            }
        }
        rows
    }

    fn push(
        &mut self,
        purpose: &'s str,
        owner: &'s str,
        window: String,
        (spent, limit): (Amount, Amount),
        depth: usize,
    ) {
        let book = self.lens.book();
        let left = Amount::new(room_amount(limit, spent), limit.unit);
        let purpose = if purpose.is_empty() { Cell::Blank } else { Cell::Purpose(purpose) };
        let style = match (left.qty.is_negative(), depth) {
            (true, _) => Style::Alert,
            (false, 0) => Style::Normal,
            (false, _) => Style::Muted,
        };
        self.section.push(
            Row::new([
                purpose,
                Cell::Name(owner),
                Cell::text(window),
                Cell::amount(book, spent),
                Cell::amount(book, limit),
                Cell::amount(book, left),
                used_amount(spent, limit).map_or(Cell::Blank, Cell::Percent),
            ])
            .depth(depth)
            .style(style),
        );
    }
}

/// Preserve owners recorded by the engine, but still show a wholly unused
/// typed budget when it has not needed an owner-specific comparison yet.
fn budget_owners(lens: Lens<'_, '_, '_, '_>, run: &Run, budget: &Budget) -> Vec<Option<Id<Entity>>> {
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

fn matching_reading<'a>(run: &'a Run, budget: &Budget, owner: Option<Id<Entity>>, days: Days) -> Option<&'a Headroom> {
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

fn sum_periods(lens: Lens<'_, '_, '_, '_>, months: &[Month]) -> Option<(Amount, Amount)> {
    if months.iter().any(|month| month.carries) {
        let last = months.last().expect("a nonempty month series");
        return Some((last.spent, last.limit));
    }
    let spent = months.iter().try_fold(Qty::ZERO, |sum, month| Some(sum + lens.value(month.spent)?))?;
    let limit = months.iter().try_fold(Qty::ZERO, |sum, month| Some(sum + lens.value(month.limit)?))?;
    let unit = lens.book().base;
    Some((Amount::new(spent, unit), Amount::new(limit, unit)))
}

fn owner_name<'s>(book: &'s Book<'_>, owner: Option<Id<Entity>>) -> &'s str {
    owner.map_or("budget", |owner| book.name(book.entities[owner].path))
}

fn window_label(days: Days) -> String {
    match axiom_core::calendar::Window::exactly(days) {
        Some(window) => window.to_string(),
        None => format!("{}..{}", days.first(), days.last()),
    }
}

fn room_amount(limit: Amount, spent: Amount) -> Qty {
    limit.qty - spent.qty
}

fn used_amount(spent: Amount, limit: Amount) -> Option<Ratio> {
    (spent.unit == limit.unit).then(|| Ratio::new(spent.qty.0.into(), limit.qty.0.into())).flatten()
}
