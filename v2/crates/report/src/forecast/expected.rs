//! What is expected to happen again: plans, and the rhythms history shows.
//!
//! Both come out as [`Expectation`]s. A plan is written down, so it is always
//! believed. A rhythm is learned from real flows, and is dropped when a plan
//! says the same thing, or when the account behind it has run dry.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Map, Qty};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Class, Entity, Flow, Place, Plan, Recognition, Txn};

use super::recurrence::{Schedule, detect, median};
use crate::history::{Posting, postings};
use crate::lens::Lens;
use crate::synth::planned;

/// How far apart two amounts may be, in percent of the larger, and still be
/// the same recurrence: a raise is still the paycheck.
const TOLERANCE_PERCENT: i64 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// Written as `every …`.
    Plan(Id<Plan>),
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
        let width = self.template.recognized.until.0 - self.template.recognized.from.0;
        let days = self.schedule.days(after, horizon);
        days.into_iter()
            .map(|day| Flow {
                recognized: Recognition { from: day, until: day.add_days(width) },
                ..planned(self.template, day, self.out, self.arrive)
            })
            .collect()
    }

    /// Whether `other` says the same thing: the same pair, at the same
    /// cadence, for an amount within tolerance.
    fn describes(&self, other: &Expectation) -> bool {
        let (a, b) = (self.out.qty.0, other.out.qty.0);
        (self.template.from, self.template.to) == (other.template.from, other.template.to)
            && self.schedule.every == other.schedule.every
            && (a - b).abs() * 100 <= a.abs().max(b.abs()) * TOLERANCE_PERCENT
    }

    /// Whether a real flow is one of this expectation's kind: the same pair.
    pub fn covers(&self, flow: &Flow) -> bool {
        (self.template.from, self.template.to) == (flow.from, flow.to)
    }

    /// The flows of one transaction, or one plan, are one thing that recurs.
    pub fn group(&self) -> (bool, u32) {
        match self.origin {
            Origin::Plan(plan) => (false, plan.index() as u32),
            Origin::Habit { .. } => (true, self.template.txn.index() as u32),
        }
    }
}

/// Everything expected after `today` for the lens's owners: plans, and the
/// rhythms in history that no plan already says.
pub fn expected<'b>(lens: Lens<'b, '_>, run: &Run) -> Vec<Expectation<'b>> {
    let book = lens.book;
    let mut expected = from_plans(book, run, run.today);
    let plans = expected.len();
    for habit in from_history(book, run) {
        if !expected[..plans].iter().any(|plan| plan.describes(&habit)) && !has_ended(book, run, &habit) {
            expected.push(habit);
        }
    }
    expected.retain(|item| lens.owns(item.template.from) || lens.owns(item.template.to));
    expected
}

/// One expectation per flow of each plan. A plan the journal has instantiated
/// goes on from its latest occurrence, in the flows that occurrence wrote (a
/// raise, a changed leg); otherwise from its own template.
fn from_plans<'b>(book: &'b Book, run: &Run, today: Day) -> Vec<Expectation<'b>> {
    let mut latest: Map<Id<Plan>, (Day, Id<Txn>)> = Map::default();
    for (id, txn) in book.txns.iter().filter(|(_, txn)| txn.day <= today) {
        if let Some(plan) = txn.plan {
            let slot = latest.entry(plan).or_insert((txn.day, id));
            *slot = (*slot).max((txn.day, id));
        }
    }
    let mut found = Vec::new();
    for (id, plan) in book.plans.iter() {
        // What each flow moves, and the day the schedule is counted from.
        let (legs, anchor): (Vec<(&Flow, Amount, Amount)>, Option<Day>) = match latest.get(&id) {
            Some(&(day, txn)) => {
                let txn = &book.txns[txn];
                let flows = (txn.first.index()..txn.first.index() + txn.len as usize).map(|at| {
                    let (flow, posted) = (&book.flows[Id::new(at as u32)], &run.posted[at]);
                    (flow, Amount::new(posted.out, flow.out.unit), Amount::new(posted.arrive, flow.arrive.unit))
                });
                (flows.collect(), Some(day))
            }
            None => (plan.template.iter().map(|flow| (flow, flow.out, flow.arrive)).collect(), plan.from),
        };
        for (template, out, arrive) in legs {
            let schedule =
                Schedule { anchor: anchor.unwrap_or(template.day), every: plan.every, on: plan.on, until: plan.until };
            found.push(Expectation { origin: Origin::Plan(id), schedule, template, out, arrive });
        }
    }
    found
}

/// Who pays whom, through whom: what makes two flows the same habit.
type Key = (Id<Place>, Id<Place>, Option<Id<Entity>>);

/// One expectation per rhythm found among real flows, grouped by
/// (from, to, payee). Occurrences of a plan are the plan's, not a habit.
fn from_history<'b>(book: &'b Book, run: &Run) -> Vec<Expectation<'b>> {
    let mut groups: BTreeMap<Key, Vec<Posting>> = BTreeMap::new();
    let candidates = postings(book, run).filter(|posting| {
        let flow = posting.flow;
        posting.is_real_on(run.today)
            && flow.from != book.roots.unknown
            && flow.to != book.roots.unknown
            && book.txns[flow.txn].plan.is_none()
    });
    for posting in candidates {
        groups.entry((posting.flow.from, posting.flow.to, posting.flow.payee)).or_default().push(posting);
    }
    let mut habits: Vec<Expectation> =
        groups.into_values().filter_map(|group| habit(book, &group, run.today)).collect();
    habits.sort_by_key(|habit| (habit.template.txn, habit.template.from, habit.template.to));
    habits
}

