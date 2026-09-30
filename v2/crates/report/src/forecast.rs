//! `forecast`: where the books are heading.
//!
//! Plans, the rhythms history shows, obligations coming due, and growth
//! models are run forward through a fork of the ledger, so the laws judge the
//! future the way they judge the past. Variable spending, which nobody planned,
//! is bootstrapped from history into bands around that committed path.

mod bands;
mod expected;
mod projection;
mod recurrence;
mod variable;

use std::iter;

use axiom_core::day::days_in_month;
use axiom_core::{Day, Id, Map, Qty, Span};
use axiom_engine::{Effect, Violation};
use axiom_model::{Amount, Book, Flow, Law, Period, Subject};

use self::bands::{Bands, Share};
use self::expected::{Expectation, Origin, expected};
use self::projection::{Trace, project};
use self::variable::Variable;
use crate::calendar::Periods;
use crate::closings;
use crate::lens::{Lens, Whose};
use crate::places::{path, route};
use crate::{Cell, Column, Report, Row, Section, Style};

/// A fixed seed: the same books always give the same bands.
const SEED: u64 = 0x5EED_0A11_CE00_0001;

/// Bootstrapping needs a past to draw from.
const MIN_HISTORY_MONTHS: usize = 3;

pub fn view<'s>(lens: Lens<'_, 's>, until: Option<Day>, paths: u32) -> Report<'s> {
    let (book, run, whose) = (lens.book, lens.run, lens.whose);
    let today = run.today;
    let until = until.unwrap_or_else(|| default_horizon(book, today)).max(today);
    let lens = lens.on(today);

    let expected = expected(lens);
    let mut flows: Vec<Flow> = expected.iter().flat_map(|expectation| expectation.flows(today, until)).collect();
    flows.sort_by_key(|flow| flow.day);
    let checkpoints = checkpoints(today, until);
    let trace = project(lens, today, flows, &checkpoints);

    let due = coming_due(&trace, whose, today);
    let committed = committed(lens, &checkpoints, &trace.liquid, &due);
    let variable = Variable::from_history(lens, |flow| expected.iter().any(|expectation| expectation.covers(flow)));
    let bands = simulate(&checkpoints, &committed, &variable, paths);

    let mut outlook = outlook_section(book, &checkpoints, &committed, &trace.worth, bands.as_ref());
    for note in method_notes(book, bands.as_ref(), variable.months, paths) {
        outlook.note(note);
    }

    Report::new(["Forecast to".into(), Cell::Day(until)])
        .with(outlook)
        .with(expected_section(book, &expected, today, until))
        .with(owed_section(book, &due))
        .with(problems_section(book, &trace, today))
}

/// A return that closes within this long after the default horizon is looked
/// at too: its tax is part of where the year the horizon falls in is heading.
const CLOSING_REACH: Span = Span::months(4);

/// A year from today, or the day a return closes if that is soon after.
fn default_horizon(book: &Book, today: Day) -> Day {
    let year_ahead = today.add(Span::months(12));
    match closings::next_after(book, year_ahead) {
        Some(closes) if closes <= year_ahead.add(CLOSING_REACH) => closes,
        _ => year_ahead,
    }
}

/// Today, then the end of every month up to `until`, which closes the last.
fn checkpoints(today: Day, until: Day) -> Vec<Day> {
    let months = Periods::covering(Period::Month, today, until);
    let mut days: Vec<Day> = iter::once(today).chain(months.ends().map(|end| end.min(until))).collect();
    days.dedup();
    days
}

/// Obligations of the lens's owners falling due after `today`, soonest first.
fn coming_due<'t>(trace: &'t Trace, whose: &Whose, today: Day) -> Vec<&'t Effect> {
    let owed = |effect: &&Effect| whose.includes(effect.owner) && effect.owe.is_some_and(|owed| owed.due > today);
    let mut due: Vec<&Effect> = trace.ledger.recorded().effects.iter().filter(owed).collect();
    due.sort_by_key(|effect| effect.owe.map(|owed| owed.due));
    due
}

/// The liquid position at each checkpoint, less obligations already due.
fn committed(lens: Lens, checkpoints: &[Day], liquid: &[Qty], due: &[&Effect]) -> Vec<Qty> {
    let lens = lens.on(checkpoints[0]);
    let owed_by = |day: Day| -> Qty {
        let paid = due.iter().filter(|effect| effect.owe.is_some_and(|owed| owed.due <= day));
        paid.filter_map(|effect| lens.value(effect.amount)).sum()
    };
    checkpoints.iter().zip(liquid).map(|(&day, &liquid)| liquid - owed_by(day)).collect()
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
fn method_notes<'s>(book: &Book<'s>, bands: Option<&Bands>, history_months: usize, paths: u32) -> [Cell<'s>; 2] {
    let committed = [
        "Committed: money in hand, less what is owed on debts with no term and on obligations as they fall due, in"
            .into(),
        Cell::Name(book.name(book.commodities[book.base].symbol)),
        ", after plans, recurring flows found in history and what they can still give. Investments and property \
         are net worth, not liquid. Variable spending is not in it."
            .into(),
    ];
    let spread = match bands {
        Some(bands) => [
            "p10, p50 and p90 are the 10th, 50th and 90th percentile of".into(),
            Cell::Count(bands.paths(), "path"),
            ". Each path takes the committed figure and subtracts, month by month, a random past month of spending \
             for every top-level expense category (from"
                .into(),
            Cell::Count(history_months, "month"),
            "of history, without the flows already projected). The seed is fixed, so the bands are reproducible."
                .into(),
        ]
        .into(),
        None if paths == 0 => "Bands are off (--paths 0).".into(),
        None => [
            "Bands need".into(),
            Cell::Count(MIN_HISTORY_MONTHS, "full month"),
            "of spending history; there is not enough yet.".into(),
        ]
        .into(),
    };
    [committed.into(), spread]
}

