//! Small, typed domain model for the canonical ledger.
//!
//! The model intentionally stops at the evidence boundary.  Resolution and
//! recognition belong to the engine; these values are the stable, boring
//! things that the parser and every later view can share.

use core::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use blake3::Hasher;

use crate::exact::{ExactError, ExactNumber};

/// A 32-byte content address.  It is deliberately not an occurrence ID:
/// equal contents can occur more than once in a ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_digest(digest: blake3::Hash) -> Self {
        Self(*digest.as_bytes())
    }

    pub fn domain_separated(domain: &str, bytes: &[u8]) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(domain.as_bytes());
        hasher.update(&[0]);
        hasher.update(bytes);
        Self::from_digest(hasher.finalize())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(source: &str) -> Result<Self, ModelError> {
        if source.len() != 64 {
            return Err(ModelError::InvalidHash(source.to_string()));
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in source.as_bytes().chunks_exact(2).enumerate() {
            let text = std::str::from_utf8(chunk)
                .map_err(|_| ModelError::InvalidHash(source.to_string()))?;
            bytes[index] = u8::from_str_radix(text, 16)
                .map_err(|_| ModelError::InvalidHash(source.to_string()))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for ContentHash {
    type Err = ModelError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::from_hex(source)
    }
}

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(String);

        impl $name {
            /// Construct an ID, rejecting empty or whitespace-only values.
            ///
            /// The infallible form is kept for the many typed-ID call sites
            /// that already receive a validated source token.  Callers that
            /// handle external input should prefer [`Self::try_new`].
            pub fn new(value: impl Into<String>) -> Self {
                Self::try_new(value).expect(concat!(stringify!($name), " must not be empty"))
            }

            pub fn try_new(value: impl Into<String>) -> Result<Self, ModelError> {
                let value = value.into();
                if value.trim().is_empty() {
                    Err(ModelError::EmptyId(stringify!($name)))
                } else {
                    Ok(Self(value))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }

            pub fn is_empty(&self) -> bool {
                self.0.trim().is_empty()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ModelError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_new(value)
            }
        }
    };
}

typed_id!(AccountId);
typed_id!(BookId);
typed_id!(DecisionId);
typed_id!(EntityId);
typed_id!(ExternalId);
typed_id!(InstrumentId);
typed_id!(LotId);
typed_id!(OccurrenceId);
typed_id!(PolicyId);
typed_id!(QuoteId);
typed_id!(SourceId);
typed_id!(UnitId);
typed_id!(VenueId);

/// A general identity relation carried by an observed or authored value.
///
/// `occurrence` answers “which instance?”; `content` answers “which normalized
/// value?”; `external` retains the source system's own identifier.  None of
/// these fields may be inferred from the others.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Identity {
    pub occurrence: OccurrenceId,
    pub content: ContentHash,
    pub external: Option<ExternalId>,
}

impl Identity {
    pub fn new(occurrence: impl Into<OccurrenceId>, content: ContentHash) -> Self {
        Self {
            occurrence: occurrence.into(),
            content,
            external: None,
        }
    }

    pub fn with_external(mut self, external: impl Into<ExternalId>) -> Self {
        self.external = Some(external.into());
        self
    }
}

/// A value with its identity kept next to it, without conflating either form
/// of identity with equality of the value itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identified<T> {
    pub identity: Identity,
    pub value: T,
}

impl<T> Identified<T> {
    pub fn new(identity: Identity, value: T) -> Self {
        Self { identity, value }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Identified<U> {
        Identified {
            identity: self.identity,
            value: map(self.value),
        }
    }
}

/// A source date.  Time zones and richer temporal roles belong in later
/// evidence forms; the constrained V0 grammar intentionally uses civil dates.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub fn is_weekend(self) -> bool {
        matches!(self, Self::Saturday | Self::Sunday)
    }

    pub(crate) fn monday_index(self) -> u8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, ModelError> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(ModelError::InvalidDate { year, month, day });
        }
        Ok(Self { year, month, day })
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }

    pub fn checked_add_days(self, days: i64) -> Option<Self> {
        let serial = date_days_from_civil(self.year, self.month, self.day);
        let serial = serial.checked_add(days as i128)?;
        let (year, month, day) = date_civil_from_days(serial);
        Self::new(year, month, day).ok()
    }

    pub fn weekday(self) -> Weekday {
        match (date_days_from_civil(self.year, self.month, self.day) + 4).rem_euclid(7) {
            0 => Weekday::Sunday,
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            _ => Weekday::Saturday,
        }
    }
}