/// The rhythm in one group of flows, if it has one. An exchange is learned
/// from the side that does not vary: a monthly 1,500 USD purchase buys a
/// different number of shares each time, and a monthly conversion of a fixed
/// 1,000 EUR returns a different sum.
fn habit<'b>(book: &'b Book, group: &[Posting], today: Day) -> Option<Expectation<'b>> {
    let last = *group.last()?;
    let by_out = |posting: &Posting| (posting.flow.day, posting.posted.out);
    let by_arrive = |posting: &Posting| (posting.flow.day, posting.posted.arrive);
    let (out_series, arrive_series): (Vec<_>, Vec<_>) = group.iter().map(|p| (by_out(p), by_arrive(p))).unzip();
    let steady_out = spread(&out_series) <= spread(&arrive_series);
    let recurrence = detect(if steady_out { &out_series } else { &arrive_series }, today)?;

    let (steady, other) = if steady_out { (last.out(), last.arrive()) } else { (last.arrive(), last.out()) };
    let stated = Amount::new(recurrence.amount, steady.unit);
    // The other side follows the latest price.
    let follows = Amount::new(other.qty.share(recurrence.amount, steady.qty).unwrap_or(other.qty), other.unit);
    let (out, arrive) = if steady_out { (stated, follows) } else { (follows, stated) };
    Some(Expectation {
        origin: Origin::Habit { occurrences: group.len() },
        schedule: recurrence.schedule(),
        template: &book.flows[last.id],
        out,
        arrive,
    })
}

/// How much a series varies: the median deviation from the median, in parts
/// per thousand.
fn spread(series: &[(Day, Qty)]) -> i64 {
    let amounts: Vec<Qty> = series.iter().map(|&(_, qty)| qty).collect();
    let middle = median(&amounts).0.abs().max(1);
    let deviations: Vec<i64> = amounts.iter().map(|qty| (qty.0 - middle).abs()).collect();
    median(&deviations) * 1000 / middle
}

/// A habit whose reason is gone: an account it moves value through holds
/// nothing, and nothing else has touched it for two periods (a loan repaid, a
/// prepaid cost used up, a claim settled).
fn has_ended(book: &Book, run: &Run, habit: &Expectation) -> bool {
    let (every, flow) = (habit.schedule.every, habit.template);
    let since = run.today.add_days(-2 * (every.months * 31 + every.days));
    let held = |place, unit| {
        let found = run.holdings.binary_search_by_key(&(place, unit), |holding| (holding.place, holding.unit));
        found.map_or(Qty::ZERO, |at| run.holdings[at].qty())
    };
    let ends = [(flow.from, flow.out.unit), (flow.to, flow.arrive.unit)];
    ends.into_iter().filter(|&(place, _)| matches!(book.places[place].class, Class::Asset | Class::Liability)).any(
        |(place, unit)| {
            let recent = book.touching[place].iter().rev().map(|&id| &book.flows[id]);
            let mut others = recent.take_while(|other| other.day >= since);
            held(place, unit).is_zero()
                && !others.any(|other| (other.from, other.to, other.payee) != (flow.from, flow.to, flow.payee))
        },
    )
}

#[cfg(test)]
mod tests {
    use axiom_engine::Posted;
    use axiom_engine::State;
    use axiom_model::Mode;

    use super::*;
    use crate::tests::household;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    /// Four monthly purchases of shares with dollars: a fixed 1,500 buys a different number each time.
    #[test]
    fn a_standing_order_is_learned_from_the_side_that_does_not_vary() {
        let house = household();
        let (usd, shares) = (house.book.base, Id::new(1));
        let template = house.book.flows.iter().next().unwrap().0;
        let buys: Vec<(Flow, Posted)> = [(1, 5_250), (2, 5_310), (3, 5_120), (4, 5_400)]
            .map(|(month, quanta)| {
                let mut flow = house.book.flows[template].clone();
                flow.day = day(2026, month, 3);
                flow.mode = Mode::Actual;
                flow.arrive.unit = shares;
                (flow, Posted { out: Qty(150_000), arrive: Qty(quanta), state: State::Actual })
            })
            .into();
        let group: Vec<Posting> = buys.iter().map(|(flow, posted)| Posting { id: template, flow, posted }).collect();
        // The last buy fetched 5.400 shares for 1,500.00: the projection buys the same 1,500.00
        // at that price, not the median share count at some other cost.
        let habit = habit(&house.book, &group, day(2026, 4, 20)).expect("a monthly standing order");
        assert_eq!((habit.out, habit.arrive), (Amount::new(Qty(150_000), usd), Amount::new(Qty(5_400), shares)));
        assert_eq!(habit.schedule.every, axiom_core::Span::months(1));
    }

    /// The client's claim is settled and nothing else touches it: what came out of it is over.
    #[test]
    fn a_habit_ends_when_the_account_it_draws_on_has_run_dry() {
        let mut house = household();
        house.run.today = day(2026, 9, 1);
        let (clients, owed) = (house.place("assets/owed/clients"), house.book.flows.iter().nth(15).unwrap().0);
        let template = &house.book.flows[owed];
        assert_eq!(template.from, clients);
        let habit = Expectation {
            origin: Origin::Habit { occurrences: 3 },
            schedule: Schedule { anchor: day(2026, 3, 26), every: axiom_core::Span::months(1), on: None, until: None },
            template,
            out: template.out,
            arrive: template.arrive,
        };
        assert!(!has_ended(&house.book, &house.run, &habit), "3,000 is still owed");
        house.run.holdings.retain(|holding| holding.place != clients);
        assert!(has_ended(&house.book, &house.run, &habit), "settled, and quiet for two months");
        // Recent activity elsewhere on the account keeps it alive.
        house.run.today = day(2026, 4, 10);
        assert!(!has_ended(&house.book, &house.run, &habit), "an invoice was written on it this month");
    }
}
