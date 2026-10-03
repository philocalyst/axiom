//! When a claim counts as income or spending: LANGUAGE §7, written as source and run.
//!
//! The language says: "A claim's purpose is its recognition: an invoice is income when invoiced in accrual books, when
//! settled in cash books (the owner's `books cash|accrual`). A claim `waived` is forgiven; in accrual books what was recognized
//! is reversed." What a purpose counts is read two ways here: by a law of the purpose that counts each flow's amount
//! (`receipts`: what the law saw of each flow it fired on) and by one that counts the purpose's running total (`running`: what
//! the purpose's total was after the flow, which a reversal lowers). Each test says on which days, and of how much, they
//! counted.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_model::Book;

use crate::Run;
use crate::source_tests::{day, with_run};

/// The owner's books, a purpose whose laws count what it is recognized for, and a second purpose for what a payment says of
/// itself, in the owner's `books`.
fn prelude(books: &str) -> String {
    format!(
        "\
base USD
commodity USD
  precision 2
kind receivable : asset
  claim
purpose design : income
  law receipts
    on flow
    count amount as receipts
  law running
    on flow
    count total(ever) as running
purpose retail : income
  law retail
    on flow
    count amount as retail-receipts
purpose fees : spending
  law fees
    on flow
    count amount as fee-expenses
account checking
account owed : receivable
entity me
  books {books}
entity ann
entity stripe
opening 2026-01-01
  checking 1_000 USD
"
    )
}

/// What a tally counted, by day and quantity in quanta.
fn counted(book: &Book, run: &Run, name: &str) -> Vec<(String, i64)> {
    let mut found: Vec<_> = run
        .effects
        .iter()
        .filter(|effect| book.name(effect.name) == name)
        .map(|effect| (effect.day.to_string(), effect.amount.qty.0))
        .collect();
    found.sort();
    found
}

fn on(day: &str, qty: i64) -> (String, i64) {
    (day.to_string(), qty)
}

/// Folds the prelude of `books` and `lines` through the first of April.
fn with<R>(books: &str, lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_run(&format!("{}{lines}", prelude(books)), day(2026, 4, 1), then)
}

const INVOICE: &str = "2026-01-02 ann owes me 300 USD due 2026-02-01 #design ^i1\n";

// ─── Cash books: a claim counts when it is settled ──────────────────────────

#[test]
fn a_claim_made_with_a_purpose_counts_nothing_in_cash_books() {
    with("cash", INVOICE, |book, run| {
        assert_eq!(counted(book, run, "receipts"), []);
        assert_eq!(counted(book, run, "running"), []);
    });
}

#[test]
fn its_payment_counts_the_claims_purpose_on_the_day_it_settles_it() {
    let lines = format!("{INVOICE}2026-01-20 ann -> checking 300 USD ^i1\n");
    with("cash", &lines, |book, run| {
        assert_eq!(counted(book, run, "receipts"), [on("2026-01-20", 300_00)]);
        assert_eq!(counted(book, run, "running"), [on("2026-01-20", 300_00)]);
    });
}

#[test]
fn a_payment_that_says_the_claims_purpose_is_counted_once() {
    let lines = format!("{INVOICE}2026-01-20 ann -> checking 300 USD #design ^i1\n");
    with("cash", &lines, |book, run| {
        assert_eq!(
            counted(book, run, "receipts"),
            [on("2026-01-20", 300_00)],
            "not 600.00: the payment is the claim's value"
        );
    });
}

#[test]
fn what_a_payment_does_not_settle_counts_as_the_payment_says() {
    let lines = format!("{INVOICE}2026-01-20 ann -> checking 500 USD #retail\n");
    with("cash", &lines, |book, run| {
        assert_eq!(counted(book, run, "receipts"), [on("2026-01-20", 300_00)], "the claim's 300.00");
        assert_eq!(counted(book, run, "retail-receipts"), [on("2026-01-20", 200_00)], "and the payment's own 200.00");
    });
}

#[test]
fn each_purpose_of_an_itemized_claim_counts_what_its_line_was() {
    let lines = "\
2026-01-02 ann owes me due 2026-02-01 ^i1
  3_000 USD #design
  800 USD #retail
2026-01-20 ann -> checking 3_800 USD ^i1
";
    with("cash", lines, |book, run| {
        assert_eq!(counted(book, run, "receipts"), [on("2026-01-20", 3_000_00)]);
        assert_eq!(counted(book, run, "retail-receipts"), [on("2026-01-20", 800_00)]);
    });
}

