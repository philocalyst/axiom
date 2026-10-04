//! Running the forecast: the fold continued past today, and the position read at each month end.
//!
//! The ledger the forecast goes on from is the one the run came from, resumed at its checkpoint (or folded from the journal
//! when there is none), made to promise what the contracts promise and run to the horizon. What it posts of the contracts
//! is the engine's: this module applies the flows of the habits the journal shows (`Ledger::apply`, as a flow nobody wrote
//! is always applied) and reads. So the laws judge the future the way they judge the past: a limit that will be crossed in
//! November, or a deadline that will pass, is recorded by the same rules.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Holding, Ledger, Options};
use axiom_model::{Book, Class, Commodity, Flow, Place};

use super::Past;
use crate::view::{Basket, Liquidity, View};

/// A cash place that goes below zero.
pub struct Overdraft {
    pub place: Id<Place>,
    pub first: Day,
    pub lowest: Qty,
}

/// What running the forecast produced.
pub struct Trace<'p, 'b, 's> {
    /// Everything the ledger recorded after the day it was resumed on: what it promised, and what the laws made of it.
    pub ledger: Ledger<'p, 'b, 's>,
    /// Money in hand less the debts with no term, at each checkpoint.
    pub liquid: Vec<Qty>,
    /// Everything owned less everything owed at each checkpoint, with
    /// commodities grown by their models.
    pub worth: Vec<Qty>,
    pub overdrafts: Vec<Overdraft>,
}

impl<'p, 'b, 's> Trace<'p, 'b, 's> {
    /// Goes on from `past` to the last of `checkpoints`, the first being the day the run stands on, applying `habits` (in
    /// date order) as they fall due and reading the position at each checkpoint. Laws with deadlines fire all the way to
    /// the last checkpoint.
    pub fn run(
        view: View<'b, 's, 'p>,
        past: &Past<'_>,
        options: Options,
        habits: Vec<Flow>,
        checkpoints: &[Day],
    ) -> Trace<'p, 'b, 's> {
        let today = checkpoints[0];
        let mut ledger = promising(view, past, options, checkpoints);
        let mut overdrawn: BTreeMap<Id<Place>, Overdraft> = BTreeMap::new();
        let (mut liquid, mut worth) = (Vec::new(), Vec::new());
        let mut coming = habits.into_iter().peekable();
        for &checkpoint in checkpoints {
            while let Some(flow) = coming.next_if(|flow| flow.day <= checkpoint) {
                apply_habit(view, &mut ledger, flow, &mut overdrawn);
            }
            note_promised(view, &mut ledger, checkpoint, &mut overdrawn);
            ledger.advance(checkpoint);
            let months = checkpoint.since(today).months;
            let position = |pick: &dyn Fn(&Holding) -> Qty| grown(view, months, &ledger, pick);
            liquid.push(position(&|holding| in_hand_or_owed(view, holding)));
            worth.push(position(&|holding| holding.qty()));
        }
        Trace { ledger, liquid, worth, overdrafts: overdrawn.into_values().collect() }
    }
}

/// The ledger a forecast goes on with: the run's fold, resumed from its checkpoint, closed through today, made to reach the last checkpoint and to promise the contracts of the view's owners.
fn promising<'p, 'b, 's>(
    view: View<'b, 's, 'p>,
    past: &Past<'_>,
    options: Options,
    checkpoints: &[Day],
) -> Ledger<'p, 'b, 's> {
    let (plan, book) = (view.plan(), view.book());
    let (today, horizon) = (options.today, checkpoints[checkpoints.len() - 1]);
    debug_assert_eq!(today, checkpoints[0], "the forecast begins on the day the run stands on");
    debug_assert!(past.at.day() <= today, "a forecast cannot rewind the checkpoint it goes on from");
    let mut ledger = plan.resume(past.at, options);
    ledger.advance(today);
    ledger.reach(horizon);
    ledger.promise(|contract| view.owns_entity(book.contracts[contract].owner));
    ledger
}

/// Applies a flow a habit expects, after every occurrence the contracts promise by its day, unless the means to make it are
/// not there.
fn apply_habit(view: View, ledger: &mut Ledger, flow: Flow, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    note_promised(view, ledger, flow.day, overdrawn);
    let Some(flow) = within_means(view, ledger, flow) else { return };
    ledger.apply(&flow);
    note_overdrafts(view, ledger, &flow, overdrawn);
}

