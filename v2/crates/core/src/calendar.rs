//! The calendar vocabulary: ranges of days, months and years, an amount spread
//! over days, and the days a schedule falls due.
//!
//! [`Day`] and [`Span`] are points and lengths; everything above them that more
//! than one crate needs lives here, once: what a flow is recognized over, which
//! month a total is for, and when the rent is due.

use std::fmt;

use crate::day::{Day, Span, days_in_month};
use crate::num::Qty;

/// An inclusive range of days, never empty: what a flow is recognized over,
/// what a residence or a contract lasts, and where a declaration holds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Days {
    first: Day,
    last: Day,
}

impl Days {
    /// Every day there is: an unbounded span, where a declaration holds.
    pub const ALWAYS: Days = Days { first: Day::MIN, last: Day::MAX };

    /// `None` when `last` is before `first`.
    pub fn new(first: Day, last: Day) -> Option<Days> {
        (first <= last).then_some(Days { first, last })
    }

    /// The one day.
    pub const fn on(day: Day) -> Days {
        Days { first: day, last: day }
    }

    pub const fn first(self) -> Day {
        self.first
    }

    pub const fn last(self) -> Day {
        self.last
    }

    pub fn contains(self, day: Day) -> bool {
        self.first <= day && day <= self.last
    }

    pub fn overlaps(self, other: Days) -> bool {
        self.first <= other.last && other.first <= self.last
    }

    /// The days in both, if there are any.
    pub fn intersect(self, other: Days) -> Option<Days> {
        Days::new(self.first.max(other.first), self.last.min(other.last))
    }

    /// The one range that both make together, if they overlap or run right up
    /// to each other.
    pub fn merge(self, other: Days) -> Option<Days> {
        let meets = |a: Days, b: Days| a.first.0 <= b.last.0.saturating_add(1);
        (meets(self, other) && meets(other, self))
            .then(|| Days { first: self.first.min(other.first), last: self.last.max(other.last) })
    }

    /// The same days `by` days later, or earlier when negative. An unbounded
    /// end (`Day::MIN`, `Day::MAX`) stays unbounded.
    pub fn moved(self, by: i32) -> Days {
        let shift = |day: Day| if day == Day::MIN || day == Day::MAX { day } else { Day(day.0.saturating_add(by)) };
        Days { first: shift(self.first), last: shift(self.last) }
    }

    /// The day, if these are just the one.
    pub fn single(self) -> Option<Day> {
        (self.first == self.last).then_some(self.first)
    }

    /// How many days: 1 for [`Days::on`]. [`Days::ALWAYS`] has more than a
    /// `u32` counts, and says `u32::MAX`.
    pub fn len(self) -> u32 {
        u32::try_from(self.count()).unwrap_or(u32::MAX)
    }

    fn count(self) -> i64 {
        i64::from(self.last.0) - i64::from(self.first.0) + 1
    }
}

/// A calendar month or year.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Period {
    Month,
    Year,
}

impl Period {
    /// How many months one period is.
    pub const fn months(self) -> i32 {
        match self {
            Period::Month => 1,
            Period::Year => 12,
        }
    }
}

/// One calendar month or year: `2026-03`, `2026`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Window {
    period: Period,
    days: Days,
}

impl Window {
    /// The month or year that has `day` in it. Total: at the ends of time, where
    /// the calendar gives out, the window is cut short rather than wrong.
    pub fn containing(period: Period, day: Day) -> Window {
        let (year, month, date) = day.ymd();
        let (first, last) = match period {
            Period::Month => {
                let first = Day(day.0.saturating_sub(date as i32 - 1));
                (first, Day(first.0.saturating_add(days_in_month(year, month) as i32 - 1)))
            }
            Period::Year => (Day::from_ymd(year, 1, 1).unwrap_or(day), Day::from_ymd(year, 12, 31).unwrap_or(day)),
        };
        Window { period, days: Days { first, last } }
    }

    /// The window `days` are exactly, if they are one.
    pub fn exactly(days: Days) -> Option<Window> {
        [Period::Month, Period::Year]
            .into_iter()
            .map(|period| Window::containing(period, days.first()))
            .find(|window| window.days == days)
    }

    /// The windows of `period` that any of `days` fall in, in order. `days` must end.
    pub fn covering(period: Period, days: Days) -> impl Iterator<Item = Window> {
        let first = Window::containing(period, days.first());
        // The next window is worked out only if these days go on into it.
        std::iter::successors(Some(first), move |window| (window.days.last() < days.last()).then(|| window.following()))
    }

