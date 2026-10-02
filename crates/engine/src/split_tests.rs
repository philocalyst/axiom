//! What LANGUAGE §3 says a split and its items mean, written as source and run.
//!
//! The language says: "When the header names only one end, the indented legs are the other side; their total is the
//! header amount, or the sum of the legs, and at most one leg is `...` (the remainder)", and of line items:
//! `AMOUNT` is "carved out of the header's amount: what the items do not claim keeps the header's purpose",
//! `+ AMOUNT` "comes on top of it", `- AMOUNT` "is taken off it". These tests are that text, line by line, with
//! its own worked example first, so a reader can check them against the spec and not against the code.
//!
//! Every test names what the book moves (`checking -> shop 75.90 USD #household`: from, to, the posted amount, the
//! purpose) and what a place holds after. Value is conserved by construction in a flow that moves it, so a split
//! that leaves its source whole, or a remainder that is zero, shows here as the wrong number.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, FileId};
use axiom_model::{Amount, Book, Source};
use axiom_syntax::Folder;

use crate::{Options, Posted, Run};

const BOOK: &str = "\
base USD
commodity USD
  precision 2
commodity EUR
  precision 2
purpose household : spending
purpose groceries : spending
purpose gifts : spending
purpose fun : spending
purpose fees : spending
account assets/checking
account assets/savings
entity shop
entity acme
entity buyer
opening 2026-01-01
  checking 1_000 USD
  savings 500 USD

";

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).unwrap()
}

/// What the model says of `lines` after the standard book: every diagnostic.
fn built(lines: &str) -> Vec<axiom_core::Diagnostic> {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]).1
}

/// Folds the standard book and `lines` through the end of March.
fn with_run<R>(lines: &str, then: impl FnOnce(&Book, &Run) -> R) -> R {
    let text = format!("{BOOK}{lines}");
    let (file, parsed) = axiom_syntax::parse(FileId(0), &text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, diagnostics) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {diagnostics:?}");
    then(&book, &crate::run(&book, Options { today: day(2026, 3, 31), relaxed: false }))
}

/// Every flow after the opening, as the fold posted it: `checking -> shop 75.90 USD #household`.
fn moves(book: &Book, run: &Run) -> Vec<String> {
    let mut said = Vec::new();
    for (id, flow) in book.flows.iter() {
        if flow.day < day(2026, 3, 1) {
            continue;
        }
        let Posted { out, arrive, .. } = run.posted[id.index()];
        let place = |id| book.name(book.places[id].path).rsplit('/').next().unwrap_or_default();
        let purpose =
            flow.purpose.map_or(String::new(), |said| format!(" #{}", book.name(book.purposes[said.purpose].name)));
        let amount = |qty| book.show(Amount::new(qty, flow.out.unit));
        let shown =
            if out == arrive { amount(out).to_string() } else { format!("{} out, {} in", amount(out), amount(arrive)) };
        said.push(format!("{} -> {} {shown}{purpose}", place(flow.from), place(flow.to)));
    }
    said
}

/// What `place` holds, in whole cents.
fn cents(book: &Book, run: &Run, place: &str) -> i64 {
    let (place, usd) = (book.place(place).unwrap(), book.commodity("USD").unwrap());
    run.holdings
        .iter()
        .find(|holding| holding.place == place && holding.unit == usd)
        .map_or(0, |holding| holding.qty().0)
}

// ─── Items: LANGUAGE §3, "Line items" ───────────────────────────────────────

#[test]
#[ignore = "fails before the statement path is solved: the header posts whole beside its items"]
fn the_language_example_an_item_is_carved_out_of_the_header_and_what_is_left_keeps_its_purpose() {
    // The worked example of §3 (there a visa and a target, here a bank account and a shop):
    // `14 visa -> target 120.00 USD #household`, 32.10 USD #groceries, 12.00 USD #gifts: "75.90 stays #household".
    let lines = "\
2026-03-14 checking -> shop 120.00 USD #household
  32.10 USD #groceries
  12.00 USD #gifts \"for jo's birthday\"
";
    with_run(lines, |book, run| {
        assert_eq!(
            moves(book, run),
            [
                "checking -> shop 75.90 USD #household",
                "checking -> shop 32.10 USD #groceries",
                "checking -> shop 12.00 USD #gifts"
            ]
        );
        assert_eq!(cents(book, run, "checking"), 88_000, "120.00 left the account, as the header says");
    });
}

