//! Books written as source text, run through parse, model and engine.
//!
//! What the engine does with a flow depends on what the model made of the line
//! that wrote it (an entity as the source, a fee leg, a window a flow was
//! recognized for), so the behaviour that turns on both is tested from `.ax`
//! text to `Run`.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, FileId};
use axiom_model::{Book, Source};
use axiom_syntax::Folder;

use crate::{Holding, Options, Run};

fn day(year: i32, month: u32, day: u32) -> Day {
    Day::from_ymd(year, month, day).unwrap()
}

/// Compiles `text` as a project of one file, which must have no errors.
fn with_book<R>(text: &str, then: impl FnOnce(&Book) -> R) -> R {
    let (file, parsed) = axiom_syntax::parse(FileId(0), text, Folder::default());
    assert!(parsed.is_empty(), "the source does not parse: {parsed:?}");
    let (book, built) = axiom_model::build(&[Source { path: "axiom.ax", file, embedded: false }]);
    assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "the book has errors: {built:?}");
    then(&book)
}

/// Folds `text` through `today`.
fn with_run<R>(text: &str, today: Day, then: impl FnOnce(&Book, &Run) -> R) -> R {
    with_book(text, |book| then(book, &crate::run(book, Options { today, relaxed: false })))
}

/// What `place` holds of `unit`, as the run ends.
fn holding<'r>(book: &Book, run: &'r Run, place: &str, unit: &str) -> Option<&'r Holding> {
    let (place, unit) = (book.place(place).unwrap(), book.commodity(unit).unwrap());
    run.holdings.iter().find(|holding| holding.place == place && holding.unit == unit)
}

// ─── Paying out of an envelope ──────────────────────────────────────────────

const ENVELOPES: &str = "\
base USD
commodity USD
  precision 2
kind envelope : entity
  restricted

account assets/checking
account assets/savings
account expenses/car-repair

entity trip-fund : envelope
  via assets/savings
entity car-fund : envelope
  via assets/savings

opening 2025-09-01
  checking 2_000 USD

2025-09-06 checking -> savings 500 USD for trip-fund
2025-09-07 checking -> savings 200 USD for car-fund
";

/// The parcels of `savings` that are tied, as `(entity, quantity)`.
fn tied(book: &Book, run: &Run) -> Vec<(String, i64)> {
    let lots = &holding(book, run, "savings", "USD").unwrap().lots;
    let name = |lot: &crate::Parcel| book.name(book.entities[lot.tied.unwrap()].path).to_string();
    lots.iter().map(|lot| (name(lot), lot.qty.0)).collect()
}

#[test]
fn a_payment_out_of_an_envelope_takes_that_envelopes_parcels() {
    let text = format!("{ENVELOPES}2025-10-01 car-fund -> car-repair 150 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00), ("car-fund".into(), 50_00)]);
    });
}

#[test]
fn an_envelope_that_runs_out_is_topped_up_from_what_is_not_tied() {
    let text = format!("{ENVELOPES}2025-09-20 checking -> savings 300 USD\n2025-10-01 car-fund -> car-repair 250 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00)]);
        let savings = holding(book, run, "savings", "USD").unwrap();
        assert_eq!(savings.qty().0, 500_00 + 250_00, "the last 50 came from the 300 that was not tied");
    });
}

#[test]
fn an_overspent_envelope_leaves_the_other_envelopes_alone() {
    let text = format!("{ENVELOPES}2025-10-01 car-fund -> car-repair 250 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        assert_eq!(tied(book, run), [("trip-fund".into(), 500_00)]);
        assert_eq!(holding(book, run, "savings", "USD").unwrap().plain.0, -50_00, "the account owes 50 to nobody's money");
    });
}

