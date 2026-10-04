//! Exact numbers.
//!
//! An amount is an `i64` count of its commodity's *quantum*, `10^-scale` of one
//! unit: 0.01 USD, 1 JPY, 1e-8 BTC. Within a commodity, arithmetic is integer
//! arithmetic. Every scaling operation (prices, pro-rata relief, percentages)
//! is a single [`mul_div`] through an exact `i128` intermediate, rounded half to
//! even. `i64 × i64` always fits in `i128`, so no big integers are needed.
//! Nothing here allocates or touches a float.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// A quantity, counted in quanta of some commodity.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Qty(pub i64);

const OVERFLOW: &str = "amount overflow: a running total left ±9.2e18 quanta";

impl Qty {
    pub const ZERO: Qty = Qty(0);

    /// The largest magnitude a single written amount may have. Sums of up to 92
    /// such amounts cannot overflow; real ledgers stay many orders below.
    pub const LIMIT: i64 = 100_000_000_000_000_000;

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn abs(self) -> Qty {
        Qty(self.0.abs())
    }

    /// `self × ratio`, rounded half to even. `None` if the result leaves `i64`.
    pub fn scale(self, ratio: Ratio) -> Option<Qty> {
        mul_div(self.0 as i128, ratio.num as i128, ratio.den as i128).and_then(|v| i64::try_from(v).ok()).map(Qty)
    }

    /// `self × num ÷ den` with the same rounding: the share of `self` that `num`
    /// is of `den`. Used for pro-rata relief, where `den` is a holding's size.
    pub fn share(self, num: Qty, den: Qty) -> Option<Qty> {
        mul_div(self.0 as i128, num.0 as i128, den.0 as i128).and_then(|v| i64::try_from(v).ok()).map(Qty)
    }

    /// Displays with `scale` decimal places and thousands separators:
    /// `-12,000.50`. Every digit is shown, so a column of them aligns.
    pub fn show(self, scale: u8) -> Shown {
        Shown { qty: self, scale, keep: scale }
    }

    /// Displays for prose: like [`Qty::show`], but zeros past the second
    /// decimal are dropped, so `0.50000000 BTC` reads `0.50 BTC`.
    pub fn brief(self, scale: u8) -> Shown {
        Shown { qty: self, scale, keep: scale.min(2) }
    }
}

impl Add for Qty {
    type Output = Qty;
    fn add(self, rhs: Qty) -> Qty {
        Qty(self.0.checked_add(rhs.0).expect(OVERFLOW))
    }
}

impl Sub for Qty {
    type Output = Qty;
    fn sub(self, rhs: Qty) -> Qty {
        Qty(self.0.checked_sub(rhs.0).expect(OVERFLOW))
    }
}

impl AddAssign for Qty {
    fn add_assign(&mut self, rhs: Qty) {
        *self = *self + rhs;
    }
}

impl SubAssign for Qty {
    fn sub_assign(&mut self, rhs: Qty) {
        *self = *self - rhs;
    }
}

impl Neg for Qty {
    type Output = Qty;
    fn neg(self) -> Qty {
        Qty(-self.0)
    }
}

impl std::iter::Sum for Qty {
    fn sum<I: Iterator<Item = Qty>>(iter: I) -> Qty {
        iter.fold(Qty::ZERO, Add::add)
    }
}

/// `a × n ÷ d`, rounded half to even. `None` if `d` is zero or the product
/// overflows `i128`.
pub fn mul_div(a: i128, n: i128, d: i128) -> Option<i128> {
    div_round(a.checked_mul(n)?, d)
}

/// `n ÷ d`, rounded half to even: ties go to the even neighbour, so repeated
/// rounding carries no bias.
pub fn div_round(n: i128, d: i128) -> Option<i128> {
    if d == 0 {
        return None;
    }
    let (q, r) = (n / d, n % d);
    let (twice, whole) = (r.unsigned_abs() * 2, d.unsigned_abs());
    let away = twice > whole || (twice == whole && q & 1 != 0);
    Some(if away { q + n.signum() * d.signum() } else { q })
}

/// Powers of ten that fit in `i128`.
pub const POW10: [i128; 39] = {
    let mut table = [1i128; 39];
    let mut i = 1;
    while i < 39 {
        table[i] = table[i - 1] * 10;
        i += 1;
    }
    table
};

/// An exact rational in lowest terms with a positive denominator.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ratio {
    num: i64,
    den: i64,
}

impl Ratio {
    pub const ZERO: Ratio = Ratio { num: 0, den: 1 };
    pub const ONE: Ratio = Ratio { num: 1, den: 1 };

