//! Exact temporal values used by the economic model.
//!
//! Time is deliberately a small sum of named representations.  An instant,
//! a local date, and a month are not interchangeable resolutions, and no
//! constructor below silently turns one into another.  In particular, a
//! named time zone is retained as a name: resolving its daylight-saving rules
//! requires a separately versioned time-zone database and is therefore not a
//! responsibility of this module.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

pub use crate::model::{Date as LocalDate, Weekday};

/// The roles an economic fact may assign to a temporal value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TimeRole {
    Occurred,
    Effective,
    Authorized,
    Captured,
    Settled,
    Due,
    Observed,
    Recorded,
    Recognized,
    Valid,
    Superseded,
}

impl fmt::Display for TimeRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Occurred => "occurred",
            Self::Effective => "effective",
            Self::Authorized => "authorized",
            Self::Captured => "captured",
            Self::Settled => "settled",
            Self::Due => "due",
            Self::Observed => "observed",
            Self::Recorded => "recorded",
            Self::Recognized => "recognized",
            Self::Valid => "valid",
            Self::Superseded => "superseded",
        };
        f.write_str(name)
    }
}

/// Named temporal facts carried by one occurrence.  Roles are independent:
/// recording an observation does not overwrite when the underlying event
/// occurred or when a book recognizes it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeAssignments {
    values: BTreeMap<TimeRole, TimeValue>,
}

impl TimeAssignments {
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, role: TimeRole, value: TimeValue) -> Option<TimeValue> {
        self.values.insert(role, value)
    }

    pub fn get(&self, role: TimeRole) -> Option<&TimeValue> {
        self.values.get(&role)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TimeRole, &TimeValue)> {
        self.values.iter()
    }
}

impl Default for TimeAssignments {
    fn default() -> Self {
        Self::new()
    }
}

/// A signed, exact count of nanoseconds from the Unix epoch.
///
/// Nanoseconds are a representation choice, not a floating-point precision
/// claim.  Values that are only known to a date or period belong to the
/// corresponding [`TimeValue`] variant instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Instant {
    nanos: i128,
}

impl Instant {
    pub const EPOCH: Self = Self { nanos: 0 };

    pub const fn from_unix_nanos(nanos: i128) -> Self {
        Self { nanos }
    }

    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self {
            nanos: seconds as i128 * 1_000_000_000,
        }
    }

    pub const fn unix_nanos(self) -> i128 {
        self.nanos
    }

    pub const fn unix_seconds(self) -> i128 {
        self.nanos / 1_000_000_000
    }

    pub fn checked_add_nanos(self, nanos: i128) -> Option<Self> {
        self.nanos.checked_add(nanos).map(Self::from_unix_nanos)
    }

    pub fn duration_since(self, earlier: Self) -> Option<i128> {
        self.nanos.checked_sub(earlier.nanos)
    }
}

impl fmt::Display for Instant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.nanos)
    }
}

/// A wall-clock time without a zone conversion hidden inside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LocalTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
}

impl LocalTime {
    pub fn new(hour: u8, minute: u8, second: u8, nanosecond: u32) -> Result<Self, TimeError> {
        if hour >= 24 || minute >= 60 || second >= 60 || nanosecond >= 1_000_000_000 {
            return Err(TimeError::InvalidLocalTime {
                hour,
                minute,
                second,
                nanosecond,
            });
        }
        Ok(Self {
            hour,
            minute,
            second,
            nanosecond,
        })
    }
}

/// A fixed offset or an unresolved named zone.  Named zones intentionally do
/// not carry a bundled timezone database.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TimeZone {
    FixedOffsetSeconds(i32),
    Named(String),
}

/// How a local wall-clock label relates to a zone's transition rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum LocalTimeStatus {
    Exact,
    Ambiguous,
    Skipped,
    Unresolved,
}

/// A local datetime keeps its zone and DST ambiguity visible.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LocalDateTime {
    date: LocalDate,
    time: LocalTime,
    zone: TimeZone,
    status: LocalTimeStatus,
}

