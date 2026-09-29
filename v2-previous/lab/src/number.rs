//! Small exact rationals with a safe, explicitly tagged exception arena.
//! This is a runtime representation, not a different semantic/canonical codec.
use num_bigint::BigInt;
use num_rational::{BigRational, Ratio};
#[cfg(test)]
use num_traits::ToPrimitive;

#[derive(Clone, Copy, Debug)]
pub struct Exact64 {
    pub numerator: i64,
    pub denominator: u32,
    pub exception: u32,
}
#[derive(Debug)]
pub enum Exceptional {
    Wide(Ratio<i128>),
    Big(BigRational),
}

/// A simpler owned alternative when a number cannot depend on an immutable
/// arena. Only its actual ABI size is measured; it is not a benchmarked layout.
#[allow(dead_code)]
pub enum BoxedNumber {
    Integer(i64),
    Rational64(i64, std::num::NonZeroU64),
    Wide(Box<Ratio<i128>>),
    Big(Box<BigRational>),
}

pub fn components(i: usize) -> (i128, u32, bool) {
    let ordinary = ((i * 7919 % 100_000) as i128) - 50_000;
    if i.is_multiple_of(4093) {
        (0, 7, true)
    } else if i.is_multiple_of(257) {
        ((1i128 << 70) + ordinary, 100, false)
    } else {
        (ordinary, 100, false)
    }
}

pub fn rational(i: usize) -> BigRational {
    let (n, d, big) = components(i);
    let n = if big {
        (BigInt::from(1) << 150usize) + BigInt::from(i as u64)
    } else {
        BigInt::from(n)
    };
    BigRational::new(n, BigInt::from(d))
}

pub fn compact(i: usize, exceptions: &mut Vec<Exceptional>) -> Exact64 {
    let (n, d, big) = components(i);
    if !big {
        let value = Ratio::new(n, d as i128);
        if let Ok(n) = i64::try_from(*value.numer()) {
            return Exact64 {
                numerator: n,
                denominator: *value.denom() as u32,
                exception: 0,
            };
        }
        exceptions.push(Exceptional::Wide(value));
    } else {
        exceptions.push(Exceptional::Big(rational(i)));
    }
    Exact64 {
        numerator: 0,
        denominator: 0,
        exception: u32::try_from(exceptions.len()).expect("exception arena exceeds u32"),
    }
}

#[derive(Clone, Copy)]
pub enum NumberRef<'a> {
    Big(&'a BigRational),
    Compact(Exact64, &'a [Exceptional]),
}

impl NumberRef<'_> {
    pub fn to_big(self) -> BigRational {
        match self {
            Self::Big(v) => v.clone(),
            Self::Compact(v, _) if v.denominator != 0 => {
                BigRational::new(v.numerator.into(), v.denominator.into())
            }
            Self::Compact(v, arena) => match &arena[v.exception as usize - 1] {
                Exceptional::Wide(v) => BigRational::new((*v.numer()).into(), (*v.denom()).into()),
                Exceptional::Big(v) => v.clone(),
            },
        }
    }
    pub fn small(self) -> Option<Ratio<i128>> {
        match self {
            Self::Big(_) => None, // Baseline deliberately uses production-style BigRational arithmetic.
            Self::Compact(v, _) if v.denominator != 0 => {
                Some(Ratio::new_raw(v.numerator as i128, v.denominator as i128))
            }
            Self::Compact(v, arena) => match &arena[v.exception as usize - 1] {
                Exceptional::Wide(v) => Some(*v),
                Exceptional::Big(_) => None,
            },
        }
    }
    #[cfg(test)]
    pub fn fingerprint(self) -> u64 {
        // Exact residue n * inverse(d) mod prime. Every generated denominator
        // is 1, 2, 4, 5, 7, 10, 20, 25, 50 or 100, hence invertible.
        const P: i128 = 2_147_483_647;
        let (n, d) = match self {
            Self::Compact(v, _) if v.denominator != 0 => {
                (v.numerator as i128 % P, v.denominator as i128)
            }
            Self::Compact(v, arena) => match &arena[v.exception as usize - 1] {
                Exceptional::Wide(v) => (*v.numer() % P, *v.denom()),
                Exceptional::Big(v) => return big_fingerprint(v),
            },
            Self::Big(v) => return big_fingerprint(v),
        };
        residue(n, d)
    }
}

