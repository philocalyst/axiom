//! Folds over hand-built books: the promises the engine makes, end to end.
//!
//! Amounts are written in cents as `1_000_00` (a thousand and no cents), which
//! clippy reads as inconsistent digit grouping.
#![allow(clippy::inconsistent_digit_grouping)]

use axiom_core::{Day, Diagnostic, Id, Qty, Ratio, Severity};
use axiom_model::*;

use crate::fixture::{Fixture, LawBuilder};
use crate::{Cause, Holding, Ledger, Options, Owed, Parcel, Run, State, run};

fn options() -> Options {
    Options { today: Day(1000), relaxed: false }
}

fn held(run: &Run, place: Id<Place>, unit: Id<Commodity>) -> Option<&Holding> {
    run.holdings.iter().find(|h| h.place == place && h.unit == unit)
}

fn qty(run: &Run, place: Id<Place>, unit: Id<Commodity>) -> i64 {
    held(run, place, unit).map_or(0, |h| h.qty().0)
}

fn diagnostic<'a>(run: &'a Run, code: &str) -> &'a Diagnostic {
    run.diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no `{code}` diagnostic in {:?}", run.diagnostics))
}

#[test]
fn an_empty_book_folds_to_nothing() {
    let book = Fixture::new().book();
    let mut ledger = Ledger::new(&book, options());
    ledger.advance(Day(500));
    assert_eq!(ledger.day(), Day(500));
    let run = ledger.finish();
    assert!(run.posted.is_empty() && run.holdings.is_empty() && run.diagnostics.is_empty());
}

#[test]
fn plain_money_is_one_integer_per_place() {
    let mut f = Fixture::new();
    let (equity, checking, food, salary, usd) = (f.equity, f.checking, f.food, f.salary, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.flow(2, checking, food, 20_00);
    f.flow(3, salary, checking, 500_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(qty(&run, checking, usd), 1_480_00);
    assert_eq!(qty(&run, food, usd), 20_00);
    assert_eq!(qty(&run, equity, usd), -1_000_00, "sources go negative: balances are inflow minus outflow");
    assert!(run.holdings.iter().all(|h| h.lots.is_empty()));
    assert!(run.diagnostics.is_empty());
}

fn brokerage_book(policy: Option<Policy>) -> (Book<'static>, Fixture) {
    let mut f = Fixture::new();
    let (equity, checking) = (f.equity, f.checking);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 1_500_00, 10);
    let sale = f.sell(4, 5, 800_00);
    if let Some(policy) = policy {
        f.flows[sale.index()].select = Box::new([Select::Policy(policy)]);
    }
    let names = Fixture::new();
    (f.book(), names)
}

#[test]
fn ambiguous_lots_report_candidates_with_gains_and_fall_back_to_fifo() {
    let (book, f) = brokerage_book(None);
    let run = run(&book, options());
    let d = diagnostic(&run, "ambiguous-lots");
    assert_eq!(d.severity, Severity::Error);
    let rows: Vec<_> = d.labels.iter().filter(|l| !l.primary).map(|l| l.text.as_str()).collect();
    assert_eq!(
        rows,
        [
            "acquired 1970-01-03: 10 VTI, basis 1,000.00 USD; from it alone the gain is 300.00 USD",
            "acquired 1970-01-04: 10 VTI, basis 1,500.00 USD; from it alone the gain is 50.00 USD",
        ]
    );
    let gain = run.gains[0];
    assert!(gain.ambiguous);
    assert_eq!((gain.qty, gain.basis, gain.proceeds, gain.acquired), (Qty(5), Qty(500_00), Qty(800_00), Day(2)));
    assert_eq!(gain.gain(), Qty(300_00), "first in, first out");
    drop(f);
}

#[test]
fn a_policy_decides_and_hifo_sells_the_dearest_lot() {
    let (book, _) = brokerage_book(Some(Policy::Hifo));
    let run = run(&book, options());
    assert!(run.diagnostics.is_empty());
    let gain = run.gains[0];
    assert_eq!((gain.basis, gain.gain(), gain.acquired, gain.ambiguous), (Qty(750_00), Qty(50_00), Day(3), false));
    let brokerage = held(&run, book.places.ids().nth(4).unwrap(), Id::new(1)).unwrap();
    let lots: Vec<_> = brokerage.lots.iter().map(|l| (l.qty.0, l.basis.0)).collect();
    assert_eq!(lots, [(10, 1_000_00), (5, 750_00)]);
}

#[test]
fn all_realizes_every_lot_at_its_share_of_the_proceeds() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, usd, vti) = (f.equity, f.checking, f.brokerage, f.usd, f.vti);
    f.flow(1, equity, checking, 10_000_00);
    f.buy(2, 1_000_00, 10);
    f.buy(3, 1_500_00, 10);
    let sale = f.sell_all(4, 3_000_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[sale.index()].out, Qty(20));
    let gains: Vec<_> = run.gains.iter().map(|g| (g.qty.0, g.gain().0)).collect();
    assert_eq!(gains, [(10, 500_00), (10, 0)]);
    assert!(run.diagnostics.is_empty(), "taking everything leaves no choice to make");
    assert_eq!(qty(&run, brokerage, vti), 0);
    assert_eq!(qty(&run, checking, usd), 10_000_00 - 2_500_00 + 3_000_00);
}