#[test]
#[ignore = "fails before the statement path is solved: a discount that says nothing bears on nothing"]
fn a_discount_that_says_nothing_is_taken_off_the_header() {
    let lines = "\
2026-03-14 checking -> shop 100 USD #household
  60 USD #fun
  - 5 USD
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 35.00 USD #household", "checking -> shop 60.00 USD #fun"]);
        assert_eq!(cents(book, run, "checking"), 90_500, "100 less the 5 discount left the account");
    });
}

#[test]
fn an_item_that_comes_on_top_is_paid_as_well() {
    let lines = "\
2026-03-14 checking -> shop 100 USD #household
  + 10 USD #fees
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 100.00 USD #household", "checking -> shop 10.00 USD #fees"]);
        assert_eq!(cents(book, run, "checking"), 89_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: `amount` in an item is the item's own zero"]
fn a_share_of_amount_in_an_item_is_a_share_of_the_header() {
    let lines = "\
2026-03-14 checking -> shop 200 USD #household
  25% of amount #fun
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 150.00 USD #household", "checking -> shop 50.00 USD #fun"]);
        assert_eq!(cents(book, run, "checking"), 80_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: `6%` is not an amount, and §3's second example does not compile"]
fn the_language_example_a_cost_withheld_from_proceeds_is_a_share_of_the_header() {
    // §3: `15 title-co -> checking 627_000 USD #sale of condo` with `- 6% #selling-costs "commission"`: 37,620 withheld.
    let lines = "\
2026-03-14 buyer -> checking 1_000 USD #household
  - 6% #fees \"commission\"
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["buyer -> checking 1000.00 USD #household", "checking -> buyer 60.00 USD #fees"]);
        assert_eq!(cents(book, run, "checking"), 100_000 + 94_000);
    });
}

// ─── Splits: LANGUAGE §3, "Split flows" ─────────────────────────────────────

#[test]
#[ignore = "fails before the statement path is solved: a split never debits its source"]
fn a_split_takes_every_leg_from_its_source() {
    let split = "\
2026-03-14 checking 300 USD ->
  shop 100 USD
  acme 100 USD
  savings 100 USD
";
    let plain = "\
2026-03-14 checking -> shop 100 USD
2026-03-14 checking -> acme 100 USD
2026-03-14 checking -> savings 100 USD
";
    with_run(split, |book, run| {
        assert_eq!(
            moves(book, run),
            ["checking -> shop 100.00 USD", "checking -> acme 100.00 USD", "checking -> savings 100.00 USD"]
        );
        assert_eq!(cents(book, run, "checking"), 70_000, "300 left the account");
        assert_eq!(cents(book, run, "savings"), 60_000);
    });
    let (spent, saved) = with_run(plain, |book, run| (cents(book, run, "checking"), cents(book, run, "savings")));
    with_run(split, |book, run| {
        assert_eq!((cents(book, run, "checking"), cents(book, run, "savings")), (spent, saved), "three transfers");
    });
}

#[test]
#[ignore = "fails before the statement path is solved: the remainder posts zero"]
fn the_remainder_leg_is_what_the_other_legs_leave_of_the_total() {
    let lines = "\
2026-03-14 checking 300 USD ->
  shop 100 USD
  savings ...
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 100.00 USD", "checking -> savings 200.00 USD"]);
        assert_eq!(cents(book, run, "checking"), 70_000);
        assert_eq!(cents(book, run, "savings"), 70_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: a total written after the arrow is dropped"]
fn a_total_written_after_the_arrow_is_the_total() {
    let lines = "\
2026-03-14 checking -> 300 USD
  shop 100 USD
  savings ...
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 100.00 USD", "checking -> savings 200.00 USD"]);
        assert_eq!(cents(book, run, "checking"), 70_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: a split to one end credits it nothing"]
fn a_split_into_one_end_pays_it_by_its_legs() {
    let lines = "\
2026-03-14 -> checking 300 USD
  shop 100 USD
  acme ...
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["shop -> checking 100.00 USD", "acme -> checking 200.00 USD"]);
        assert_eq!(cents(book, run, "checking"), 130_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: a split never debits its source"]
fn a_split_with_no_total_is_the_sum_of_its_legs() {
    let lines = "\
2026-03-14 checking ->
  shop 100 USD
  savings 50 USD
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 100.00 USD", "checking -> savings 50.00 USD"]);
        assert_eq!(cents(book, run, "checking"), 85_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: computed legs and the remainder are not solved together"]
fn a_computed_leg_is_solved_when_the_split_lands_and_the_remainder_follows() {
    let lines = "\
2026-03-14 checking 300 USD ->
  shop 10% of 100 USD
  savings ...
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> shop 10.00 USD", "checking -> savings 290.00 USD"]);
        assert_eq!(cents(book, run, "checking"), 70_000);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: `=` and the remainder are not solved together"]
fn a_target_leg_is_the_gap_to_its_balance_and_the_remainder_is_what_is_left() {
    // savings holds 500.00; `= 800 USD` is a leg of 300.00.
    let lines = "\
2026-03-14 checking 1_000 USD ->
  savings = 800 USD
  shop ...
";
    with_run(lines, |book, run| {
        assert_eq!(moves(book, run), ["checking -> savings 300.00 USD", "checking -> shop 700.00 USD"]);
        assert_eq!(cents(book, run, "savings"), 80_000);
        assert_eq!(cents(book, run, "checking"), 0);
    });
}

#[test]
#[ignore = "fails before the statement path is solved: items sit beside the first leg, and take from nothing"]
fn items_under_a_split_sit_between_the_source_and_the_remainder_leg() {
    let lines = "\
2026-03-14 checking 100 USD ->
  shop 60 USD
  acme ...
  10 USD #fees
";
    with_run(lines, |book, run| {
        assert_eq!(
            moves(book, run),
            ["checking -> shop 60.00 USD", "checking -> acme 30.00 USD", "checking -> acme 10.00 USD #fees"]
        );
        assert_eq!(cents(book, run, "checking"), 90_000);
    });
}

// ─── Conservation: a split that cannot add up is said, at its legs ──────────

fn imbalances(lines: &str) -> Vec<axiom_core::Diagnostic> {
    built(lines).into_iter().filter(|diagnostic| diagnostic.code == "split-imbalance").collect()
}

#[test]
#[ignore = "fails before the static check: the book builds, and posts 70.00 of a 100.00 total"]
fn legs_that_are_short_of_the_total_are_an_error_at_the_legs() {
    let lines = "\
2026-03-14 checking 100 USD ->
  shop 40 USD
  savings 30 USD
";
    let said = imbalances(lines);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].is_error());
    assert!(said[0].message.contains("70.00 USD") && said[0].message.contains("100.00 USD"), "{}", said[0].message);
    assert!(said[0].labels.len() >= 3, "the header and both legs are pointed at: {:?}", said[0].labels);
}

#[test]
#[ignore = "fails before the static check: the remainder would be negative and posts as zero"]
fn legs_that_take_more_than_the_total_leave_a_negative_remainder_and_are_an_error() {
    let lines = "\
2026-03-14 checking 100 USD ->
  shop 80 USD
  acme 50 USD
  savings ...
";
    assert_eq!(imbalances(lines).len(), 1);
}

#[test]
#[ignore = "fails before the static check: a leg in another commodity than its total cannot add to it"]
fn a_leg_in_another_commodity_than_the_total_cannot_add_to_it() {
    let lines = "\
2026-03-14 checking 100 USD ->
  shop 40 EUR
  savings ...
";
    let diagnostics = built(lines);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.is_error()), "{diagnostics:?}");
}

#[test]
#[ignore = "fails before the static check: items that take more than their header have nowhere to come from"]
fn items_that_take_more_than_their_header_are_an_error() {
    let lines = "\
2026-03-14 checking -> shop 100 USD #household
  150 USD #fun
";
    assert_eq!(imbalances(lines).len(), 1);
}

#[test]
fn a_split_that_adds_up_is_not_an_error() {
    let lines = "\
2026-03-14 checking 100 USD ->
  shop 40 USD
  savings 60 USD
2026-03-15 checking 100 USD ->
  shop 40 USD
  savings ...
2026-03-16 checking ->
  shop 40 USD
  savings 10 USD
";
    assert!(imbalances(lines).is_empty());
}
