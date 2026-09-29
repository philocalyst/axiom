//! Views over books written as source text, run through the whole pipeline.
//!
//! What a view should say depends on what the engine made of a flow, so the
//! behaviour that turns on it is tested from `.ax` text to report.

use axiom_core::{Day, FileId};
use axiom_engine::{Options, Run};
use axiom_model::{Book, Source};

use crate::tests::{lines, show};
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

// ─── Laws ───────────────────────────────────────────────────────────────────

#[test]
fn why_a_law_says_the_day_a_closing_law_judges_the_year() {
    let source = "\
base USD
commodity USD
  precision 2

account assets/checking

/// Figures the year's return.
law return
  each year closing 04-15
  count 1 USD as returns

law audit
  each year
  count 1 USD as audits
";
    with_run(source, day(2026, 6, 1), |book, run| {
        let when = |law| {
            let report = crate::report(book, run, &Query::Why { target: law }, None).unwrap();
            lines(&report.sections[0]).into_iter().find(|row| row.starts_with("When")).unwrap()
        };
        assert_eq!(when("return"), "When | each year closing 04-15");
        assert_eq!(when("audit"), "When | each year");
    });
}

// ─── Taxes before the return closes ─────────────────────────────────────────

/// A tally counted as the year goes, and a tax figured from it on April 15 of
/// the next. The last flow is in 2027, so the journal itself reaches into the
/// year after the one taxed.
const RETURN: &str = "\
base USD
commodity USD
  precision 2

entity treasury

account assets/checking
account income/salary

law count-pay
  on in
  when to is assets/checking
  count amount as pay

law return
  each year closing 04-15
  owe tally(pay) * 10% to treasury as income-tax

2026-03-01 income/salary -> checking 1_000 USD
2026-09-01 income/salary -> checking 1_000 USD
2027-01-05 income/salary -> checking 500 USD
";

#[test]
fn tax_says_the_return_is_not_closed_and_leaves_what_it_owes_out_instead_of_at_zero() {
    with_run(RETURN, day(2027, 3, 1), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [counted, owed] = &report.sections[..] else { panic!("two sections: {}", show(&report)) };
        assert_eq!(lines(counted), ["=project |  |", "  pay | 2,000.00 USD | 2 sources"]);
        assert!(owed.rows.is_empty(), "no line of the return is figured yet");
        assert_eq!(
            owed.notes,
            ["The 2026 return closes on 2027-04-15; what it owes is not figured yet; the tallies are counted so far."]
        );
    });
}

#[test]
fn tax_after_the_return_closes_shows_what_it_owes() {
    with_run(RETURN, day(2027, 4, 20), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [_, owed] = &report.sections[..] else { panic!("two sections: {}", show(&report)) };
        assert_eq!(lines(owed)[1], "  income-tax | treasury | 2027-04-15 | 200.00 USD | period end");
        assert!(owed.notes.iter().all(|note| !note.contains("not figured")), "{:?}", owed.notes);
    });
}

/// One of two returns has closed: what it owes is listed, and the total is what
/// is owed so far.
#[test]
fn tax_with_one_return_closed_and_one_not_totals_what_is_owed_so_far() {
    let state = "\nlaw state-return\n  each year closing 06-15\n  owe tally(pay) * 5% to treasury as state-tax\n";
    with_run(&format!("{RETURN}{state}"), day(2027, 5, 1), |book, run| {
        let tax = Query::Tax { year: Some(2026) };
        let report = crate::report(book, run, &tax, None).unwrap();
        let [_, owed] = &report.sections[..] else { panic!("two sections: {}", show(&report)) };
        assert_eq!(
            lines(owed),
            [
                "=project |  |  |  |",
                "  income-tax | treasury | 2027-04-15 | 200.00 USD | period end",
                "=Total owed so far |  |  | 200.00 USD |",
            ]
        );
        assert_eq!(owed.notes[0], "The 2026 return closes on 2027-06-15; what it owes is not figured yet; the tallies are counted so far.");
    });
}

// ─── Accepted gaps ──────────────────────────────────────────────────────────

/// One gap of each kind: a revaluation, a gap accepted as unexplained, and a
/// gap that came out of another account.
const GAPS: &str = "\
base USD
commodity USD
  precision 2

account assets/k
account assets/checking

opening 2025-01-01
  k        10_000 USD
  checking  5_000 USD

2025-03-31 k = 9_000 USD via market
2025-06-30 k = 9_500 USD !
2025-09-30 k = 9_800 USD via checking
";

#[test]
fn a_register_says_where_each_gap_came_from() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let register = Query::Register { place: "k", from: None, to: None };
        assert_eq!(
            rows(book, run, register),
            [
                "2025-01-01 | equity/opening |  |  | 10,000.00 USD | 10,000.00 USD",
                "2025-03-31 | income/market |  | revalued via income/market | -1,000.00 USD | 9,000.00 USD",
                "2025-06-30 | equity/unknown |  | unexplained gap, accepted with ! | 500.00 USD | 9,500.00 USD",
                "2025-09-30 | assets/checking |  | gap via assets/checking | 300.00 USD | 9,800.00 USD",
            ]
        );
    });
}

#[test]
fn the_line_of_an_assertion_says_where_its_gap_came_from() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let words: Vec<String> = book
            .asserts
            .iter()
            .map(|assertion| {
                let line = Query::Line { loc: assertion.loc };
                lines(&crate::report(book, run, &line, None).unwrap().sections[0])[0].clone()
            })
            .collect();
        assert!(words[0].starts_with("assertion: assets/k = 9,000.00 USD, revalued via income/market"), "{words:?}");
        assert!(words[1].starts_with("assertion: assets/k = 9,500.00 USD, unexplained gap, accepted with !"));
        assert!(words[2].starts_with("assertion: assets/k = 9,800.00 USD, gap via assets/checking"));
    });
}

/// The gap is a flow from its counter place, so that place's register lists it too.
#[test]
fn the_register_of_a_gaps_counter_place_lists_it_as_well() {
    with_run(GAPS, day(2025, 12, 31), |book, run| {
        let register = |place| Query::Register { place, from: None, to: None };
        assert_eq!(
            rows(book, run, register("checking")).last().unwrap(),
            "2025-09-30 | assets/k |  | gap via assets/checking | -300.00 USD | 4,700.00 USD"
        );
        assert_eq!(
            rows(book, run, register("market")),
            ["2025-03-31 | assets/k |  | revalued via income/market | -1,000.00 USD | -1,000.00 USD"]
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