#[test]
fn a_payment_written_out_of_an_envelope_is_judged_by_its_laws_not_dodged() {
    let text = "\
base USD
commodity USD
  precision 2
kind car-cost : expense
kind envelope : entity
  restricted
  has purpose kind
  law purpose
    on spend
    require to is self.purpose \"envelope money spent on something else\"

account assets/checking
account assets/savings
account expenses/dining
account expenses/car-repair : car-cost

entity car-fund : envelope
  via assets/savings
  purpose car-cost

opening 2025-09-01
  checking 2_000 USD

2025-09-07 checking -> savings 200 USD for car-fund
2025-09-08 checking -> savings 300 USD
2025-10-10 car-fund -> dining 20 USD
2025-10-20 car-fund -> car-repair 150 USD
";
    with_run(text, day(2025, 12, 31), |book, run| {
        let broken: Vec<_> =
            run.violations.iter().map(|v| run.diagnostics[v.diagnostic as usize].message.as_str()).collect();
        assert_eq!(broken.len(), 1, "only the dinner is outside the envelope's purpose: {broken:?}");
        let lots = &holding(book, run, "savings", "USD").unwrap().lots;
        assert_eq!(lots[0].qty.0, 30_00, "both payments came out of the 200 that was tied");
    });
}

// ─── What a member counts, the household's return reads ─────────────────────

#[test]
fn what_a_members_own_laws_count_is_also_a_line_of_the_households_year() {
    let text = "\
base USD
commodity USD
  precision 2
kind person : entity
kind household : entity
kind ira : asset
  /// Each person has a limit of their own.
  law limit
    on in
    count amount as contributions
    require tally(contributions) <= 700 USD \"over the limit\"

law close
  each year
  count tally(contributions) as reported

account assets/checking
  owner family
account assets/alex-ira : ira
  owner alex
account assets/jordan-ira : ira
  owner jordan

entity family : household
entity alex : person
  member family
entity jordan : person
  member family

opening 2025-01-01
  checking 5_000 USD

2025-04-01 checking -> alex-ira 500 USD
2025-04-02 checking -> jordan-ira 500 USD
";
    with_run(text, day(2026, 1, 31), |book, run| {
        let name = |entity: axiom_core::Id<axiom_model::Entity>| book.name(book.entities[entity].path);
        let counted: Vec<_> = run.effects.iter().map(|e| (book.name(e.name), name(e.owner), e.amount.qty.0)).collect();
        assert_eq!(
            counted,
            [
                ("contributions", "alex", 500_00),
                ("contributions", "jordan", 500_00),
                ("reported", "family", 1_000_00),
            ],
            "the return reads both, and each limit only its own person's"
        );
        assert!(run.violations.is_empty(), "each is under 700.00: {:?}", run.violations);
    });
}

// ─── The facts of a closing day come before the closing ─────────────────────

/// A year's payments toward an estimate, judged by a law that closes the year on
/// January 15.
const CLOSING: &str = "\
base USD
commodity USD
  precision 2

account assets/checking
account expenses/estimated
  law paid
    on in
    count amount as paid

law close
  each year closing 01-15
  require tally(paid) <= 120 USD \"over the estimate\"

opening 2025-01-01
  checking 1_000 USD

2025-06-01 checking -> estimated 100 USD for 2025
";

/// The last payment is dated on the closing day, and written after the flow of a later day.
#[test]
fn a_payment_dated_on_the_closing_day_counts_wherever_the_journal_writes_it() {
    let text = format!(
        "{CLOSING}2026-03-01 checking -> estimated 10 USD\n2026-01-15 checking -> estimated 50 USD for 2025\n"
    );
    with_run(&text, day(2026, 3, 31), |_, run| {
        let said = run.violations.iter().map(|v| run.diagnostics[v.diagnostic as usize].message.as_str());
        let messages: Vec<_> = said.collect();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("150.00 USD in 2025 against a limit of 120.00 USD"), "{}", messages[0]);
    });
}

/// A law that closes on February 29 judges a year on the 28th in a year that has no 29th.
#[test]
fn a_february_29_closing_falls_on_the_28th_in_a_year_without_a_29th() {
    let text = CLOSING.replace("closing 01-15", "closing 02-29").replace("<= 120 USD", "<= 30 USD");
    let text = format!("{text}2027-06-01 checking -> estimated 40 USD for 2027\n");
    with_run(&text, day(2028, 3, 31), |_, run| {
        let judged: Vec<_> = run.violations.iter().map(|v| v.day.to_string()).collect();
        assert_eq!(judged, ["2026-02-28", "2028-02-29"], "the 100.00 USD of 2025, then the 40.00 USD of 2027");
    });
}

