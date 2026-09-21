//! Nominal instruments, exact quantities, and explicit valuation.
//!
//! Units are identities, not strings that happen to compare equal.  A
//! conversion is an exact, inspectable path of ratios.  Nothing in this
//! module rounds a quantity implicitly; a rounding operation produces a
//! certificate that can be checked independently.

use core::fmt;
use std::collections::BTreeSet;

use crate::exact::{ExactError, ExactNumber, RoundingMode};
use crate::model::{InstrumentId, QuoteId, SourceId, UnitId, VenueId};
pub use crate::model::{Quantity, Unit};
use crate::time::{Bound, Instant, InstantInterval, TimeError};

/// A nominal unit tied to the instrument that issues or defines it.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InstrumentUnit {
    pub id: UnitId,
    pub instrument: InstrumentId,
}

impl InstrumentUnit {
    pub fn new(id: impl Into<UnitId>, instrument: impl Into<InstrumentId>) -> Self {
        Self {
            id: id.into(),
            instrument: instrument.into(),
        }
    }

    pub fn unit_id(&self) -> &UnitId {
        &self.id
    }

    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument
    }

    /// Convert this operational nominal identity into the canonical model
    /// unit.  The instrument suffix is retained so equal labels from
    /// different instruments cannot unify accidentally.
    pub fn as_model_unit(&self) -> Unit {
        Unit::new(format!("{}@{}", self.id, self.instrument))
            .expect("validated nominal identities produce nonempty units")
    }

    pub fn from_model_unit(unit: &Unit) -> Result<Self, UnitError> {
        let (id, instrument) = unit
            .as_str()
            .split_once('@')
            .ok_or_else(|| UnitError::InvalidCanonicalUnit(unit.clone()))?;
        if id.is_empty() || instrument.is_empty() {
            return Err(UnitError::InvalidCanonicalUnit(unit.clone()));
        }
        Ok(Self::new(id.to_owned(), instrument.to_owned()))
    }
}

impl fmt::Display for InstrumentUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.id, self.instrument)
    }
}

impl From<InstrumentUnit> for Unit {
    fn from(value: InstrumentUnit) -> Self {
        value.as_model_unit()
    }
}

impl PartialEq<InstrumentUnit> for Unit {
    fn eq(&self, other: &InstrumentUnit) -> bool {
        self == &other.as_model_unit()
    }
}

impl PartialEq<Unit> for InstrumentUnit {
    fn eq(&self, other: &Unit) -> bool {
        other == self
    }
}

/// An exact exchange ratio: one `from` unit is worth `value` `to` units.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Ratio {
    from: InstrumentUnit,
    to: InstrumentUnit,
    value: ExactNumber,
}

impl Ratio {
    pub fn new(from: InstrumentUnit, to: InstrumentUnit, value: ExactNumber) -> Self {
        Self::checked_new(from, to, value).expect("exchange ratio must be strictly positive")
    }

    /// Construct a ratio after enforcing the economic invariant that an
    /// exchange rate is strictly positive.
    ///
    /// [`Ratio::new`] remains available for raw boundaries where validation
    /// is performed separately. Callers accepting external values should
    /// use this checked constructor.
    pub fn checked_new(
        from: InstrumentUnit,
        to: InstrumentUnit,
        value: ExactNumber,
    ) -> Result<Self, UnitError> {
        let ratio = Self { from, to, value };
        ratio.validate()?;
        Ok(ratio)
    }

    /// Alias for [`Ratio::checked_new`] following the `new_checked` naming
    /// used by other proof-carrying constructors in the crate.
    pub fn new_checked(
        from: InstrumentUnit,
        to: InstrumentUnit,
        value: ExactNumber,
    ) -> Result<Self, UnitError> {
        Self::checked_new(from, to, value)
    }

    /// Fallible-constructor spelling convenient at parsing boundaries.
    pub fn try_new(
        from: InstrumentUnit,
        to: InstrumentUnit,
        value: ExactNumber,
    ) -> Result<Self, UnitError> {
        Self::checked_new(from, to, value)
    }

    pub fn from(&self) -> &InstrumentUnit {
        &self.from
    }

    pub fn to(&self) -> &InstrumentUnit {
        &self.to
    }

    pub fn value(&self) -> &ExactNumber {
        &self.value
    }

    fn validate(&self) -> Result<(), UnitError> {
        if self.value <= ExactNumber::integer(0) {
            Err(UnitError::NonPositiveRatio)
        } else {
            Ok(())
        }
    }