#[test]
fn a_sale_of_more_than_is_held_leaves_a_negative_balance() {
    let mut f = Fixture::new();
    let (equity, checking, brokerage, vti) = (f.equity, f.checking, f.brokerage, f.vti);
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 700_00, 7);
    f.sell(3, 10, 1_300_00);
    let book = f.book();
    let run = run(&book, options());
    let d = diagnostic(&run, "insufficient-holding");
    assert_eq!(d.message, "assets/brokerage does not hold 10 VTI");
    assert_eq!(qty(&run, brokerage, vti), -3);
    assert_eq!(held(&run, brokerage, vti).unwrap().plain, Qty(-3));
    assert_eq!(run.gains[0].gain(), Qty(210_00), "the seven shares held sold at their share of the proceeds");
}

/// `total(in, year) <= 24_500 USD`, on flows into the retirement account.
fn deferral_limit(f: &mut Fixture) -> Id<Law> {
    let (name, doc) = (f.sym("deferral-limit"), f.sym("/// Elective deferrals are capped per calendar year."));
    let limit = f.usd(24_500_00);
    let mut law = LawBuilder::new(name, Trigger::In).doc(doc);
    let total = law.call(Func::Total(Dir::In, Window::Year), &[], Ty::Amount);
    let cap = law.konst(Value::Amount(limit), Ty::Amount);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.require(cond, None));
    let (retirement, rule) = (f.retirement, f.rule(law, Subject::Place(f.retirement)));
    f.on_in.push((retirement, rule));
    law
}

#[test]
fn a_failed_law_explains_itself_power_assert_style() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    let law = deferral_limit(&mut f);
    f.flow(10, salary, retirement, 22_700_00);
    let over = f.flow(20, salary, retirement, 2_600_00);
    let book = f.book();
    let run = run(&book, options());

    assert_eq!(run.checks[law.index()], 2);
    assert_eq!(run.violations.len(), 1);
    let violation = run.violations[0];
    assert_eq!((violation.cause, violation.warn, violation.waived), (Cause::Flow(over), false, false));
    let d = &run.diagnostics[violation.diagnostic as usize];
    assert_eq!((&*d.code, d.severity), ("law", Severity::Error));
    assert_eq!(d.message, "Elective deferrals are capped per calendar year.");
    let primary = d.labels.iter().find(|l| l.primary).unwrap();
    assert_eq!(primary.loc, book.flows[over].loc);
    assert_eq!(primary.text, "this flow: 2,600.00 USD into assets/retirement");
    let values: Vec<_> = d.labels.iter().filter(|l| !l.primary).map(|l| l.text.as_str()).collect();
    assert_eq!(values, ["25,300.00 USD", "false"], "each non-constant subexpression, with its value");
    assert!(d.labels.iter().filter(|l| !l.primary).all(|l| l.loc.file.0 == 1), "located in the law's own source");
    assert_eq!(d.help[0].text, "at most 1,800.00 USD more can go in this year");
}

#[test]
fn a_tally_bound_names_the_room_left_when_the_law_counted_this_flow() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    let (name, deferrals, message) =
        (f.sym("deferral-limit"), f.sym("elective-deferrals"), f.sym("401(k) deferrals over the yearly limit"));
    let mut law = LawBuilder::new(name, Trigger::In);
    let amount = law.var(Var::Amount, Ty::Amount);
    let counted = law.call(Func::Tally(deferrals), &[], Ty::Amount);
    let cap = law.konst(Value::Amount(f.usd(24_500_00)), Ty::Amount);
    let cond = law.bin(BinOp::Le, counted, cap, Ty::Bool);
    let law = f.law(law.count(amount, deferrals).require(cond, Some(message)));
    let rule = f.rule(law, Subject::Entity(f.me));
    f.on_in.push((retirement, rule));
    f.flow(10, salary, retirement, 22_700_00);
    f.flow(20, salary, retirement, 26_000_00);
    let book = f.book();
    let run = run(&book, options());
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert_eq!(d.message, "401(k) deferrals over the yearly limit");
    assert_eq!(d.help[0].text, "at most 1,800.00 USD more can count toward `elective-deferrals` this year");
}

#[test]
fn a_waived_transaction_or_a_relaxed_book_demotes_the_error() {
    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    deferral_limit(&mut f);
    let over = f.flow(20, salary, retirement, 25_000_00);
    let loc = f.flows[over.index()].loc;
    f.flows[over.index()].waive = Some(Waive { loc, reason: None });
    let book = f.book();
    let run = run(&book, options());
    assert!(run.violations[0].waived);
    assert_eq!(run.diagnostics[0].severity, Severity::Warning);
    assert!(run.diagnostics[0].labels.iter().any(|l| l.text == "waived here"));

    let mut f = Fixture::new();
    let (salary, retirement) = (f.salary, f.retirement);
    deferral_limit(&mut f);
    f.flow(20, salary, retirement, 25_000_00);
    let book = f.book();
    let relaxed = crate::run(&book, Options { relaxed: true, ..options() });
    assert_eq!(relaxed.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn value_moving_inside_a_subject_does_not_count_toward_its_totals() {
    let mut f = Fixture::new();
    let (equity, salary, checking, savings, assets) = (f.equity, f.salary, f.checking, f.savings, f.assets);
    let (name, cap) = (f.sym("monthly-cap"), f.usd(10_000_00));
    let mut law = LawBuilder::new(name, Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::Amount);
    let cap = law.konst(Value::Amount(cap), Ty::Amount);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.require(cond, None));
    for place in [checking, savings] {
        let rule = f.rule(law, Subject::Place(assets));
        f.on_in.push((place, rule));
    }
    f.flow(1, equity, checking, 5_000_00);
    f.flow(2, checking, savings, 3_000_00);
    f.flow(3, salary, savings, 4_000_00);
    let last = f.flow(4, salary, checking, 1_500_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.checks[law.index()], 3, "the transfer between two places under `assets` is skipped");
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].cause, Cause::Flow(last));
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert!(d.labels.iter().any(|l| l.text == "10,500.00 USD"), "9,000 already in, 1,500 more: {d:?}");
}

