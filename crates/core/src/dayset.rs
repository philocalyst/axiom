//! Finite unions of day intervals: the days a condition holds.
//!
//! A residence, a contract's life, a stretch over the FEIE threshold are each not one range of days but several. A
//! set is its maximal runs: [`Days`] intervals, sorted, disjoint, and not even adjacent, so that every set of days
//! has one spelling and equal sets are equal values. Every constructor restores that form and every operation keeps
//! it.
//!
//! This layout, rather than a bitmap of days or a tree of ranges, because a set here is a few intervals however many
//! days it spans (a year is 365 days and often one interval), and because intervals sit in a flat slice: a kernel keeps
//! many sets in one `Vec<Days>` and refers to each by a [`Run`], as [`Intervals::store`] and [`DaySlice::load`] do.
//! The three set operations are then linear merges of two slices, and a membership test is a binary search.
//!
//! One type serves both forms. [`DaySet`] owns its intervals, for building; [`DaySlice`] borrows them, for reading a
//! run of an arena. Both are [`Intervals`] over their storage and have every method.

use crate::calendar::Days;
use crate::day::Day;
use crate::id::{Id, Run};

/// A set of days, as the intervals of `S`: a `Vec<Days>` or a `&[Days]`.
///
/// The intervals are sorted, disjoint and non-adjacent. The field is private: that form is why two sets with the same
/// days are equal, and only this module may build one.
#[derive(Clone, Copy, Default, Debug)]
pub struct Intervals<S>(S);

/// A set that owns its intervals.
pub type DaySet = Intervals<Vec<Days>>;

/// A set that borrows its intervals, from a [`DaySet`] or from a run of an arena.
pub type DaySlice<'a> = Intervals<&'a [Days]>;

impl FromIterator<Days> for DaySet {
    /// The days in any of `days`, whatever their order, overlap or adjacency.
    fn from_iter<I: IntoIterator<Item = Days>>(days: I) -> DaySet {
        let mut days: Vec<Days> = days.into_iter().collect();
        days.sort_unstable();
        // Sorted by first day, so each interval can only join the one before it, and the joined one grows.
        days.dedup_by(|later, earlier| match earlier.merge(*later) {
            Some(joined) => {
                *earlier = joined;
                true
            }
            None => false,
        });
        Intervals(days)
    }
}

impl<'a> DaySlice<'a> {
    /// The set stored at `run` of `arena`, which must be a run that [`Intervals::store`] wrote.
    pub fn load(arena: &'a [Days], run: Run<Days>) -> DaySlice<'a> {
        let start = run.start().index();
        let intervals = &arena[start..start + run.len() as usize];
        debug_assert!(is_normal(intervals), "a run of the arena is not a set");
        Intervals(intervals)
    }
}

impl<S: AsRef<[Days]>> Intervals<S> {
    /// The maximal runs of days, in order.
    pub fn intervals(&self) -> &[Days] {
        self.0.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.intervals().is_empty()
    }

    /// How many days are in the set. More than a `u32` counts when it holds [`Days::ALWAYS`].
    pub fn len(&self) -> u64 {
        self.intervals().iter().map(|days| days_in(*days)).sum()
    }

    pub fn contains(&self, day: Day) -> bool {
        let intervals = self.intervals();
        let first_not_before = intervals.partition_point(|days| days.last() < day);
        intervals.get(first_not_before).is_some_and(|days| days.contains(day))
    }

    /// The days of the set that are in `window`.
    pub fn within(&self, window: Days) -> DaySet {
        let intervals = self.intervals();
        let from = intervals.partition_point(|days| days.last() < window.first());
        let to = intervals.partition_point(|days| days.first() <= window.last());
        // Each interval in `from..to` meets the window, so clipping loses none; only the first and last get shorter.
        Intervals(intervals[from..to].iter().filter_map(|days| days.intersect(window)).collect())
    }

    /// The days in either set.
    pub fn union(&self, other: &Intervals<impl AsRef<[Days]>>) -> DaySet {
        self.combine(other, |ours, theirs| ours || theirs)
    }

