//! A loan's level payment, how many it takes, and what each leaves owed.
//!
//! `loan 320_000 USD on 2024-02-20 at 5.875% over 30y` is an annuity (ACTUS ANN): one payment, the same every period,
//! that pays the interest of the period and what is left of it off the principal, so that the principal is paid after
//! the last. Today's fold knows the payment and nothing else (`loan_payment` in the engine), so a forecast pays it for as
//! long as it is asked to. An [`Annuity`] knows how many payments there are and what is owed after each, so the
//! promise it belongs to is done after the last.
//!
//! The payment is worked out once, when the book is built, with the arithmetic the engine uses: 18 decimal places of
//! fixed point, rounded half to even at every step, because the cents of a payment must be the ones the fold has. (A
//! power by squaring would be quicker, and would round differently.)

use axiom_core::{Cadence, Day, Qty, Ratio, Span};

use crate::book::{Amount, Loan};

/// A loan's payment, and the number of them that pays it off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Annuity {
    /// The day the loan was made: its first payment is the first due day after it.
    begins: Day,
    principal: Amount,
    payment: Qty,
    periods: u32,
    /// The rate of one period.
    rate: Ratio,
}

/// What one payment did: the interest it paid, the principal it paid off, and what is owed after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Paid {
    pub interest: Qty,
    pub principal: Qty,
    pub open: Qty,
}

impl Annuity {
    /// The annuity `loan` makes when it is paid every `every` at `yearly` (none: no interest). None for a cadence the
    /// loan has no payment for (a span of months and days, a term shorter than a period) and for a payment too large to
    /// count.
    pub fn new(loan: &Loan, every: Cadence, yearly: Option<Ratio>) -> Option<Annuity> {
        let annual = yearly.unwrap_or(Ratio::ZERO);
        let (periods, rate) = match every {
            Cadence::Every(Span { months, days: 0 }) if months > 0 => {
                let periods = loan.term.months.checked_add(months - 1)?.checked_div(months)?;
                (periods, annual.checked_mul(Ratio::new(i128::from(months), 12)?)?)
            }
            Cadence::Every(Span { months: 0, days }) if days > 0 => {
                let periods = loan.term.days.checked_add(days - 1)?.checked_div(days)?;
                (periods, annual.checked_mul(Ratio::new(i128::from(days), 365)?)?)
            }
            Cadence::TwiceMonthly => (loan.term.months.checked_mul(2)?, annual.checked_div(Ratio::int(24))?),
            _ => return None,
        };
        if periods <= 0 || rate.is_negative() {
            return None;
        }
        let payment = loan.principal.qty.scale(payment_factor(rate, periods)?)?;
        Some(Annuity { begins: loan.on, principal: loan.principal, payment, periods: periods as u32, rate })
    }

    /// The day the loan was made.
    pub fn begins(&self) -> Day {
        self.begins
    }

    /// What the loan was.
    pub fn principal(&self) -> Amount {
        self.principal
    }

    /// The payment of every period but the last.
    pub fn payment(&self) -> Amount {
        Amount::new(self.payment, self.principal.unit)
    }

    /// How many payments there are.
    pub fn periods(&self) -> u32 {
        self.periods
    }

    /// The payment of period `index` (counting from 0) when `open` is owed before it: the interest of the period, then
    /// what the payment leaves of the principal. The last payment is what is left, whatever rounding left, so that
    /// nothing is owed after it.
    pub fn pay(&self, open: Qty, index: u32) -> Option<Paid> {
        let interest = open.scale(self.rate)?;
        let due = if index + 1 >= self.periods { open + interest } else { self.payment };
        let principal = (due - interest).clamp(Qty::ZERO, open);
        Some(Paid { interest, principal, open: open - principal })
    }
}

/// The most periods a payment is compounded over. The loop is one multiplication a period, and the payment is worked out
/// for every contract when a book is built; a loan of a hundred thousand payments (a daily one for 270 years) is the
/// longest that is not a typing mistake, and the engine's own loop would overflow long before a million.
const MOST_COMPOUNDED: i32 = 100_000;

/// The share of the principal that is paid every period: `r / (1 - (1 + r)^-n)`, at 18 places and rounded at each step
/// as the engine does.
fn payment_factor(rate: Ratio, periods: i32) -> Option<Ratio> {
    if rate.is_zero() {
        return Ratio::new(1, i128::from(periods));
    }
    const SCALE: i128 = 1_000_000_000_000_000_000;
    if periods > MOST_COMPOUNDED {
        return None;
    }
    let rate = mul_div(i128::from(rate.num()), SCALE, i128::from(rate.den()))?;
    let mut growth = SCALE;
    for _ in 0..periods {
        growth = mul_div(growth, SCALE.checked_add(rate)?, SCALE)?;
    }
    Ratio::new(mul_div(rate, growth, growth.checked_sub(SCALE)?)?, SCALE)
}

