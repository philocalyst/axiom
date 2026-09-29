//! Regular flows: finding a rhythm in history, and the calendar rule that
//! projects a plan or a rhythm forward.

use std::borrow::Cow;

use axiom_core::day::days_in_month;
use axiom_core::{Day, Qty, Span};
use axiom_model::On;

/// A rhythm needs at least this many occurrences to be believed.
const MIN_OCCURRENCES: usize = 3;

/// How often a recurring flow comes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    Weekly,
    Biweekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Cadence {
    const ALL: [Cadence; 5] =
        [Cadence::Weekly, Cadence::Biweekly, Cadence::Monthly, Cadence::Quarterly, Cadence::Yearly];

    /// A typical gap between occurrences, in days.
    fn days(self) -> i32 {
        match self {
            Cadence::Weekly => 7,
            Cadence::Biweekly => 14,
            Cadence::Monthly => 30,
            Cadence::Quarterly => 91,
            Cadence::Yearly => 365,
        }
    }

    /// The calendar step: whole months where the rhythm is monthly, so the
    /// 31st does not drift.
    pub fn every(self) -> Span {
        match self {
            Cadence::Weekly => Span::days(7),
            Cadence::Biweekly => Span::days(14),
            Cadence::Monthly => Span::months(1),
            Cadence::Quarterly => Span::months(3),
            Cadence::Yearly => Span::months(12),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Cadence::Weekly => "weekly",
            Cadence::Biweekly => "every 2 weeks",
            Cadence::Monthly => "monthly",
            Cadence::Quarterly => "quarterly",
            Cadence::Yearly => "yearly",
        }
    }

    /// The cadence a typical gap belongs to: within 15% of its length.
    fn snap(gap: i32) -> Option<Cadence> {
        Cadence::ALL.into_iter().find(|cadence| (gap - cadence.days()).abs() * 100 <= cadence.days() * 15)
    }
}

/// A step between occurrences, in words: `monthly`, `every 3d`.
pub fn describe(every: Span) -> Cow<'static, str> {
    match Cadence::ALL.into_iter().find(|cadence| cadence.every() == every) {
        Some(cadence) => cadence.word().into(),
        None => format!("every {every}").into(),
    }
}

/// A rhythm found in a series of occurrences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recurrence {
    pub cadence: Cadence,
    /// The median amount: an observed one, not an average.
    pub amount: Qty,
    pub last: Day,
    /// The day of the month most occurrences fell on, for monthly rhythms.
    pub on: Option<On>,
}

impl Recurrence {
    /// Where the rhythm continues after the last occurrence.
    pub fn schedule(&self) -> Schedule {
        Schedule { anchor: self.last, every: self.cadence.every(), on: self.on, until: None }
    }
}

/// Looks for a rhythm in `occurrences` (oldest first): at least three, gaps
/// whose median snaps to a known cadence with little deviation, and the last
/// one recent enough that the rhythm is plausibly still going.
pub fn detect(occurrences: &[(Day, Qty)], today: Day) -> Option<Recurrence> {
    if occurrences.len() < MIN_OCCURRENCES {
        return None;
    }
    let gaps: Vec<i32> = occurrences.windows(2).map(|pair| pair[1].0.0 - pair[0].0.0).collect();
    let typical = median(&gaps);
    let cadence = Cadence::snap(typical)?;
    let deviations: Vec<i32> = gaps.iter().map(|gap| (gap - typical).abs()).collect();
    if median(&deviations) > (cadence.days() / 10).max(1) {
        return None;
    }

    let last = occurrences.last()?.0;
    // Within one and a half cadences of today.
    if i64::from(today.0 - last.0) * 2 > i64::from(cadence.days()) * 3 {
        return None;
    }
    let amounts: Vec<Qty> = occurrences.iter().map(|&(_, amount)| amount).collect();
    let on = match cadence {
        Cadence::Monthly | Cadence::Quarterly => {
            let days_of_month: Vec<u32> = occurrences.iter().map(|&(day, _)| day.ymd().2).collect();
            Some(On::MonthDay(median(&days_of_month) as u8))
        }
        Cadence::Weekly | Cadence::Biweekly | Cadence::Yearly => None,
    };
    Some(Recurrence { cadence, amount: median(&amounts), last, on })
}

/// The middle value; of two middles, the upper, so it is one actually seen.
fn median<T: Ord + Copy>(values: &[T]) -> T {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

/// When a plan or a rhythm falls: every `every` from `anchor`, on a given day
/// of its period.
#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    pub anchor: Day,
    pub every: Span,
    pub on: Option<On>,
    /// Inclusive.
    pub until: Option<Day>,
}