/// Periods close up to the journal's last fact, or to today if that is later, and the run says where it stopped.
#[test]
fn the_run_says_the_last_day_it_folded() {
    let text = format!("{CLOSING}2026-03-01 checking -> estimated 10 USD\n");
    with_run(&text, day(2026, 1, 1), |_, run| assert_eq!(run.horizon, day(2026, 3, 1)));
    with_run(&text, day(2026, 6, 1), |_, run| assert_eq!(run.horizon, day(2026, 6, 1)));
}

/// The journal's 100.00 USD payment, as a 50.00 USD one on the closing day that the journal does not hold.
fn payment_on_the_closing_day(book: &Book) -> axiom_model::Flow {
    let mut flow = book.flows[axiom_core::Id::new(1)].clone();
    flow.day = day(2026, 1, 15);
    flow.out.qty = axiom_core::Qty(50_00);
    flow.arrive = flow.out;
    flow
}

/// A withdrawal, or a planned payment, made on the day a year closes is a fact
/// of that day: it comes before the closing when the ledger stops short of it.
#[test]
fn a_flow_applied_on_a_day_whose_closings_are_still_to_come_is_counted_by_them() {
    with_book(CLOSING, |book| {
        let plan = crate::Plan::new(book);
        let mut ledger = plan.start(Options { today: day(2026, 1, 15), relaxed: false });
        ledger.advance_to_closing(day(2026, 1, 15));
        ledger.apply(&payment_on_the_closing_day(book));
        ledger.advance(day(2026, 1, 15));
        assert_eq!(ledger.recorded().violations.len(), 1);
    });
}

/// A pre-closing checkpoint resumes at the same boundary, so a hypothetical
/// payment on its day still reaches that day's law exactly once.
#[test]
fn a_view_checkpoint_preserves_closings_after_same_day_flows() {
    with_book(CLOSING, |book| {
        let day = day(2026, 1, 15);
        let plan = crate::Plan::new(book);
        let mut before_close = plan.start(Options { today: day, relaxed: false });
        before_close.advance_to_closing(day);
        let checkpoint = before_close.checkpoint();

        let mut resumed = plan.resume(&checkpoint, Options { today: day, relaxed: false });
        resumed.apply(&payment_on_the_closing_day(book));
        resumed.advance(day);

        assert_eq!(resumed.recorded().violations.len(), 1, "the same-day payment is judged at closing");
        assert_eq!(resumed.finish().checks[0], 1, "resuming does not close the year twice");
    });
}

#[test]
fn advancing_to_an_earlier_day_keeps_the_checkpoint_boundary() {
    with_book(CLOSING, |book| {
        let day = day(2026, 1, 15);
        let plan = crate::Plan::new(book);
        let mut ledger = plan.start(Options { today: day, relaxed: false });
        ledger.advance_to_closing(day);
        ledger.advance(day.add_days(-1));
        let checkpoint = ledger.checkpoint();

        let mut resumed = plan.resume(&checkpoint, Options { today: day, relaxed: false });
        resumed.apply(&payment_on_the_closing_day(book));
        resumed.advance(day);
        assert_eq!(resumed.finish().checks[0], 1, "an older advance must not close the checkpoint's day");
    });
}

#[test]
fn resuming_after_an_applied_flow_keeps_its_cause_sequence() {
    with_book(CLOSING, |book| {
        let day = day(2026, 1, 15);
        let plan = crate::Plan::new(book);
        let mut ledger = plan.start(Options { today: day, relaxed: false });
        ledger.advance_to_closing(day);
        ledger.apply(&payment_on_the_closing_day(book));
        assert_eq!(ledger.clock.applied, 1);
        let checkpoint = ledger.checkpoint();

        let mut resumed = plan.resume(&checkpoint, Options { today: day, relaxed: false });
        assert_eq!(resumed.clock.applied, 1);
        resumed.apply(&payment_on_the_closing_day(book));
        resumed.advance(day);
        assert_eq!(resumed.clock.applied, 2, "the resumed ledger continues the cause sequence");
    });
}

/// Once the day is closed a flow dated on it is late for its closings, like a
/// journal flow written after them would be: the fold does not travel back.
#[test]
fn a_flow_applied_on_a_day_already_closed_is_late_for_its_closings() {
    with_book(CLOSING, |book| {
        let plan = crate::Plan::new(book);
        let mut ledger = plan.start(Options { today: day(2026, 1, 15), relaxed: false });
        ledger.advance(day(2026, 1, 15));
        ledger.apply(&payment_on_the_closing_day(book));
        ledger.advance(day(2026, 12, 31));
        assert!(ledger.recorded().violations.is_empty());
    });
}

