//! Running the forecast: expected flows, folded through a ledger.
//!
//! The projection is a real fold, not arithmetic beside one. Every expected
//! flow is applied to the ledger in date order, so laws run on the future
//! exactly as they run on the past: a limit that will be crossed in November,
//! or a deadline that will pass, is recorded by the same rules.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Holding, Ledger, Options};
use axiom_model::{Book, Class, Commodity, Flow, Place, Value};

use crate::history::Held;
use crate::lens::{Basket, Lens, Liquidity};

/// A liquid place that goes below zero.
pub struct Overdraft {
    pub place: Id<Place>,
    pub first: Day,
    pub lowest: Qty,
}

/// What running the expected flows produced.
pub struct Trace<'b, 's> {
    /// Everything the ledger recorded, history and projection alike.
    pub ledger: Ledger<'b, 's>,
    /// Money in hand less the debts that fall due, at each checkpoint.
    pub liquid: Vec<Qty>,
    /// Everything owned less everything owed, at each checkpoint, with
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

    let growth = Growth::new(book);
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
        let position = |pick: &dyn Fn(&Holding) -> Qty| grown(lens, &growth, months, &ledger, pick);
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
    match lens.liquidity(holding.place, holding.unit) {
        Some(Liquidity::Cash) => lens.free(holding),
        _ if place.class == Class::Liability && !has_term(lens.book, place) => holding.qty(),
        _ => Qty::ZERO,
    }
}

/// Whether a debt has a term (`maturity 2050-01-01`).
fn has_term(book: &Book, place: &Place) -> bool {
    let maturity = book.names.get("maturity");
    maturity.is_some_and(|name| place.props.iter().any(|prop| prop.name == name && matches!(prop.value, Value::Day(_))))
}

/// What the lens's owners hold on the balance sheet, as `pick` counts it,
/// at today's prices with every commodity grown `months` ahead.
fn grown(lens: Lens, growth: &Growth, months: i32, ledger: &Ledger, pick: &dyn Fn(&Holding) -> Qty) -> Qty {
    let mut basket = Basket::default();
    let on_sheet = ledger.holdings().filter(|holding| {
        lens.owns(holding.place) && matches!(lens.book.places[holding.place].class, Class::Asset | Class::Liability)
    });
    for holding in on_sheet {
        basket.add(holding.unit, Held { qty: pick(holding), booked: Qty::ZERO });
    }
    let worth = basket.amounts().filter_map(|amount| Some(growth.apply(amount.unit, lens.value(amount)?, months)));
    worth.sum()
}

/// A flow that cannot move more than its ends hold: what leaves an account
/// that is not cash is limited by what it holds, and a payment into a debt by
/// what is owed. `None` when there is nothing to move.
fn within_means(lens: Lens, ledger: &Ledger, mut flow: Flow) -> Option<Flow> {
    let book = lens.book;
    let held_back = matches!(lens.liquidity(flow.from, flow.out.unit), Some(Liquidity::Slow(_) | Liquidity::Claim));
    let room = if held_back {
        Some(ledger.balance(flow.from, flow.out.unit))
    } else if book.places[flow.to].class == Class::Liability {
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

/// Records where a flow left a liquid place below zero.
fn note_overdrafts(lens: Lens, ledger: &Ledger, flow: &Flow, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    let book = lens.book;
    for place in
        [flow.from, flow.to].into_iter().filter(|&place| lens.liquidity(place, book.base) == Some(Liquidity::Cash))
    {
        let balance = ledger.balance(place, book.base);
        if balance.is_negative() {
            let overdraft = overdrawn.entry(place).or_insert(Overdraft { place, first: flow.day, lowest: balance });
            overdraft.lowest = overdraft.lowest.min(balance);
        }
    }
}

/// Growth models: a commodity that `grows 5% yearly` is priced 5%/12 higher
/// each month ahead, compounding. The base currency is the yardstick and does
/// not grow.
struct Growth {
    monthly: Vec<Option<Ratio>>,
}

impl Growth {
    fn new(book: &Book) -> Growth {
        let monthly = book
            .commodities
            .iter()
            .map(|(unit, commodity)| commodity.growth.filter(|_| unit != book.base).and_then(monthly_factor));
        Growth { monthly: monthly.collect() }
    }

    fn apply(&self, unit: Id<Commodity>, value: Qty, months: i32) -> Qty {
        let Some(factor) = self.monthly[unit.index()] else { return value };
        (0..months).fold(value, |worth, _| worth.scale(factor).unwrap_or(worth))
    }
}

/// What one month multiplies a price by: 5% a year is 1 + 5%/12.
fn monthly_factor(yearly: Ratio) -> Option<Ratio> {
    Ratio::ONE.checked_add(yearly.checked_div(Ratio::int(12))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_compounds_monthly_and_leaves_the_yardstick_alone() {
        let five_percent = Ratio::percent(5, 0).unwrap();
        let growth = Growth { monthly: vec![None, monthly_factor(five_percent)] };
        // 100,000.00 at 241/240 a month for a year, rounded half-even each month.
        assert_eq!(growth.apply(Id::new(1), Qty(10_000_000), 12), Qty(10_511_619));
        assert_eq!(growth.apply(Id::new(0), Qty(10_000_000), 12), Qty(10_000_000));
        assert_eq!(growth.apply(Id::new(1), Qty(10_000_000), 0), Qty(10_000_000));
    }
}