impl Schedule {
    /// The days after `after`, no later than `horizon` (or the schedule's own
    /// end), on which the schedule falls.
    pub fn days(&self, after: Day, horizon: Day) -> Vec<Day> {
        let last = self.until.map_or(horizon, |until| until.min(horizon));
        let mut days = Vec::new();
        if self.every == Span::default() {
            return days;
        }
        for step in 0.. {
            let day = self.nth(step);
            if day > last {
                break;
            }
            if day > after && day >= self.anchor {
                days.push(day);
            }
        }
        days
    }

    /// Steps are taken from the anchor, never from the previous day, so a
    /// month-end clamp does not drag every later month with it.
    fn nth(&self, step: i32) -> Day {
        let span = Span { months: self.every.months * step, days: self.every.days * step };
        land(self.anchor.add(span), self.on)
    }
}

/// Moves `base` to the day its period asks for.
fn land(base: Day, on: Option<On>) -> Day {
    let (year, month, _) = base.ymd();
    match on {
        None => base,
        // Past the month's end clamps to its last day.
        Some(On::MonthDay(day)) => {
            Day::from_ymd(year, month, u32::from(day).min(days_in_month(year, month))).unwrap_or(base)
        }
        Some(On::YearDay { month, day }) => {
            let month = u32::from(month);
            Day::from_ymd(year, month, u32::from(day).min(days_in_month(year, month.clamp(1, 12)))).unwrap_or(base)
        }
        Some(On::Weekday(weekday)) => base.add_days(((u32::from(weekday) + 7 - base.weekday()) % 7) as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    fn series(days: &[Day], amount: i64) -> Vec<(Day, Qty)> {
        days.iter().map(|&day| (day, Qty(amount))).collect()
    }

    #[test]
    fn a_monthly_rhythm_is_found_despite_uneven_month_lengths() {
        let rent =
            series(&[day(2026, 1, 1), day(2026, 2, 1), day(2026, 3, 1), day(2026, 4, 1), day(2026, 5, 1)], 1_800_00);
        let found = detect(&rent, day(2026, 5, 20)).expect("rent recurs");
        assert_eq!((found.cadence, found.amount, found.last), (Cadence::Monthly, Qty(1_800_00), day(2026, 5, 1)));
        assert_eq!(found.on, Some(On::MonthDay(1)));
        // Next comes June 1st, then July 1st.
        assert_eq!(found.schedule().days(day(2026, 5, 20), day(2026, 7, 31)), [day(2026, 6, 1), day(2026, 7, 1)]);
    }

    #[test]
    fn irregular_or_stale_series_are_not_rhythms() {
        let groceries = series(&[day(2026, 5, 1), day(2026, 5, 6), day(2026, 5, 16), day(2026, 5, 19)], 84_20);
        assert_eq!(detect(&groceries, day(2026, 5, 20)), None);
        let two = series(&[day(2026, 1, 1), day(2026, 2, 1)], 10_00);
        assert_eq!(detect(&two, day(2026, 2, 2)), None);
        let cancelled = series(&[day(2025, 1, 5), day(2025, 2, 5), day(2025, 3, 5)], 15_99);
        assert_eq!(detect(&cancelled, day(2026, 5, 20)), None);
    }

    #[test]
    fn the_median_amount_ignores_a_one_off_spike() {
        let bill = [
            (day(2026, 1, 9), Qty(60_00)),
            (day(2026, 1, 23), Qty(60_00)),
            (day(2026, 2, 6), Qty(900_00)),
            (day(2026, 2, 20), Qty(60_00)),
        ];
        let found = detect(&bill, day(2026, 3, 1)).expect("biweekly");
        assert_eq!((found.cadence, found.amount), (Cadence::Biweekly, Qty(60_00)));
    }

    #[test]
    fn month_days_clamp_without_drifting() {
        let schedule =
            Schedule { anchor: day(2026, 1, 31), every: Span::months(1), on: Some(On::MonthDay(31)), until: None };
        let days = schedule.days(day(2026, 1, 31), day(2026, 4, 30));
        assert_eq!(days, [day(2026, 2, 28), day(2026, 3, 31), day(2026, 4, 30)]);
    }

    #[test]
    fn weekdays_and_year_days_land_where_asked() {
        // 2026-01-01 is a Thursday; the first Monday on or after it is the 5th.
        let mondays = Schedule {
            anchor: day(2026, 1, 1),
            every: Span::days(7),
            on: Some(On::Weekday(0)),
            until: Some(day(2026, 1, 19)),
        };
        assert_eq!(
            mondays.days(day(2025, 12, 31), day(2026, 12, 31)),
            [day(2026, 1, 5), day(2026, 1, 12), day(2026, 1, 19)]
        );
        let taxes = Schedule {
            anchor: day(2026, 1, 1),
            every: Span::months(12),
            on: Some(On::YearDay { month: 4, day: 15 }),
            until: None,
        };
        assert_eq!(taxes.days(day(2026, 6, 1), day(2028, 12, 31)), [day(2027, 4, 15), day(2028, 4, 15)]);
    }
}