// ─── A tally of another year ────────────────────────────────────────────────

/// What was paid in a year, and each year's law reading the year before it.
const YEARS: &str = "\
base USD
commodity USD
  precision 2

account assets/checking
account expenses/estimated
  law paid
    on in
    count amount as paid

law look-back
  each year
  count tally(paid, year - 1) as before
  count tally(paid) as this

opening 2025-01-01
  checking 1_000 USD

2025-06-01 checking -> estimated 100 USD
2026-06-01 checking -> estimated 30 USD
";

#[test]
fn a_tally_is_read_for_the_year_asked_and_for_this_one_without_asking() {
    with_run(YEARS, day(2026, 12, 31), |book, run| {
        let read = run.effects.iter().filter(|e| book.name(e.name) != "paid");
        let counted: Vec<_> = read.map(|e| (book.name(e.name), e.day.year(), e.amount.qty.0)).collect();
        assert_eq!(
            counted,
            [("this", 2025, 100_00), ("before", 2026, 100_00), ("this", 2026, 30_00)],
            "2025 has no year before it with anything in it, and 2026 reads the 100.00 USD of 2025"
        );
    });
}

// ─── A basis flow moves no quantity ─────────────────────────────────────────

/// A place that holds shares and money, and an improvement to the shares' basis
/// between a payment of an amount to solve and the assertion that solves it.
const REBASED: &str = "\
base USD
commodity USD
  precision 2
commodity UNH

account assets/broker
account assets/checking
account income/discount

opening 2025-01-01
  broker 10 UNH basis 1_000 USD since 2024-01-01
  checking 5_000 USD

2025-02-01 checking -> broker ? USD
2025-02-15 income/discount -> broker[2024-01-01].basis 100 USD
2025-03-01 broker = 500 USD
";

/// The 100.00 USD went into the shares' basis and no money arrived, so the
/// payment before it is the whole 500.00 USD the assertion says the place holds.
#[test]
fn an_amount_to_solve_is_not_short_by_a_flow_that_only_changed_a_basis() {
    with_run(REBASED, day(2025, 6, 1), |book, run| {
        let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.is_error()).map(|d| &d.message).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(holding(book, run, "assets/broker", "USD").map(|h| h.qty()), Some(axiom_core::Qty(500_00)));
    });
}

/// An assertion that fails after a basis flow does not count that flow among the flows that moved
/// the balance it checks.
#[test]
fn a_failed_assertion_does_not_list_a_flow_that_only_changed_a_basis() {
    let text = REBASED.replace("broker ? USD", "broker 300 USD");
    with_run(&text, day(2025, 6, 1), |_, run| {
        let assertion = run.diagnostics.iter().find(|d| &*d.code == "assertion").expect("the assertion fails");
        let listed: Vec<_> = assertion.labels.iter().map(|label| label.text.as_str()).collect();
        assert!(listed.iter().all(|text| !text.contains("income/discount")), "{listed:?}");
        assert!(listed.iter().any(|text| text.contains("from assets/checking")), "{listed:?}");
    });
}

// ─── Which parcel of a currency is spent ────────────────────────────────────

/// Two purchases of euros on different days, and half of what they made spent, in an account that says nothing
/// about lots. `kind` is what the euros are: legal tender, or a thing kept for what it will fetch.
fn spent_euros(kind: &str) -> String {
    format!(
        "\
base USD
commodity USD
  precision 2
kind currency : commodity
  select fifo
kind good : commodity
commodity EUR : {kind}
  precision 2

account assets/checking
account assets/wallet
account expenses/food

opening 2025-01-01
  checking 5_000 USD

2025-01-05 checking 1_100 USD -> wallet 1_000 EUR
2025-02-05 checking 1_150 USD -> wallet 1_000 EUR
2025-03-01 EUR 1.2 USD
2025-03-01 wallet -> food 1_500 EUR
"
    )
}

