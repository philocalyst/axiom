//! Calendar days.
//!
//! A [`Day`] counts days since 1970-01-01. Converting to and from the civil
//! calendar uses Ben Joffe's algorithms (<https://www.benjoffe.com/fast-date-64>,
//! Boost Software License 1.0, © 2025 Ben Joffe): the Gregorian cycle is
//! unfolded by a handful of multiply-shifts instead of divisions. The weekday is
//! one multiplication (<https://www.benjoffe.com/fast-day-of-week>). Dates are
//! parsed eight bytes at a time.

use std::fmt;

/// Days since 1970-01-01.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Day(pub i32);

// Joffe's constants for the full 32-bit range. Counting runs backwards from a
// far-future epoch so every intermediate stays positive.
const ERAS: u64 = 14_704;
const D_SHIFT: u64 = 146_097 * ERAS - 719_469;
const Y_SHIFT: u32 = (400 * ERAS - 1) as u32;
const SCALE: u32 = 32;
const SHIFT_0: u32 = 30_556 * SCALE;
const SHIFT_1: u32 = 5_980 * SCALE;
const C1: u64 = 505_054_698_555_331; // ⌊2^64·4 / 146097⌋: divides by days per century
const C2: u64 = 50_504_432_782_230_121; // ⌈2^64·4 / 1461⌉: divides by days per year
const C3: u64 = 8_619_973_866_219_416 * 32 / SCALE as u64; // ⌊2^64 / 2140⌋

