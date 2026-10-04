//! A loan made before the book began, in the fold: its debt is open with what its terms say, so the payments the book keeps take
//! it down from there, a statement of what is owed agrees with the tab and the schedule, and a book that says nothing else is
//! begun by its loan and waits for every payment since.
//!
//! The loan is `crates/model/tests/loan_opening.rs`'s: 120,000.00 at 6% made 2025-01-15, paid on the 1st from 2025-02-01. After
//! the 11 payments before 2026-01-01 85,591.44 is owed; the 12th, the book's first, is 428.96 of interest and 3,221.67 of
//! principal (numbers of `docs/v5/measure/loans.py`).

use axiom_core::{Day, FileId};
use axiom_model::{Book, Source};
use axiom_syntax::Folder;

use crate::loan_tests::split;
use crate::{Options, Run};

const STD: &str = include_str!("../../systems/src/std.ax");

const LOAN: &str = "\
use std
base USD
entity bank
account checking
asset condo : property
contract home-loan with bank
  loan 120_000 USD on 2025-01-15 at 6% over 3y for condo
  monthly on 1 from checking
  from 2025-02-01
";

const OPENING: &str = "opening 2026-01-01\n  checking 900_000 USD\n";

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let text = format!("{LOAN}{text}");
    let sources: Vec<_> = [("std.ax", STD, true), ("main.ax", text.as_str(), false)]
        .into_iter()
        .enumerate()
        .map(|(index, (path, text, embedded))| {
            let (file, parsed) = axiom_syntax::parse(FileId(index as u16), text, Folder::of(path));
            assert!(parsed.is_empty(), "{path} does not parse: {parsed:?}");
            Source { path, file, embedded }
        })
        .collect();
    let (book, built) = axiom_model::build(&sources);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    then(&book, &crate::run(&book, Options { today, relaxed: false }))
}

/// What the debt tab of the loan holds, as owed.
fn owed(book: &Book, run: &Run) -> i64 {
    let debt = book.contracts[book.contract("home-loan").unwrap()].loan.unwrap().debt;
    -run.holdings.iter().filter(|holding| holding.place == debt).map(|holding| holding.qty().0).sum::<i64>()
}

fn said(run: &Run, code: &str) -> Vec<String> {
    run.diagnostics.iter().filter(|d| &*d.code == code).map(|d| d.message.clone()).collect()
}

#[test]
fn a_kept_payment_takes_the_debt_down_from_what_the_terms_said_when_the_book_began() {
    let book_text = format!("{OPENING}2026-01-01 home-loan\n");
    with_run(&book_text, day(2026, 1, 15), |book, run| {
        assert_eq!(split(book, run, day(2026, 1, 1)), (42_796, 322_267), "the 12th payment of the schedule");
        assert_eq!(owed(book, run), 8_236_877, "85,591.44 less the principal of the payment the book kept");
    });
}

#[test]
fn the_debt_is_the_same_whether_the_book_begins_with_the_opening_or_with_the_payment() {
    // The payment of 2026-01-01 is the first record; the checking is held from the day after. The opening of the debt follows the
    // payment in the fold (it is lowered after the record that begins the book), and a day's balance does not depend on the
    // order of its flows.
    let begins_with_the_payment = "2026-01-01 home-loan\nopening 2026-01-02\n  checking 900_000 USD\n";
    with_run(begins_with_the_payment, day(2026, 1, 15), |book, run| {
        assert_eq!(book.first_fact(), Some(day(2026, 1, 1)));
        assert_eq!(owed(book, run), 8_236_877);
    });
}

#[test]
fn a_statement_agrees_with_the_tab_and_with_the_schedule_and_the_book_says_nothing() {
    let book_text = format!("{OPENING}2026-01-01 home-loan\n2026-01-31 home-loan = 82_368.77 USD\n");
    with_run(&book_text, day(2026, 2, 15), |_, run| {
        assert!(said(run, "assertion").is_empty(), "{:?}", said(run, "assertion"));
        assert!(said(run, "loan-balance").is_empty(), "{:?}", said(run, "loan-balance"));
    });
}

#[test]
fn a_statement_of_the_lenders_number_is_held_to_the_schedule_that_opened_the_debt() {
    // The lender says 100.00 more is owed than the schedule does, and no payment of the book was missed: it is not a cause the
    // schedule can name, and the tab, which the schedule opened, is exactly as far from the statement.
    let book_text = format!("{OPENING}2026-01-01 home-loan\n2026-01-31 home-loan = 82_468.77 USD\n");
    with_run(&book_text, day(2026, 2, 15), |_, run| {
        assert_eq!(said(run, "loan-balance").len(), 1, "{:?}", run.diagnostics);
    });
}

#[test]
fn a_book_that_says_nothing_but_a_loan_is_begun_by_it_and_waits_for_every_payment_since() {
    with_run("", day(2026, 6, 30), |book, run| {
        assert_eq!(book.first_fact(), Some(day(2025, 1, 15)));
        let missed = said(run, "missed-occurrence");
        assert_eq!(missed.len(), 1, "one warning for the loan, not one for each payment: {missed:?}");
        assert!(missed[0].contains("home-loan"), "{missed:?}");
        assert_eq!(owed(book, run), 12_000_000, "all of it is owed: no payment has been kept");
    });
}