fn outlook_section<'s>(
    book: &Book<'s>,
    checkpoints: &[Day],
    committed: &[Qty],
    worth: &[Qty],
    bands: Option<&Bands>,
) -> Section<'s> {
    let mut columns = vec![Column::left("Month end"), Column::right("Committed"), Column::right("Net worth")];
    if bands.is_some() {
        columns.extend(["p10", "p50", "p90"].map(Column::right));
    }
    let mut section = Section::new(columns).headed("Liquid net worth");
    for (index, ((&day, &qty), &worth)) in checkpoints.iter().zip(committed).zip(worth).enumerate() {
        let mut cells = vec![Cell::Day(day), Cell::base(book, qty), Cell::base(book, worth)];
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

/// One row for each thing that recurs, soonest first: a paycheck's legs are one
/// row, shown under its biggest leg with what all of them come to.
fn expected_section<'s>(book: &Book<'s>, expected: &[Expectation], today: Day, until: Day) -> Section<'s> {
    let columns = [
        Column::left("Expected"),
        Column::left("Every"),
        Column::right("Amount"),
        Column::left("Next"),
        Column::left("Source"),
    ];
    let mut section = Section::new(columns).headed("What recurs");
    let mut rows = Vec::new();
    for legs in expected.chunk_by(|a, b| a.group() == b.group()) {
        let Some(main) = legs.iter().max_by_key(|leg| leg.out.qty.abs()) else { continue };
        let flow = main.template;
        let payee = flow.payee.map(|entity| {
            Cell::Join("", vec![" (".into(), Cell::Name(book.name(book.entities[entity].path)), ")".into()])
        });
        let more =
            (legs.len() > 1).then(|| Cell::Join("", vec![", and ".into(), Cell::Count(legs.len() - 1, "more leg")]));
        let what = Cell::Join("", iter::once(route(book, flow)).chain(payee).chain(more).collect());
        let total = legs.iter().filter(|leg| leg.out.unit == main.out.unit).map(|leg| leg.out.qty).sum();
        let next = legs.iter().filter_map(|leg| leg.schedule.days(today, until).first().copied()).min();
        let source = match main.origin {
            Origin::Plan(_) => "plan".into(),
            Origin::Habit { occurrences } => ["seen".into(), Cell::Count(occurrences, "time")].into(),
        };
        let cells = [
            what,
            recurrence::describe(main.schedule.every),
            Cell::amount(book, Amount::new(total, main.out.unit)),
            next.map_or(Cell::Blank, Cell::Day),
            source,
        ];
        rows.push((next, Row::new(cells)));
    }
    // Soonest first.
    rows.sort_by_key(|&(next, _)| next);
    for (_, row) in rows {
        section.push(row);
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
            Cell::Name(book.name(effect.name)),
            Cell::Name(book.name(book.entities[owed.to].path)),
            Cell::amount(book, effect.amount),
        ];
        section.push(Row::new(cells));
    }
    section
}

/// Laws the projection breaks, and places it overdraws, by date.
fn problems_section<'s>(book: &Book<'s>, trace: &Trace, today: Day) -> Section<'s> {
    let recorded = trace.ledger.recorded();
    let mut repeats: Map<(Id<Law>, Subject), (usize, &Violation)> = Map::default();
    for violation in recorded.violations.iter().filter(|violation| violation.day > today) {
        repeats.entry((violation.law, violation.subject)).or_insert((0, violation)).0 += 1;
    }
    // Each problem by its day, the name it is about, what to say, and how to show it.
    let mut problems: Vec<(Day, &str, Cell, Style)> = Vec::new();
    for (count, first) in repeats.into_values() {
        let (law, message) =
            (book.name(book.laws[first.law].name), &recorded.diagnostics[first.diagnostic as usize].message);
        let more =
            (count > 1).then(|| Cell::Join("", vec![" (and ".into(), Cell::Count(count - 1, ""), " more)".into()]));
        let text =
            Cell::Join("", [Cell::Name(law), ": ".into(), Cell::headline(message)].into_iter().chain(more).collect());
        problems.push((first.day, law, text, if first.waived { Style::Muted } else { Style::Alert }));
    }
    for overdraft in &trace.overdrafts {
        let (place, lowest) = (path(book, overdraft.place), Amount::new(overdraft.lowest, book.base));
        let text = [Cell::Name(place), "is overdrawn, down to".into(), Cell::amount(book, lowest)].into();
        problems.push((overdraft.first, place, text, Style::Alert));
    }
    problems.sort_by_key(|&(day, name, ..)| (day, name));

    let mut section = Section::new([Column::left("Date"), Column::left("Problem")]).headed("Problems ahead");
    for (day, _, text, style) in problems {
        section.push(Row::new([Cell::Day(day), text]).style(style));
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
