//! What LANGUAGE §7 says settles and ends a claim, written as source and run.
//!
//! The language says: "A later flow between them settles open claims: those its codes name, in order; else the one whose
//! open amount is exactly the flow's; else the oldest first", and "A claim `waived` is forgiven". A claim is a parcel
//! in a claim place, so a settlement is relief and a write-off is the relief of one transaction's parcels. Each test
//! names the claims a book makes (`^i1` 300.00 USD, then `^i2` 200.00, then `^i3` 300.00, so the oldest is not the one
//! whose amount is exact) and says what each is left at after the flow or the statement.
//!
//! The first block settles in a declared claim place, `owed`, which is the one place a flow can be written out of. A
//! payment *from the party* does not reach a claim (a tab has no name, K3c-map section 0.1): its test is here, ignored,
//! with the reason.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, FileId, Id};
use axiom_model::{Book, Place, Role, Source};
use axiom_syntax::Folder;

use crate::{Options, Run};

const BOOK: &str = "\
base USD
commodity USD
  precision 2
commodity BOX
  precision 0
kind receivable : asset
  claim
kind fifo-receivable : asset
  claim
  select fifo
purpose design : income
account assets/checking
account assets/stock
account assets/owed : receivable
account assets/queue : fifo-receivable
entity ann
entity bob
opening 2026-01-01
  checking 1_000 USD
2026-01-01 BOX = 10 USD
";

/// Three claims in `owed`, the oldest not the one that is exactly 200.00: 300.00, 200.00, 300.00.
const THREE: &str = "\
2026-01-02 ann -> owed 300 USD due 2026-02-01 #design ^i1
2026-01-03 ann -> owed 200 USD due 2026-02-01 #design ^i2
2026-01-04 ann -> owed 300 USD due 2026-02-01 #design ^i3
";

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).unwrap()
}

/// Folds the standard book and `lines` through the first of March.
fn with_run<R>(lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    then(&book, &crate::run(&book, Options { today: day(2026, 3, 1), relaxed: false }))
}

/// The model's diagnostics for the standard book and `lines`.
fn built(lines: &str) -> Vec<axiom_core::Diagnostic> {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]).1
}

/// What `place` holds, oldest first: each parcel's codes and what is left of it, in quanta.
fn parcels(book: &Book, run: &Run, place: Id<Place>) -> Vec<(String, i64)> {
    let mut left = Vec::new();
    for holding in run.holdings.iter().filter(|holding| holding.place == place) {
        for lot in holding.lots.iter().filter(|lot| lot.qty.0 != 0) {
            let codes = [lot.codes.header, lot.codes.local].into_iter().flat_map(|codes| book.codes[codes].iter());
            let named: Vec<_> = codes.map(|&code| book.name(code)).collect();
            left.push((named.join(" "), lot.qty.0));
        }
    }
    left
}

/// What a declared claim place holds.
fn open(book: &Book, run: &Run, place: &str) -> Vec<(String, i64)> {
    parcels(book, run, book.place(place).unwrap())
}

/// What the tab with `party` holds: a tab has no name, so it is found by whom it is with.
fn tab(book: &Book, run: &Run, party: &str) -> Vec<(String, i64)> {
    let tab = book.places.iter().find_map(|(id, place)| match place.role {
        Role::Tab(entity) if book.name(book.entities[entity].path) == party => Some(id),
        _ => None,
    });
    parcels(book, run, tab.expect("the party has a tab"))
}

fn claims(left: &[(&str, i64)]) -> Vec<(String, i64)> {
    left.iter().map(|&(code, qty)| (code.to_string(), qty)).collect()
}

fn said(run: &Run, code: &str) -> Vec<String> {
    run.diagnostics.iter().filter(|d| &*d.code == code).map(|d| d.message.clone()).collect()
}

// ─── Settlement is relief: a code, then the exact amount, then the oldest ───

/// "else the one whose open amount is exactly the flow's": 200.00 settles `^i2`, which is not the oldest.
#[test]
fn a_flow_settles_the_claim_whose_open_amount_is_exactly_its_own_before_an_older_one() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 200 USD\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i3", 300_00)]));
        assert!(said(run, "ambiguous-lots").is_empty(), "a rule chose, so nothing is ambiguous");
    });
}

/// "else the oldest first": nothing is exactly 100.00.
#[test]
fn a_flow_that_is_exactly_no_claim_settles_the_oldest_first() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 100 USD\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 200_00), ("i2", 200_00), ("i3", 300_00)]));
    });
}

