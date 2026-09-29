//! Spreading an amount over days without losing a quantum.

use axiom_core::num::mul_div;
use axiom_core::{Day, Qty};

/// A total recognized linearly over `first..=last`, one equal share per day.
///
/// Recognized-so-far is rounded once per day boundary and a period's share is
/// the difference of two such values, so the shares of consecutive periods
/// telescope: they always add up to the whole.
#[derive(Clone, Copy, Debug)]
pub struct Apportion {
    total: Qty,
    first: Day,
    days: i64,
}

impl Apportion {
    pub fn new(total: Qty, first: Day, last: Day) -> Apportion {
        Apportion { total, first, days: i64::from((last.0 - first.0).max(0)) + 1 }
    }

    /// What has been recognized by the end of `day`.
    pub fn through(&self, day: Day) -> Qty {
        let elapsed = i64::from(day.0 - self.first.0 + 1).clamp(0, self.days);
        let share = mul_div(self.total.0.into(), elapsed.into(), self.days.into()).expect("i64 × i64 fits i128");
        Qty(i64::try_from(share).expect("a share never exceeds the total"))
    }

    /// What is recognized on the days from `from` to `to`, inclusive.
    pub fn between(&self, from: Day, to: Day) -> Qty {
        self.through(to) - self.through(from.add_days(-1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn shares_add_up_to_the_whole() {
        // A year's premium that does not divide by 365, cut at month ends.
        let (first, last) = (day(2026, 1, 1), day(2026, 12, 31));
        let premium = Apportion::new(Qty(120_001), first, last);
        let mut recognized = Qty::ZERO;
        for month in 1..=12 {
            let start = day(2026, month, 1);
            let end = start.month_end();
            recognized += premium.between(start, end);
        }
        assert_eq!(recognized, Qty(120_001));
    }

    #[test]
    fn a_window_inside_the_range_gets_its_days() {
        let premium = Apportion::new(Qty(36_500), day(2026, 1, 1), day(2026, 12, 31));
        assert_eq!(premium.between(day(2026, 1, 1), day(2026, 1, 31)), Qty(3_100));
        assert_eq!(premium.between(day(2025, 1, 1), day(2025, 12, 31)), Qty::ZERO);
        assert_eq!(premium.between(day(2026, 12, 1), day(2027, 6, 1)), Qty(3_100));
    }

    #[test]
    fn a_single_day_is_recognized_at_once() {
        let once = Apportion::new(Qty(-8_420), day(2026, 1, 18), day(2026, 1, 18));
        assert_eq!(once.between(day(2026, 1, 18), day(2026, 1, 18)), Qty(-8_420));
    }
}