    /// Reduces `num / den`. `None` for a zero denominator or terms beyond `i64`.
    pub fn new(num: i128, den: i128) -> Option<Ratio> {
        if den == 0 {
            return None;
        }
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i128;
        let sign = den.signum();
        Some(Ratio { num: i64::try_from(sign * num / g).ok()?, den: i64::try_from(sign * den / g).ok()? })
    }

    pub const fn int(n: i64) -> Ratio {
        Ratio { num: n, den: 1 }
    }

    /// `pct / 100`: `Ratio::percent(35, 1)` is 3.5%.
    pub fn percent(mantissa: i128, scale: u8) -> Option<Ratio> {
        Ratio::new(mantissa, 100 * *POW10.get(scale as usize)?)
    }

    pub fn num(self) -> i64 {
        self.num
    }

    pub fn den(self) -> i64 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn is_negative(self) -> bool {
        self.num < 0
    }

    pub fn is_integer(self) -> bool {
        self.den == 1
    }

    pub fn abs(self) -> Ratio {
        Ratio { num: self.num.abs(), den: self.den }
    }

    pub fn recip(self) -> Option<Ratio> {
        Ratio::new(self.den as i128, self.num as i128)
    }

    pub fn checked_add(self, o: Ratio) -> Option<Ratio> {
        let (a, b) = (self.wide(), o.wide());
        Ratio::new(a.0 * b.1 + b.0 * a.1, a.1 * b.1)
    }

    pub fn checked_sub(self, o: Ratio) -> Option<Ratio> {
        self.checked_add(-o)
    }

    pub fn checked_mul(self, o: Ratio) -> Option<Ratio> {
        let (a, b) = (self.wide(), o.wide());
        Ratio::new(a.0 * b.0, a.1 * b.1)
    }

    pub fn checked_div(self, o: Ratio) -> Option<Ratio> {
        self.checked_mul(o.recip()?)
    }

    /// Rounds to the nearest integer, half to even.
    pub fn round(self) -> i128 {
        div_round(self.num as i128, self.den as i128).expect("denominator is positive")
    }

    fn wide(self) -> (i128, i128) {
        (self.num as i128, self.den as i128)
    }
}

impl Neg for Ratio {
    type Output = Ratio;
    fn neg(self) -> Ratio {
        Ratio { num: -self.num, den: self.den }
    }
}

impl Neg for Dec {
    type Output = Dec;
    fn neg(self) -> Dec {
        Dec { mantissa: -self.mantissa, scale: self.scale }
    }
}

impl Ord for Ratio {
    fn cmp(&self, o: &Ratio) -> Ordering {
        (self.num as i128 * o.den as i128).cmp(&(o.num as i128 * self.den as i128))
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, o: &Ratio) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl fmt::Display for Ratio {
    /// Terminating decimals print as decimals (`0.035`), others as `n/d`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (mut twos, mut fives, mut rest) = (0u8, 0u8, self.den);
        while rest % 2 == 0 {
            rest /= 2;
            twos += 1;
        }
        while rest % 5 == 0 {
            rest /= 5;
            fives += 1;
        }
        let places = twos.max(fives);
        match (rest, POW10.get(places as usize)) {
            (1, Some(&p)) if places <= 18 => {
                let mantissa = self.num as i128 * p / self.den as i128;
                write!(f, "{}", Shown128 { value: mantissa, scale: places, group: false })
            }
            _ => write!(f, "{}/{}", self.num, self.den),
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// A decimal literal exactly as written: `mantissa × 10^-scale`.
///
/// Eighteen significant digits fit, more than any amount may have
/// ([`Qty::LIMIT`]), and the whole literal is 16 bytes, so syntax trees full
/// of amounts stay small.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Dec {
    pub mantissa: i64,
    pub scale: u8,
}

/// Why a written decimal cannot become an amount.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecError {
    /// More decimal places than the commodity's precision.
    Inexact,
    /// Beyond [`Qty::LIMIT`] quanta.
    Range,
}

impl Dec {
    pub const ZERO: Dec = Dec { mantissa: 0, scale: 0 };

    /// Parses `digits[.digits]`, ignoring `_` separators. `None` for anything
    /// else, or for more digits than a mantissa holds.
    pub fn parse(text: &[u8]) -> Option<Dec> {
        match Dec::prefix(text) {
            Some((dec, len)) if len == text.len() => Some(dec),
            _ => Dec::separated(text),
        }
    }

