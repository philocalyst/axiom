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
2026-01-02 owed <- ann 300 USD #design ^i1 due 2026-02-01
2026-01-03 owed <- ann 200 USD #design ^i2 due 2026-02-01
2026-01-04 owed <- ann 300 USD #design ^i3 due 2026-02-01
";

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).unwrap()
}

/// Folds the standard book and `lines` through the first of March.
fn with_run<R>(lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_run_on(day(2026, 3, 1), lines, then)
}

/// Folds the standard book and `lines` through `today`.
fn with_run_on<R>(today: Day, lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    then(&book, &crate::run(&book, Options { today, relaxed: false }))
}

/// The model's diagnostics for the standard book and `lines`.
fn built(lines: &str) -> Vec<axiom_core::Diagnostic> {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]).1
}

/// What `place` holds, oldest first: each parcel's codes and what is left of it, in quanta.
pub(crate) fn parcels(book: &Book, run: &Run, place: Id<Place>) -> Vec<(String, i64)> {
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
pub(crate) fn tab(book: &Book, run: &Run, party: &str) -> Vec<(String, i64)> {
    let tab = book.places.iter().find_map(|(id, place)| match place.role {
        Role::Tab(entity) if book.name(book.entities[entity].path) == party => Some(id),
        _ => None,
    });
    parcels(book, run, tab.expect("the party has a tab"))
}

pub(crate) fn claims(left: &[(&str, i64)]) -> Vec<(String, i64)> {
    left.iter().map(|&(code, qty)| (code.to_string(), qty)).collect()
}

pub(crate) fn said(run: &Run, code: &str) -> Vec<String> {
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
2026-01-02 owed <- ann      200 USD #design ^i1 due 2026-02-01
2026-01-03 owed <- ann      300 USD #design ^i2 due 2026-02-01
2026-01-04 owed <- ann      300 USD #design ^i3 due 2026-02-01
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
    let lines = format!("{THREE}2026-01-20 owed[^i3] -> checking 200 USD ^i1\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i2", 200_00), ("i3", 100_00)]));
    });
}

/// The same for a day: `[2026-01-04]` is `^i3`, and the code on the line, `^i1`, does not widen it.
#[test]
fn a_written_day_beats_the_flows_own_code() {
    let lines = format!("{THREE}2026-01-20 owed[2026-01-04] -> checking 200 USD ^i1\n");
    with_run(&lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[("i1", 300_00), ("i2", 200_00), ("i3", 100_00)]));
    });
}

/// A claim of boxes: 5 BOX is exactly `^i2`, and a rule chose it, so no lot is ambiguous (the commodity has no policy).
#[test]
fn a_claim_of_any_commodity_is_settled_by_the_same_order() {
    let lines = "\
2026-01-02 owed <- ann   12 BOX ^i1 due 2026-02-01
2026-01-03 owed <- ann   5 BOX  ^i2 due 2026-02-01
2026-01-04 owed <- ann   12 BOX ^i3 due 2026-02-01
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
2026-01-02 queue <- ann      300 USD #design ^i1 due 2026-02-01
2026-01-03 queue <- ann      200 USD #design ^i2 due 2026-02-01
2026-01-04 queue <- ann      300 USD #design ^i3 due 2026-02-01
2026-01-20 queue -> checking 200 USD
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/queue"), claims(&[("i1", 100_00), ("i2", 200_00), ("i3", 300_00)]));
    });
}

// ─── A payment from the party settles its claims ────────────────────────────

/// Three claims on `ann`, the oldest not the one that is exactly 200.00, and what each is left at after a payment of hers.
const OWED_BY_ANN: &str = "\
2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1
2026-01-03 ann owes me 200 USD due 2026-02-01 ^i2
2026-01-04 ann owes me 300 USD due 2026-02-01 ^i3
";

fn paid(payment: &str) -> String {
    format!("{OWED_BY_ANN}{payment}\n")
}

/// "A later flow between them settles open claims ... the one whose open amount is exactly the flow's".
#[test]
fn a_payment_from_the_party_settles_the_claim_whose_open_amount_is_exactly_its_own() {
    with_run(&paid("2026-01-20 checking <- ann 200 USD"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00), ("i3", 300_00)]));
    });
}

/// "else the oldest first".
#[test]
fn a_payment_that_is_exactly_no_claim_settles_the_oldest_first() {
    with_run(&paid("2026-01-20 checking <- ann 400 USD"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i2", 100_00), ("i3", 300_00)]));
    });
}

