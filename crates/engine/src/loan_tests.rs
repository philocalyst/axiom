//! What a loan does in the fold: a payment is the split the schedule says, the debt tab falls by its principal, a promised
//! payment is the same split, a prepayment ends a loan and with it the payments the monitor waits for, and a statement of what is
//! owed is held to the schedule.
//!
//! The loan is the 36-month one of the model's tests: 120,000.00 at 6% from 2026-01-15, paid on the 1st from 2026-02-01, a level
//! payment of 3,650.63 whose first interest is 600.00 and first principal 3,050.63 (numbers of `docs/v5/measure/loans.py`).

use axiom_core::{Day, FileId};
use axiom_model::{Book, Source};
use axiom_syntax::Folder;

use crate::{Options, Plan, Run};

const STD: &str = include_str!("../../systems/src/std.ax");

const BOOK: &str = "\
use std
base USD
entity bank
account checking
asset condo : property
opening 2026-01-01
  checking 900_000 USD
contract home-loan with bank
  loan 120_000 USD on 2026-01-15 at 6% over 3y for condo
  monthly on 1 from checking
  from 2026-02-01
2026-01-15 home-loan
";

fn day(year: i32, month: u32, date: u32) -> Day {
    Day::from_ymd(year, month, date).unwrap()
}

fn with_book<R>(lines: &str, then: impl FnOnce(&Book) -> R) -> R {
    let text = format!("{BOOK}{lines}");
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
    then(&book)
}

fn with_run<R>(lines: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_book(lines, |book| then(book, &crate::run(book, Options { today, relaxed: false })))
}

/// What the debt tab of the loan holds, as owed.
fn owed(book: &Book, run: &Run) -> i64 {
    let debt = book.contracts[book.contract("home-loan").unwrap()].loan.unwrap().debt;
    -run.holdings.iter().filter(|holding| holding.place == debt).map(|holding| holding.qty().0).sum::<i64>()
}

fn checking(book: &Book, run: &Run) -> i64 {
    let place = book.place("checking").unwrap();
    run.holdings.iter().filter(|holding| holding.place == place).map(|holding| holding.qty().0).sum()
}

/// What the occurrence of `due` posted, by purpose name: `(interest, principal)`.
fn split(book: &Book, run: &Run, due: Day) -> (i64, i64) {
    let promise =
        run.promises.iter().find(|promise| promise.due == due && promise.kept.is_some()).expect("a kept payment");
    let purposed = |name: &str| -> i64 {
        let purpose = book.purpose(name).unwrap();
        run.promise_flows(promise)
            .iter()
            .filter(|runtime| runtime.flow.purpose.is_some_and(|purposed| purposed.purpose == purpose))
            .map(|runtime| runtime.flow.out.qty.0)
            .sum()
    };
    (purposed("interest"), purposed("principal"))
}

#[test]
fn a_kept_payment_posts_the_interest_to_the_lender_and_the_principal_to_the_debt_tab() {
    with_run("2026-02-01 home-loan\n2026-03-01 home-loan\n", day(2026, 3, 15), |book, run| {
        assert_eq!(split(book, run, day(2026, 2, 1)), (60_000, 305_063));
        assert_eq!(split(book, run, day(2026, 3, 1)), (58_475, 306_588));
        assert_eq!(
            owed(book, run),
            11_388_349,
            "what the schedule owes after the second payment: the debt tab fell by both principals"
        );
        // What the owner paid is the payment, twice, and nothing else: the origination put the principal into checking.
        assert_eq!(checking(book, run), 90_000_000 + 12_000_000 - 2 * 365_063);
    });
}

#[test]
fn the_interest_is_of_the_asset_the_loan_is_for_and_the_principal_is_a_transfer() {
    with_run("2026-02-01 home-loan\n", day(2026, 3, 15), |book, run| {
        let promise = run.promises.iter().find(|promise| promise.kept.is_some()).unwrap();
        let flows = run.promise_flows(promise);
        let named = |flow: &axiom_model::RuntimeFlow| {
            let purposed = flow.flow.purpose.unwrap();
            (book.name(book.purposes[purposed.purpose].name), purposed.of)
        };
        let mut said: Vec<_> = flows.iter().map(named).collect();
        said.sort_by_key(|(name, _)| *name);
        let condo = Some(axiom_model::journal::Object::Asset(book.asset("condo").unwrap()));
        assert_eq!(said, [("interest", condo), ("principal", None)]);
    });
}

#[test]
fn a_line_that_states_its_own_amount_pays_the_interest_first_and_never_makes_a_negative_principal() {
    // 02-01 states 1,000.00: less than the interest of 600.00 and the payment; 03-01 states 500.00 of a payment whose interest is 584.75.
    with_run("2026-02-01 home-loan 1_000 USD\n2026-03-01 home-loan 500 USD\n", day(2026, 3, 15), |book, run| {
        assert_eq!(split(book, run, day(2026, 2, 1)), (60_000, 40_000));
        assert_eq!(
            split(book, run, day(2026, 3, 1)),
            (50_000, 0),
            "the interest is held to what the line states, and the principal is nothing"
        );
    });
}

#[test]
fn a_line_that_states_more_than_the_payment_pays_the_difference_off_the_principal() {
    with_run("2026-02-01 home-loan 4_650.63 USD\n", day(2026, 2, 15), |book, run| {
        assert_eq!(split(book, run, day(2026, 2, 1)), (60_000, 405_063));
        assert_eq!(owed(book, run), 11_594_937, "the schedule took the thousand as a prepayment, and so does the tab");
    });
}

