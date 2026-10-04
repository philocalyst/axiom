//! `forecast`: where the books are heading.
//!
//! Plans, the rhythms history shows, obligations coming due, and growth
//! models are run forward through a fork of the ledger, so the laws judge the
//! future the way they judge the past. Variable spending, which nobody planned,
//! is bootstrapped from history into bands around that committed path.

mod bands;
mod expected;
mod recurrence;
mod trace;
mod variable;

use std::iter;

use axiom_core::day::days_in_month;
use axiom_core::{Day, Id, Map, Qty, Span};
use axiom_engine::{Checkpoint, Effect, Options, Planned, Recorded, Run, Violation};
use axiom_model::{Amount, Book, Contract, Flow, Law, Period, Subject};

use self::bands::{Bands, Share};
use self::expected::{Expectation, Origin, covered_by_contract, covered_on, expected};
use self::trace::Trace;
use self::variable::Variable;
use crate::calendar::Periods;
use crate::closings;
use crate::lens::{Lens, Whose};
use crate::places::{path, route};
use crate::table::{headline, plural};
use crate::{Cell, Column, Report, Row, Section, Style};

/// A fixed seed: the same books always give the same bands.
const SEED: u64 = 0x5EED_0A11_CE00_0001;

/// Bootstrapping needs a past to draw from.
const MIN_HISTORY_MONTHS: usize = 3;

/// What a forecast goes on from: the fold the run came from. The checkpoint paired with the run, standing on its day before
/// that day's closings, and the effects the run had recorded by then: the checkpoint forgets what happened, and a forecast
/// reads what it caused and these.
pub(crate) struct Past<'a> {
    pub at: &'a Checkpoint,
    pub effects: &'a [Effect],
}

/// The forecast of the lens's owners to `until` (default: a year ahead), with `paths` simulated futures around it.
pub(crate) fn view<'b, 's, 'p>(
    past: Past<'_>,
    run: &Run,
    lens: Lens<'b, 's, '_, 'p>,
    options: Options,
    until: Option<Day>,
    paths: u32,
) -> Report<'b> {
    let book = lens.book();
    let today = run.today;
    let until = until.unwrap_or_else(|| default_horizon(book, today)).max(today);

    let expected = expected(lens, run);
    let habits = habit_flows(book, &expected, today, until);
    let checkpoints = checkpoints(today, until);
    let trace = Trace::run(lens, &past, options, habits, &checkpoints);
    let (contract_rows, contract_issues) = contract_rows(lens, trace.ledger.recorded());

    let due = coming_due(&trace, past.effects, lens.whose, today);
    let committed = committed(lens, &checkpoints, &trace.liquid, &due);
    let variable = Variable::from_history(lens, run, |flow| {
        expected.iter().any(|expectation| expectation.covers(flow)) || covered_by_contract(book, flow)
    });
    let bands = simulate(&checkpoints, &committed, &variable, paths);

    let mut outlook = outlook_section(book, &checkpoints, &committed, &trace.worth, bands.as_ref());
    for note in outlook_notes(book, bands.as_ref(), variable.months, paths, contract_issues.len()) {
        outlook.note(note);
    }

    let mut report =
        Report::new(format!("Forecast to {until}")).with(outlook).with(expected_section(lens, &expected, today, until));
    if !book.contracts.is_empty() {
        report = report.with(contract_section(book, &contract_rows, &contract_issues));
    }
    report.with(owed_section(book, &due)).with(problems_section(book, &trace, today))
}

/// The flows the habits the journal shows expect after today, up to the horizon, in date order, less the ones a contract
/// already promises.
fn habit_flows(book: &Book, expected: &[Expectation], today: Day, until: Day) -> Vec<Flow> {
    let mut flows: Vec<_> = expected
        .iter()
        .flat_map(|expectation| expectation.flows(today, until))
        .filter(|flow| !covered_by_contract(book, flow))
        .collect();
    flows.sort_by_key(|flow| flow.day);
    flows
}