    /// The days in both sets.
    pub fn intersection(&self, other: &Intervals<impl AsRef<[Days]>>) -> DaySet {
        self.combine(other, |ours, theirs| ours && theirs)
    }

    /// The days in this set and not in `other`.
    pub fn difference(&self, other: &Intervals<impl AsRef<[Days]>>) -> DaySet {
        self.combine(other, |ours, theirs| ours && !theirs)
    }

    /// The first day on which the `window_len` days up to and including it hold at least `n` days of the set: the
    /// 330-in-365 and the 183-day tests. `None` if no day does. With `n` of nothing every day does, and the first is
    /// [`Day::MIN`].
    ///
    /// The count of days in the window grows only on a day of the set, so the answer is one. It is the first day `x`
    /// of the set for which the `n` days of the set up to `x` fit in the window: `x` less the day `n - 1` days of the
    /// set before it is under `window_len`. Two cursors, `n - 1` days of the set apart, walk the set together. Their
    /// distance changes only when one of them crosses a gap, once for each interval: a sweep, not a count of days.
    pub fn earliest_reaching(&self, n: u32, window_len: u32) -> Option<Day> {
        if n == 0 {
            return Some(Day::MIN);
        }
        let mut tail = Cursor::at_first_day(self.intervals())?;
        let mut head = tail;
        head.advance(u64::from(n) - 1)?;
        while head.day - tail.day >= i64::from(window_len) {
            // Together they can move this far before either meets a gap; the next step is where the distance changes.
            let steps = head.room().min(tail.room()) + 1;
            head.advance(steps)?;
            tail.advance(steps)?;
        }
        Some(day_at(head.day))
    }

    /// Appends the intervals to `arena` and says where they went, to be read again with [`DaySlice::load`].
    pub fn store(&self, arena: &mut Vec<Days>) -> Run<Days> {
        let start = u32::try_from(arena.len()).expect("fewer than 2^32 intervals");
        arena.extend_from_slice(self.intervals());
        Run::new(Id::new(start), self.intervals().len() as u32)
    }

    /// The days for which `keep(ours, theirs)` holds, `ours` and `theirs` saying whether the day is in this set and in
    /// `other`: one sweep over the edges of both.
    ///
    /// Walking forward, a set flips between out and in at each of its edges. A sweep that takes the next edge of either
    /// always knows whether it is in each set, and so whether `keep` holds; the result flips at the edges where that
    /// changes. `keep` must be false for a day in neither set, or the last interval would never end.
    fn combine(&self, other: &Intervals<impl AsRef<[Days]>>, keep: impl Fn(bool, bool) -> bool) -> DaySet {
        let (mut ours, mut theirs) = (edges(self.intervals()).peekable(), edges(other.intervals()).peekable());
        let (mut in_ours, mut in_theirs, mut opened) = (false, false, None);
        let mut result = Vec::new();
        while let Some(edge) = ours.peek().into_iter().chain(theirs.peek()).min().copied() {
            in_ours ^= ours.next_if_eq(&edge).is_some();
            in_theirs ^= theirs.next_if_eq(&edge).is_some();
            match (keep(in_ours, in_theirs), opened) {
                (true, None) => opened = Some(edge),
                (false, Some(first)) => {
                    result.push(Days::new(day_at(first), day_at(edge - 1)).expect("an interval ends after it begins"));
                    opened = None;
                }
                _ => {}
            }
        }
        Intervals(result)
    }
}

impl<A: AsRef<[Days]>, B: AsRef<[Days]>> PartialEq<Intervals<B>> for Intervals<A> {
    fn eq(&self, other: &Intervals<B>) -> bool {
        self.intervals() == other.intervals()
    }
}

impl<S: AsRef<[Days]>> Eq for Intervals<S> {}

/// Where a set flips between out and in, walking forward: each interval's first day, then the day after its last. The
/// second can be one past [`Day::MAX`], so edges are `i64`. A set's edges strictly increase, because its intervals are
/// not even adjacent.
fn edges(intervals: &[Days]) -> impl Iterator<Item = i64> + '_ {
    intervals.iter().flat_map(|days| [i64::from(days.first().0), i64::from(days.last().0) + 1])
}

