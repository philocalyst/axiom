//! Isolated planning worlds and forecast reconciliation.
//!
//! A scenario is a view over an accepted actual root.  It never appends to,
//! or edits, that root: assumptions, projected events, and constraints live
//! in the scenario namespace.  A realized event may be related to a projected
//! event, but the relation is evidence about variance, not a promotion of the
//! projection into the actual world.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::exact::Exact;
use crate::model::{AccountId, ContentHash, Date, OccurrenceId, Quantity};
use crate::time::{Recurrence, TimeError};

/// An immutable content-addressed accepted-world root.
///
/// The root is intentionally a reference-sized object.  A scenario stores an
/// `Arc` to it rather than copying actual facts into its own fact collection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AcceptedRoot {
    hash: ContentHash,
}

impl AcceptedRoot {
    pub fn new(hash: ContentHash) -> Self {
        Self { hash }
    }

    pub fn hash(&self) -> ContentHash {
        self.hash
    }
}

/// Alias used by callers that describe the root as an accepted world.
pub type AcceptedActualRoot = AcceptedRoot;

/// A resolver-backed proof that one event belongs to an accepted actual root.
/// Scenario reconciliation accepts this token, rather than trusting a caller
/// supplied event with the same display ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedEventToken {
    pub root: ContentHash,
    pub event: RealizedEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedEventResolver {
    root: ContentHash,
    events: BTreeMap<OccurrenceId, RealizedEvent>,
}

impl AcceptedEventResolver {
    pub fn new(root: ContentHash) -> Self {
        Self {
            root,
            events: BTreeMap::new(),
        }
    }

    pub fn root(&self) -> ContentHash {
        self.root
    }

    pub fn add_event(&mut self, event: RealizedEvent) -> Result<(), ScenarioError> {
        if self.events.insert(event.id.clone(), event).is_some() {
            return Err(ScenarioError::DuplicateAcceptedEvent);
        }
        Ok(())
    }

    pub fn resolve(&self, event_id: &OccurrenceId) -> Result<AcceptedEventToken, ScenarioError> {
        let event = self
            .events
            .get(event_id)
            .cloned()
            .ok_or_else(|| ScenarioError::ActualEventNotAccepted(event_id.clone()))?;
        Ok(AcceptedEventToken {
            root: self.root,
            event,
        })
    }

    fn verify(&self, token: &AcceptedEventToken) -> Result<RealizedEvent, ScenarioError> {
        if token.root != self.root {
            return Err(ScenarioError::ActualRootMismatch);
        }
        let accepted = self.resolve(&token.event.id)?;
        if accepted.event != token.event {
            return Err(ScenarioError::ActualEventMismatch(token.event.id.clone()));
        }
        Ok(accepted.event)
    }
}

/// A named, actual-world-independent planning scenario.
#[derive(Clone, Debug)]
pub struct Scenario {
    name: String,
    actual: Arc<AcceptedRoot>,
    actual_events: Arc<AcceptedEventResolver>,
    assumptions: BTreeMap<String, Assumption>,
    expected_events: BTreeMap<OccurrenceId, ExpectedEvent>,
    constraints: BTreeMap<String, Constraint>,
    links: BTreeMap<ForecastOccurrenceId, Vec<ForecastLink>>,
}

impl PartialEq for Scenario {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.actual == other.actual
            && self.actual_events == other.actual_events
            && self.assumptions == other.assumptions
            && self.expected_events == other.expected_events
            && self.constraints == other.constraints
            && self.links == other.links
    }
}

impl Eq for Scenario {}

impl Scenario {
    /// Fork a named scenario from an accepted root.
    pub fn new(name: impl Into<String>, root: ContentHash) -> Result<Self, ScenarioError> {
        let name = checked_name(name.into(), "scenario name")?;
        Ok(Self {
            name,
            actual: Arc::new(AcceptedRoot::new(root)),
            actual_events: Arc::new(AcceptedEventResolver::new(root)),
            assumptions: BTreeMap::new(),
            expected_events: BTreeMap::new(),
            constraints: BTreeMap::new(),
            links: BTreeMap::new(),
        })
    }

    /// Fork while retaining the caller's accepted-root allocation by
    /// reference.  This is useful when several scenarios share one root.
    pub fn from_accepted(
        name: impl Into<String>,
        root: Arc<AcceptedRoot>,
    ) -> Result<Self, ScenarioError> {
        let name = checked_name(name.into(), "scenario name")?;
        let root_hash = root.hash();
        Ok(Self {
            name,
            actual: root,
            actual_events: Arc::new(AcceptedEventResolver::new(root_hash)),
            assumptions: BTreeMap::new(),
            expected_events: BTreeMap::new(),
            constraints: BTreeMap::new(),
            links: BTreeMap::new(),
        })
    }

