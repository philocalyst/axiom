//! The post host: what a law derives from a flow that has posted, in the order, with the identity, the record, the guard
//! and the return that `offspring.rs` says.

use axiom_core::{Id, Qty};
use axiom_model::{Book, Cause, Derivation, Origin};

use crate::Run;
use crate::source_tests::{day, with_run};

/// A book of the words every test here uses, with the lines each test writes under what it declares, and a journal.
#[derive(Default)]
struct Lines {
    /// Under `kind card : debt`, of which `visa` is an account.
    card: &'static str,
    /// Under `purpose fee`, `purpose rebate` and `purpose tip`.
    fee: &'static str,
    rebate: &'static str,
    tip: &'static str,
    /// Under `entity stripe`.
    stripe: &'static str,
    /// Under `account checking`.
    checking: &'static str,
    journal: &'static str,
}

impl Lines {
    fn book(&self) -> String {
        let Lines { card, fee, rebate, tip, stripe, checking, journal } = self;
        format!(
            "base USD
commodity USD
  precision 2
purpose fee : spending
{fee}purpose rebate : income
{rebate}purpose tip : spending
{tip}purpose groceries : spending
entity me
entity issuer
entity shop
entity stripe
{stripe}kind card : debt
{card}account checking
{checking}account visa : card
opening 2026-01-01
  checking 10_000.00 USD
{journal}"
        )
    }
}

const CASH_BACK: &str = "  also issuer -> self 2% of amount #rebate\n";

/// What `place` holds of the base currency when the run ends, in cents; a debt is negative.
fn cents(book: &Book, run: &Run, place: &str) -> i64 {
    let place = book.place(place).unwrap();
    run.holdings.iter().filter(|holding| holding.place == place).map(|holding| holding.qty().0).sum()
}

#[test]
fn a_cards_cash_back_credits_the_card_and_is_not_a_charge_that_earns_cash_back() {
    let journal = "2026-02-05 visa -> shop 100.00 USD #groceries\n2026-02-06 visa -> shop 50.00 USD #groceries\n";
    let text = Lines { card: CASH_BACK, journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        // The card owes 150.00 less the 2% the issuer credited, and nothing came of the credits.
        assert_eq!(cents(book, run, "visa"), -(15_000 - 300));
        assert_eq!(run.offspring.len(), 2);
        let first = &run.offspring[0];
        let issuer = book.entities[book.entity("issuer").unwrap()].place.unwrap();
        assert_eq!((first.flow.from, first.flow.to), (issuer, book.place("visa").unwrap()));
        assert_eq!(first.flow.out.qty, Qty(200));
        assert_eq!(first.flow.purpose.unwrap().purpose, book.purpose("rebate").unwrap());
        assert!(matches!(first.flow.origin, Origin::Derived(Derivation::Law(law)) if law == first.law));
        assert_eq!(first.flow.day, day(2026, 2, 5), "it posts the day the flow it comes with does");
        assert!(run.diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{:?}", run.diagnostics);
    });
}

#[test]
fn a_derived_flow_says_which_flow_it_came_from_and_the_effects_it_causes_say_so() {
    let rebate = "  law counted\n    on flow\n    count amount as rebates\n";
    let journal = "2026-02-05 visa -> shop 100.00 USD #groceries\n";
    let text = Lines { card: CASH_BACK, rebate, journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |_, run| {
        let offspring = &run.offspring[0];
        assert!(matches!(offspring.parent, Cause::Flow(_)), "the line that wrote the charge");
        let effect = run.effects.iter().find(|effect| effect.amount.qty == Qty(200)).expect("the rebate was counted");
        assert_eq!(effect.cause, Cause::Derived(Id::new(0)), "what a derived flow causes says it did");
    });
}

#[test]
fn two_places_one_law_governs_make_one_flow_not_two() {
    let text =
        Lines { card: CASH_BACK, journal: "2026-02-05 visa -> checking 100.00 USD\n", ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |_, run| {
        assert_eq!(run.offspring.len(), 1, "a step derives once for a flow, whichever end it touches");
    });
}