#[test]
fn a_currency_is_spent_oldest_first_wherever_it_is_held() {
    with_run(&spent_euros("currency"), day(2025, 3, 31), |book, run| {
        assert!(run.diagnostics.iter().all(|d| d.code != "ambiguous-lots"), "{:?}", run.diagnostics);
        let lots = &holding(book, run, "wallet", "EUR").unwrap().lots;
        assert_eq!(lots.iter().map(|lot| (lot.qty.0, lot.basis.0)).collect::<Vec<_>>(), [(500_00, 575_00)]);
    });
}

#[test]
fn anything_else_that_differs_is_still_ambiguous_without_a_policy() {
    with_run(&spent_euros("good"), day(2025, 3, 31), |_, run| {
        assert!(run.diagnostics.iter().any(|d| d.code == "ambiguous-lots"));
    });
}

// ─── Claims made by the legs of a split ─────────────────────────────────────

#[test]
fn each_leg_of_a_split_is_a_claim_on_its_own_debtor_and_falls_due_on_its_own_day() {
    let text = "\
base USD
commodity USD
  precision 2
kind receivable : asset
  claim
kind org : entity

account assets/checking
account assets/owed/ben : receivable
account assets/owed/cleo : receivable
account expenses/rent

entity ben : org
  via assets/owed/ben
entity cleo : org
  via assets/owed/cleo
entity landlord : org
  via expenses/rent

opening 2025-02-01
  checking 5_000 USD

2025-03-01 checking -> 3_150 USD / landlord #rent due 2025-03-08
  rent  1_050 USD
  ben   1_050 USD
  cleo  1_050 USD due 2025-03-15
";
    with_run(text, day(2025, 4, 1), |_, run| {
        let overdue: Vec<_> =
            run.diagnostics.iter().filter(|d| d.code == "overdue").map(|d| d.message.as_str()).collect();
        assert_eq!(
            overdue,
            [
                "ben still owes 1,050.00 USD, 24 days past its due day 2025-03-08",
                "cleo still owes 1,050.00 USD, 17 days past its due day 2025-03-15",
            ]
        );
    });
}

// ─── The fee legs of an exchange ────────────────────────────────────────────

const TRADES: &str = "\
base USD
commodity USD
  precision 2
commodity VTI
  precision 0

account assets/broker
account assets/checking
account expenses/fees

opening 2024-01-01
  checking 5_000 USD
  broker   10 VTI basis 1_000 USD since 2020-01-01
";

#[test]
fn a_fee_leg_of_a_sale_comes_off_its_proceeds() {
    let text = format!("{TRADES}2025-03-01 broker 10 VTI -> 1_500 USD\n  checking 1_490 USD\n  fees 10 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        let [sale] = run.gains[..] else { panic!("one disposal: {:?}", run.gains) };
        assert_eq!((sale.proceeds.0, sale.basis.0, sale.gain().0), (1_490_00, 1_000_00, 490_00));
        let usd = |place| holding(book, run, place, "USD").unwrap().qty().0;
        assert_eq!((usd("checking"), usd("fees")), (5_000_00 + 1_490_00, 10_00), "the fee is still an expense");
    });
}

#[test]
fn a_fee_leg_of_a_purchase_is_part_of_what_the_shares_cost() {
    let text = format!("{TRADES}2025-04-01 checking -> 2_000 USD\n  broker 7 VTI\n  fees 5 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        let lots = &holding(book, run, "broker", "VTI").unwrap().lots;
        let bought: Vec<_> = lots.iter().map(|lot| (lot.qty.0, lot.basis.0)).collect();
        assert_eq!(bought, [(10, 1_000_00), (7, 1_995_00 + 5_00)], "1,995.00 for the shares and 5.00 to buy them");
    });
}

#[test]
fn a_purchase_that_states_its_basis_keeps_it() {
    let text = format!("{TRADES}2025-04-01 checking -> 2_000 USD basis 1_990 USD\n  broker 7 VTI\n  fees 5 USD\n");
    with_run(&text, day(2025, 12, 31), |book, run| {
        let lots = &holding(book, run, "broker", "VTI").unwrap().lots;
        assert_eq!(lots.last().unwrap().basis.0, 1_990_00);
    });
}

// ─── Value recognized ahead of the window it belongs to ─────────────────────

const INSURANCE: &str = "\
base USD
commodity USD
  precision 2

account assets/checking
account expenses/insurance
  budget 1_200 USD yearly
