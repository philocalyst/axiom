//! The calendar vocabulary: ranges of days, months and years, an amount spread
//! over days, and the days a schedule falls due.
//!
//! [`Day`] and [`Span`] are points and lengths; everything above them that more
//! than one crate needs lives here, once: what a flow is recognized over, which
//! month a total is for, and when the rent is due.

use std::fmt;

use crate::day::{Day, Span, days_in_month};
use crate::num::Qty;

/// A compiled date layout such as `MM/DD/YYYY`, used by imported records.
/// The original spelling and three token spans are enough to parse dates and
/// suggest the same layout with day and month exchanged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DateLayout {
    pattern: Box<str>,
    fields: [DateField; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DateField {
    kind: DateFieldKind,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DateFieldKind {
    Year4,
    Year2,
    Month2,
    Month,
    Day2,
    Day,
}

impl DateFieldKind {
    fn index(self) -> usize {
        match self {
            DateFieldKind::Year4 | DateFieldKind::Year2 => 0,
            DateFieldKind::Month2 | DateFieldKind::Month => 1,
            DateFieldKind::Day2 | DateFieldKind::Day => 2,
        }
    }

    fn spelling(self) -> &'static str {
        match self {
            DateFieldKind::Year4 => "YYYY",
            DateFieldKind::Year2 => "YY",
            DateFieldKind::Month2 => "MM",
            DateFieldKind::Month => "M",
            DateFieldKind::Day2 => "DD",
            DateFieldKind::Day => "D",
        }
    }

    fn widths(self) -> std::ops::RangeInclusive<usize> {
        match self {
            DateFieldKind::Year4 => 4..=4,
            DateFieldKind::Year2 | DateFieldKind::Month2 | DateFieldKind::Day2 => 2..=2,
            DateFieldKind::Month | DateFieldKind::Day => 1..=2,
        }
    }
}

impl DateLayout {
    /// Compiles a layout with one `YYYY` or `YY`, one `MM` or `M`, one `DD` or
    /// `D`, and literal separators. Single-letter month/day fields accept one
    /// or two digits; a two-digit year is read as 2000 through 2099.
    pub fn parse(pattern: &str) -> Option<DateLayout> {
        let (mut fields, mut count, mut at) = ([DateField { kind: DateFieldKind::Year4, start: 0, end: 0 }; 3], 0, 0);
        let mut found = [false; 3];
        while at < pattern.len() {
            let rest = &pattern[at..];
            let token = [
                ("YYYY", DateFieldKind::Year4),
                ("YY", DateFieldKind::Year2),
                ("MM", DateFieldKind::Month2),
                ("M", DateFieldKind::Month),
                ("DD", DateFieldKind::Day2),
                ("D", DateFieldKind::Day),
            ]
            .into_iter()
            .find(|(token, _)| rest.starts_with(token));
            if let Some((token, kind)) = token {
                let index = kind.index();
                if found[index] || count == fields.len() {
                    return None;
                }
                found[index] = true;
                fields[count] = DateField {
                    kind,
                    start: u32::try_from(at).ok()?,
                    end: u32::try_from(at + token.len()).ok()?,
                };
                count += 1;
                at += token.len();
            } else {
                let ch = rest.chars().next()?;
                if ch.is_alphanumeric() {
                    return None;
                }
                at += ch.len_utf8();
            }
        }
        if count != 3 || found.iter().any(|&field| !field) {
            return None;
        }
        // A variable-width field next to another field needs a separator to
        // say where its digits end.
        if fields.windows(2).any(|pair| {
            pair[0].end == pair[1].start
                && (pair[0].kind.widths().start() != pair[0].kind.widths().end()
                    || pair[1].kind.widths().start() != pair[1].kind.widths().end())
        }) {
            return None;
        }
        Some(DateLayout { pattern: pattern.into(), fields })
    }

    /// Reads a date in this layout. Invalid digits, separators and calendar
    /// dates return `None`.
    pub fn read(&self, text: &str) -> Option<Day> {
        self.read_field(text, 0, 0, 0, [None; 3])
    }

    fn read_field(
        &self,
        text: &str,
        field_at: usize,
        pattern_at: usize,
        text_at: usize,
        values: [Option<u32>; 3],
    ) -> Option<Day> {
        if field_at == self.fields.len() {
            let suffix = &self.pattern[pattern_at..];
            let rest = text.get(text_at..)?.strip_prefix(suffix)?;
            if !rest.is_empty() {
                return None;
            }
            let mut year = values[0]?;
            let year2 = matches!(self.fields[0].kind, DateFieldKind::Year2)
                || matches!(self.fields[1].kind, DateFieldKind::Year2)
                || matches!(self.fields[2].kind, DateFieldKind::Year2);
            if year2 {
                year += 2000;
            }
            return Day::from_ymd(year as i32, values[1]?, values[2]?);
        }
        let field = self.fields[field_at];
        let separator = &self.pattern[pattern_at..field.start as usize];
        text.get(text_at..)?.strip_prefix(separator)?;
        let text_at = text_at + separator.len();
        let widths = field.kind.widths();
        for width in widths {
            let digits = text.get(text_at..)?.get(..width)?;
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            let mut values = values;
            values[field.kind.index()] = Some(digits.parse().ok()?);
            if let Some(day) = self.read_field(text, field_at + 1, field.end as usize, text_at + width, values) {
                return Some(day);
            }
        }
        None
    }

    /// The same layout with `MM`/`M` and `DD`/`D` exchanged.
    pub fn swapped(&self) -> DateLayout {
        let mut pattern = String::with_capacity(self.pattern.len());
        let mut at = 0;
        for field in self.fields {
            let start = field.start as usize;
            let end = field.end as usize;
            pattern.push_str(&self.pattern[at..start]);
            let kind = match field.kind {
                DateFieldKind::Month2 => DateFieldKind::Day2,
                DateFieldKind::Month => DateFieldKind::Day,
                DateFieldKind::Day2 => DateFieldKind::Month2,
                DateFieldKind::Day => DateFieldKind::Month,
                year => year,
            };
            pattern.push_str(kind.spelling());
            at = end;
        }
        pattern.push_str(&self.pattern[at..]);
        DateLayout::parse(&pattern).expect("swapping month and day preserves a valid date layout")
    }
}

impl fmt::Display for DateLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.pattern)
    }
}