/// `left * right / denominator`, rounded half to even.
fn mul_div(left: i128, right: i128, denominator: i128) -> Option<i128> {
    let numerator = left.checked_mul(right)?;
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let twice = remainder.unsigned_abs().checked_mul(2)?;
    let divisor = denominator.unsigned_abs();
    let away = twice > divisor || (twice == divisor && quotient & 1 != 0);
    Some(if away { quotient.checked_add(numerator.signum() * denominator.signum())? } else { quotient })
}

#[cfg(test)]
mod tests {
    use axiom_core::{Day, Id};

    use super::*;

    fn loan(principal: i64, months: i32, days: i32) -> Loan {
        Loan {
            principal: Amount::new(Qty(principal), Id::new(0)),
            on: Day(0),
            term: Span { months, days },
            asset: None,
            debt: Id::new(0),
            resets: None,
            prepay: Default::default(),
        }
    }

    fn monthly() -> Cadence {
        Cadence::Every(Span::months(1))
    }

    fn percent(rate: i128) -> Option<Ratio> {
        Ratio::percent(rate, 0)
    }

    /// Pays the loan off, the way a fold that kept every payment would.
    fn paid_off(annuity: &Annuity) -> Vec<Paid> {
        let mut open = annuity.principal().qty;
        let mut payments = Vec::new();
        for index in 0..annuity.periods() {
            let paid = annuity.pay(open, index).unwrap();
            open = paid.open;
            payments.push(paid);
        }
        payments
    }

    #[test]
    fn a_loan_with_no_interest_pays_its_principal_in_equal_parts() {
        let annuity = Annuity::new(&loan(300_000, 3, 0), monthly(), None).unwrap();
        assert_eq!((annuity.periods(), annuity.payment().qty), (3, Qty(100_000)));
        let payments = paid_off(&annuity);
        assert!(payments.iter().all(|paid| paid.interest == Qty::ZERO && paid.principal == Qty(100_000)));
        assert_eq!(payments.last().unwrap().open, Qty::ZERO);
    }

    #[test]
    fn a_loan_with_interest_is_owed_nothing_after_its_last_payment() {
        for (principal, months, rate) in [(30_000_00, 36, 5), (320_000_00, 360, 6), (3_000_00, 12, 9), (99_99, 7, 4)] {
            let annuity = Annuity::new(&loan(principal, months, 0), monthly(), percent(rate)).unwrap();
            let payments = paid_off(&annuity);
            assert_eq!(payments.len(), months as usize);
            assert_eq!(payments.last().unwrap().open, Qty::ZERO, "{principal} over {months}");
            assert_eq!(payments.iter().map(|paid| paid.principal).sum::<Qty>(), Qty(principal));
            // Every payment but the last is the level payment; the last differs by what rounding left.
            let level = annuity.payment().qty;
            let last = payments.last().unwrap();
            assert!(payments[..payments.len() - 1].iter().all(|paid| paid.interest + paid.principal == level));
            assert!(
                (last.interest + last.principal - level).abs() < Qty(months as i64 * 2),
                "{principal} over {months}"
            );
        }
    }

    #[test]
    fn interest_is_paid_first_and_falls_as_the_principal_does() {
        let annuity = Annuity::new(&loan(100_000_00, 120, 0), monthly(), percent(6)).unwrap();
        let payments = paid_off(&annuity);
        assert!(payments.windows(2).all(|pair| pair[1].interest <= pair[0].interest));
        assert_eq!(payments[0].interest, Qty(100_000_00).scale(Ratio::new(1, 200).unwrap()).unwrap());
    }

    #[test]
    fn how_many_payments_follows_the_cadence() {
        let periods =
            |every, months, days| Annuity::new(&loan(100_000, months, days), every, None).map(|a| a.periods());
        assert_eq!(periods(monthly(), 36, 0), Some(36));
        assert_eq!(periods(Cadence::Every(Span::months(2)), 5, 0), Some(3), "a part of a period is one");
        assert_eq!(periods(Cadence::Every(Span::days(14)), 0, 90), Some(7));
        assert_eq!(periods(Cadence::TwiceMonthly, 12, 0), Some(24));
        assert_eq!(periods(monthly(), 0, 90), None, "a loan in days has no monthly payment");
        assert_eq!(periods(Cadence::Every(Span { months: 1, days: 15 }), 36, 0), None);
        assert_eq!(periods(Cadence::Every(Span::months(48)), 36, 0), Some(1));
    }

    #[test]
    fn an_annuity_is_small() {
        assert!(size_of::<Annuity>() <= 64, "{}", size_of::<Annuity>());
    }
}
