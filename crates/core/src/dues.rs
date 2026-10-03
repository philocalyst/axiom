//! The days a schedule falls due, as an ordered set that can be counted and indexed.
//!
//! [`due`] lists the due days of a window by walking the steps of a cadence. A promise asks the same days other things:
//! which is the ninth, how many are before this one, which is nearest this day. A walk answers those in O(n), and n counts
//! from the first due day: a contract that never said when it began counts from [`Day::MIN`], about 71 million months.
//! [`Dues`] answers them by arithmetic, in O(log n), whatever the contract's age.
//!
//! # The set
//!
//! Step `b` of a schedule is `anchor + b·every` (months first, clamped to the end of the month, counted from the anchor and
//! never from the previous step, then days). It lands on each day `on` names in the month, year or week it falls in, or on
//! itself when `on` names none. The due days are every day any step lands on, from the anchor on, **once**: [`due`] yields
//! a day once for each step that lands on it, so `weekly on 15` lists the 15th four or five times a month. The set has no
//! such repeats, and an index in it is what an occurrence's ordinal means.
//!
//! # Why the layout is the one it is
//!
//! A `Dues` is a value made from three things a contract already has (its cadence, its `on` and its anchor) and borrows the
//! `on`. It stores only what it worked out once: which of three shapes the schedule has.
//!
//! * **Never**: a cadence of no days.
//! * **Tiled**: the days of step `b` (its *block*) are `per_step` of them, all before the first day of step `b + 1`. Then
//!   day number `s` of the set, counting the days before the anchor that the first block has (`head`), is block `s / per_step`
//!   and day `s % per_step` of it, and the number of days before `x` is the number of blocks before the first that reaches
//!   `x`, times `per_step`, and the days of that block before it. A block is a handful of landings, worked out, sorted and
//!   compared: a binary search over `b` with an O(k) probe. This is every schedule that means something: `monthly on 1, 15`,
//!   `every 2w on monday`, `quarterly on last`, `yearly on 04-15, 06-15, 09-15, 01-15`, a cadence with no `on`.
//! * **Walked**: what is left: a longer `on` than the cadence steps by (`weekly on 15`), two days that fall on one in a short
//!   month (`on 30, last`), days of two kinds (`on 1, monday`), more than [`MAX_LANDINGS`]. They are the set a walk finds, with
//!   the repeats taken out, and they cost O(n). Nothing in the language gives them a meaning; they are here so that the answer
//!   is defined for every schedule the parser takes.

use crate::calendar::{Cadence, Days, On, cadence_day, due};
use crate::day::{Day, Span};

/// The most days of one period a schedule may name and still be counted by arithmetic.
pub const MAX_LANDINGS: usize = 16;

/// The days a schedule falls due: see the module.
#[derive(Clone, Copy, Debug)]
pub struct Dues<'a> {
    every: Cadence,
    on: &'a [On],
    anchor: Day,
    shape: Shape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    Never,
    Tiled { step: Span, per_step: u8, head: u8 },
    Walked,
}

/// The kind of period a day of `on` is a day of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Within {
    Month,
    Year,
    Week,
}

impl On {
    fn within(self) -> Within {
        match self {
            On::MonthDay(_) | On::Last => Within::Month,
            On::YearDay { .. } => Within::Year,
            On::Weekday(_) => Within::Week,
        }
    }

    /// Whether the day exists in some month: the parser makes no other, a caller of the library might.
    fn exists(self) -> bool {
        match self {
            On::MonthDay(day) => (1..=31).contains(&day),
            On::YearDay { month, day } => (1..=12).contains(&month) && (1..=31).contains(&day),
            On::Weekday(weekday) => weekday < 7,
            On::Last => true,
        }
    }

    /// Where it sorts among the days of one period, and what the shortest period it is a day of has to give it.
    fn rank(self) -> (u8, u8, u8) {
        match self {
            On::MonthDay(day) => (0, day, 0),
            On::Last => (0, u8::MAX, 0),
            On::YearDay { month, day } => (1, month, day),
            On::Weekday(weekday) => (2, weekday, 0),
        }
    }
}