#[cfg(test)]
mod date_layout_tests {
    use super::*;

    #[test]
    fn reads_fixed_and_variable_width_dates_and_swaps_month_and_day() {
        let us = DateLayout::parse("MM/DD/YYYY").unwrap();
        let eu = us.swapped();
        assert_eq!(us.to_string(), "MM/DD/YYYY");
        assert_eq!(eu.to_string(), "DD/MM/YYYY");
        assert_eq!(us.read("03/09/2026"), Day::from_ymd(2026, 3, 9));
        assert_eq!(eu.read("09/03/2026"), Day::from_ymd(2026, 3, 9));

        let short = DateLayout::parse("M/D/YY").unwrap();
        assert_eq!(short.read("3/9/26"), Day::from_ymd(2026, 3, 9));
        assert_eq!(short.read("13/9/26"), None);
        assert_eq!(DateLayout::parse("MM-M-YYYY"), None);
        assert_eq!(DateLayout::parse("YYYY.MM.DD").unwrap().read("2026.02.30"), None);
    }
}

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
    fn land(self, base: Day) -> Option<Day> {
        let (year, month, _) = base.ymd();
        let clamped = |month: u32, day: u32| checked_day(year, month, day.min(days_in_month(year, month)));
        match self {
            On::MonthDay(day) => clamped(month, u32::from(day)),
            On::Last => checked_day(year, month, days_in_month(year, month)),
            On::YearDay { month, day } => clamped(u32::from(month).clamp(1, 12), u32::from(day)),
            On::Weekday(weekday) => i32::try_from(i64::from(base.0) + i64::from((u32::from(weekday) + 7 - base.weekday()) % 7))
                .ok()
                .map(Day),
        }
    }
}

/// Converts a civil date without the user-facing year bound on `Day::from_ymd`.
fn checked_day(year: i32, month: u32, day: u32) -> Option<Day> {
    if !(1..=12).contains(&month) || !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    let mut year = i64::from(year);
    let month = i64::from(month);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let ordinal = era * 146_097 + day_of_era - 719_468;
    i32::try_from(ordinal).ok().map(Day)
}

fn checked_add(day: Day, months: i64, days: i64) -> Option<Day> {
    let (year, month, day_of_month) = day.ymd();
    let month_index = i64::from(year).checked_mul(12)?.checked_add(i64::from(month) - 1)?.checked_add(months)?;
    let year = i32::try_from(month_index.div_euclid(12)).ok()?;
    let month = u32::try_from(month_index.rem_euclid(12) + 1).ok()?;
    let landed = checked_day(year, month, day_of_month.min(days_in_month(year, month)))?;
    i32::try_from(i64::from(landed.0).checked_add(days)?).ok().map(Day)
}

fn cadence_day(anchor: Day, step: Span, n: u64) -> Option<Day> {
    let n = i64::try_from(n).ok()?;
    checked_add(anchor, i64::from(step.months).checked_mul(n)?, i64::from(step.days).checked_mul(n)?)
}

