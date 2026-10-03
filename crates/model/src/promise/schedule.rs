//! The days one stream of occurrences falls due, and what a day asks of them.
//!
//! [`Dues`] says the days a cadence, its `on` and an anchor give. A schedule is those, and two things a contract adds:
//! the last day it lives (`until`, or where an `ends` cut it) and the stretches that are waived. They are what an
//! ordinal and a due day are counted over, so they are here and not in the fold:
//!
//! * an **ordinal** is an index among the days that are *owed*: the due days in the contract's life that no waiver
//!   takes out, counted from its first. [`Sched::ordinal`] and [`Sched::nth`] are inverses, in O(log n);
//! * a **hole** is a waived stretch and the number of due days it swallows. Counting past it is a subtraction, so the
//!   count of the days before a day is the count of the cadence's less the holes before it, and the `n`th owed day is
//!   the cadence's `n + (days swallowed by the holes before it)`th;
//! * **keeping** is the matching of a line to its due day: the nearest owed day on either side, the earlier when they
//!   are equally near, if it is within the contract's reach.

use axiom_core::{Cadence, Day, Days, Dues, Id, On, Ratio, Run};

use super::{Promises, Reckoning};
use crate::book::{Book, ForecastError, ScheduleKind};

/// One stream of a contract's occurrences: a regular schedule, or a standing order's.
#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    kind: ScheduleKind,
    every: Cadence,
    on: Run<On>,
    /// The days the contract lives, the first being where the cadence counts from.
    life: Days,
    skips: Run<Skip>,
    reckoning: Id<Reckoning>,
}

const _: () = assert!(size_of::<Schedule>() <= 48);

impl Schedule {
    pub(super) fn new(
        kind: ScheduleKind,
        every: Cadence,
        on: Run<On>,
        life: Days,
        skips: Run<Skip>,
        reckoning: Id<Reckoning>,
    ) -> Schedule {
        Schedule { kind, every, on, life, skips, reckoning }
    }
}

/// A waived stretch of a schedule: its days, and where in the cadence's days it is.
#[derive(Clone, Copy, Debug)]
pub struct Skip {
    days: Days,
    /// How many of the cadence's days are before the stretch.
    from: u32,
    /// How many due days the stretches before this one swallow.
    before: u32,
    /// How many due days this one swallows.
    gone: u32,
}

const _: () = assert!(size_of::<Skip>() <= 24);

impl Skip {
    /// The stretch `days` of a schedule whose cadence gives `dues`, after stretches that swallow `before` of them. A
    /// schedule that can only be walked has no counts: finding them would walk from its first day, to the end of the
    /// stretch, whenever a book is built.
    pub(super) fn of(days: Days, dues: &Dues<'_>, before: u32) -> Skip {
        if !dues.is_counted() {
            return Skip { days, from: 0, before: 0, gone: 0 };
        }
        let from = dues.before(days.first());
        Skip { days, from, before, gone: through(dues, days.last()) - from }
    }

    /// How many due days it swallows.
    pub fn gone(&self) -> u32 {
        self.gone
    }

    /// How many days are owed before it: the cadence's, less the ones swallowed.
    fn owed_before(&self) -> u32 {
        self.from - self.before
    }
}

/// How many of the days `dues` gives are on or before `day`.
fn through(dues: &Dues<'_>, day: Day) -> u32 {
    let before = dues.before(day);
    before.saturating_add(u32::from(dues.nth(before) == Some(day)))
}

/// The owed day nearest a line's, and how far it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nearest {
    pub due: Day,
    pub apart: i64,
}

/// Which due day a line keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keep {
    /// No day of the contract's schedules is near enough, or the line is not in its life.
    Outside,
    Kept {
        schedule: ScheduleKind,
        due: Day,
    },
    /// Equally near a regular and a standing day.
    Ambiguous {
        regular: Day,
        standing: Day,
    },
}

/// A schedule with the pools it reads: what a promise is asked.
#[derive(Clone, Copy)]
pub struct Sched<'p> {
    promises: &'p Promises,
    schedule: &'p Schedule,
}

impl<'p> Sched<'p> {
    pub(super) fn new(promises: &'p Promises, id: Id<Schedule>) -> Sched<'p> {
        Sched { promises, schedule: &promises.schedules[id] }
    }

    pub fn kind(&self) -> ScheduleKind {
        self.schedule.kind
    }

    /// The days the contract lives.
    pub fn life(&self) -> Days {
        self.schedule.life
    }

    /// The `n`th owed day, counting from 0.
    pub fn nth(&self, n: u32) -> Option<Day> {
        self.nth_in(&self.dues(), n)
    }

    /// How many owed days are before `day`: the index of the first on or after it.
    pub fn before(&self, day: Day) -> u32 {
        self.before_in(&self.dues(), day)
    }

    /// The index of `due` among the owed days, if it is one.
    pub fn ordinal(&self, due: Day) -> Option<u32> {
        let dues = self.dues();
        let at = self.before_in(&dues, due);
        (self.nth_in(&dues, at) == Some(due)).then_some(at)
    }

    /// The owed days in `window`, in order.
    pub fn days(&self, window: Days) -> impl Iterator<Item = Day> + 'p {
        let (this, window) = (*self, window.intersect(self.schedule.life));
        window
            .into_iter()
            .flat_map(move |window| this.dues().days(window))
            .filter(move |day| this.counted(*day).is_ok())
    }