account expenses/rent
  budget 100 USD monthly

opening 2025-09-01
  checking 5_000 USD
";

/// What the limits read in each window, as `(first day, counted)`, for one place's budget.
fn read(book: &Book, run: &Run, place: &str) -> Vec<(String, i64)> {
    let subject = axiom_model::Subject::Place(book.place(place).unwrap());
    let readings = run.headroom.iter().filter(|reading| reading.subject == subject);
    readings.map(|reading| (reading.days.first().to_string(), reading.counted.qty.0)).collect()
}

fn broken<'r>(book: &Book, run: &'r Run) -> Vec<&'r str> {
    let named = |v: &&crate::Violation| book.name(book.laws[v.law].name) == "budget";
    run.violations.iter().filter(named).map(|v| run.diagnostics[v.diagnostic as usize].message.as_str()).collect()
}

#[test]
fn a_flow_recognized_for_next_year_is_in_next_years_headroom_before_anything_else_lands_there() {
    let text = format!("{INSURANCE}2025-12-12 checking -> insurance 1_140 USD for 2026\n");
    with_run(&text, day(2026, 2, 14), |book, run| {
        let years = read(book, run, "insurance");
        assert_eq!(years, [("2025-01-01".into(), 0), ("2026-01-01".into(), 1_140_00)]);
        assert!(broken(book, run).is_empty());
    });
}

#[test]
fn an_accrual_that_alone_breaks_a_limit_is_reported_once_however_much_lands_after() {
    let text = format!(
        "{INSURANCE}2025-12-12 checking -> insurance 1_300 USD for 2026\n2026-01-05 checking -> insurance 50 USD\n"
    );
    with_run(&text, day(2026, 2, 14), |book, run| {
        let broken = broken(book, run);
        assert_eq!(broken.len(), 1, "{broken:?}");
        assert!(broken[0].contains("1,300.00 USD in 2026 against a limit of 1,200.00 USD, over by 100.00 USD"));
        assert_eq!(read(book, run, "insurance").last().unwrap().1, 1_350_00, "and the reading goes on counting");
    });
}

#[test]
fn a_range_reaches_each_month_it_covers_up_to_where_the_fold_has_got() {
    // 121 days from December to March: 31, 31, 28 and 31 of them.
    let text = format!("{INSURANCE}2025-12-01..2026-03-31 checking -> rent 1_200 USD\n");
    with_run(&text, day(2026, 1, 31), |book, run| {
        let months = read(book, run, "rent");
        assert_eq!(months, [("2025-12-01".into(), 307_44), ("2026-01-01".into(), 307_44)], "February is not here yet");
        assert_eq!(broken(book, run).len(), 2);
    });
    with_run(&text, day(2026, 6, 30), |book, run| {
        let months = read(book, run, "rent");
        let counted: Vec<_> = months.iter().map(|(_, counted)| *counted).collect();
        assert_eq!(counted, [307_44, 307_44, 277_68, 307_44], "the shares add up to the 1,200.00 USD");
        assert_eq!(broken(book, run).len(), 4);
    });
}

#[test]
fn a_planned_flow_reaches_the_months_ahead_as_its_ledger_advances() {
    let text = format!("{INSURANCE}2025-12-01 checking -> rent 50 USD\n");
    with_book(&text, |book| {
        let plan = crate::Plan::new(book);
        let mut ledger = plan.start(Options { today: day(2025, 12, 31), relaxed: false });
        ledger.advance(day(2025, 12, 31));
        // The rent flow again, 81 days from the tenth of January: 22, 28 and 31 of them.
        let mut prepaid = book.flows[axiom_core::Id::new(1)].clone();
        prepaid.day = day(2026, 1, 10);
        prepaid.recognized = axiom_core::Days::new(day(2026, 1, 10), day(2026, 3, 31)).unwrap();
        prepaid.out.qty = axiom_core::Qty(1_200_00);
        prepaid.arrive = prepaid.out;
        let mut fork = ledger.fork();
        fork.apply(&prepaid);
        assert_eq!(fork.recorded().violations.len(), 1, "January, where the flow lands");
        fork.advance(day(2026, 3, 31));
        assert_eq!(fork.recorded().violations.len(), 3, "and February and March as the fork gets there");
    });
}
