//! What is expected to happen again: plans, and the rhythms history shows.
//!
//! Both come out as [`Expectation`]s. A plan is written down, so it is always
//! believed. A rhythm is learned from real flows, and is dropped when a plan
//! says the same thing, or when the account behind it has run dry.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Class, Entity, Flow, Place};

use super::recurrence::{Schedule, detect, median};
use crate::history::{Posting, postings};
use crate::lens::Lens;
use crate::synth::planned;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
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
    pub fn flows(&self, after: Day, horizon: Day) -> impl Iterator<Item = Flow> + '_ {
        self.schedule.days(after, horizon).map(|day| planned(self.template, day, self.out, self.arrive))
    }

    /// Whether a real flow is one of this expectation's kind: the same pair.
    pub fn covers(&self, flow: &Flow) -> bool {
        (self.template.from, self.template.to) == (flow.from, flow.to)
    }

    /// The flows of one transaction, or one plan, are one thing that recurs.
    pub fn group(&self) -> (bool, u32) {
        let Origin::Habit { .. } = self.origin;
        (true, self.template.txn.index() as u32)
    }
}

/// Everything expected after `today` for the lens's owners: plans, and the
/// rhythms in history that no plan already says.
pub fn expected<'b>(lens: Lens<'b, '_, '_, '_>, run: &Run) -> Vec<Expectation<'b>> {
    let book = lens.book();
    let mut expected: Vec<_> =
        from_history(book, run).into_iter().filter(|habit| !has_ended(book, run, habit)).collect();
    expected.retain(|item| lens.owns(item.template.from) || lens.owns(item.template.to));
    expected
}

/// Whether a contract covers this exact movement on its scheduled date,
/// preserving explicit waivers so a fallback rhythm does not reappear.
pub fn covered_by_contract(book: &Book, flow: &Flow) -> bool {
    covered_on(book, flow, flow.day)
}

/// Whether a contract covers a template's movement on a projected date.
pub fn covered_on(book: &Book, template: &Flow, day: Day) -> bool {
    book.contracts.iter().any(|(_, contract)| contract.covers(template, day) != axiom_model::ContractCoverage::None)
}

/// Who pays whom, through whom: what makes two flows the same habit.
type Key = (Id<Place>, Id<Place>, Option<Id<Entity>>);

/// One expectation per rhythm found among written flows, grouped by
/// (from, to, payee). Contract occurrences are supplied by the engine monitor.
fn from_history<'b>(book: &'b Book, run: &Run) -> Vec<Expectation<'b>> {
    let mut groups: BTreeMap<Key, Vec<Posting>> = BTreeMap::new();
    let candidates = postings(book, run).filter(|posting| {
        let flow = posting.flow;
        let unknown = book.entities[book.roots.unknown].place;
        posting.is_real_on(run.today)
            && Some(flow.from) != unknown
            && Some(flow.to) != unknown
            && matches!(flow.origin, axiom_model::Origin::Written)
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
    ends.into_iter().filter(|(place, _)| matches!(book.places[*place].class, Class::Asset | Class::Debt)).any(
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
    use axiom_core::Days;
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

    /// Suppression follows each contract interval, including explicit waivers,
    /// while keeping unrelated or out-of-term rhythms visible.
    #[test]
    fn contract_suppression_tracks_each_projected_date_and_typed_identity() {
        use axiom_core::{Loc, Timeline};
        use axiom_model::{Cadence, Contract, Expr, FlowSide, Header, Program, Promised, Quantity, Terms};

        let mut house = household();
        let today = day(2026, 5, 1);
        let start = today.add_days(10);
        let end = today.add_days(40);
        let template_id = house.book.flows.iter().next().unwrap().0;
        let template = house.book.flows[template_id].clone();
        let terms = Terms {
            every: Cadence::Every(axiom_core::Span::months(1)),
            on: Box::default(),
            template: vec![Promised {
                header: Header {
                    flow: template.clone(),
                    out: Quantity::Amount(Expr::Literal(template.out)),
                    arrive: Quantity::Amount(Expr::Literal(template.arrive)),
                },
                side: FlowSide::Out,
                legs: Box::default(),
                items: Box::default(),
            }]
            .into(),
            program: Program::default(),
            inputs: Box::default(),
            estimate: false,
            due: None,
            grace: Some(axiom_core::Span::default()),
            period: None,
            covers: None,
            prorated: false,
            escalation: None,
            rate: None,
        };
        let contract = Contract {
            name: house.book.names.intern("rent-promise"),
            party: Id::new(0),
            owner: Id::new(0),
            purpose: None,
            description: None,
            area: None,
            days: Days::new(start, end).unwrap(),
            terms: Some(terms),
            standing: None,
            waived: Timeline::new(None),
            buys: None,
            deposit: None,
            deposit_holding: None,
            loan: None,
            ended: None,
            laws: Box::default(),
            doc: None,
            loc: Loc::default(),
        };
        house.book.contracts.push(contract);
        assert!(!covered_on(&house.book, &template, today.add_days(5)), "start after today does not suppress now");
        assert!(
            covered_on(&house.book, &template, start.add_days(1)),
            "coverage does not require the contract due day"
        );
        assert!(
            !covered_on(&house.book, &template, end.add_days(1)),
            "an ended contract does not suppress later dates"
        );

        let waiver_start = start.add_days(10);
        let waiver_end = start.add_days(14);
        let days = Days::new(waiver_start, waiver_end).unwrap();
        let waiver = axiom_model::Change { days, description: None, code: None, loc: Loc::default() };
        house.book.contracts[Id::new(0)].waived.paint(days, Some(waiver));
        assert!(
            covered_on(&house.book, &template, waiver_start.add_days(1)),
            "a waived stretch keeps the contract's template, so the fallback does not resurrect"
        );
        let mut other_owner = template.clone();
        other_owner.owner = Id::new(99);
        assert!(!covered_on(&house.book, &other_owner, start.add_days(1)));
        let mut other_unit = template.clone();
        other_unit.out.unit = Id::new(99);
        assert!(!covered_on(&house.book, &other_unit, start.add_days(1)));
    }
}