fn day_at(edge: i64) -> Day {
    Day(i32::try_from(edge).expect("an edge is a day there is, or the one after the last"))
}

fn days_in(days: Days) -> u64 {
    (i64::from(days.last().0) - i64::from(days.first().0)) as u64 + 1
}

fn is_normal(intervals: &[Days]) -> bool {
    intervals.windows(2).all(|pair| i64::from(pair[0].last().0) + 1 < i64::from(pair[1].first().0))
}

/// A place among a set's days, in order: an interval and a day within it.
#[derive(Clone, Copy)]
struct Cursor<'a> {
    intervals: &'a [Days],
    at: usize,
    day: i64,
}

impl<'a> Cursor<'a> {
    fn at_first_day(intervals: &'a [Days]) -> Option<Cursor<'a>> {
        Some(Cursor { intervals, at: 0, day: i64::from(intervals.first()?.first().0) })
    }

    /// How many days of the set follow this one in its interval.
    fn room(&self) -> u64 {
        (i64::from(self.intervals[self.at].last().0) - self.day) as u64
    }

    /// Moves `by` days of the set on, over the gaps between intervals. `None` if the set ends first.
    fn advance(&mut self, mut by: u64) -> Option<()> {
        while by > self.room() {
            by -= self.room() + 1;
            self.at += 1;
            self.day = i64::from(self.intervals.get(self.at)?.first().0);
        }
        self.day += by as i64;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Rng;

    fn days(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn set(intervals: &[(i32, i32)]) -> DaySet {
        intervals.iter().map(|&(first, last)| days(first, last)).collect()
    }

    fn spelled(set: &DaySet) -> Vec<(i32, i32)> {
        set.intervals().iter().map(|days| (days.first().0, days.last().0)).collect()
    }

    #[test]
    fn a_set_is_built_in_its_one_normal_form() {
        let scrambled = set(&[(20, 25), (1, 3), (4, 6), (5, 9), (30, 30), (22, 24), (11, 12)]);
        assert_eq!(spelled(&scrambled), [(1, 9), (11, 12), (20, 25), (30, 30)]);
        let again = set(&[(30, 30), (11, 12), (1, 9), (20, 22), (23, 25)]);
        assert_eq!(scrambled, again, "the same days are the same value, however they were written");
        assert_eq!(DaySet::from_iter([]), DaySet::default());
        assert_eq!(scrambled, Intervals(scrambled.intervals()), "an owned set equals a borrowed one");
    }

    #[test]
    fn the_operations_on_small_sets() {
        let (a, b) = (set(&[(1, 10), (20, 30)]), set(&[(5, 22), (40, 41)]));
        assert_eq!(spelled(&a.union(&b)), [(1, 30), (40, 41)]);
        assert_eq!(spelled(&a.intersection(&b)), [(5, 10), (20, 22)]);
        assert_eq!(spelled(&a.difference(&b)), [(1, 4), (23, 30)]);
        assert_eq!(spelled(&b.difference(&a)), [(11, 19), (40, 41)]);
        assert_eq!(spelled(&a.union(&set(&[(11, 19)]))), [(1, 30)], "a set that fills the gap joins the two");
        assert!(a.intersection(&set(&[(11, 19)])).is_empty());
        assert_eq!((a.len(), b.len()), (21, 20));
        assert_eq!(
            [0, 1, 10, 11, 19, 20, 30, 31].map(|day| a.contains(Day(day))),
            [false, true, true, false, false, true, true, false]
        );
    }

    #[test]
    fn the_ends_of_time_are_days_like_any_other() {
        let always = DaySet::from_iter([Days::ALWAYS]);
        assert_eq!(always.len(), 1 << 32);
        assert!(always.contains(Day::MIN) && always.contains(Day::MAX));
        let hole = always.difference(&set(&[(-1, 1)]));
        assert_eq!(spelled(&hole), [(i32::MIN, -2), (2, i32::MAX)]);
        assert_eq!(hole.union(&set(&[(-1, 1)])), always);
        let near_the_end = set(&[(i32::MAX - 3, i32::MAX - 2), (i32::MAX - 1, i32::MAX)]);
        assert_eq!(spelled(&near_the_end), [(i32::MAX - 3, i32::MAX)]);
        assert_eq!(near_the_end.difference(&set(&[(i32::MAX, i32::MAX)])).len(), 3);
        assert_eq!(always.within(days(5, 9)), set(&[(5, 9)]));
    }

    #[test]
    fn a_window_is_clipped_at_both_ends() {
        let s = set(&[(1, 10), (20, 30), (40, 50)]);
        assert_eq!(spelled(&s.within(days(5, 25))), [(5, 10), (20, 25)]);
        assert_eq!(spelled(&s.within(days(11, 19))), []);
        assert_eq!(spelled(&s.within(days(30, 40))), [(30, 30), (40, 40)]);
        assert_eq!(spelled(&s.within(days(-9, 100))), [(1, 10), (20, 30), (40, 50)]);
    }

    #[test]
    fn the_first_day_a_window_holds_enough() {
        // The 183-day rule over a year: three stretches of 100, 50 and 60 days, 20 days apart.
        let stays = set(&[(0, 99), (120, 169), (190, 249)]);
        assert_eq!(stays.earliest_reaching(183, 365), Some(Day(222)), "the 183rd day of the set is day 222");
        assert_eq!(stays.earliest_reaching(183, 223), Some(Day(222)), "and it takes a window of 223 days to hold them");
        assert_eq!(stays.earliest_reaching(183, 222), None, "no window of 222 days ever holds 183 of them");
        assert_eq!(stays.earliest_reaching(1, 1), Some(Day(0)));
        assert_eq!(stays.earliest_reaching(100, 100), Some(Day(99)));
        assert_eq!(stays.earliest_reaching(0, 0), Some(Day::MIN));
        assert_eq!(stays.earliest_reaching(1, 0), None, "a window of no days holds none");
        assert_eq!(stays.earliest_reaching(211, 365), None, "more days than the set has");
        assert_eq!(DaySet::default().earliest_reaching(1, 365), None);
    }

    #[test]
    fn sets_live_in_an_arena_and_are_read_from_it() {
        let (a, b) = (set(&[(1, 5), (9, 9)]), set(&[(-3, 0)]));
        let mut arena = Vec::new();
        let (in_arena_a, nothing, in_arena_b) =
            (a.store(&mut arena), DaySet::default().store(&mut arena), b.store(&mut arena));
        assert_eq!(arena.len(), 3);
        assert_eq!(DaySlice::load(&arena, in_arena_a), a);
        assert!(DaySlice::load(&arena, nothing).is_empty());
        let loaded = DaySlice::load(&arena, in_arena_b);
        assert_eq!(loaded.union(&DaySlice::load(&arena, in_arena_a)), a.union(&b));
    }

    /// The days from `origin`, one flag each: the model every operation is checked against.
    const REACH: usize = 3 * 366;

    fn paint(origin: Day, intervals: &[Days]) -> Vec<bool> {
        let mut bitmap = vec![false; REACH];
        for days in intervals {
            let (from, to) =
                ((days.first().0 as i64 - origin.0 as i64) as usize, (days.last().0 as i64 - origin.0 as i64) as usize);
            bitmap[from..=to].fill(true);
        }
        bitmap
    }

    /// The maximal runs of a bitmap, found the slow way.
    fn runs(origin: Day, bitmap: &[bool]) -> Vec<Days> {
        let mut runs: Vec<Days> = Vec::new();
        for at in (0..bitmap.len()).filter(|&at| bitmap[at]) {
            let day = origin.add_days(at as i32);
            match runs.last_mut() {
                Some(run) if run.last().add_days(1) == day => *run = Days::new(run.first(), day).unwrap(),
                _ => runs.push(Days::on(day)),
            }
        }
        runs
    }

    fn random_intervals(rng: &mut Rng, origin: Day) -> Vec<Days> {
        (0..rng.below(8))
            .map(|_| {
                let first = rng.below(REACH);
                let length = if rng.below(10) == 0 { rng.below(300) } else { rng.below(20) };
                let last = (first + length).min(REACH - 1);
                Days::new(origin.add_days(first as i32), origin.add_days(last as i32)).unwrap()
            })
            .collect()
    }

    /// The window's days counted by hand: `at` and the `window_len - 1` before it.
    fn earliest_reaching_by_counting(bitmap: &[bool], origin: Day, n: usize, window_len: usize) -> Option<Day> {
        if n == 0 {
            return Some(Day::MIN);
        }
        let held = |at: usize| bitmap[(at + 1).saturating_sub(window_len)..=at].iter().filter(|&&set| set).count();
        (0..REACH).find(|&at| held(at) >= n).map(|at| origin.add_days(at as i32))
    }

    fn check_against_bitmaps(origin: Day, rng: &mut Rng, case: usize) {
        let (raw_a, raw_b) = (random_intervals(rng, origin), random_intervals(rng, origin));
        let (a, b): (DaySet, DaySet) = (raw_a.iter().copied().collect(), raw_b.iter().copied().collect());
        let (bits_a, bits_b) = (paint(origin, &raw_a), paint(origin, &raw_b));
        let both = |keep: fn(bool, bool) -> bool| -> Vec<bool> {
            bits_a.iter().zip(&bits_b).map(|(&x, &y)| keep(x, y)).collect()
        };

        let expected = [
            (a.union(&b), both(|x, y| x || y)),
            (a.intersection(&b), both(|x, y| x && y)),
            (a.difference(&b), both(|x, y| x && !y)),
            (a.clone(), bits_a.clone()),
        ];
        for (got, bits) in &expected {
            assert_eq!(got.intervals(), runs(origin, bits), "case {case}: the intervals are the maximal runs");
            assert_eq!(got.len(), bits.iter().filter(|&&set| set).count() as u64, "case {case}");
        }
        for at in -3..REACH as i64 + 3 {
            let Ok(day) = i32::try_from(i64::from(origin.0) + at) else { continue };
            let held = usize::try_from(at).ok().and_then(|at| bits_a.get(at)).copied().unwrap_or(false);
            assert_eq!(a.contains(Day(day)), held, "case {case}: day {at}");
        }

        let window_first = rng.below(REACH);
        let window = Days::new(
            origin.add_days(window_first as i32),
            origin.add_days((window_first + rng.below(400)).min(REACH - 1) as i32),
        )
        .unwrap();
        let clipped: Vec<bool> =
            (0..REACH).map(|at| bits_a[at] && window.contains(origin.add_days(at as i32))).collect();
        assert_eq!(a.within(window).intervals(), runs(origin, &clipped), "case {case}");

        let (n, window_len) = (rng.below(400), rng.below(400));
        assert_eq!(
            a.earliest_reaching(n as u32, window_len as u32),
            earliest_reaching_by_counting(&bits_a, origin, n, window_len),
            "case {case}: {n} days in {window_len}"
        );
    }

    #[test]
    fn every_operation_agrees_with_a_bitmap_of_days() {
        let mut rng = Rng::new(0x2545_F491_4F6C_DD1D);
        // In the middle of the calendar, and with the sets pressed against each end of the days there are.
        for origin in [Day::from_ymd(2026, 1, 1).unwrap(), Day(i32::MIN), Day(i32::MAX - REACH as i32 + 1)] {
            for case in 0..300 {
                check_against_bitmaps(origin, &mut rng, case);
            }
        }
    }

    #[test]
    fn many_sets_share_one_arena() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        let origin = Day(0);
        let sets: Vec<DaySet> = (0..50).map(|_| random_intervals(&mut rng, origin).into_iter().collect()).collect();
        let mut arena = Vec::new();
        let runs: Vec<Run<Days>> = sets.iter().map(|set| set.store(&mut arena)).collect();
        for (set, run) in sets.iter().zip(runs) {
            assert_eq!(&DaySlice::load(&arena, run), set);
        }
        assert_eq!(arena.len(), sets.iter().map(|set| set.intervals().len()).sum::<usize>());
    }
}
