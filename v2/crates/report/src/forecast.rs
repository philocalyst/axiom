//! `forecast`: where the books are heading.
//!
//! Plans, the rhythms history shows, obligations coming due, and growth
//! models are run forward through a clone of the ledger, so the laws judge the
//! future the way they judge the past. Variable spending, which nobody planned,
//! is bootstrapped from history into bands around that committed path.

mod bands;
mod habits;
mod projection;
mod recurrence;

use std::iter;

use axiom_core::day::days_in_month;
use axiom_core::{Day, Id, Map, Qty, Set, Span};
use axiom_engine::{Effect, Run, Violation};
use axiom_model::{Amount, Book, Entity, Flow, Law, Period, Place, Subject};

use self::bands::{Bands, Share};
use self::habits::{Habits, Variable};
use self::projection::{Expectation, Origin, Trace};
use crate::calendar::Periods;
use crate::places::{path, route};
use crate::table::headline;
use crate::value::Valuer;
use crate::{Cell, Column, Report, Row, Section, Style};

/// A fixed seed: the same books always give the same bands.
const SEED: u64 = 0x5EED_0A11_CE00_0001;

/// Bootstrapping needs a past to draw from.
const MIN_HISTORY_MONTHS: usize = 3;

pub fn view<'s>(book: &Book<'s>, run: &Run, until: Option<Day>, paths: u32) -> Report<'s> {
    let today = run.today;
    let until = until.unwrap_or_else(|| today.add(Span::months(12))).max(today);

    let mut expected = projection::from_plans(book);
    let planned: Set<(Id<Place>, Id<Place>)> =
        expected.iter().map(|plan| (plan.template.from, plan.template.to)).collect();
    let habits = Habits::infer(book, run, &planned);
    expected.extend(projection::from_habits(book, &habits));

    let mut flows: Vec<Flow> = expected.iter().flat_map(|expectation| expectation.flows(today, until)).collect();
    flows.sort_by_key(|flow| flow.day);
    let checkpoints = checkpoints(today, until);
    let trace = projection::project(book, today, flows, &checkpoints);

    let due = coming_due(&trace.run, book.roots.me, today);
    let committed = committed(book, &checkpoints, &trace, &due);
    let variable =
        Variable::from_history(book, run, |flow| planned.contains(&(flow.from, flow.to)) || habits.explains(flow));
    let bands = simulate(&checkpoints, &committed, &variable, paths);

    let mut outlook = outlook_section(book, &checkpoints, &committed, bands.as_ref());
    for note in method_notes(book, bands.as_ref(), variable.months, paths) {
        outlook.note(note);
    }

    Report::new(format!("Forecast to {until}"))
        .with(outlook)
        .with(expected_section(book, &expected, today, until))
        .with(owed_section(book, &due))
        .with(problems_section(book, &trace, today))
}

/// Today, then the end of every month up to `until`, which closes the last.
fn checkpoints(today: Day, until: Day) -> Vec<Day> {
    let months = Periods::covering(Period::Month, today, until);
    let mut days: Vec<Day> = iter::once(today).chain(months.ends().map(|end| end.min(until))).collect();
    days.dedup();
    days
}

/// Obligations of `owner` falling due after `today`, soonest first.
fn coming_due(run: &Run, owner: Id<Entity>, today: Day) -> Vec<&Effect> {
    let mut due: Vec<&Effect> = run
        .effects
        .iter()
        .filter(|effect| effect.owner == owner && effect.owe.is_some_and(|owed| owed.due > today))
        .collect();
    due.sort_by_key(|effect| effect.owe.map(|owed| owed.due));
    due
}

/// The liquid position at each checkpoint, less obligations already due.
fn committed(book: &Book, checkpoints: &[Day], trace: &Trace, due: &[&Effect]) -> Vec<Qty> {
    let valuer = Valuer::new(book, checkpoints[0]);
    let owed_by = |day: Day| -> Qty {
        let paid = due.iter().filter(|effect| effect.owe.is_some_and(|owed| owed.due <= day));
        paid.filter_map(|effect| valuer.qty(effect.amount)).sum()
    };
    checkpoints.iter().zip(&trace.liquid).map(|(&day, &liquid)| liquid - owed_by(day)).collect()
}

/// Bands for every checkpoint after today, if there is a past to draw on.
fn simulate(checkpoints: &[Day], committed: &[Qty], variable: &Variable, paths: u32) -> Option<Bands> {
    if paths == 0 || variable.months < MIN_HISTORY_MONTHS || variable.categories.is_empty() {
        return None;
    }
    let standing: Vec<i64> = committed[1..].iter().map(|qty| qty.0).collect();
    // A period may be part of a month: what is left of this one, or what is
    // wanted of the last.
    let shares: Vec<Share> = checkpoints
        .windows(2)
        .map(|pair| {
            let (year, month, _) = pair[1].ymd();
            Share { days: i64::from(pair[1].0 - pair[0].0), of: i64::from(days_in_month(year, month)) }
        })
        .collect();
    Some(bands::simulate(&standing, &shares, &variable.categories, paths as usize, SEED))
}

// ─── Sections ───────────────────────────────────────────────────────────────

/// How the outlook was worked out, in plain words.
fn method_notes(book: &Book, bands: Option<&Bands>, history_months: usize, paths: u32) -> [String; 2] {
    let committed = format!(
        "Committed: liquid assets less debts, in {}, after plans, recurring flows found in history, growth models and \
         obligations as they fall due. Variable spending is not in it.",
        book.name(book.commodities[book.base].symbol)
    );
    let spread = match bands {
        Some(bands) => format!(
            "p10, p50 and p90 are the 10th, 50th and 90th percentile of {} paths. Each path takes the committed figure and subtracts, month \
             by month, a random past month of spending for every top-level expense category (from {history_months} months of history, without \
             the flows already projected). The seed is fixed, so the bands are reproducible.",
            bands.paths()
        ),
        None if paths == 0 => "Bands are off (--paths 0).".to_string(),
        None => format!("Bands need {MIN_HISTORY_MONTHS} full months of spending history; there is not enough yet."),
    };
    [committed, spread]
}

