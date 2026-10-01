//! Arithmetic and comparison on law values.
//!
//! Everything is exact. An amount times a number rounds half to even to the
//! commodity's quantum; a number is a rational. Amounts of different
//! commodities are converted at the context day's prices: a comparison converts
//! its left side into the right side's commodity, a sum converts its right side
//! into the left's. A missing price is a [`Fault`], and faults propagate.
//!
//! Operands are type-checked by the model (`Node::ty`), so a combination the
//! type rules forbid cannot arrive here.

use std::cmp::Ordering;

use axiom_core::{Day, Days, Id, Qty, Ratio, Span, day::days_in_month};
use axiom_model::{Amount, BinOp, Book, Bracket, Commodity, Fault, Value, Window};

pub(crate) struct Calc<'a, 's> {
    pub book: &'a Book<'s>,
    /// The day prices are read on.
    pub day: Day,
}

const TYPED: &str = "the model type-checks operands";

impl Calc<'_, '_> {
    pub fn convert(&self, amount: Amount, unit: Id<Commodity>) -> Result<Amount, Fault> {
        if amount.unit == unit {
            return Ok(amount);
        }
        self.book.convert(amount, unit, self.day).ok_or(Fault::NoPrice { unit: amount.unit, quote: unit })
    }

    pub fn binary(&self, op: BinOp, left: Value, right: Value) -> Value {
        match op {
            BinOp::And => return logic(left, right, false),
            BinOp::Or => return logic(left, right, true),
            _ => {}
        }
        if let Value::Fault(_) = left {
            return left;
        }
        if let Value::Fault(_) = right {
            return right;
        }
        let result = match op {
            BinOp::Add | BinOp::Sub => self.sum(op == BinOp::Sub, left, right),
            BinOp::Mul => self.product(left, right),
            BinOp::Div => self.quotient(left, right),
            _ => self.compare(op, left, right).map(Value::Bool),
        };
        result.unwrap_or_else(Value::Fault)
    }

    fn sum(&self, minus: bool, left: Value, right: Value) -> Result<Value, Fault> {
        let signed = |qty: Qty| if minus { -qty } else { qty };
        Ok(match (left, right) {
            (Value::Amount(a), Value::Amount(b)) => {
                let b = self.convert(b, a.unit)?;
                Value::Amount(Amount::new(a.qty + signed(b.qty), a.unit))
            }
            (Value::Amount(_), Value::Empty) | (Value::Empty, Value::Empty) => left,
            (Value::Empty, Value::Amount(b)) => Value::Amount(Amount::new(signed(b.qty), b.unit)),
            (Value::Num(a), Value::Num(b)) => {
                Value::Num(if minus { a.checked_sub(b) } else { a.checked_add(b) }.ok_or(Fault::Overflow)?)
            }
            (Value::Day(day), Value::Span(span)) => {
                let span = if minus { Span { months: -span.months, days: -span.days } } else { span };
                Value::Day(day.add(span))
            }
            (Value::Day(later), Value::Day(earlier)) if minus => Value::Span(later.since(earlier)),
            _ => unreachable!("{TYPED}"),
        })
    }

    fn product(&self, left: Value, right: Value) -> Result<Value, Fault> {
        Ok(match (left, right) {
            (Value::Amount(a), Value::Num(n)) | (Value::Num(n), Value::Amount(a)) => {
                Value::Amount(Amount::new(a.qty.scale(n).ok_or(Fault::Overflow)?, a.unit))
            }
            (Value::Empty, Value::Num(_)) | (Value::Num(_), Value::Empty) => Value::Empty,
            (Value::Num(a), Value::Num(b)) => Value::Num(a.checked_mul(b).ok_or(Fault::Overflow)?),
            _ => unreachable!("{TYPED}"),
        })
    }

    fn quotient(&self, left: Value, right: Value) -> Result<Value, Fault> {
        Ok(match (left, right) {
            (Value::Amount(a), Value::Num(n)) => {
                let by = n.recip().ok_or(Fault::DivideByZero)?;
                Value::Amount(Amount::new(a.qty.scale(by).ok_or(Fault::Overflow)?, a.unit))
            }
            (Value::Amount(a), Value::Amount(b)) => {
                let b = self.convert(b, a.unit)?;
                let ratio = Ratio::new(a.qty.0 as i128, b.qty.0 as i128);
                Value::Num(ratio.ok_or(if b.qty.is_zero() { Fault::DivideByZero } else { Fault::Overflow })?)
            }
            (Value::Empty, Value::Num(_)) => Value::Empty,
            (Value::Num(a), Value::Num(b)) => Value::Num(a.checked_div(b).ok_or(Fault::DivideByZero)?),
            _ => unreachable!("{TYPED}"),
        })
    }