    pub fn inverse(&self) -> Result<Self, UnitError> {
        self.validate()?;
        Ok(Self {
            from: self.to.clone(),
            to: self.from.clone(),
            value: ExactNumber::integer(1).checked_div(&self.value)?,
        })
    }

    pub fn equivalent(&self, other: &Self) -> bool {
        self.from == other.from && self.to == other.to && self.value == other.value
    }

    pub fn apply(&self, quantity: &Quantity) -> Result<Quantity, UnitError> {
        let expected = self.from.as_model_unit();
        if quantity.unit() != Some(&expected) {
            return Err(UnitError::UnitMismatch {
                expected: Box::new(self.from.clone()),
                found: quantity.unit().cloned().map(Box::new),
            });
        }
        Ok(Quantity::typed(
            quantity.number.checked_mul(&self.value),
            self.to.clone(),
        ))
    }
}

/// One leg in a conversion path.  A quote identifier remains attached even
/// when a leg is traversed in reverse.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConversionLeg {
    pub ratio: Ratio,
    pub quote: Option<QuoteId>,
    pub reversed: bool,
}

impl ConversionLeg {
    pub fn direct(ratio: Ratio, quote: Option<QuoteId>) -> Self {
        Self {
            ratio,
            quote,
            reversed: false,
        }
    }

    pub fn reversed(ratio: Ratio, quote: Option<QuoteId>) -> Result<Self, UnitError> {
        Ok(Self {
            ratio: ratio.inverse()?,
            quote,
            reversed: true,
        })
    }
}

/// A conversion retains every route leg and its exact compounded rate.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConversionPath {
    from: InstrumentUnit,
    to: InstrumentUnit,
    legs: Vec<ConversionLeg>,
    rate: ExactNumber,
}

impl ConversionPath {
    pub fn identity(unit: InstrumentUnit) -> Self {
        Self {
            from: unit.clone(),
            to: unit,
            legs: Vec::new(),
            rate: ExactNumber::integer(1),
        }
    }

    pub fn from(&self) -> &InstrumentUnit {
        &self.from
    }

    pub fn to(&self) -> &InstrumentUnit {
        &self.to
    }

    pub fn legs(&self) -> &[ConversionLeg] {
        &self.legs
    }

    pub fn rate(&self) -> &ExactNumber {
        &self.rate
    }

    pub fn new(legs: Vec<ConversionLeg>) -> Result<Self, UnitError> {
        let first = legs.first().ok_or(UnitError::EmptyConversionPath)?;
        let from = first.ratio.from.clone();
        let mut current = from.clone();
        let mut rate = ExactNumber::integer(1);
        for leg in &legs {
            leg.ratio.validate()?;
            if leg.ratio.from != current {
                return Err(UnitError::BrokenConversionPath);
            }
            current = leg.ratio.to.clone();
            rate = rate.checked_mul(&leg.ratio.value);
        }
        Ok(Self {
            from,
            to: current,
            legs,
            rate,
        })
    }

    pub fn convert(&self, quantity: &Quantity) -> Result<Quantity, UnitError> {
        let expected = self.from.as_model_unit();
        if quantity.unit() != Some(&expected) {
            return Err(UnitError::UnitMismatch {
                expected: Box::new(self.from.clone()),
                found: quantity.unit().cloned().map(Box::new),
            });
        }
        Ok(Quantity::typed(
            quantity.number.checked_mul(&self.rate),
            self.to.clone(),
        ))
    }
}

/// Canonical instrument quantum.  It is an exact divisibility constraint,
/// not an instruction to round values that fail it.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InstrumentDefinition {
    instrument: InstrumentId,
    unit: InstrumentUnit,
    canonical_quantum: ExactNumber,
}

impl InstrumentDefinition {
    pub fn new(
        instrument: impl Into<InstrumentId>,
        unit: InstrumentUnit,
        canonical_quantum: ExactNumber,
    ) -> Result<Self, UnitError> {
        let instrument = instrument.into();
        if unit.instrument_id() != &instrument {
            return Err(UnitError::InstrumentMismatch {
                expected: Box::new(instrument),
                found: Box::new(unit.instrument_id().clone()),
            });
        }
        if canonical_quantum <= ExactNumber::integer(0) {
            return Err(UnitError::InvalidQuantum);
        }
        Ok(Self {
            instrument,
            unit,
            canonical_quantum,
        })
    }