/// More than any one claim: oldest first across claims, and each is settled whole before the next is touched.
#[test]
fn a_flow_larger_than_any_claim_settles_oldest_first_across_them() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 400 USD\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i2", 100_00), ("i3", 300_00)]));
    });
}

/// Two claims of exactly the flow's amount: the older. FIFO would take the 200.00 of `^i1` and then 100.00 of `^i2`.
#[test]
fn of_two_claims_that_are_exactly_the_flows_amount_the_older_is_settled() {
    let lines = "\
2026-01-02 ann -> owed 200 USD due 2026-02-01 #design ^i1
2026-01-03 ann -> owed 300 USD due 2026-02-01 #design ^i2
2026-01-04 ann -> owed 300 USD due 2026-02-01 #design ^i3
2026-01-20 owed -> checking 300 USD
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 200_00), ("i3", 300_00)]));
    });
}

/// "those its codes name": `^i3` is named, so 200.00 comes out of it, though `^i2` is exactly 200.00 and `^i1` is the oldest.
#[test]
fn a_code_beats_both_the_exact_amount_and_the_oldest() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 200 USD ^i3\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i2", 200_00), ("i3", 100_00)]));
    });
}

/// Two codes: the claims they name are the ones that can be settled, oldest first among them.
#[test]
fn several_codes_name_the_claims_that_may_be_settled_and_the_oldest_of_them_goes_first() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 400 USD ^i2 ^i3\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i3", 100_00)]));
    });
}

/// A code that no claim there carries labels the payment, as it always did: 200.00 is exactly `^i2`.
#[test]
fn a_code_that_names_no_claim_is_only_a_label() {
    let lines = format!("{THREE}2026-01-20 owed -> checking 200 USD ^check-17\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i3", 300_00)]));
    });
}

/// The selector the author wrote is the more explicit: it is a filter, and the flow's code is left a label.
#[test]
fn a_written_selector_beats_the_flows_own_code() {
    let lines = format!("{THREE}2026-01-20 owed[^i1] -> checking 200 USD ^i3\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 100_00), ("i2", 200_00), ("i3", 300_00)]));
    });
}

/// A claim of boxes: 5 BOX is exactly `^i2`, and a rule chose it, so no lot is ambiguous (the commodity has no policy).
#[test]
fn a_claim_of_any_commodity_is_settled_by_the_same_order() {
    let lines = "\
2026-01-02 ann -> owed 12 BOX due 2026-02-01 ^i1
2026-01-03 ann -> owed 5 BOX due 2026-02-01 ^i2
2026-01-04 ann -> owed 12 BOX due 2026-02-01 ^i3
2026-01-20 owed -> stock 5 BOX
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 12), ("i3", 12)]));
        assert!(said(run, "ambiguous-lots").is_empty());
    });
}

/// A place that says its own policy keeps it: `Exact` is what a claim place does when it says nothing.
#[test]
fn a_claim_place_that_names_a_policy_keeps_it() {
    let lines = "\
2026-01-02 ann -> queue 300 USD due 2026-02-01 #design ^i1
2026-01-03 ann -> queue 200 USD due 2026-02-01 #design ^i2
2026-01-04 ann -> queue 300 USD due 2026-02-01 #design ^i3
2026-01-20 queue -> checking 200 USD
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/queue"), claims(&[("i1", 100_00), ("i2", 200_00), ("i3", 300_00)]));
    });
}

/// "A later flow between them settles open claims": a payment from the party reaches the claim. Not built: a tab has no
/// name, and settling it by the party's flow needs `books` read and a flow posted as two movements (K3c-map section 6).
#[test]
#[ignore = "K3c-map section 6: a payment from the party does not settle its claims (not built)"]
fn a_payment_from_the_party_settles_the_claims_it_names() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-03 ann owes me 200 USD due 2026-02-01 ^i2
2026-01-20 ann -> checking 200 USD ^i2
";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00)]));
    });
}

// ─── A write-off is the relief of one transaction's parcels ─────────────────

const OWES: &str = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-03 ann owes me 200 USD due 2026-02-01 ^i2
";