/// Takes every occurrence the contracts promise by `day`, one at a time, and notes where each left a cash place.
fn note_promised(view: View, ledger: &mut Ledger, day: Day, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    while let Some(planned) = ledger.promise_through(day) {
        let Ok(made) = planned.made else { continue };
        for flow in made.flows(ledger.recorded().promised_flows).unwrap_or_default() {
            note_overdrafts(view, ledger, &flow.flow, overdrawn);
        }
    }
}

/// What a holding adds to what can be spent: its free money, less what is
/// owed on debts with no term (a card, a tab). A loan with a term is paid by
/// the payments the projection already makes.
fn in_hand_or_owed(view: View, holding: &Holding) -> Qty {
    let place = &view.book().places[holding.place];
    let has_term = view.known().maturity.is_some_and(|name| view.book().says(holding.place, name));
    match view.liquidity(holding.place, holding.unit) {
        Some(Liquidity::Cash) => view.free(holding),
        _ if place.class == Class::Debt && !has_term => holding.qty(),
        _ => Qty::ZERO,
    }
}

/// What the view's owners hold on the balance sheet, as `pick` counts it,
/// each commodity priced as a whole at today's prices and grown `months` ahead.
fn grown(view: View, months: i32, ledger: &Ledger, pick: &dyn Fn(&Holding) -> Qty) -> Qty {
    let mut basket = Basket::default();
    for holding in ledger.holdings() {
        if !matches!(view.book().places[holding.place].class, Class::Asset | Class::Debt) {
            continue;
        }
        let qty = view.place_qty(holding.place, pick(holding));
        if qty.is_zero() {
            continue;
        }
        basket.add(holding.unit, qty);
    }
    basket.amounts().filter_map(|amount| Some(compound(view.book(), amount.unit, view.value(amount)?, months))).sum()
}

/// A flow that cannot move more than its ends hold: what leaves an account
/// that is not cash is limited by what it holds, and a payment into a debt by
/// what is owed. `None` when there is nothing to move.
fn within_means(view: View, ledger: &Ledger, mut flow: Flow) -> Option<Flow> {
    let movement = &flow;
    let (from, to, out_unit, arrive_unit, exchange, amount) =
        (movement.from, movement.to, movement.out.unit, movement.arrive.unit, movement.is_exchange(), movement.out.qty);
    // Slow holdings and claims cannot move more than they currently hold.
    let held_back = matches!(view.liquidity(from, out_unit), Some(Liquidity::Slow(_) | Liquidity::Claim));
    let room = if held_back {
        Some(ledger.balance(from, out_unit))
    } else if view.book().places[to].class == Class::Debt {
        Some(-ledger.balance(to, arrive_unit))
    } else {
        None
    };
    match room {
        Some(room) if !exchange && room < amount => {
            if room <= Qty::ZERO {
                None
            } else {
                flow.out.qty = room;
                flow.arrive.qty = room;
                Some(flow)
            }
        }
        _ => Some(flow),
    }
}

/// Records where a flow left a cash place below zero.
fn note_overdrafts(view: View, ledger: &Ledger, flow: &Flow, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    let book = view.book();
    let cash = |&place: &Id<Place>| view.liquidity(place, book.base) == Some(Liquidity::Cash);
    for place in [flow.from, flow.to].into_iter().filter(cash) {
        let balance = ledger.balance(place, book.base);
        if balance.is_negative() {
            let overdraft = overdrawn.entry(place).or_insert(Overdraft { place, first: flow.day, lowest: balance });
            overdraft.lowest = overdraft.lowest.min(balance);
        }
    }
}