    pub fn accepts(&self, quantity: &Quantity) -> Result<(), UnitError> {
        Quantum::new(self.unit.clone(), self.canonical_quantum.clone())?.accepts(quantity)
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn unit(&self) -> &InstrumentUnit {
        &self.unit
    }

    pub fn canonical_quantum(&self) -> &ExactNumber {
        &self.canonical_quantum
    }
}

/// Operational precision imposed by a particular venue or custodian.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VenueQuantum {
    venue: VenueId,
    quantum: Quantum,
}

impl VenueQuantum {
    pub fn new(
        venue: impl Into<VenueId>,
        unit: InstrumentUnit,
        amount: ExactNumber,
    ) -> Result<Self, UnitError> {
        Ok(Self {
            venue: venue.into(),
            quantum: Quantum::new(unit, amount)?,
        })
    }

    pub fn accepts(&self, quantity: &Quantity) -> Result<(), UnitError> {
        self.quantum.accepts(quantity)
    }

    pub fn venue(&self) -> &VenueId {
        &self.venue
    }

    pub fn quantum(&self) -> &Quantum {
        &self.quantum
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Quantum {
    unit: InstrumentUnit,
    amount: ExactNumber,
}

impl Quantum {
    pub fn new(unit: InstrumentUnit, amount: ExactNumber) -> Result<Self, UnitError> {
        if amount <= ExactNumber::integer(0) {
            return Err(UnitError::InvalidQuantum);
        }
        Ok(Self { unit, amount })
    }

    pub fn accepts(&self, quantity: &Quantity) -> Result<(), UnitError> {
        let expected = self.unit.as_model_unit();
        if quantity.unit() != Some(&expected) {
            return Err(UnitError::UnitMismatch {
                expected: Box::new(self.unit.clone()),
                found: quantity.unit().cloned().map(Box::new),
            });
        }
        let quotient = quantity.number.checked_div(&self.amount)?;
        if quotient.as_rational().is_integer() {
            Ok(())
        } else {
            Err(UnitError::OffQuantum {
                quantity: Box::new(quantity.number.clone()),
                quantum: Box::new(self.amount.clone()),
            })
        }
    }

    pub fn unit(&self) -> &InstrumentUnit {
        &self.unit
    }

    pub fn amount(&self) -> &ExactNumber {
        &self.amount
    }
}

/// A proof-carrying explicit rounding operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundingCertificate {
    input: ExactNumber,
    output: ExactNumber,
    scale: u32,
    mode: RoundingMode,
}

impl RoundingCertificate {
    pub fn apply(input: ExactNumber, scale: u32, mode: RoundingMode) -> Result<Self, UnitError> {
        let output = input.round(scale, mode)?;
        Ok(Self {
            input,
            output,
            scale,
            mode,
        })
    }

    pub fn verify(&self) -> Result<(), UnitError> {
        let expected = self.input.round(self.scale, self.mode)?;
        if expected == self.output {
            Ok(())
        } else {
            Err(UnitError::InvalidRoundingCertificate)
        }
    }

    pub fn input(&self) -> &ExactNumber {
        &self.input
    }