    fn compare(&self, op: BinOp, left: Value, right: Value) -> Result<bool, Fault> {
        let ordering = match (left, right) {
            (Value::Amount(a), Value::Amount(b)) => Some(self.convert(a, b.unit)?.qty.cmp(&b.qty)),
            (Value::Amount(a), Value::Empty) => Some(a.qty.cmp(&Qty::ZERO)),
            (Value::Empty, Value::Amount(b)) => Some(Qty::ZERO.cmp(&b.qty)),
            (Value::Empty, Value::Empty) => Some(Ordering::Equal),
            (Value::Num(a), Value::Num(b)) => Some(a.cmp(&b)),
            (Value::Day(a), Value::Day(b)) => Some(a.cmp(&b)),
            (Value::Span(a), Value::Span(b)) => Some(a.cmp(&b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(&b)),
            _ => None,
        };
        Ok(match (op, ordering) {
            (BinOp::Eq, Some(o)) => o.is_eq(),
            (BinOp::Eq, None) => left == right,
            (BinOp::Ne, Some(o)) => o.is_ne(),
            (BinOp::Ne, None) => left != right,
            (BinOp::Lt, Some(o)) => o.is_lt(),
            (BinOp::Le, Some(o)) => o.is_le(),
            (BinOp::Gt, Some(o)) => o.is_gt(),
            (BinOp::Ge, Some(o)) => o.is_ge(),
            _ => unreachable!("{TYPED}"),
        })
    }
}

/// `and` / `or` over values already computed. One decisive operand settles the
/// result even if the other is a fault: `false and <fault>` is `false`.
fn logic(left: Value, right: Value, or: bool) -> Value {
    match (left, right) {
        (Value::Bool(l), _) if l == or => Value::Bool(or),
        (_, Value::Bool(r)) if r == or => Value::Bool(or),
        (Value::Fault(_), _) => left,
        (_, Value::Fault(_)) => right,
        (Value::Bool(_), Value::Bool(_)) => Value::Bool(!or),
        _ => unreachable!("{TYPED}"),
    }
}

/// Tax on `income` under marginal `brackets` (ascending, the first at zero):
/// each slice of income is taxed at its bracket's rate, and each bracket's tax
/// is rounded half to even to the quantum. `None` on overflow.
pub(crate) fn progressive(brackets: &[Bracket], income: Qty) -> Option<Qty> {
    let tops = brackets.iter().skip(1).map(|b| b.from).chain(std::iter::once(Qty(i64::MAX)));
    let mut tax = Qty::ZERO;
    for (bracket, top) in brackets.iter().zip(tops) {
        let slice = income.min(top) - bracket.from;
        if slice <= Qty::ZERO {
            break;
        }
        tax += slice.scale(bracket.rate)?;
    }
    Some(tax)
}

/// The share of a straight-line life in one requested period. The schedule is
/// rounded only at its actual calendar boundaries, so splitting a period into
/// smaller windows cannot create or lose a cent. Mid-month lives have a
/// half-month at each end and one extra calendar month to preserve the stated
/// life.
pub(crate) fn straight_line(cost: Qty, life: Span, from: Day, over: Days, period: Window, mid_month: bool) -> Option<Qty> {
    if life.months <= 0 || life.days != 0 || period == Window::Ever {
        return None;
    }
    let months = life.months;
    let month_index = |day: Day| -> Option<i64> {
        let (year, month, _) = day.ymd();
        i64::from(year).checked_mul(12)?.checked_add(i64::from(month) - 1)
    };
    let first_month = month_index(from)?;
    let first_weight = if mid_month {
        Ratio::new(1, 2)?
    } else {
        let (_, month, day) = from.ymd();
        let days = days_in_month(from.year(), month);
        Ratio::new(i128::from(days - day + 1), i128::from(days))?
    };
    let months = i64::from(months);

    // A boundary includes every completed month through `day`'s month. The
    // first partial month is followed by full months and a final remainder,
    // which makes the total exactly `life` months without moving the endpoint
    // when the acquisition day is not the first.
    let cumulative = |day: Day| -> Option<Qty> {
        let offset = month_index(day)?.checked_sub(first_month)?;
        if offset < 0 {
            return Some(Qty::ZERO);
        }
        let units = if offset == 0 {
            first_weight
        } else if offset < months {
            first_weight.checked_add(Ratio::int(offset))?
        } else {
            Ratio::int(months)
        };
        cost.scale(units.checked_div(Ratio::int(months))?)
    };

    let through = cumulative(over.last())?;
    let before = cumulative(over.first().add_days(-1))?;
    Some(through - before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bracket(from: i64, percent: i128) -> Bracket {
        Bracket { from: Qty(from), rate: Ratio::percent(percent, 0).unwrap() }
    }

    #[test]
    fn progressive_taxes_each_slice_at_its_rate() {
        let brackets = [bracket(0, 10), bracket(1_000_00, 12), bracket(5_000_00, 22)];
        assert_eq!(progressive(&brackets, Qty(0)), Some(Qty(0)));
        assert_eq!(progressive(&brackets, Qty(800_00)), Some(Qty(80_00)));
        assert_eq!(progressive(&brackets, Qty(3_000_00)), Some(Qty(100_00 + 240_00)));
        assert_eq!(progressive(&brackets, Qty(6_000_00)), Some(Qty(100_00 + 480_00 + 220_00)));
        // 12% of 1 cent is 0.12 cents: half-even rounds each bracket to zero.
        assert_eq!(progressive(&brackets, Qty(1_000_01)), Some(Qty(100_00)));
    }

    #[test]
    fn decisive_operands_beat_faults() {
        let fault = Value::Fault(Fault::DivideByZero);
        assert_eq!(logic(Value::Bool(false), fault, false), Value::Bool(false));
        assert_eq!(logic(fault, Value::Bool(true), true), Value::Bool(true));
        assert_eq!(logic(Value::Bool(true), fault, false), fault);
        assert_eq!(logic(Value::Bool(true), Value::Bool(true), false), Value::Bool(true));
        assert_eq!(logic(Value::Bool(false), Value::Bool(false), true), Value::Bool(false));
    }

    #[test]
    fn straight_line_splits_at_calendar_boundaries_without_changing_total() {
        let day = |y, m, d| Day::from_ymd(y, m, d).unwrap();
        let cost = Qty(28_200_000); // $282,000.00 at cent precision
        let life = Span::months(330);
        let part = |year, month| {
            let first = day(year, month, 1);
            let last = day(year, month, days_in_month(year, month));
            straight_line(cost, life, day(2024, 3, 1), Days::new(first, last).unwrap(), Window::Month, true).unwrap()
        };

        let before_2026: Qty = (3..=12).map(|month| part(2024, month)).chain((1..=12).map(|month| part(2025, month))).sum();
        let first_quarter: Qty = (1..=3).map(|month| part(2026, month)).sum();
        assert_eq!(before_2026, Qty(1_837_273));
        assert_eq!(first_quarter, Qty(256_363));
        assert_eq!([part(2026, 1), part(2026, 2), part(2026, 3)], [Qty(85_454), Qty(85_455), Qty(85_454)]);

        let first_day_2026 = day(2026, 1, 1);
        let march_end = day(2026, 3, 31);
        let whole_quarter = straight_line(
            cost,
            life,
            day(2024, 3, 1),
            Days::new(first_day_2026, march_end).unwrap(),
            Window::Year,
            true,
        )
        .unwrap();
        assert_eq!(whole_quarter, first_quarter);
    }

    #[test]
    fn a_mid_month_improvement_gets_its_own_half_month_start() {
        let day = |y, m, d| Day::from_ymd(y, m, d).unwrap();
        let cost = Qty(148_000); // $1,480.00 at cent precision
        let life = Span::months(330);
        let month = |m| {
            let first = day(2026, m, 1);
            let last = day(2026, m, days_in_month(2026, m));
            straight_line(cost, life, day(2026, 2, 2), Days::new(first, last).unwrap(), Window::Month, true).unwrap()
        };
        assert_eq!(month(2) + month(3), Qty(673));
    }
}
