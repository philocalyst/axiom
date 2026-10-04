//! `budget`: what each purpose has spent against its dated allowance.

use axiom_core::{Day, Days, Id, Qty, Ratio};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Budget, Entity, Limit, Period};

use crate::calendar::Periods;
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section, Style};

pub(crate) fn report<'s>(view: View<'s, '_, '_>, at: Option<Day>, by: Period) -> Report<'s> {
    let at = at.unwrap_or(view.run.today);
    let nothing = if view.book().budgets.is_empty() {
        "No budgets are declared in this book."
    } else {
        "No budget headroom was recorded for this window."
    };
    let title = format!("Budgets for {}", Periods::covering(by, at, at).title(0));
    Report::new(title).with(section(view.on(at), at, by, |_| true, nothing))
}

/// The budgets `wanted` picks, each spent against its limit in the month or the year (`by`) that holds `at`. `nothing` says
/// why the table is empty when no budget has anything to show.
pub(crate) fn section<'s>(
    view: View<'s, '_, '_>,
    at: Day,
    by: Period,
    wanted: impl Fn(&Budget) -> bool,
    nothing: &'static str,
) -> Section<'s> {
    let book = view.book();
    let query = Periods::covering(by, at, at).window(0).days();
    let end = query.last().min(view.run.horizon);
    let mut table = Table::new(view, Asked { query, end, by });
    if !book.budgets.is_empty() && query.first() > end {
        table.section.note("The requested budget window is beyond the run horizon.");
        return table.section;
    }
    let mut budgets: Vec<_> = book.budgets.values().filter(|&budget| wanted(budget)).collect();
    budgets.sort_by_key(|budget| book.name(book.purposes[budget.purpose].name));
    for budget in budgets.into_iter().filter(|budget| budget.starts <= end) {
        let name = book.name(book.purposes[budget.purpose].name);
        let terms = *budget.terms.at(at.max(budget.starts).min(view.run.horizon));
        let owners = budget_owners(view, budget);
        if terms.period == Period::Year {
            let window = Periods::covering(Period::Year, at.max(budget.starts), end).window(0).days();
            table.yearly(budget, name, &terms.limit, &owners, window);
        } else {
            table.monthly(budget, name, &owners);
        }
    }
    if table.section.rows.is_empty() {
        table.section.note(nothing);
    }
    table.section.unpriced(table.unpriced, "budget total");
    table.section
}

/// What a budget spent in one window, against its limit.
struct Month {
    window: Days,
    spent: Amount,
    limit: Amount,
    /// Whether the budget is judged on the total since it began.
    carries: bool,
}

/// The window a budget table is about, and how it groups what it shows.
#[derive(Clone, Copy)]
struct Asked {
    /// The days the view is of: the month or the year asked for.
    query: Days,
    /// Where it stops: its last day, or the end of what the run covers.
    end: Day,
    by: Period,
}

/// The budget table as it fills: its rows, and how many budget totals could not be priced.
struct Table<'s, 'b, 'v> {
    view: View<'s, 'b, 'v>,
    asked: Asked,
    section: Section<'s>,
    unpriced: usize,
}

impl<'s, 'b, 'v> Table<'s, 'b, 'v> {
    fn new(view: View<'s, 'b, 'v>, asked: Asked) -> Table<'s, 'b, 'v> {
        let columns = ["Purpose", "Owner", "Window"]
            .map(Column::left)
            .into_iter()
            .chain(["Spent", "Limit", "Left", "Used"].map(Column::right));
        Table { view, asked, section: Section::new(columns), unpriced: 0 }
    }

    /// A budget that allows a limit for each year: one row for each owner, if it has one for the year.
    fn yearly(&mut self, budget: &Budget, name: &'s str, limit: &Limit, owners: &[Option<Id<Entity>>], window: Days) {
        let book = self.view.book();
        for &owner in owners {
            let reading = matching_reading(self.view.run, budget, owner, window);
            if let Some(values) = values(limit, reading) {
                self.push(name, owner_name(book, owner), window_label(window), values, 0);
            }
        }
    }

    /// A budget that allows a limit for each month: for each owner, a row for each month, and when the view is
    /// of a year, the year's total above them.
    fn monthly(&mut self, budget: &Budget, name: &'s str, owners: &[Option<Id<Entity>>]) {
        let book = self.view.book();
        let Asked { query, end, by } = self.asked;
        let months = Periods::covering(Period::Month, budget.starts.max(query.first()), end);
        for &owner in owners {
            let rows = self.months(budget, owner, months);
            if by == Period::Year {
                match sum_periods(self.view, &rows) {
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
            if start > self.asked.end || start > window.last() {
                continue;
            }
            let active = *budget.terms.at(start);
            if active.period != Period::Month {
                continue;
            }
            let reading = matching_reading(self.view.run, budget, owner, window);
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
        let book = self.view.book();
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
fn budget_owners(view: View<'_, '_, '_>, budget: &Budget) -> Vec<Option<Id<Entity>>> {
    let mut owners: Vec<_> = view
        .run
        .headroom
        .iter()
        .filter(|reading| reading.law == budget.law && view.owns_entity(reading.owner))
        .map(|reading| Some(reading.owner))
        .collect();
    owners.sort_unstable();
    if let Some(selected) = view.whose.owners() {
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

fn sum_periods(view: View<'_, '_, '_>, months: &[Month]) -> Option<(Amount, Amount)> {
    if months.iter().any(|month| month.carries) {
        let last = months.last().expect("a nonempty month series");
        return Some((last.spent, last.limit));
    }
    let spent = months.iter().try_fold(Qty::ZERO, |sum, month| Some(sum + view.value(month.spent)?))?;
    let limit = months.iter().try_fold(Qty::ZERO, |sum, month| Some(sum + view.value(month.limit)?))?;
    let unit = view.book().base;
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