fn outlook_section<'s>(book: &Book<'s>, checkpoints: &[Day], committed: &[Qty], bands: Option<&Bands>) -> Section<'s> {
    let mut columns = vec![Column::left("Month end"), Column::right("Committed")];
    if bands.is_some() {
        columns.extend(["p10", "p50", "p90"].map(Column::right));
    }
    let mut section = Section::new(columns).headed("Liquid net worth");
    for (index, (&day, &qty)) in checkpoints.iter().zip(committed).enumerate() {
        let mut cells = vec![Cell::Day(day), Cell::base(book, qty)];
        if let Some(bands) = bands {
            // The first checkpoint is today: nothing has been spent yet.
            let spread = match index.checked_sub(1) {
                Some(month) => bands.percentiles(month).map(|quanta| Cell::base(book, Qty(quanta))),
                None => [Cell::Blank, Cell::Blank, Cell::Blank],
            };
            cells.extend(spread);
        }
        section.push(Row::new(cells).style(if qty.is_negative() { Style::Alert } else { Style::Normal }));
    }
    section
}

fn expected_section<'s>(book: &Book<'s>, expected: &[Expectation], today: Day, until: Day) -> Section<'s> {
    let columns = [
        Column::left("Expected"),
        Column::left("Every"),
        Column::right("Amount"),
        Column::left("Next"),
        Column::left("Source"),
    ];
    let mut section = Section::new(columns).headed("What recurs");
    for expectation in expected {
        let flow = expectation.template;
        let payee = flow.payee.map(|entity| format!(" ({})", book.name(book.entities[entity].path)));
        let what = format!("{}{}", route(book, flow), payee.unwrap_or_default());
        let next = expectation.schedule.days(today, until).first().copied();
        let source = match expectation.origin {
            Origin::Plan => "plan".to_string(),
            Origin::Habit { occurrences } => format!("seen {occurrences} times"),
        };
        let cells = [
            Cell::text(what),
            Cell::text(recurrence::describe(expectation.schedule.every)),
            Cell::amount(book, expectation.out),
            next.map_or(Cell::Blank, Cell::Day),
            Cell::text(source),
        ];
        section.push(Row::new(cells));
    }
    if section.rows.is_empty() {
        section.note(
            "Nothing recurs yet. Write `every month …` plans, or keep the journal going until history shows a rhythm.",
        );
    }
    section
}

fn owed_section<'s>(book: &Book<'s>, due: &[&Effect]) -> Section<'s> {
    let columns = [Column::left("Due"), Column::left("Obligation"), Column::left("To"), Column::right("Amount")];
    let mut section = Section::new(columns).headed("Obligations coming due");
    for effect in due {
        let Some(owed) = effect.owe else { continue };
        let cells = [
            Cell::Day(owed.due),
            Cell::text(book.name(effect.name)),
            Cell::text(book.name(book.entities[owed.to].path)),
            Cell::amount(book, effect.amount),
        ];
        section.push(Row::new(cells));
    }
    section
}

/// Laws the projection breaks, and places it overdraws, by date.
fn problems_section<'s>(book: &Book<'s>, trace: &Trace, today: Day) -> Section<'s> {
    let mut repeats: Map<(Id<Law>, Subject), (usize, &Violation)> = Map::default();
    for violation in trace.run.violations.iter().filter(|violation| violation.day > today) {
        repeats.entry((violation.law, violation.subject)).or_insert((0, violation)).0 += 1;
    }
    let mut problems: Vec<(Day, String, Style)> = Vec::new();
    for (count, first) in repeats.into_values() {
        let message = &trace.run.diagnostics[first.diagnostic as usize].message;
        let more = if count > 1 { format!(" (and {} more)", count - 1) } else { String::new() };
        let text = format!("{}: {}{more}", book.name(book.laws[first.law].name), headline(message));
        problems.push((first.day, text, if first.waived { Style::Muted } else { Style::Alert }));
    }
    for overdraft in &trace.overdrafts {
        let lowest = book.show(Amount::new(overdraft.lowest, book.base));
        problems.push((
            overdraft.first,
            format!("{} is overdrawn, down to {lowest}", path(book, overdraft.place)),
            Style::Alert,
        ));
    }
    problems.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

    let mut section = Section::new([Column::left("Date"), Column::left("Problem")]).headed("Problems ahead");
    for (day, text, style) in problems {
        section.push(Row::new([Cell::Day(day), Cell::text(text)]).style(style));
    }
    if section.rows.is_empty() {
        section.note("No law violations or overdrafts are projected.");
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn checkpoints_are_today_then_month_ends_closed_by_the_horizon() {
        // Mid-month: a partial first month, and a horizon that ends the last.
        assert_eq!(
            checkpoints(day(2026, 3, 15), day(2026, 5, 10)),
            [day(2026, 3, 15), day(2026, 3, 31), day(2026, 4, 30), day(2026, 5, 10)]
        );
        // Standing on a month end, it is not repeated.
        assert_eq!(checkpoints(day(2026, 3, 31), day(2026, 4, 30)), [day(2026, 3, 31), day(2026, 4, 30)]);
        assert_eq!(checkpoints(day(2026, 3, 31), day(2026, 3, 31)), [day(2026, 3, 31)]);
    }
}
