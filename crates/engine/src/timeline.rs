//! The order of the fold.
//!
//! Every fact that can change the ledger is a [`Moment`]: a day and a
//! [`Fact`]. Moments are totally ordered by `(day, kind of fact, sequence)`,
//! and that order is nothing more than `#[derive(Ord)]`: the fields are compared
//! in declaration order, and so are the variants of `Fact`. Within a day:
//!
//! 1. `Split`: a commodity is split, so the day's flows are in the new units;
//! 2. `Settle`: a pending flow settles (it lands now) or an actual one is
//!    returned (it reverses now), lowest flow first;
//! 3. `Source`: journal flows and written contract occurrences, interleaved
//!    in source transaction/flow order;
//! 4. `ClaimChange`: explicit claim write-offs after same-day movements;
//! 5. `Assert`: end-of-day balance assertions, in declaration order;
//! 6. `Deadline`: `by` laws whose date this is, and `each` laws whose period
//!    closes this day (a month's last day, December 31, or an `each year
//!    closing` law's closing day), in rule order.
//!
//! So everything that happens on a day comes before that day's closings: a
//! payment dated on the closing day counts, wherever the journal writes it. A
//! flow applied to a ledger (planned, or a withdrawal asked about) is one more
//! fact of its day, placed after the journal's flows, and comes before the
//! closings while the ledger has not closed the day.
//!
//! Prices are not moments: a price lookup asks for "the latest quote on or
//! before the day", so a price is in force from its own day for everything
//! that day.
//!
//! The facts are not stored as one sorted stream. Flows, kept occurrences,
//! assertions, splits, settlement changes and claim changes are each already
//! sorted, and deadlines come from a heap that holds the next one of every
//! timed rule. [`Timeline`] merges those sparse streams with cursors.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use axiom_core::calendar::Window;
use axiom_core::{Day, Days, Id, Period};
use axiom_model::{Book, Closing, Flow, Law, Mode, Rule, Subject, Trigger, Txn, Value};