/// How the outlook was worked out, and that it is not whole if a contract could not be forecast.
fn outlook_notes(book: &Book, bands: Option<&Bands>, months: usize, paths: u32, issues: usize) -> Vec<String> {
    let mut notes = method_notes(book, bands, months, paths).to_vec();
    if issues > 0 {
        notes.push(format!(
            "Projection is incomplete: {issues} contract(s) were omitted. See Contract occurrences for details."
        ));
    }
    notes
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
/// A checkpoint deliberately forgets past records, so its paired run supplies
/// the exact pre-close prefix while the resumed ledger supplies closings and
/// flows from today onward.
fn coming_due<'a>(
    trace: &'a Trace<'_, '_, '_>,
    historical_effects: &'a [Effect],
    whose: &Whose,
    today: Day,
) -> Vec<&'a Effect> {
    let mut due = select_due_effects(historical_effects, trace.ledger.recorded().effects, whose, today);
    due.sort_by_key(|effect| effect.owed().map(|owed| owed.due));
    due
}

/// Select the exact historical prefix and resumed effects without guessing
/// record membership from dates or causes. The paired prefix ends before
/// today's closings.
fn select_due_effects<'a>(
    historical: &'a [Effect],
    resumed: &'a [Effect],
    whose: &Whose,
    today: Day,
) -> Vec<&'a Effect> {
    let belongs = |effect: &&Effect| whose.includes(effect.owner) && effect.owed().is_some_and(|owed| owed.due > today);
    historical.iter().chain(resumed).filter(belongs).collect()
}