    /// The number a text starts with, if it is the commonest kind: digits and
    /// perhaps a point and more digits (`84.20`), with no separators. Also how
    /// many bytes it takes. Eighteen digits cannot overflow, so nothing is
    /// checked; a longer run is left to [`Dec::parse`].
    pub fn prefix(text: &[u8]) -> Option<(Dec, usize)> {
        let (whole, mantissa) = digit_run(text, 0, 0);
        if whole == 0 {
            return None;
        }
        let (end, mantissa) = match text[whole..] {
            [b'.', next, ..] if next.is_ascii_digit() => digit_run(text, whole + 1, mantissa),
            _ => (whole, mantissa),
        };
        let point = usize::from(end > whole);
        (end - point <= 18).then_some((Dec { mantissa, scale: end.saturating_sub(whole + 1) as u8 }, end))
    }

    /// Any number: `_` separators, long runs, and everything that is not one.
    /// Eight-digit runs are folded in one SWAR step; the rest goes a digit at a
    /// time, checking for overflow.
    fn separated(text: &[u8]) -> Option<Dec> {
        let (mut mantissa, mut scale, mut seen, mut dot) = (0i64, 0u8, false, false);
        let mut rest = text;
        while let Some((&b, tail)) = rest.split_first() {
            if let Some(eight) = rest.first_chunk::<8>().filter(|c| all_digits(c)) {
                mantissa = mantissa.checked_mul(100_000_000)?.checked_add(digits8(eight) as i64)?;
                scale = scale.checked_add(if dot { 8 } else { 0 })?;
                (seen, rest) = (true, &rest[8..]);
                continue;
            }
            match b {
                b'0'..=b'9' => {
                    mantissa = mantissa.checked_mul(10)?.checked_add((b - b'0') as i64)?;
                    scale = scale.checked_add(dot as u8)?;
                    seen = true;
                }
                b'_' if seen && !dot => {}
                b'.' if seen && !dot && tail.first().is_some_and(u8::is_ascii_digit) => dot = true,
                _ => return None,
            }
            rest = tail;
        }
        // Every power of ten a scale may need must fit the `i128` table.
        (seen && (scale as usize) < POW10.len()).then_some(Dec { mantissa, scale })
    }

    pub fn is_zero(self) -> bool {
        self.mantissa == 0
    }

    /// Decimal places actually needed: `2.50` needs one.
    pub fn places(self) -> u8 {
        let (mut m, mut s) = (self.mantissa, self.scale);
        while s > 0 && m % 10 == 0 {
            m /= 10;
            s -= 1;
        }
        s
    }

    /// This value in quanta of a commodity with `scale` decimal places.
    pub fn to_qty(self, scale: u8) -> Result<Qty, DecError> {
        // Amounts are nearly always written to the commodity's own precision.
        if scale == self.scale {
            let fits = self.mantissa.unsigned_abs() <= Qty::LIMIT as u64;
            return if fits { Ok(Qty(self.mantissa)) } else { Err(DecError::Range) };
        }
        let mantissa = self.mantissa as i128;
        let value = if scale >= self.scale {
            let p = POW10.get((scale - self.scale) as usize).ok_or(DecError::Range)?;
            mantissa.checked_mul(*p).ok_or(DecError::Range)?
        } else {
            let p = POW10[(self.scale - scale) as usize];
            if mantissa % p != 0 {
                return Err(DecError::Inexact);
            }
            mantissa / p
        };
        match i64::try_from(value) {
            Ok(v) if v.abs() <= Qty::LIMIT => Ok(Qty(v)),
            _ => Err(DecError::Range),
        }
    }

    pub fn to_ratio(self) -> Option<Ratio> {
        Ratio::new(self.mantissa as i128, *POW10.get(self.scale as usize)?)
    }
}

/// Where the run of digits starting at `from` ends, and `mantissa` with those
/// digits appended (wrapping: the caller refuses runs too long to fit).
fn digit_run(text: &[u8], from: usize, mut mantissa: i64) -> (usize, i64) {
    let mut at = from;
    while let Some(&b) = text.get(at).filter(|b| b.is_ascii_digit()) {
        mantissa = mantissa.wrapping_mul(10).wrapping_add((b - b'0') as i64);
        at += 1;
    }
    (at, mantissa)
}

pub(crate) fn all_digits(chunk: &[u8; 8]) -> bool {
    let w = u64::from_le_bytes(*chunk);
    const HIGH: u64 = 0xF0F0_F0F0_F0F0_F0F0;
    const ZEROS: u64 = 0x3030_3030_3030_3030;
    // Every high nibble is 3, and adding 6 to each byte does not carry it to 4.
    w & HIGH == ZEROS && w.wrapping_add(0x0606_0606_0606_0606) & HIGH == ZEROS
}

/// Eight ASCII digits folded into four two-digit values, one in each even byte,
/// the first pair lowest. One multiply; the digits must already be checked.
pub(crate) fn digit_pairs(chunk: &[u8; 8]) -> u64 {
    let w = u64::from_le_bytes(*chunk) - 0x3030_3030_3030_3030;
    (w.wrapping_mul(10) + (w >> 8)) & 0x00FF_00FF_00FF_00FF
}

/// Folds eight ASCII digits into their value: the pairs, then quads, then the
/// whole word. The first digit sits in the lowest byte.
fn digits8(chunk: &[u8; 8]) -> u32 {
    let w = digit_pairs(chunk);
    let w = (w.wrapping_mul(100) + (w >> 16)) & 0x0000_FFFF_0000_FFFF;
    (w.wrapping_mul(10_000) + (w >> 32)) as u32
}

/// A quantity ready to print: `-12,345.60`.
#[derive(Clone, Copy)]
pub struct Shown {
    qty: Qty,
    scale: u8,
    /// Decimal places always shown; trailing zeros beyond them are dropped.
    keep: u8,
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut value = self.qty.0 as i128;
        let mut scale = self.scale;
        while scale > self.keep && value % 10 == 0 {
            (value, scale) = (value / 10, scale - 1);
        }
        f.pad(&Shown128 { value, scale, group: true }.to_string())
    }
}

