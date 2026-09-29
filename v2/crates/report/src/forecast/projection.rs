//! Running the forecast: expected flows, folded through a clone of the ledger.
//!
//! The projection is a real fold, not arithmetic beside one. Every expected
//! flow is applied to the ledger in date order, so laws run on the future
//! exactly as they run on the past: a limit that will be crossed in November,
//! or a deadline that will pass, is recorded by the same rules.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Ledger, Options, Run};
use axiom_model::{Amount, Book, Class, Commodity, Flow, Place};

use super::habits::Habits;
use super::recurrence::Schedule;
use crate::places::is_liquid;
use crate::synth::planned;
use crate::value::Valuer;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// Written as `every …`.
    Plan,
    /// Found in the journal's history, seen this many times.
    Habit { occurrences: usize },
}

/// A flow that will happen again and again.
pub struct Expectation<'b> {
    pub origin: Origin,
    pub schedule: Schedule,
    /// Where it goes, and through whom.
    pub template: &'b Flow,
    pub out: Amount,
    pub arrive: Amount,
}

impl Expectation<'_> {
    /// Its occurrences after `after`, up to `horizon`, as flows to apply.
    pub fn flows(&self, after: Day, horizon: Day) -> Vec<Flow> {
        // A plan over a date range keeps its width.
        let width = self.template.until.0 - self.template.day.0;
        let occurrences = self.schedule.days(after, horizon);
        occurrences
            .into_iter()
            .map(|day| Flow { until: day.add_days(width), ..planned(self.template, day, self.out, self.arrive) })
            .collect()
    }
}

/// One expectation per flow of each plan.
pub fn from_plans<'b>(book: &'b Book) -> Vec<Expectation<'b>> {
    book.plans
        .iter()
        .flat_map(|plan| {
            plan.template.iter().map(move |flow| Expectation {
                origin: Origin::Plan,
                schedule: Schedule {
                    anchor: plan.from.unwrap_or(flow.day),
                    every: plan.every,
                    on: plan.on,
                    until: plan.until,
                },
                template: flow,
                out: flow.out,
                arrive: flow.arrive,
            })
        })
        .collect()
}

/// One expectation per habit found in history.
pub fn from_habits<'b>(book: &'b Book, habits: &Habits) -> Vec<Expectation<'b>> {
    let expectations = habits.found.iter().map(|habit| {
        let template = &book.flows[habit.template];
        let amount = habit.recurrence.amount;
        Expectation {
            origin: Origin::Habit { occurrences: habit.occurrences },
            schedule: habit.recurrence.schedule(),
            template,
            out: Amount::new(amount, template.out.unit),
            arrive: Amount::new(amount, template.arrive.unit),
        }
    });
    expectations.collect()
}

/// A liquid place that goes below zero.
pub struct Overdraft {
    pub place: Id<Place>,
    pub first: Day,
    pub lowest: Qty,
}

/// What running the expected flows produced.
pub struct Trace {
    /// Liquid net worth in the base currency at each checkpoint.
    pub liquid: Vec<Qty>,
    /// Everything recorded along the way: obligations, violations.
    pub run: Run,
    pub overdrafts: Vec<Overdraft>,
}

/// Applies `flows` (in date order) to a ledger standing at `today`, reading
/// the liquid position at each checkpoint. Laws with deadlines fire all the
/// way to the last checkpoint.
pub fn project(book: &Book, today: Day, flows: Vec<Flow>, checkpoints: &[Day]) -> Trace {
    let horizon = checkpoints.last().copied().unwrap_or(today);
    let mut ledger = Ledger::new(book, Options { today: horizon, relaxed: book.relaxed });
    ledger.advance(today);

    let (growth, valuer) = (Growth::new(book), Valuer::new(book, today));
    let mut overdrawn: BTreeMap<Id<Place>, Overdraft> = BTreeMap::new();
    let mut liquid = Vec::with_capacity(checkpoints.len());
    let mut coming = flows.into_iter().peekable();
    for &checkpoint in checkpoints {
        while let Some(flow) = coming.next_if(|flow| flow.day <= checkpoint) {
            ledger.apply(&flow);
            note_overdrafts(book, &ledger, &flow, &mut overdrawn);
        }
        ledger.advance(checkpoint);
        let months = checkpoint.since(today).months;
        let worth = ledger.holdings().filter(|holding| counts_toward_liquid_net_worth(book, holding.place));
        let worth = worth.filter_map(|holding| {
            let worth = valuer.qty(Amount::new(holding.qty(), holding.unit))?;
            Some(growth.apply(holding.unit, worth, months))
        });
        liquid.push(worth.sum());
    }
    Trace { liquid, run: ledger.finish(), overdrafts: overdrawn.into_values().collect() }
}

/// Liquid net worth is what can be spent, less everything owed. Debts count
/// whole, so paying one down is neutral, and money charged to a card is spent
/// the day it is charged rather than the day the card is paid.
fn counts_toward_liquid_net_worth(book: &Book, place: Id<Place>) -> bool {
    is_liquid(book, place) || book.places[place].class == Class::Liability
}

/// Records where a flow left a liquid place below zero.
fn note_overdrafts(book: &Book, ledger: &Ledger, flow: &Flow, overdrawn: &mut BTreeMap<Id<Place>, Overdraft>) {
    for place in [flow.from, flow.to].into_iter().filter(|&place| is_liquid(book, place)) {
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