impl Day {
    /// The day for a civil date, if it exists.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Day> {
        (-999_999..=999_999).contains(&year).then_some(())?;
        (1..=12).contains(&month).then_some(())?;
        (1..=days_in_month(year, month)).contains(&day).then_some(())?;
        // Joffe's overflow-safe inverse: March-based years, then a month table
        // folded into one multiply-shift.
        let bump = (month <= 2) as u32;
        let years = (year + 5_880_000) as u32 - bump;
        let centuries = years / 100;
        let shift: i32 = if bump == 1 { 8_829 } else { -2_919 };
        let year_days = years * 365 + years / 4 - centuries + centuries / 4;
        let month_days = (979 * month as i32 + shift) as u32 / 32;
        Some(Day(year_days.wrapping_add(month_days + day).wrapping_sub(2_148_345_369) as i32))
    }

    /// The civil date: `(year, month 1–12, day 1–31)`.
    pub fn ymd(self) -> (i32, u32, u32) {
        // 1. Reverse the count, then apply the 100/400-year rule by mapping onto
        //    a Julian calendar (the "Julian map").
        let rev = (D_SHIFT as i64 - self.0 as i64) as u64;
        let cen = ((C1 as u128 * rev as u128) >> 64) as u64;
        let jul = rev - cen / 4 + cen;
        // 2. Years and the fraction of the year, from one 128-bit product.
        let num = C2 as u128 * jul as u128;
        let years = Y_SHIFT.wrapping_sub((num >> 64) as u32);
        let part = ((24_451 * SCALE) as u128 * (num as u64) as u128 >> 64) as u32;
        // 3. January and February belong to the next civil year.
        let bump = (part < 3_952 * SCALE) as u32;
        let shift = if bump == 1 { SHIFT_1 } else { SHIFT_0 };
        // 4. Month and day from one numerator, corrected for leap years by
        //    shifting on the year modulo four.
        let n = (years % 4) * (16 * SCALE) + shift - part;
        let month = n / (2_048 * SCALE);
        let day = ((C3 as u128 * (n % (2_048 * SCALE)) as u128) >> 64) as u32 + 1;
        (years.wrapping_add(bump) as i32, month, day)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }

    /// Monday = 0 … Sunday = 6. One multiply: `rd × ⌊2^32/7⌋` places the
    /// remainder mod 7 in the top three bits, with two shifts correcting the
    /// truncated constant. Joffe's full-range `get_weekday_32unix` (MIT) yields
    /// Sunday = 0; a table rotates it.
    pub fn weekday(self) -> u32 {
        const M: u32 = ((1u64 << 32) / 7) as u32;
        const Z: u32 = 0x9500_0000;
        const MONDAY_FIRST: [u32; 8] = [6, 0, 1, 2, 3, 4, 5, 0];
        let rd = self.0;
        let a = (rd as u32).wrapping_mul(M).wrapping_add(Z);
        let b = ((rd >> 1) + (rd >> 4)) as u32;
        MONDAY_FIRST[(a.wrapping_add(b) >> 29) as usize]
    }

    pub fn add_days(self, n: i32) -> Day {
        Day(self.0 + n)
    }

    /// Adds whole months (clamping to month end, so Jan 31 + 1m = Feb 28),
    /// then days.
    pub fn add(self, span: Span) -> Day {
        let (y, m, d) = self.ymd();
        let months = y as i64 * 12 + (m as i64 - 1) + span.months as i64;
        let (y, m) = (months.div_euclid(12) as i32, months.rem_euclid(12) as u32 + 1);
        let clamped = Day::from_ymd(y, m, d.min(days_in_month(y, m))).expect("clamped date exists");
        clamped.add_days(span.days)
    }

    /// The calendar span from `earlier` to `self`: whole months, then days.
    pub fn since(self, earlier: Day) -> Span {
        let ((y1, m1, d1), (y2, m2, d2)) = (earlier.ymd(), self.ymd());
        let mut months = (y2 - y1) * 12 + m2 as i32 - m1 as i32;
        if d2 < d1 {
            months -= 1;
        }
        let days = self.0 - earlier.add(Span::months(months)).0;
        Span { months, days }
    }

    pub fn month_start(self) -> Day {
        let (y, m, _) = self.ymd();
        Day::from_ymd(y, m, 1).expect("first of month")
    }

    pub fn month_end(self) -> Day {
        let (y, m, _) = self.ymd();
        Day::from_ymd(y, m, days_in_month(y, m)).expect("last of month")
    }

    pub fn year_start(self) -> Day {
        Day::from_ymd(self.year(), 1, 1).expect("new year")
    }

    pub fn year_end(self) -> Day {
        Day::from_ymd(self.year(), 12, 31).expect("new year's eve")
    }

    /// Parses `YYYY-MM-DD`. The eight digits are gathered into one word,
    /// validated together, and folded into two-digit fields with a single
    /// multiply.
    pub fn parse(text: &[u8]) -> Option<Day> {
        let s: &[u8; 10] = text.try_into().ok()?;
        if s[4] != b'-' || s[7] != b'-' {
            return None;
        }
        let w = u64::from_le_bytes([s[0], s[1], s[2], s[3], s[5], s[6], s[8], s[9]]);
        const HIGH: u64 = 0xF0F0_F0F0_F0F0_F0F0;
        const ZEROS: u64 = 0x3030_3030_3030_3030;
        if w & HIGH != ZEROS || w.wrapping_add(0x0606_0606_0606_0606) & HIGH != ZEROS {
            return None;
        }
        // Each byte is a digit; `w×10 + w>>8` leaves the pairs YY YY MM DD in
        // the even bytes.
        let w = w - ZEROS;
        let w = (w.wrapping_mul(10) + (w >> 8)) & 0x00FF_00FF_00FF_00FF;
        let year = (w & 0xFF) * 100 + (w >> 16 & 0xFF);
        Day::from_ymd(year as i32, (w >> 32 & 0xFF) as u32, (w >> 48) as u32)
    }
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

pub fn is_leap(year: i32) -> bool {
    // Divisible by 4, and by 400 when divisible by 100. For multiples of 100,
    // "divisible by 400" is the same as "divisible by 16".
    let mask = if year % 100 == 0 { 15 } else { 3 };
    year & mask == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 2 {
        28 + is_leap(year) as u32
    } else {
        // 31 for odd months through July, for even months from August.
        30 | (month ^ month >> 3)
    }
}