    pub fn days(self) -> Days {
        self.days
    }

    /// The window `n` periods on, or before when `n` is negative.
    pub fn after(self, n: i32) -> Window {
        Window::containing(self.period, self.days.first().add(Span::months(self.period.months() * n)))
    }

    /// How many windows on from this one the window containing `day` is: 0 for
    /// this one, negative before it.
    pub fn steps_to(self, day: Day) -> i64 {
        let months = |day: Day| {
            let (year, month, _) = day.ymd();
            i64::from(year) * 12 + i64::from(month) - 1
        };
        (months(day) - months(self.days.first())).div_euclid(self.period.months().into())
    }

    pub fn next(self) -> Window {
        self.after(1)
    }

    pub fn previous(self) -> Window {
        self.after(-1)
    }

    /// The window that starts the day after this one ends. Not the last day there is.
    fn following(&self) -> Window {
        Window::containing(self.period, self.days.last().add_days(1))
    }
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, _) = self.days.first().ymd();
        match self.period {
            Period::Month => write!(f, "{year:04}-{month:02}"),
            Period::Year => write!(f, "{year:04}"),
        }
    }
}

/// The share of `qty`, spread evenly per day over `over`, that falls in
/// `within`. What has been recognized by the end of each day is rounded once,
/// and a share is the difference of two such values, so the shares of a
/// partition of `over` add up to `qty` exactly.
pub fn spread(qty: Qty, over: Days, within: Days) -> Qty {
    // Nearly every flow belongs to one day: no calendar to consult.
    if let Some(day) = over.single() {
        return if within.contains(day) { qty } else { Qty::ZERO };
    }
    let whole = over.count();
    let through = |day: i64| {
        let elapsed = (day - i64::from(over.first.0) + 1).clamp(0, whole);
        qty.share(Qty(elapsed), Qty(whole)).expect("a part of the whole is no larger than it")
    };
    through(i64::from(within.last.0)) - through(i64::from(within.first.0) - 1)
}

/// How often something falls due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    /// `monthly` is one month, `every 2w` fourteen days.
    Every(Span),
    /// Twice a month, on two days (`on 15, last`).
    TwiceMonthly,
}

/// The day within a month, a year or a week that something falls on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    /// `on 15`: past the month's end clamps to its last day.
    MonthDay(u8),
    /// `on last`: the month's last day, whatever its length.
    Last,
    /// `on 04-15`: that month (1 to 12) and day of every year.
    YearDay { month: u8, day: u8 },
    /// `on monday`: Monday = 0 … Sunday = 6, as [`Day::weekday`].
    Weekday(u8),
}

impl On {
    /// The day this asks for, in the month, year or week `base` is in.
    fn land(self, base: Day) -> Day {
        let (year, month, _) = base.ymd();
        let clamped = |month: u32, day: u32| Day::from_ymd(year, month, day.min(days_in_month(year, month)));
        match self {
            On::MonthDay(day) => clamped(month, u32::from(day)).unwrap_or(base),
            On::Last => base.month_end(),
            On::YearDay { month, day } => clamped(u32::from(month).clamp(1, 12), u32::from(day)).unwrap_or(base),
            On::Weekday(weekday) => base.add_days(((u32::from(weekday) + 7 - base.weekday()) % 7) as i32),
        }
    }
}

