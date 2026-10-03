//! What a due day is worth and what it is for: the factor an occurrence on that day takes, and the days it is
//! recognized over.
//!
//! Neither reads a flow or the state of the fold. `rising 3% yearly` counts the anniversaries of the contract's first
//! day; `indexed to cpi yearly` reads a parameter on the latest anniversary; `prorated` is the share of the period an
//! occurrence covers that the contract lives in; `for last month` and `covers the quarter` say which period that is.
//! Every one is a function of the day, the contract's life and the book's parameters, so they are the schedule's, kept
//! beside the due days, and the fold is handed the factor of the day it makes an occurrence for.

use axiom_core::calendar::{self, Window};
use axiom_core::{Day, Days, Id, Ratio, Span};

use crate::book::{Book, Coverage, Escalation, ForecastError, Param, Relative, Terms};
use crate::law::Value;

/// How an occurrence is recognized, as the clauses of a contract say it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recognition {
    /// Neither `for` nor `covers`: on the day it is due.
    OnTheDay,
    /// `for last month|quarter|year`: the period before the day's.
    Last(Relative),
    /// `covers the month|quarter|year` or `covers SPAN`: the period the day is in, or the span from it.
    Covers(Coverage),
    /// Both were written, which lowering takes and no day can be recognized by.
    Conflict,
}

/// Whether an occurrence that starts or ends inside its period is that share of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proration {
    Whole,
    Prorated,
}

/// What turns a due day into an amount and a window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reckoning {
    pub escalation: Option<Escalation>,
    pub recognition: Recognition,
    pub proration: Proration,
}

impl Reckoning {
    /// The reckoning of the terms a schedule was written with.
    pub fn of(terms: &Terms) -> Reckoning {
        let recognition = match (terms.period, terms.covers) {
            (None, None) => Recognition::OnTheDay,
            (Some(relative), None) => Recognition::Last(relative),
            (None, Some(coverage)) => Recognition::Covers(coverage),
            (Some(_), Some(_)) => Recognition::Conflict,
        };
        let proration = if terms.prorated { Proration::Prorated } else { Proration::Whole };
        Reckoning { escalation: terms.escalation, recognition, proration }
    }

    /// The multiplier for an occurrence on `day` of a contract that lives `life`: its anniversary rise or the index's
    /// movement since it began, and the share of its period it lives in.
    pub fn factor(&self, book: &Book<'_>, life: Days, day: Day) -> Result<Ratio, ForecastError> {
        let risen = match self.escalation {
            None => Ratio::ONE,
            Some(Escalation::Rising(rate)) => rise(life.first(), rate, day)?,
            Some(Escalation::Indexed(param)) => index_moved(book, param, life.first(), day)?,
        };
        match self.proration {
            Proration::Whole => Ok(risen),
            Proration::Prorated => {
                let period = self.period(day)?.ok_or(ForecastError::UnsupportedProration(day))?;
                risen.checked_mul(share_alive(life, period)?).ok_or(ForecastError::Overflow)
            }
        }
    }

    /// The days an occurrence on `day` is recognized over.
    pub fn recognized(&self, day: Day) -> Result<Days, ForecastError> {
        Ok(self.period(day)?.unwrap_or(Days::on(day)))
    }

    /// The period `for` or `covers` names for `day`, if either does.
    fn period(&self, day: Day) -> Result<Option<Days>, ForecastError> {
        match self.recognition {
            Recognition::OnTheDay => Ok(None),
            Recognition::Conflict => Err(ForecastError::ConflictingRecognition(day)),
            Recognition::Last(Relative::Last(period)) => Ok(Some(Window::containing(period, day).previous().days())),
            Recognition::Last(Relative::LastQuarter) => Ok(Some(calendar::quarter(day, -1))),
            Recognition::Covers(Coverage::Calendar(period)) => Ok(Some(Window::containing(period, day).days())),
            Recognition::Covers(Coverage::Quarter) => Ok(Some(calendar::quarter(day, 0))),
            Recognition::Covers(Coverage::Span(span)) => covered(day, span).map(Some),
        }
    }
}

/// `(1 + rate)` for each anniversary of `first` that `day` has passed.
fn rise(first: Day, rate: Ratio, day: Day) -> Result<Ratio, ForecastError> {
    let yearly = Ratio::ONE.checked_add(rate).ok_or(ForecastError::Overflow)?;
    if yearly.is_negative() {
        return Err(ForecastError::InvalidRate);
    }
    let (_, years) = calendar::anniversary(first, day).ok_or(ForecastError::Overflow)?;
    power(yearly, u32::try_from(years).map_err(|_| ForecastError::Overflow)?)
}

/// `base` to the `years`th power, by squaring.
fn power(mut base: Ratio, mut years: u32) -> Result<Ratio, ForecastError> {
    let mut product = Ratio::ONE;
    while years > 0 {
        if years & 1 == 1 {
            product = product.checked_mul(base).ok_or(ForecastError::Overflow)?;
        }
        years >>= 1;
        if years > 0 {
            base = base.checked_mul(base).ok_or(ForecastError::Overflow)?;
        }
    }
    Ok(product)
}

/// What the index `param` reads on the latest anniversary of `first` as a multiple of what it read on `first`.
fn index_moved(book: &Book<'_>, param: Id<Param>, first: Day, day: Day) -> Result<Ratio, ForecastError> {
    let (anniversary, _) = calendar::anniversary(first, day).ok_or(ForecastError::Overflow)?;
    let (base, current) = (index_on(book, param, first)?, index_on(book, param, anniversary)?);
    current.checked_div(base).ok_or(ForecastError::Overflow)
}

/// The index `param` stood at on `day`: its latest row at or before it that has no keys, and a positive number.
fn index_on(book: &Book<'_>, param: Id<Param>, day: Day) -> Result<Ratio, ForecastError> {
    let row = book.params.get(param).and_then(|data| data.row(day, &[]));
    match row.ok_or(ForecastError::MissingIndex { param, day })?.value {
        Value::Num(index) if index > Ratio::ZERO => Ok(index),
        Value::Fault(fault) => Err(ForecastError::IndexFault { param, day, fault }),
        _ => Err(ForecastError::InvalidIndex { param, day }),
    }
}

/// The share of `period` that falls in the days the contract lives.
fn share_alive(life: Days, period: Days) -> Result<Ratio, ForecastError> {
    let Some(alive) = life.intersect(period) else { return Ok(Ratio::ZERO) };
    let count = |days: Days| i128::from(days.last().0) - i128::from(days.first().0) + 1;
    Ratio::new(count(alive), count(period)).ok_or(ForecastError::Overflow)
}

/// `covers 6m`: the days from `day` for `span`.
fn covered(day: Day, span: Span) -> Result<Days, ForecastError> {
    let after = day.checked_add(span).ok_or(ForecastError::Overflow)?;
    if after <= day {
        return Err(ForecastError::InvalidCoverage(day));
    }
    Days::new(day, Day(after.0 - 1)).ok_or(ForecastError::InvalidCoverage(day))
}