    /// The owed day nearest `day`, within `reach` of it: the earlier of two equally near. It looks at the days within
    /// reach and no further, so that it does not need to know how many days there are before them.
    pub fn nearest(&self, day: Day, reach: i32) -> Option<Nearest> {
        let window = Days::new(Day(day.0.saturating_sub(reach)), Day(day.0.saturating_add(reach)))?;
        let near = |due: Day| Nearest { due, apart: (i64::from(due.0) - i64::from(day.0)).abs() };
        self.days(window).map(near).min_by_key(|nearest| (nearest.apart, nearest.due > day))
    }

    /// The multiplier of an occurrence on `day`: what `amount_on_schedule` says of it.
    pub fn factor(&self, book: &Book<'_>, day: Day) -> Result<Ratio, ForecastError> {
        self.counted(day)?;
        self.promises.reckoning(self.schedule.reckoning).factor(book, self.schedule.life, day)
    }

    /// The days an occurrence on `day` is recognized over: what `recognition_on_schedule` says of it.
    pub fn recognized(&self, day: Day) -> Result<Days, ForecastError> {
        self.counted(day)?;
        self.promises.reckoning(self.schedule.reckoning).recognized(day)
    }

    /// Whether `day` is one the schedule can say anything of: in the contract's life and not waived.
    fn counted(&self, day: Day) -> Result<(), ForecastError> {
        if !self.schedule.life.contains(day) {
            return Err(ForecastError::OutsideContract(day));
        }
        let holes = self.holes();
        let at = holes.partition_point(|hole| hole.days.last() < day);
        match holes.get(at) {
            Some(hole) if hole.days.contains(day) => Err(ForecastError::Waived(day)),
            _ => Ok(()),
        }
    }

    fn dues(&self) -> Dues<'p> {
        let on = self.schedule.on.get(&self.promises.landings).expect("a run of this arena");
        Dues::new(self.schedule.every, on, self.schedule.life.first())
    }

    fn holes(&self) -> &'p [Skip] {
        self.schedule.skips.get(&self.promises.skips).expect("a run of this arena")
    }

    fn nth_in(&self, dues: &Dues<'_>, n: u32) -> Option<Day> {
        if !dues.is_counted() {
            return self.days(self.schedule.life).nth(n as usize);
        }
        let holes = self.holes();
        // The holes this day is past are those with no more than `n` owed days before them.
        let passed = holes.partition_point(|hole| hole.owed_before() <= n);
        let swallowed = passed.checked_sub(1).map_or(0, |last| holes[last].before + holes[last].gone);
        let day = dues.nth(n.checked_add(swallowed)?)?;
        (day <= self.schedule.life.last()).then_some(day)
    }

    fn before_in(&self, dues: &Dues<'_>, day: Day) -> u32 {
        // Days after the contract's last are not owed: count to the day after it.
        let limit = Day(day.0.min(self.schedule.life.last().0.saturating_add(1)));
        if !dues.is_counted() {
            let walk =
                day.0.checked_sub(1).and_then(|last| Days::new(self.schedule.life.first(), Day(last.min(limit.0 - 1))));
            return walk.map_or(0, |window| u32::try_from(self.days(window).count()).unwrap_or(u32::MAX));
        }
        let cadence = dues.before(limit);
        let holes = self.holes();
        let started = holes.partition_point(|hole| hole.days.first() < limit);
        let swallowed = match started.checked_sub(1).map(|last| &holes[last]) {
            None => 0,
            Some(hole) if hole.days.last() < limit => hole.before + hole.gone,
            Some(hole) => hole.before + (cadence - hole.from),
        };
        cadence - swallowed
    }
}

impl Promises {
    /// Which due day a line dated `day` keeps, as the old `nearest_occurrence` says it: the nearest owed day of the
    /// regular and of the standing schedule, within the contract's reach, and the nearer of the two.
    pub fn keep(&self, contract: Id<crate::book::Contract>, day: Day) -> Keep {
        let promise = self.of(contract);
        if !promise.life.contains(day) {
            return Keep::Outside;
        }
        let near = |kind| self.schedule(contract, kind).and_then(|schedule| schedule.nearest(day, promise.reach));
        match (near(ScheduleKind::Regular), near(ScheduleKind::Standing)) {
            (Some(regular), Some(standing)) if regular.apart == standing.apart => {
                Keep::Ambiguous { regular: regular.due, standing: standing.due }
            }
            (Some(regular), Some(standing)) if regular.apart < standing.apart => kept(ScheduleKind::Regular, regular),
            (Some(_), Some(standing)) => kept(ScheduleKind::Standing, standing),
            (Some(regular), None) => kept(ScheduleKind::Regular, regular),
            (None, Some(standing)) => kept(ScheduleKind::Standing, standing),
            (None, None) => Keep::Outside,
        }
    }
}

fn kept(schedule: ScheduleKind, nearest: Nearest) -> Keep {
    Keep::Kept { schedule, due: nearest.due }
}
