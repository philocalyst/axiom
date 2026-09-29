//! Views over books written as source text, run through the whole pipeline.
//!
//! What a view should say depends on what the engine made of a flow, so the
//! behaviour that turns on it is tested from `.ax` text to report.

use axiom_core::{Day, FileId};
use axiom_engine::{Options, Run};
use axiom_model::{Book, Source};

use crate::tests::lines;
use crate::Query;

fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

/// Compiles `text` as a project of one file, runs it through `today`, and
/// hands the book and the run to `then`. The book must have no errors.
fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let (file, parsed) = axiom_syntax::parse(FileId(0), text);
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    let run = axiom_engine::run(&book, Options { today, relaxed: false });
    then(&book, &run)
}

/// The rows of the first section of the report `query` asks for.
fn rows(book: &Book, run: &Run, query: Query) -> Vec<String> {
    lines(&crate::report(book, run, &query, None).expect("the query resolves").sections[0])
}

// ─── Basis flows ────────────────────────────────────────────────────────────

/// An improvement to a holding: 100 USD paid into the basis of ten shares.
const DISCOUNT: &str = "\
base USD
commodity USD
  precision 2
commodity UNH

account assets/broker
account assets/checking
account income/discount

opening 2025-01-01
  broker    10 UNH basis 1_000 USD since 2024-01-01
  checking  5_000 USD

2025-01-01 UNH 150 USD
2025-02-01 income/discount -> broker[2024-01-01].basis 100 USD
";

/// What the shares fetch is their price, whatever their basis: the 100 USD
/// changed what they cost, and nothing arrived in the account.
#[test]
fn a_basis_flow_is_worth_nothing_to_the_place_it_rebases() {
    with_run(DISCOUNT, day(2025, 6, 1), |book, run| {
        let balance = Query::Balance { globs: vec![], at: None, value: true, monthly: false };
        let report = crate::report(book, run, &balance, None).unwrap();
        let worth = lines(&report.sections[1]);
        assert_eq!(worth[2], "=Net worth | 6,500.00 USD", "1,500 of shares and 5,000 of cash");
        let broker = lines(&report.sections[0]).into_iter().find(|row| row.starts_with("  broker")).unwrap();
        assert_eq!(broker, "  broker | 1,500.00 USD");
    });
}

#[test]
fn a_register_lists_a_basis_flow_as_a_change_of_basis_not_an_amount() {
    with_run(DISCOUNT, day(2025, 6, 1), |book, run| {
        let register = Query::Register { place: "broker", from: None, to: None };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 10 UNH | 10 UNH",
                "2025-02-01 | income/discount |  | basis +100.00 USD |  |",
            ]
        );
    });
}

#[test]
fn a_register_lists_an_exchange_inside_one_place_at_both_of_its_ends() {
    let source = "\
base USD
commodity USD
  precision 2
commodity GBP
  precision 2

account assets/wise

opening 2025-01-01
  wise 1_000 USD

2025-02-01 wise 500 USD -> wise 400 GBP
";
    with_run(source, day(2025, 6, 1), |book, run| {
        let register = Query::Register { place: "wise", from: None, to: None };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 1,000.00 USD | 1,000.00 USD",
                "2025-02-01 | assets/wise |  |  | -500.00 USD | 500.00 USD",
                "2025-02-01 | assets/wise |  |  | 400.00 GBP | 400.00 GBP",
            ]
        );
    });
}

/// Depreciation lowers a house's basis and recognizes an expense, and the
/// house still holds its one HOME. A later flow makes the balance replay the
/// journal instead of reading the run's holdings.
const DEPRECIATION: &str = "\
base USD
commodity USD
  precision 2
commodity HOME
  precision 0

account assets/house
account assets/checking
account expenses/depreciation

opening 2025-01-01
  checking 101_000 USD

2025-01-01 checking -> house 1 HOME @ 100_000 USD
2025-02-01 house.basis -> expenses/depreciation 300 USD
2026-05-01 checking -> expenses/depreciation 1 USD
";

#[test]
fn a_balance_replaying_the_journal_books_no_money_out_of_a_place_that_lost_basis() {
    with_run(DEPRECIATION, day(2026, 4, 16), |book, run| {
        let balance = Query::Balance { globs: vec![], at: None, value: false, monthly: false };
        assert_eq!(
            rows(book, run, balance),
            [
                "=assets | 1,000.00 USD",
                "= | 1 HOME",
                "  checking | 1,000.00 USD",
                "  house | 1 HOME",
                "=equity | 101,000.00 USD",
                "  opening | 101,000.00 USD",
                "=expenses | 300.00 USD",
                "  depreciation | 300.00 USD",
            ]
        );
    });
}