#[test]
fn unknown_amounts_are_solved_and_a_failed_assertion_names_the_flows_since() {
    let mut f = Fixture::new();
    let (equity, checking, cash, food) = (f.equity, f.checking, f.cash, f.food);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 1_000_00);
    let atm = f.unknown(3, checking, cash);
    f.flow(4, checking, food, 20_00);
    f.assert(5, checking, 700_00);
    let lunch = f.flow(6, checking, food, 10_00);
    f.assert(7, checking, 600_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[atm.index()].out, Qty(280_00));
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    let d = errors[0];
    assert_eq!((&*d.code, d.message.as_str()), ("assertion", "assets/checking holds 690.00 USD, not 600.00 USD"));
    assert_eq!(d.labels[0].text, "90.00 USD too much: the ledger holds more than this");
    let since: Vec<_> = d.labels.iter().skip(1).map(|l| (l.loc, l.text.as_str())).collect();
    assert_eq!(
        since,
        [(book.flows[lunch].loc, "-10.00 USD to expenses/food")],
        "only the flows after the last assertion that held"
    );
    assert_eq!(d.help[0].edit.as_ref().unwrap().1, " !");
}

#[test]
fn liabilities_are_asserted_and_solved_in_the_sign_they_are_read() {
    let mut f = Fixture::new();
    let (card, checking, food, cash) = (f.card, f.checking, f.food, f.cash);
    f.flow(1, card, food, 84_00);
    f.assert(2, card, 84_00);
    let advance = f.unknown(3, card, cash);
    f.assert(4, card, 184_00);
    f.flow(5, checking, card, 30_00);
    f.assert(6, card, 154_00);
    f.assert(7, card, 100_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[advance.index()].out, Qty(100_00), "100 more owed: the card paid out 100");
    let errors: Vec<_> = run.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].message, "liabilities/card holds 154.00 USD, not 100.00 USD");
    assert_eq!(qty(&run, card, Id::new(0)), -154_00, "a liability's balance is naturally negative");
}

#[test]
fn only_the_unwritten_side_of_an_exchange_is_unknown() {
    let mut f = Fixture::new();
    let (equity, checking) = (f.equity, f.checking);
    f.flow(1, equity, checking, 5_000_00);
    f.assert(2, checking, 5_000_00);
    let purchase = f.unknown_buy(3, 7);
    f.assert(4, checking, 3_000_10);
    let book = f.book();
    let run = run(&book, options());
    let posted = run.posted[purchase.index()];
    assert_eq!(
        (posted.out, posted.arrive),
        (Qty(1_999_90), Qty(7)),
        "the 7 shares were written; the price is what the bank says"
    );
    assert!(run.diagnostics.is_empty(), "{:?}", run.diagnostics);
}

#[test]
fn an_assertion_that_accepts_a_gap_cannot_solve_an_unknown_amount() {
    let mut f = Fixture::new();
    let (equity, checking, cash, food) = (f.equity, f.checking, f.cash, f.food);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 1_000_00);
    let atm = f.unknown(3, checking, cash);
    f.assert(4, checking, 900_00);
    f.flow(5, cash, food, 22_50);
    f.assert(6, cash, 63_00);
    f.pad_last();
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[atm.index()].out, Qty(100_00), "the bank's number, not the wallet's, which admits a gap");
    assert_eq!(run.pads[0].amount.qty, Qty(-14_50));
}

#[test]
fn a_padded_assertion_moves_the_gap_from_unknown() {
    let mut f = Fixture::new();
    let (equity, checking, unknown, usd) = (f.equity, f.checking, f.unknown, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.assert(2, checking, 900_00);
    f.pad_last();
    f.assert(3, checking, 900_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.pads.len(), 1);
    assert_eq!(run.pads[0].amount.qty, Qty(-100_00));
    assert_eq!(qty(&run, checking, usd), 900_00);
    assert_eq!(qty(&run, unknown, usd), 100_00);
    assert_eq!(diagnostic(&run, "pad").severity, Severity::Note);
    assert!(run.diagnostics.iter().all(|d| !d.is_error()), "the second assertion holds after the pad");
}

#[test]
fn a_target_leg_is_whatever_reaches_the_balance() {
    let mut f = Fixture::new();
    let (equity, checking, savings, usd) = (f.equity, f.checking, f.savings, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    let sweep = f.target_leg(2, checking, savings, End::From, 400_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.posted[sweep.index()].out, Qty(600_00));
    assert_eq!((qty(&run, checking, usd), qty(&run, savings, usd)), (400_00, 600_00));
}

#[test]
fn deferred_money_has_no_basis_and_a_withdrawal_realizes_all_of_it() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, usd) = (f.salary, f.retirement, f.checking, f.usd);
    let (name, penalty, irs) = (f.sym("early-withdrawal"), f.sym("penalty"), f.grant);
    let mut law = LawBuilder::new(name, Trigger::Gain);
    let tenth = law.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num);
    let gain = law.var(Var::Gain, Ty::Amount);
    let tax = law.bin(BinOp::Mul, tenth, gain, Ty::Amount);
    let law = f.law(law.owe(tax, irs, penalty));
    let rule = f.rule(law, Subject::Place(retirement));
    f.on_gain.push((retirement, rule));
    f.flow(1, salary, retirement, 10_000_00);
    let withdrawal = f.flow(2, retirement, checking, 4_000_00);
    let book = f.book();
    let run = run(&book, options());

    assert_eq!(run.gains.len(), 1);
    let gain = run.gains[0];
    assert_eq!((gain.basis, gain.proceeds, gain.gain()), (Qty::ZERO, Qty(4_000_00), Qty(4_000_00)));
    assert_eq!(gain.cause, Cause::Flow(withdrawal));
    let effect = run.effects[0];
    assert_eq!((effect.amount.qty, effect.amount.unit, effect.name), (Qty(400_00), usd, penalty));
    assert_eq!(effect.owe, Some(Owed { to: irs, due: Day(2) }));
    let left = held(&run, retirement, usd).unwrap();
    assert_eq!(
        (left.plain, left.lots.iter().map(|l| (l.qty.0, l.basis.0)).collect::<Vec<_>>()),
        (Qty::ZERO, vec![(6_000_00, 0)])
    );
    assert_eq!(held(&run, checking, usd).unwrap().plain, Qty(4_000_00), "taxed money arrives as plain money");
}