#[test]
fn a_flow_out_of_a_claim_place_settles_it_too() {
    let lines = "\
2026-01-02 ann -> owed 300 USD due 2026-02-01 #design ^i1
2026-01-20 owed[^i1] -> checking 300 USD
";
    with("cash", lines, |book, run| assert_eq!(counted(book, run, "receipts"), [on("2026-01-20", 300_00)]));
    with("accrual", lines, |book, run| assert_eq!(counted(book, run, "receipts"), [on("2026-01-02", 300_00)]));
}

#[test]
fn a_claim_with_no_purpose_has_no_recognition_to_wait_for() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-20 ann -> checking 300 USD #design ^i1
";
    for books in ["cash", "accrual"] {
        with(books, lines, |book, run| {
            assert_eq!(
                counted(book, run, "receipts"),
                [on("2026-01-20", 300_00)],
                "{books}: the payment counts as it says"
            );
        });
    }
}

// ─── Accrual books: a claim counts when it is made ──────────────────────────

#[test]
fn a_claim_counts_when_it_is_made_in_accrual_books_and_its_payment_counts_nothing() {
    let lines = format!("{INVOICE}2026-01-20 ann -> checking 300 USD #design ^i1\n");
    with("accrual", &lines, |book, run| {
        assert_eq!(counted(book, run, "receipts"), [on("2026-01-02", 300_00)]);
        assert_eq!(counted(book, run, "running"), [on("2026-01-02", 300_00)]);
    });
}

#[test]
fn a_write_off_reverses_in_accrual_books_what_the_claim_recognized() {
    let lines = format!(
        "{INVOICE}2026-01-20 ann -> checking 100 USD ^i1\n2026-02-15 ^i1 waived \"not collected\"\n2026-03-01 ann -> checking 50 USD #design\n"
    );
    // 300.00 recognized when made, 200.00 forgiven: the 100.00 that was collected, and then 50.00 more.
    with("accrual", &lines, |book, run| {
        assert_eq!(counted(book, run, "running"), [on("2026-01-02", 300_00), on("2026-03-01", 150_00)]);
    });
    // Nothing was recognized of the forgiven part in cash books, so there is nothing to take back.
    with("cash", &lines, |book, run| {
        assert_eq!(counted(book, run, "running"), [on("2026-01-20", 100_00), on("2026-03-01", 150_00)]);
    });
}

// ─── A payment that is returned, and a leg that is the owner's cost ─────────

#[test]
fn a_payment_that_is_returned_takes_back_what_it_counted_in_cash_books() {
    let lines = format!(
        "{INVOICE}2026-01-20 ann -> checking 300 USD ^pay-1\n2026-01-25 ^pay-1 returned\n2026-02-01 ann -> checking 50 USD #design\n"
    );
    // The purpose's total was 300.00 when the payment settled the claim, and 0.00 when it bounced (a law that counts zero
    // records nothing), so what the next flow finds is its own 50.00.
    with("cash", &lines, |book, run| {
        assert_eq!(counted(book, run, "running"), [on("2026-01-20", 300_00), on("2026-02-01", 50_00)]);
    });
}

#[test]
fn the_fee_leg_counts_its_own_purpose_and_the_claim_its_own() {
    let lines = "\
2026-01-02 ann owes me 3_100 USD due 2026-02-01 #design ^i1
2026-01-20 ann -> 3_100 USD #design ^i1
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    with("cash", lines, |book, run| {
        let mut receipts = counted(book, run, "receipts");
        receipts.sort();
        assert_eq!(
            receipts,
            [on("2026-01-20", 90_20), on("2026-01-20", 3_009_80)],
            "the invoice, in the two parts it was paid"
        );
        assert_eq!(counted(book, run, "fee-expenses"), [on("2026-01-20", 90_20)], "the fee is the owner's cost, whole");
    });
    with("accrual", lines, |book, run| {
        assert_eq!(counted(book, run, "receipts"), [on("2026-01-02", 3_100_00)], "the invoice when it was made");
        assert_eq!(counted(book, run, "fee-expenses"), [on("2026-01-20", 90_20)], "the fee is the owner's cost, whole");
    });
}
