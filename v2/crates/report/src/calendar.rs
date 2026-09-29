//! Cutting time into consecutive calendar months or years.

use std::ops::Range;

use axiom_core::{Day, Span};
use axiom_model::Period;

/// A run of consecutive periods: the columns of a monthly or yearly report.
#[derive(Clone, Copy, Debug)]
pub struct Periods {
    grain: Period,
    /// The first day of the first period.
    first: Day,
    count: usize,
}

impl Periods {
    /// The periods that together contain every day from `from` to `to`.
    pub fn covering(grain: Period, from: Day, to: Day) -> Periods {
        let first = match grain {
            Period::Month => from.month_start(),
            Period::Year => from.year_start(),
        };
        let mut periods = Periods { grain, first, count: 1 };
        periods.count = (periods.position(to) + 1).max(1) as usize;
        periods
    }

    /// Only the last `n` periods.
    pub fn last(self, n: usize) -> Periods {
        let dropped = self.count.saturating_sub(n);
        Periods { first: self.start(dropped), count: self.count - dropped, ..self }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn start(&self, index: usize) -> Day {
        self.first.add(Span::months(self.months() * index as i32))
    }

    pub fn end(&self, index: usize) -> Day {
        self.start(index + 1).add_days(-1)
    }

    /// The last day of every period, in order.
    pub fn ends(&self) -> impl Iterator<Item = Day> + '_ {
        (0..self.count).map(|index| self.end(index))
    }

    /// The periods that contain any day from `first` to `last`.
    pub fn overlapping(&self, first: Day, last: Day) -> Range<usize> {
        let lo = self.position(first).max(0);
        let hi = (self.position(last) + 1).min(self.count as i64);
        lo as usize..hi.max(lo) as usize
    }

    /// The period containing `day`, if it is one of these.
    pub fn index_of(&self, day: Day) -> Option<usize> {
        self.overlapping(day, day).next()
    }

    /// `2026-03` for a month, `2026` for a year.
    pub fn title(&self, index: usize) -> String {
        let (year, month, _) = self.start(index).ymd();
        match self.grain {
            Period::Month => format!("{year:04}-{month:02}"),
            Period::Year => format!("{year:04}"),
        }
    }

    fn months(&self) -> i32 {
        match self.grain {
            Period::Month => 1,
            Period::Year => 12,
        }
    }

    /// Which period `day` falls in, counting from the first; negative before it.
    fn position(&self, day: Day) -> i64 {
        let months = |day: Day| {
            let (year, month, _) = day.ymd();
            i64::from(year) * 12 + i64::from(month) - 1
        };
        (months(day) - months(self.first)).div_euclid(self.months().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn periods_cover_and_overlap() {
        let months = Periods::covering(Period::Month, day(2025, 11, 20), day(2026, 2, 3));
        assert_eq!(months.len(), 4);
        assert_eq!(months.title(3), "2026-02");
        assert_eq!(months.end(1), day(2025, 12, 31));
        assert_eq!(months.overlapping(day(2025, 12, 15), day(2026, 1, 15)), 1..3);
        assert_eq!(months.overlapping(day(2024, 1, 1), day(2024, 2, 1)), 0..0);
        assert_eq!(months.last(2).title(0), "2026-01");
    }

    #[test]
    fn years_step_by_twelve_months() {
        let years = Periods::covering(Period::Year, day(2024, 6, 1), day(2026, 1, 1));
        assert_eq!(years.len(), 3);
        assert_eq!(years.start(1), day(2025, 1, 1));
        assert_eq!(years.overlapping(day(2023, 12, 31), day(2024, 1, 1)), 0..1);
        assert_eq!(years.ends().last(), Some(day(2026, 12, 31)));
    }
}