impl LocalDateTime {
    pub fn new(date: LocalDate, time: LocalTime, zone: TimeZone) -> Self {
        let status = match &zone {
            TimeZone::FixedOffsetSeconds(_) => LocalTimeStatus::Exact,
            TimeZone::Named(_) => LocalTimeStatus::Unresolved,
        };
        Self {
            date,
            time,
            zone,
            status,
        }
    }

    pub fn ambiguous(date: LocalDate, time: LocalTime, zone: impl Into<String>) -> Self {
        Self {
            date,
            time,
            zone: TimeZone::Named(zone.into()),
            status: LocalTimeStatus::Ambiguous,
        }
    }

    pub fn skipped(date: LocalDate, time: LocalTime, zone: impl Into<String>) -> Self {
        Self {
            date,
            time,
            zone: TimeZone::Named(zone.into()),
            status: LocalTimeStatus::Skipped,
        }
    }

    pub fn date(&self) -> LocalDate {
        self.date
    }

    pub fn time(&self) -> LocalTime {
        self.time
    }

    pub fn zone(&self) -> &TimeZone {
        &self.zone
    }

    pub fn status(&self) -> LocalTimeStatus {
        self.status
    }

    /// Resolve only a fixed offset.  Named zones require external rules and
    /// therefore return an explicit error rather than an invented instant.
    pub fn to_instant(&self) -> Result<Instant, TimeError> {
        let TimeZone::FixedOffsetSeconds(offset) = &self.zone else {
            return Err(TimeError::TimezoneRulesRequired);
        };
        if self.status != LocalTimeStatus::Exact {
            return Err(TimeError::AmbiguousLocalTime);
        }
        let days = days_from_civil(self.date.year(), self.date.month(), self.date.day());
        let seconds = days
            .checked_mul(86_400)
            .and_then(|value| value.checked_add(i128::from(self.time.hour) * 3_600))
            .and_then(|value| value.checked_add(i128::from(self.time.minute) * 60))
            .and_then(|value| value.checked_add(i128::from(self.time.second)))
            .and_then(|value| value.checked_sub(i128::from(*offset)))
            .ok_or(TimeError::Overflow)?;
        let nanos = seconds
            .checked_mul(1_000_000_000)
            .and_then(|value| value.checked_add(i128::from(self.time.nanosecond)))
            .ok_or(TimeError::Overflow)?;
        Ok(Instant::from_unix_nanos(nanos))
    }
}

/// A deliberately coarse accounting/calendar period.  A month is not a date
/// range with a fabricated day or time; callers must retain this value when
/// the source only establishes month-level precision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Period {
    Day { date: LocalDate },
    Week { year: i32, week: u8 },
    Month { year: i32, month: u8 },
    Quarter { year: i32, quarter: u8 },
    Year { year: i32 },
}

impl Period {
    pub fn month(year: i32, month: u8) -> Result<Self, TimeError> {
        if !(1..=12).contains(&month) {
            return Err(TimeError::InvalidPeriod);
        }
        Ok(Self::Month { year, month })
    }

    pub fn quarter(year: i32, quarter: u8) -> Result<Self, TimeError> {
        if !(1..=4).contains(&quarter) {
            return Err(TimeError::InvalidPeriod);
        }
        Ok(Self::Quarter { year, quarter })
    }

    pub fn precision(&self) -> PeriodPrecision {
        match self {
            Self::Day { .. } => PeriodPrecision::Day,
            Self::Week { .. } => PeriodPrecision::Week,
            Self::Month { .. } => PeriodPrecision::Month,
            Self::Quarter { .. } => PeriodPrecision::Quarter,
            Self::Year { .. } => PeriodPrecision::Year,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PeriodPrecision {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

/// Temporal values retain their original precision and kind.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TimeValue {
    Instant(Instant),
    LocalDate(LocalDate),
    LocalDateTime(LocalDateTime),
    Period(Period),
}

/// Open or closed interval endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Bound<T> {
    Open(T),
    Closed(T),
    Unbounded,
}

impl<T> Bound<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Open(value) | Self::Closed(value) => Some(value),
            Self::Unbounded => None,
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed(_))
    }
}

/// An interval whose endpoints are ordered and whose open/closed status is
/// preserved through intersection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Interval<T> {
    start: Bound<T>,
    end: Bound<T>,
}