/// A calendar duration: whole months plus days. `59y6m` is 714 months;
/// `2w` is 14 days. Spans compare months first, which is exact for spans
/// produced by [`Day::since`], whose days never fill a month.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Span {
    pub months: i32,
    pub days: i32,
}

impl Span {
    pub const fn months(months: i32) -> Span {
        Span { months, days: 0 }
    }

    pub const fn days(days: i32) -> Span {
        Span { months: 0, days }
    }

    /// Parses `(digits [ymwd])+`: `59y6m`, `2w`, `60d`.
    pub fn parse(text: &[u8]) -> Option<Span> {
        let (mut span, mut n, mut digits) = (Span::default(), 0i32, false);
        for &b in text {
            match b {
                b'0'..=b'9' => {
                    n = n.checked_mul(10)?.checked_add((b - b'0') as i32)?;
                    digits = true;
                }
                b'y' | b'm' | b'w' | b'd' if digits => {
                    match b {
                        b'y' => span.months += n.checked_mul(12)?,
                        b'm' => span.months += n,
                        b'w' => span.days += n.checked_mul(7)?,
                        _ => span.days += n,
                    }
                    (n, digits) = (0, false);
                }
                _ => return None,
            }
        }
        (!digits && !text.is_empty()).then_some(span)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (years, months) = (self.months / 12, self.months % 12);
        let parts = [(years, "y"), (months, "m"), (self.days, "d")];
        let mut wrote = false;
        for (n, unit) in parts.into_iter().filter(|&(n, _)| n != 0) {
            write!(f, "{n}{unit}")?;
            wrote = true;
        }
        if !wrote {
            f.write_str("0d")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Howard Hinnant's `civil_from_days`, the classic reference.
    fn reference(z: i64) -> (i32, u32, u32) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        ((yoe + era * 400 + (m <= 2) as i64) as i32, m as u32, d as u32)
    }

    #[test]
    fn joffe_matches_reference_across_ten_millennia() {
        for z in -2_000_000..=2_000_000 {
            let (y, m, d) = Day(z).ymd();
            assert_eq!((y, m, d), reference(z as i64), "day {z}");
            assert_eq!(Day::from_ymd(y, m, d), Some(Day(z)), "{y}-{m}-{d}");
            assert_eq!(Day(z).weekday() as i64, (z as i64 + 3).rem_euclid(7), "weekday of {z}");
        }
    }

    #[test]
    fn parsing_is_strict() {
        assert_eq!(Day::parse(b"1970-01-01"), Some(Day(0)));
        assert_eq!(Day::parse(b"2024-02-29").map(|d| d.to_string()).as_deref(), Some("2024-02-29"));
        for bad in [&b"2023-02-29"[..], b"2026-13-01", b"2026-00-10", b"2026/01/01", b"2026-1-011", b"20a6-01-01", b"2026-01-32"] {
            assert_eq!(Day::parse(bad), None, "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn calendar_arithmetic() {
        let jan31 = Day::parse(b"2026-01-31").unwrap();
        assert_eq!(jan31.add(Span::months(1)).to_string(), "2026-02-28");
        let born = Day::parse(b"1966-08-15").unwrap();
        let age = Day::parse(b"2026-02-14").unwrap().since(born);
        assert_eq!(age, Span { months: 59 * 12 + 5, days: 30 });
        assert!(age < Span::parse(b"59y6m").unwrap());
        assert!(Day::parse(b"2026-02-15").unwrap().since(born) >= Span::parse(b"59y6m").unwrap());
        assert_eq!(Span::parse(b"2w"), Some(Span::days(14)));
        assert_eq!(days_in_month(2026, 7), 31);
        assert_eq!(days_in_month(2026, 8), 31);
        assert_eq!(days_in_month(2026, 9), 30);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
    }
}
