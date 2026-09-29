//! The order of the fold.
//!
//! Every fact that can change the ledger is a [`Moment`]: a day and a
//! [`Fact`]. Moments are totally ordered by `(day, kind of fact, sequence)`,
//! and that order is nothing more than `#[derive(Ord)]`: the fields are compared
//! in declaration order, and so are the variants of `Fact`. Within a day:
//!
//! 1. `Settle`: a pending flow settles (it lands now) or an actual one is
//!    returned (it reverses now), lowest flow first;
//! 2. `Flow`: journal flows in declaration order;
//! 3. `Assert`: end-of-day balance assertions, in declaration order;
//! 4. `Period`: `each month` (on the month's last day) and `each year` (on
//!    December 31) laws;
//! 5. `Deadline`: `by` laws whose date this is, in rule order.
//!
//! Prices are not moments: a price lookup asks for "the latest quote on or
//! before the day", so a price is in force from its own day for everything
//! that day.
//!
//! The facts are not stored as one sorted stream. Flows, assertions,
//! settlement changes and deadlines are each already sorted, and periods are
//! computed, so [`Timeline`] merges five cursors: a clone copies five numbers.

use axiom_core::{Day, Id};
use axiom_model::{Book, Flow, Trigger, Value};

use crate::State;
use crate::eval::{self, Context, Env};
use crate::events::Events;
use crate::scope::owner_of;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Fact {
    Settle(Id<Flow>),
    Flow(Id<Flow>),
    Assert(u32),
    Period,
    Deadline(u32),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Moment {
    pub day: Day,
    pub fact: Fact,
}

impl Moment {
    /// The last moment of `day`.
    pub fn end_of(day: Day) -> Moment {
        Moment { day, fact: Fact::Deadline(u32::MAX) }
    }

    /// After every journal flow of `day`, before its assertions: where a flow
    /// applied on `day` takes its place.
    pub fn after_flows(day: Day) -> Moment {
        Moment { day, fact: Fact::Flow(Id::new(u32::MAX)) }
    }

    pub const LAST: Moment = Moment { day: Day(i32::MAX), fact: Fact::Deadline(u32::MAX) };
}

/// The last day the fold reaches: `today`, or the journal's last fact if that
/// is later. Periods close and deadlines fire up to here and no further, so a
/// deadline the journal itself reaches (an assertion dated on it) fires even
/// when `today` is earlier, and a book with nothing after `today` never
/// closes a month in the future.
pub(crate) fn horizon(book: &Book, events: &Events, today: Day) -> Day {
    let last_flow = book.flows.as_slice().last().map(|flow| flow.day);
    let last_assert = book.asserts.last().map(|assert| assert.day);
    let last_change = events.changes.last().map(|&(day, _)| day);
    [last_flow, last_assert, last_change].into_iter().flatten().fold(today, Day::max)
}

/// A `by` law's date for one subject.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    pub day: Day,
    /// Index into `Rules::timed`.
    pub rule: usize,
}

/// The dates of every `by` rule that falls due by `horizon`, sorted. A rule
/// whose date cannot be computed (an unset property) has no deadline.
pub(crate) fn deadlines(env: Env, horizon: Day, values: &mut Vec<Value>) -> Vec<Deadline> {
    let book = env.book;
    let mut due = Vec::new();
    for (rule_index, rule) in book.rules.timed.iter().enumerate() {
        let law = &book.laws[rule.law];
        let Trigger::By(when) = law.trigger else { continue };
        let ctx = Context {
            day: rule.from,
            subject: rule.subject,
            owner: owner_of(book, rule.subject),
            motion: None,
            amount: None,
            realized: None,
        };
        let Value::Day(day) = eval::expression(env, law, when, &ctx, values) else { continue };
        if (rule.from..=rule.until).contains(&day) && day <= horizon {
            due.push(Deadline { day, rule: rule_index });
        }
    }
    due.sort_unstable_by_key(|d| (d.day, d.rule));
    due
}

/// What the timeline merges.
pub(crate) struct Sources<'a> {
    pub book: &'a Book<'a>,
    pub events: &'a Events,
    pub deadlines: &'a [Deadline],
    /// Periods and deadlines never fire past this day: [`horizon`].
    pub horizon: Day,
}