    /// Fork from an accepted-world resolver that can prove actual event
    /// membership.  The resolver's root is the scenario's inherited root.
    pub fn from_resolver(
        name: impl Into<String>,
        resolver: Arc<AcceptedEventResolver>,
    ) -> Result<Self, ScenarioError> {
        let name = checked_name(name.into(), "scenario name")?;
        let actual = Arc::new(AcceptedRoot::new(resolver.root()));
        Ok(Self {
            name,
            actual,
            actual_events: resolver,
            assumptions: BTreeMap::new(),
            expected_events: BTreeMap::new(),
            constraints: BTreeMap::new(),
            links: BTreeMap::new(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn actual_root(&self) -> ContentHash {
        self.actual.hash()
    }

    pub fn accepted_root(&self) -> &AcceptedRoot {
        self.actual.as_ref()
    }

    pub fn accepted_root_ref(&self) -> Arc<AcceptedRoot> {
        Arc::clone(&self.actual)
    }

    pub fn actual_event_resolver(&self) -> Arc<AcceptedEventResolver> {
        Arc::clone(&self.actual_events)
    }

    /// Register immutable accepted evidence in the resolver reference.  This
    /// changes only the resolver index, never the scenario's forecast state.
    pub fn register_actual_event(&mut self, event: RealizedEvent) -> Result<(), ScenarioError> {
        Arc::make_mut(&mut self.actual_events).add_event(event)
    }

    pub fn assumptions(&self) -> impl Iterator<Item = &Assumption> {
        self.assumptions.values()
    }

    pub fn expected_events(&self) -> impl Iterator<Item = &ExpectedEvent> {
        self.expected_events.values()
    }

    pub fn constraints(&self) -> impl Iterator<Item = &Constraint> {
        self.constraints.values()
    }

    pub fn links(&self) -> impl Iterator<Item = &ForecastLink> {
        self.links.values().flat_map(|links| links.iter())
    }

    pub fn assumption(&self, id: &str) -> Option<&Assumption> {
        self.assumptions.get(id)
    }

    pub fn expected_event(&self, id: &OccurrenceId) -> Option<&ExpectedEvent> {
        self.expected_events.get(id)
    }

    pub fn constraint(&self, id: &str) -> Option<&Constraint> {
        self.constraints.get(id)
    }

    pub fn add_assumption(&mut self, assumption: Assumption) -> Result<(), ScenarioError> {
        if self
            .assumptions
            .insert(assumption.id.clone(), assumption)
            .is_some()
        {
            return Err(ScenarioError::DuplicateId("assumption"));
        }
        Ok(())
    }

    pub fn assume(&mut self, assumption: Assumption) -> Result<(), ScenarioError> {
        self.add_assumption(assumption)
    }

    pub fn add_expected_event(&mut self, event: ExpectedEvent) -> Result<(), ScenarioError> {
        if self
            .expected_events
            .insert(event.id.clone(), event)
            .is_some()
        {
            return Err(ScenarioError::DuplicateId("expected event"));
        }
        Ok(())
    }

    pub fn expect(&mut self, event: ExpectedEvent) -> Result<(), ScenarioError> {
        self.add_expected_event(event)
    }

    pub fn add_constraint(&mut self, constraint: Constraint) -> Result<(), ScenarioError> {
        if self
            .constraints
            .insert(constraint.id.clone(), constraint)
            .is_some()
        {
            return Err(ScenarioError::DuplicateId("constraint"));
        }
        Ok(())
    }

    pub fn constrain(&mut self, constraint: Constraint) -> Result<(), ScenarioError> {
        self.add_constraint(constraint)
    }

    /// Verify an exact plan against every scenario constraint.  A failed
    /// candidate is returned with a deletion-minimal violation core; no
    /// approximate objective is involved in this decision.
    pub fn verify_plan(&self, plan: &PlanMetrics) -> Result<VerifiedPlan, PlanVerificationError> {
        let mut failed = Vec::new();
        for constraint in self.constraints.values() {
            if matches!(constraint.expression, ConstraintExpression::Text(_)) {
                return Err(PlanVerificationError::UnsupportedConstraint {
                    id: constraint.id.clone(),
                });
            }
            if !constraint.expression.satisfied_by(plan) {
                failed.push(constraint.id.clone());
            }
        }
        if failed.is_empty() {
            return Ok(VerifiedPlan {
                metrics: plan.clone(),
            });
        }
        let core = ConstraintCore::new(failed.clone()).trimmed_by(|ids| {
            // This predicate describes this concrete candidate: a subset
            // remains infeasible exactly when at least one included
            // constraint is violated.  Trimming therefore turns a noisy
            // solver explanation into an exact minimal witness.
            ids.iter().any(|id| {
                self.constraints
                    .get(id)
                    .is_some_and(|constraint| !constraint.expression.satisfied_by(plan))
            })
        });
        Err(PlanVerificationError::ConstraintViolation {
            core: core.unwrap_or_else(|| ConstraintCore::new(failed.clone())),
            violations: failed,
        })
    }

    /// Exact verification boundary for a plan proposed by an approximate
    /// optimizer.  The objective is accepted only as metadata; constraint
    /// satisfaction is recomputed from exact plan values.
    pub fn verify_approximate_plan(
        &self,
        candidate: &ApproximatePlanCandidate,
    ) -> Result<VerifiedPlan, PlanVerificationError> {
        if !candidate.objective.is_finite() {
            return Err(PlanVerificationError::NonFiniteApproximation);
        }
        self.verify_plan(&candidate.metrics)
    }

    /// Materialize all projected dates in an explicit bounded horizon.
    ///
    /// A recurrence with no intrinsic `count`/`until` remains safe because the
    /// query supplies a finite horizon.  `ExpectedEvent::occurrences` does not
    /// expose an unbounded iterator.
    pub fn materialize(&self, horizon: Horizon) -> Result<Vec<ForecastOccurrence>, ScenarioError> {
        let mut occurrences = Vec::new();
        for event in self.expected_events.values() {
            occurrences.extend(event.occurrences(horizon)?);
        }
        occurrences.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.forecast_id.cmp(&right.forecast_id))
                .then_with(|| left.index.cmp(&right.index))
        });
        Ok(occurrences)
    }

    /// Relate one actual event to a forecast occurrence.  This only records a
    /// link in the scenario and leaves both the actual root and forecast event
    /// unchanged.
    pub fn link_realized(
        &mut self,
        forecast_id: impl Into<OccurrenceId>,
        realized: RealizedEvent,
    ) -> Result<ForecastLink, ScenarioError> {
        let forecast_id = forecast_id.into();
        let forecast = self
            .expected_events
            .get(&forecast_id)
            .ok_or_else(|| ScenarioError::UnknownExpectedEvent(forecast_id.to_string()))?;
        let date = forecast
            .date
            .ok_or_else(|| ScenarioError::RequiresOccurrenceIdentity(forecast_id.to_string()))?;
        self.link_realized_occurrence(ForecastOccurrenceId::new(forecast_id, 0, date), realized)
    }

    /// Link a realized event to one specific projected occurrence.  The key
    /// includes the forecast, ordinal, and date so recurring forecasts can
    /// have several independent realized links.
    pub fn link_realized_occurrence(
        &mut self,
        occurrence: ForecastOccurrenceId,
        realized: RealizedEvent,
    ) -> Result<ForecastLink, ScenarioError> {
        let token = self.actual_events.resolve(&realized.id)?;
        if token.event != realized {
            return Err(ScenarioError::ActualEventMismatch(realized.id));
        }
        self.link_realized_occurrence_token(occurrence, token)
    }

    pub fn link_realized_occurrence_token(
        &mut self,
        occurrence: ForecastOccurrenceId,
        token: AcceptedEventToken,
    ) -> Result<ForecastLink, ScenarioError> {
        let realized = self.actual_events.verify(&token)?;
        if self
            .links
            .values()
            .flat_map(|links| links.iter())
            .any(|link| link.realized.id == realized.id)
        {
            return Err(ScenarioError::DuplicateRealizedEvent(realized.id));
        }
        let forecast = self
            .expected_events
            .get(&occurrence.forecast_id)
            .ok_or_else(|| {
                ScenarioError::UnknownExpectedEvent(occurrence.forecast_id.to_string())
            })?;
        let occurrence_horizon = forecast
            .recurrence
            .as_ref()
            .map(|recurrence| recurrence.horizon)
            .unwrap_or(Horizon {
                start: occurrence.date,
                end: occurrence.date,
            });
        if !forecast
            .occurrences(occurrence_horizon)?
            .iter()
            .any(|candidate| {
                candidate.forecast_id == occurrence.forecast_id
                    && candidate.index == occurrence.index
                    && candidate.date == occurrence.date
            })
        {
            return Err(ScenarioError::UnknownForecastOccurrence(occurrence));
        }
        self.validate_allocation(&occurrence, forecast, &realized)?;
        let link = ForecastLink::new(self.actual_root(), occurrence.clone(), forecast, realized)?;
        self.links.entry(occurrence).or_default().push(link.clone());
        Ok(link)
    }

    fn validate_allocation(
        &self,
        occurrence: &ForecastOccurrenceId,
        forecast: &ExpectedEvent,
        realized: &RealizedEvent,
    ) -> Result<(), ScenarioError> {
        if realized
            .quantity
            .as_ref()
            .is_some_and(|quantity| quantity.number.is_negative())
        {
            return Err(ScenarioError::NegativeAllocation(realized.id.clone()));
        }
        let Some(existing) = self.links.get(occurrence) else {
            return Ok(());
        };
        let expected =
            forecast
                .quantity
                .as_ref()
                .ok_or(ScenarioError::ExplicitAllocationRequired(
                    occurrence.clone(),
                ))?;
        let new_quantity =
            realized
                .quantity
                .as_ref()
                .ok_or(ScenarioError::ExplicitAllocationRequired(
                    occurrence.clone(),
                ))?;
        let mut total = Quantity::zero();
        for link in existing {
            let quantity = link.realized.quantity.as_ref().ok_or(
                ScenarioError::ExplicitAllocationRequired(occurrence.clone()),
            )?;
            total = total.checked_add(quantity).map_err(|error| {
                ScenarioError::IncompatibleAllocationUnit {
                    detail: error.to_string(),
                }
            })?;
        }
        total = total.checked_add(new_quantity).map_err(|error| {
            ScenarioError::IncompatibleAllocationUnit {
                detail: error.to_string(),
            }
        })?;
        let remainder = expected.checked_sub(&total).map_err(|error| {
            ScenarioError::IncompatibleAllocationUnit {
                detail: error.to_string(),
            }
        })?;
        if remainder.number.is_negative() {
            return Err(ScenarioError::AllocationExceedsForecast(occurrence.clone()));
        }
        Ok(())
    }

    pub fn realized_link(&self, forecast_id: &OccurrenceId) -> Option<&ForecastLink> {
        self.links
            .values()
            .flat_map(|links| links.iter())
            .find(|link| &link.forecast_id == forecast_id)
    }

    pub fn realized_links(
        &self,
        forecast_id: &OccurrenceId,
    ) -> impl Iterator<Item = &ForecastLink> {
        self.links
            .values()
            .flat_map(|links| links.iter())
            .filter(move |link| &link.forecast_id == forecast_id)
    }

    /// Compare two scenarios by semantic fields, never by rendered text.
    pub fn diff(&self, other: &Scenario) -> ScenarioDiff {
        ScenarioDiff::between(self, other)
    }
}

/// A typed assumption that exists only in one scenario.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assumption {
    pub id: String,
    pub value: AssumptionValue,
}

