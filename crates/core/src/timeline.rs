//! A value that changes on days.
//!
//! A statement holds from its day (LANGUAGE §3): a contract's terms, a
//! budget's limit and a person's residence are each a declaration's value
//! and then whatever later statements said, over the days they said it.
//!
//! The painting is a function of its own, `paint_steps`, on a plain vector of `(Day, T)`, so that [`Timeline`] and
//! the facts builder, which paints thousands of small timelines into one reused vector, run the same code.

use crate::calendar::Days;
use crate::day::Day;

/// A value over all time: a declaration's, then each statement's from its day.
/// Built by painting, in the order the statements are written, so a later
/// statement overrides an earlier one for the days it covers; read by binary
/// search.
#[derive(Clone, Debug)]
pub struct Timeline<T> {
    /// Sorted by day; the first step starts at `Day::MIN`; never empty; no two
    /// neighbours hold the same value.
    steps: Vec<(Day, T)>,
}

impl<T: Clone + PartialEq> Timeline<T> {
    /// `value`, always.
    pub fn new(value: T) -> Timeline<T> {
        Timeline { steps: vec![(Day::MIN, value)] }
    }

    /// `value` holds over `days`; outside them what held before stands.
    /// Painting an unbounded end (`Days::new(day, Day::MAX)`) is "from now on".
    /// Adjacent equal steps merge.
    pub fn paint(&mut self, days: Days, value: T) {
        paint_steps(&mut self.steps, days, value);
    }

    /// What holds on `day`.
    pub fn at(&self, day: Day) -> &T {
        step_at(&self.steps, day)
    }

    /// Every stretch that meets `within`, in order, with what holds through
    /// it. The stretches are whole: the first may begin before `within` and the
    /// last end after it.
    pub fn within(&self, within: Days) -> impl Iterator<Item = (Days, &T)> {
        let ends = self.steps[1..].iter().map(|(next, _)| next.add_days(-1)).chain([Day::MAX]);
        let stretches =
            self.steps.iter().zip(ends).filter_map(|((first, value), last)| Some((Days::new(*first, last)?, value)));
        stretches
            .skip_while(move |(days, _)| days.last() < within.first())
            .take_while(move |(days, _)| days.first() <= within.last())
    }

    /// The steps after the first: what changed, and from when.
    pub fn changes(&self) -> impl Iterator<Item = (Day, &T)> {
        self.steps[1..].iter().map(|(day, value)| (*day, value))
    }
}

/// What holds on `day` in `steps`: the last step that has begun. `steps` is sorted by day and begins at [`Day::MIN`],
/// so there always is one.
pub(crate) fn step_at<T>(steps: &[(Day, T)], day: Day) -> &T {
    let after = steps.partition_point(|(from, _)| *from <= day);
    &steps[after - 1].1
}