/// "those its codes name": the code on the payment beats the exact amount and the oldest.
#[test]
fn a_payment_that_carries_the_code_of_a_claim_settles_that_claim() {
    with_run(&paid("2026-01-20 checking <- ann 200 USD ^i3"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00), ("i2", 200_00), ("i3", 100_00)]));
    });
}

/// A code written on a line item of the payment is the code of that item's flow, and names its claim as the header's does.
/// The item is carved from the header (LANGUAGE §3): the 300 the header keeps settles the oldest claim, and the item's 100
/// settles the claim it names.
#[test]
fn a_code_on_a_line_item_of_a_payment_names_the_claim_that_item_settles() {
    with_run(&paid("2026-01-20 checking <- ann 400 USD\n  100 USD ^i3"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i2", 200_00), ("i3", 200_00)]));
    });
}

/// "What remains is an ordinary flow": it pays checking all the same, and more than the claims is no claim.
#[test]
fn what_a_payment_does_not_settle_is_an_ordinary_flow() {
    with_run(&paid("2026-01-20 checking <- ann 1_000 USD"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        let checking = book.place("assets/checking").unwrap();
        let held: i64 = run.holdings.iter().filter(|h| h.place == checking).map(|h| h.qty().0).sum();
        assert_eq!(held, 2_000_00, "the thousand arrived whole");
        assert!(said(run, "overdue").is_empty());
    });
}

/// Value is conserved: what the party was debited, with what the claims were, adds up to what checking was paid.
#[test]
fn settling_a_claim_does_not_create_or_lose_value() {
    with_run(&paid("2026-01-20 checking <- ann 500 USD"), |book, run| {
        let total: i64 = run.holdings.iter().filter(|h| h.unit == book.base).map(|h| h.qty().0).sum();
        assert_eq!(total, 0, "every place, the parties' too: {:?}", run.holdings);
    });
}

/// A party the owner holds no claim on pays as before; and so does one who pays before there is a claim.
#[test]
fn a_payment_settles_only_what_was_owed_by_its_party_and_already() {
    let lines = "\
2026-01-01 checking <-   bob 50 USD
2026-01-02 ann      owes me  300 USD ^i1 due 2026-02-01
2026-01-03 checking <-   bob 300 USD
2026-01-04 checking <-   ann 20 USD
";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 280_00)]), "ann's 20.00 settled in part");
    });
}

/// A payment in another commodity than the claim is not the claim's settlement.
#[test]
fn a_payment_in_another_commodity_is_no_settlement() {
    let lines = "2026-01-02 ann       owes me  300 USD ^i1 due 2026-02-01\n2026-01-04 ann -> stockroom 5 BOX\n";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00)]));
    });
}

/// A payment that bounces runs backwards: the claim it settled is open again, as it was.
#[test]
fn a_payment_that_is_returned_opens_the_claims_it_settled() {
    let lines = "\
2026-01-20 checking <-       ann 200 USD ^pay-1
2026-01-25 ^pay-1   returned
";
    with_run(&paid(lines).replace("\n\n", "\n"), |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i1", 300_00), ("i2", 200_00), ("i3", 300_00)]));
        let total: i64 = run.holdings.iter().filter(|h| h.unit == book.base).map(|h| h.qty().0).sum();
        assert_eq!(total, 0, "nothing was lost when it was put back");
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
fn a_write_off_forgives_what_the_claim_still_owed_and_nothing_else() {
    let lines = format!("{OWES}2026-02-15 ^i1 waived \"not collected\"\n");
    with_run(&lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("i2", 200_00)]));
        let overdue = said(run, "overdue");
        assert_eq!(overdue.len(), 1, "{overdue:?}");
        assert!(overdue[0].contains("200.00 USD"), "{overdue:?}");
    });
}

/// The write-off is recorded: which statement, where, and what was open of the claim.
#[test]
fn a_write_off_is_recorded_with_the_statement_and_what_was_forgiven() {
    let lines = format!("{OWES}2026-02-15 ^i1 waived \"not collected\"\n");
    with_run(&lines, |book, run| {
        let [off] = run.written_off[..] else { panic!("one parcel was open: {:?}", run.written_off) };
        assert_eq!((off.qty.0, off.basis.0, off.acquired), (300_00, 300_00, day(2026, 1, 2)));
        let change = book.claim_changes[off.change as usize];
        assert_eq!(
            (change.day, change.description.map(|text| book.text(text))),
            (day(2026, 2, 15), Some("not collected"))
        );
    });
}