impl FromStr for Date {
    type Err = ModelError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let mut pieces = source.split('-');
        let year = pieces
            .next()
            .ok_or_else(|| ModelError::InvalidDateText(source.to_string()))?
            .parse()
            .map_err(|_| ModelError::InvalidDateText(source.to_string()))?;
        let month = pieces
            .next()
            .ok_or_else(|| ModelError::InvalidDateText(source.to_string()))?
            .parse()
            .map_err(|_| ModelError::InvalidDateText(source.to_string()))?;
        let day = pieces
            .next()
            .ok_or_else(|| ModelError::InvalidDateText(source.to_string()))?
            .parse()
            .map_err(|_| ModelError::InvalidDateText(source.to_string()))?;
        if pieces.next().is_some() {
            return Err(ModelError::InvalidDateText(source.to_string()));
        }
        Self::new(year, month, day)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// A unit is nominal.  `USD`, `ABC`, and `brokerage` are not interchangeable
/// merely because they happen to have the same spelling in another context.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Unit(String);

impl Unit {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(ModelError::EmptyUnit)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Unit {
    type Err = ModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for Unit {
    type Error = ModelError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A quantity is exact and unit-bearing, except that zero may be left
/// polymorphic.  This models the ledger rule that every non-zero number must
/// name its unit without making `0` awkward in a generic equation.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Quantity {
    pub number: ExactNumber,
    pub unit: Option<Unit>,
}

impl Quantity {
    pub fn new(number: ExactNumber, unit: Option<Unit>) -> Result<Self, ModelError> {
        if !number.is_zero() && unit.is_none() {
            return Err(ModelError::UnitRequired);
        }
        Ok(Self { number, unit })
    }

    pub fn with_unit(number: ExactNumber, unit: impl Into<String>) -> Result<Self, ModelError> {
        Self::new(number, Some(Unit::new(unit)?))
    }

    pub fn typed(number: ExactNumber, unit: impl Into<Unit>) -> Self {
        Self {
            number,
            unit: Some(unit.into()),
        }
    }

    pub fn zero() -> Self {
        Self {
            number: ExactNumber::integer(0),
            unit: None,
        }
    }

    pub fn unit(&self) -> Option<&Unit> {
        self.unit.as_ref()
    }

    pub fn amount(&self) -> &ExactNumber {
        &self.number
    }

    pub fn is_zero(&self) -> bool {
        self.number.is_zero()
    }

    pub fn canonical(&self) -> String {
        let unit = self
            .unit
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "?".into());
        format!("{} {unit}", self.number.canonical_string())
    }

    pub fn checked_add(&self, rhs: &Self) -> Result<Self, ModelError> {
        let unit = compatible_unit(
            self.unit.as_ref(),
            rhs.unit.as_ref(),
            self.is_zero(),
            rhs.is_zero(),
        )?;
        Self::new(&self.number + &rhs.number, unit.cloned())
    }

    pub fn checked_sub(&self, rhs: &Self) -> Result<Self, ModelError> {
        let unit = compatible_unit(
            self.unit.as_ref(),
            rhs.unit.as_ref(),
            self.is_zero(),
            rhs.is_zero(),
        )?;
        Self::new(&self.number - &rhs.number, unit.cloned())
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.number.fmt(formatter)?;
        if let Some(unit) = &self.unit {
            write!(formatter, " {unit}")?;
        }
        Ok(())
    }
}

/// `?name` and anonymous `?` are typed existential holes, not magic values.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Hole {
    Anonymous,
    Named(String),
}

impl Hole {
    pub fn named(name: impl Into<String>) -> Result<Self, ModelError> {
        let name = name.into();
        if name.trim().is_empty() {
            Err(ModelError::EmptyHole)
        } else {
            Ok(Self::Named(name))
        }
    }
}

impl fmt::Display for Hole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("?"),
            Self::Named(name) => write!(formatter, "?{name}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum LotSelector {
    Explicit(LotId),
    Hole(Hole),
}

impl LotSelector {
    pub fn explicit(id: impl Into<LotId>) -> Self {
        Self::Explicit(id.into())
    }