impl Assumption {
    pub fn new(
        id: impl Into<String>,
        value: impl Into<AssumptionValue>,
    ) -> Result<Self, ScenarioError> {
        let id = checked_name(id.into(), "assumption id")?;
        Ok(Self {
            id,
            value: value.into(),
        })
    }

    pub fn text(id: impl Into<String>, value: impl Into<String>) -> Result<Self, ScenarioError> {
        Self::new(id, AssumptionValue::Text(value.into()))
    }

    pub fn exact(id: impl Into<String>, value: Exact) -> Result<Self, ScenarioError> {
        Self::new(id, AssumptionValue::Number(value))
    }

    pub fn quantity(id: impl Into<String>, value: Quantity) -> Result<Self, ScenarioError> {
        Self::new(id, AssumptionValue::Quantity(value))
    }

    pub fn boolean(id: impl Into<String>, value: bool) -> Result<Self, ScenarioError> {
        Self::new(id, AssumptionValue::Boolean(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssumptionValue {
    Text(String),
    Number(Exact),
    Quantity(Quantity),
    Boolean(bool),
    Date(Date),
}

impl From<String> for AssumptionValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for AssumptionValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<Exact> for AssumptionValue {
    fn from(value: Exact) -> Self {
        Self::Number(value)
    }
}

impl From<Quantity> for AssumptionValue {
    fn from(value: Quantity) -> Self {
        Self::Quantity(value)
    }
}

impl From<bool> for AssumptionValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<Date> for AssumptionValue {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}

/// An expected event is forecast-only until explicitly linked to realized
/// evidence.  A recurrence is materialized only by an explicit horizon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedEvent {
    pub id: OccurrenceId,
    pub label: String,
    pub date: Option<Date>,
    pub quantity: Option<Quantity>,
    pub from: Option<AccountId>,
    pub to: Option<AccountId>,
    pub recurrence: Option<BoundedRecurrence>,
}

impl ExpectedEvent {
    pub fn new(id: impl Into<OccurrenceId>) -> Self {
        let id = id.into();
        Self {
            label: id.to_string(),
            id,
            date: None,
            quantity: None,
            from: None,
            to: None,
            recurrence: None,
        }
    }

    pub fn dated(id: impl Into<OccurrenceId>, date: Date) -> Self {
        let mut event = Self::new(id);
        event.date = Some(date);
        event
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_quantity(mut self, quantity: Quantity) -> Self {
        self.quantity = Some(quantity);
        self
    }

    pub fn move_from(mut self, account: impl Into<AccountId>) -> Self {
        self.from = Some(account.into());
        self
    }

    pub fn move_to(mut self, account: impl Into<AccountId>) -> Self {
        self.to = Some(account.into());
        self
    }

    pub fn recurring(mut self, recurrence: BoundedRecurrence) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn occurrences(&self, horizon: Horizon) -> Result<Vec<ForecastOccurrence>, ScenarioError> {
        let dates = if let Some(recurrence) = &self.recurrence {
            recurrence
                .materialize(recurrence.horizon)?
                .into_iter()
                .filter(|date| horizon.contains(*date))
                .collect()
        } else if let Some(date) = self.date {
            if horizon.contains(date) {
                vec![date]
            } else {
                Vec::new()
            }
        } else {
            return Err(ScenarioError::MissingEventDate(self.id.to_string()));
        };
        Ok(dates
            .into_iter()
            .enumerate()
            .map(|(index, date)| ForecastOccurrence {
                forecast_id: self.id.clone(),
                index,
                date,
                quantity: self.quantity.clone(),
                from: self.from.clone(),
                to: self.to.clone(),
            })
            .collect())
    }
}

/// A recurrence plus the mandatory horizon that makes it finite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedRecurrence {
    pub recurrence: Recurrence,
    pub horizon: Horizon,
}

impl BoundedRecurrence {
    pub fn new(recurrence: Recurrence, horizon: Horizon) -> Result<Self, ScenarioError> {
        if horizon.start > horizon.end {
            return Err(ScenarioError::InvalidHorizon);
        }
        Ok(Self {
            recurrence,
            horizon,
        })
    }

    pub fn materialize(&self, query_horizon: Horizon) -> Result<Vec<Date>, ScenarioError> {
        let horizon = self
            .horizon
            .intersection(query_horizon)
            .ok_or(ScenarioError::InvalidHorizon)?;
        self.recurrence
            .between(horizon.start, horizon.end)
            .map(|dates| dates.into_iter().collect())
            .map_err(ScenarioError::Time)
    }
}

/// Inclusive civil-date horizon used to bound all forecast generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Horizon {
    pub start: Date,
    pub end: Date,
}

impl Horizon {
    pub fn new(start: Date, end: Date) -> Result<Self, ScenarioError> {
        if start > end {
            return Err(ScenarioError::InvalidHorizon);
        }
        Ok(Self { start, end })
    }

    pub fn contains(self, date: Date) -> bool {
        self.start <= date && date <= self.end
    }

    fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start <= end).then_some(Self { start, end })
    }
}