#[test]
fn a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot() {
    let mut f = Fixture::new();
    let (equity, salary, checking, retirement, usd) = (f.equity, f.salary, f.checking, f.retirement, f.usd);
    f.places[retirement].select = Some(Policy::Prorata);
    f.flow(1, equity, checking, 6_300_00);
    f.flow(2, checking, retirement, 6_300_00);
    f.flow(3, salary, retirement, 1_000_00);
    f.flow(9, salary, retirement, 1_200_00);
    f.flow(10, retirement, checking, 1_500_00);
    let book = f.book();
    let run = run(&book, options());
    assert!(run.diagnostics.is_empty(), "{:?}", run.diagnostics);
    let held = held(&run, retirement, usd).unwrap();
    assert_eq!(held.plain, Qty(6_300_00 - 1_111_76), "after-tax contributions are plain money");
    assert_eq!(held.lots.len(), 1, "two deferrals of zero-basis money are one lot");
    assert_eq!((held.lots[0].qty, held.lots[0].basis), (Qty(2_200_00 - 388_24), Qty::ZERO));
    assert_eq!(run.gains.len(), 1, "plain money never realizes");
    assert_eq!((run.gains[0].qty, run.gains[0].gain()), (Qty(388_24), Qty(388_24)));
}

#[test]
fn a_require_with_an_else_prices_the_violation_instead_of_failing() {
    let mut f = Fixture::new();
    let (salary, retirement, checking, irs) = (f.salary, f.retirement, f.checking, f.grant);
    let (name, penalty) = (f.sym("early-withdrawal"), f.sym("penalty"));
    let mut law = LawBuilder::new(name, Trigger::Gain);
    let gain = law.var(Var::Gain, Ty::Amount);
    let nothing = law.konst(Value::Empty, Ty::Empty);
    let cond = law.bin(BinOp::Le, gain, nothing, Ty::Bool);
    let tenth = law.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num);
    let again = law.var(Var::Gain, Ty::Amount);
    let tax = law.bin(BinOp::Mul, tenth, again, Ty::Amount);
    let law = f.law(law.require_else_owe(cond, tax, irs, penalty));
    let rule = f.rule(law, Subject::Place(retirement));
    f.on_gain.push((retirement, rule));
    f.flow(1, salary, retirement, 5_000_00);
    f.flow(2, retirement, checking, 1_000_00);
    let book = f.book();
    let run = run(&book, options());
    assert!(run.violations.is_empty() && run.diagnostics.is_empty(), "a priced violation is not a failure");
    assert_eq!(run.effects.len(), 1);
    assert_eq!((run.effects[0].amount.qty, run.effects[0].name), (Qty(100_00), penalty));
}

#[test]
fn restricted_money_stays_tied_and_is_spent_first_only_where_its_laws_permit() {
    let mut f = Fixture::new();
    let (equity, grants, checking, savings, food, unknown, usd) =
        (f.equity, f.grants, f.checking, f.savings, f.food, f.unknown, f.usd);
    let (nsf, name) = (f.grant, f.sym("grant-purpose"));
    let mut law = LawBuilder::new(name, Trigger::Spend);
    let to = law.var(Var::To, Ty::Place);
    let allowed = law.konst(Value::Place(food), Ty::Place);
    let cond = law.is(to, &[allowed]);
    let law = f.law(law.require(cond, None));
    let rule = f.rule(law, Subject::Entity(nsf));
    f.on_spend.push((nsf, rule));

    f.flow(1, equity, checking, 1_000_00);
    let award = f.flow(2, grants, checking, 5_000_00);
    f.flows[award.index()].payee = Some(nsf);
    f.flow(3, checking, food, 800_00);
    let misuse = f.flow(4, checking, unknown, 1_200_00);
    f.flow(5, checking, savings, 500_00);
    let book = f.book();
    let run = run(&book, options());

    let tied = |amount: i64, txn: u32| Parcel {
        qty: Qty(amount),
        basis: Qty(amount),
        acquired: Day(2),
        txn: Id::new(txn),
        tied: Some(nsf),
    };
    let checking_held = held(&run, checking, usd).unwrap();
    assert_eq!(checking_held.plain, Qty::ZERO);
    assert_eq!(checking_held.lots, [tied(3_500_00, 1)], "800 went on the purpose, then plain 1,000 before tied 200");
    let savings_held = held(&run, savings, usd).unwrap();
    assert_eq!(savings_held.lots, [tied(500_00, 1)], "the tie travels with an internal transfer");
    assert_eq!(run.checks[law.index()], 2, "it fires only for tied money leaving the owner's places");
    assert_eq!(run.violations.len(), 1);
    assert_eq!(run.violations[0].cause, Cause::Flow(misuse));
    assert_eq!(run.violations[0].subject, Subject::Entity(nsf));
}

