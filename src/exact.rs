//! Exact numeric values used by the ledger.
//!
//! The ledger never needs an approximation.  `ExactNumber` therefore keeps
//! integers, source-spelled finite decimals, and normalized rationals as
//! separate values.  Decimal scale is not cosmetic: `10.00` is useful
//! evidence even though it compares equal to `10`.

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::{Add, Mul, Neg, Sub};
use std::str::FromStr;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

/// The rounding rule used by an explicitly requested decimal conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundingMode {
    /// Round to the nearest value; ties go to the even last digit.
    HalfEven,
    /// Round to the nearest value; ties go away from zero.
    HalfAwayFromZero,
    /// Discard digits after the requested scale.
    TowardZero,
    /// Round towards positive infinity.
    TowardPositiveInfinity,
    /// Round towards negative infinity.
    TowardNegativeInfinity,
}

/// An arbitrary precision exact number.
///
/// Decimal values retain their source scale.  Rational values are reduced,
/// have a positive denominator, and are the result of operations which cannot
/// be represented exactly as a finite decimal at the requested scale.
#[derive(Clone, Debug)]
pub enum ExactNumber {
    /// An integer with no decimal point in its source representation.
    Integer(BigInt),
    /// `coefficient / 10^scale`; `scale` includes trailing source zeroes.
    Decimal { coefficient: BigInt, scale: u32 },
    /// A reduced fraction with a positive denominator.
    Rational {
        numerator: BigInt,
        denominator: BigInt,
    },
}

impl PartialEq for ExactNumber {
    fn eq(&self, rhs: &Self) -> bool {
        self.as_rational() == rhs.as_rational()
    }
}

impl Eq for ExactNumber {}

impl Hash for ExactNumber {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the normalized rational, not the source scale.  This keeps the
        // Hash/Eq contract while Display still retains that source scale.
        let rational = self.as_rational();
        rational.numer().hash(state);
        rational.denom().hash(state);
    }
}

/// The short name is pleasant at the domain boundary and keeps call sites
/// from suggesting that all values are decimal floating point.
pub type Exact = ExactNumber;

impl ExactNumber {
    pub fn integer(value: impl Into<BigInt>) -> Self {
        Self::Integer(value.into())
    }

    pub fn decimal(coefficient: impl Into<BigInt>, scale: u32) -> Self {
        let coefficient = coefficient.into();
        if scale == 0 {
            Self::Integer(coefficient)
        } else {
            Self::Decimal { coefficient, scale }
        }
    }

    pub fn rational(
        numerator: impl Into<BigInt>,
        denominator: impl Into<BigInt>,
    ) -> Result<Self, ExactError> {
        Self::from_rational(numerator.into(), denominator.into())
    }

    pub fn parse(source: &str) -> Result<Self, ExactError> {
        source.parse()
    }

    /// Returns the normalized rational interpretation of this number.
    pub fn as_rational(&self) -> BigRational {
        match self {
            Self::Integer(value) => BigRational::from_integer(value.clone()),
            Self::Decimal { coefficient, scale } => {
                BigRational::new(coefficient.clone(), ten_pow(*scale))
            }
            Self::Rational {
                numerator,
                denominator,
            } => BigRational::new(numerator.clone(), denominator.clone()),
        }
    }

    /// Alias useful at arithmetic boundaries.
    pub fn to_rational(&self) -> BigRational {
        self.as_rational()
    }

    /// Returns a canonical decimal when the rational has a finite decimal
    /// expansion.  The returned scale is minimal; source scale belongs to the
    /// original value and is intentionally not invented here.
    pub fn finite_decimal(&self) -> Option<Self> {
        let rational = self.as_rational();
        let denominator = rational.denom();
        let mut remainder = denominator.clone();
        let mut twos = 0u32;
        let mut fives = 0u32;
        while (&remainder % 2u8).is_zero() {
            remainder /= 2u8;
            twos += 1;
        }
        while (&remainder % 5u8).is_zero() {
            remainder /= 5u8;
            fives += 1;
        }
        if remainder != BigInt::one() {
            return None;
        }
        let scale = twos.max(fives);
        let coefficient = rational.numer() * ten_pow(scale) / denominator;
        Some(Self::decimal(coefficient, scale))
    }