#[test]
fn value_that_moved_around_inside_what_a_law_governs_derives_nothing() {
    let journal = "2026-02-05 checking -> visa 100.00 USD\n";
    let text = Lines { checking: "  also + 1% of amount #fee\n", journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |_, run| {
        assert_eq!(run.offspring.len(), 1, "the account's own law reads the flow that leaves it");
    });
    let stays = "2026-02-05 checking -> checking 1.00 USD\n";
    let text = Lines { checking: "  also + 1% of amount #fee\n", journal: stays, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |_, run| {
        assert!(run.offspring.is_empty(), "a flow that stays where it was left nothing")
    });
}

#[test]
fn a_flow_a_pad_makes_and_an_opening_derive_nothing() {
    let journal = "2026-02-05 checking = 9_000.00 USD !\n";
    let text = Lines { checking: "  also - 1% of amount #fee\n", journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        assert!(!run.pads.is_empty(), "the assertion was padded");
        assert!(run.offspring.is_empty(), "a pad closes a gap and causes nothing; an opening is seen by no law");
        assert_eq!(cents(book, run, "checking"), 900_000);
    });
}

#[test]
fn what_a_flow_derives_posts_right_after_it_and_a_chain_goes_deep_before_the_next_sibling() {
    // A fee on every flow at checking (the source end), a tip of a tenth of every fee.
    let journal = "2026-02-05 checking -> shop 100.00 USD\n2026-02-06 checking -> shop 200.00 USD\n";
    let (checking, fee) = ("  also + 1 USD #fee\n", "  also - 10% of amount #tip\n");
    let text = Lines { checking, fee, journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |_, run| {
        let amounts: Vec<_> = run.offspring.iter().map(|offspring| offspring.flow.out.qty.0).collect();
        assert_eq!(amounts, [100, 10, 100, 10], "a fee, the tip of it, then the next line's");
        assert_eq!(run.offspring[1].parent, Cause::Derived(Id::new(0)), "the tip is derived from the fee");
        assert!(matches!(run.offspring[0].parent, Cause::Flow(_)));
    });
}

#[test]
fn a_chain_that_comes_back_to_a_law_is_said_once_with_each_law_in_order_and_the_flow_that_started_it() {
    // The card derives a cash back at the card; the purpose of a cash back derives a flow back along the reversed ends,
    // which is at the card, which the card's law watches: the chain is [card, rebate], and the card would come again.
    let journal = "2026-02-05 visa -> shop 100.00 USD #groceries\n2026-02-06 visa -> shop 50.00 USD #groceries\n";
    let text = Lines { card: CASH_BACK, rebate: "  also - 50% of amount #tip\n", journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        let said: Vec<_> = run.diagnostics.iter().filter(|diagnostic| diagnostic.code == "derive-cycle").collect();
        assert_eq!(said.len(), 1, "once, however many flows start the chain: {:?}", run.diagnostics);
        let texts: Vec<_> = said[0].labels.iter().map(|label| label.text.as_str()).collect();
        assert!(texts.iter().any(|text| text.starts_with("the flow that started it")), "{texts:?}");
        assert!(texts.iter().any(|text| text.starts_with("1. the `also` of kind `card`")), "{texts:?}");
        assert!(texts.iter().any(|text| text.starts_with("2. the `also` of purpose `#rebate`")), "{texts:?}");
        assert!(
            texts.iter().any(|text| text.starts_with("3. and the `also` of kind `card` would derive again")),
            "{texts:?}"
        );
        // The chain stopped where it began: two flows of each charge (the cash back, and what was taken back of it), not three.
        assert_eq!(run.offspring.len(), 4);
        assert_eq!(cents(book, run, "visa"), -(15_000 - 300 + 150));
    });
}