/// The liquid position at each checkpoint, less obligations already due.
fn committed(lens: Lens, checkpoints: &[Day], liquid: &[Qty], due: &[&Effect]) -> Vec<Qty> {
    let lens = lens.on(checkpoints[0]);
    let owed_by = |day: Day| -> Qty {
        let paid = due.iter().filter(|effect| effect.owed().is_some_and(|owed| owed.due <= day));
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
    Some(bands::simulate(
        &standing,
        &shares,
        &variable.amounts,
        &variable.categories,
        variable.months,
        paths as usize,
        SEED,
    ))
}

// ─── Sections ───────────────────────────────────────────────────────────────

/// How the outlook was worked out, in plain words.
fn method_notes(book: &Book, bands: Option<&Bands>, history_months: usize, paths: u32) -> [String; 2] {
    let committed = format!(
        "Committed: money in hand, less what is owed on debts with no term and on obligations as they fall due, in {}, \
         after plans, recurring flows found in history and what they can still give. Investments and property are net \
         worth, not liquid. Variable spending is not in it.",
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

fn outlook_section<'s>(
    book: &'s Book<'_>,
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
fn expected_section<'s>(lens: Lens<'s, '_, '_, '_>, expected: &[Expectation], today: Day, until: Day) -> Section<'s> {
    let book = lens.book();
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
        let Some(main) = legs.iter().max_by_key(|leg| leg.out.qty.abs()) else {
            continue;
        };
        let flow = main.template;
        let payee = flow.payee.map(|entity| format!(" ({})", book.name(book.entities[entity].path)));
        let more = (legs.len() > 1).then(|| format!(", and {}", plural(legs.len() - 1, "more leg")));
        let what = format!("{}{}{}", route(book, flow), payee.unwrap_or_default(), more.unwrap_or_default());
        let total = legs
            .iter()
            .filter(|leg| leg.out.unit == main.out.unit)
            .map(|leg| crate::flow::scoped_movement_qty(lens, leg.template, leg.out.qty))
            .sum();
        let next = legs
            .iter()
            .flat_map(|leg| leg.schedule.days(today, until).filter(move |&day| !covered_on(book, leg.template, day)))
            .min();
        let source = match main.origin {
            Origin::Habit { occurrences } => format!("seen {occurrences} times"),
        };
        let cells = [
            Cell::text(what),
            Cell::text(recurrence::describe(main.schedule.every)),
            Cell::amount(book, Amount::new(total, main.out.unit)),
            next.map_or(Cell::Blank, Cell::Day),
            Cell::text(source),
        ];
        rows.push((next, Row::new(cells)));
    }
    // Soonest first.
    rows.sort_by_key(|&(next, _)| next);
    for (_, row) in rows {
        section.push(row);
    }
    if section.rows.is_empty() {
        if book.contracts.is_empty() {
            section.note("Nothing recurs yet. Keep the journal going until it shows a rhythm.");
        } else {
            section.note("No history-based rhythm recurs in this window. Contract occurrences are listed separately.");
        }
    }
    section
}

struct ContractRow {
    contract: Id<Contract>,
    every: String,
    what: String,
    next: Day,
    amount: Option<Amount>,
}

/// A row of "Contract occurrences" for each occurrence the ledger promised, and what could not be forecast: an occurrence
/// that could not be made, and the inputs an occurrence needs that nothing says. The promised occurrences are in the
/// order they fell due, which is the order of the rows.
fn contract_rows(lens: Lens, recorded: Recorded) -> (Vec<ContractRow>, Vec<(Id<Contract>, String)>) {
    let book = lens.book();
    let (mut rows, mut issues) = (Vec::new(), Vec::new());
    for planned in recorded.planned {
        let made = match planned.made {
            Ok(made) => made,
            Err(error) => {
                issues.push((planned.contract, format!("{error:?}")));
                continue;
            }
        };
        let flows = made.flows(recorded.promised_flows).unwrap_or_default();
        rows.push(contract_row(lens, planned, flows));
        let missing = made.missing(recorded.promised_inputs).unwrap_or_default();
        if !missing.is_empty() {
            let terms = book.contracts[planned.contract].terms_of(planned.schedule);
            let inputs =
                terms.into_iter().flat_map(|terms| missing.iter().filter_map(|&at| terms.inputs.get(at as usize)));
            let names = inputs.map(|input| book.name(input.name)).collect::<Vec<_>>().join(", ");
            issues.push((planned.contract, format!("required input(s) are missing: {names}")));
        }
    }
    (rows, issues)
}

/// One occurrence as a row: what it is (its biggest flow's route), how often, and what it comes to.
fn contract_row(lens: Lens, planned: &Planned, flows: &[axiom_model::RuntimeFlow]) -> ContractRow {
    let (book, contract) = (lens.book(), &lens.book().contracts[planned.contract]);
    let main = flows.iter().filter(|flow| flow.flow.day == planned.due).max_by_key(|flow| flow.flow.out.qty.abs());
    let amount = main.map(|main| {
        let same_unit =
            |flow: &&axiom_model::RuntimeFlow| flow.flow.day == planned.due && flow.flow.out.unit == main.flow.out.unit;
        let qty = flows
            .iter()
            .filter(same_unit)
            .map(|flow| crate::flow::scoped_movement_qty(lens, &flow.flow, flow.flow.out.qty))
            .sum();
        Amount::new(qty, main.flow.out.unit)
    });
    let what = main.map_or_else(|| book.name(contract.name).to_string(), |flow| route(book, &flow.flow));
    let every = contract
        .terms_of(planned.schedule)
        .map_or_else(|| "scheduled".to_string(), |terms| describe_contract(terms.every));
    ContractRow { contract: planned.contract, every, what, next: planned.due, amount }
}

fn describe_contract(every: axiom_model::Cadence) -> String {
    match every {
        axiom_model::Cadence::Every(span) => recurrence::describe(span).into_owned(),
        axiom_model::Cadence::TwiceMonthly => "twice monthly".to_string(),
    }
}

fn contract_section<'s>(
    book: &'s Book<'_>,
    contracts: &[ContractRow],
    issues: &[(Id<Contract>, String)],
) -> Section<'s> {
    let mut section =
        Section::new([Column::left("Contract"), Column::left("Every"), Column::right("Amount"), Column::left("Next")])
            .headed("Contract occurrences");
    for row in contracts {
        let amount = row.amount.map_or(Cell::Blank, |amount| Cell::amount(book, amount));
        section.push(Row::new([
            Cell::text(format!("{} · {}", book.name(book.contracts[row.contract].name), row.what)),
            Cell::text(row.every.clone()),
            amount,
            Cell::Day(row.next),
        ]));
    }
    for (id, error) in issues {
        section.note(format!("{} could not be forecast: {}", book.name(book.contracts[*id].name), error));
    }
    if !issues.is_empty() {
        section.note(format!(
            "{} contract occurrence(s) could not be fully projected; see the notes above.",
            issues.len()
        ));
    }
    if section.rows.is_empty() && issues.is_empty() {
        section.note("No active contract occurrence falls within this forecast window.");
    }
    section
}