/// One materialized occurrence of a projected event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastOccurrence {
    pub forecast_id: OccurrenceId,
    pub index: usize,
    pub date: Date,
    pub quantity: Option<Quantity>,
    pub from: Option<AccountId>,
    pub to: Option<AccountId>,
}

/// Stable identity for one projected occurrence.  A forecast ID alone is not
/// sufficient for a recurring plan: its ordinal and realized date are part of
/// the semantic identity used by reconciliation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ForecastOccurrenceId {
    pub forecast_id: OccurrenceId,
    pub index: usize,
    pub date: Date,
}

impl ForecastOccurrenceId {
    pub fn new(forecast_id: impl Into<OccurrenceId>, index: usize, date: Date) -> Self {
        Self {
            forecast_id: forecast_id.into(),
            index,
            date,
        }
    }
}

impl ForecastOccurrence {
    pub fn identity(&self) -> ForecastOccurrenceId {
        ForecastOccurrenceId::new(self.forecast_id.clone(), self.index, self.date)
    }
}

/// Scenario-only constraint.  The expression remains data, so adding a
/// constraint cannot mutate or shadow an accepted fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Constraint {
    pub id: String,
    pub expression: ConstraintExpression,
}

impl Constraint {
    pub fn new(
        id: impl Into<String>,
        expression: impl Into<ConstraintExpression>,
    ) -> Result<Self, ScenarioError> {
        let id = checked_name(id.into(), "constraint id")?;
        Ok(Self {
            id,
            expression: expression.into(),
        })
    }

    pub fn text(
        id: impl Into<String>,
        expression: impl Into<String>,
    ) -> Result<Self, ScenarioError> {
        Self::new(id, ConstraintExpression::Text(expression.into()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstraintExpression {
    Text(String),
    QuantityAtLeast { metric: String, amount: Quantity },
    QuantityAtMost { metric: String, amount: Quantity },
    ExactAtLeast { metric: String, amount: Exact },
    ExactAtMost { metric: String, amount: Exact },
}

impl ConstraintExpression {
    fn satisfied_by(&self, plan: &PlanMetrics) -> bool {
        match self {
            Self::Text(_) => false,
            Self::QuantityAtLeast { metric, amount } => plan
                .quantity(metric)
                .is_some_and(|value| quantity_at_least_exact(value, amount)),
            Self::QuantityAtMost { metric, amount } => plan
                .quantity(metric)
                .is_some_and(|value| quantity_at_least_exact(amount, value)),
            Self::ExactAtLeast { metric, amount } => {
                plan.exact(metric).is_some_and(|value| value >= amount)
            }
            Self::ExactAtMost { metric, amount } => {
                plan.exact(metric).is_some_and(|value| value <= amount)
            }
        }
    }
}

/// Exact values supplied by a planner for constraint verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanValue {
    Quantity(Quantity),
    Exact(Exact),
    Boolean(bool),
    Text(String),
}

/// A concrete plan's exact metric values.  Approximate solver scores never
/// enter this map.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct PlanMetrics {
    values: BTreeMap<String, PlanValue>,
}

impl PlanMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, metric: impl Into<String>, value: PlanValue) {
        self.values.insert(metric.into(), value);
    }

    pub fn with_quantity(mut self, metric: impl Into<String>, value: Quantity) -> Self {
        self.insert(metric, PlanValue::Quantity(value));
        self
    }

    pub fn with_exact(mut self, metric: impl Into<String>, value: Exact) -> Self {
        self.insert(metric, PlanValue::Exact(value));
        self
    }

    pub fn quantity(&self, metric: &str) -> Option<&Quantity> {
        match self.values.get(metric) {
            Some(PlanValue::Quantity(value)) => Some(value),
            _ => None,
        }
    }

    pub fn exact(&self, metric: &str) -> Option<&Exact> {
        match self.values.get(metric) {
            Some(PlanValue::Exact(value)) => Some(value),
            _ => None,
        }
    }

    pub fn value(&self, metric: &str) -> Option<&PlanValue> {
        self.values.get(metric)
    }
}

pub type Plan = PlanMetrics;