#[cfg(test)]
fn big_fingerprint(v: &BigRational) -> u64 {
    const P: i128 = 2_147_483_647;
    let numerator = (v.numer() % BigInt::from(P)).to_i128().unwrap();
    let denominator = v.denom().to_i128().unwrap();
    residue(numerator, denominator)
}
#[cfg(test)]
fn residue(n: i128, d: i128) -> u64 {
    const P: i128 = 2_147_483_647;
    let (mut a, mut b, mut x, mut y) = (d, P, 1i128, 0i128);
    while b != 0 {
        let q = a / b;
        (a, b) = (b, a % b);
        (x, y) = (y, x - q * y);
    }
    assert_eq!(a, 1);
    (n.rem_euclid(P) * x.rem_euclid(P) % P) as u64
}

#[derive(Debug)]
pub enum Sum {
    Small(Ratio<i128>),
    Big(BigRational),
}
impl Sum {
    pub fn zero() -> Self {
        Self::Small(Ratio::from_integer(0))
    }
    pub fn add(&mut self, value: NumberRef<'_>) {
        if let Self::Small(total) = self {
            if let Some(value) = value.small()
                && let Some(sum) = checked_add(*total, value)
            {
                *total = sum;
                return;
            }
            *self = Self::Big(BigRational::new(
                (*total.numer()).into(),
                (*total.denom()).into(),
            ));
        }
        if let Self::Big(total) = self {
            *total += value.to_big();
        }
    }
    pub fn into_big(self) -> BigRational {
        match self {
            Self::Small(v) => BigRational::new((*v.numer()).into(), (*v.denom()).into()),
            Self::Big(v) => v,
        }
    }
}

/// All intermediates are checked before Ratio::new. Inputs must be normalized
/// with a positive denominator. Failure promotes to arbitrary precision.
pub fn checked_add(a: Ratio<i128>, b: Ratio<i128>) -> Option<Ratio<i128>> {
    let (mut x, mut y) = (*a.denom(), *b.denom());
    while y != 0 {
        (x, y) = (y, x % y);
    }
    let ad = a.denom().checked_div(x)?;
    let bd = b.denom().checked_div(x)?;
    let n = a
        .numer()
        .checked_mul(bd)?
        .checked_add(b.numer().checked_mul(ad)?)?;
    let d = ad.checked_mul(*b.denom())?;
    // num-rational normalizes via abs; i128::MIN cannot be safely normalized.
    if n == i128::MIN || d <= 0 {
        return None;
    }
    Some(Ratio::new(n, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::Zero;
    #[test]
    fn compact_matches_big_and_exact_sums() {
        let mut arena = Vec::new();
        let mut actual = Sum::zero();
        let mut expected = BigRational::zero();
        for i in 0..20_000 {
            let value = compact(i, &mut arena);
            let reference = NumberRef::Compact(value, &arena);
            assert_eq!(reference.to_big(), rational(i));
            assert_eq!(
                reference.fingerprint(),
                NumberRef::Big(&rational(i)).fingerprint()
            );
            actual.add(reference);
            expected += rational(i);
        }
        assert_eq!(actual.into_big(), expected);
    }
    #[test]
    fn overflow_promotes_without_wrapping() {
        assert!(checked_add(Ratio::from_integer(i128::MAX), Ratio::from_integer(1)).is_none());
        assert!(checked_add(Ratio::from_integer(i128::MIN + 1), Ratio::from_integer(-1)).is_none());
        assert_eq!(
            checked_add(Ratio::new(1, 3), Ratio::new(-1, 3)),
            Some(Ratio::from_integer(0))
        );
    }
}
