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

/// Compiles `text` as a project of one file and folds it through `today`.
/// The book must have no errors.
fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let (file, parsed) = axiom_syntax::parse(FileId(0), text);
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    let run = crate::run(&book, Options { today, relaxed: false });
    then(&book, &run)
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