/// Paints `value` over `days` in `steps`, which are sorted by day, begin at [`Day::MIN`] and have no two neighbours
/// that hold the same value, and leaves them so: a statement overrides what was there over its days, what held before
/// resumes after it, and neighbours that come to hold the same value merge.
pub(crate) fn paint_steps<T: Clone + PartialEq>(steps: &mut Vec<(Day, T)>, days: Days, value: T) {
    // What held on the day after: it resumes there.
    let resumed = days.last().0.checked_add(1).map(|next| (Day(next), step_at(steps, Day(next)).clone()));
    let from = steps.partition_point(|(day, _)| *day < days.first());
    let to = steps.partition_point(|(day, _)| *day <= resumed.as_ref().map_or(Day::MAX, |(next, _)| *next));
    steps.splice(from..to, [(days.first(), value)].into_iter().chain(resumed));
    steps.dedup_by(|later, earlier| later.1 == earlier.1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(month: u32, date: u32) -> Day {
        Day::from_ymd(2026, month, date).unwrap()
    }

    fn days(from: (u32, u32), to: (u32, u32)) -> Days {
        Days::new(day(from.0, from.1), day(to.0, to.1)).unwrap()
    }

    /// The timeline as its steps, `(from, value)`, the first from `Day::MIN`.
    fn steps(timeline: &Timeline<char>) -> Vec<(String, char)> {
        let first = ("start".to_string(), *timeline.at(Day::MIN));
        let rest = timeline.changes().map(|(from, value)| (from.to_string()[5..].to_string(), *value));
        std::iter::once(first).chain(rest).collect()
    }

    fn painted(paints: &[(Days, char)]) -> Timeline<char> {
        let mut timeline = Timeline::new('a');
        for &(days, value) in paints {
            timeline.paint(days, value);
        }
        timeline
    }

    fn read(timeline: &Timeline<char>, days: &[(u32, u32)]) -> String {
        days.iter().map(|&(month, date)| *timeline.at(day(month, date))).collect()
    }

    #[test]
    fn a_new_timeline_holds_its_value_through_all_time() {
        let timeline = Timeline::new('a');
        assert_eq!([Day::MIN, day(6, 1), Day::MAX].map(|at| *timeline.at(at)), ['a'; 3]);
        assert_eq!(timeline.changes().count(), 0);
    }

    #[test]
    fn a_window_holds_and_what_was_before_resumes() {
        let timeline = painted(&[(days((3, 1), (5, 31)), 'b')]);
        assert_eq!(steps(&timeline), [("start".into(), 'a'), ("03-01".into(), 'b'), ("06-01".into(), 'a')]);
        assert_eq!(read(&timeline, &[(2, 28), (3, 1), (5, 31), (6, 1)]), "abba");
    }

    #[test]
    fn a_window_nested_in_an_earlier_one_restores_the_earlier_value_after_it() {
        let timeline = painted(&[(days((1, 1), (12, 31)), 'b'), (days((4, 1), (4, 30)), 'c')]);
        assert_eq!(read(&timeline, &[(3, 31), (4, 1), (4, 30), (5, 1)]), "bccb");
        assert_eq!(steps(&timeline).len(), 5, "a, b, c, b, a");
    }

    #[test]
    fn a_later_window_that_overlaps_an_earlier_one_wins_where_they_meet() {
        let timeline = painted(&[(days((3, 1), (5, 31)), 'b'), (days((5, 1), (8, 31)), 'c')]);
        assert_eq!(read(&timeline, &[(2, 28), (3, 1), (4, 30), (5, 1), (8, 31), (9, 1)]), "abbcca");
        // …and the other way round: the later one begins first.
        let timeline = painted(&[(days((5, 1), (8, 31)), 'c'), (days((3, 1), (5, 31)), 'b')]);
        assert_eq!(read(&timeline, &[(2, 28), (3, 1), (5, 31), (6, 1), (8, 31), (9, 1)]), "abbcca");
    }

    #[test]
    fn a_window_covering_several_steps_replaces_them_all() {
        let timeline =
            painted(&[(days((2, 1), (2, 28)), 'b'), (days((4, 1), (4, 30)), 'c'), (days((1, 1), (6, 30)), 'd')]);
        assert_eq!(steps(&timeline), [("start".into(), 'a'), ("01-01".into(), 'd'), ("07-01".into(), 'a')]);
    }

    #[test]
    fn an_unbounded_end_is_from_now_on() {
        let from_june = Days::new(day(6, 1), Day::MAX).unwrap();
        let timeline = painted(&[(from_june, 'b'), (days((8, 1), (8, 31)), 'c')]);
        assert_eq!(
            steps(&timeline),
            [("start".into(), 'a'), ("06-01".into(), 'b'), ("08-01".into(), 'c'), ("09-01".into(), 'b')]
        );
        assert_eq!(*timeline.at(Day::MAX), 'b');
        let always = painted(&[(Days::ALWAYS, 'z')]);
        assert_eq!(steps(&always), [("start".into(), 'z')], "everything painted over leaves one step");
    }

    #[test]
    fn painting_the_same_value_merges_steps() {
        let timeline = painted(&[(days((3, 1), (3, 31)), 'b'), (days((4, 1), (4, 30)), 'b')]);
        assert_eq!(steps(&timeline), [("start".into(), 'a'), ("03-01".into(), 'b'), ("05-01".into(), 'a')]);
        let again =
            painted(&[(days((3, 1), (3, 31)), 'b'), (days((3, 1), (3, 31)), 'b'), (days((3, 10), (3, 20)), 'b')]);
        assert_eq!(steps(&again).len(), 3, "painting what already holds changes nothing");
        let undone = painted(&[(days((3, 1), (3, 31)), 'b'), (days((3, 1), (3, 31)), 'a')]);
        assert_eq!(steps(&undone), [("start".into(), 'a')], "painting the old value back is no change at all");
        let bridged =
            painted(&[(days((3, 1), (3, 10)), 'b'), (days((3, 21), (3, 31)), 'b'), (days((3, 11), (3, 20)), 'b')]);
        assert_eq!(steps(&bridged), [("start".into(), 'a'), ("03-01".into(), 'b'), ("04-01".into(), 'a')]);
    }

    #[test]
    fn segments_are_read_in_order_and_whole() {
        let timeline = painted(&[(days((3, 1), (5, 31)), 'b'), (days((5, 1), (8, 31)), 'c')]);
        let seen = |within: Days| {
            timeline
                .within(within)
                .map(|(days, value)| (days.first() == Day::MIN, *value, days.last()))
                .collect::<Vec<_>>()
        };
        assert_eq!(seen(Days::ALWAYS).iter().map(|s| s.1).collect::<String>(), "abca");
        assert_eq!(seen(days((4, 1), (4, 30))), [(false, 'b', day(4, 30))]);
        assert_eq!(
            seen(days((4, 1), (5, 1))),
            [(false, 'b', day(4, 30)), (false, 'c', day(8, 31))],
            "whole stretches, not cut at `within`"
        );
        assert_eq!(seen(Days::on(day(1, 1))), [(true, 'a', day(2, 28))]);
        assert_eq!(seen(Days::on(day(9, 1))).len(), 1);
        assert_eq!(timeline.changes().map(|(from, _)| from).collect::<Vec<_>>(), [day(3, 1), day(5, 1), day(9, 1)]);
    }
}