    /// Returns the decimal scale carried by this value, if it is a decimal.
    pub fn scale(&self) -> Option<u32> {
        match self {
            Self::Decimal { scale, .. } => Some(*scale),
            _ => None,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.as_rational().is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.as_rational().is_negative()
    }

    pub fn abs(&self) -> Self {
        match self {
            Self::Integer(value) => Self::Integer(value.abs()),
            Self::Decimal { coefficient, scale } => Self::Decimal {
                coefficient: coefficient.abs(),
                scale: *scale,
            },
            Self::Rational {
                numerator,
                denominator,
            } => Self::Rational {
                numerator: numerator.abs(),
                denominator: denominator.clone(),
            },
        }
    }

    /// Canonical representation for hashes and stable machine output.
    /// Unlike [`Display`], this removes decimal trailing zeroes.
    pub fn canonical_string(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Decimal { coefficient, scale } => {
                let rational = BigRational::new(coefficient.clone(), ten_pow(*scale));
                canonical_rational_string(&rational)
            }
            Self::Rational {
                numerator,
                denominator,
            } => {
                canonical_rational_string(&BigRational::new(numerator.clone(), denominator.clone()))
            }
        }
    }

    /// Add two exact values.  Decimal operands retain the more precise source
    /// scale whenever their result is still a finite decimal.
    pub fn checked_add(&self, rhs: &Self) -> Self {
        self.add_rational(rhs, |a, b| a + b)
    }

    pub fn checked_sub(&self, rhs: &Self) -> Self {
        self.add_rational(rhs, |a, b| a - b)
    }

    pub fn checked_mul(&self, rhs: &Self) -> Self {
        let value = self.as_rational() * rhs.as_rational();
        self.prefer_decimal(
            rhs,
            value,
            self.scale()
                .unwrap_or(0)
                .saturating_add(rhs.scale().unwrap_or(0)),
        )
    }

    pub fn checked_div(&self, rhs: &Self) -> Result<Self, ExactError> {
        if rhs.is_zero() {
            return Err(ExactError::DivisionByZero);
        }
        let value = self.as_rational() / rhs.as_rational();
        // Division is a rational operation.  Keeping `1 / 2` visibly as a
        // ratio matters when the result is later used as an allocation proof;
        // callers can request a decimal explicitly with `finite_decimal` or
        // `round`.
        Ok(Self::from_rational_value(value))
    }

    /// Round to an exact decimal scale.  Rounding is explicit so it cannot
    /// accidentally enter a proof or balance calculation.
    pub fn round(&self, scale: u32, mode: RoundingMode) -> Result<Self, ExactError> {
        let denominator = ten_pow(scale);
        let scaled = self.as_rational() * BigRational::from_integer(denominator.clone());
        let quotient = scaled.numer() / scaled.denom();
        let remainder = scaled.numer() % scaled.denom();
        if remainder.is_zero() {
            return Ok(Self::decimal(quotient, scale));
        }
        let sign = if scaled.is_negative() { -1 } else { 1 };
        let abs_remainder = remainder.abs();
        let abs_denominator = scaled.denom().abs();
        let increment = match mode {
            RoundingMode::TowardZero => false,
            RoundingMode::TowardPositiveInfinity => sign > 0,
            RoundingMode::TowardNegativeInfinity => sign < 0,
            RoundingMode::HalfAwayFromZero => abs_remainder * 2 >= abs_denominator,
            RoundingMode::HalfEven => {
                let twice = abs_remainder * 2;
                twice > abs_denominator
                    || (twice == abs_denominator && (quotient.clone() % 2u8).abs() == BigInt::one())
            }
        };
        let rounded = if increment { quotient + sign } else { quotient };
        Ok(Self::decimal(rounded, scale))
    }

    fn add_rational<F>(&self, rhs: &Self, operation: F) -> Self
    where
        F: FnOnce(BigRational, BigRational) -> BigRational,
    {
        let scale = self.scale().unwrap_or(0).max(rhs.scale().unwrap_or(0));
        self.prefer_decimal(rhs, operation(self.as_rational(), rhs.as_rational()), scale)
    }

    fn prefer_decimal(&self, rhs: &Self, value: BigRational, scale: u32) -> Self {
        if self.scale().is_some() || rhs.scale().is_some() {
            let denominator = ten_pow(scale);
            let scaled = &value * BigRational::from_integer(denominator.clone());
            if scaled.denom() == &BigInt::one() {
                return Self::decimal(scaled.numer().clone(), scale);
            }
        }
        Self::from_rational_value(value)
    }

    fn from_rational(numerator: BigInt, denominator: BigInt) -> Result<Self, ExactError> {
        if denominator.is_zero() {
            return Err(ExactError::DivisionByZero);
        }
        Ok(Self::from_rational_value(BigRational::new(
            numerator,
            denominator,
        )))
    }

    fn from_rational_value(value: BigRational) -> Self {
        if value.denom() == &BigInt::one() {
            return Self::Integer(value.numer().clone());
        }
        Self::Rational {
            numerator: value.numer().clone(),
            denominator: value.denom().clone(),
        }
    }
}

impl From<BigInt> for ExactNumber {
    fn from(value: BigInt) -> Self {
        Self::Integer(value)
    }
}

