//! The histories the fold records, held to the fold itself: a ledger advanced a day at a time knows what every position
//! holds that day, and the run's history of the position must say the same, for every position and every day.
//!
//! The oracle uses nothing the recorder does: it reads `Ledger::balance`, which is the slot's own quantity. A hook that
//! is missed (a change that goes through no `entry`), a day that is off by one, a step dropped or merged with another
//! day's, all show as a day on which the two disagree. The same check runs on books written as fixtures (a pending
//! flow, a return, a split, a pad, lots) and as source (a claim, its payment and its write-off), the places a balance
//! changes that are not a flow's two ends.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, FileId, Qty};
use axiom_model::{Book, EventState, Role, Source};
use axiom_syntax::Folder;

use crate::fixture::Fixture;
use crate::{Options, Plan, Position, Run};

fn options() -> Options {
    Options { today: Day(1000), relaxed: false }
}

/// Folds `book` a day at a time from `first` through `last`, and holds each position's history to what the ledger holds on
/// the day. Every holding the ledger has is a position of the history, and the history ends where the holdings end.
fn history_is_the_fold(book: &Book, options: Options, first: Day, last: Day) -> Run {
    let (plan, run) = (Plan::new(book), crate::run(book, options));
    let mut ledger = plan.start(options);
    let mut day = first;
    while day <= last {
        ledger.advance(day);
        for (id, at) in run.histories.positions() {
            assert_eq!(run.histories.at(id, day), ledger.balance(at.place, at.unit), "{at:?} on {day}");
        }
        let known: Vec<Position> = run.histories.positions().map(|(_, at)| at).collect();
        for holding in ledger.holdings().filter(|holding| !holding.qty().is_zero()) {
            let at = Position { place: holding.place, unit: holding.unit };
            assert!(known.binary_search(&at).is_ok(), "{at:?} holds {:?} on {day} and has no history", holding.qty());
        }
        day = Day(day.0 + 1);
    }
    for holding in &run.holdings {
        let at = Position { place: holding.place, unit: holding.unit };
        let (id, _) = run.histories.positions().find(|&(_, found)| found == at).expect("a holding has a history");
        assert_eq!(run.histories.at(id, Day::MAX), holding.qty(), "{at:?} at the end");
    }
    run
}

#[test]
fn flows_a_day_apart_and_on_the_same_day_are_each_a_step_or_one() {
    let mut f = Fixture::new();
    let (equity, checking, food, savings, usd) = (f.equity, f.checking, f.food, f.savings, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.flow(2, checking, food, 30_00);
    f.flow(2, checking, food, 20_00);
    f.flow(2, checking, savings, 100_00);
    f.flow(5, savings, checking, 100_00);
    let book = f.book();
    let run = history_is_the_fold(&book, options(), Day(0), Day(8));
    let steps = |place, unit| {
        let (id, _) = run.histories.positions().find(|(_, at)| at.place == place && at.unit == unit).unwrap();
        run.histories.steps(id).iter().map(|(day, held)| (day.0, held.0)).collect::<Vec<_>>()
    };
    assert_eq!(steps(checking, usd), [(1, 1_000_00), (2, 850_00), (5, 950_00)], "three flows on day 2 are one step");
    assert_eq!(steps(savings, usd), [(2, 100_00), (5, 0)]);
    assert_eq!(run.histories.steps_in_all(), 3 + 2 + 1 + 1, "checking, savings, food and equity: a step a day each");
}

#[test]
fn a_pending_flow_moves_nothing_until_it_settles_and_a_returned_one_is_undone_on_its_return_day() {
    let mut f = Fixture::new();
    let (equity, checking, food) = (f.equity, f.checking, f.food);
    f.flow(1, equity, checking, 100_00);
    let check = f.flow(2, checking, food, 50_00);
    let bounced = f.flow(3, checking, food, 30_00);
    f.pending(check, "#c1");
    f.mark(bounced, "#b1");
    f.event(5, "#c1", EventState::Settled);
    f.event(7, "#b1", EventState::Returned);
    let book = f.book();
    history_is_the_fold(&book, options(), Day(0), Day(9));
}

#[test]
fn lots_a_sale_and_a_split_that_scales_every_parcel_of_a_commodity() {
    let mut f = Fixture::new();
    let (equity, checking, vti) = (f.equity, f.checking, f.vti);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 1_300_00, 10);
    f.split(4, vti, 3, 2);
    f.sell(6, 7, 1_000_00);
    f.buy(8, 500_00, 3);
    let book = f.book();
    history_is_the_fold(&book, options(), Day(0), Day(10));
}

