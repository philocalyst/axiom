//! What LANGUAGE §7 says of a bill, written as source and run: "`OWNER owes PARTY AMOUNT` records one without moving money", and
//! "a later flow between them settles open claims: those its codes name, in order; else the one whose open amount is exactly the
//! flow's; else the oldest first". What the owner owes is a parcel in a tab of the owner's, as what a party owes is, so paying the
//! party is relief of it, and the party forgiving it (`^b1 waived`) is relief too.
//!
//! The parcels of a debt are negative quantities (a place holds what flowed into it less what flowed out), so each test reads
//! what is *owed* through [`owed`], which says them in the sign people write them. Three bills are made as `THREE` says, the oldest
//! not the one that is exactly 200.00, as `claim_tests.rs` does for claims. The last blocks say when a bill counts as spending, in
//! the owner's `books`, and that a loan stays a balance beside a bill with the same lender.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::Id;
use axiom_model::{Book, Class, Place, Role};

use crate::Run;
use crate::claim_tests::{claims, parcels, said};
use crate::recognition_tests::{counted, on};
use crate::source_tests::{day, with_run};

fn prelude(books: &str) -> String {
    format!(
        "\
base USD
commodity USD
  precision 2
kind payable : debt
  claim
purpose utilities : spending
  law bills
    on flow
    count amount as bill-spending
  law running
    on flow
    count total(ever) as bill-running
account checking
account owed-to-ben : payable
entity me
  books {books}
entity pge
entity ben
entity bob
opening 2026-01-01
  checking 1_000 USD
"
    )
}

/// Three bills from `pge`, the oldest not the one that is exactly 200.00: 300.00, 200.00, 300.00.
const THREE: &str = "\
2026-01-02 me owes pge 300 USD due 2026-02-01 ^b1
2026-01-03 me owes pge 200 USD due 2026-02-01 ^b2
2026-01-04 me owes pge 300 USD due 2026-02-01 ^b3
";

/// Folds the prelude of `books` and `lines` through the first of March.
fn with<R>(books: &str, lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_run(&format!("{}{lines}", prelude(books)), day(2026, 3, 1), then)
}

fn cash<R>(lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with("cash", lines, then)
}

/// The tab the owner keeps with `party` of the bills it owes, which has no name: found by whom it is with.
fn tab(book: &Book, party: &str) -> Id<Place> {
    let is = |entity| book.name(book.entities[entity].path) == party;
    let found = book.places.iter().find(|&(id, place)| {
        matches!(place.role, Role::Tab(entity) if is(entity)) && place.class == Class::Debt && book.is_claim(id)
    });
    found.expect("the party has a tab the owner owes").0
}

/// What the owner owes `party`, bill by bill, in quanta and the sign a bill is written in.
fn owed(book: &Book, run: &Run, party: &str) -> Vec<(String, i64)> {
    parcels(book, run, tab(book, party)).into_iter().map(|(code, qty)| (code, -qty)).collect()
}

/// What `place` holds in the base currency, in quanta, parcels and plain together.
fn held(book: &Book, run: &Run, place: Id<Place>) -> i64 {
    run.holdings.iter().filter(|holding| holding.place == place && holding.unit == book.base).map(|h| h.qty().0).sum()
}

fn at(book: &Book, run: &Run, name: &str) -> i64 {
    held(book, run, book.place(name).unwrap())
}

/// What every place holds, the parties' too: value is conserved, so this is zero whatever settles.
fn everything(book: &Book, run: &Run) -> i64 {
    run.holdings.iter().filter(|holding| holding.unit == book.base).map(|holding| holding.qty().0).sum()
}

// ─── A bill is a parcel ─────────────────────────────────────────────────────

#[test]
fn a_bill_is_a_parcel_the_owner_owes_and_the_party_is_credited() {
    let lines = "2026-01-05 me owes pge 142.50 USD due 2026-02-20 ^b1\n";
    cash(lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 142_50)]));
        assert_eq!(
            held(book, run, tab(book, "pge")),
            -142_50,
            "what the tab holds is what it owes, as a liability's balance is"
        );
        assert_eq!(at(book, run, "checking"), 1_000_00, "a bill moves no money");
        assert_eq!(everything(book, run), 0);
    });
}

#[test]
fn a_bill_paid_in_full_is_settled_and_is_owed_once() {
    let lines = "2026-01-05 me owes pge 142.50 USD due 2026-02-20 ^b1\n2026-01-20 checking -> pge 142.50 USD ^b1\n";
    cash(lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[]));
        assert_eq!(held(book, run, tab(book, "pge")), 0);
        assert_eq!(at(book, run, "checking"), 1_000_00 - 142_50);
        assert_eq!(at(book, run, "pge"), 142_50, "the party was billed it once, and not again when it was paid");
        assert_eq!(everything(book, run), 0, "value was conserved");
    });
}

