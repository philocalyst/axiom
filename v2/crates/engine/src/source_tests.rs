//! Books written as source text, run through parse, model and engine.
//!
//! What the engine does with a flow depends on what the model made of the line
//! that wrote it (an entity as the source, a fee leg, a window a flow was
//! recognized for), so the behaviour that turns on both is tested from `.ax`
//! text to `Run`.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, FileId};
use axiom_model::{Book, Source};

use crate::{Holding, Options, Run};

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).unwrap()
}

/// Compiles `text` as a project of one file, which must have no errors.
fn with_book<R>(text: &str, then: impl FnOnce(&Book) -> R) -> R {
    let (file, parsed) = axiom_syntax::parse(FileId(0), text);
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    then(&book)
}

/// Folds `text` through `today`.
fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_book(text, |book| then(book, &crate::run(book, Options { today, relaxed: false })))
}

/// What `place` holds of `unit`, as the run ends.
fn holding<'r>(book: &Book, run: &'r Run, place: &str, unit: &str) -> Option<&'r Holding> {
    let (place, unit) = (book.place(place).unwrap(), book.commodity(unit).unwrap());
    run.holdings.iter().find(|holding| holding.place == place && holding.unit == unit)
}

// ─── Paying out of an envelope ──────────────────────────────────────────────

const ENVELOPES: &str = "\
base USD
commodity USD
  precision 2
kind envelope : entity
  restricted

account assets/checking
account assets/savings
account expenses/car-repair

entity trip-fund : envelope
  via assets/savings
entity car-fund : envelope
  via assets/savings

opening 2025-09-01
  checking 2_000 USD

2025-09-06 checking -> savings 500 USD for trip-fund
2025-09-07 checking -> savings 200 USD for car-fund
";

/// The parcels of `savings` that are tied, as `(entity, quantity)`.
fn tied(book: &Book, run: &Run) -> Vec<(String, i64)> {
    let lots = &holding(book, run, "savings", "USD").unwrap().lots;
    let name = |lot: &crate::Parcel| book.name(book.entities[lot.tied.unwrap()].path).to_string();
    lots.iter().map(|lot| (name(lot), lot.qty.0)).collect()
}

#[test]
fn a_payment_out_of_an_envelope_takes_that_envelopes_parcels() {
    let text = format!("{ENVELOPES}2025-10-01 car-fund -> car-repair 150 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00), ("car-fund".into(), 50_00)]);
    });
}

#[test]
fn an_envelope_that_runs_out_is_topped_up_from_what_is_not_tied() {
    let text = format!("{ENVELOPES}2025-09-20 checking -> savings 300 USD\n2025-10-01 car-fund -> car-repair 250 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00)]);
        let savings = holding(book, run, "savings", "USD").unwrap();
        assert_eq!(savings.qty().0, 500_00 + 250_00, "the last 50 came from the 300 that was not tied");
    });
}

#[test]
fn an_overspent_envelope_leaves_the_other_envelopes_alone() {
    let text = format!("{ENVELOPES}2025-10-01 car-fund -> car-repair 250 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00)]);
        assert_eq!(holding(book, run, "savings", "USD").unwrap().plain.0, -50_00, "the account owes 50 to nobody's money");
    });
}

#[test]
fn a_payment_written_out_of_an_envelope_is_judged_by_its_laws_not_dodged() {
    let text = "\
base USD
commodity USD
  precision 2
kind car-cost : expense
kind envelope : entity
  restricted
  has purpose kind
  law purpose
    on spend
    require to is self.purpose \"envelope money spent on something else\"

account assets/checking
account assets/savings
account expenses/dining
account expenses/car-repair : car-cost

entity car-fund : envelope
  via assets/savings
  purpose car-cost

opening 2025-09-01
  checking 2_000 USD

2025-09-07 checking -> savings 200 USD for car-fund
2025-09-08 checking -> savings 300 USD
2025-10-10 car-fund -> dining 20 USD
2025-10-20 car-fund -> car-repair 150 USD
";
    with_run(text, day(2025, 12, 31), |book, run| {
        let broken: Vec<_> =
            run.violations.iter().map(|v| run.diagnostics[v.diagnostic as usize].message.as_str()).collect();
        assert_eq!(broken.len(), 1, "only the dinner is outside the envelope's purpose: {broken:?}");
        let lots = &holding(book, run, "savings", "USD").unwrap().lots;
        assert_eq!(lots[0].qty.0, 30_00, "both payments came out of the 200 that was tied");
    });
}