#[test]
fn a_pending_flow_lands_on_its_settlement_day_and_a_typo_gets_a_suggestion() {
    let mut f = Fixture::new();
    let (equity, checking, food, usd) = (f.equity, f.checking, f.food, f.usd);
    f.flow(1, equity, checking, 100_00);
    let check = f.flow(2, checking, food, 50_00);
    f.pending(check, "#c1");
    f.event(5, "#c1", EventState::Settled);
    f.event(6, "#c2", EventState::Void);
    let book = f.book();
    let mut ledger = Ledger::new(&book, options());
    ledger.advance(Day(4));
    assert_eq!(ledger.balance(checking, usd), Qty(100_00), "pending money has not moved");
    ledger.advance(Day(5));
    assert_eq!(ledger.balance(checking, usd), Qty(50_00));
    let run = ledger.finish();
    assert_eq!(run.posted[check.index()].state, State::Settled(Day(5)));
    let d = diagnostic(&run, "unknown-code");
    assert_eq!(d.help[0].text, "did you mean `#c1`?");
}

/// A monthly `count` law and a `by` law due on 1970-02-10 (day 40), over one flow on day 1.
fn timed_book(assertion_on: Option<i32>) -> (Book<'static>, Id<Law>) {
    let mut f = Fixture::new();
    let (equity, cash, me) = (f.equity, f.cash, f.me);
    let (monthly, deadline, months) = (f.sym("monthly"), f.sym("payoff"), f.sym("months"));
    let one = f.usd(100);
    let mut each = LawBuilder::new(monthly, Trigger::Each(Period::Month, None));
    let amount = each.konst(Value::Amount(one), Ty::Amount);
    let each = f.law(each.count(amount, months));
    let mut by = LawBuilder::new(deadline, Trigger::Always);
    let (y, m, d) = (
        by.konst(Value::Num(Ratio::int(1970)), Ty::Num),
        by.konst(Value::Num(Ratio::int(2)), Ty::Num),
        by.konst(Value::Num(Ratio::int(10)), Ty::Num),
    );
    let date = by.call(Func::Date, &[y, m, d], Ty::Day);
    by.by(date);
    let balance = by.var(Var::Balance, Ty::Amount);
    let nothing = by.konst(Value::Empty, Ty::Empty);
    let cond = by.bin(BinOp::Eq, balance, nothing, Ty::Bool);
    let by = f.law(by.require(cond, None));
    let (each_rule, by_rule) = (f.rule(each, Subject::Entity(me)), f.rule(by, Subject::Place(cash)));
    f.timed.extend([each_rule, by_rule]);
    f.flow(1, equity, cash, 5_00);
    if let Some(day) = assertion_on {
        f.assert(day, cash, 5_00);
    }
    (f.book(), each)
}

#[test]
fn balance_reads_in_the_display_sign_and_a_lasting_condition_is_reported_once() {
    let mut f = Fixture::new();
    let (card, checking, food, equity) = (f.card, f.checking, f.food, f.equity);
    let (name, limit) = (f.sym("card-limit"), f.usd(100_00));
    let mut law = LawBuilder::new(name, Trigger::Always);
    let owed = law.var(Var::Balance, Ty::Amount);
    let cap = law.konst(Value::Amount(limit), Ty::Amount);
    let cond = law.bin(BinOp::Le, owed, cap, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(card));
    f.always.push((card, rule));
    f.flow(1, equity, checking, 1_000_00);
    f.flow(2, card, food, 84_00);
    let over = f.flow(3, card, food, 30_00);
    f.flow(4, card, food, 5_00);
    f.flow(5, checking, card, 60_00);
    let again = f.flow(6, card, food, 60_00);
    let book = f.book();
    let run = run(&book, options());
    let causes: Vec<_> = run.violations.iter().map(|v| v.cause).collect();
    assert_eq!(
        causes,
        [Cause::Flow(over), Cause::Flow(again)],
        "owed 114, still owed 119 (quiet), paid down to 59, owed 119"
    );
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert!(d.labels.iter().any(|l| l.text == "114.00 USD"), "what is owed, not the negative balance: {d:?}");
}

#[test]
fn a_returned_flow_is_reversed_on_the_day_of_the_return() {
    let mut f = Fixture::new();
    let (equity, checking, food, usd) = (f.equity, f.checking, f.food, f.usd);
    f.flow(1, equity, checking, 100_00);
    let bounced = f.flow(2, checking, food, 30_00);
    let instant = f.flow(2, checking, food, 5_00);
    f.mark(bounced, "#b1");
    f.mark(instant, "#b2");
    f.event(5, "#b1", EventState::Returned);
    f.event(2, "#b2", EventState::Returned);
    let book = f.book();
    let mut ledger = Ledger::new(&book, options());
    ledger.advance(Day(4));
    assert_eq!(ledger.balance(checking, usd), Qty(70_00), "the returned flow counted until it was returned");
    ledger.advance(Day(5));
    assert_eq!(ledger.balance(checking, usd), Qty(100_00), "and its reversal put everything back");
    let run = ledger.finish();
    assert_eq!(run.posted[bounced.index()].state, State::Returned(Day(5)));
    assert_eq!(run.posted[instant.index()].state, State::Void, "returned on its own day: it never happened");
    assert_eq!(qty(&run, food, usd), 0);
}