#[test]
fn a_padded_gap_is_a_step_on_the_day_of_its_assertion() {
    let mut f = Fixture::new();
    let (equity, checking, unknown, usd) = (f.equity, f.checking, f.unknown, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(3, checking, 900_00);
    f.pad_last();
    let book = f.book();
    let run = history_is_the_fold(&book, options(), Day(0), Day(5));
    let (checking_at, _) = run.histories.positions().find(|(_, at)| at.place == checking && at.unit == usd).unwrap();
    assert_eq!(run.histories.at(checking_at, Day(2)), Qty(1_000_00));
    assert_eq!(run.histories.at(checking_at, Day(3)), Qty(900_00));
    let (unknown_at, _) = run.histories.positions().find(|(_, at)| at.place == unknown).unwrap();
    assert_eq!(run.histories.at(unknown_at, Day(3)), Qty(100_00));
}

/// A tenant who does not pay: each claim is made on the day the monitor finds a due day missed, a day that holds no fact of
/// the journal, so the history must have a step there and not wait for the next fact (there is none).
const RENT: &str = "\
base USD
commodity USD
  precision 2
account assets/checking
entity ann
opening 2026-01-01
  checking 1_000 USD
contract rent with ann
  1_000 USD monthly on 1 into checking
  from 2026-01-01
  due 5d
";

#[test]
fn a_claim_the_monitor_makes_is_a_step_on_the_day_it_finds_a_due_day_missed() {
    let (file, parsed) = axiom_syntax::parse(FileId(0), RENT, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    let day = |month, date| Day::from_ymd(2026, month, date).unwrap();
    let run = history_is_the_fold(&book, Options { today: day(3, 1), relaxed: false }, day(1, 1), day(3, 1));
    let is_tab = |place: &axiom_model::Place| matches!(place.role, Role::Tab(_));
    let tab = book.places.iter().find(|(_, place)| is_tab(place)).map(|(id, _)| id).expect("ann's tab");
    let (tab_at, _) = run.histories.positions().find(|(_, at)| at.place == tab).expect("the tab held something");
    let held = |day| run.histories.at(tab_at, day).0;
    let found = [held(day(1, 16)), held(day(1, 17)), held(day(2, 16)), held(day(2, 17))];
    assert_eq!(found, [0, 100_000, 100_000, 200_000], "a claim on each day the monitor finds a due day missed");
}

/// Claims: made, settled in part by a payment from the party, and the rest forgiven, are changes to the places a flow does
/// not name: the tab is relieved by a payment and by a write-off.
const CLAIMS: &str = "\
base USD
commodity USD
  precision 2
purpose design : income
account assets/checking
entity ann
entity bob
opening 2026-01-01
  checking 1_000 USD
2026-01-02 ann      owes   me  300 USD ^i1 due 2026-02-01
2026-01-03 bob      owes   me  200 USD ^i2 due 2026-02-01
2026-01-20 checking <-     ann 100 USD ^i1
2026-02-15 ^i1      waived             \"not collected\"
2026-02-16 checking <-     bob 200 USD
";

#[test]
fn a_payment_from_a_party_and_a_write_off_relieve_the_tab_on_their_days() {
    let (file, parsed) = axiom_syntax::parse(FileId(0), CLAIMS, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    let day = |month, date| Day::from_ymd(2026, month, date).unwrap();
    let run = history_is_the_fold(&book, Options { today: day(3, 1), relaxed: false }, day(1, 1), day(3, 1));
    let is_ann = |place: &axiom_model::Place| matches!(place.role, Role::Tab(entity) if book.name(book.entities[entity].path) == "ann");
    let tab = book.places.iter().find(|(_, place)| is_ann(place)).map(|(id, _)| id).expect("ann's tab");
    let (tab_at, _) = run.histories.positions().find(|(_, at)| at.place == tab).expect("the tab held something");
    let held = |day| run.histories.at(tab_at, day).0;
    assert_eq!([held(day(1, 2)), held(day(1, 20)), held(day(2, 14)), held(day(2, 15))], [300_00, 200_00, 200_00, 0]);
}