fn owed_section<'s>(book: &'s Book<'_>, due: &[&Effect]) -> Section<'s> {
    let columns = [Column::left("Due"), Column::left("Obligation"), Column::left("To"), Column::right("Amount")];
    let mut section = Section::new(columns).headed("Obligations coming due");
    for effect in due {
        let Some(owed) = effect.owed() else { continue };
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
fn problems_section<'s>(book: &'s Book<'_>, trace: &Trace<'_, '_, '_>, today: Day) -> Section<'s> {
    let recorded = trace.ledger.recorded();
    let mut repeats: Map<(Id<Law>, Subject), (usize, &Violation)> = Map::default();
    for violation in recorded.violations.iter().filter(|violation| violation.day > today) {
        repeats.entry((violation.law, violation.subject)).or_insert((0, violation)).0 += 1;
    }
    let mut problems: Vec<(Day, String, Style)> = Vec::new();
    for (count, first) in repeats.into_values() {
        let message = &recorded.diagnostics[first.diagnostic as usize].message;
        let more = if count > 1 { format!(" (and {} more)", count - 1) } else { String::new() };
        let text = format!("{}: {}{more}", book.name(book.laws[first.law].name), headline(message));
        problems.push((first.day, text, if first.verdict.is_waived() { Style::Muted } else { Style::Alert }));
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

    #[test]
    fn resumed_forecast_keeps_historical_obligations_and_closes_today_once() {
        use crate::tests::household;
        use axiom_engine::{Cause, Consequence, Owed};
        use axiom_model::Subject;

        let mut house = household();
        let today = day(2026, 5, 15);
        let base = house.book.base;
        let name = house.book.names.intern("obligation");
        let effect = |when: Day, cause: Cause, due: Day, qty: i64| Effect {
            law: Id::new(0),
            subject: Subject::Entity(Id::new(0)),
            owner: Id::new(0),
            system: None,
            day: when,
            name,
            amount: Amount::new(Qty(qty), base),
            consequence: Consequence::Owe(Owed { to: Id::new(0), due }),
            cause,
        };
        let historical = [
            effect(today.add_days(-10), Cause::Flow(Id::new(0)), today.add_days(5), 1),
            effect(today, Cause::Flow(Id::new(1)), today.add_days(4), 2),
            // A same-day assertion posting before closing also has Cause::Time.
            effect(today, Cause::Time, today.add_days(3), 3),
            effect(today.add_days(-1), Cause::Flow(Id::new(2)), today, 4),
        ];
        let resumed = [
            // The pending closing comes from the resumed ledger, not the prefix.
            effect(today, Cause::Time, today.add_days(3), 5),
            effect(today, Cause::Flow(Id::new(3)), today.add_days(4), 6),
            effect(today.add_days(1), Cause::Time, today.add_days(8), 7),
        ];
        let due = select_due_effects(&historical, &resumed, &Whose::default(), today);
        assert_eq!(
            due.iter().map(|effect| effect.amount.qty.0).collect::<Vec<_>>(),
            [1, 2, 3, 5, 6, 7],
            "the exact prefix keeps same-day pre-close Time effects, excludes already-due items, and adds each resumed closing once"
        );

        let fresh_trace_effects = select_due_effects(&[], &historical, &Whose::default(), today);
        assert_eq!(
            fresh_trace_effects.iter().map(|effect| effect.amount.qty.0).collect::<Vec<_>>(),
            [1, 2, 3],
            "a forecast with no prefix uses only its trace and does not add a paired-run prefix"
        );
    }

    /// Two habits in a journal, one on the 5th and one on the 20th: each expects a flow every month, and what the report applies
    /// is all of them in date order, not one habit's and then the other's.
    #[test]
    fn the_flows_habits_expect_come_in_date_order() {
        let mut text =
            String::from("base USD\ncommodity USD\n  precision 2\nentity landlord\nentity gym\naccount checking\n");
        text += "opening 2025-10-01\n  checking 10_000.00 USD\n";
        for (year, month) in [(2025, 11), (2025, 12), (2026, 1), (2026, 2)] {
            text += &format!("{year}-{month:02}-05 checking -> landlord 500.00 USD\n");
            text += &format!("{year}-{month:02}-20 checking -> gym 30.00 USD\n");
        }
        crate::source_tests::with_run(&text, day(2026, 3, 1), |book, run| {
            let plan = axiom_engine::Plan::new(book);
            let whose = Whose::default();
            let lens = Lens::new(&plan, &whose, day(2026, 3, 1));
            let habits = expected(lens, run);
            let (today, until) = (day(2026, 3, 1), day(2026, 6, 30));
            let apart: Vec<_> =
                habits.iter().flat_map(|habit| habit.flows(today, until)).map(|flow| flow.day).collect();
            assert!(apart.windows(2).any(|pair| pair[0] > pair[1]), "one habit and then the other: {apart:?}");
            let together: Vec<_> = habit_flows(book, &habits, today, until).iter().map(|flow| flow.day).collect();
            assert_eq!(together.len(), apart.len());
            assert!(together.windows(2).all(|pair| pair[0] <= pair[1]), "in date order: {together:?}");
        });
    }

    const RENT: &str = "\
base USD
commodity USD
  precision 2
entity me
entity landlord
account checking : asset
opening 2026-01-01
  checking 1_000.00 USD
contract rent with landlord
  100.00 USD monthly on 15 from checking
  from 2026-01-15
  until 2026-06-30
";

    #[test]
    fn forecast_materializes_contracts_beside_a_run_whose_monitor_is_complete() {
        crate::source_tests::with_run(RENT, day(2026, 2, 1), |book, run| {
            assert!(run.monitor_complete);
            let report = crate::tests::report(
                book,
                run,
                &crate::Query::Forecast { until: Some(day(2026, 2, 28)), paths: 0 },
                None,
            )
            .unwrap();
            let occurrences = report
                .sections
                .iter()
                .find(|section| crate::tests::heading(section) == Some("Contract occurrences"))
                .unwrap();
            assert_eq!(occurrences.rows.len(), 1);
            assert!(matches!(
                (&occurrences.rows[0].cells[2], &occurrences.rows[0].cells[3]),
                (Cell::Amount { qty: Qty(10_000), .. }, Cell::Day(due)) if *due == day(2026, 2, 15)
            ));
            assert!(
                occurrences
                    .notes
                    .iter()
                    .all(|note| { !crate::tests::cell(note).contains("did not finish the native occurrence monitor") })
            );
        });
    }

    /// `--for` takes the contracts of one owner and leaves the others out, as the laws, the balances and the tallies of
    /// the forecast never see them.
    #[test]
    fn a_forecast_for_one_owner_promises_only_that_owners_contracts() {
        let text = "\
base USD
commodity USD
  precision 2
entity me
entity jordan
entity landlord
account assets/mine
  owner me
account assets/theirs
  owner jordan
opening 2026-01-01
  mine 1_000.00 USD
  theirs 1_000.00 USD
contract mine-rent with landlord
  100.00 USD monthly on 15 from mine
  from 2026-01-15
contract theirs-rent with landlord
  200.00 USD monthly on 15 from theirs
  from 2026-01-15
";
        crate::source_tests::with_run(text, day(2026, 2, 1), |book, run| {
            let rows = |whose| {
                let query = crate::Query::Forecast { until: Some(day(2026, 4, 30)), paths: 0 };
                let report = crate::tests::report(book, run, &query, whose).unwrap();
                let section = report
                    .sections
                    .iter()
                    .find(|section| crate::tests::heading(section) == Some("Contract occurrences"))
                    .unwrap();
                section.rows.iter().map(|row| crate::tests::cell(&row.cells[0])).collect::<Vec<_>>()
            };
            let mine = rows(Some("me"));
            assert_eq!(mine.len(), 3);
            assert!(mine.iter().all(|row| row.starts_with("mine-rent")), "{mine:?}");
            assert_eq!(rows(None).len(), 6);
        });
    }

    /// Full native materializer integration: the forecast includes every
    /// monthly occurrence and projects its typed amount into the same ledger.
    #[test]
    fn native_contract_occurrences_change_projection_and_keep_typed_amounts() {
        crate::source_tests::with_run(RENT, day(2026, 2, 1), |book, run| {
            let report = crate::tests::report(
                book,
                run,
                &crate::Query::Forecast { until: Some(day(2026, 4, 30)), paths: 0 },
                None,
            )
            .unwrap();
            let occurrences = report
                .sections
                .iter()
                .find(|section| crate::tests::heading(section) == Some("Contract occurrences"))
                .unwrap();
            let rent: Vec<_> = occurrences
                .rows
                .iter()
                .filter_map(|row| match (&row.cells[0], &row.cells[2], &row.cells[3]) {
                    (Cell::Text(title), Cell::Amount { qty, .. }, Cell::Day(due)) if title.contains("rent") => {
                        Some((*qty, *due))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                rent,
                [(Qty(10_000), day(2026, 2, 15)), (Qty(10_000), day(2026, 3, 15)), (Qty(10_000), day(2026, 4, 15)),]
            );
            let outlook = &report.sections[0].rows;
            let Cell::Amount { qty: ending, .. } = outlook.last().unwrap().cells[1] else {
                panic!("forecast end is a typed amount")
            };
            assert_eq!(ending, Qty(70_000));
        });
    }
}
