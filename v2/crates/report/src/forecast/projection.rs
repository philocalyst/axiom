//! Running the forecast: expected flows, folded through a ledger.
//!
//! The projection is a real fold, not arithmetic beside one. Every expected
//! flow is applied to the ledger in date order, so laws run on the future
//! exactly as they run on the past: a limit that will be crossed in November,
//! or a deadline that will pass, is recorded by the same rules.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Holding, Ledger, Options};
use axiom_model::{Book, Class, Commodity, End, Flow, Place, Value};

use crate::history::Held;
use crate::lens::{Basket, Lens, Liquidity};

/// A cash place that goes below zero.
pub struct Overdraft {
    pub place: Id<Place>,
    pub first: Day,
    pub lowest: Qty,
}

/// What running the expected flows produced.
pub struct Trace<'b, 's> {
    /// Everything the ledger recorded, history and projection alike.
    pub ledger: Ledger<'b, 's>,
    /// Money in hand less the debts with no term, at each checkpoint.
    pub liquid: Vec<Qty>,
    /// Everything owned less everything owed at each checkpoint, with
    /// commodities grown by their models.
    pub worth: Vec<Qty>,
    pub overdrafts: Vec<Overdraft>,
}

/// Applies `flows` (in date order) to a ledger standing at `today`, reading
/// the position at each checkpoint. Laws with deadlines fire all the way to
/// the last checkpoint.
pub fn project<'b, 's>(lens: Lens<'b, 's>, today: Day, flows: Vec<Flow>, checkpoints: &[Day]) -> Trace<'b, 's> {
    let book = lens.book;
    let horizon = checkpoints.last().copied().unwrap_or(today);
    let mut ledger = Ledger::new(book, Options { today: horizon, relaxed: book.relaxed });
    ledger.advance(today);

    let mut overdrawn: BTreeMap<Id<Place>, Overdraft> = BTreeMap::new();
    let (mut liquid, mut worth) = (Vec::new(), Vec::new());
    let mut coming = flows.into_iter().peekable();
    for &checkpoint in checkpoints {
        while let Some(flow) = coming.next_if(|flow| flow.day <= checkpoint) {
            let Some(flow) = within_means(lens, &ledger, flow) else { continue };
            ledger.apply(&flow);
            note_overdrafts(lens, &ledger, &flow, &mut overdrawn);
        }
        ledger.advance(checkpoint);
        let months = checkpoint.since(today).months;
        let position = |pick: &dyn Fn(&Holding) -> Qty| grown(lens, months, &ledger, pick);
        liquid.push(position(&|holding| in_hand_or_owed(lens, holding)));
        worth.push(position(&|holding| holding.qty()));
    }
    Trace { ledger, liquid, worth, overdrafts: overdrawn.into_values().collect() }
}

/// What a holding adds to what can be spent: its free money, less what is
/// owed on debts with no term (a card, a tab). A loan with a term is paid by
/// the payments the projection already makes.
fn in_hand_or_owed(lens: Lens, holding: &Holding) -> Qty {
    let place = &lens.book.places[holding.place];
    let has_term = lens.maturity.is_some_and(|name| {
        place.props.iter().any(|prop| prop.name == name && matches!(prop.value, Value::Day(_)))
    });
    match lens.liquidity(holding.place, holding.unit) {
        Some(Liquidity::Cash) => lens.free(holding),
        _ if place.class == Class::Debt && !has_term => holding.qty(),
        _ => Qty::ZERO,
    }
}

/// What the lens's owners hold on the balance sheet, as `pick` counts it,
/// each commodity priced as a whole at today's prices and grown `months` ahead.
fn grown(lens: Lens, months: i32, ledger: &Ledger, pick: &dyn Fn(&Holding) -> Qty) -> Qty {
    let mut basket = Basket::default();
    let on_sheet = |holding: &&Holding| {
        lens.owns(holding.place) && matches!(lens.book.places[holding.place].class, Class::Asset | Class::Debt)
    };
    for holding in ledger.holdings().filter(on_sheet) {
        basket.add(holding.unit, Held { qty: pick(holding), booked: Qty::ZERO });
    }
    basket.amounts().filter_map(|amount| Some(compound(lens.book, amount.unit, lens.value(amount)?, months))).sum()
}

/// A flow that cannot move more than its ends hold: what leaves an account
/// that is not cash is limited by what it holds, and a payment into a debt by
/// what is owed. `None` when there is nothing to move.
fn within_means(lens: Lens, ledger: &Ledger, mut flow: Flow) -> Option<Flow> {
    // A basis end moves no quantity, so there is none for it to run out of.
    let leaves = flow.moves_quantity(End::From);
    let arrives = flow.moves_quantity(End::To);
    let held_back =
        leaves && matches!(lens.liquidity(flow.from, flow.out.unit), Some(Liquidity::Slow(_) | Liquidity::Claim));
    let room = if held_back {
        Some(ledger.balance(flow.from, flow.out.unit))
    } else if arrives && lens.book.places[flow.to].class == Class::Debt {
        Some(-ledger.balance(flow.to, flow.arrive.unit))
    } else {
        None
    };
    match room {
        Some(room) if !flow.is_exchange() && room < flow.out.qty => (room > Qty::ZERO).then(|| {
            (flow.out.qty, flow.arrive.qty) = (room, room);
            flow
        }),
        _ => Some(flow),
    }
}

/// Records where a flow left a cash place below zero.
fn note_overdrafts(lens: Lens, ledger: &Ledger, flow: &Flow, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    let book = lens.book;
    let cash = |&place: &Id<Place>| lens.liquidity(place, book.base) == Some(Liquidity::Cash);
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
    let yearly = book.commodities[unit].growth.filter(|_| unit != book.base);
    let monthly = yearly.and_then(|yearly| Ratio::ONE.checked_add(yearly.checked_div(Ratio::int(12))?));
    monthly.map_or(value, |factor| (0..months).fold(value, |worth, _| worth.scale(factor).unwrap_or(worth)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::household;

    #[test]
    fn growth_compounds_monthly_and_leaves_the_yardstick_alone() {
        let mut house = household();
        let (usd, vti) = (house.book.base, Id::new(1));
        house.book.commodities[vti].growth = Ratio::percent(5, 0);
        // 100,000.00 at 241/240 a month for a year, rounded half-even each month.
        assert_eq!(compound(&house.book, vti, Qty(10_000_000), 12), Qty(10_511_619));
        assert_eq!(compound(&house.book, usd, Qty(10_000_000), 12), Qty(10_000_000));
        assert_eq!(compound(&house.book, vti, Qty(10_000_000), 0), Qty(10_000_000));
    }
}
