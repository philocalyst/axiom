//! What a party pays in all settles its claims: LANGUAGE §7, for a payment written as a statement of several legs.
//!
//! "A later flow between them settles open claims." A client who pays an invoice of 3,100.00 as 3,009.80 into the bank and
//! 90.20 to the processor that took its fee has paid the invoice (the README of `examples/04-freelancer`: "Stripe receipts
//! retain gross income and separately tag the fee"), and the leg to the processor is as much of the payment as the leg that
//! reached the bank. A party that pays someone else, with no leg to the owner, settles nothing of the owner's. Each test
//! says what a claim is left at, and what the party's payments leave in net worth.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_model::Book;

use crate::Run;
use crate::claim_tests::{claims, said, tab};
use crate::source_tests::{day, with_run};

const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
purpose design : income
purpose fees : spending
account checking
account savings
entity ann
entity bob
entity stripe
opening 2026-01-01
  checking 1_000 USD
";

/// Folds the prelude and `lines` through the first of March.
fn paid<R>(lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_run(&format!("{PRELUDE}{lines}"), day(2026, 3, 1), then)
}

/// What `place` holds of the base currency, in quanta.
fn held(book: &Book, run: &Run, place: &str) -> i64 {
    let place = book.place(place).unwrap();
    run.holdings.iter().filter(|holding| holding.place == place).map(|holding| holding.qty().0).sum()
}

/// What every place holds, the parties' too: value is conserved, so this is zero whatever settles.
fn everything(book: &Book, run: &Run) -> i64 {
    run.holdings.iter().filter(|holding| holding.unit == book.base).map(|holding| holding.qty().0).sum()
}

/// The statement of the README: a client pays 3,100.00 net of the processor's 90.20 fee, and the invoice is settled whole.
#[test]
fn a_split_payment_net_of_a_fee_settles_the_whole_invoice() {
    let lines = "\
2026-01-02 ann owes me 3_100 USD due 2026-02-01 #design ^i1
2026-01-20 ann -> 3_100 USD ^i1
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        assert!(said(run, "overdue").is_empty(), "a paid invoice is not overdue: {:?}", run.diagnostics);
        assert_eq!(held(book, run, "checking"), 1_000_00 + 3_009_80);
        assert_eq!(everything(book, run), 0, "value was conserved");
    });
}

/// The fee leg may be written first: the legs are one payment whatever their order.
#[test]
fn the_fee_leg_may_come_first() {
    let lines = "\
2026-01-02 ann owes me 3_100 USD due 2026-02-01 #design ^i1
2026-01-20 ann -> 3_100 USD ^i1
  stripe 90.20 USD #fees
  checking 3_009.80 USD
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        assert_eq!(held(book, run, "checking"), 1_000_00 + 3_009_80);
    });
}

/// "The one whose open amount is exactly the flow's" is what the party pays in all: 3,100.00, which is `^i2` and not the
/// oldest. Judged leg by leg it would be 3,009.80, which is no claim, and the oldest would have been paid.
#[test]
fn a_split_payment_settles_the_claim_it_is_the_size_of_and_not_the_oldest() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-03 ann owes me 3_100 USD due 2026-02-01 ^i2
2026-01-20 ann -> 3_100 USD
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00)]));
        assert_eq!(everything(book, run), 0);
    });
}

/// More than the claim: the claim is settled and what is left of the payment is an ordinary flow, as it is for one flow.
#[test]
fn a_split_payment_of_more_than_the_claim_settles_it_and_the_rest_is_ordinary() {
    let lines = "\
2026-01-02 ann owes me 3_000 USD due 2026-02-01 ^i1
2026-01-20 ann -> 3_100 USD
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        assert_eq!(held(book, run, "checking"), 1_000_00 + 3_009_80);
        assert_eq!(everything(book, run), 0);
    });
}

/// A payment that bounces runs backwards, every leg: the claim is open again as it was, and nothing is lost. Without the
/// return it is paid.
#[test]
fn a_returned_split_payment_opens_what_each_leg_settled() {
    let lines = "\
2026-01-02 ann owes me 3_100 USD due 2026-02-01 #design ^i1
2026-01-20 ann -> 3_100 USD ^pay-1
  checking 3_009.80 USD
  stripe 90.20 USD #fees
";
    paid(lines, |book, run| assert_eq!(tab(book, run, "ann"), claims(&[])));
    paid(&format!("{lines}2026-01-25 ^pay-1 returned\n"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 3_100_00)]));
        assert_eq!(held(book, run, "checking"), 1_000_00);
        assert_eq!(everything(book, run), 0, "nothing was lost when it was put back");
    });
}

/// A party that pays someone else, in a statement of its own, settles nothing of the owner's.
#[test]
fn a_payment_to_a_third_party_settles_nothing() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-20 ann -> bob 300 USD
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00)]));
    });
}

/// Nor does a statement whose legs all go to third parties: none of them reaches the owner, so none is paid on its behalf.
#[test]
fn a_split_of_which_no_leg_reaches_the_owner_settles_nothing() {
    let lines = "\
2026-01-02 ann owes me 200 USD due 2026-02-01 ^i1
2026-01-20 ann -> 200 USD
  bob 120 USD
  stripe 80 USD
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 200_00)]));
    });
}

/// A leg that is pending has not been paid yet: what the party pays at this moment is the legs that are real, so the first
/// leg judges "exactly" on its own 3,009.80 and settles the oldest claim, and the fee leg settles what is left when it lands.
#[test]
fn a_leg_that_is_still_pending_is_not_yet_what_the_party_pays() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i0
2026-01-03 ann owes me 3_100 USD due 2026-02-01 ^i1
2026-01-20 ann -> 3_100 USD
  checking 3_009.80 USD
  stripe (90.20 USD) ^fee-1
2026-01-25 ^fee-1 settled
";
    paid(lines, |book, run| {
        assert_eq!(
            tab(book, run, "ann"),
            claims(&[("i1", 300_00)]),
            "the oldest claim went first, then 90.20 of the other"
        );
        assert_eq!(everything(book, run), 0);
    });
}

/// The legs of a statement that come from two parties are two payments: what one party pays in all is not what the other
/// pays, so ann's 600.00 settles the claim of 600.00 and not the oldest.
#[test]
fn the_legs_of_a_statement_from_two_parties_are_a_payment_each() {
    let lines = "\
2026-01-02 ann owes me 100 USD due 2026-02-01 ^i0
2026-01-03 ann owes me 600 USD due 2026-02-01 ^i1
2026-01-04 bob owes me 400 USD due 2026-02-01 ^j1
2026-01-20 -> checking 1_000 USD
  ann 600 USD
  bob 400 USD
";
    paid(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i0", 100_00)]));
        assert_eq!(tab(book, run, "bob"), claims(&[]));
    });
}