#[test]
fn periods_and_deadlines_fire_as_the_journal_reaches_them_and_never_past_today() {
    let (book, each) = timed_book(None);
    let run = run(&book, Options { today: Day(70), relaxed: false });
    let days: Vec<_> = run.effects.iter().map(|e| (e.day, e.cause)).collect();
    assert_eq!(
        days,
        [(Day(30), Cause::Time), (Day(58), Cause::Time)],
        "January and February closed; March 31 is past today"
    );
    assert_eq!(run.violations.len(), 1);
    assert_eq!((run.violations[0].day, run.violations[0].cause), (Day(40), Cause::Time));
    assert_eq!(run.checks[each.index()], 2);
}

#[test]
fn a_deadline_the_journal_itself_reaches_fires_even_before_today() {
    let (book, _) = timed_book(Some(45));
    let run = run(&book, Options { today: Day(10), relaxed: false });
    assert_eq!(run.violations.len(), 1, "the assertion on day 45 carries the journal past the deadline on day 40");
    assert_eq!(
        run.effects.iter().map(|e| e.day).collect::<Vec<_>>(),
        [Day(30)],
        "and past January's end, but no further"
    );
    let (book, _) = timed_book(None);
    let quiet = crate::run(&book, Options { today: Day(10), relaxed: false });
    assert!(
        quiet.violations.is_empty() && quiet.effects.is_empty(),
        "with nothing after today, the future stays unfired"
    );
}

#[test]
fn tallies_count_what_a_filter_lets_through_and_year_end_laws_read_them() {
    let mut f = Fixture::new();
    let (equity, salary, checking, me, irs) = (f.equity, f.salary, f.checking, f.me, f.grant);
    let (counting, taxing, wages, tax) =
        (f.sym("count-wages"), f.sym("income-tax"), f.sym("wages"), f.sym("federal-tax"));

    let mut count = LawBuilder::new(counting, Trigger::In);
    let (from, source) = (count.var(Var::From, Ty::Place), count.konst(Value::Place(salary), Ty::Place));
    let paid_by_employer = count.is(from, &[source]);
    let amount = count.var(Var::Amount, Ty::Amount);
    let count = f.law(count.when(paid_by_employer).count(amount, wages));
    let rule = f.rule(count, Subject::Entity(me));
    f.on_in.push((checking, rule));

    let mut owe = LawBuilder::new(taxing, Trigger::Each(Period::Year, None));
    let (rate, earned) =
        (owe.konst(Value::Num(Ratio::new(1, 10).unwrap()), Ty::Num), owe.call(Func::Tally(wages), &[], Ty::Amount));
    let due = owe.bin(BinOp::Mul, rate, earned, Ty::Amount);
    let owe = f.law(owe.owe(due, irs, tax));
    let rule = f.rule(owe, Subject::Entity(me));
    f.timed.push(rule);

    f.flow(1, salary, checking, 3_000_00);
    f.flow(2, equity, checking, 1_000_00);
    f.flow(40, salary, checking, 2_000_00);
    let book = f.book();
    let run = run(&book, Options { today: Day(400), relaxed: false });
    let counted: Vec<_> = run.effects.iter().filter(|e| e.name == wages).map(|e| e.amount.qty.0).collect();
    assert_eq!(counted, [3_000_00, 2_000_00], "the equity inflow is filtered out by `when`");
    assert_eq!(run.checks[count.index()], 2);
    let owed = run.effects.iter().find(|e| e.name == tax).unwrap();
    assert_eq!(
        (owed.amount.qty, owed.day, owed.owner),
        (Qty(500_00), Day(364), me),
        "10% of the year's wages, at December 31"
    );
}

#[test]
fn a_warn_law_is_a_warning_and_says_how_much_room_is_left() {
    let mut f = Fixture::new();
    let (checking, food) = (f.checking, f.food);
    let (name, budget) = (f.sym("budget"), f.usd(100_00));
    let mut law = LawBuilder::new(name, Trigger::In);
    let total = law.call(Func::Total(Dir::In, Window::Month), &[], Ty::Amount);
    let cap = law.konst(Value::Amount(budget), Ty::Amount);
    let cond = law.bin(BinOp::Le, total, cap, Ty::Bool);
    let law = f.law(law.warn(cond));
    let rule = f.rule(law, Subject::Place(food));
    f.on_in.push((food, rule));
    f.flow(1, checking, food, 80_00);
    f.flow(2, checking, food, 50_00);
    let book = f.book();
    let run = run(&book, options());
    assert_eq!(run.violations.len(), 1);
    assert!(run.violations[0].warn);
    let d = &run.diagnostics[run.violations[0].diagnostic as usize];
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.help[0].text, "at most 20.00 USD more can go in this month");
}