impl<T: Ord + Clone> Interval<T> {
    pub fn new(start: Bound<T>, end: Bound<T>) -> Result<Self, TimeError> {
        if let (Some(left), Some(right)) = (start.value(), end.value())
            && (left > right || (left == right && (!start.is_closed() || !end.is_closed())))
        {
            return Err(TimeError::EmptyInterval);
        }
        Ok(Self { start, end })
    }

    pub fn closed(start: T, end: T) -> Result<Self, TimeError> {
        Self::new(Bound::Closed(start), Bound::Closed(end))
    }

    pub fn start(&self) -> &Bound<T> {
        &self.start
    }

    pub fn end(&self) -> &Bound<T> {
        &self.end
    }

    pub fn open(start: T, end: T) -> Result<Self, TimeError> {
        Self::new(Bound::Open(start), Bound::Open(end))
    }

    pub fn contains(&self, value: &T) -> bool {
        let after_start = match &self.start {
            Bound::Unbounded => true,
            Bound::Closed(start) => value >= start,
            Bound::Open(start) => value > start,
        };
        let before_end = match &self.end {
            Bound::Unbounded => true,
            Bound::Closed(end) => value <= end,
            Bound::Open(end) => value < end,
        };
        after_start && before_end
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.intersection(other).is_some()
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let start = max_start(&self.start, &other.start);
        let end = min_end(&self.end, &other.end);
        Self::new(start, end).ok()
    }
}

pub type InstantInterval = Interval<Instant>;

/// A bounded uncertainty statement.  It does not collapse to a midpoint.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct UncertainInterval<T> {
    earliest: T,
    latest: T,
}

impl<T: Ord + Clone> UncertainInterval<T> {
    pub fn new(earliest: T, latest: T) -> Result<Self, TimeError> {
        if earliest > latest {
            return Err(TimeError::InvalidUncertainty);
        }
        Ok(Self { earliest, latest })
    }

    pub fn contains(&self, value: &T) -> bool {
        &self.earliest <= value && value <= &self.latest
    }

    pub fn earliest(&self) -> &T {
        &self.earliest
    }

    pub fn latest(&self) -> &T {
        &self.latest
    }

    pub fn as_interval(&self) -> Result<Interval<T>, TimeError> {
        Interval::closed(self.earliest.clone(), self.latest.clone())
    }
}

/// A strict before/after fact.  Equality is intentionally not accepted.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BeforeAfter<T> {
    before: T,
    after: T,
}

impl<T: Ord> BeforeAfter<T> {
    pub fn new(before: T, after: T) -> Result<Self, TimeError> {
        if before >= after {
            return Err(TimeError::InvalidOrdering);
        }
        Ok(Self { before, after })
    }

    pub fn holds(&self) -> bool {
        self.before < self.after
    }

    pub fn before(&self) -> &T {
        &self.before
    }