/// A commodity that `grows 5% yearly` is priced 5%/12 higher each month
/// ahead, compounding, rounded half to even each month. The base currency is
/// the yardstick and does not grow.
fn compound(book: &Book, unit: Id<Commodity>, value: Qty, months: i32) -> Qty {
    let yearly = book.growth(unit).filter(|_| unit != book.base);
    let monthly = yearly.and_then(|yearly| Ratio::ONE.checked_add(yearly.checked_div(Ratio::int(12))?));
    monthly.map_or(value, |factor| (0..months).fold(value, |worth, _| worth.scale(factor).unwrap_or(worth)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::household;
    use axiom_core::Days;
    use axiom_engine::Plan;
    use axiom_model::Amount;

    #[test]
    fn growth_compounds_monthly_and_leaves_the_yardstick_alone() {
        let mut house = household();
        let (usd, vti) = (house.book.base, Id::new(1));
        let mut said = axiom_core::Facts::builder(house.book.holders.len());
        let growth = Ratio::percent(5, 0).unwrap();
        said.paint_always(
            house.book.holders.number(axiom_model::Holder::Commodity(vti)),
            axiom_model::builtin::GROWS,
            growth,
        );
        house.book.facts = said.freeze();
        // 100,000.00 at 241/240 a month for a year, rounded half-even each month.
        assert_eq!(compound(&house.book, vti, Qty(10_000_000), 12), Qty(10_511_619));
        assert_eq!(compound(&house.book, usd, Qty(10_000_000), 12), Qty(10_000_000));
        assert_eq!(compound(&house.book, vti, Qty(10_000_000), 0), Qty(10_000_000));
    }

    #[test]
    fn projection_resumes_the_supplied_checkpoint_without_refolding() {
        let house = household();
        let plan = Plan::new(&house.book);
        let today = house.run.today;
        let options = Options { today, relaxed: false };
        let (_, mut view) = plan.run_with_view(options);

        // Add a hypothetical salary to the checkpoint. A fresh fold of the
        // book cannot contain this amount, so the resumed projection must.
        let tomorrow = today.add_days(1);
        let mut salary = house.book.flows[Id::new(0)].clone();
        let checking = house.place("assets/bank/checking");
        salary.day = tomorrow;
        salary.recognized = Days::on(tomorrow);
        salary.from = house.place("income/salary");
        salary.to = checking;
        salary.out = Amount::new(Qty(1_000), house.book.base);
        salary.arrive = salary.out;
        view.apply(&salary);
        let checkpoint = view.checkpoint();

        let whose = crate::view::Whose::default();
        let view = View::new(&plan, &whose, &house.run, tomorrow);
        let resumed = Trace::run(
            view,
            &Past { at: &checkpoint, effects: &[] },
            Options { today: tomorrow, relaxed: false },
            Vec::new(),
            &[tomorrow],
        );
        let start = plan.start(Options { today: tomorrow, relaxed: false }).checkpoint();
        let folded = Trace::run(
            view,
            &Past { at: &start, effects: &[] },
            Options { today: tomorrow, relaxed: false },
            Vec::new(),
            &[tomorrow],
        );

        assert_eq!(resumed.liquid[0] - folded.liquid[0], Qty(1_000));
        assert_eq!(resumed.worth[0] - folded.worth[0], Qty(1_000));
    }

    #[test]
    fn projected_worth_uses_cent_conserving_owner_shares() {
        let source = "\
base USD
commodity USD
  precision 2
entity me
entity jordan
account assets/shared
  owner me 60%, jordan 40%
opening 2026-01-01
  shared 100 USD
";
        crate::source_tests::with_run(source, Day::from_ymd(2026, 1, 2).unwrap(), |book, run| {
            let plan = Plan::new(book);
            let place = book
                .places
                .iter()
                .find(|(_, place)| book.name(place.path) == "assets/shared")
                .map(|(id, _)| id)
                .unwrap();
            let owner = |name| {
                book.entities.iter().find(|(_, entity)| book.name(entity.path) == name).map(|(id, _)| id).unwrap()
            };
            let options = Options { today: run.today, relaxed: false };
            let start = plan.start(options).checkpoint();
            let past = Past { at: &start, effects: &[] };
            let forecast_for = |name| {
                let whose = crate::view::Whose::of(book, owner(name));
                let view = View::new(&plan, &whose, run, run.today);
                Trace::run(view, &past, options, Vec::new(), &[run.today]).worth[0]
            };

            assert_eq!(forecast_for("me"), Qty(6_000));
            assert_eq!(forecast_for("jordan"), Qty(4_000));

            let everyone = crate::view::Whose::default();
            let view = View::new(&plan, &everyone, run, run.today);
            assert_eq!(Trace::run(view, &past, options, Vec::new(), &[run.today]).worth[0], Qty(10_000));
            assert_eq!(plan.allocate(place, Qty(10_000)).map(|(_, amount)| amount).sum::<Qty>(), Qty(10_000));
        });
    }
}
