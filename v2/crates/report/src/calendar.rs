//! Cutting time into consecutive calendar months or years.

use std::ops::Range;

use axiom_core::Day;
use axiom_core::calendar::Window;
use axiom_model::Period;

/// A run of consecutive windows: the columns of a monthly or yearly report.
#[derive(Clone, Copy, Debug)]
pub struct Periods {
    first: Window,
    count: usize,
}

impl Periods {
    /// The periods that together contain every day from `from` to `to`.
    pub fn covering(grain: Period, from: Day, to: Day) -> Periods {
        let first = Window::containing(grain, from);
        Periods { first, count: (first.steps_to(to) + 1).max(1) as usize }
    }

    /// Only the last `n` periods.
    pub fn last(self, n: usize) -> Periods {
        let dropped = self.count.saturating_sub(n);
        Periods { first: self.first.after(dropped as i32), count: self.count - dropped }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn window(&self, index: usize) -> Window {
        self.first.after(index as i32)
    }

    pub fn end(&self, index: usize) -> Day {
        self.window(index).days().last()
    }

    /// The last day of every period, in order.
    pub fn ends(&self) -> impl Iterator<Item = Day> + '_ {
        (0..self.count).map(|index| self.end(index))
    }

    /// The periods that contain any day from `first` to `last`.
    pub fn overlapping(&self, first: Day, last: Day) -> Range<usize> {
        let lo = self.first.steps_to(first).max(0);
        let hi = (self.first.steps_to(last) + 1).min(self.count as i64);
        lo as usize..hi.max(lo) as usize
    }

    /// The period containing `day`, if it is one of these.
    pub fn index_of(&self, day: Day) -> Option<usize> {
        self.overlapping(day, day).next()
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
        assert_eq!(months.window(3).to_string(), "2026-02");
        assert_eq!(months.end(1), day(2025, 12, 31));
        assert_eq!(months.overlapping(day(2025, 12, 15), day(2026, 1, 15)), 1..3);
        assert_eq!(months.overlapping(day(2024, 1, 1), day(2024, 2, 1)), 0..0);
        assert_eq!(months.last(2).window(0).to_string(), "2026-01");
    }

    #[test]
    fn years_step_by_twelve_months() {
        let years = Periods::covering(Period::Year, day(2024, 6, 1), day(2026, 1, 1));
        assert_eq!(years.len(), 3);
        assert_eq!(years.window(1).days().first(), day(2025, 1, 1));
        assert_eq!(years.overlapping(day(2023, 12, 31), day(2024, 1, 1)), 0..1);
        assert_eq!(years.ends().last(), Some(day(2026, 12, 31)));
    }
}