    pub fn after(&self) -> &T {
        &self.after
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MissingDayPolicy {
    Skip,
    ClampToLastDay,
    RollForward,
    RollBackward,
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum BusinessDayPolicy {
    Unchanged,
    Following,
    Preceding,
    ModifiedFollowing,
}

/// A named business-day definition.  Weekends and holidays are data, not
/// hidden global assumptions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BusinessCalendar {
    pub weekend: BTreeSet<Weekday>,
    pub holidays: BTreeSet<LocalDate>,
}

impl Default for BusinessCalendar {
    fn default() -> Self {
        Self {
            weekend: [Weekday::Saturday, Weekday::Sunday].into_iter().collect(),
            holidays: BTreeSet::new(),
        }
    }
}

impl BusinessCalendar {
    pub fn is_business_day(&self, date: LocalDate) -> bool {
        !self.weekend.contains(&date.weekday()) && !self.holidays.contains(&date)
    }

    pub fn adjust(
        &self,
        date: LocalDate,
        policy: BusinessDayPolicy,
    ) -> Result<LocalDate, TimeError> {
        if policy == BusinessDayPolicy::Unchanged || self.is_business_day(date) {
            return Ok(date);
        }
        let direction = match policy {
            BusinessDayPolicy::Preceding => -1,
            BusinessDayPolicy::Following | BusinessDayPolicy::ModifiedFollowing => 1,
            BusinessDayPolicy::Unchanged => 0,
        };
        let mut candidate = date;
        loop {
            candidate = candidate
                .checked_add_days(direction)
                .ok_or(TimeError::Overflow)?;
            if self.is_business_day(candidate) {
                if policy == BusinessDayPolicy::ModifiedFollowing
                    && candidate.month() != date.month()
                {
                    return self.adjust(date, BusinessDayPolicy::Preceding);
                }
                return Ok(candidate);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Frequency {
    Daily { every: u32 },
    Weekly { every: u32, weekdays: Vec<Weekday> },
    Monthly { every: u32, day: u8 },
    Yearly { every: u32, month: u8, day: u8 },
}

impl Frequency {
    fn validate(&self) -> Result<(), TimeError> {
        match self {
            Self::Daily { every } | Self::Weekly { every, .. } | Self::Monthly { every, .. }
                if *every == 0 =>
            {
                Err(TimeError::InvalidRecurrence)
            }
            Self::Yearly { every, month, day } if *every == 0 || !(1..=12).contains(month) => {
                Err(TimeError::InvalidRecurrence)
            }
            Self::Yearly { day, .. } if *day == 0 || *day > 31 => Err(TimeError::InvalidRecurrence),
            Self::Monthly { day, .. } if *day == 0 || *day > 31 => {
                Err(TimeError::InvalidRecurrence)
            }
            Self::Weekly { weekdays, .. } if weekdays.is_empty() => {
                Err(TimeError::InvalidRecurrence)
            }
            Self::Weekly { weekdays, .. } if duplicate_weekdays(weekdays) => {
                Err(TimeError::InvalidRecurrence)
            }
            _ => Ok(()),
        }
    }
}

/// A bounded recurrence generator.  An absent count/end is legal, but
/// callers must use a bounded query (`between`) to materialize it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recurrence {
    start: LocalDate,
    frequency: Frequency,
    count: Option<usize>,
    until: Option<LocalDate>,
    missing_day: MissingDayPolicy,
    business_day: BusinessDayPolicy,
    calendar: BusinessCalendar,
}

impl Recurrence {
    pub fn new(start: LocalDate, frequency: Frequency) -> Result<Self, TimeError> {
        frequency.validate()?;
        Ok(Self {
            start,
            frequency,
            count: None,
            until: None,
            missing_day: MissingDayPolicy::Reject,
            business_day: BusinessDayPolicy::Unchanged,
            calendar: BusinessCalendar::default(),
        })
    }

    pub fn with_count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn through(mut self, until: LocalDate) -> Result<Self, TimeError> {
        if until < self.start {
            return Err(TimeError::InvalidRecurrence);
        }
        self.until = Some(until);
        Ok(self)
    }

    pub fn with_missing_day_policy(mut self, policy: MissingDayPolicy) -> Self {
        self.missing_day = policy;
        self
    }

    pub fn with_business_days(
        mut self,
        policy: BusinessDayPolicy,
        calendar: BusinessCalendar,
    ) -> Self {
        self.business_day = policy;
        self.calendar = calendar;
        self
    }

    pub fn start(&self) -> LocalDate {
        self.start
    }

    pub fn frequency(&self) -> &Frequency {
        &self.frequency
    }

    pub fn count(&self) -> Option<usize> {
        self.count
    }

    pub fn until(&self) -> Option<LocalDate> {
        self.until
    }

    pub fn missing_day_policy(&self) -> MissingDayPolicy {
        self.missing_day
    }

    pub fn business_day_policy(&self) -> BusinessDayPolicy {
        self.business_day
    }

    pub fn calendar(&self) -> &BusinessCalendar {
        &self.calendar
    }

    /// Materialize occurrences in an explicit date window.  Dates moved by a
    /// business-day policy are still checked against that window.
    pub fn between(
        &self,
        from: LocalDate,
        through: LocalDate,
    ) -> Result<Vec<LocalDate>, TimeError> {
        if from > through {
            return Err(TimeError::InvalidRecurrence);
        }
        let output_through = self.until.map_or(through, |until| through.min(until));
        let generation_through = match &self.frequency {
            Frequency::Weekly { .. } => output_through
                .checked_add_days(6)
                .ok_or(TimeError::Overflow)?,
            _ => output_through,
        };
        let mut result = Vec::new();
        let mut cursor = self.start;
        let mut generated = 0usize;
        let limit = self.count.unwrap_or(usize::MAX);
        while cursor <= generation_through && generated < limit {
            let raw_dates = self.raw_occurrence(generated, cursor)?;
            for raw in raw_dates {
                if raw < self.start || raw > output_through {
                    continue;
                }
                let adjusted = self.calendar.adjust(raw, self.business_day)?;
                if adjusted >= from && adjusted <= output_through {
                    result.push(adjusted);
                }
            }
            generated = generated.checked_add(1).ok_or(TimeError::Overflow)?;
            cursor = match &self.frequency {
                Frequency::Daily { every } => cursor
                    .checked_add_days(i64::from(*every))
                    .ok_or(TimeError::Overflow)?,
                Frequency::Weekly { every, .. } => cursor
                    .checked_add_days(i64::from(*every) * 7)
                    .ok_or(TimeError::Overflow)?,
                Frequency::Monthly { every, .. } => add_months(cursor, *every as i64)?,
                Frequency::Yearly { every, .. } => add_months(cursor, (*every as i64) * 12)?,
            };
        }
        result.sort();
        result.dedup();
        Ok(result)
    }

    fn raw_occurrence(&self, index: usize, cursor: LocalDate) -> Result<Vec<LocalDate>, TimeError> {
        match &self.frequency {
            Frequency::Daily { .. } => Ok(vec![cursor]),
            Frequency::Weekly { weekdays, .. } => {
                let week_start = cursor
                    .checked_add_days(-i64::from(cursor.weekday().monday_index()))
                    .ok_or(TimeError::Overflow)?;
                Ok(weekdays
                    .iter()
                    .filter_map(|weekday| {
                        week_start.checked_add_days(i64::from(weekday.monday_index()))
                    })
                    .collect())
            }
            Frequency::Monthly { day, .. } => {
                let _ = index;
                match resolve_month_day(cursor.year(), cursor.month(), *day, self.missing_day)? {
                    Some(date) => Ok(vec![date]),
                    None => Ok(Vec::new()),
                }
            }
            Frequency::Yearly { month, day, .. } => {
                resolve_month_day(cursor.year(), *month, *day, self.missing_day)
                    .map(|date| date.into_iter().collect())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimeError {
    InvalidDate(String),
    InvalidLocalTime {
        hour: u8,
        minute: u8,
        second: u8,
        nanosecond: u32,
    },
    InvalidPeriod,
    TimezoneRulesRequired,
    AmbiguousLocalTime,
    Overflow,
    EmptyInterval,
    InvalidUncertainty,
    InvalidOrdering,
    InvalidRecurrence,
    MissingDay,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDate(value) => write!(f, "invalid date: {value}"),
            Self::InvalidLocalTime {
                hour,
                minute,
                second,
                nanosecond,
            } => write!(
                f,
                "invalid local time {hour:02}:{minute:02}:{second:02}.{nanosecond:09}"
            ),
            Self::InvalidPeriod => f.write_str("invalid calendar period"),
            Self::TimezoneRulesRequired => f.write_str("named timezone rules are required"),
            Self::AmbiguousLocalTime => f.write_str("local time is ambiguous or skipped"),
            Self::Overflow => f.write_str("temporal arithmetic overflow"),
            Self::EmptyInterval => f.write_str("interval is empty"),
            Self::InvalidUncertainty => f.write_str("uncertain interval is inverted"),
            Self::InvalidOrdering => f.write_str("before/after ordering is not strict"),
            Self::InvalidRecurrence => f.write_str("invalid recurrence"),
            Self::MissingDay => f.write_str("recurrence day does not exist"),
        }
    }
}

impl std::error::Error for TimeError {}

fn max_start<T: Ord + Clone>(left: &Bound<T>, right: &Bound<T>) -> Bound<T> {
    match (left, right) {
        (Bound::Unbounded, value) | (value, Bound::Unbounded) => value.clone(),
        (Bound::Closed(a), Bound::Closed(b)) => {
            if a >= b {
                Bound::Closed(a.clone())
            } else {
                Bound::Closed(b.clone())
            }
        }
        (Bound::Open(a), Bound::Open(b)) => {
            if a >= b {
                Bound::Open(a.clone())
            } else {
                Bound::Open(b.clone())
            }
        }
        (Bound::Open(a), Bound::Closed(b)) => match a.cmp(b) {
            std::cmp::Ordering::Greater => Bound::Open(a.clone()),
            std::cmp::Ordering::Less => Bound::Closed(b.clone()),
            std::cmp::Ordering::Equal => Bound::Open(a.clone()),
        },
        (Bound::Closed(a), Bound::Open(b)) => match a.cmp(b) {
            std::cmp::Ordering::Greater => Bound::Closed(a.clone()),
            std::cmp::Ordering::Less => Bound::Open(b.clone()),
            std::cmp::Ordering::Equal => Bound::Open(a.clone()),
        },
    }
}

fn min_end<T: Ord + Clone>(left: &Bound<T>, right: &Bound<T>) -> Bound<T> {
    match (left, right) {
        (Bound::Unbounded, value) | (value, Bound::Unbounded) => value.clone(),
        (Bound::Closed(a), Bound::Closed(b)) => {
            if a <= b {
                Bound::Closed(a.clone())
            } else {
                Bound::Closed(b.clone())
            }
        }
        (Bound::Open(a), Bound::Open(b)) => {
            if a <= b {
                Bound::Open(a.clone())
            } else {
                Bound::Open(b.clone())
            }
        }
        (Bound::Open(a), Bound::Closed(b)) => match a.cmp(b) {
            std::cmp::Ordering::Less => Bound::Open(a.clone()),
            std::cmp::Ordering::Greater => Bound::Closed(b.clone()),
            std::cmp::Ordering::Equal => Bound::Open(a.clone()),
        },
        (Bound::Closed(a), Bound::Open(b)) => match a.cmp(b) {
            std::cmp::Ordering::Less => Bound::Closed(a.clone()),
            std::cmp::Ordering::Greater => Bound::Open(b.clone()),
            std::cmp::Ordering::Equal => Bound::Open(a.clone()),
        },
    }
}

fn duplicate_weekdays(days: &[Weekday]) -> bool {
    let mut seen = BTreeSet::new();
    days.iter().any(|day| !seen.insert(*day))
}

fn resolve_month_day(
    year: i32,
    month: u8,
    day: u8,
    policy: MissingDayPolicy,
) -> Result<Option<LocalDate>, TimeError> {
    let last = days_in_month(year, month);
    if day <= last {
        return new_local_date(year, month, day).map(Some);
    }
    match policy {
        MissingDayPolicy::Skip => Ok(None),
        MissingDayPolicy::ClampToLastDay => new_local_date(year, month, last).map(Some),
        MissingDayPolicy::RollForward => new_local_date(year, month, last)?
            .checked_add_days(i64::from(day - last))
            .ok_or(TimeError::Overflow)
            .map(Some),
        MissingDayPolicy::RollBackward => new_local_date(year, month, last)?
            .checked_add_days(-i64::from(day - last))
            .ok_or(TimeError::Overflow)
            .map(Some),
        MissingDayPolicy::Reject => Err(TimeError::MissingDay),
    }
}

fn add_months(date: LocalDate, months: i64) -> Result<LocalDate, TimeError> {
    let index = i64::from(date.year()) * 12 + i64::from(date.month() - 1);
    let target = index.checked_add(months).ok_or(TimeError::Overflow)?;
    let year = target.div_euclid(12);
    let month = target.rem_euclid(12) as u8 + 1;
    let year = i32::try_from(year).map_err(|_| TimeError::Overflow)?;
    let day = date.day().min(days_in_month(year, month));
    new_local_date(year, month, day)
}

fn new_local_date(year: i32, month: u8, day: u8) -> Result<LocalDate, TimeError> {
    LocalDate::new(year, month, day).map_err(|error| TimeError::InvalidDate(error.to_string()))
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// Howard Hinnant's civil-calendar algorithms, expressed with i128 so the
// intermediate arithmetic cannot overflow for the i32 year domain.
fn days_from_civil(year: i32, month: u8, day: u8) -> i128 {
    let year = i128::from(year) - if month <= 2 { 1 } else { 0 };
    let era = (if year >= 0 { year } else { year - 399 }).div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i128::from(month);
    let day = i128::from(day);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u8, day: u8) -> LocalDate {
        LocalDate::new(year, month, day).unwrap()
    }

    #[test]
    fn fixed_offsets_are_exact_but_named_zones_are_not_invented() {
        let local = LocalDateTime::new(
            date(2026, 9, 21),
            LocalTime::new(12, 0, 0, 0).unwrap(),
            TimeZone::FixedOffsetSeconds(0),
        );
        let expected =
            days_from_civil(2026, 9, 21) * 86_400 * 1_000_000_000 + 12 * 3_600 * 1_000_000_000;
        assert_eq!(
            local.to_instant().unwrap(),
            Instant::from_unix_nanos(expected)
        );
    }

    #[test]
    fn ambiguous_dst_label_remains_ambiguous_without_timezone_database() {
        let local = LocalDateTime::ambiguous(
            date(2026, 11, 1),
            LocalTime::new(1, 30, 0, 0).unwrap(),
            "America/New_York",
        );
        assert_eq!(local.status(), LocalTimeStatus::Ambiguous);
        assert_eq!(local.to_instant(), Err(TimeError::TimezoneRulesRequired));
    }

    #[test]
    fn interval_intersection_preserves_open_endpoints() {
        let left = Interval::closed(1, 5).unwrap();
        let right = Interval::new(Bound::Open(5), Bound::Closed(9)).unwrap();
        assert!(!left.intersects(&right));
        let overlap = Interval::new(Bound::Open(3), Bound::Closed(8)).unwrap();
        let intersection = left.intersection(&overlap).unwrap();
        assert_eq!(
            intersection,
            Interval::new(Bound::Open(3), Bound::Closed(5)).unwrap()
        );
    }

    #[test]
    fn coarse_month_does_not_become_a_date() {
        let period = Period::month(2024, 2).unwrap();
        assert_eq!(period.precision(), PeriodPrecision::Month);
        assert_eq!(TimeValue::Period(period), TimeValue::Period(period));
    }

    #[test]
    fn recurrence_exposes_missing_day_policy() {
        let recurrence =
            Recurrence::new(date(2024, 1, 31), Frequency::Monthly { every: 1, day: 31 })
                .unwrap()
                .with_missing_day_policy(MissingDayPolicy::Skip)
                .through(date(2024, 3, 31))
                .unwrap();
        assert_eq!(
            recurrence
                .between(date(2024, 1, 1), date(2024, 3, 31))
                .unwrap(),
            vec![date(2024, 1, 31), date(2024, 3, 31)]
        );
    }

    #[test]
    fn weekly_recurrence_never_emits_before_its_start() {
        let recurrence = Recurrence::new(
            date(2024, 1, 3),
            Frequency::Weekly {
                every: 1,
                weekdays: vec![Weekday::Monday],
            },
        )
        .unwrap();
        assert_eq!(
            recurrence
                .between(date(2024, 1, 1), date(2024, 1, 8))
                .unwrap(),
            vec![date(2024, 1, 8)]
        );
    }

    #[test]
    fn business_day_adjustment_is_named_and_deterministic() {
        let calendar = BusinessCalendar::default();
        let saturday = date(2024, 1, 6);
        assert_eq!(
            calendar
                .adjust(saturday, BusinessDayPolicy::Following)
                .unwrap(),
            date(2024, 1, 8)
        );
    }

    #[test]
    fn strict_before_after_and_uncertainty_are_checked() {
        assert!(BeforeAfter::new(1, 2).is_ok());
        assert!(BeforeAfter::new(2, 2).is_err());
        assert!(UncertainInterval::new(2, 1).is_err());
    }
}