/// The five streams the timeline merges.
#[derive(Clone, Copy)]
enum Stream {
    Flow,
    Change,
    Assert,
    Period,
    Deadline,
}

/// Where the fold is in each stream, and each stream's next moment. Only the
/// stream just consumed is looked at again, so choosing the next moment is a
/// minimum over five values already in hand.
#[derive(Clone)]
pub(crate) struct Timeline {
    flow: usize,
    change: usize,
    assert: usize,
    deadline: usize,
    /// The next month end to close, if any law is periodic.
    period: Option<Day>,
    heads: [Option<Moment>; 5],
}

impl Timeline {
    /// At the start. Period ends begin with the month of the first fact:
    /// before it there is nothing to close.
    pub fn new(s: &Sources, periodic: bool) -> Timeline {
        let mut timeline = Timeline { flow: 0, change: 0, assert: 0, deadline: 0, period: None, heads: [None; 5] };
        timeline.skip_unreal(s);
        for stream in [Stream::Flow, Stream::Change, Stream::Assert, Stream::Deadline] {
            timeline.refresh(stream, s);
        }
        if periodic {
            timeline.period = timeline.peek().map(|first| first.day.month_end());
            timeline.refresh(Stream::Period, s);
        }
        timeline
    }

    /// The next moment, without consuming it.
    pub fn peek(&self) -> Option<Moment> {
        self.heads.iter().flatten().min().copied()
    }

    /// Steps past `moment`, which must be the one [`peek`](Self::peek) returned.
    pub fn consume(&mut self, moment: Moment, s: &Sources) {
        let stream = match moment.fact {
            Fact::Flow(_) => {
                self.flow += 1;
                self.skip_unreal(s);
                Stream::Flow
            }
            Fact::Settle(_) => {
                self.change += 1;
                Stream::Change
            }
            Fact::Assert(_) => {
                self.assert += 1;
                Stream::Assert
            }
            Fact::Period => {
                self.period = Some(moment.day.add_days(1).month_end());
                Stream::Period
            }
            Fact::Deadline(_) => {
                self.deadline += 1;
                Stream::Deadline
            }
        };
        self.refresh(stream, s);
    }

    fn refresh(&mut self, stream: Stream, s: &Sources) {
        self.heads[stream as usize] = match stream {
            Stream::Flow => {
                let id = Id::new(self.flow as u32);
                s.book.flows.get(id).map(|flow| Moment { day: flow.day, fact: Fact::Flow(id) })
            }
            Stream::Change => {
                s.events.changes.get(self.change).map(|&(day, id)| Moment { day, fact: Fact::Settle(id) })
            }
            Stream::Assert => {
                s.book.asserts.get(self.assert).map(|a| Moment { day: a.day, fact: Fact::Assert(self.assert as u32) })
            }
            Stream::Period => self.period.filter(|&day| day <= s.horizon).map(|day| Moment { day, fact: Fact::Period }),
            Stream::Deadline => s
                .deadlines
                .get(self.deadline)
                .map(|d| Moment { day: d.day, fact: Fact::Deadline(self.deadline as u32) }),
        };
    }

    /// Skips flows that do not move value on their own day: pending, void, and
    /// settled ones (their `Settle` moment lands them).
    fn skip_unreal(&mut self, s: &Sources) {
        while let Some(flow) = s.book.flows.get(Id::new(self.flow as u32)) {
            if matches!(s.events.state(Id::new(self.flow as u32), flow), State::Actual | State::Returned(_)) {
                break;
            }
            self.flow += 1;
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
            Moment { day, fact: Fact::Settle(b) },
            Moment { day, fact: Fact::Flow(a) },
            Moment { day, fact: Fact::Flow(b) },
            Moment { day, fact: Fact::Assert(0) },
            Moment { day, fact: Fact::Period },
            Moment { day, fact: Fact::Deadline(3) },
            Moment { day: Day(101), fact: Fact::Settle(a) },
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            Moment::end_of(day) >= order[5]
                && Moment::after_flows(day) >= order[2]
                && Moment::after_flows(day) < order[3]
        );
    }
}
