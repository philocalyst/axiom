//! Regular flows: finding a rhythm in history, and the calendar rule that
//! projects a plan or a rhythm forward.

use std::borrow::Cow;

use axiom_core::{Cadence as CalendarCadence, Day, Days, Qty, Span, due};
use axiom_model::On;

/// A rhythm needs at least this many occurrences to be believed.
const MIN_OCCURRENCES: usize = 3;

/// How often a recurring flow comes: a calendar step, and the typical gap in
/// days that snaps to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cadence {
    /// Whole months where the rhythm is monthly, so the 31st does not drift.
    pub every: Span,
    days: i32,
    word: &'static str,
}

const WEEKLY: Cadence = Cadence {
    every: Span::days(7),
    days: 7,
    word: "weekly",
};
const BIWEEKLY: Cadence = Cadence {
    every: Span::days(14),
    days: 14,
    word: "every 2 weeks",
};
const MONTHLY: Cadence = Cadence {
    every: Span::months(1),
    days: 30,
    word: "monthly",
};
const QUARTERLY: Cadence = Cadence {
    every: Span::months(3),
    days: 91,
    word: "quarterly",
};
const YEARLY: Cadence = Cadence {
    every: Span::months(12),
    days: 365,
    word: "yearly",
};
const CADENCES: [Cadence; 5] = [WEEKLY, BIWEEKLY, MONTHLY, QUARTERLY, YEARLY];

impl Cadence {
    /// The cadence a typical gap belongs to: within 15% of its length.
    fn snap(gap: i32) -> Option<Cadence> {
        CADENCES
            .into_iter()
            .find(|cadence| (gap - cadence.days).abs() * 100 <= cadence.days * 15)
    }
}

/// A step between occurrences, in words: `monthly`, `every 3d`.
pub fn describe(every: Span) -> Cow<'static, str> {
    match CADENCES.into_iter().find(|cadence| cadence.every == every) {
        Some(cadence) => cadence.word.into(),
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
        Schedule {
            anchor: self.last,
            every: self.cadence.every,
            on: self.on,
            until: None,
        }
    }
}

/// Looks for a rhythm in `occurrences` (oldest first): at least three, gaps
/// whose median snaps to a known cadence with little deviation, and the last
/// one recent enough that the rhythm is plausibly still going.
pub fn detect(occurrences: &[(Day, Qty)], today: Day) -> Option<Recurrence> {
    if occurrences.len() < MIN_OCCURRENCES {
        return None;
    }
    let gaps: Vec<i32> = occurrences
        .windows(2)
        .map(|pair| pair[1].0.0 - pair[0].0.0)
        .collect();
    let typical = median(&gaps);
    let cadence = Cadence::snap(typical)?;
    let deviations: Vec<i32> = gaps.iter().map(|gap| (gap - typical).abs()).collect();
    if median(&deviations) > (cadence.days / 10).max(1) {
        return None;
    }

    let last = occurrences.last()?.0;
    // Within one and a half cadences of today.
    if i64::from(today.0 - last.0) * 2 > i64::from(cadence.days) * 3 {
        return None;
    }
    let amounts: Vec<Qty> = occurrences.iter().map(|&(_, amount)| amount).collect();
    let by_month = matches!(cadence.every.months, 1 | 3);
    let on = by_month.then(|| {
        let days_of_month: Vec<u32> = occurrences.iter().map(|&(day, _)| day.ymd().2).collect();
        On::MonthDay(median(&days_of_month) as u8)
    });
    Some(Recurrence {
        cadence,
        amount: median(&amounts),
        last,
        on,
    })
}

/// The middle value; of two middles, the upper, so it is one actually seen.
pub fn median<T: Ord + Copy>(values: &[T]) -> T {
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
    /// end), on which the schedule falls. The iterator avoids an intermediate
    /// date vector when callers only need the next day or to stream flows.
    pub fn days(&self, after: Day, horizon: Day) -> impl Iterator<Item = Day> + '_ {
        let last = self.until.map_or(horizon, |until| until.min(horizon));
        after
            .0
            .checked_add(1)
            .map(Day)
            .into_iter()
            .flat_map(move |first| Days::new(first, last))
            .flat_map(|within| {
                due(
                    CalendarCadence::Every(self.every),
                    self.on.as_slice(),
                    self.anchor,
                    within,
                )
            })
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
        let rent = series(
            &[
                day(2026, 1, 1),
                day(2026, 2, 1),
                day(2026, 3, 1),
                day(2026, 4, 1),
                day(2026, 5, 1),
            ],
            1_800_00,
        );
        let found = detect(&rent, day(2026, 5, 20)).expect("rent recurs");
        assert_eq!(
            (found.cadence, found.amount, found.last),
            (MONTHLY, Qty(1_800_00), day(2026, 5, 1))
        );
        assert_eq!(found.on, Some(On::MonthDay(1)));
        // Next comes June 1st, then July 1st.
        assert_eq!(
            found
                .schedule()
                .days(day(2026, 5, 20), day(2026, 7, 31))
                .collect::<Vec<_>>(),
            [day(2026, 6, 1), day(2026, 7, 1)]
        );
    }

    #[test]
    fn irregular_or_stale_series_are_not_rhythms() {
        let groceries = series(
            &[
                day(2026, 5, 1),
                day(2026, 5, 6),
                day(2026, 5, 16),
                day(2026, 5, 19),
            ],
            84_20,
        );
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
        assert_eq!((found.cadence, found.amount), (BIWEEKLY, Qty(60_00)));
    }

    #[test]
    fn month_days_clamp_without_drifting() {
        let schedule = Schedule {
            anchor: day(2026, 1, 31),
            every: Span::months(1),
            on: Some(On::MonthDay(31)),
            until: None,
        };
        let days = schedule
            .days(day(2026, 1, 31), day(2026, 4, 30))
            .collect::<Vec<_>>();
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
            mondays
                .days(day(2025, 12, 31), day(2026, 12, 31))
                .collect::<Vec<_>>(),
            [day(2026, 1, 5), day(2026, 1, 12), day(2026, 1, 19)]
        );
        let taxes = Schedule {
            anchor: day(2026, 1, 1),
            every: Span::months(12),
            on: Some(On::YearDay { month: 4, day: 15 }),
            until: None,
        };
        assert_eq!(
            taxes
                .days(day(2026, 6, 1), day(2028, 12, 31))
                .collect::<Vec<_>>(),
            [day(2027, 4, 15), day(2028, 4, 15)]
        );
        assert_eq!(taxes.days(Day::MAX, Day::MAX).next(), None);
    }
}