/// A candidate returned by an approximate optimizer.  Its objective is
/// deliberately not part of exact acceptance.
#[derive(Clone, Debug, PartialEq)]
pub struct ApproximatePlanCandidate {
    pub metrics: PlanMetrics,
    pub objective: f64,
}

impl ApproximatePlanCandidate {
    pub fn new(metrics: PlanMetrics, objective: f64) -> Self {
        Self { metrics, objective }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPlan {
    pub metrics: PlanMetrics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstraintCore {
    ids: Vec<String>,
}

impl ConstraintCore {
    pub fn new<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut unique = Vec::new();
        for id in ids {
            let id = id.into();
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        Self { ids: unique }
    }

    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn is_minimal<F>(&self, mut remains_infeasible: F) -> bool
    where
        F: FnMut(&[String]) -> bool,
    {
        if !remains_infeasible(&self.ids) {
            return false;
        }
        (0..self.ids.len()).all(|index| {
            let mut reduced = self.ids.clone();
            reduced.remove(index);
            !remains_infeasible(&reduced)
        })
    }

    pub fn trimmed_by<F>(&self, mut remains_infeasible: F) -> Option<Self>
    where
        F: FnMut(&[String]) -> bool,
    {
        if !remains_infeasible(&self.ids) {
            return None;
        }
        let mut reduced = self.ids.clone();
        let mut index = 0;
        while index < reduced.len() {
            let mut candidate = reduced.clone();
            candidate.remove(index);
            if remains_infeasible(&candidate) {
                reduced = candidate;
            } else {
                index += 1;
            }
        }
        Some(Self::new(reduced))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanVerificationError {
    ConstraintViolation {
        core: ConstraintCore,
        violations: Vec<String>,
    },
    UnsupportedConstraint {
        id: String,
    },
    NonFiniteApproximation,
}

impl fmt::Display for PlanVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConstraintViolation { core, .. } => {
                write!(f, "plan violates constraints: {:?}", core.ids())
            }
            Self::UnsupportedConstraint { id } => {
                write!(
                    f,
                    "constraint {id} is opaque and cannot be verified exactly"
                )
            }
            Self::NonFiniteApproximation => {
                f.write_str("approximate optimizer objective is not finite")
            }
        }
    }
}

impl std::error::Error for PlanVerificationError {}

fn quantity_at_least_exact(left: &Quantity, right: &Quantity) -> bool {
    if left.unit != right.unit && !left.is_zero() && !right.is_zero() {
        return false;
    }
    left.number >= right.number
}

impl From<String> for ConstraintExpression {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ConstraintExpression {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// A realized event is actual evidence referred to by a scenario link, never
/// a replacement for the forecast record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealizedEvent {
    pub id: OccurrenceId,
    pub date: Date,
    pub quantity: Option<Quantity>,
}

impl RealizedEvent {
    pub fn new(id: impl Into<OccurrenceId>, date: Date) -> Self {
        Self {
            id: id.into(),
            date,
            quantity: None,
        }
    }

    pub fn with_quantity(mut self, quantity: Quantity) -> Self {
        self.quantity = Some(quantity);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastLink {
    pub actual_root: ContentHash,
    pub occurrence: ForecastOccurrenceId,
    pub forecast_id: OccurrenceId,
    pub realized: RealizedEvent,
    pub variance: Variance,
}

impl ForecastLink {
    fn new(
        actual_root: ContentHash,
        occurrence: ForecastOccurrenceId,
        forecast: &ExpectedEvent,
        realized: RealizedEvent,
    ) -> Result<Self, ScenarioError> {
        let expected_date = occurrence.date;
        let quantity_delta = match (&forecast.quantity, &realized.quantity) {
            (Some(expected), Some(actual)) => {
                Some(actual.checked_sub(expected).map_err(|error| {
                    ScenarioError::IncompatibleVarianceUnit {
                        expected: expected.unit.as_ref().map(ToString::to_string),
                        realized: actual.unit.as_ref().map(ToString::to_string),
                        detail: error.to_string(),
                    }
                })?)
            }
            _ => None,
        };
        let variance = Variance {
            expected_date,
            realized_date: realized.date,
            expected_quantity: forecast.quantity.clone(),
            realized_quantity: realized.quantity.clone(),
            quantity_delta,
        };
        Ok(Self {
            actual_root,
            occurrence: occurrence.clone(),
            forecast_id: forecast.id.clone(),
            realized,
            variance,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variance {
    pub expected_date: Date,
    pub realized_date: Date,
    pub expected_quantity: Option<Quantity>,
    pub realized_quantity: Option<Quantity>,
    /// The delta remains a quantity so its unit cannot be discarded during
    /// forecast-vs-actual comparison.
    pub quantity_delta: Option<Quantity>,
}

impl Variance {
    pub fn date_changed(&self) -> bool {
        self.expected_date != self.realized_date
    }
    pub fn quantity_changed(&self) -> bool {
        self.expected_quantity != self.realized_quantity
    }
}

/// A semantic diff between scenarios.  The actual roots are reported
/// separately; projected changes never become actual events in this object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioDiff {
    pub left: String,
    pub right: String,
    pub actual_root_changed: bool,
    pub added_assumptions: Vec<Assumption>,
    pub removed_assumptions: Vec<Assumption>,
    pub changed_assumptions: Vec<ChangedAssumption>,
    pub added_expected_events: Vec<ExpectedEvent>,
    pub removed_expected_events: Vec<ExpectedEvent>,
    pub changed_expected_events: Vec<ChangedExpectedEvent>,
    pub added_constraints: Vec<Constraint>,
    pub removed_constraints: Vec<Constraint>,
    pub changed_constraints: Vec<ChangedConstraint>,
    pub added_links: Vec<ForecastLink>,
    pub removed_links: Vec<ForecastLink>,
    pub changed_links: Vec<ChangedForecastLink>,
}

impl ScenarioDiff {
    pub fn between(left: &Scenario, right: &Scenario) -> Self {
        let mut diff = Self {
            left: left.name.clone(),
            right: right.name.clone(),
            actual_root_changed: left.actual_root() != right.actual_root(),
            added_assumptions: Vec::new(),
            removed_assumptions: Vec::new(),
            changed_assumptions: Vec::new(),
            added_expected_events: Vec::new(),
            removed_expected_events: Vec::new(),
            changed_expected_events: Vec::new(),
            added_constraints: Vec::new(),
            removed_constraints: Vec::new(),
            changed_constraints: Vec::new(),
            added_links: Vec::new(),
            removed_links: Vec::new(),
            changed_links: Vec::new(),
        };
        diff_maps(
            &left.assumptions,
            &right.assumptions,
            &mut diff.added_assumptions,
            &mut diff.removed_assumptions,
            |a, b| {
                diff.changed_assumptions.push(ChangedAssumption {
                    id: a.id.clone(),
                    left: a.clone(),
                    right: b.clone(),
                })
            },
        );
        diff_maps(
            &left.expected_events,
            &right.expected_events,
            &mut diff.added_expected_events,
            &mut diff.removed_expected_events,
            |a, b| {
                diff.changed_expected_events.push(ChangedExpectedEvent {
                    id: a.id.clone(),
                    left: a.clone(),
                    right: b.clone(),
                })
            },
        );
        diff_maps(
            &left.constraints,
            &right.constraints,
            &mut diff.added_constraints,
            &mut diff.removed_constraints,
            |a, b| {
                diff.changed_constraints.push(ChangedConstraint {
                    id: a.id.clone(),
                    left: a.clone(),
                    right: b.clone(),
                })
            },
        );
        diff_link_maps(
            &left.links,
            &right.links,
            &mut diff.added_links,
            &mut diff.removed_links,
            |a, b| {
                diff.changed_links.push(ChangedForecastLink {
                    occurrence: a
                        .first()
                        .or_else(|| b.first())
                        .expect("changed link groups are non-empty")
                        .occurrence
                        .clone(),
                    left: a.clone(),
                    right: b.clone(),
                })
            },
        );
        diff
    }

    pub fn is_empty(&self) -> bool {
        !self.actual_root_changed
            && self.added_assumptions.is_empty()
            && self.removed_assumptions.is_empty()
            && self.changed_assumptions.is_empty()
            && self.added_expected_events.is_empty()
            && self.removed_expected_events.is_empty()
            && self.changed_expected_events.is_empty()
            && self.added_constraints.is_empty()
            && self.removed_constraints.is_empty()
            && self.changed_constraints.is_empty()
            && self.added_links.is_empty()
            && self.removed_links.is_empty()
            && self.changed_links.is_empty()
    }
}

pub type SemanticScenarioDiff = ScenarioDiff;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedAssumption {
    pub id: String,
    pub left: Assumption,
    pub right: Assumption,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedExpectedEvent {
    pub id: OccurrenceId,
    pub left: ExpectedEvent,
    pub right: ExpectedEvent,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedConstraint {
    pub id: String,
    pub left: Constraint,
    pub right: Constraint,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedForecastLink {
    pub occurrence: ForecastOccurrenceId,
    pub left: Vec<ForecastLink>,
    pub right: Vec<ForecastLink>,
}

fn diff_maps<K: Ord, V: Clone + Eq>(
    left: &BTreeMap<K, V>,
    right: &BTreeMap<K, V>,
    added: &mut Vec<V>,
    removed: &mut Vec<V>,
    mut changed: impl FnMut(&V, &V),
) {
    for (key, value) in left {
        match right.get(key) {
            Some(other) if other != value => changed(value, other),
            Some(_) => {}
            None => removed.push(value.clone()),
        }
    }
    for (key, value) in right {
        if !left.contains_key(key) {
            added.push(value.clone());
        }
    }
}

fn diff_link_maps<K: Ord, V: Clone + Eq>(
    left: &BTreeMap<K, Vec<V>>,
    right: &BTreeMap<K, Vec<V>>,
    added: &mut Vec<V>,
    removed: &mut Vec<V>,
    mut changed: impl FnMut(&Vec<V>, &Vec<V>),
) {
    for (key, values) in left {
        match right.get(key) {
            Some(other) if other != values => changed(values, other),
            Some(_) => {}
            None => removed.extend(values.iter().cloned()),
        }
    }
    for (key, values) in right {
        if !left.contains_key(key) {
            added.extend(values.iter().cloned());
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioError {
    EmptyName(&'static str),
    DuplicateId(&'static str),
    DuplicateAcceptedEvent,
    UnknownExpectedEvent(String),
    ActualEventNotAccepted(OccurrenceId),
    ActualEventMismatch(OccurrenceId),
    ActualRootMismatch,
    DuplicateRealizedEvent(OccurrenceId),
    ExplicitAllocationRequired(ForecastOccurrenceId),
    AllocationExceedsForecast(ForecastOccurrenceId),
    NegativeAllocation(OccurrenceId),
    IncompatibleAllocationUnit {
        detail: String,
    },
    RequiresOccurrenceIdentity(String),
    UnknownForecastOccurrence(ForecastOccurrenceId),
    DuplicateLink(ForecastOccurrenceId),
    MissingEventDate(String),
    IncompatibleVarianceUnit {
        expected: Option<String>,
        realized: Option<String>,
        detail: String,
    },
    InvalidHorizon,
    Time(TimeError),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName(kind) => write!(f, "empty {kind}"),
            Self::DuplicateId(kind) => write!(f, "duplicate {kind}"),
            Self::DuplicateAcceptedEvent => f.write_str("duplicate accepted actual event"),
            Self::UnknownExpectedEvent(id) => write!(f, "unknown expected event {id}"),
            Self::ActualEventNotAccepted(id) => {
                write!(f, "actual event {id} is not in the accepted root")
            }
            Self::ActualEventMismatch(id) => {
                write!(f, "actual event {id} differs from accepted evidence")
            }
            Self::ActualRootMismatch => {
                f.write_str("accepted event token belongs to another actual root")
            }
            Self::DuplicateRealizedEvent(id) => write!(f, "actual event {id} was already linked"),
            Self::ExplicitAllocationRequired(occurrence) => write!(
                f,
                "repeated links for forecast occurrence {}#{} require explicit quantities",
                occurrence.forecast_id, occurrence.index
            ),
            Self::AllocationExceedsForecast(occurrence) => write!(
                f,
                "realized allocations exceed forecast occurrence {}#{}",
                occurrence.forecast_id, occurrence.index
            ),
            Self::NegativeAllocation(id) => write!(f, "negative realized allocation {id}"),
            Self::IncompatibleAllocationUnit { detail } => {
                write!(f, "incompatible realized allocation units: {detail}")
            }
            Self::RequiresOccurrenceIdentity(id) => {
                write!(f, "forecast {id} requires an explicit occurrence identity")
            }
            Self::UnknownForecastOccurrence(occurrence) => write!(
                f,
                "unknown forecast occurrence {}#{} on {}",
                occurrence.forecast_id, occurrence.index, occurrence.date
            ),
            Self::DuplicateLink(occurrence) => write!(
                f,
                "forecast occurrence {}#{} on {} already has a realized link",
                occurrence.forecast_id, occurrence.index, occurrence.date
            ),
            Self::MissingEventDate(id) => write!(f, "expected event {id} has no date"),
            Self::IncompatibleVarianceUnit {
                expected,
                realized,
                detail,
            } => write!(
                f,
                "incompatible forecast variance units {:?} and {:?}: {detail}",
                expected, realized
            ),
            Self::InvalidHorizon => f.write_str("invalid or empty scenario horizon"),
            Self::Time(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ScenarioError {}

impl From<TimeError> for ScenarioError {
    fn from(value: TimeError) -> Self {
        Self::Time(value)
    }
}

fn checked_name(value: String, kind: &'static str) -> Result<String, ScenarioError> {
    if value.trim().is_empty() {
        Err(ScenarioError::EmptyName(kind))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::Exact;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn scenario_isolation_and_semantic_diff() {
        let root = ContentHash::domain_separated("test", b"accepted");
        let mut left = Scenario::new("base", root).unwrap();
        let mut right = Scenario::new("new-job", root).unwrap();
        left.add_assumption(Assumption::exact("growth", Exact::parse("0.02").unwrap()).unwrap())
            .unwrap();
        right
            .add_assumption(Assumption::exact("growth", Exact::parse("0.04").unwrap()).unwrap())
            .unwrap();
        assert_eq!(left.actual_root(), root);
        assert!(left.diff(&right).changed_assumptions.len() == 1);
        assert!(left.expected_events().next().is_none());
    }

    #[test]
    fn recurring_projection_requires_a_finite_horizon() {
        let start: crate::time::LocalDate = date("2024-01-01");
        let recurrence =
            Recurrence::new(start, crate::time::Frequency::Monthly { every: 1, day: 31 })
                .unwrap()
                .with_missing_day_policy(crate::time::MissingDayPolicy::ClampToLastDay);
        let bounded = BoundedRecurrence::new(
            recurrence,
            Horizon::new(date("2024-01-01"), date("2024-04-30")).unwrap(),
        )
        .unwrap();
        let event = ExpectedEvent::new("rent").recurring(bounded);
        let dates = event
            .occurrences(Horizon::new(date("2024-01-01"), date("2024-04-30")).unwrap())
            .unwrap();
        assert_eq!(dates.len(), 4);
    }

    #[test]
    fn linking_keeps_forecast_and_reports_variance() {
        let mut scenario =
            Scenario::new("baseline", ContentHash::domain_separated("test", b"r")).unwrap();
        scenario
            .add_expected_event(
                ExpectedEvent::dated("rent", date("2024-01-01")).with_quantity(
                    Quantity::with_unit(Exact::parse("100").unwrap(), "USD").unwrap(),
                ),
            )
            .unwrap();
        let realized = RealizedEvent::new("actual/rent", date("2024-01-03"))
            .with_quantity(Quantity::with_unit(Exact::parse("110").unwrap(), "USD").unwrap());
        scenario.register_actual_event(realized.clone()).unwrap();
        let link = scenario.link_realized("rent", realized).unwrap();
        assert!(link.variance.date_changed());
        assert_eq!(
            link.variance.quantity_delta,
            Some(Quantity::with_unit(Exact::parse("10").unwrap(), "USD").unwrap())
        );
        assert_eq!(
            scenario
                .expected_event(&OccurrenceId::new("rent"))
                .unwrap()
                .date,
            Some(date("2024-01-01"))
        );
    }

    #[test]
    fn recurring_occurrences_have_distinct_links_bound_to_root() {
        let root = ContentHash::domain_separated("test", b"root");
        let mut scenario = Scenario::new("baseline", root).unwrap();
        let recurrence = Recurrence::new(
            date("2024-01-01"),
            crate::time::Frequency::Monthly { every: 1, day: 1 },
        )
        .unwrap();
        let bounded = BoundedRecurrence::new(
            recurrence,
            Horizon::new(date("2024-01-01"), date("2024-03-01")).unwrap(),
        )
        .unwrap();
        scenario
            .add_expected_event(
                ExpectedEvent::new("rent")
                    .recurring(bounded)
                    .with_quantity(Quantity::with_unit(Exact::integer(100), "USD").unwrap()),
            )
            .unwrap();
        let occurrences = scenario
            .materialize(Horizon::new(date("2024-01-01"), date("2024-03-01")).unwrap())
            .unwrap();
        assert_eq!(occurrences.len(), 3);
        for (index, occurrence) in occurrences.iter().enumerate() {
            let actual = RealizedEvent::new(format!("actual/{index}"), occurrence.date)
                .with_quantity(
                    Quantity::with_unit(Exact::integer(if index == 0 { 60 } else { 100 }), "USD")
                        .unwrap(),
                );
            scenario.register_actual_event(actual.clone()).unwrap();
            let link = scenario
                .link_realized_occurrence(occurrence.identity(), actual)
                .unwrap();
            assert_eq!(link.actual_root, root);
        }
        assert_eq!(
            scenario.realized_links(&OccurrenceId::new("rent")).count(),
            3
        );
        let first = occurrences[0].identity();
        scenario
            .register_actual_event(
                RealizedEvent::new("actual/partial", date("2024-01-01"))
                    .with_quantity(Quantity::with_unit(Exact::integer(40), "USD").unwrap()),
            )
            .unwrap();
        scenario
            .link_realized_occurrence(
                first,
                RealizedEvent::new("actual/partial", date("2024-01-01"))
                    .with_quantity(Quantity::with_unit(Exact::integer(40), "USD").unwrap()),
            )
            .unwrap();
        assert_eq!(
            scenario.realized_links(&OccurrenceId::new("rent")).count(),
            4
        );
    }

    #[test]
    fn variance_rejects_unit_mismatch_and_diff_reports_changed_links() {
        let root = ContentHash::domain_separated("test", b"root");
        let mut left = Scenario::new("left", root).unwrap();
        left.add_expected_event(
            ExpectedEvent::dated("rent", date("2024-01-01"))
                .with_quantity(Quantity::with_unit(Exact::integer(100), "USD").unwrap()),
        )
        .unwrap();
        let occurrence = ForecastOccurrenceId::new("rent", 0, date("2024-01-01"));
        let bad = RealizedEvent::new("actual/bad", date("2024-01-01"))
            .with_quantity(Quantity::with_unit(Exact::integer(100), "EUR").unwrap());
        left.register_actual_event(bad.clone()).unwrap();
        assert!(matches!(
            left.link_realized_occurrence(occurrence.clone(), bad),
            Err(ScenarioError::IncompatibleVarianceUnit { .. })
        ));
        let good = RealizedEvent::new("actual/good", date("2024-01-02"))
            .with_quantity(Quantity::with_unit(Exact::integer(101), "USD").unwrap());
        left.register_actual_event(good.clone()).unwrap();
        left.link_realized_occurrence(occurrence.clone(), good)
            .unwrap();
        let mut right = left.clone();
        right.links.get_mut(&occurrence).unwrap()[0].realized.date = date("2024-01-03");
        let diff = left.diff(&right);
        assert_eq!(diff.changed_links.len(), 1);
    }

    #[test]
    fn realized_links_require_membership_and_conserve_partial_allocations() {
        let root = ContentHash::domain_separated("test", b"root");
        let mut scenario = Scenario::new("baseline", root).unwrap();
        scenario
            .add_expected_event(
                ExpectedEvent::dated("invoice", date("2024-01-01"))
                    .with_quantity(Quantity::with_unit(Exact::integer(100), "USD").unwrap()),
            )
            .unwrap();
        let first = RealizedEvent::new("actual/one", date("2024-01-01"))
            .with_quantity(Quantity::with_unit(Exact::integer(60), "USD").unwrap());
        let second = RealizedEvent::new("actual/two", date("2024-01-02"))
            .with_quantity(Quantity::with_unit(Exact::integer(50), "USD").unwrap());
        let occurrence = ForecastOccurrenceId::new("invoice", 0, date("2024-01-01"));
        assert!(matches!(
            scenario.link_realized_occurrence(occurrence.clone(), first.clone()),
            Err(ScenarioError::ActualEventNotAccepted(_))
        ));
        scenario.register_actual_event(first.clone()).unwrap();
        scenario.register_actual_event(second.clone()).unwrap();
        scenario
            .link_realized_occurrence(occurrence.clone(), first.clone())
            .unwrap();
        assert!(matches!(
            scenario.link_realized_occurrence(occurrence.clone(), first),
            Err(ScenarioError::DuplicateRealizedEvent(_))
        ));
        assert!(matches!(
            scenario.link_realized_occurrence(occurrence, second),
            Err(ScenarioError::AllocationExceedsForecast(_))
        ));
    }

    #[test]
    fn repeated_partial_links_reject_incompatible_units() {
        let root = ContentHash::domain_separated("test", b"units");
        let mut scenario = Scenario::new("units", root).unwrap();
        scenario
            .add_expected_event(
                ExpectedEvent::dated("invoice", date("2024-01-01"))
                    .with_quantity(Quantity::with_unit(Exact::integer(100), "USD").unwrap()),
            )
            .unwrap();
        let first = RealizedEvent::new("actual/usd", date("2024-01-01"))
            .with_quantity(Quantity::with_unit(Exact::integer(50), "USD").unwrap());
        let second = RealizedEvent::new("actual/eur", date("2024-01-02"))
            .with_quantity(Quantity::with_unit(Exact::integer(50), "EUR").unwrap());
        scenario.register_actual_event(first.clone()).unwrap();
        scenario.register_actual_event(second.clone()).unwrap();
        let occurrence = ForecastOccurrenceId::new("invoice", 0, date("2024-01-01"));
        scenario
            .link_realized_occurrence(occurrence.clone(), first)
            .unwrap();
        assert!(matches!(
            scenario.link_realized_occurrence(occurrence, second),
            Err(ScenarioError::IncompatibleAllocationUnit { .. })
        ));
    }

    #[test]
    fn nonminimal_constraint_core_is_trimmed() {
        let core = ConstraintCore::new(["necessary", "noise"]);
        let trimmed = core
            .trimmed_by(|ids| ids.iter().any(|id| id == "necessary"))
            .expect("necessary constraint keeps the set infeasible");
        assert_eq!(trimmed.ids(), &["necessary".to_string()]);
        assert!(trimmed.is_minimal(|ids| ids.iter().any(|id| id == "necessary")));
    }

    #[test]
    fn approximate_plan_is_verified_against_exact_constraints() {
        let root = ContentHash::domain_separated("test", b"plan");
        let mut scenario = Scenario::new("plan", root).unwrap();
        scenario
            .constrain(
                Constraint::new(
                    "cash-floor",
                    ConstraintExpression::QuantityAtLeast {
                        metric: "cash".into(),
                        amount: Quantity::with_unit(Exact::integer(100), "USD").unwrap(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let candidate = ApproximatePlanCandidate::new(
            PlanMetrics::new().with_quantity(
                "cash",
                Quantity::with_unit(Exact::integer(99), "USD").unwrap(),
            ),
            -1000.0,
        );
        let Err(PlanVerificationError::ConstraintViolation { core, .. }) =
            scenario.verify_approximate_plan(&candidate)
        else {
            panic!("infeasible approximate plan was accepted")
        };
        assert_eq!(core.ids(), &["cash-floor".to_string()]);
        assert!(core.is_minimal(|ids| ids == ["cash-floor".to_string()]));
    }

    #[test]
    fn nonfinite_approximate_objective_cannot_cross_boundary() {
        let root = ContentHash::domain_separated("test", b"nonfinite");
        let scenario = Scenario::new("plan", root).unwrap();
        let candidate = ApproximatePlanCandidate::new(PlanMetrics::new(), f64::NAN);
        assert_eq!(
            scenario.verify_approximate_plan(&candidate),
            Err(PlanVerificationError::NonFiniteApproximation)
        );
    }

    #[test]
    fn opaque_text_constraint_is_not_misreported_as_false() {
        let root = ContentHash::domain_separated("test", b"opaque-constraint");
        let mut scenario = Scenario::new("plan", root).unwrap();
        scenario
            .constrain(Constraint::text("manual", "cash stays comfortable").unwrap())
            .unwrap();
        assert_eq!(
            scenario.verify_plan(&PlanMetrics::new()),
            Err(PlanVerificationError::UnsupportedConstraint {
                id: "manual".into()
            })
        );
    }
}