struct Shown128 {
    value: i128,
    scale: u8,
    group: bool,
}

impl fmt::Display for Shown128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = POW10[self.scale as usize] as u128;
        let (int, frac) = (self.value.unsigned_abs() / p, self.value.unsigned_abs() % p);
        // Integer digits right to left, a comma before every third.
        let (mut buf, mut at, mut n, mut digits) = ([0u8; 64], 64, int, 0);
        loop {
            if self.group && digits > 0 && digits % 3 == 0 {
                at -= 1;
                buf[at] = b',';
            }
            at -= 1;
            buf[at] = b'0' + (n % 10) as u8;
            (n, digits) = (n / 10, digits + 1);
            if n == 0 {
                break;
            }
        }
        if self.value < 0 {
            at -= 1;
            buf[at] = b'-';
        }
        f.write_str(std::str::from_utf8(&buf[at..]).expect("ascii digits"))?;
        if self.scale > 0 {
            write!(f, ".{:0width$}", frac, width = self.scale as usize)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_is_half_even() {
        assert_eq!(div_round(5, 2), Some(2));
        assert_eq!(div_round(7, 2), Some(4));
        assert_eq!(div_round(-5, 2), Some(-2));
        assert_eq!(div_round(-7, 2), Some(-4));
        assert_eq!(div_round(10, 3), Some(3));
        assert_eq!(div_round(-10, 3), Some(-3));
        assert_eq!(div_round(11, -3), Some(-4));
    }

    #[test]
    fn decimals_parse_and_scale() {
        let d = Dec::parse(b"24_500.50").unwrap();
        assert_eq!(d, Dec { mantissa: 2450050, scale: 2 });
        assert_eq!(d.to_qty(2), Ok(Qty(2450050)));
        assert_eq!(d.to_qty(4), Ok(Qty(245005000)));
        assert_eq!(d.to_qty(0), Err(DecError::Inexact));
        assert_eq!(Dec::parse(b"123456789012.5").unwrap().mantissa, 1234567890125);
        assert!(Dec::parse(b"1.").is_none() && Dec::parse(b"_1").is_none());
        assert_eq!(Dec::parse(b"9_223_372_036_854_775_807").unwrap().mantissa, i64::MAX);
        assert!(Dec::parse(b"9_223_372_036_854_775_808").is_none(), "one past i64 is refused, not wrapped");
        assert_eq!(std::mem::size_of::<Dec>(), 16);
        let tiny = format!("0.{}1", "0".repeat(300));
        assert!(Dec::parse(tiny.as_bytes()).is_none(), "a scale past the table is refused");
        assert_eq!(Dec::parse(b"2.50").unwrap().places(), 1);
    }

    #[test]
    fn display_groups_thousands() {
        assert_eq!(Qty(-1234567).show(2).to_string(), "-12,345.67");
        assert_eq!(Qty(5).show(2).to_string(), "0.05");
        assert_eq!(Qty(1000).show(0).to_string(), "1,000");
        assert_eq!(Qty(50_000_000).brief(8).to_string(), "0.50");
        assert_eq!(Qty(12_345_678).brief(8).to_string(), "0.12345678");
        assert_eq!(Qty(1_250).brief(2).to_string(), "12.50");
        assert_eq!(Ratio::percent(35, 1).unwrap().to_string(), "0.035");
        assert_eq!(Ratio::new(1, 3).unwrap().to_string(), "1/3");
    }
}