#[test]
fn a_promised_payment_is_the_same_split_and_a_prepayment_changes_what_is_promised() {
    let lines = "2026-02-01 home-loan\n2026-03-15 checking -> home-loan 10_000 USD\n";
    with_book(lines, |book| {
        let plan = Plan::new(book);
        let options = Options { today: day(2026, 3, 31), relaxed: false };
        let mut ledger = plan.start(options);
        ledger.advance(day(2026, 3, 31));
        ledger.reach(day(2026, 6, 30));
        ledger.promise(|_| true);
        ledger.advance(day(2026, 6, 30));
        let recorded = ledger.recorded();
        let promised: Vec<_> = recorded
            .planned
            .iter()
            .map(|planned| {
                let flows = planned.made.unwrap().flows(recorded.promised_flows).unwrap();
                let total: i64 = flows.iter().map(|runtime| runtime.flow.out.qty.0).sum();
                let interest = book.purpose("interest").unwrap();
                let paid = |flow: &&axiom_model::RuntimeFlow| flow.flow.purpose.is_some_and(|p| p.purpose == interest);
                (
                    planned.due.to_string(),
                    flows.iter().filter(paid).map(|runtime| runtime.flow.out.qty.0).sum::<i64>(),
                    total,
                )
            })
            .collect();
        // The payment stays 3,650.63; the interest of the 04-01 payment is of what is owed after the prepayment.
        assert_eq!(
            promised[..3],
            [
                ("2026-04-01".to_string(), 51_942, 365_063),
                ("2026-05-01".to_string(), 50_376, 365_063),
                ("2026-06-01".to_string(), 48_803, 365_063)
            ]
        );
    });
}

#[test]
fn a_loan_paid_off_by_a_prepayment_is_owed_nothing_and_its_payments_are_not_waited_for() {
    let lines = "2026-02-01 home-loan\n2026-03-01 home-loan\n2026-03-20 checking -> home-loan 113_883.49 USD\n";
    with_run(lines, day(2026, 8, 1), |book, run| {
        assert_eq!(owed(book, run), 0);
        let missed: Vec<_> = run.promises.iter().filter(|promise| promise.kept.is_none()).collect();
        assert!(missed.is_empty(), "nothing is due after the loan is paid off: {missed:?}");
        assert!(!run.diagnostics.iter().any(|diagnostic| diagnostic.code == "missed-occurrence"));
    });
}

/// The codes of what a book's statements make the fold say, and the first `loan-balance` text.
fn said(lines: &str, today: Day) -> (Vec<String>, Option<String>) {
    with_run(lines, today, |_, run| {
        let codes = run.diagnostics.iter().map(|diagnostic| diagnostic.code.to_string()).collect();
        let loan = run.diagnostics.iter().find(|diagnostic| diagnostic.code == "loan-balance");
        let text = loan.map(|diagnostic| {
            let notes: Vec<_> = diagnostic.notes.iter().map(String::as_str).collect();
            format!("{}\n{}", diagnostic.message, notes.join("\n"))
        });
        (codes, text)
    })
}

#[test]
fn a_statement_that_agrees_with_the_schedule_and_the_book_says_nothing() {
    let lines = "2026-02-01 home-loan\n2026-03-01 home-loan\n2026-03-31 home-loan = 113_883.49 USD\n";
    assert_eq!(said(lines, day(2026, 4, 1)).0, Vec::<String>::new());
}

#[test]
fn a_statement_where_a_payment_was_missed_is_held_to_the_schedule_and_names_the_payment() {
    // 03-01 was not kept: the tab is what the lender says (113,883.49 less the principal of that payment is what the book holds,
    // and the statement agrees with the book), and the schedule, which paid it, is the one that is not.
    let lines = "2026-02-01 home-loan\n2026-03-31 home-loan = 116_949.37 USD\n";
    let (codes, text) = said(lines, day(2026, 4, 1));
    assert!(codes.iter().any(|code| code == "loan-balance"), "{codes:?}");
    assert!(!codes.iter().any(|code| code == "assertion"), "the book agrees with the statement: {codes:?}");
    assert!(text.unwrap().contains("a payment was missed"));
}

#[test]
fn where_the_book_and_the_schedule_agree_and_the_statement_does_not_the_loan_says_it_once() {
    let lines = "2026-02-01 home-loan\n2026-02-28 home-loan = 115_000 USD\n";
    let (codes, text) = said(lines, day(2026, 4, 1));
    assert_eq!(codes.iter().filter(|code| *code == "loan-balance").count(), 1);
    assert_eq!(
        codes.iter().filter(|code| *code == "assertion").count(),
        0,
        "the loan's diagnostic is the one: {codes:?}"
    );
    assert!(text.unwrap().contains("a prepayment nobody wrote"));
}

#[test]
fn a_statement_that_accepts_its_gap_is_not_held_to_the_schedule() {
    let lines = "2026-02-01 home-loan\n2026-02-28 home-loan = 115_000 USD !\n";
    let (codes, _) = said(lines, day(2026, 4, 1));
    assert!(!codes.iter().any(|code| code == "loan-balance"), "{codes:?}");
}

#[test]
fn a_statement_is_held_to_the_schedule_in_the_loans_own_commodity_only() {
    let lines = "2026-02-28 home-loan = 1_000 EUR\n";
    assert!(!said(lines, day(2026, 4, 1)).0.iter().any(|code| code == "loan-balance"));
}
