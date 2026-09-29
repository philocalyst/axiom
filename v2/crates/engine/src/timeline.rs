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
//! 3. `Flow`: journal flows in declaration order;
//! 4. `Assert`: end-of-day balance assertions, in declaration order;
//! 5. `Deadline`: `by` laws whose date this is, and `each` laws whose period
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
//! The facts are not stored as one sorted stream. Flows, assertions, splits,
//! settlement changes and deadlines (periods included) are each already
//! sorted, so [`Timeline`] merges five cursors: a clone copies five numbers.

use std::iter::successors;

use axiom_core::{Day, Id, Set};
use axiom_model::{Book, Flow, Mode, Period, Recognition, Trigger, Value};

use crate::State;
use crate::eval::{self, Env, Occasion};
use crate::events::Events;
use crate::scope::owner_of;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Fact {
    Split(u32),
    Settle(Id<Flow>),
    Flow(Id<Flow>),
    Assert(u32),
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

    /// After everything the journal holds for `day`, before its closings.
    pub fn before_closings(day: Day) -> Moment {
        Moment { day, fact: Fact::Assert(u32::MAX) }
    }

    pub const LAST: Moment = Moment { day: Day(i32::MAX), fact: Fact::Deadline(u32::MAX) };
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
    let others = [
        book.asserts.first().map(|a| a.day),
        events.changes.first().map(|&(day, _)| day),
        book.splits.first().map(|s| s.day),
    ];
    others.into_iter().chain([flow]).flatten().min()
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

/// A `by` law's date for one subject, or the day an `each` law closes a period.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    pub day: Day,
    /// Index into `Rules::timed`.
    pub rule: usize,
    /// The days the law runs for: the deadline itself, or the month or year it closes.
    pub period: Recognition,
}

/// Every deadline that falls due by `horizon`, sorted. A rule whose date
/// cannot be computed (an unset property) has no deadline; a period closes
/// only if the rule was in force for some day of it. Periods begin with the
/// month or year of `first`, the first fact that starts one: before it there
/// is nothing to close.
pub(crate) fn deadlines(env: Env, horizon: Day, first: Option<Day>, values: &mut Vec<Value>) -> Vec<Deadline> {
    let book = env.book;
    let mut due = Vec::new();
    for (rule_index, rule) in book.rules.timed.iter().enumerate() {
        let law = &book.laws[rule.law];
        let mut close = |day: Day, period: Recognition| {
            if rule.from <= period.until && period.from <= rule.until {
                due.push(Deadline { day, rule: rule_index, period });
            }
        };
        match law.trigger {
            Trigger::By(when) => {
                let day = rule.from;
                let on = Occasion::time(day, Recognition::on(day));
                let ctx = eval::Context::new(rule.subject, owner_of(book, rule.subject), &on);
                let Value::Day(day) = eval::expression(env, law, when, &ctx, values) else { continue };
                if day <= horizon {
                    close(day, Recognition::on(day));
                }
            }
            Trigger::Each(Period::Month, _) => {
                let ends = successors(first.map(Day::month_end), |end| Some(end.add_days(1).month_end()));
                ends.take_while(|&end| end <= horizon)
                    .for_each(|end| close(end, Recognition { from: end.month_start(), until: end }));
            }
            Trigger::Each(Period::Year, closing) => {
                for year in first.map_or(0..0, |first| first.year()..horizon.year() + 1) {
                    let period = Recognition {
                        from: Day::from_ymd(year, 1, 1).unwrap_or(horizon),
                        until: Day::from_ymd(year, 12, 31).unwrap_or(horizon),
                    };
                    let closes = closing.map_or(Some(period.until), |closing| closing.day_for(year));
                    let closes = closes.unwrap_or(period.until.add_days(1).month_end());
                    if closes <= horizon {
                        close(closes, period);
                    }
                }
            }
            _ => {}
        }
    }
    due.sort_unstable_by_key(|d| (d.day, d.rule));
    // Two residences under one system bring its law twice: it runs once for each period.
    let mut seen = Set::default();
    due.retain(|d| {
        let rule = &book.rules.timed[d.rule];
        seen.insert((d.day, rule.law, rule.subject, d.period.from))
    });
    due
}