#[test]
fn a_chain_of_nine_different_laws_is_said_to_be_too_deep_where_it_stops() {
    // Each purpose p0..p8 derives a flow for the next, back along the reversed ends: nine laws, and the bound is eight.
    let purposes: String = (0..9)
        .map(|n| format!("purpose p{n} : spending\n  also - 100% of amount #p{}\n", n + 1))
        .chain(std::iter::once("purpose p9 : spending\n".to_owned()))
        .collect();
    let text = format!(
        "base USD\ncommodity USD\n  precision 2\n{purposes}entity me\nentity shop\naccount checking\nopening 2026-01-01\n  checking 100.00 USD\n2026-02-05 checking -> shop 10.00 USD #p0\n"
    );
    with_run(&text, day(2026, 3, 1), |_, run| {
        assert_eq!(run.offspring.len(), 8, "eight laws made a flow each");
        let said: Vec<_> = run.diagnostics.iter().filter(|diagnostic| diagnostic.code == "derive-depth").collect();
        assert_eq!(said.len(), 1, "{:?}", run.diagnostics);
        assert!(said[0].message.contains("more than 8 laws"), "{}", said[0].message);
    });
}

#[test]
fn a_pending_flow_derives_when_it_is_settled_and_a_void_one_never() {
    let journal = "2026-02-05 visa -> shop (100.00 USD) ^c1\n2026-02-06 visa -> shop (50.00 USD) ^c2\n2026-02-10 ^c1 settled\n2026-02-10 ^c2 void\n";
    let text = Lines { card: CASH_BACK, journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        assert_eq!(run.offspring.len(), 1, "the one that landed");
        assert_eq!(run.offspring[0].flow.day, day(2026, 2, 10), "it posts the day its cause did: the settlement");
        assert_eq!(cents(book, run, "visa"), -(10_000 - 200));
    });
}

#[test]
fn a_returned_flow_returns_what_it_derived_in_the_order_it_posted_it() {
    let journal = "2026-02-05 visa -> shop 100.00 USD ^c1\n2026-02-20 ^c1 returned\n";
    let text = Lines { card: CASH_BACK, journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        assert_eq!(run.offspring.len(), 1, "the return derives nothing anew");
        assert_eq!(cents(book, run, "visa"), 0, "the charge and its cash back are both reversed");
        let issuer = book.entities[book.entity("issuer").unwrap()].place.unwrap();
        assert_eq!(cents(book, run, "checking"), 1_000_000);
        assert_eq!(
            run.holdings.iter().filter(|holding| holding.place == issuer).map(|holding| holding.qty().0).sum::<i64>(),
            0
        );
    });
}

#[test]
fn a_return_dated_after_a_checkpoint_reverses_what_was_derived_before_it() {
    use crate::{Options, Plan};
    let journal = "2026-02-05 visa -> shop 100.00 USD ^c1\n2026-02-20 ^c1 returned\n";
    let text = Lines { card: CASH_BACK, journal, ..Lines::default() }.book();
    crate::source_tests::with_book(&text, |book| {
        let plan = Plan::new(book);
        let options = Options { today: day(2026, 3, 1), relaxed: false };
        let mut ledger = plan.start(options);
        ledger.advance(day(2026, 2, 10));
        assert_eq!(ledger.recorded().offspring.len(), 1);
        let checkpoint = ledger.checkpoint();
        let mut resumed = plan.resume(&checkpoint, options);
        resumed.advance(day(2026, 3, 1));
        let visa = book.place("visa").unwrap();
        assert_eq!(
            resumed.balance(visa, book.base),
            Qty(0),
            "the return reversed the cash back that the checkpoint remembers"
        );
        assert!(
            resumed.recorded().offspring.is_empty(),
            "and derived nothing: its record begins where the checkpoint's ended"
        );
        assert_eq!(resumed.recorded().first_offspring, 1);
    });
}