#[test]
fn a_bill_paid_in_part_is_owed_what_is_left() {
    let lines = "2026-01-05 me owes pge 142.50 USD due 2026-02-20 ^b1\n2026-01-20 checking -> pge 100 USD ^b1\n";
    cash(lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 42_50)]));
        assert_eq!(at(book, run, "pge"), 142_50);
        assert_eq!(everything(book, run), 0);
    });
}

/// "those its codes name": `^b3` is named, so 200.00 comes out of it, though `^b2` is exactly 200.00 and `^b1` is the oldest.
#[test]
fn a_payment_that_carries_the_code_of_a_bill_settles_that_bill() {
    let lines = format!("{THREE}2026-01-20 checking -> pge 200 USD ^b3\n");
    cash(&lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 300_00), ("b2", 200_00), ("b3", 100_00)]))
    });
}

/// "else the one whose open amount is exactly the flow's": 200.00 settles `^b2`, which is not the oldest.
#[test]
fn a_payment_settles_the_bill_it_is_exactly_the_size_of_before_an_older_one() {
    let lines = format!("{THREE}2026-01-20 checking -> pge 200 USD\n");
    cash(&lines, |book, run| assert_eq!(owed(book, run, "pge"), claims(&[("b1", 300_00), ("b3", 300_00)])));
}

/// "else the oldest first": nothing is exactly 100.00, or 400.00, which settles `^b1` and takes 100.00 of `^b2`.
#[test]
fn a_payment_that_is_exactly_no_bill_settles_the_oldest_first() {
    let lines = format!("{THREE}2026-01-20 checking -> pge 100 USD\n");
    cash(&lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 200_00), ("b2", 200_00), ("b3", 300_00)]));
    });
    let lines = format!("{THREE}2026-01-20 checking -> pge 400 USD\n");
    cash(&lines, |book, run| assert_eq!(owed(book, run, "pge"), claims(&[("b2", 100_00), ("b3", 300_00)])));
}

/// "What remains is an ordinary flow": the party is credited what did not settle a bill, so all of it was paid.
#[test]
fn what_a_payment_does_not_settle_is_an_ordinary_flow() {
    let lines = format!("{THREE}2026-01-20 checking -> pge 900 USD\n");
    cash(&lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[]));
        assert_eq!(at(book, run, "checking"), 100_00);
        assert_eq!(at(book, run, "pge"), 900_00, "the 800.00 that was billed and the 100.00 more that was paid");
        assert_eq!(everything(book, run), 0);
    });
}

#[test]
fn a_payment_to_someone_else_settles_nothing() {
    let lines = format!("{THREE}2026-01-20 checking -> bob 300 USD\n");
    cash(&lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 300_00), ("b2", 200_00), ("b3", 300_00)]));
        assert_eq!(at(book, run, "bob"), 300_00);
    });
}

/// A payment before the bill is no payment of it: it is paid, and the bill comes after.
#[test]
fn a_payment_before_there_is_a_bill_settles_nothing() {
    let lines = "2026-01-02 checking -> pge 300 USD\n2026-01-20 me owes pge 300 USD due 2026-03-01 ^b1\n";
    cash(lines, |book, run| assert_eq!(owed(book, run, "pge"), claims(&[("b1", 300_00)])));
}

#[test]
fn a_payment_that_is_returned_opens_the_bill_again() {
    let lines = "2026-01-05 me owes pge 300 USD due 2026-02-20 ^b1\n2026-01-20 checking -> pge 300 USD ^pay-1\n";
    cash(lines, |book, run| assert_eq!(owed(book, run, "pge"), claims(&[])));
    cash(&format!("{lines}2026-01-25 ^pay-1 returned\n"), |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[("b1", 300_00)]));
        assert_eq!(at(book, run, "checking"), 1_000_00);
        assert_eq!(at(book, run, "pge"), 300_00, "the party is credited the bill, and not the payment that bounced");
        assert_eq!(everything(book, run), 0, "nothing was lost when it was put back");
    });
}

// ─── A bill forgiven ────────────────────────────────────────────────────────

#[test]
fn a_bill_the_party_forgives_is_no_longer_owed_and_the_party_is_no_longer_credited_it() {
    let lines = "\
2026-01-05 me owes pge 300 USD due 2026-02-20 ^b1
2026-01-20 checking -> pge 100 USD ^b1
2026-02-15 ^b1 waived \"a credit\"
";
    cash(lines, |book, run| {
        assert_eq!(owed(book, run, "pge"), claims(&[]));
        assert_eq!(at(book, run, "pge"), 100_00, "what was paid stays paid; the 200.00 forgiven was never to be");
        assert_eq!(at(book, run, "checking"), 900_00);
        assert_eq!(everything(book, run), 0);
        assert_eq!(run.written_off.len(), 1);
        assert!(said(run, "claim-writeoff-empty").is_empty());
    });
}