impl From<i64> for ExactNumber {
    fn from(value: i64) -> Self {
        Self::Integer(value.into())
    }
}

impl From<i128> for ExactNumber {
    fn from(value: i128) -> Self {
        Self::Integer(value.into())
    }
}

impl FromStr for ExactNumber {
    type Err = ExactError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let source = source.trim();
        if source.is_empty() {
            return Err(ExactError::Empty);
        }
        if let Some((numerator, denominator)) = source.split_once('/') {
            if denominator.contains('/')
                || numerator.trim().is_empty()
                || denominator.trim().is_empty()
            {
                return Err(ExactError::Invalid(source.to_string()));
            }
            return Self::rational(
                parse_integer(numerator.trim())?,
                parse_integer(denominator.trim())?,
            );
        }

        // Exponents are parsed exactly by moving the decimal point, never by
        // converting through a machine float.
        let (mantissa, exponent) = match source.find(['e', 'E']) {
            Some(index) => {
                let exponent = source[index + 1..]
                    .parse::<i64>()
                    .map_err(|_| ExactError::Invalid(source.to_string()))?;
                (&source[..index], Some(exponent))
            }
            None => (source, None),
        };
        let signless = mantissa.strip_prefix('+').unwrap_or(mantissa);
        let negative = signless.starts_with('-');
        let digits = if negative { &signless[1..] } else { signless };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        if whole.is_empty() && fraction.is_empty() {
            return Err(ExactError::Invalid(source.to_string()));
        }
        if !whole.chars().all(|c| c.is_ascii_digit())
            || !fraction.chars().all(|c| c.is_ascii_digit())
        {
            return Err(ExactError::Invalid(source.to_string()));
        }
        let mut coefficient_text = String::with_capacity(whole.len() + fraction.len() + 1);
        if negative {
            coefficient_text.push('-');
        }
        coefficient_text.push_str(if whole.is_empty() { "0" } else { whole });
        coefficient_text.push_str(fraction);
        let mut coefficient = coefficient_text
            .parse::<BigInt>()
            .map_err(|_| ExactError::Invalid(source.to_string()))?;
        let mut scale = u32::try_from(fraction.len()).map_err(|_| ExactError::ScaleOverflow)?;
        if let Some(exponent) = exponent {
            if exponent >= 0 {
                let exponent = u32::try_from(exponent).map_err(|_| ExactError::ScaleOverflow)?;
                if exponent >= scale {
                    coefficient *= ten_pow(exponent - scale);
                    scale = 0;
                } else {
                    scale -= exponent;
                }
            } else {
                let amount = u32::try_from(exponent.unsigned_abs())
                    .map_err(|_| ExactError::ScaleOverflow)?;
                scale = scale.checked_add(amount).ok_or(ExactError::ScaleOverflow)?;
            }
        }
        // A decimal keeps the source scale, including trailing zeroes.  An
        // integer source remains an integer so its rendering stays faithful.
        if scale == 0 {
            Ok(Self::Integer(coefficient))
        } else {
            Ok(Self::Decimal { coefficient, scale })
        }
    }
}

impl fmt::Display for ExactNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::Decimal { coefficient, scale } => write_decimal(formatter, coefficient, *scale),
            Self::Rational {
                numerator,
                denominator,
            } => {
                write!(formatter, "{numerator}/{denominator}")
            }
        }
    }
}

impl PartialOrd for ExactNumber {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}

impl Ord for ExactNumber {
    fn cmp(&self, rhs: &Self) -> Ordering {
        self.as_rational().cmp(&rhs.as_rational())
    }
}

impl Neg for ExactNumber {
    type Output = Self;

    fn neg(self) -> Self::Output {
        match self {
            Self::Integer(value) => Self::Integer(-value),
            Self::Decimal { coefficient, scale } => Self::Decimal {
                coefficient: -coefficient,
                scale,
            },
            Self::Rational {
                numerator,
                denominator,
            } => Self::Rational {
                numerator: -numerator,
                denominator,
            },
        }
    }
}

impl<'b> Add<&'b ExactNumber> for &ExactNumber {
    type Output = ExactNumber;

    fn add(self, rhs: &'b ExactNumber) -> Self::Output {
        self.checked_add(rhs)
    }
}

impl Add for ExactNumber {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(&rhs)
    }
}

impl<'b> Sub<&'b ExactNumber> for &ExactNumber {
    type Output = ExactNumber;

    fn sub(self, rhs: &'b ExactNumber) -> Self::Output {
        self.checked_sub(rhs)
    }
}

impl Sub for ExactNumber {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(&rhs)
    }
}

impl<'b> Mul<&'b ExactNumber> for &ExactNumber {
    type Output = ExactNumber;