/// The days a schedule falls due in `within`, in order: every `every` from
/// `anchor`, landed on `on` (the step's own day where `on` says nothing). No
/// day before `anchor` is due. Steps are counted from the anchor, never from
/// the previous day, so a month-end clamp does not drag later months with it:
/// `monthly on 31` from January 31 is February 28, then March 31. A step that
/// lands on two days takes both, so `TwiceMonthly` is monthly steps landing on
/// both of `on`'s days; a day two of them land on is due once. `within` must
/// end: the schedule never does.
pub fn due<'a>(every: Cadence, on: &'a [On], anchor: Day, within: Days) -> impl Iterator<Item = Day> + 'a {
    let step = match every {
        Cadence::Every(span) => span,
        Cadence::TwiceMonthly => Span::months(1),
    };
    // A step that goes nowhere would never end.
    let advances = step > Span::default();
    let landed = move |base: Day| {
        let mut days: Vec<Day> = if on.is_empty() { vec![base] } else { on.iter().map(|on| on.land(base)).collect() };
        days.sort_unstable();
        days.dedup();
        days
    };
    (0..)
        .take_while(move |_| advances)
        .map(move |n| anchor.add(Span { months: step.months * n, days: step.days * n }))
        .flat_map(landed)
        .skip_while(move |&day| day < anchor.max(within.first()))
        .take_while(move |&day| day <= within.last())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    fn days(first: Day, last: Day) -> Days {
        Days::new(first, last).unwrap()
    }

    #[test]
    fn days_are_never_empty() {
        let (jan, feb) = (day(2026, 1, 31), day(2026, 2, 1));
        assert!(Days::new(feb, jan).is_none());
        assert_eq!(Days::new(jan, jan), Some(Days::on(jan)));
        assert_eq!((Days::on(jan).len(), days(jan, feb).len()), (1, 2));
        assert_eq!(Days::on(jan).single(), Some(jan));
        assert_eq!(days(jan, feb).single(), None);
        assert_eq!(Days::ALWAYS.len(), u32::MAX, "more days than a u32 counts");
    }

    #[test]
    fn days_meet_at_their_edges() {
        let (a, b) = (days(day(2026, 1, 1), day(2026, 1, 31)), days(day(2026, 1, 31), day(2026, 2, 28)));
        let c = days(day(2026, 2, 1), day(2026, 2, 28));
        assert!(a.contains(day(2026, 1, 31)) && !a.contains(day(2026, 2, 1)));
        assert!(a.overlaps(b) && b.overlaps(a) && !a.overlaps(c));
        assert_eq!(a.intersect(b), Some(Days::on(day(2026, 1, 31))));
        assert_eq!(a.intersect(c), None);
        assert_eq!(a.intersect(Days::ALWAYS), Some(a));
    }

    #[test]
    fn days_that_meet_or_touch_merge() {
        let (jan, feb, mar) = (
            days(day(2026, 1, 1), day(2026, 1, 31)),
            days(day(2026, 2, 1), day(2026, 2, 28)),
            days(day(2026, 3, 2), day(2026, 3, 31)),
        );
        assert_eq!(
            jan.merge(feb),
            Some(days(day(2026, 1, 1), day(2026, 2, 28))),
            "the 31st and the 1st are neighbours"
        );
        assert_eq!(feb.merge(jan), jan.merge(feb));
        assert_eq!(feb.merge(mar), None, "a day between them is a gap");
        assert_eq!(jan.merge(Days::on(day(2026, 1, 10))), Some(jan));
        assert_eq!(Days::ALWAYS.merge(jan), Some(Days::ALWAYS), "nothing is one past the end of time");
    }

    #[test]
    fn windows_step_through_months_and_years() {
        let leap = Window::containing(Period::Month, day(2024, 2, 10));
        assert_eq!(leap.days(), days(day(2024, 2, 1), day(2024, 2, 29)));
        let named = [leap.previous(), leap, leap.next()].map(|window| window.to_string());
        assert_eq!(named, ["2024-01", "2024-02", "2024-03"]);
        assert_eq!(leap.after(-2).days(), days(day(2023, 12, 1), day(2023, 12, 31)), "back over a year's end");
        let year = Window::containing(Period::Year, day(2026, 6, 15));
        assert_eq!((year.days(), year.to_string()), (days(day(2026, 1, 1), day(2026, 12, 31)), "2026".to_string()));
        assert_eq!(year.next().days().first(), day(2027, 1, 1));
        assert_eq!(year.previous().days().last(), day(2025, 12, 31));
        assert_eq!(Window::containing(Period::Month, day(2026, 1, 1)).days().first(), day(2026, 1, 1));
    }

    #[test]
    fn a_window_counts_the_windows_to_a_day() {
        let march = Window::containing(Period::Month, day(2026, 3, 15));
        let steps =
            [day(2025, 12, 31), day(2026, 2, 28), day(2026, 3, 1), day(2026, 3, 31), day(2026, 4, 1), day(2027, 3, 1)];
        assert_eq!(steps.map(|at| march.steps_to(at)), [-3, -1, 0, 0, 1, 12]);
        let year = Window::containing(Period::Year, day(2026, 6, 1));
        assert_eq!(
            [day(2025, 12, 31), day(2026, 12, 31), day(2027, 1, 1), day(2020, 1, 1)].map(|at| year.steps_to(at)),
            [-1, 0, 1, -6]
        );
        for at in steps {
            assert!(march.after(march.steps_to(at) as i32).days().contains(at), "{at}");
        }
    }

    #[test]
    fn days_move_and_stay_put_at_the_ends_of_time() {
        let march = days(day(2026, 3, 1), day(2026, 3, 31));
        assert_eq!(march.moved(31), days(day(2026, 4, 1), day(2026, 5, 1)));
        assert_eq!(march.moved(-1).first(), day(2026, 2, 28));
        assert_eq!(Days::ALWAYS.moved(9), Days::ALWAYS);
        assert_eq!(Days::ALWAYS.moved(-9), Days::ALWAYS);
    }

    #[test]
    fn a_window_is_found_from_exactly_its_days() {
        let march = days(day(2026, 3, 1), day(2026, 3, 31));
        assert_eq!(Window::exactly(march), Some(Window::containing(Period::Month, day(2026, 3, 9))));
        let year = days(day(2026, 1, 1), day(2026, 12, 31));
        assert_eq!(Window::exactly(year), Some(Window::containing(Period::Year, day(2026, 7, 4))));
        assert_eq!(Window::exactly(days(day(2026, 3, 2), day(2026, 3, 31))), None);
        assert_eq!(Window::exactly(Days::ALWAYS), None);
    }

    #[test]
    fn windows_cover_the_days_they_touch() {
        let names = |period, over| Window::covering(period, over).map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            names(Period::Month, days(day(2025, 11, 20), day(2026, 2, 3))),
            ["2025-11", "2025-12", "2026-01", "2026-02"]
        );
        assert_eq!(names(Period::Year, days(day(2025, 12, 31), day(2026, 1, 1))), ["2025", "2026"]);
        assert_eq!(names(Period::Month, Days::on(day(2026, 5, 31))), ["2026-05"]);
    }

    #[test]
    fn a_share_of_a_range_is_by_days_and_the_shares_add_up() {
        let over = days(day(2025, 11, 1), day(2026, 1, 31));
        let month = |year, month| Window::containing(Period::Month, day(year, month, 15)).days();
        let parts =
            [month(2025, 11), month(2025, 12), month(2026, 1)].map(|within| spread(Qty(120_00), over, within).0);
        assert_eq!(parts, [3_913, 4_044, 4_043]);
        assert_eq!(parts.iter().sum::<i64>(), 120_00);
    }

    #[test]
    fn a_share_is_what_falls_in_the_window() {
        let over = days(day(2026, 1, 1), day(2026, 12, 31));
        let month = |m| Window::containing(Period::Month, day(2026, m, 1)).days();
        assert_eq!(spread(Qty(36_500), over, month(1)), Qty(3_100));
        assert_eq!(spread(Qty(36_500), over, Days::ALWAYS), Qty(36_500));
        assert_eq!(spread(Qty(36_500), over, days(day(2025, 1, 1), day(2025, 12, 31))), Qty::ZERO);
        assert_eq!(spread(Qty(36_500), over, days(day(2026, 12, 1), day(2027, 6, 1))), Qty(3_100));
        assert_eq!(
            spread(Qty(-7), over, Days::ALWAYS),
            Qty(-7),
            "whatever the sign, and however far the window reaches"
        );
        // A premium that does not divide by 365, cut at every month's end.
        let cut: Vec<_> = Window::covering(Period::Month, over).map(|w| spread(Qty(120_001), over, w.days())).collect();
        assert_eq!(cut.iter().copied().sum::<Qty>(), Qty(120_001));
    }

    #[test]
    fn one_day_is_recognized_at_once_or_not_at_all() {
        let once = Days::on(day(2026, 1, 18));
        assert_eq!(spread(Qty(-8_420), once, days(day(2026, 1, 1), day(2026, 1, 31))), Qty(-8_420));
        assert_eq!(spread(Qty(-8_420), once, Days::on(day(2026, 1, 18))), Qty(-8_420));
        assert_eq!(spread(Qty(-8_420), once, Days::on(day(2026, 1, 19))), Qty::ZERO);
    }

    /// The days `due` names, written out.
    fn due_days(every: Cadence, on: &[On], anchor: Day, within: Days) -> Vec<String> {
        due(every, on, anchor, within).map(|day| day.to_string()).collect()
    }

    fn monthly() -> Cadence {
        Cadence::Every(Span::months(1))
    }

    #[test]
    fn a_month_end_clamp_does_not_drag_later_months() {
        let all = days(day(2026, 1, 1), day(2026, 6, 30));
        let jan31 = day(2026, 1, 31);
        assert_eq!(
            due_days(monthly(), &[On::MonthDay(31)], jan31, all),
            ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30", "2026-05-31", "2026-06-30"]
        );
        // With no `on`, the anchor's own day steps: 31st, then the 28th, then the 31st again.
        assert_eq!(due_days(monthly(), &[], jan31, all)[..3], ["2026-01-31", "2026-02-28", "2026-03-31"]);
    }

    #[test]
    fn last_is_the_end_of_each_month() {
        let leap = days(day(2028, 1, 1), day(2028, 4, 30));
        assert_eq!(
            due_days(monthly(), &[On::Last], day(2028, 1, 10), leap),
            ["2028-01-31", "2028-02-29", "2028-03-31", "2028-04-30"]
        );
    }

    #[test]
    fn twice_monthly_takes_both_days_of_each_month_in_order() {
        let q1 = days(day(2026, 1, 1), day(2026, 3, 31));
        let expected = ["2026-01-15", "2026-01-31", "2026-02-15", "2026-02-28", "2026-03-15", "2026-03-31"];
        assert_eq!(due_days(Cadence::TwiceMonthly, &[On::MonthDay(15), On::Last], day(2026, 1, 1), q1), expected);
        assert_eq!(
            due_days(Cadence::TwiceMonthly, &[On::Last, On::MonthDay(15)], day(2026, 1, 1), q1),
            expected,
            "in whatever order they are written"
        );
        // Days two landings share are due once: the 30th clamps to February's last day.
        let feb = days(day(2026, 2, 1), day(2026, 2, 28));
        assert_eq!(
            due_days(Cadence::TwiceMonthly, &[On::MonthDay(30), On::Last], day(2026, 1, 1), feb),
            ["2026-02-28"]
        );
    }

    #[test]
    fn every_two_weeks_counts_from_the_anchor() {
        let anchor = day(2026, 1, 9);
        let every = Cadence::Every(Span::days(14));
        let within = days(day(2026, 2, 1), day(2026, 3, 15));
        assert_eq!(due_days(every, &[], anchor, within), ["2026-02-06", "2026-02-20", "2026-03-06"]);
        assert_eq!(due_days(every, &[], anchor, Days::on(anchor)), ["2026-01-09"], "the anchor itself is the first");
        assert_eq!(
            due_days(every, &[], anchor, days(day(2025, 1, 1), day(2026, 1, 8))),
            Vec::<String>::new(),
            "nothing before it"
        );
    }

    #[test]
    fn a_weekday_lands_on_or_after_each_step() {
        // 2026-01-01 is a Thursday: the first Monday on or after it is the 5th.
        let mondays = [On::Weekday(0)];
        let every_week = Cadence::Every(Span::days(7));
        assert_eq!(
            due_days(every_week, &mondays, day(2026, 1, 1), days(day(2026, 1, 1), day(2026, 1, 31))),
            ["2026-01-05", "2026-01-12", "2026-01-19", "2026-01-26"]
        );
    }

    #[test]
    fn yearly_on_the_leap_day_falls_on_the_28th_between_leap_years() {
        let cadence = Cadence::Every(Span::months(12));
        let feb29 = [On::YearDay { month: 2, day: 29 }];
        let years = days(day(2026, 1, 1), day(2029, 12, 31));
        assert_eq!(
            due_days(cadence, &feb29, day(2026, 1, 1), years),
            ["2026-02-28", "2027-02-28", "2028-02-29", "2029-02-28"]
        );
        // Anchored on the leap day itself, the clamp still does not drift the years after it.
        assert_eq!(
            due_days(cadence, &feb29, day(2024, 2, 29), days(day(2024, 1, 1), day(2028, 12, 31))),
            ["2024-02-29", "2025-02-28", "2026-02-28", "2027-02-28", "2028-02-29"]
        );
    }

    #[test]
    fn a_day_on_the_year_is_landed_in_the_year_of_each_step() {
        let taxes = [On::YearDay { month: 4, day: 15 }];
        let every_year = Cadence::Every(Span::months(12));
        // Anchored after April 15, the first due day is next year's: nothing due precedes the anchor.
        assert_eq!(
            due_days(every_year, &taxes, day(2026, 6, 1), days(day(2026, 1, 1), day(2028, 12, 31))),
            ["2027-04-15", "2028-04-15"]
        );
    }

    #[test]
    fn a_schedule_that_goes_nowhere_is_never_due() {
        let always = Days::ALWAYS;
        assert_eq!(due(Cadence::Every(Span::default()), &[], day(2026, 1, 1), always).next(), None);
    }
}