/// The forgiven value leaves the owner: checking is as it was and the claim was part of net worth.
#[test]
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
fn a_claim_is_forgiven_on_the_day_it_was_made() {
    let lines = "2026-01-02 ann owes me 300 USD due 2026-02-01 ^i1\n2026-01-02 ^i1 waived\n";
    with_run(lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[]));
        assert!(said(run, "overdue").is_empty());
    });
}

/// A payment of the day relieves first, so the write-off forgives what is left of it, and no more.
#[test]
fn a_write_off_forgives_what_a_payment_of_the_same_day_left() {
    let lines = "\
2026-01-02 owed      <-     ann      300 USD #design ^i1 due 2026-02-01
2026-01-20 owed[^i1] ->     checking 100 USD
2026-01-20 ^i1       waived
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[]));
        let checking = book.place("assets/checking").unwrap();
        let held: i64 = run.holdings.iter().filter(|h| h.place == checking).map(|h| h.qty().0).sum();
        assert_eq!(held, 1_100_00, "the 100.00 that was paid, and not the 200.00 that was forgiven");
    });
}

/// A payment carries the code of the claim it settles, so the code is on two transactions: the write-off names the one
/// that made the claim.
#[test]
fn a_claim_whose_payment_carries_its_code_is_still_written_off_by_that_code() {
    let lines = "\
2026-01-02 owed <-     ann      300 USD #design ^i1 due 2026-02-01
2026-01-20 owed ->     checking 100 USD ^i1
2026-02-15 ^i1  waived
";
    with_run(lines, |book, run| {
        assert_eq!(open(book, run, "assets/owed"), claims(&[]));
        assert_eq!(run.written_off.iter().map(|off| off.qty.0).collect::<Vec<_>>(), [200_00]);
    });
}

/// Two claims of one amount from one party on one day are told apart by their transaction.
#[test]
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
fn a_write_off_of_a_claim_that_is_settled_says_it_forgave_nothing() {
    let lines = "\
2026-01-02 owed      <-     ann      300 USD #design ^i1 due 2026-02-01
2026-01-20 owed[^i1] ->     checking 300 USD
2026-02-15 ^i1       waived
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

// ─── A due day past its deadline that nothing kept is a claim on whoever is blamed ───

/// A tenant pays `rent` on the first of each month, five days after it falls due at the latest.
const RENT: &str = "\
contract rent with ann
  1_000 USD monthly on 1 into checking
  from 2026-01-01
  due 5d
";

fn tabs(book: &Book) -> usize {
    book.places.iter().filter(|(_, place)| matches!(place.role, Role::Tab(_))).count()
}

fn tab_parcels(book: &Book, run: &Run) -> usize {
    let tab = book.places.iter().find_map(|(id, place)| matches!(place.role, Role::Tab(_)).then_some(id));
    tab.map_or(0, |tab| parcels(book, run, tab).len())
}

/// The party was to pay and did not: what it owed is claimed, once for each due day, in the tab the owner keeps with it.
#[test]
fn a_due_day_nothing_kept_is_a_claim_on_the_party() {
    with_run(RENT, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("", 1_000_00), ("", 1_000_00)]), "January and February");
        assert!(said(run, "missed-occurrence").is_empty(), "a claim is said as overdue, not as missed as well");
    });
}

/// A line that kept the due day made the payment, so nothing is owed for it.
#[test]
fn a_due_day_a_line_kept_is_no_claim() {
    let lines = format!("{RENT}2026-01-03 rent\n");
    with_run(&lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("", 1_000_00)]), "only February");
    });
}

/// The claim is made on the day the due day is missed: past the reach of its schedule, which for a month is half a month,
/// and not before.
#[test]
fn the_claim_is_made_the_day_the_due_day_is_missed() {
    let text = "contract rent with ann\n  1_000 USD monthly on 1 into checking\n  from 2026-02-01\n  due 5d\n";
    with_run_on(day(2026, 2, 16), text, |book, run| assert_eq!(tab_parcels(book, run), 0));
    with_run_on(day(2026, 2, 17), text, |book, run| assert_eq!(tab_parcels(book, run), 1));
}

