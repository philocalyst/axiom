//! What the reports say a claim is worth to a purpose, in cash and in accrual books: LANGUAGE §7.
//!
//! The fold counts what a purpose is worth by `recognition`'s rule, and `flow`, `flow --by party` and `why #purpose` ask
//! the same rule of the run, so each of them says what a law or a limit of the purpose counted. A claim of 300.00 USD for
//! `#design` made in January, settled by 100.00 in February, and the rest forgiven in March, is in cash books January
//! nothing, February 100.00 and March nothing, and in accrual books January 300.00, February nothing and March minus
//! 200.00 (the part that was forgiven, taken back).
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::Day;
use axiom_model::Book;

use crate::source_tests::with_run;
use crate::tests::lines;
use crate::{FlowBy, Query};

fn day(y: i32, m: u32, d: u32) -> Day {
    Day::from_ymd(y, m, d).unwrap()
}

fn book(books: &str, lines: &str) -> String {
    format!(
        "\
base USD
commodity USD
  precision 2
kind receivable : asset
  claim
purpose design : income
purpose fees : spending
account checking
entity me
  books {books}
entity ann
entity stripe
opening 2026-01-01
  checking 1_000 USD
{lines}"
    )
}

/// The rows of the `flow` report by month, from January to March.
fn flow(book: &Book, run: &axiom_engine::Run, by: FlowBy) -> Vec<String> {
    let report = crate::report(book, run, &Query::Flow { by, from: Some(day(2026, 1, 1)), to: None }, None).unwrap();
    lines(&report.sections[0])
}

const CLAIM: &str = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 #design ^i1
2026-02-10 ann -> checking 100 USD ^i1
2026-03-15 ^i1 waived \"not collected\"
";

#[test]
fn flow_counts_a_claim_when_it_is_settled_in_cash_books() {
    with_run(&book("cash", CLAIM), day(2026, 3, 31), |book, run| {
        let rows = flow(book, run, FlowBy::Period(axiom_model::Period::Month));
        assert_eq!(rows[0], "=income |  | 100.00 USD |  | 100.00 USD", "{rows:#?}");
        assert_eq!(rows[1], "  design |  | 100.00 USD |  | 100.00 USD");
    });
}

#[test]
fn flow_counts_a_claim_when_it_is_made_in_accrual_books_and_takes_back_what_was_forgiven() {
    with_run(&book("accrual", CLAIM), day(2026, 3, 31), |book, run| {
        let rows = flow(book, run, FlowBy::Period(axiom_model::Period::Month));
        assert_eq!(rows[0], "=income | 300.00 USD |  | -200.00 USD | 100.00 USD", "{rows:#?}");
        assert_eq!(rows[1], "  design | 300.00 USD |  | -200.00 USD | 100.00 USD");
    });
}

/// By party the claim is the party's: the 100.00 that settled it, and nobody else's.
#[test]
fn flow_by_party_counts_what_a_party_settled_as_the_claims_purpose() {
    with_run(&book("cash", CLAIM), day(2026, 3, 31), |book, run| {
        let rows = flow(book, run, FlowBy::Party);
        assert_eq!(rows[0], "=Income |  | 100.00 USD |  | 100.00 USD", "{rows:#?}");
        assert_eq!(rows[1], "  ann |  | 100.00 USD |  | 100.00 USD");
    });
}

/// The payment net of the fee: the invoice is income whole, and the fee the processor kept is the owner's cost.
#[test]
fn a_payment_net_of_a_fee_is_the_invoice_as_income_and_the_fee_as_spending() {
    let lines = "\
2026-01-02 ann owes me 3_100 USD due 2026-02-01 #design ^i1
2026-01-20 ann -> 3_100 USD #design ^i1
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    with_run(&book("cash", lines), day(2026, 1, 31), |book, run| {
        let rows = flow(book, run, FlowBy::Period(axiom_model::Period::Month));
        let wanted =
            ["=income | 3,100.00 USD", "  design | 3,100.00 USD", "=spending | 90.20 USD", "  fees | 90.20 USD"];
        assert_eq!(rows[..4], wanted);
    });
}

/// `why #design` adds up what the purpose counted, by the same rule.
#[test]
fn why_a_purpose_says_what_it_counted_in_cash_books() {
    with_run(&book("cash", CLAIM), day(2026, 3, 31), |book, run| {
        let report = crate::report(book, run, &Query::Why { target: "#design" }, None).unwrap();
        let said: String = report.sections.iter().flat_map(|section| lines(section)).collect::<Vec<_>>().join("\n");
        assert!(said.contains("100.00 USD"), "{said}");
        assert!(!said.contains("300.00 USD"), "the claim made counted nothing: {said}");
    });
}