    pub fn output(&self) -> &ExactNumber {
        &self.output
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn mode(&self) -> RoundingMode {
        self.mode
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum QuoteKind {
    Mid,
    Bid,
    Ask,
    Close,
    NetAssetValue,
    Model,
    Tax,
    Other(String),
}

impl QuoteKind {
    /// A bid or ask is one side of a spread.  Its inverse is not implied by
    /// the observation; only symmetric exchange-rate kinds may be traversed
    /// backwards without another quote.
    fn permits_reverse(&self) -> bool {
        matches!(
            self,
            Self::Mid | Self::Close | Self::NetAssetValue | Self::Model | Self::Tax
        )
    }
}

/// A time-indexed exchange observation.  Effective and observed times are
/// separate dimensions; venue, source, kind, and validity are never inferred.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Quote {
    pub id: QuoteId,
    pub ratio: Ratio,
    pub kind: QuoteKind,
    pub effective: Instant,
    pub observed: Instant,
    pub venue: VenueId,
    pub source: SourceId,
    pub validity: InstantInterval,
}

impl Quote {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<QuoteId>,
        ratio: Ratio,
        kind: QuoteKind,
        effective: Instant,
        observed: Instant,
        venue: impl Into<VenueId>,
        source: impl Into<SourceId>,
        validity: InstantInterval,
    ) -> Self {
        Self {
            id: id.into(),
            ratio,
            kind,
            effective,
            observed,
            venue: venue.into(),
            source: source.into(),
            validity,
        }
    }

    /// Construct a quote after checking that its exchange ratio is strictly
    /// positive. Quote metadata is retained exactly; only the economic
    /// invariant is checked here.
    #[allow(clippy::too_many_arguments)]
    pub fn checked_new(
        id: impl Into<QuoteId>,
        ratio: Ratio,
        kind: QuoteKind,
        effective: Instant,
        observed: Instant,
        venue: impl Into<VenueId>,
        source: impl Into<SourceId>,
        validity: InstantInterval,
    ) -> Result<Self, UnitError> {
        ratio.validate()?;
        Ok(Self::new(
            id, ratio, kind, effective, observed, venue, source, validity,
        ))
    }

    /// Alias for [`Quote::checked_new`] following the `new_checked` naming
    /// used by other proof-carrying constructors in the crate.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        id: impl Into<QuoteId>,
        ratio: Ratio,
        kind: QuoteKind,
        effective: Instant,
        observed: Instant,
        venue: impl Into<VenueId>,
        source: impl Into<SourceId>,
        validity: InstantInterval,
    ) -> Result<Self, UnitError> {
        Self::checked_new(
            id, ratio, kind, effective, observed, venue, source, validity,
        )
    }

    /// Fallible-constructor spelling convenient at parsing boundaries.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: impl Into<QuoteId>,
        ratio: Ratio,
        kind: QuoteKind,
        effective: Instant,
        observed: Instant,
        venue: impl Into<VenueId>,
        source: impl Into<SourceId>,
        validity: InstantInterval,
    ) -> Result<Self, UnitError> {
        Self::checked_new(
            id, ratio, kind, effective, observed, venue, source, validity,
        )
    }

    pub fn applies_at(&self, at: Instant) -> bool {
        self.effective <= at && self.validity.contains(&at)
    }

    pub fn age_at(&self, at: Instant) -> Option<i128> {
        at.duration_since(self.observed)
    }