/// The shortest month a month of the year can be.
fn shortest(month: u8) -> u8 {
    match month {
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The days of one step as day numbers, which may lie beyond what a [`Day`] counts: ascending, without repeats.
#[derive(Clone, Copy)]
struct Block {
    days: [i64; MAX_LANDINGS],
    len: usize,
}

impl Block {
    fn as_slice(&self) -> &[i64] {
        &self.days[..self.len]
    }
}

impl<'a> Dues<'a> {
    /// The days `every` and `on` give from `anchor`: none before it.
    pub fn new(every: Cadence, on: &'a [On], anchor: Day) -> Dues<'a> {
        let step = match every {
            Cadence::Every(span) => span,
            Cadence::TwiceMonthly => Span::months(1),
        };
        let advances = step.months >= 0 && step.days >= 0 && step > Span::default();
        let shape = match (advances, tiling(step, on)) {
            (false, _) => Shape::Never,
            (true, None) => Shape::Walked,
            (true, Some(per_step)) => {
                let first = block(anchor, step, on, 0);
                let head = first.as_slice().iter().take_while(|&&day| day < i64::from(anchor.0)).count();
                Shape::Tiled { step, per_step, head: head as u8 }
            }
        };
        Dues { every, on, anchor, shape }
    }

    /// Whether the days are found by arithmetic: what [`nth`](Dues::nth) and [`before`](Dues::before) cost, O(log n)
    /// or, for a schedule that is only walked, O(n).
    pub fn is_counted(&self) -> bool {
        self.shape != Shape::Walked
    }

    /// The `n`th due day, counting from 0: none past the end of the calendar.
    pub fn nth(&self, n: u32) -> Option<Day> {
        match self.shape {
            Shape::Never => None,
            Shape::Tiled { step, per_step, head } => {
                let slot = u64::from(n) + u64::from(head);
                let block = block(self.anchor, step, self.on, slot / u64::from(per_step));
                let day = *block.as_slice().get((slot % u64::from(per_step)) as usize)?;
                i32::try_from(day).ok().map(Day)
            }
            Shape::Walked => self.walked_nth(n),
        }
    }

    /// How many due days are before `day`: the index of the first on or after it.
    pub fn before(&self, day: Day) -> u32 {
        match self.shape {
            Shape::Never => 0,
            Shape::Tiled { step, per_step, head } => {
                let x = i64::from(day.0);
                let ends_before =
                    |n: u64| block(self.anchor, step, self.on, n).as_slice().last().is_some_and(|&last| last < x);
                let at = first_where(|n| !ends_before(n));
                let below =
                    block(self.anchor, step, self.on, at).as_slice().iter().take_while(|&&landing| landing < x).count()
                        as u64;
                let slots = (at * u64::from(per_step) + below).saturating_sub(u64::from(head));
                u32::try_from(slots).unwrap_or(u32::MAX)
            }
            Shape::Walked => self.walked_before(day),
        }
    }

    /// The due days in `within`, in order.
    pub fn days(&self, within: Days) -> Window<'a> {
        match self.shape {
            Shape::Walked => Window::Walked {
                dues: *self,
                from: Some(within.first()),
                last: within.last(),
                reach: 64,
                found: Vec::new().into_iter(),
            },
            _ => {
                let first = self.before(within.first());
                Window::Counted { dues: *self, next: Some(first), last: within.last() }
            }
        }
    }

    /// What a walk finds in `within`: ascending, without repeats.
    fn walked_in(&self, within: Days) -> Vec<Day> {
        let early = Day(within.first().0.saturating_sub(35).max(self.anchor.0));
        let Some(wide) = Days::new(early.min(within.first()), within.last()) else { return Vec::new() };
        let mut days: Vec<Day> =
            due(self.every, self.on, self.anchor, wide).filter(|day| within.contains(*day)).collect();
        days.sort_unstable();
        days.dedup();
        days
    }

    /// What a walk finds in `[anchor, last]`: ascending, without repeats.
    fn walked(&self, last: Day) -> Vec<Day> {
        let Some(window) = Days::new(self.anchor, last) else { return Vec::new() };
        let mut days: Vec<Day> = due(self.every, self.on, self.anchor, window).collect();
        days.sort_unstable();
        days.dedup();
        days
    }

    fn walked_before(&self, day: Day) -> u32 {
        let Some(last) = day.0.checked_sub(1) else { return 0 };
        u32::try_from(self.walked(Day(last)).len()).unwrap_or(u32::MAX)
    }

    fn walked_nth(&self, n: u32) -> Option<Day> {
        let mut reach = 64i64;
        loop {
            let last = Day(i32::try_from(i64::from(self.anchor.0) + reach).unwrap_or(i32::MAX));
            let days = self.walked(last);
            if let Some(&day) = days.get(n as usize) {
                return Some(day);
            }
            if last == Day::MAX {
                return None;
            }
            reach *= 4;
        }
    }
}

/// The days of step `n` of a schedule, ascending: none if the step is past the calendar.
fn block(anchor: Day, step: Span, on: &[On], n: u64) -> Block {
    let mut block = Block { days: [0; MAX_LANDINGS], len: 0 };
    let Some(base) = cadence_day(anchor, step, n) else { return block };
    if on.is_empty() {
        block.days[0] = i64::from(base.0);
        block.len = 1;
        return block;
    }
    let civil = base.ymd();
    for on in on {
        // A day that does not exist lands nowhere: `tiling` has sent every such schedule to a walk.
        block.days[block.len] = on.land_number(base, civil).unwrap_or(i64::MAX);
        block.len += 1;
    }
    block.days[..block.len].sort_unstable();
    // Equal days are one: `on 15, 15`.
    let mut kept = 0;
    for at in 0..block.len {
        if kept == 0 || block.days[kept - 1] != block.days[at] {
            block.days[kept] = block.days[at];
            kept += 1;
        }
    }
    block.len = kept;
    block
}

/// The days of a window, as [`Dues::days`] gives them: counted one at a time, or, for a walked schedule, walked a stretch
/// at a time, each twice the last, so that asking for the first few of a window that runs to the end of the calendar walks
/// a few days and not four billion.
pub enum Window<'a> {
    Counted {
        dues: Dues<'a>,
        next: Option<u32>,
        last: Day,
    },
    Walked {
        dues: Dues<'a>,
        /// Where the next stretch begins: none when the window is walked to its end.
        from: Option<Day>,
        last: Day,
        /// How many days the next stretch is.
        reach: i64,
        found: std::vec::IntoIter<Day>,
    },
}

impl Iterator for Window<'_> {
    type Item = Day;

    fn next(&mut self) -> Option<Day> {
        match self {
            Window::Walked { dues, from, last, reach, found } => loop {
                if let Some(day) = found.next() {
                    return Some(day);
                }
                let first = (*from)?;
                let end = Day(i32::try_from(i64::from(first.0) + *reach - 1).unwrap_or(i32::MAX).min(last.0));
                *found = dues.walked_in(Days::new(first, end)?).into_iter();
                *from = end.0.checked_add(1).filter(|_| end < *last).map(Day);
                *reach = reach.saturating_mul(2);
            },
            Window::Counted { dues, next, last } => {
                let index = (*next)?;
                let day = dues.nth(index).filter(|day| day <= last)?;
                *next = index.checked_add(1);
                Some(day)
            }
        }
    }
}