    pub fn hole(name: Option<impl Into<String>>) -> Result<Self, ModelError> {
        match name {
            Some(name) => Ok(Self::Hole(Hole::named(name)?)),
            None => Ok(Self::Hole(Hole::Anonymous)),
        }
    }
}

fn require_positive_quantity(
    quantity: Quantity,
    field: &'static str,
) -> Result<Quantity, ModelError> {
    if quantity.number.is_zero() || quantity.number.is_negative() {
        Err(ModelError::QuantityMustBePositive(field))
    } else {
        Ok(quantity)
    }
}

fn require_non_negative_amount(
    amount: Quantity,
    field: &'static str,
) -> Result<Quantity, ModelError> {
    if amount.number.is_negative() {
        Err(ModelError::AmountMustBeNonNegative(field))
    } else {
        Ok(amount)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buy {
    pub occurrence: OccurrenceId,
    pub label: String,
    pub date: Date,
    pub quantity: Quantity,
    pub into: AccountId,
    pub cost: Quantity,
    pub fee: Option<Quantity>,
}

impl Buy {
    pub fn new(
        label: impl Into<String>,
        date: Date,
        quantity: Quantity,
        into: impl Into<AccountId>,
        cost: Quantity,
        fee: Option<Quantity>,
    ) -> Result<Self, ModelError> {
        let label = label.into();
        Ok(Self {
            occurrence: OccurrenceId::try_new(label.clone())?,
            label,
            date,
            quantity: require_positive_quantity(quantity, "buy quantity")?,
            into: into.into(),
            cost: require_non_negative_amount(cost, "buy cost")?,
            fee: fee
                .map(|fee| require_non_negative_amount(fee, "buy fee"))
                .transpose()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sell {
    pub occurrence: OccurrenceId,
    pub label: String,
    pub date: Date,
    pub quantity: Quantity,
    pub from: AccountId,
    pub proceeds: Quantity,
    pub lot: LotSelector,
}

impl Sell {
    pub fn new(
        label: impl Into<String>,
        date: Date,
        quantity: Quantity,
        from: impl Into<AccountId>,
        proceeds: Quantity,
        lot: LotSelector,
    ) -> Result<Self, ModelError> {
        let label = label.into();
        Ok(Self {
            occurrence: OccurrenceId::try_new(label.clone())?,
            label,
            date,
            quantity: require_positive_quantity(quantity, "sell quantity")?,
            from: from.into(),
            proceeds: require_non_negative_amount(proceeds, "sell proceeds")?,
            lot,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    pub occurrence: OccurrenceId,
    pub label: String,
    pub date: Date,
    pub base: Quantity,
    pub counter: Quantity,
}

impl Quote {
    pub fn new(label: impl Into<String>, date: Date, base: Quantity, counter: Quantity) -> Self {
        let label = label.into();
        Self {
            occurrence: OccurrenceId::new(label.clone()),
            label,
            date,
            base,
            counter,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionObservation {
    /// A source occurrence identity.  Equal account/quantity rows are still
    /// distinct observations when they came from different source entries.
    pub occurrence: OccurrenceId,
    pub account: AccountId,
    pub quantity: Quantity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementObservation {
    /// A source occurrence identity, independent of the referenced sale and
    /// normalized amount so duplicate settlement rows remain representable.
    pub occurrence: OccurrenceId,
    pub sale: OccurrenceId,
    pub amount: Quantity,
    pub into: Option<AccountId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyUse {
    pub book: BookId,
    pub policy: PolicyId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub sale: OccurrenceId,
    pub lot: LotId,
}

/// The fixed economic entries supported by the V0 source language.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LedgerForm {
    Buy(Buy),
    Sell(Sell),
    Quote(Quote),
    ObservePosition(PositionObservation),
    ObserveSettlement(SettlementObservation),
    UsePolicy(PolicyUse),
    Decide(Decision),
}

impl LedgerForm {
    pub fn occurrence(&self) -> Option<&OccurrenceId> {
        match self {
            Self::Buy(value) => Some(&value.occurrence),
            Self::Sell(value) => Some(&value.occurrence),
            Self::Quote(value) => Some(&value.occurrence),
            Self::ObservePosition(value) => Some(&value.occurrence),
            Self::ObserveSettlement(value) => Some(&value.occurrence),
            Self::UsePolicy(_) | Self::Decide(_) => None,
        }
    }
}

/// The parsed canonical source ledger.  The source entries, in order, remain
/// the authoritative data; every report is a view over this sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ledger {
    pub book: BookId,
    pub forms: Vec<LedgerForm>,
}

impl Ledger {
    pub fn new(book: impl Into<BookId>) -> Self {
        Self {
            book: book.into(),
            forms: Vec::new(),
        }
    }

    pub fn push(&mut self, form: LedgerForm) {
        self.forms.push(form);
    }

    pub fn iter(&self) -> impl Iterator<Item = &LedgerForm> {
        self.forms.iter()
    }
}

/// A typed wrapper for phase-indexed values.  It is deliberately tiny: the
/// marker prevents accidental phase mixing without imposing an inheritance
/// hierarchy on the economic model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhaseValue<P, T> {
    pub value: T,
    marker: PhantomData<P>,
}

impl<P, T> PhaseValue<P, T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value,
            marker: PhantomData,
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> PhaseValue<P, U> {
        PhaseValue::new(map(self.value))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ObservedPhase {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CandidatePhase {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AcceptedPhase {}

/// The book marker makes a recognized value impossible to use for the wrong
/// book without an explicit projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RecognizedPhase<B>(PhantomData<B>);

pub type Observed<T> = PhaseValue<ObservedPhase, T>;
pub type Candidate<T> = PhaseValue<CandidatePhase, T>;
pub type Accepted<T> = PhaseValue<AcceptedPhase, T>;
pub type Recognized<Book, T> = PhaseValue<RecognizedPhase<Book>, T>;

/// Public model errors shared by the parser and domain value builders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    EmptyId(&'static str),
    EmptyUnit,
    EmptyHole,
    UnitRequired,
    UnitMismatch { left: String, right: String },
    QuantityMustBePositive(&'static str),
    AmountMustBeNonNegative(&'static str),
    InvalidDate { year: i32, month: u8, day: u8 },
    InvalidDateText(String),
    InvalidHash(String),
    Numeric(ExactError),
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId(kind) => write!(formatter, "empty {kind}"),
            Self::EmptyUnit => formatter.write_str("empty unit"),
            Self::EmptyHole => formatter.write_str("empty named hole"),
            Self::UnitRequired => formatter.write_str("a non-zero quantity requires a unit"),
            Self::UnitMismatch { left, right } => {
                write!(formatter, "unit mismatch: {left} versus {right}")
            }
            Self::QuantityMustBePositive(field) => {
                write!(formatter, "{field} must be greater than zero")
            }
            Self::AmountMustBeNonNegative(field) => {
                write!(formatter, "{field} must be non-negative")
            }
            Self::InvalidDate { year, month, day } => {
                write!(formatter, "invalid date {year:04}-{month:02}-{day:02}")
            }
            Self::InvalidDateText(value) => write!(formatter, "invalid date: {value}"),
            Self::InvalidHash(value) => write!(formatter, "invalid content hash: {value}"),
            Self::Numeric(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<ExactError> for ModelError {
    fn from(error: ExactError) -> Self {
        Self::Numeric(error)
    }
}

fn compatible_unit<'a>(
    left: Option<&'a Unit>,
    right: Option<&'a Unit>,
    left_zero: bool,
    right_zero: bool,
) -> Result<Option<&'a Unit>, ModelError> {
    match (left, right) {
        (Some(left), Some(right)) if left != right => Err(ModelError::UnitMismatch {
            left: left.to_string(),
            right: right.to_string(),
        }),
        (Some(unit), _) => Ok(Some(unit)),
        (_, Some(unit)) => Ok(Some(unit)),
        (None, None) if left_zero && right_zero => Ok(None),
        (None, None) => Err(ModelError::UnitRequired),
    }
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

fn date_days_from_civil(year: i32, month: u8, day: u8) -> i128 {
    let year = i128::from(year) - if month <= 2 { 1 } else { 0 };
    let era = (if year >= 0 { year } else { year - 399 }).div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i128::from(month);
    let day_of_year =
        (153 * (month + if month > 2 { -3 } else { 9 }) + 2).div_euclid(5) + i128::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn date_civil_from_days(days: i128) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = (if days >= 0 { days } else { days - 146_096 }).div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_prime + 2).div_euclid(5) + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_and_contents_are_independent() {
        let content = ContentHash::domain_separated("test", b"same");
        let first = Identity::new("first", content);
        let second = Identity::new("second", content);
        assert_ne!(first.occurrence, second.occurrence);
        assert_eq!(first.content, second.content);
    }

    #[test]
    fn dates_and_units_are_checked_at_the_boundary() {
        assert!("2024-02-29".parse::<Date>().is_ok());
        assert!("2023-02-29".parse::<Date>().is_err());
        let unit = Unit::new("USD").unwrap();
        let value = Quantity::new("1.00".parse().unwrap(), Some(unit.clone())).unwrap();
        assert_eq!(value.to_string(), "1.00 USD");
        assert!(Quantity::new("1".parse().unwrap(), None).is_err());
        assert!(Quantity::new("0".parse().unwrap(), None).is_ok());
    }

    #[test]
    fn typed_ids_are_not_plain_strings() {
        let account = AccountId::new("checking");
        let book = BookId::new("tax-us");
        assert_eq!(account.as_str(), "checking");
        assert_eq!(book.to_string(), "tax-us");
        assert!(AccountId::try_new(" ").is_err());
        assert!(std::panic::catch_unwind(|| AccountId::new(" ")).is_err());
    }

    #[test]
    fn trade_builders_reject_invalid_economics() {
        let date: Date = "2026-01-01".parse().unwrap();
        let quantity = Quantity::with_unit("1".parse().unwrap(), "ABC").unwrap();
        let cost = Quantity::with_unit("1".parse().unwrap(), "USD").unwrap();
        let negative = Quantity::with_unit("-1".parse().unwrap(), "USD").unwrap();
        let lot = LotSelector::Hole(Hole::Anonymous);

        let error = Buy::new(
            "buy/one",
            date,
            Quantity::zero(),
            "checking",
            cost.clone(),
            None,
        )
        .expect_err("zero acquisition quantity");
        assert_eq!(error, ModelError::QuantityMustBePositive("buy quantity"));

        let error = Buy::new(
            "buy/two",
            date,
            quantity.clone(),
            "checking",
            negative.clone(),
            None,
        )
        .expect_err("negative cost");
        assert_eq!(error, ModelError::AmountMustBeNonNegative("buy cost"));

        let error =
            Sell::new("sell/one", date, quantity, "checking", cost, lot).expect("valid sale");
        assert_eq!(error.label, "sell/one");

        let error = Sell::new(
            "sell/two",
            date,
            Quantity::with_unit("1".parse().unwrap(), "ABC").unwrap(),
            "checking",
            negative,
            LotSelector::Hole(Hole::Anonymous),
        )
        .expect_err("negative proceeds");
        assert_eq!(error, ModelError::AmountMustBeNonNegative("sell proceeds"));
    }

    #[test]
    fn observations_keep_equal_rows_distinct_by_occurrence() {
        let quantity = Quantity::with_unit("1".parse().unwrap(), "USD").unwrap();
        let first = PositionObservation {
            occurrence: OccurrenceId::new("position/1"),
            account: AccountId::new("checking"),
            quantity: quantity.clone(),
        };
        let second = PositionObservation {
            occurrence: OccurrenceId::new("position/2"),
            account: AccountId::new("checking"),
            quantity,
        };
        assert_ne!(first, second);
        assert_eq!(first.account, second.account);
        assert_eq!(first.quantity, second.quantity);
    }
}