/// The first cadence index whose base day is on or after `target`. Positive
/// calendar steps are monotonic, so exponential search plus binary search
/// keeps a `Day::MIN` anchor bounded by the logarithm of the elapsed span.
fn first_cadence_at_or_after(anchor: Day, step: Span, target: Day) -> Option<u64> {
    if anchor >= target {
        return Some(0);
    }
    let (mut low, mut high) = (0u64, 1u64);
    loop {
        match cadence_day(anchor, step, high) {
            Some(day) if day < target => {
                low = high;
                high = high.checked_mul(2)?;
            }
            Some(_) | None => break,
        }
    }
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if cadence_day(anchor, step, middle).is_some_and(|day| day < target) {
            low = middle;
        } else {
            high = middle;
        }
    }
    cadence_day(anchor, step, high).map(|_| high)
}

struct Landings<'a> {
    base: Day,
    on: &'a [On],
    previous: Option<Day>,
    empty_pending: bool,
}

impl<'a> Iterator for Landings<'a> {
    type Item = Day;

    fn next(&mut self) -> Option<Self::Item> {
        if self.on.is_empty() {
            return std::mem::replace(&mut self.empty_pending, false).then_some(self.base);
        }
        let next = self
            .on
            .iter()
            .filter_map(|on| on.land(self.base))
            .filter(|&day| self.previous.is_none_or(|previous| day > previous))
            .min()?;
        self.previous = Some(next);
        Some(next)
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
    // Cadences are positive spans. Reject malformed direct API values too.
    let advances = step.months >= 0 && step.days >= 0 && step > Span::default();
    let landing = |on: &On, backward: bool| match (on, backward) {
        (On::YearDay { .. }, _) => 365,
        (On::MonthDay(_), _) => 30,
        (On::Weekday(_), false) => 6,
        (On::Weekday(_) | On::Last, true) | (On::Last, false) => 0,
    };
    let forward_landing = on.iter().map(|on| landing(on, false)).max().unwrap_or(0);
    let backward_landing = on.iter().map(|on| landing(on, true)).max().unwrap_or(0);
    let target_ordinal = (i64::from(anchor.max(within.first()).0) - forward_landing)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    let target = Day(target_ordinal as i32);
    let first_step = first_cadence_at_or_after(anchor, step, target).unwrap_or(u64::MAX);
    let base_limit = (i64::from(within.last().0) + backward_landing).min(i64::from(i32::MAX));
    std::iter::successors(Some(first_step), |&n| n.checked_add(1))
        .take_while(move |_| advances)
        .map_while(move |n| cadence_day(anchor, step, n))
        .take_while(move |day| i64::from(day.0) <= base_limit)
        .flat_map(move |base| Landings { base, on, previous: None, empty_pending: true })
        .filter(move |&day| day >= anchor && within.contains(day))
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

    #[test]
    fn schedules_fast_forward_from_day_min_without_changing_their_phase() {
        let february = days(day(2026, 2, 1), day(2026, 2, 28));
        let target = day(2026, 1, 2);
        let monthly_index = first_cadence_at_or_after(Day::MIN, Span::months(1), target).unwrap();
        assert!(monthly_index > 1_000_000);
        assert!(cadence_day(Day::MIN, Span::months(1), monthly_index).unwrap() >= target);
        assert!(cadence_day(Day::MIN, Span::months(1), monthly_index - 1).unwrap() < target);
        assert_eq!(
            due_days(Cadence::Every(Span::days(1)), &[], Day::MIN, february),
            (1..=28).map(|d| format!("2026-02-{d:02}")).collect::<Vec<_>>()
        );
        assert_eq!(
            due_days(Cadence::Every(Span::months(1)), &[On::MonthDay(1)], Day::MIN, february),
            ["2026-02-01"]
        );
        assert_eq!(
            due_days(
                Cadence::Every(Span::months(1)),
                &[On::MonthDay(1)],
                Day::MIN,
                days(day(2026, 1, 31), day(2026, 2, 6))
            ),
            ["2026-02-01"]
        );
        assert_eq!(
            due_days(Cadence::Every(Span::months(12)), &[On::YearDay { month: 2, day: 29 }], Day::MIN, february),
            ["2026-02-28"]
        );
    }

    #[test]
    fn schedules_stop_at_day_max_without_overflowing_landing_dates() {
        let first = Day(i32::MAX - 2);
        let within = days(first, Day::MAX);
        assert_eq!(
            due(Cadence::Every(Span::days(1)), &[], first, within).collect::<Vec<_>>(),
            [first, Day(i32::MAX - 1), Day::MAX]
        );
        assert!(due(Cadence::Every(Span::months(1)), &[On::Last], Day::MAX, within).all(|day| within.contains(day)));
    }
}