#[test]
fn a_fork_reports_only_what_it_causes() {
    let mut f = Fixture::new();
    let (equity, checking, retirement, salary) = (f.equity, f.checking, f.retirement, f.salary);
    f.flow(1, equity, checking, 1_000_00);
    f.buy(2, 500_00, 5);
    f.sell(3, 5, 600_00);
    f.flow(4, salary, retirement, 10_000_00);
    let book = f.book();
    let mut ledger = Ledger::new(&book, options());
    ledger.advance(Day(5));
    let mut withdrawal = book.flows[Id::new(3)].clone();
    (withdrawal.from, withdrawal.to, withdrawal.day) = (retirement, checking, Day(5));
    (withdrawal.out.qty, withdrawal.arrive.qty) = (Qty(2_000_00), Qty(2_000_00));

    let (mut fork, mut copy) = (ledger.fork(), ledger.clone());
    let (applied, also) = (fork.apply(&withdrawal), copy.apply(&withdrawal));
    assert_eq!((applied.gains, also.gains), (0..1, 1..2), "the copy remembers the journal's sale; the fork does not");
    let (fork, copy) = (fork.finish(), copy.finish());
    assert_eq!((fork.gains.len(), copy.gains.len()), (1, 2));
    assert_eq!(fork.gains[0].gain(), Qty(2_000_00), "pre-tax money: all of it is gain");
    assert_eq!(ledger.finish().gains.len(), 1, "and the original is untouched");
}

#[test]
fn a_clone_diverges_without_touching_the_original() {
    let mut f = Fixture::new();
    let (equity, checking, cash, usd) = (f.equity, f.checking, f.cash, f.usd);
    f.flow(1, equity, checking, 1_000_00);
    f.flow(9, checking, cash, 1_00);
    let book = f.book();
    let mut ledger = Ledger::new(&book, options());
    ledger.advance(Day(3));
    let mut fork = ledger.clone();
    let mut withdrawal = book.flows[Id::new(0)].clone();
    (withdrawal.from, withdrawal.to, withdrawal.day) = (checking, cash, Day(3));
    (withdrawal.out.qty, withdrawal.arrive.qty) = (Qty(250_00), Qty(250_00));
    let applied = fork.apply(&withdrawal);
    assert!(applied.gains.is_empty() && applied.violations.is_empty());
    assert_eq!((ledger.balance(cash, usd), fork.balance(cash, usd)), (Qty::ZERO, Qty(250_00)));
    fork.advance(Day(20));
    ledger.advance(Day(20));
    assert_eq!((ledger.balance(cash, usd), fork.balance(cash, usd)), (Qty(1_00), Qty(251_00)));
    assert_eq!(
        fork.finish().holdings.iter().filter(|h| h.unit == usd && h.place == checking).map(|h| h.qty().0).sum::<i64>(),
        749_00
    );
}

/// A small deterministic generator: xorshift64*.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % below
    }
}

/// Whatever the journal says, value is neither created nor destroyed: each
/// commodity's balances sum to what flowed in less what flowed out, basis is
/// carried or relieved but never lost, lots hold positive quantities in a
/// stable order, and no two lots are interchangeable.
#[test]
fn value_is_conserved_over_random_journals() {
    for seed in 1..=8u64 {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut f = Fixture::new();
        let (equity, salary, food, usd, vti) = (f.equity, f.salary, f.food, f.usd, f.vti);
        let (checking, savings, cash, brokerage, retirement) =
            (f.checking, f.savings, f.cash, f.brokerage, f.retirement);
        f.places[brokerage].select =
            [None, Some(Policy::Fifo), Some(Policy::Hifo), Some(Policy::Prorata)][seed as usize % 4];
        f.places[retirement].select = Some(Policy::Prorata);
        let pool = [checking, savings, cash, retirement, food];
        f.flow(1, equity, checking, 50_000_00);
        let mut expected = std::collections::BTreeMap::<Id<Commodity>, i64>::new();
        let mut paid = 0;
        for i in 0..3_000 {
            let day = 2 + i / 20;
            match dice.roll(8) {
                0 => {
                    let (shares, cost) = (1 + dice.roll(20) as i64, 100_00 + dice.roll(5_000_00) as i64);
                    f.buy(day, cost, shares);
                    paid += cost;
                    *expected.entry(vti).or_default() += shares;
                    *expected.entry(usd).or_default() -= cost;
                }
                1 | 2 => {
                    let (shares, proceeds) = (1 + dice.roll(25) as i64, 100_00 + dice.roll(6_000_00) as i64);
                    let sale = f.sell(day, shares, proceeds);
                    if dice.roll(3) == 0 {
                        let from = Day(2 + dice.roll(day as u64) as i32);
                        f.flows[sale.index()].select = Box::new([Select::Range(from, Day(day))]);
                    }
                    *expected.entry(vti).or_default() -= shares;
                    *expected.entry(usd).or_default() += proceeds;
                }
                3 => {
                    f.flow(day, salary, retirement, 1_000_00 + dice.roll(500_00) as i64);
                }
                _ => {
                    let (from, to) = (pool[dice.roll(5) as usize], pool[dice.roll(5) as usize]);
                    f.flow(day, from, to, 1_00 + dice.roll(2_000_00) as i64);
                }
            }
        }
        let book = f.book();
        let run = run(&book, Options { today: Day(1_000), relaxed: false });

        let mut totals = std::collections::BTreeMap::<Id<Commodity>, i64>::new();
        for holding in &run.holdings {
            *totals.entry(holding.unit).or_default() += holding.qty().0;
            assert!(
                holding.lots.iter().all(|lot| lot.qty > Qty::ZERO && lot.basis >= Qty::ZERO),
                "seed {seed}: {holding:?}"
            );
            assert!(
                holding.lots.windows(2).all(|pair| pair[0].acquired <= pair[1].acquired),
                "seed {seed}: lots out of order"
            );
            let is_base = holding.unit == usd;
            let alike =
                |a: &Parcel, b: &Parcel| crate::lots::identity(a, is_base) == crate::lots::identity(b, is_base);
            for (at, lot) in holding.lots.iter().enumerate() {
                assert!(
                    holding.lots[at + 1..].iter().all(|other| !alike(lot, other)),
                    "seed {seed}: unmerged lots in {holding:?}"
                );
            }
        }
        // Every dollar paid for shares is still the basis of a lot or was relieved with a sale.
        let held = held(&run, brokerage, vti).map_or(0, |h| h.lots.iter().map(|lot| lot.basis.0).sum::<i64>());
        let sold: i64 = run.gains.iter().filter(|g| g.unit == vti).map(|g| g.basis.0).sum();
        assert_eq!(paid, held + sold, "seed {seed}: basis leaked");
        // Transfers move value between places; only exchanges change a commodity's total.
        for (unit, want) in expected {
            assert_eq!(totals.get(&unit).copied().unwrap_or(0), want, "seed {seed}: unit {unit:?}");
        }
    }
}