// ─── A declared place for what the owner owes ───────────────────────────────

/// `owed-to-ben` says `payable`, so it says `claim`: what is paid out of it is a bill, and what is paid into it settles one.
#[test]
fn a_declared_payable_place_holds_its_bills_and_a_payment_into_it_settles_the_one_it_names() {
    let lines = "\
2026-01-02 owed-to-ben -> pge 300 USD due 2026-02-01 ^p1
2026-01-03 owed-to-ben -> pge 200 USD due 2026-02-01 ^p2
2026-01-20 checking -> owed-to-ben 200 USD ^p2
2026-01-21 checking -> owed-to-ben 50 USD ^p1
";
    cash(lines, |book, run| {
        let place = book.place("owed-to-ben").unwrap();
        let left: Vec<_> = parcels(book, run, place).into_iter().map(|(code, qty)| (code, -qty)).collect();
        assert_eq!(left, claims(&[("p1", 250_00)]));
        assert_eq!(held(book, run, place), -250_00);
        assert_eq!(everything(book, run), 0);
    });
}

/// A payment into a declared place that bounces runs backwards, from the place: the bill it settled is open again, and the
/// credit it left (what was more than the bill) is taken back, so nothing is owed that was not and nothing is lost.
#[test]
fn a_payment_into_a_declared_place_that_is_returned_opens_the_bill_again() {
    let lines = "\
2026-01-02 owed-to-ben -> pge 300 USD due 2026-02-01 ^p1
2026-01-20 checking -> owed-to-ben 400 USD ^pay-1
";
    let place = |book: &Book| book.place("owed-to-ben").unwrap();
    cash(lines, |book, run| {
        assert_eq!(parcels(book, run, place(book)), []);
        assert_eq!(held(book, run, place(book)), 100_00, "the bill is settled and 100.00 is credit");
    });
    cash(&format!("{lines}2026-01-25 ^pay-1 returned\n"), |book, run| {
        let left: Vec<_> = parcels(book, run, place(book)).into_iter().map(|(code, qty)| (code, -qty)).collect();
        assert_eq!(left, claims(&[("p1", 300_00)]));
        assert_eq!(held(book, run, place(book)), -300_00, "the credit went back with the payment");
        assert_eq!(at(book, run, "checking"), 1_000_00);
        assert_eq!(everything(book, run), 0);
    });
}

/// More than is owed through a declared place is a credit with the party, a positive balance: an overpayment, which is no parcel.
#[test]
fn an_overpayment_into_a_declared_place_is_a_credit() {
    let lines =
        "2026-01-02 owed-to-ben -> pge 300 USD due 2026-02-01 ^p1\n2026-01-20 checking -> owed-to-ben 400 USD\n";
    cash(lines, |book, run| {
        let place = book.place("owed-to-ben").unwrap();
        assert_eq!(parcels(book, run, place), []);
        assert_eq!(held(book, run, place), 100_00);
        assert_eq!(everything(book, run), 0);
    });
}

// ─── When a bill counts as spending ─────────────────────────────────────────

const BILL: &str = "2026-01-02 me owes pge 300 USD due 2026-02-01 #utilities ^b1\n";

#[test]
fn a_bill_counts_nothing_when_it_is_made_in_cash_books_and_its_payment_counts_the_bills_purpose() {
    cash(BILL, |book, run| assert_eq!(counted(book, run, "bill-spending"), []));
    let lines = format!("{BILL}2026-01-20 checking -> pge 300 USD ^b1\n");
    cash(&lines, |book, run| {
        assert_eq!(counted(book, run, "bill-spending"), [on("2026-01-20", 300_00)]);
        assert_eq!(counted(book, run, "bill-running"), [on("2026-01-20", 300_00)]);
    });
}

#[test]
fn a_payment_that_says_the_bills_purpose_is_counted_once() {
    let lines = format!("{BILL}2026-01-20 checking -> pge 300 USD #utilities ^b1\n");
    cash(&lines, |book, run| assert_eq!(counted(book, run, "bill-spending"), [on("2026-01-20", 300_00)], "not 600.00"));
}

#[test]
fn a_bill_counts_when_it_is_made_in_accrual_books_and_its_payment_counts_nothing() {
    let lines = format!("{BILL}2026-01-20 checking -> pge 300 USD #utilities ^b1\n");
    with("accrual", &lines, |book, run| {
        assert_eq!(counted(book, run, "bill-spending"), [on("2026-01-02", 300_00)]);
        assert_eq!(counted(book, run, "bill-running"), [on("2026-01-02", 300_00)]);
    });
}