#[test]
fn the_numbers_of_derived_flows_go_on_after_a_checkpoint_and_the_effects_they_cause_say_which() {
    use crate::{Options, Plan};
    let journal = "2026-02-05 visa -> shop 100.00 USD\n2026-02-20 visa -> shop 100.00 USD\n";
    let rebate = "  law counted\n    on flow\n    count amount as rebates\n";
    let text = Lines { card: CASH_BACK, rebate, journal, ..Lines::default() }.book();
    crate::source_tests::with_book(&text, |book| {
        let plan = Plan::new(book);
        let options = Options { today: day(2026, 3, 1), relaxed: false };
        let mut ledger = plan.start(options);
        ledger.advance(day(2026, 2, 10));
        let mut resumed = plan.resume(&ledger.checkpoint(), options);
        resumed.advance(day(2026, 3, 1));
        let recorded = resumed.recorded();
        assert_eq!(
            (recorded.first_offspring, recorded.offspring.len()),
            (1, 1),
            "the second, after the one the checkpoint made"
        );
        assert_eq!(recorded.effects.len(), 1);
        assert_eq!(recorded.effects[0].cause, Cause::Derived(Id::new(1)), "named by its place in the whole fold");
    });
}

#[test]
fn an_entitys_also_reads_the_flows_at_its_own_place_and_a_minus_item_goes_back_along_the_ends() {
    let journal = "2026-02-05 checking -> stripe 100.00 USD\n";
    let text = Lines { stripe: "  also - 3% of amount #rebate\n", journal, ..Lines::default() }.book();
    with_run(&text, day(2026, 3, 1), |book, run| {
        let stripe = book.entities[book.entity("stripe").unwrap()].place.unwrap();
        let back = &run.offspring[0].flow;
        assert_eq!((back.from, back.to, back.out.qty), (stripe, book.place("checking").unwrap(), Qty(300)));
        assert_eq!(cents(book, run, "checking"), 1_000_000 - 10_000 + 300);
    });
}

#[test]
fn a_flow_handed_to_a_ledger_derives_as_a_flow_of_the_journal_does() {
    use crate::{Options, Plan};
    let text = Lines { card: CASH_BACK, journal: "2026-02-05 visa -> shop 100.00 USD\n", ..Lines::default() }.book();
    crate::source_tests::with_book(&text, |book| {
        let plan = Plan::new(book);
        let mut ledger = plan.start(Options { today: day(2026, 2, 5), relaxed: false });
        ledger.advance(day(2026, 2, 5));
        let mut hypothetical = book.flows.iter().last().unwrap().1.clone();
        hypothetical.day = day(2026, 2, 6);
        hypothetical.out.qty = Qty(50_00);
        hypothetical.arrive = hypothetical.out;
        let mut fork = ledger.fork();
        fork.apply(&hypothetical);
        let visa = book.place("visa").unwrap();
        assert_eq!(fork.balance(visa, book.base), Qty(-(10_000 - 200) - (5_000 - 100)));
        assert_eq!(fork.recorded().offspring.len(), 1, "the fork records only what it derived itself");
        assert_eq!(fork.recorded().first_offspring, 1);
    });
}

#[test]
fn an_occurrence_a_line_keeps_and_one_a_forecast_promises_derive_the_same() {
    use crate::{Options, Plan};
    let contract =
        "contract rent with stripe\n  100.00 USD monthly on 15 from checking\n  from 2026-01-15\n  until 2026-06-30\n";
    let head = Lines { stripe: "  also + 2% of amount #fee\n", ..Lines::default() }.book();
    let (kept, promised) = (format!("{head}{contract}2026-04-15 rent\n2026-05-15 rent\n"), format!("{head}{contract}"));
    // Two occurrences of 100.00 USD to the party, and 2.00 USD more of the fee along the same ends for each.
    let written = with_run(&kept, day(2026, 5, 31), |book, run| {
        assert_eq!(run.offspring.len(), 2);
        cents(book, run, "checking")
    });
    assert_eq!(written, 1_000_000 - 2 * 10_200);
    crate::source_tests::with_book(&promised, |book| {
        let plan = Plan::new(book);
        let mut ledger = plan.start(Options { today: day(2026, 3, 15), relaxed: false });
        ledger.advance(day(2026, 3, 15));
        ledger.reach(day(2026, 5, 31));
        ledger.promise(|_| true);
        ledger.advance(day(2026, 5, 31));
        assert_eq!(ledger.recorded().offspring.len(), 2, "the forecast is the fold: the same flows derive");
        assert_eq!(ledger.balance(book.place("checking").unwrap(), book.base).0, written);
    });
}