    fn temporal_state(&self, at: Instant) -> QuoteTemporalState {
        let validity_starts_after = match self.validity.start() {
            Bound::Closed(start) => *start > at,
            Bound::Open(start) => *start >= at,
            Bound::Unbounded => false,
        };
        if self.effective > at || self.observed > at || validity_starts_after {
            QuoteTemporalState::Future
        } else if self.applies_at(at) {
            QuoteTemporalState::Current
        } else {
            QuoteTemporalState::Historical
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuoteTemporalState {
    Current,
    Historical,
    Future,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValuationStatus {
    Unique,
    Ambiguous,
    Stale,
    Incomplete,
    NotYetEffective,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Valuation {
    pub status: ValuationStatus,
    pub quantity: Option<Quantity>,
    pub paths: Vec<ConversionPath>,
}

impl Valuation {
    pub fn is_unique(&self) -> bool {
        self.status == ValuationStatus::Unique
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValuationPolicy {
    pub max_age_nanos: Option<i128>,
    pub max_hops: usize,
    pub max_paths: usize,
}

impl Default for ValuationPolicy {
    fn default() -> Self {
        Self {
            max_age_nanos: None,
            max_hops: 4,
            max_paths: 32,
        }
    }
}

/// Value a quantity through direct or triangulated quotes.  Equivalent exact
/// paths collapse to a unique result; conflicting paths remain ambiguous.
pub fn value(
    quantity: &Quantity,
    target: &InstrumentUnit,
    at: Instant,
    quotes: &[Quote],
    policy: ValuationPolicy,
) -> Result<Valuation, UnitError> {
    let from = InstrumentUnit::from_model_unit(quantity.unit().ok_or(UnitError::UnitRequired)?)?;
    if quantity.number.is_negative() {
        return Ok(Valuation {
            status: ValuationStatus::Unavailable,
            quantity: None,
            paths: Vec::new(),
        });
    }
    if &from == target {
        let path = ConversionPath::identity(from);
        return Ok(Valuation {
            status: ValuationStatus::Unique,
            quantity: Some(quantity.clone()),
            paths: vec![path],
        });
    }
    let mut fresh = Vec::new();
    let mut all = Vec::new();
    let mut future = Vec::new();
    for quote in quotes {
        if quote.ratio.value <= ExactNumber::integer(0) {
            continue;
        }
        let edge = QuoteEdge::direct(quote);
        match quote.temporal_state(at) {
            QuoteTemporalState::Current => {
                all.push(edge.clone());
                let fresh_enough = quote.age_at(at).is_some_and(|age| {
                    age >= 0 && policy.max_age_nanos.is_none_or(|max| age <= max)
                });
                if fresh_enough {
                    fresh.push(edge);
                }
            }
            QuoteTemporalState::Historical => all.push(edge),
            QuoteTemporalState::Future => future.push(edge),
        }
    }
    let fresh_paths = find_paths(&from, target, &fresh, policy);
    if fresh_paths.truncated {
        return Ok(Valuation {
            status: ValuationStatus::Incomplete,
            quantity: None,
            paths: fresh_paths.paths,
        });
    }
    if !fresh_paths.paths.is_empty() {
        return valuation_from_paths(quantity, fresh_paths.paths);
    }
    let stale_paths = find_paths(&from, target, &all, policy);
    if stale_paths.truncated {
        return Ok(Valuation {
            status: ValuationStatus::Incomplete,
            quantity: None,
            paths: stale_paths.paths,
        });
    }
    if !stale_paths.paths.is_empty() {
        return Ok(Valuation {
            status: ValuationStatus::Stale,
            quantity: None,
            paths: stale_paths.paths,
        });
    }
    let future_paths = find_paths(&from, target, &future, policy);
    if future_paths.truncated {
        return Ok(Valuation {
            status: ValuationStatus::Incomplete,
            quantity: None,
            paths: future_paths.paths,
        });
    }
    if !future_paths.paths.is_empty() {
        return Ok(Valuation {
            status: ValuationStatus::NotYetEffective,
            quantity: None,
            paths: future_paths.paths,
        });
    }
    Ok(Valuation {
        status: ValuationStatus::Unavailable,
        quantity: None,
        paths: Vec::new(),
    })
}

fn valuation_from_paths(
    quantity: &Quantity,
    paths: Vec<ConversionPath>,
) -> Result<Valuation, UnitError> {
    let equivalent = paths.iter().all(|path| path.rate() == paths[0].rate());
    if equivalent {
        Ok(Valuation {
            status: ValuationStatus::Unique,
            quantity: Some(paths[0].convert(quantity)?),
            paths,
        })
    } else {
        Ok(Valuation {
            status: ValuationStatus::Ambiguous,
            quantity: None,
            paths,
        })
    }
}

#[derive(Clone)]
struct QuoteEdge {
    from: InstrumentUnit,
    to: InstrumentUnit,
    ratio: Ratio,
    quote: QuoteId,
    reversed: bool,
    reverse_allowed: bool,
}

impl QuoteEdge {
    fn direct(quote: &Quote) -> Self {
        Self {
            from: quote.ratio.from.clone(),
            to: quote.ratio.to.clone(),
            ratio: quote.ratio.clone(),
            quote: quote.id.clone(),
            reversed: false,
            reverse_allowed: quote.kind.permits_reverse(),
        }
    }

    fn outgoing(&self) -> Vec<Self> {
        let mut edges = vec![self.clone()];
        if self.reverse_allowed {
            edges.push(Self {
                from: self.to.clone(),
                to: self.from.clone(),
                ratio: self.ratio.inverse().expect("quote ratio is nonzero"),
                quote: self.quote.clone(),
                reversed: true,
                reverse_allowed: self.reverse_allowed,
            });
        }
        edges
    }
}

fn find_paths(
    from: &InstrumentUnit,
    target: &InstrumentUnit,
    edges: &[QuoteEdge],
    policy: ValuationPolicy,
) -> PathSearchResult {
    let mut search = PathSearch {
        target,
        edges,
        max_paths: policy.max_paths,
        visited: BTreeSet::from([from.clone()]),
        legs: Vec::new(),
        paths: Vec::new(),
        truncated: false,
    };
    search.visit(from, policy.max_hops);
    PathSearchResult {
        paths: search.paths,
        truncated: search.truncated,
    }
}

struct PathSearch<'a> {
    target: &'a InstrumentUnit,
    edges: &'a [QuoteEdge],
    max_paths: usize,
    visited: BTreeSet<InstrumentUnit>,
    legs: Vec<ConversionLeg>,
    paths: Vec<ConversionPath>,
    truncated: bool,
}

struct PathSearchResult {
    paths: Vec<ConversionPath>,
    truncated: bool,
}

impl PathSearch<'_> {
    fn visit(&mut self, current: &InstrumentUnit, remaining: usize) {
        if current == self.target {
            if self.max_paths == 0 {
                self.truncated = true;
                return;
            }
            if let Ok(path) = ConversionPath::new(self.legs.clone()) {
                self.paths.push(path);
            }
            return;
        }
        if remaining == 0 {
            if self.edges.iter().any(|edge| {
                edge.outgoing().into_iter().any(|outgoing| {
                    outgoing.from == *current && !self.visited.contains(&outgoing.to)
                })
            }) {
                self.truncated = true;
            }
            return;
        }
        for index in 0..self.edges.len() {
            for outgoing in self.edges[index].outgoing() {
                if outgoing.from != *current || self.visited.contains(&outgoing.to) {
                    continue;
                }
                if self.paths.len() >= self.max_paths {
                    self.truncated = true;
                    return;
                }
                self.visited.insert(outgoing.to.clone());
                self.legs.push(ConversionLeg {
                    ratio: outgoing.ratio.clone(),
                    quote: Some(outgoing.quote.clone()),
                    reversed: outgoing.reversed,
                });
                self.visit(&outgoing.to, remaining - 1);
                self.legs.pop();
                self.visited.remove(&outgoing.to);
                if self.truncated {
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnitError {
    UnitRequired,
    UnitMismatch {
        expected: Box<InstrumentUnit>,
        found: Option<Box<Unit>>,
    },
    DivisionByZero,
    InvalidQuantum,
    NonPositiveRatio,
    InstrumentMismatch {
        expected: Box<InstrumentId>,
        found: Box<InstrumentId>,
    },
    OffQuantum {
        quantity: Box<ExactNumber>,
        quantum: Box<ExactNumber>,
    },
    EmptyConversionPath,
    BrokenConversionPath,
    InvalidRoundingCertificate,
    InvalidCanonicalUnit(Unit),
    Numeric(Box<ExactError>),
    Time(Box<TimeError>),
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitRequired => f.write_str("a nonzero quantity requires a nominal unit"),
            Self::UnitMismatch { expected, found } => {
                write!(f, "unit mismatch: expected {expected}, found {found:?}")
            }
            Self::DivisionByZero => f.write_str("division by zero"),
            Self::InvalidQuantum => f.write_str("quantum must be positive"),
            Self::NonPositiveRatio => f.write_str("ratio must be strictly positive"),
            Self::InstrumentMismatch { expected, found } => {
                write!(f, "unit belongs to {found}, expected instrument {expected}")
            }
            Self::OffQuantum { quantity, quantum } => {
                write!(
                    f,
                    "{quantity} is not an exact multiple of quantum {quantum}"
                )
            }
            Self::EmptyConversionPath => f.write_str("conversion path has no legs"),
            Self::BrokenConversionPath => f.write_str("conversion path has discontinuous units"),
            Self::InvalidRoundingCertificate => f.write_str("invalid rounding certificate"),
            Self::InvalidCanonicalUnit(unit) => {
                write!(f, "unit {unit} is not an instrument nominal identity")
            }
            Self::Numeric(error) => error.fmt(f),
            Self::Time(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for UnitError {}

impl From<ExactError> for UnitError {
    fn from(error: ExactError) -> Self {
        Self::Numeric(Box::new(error))
    }
}

impl From<TimeError> for UnitError {
    fn from(error: TimeError) -> Self {
        Self::Time(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::RoundingMode;
    use crate::time::Bound;

    fn unit(name: &str) -> InstrumentUnit {
        InstrumentUnit::new(name, format!("instrument/{name}"))
    }

    fn instant(seconds: i64) -> Instant {
        Instant::from_unix_seconds(seconds)
    }

    fn quote(
        id: &str,
        from: InstrumentUnit,
        to: InstrumentUnit,
        rate: &str,
        observed: i64,
    ) -> Quote {
        Quote::new(
            id,
            Ratio::new(from, to, rate.parse().unwrap()),
            QuoteKind::Mid,
            instant(0),
            instant(observed),
            "venue/main",
            format!("source/{id}"),
            InstantInterval::new(Bound::Closed(instant(0)), Bound::Closed(instant(10_000)))
                .unwrap(),
        )
    }

    #[test]
    fn zero_requires_no_unit_but_nonzero_does() {
        assert!(Quantity::new(ExactNumber::integer(0), None).is_ok());
        assert!(Quantity::new(ExactNumber::integer(1), None).is_err());
        let usd = unit("USD");
        let zero = Quantity::zero();
        let one = Quantity::typed(ExactNumber::integer(1), usd.clone());
        assert_eq!(
            zero.checked_add(&one).unwrap().unit,
            Some(usd.as_model_unit())
        );
    }

    #[test]
    fn exact_quantities_require_nominal_compatibility() {
        let usd = unit("USD");
        let eur = unit("EUR");
        let one = Quantity::typed(ExactNumber::integer(1), usd);
        let two = Quantity::typed(ExactNumber::integer(2), eur);
        assert!(matches!(
            one.checked_add(&two),
            Err(crate::model::ModelError::UnitMismatch { .. })
        ));
    }

    #[test]
    fn ratio_to_orients_from_base_to_counter() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let ratio = Ratio::new(abc, usd, "20".parse::<ExactNumber>().unwrap());
        assert_eq!(ratio.from.instrument_id().as_str(), "instrument/ABC");
        assert_eq!(ratio.to.instrument_id().as_str(), "instrument/USD");
        assert_eq!(ratio.value, "20".parse::<ExactNumber>().unwrap());
    }

    #[test]
    fn checked_ratio_construction_rejects_zero_and_negative_values() {
        let abc = unit("ABC");
        let usd = unit("USD");
        for value in ["0", "-1", "-1/3"] {
            let error = Ratio::checked_new(
                abc.clone(),
                usd.clone(),
                value.parse::<ExactNumber>().unwrap(),
            )
            .unwrap_err();
            assert_eq!(error, UnitError::NonPositiveRatio);
        }

        let positive = Ratio::checked_new(
            abc.clone(),
            usd.clone(),
            "20.00".parse::<ExactNumber>().unwrap(),
        )
        .unwrap();
        assert_eq!(positive.value, "20.00".parse::<ExactNumber>().unwrap());
        assert!(ConversionPath::new(vec![ConversionLeg::direct(positive, None)]).is_ok());
    }

    #[test]
    fn checked_quote_construction_rejects_non_positive_ratio() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let ratio = Ratio {
            from: abc,
            to: usd,
            value: ExactNumber::integer(0),
        };
        let error = Quote::checked_new(
            "zero",
            ratio,
            QuoteKind::Mid,
            instant(0),
            instant(0),
            "venue/main",
            "source/zero",
            InstantInterval::new(Bound::Closed(instant(0)), Bound::Closed(instant(10))).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, UnitError::NonPositiveRatio);
    }

    #[test]
    fn valuation_drops_zero_and_negative_quotes() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let mut zero = quote("zero", abc.clone(), usd.clone(), "1", 1);
        zero.ratio.value = ExactNumber::integer(0);
        let mut negative = quote("negative", abc.clone(), usd.clone(), "1", 1);
        negative.ratio.value = ExactNumber::integer(-2);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[zero, negative],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Unavailable);
        assert!(result.quantity.is_none());
    }

    #[test]
    fn valuation_does_not_return_a_negative_quantity() {
        let abc = unit("ABC");
        let result = value(
            &Quantity::typed(ExactNumber::integer(-1), abc.clone()),
            &abc,
            instant(2),
            &[],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Unavailable);
        assert!(result.quantity.is_none());
    }

    #[test]
    fn conversion_path_preserves_exact_triangulation_proof() {
        let abc = unit("ABC");
        let eur = unit("EUR");
        let usd = unit("USD");
        let path = ConversionPath::new(vec![
            ConversionLeg::direct(
                Ratio::new(abc.clone(), eur.clone(), "2".parse().unwrap()),
                Some("abc-eur".into()),
            ),
            ConversionLeg::direct(
                Ratio::new(eur, usd.clone(), "1.5".parse().unwrap()),
                Some("eur-usd".into()),
            ),
        ])
        .unwrap();
        let converted = path
            .convert(&Quantity::typed("10".parse().unwrap(), abc))
            .unwrap();
        assert_eq!(converted.amount(), &"30".parse::<ExactNumber>().unwrap());
        assert_eq!(converted.unit(), Some(&usd.as_model_unit()));
        assert_eq!(path.legs().len(), 2);
    }

    #[test]
    fn equivalent_rates_are_unique_by_exact_equality() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let first = quote("q1", abc.clone(), usd.clone(), "2", 1);
        let second = quote("q2", abc.clone(), usd.clone(), "4/2", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(3), abc),
            &usd,
            instant(2),
            &[first, second],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Unique);
        assert_eq!(result.quantity.unwrap().number, ExactNumber::integer(6));
        assert_eq!(result.paths.len(), 2);
    }

    #[test]
    fn one_sided_bid_quote_does_not_create_a_reverse_edge() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let mut bid = quote("bid", usd.clone(), abc.clone(), "2", 1);
        bid.kind = QuoteKind::Bid;
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[bid],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Unavailable);
    }

    #[test]
    fn path_cap_is_reported_as_incomplete_not_unique() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let first = quote("q1", abc.clone(), usd.clone(), "2", 1);
        let second = quote("q2", abc.clone(), usd.clone(), "3", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[first, second],
            ValuationPolicy {
                max_paths: 1,
                ..ValuationPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Incomplete);
        assert!(result.quantity.is_none());
    }

    #[test]
    fn an_exactly_filled_path_cap_remains_unique_when_no_branch_is_hidden() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let only = quote("only", abc.clone(), usd.clone(), "2", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[only],
            ValuationPolicy {
                max_paths: 1,
                ..ValuationPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Unique);
        assert_eq!(result.quantity.unwrap().amount(), &ExactNumber::integer(2));
    }

    #[test]
    fn hop_cap_is_reported_as_incomplete_not_unavailable() {
        let abc = unit("ABC");
        let eur = unit("EUR");
        let usd = unit("USD");
        let first = quote("abc-eur", abc.clone(), eur, "2", 1);
        let second = quote("eur-usd", unit("EUR"), usd.clone(), "3", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[first, second],
            ValuationPolicy {
                max_hops: 1,
                ..ValuationPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Incomplete);
    }

    #[test]
    fn conflicting_quotes_remain_ambiguous() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let first = quote("q1", abc.clone(), usd.clone(), "2", 1);
        let second = quote("q2", abc.clone(), usd.clone(), "3", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[first, second],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Ambiguous);
        assert!(result.quantity.is_none());
    }

    #[test]
    fn stale_quotes_are_not_used_as_current_value() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let stale = quote("q1", abc.clone(), usd.clone(), "2", 1);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(100),
            &[stale],
            ValuationPolicy {
                max_age_nanos: Some(10),
                ..ValuationPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::Stale);
    }

    #[test]
    fn pre_effective_quotes_are_not_misreported_as_stale() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let mut future = quote("future", abc.clone(), usd.clone(), "2", 1);
        future.effective = instant(10);
        let result = value(
            &Quantity::typed(ExactNumber::integer(1), abc),
            &usd,
            instant(2),
            &[future],
            ValuationPolicy::default(),
        )
        .unwrap();
        assert_eq!(result.status, ValuationStatus::NotYetEffective);
    }

    #[test]
    fn quanta_are_constraints_and_rounding_is_explicit() {
        let usd = unit("USD");
        let quantum = Quantum::new(usd.clone(), "0.01".parse().unwrap()).unwrap();
        assert!(
            quantum
                .accepts(&Quantity::typed("1.23".parse().unwrap(), usd.clone()))
                .is_ok()
        );
        assert!(
            quantum
                .accepts(&Quantity::typed("1.235".parse().unwrap(), usd))
                .is_err()
        );
        let certificate =
            RoundingCertificate::apply("1.235".parse().unwrap(), 2, RoundingMode::HalfEven)
                .unwrap();
        assert_eq!(
            certificate.output(),
            &"1.24".parse::<ExactNumber>().unwrap()
        );
        certificate.verify().unwrap();
    }

    #[test]
    fn instrument_definition_rejects_a_unit_from_another_instrument() {
        let usd = unit("USD");
        let error = InstrumentDefinition::new("instrument/EUR", usd, ExactNumber::decimal(1, 2))
            .unwrap_err();
        assert!(matches!(error, UnitError::InstrumentMismatch { .. }));
    }

    #[test]
    fn validated_paths_are_read_through_accessors() {
        let abc = unit("ABC");
        let usd = unit("USD");
        let path = ConversionPath::new(vec![ConversionLeg::direct(
            Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(2)),
            None,
        )])
        .unwrap();
        assert_eq!(path.from(), &abc);
        assert_eq!(path.to(), &usd);
        assert_eq!(path.rate(), &ExactNumber::integer(2));
        assert_eq!(path.legs().len(), 1);
    }
}