/// 300.00 was recognized when the bill was made, and 200.00 of it is forgiven: the total is what was paid, and then 50.00 more.
#[test]
fn a_bill_forgiven_is_taken_back_in_accrual_books_and_there_is_nothing_to_take_back_in_cash_books() {
    let lines = format!(
        "{BILL}2026-01-20 checking -> pge 100 USD ^b1\n2026-02-15 ^b1 waived \"a credit\"\n2026-03-01 checking -> pge 50 USD #utilities\n"
    );
    with("accrual", &lines, |book, run| {
        assert_eq!(counted(book, run, "bill-running"), [on("2026-01-02", 300_00), on("2026-03-01", 150_00)]);
    });
    with("cash", &lines, |book, run| {
        assert_eq!(counted(book, run, "bill-running"), [on("2026-01-20", 100_00), on("2026-03-01", 150_00)]);
    });
}

#[test]
fn a_payment_that_is_returned_takes_back_what_it_counted_in_cash_books() {
    let lines = format!(
        "{BILL}2026-01-20 checking -> pge 300 USD ^pay-1\n2026-01-25 ^pay-1 returned\n2026-02-01 checking -> pge 50 USD #utilities\n"
    );
    cash(&lines, |book, run| {
        assert_eq!(counted(book, run, "bill-running"), [on("2026-01-20", 300_00), on("2026-02-01", 50_00)]);
    });
}

/// A credit note from outside, paid into the place that holds the bill, settles it and is what the bill's purpose took back:
/// the claim counted as spending and the note as spending refunded come to nothing, where a note counted as the payment of
/// a bill (which replaces what it counts of itself) would leave the bill's spending standing.
#[test]
fn a_credit_note_into_a_declared_place_settles_the_bill_and_is_its_spending_refunded() {
    let lines = "\
2026-01-02 owed-to-ben -> pge 300 USD due 2026-02-01 #utilities ^p1
2026-01-20 ben -> owed-to-ben 300 USD #utilities ^p1
";
    cash(lines, |book, run| {
        assert_eq!(parcels(book, run, book.place("owed-to-ben").unwrap()), []);
        assert_eq!(counted(book, run, "bill-running"), [], "300.00 of spending, and 300.00 refunded");
        assert_eq!(everything(book, run), 0);
    });
}

#[test]
fn a_bill_with_no_purpose_has_no_recognition_to_wait_for() {
    let lines =
        "2026-01-02 me owes pge 300 USD due 2026-02-01 ^b1\n2026-01-20 checking -> pge 300 USD #utilities ^b1\n";
    for books in ["cash", "accrual"] {
        with(books, lines, |book, run| {
            assert_eq!(
                counted(book, run, "bill-spending"),
                [on("2026-01-20", 300_00)],
                "{books}: the payment counts as it says"
            );
        });
    }
}

// ─── A loan is a balance, and a bill beside it is not ───────────────────────

/// A bill from `bank` and a loan with `bank` are two places: a tab is a bill's or a loan's by its kind. The loan's tab holds a
/// balance and no parcel, and what is paid to `bank` settles the bill and not the loan.
#[test]
fn a_loan_with_the_lender_a_bill_is_from_is_a_balance_in_a_tab_of_its_own() {
    let text = "\
use std
base USD
entity bank
account checking
opening 2026-01-01
  checking 100_000 USD
contract mortgage with bank
  loan 12_000 USD on 2026-01-15 at 0% over 1y
  monthly on 1 from checking
  from 2026-02-01
2026-01-15 mortgage
2026-01-20 me owes bank 70 USD due 2026-03-01 ^b1
2026-02-01 mortgage
2026-02-10 checking -> bank 70 USD ^b1
";
    let sources = [("std.ax", include_str!("../../systems/src/std.ax"), true), ("main.ax", text, false)];
    let sources: Vec<_> = sources
        .into_iter()
        .enumerate()
        .map(|(index, (path, text, embedded))| {
            let (file, parsed) =
                axiom_syntax::parse(axiom_core::FileId(index as u16), text, axiom_syntax::Folder::of(path));
            assert!(parsed.is_empty(), "{path} does not parse: {parsed:?}");
            axiom_model::Source { path, file, embedded }
        })
        .collect();
    let (book, built) = axiom_model::build(&sources);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    let run = crate::run(&book, crate::Options { today: day(2026, 3, 1), relaxed: false });
    let loan = book.contracts[book.contract("mortgage").unwrap()].loan.unwrap().debt;
    assert_ne!(loan, tab(&book, "bank"), "a loan's debt and a bill are not one tab");
    assert!(!book.is_claim(loan) && book.is_claim(tab(&book, "bank")));
    assert_eq!(parcels(&book, &run, loan), [], "a loan holds a balance and no parcel");
    assert_eq!(held(&book, &run, loan), -11_000_00, "a payment of a thousand a month, the first in February");
    assert_eq!(owed(&book, &run, "bank"), claims(&[]), "what was paid to `bank` settled the bill, and not the loan");
}