/// What the tab holds is made on the day each due day was missed, and not on the day it fell due or the day the fold is run to.
#[test]
fn a_claim_is_dated_the_day_it_was_made() {
    with_run(RENT, |book, run| {
        let tab = book.places.iter().find_map(|(id, place)| matches!(place.role, Role::Tab(_)).then_some(id)).unwrap();
        let made: Vec<_> =
            run.holdings.iter().filter(|holding| holding.place == tab).flat_map(|holding| &holding.lots).collect();
        let days: Vec<_> = made.iter().map(|lot| lot.acquired.to_string()).collect();
        assert_eq!(days, ["2026-01-17", "2026-02-17"]);
    });
}

/// A deadline longer than the reach is the later of the two: it is not missed while the party may still pay.
#[test]
fn a_deadline_longer_than_the_reach_delays_the_claim() {
    let text = "contract rent with ann\n  1_000 USD monthly on 1 into checking\n  from 2026-02-01\n  due 30d\n";
    with_run_on(day(2026, 3, 3), text, |book, run| assert_eq!(tab_parcels(book, run), 0));
    with_run_on(day(2026, 3, 4), text, |book, run| assert_eq!(tab_parcels(book, run), 1));
}

/// A later payment from the party settles what it owes as any payment does: the oldest claim, when none is exact.
#[test]
fn a_later_payment_from_the_party_settles_the_claim() {
    let lines = format!("{RENT}2026-02-20 ann -> checking 600 USD\n");
    with_run(&lines, |book, run| {
        assert_eq!(tab(book, run, "ann"), claims(&[("", 400_00), ("", 1_000_00)]), "January, less what was paid");
    });
}

/// A contract that gives no deadline has no day on which it fails: it is missed, and warned of, and nothing is claimed.
#[test]
fn a_contract_with_no_deadline_makes_no_claim() {
    let text = "contract rent with ann\n  1_000 USD monthly on 1 into checking\n  from 2026-01-01\n";
    with_run(text, |book, run| {
        assert_eq!(tab_parcels(book, run), 0);
        assert_eq!(tabs(book), 0, "nor is a tab asked for");
        assert_eq!(said(run, "missed-occurrence").len(), 1);
    });
}

/// An occurrence that cannot be made owes nothing that can be said, so it is warned of as missed, as it was before a miss
/// could be a claim.
#[test]
fn an_occurrence_that_cannot_be_made_is_warned_of_and_claims_nothing() {
    let text = "param cpi\n  2026-12-01 100\ncontract rent with ann\n  1_000 USD monthly on 1 into checking\n  from 2026-01-01\n  indexed to cpi yearly\n  due 5d\n";
    with_run(text, |book, run| {
        assert_eq!(tab_parcels(book, run), 0);
        assert_eq!(said(run, "missed-occurrence").len(), 1);
    });
}

/// A claim the monitor made has no purpose: it recognizes nothing and no law that counts a purpose's flows counts it, as
/// nothing reads `books cash|accrual` yet to say whether it should.
#[test]
fn a_claim_the_monitor_made_has_no_purpose_for_a_law_to_count() {
    let text = "purpose gigs : income\n  law per-flow\n    on flow\n    owe 1 USD to treasury by date(2026, 12, 31) as per-flow\nentity treasury\ncontract rent with ann\n  1_000 USD monthly on 1 into checking #gigs\n  from 2026-01-01\n  due 5d\n";
    with_run(text, |book, run| {
        assert_eq!(tab_parcels(book, run), 2, "January and February");
        assert!(run.effects.is_empty(), "no law counted a claim: {:?}", run.effects);
    });
}

/// A header that is no amount owes nothing, so there is nothing to claim of it: it is warned of as missed.
#[test]
fn an_occurrence_of_no_amount_is_warned_of_and_claims_nothing() {
    let text = "contract rent with ann\n  0 USD monthly on 1 into checking\n  from 2026-01-01\n  due 5d\n";
    with_run(text, |book, run| {
        assert_eq!(tab_parcels(book, run), 0);
        assert_eq!(said(run, "missed-occurrence").len(), 1);
    });
}

/// What the owner was to pay is not a claim of the owner's: a debt is a plain balance and no payment to the party settles it.
#[test]
fn what_the_owner_failed_to_pay_is_not_claimed() {
    let text = "contract rent with ann\n  1_000 USD monthly on 1 from checking\n  from 2026-01-01\n  due 5d\n";
    with_run(text, |book, run| {
        assert_eq!(tab_parcels(book, run), 0);
        assert_eq!(said(run, "missed-occurrence").len(), 1);
    });
}