// ─── Value recognized ahead of the window it belongs to ─────────────────────

const INSURANCE: &str = "\
base USD
commodity USD
  precision 2

account assets/checking
account expenses/insurance
  budget 1_200 USD yearly
account expenses/rent
  budget 100 USD monthly

opening 2025-09-01
  checking 5_000 USD
";

/// What the limits read in each window, as `(first day, counted)`, for one place's budget.
fn read(book: &Book, run: &Run, place: &str) -> Vec<(String, i64)> {
    let subject = axiom_model::Subject::Place(book.place(place).unwrap());
    let readings = run.headroom.iter().filter(|reading| reading.subject == subject);
    readings.map(|reading| (reading.from.to_string(), reading.counted.qty.0)).collect()
}

fn broken<'r>(book: &Book, run: &'r Run) -> Vec<&'r str> {
    let named = |v: &&crate::Violation| book.name(book.laws[v.law].name) == "budget";
    run.violations.iter().filter(named).map(|v| run.diagnostics[v.diagnostic as usize].message.as_str()).collect()
}

#[test]
fn a_flow_recognized_for_next_year_is_in_next_years_headroom_before_anything_else_lands_there() {
    let text = format!("{INSURANCE}2025-12-12 checking -> insurance 1_140 USD for 2026\n");
    with_run(&text, day(2026, 2, 14), |book, run| {
        let years = read(book, run, "insurance");
        assert_eq!(years, [("2025-01-01".into(), 0), ("2026-01-01".into(), 1_140_00)]);
        assert!(broken(book, run).is_empty());
    });
}

#[test]
fn an_accrual_that_alone_breaks_a_limit_is_reported_once_however_much_lands_after() {
    let text = format!(
        "{INSURANCE}2025-12-12 checking -> insurance 1_300 USD for 2026\n2026-01-05 checking -> insurance 50 USD\n"
    );
    with_run(&text, day(2026, 2, 14), |book, run| {
        let broken = broken(book, run);
        assert_eq!(broken.len(), 1, "{broken:?}");
        assert!(broken[0].contains("1,300.00 USD in 2026 against a limit of 1,200.00 USD, over by 100.00 USD"));
        assert_eq!(read(book, run, "insurance").last().unwrap().1, 1_350_00, "and the reading goes on counting");
    });
}

#[test]
fn a_range_reaches_each_month_it_covers_up_to_where_the_fold_has_got() {
    // 121 days from December to March: 31, 31, 28 and 31 of them.
    let text = format!("{INSURANCE}2025-12-01..2026-03-31 checking -> rent 1_200 USD\n");
    with_run(&text, day(2026, 1, 31), |book, run| {
        let months = read(book, run, "rent");
        assert_eq!(months, [("2025-12-01".into(), 307_44), ("2026-01-01".into(), 307_44)], "February is not here yet");
        assert_eq!(broken(book, run).len(), 2);
    });
    with_run(&text, day(2026, 6, 30), |book, run| {
        let months = read(book, run, "rent");
        let counted: Vec<_> = months.iter().map(|(_, counted)| *counted).collect();
        assert_eq!(counted, [307_44, 307_44, 277_68, 307_44], "the shares add up to the 1,200.00 USD");
        assert_eq!(broken(book, run).len(), 4);
    });
}

#[test]
fn a_planned_flow_reaches_the_months_ahead_as_its_ledger_advances() {
    let text = format!("{INSURANCE}2025-12-01 checking -> rent 50 USD\n");
    with_book(&text, |book| {
        let mut ledger = crate::Ledger::new(book, Options { today: day(2025, 12, 31), relaxed: false });
        ledger.advance(day(2025, 12, 31));
        // The rent flow again, 81 days from the tenth of January: 22, 28 and 31 of them.
        let mut prepaid = book.flows[axiom_core::Id::new(1)].clone();
        prepaid.day = day(2026, 1, 10);
        prepaid.recognized = axiom_model::Recognition { from: day(2026, 1, 10), until: day(2026, 3, 31) };
        prepaid.out.qty = axiom_core::Qty(1_200_00);
        prepaid.arrive = prepaid.out;
        let mut fork = ledger.fork();
        fork.apply(&prepaid);
        assert_eq!(fork.recorded().violations.len(), 1, "January, where the flow lands");
        fork.advance(day(2026, 3, 31));
        assert_eq!(fork.recorded().violations.len(), 3, "and February and March as the fork gets there");
    });
}