/// How many days each step has, if every step's days are before the next step's and no two of a step's fall on one day;
/// none if either might not hold.
fn tiling(step: Span, on: &[On]) -> Option<u8> {
    if on.is_empty() {
        return Some(1);
    }
    if on.len() > MAX_LANDINGS || !on.iter().all(|on| on.exists()) {
        return None;
    }
    let mut named = [On::Last; MAX_LANDINGS];
    named[..on.len()].copy_from_slice(on);
    named[..on.len()].sort_unstable_by_key(|on| on.rank());
    let mut count = 0;
    for at in 0..on.len() {
        if count == 0 || named[count - 1] != named[at] {
            named[count] = named[at];
            count += 1;
        }
    }
    let distinct = &named[..count];
    let within = distinct[0].within();
    let apart = match within {
        Within::Month => (step.days == 0 && step.months >= 1) || (step.months == 0 && step.days >= 31),
        Within::Year => (step.days == 0 && step.months >= 12) || (step.months == 0 && step.days >= 366),
        Within::Week => step.months >= 1 || step.days >= 7,
    };
    let alike = distinct.iter().all(|on| on.within() == within);
    (alike && apart && !fall_together(distinct)).then_some(count as u8)
}

/// Whether two of the days, all of one kind and distinct, can land on one day in some month: both are at least as late as
/// the month is short.
fn fall_together(distinct: &[On]) -> bool {
    let late = |on: &&On| match on {
        On::MonthDay(day) => *day >= 28,
        On::Last => true,
        On::YearDay { month, day } => *day >= shortest(*month),
        On::Weekday(_) => false,
    };
    match distinct.first() {
        Some(On::YearDay { .. }) => distinct
            .chunk_by(|a, b| matches!((a, b), (On::YearDay { month: x, .. }, On::YearDay { month: y, .. }) if x == y))
            .any(|month| month.iter().filter(late).count() > 1),
        _ => distinct.iter().filter(late).count() > 1,
    }
}