/// "A claim `waived` is forgiven": what it still owed is gone, the other claim is not touched, and nothing is overdue
/// for it.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn a_write_off_forgives_what_the_claim_still_owed_and_nothing_else() {
    let lines = format!("{OWES}2026-02-15 ^i1 waived \"not collected\"\n");
    with_run(&lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i2", 200_00)]));
        let overdue = said(run, "overdue");
        assert_eq!(overdue.len(), 1, "{overdue:?}");
        assert!(overdue[0].contains("200.00 USD"), "{overdue:?}");
    });
}

/// The forgiven value leaves the owner: checking is as it was and the claim was part of net worth.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn a_write_off_lowers_what_the_owner_holds_by_what_was_forgiven() {
    let lines = format!("{OWES}2026-02-15 ^i1 waived\n");
    with_run(&lines, |book, run| {
        let held = |place| {
            let place = book.place(place).unwrap();
            run.holdings.iter().filter(|holding| holding.place == place).map(|holding| holding.qty().0).sum::<i64>()
        };
        assert_eq!(held("assets/checking"), 1_000_00);
        let claimed: i64 = tab(book, run, "ann").iter().map(|(_, qty)| qty).sum();
        assert_eq!(claimed, 200_00);
    });
}

/// A claim made on the day it is forgiven exists when the statement runs: movements come first.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn a_claim_is_forgiven_on_the_day_it_was_made() {
    let lines = "2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1\n2026-01-02 ^i1 waived\n";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        assert!(said(run, "overdue").is_empty());
    });
}

/// A payment of the day relieves first, so the write-off forgives what is left of it, and no more.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange; a claim place that is not a tab cannot be waived"]
fn a_write_off_forgives_what_a_payment_of_the_same_day_left() {
    let lines = "\
2026-01-02 ann -> owed 300 USD due 2026-02-01 #design ^i1
2026-01-20 owed[^i1] -> checking 100 USD
2026-01-20 ^i1 waived
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[]));
        let checking = book.place("assets/checking").unwrap();
        let held: i64 = run.holdings.iter().filter(|h| h.place == checking).map(|h| h.qty().0).sum();
        assert_eq!(held, 1_100_00, "the 100.00 that was paid, and not the 200.00 that was forgiven");
    });
}

/// Two claims of one amount from one party on one day are told apart by their transaction.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn a_write_off_forgives_the_claim_it_names_and_not_one_of_the_same_amount() {
    let lines = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i2
2026-02-15 ^i2 waived
";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00)]));
    });
}

/// An itemized claim is one transaction with a parcel for each line: all of it is forgiven.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn an_itemized_claim_is_forgiven_whole() {
    let lines = "\
2026-01-02 ann owes me due 2026-02-01 ^i1
  300 USD #design
  200 USD #design
2026-02-15 ^i1 waived
";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
    });
}

/// A write-off that finds nothing open forgives nothing, and says so, as a `!` that waives nothing does.
#[test]
#[ignore = "K3c: the fold does nothing with a ClaimChange"]
fn a_write_off_of_a_claim_that_is_settled_says_it_forgave_nothing() {
    let lines = "\
2026-01-02 ann -> owed 300 USD due 2026-02-01 #design ^i1
2026-01-20 owed[^i1] -> checking 300 USD
2026-02-15 ^i1 waived
";
    with_run(lines, |_, run| {
        let said = said(run, "claim-writeoff-empty");
        assert_eq!(said.len(), 1, "{:?}", run.diagnostics);
        assert!(run.diagnostics.iter().all(|d| !d.is_error()), "a warning, not an error: {:?}", run.diagnostics);
    });
}

/// What the owner owes is a plain balance, not parcels: there is nothing for a write-off to relieve, so it is refused
/// where it is written.
#[test]
#[ignore = "K3c: a write-off of a debt of the owner's is accepted and does nothing"]
fn a_debt_of_the_owners_cannot_be_written_off() {
    let diagnostics = built("2026-01-05 me owes bob 100 USD due 2026-02-01 ^b1\n2026-02-15 ^b1 waived\n");
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).map(|d| &*d.code).collect();
    assert_eq!(errors, ["claim-writeoff-target"], "{diagnostics:?}");
}

/// A transaction that moved the owner's own money into a claim place made no claim on a party: nobody to give it back to.
#[test]
fn a_claim_place_funded_by_the_owners_own_money_is_not_a_claim_on_a_party() {
    let diagnostics = built("2026-01-02 checking -> owed 300 USD due 2026-02-01 ^i1\n2026-02-15 ^i1 waived\n");
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).map(|d| &*d.code).collect();
    assert_eq!(errors, ["claim-writeoff-target"], "{diagnostics:?}");
}