/// What the timeline merges.
pub(crate) struct Sources<'a> {
    pub book: &'a Book<'a>,
    pub events: &'a Events,
    pub deadlines: &'a [Deadline],
}

/// The streams the timeline merges.
#[derive(Clone, Copy, PartialEq)]
enum Stream {
    Split,
    Flow,
    Change,
    Assert,
    Deadline,
}

impl Stream {
    const ALL: [Stream; 5] = [Stream::Split, Stream::Flow, Stream::Change, Stream::Assert, Stream::Deadline];

    fn of(fact: Fact) -> Stream {
        match fact {
            Fact::Split(_) => Stream::Split,
            Fact::Flow(_) => Stream::Flow,
            Fact::Settle(_) => Stream::Change,
            Fact::Assert(_) => Stream::Assert,
            Fact::Deadline(_) => Stream::Deadline,
        }
    }
}

/// Where the fold is in each stream, and each stream's next moment. Only the
/// stream just consumed is looked at again, so choosing the next moment is a
/// minimum over five values already in hand.
#[derive(Clone)]
pub(crate) struct Timeline {
    /// How many facts of each stream were consumed.
    done: [usize; 5],
    heads: [Option<Moment>; 5],
}

impl Timeline {
    /// At the start.
    pub fn new(s: &Sources) -> Timeline {
        let mut timeline = Timeline { done: [0; 5], heads: [None; 5] };
        timeline.skip_unreal(s);
        Stream::ALL.into_iter().for_each(|stream| timeline.refresh(stream, s));
        timeline
    }

    /// The next moment, without consuming it.
    pub fn peek(&self) -> Option<Moment> {
        self.heads.iter().flatten().min().copied()
    }

    /// Steps past `moment`, which must be the one [`peek`](Self::peek) returned.
    pub fn consume(&mut self, moment: Moment, s: &Sources) {
        let stream = Stream::of(moment.fact);
        self.done[stream as usize] += 1;
        if stream == Stream::Flow {
            self.skip_unreal(s);
        }
        self.refresh(stream, s);
    }

    fn refresh(&mut self, stream: Stream, s: &Sources) {
        let at = self.done[stream as usize];
        self.heads[stream as usize] = match stream {
            Stream::Split => s.book.splits.get(at).map(|sp| Moment { day: sp.day, fact: Fact::Split(at as u32) }),
            Stream::Flow => {
                let id = Id::new(at as u32);
                s.book.flows.get(id).map(|flow| Moment { day: flow.day, fact: Fact::Flow(id) })
            }
            Stream::Change => s.events.changes.get(at).map(|&(day, id)| Moment { day, fact: Fact::Settle(id) }),
            Stream::Assert => s.book.asserts.get(at).map(|a| Moment { day: a.day, fact: Fact::Assert(at as u32) }),
            Stream::Deadline => s.deadlines.get(at).map(|d| Moment { day: d.day, fact: Fact::Deadline(at as u32) }),
        };
    }

    /// Skips flows that do not move value on their own day: pending, void, and
    /// settled ones (their `Settle` moment lands them).
    fn skip_unreal(&mut self, s: &Sources) {
        let next = &mut self.done[Stream::Flow as usize];
        while let Some(flow) = s.book.flows.get(Id::new(*next as u32)) {
            if matches!(s.events.state(Id::new(*next as u32), flow), State::Actual | State::Returned(_)) {
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
            Moment { day, fact: Fact::Flow(a) },
            Moment { day, fact: Fact::Flow(b) },
            Moment { day, fact: Fact::Assert(0) },
            Moment { day, fact: Fact::Deadline(3) },
            Moment { day: Day(101), fact: Fact::Split(0) },
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            Moment::end_of(day) >= order[5]
                && Moment::after_flows(day) >= order[3]
                && Moment::after_flows(day) < order[4]
        );
    }
}