    fn mul(self, rhs: &'b ExactNumber) -> Self::Output {
        self.checked_mul(rhs)
    }
}

impl Mul for ExactNumber {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.checked_mul(&rhs)
    }
}

/// Parsing and arithmetic errors which must remain visible to callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExactError {
    Empty,
    Invalid(String),
    DivisionByZero,
    ScaleOverflow,
}

impl fmt::Display for ExactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("empty exact number"),
            Self::Invalid(value) => write!(formatter, "invalid exact number: {value}"),
            Self::DivisionByZero => formatter.write_str("division by zero"),
            Self::ScaleOverflow => formatter.write_str("decimal scale is too large"),
        }
    }
}

impl std::error::Error for ExactError {}

fn parse_integer(source: &str) -> Result<BigInt, ExactError> {
    source
        .parse::<BigInt>()
        .map_err(|_| ExactError::Invalid(source.to_string()))
}

fn ten_pow(scale: u32) -> BigInt {
    BigInt::from(10u8).pow(scale)
}

fn canonical_rational_string(value: &BigRational) -> String {
    if value.denom() == &BigInt::one() {
        value.numer().to_string()
    } else {
        format!("{}/{}", value.numer(), value.denom())
    }
}

fn write_decimal(
    formatter: &mut fmt::Formatter<'_>,
    coefficient: &BigInt,
    scale: u32,
) -> fmt::Result {
    if scale == 0 {
        return write!(formatter, "{coefficient}");
    }
    let negative = coefficient.is_negative();
    let digits = coefficient.abs().to_string();
    let scale = usize::try_from(scale).map_err(|_| fmt::Error)?;
    if negative {
        formatter.write_str("-")?;
    }
    if digits.len() <= scale {
        formatter.write_str("0.")?;
        for _ in 0..(scale - digits.len()) {
            formatter.write_str("0")?;
        }
        formatter.write_str(&digits)
    } else {
        let split = digits.len() - scale;
        formatter.write_str(&digits[..split])?;
        formatter.write_str(".")?;
        formatter.write_str(&digits[split..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_scale_is_source_evidence() {
        let value: ExactNumber = "10.00".parse().unwrap();
        assert_eq!(value.to_string(), "10.00");
        assert_eq!(value.scale(), Some(2));
        assert_eq!(value.canonical_string(), "10");
        assert_eq!(value, "10".parse::<ExactNumber>().unwrap());
    }

    #[test]
    fn arithmetic_is_exact_and_keeps_useful_scale() {
        let left: ExactNumber = "0.10".parse().unwrap();
        let right: ExactNumber = "0.20".parse().unwrap();
        assert_eq!((&left + &right).to_string(), "0.30");
        assert_eq!(
            left.checked_div(&right).unwrap(),
            "1/2".parse::<ExactNumber>().unwrap()
        );
    }

    #[test]
    fn rational_normalization_and_finite_decimal_conversion() {
        assert_eq!(ExactNumber::rational(2, 4).unwrap().to_string(), "1/2");
        let third = ExactNumber::rational(1, 3).unwrap();
        assert_eq!(third.to_string(), "1/3");
        assert!(third.finite_decimal().is_none());
    }

    #[test]
    fn exponent_never_uses_float() {
        let value: ExactNumber = "1.2300e2".parse().unwrap();
        assert_eq!(value.to_string(), "123.00");
        let value: ExactNumber = "1.2300e-2".parse().unwrap();
        assert_eq!(value.to_string(), "0.012300");
    }

    #[test]
    fn explicit_rounding_is_deterministic() {
        let value: ExactNumber = "1.235".parse().unwrap();
        assert_eq!(
            value.round(2, RoundingMode::HalfEven).unwrap().to_string(),
            "1.24"
        );
        let value: ExactNumber = "1.225".parse().unwrap();
        assert_eq!(
            value.round(2, RoundingMode::HalfEven).unwrap().to_string(),
            "1.22"
        );
    }

    #[test]
    fn directed_rounding_names_match_their_direction() {
        let positive: ExactNumber = "1.2".parse().unwrap();
        assert_eq!(
            positive
                .round(0, RoundingMode::TowardPositiveInfinity)
                .unwrap()
                .to_string(),
            "2"
        );
        assert_eq!(
            positive
                .round(0, RoundingMode::TowardNegativeInfinity)
                .unwrap()
                .to_string(),
            "1"
        );

        let negative: ExactNumber = "-1.2".parse().unwrap();
        assert_eq!(
            negative
                .round(0, RoundingMode::TowardPositiveInfinity)
                .unwrap()
                .to_string(),
            "-1"
        );
        assert_eq!(
            negative
                .round(0, RoundingMode::TowardNegativeInfinity)
                .unwrap()
                .to_string(),
            "-2"
        );
    }
}