/// A million flows through `run`: `cargo test -p axiom-engine --release -- --ignored --nocapture million`.
#[test]
#[ignore = "a benchmark"]
fn a_million_flows() {
    let flows: i64 = std::env::var("BENCH_FLOWS").ok().and_then(|n| n.parse().ok()).unwrap_or(1_000_000);
    for laws in [false, true] {
        let mut f = Fixture::new();
        let (equity, salary, checking, savings, food, retirement, brokerage) =
            (f.equity, f.salary, f.checking, f.savings, f.food, f.retirement, f.brokerage);
        if laws {
            let (bank, budget, wages) = (f.sym("bank"), f.sym("budget"), f.sym("wages"));
            let mut never_overdrawn = LawBuilder::new(bank, Trigger::Always);
            let balance = never_overdrawn.var(Var::Balance, Ty::Amount);
            let nothing = never_overdrawn.konst(Value::Empty, Ty::Empty);
            let cond = never_overdrawn.bin(BinOp::Ge, balance, nothing, Ty::Bool);
            let law = f.law(never_overdrawn.warn(cond));
            let rule = f.rule(law, Subject::Place(checking));
            f.always.push((checking, rule));

            let cap = f.usd(1_000_000_00);
            let mut monthly = LawBuilder::new(budget, Trigger::In);
            let total = monthly.call(Func::Total(Dir::In, Window::Month), &[], Ty::Amount);
            let cap = monthly.konst(Value::Amount(cap), Ty::Amount);
            let cond = monthly.bin(BinOp::Le, total, cap, Ty::Bool);
            let law = f.law(monthly.warn(cond));
            let rule = f.rule(law, Subject::Place(food));
            f.on_in.push((food, rule));

            let mut count = LawBuilder::new(wages, Trigger::In);
            let amount = count.var(Var::Amount, Ty::Amount);
            let law = f.law(count.count(amount, wages));
            let rule = f.rule(law, Subject::Entity(f.me));
            f.on_in.push((checking, rule));
        }
        f.flow(1, equity, checking, 1_000_000_00);
        for i in 0..flows {
            let day = 2 + (i / 300) as i32;
            match i % 10 {
                0 => f.flow(day, salary, checking, 3_000_00),
                1 => f.flow(day, checking, retirement, 500_00),
                2 => f.flow(day, checking, savings, 100_00),
                3 => f.flow(day, savings, checking, 100_00),
                4 if i % 100 == 4 => f.buy(day, 1_000_00, 10),
                5 if i % 100 == 5 => {
                    let sale = f.sell(day, 10, 1_200_00);
                    f.flows[sale.index()].select = Box::new([Select::Policy(Policy::Fifo)]);
                    sale
                }
                _ => f.flow(day, checking, food, 20_00 + i % 7),
            };
        }
        let _ = brokerage;
        let book = f.book();
        let options = Options { today: Day(5_000), relaxed: false };
        let started = std::time::Instant::now();
        let mut ledger = Ledger::new(&book, options);
        let solved = started.elapsed();
        ledger.advance(options.today);
        let folded = started.elapsed();
        let cloned = std::time::Instant::now();
        drop(ledger.clone());
        let clone_ms = cloned.elapsed().as_secs_f64() * 1e3;
        let forking = std::time::Instant::now();
        drop(ledger.fork());
        let fork_ms = forking.elapsed().as_secs_f64() * 1e3;
        let run = ledger.finish();
        let took = started.elapsed();
        println!(
            "{} laws: {:.0} ms ({:.0} solve, {:.0} fold, {:.0} finish), {:.2} million flows/s ({} gains, {} effects, {} diagnostics)",
            if laws { "3" } else { "no" },
            took.as_secs_f64() * 1e3,
            solved.as_secs_f64() * 1e3,
            (folded - solved).as_secs_f64() * 1e3,
            (took - folded).as_secs_f64() * 1e3,
            (flows as f64 + 1.0) / took.as_secs_f64() / 1e6,
            run.gains.len(),
            run.effects.len(),
            run.diagnostics.len(),
        );
        println!("   after the fold: clone {clone_ms:.2} ms, fork {fork_ms:.2} ms");
    }
}