use crate::State;
use crate::checkpoint::CheckpointPhase;
use crate::eval::{self, Env, Occasion};
use crate::events::Events;
use crate::plan::Plan;
use crate::scope::owner_of;
use crate::state::World;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Fact {
    Split(u32),
    Settle(Id<Flow>),
    /// A source-ordered journal flow or a written contract occurrence.
    Source(SourceOrder, SourceFact),
    /// Full write-off of all remaining claims from one source transaction.
    ClaimChange(u32),
    Assert(u32),
    /// A timed rule (an index into `Rules::timed`) and the days it runs for:
    /// the deadline itself, or the month or year it closes.
    Deadline(u32, Days),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct SourceOrder {
    pub txn: u32,
    pub flow: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum SourceFact {
    Flow(Id<Flow>),
    Occurrence(Id<Txn>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Moment {
    pub day: Day,
    pub fact: Fact,
}

impl Moment {
    /// The last moment of `day`.
    pub fn end_of(day: Day) -> Moment {
        Moment { day, fact: Fact::Deadline(u32::MAX, Days::ALWAYS) }
    }

    /// After every journal flow of `day`, before its assertions: where a flow
    /// applied on `day` takes its place.
    pub fn after_flows(day: Day) -> Moment {
        Moment {
            day,
            fact: Fact::Source(SourceOrder { txn: u32::MAX, flow: u32::MAX }, SourceFact::Flow(Id::new(u32::MAX))),
        }
    }

    /// After everything the journal holds for `day`, before its closings.
    pub fn before_closings(day: Day) -> Moment {
        Moment { day, fact: Fact::Assert(u32::MAX) }
    }

    pub const LAST: Moment = Moment { day: Day::MAX, fact: Fact::Deadline(u32::MAX, Days::ALWAYS) };
}

/// The day of the first fact that starts a period: a flow that moves value on
/// its own day, an assertion, a settlement or a split. Opening balances start
/// nothing, so the laws that close periods begin with the first real fact.
pub(crate) fn start(book: &Book, events: &Events) -> Option<Day> {
    let real = |(id, flow): (Id<Flow>, &Flow)| {
        (flow.mode != Mode::Opening && matches!(events.state(id, flow), State::Actual | State::Returned(_)))
            .then_some(flow.day)
    };
    let flow = book.flows.iter().find_map(real);
    let occurrences = book.txns.iter().filter_map(|(_, txn)| txn.occurrence.map(|_| txn.day));
    let others = [
        book.asserts.first().map(|a| a.day),
        events.changes.first().map(|&(day, _)| day),
        book.splits.first().map(|s| s.day),
        book.claim_changes.first().map(|change| change.day),
    ];
    others.into_iter().chain([flow]).flatten().chain(occurrences).min()
}

/// The last day the journal has a fact on. Periods close and deadlines fire up
/// to the later of this and the day the fold is run to, and no further, so a
/// deadline the journal itself reaches (an assertion dated on it) fires even
/// when `today` is earlier, and a book with nothing after `today` never closes
/// a month in the future.
pub(crate) fn last_fact(book: &Book, events: &Events) -> Option<Day> {
    let last_flow = book.flows.as_slice().last().map(|flow| flow.day);
    let last_assert = book.asserts.last().map(|assert| assert.day);
    let last_change = events.changes.last().map(|&(day, _)| day);
    let last_occurrence = book.txns.iter().filter_map(|(_, txn)| txn.occurrence.map(|_| txn.day)).max();
    let last_claim_change = book.claim_changes.last().map(|change| change.day);
    [last_flow, last_assert, last_change, last_occurrence, last_claim_change].into_iter().flatten().max()
}

/// A `by` law's date for one subject, or the day an `each` law closes a period.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Deadline {
    pub day: Day,
    /// Index into `Rules::timed`.
    pub rule: u32,
    /// The days the law runs for: the deadline itself, or the month or year it closes.
    pub period: Days,
}

/// When a timed rule falls due, as far as the plan can tell: everything but
/// the horizon, which is the fold's to say.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Schedule {
    /// A `by` whose date cannot be computed (an unset property), or a law that is not timed.
    Never,
    /// A `by` law's date, worked out once.
    Once(Day),
    /// Each month or year closes on its last day, or a year on the `closing` day of the next.
    Every(Period, Option<Closing>),
}

impl Schedule {
    /// How the rule falls due. A `by` date is read on the empty book, where no
    /// balance or holding exists yet, so it depends on the laws' own terms only.
    pub fn of(plan: &Plan, rule: &Rule, empty: &World, values: &mut Vec<Value>) -> Schedule {
        let (book, law) = (plan.book, &plan.book.laws[rule.law]);
        match law.trigger {
            Trigger::By(when) => {
                let day = rule.days.first();
                let on = Occasion::time(day, Days::on(day));
                let ctx = eval::Context::new(rule.subject, owner_of(book, rule.subject), &on);
                match eval::expression(Env { plan, world: empty }, law, when, &ctx, values) {
                    Value::Day(day) => Schedule::Once(day),
                    _ => Schedule::Never,
                }
            }
            Trigger::Each(period, closing) => Schedule::Every(period, closing.filter(|_| period == Period::Year)),
            _ => Schedule::Never,
        }
    }

    /// The first deadline of `rule`, given the first day a period can start.
    /// A period closes only if the rule was in force for some day of it, and
    /// periods begin with the month or year of `first`: before it there is
    /// nothing to close.
    fn first(self, index: u32, rule: &Rule, first: Option<Day>) -> Option<Deadline> {
        match self {
            Schedule::Never => None,
            Schedule::Once(day) => {
                let period = Days::on(day);
                rule.days.overlaps(period).then_some(Deadline { day, rule: index, period })
            }
            Schedule::Every(period, _) => {
                self.closing(index, rule, Window::containing(period, first?.max(rule.days.first())))
            }
        }
    }

    /// The deadline after `done`.
    fn after(self, index: u32, rule: &Rule, done: Deadline) -> Option<Deadline> {
        let Schedule::Every(period, _) = self else { return None };
        self.closing(index, rule, Window::containing(period, done.period.first()).next())
    }

    /// The closing of the first window from `window` on that the rule is in force in.
    fn closing(self, index: u32, rule: &Rule, mut window: Window) -> Option<Deadline> {
        loop {
            let period = window.days();
            if period.first() > rule.days.last() {
                return None;
            }
            if rule.days.overlaps(period) {
                let day = match self {
                    Schedule::Every(_, Some(closing)) => {
                        let closes = closing.day_for(period.first().year());
                        closes.unwrap_or(period.last().add_days(1).month_end())
                    }
                    _ => period.last(),
                };
                return Some(Deadline { day, rule: index, period });
            }
            window = window.next();
        }
    }
}

/// What a deadline closes: a law for a subject over a period.
fn key(plan: &Plan, due: Deadline) -> (Id<Law>, Subject, Day) {
    let rule = &plan.book.rules.timed[due.rule as usize];
    (rule.law, rule.subject, due.period.first())
}

/// The streams the timeline merges.
#[derive(Clone, Copy, PartialEq)]
enum Stream {
    Split,
    Flow,
    Occurrence,
    Change,
    ClaimChange,
    Assert,
    Deadline,
}

impl Stream {
    const ALL: [Stream; 7] = [
        Stream::Split,
        Stream::Flow,
        Stream::Occurrence,
        Stream::Change,
        Stream::ClaimChange,
        Stream::Assert,
        Stream::Deadline,
    ];

    fn of(fact: Fact) -> Stream {
        match fact {
            Fact::Split(_) => Stream::Split,
            Fact::Source(_, SourceFact::Flow(_)) => Stream::Flow,
            Fact::Source(_, SourceFact::Occurrence(_)) => Stream::Occurrence,
            Fact::Settle(_) => Stream::Change,
            Fact::ClaimChange(_) => Stream::ClaimChange,
            Fact::Assert(_) => Stream::Assert,
            Fact::Deadline(..) => Stream::Deadline,
        }
    }
}

/// Where the fold is in each stream, and each stream's next moment. Only the
/// stream just consumed is looked at again, so choosing the next moment is a
/// minimum over five values already in hand.
#[derive(Clone)]
pub(crate) struct Timeline {
    /// How many facts of each stream but the deadlines were consumed.
    done: [usize; 6],
    heads: [Option<Moment>; 7],
    /// The next deadline of every timed rule that has one, soonest first.
    due: BinaryHeap<Reverse<Deadline>>,
    /// What closed on the day of the last deadline, by law, subject and the
    /// period's first day: two residences under one system bring its law
    /// twice, and it runs once for each period.
    closed: (Day, Vec<(Id<Law>, Subject, Day)>),
}

impl Timeline {
    /// At the start.
    pub fn new(plan: &Plan) -> Timeline {
        let rules = plan.book.rules.timed.iter().zip(plan.timed.iter());
        let first =
            rules.enumerate().filter_map(|(at, (rule, schedule))| schedule.first(at as u32, rule, plan.schedule_start));
        let mut timeline = Timeline {
            done: [0; 6],
            heads: [None; 7],
            due: first.map(Reverse).collect(),
            closed: (Day::MIN, Vec::new()),
        };
        timeline.skip_unreal(plan);
        Stream::ALL.into_iter().for_each(|stream| timeline.refresh(stream, plan));
        timeline
    }

    /// As it stands once everything through the end of `day` is consumed,
    /// found by search: each stream's cursor is a binary search, and only the
    /// deadlines already passed are worked through.
    pub fn after(plan: &Plan, day: Day) -> Timeline {
        Timeline::seek(plan, day, CheckpointPhase::EndOfDay)
    }

    /// As it stands after the journal's facts on `day`, before its closings.
    /// A resumed view can still apply a hypothetical flow before those laws run.
    pub fn before_closings(plan: &Plan, day: Day) -> Timeline {
        Timeline::seek(plan, day, CheckpointPhase::BeforeClosings)
    }

    /// As it stands after all journal flows on `day`, before assertions and closings.
    pub fn after_flows(plan: &Plan, day: Day) -> Timeline {
        Timeline::seek(plan, day, CheckpointPhase::AfterFlows)
    }

    fn seek(plan: &Plan, day: Day, phase: CheckpointPhase) -> Timeline {
        let (assertions, closings) = (phase != CheckpointPhase::AfterFlows, phase == CheckpointPhase::EndOfDay);
        let (book, changes) = (plan.book, &plan.events.changes);
        let mut timeline = Timeline::new(plan);
        timeline.done[Stream::Split as usize] = book.splits.partition_point(|split| split.day <= day);
        timeline.done[Stream::Flow as usize] = book.flows.as_slice().partition_point(|flow| flow.day <= day);
        timeline.done[Stream::Occurrence as usize] =
            plan.occurrence_txns.partition_point(|&id| book.txns[id].day <= day);
        timeline.done[Stream::Change as usize] = changes.partition_point(|&(when, _)| when <= day);
        timeline.done[Stream::ClaimChange as usize] = book.claim_changes.partition_point(|change| change.day <= day);
        timeline.done[Stream::Assert as usize] =
            book.asserts.partition_point(|assert| assert.day < day || (assertions && assert.day == day));
        while timeline.due.peek().is_some_and(|&Reverse(due)| due.day < day || (closings && due.day == day)) {
            timeline.close(plan);
        }
        timeline.skip_unreal(plan);
        Stream::ALL.into_iter().for_each(|stream| timeline.refresh(stream, plan));
        timeline
    }

    /// The next moment, without consuming it.
    pub fn peek(&self) -> Option<Moment> {
        self.heads.iter().flatten().min().copied()
    }

    /// Steps past `moment`, which must be the one [`peek`](Self::peek) returned.
    pub fn consume(&mut self, moment: Moment, plan: &Plan) {
        let stream = Stream::of(moment.fact);
        match stream {
            Stream::Deadline => self.close(plan),
            _ => self.done[stream as usize] += 1,
        }
        if stream == Stream::Flow {
            self.skip_unreal(plan);
        }
        self.refresh(stream, plan);
    }

    fn refresh(&mut self, stream: Stream, plan: &Plan) {
        let book = plan.book;
        // Only the four streams read from tables have a cursor.
        let at = || self.done[stream as usize];
        self.heads[stream as usize] = match stream {
            Stream::Split => book.splits.get(at()).map(|sp| Moment { day: sp.day, fact: Fact::Split(at() as u32) }),
            Stream::Flow => {
                let id = Id::new(at() as u32);
                book.flows.get(id).and_then(|flow| {
                    let txn = book.txns.get(flow.txn)?;
                    if txn.occurrence.is_some() {
                        return None;
                    }
                    let local = id.index().checked_sub(txn.flows.start().index())?;
                    let source =
                        SourceOrder { txn: u32::try_from(flow.txn.index()).ok()?, flow: u32::try_from(local).ok()? };
                    Some(Moment { day: flow.day, fact: Fact::Source(source, SourceFact::Flow(id)) })
                })
            }
            Stream::Occurrence => plan.occurrence_txns.get(at()).and_then(|&id| {
                let txn = book.txns.get(id)?;
                Some(Moment {
                    day: txn.day,
                    fact: Fact::Source(
                        SourceOrder { txn: u32::try_from(id.index()).ok()?, flow: 0 },
                        SourceFact::Occurrence(id),
                    ),
                })
            }),
            Stream::Change => plan.events.changes.get(at()).map(|&(day, id)| Moment { day, fact: Fact::Settle(id) }),
            Stream::ClaimChange => book
                .claim_changes
                .get(at())
                .map(|change| Moment { day: change.day, fact: Fact::ClaimChange(at() as u32) }),
            Stream::Assert => book.asserts.get(at()).map(|a| Moment { day: a.day, fact: Fact::Assert(at() as u32) }),
            Stream::Deadline => {
                let next = self.due.peek();
                next.map(|&Reverse(d)| Moment { day: d.day, fact: Fact::Deadline(d.rule, d.period) })
            }
        };
    }

    /// Takes the soonest deadline off the heap, and drops what would only run
    /// the same law again for the same period.
    fn close(&mut self, plan: &Plan) {
        let Some(Reverse(done)) = self.due.pop() else { return };
        self.passed(plan, done);
        while let Some(&Reverse(top)) = self.due.peek()
            && self.closed.0 == top.day
            && self.closed.1.contains(&key(plan, top))
        {
            self.due.pop();
            self.passed(plan, top);
        }
    }

    /// Puts the rule's next deadline on the heap, and notes that its period is closed.
    fn passed(&mut self, plan: &Plan, done: Deadline) {
        let rule = &plan.book.rules.timed[done.rule as usize];
        self.due.extend(plan.timed[done.rule as usize].after(done.rule, rule, done).map(Reverse));
        if self.closed.0 != done.day {
            self.closed = (done.day, Vec::new());
        }
        self.closed.1.push(key(plan, done));
    }

    /// Skips flows that do not move value on their own day: pending, void, and
    /// settled ones (their `Settle` moment lands them).
    fn skip_unreal(&mut self, plan: &Plan) {
        let next = &mut self.done[Stream::Flow as usize];
        while let Some(flow) = plan.book.flows.get(Id::new(*next as u32)) {
            let id = Id::new(*next as u32);
            if plan.book.txns[flow.txn].occurrence.is_some() {
                *next += 1;
                continue;
            }
            if matches!(plan.events.state(id, flow), State::Actual | State::Returned(_)) {
                break;
            }
            *next += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_derived_order_is_the_documented_one() {
        let (a, b) = (Id::new(1), Id::new(2));
        let day = Day(100);
        let order = [
            Moment { day, fact: Fact::Split(3) },
            Moment { day, fact: Fact::Settle(b) },
            Moment { day, fact: Fact::Source(SourceOrder { txn: 1, flow: 0 }, SourceFact::Flow(a)) },
            Moment { day, fact: Fact::Source(SourceOrder { txn: 1, flow: 1 }, SourceFact::Flow(b)) },
            Moment { day, fact: Fact::ClaimChange(0) },
            Moment { day, fact: Fact::Assert(0) },
            Moment { day, fact: Fact::Deadline(3, Days::on(day)) },
            Moment { day: Day(101), fact: Fact::Split(0) },
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            Moment::end_of(day) >= order[5]
                && Moment::after_flows(day) >= order[3]
                && Moment::after_flows(day) < order[4]
        );
    }

    #[test]
    fn source_transactions_interleave_by_transaction_before_claim_changes() {
        let day = Day(100);
        let first =
            Moment { day, fact: Fact::Source(SourceOrder { txn: 4, flow: 0 }, SourceFact::Occurrence(Id::new(4))) };
        let next_flow =
            Moment { day, fact: Fact::Source(SourceOrder { txn: 5, flow: 0 }, SourceFact::Flow(Id::new(9))) };
        let claim = Moment { day, fact: Fact::ClaimChange(0) };
        let assertion = Moment { day, fact: Fact::Assert(0) };
        assert!(first < next_flow && next_flow < claim && claim < assertion);
    }
}