/// The first `n` for which `holds` is, if it is false before it, and true from it on: found by doubling and then halving.
fn first_where(holds: impl Fn(u64) -> bool) -> u64 {
    if holds(0) {
        return 0;
    }
    let (mut low, mut high) = (0u64, 1u64);
    while !holds(high) {
        low = high;
        high *= 2;
    }
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if holds(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, date: u32) -> Day {
        Day::from_ymd(year, month, date).unwrap()
    }

    fn monthly() -> Cadence {
        Cadence::Every(Span::months(1))
    }

    fn every(span: Span) -> Cadence {
        Cadence::Every(span)
    }

    /// The set a walk finds in `within`: `due`'s days in order, each once. The walk starts a month early, because `due`
    /// starts at the first step on or after its window and a day that is the last of a month (`on last`) can fall after
    /// the window begins and belong to a step before it: asked from the middle of a month, `due` loses it.
    fn walk(every: Cadence, on: &[On], anchor: Day, within: Days) -> Vec<Day> {
        let early = Day(within.first().0.saturating_sub(35).max(anchor.0));
        let wide = Days::new(early.min(within.first()), within.last()).unwrap();
        let mut days: Vec<Day> = due(every, on, anchor, wide).filter(|day| within.contains(*day)).collect();
        days.sort_unstable();
        days.dedup();
        days
    }

    fn shape(every: Cadence, on: &[On], anchor: Day) -> Shape {
        Dues::new(every, on, anchor).shape
    }

    /// Every schedule the language means something by, and what each is: the cadences of LANGUAGE §7 with the days
    /// it lets `on` name.
    #[allow(clippy::type_complexity)]
    fn meaningful() -> Vec<(Cadence, Vec<On>)> {
        let month_days = |days: &[u8]| days.iter().map(|&d| On::MonthDay(d)).collect::<Vec<_>>();
        vec![
            (every(Span::days(1)), vec![]),
            (every(Span::days(7)), vec![]),
            (every(Span::days(7)), vec![On::Weekday(0)]),
            (every(Span::days(7)), vec![On::Weekday(1), On::Weekday(4)]),
            (every(Span::days(14)), vec![On::Weekday(6)]),
            (every(Span::days(10)), vec![On::Weekday(0)]),
            (every(Span::days(45)), vec![]),
            (every(Span::days(45)), month_days(&[15])),
            (every(Span::days(400)), vec![On::YearDay { month: 4, day: 15 }]),
            (monthly(), vec![]),
            (monthly(), month_days(&[1])),
            (monthly(), month_days(&[31])),
            (monthly(), month_days(&[1, 15])),
            (monthly(), vec![On::MonthDay(15), On::Last]),
            (monthly(), vec![On::MonthDay(1), On::MonthDay(15), On::Last]),
            (monthly(), vec![On::Last]),
            (monthly(), vec![On::Weekday(4)]),
            (every(Span::months(3)), vec![On::Last]),
            (every(Span::months(3)), vec![]),
            (every(Span::months(2)), month_days(&[29])),
            (every(Span::months(12)), vec![]),
            (every(Span::months(12)), vec![On::YearDay { month: 2, day: 29 }]),
            (
                every(Span::months(12)),
                vec![
                    On::YearDay { month: 4, day: 15 },
                    On::YearDay { month: 6, day: 15 },
                    On::YearDay { month: 9, day: 15 },
                    On::YearDay { month: 1, day: 15 },
                ],
            ),
            (every(Span::months(18)), vec![On::YearDay { month: 4, day: 15 }]),
            (every(Span::months(60)), vec![On::YearDay { month: 2, day: 29 }]),
            (Cadence::TwiceMonthly, vec![On::MonthDay(1), On::MonthDay(15)]),
            (Cadence::TwiceMonthly, vec![On::MonthDay(15), On::Last]),
            (Cadence::TwiceMonthly, vec![]),
            (every(Span { months: 1, days: 15 }), vec![]),
            (every(Span { months: 1, days: 15 }), vec![On::Weekday(0)]),
        ]
    }

    /// What the language lets be written and gives no meaning: a longer `on` than the cadence steps by, two days that
    /// fall on one in a short month, days of two kinds.
    fn meaningless() -> Vec<(Cadence, Vec<On>)> {
        vec![
            (every(Span::days(7)), vec![On::MonthDay(15)]),
            (every(Span::days(7)), vec![On::Last]),
            (every(Span::days(1)), vec![On::Weekday(0)]),
            (every(Span::days(3)), vec![On::Weekday(0)]),
            (every(Span::days(28)), vec![On::MonthDay(15)]),
            (every(Span::days(30)), vec![On::Last]),
            (monthly(), vec![On::YearDay { month: 4, day: 15 }]),
            (every(Span::months(3)), vec![On::YearDay { month: 4, day: 15 }]),
            (monthly(), vec![On::MonthDay(30), On::Last]),
            (monthly(), vec![On::MonthDay(29), On::MonthDay(30), On::MonthDay(31)]),
            (monthly(), vec![On::MonthDay(28), On::Last]),
            (Cadence::TwiceMonthly, vec![On::MonthDay(30), On::Last]),
            (every(Span::months(12)), vec![On::YearDay { month: 2, day: 28 }, On::YearDay { month: 2, day: 29 }]),
            (monthly(), vec![On::MonthDay(1), On::Weekday(0)]),
            (every(Span::months(12)), vec![On::YearDay { month: 4, day: 15 }, On::MonthDay(15)]),
            (every(Span { months: 1, days: 15 }), vec![On::MonthDay(1)]),
            (every(Span::months(1)), vec![On::MonthDay(0)]),
        ]
    }

    fn anchors() -> Vec<Day> {
        vec![
            day(2026, 1, 1),
            day(2026, 1, 15),
            day(2026, 1, 30),
            day(2026, 1, 31),
            day(2024, 2, 29),
            day(2025, 12, 31),
            day(2020, 3, 8),
            Day(0),
            Day(-40_000),
            day(1999, 12, 31),
        ]
    }

    /// `Dues` says the set a walk finds: its days in a window, its `nth`, and its count before a day.
    fn agrees_with_a_walk(every: Cadence, on: &[On], anchor: Day) {
        let dues = Dues::new(every, on, anchor);
        let window = Days::new(Day(anchor.0 - 400), Day(anchor.0 + 3_000)).unwrap();
        let walked = walk(every, on, anchor, window);
        let case = format!("{every:?} on {on:?} from {anchor}");
        assert_eq!(dues.days(window).collect::<Vec<_>>(), walked, "the days of {case}");
        let limit = if dues.shape == Shape::Walked { 24 } else { 400 };
        for (index, expected) in walked.iter().enumerate().take(limit) {
            assert_eq!(dues.nth(index as u32), Some(*expected), "the {index}th of {case}");
            assert_eq!(dues.before(*expected), index as u32, "the count before {expected} of {case}");
            assert_eq!(dues.before(Day(expected.0 + 1)), index as u32 + 1, "the count through {expected} of {case}");
        }
        for offset in (-400..3_000).step_by(if dues.shape == Shape::Walked { 700 } else { 37 }) {
            let probe = Day(anchor.0 + offset);
            let before = walked.iter().filter(|&&due| due < probe).count() as u32;
            assert_eq!(dues.before(probe), before, "the count before {probe} of {case}");
        }
    }

    #[test]
    fn the_days_of_every_schedule_that_means_something_are_found_by_arithmetic() {
        for (every, on) in meaningful() {
            assert!(
                matches!(shape(every, &on, day(2026, 1, 1)), Shape::Tiled { .. }),
                "{every:?} on {on:?} is not counted by arithmetic"
            );
            for anchor in anchors() {
                agrees_with_a_walk(every, &on, anchor);
            }
        }
    }

    #[test]
    fn the_rest_are_the_set_a_walk_finds_without_its_repeats() {
        for (every, on) in meaningless() {
            if on.iter().all(|on| on.exists()) {
                assert_eq!(shape(every, &on, day(2026, 1, 1)), Shape::Walked, "{every:?} on {on:?}");
            }
            for anchor in [day(2026, 1, 1), day(2026, 1, 31), day(2024, 2, 29)] {
                if on.iter().all(|on| on.exists()) {
                    agrees_with_a_walk(every, &on, anchor);
                }
            }
        }
    }

    #[test]
    fn a_walk_repeats_a_day_that_many_steps_land_on_and_the_set_does_not() {
        let on = [On::MonthDay(15)];
        let weekly = every(Span::days(7));
        let january = Days::new(day(2026, 1, 1), day(2026, 1, 31)).unwrap();
        let walked: Vec<_> = due(weekly, &on, day(2026, 1, 1), january).collect();
        assert_eq!(walked.len(), 5, "the 15th, once for each of the five weeks whose first day is in January");
        let dues = Dues::new(weekly, &on, day(2026, 1, 1));
        assert_eq!(dues.days(january).collect::<Vec<_>>(), [day(2026, 1, 15)]);
    }

    #[test]
    fn nothing_is_due_on_a_cadence_of_no_days() {
        for every in [Cadence::Every(Span::default()), Cadence::Every(Span { months: -1, days: 3 })] {
            let dues = Dues::new(every, &[], day(2026, 1, 1));
            assert_eq!((dues.nth(0), dues.before(day(2030, 1, 1))), (None, 0));
            assert_eq!(dues.days(Days::ALWAYS).count(), 0);
        }
    }

    #[test]
    fn a_schedule_with_no_start_is_counted_not_walked() {
        // From Day::MIN a monthly schedule has about 71 million days due before 2026; a walk takes seconds.
        let dues = Dues::new(monthly(), &[On::MonthDay(1)], Day::MIN);
        let first = dues.before(day(2026, 1, 1));
        assert!(first > 70_000_000, "{first}");
        assert_eq!(dues.nth(first), Some(day(2026, 1, 1)));
        assert_eq!(dues.nth(first + 1), Some(day(2026, 2, 1)));
        assert_eq!(dues.nth(first - 1), Some(day(2025, 12, 1)));
        let window = Days::new(day(2026, 2, 1), day(2026, 6, 30)).unwrap();
        assert_eq!(dues.days(window).count(), 5);
        // The first day there is a step lands on is not before Day::MIN, and the ones before the anchor are not due.
        assert_eq!(dues.nth(0).map(|first| first >= Day::MIN), Some(true));
        assert_eq!(dues.before(Day::MIN), 0);
    }

    #[test]
    fn a_step_whose_first_days_are_before_the_anchor_does_not_count_them() {
        // Anchored on the 20th, `on 1, 15, last` first falls due on the last of the month.
        let on = [On::MonthDay(1), On::MonthDay(15), On::Last];
        let dues = Dues::new(monthly(), &on, day(2026, 1, 20));
        assert_eq!(dues.nth(0), Some(day(2026, 1, 31)));
        assert_eq!(dues.nth(1), Some(day(2026, 2, 1)));
        assert_eq!(dues.before(day(2026, 1, 31)), 0);
        assert_eq!(dues.before(day(2026, 2, 1)), 1);
    }

    #[test]
    fn the_calendar_gives_out_at_both_ends_without_a_panic() {
        let top = Days::new(Day(i32::MAX - 2_000), Day::MAX).unwrap();
        for (every, on) in meaningful().into_iter().chain(meaningless()) {
            if !on.iter().all(|on| on.exists()) {
                continue;
            }
            for anchor in [Day(i32::MAX - 10_000), Day::MIN, Day(i32::MIN + 100)] {
                let dues = Dues::new(every, &on, anchor);
                if dues.shape == Shape::Walked && anchor.0 < 0 {
                    continue; // a walk from Day::MIN is four billion days
                }
                let ends = [Days::new(Day::MIN, Day(i32::MIN + 400)).unwrap(), top];
                for window in ends {
                    let walked = walk(every, &on, anchor, window);
                    assert_eq!(dues.days(window).collect::<Vec<_>>(), walked, "{every:?} on {on:?} from {anchor:?}");
                }
            }
        }
    }

    #[test]
    fn a_random_schedule_is_the_set_a_walk_finds() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut draw = |bound: u64| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % bound
        };
        let pool = [
            On::MonthDay(1),
            On::MonthDay(15),
            On::MonthDay(28),
            On::MonthDay(30),
            On::MonthDay(31),
            On::Last,
            On::YearDay { month: 2, day: 29 },
            On::YearDay { month: 4, day: 15 },
            On::YearDay { month: 12, day: 31 },
            On::Weekday(0),
            On::Weekday(4),
        ];
        for _ in 0..3_000 {
            let step = match draw(4) {
                0 => Span::days(1 + draw(60) as i32),
                1 => Span::months(1 + draw(30) as i32),
                2 => Span { months: draw(3) as i32, days: draw(45) as i32 },
                _ => Span::days(7 * (1 + draw(8) as i32)),
            };
            let every = if draw(8) == 0 { Cadence::TwiceMonthly } else { Cadence::Every(step) };
            let on: Vec<On> = (0..draw(4)).map(|_| pool[draw(pool.len() as u64) as usize]).collect();
            let anchor = Day(-5_000 + draw(30_000) as i32);
            agrees_with_a_walk(every, &on, anchor);
        }
    }

    /// `cargo test -p axiom-core --release what_counting_costs -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn what_counting_costs() {
        use std::time::Instant;
        let on = [On::MonthDay(1), On::MonthDay(15)];
        let cases: [(&str, Cadence, &[On], Day); 4] = [
            ("monthly on 1, 15 from 2026-01-01", monthly(), &on, day(2026, 1, 1)),
            ("monthly on 1, 15 from Day::MIN", monthly(), &on, Day::MIN),
            ("daily from Day::MIN", every(Span::days(1)), &[], Day::MIN),
            ("every 2w on monday from 2020-01-06", every(Span::days(14)), &[On::Weekday(0)], day(2020, 1, 6)),
        ];
        for (name, every, on, anchor) in cases {
            let dues = Dues::new(every, on, anchor);
            let target = day(2036, 6, 30);
            let started = Instant::now();
            let before = (0..2_000).map(|_| dues.before(std::hint::black_box(target))).last().unwrap();
            let before_ns = started.elapsed().as_nanos() / 2_000;
            let started = Instant::now();
            let nth =
                (0..20_000).map(|n| dues.nth(std::hint::black_box(before.saturating_sub(n % 1_000)))).last().unwrap();
            let nth_ns = started.elapsed().as_nanos() / 20_000;
            let started = Instant::now();
            let walked = (anchor.0 >= day(2000, 1, 1).0)
                .then(|| due(every, on, anchor, Days::new(anchor, target).unwrap()).count());
            let walk_us = started.elapsed().as_micros();
            println!(
                "{name:<40} before {before_ns:>6} ns  nth {nth_ns:>5} ns  index of 2036-06-30: {before}  {nth:?}  walk: {walked:?} days in {walk_us} us"
            );
        }
    }

    #[test]
    fn a_dues_is_small() {
        assert!(size_of::<Dues<'_>>() <= 64, "{}", size_of::<Dues<'_>>());
        assert!(size_of::<Shape>() <= 16);
    }
}
