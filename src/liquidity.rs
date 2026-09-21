//! Exact liquidity routing.
//!
//! Liquidity is a constrained graph query, not a balance label.  Positions
//! and instruments are nodes; each admissible action is an edge with explicit
//! time, fee, capacity, permission, tax, and risk dimensions.  Search returns
//! nondominated routes and reports resource exhaustion as incomplete.  A
//! separate verifier recomputes every route metric and checks exact
//! feasibility before a route can be acted on.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::exact::Exact;
use crate::model::{InstrumentId, Quantity};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PositionId(String);

impl PositionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PositionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for PositionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
impl fmt::Display for PositionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum NodeId {
    Position(PositionId),
    Instrument(InstrumentId),
}

impl NodeId {
    pub fn position(id: impl Into<PositionId>) -> Self {
        Self::Position(id.into())
    }
    pub fn instrument(id: impl Into<InstrumentId>) -> Self {
        Self::Instrument(id.into())
    }
}

impl From<PositionId> for NodeId {
    fn from(value: PositionId) -> Self {
        Self::Position(value)
    }
}
impl From<InstrumentId> for NodeId {
    fn from(value: InstrumentId) -> Self {
        Self::Instrument(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Permissions(BTreeSet<String>);

impl Permissions {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }
    pub fn from<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(values.into_iter().map(Into::into).collect())
    }
    pub fn grant(&mut self, permission: impl Into<String>) {
        self.0.insert(permission.into());
    }
    pub fn allows(&self, permission: &str) -> bool {
        self.0.contains(permission)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionNode {
    pub id: PositionId,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    pub encumbrances: BTreeSet<String>,
    pub permissions: Permissions,
}

impl PositionNode {
    pub fn new(
        id: impl Into<PositionId>,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            id: id.into(),
            instrument: instrument.into(),
            quantity,
            encumbrances: BTreeSet::new(),
            permissions: Permissions::new(),
        }
    }
    pub fn encumber(mut self, id: impl Into<String>) -> Self {
        self.encumbrances.insert(id.into());
        self
    }
    pub fn grant(mut self, permission: impl Into<String>) -> Self {
        self.permissions.grant(permission);
        self
    }
    pub fn is_encumbered(&self) -> bool {
        !self.encumbrances.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentNode {
    pub id: InstrumentId,
}

impl InstrumentNode {
    pub fn new(id: impl Into<InstrumentId>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiquidityNode {
    Position(PositionNode),
    Instrument(InstrumentNode),
}

impl LiquidityNode {
    pub fn id(&self) -> NodeId {
        match self {
            Self::Position(node) => NodeId::Position(node.id.clone()),
            Self::Instrument(node) => NodeId::Instrument(node.id.clone()),
        }
    }
}

pub type Node = LiquidityNode;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ActionKind {
    Withdraw,
    Transfer,
    Sell,
    Redeem,
    Borrow,
    Convert,
    Settle,
    WaitForMaturity,
    ReleaseEncumbrance,
    Custom(String),
}

/// How much quantity an edge delivers for one unit of the query amount.
///
/// Preserve is the safe default for transfers within one instrument.  A
/// conversion must state both its exact ratio and output unit; this prevents
/// a route from silently treating an ABC position as USD merely because two
/// nodes happen to be adjacent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum QuantityEffect {
    #[default]
    Preserve,
    Convert {
        output_unit: String,
        ratio: Exact,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionEdge {
    pub id: String,
    pub from: NodeId,
    pub to: NodeId,
    pub action: ActionKind,
    pub time: u64,
    pub fee: Quantity,
    pub capacity: Option<Quantity>,
    pub required_permissions: BTreeSet<String>,
    pub tax: Quantity,
    pub risk: Exact,
    pub minimum: Option<Quantity>,
    pub allow_encumbered: bool,
    pub quantity_effect: QuantityEffect,
}

impl ActionEdge {
    pub fn new(id: impl Into<String>, from: NodeId, to: NodeId, action: ActionKind) -> Self {
        Self {
            id: id.into(),
            from,
            to,
            action,
            time: 0,
            fee: Quantity::zero(),
            capacity: None,
            required_permissions: BTreeSet::new(),
            tax: Quantity::zero(),
            risk: Exact::integer(0),
            minimum: None,
            allow_encumbered: false,
            quantity_effect: QuantityEffect::Preserve,
        }
    }
    pub fn with_time(mut self, time: u64) -> Self {
        self.time = time;
        self
    }
    pub fn with_fee(mut self, fee: Quantity) -> Self {
        self.fee = fee;
        self
    }
    pub fn with_capacity(mut self, capacity: Quantity) -> Self {
        self.capacity = Some(capacity);
        self
    }
    pub fn unlimited(mut self) -> Self {
        self.capacity = None;
        self
    }
    pub fn requires_permission(mut self, permission: impl Into<String>) -> Self {
        self.required_permissions.insert(permission.into());
        self
    }
    pub fn requires_permissions<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_permissions
            .extend(values.into_iter().map(Into::into));
        self
    }
    pub fn with_tax(mut self, tax: Quantity) -> Self {
        self.tax = tax;
        self
    }
    pub fn with_risk(mut self, risk: Exact) -> Self {
        self.risk = risk;
        self
    }
    pub fn with_minimum(mut self, minimum: Quantity) -> Self {
        self.minimum = Some(minimum);
        self
    }
    pub fn allow_encumbered(mut self, allow: bool) -> Self {
        self.allow_encumbered = allow;
        self
    }
    pub fn releases_encumbrance(mut self) -> Self {
        self.action = ActionKind::ReleaseEncumbrance;
        self
    }
    pub fn with_quantity_effect(mut self, effect: QuantityEffect) -> Self {
        self.quantity_effect = effect;
        self
    }
    pub fn with_conversion(mut self, output_unit: impl Into<String>, ratio: Exact) -> Self {
        self.quantity_effect = QuantityEffect::Convert {
            output_unit: output_unit.into(),
            ratio,
        };
        self.action = ActionKind::Convert;
        self
    }
}

pub type Edge = ActionEdge;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiquidityGraph {
    pub nodes: BTreeMap<NodeId, LiquidityNode>,
    pub edges: BTreeMap<String, ActionEdge>,
    outgoing: BTreeMap<NodeId, Vec<String>>,
}

impl Default for LiquidityGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl LiquidityGraph {
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            outgoing: BTreeMap::new(),
        }
    }

    pub fn add_node(&mut self, node: LiquidityNode) -> Result<(), LiquidityError> {
        let id = node.id();
        if self.nodes.insert(id, node).is_some() {
            return Err(LiquidityError::DuplicateNode);
        }
        Ok(())
    }
    pub fn add_position(&mut self, node: PositionNode) -> Result<(), LiquidityError> {
        if node.quantity.number.is_negative() {
            return Err(LiquidityError::NegativeQuantity);
        }
        if let Some(unit) = &node.quantity.unit
            && unit.as_str() != node.instrument.as_str()
        {
            return Err(LiquidityError::PositionUnitMismatch);
        }
        self.add_node(LiquidityNode::Position(node))
    }
    pub fn add_instrument(&mut self, node: InstrumentNode) -> Result<(), LiquidityError> {
        self.add_node(LiquidityNode::Instrument(node))
    }

    pub fn add_edge(&mut self, edge: ActionEdge) -> Result<(), LiquidityError> {
        if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
            return Err(LiquidityError::UnknownNode);
        }
        if self.edges.contains_key(&edge.id) {
            return Err(LiquidityError::DuplicateEdge);
        }
        self.validate_edge(&edge)?;
        self.outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.id.clone());
        self.outgoing
            .get_mut(&edge.from)
            .expect("outgoing edge index was inserted")
            .sort();
        self.edges.insert(edge.id.clone(), edge);
        Ok(())
    }

    fn validate_edge(&self, edge: &ActionEdge) -> Result<(), LiquidityError> {
        if edge.fee.number.is_negative() || edge.tax.number.is_negative() || edge.risk.is_negative()
        {
            return Err(LiquidityError::NegativeCost);
        }
        if edge
            .capacity
            .as_ref()
            .is_some_and(|quantity| quantity.number.is_negative())
            || edge
                .minimum
                .as_ref()
                .is_some_and(|quantity| quantity.number.is_negative())
        {
            return Err(LiquidityError::NegativeCapacity);
        }
        let source_unit = self.node_unit(&edge.from);
        for quantity in [&edge.capacity, &edge.minimum].into_iter().flatten() {
            if !quantity.is_zero()
                && quantity
                    .unit
                    .as_ref()
                    .is_some_and(|unit| source_unit.as_deref() != Some(unit.as_str()))
            {
                return Err(LiquidityError::CapacityUnitMismatch);
            }
        }
        if let (Some(minimum), Some(capacity)) = (&edge.minimum, &edge.capacity)
            && quantity_at_least(capacity, minimum)?
        {
            // A capacity is valid only when it is at least its minimum.  The
            // inverse check below keeps the error explicit without relying on
            // a floating tolerance.
        } else if edge.minimum.is_some() && edge.capacity.is_some() {
            return Err(LiquidityError::MinimumExceedsCapacity);
        }
        if let QuantityEffect::Convert { output_unit, ratio } = &edge.quantity_effect {
            if output_unit.trim().is_empty() {
                return Err(LiquidityError::ConversionUnitMismatch);
            }
            if ratio.is_negative() {
                return Err(LiquidityError::InvalidConversionRatio);
            }
            if self.node_unit(&edge.to).as_deref() != Some(output_unit.as_str()) {
                return Err(LiquidityError::ConversionUnitMismatch);
            }
        } else if self.node_unit(&edge.from) != self.node_unit(&edge.to) {
            return Err(LiquidityError::ConversionUnitMismatch);
        }
        Ok(())
    }

    pub fn edge(&self, id: &str) -> Option<&ActionEdge> {
        self.edges.get(id)
    }
    pub fn node<N: Borrow<NodeId>>(&self, id: N) -> Option<&LiquidityNode> {
        self.nodes.get(id.borrow())
    }

    pub fn pareto_routes<N1: Borrow<NodeId>, N2: Borrow<NodeId>>(
        &self,
        from: N1,
        to: N2,
        amount: &Quantity,
    ) -> Result<SearchResult, LiquidityError> {
        self.search(from, to, amount, SearchLimits::default())
    }

    pub fn find_routes<N1: Borrow<NodeId>, N2: Borrow<NodeId>>(
        &self,
        from: N1,
        to: N2,
        amount: &Quantity,
        limits: SearchLimits,
    ) -> Result<SearchResult, LiquidityError> {
        self.search(from, to, amount, limits)
    }

    pub fn search<N1: Borrow<NodeId>, N2: Borrow<NodeId>>(
        &self,
        from: N1,
        to: N2,
        amount: &Quantity,
        limits: SearchLimits,
    ) -> Result<SearchResult, LiquidityError> {
        let from = from.borrow();
        let to = to.borrow();
        if amount.number.is_negative() {
            return Err(LiquidityError::NegativeQuantity);
        }
        if !self.nodes.contains_key(from) || !self.nodes.contains_key(to) {
            return Err(LiquidityError::UnknownNode);
        }
        if !self.start_feasible(from, amount)? {
            return Ok(SearchResult::complete());
        }
        let mut state = SearchState {
            result: SearchResult::complete(),
            origin: from.clone(),
            expansions: 0,
            requested_amount: amount.clone(),
            amount: amount.clone(),
            limits,
            path: Vec::new(),
            visited: BTreeSet::from([from.clone()]),
            metrics: RouteMetrics::zero(),
        };
        self.visit(from, to, &mut state)?;
        state.result.expanded = state.expansions;
        if let SearchCompletion::Incomplete { limit, .. } = state.result.completion {
            state.result.completion = SearchCompletion::Incomplete {
                expanded: state.expansions,
                limit,
            };
        }
        if state.result.routes.len() > 1 {
            pareto_filter(&mut state.result.routes);
        }
        Ok(state.result)
    }

    fn visit(
        &self,
        current: &NodeId,
        target: &NodeId,
        state: &mut SearchState,
    ) -> Result<(), LiquidityError> {
        if current == target {
            state.result.routes.push(LiquidityRoute {
                from: state.origin.clone(),
                to: target.clone(),
                edges: state.path.clone(),
                metrics: state.metrics.clone(),
                amount: state.requested_amount.clone(),
            });
            return Ok(());
        }
        if state.path.len() >= state.limits.max_depth {
            state
                .result
                .mark_incomplete(state.expansions, state.limits.max_depth);
            return Ok(());
        }
        let edge_ids = self.outgoing.get(current).cloned().unwrap_or_default();
        for edge_id in edge_ids {
            if state.expansions >= state.limits.max_expansions {
                state
                    .result
                    .mark_incomplete(state.expansions, state.limits.max_expansions);
                break;
            }
            state.expansions += 1;
            let edge = self
                .edges
                .get(&edge_id)
                .expect("outgoing edge index is internal");
            if !self.edge_feasible(edge, &state.amount)? || state.visited.contains(&edge.to) {
                continue;
            }
            let next_metrics = state.metrics.add(edge)?;
            let next_amount = self.output_quantity(edge, &state.amount)?;
            state.path.push(edge.id.clone());
            state.visited.insert(edge.to.clone());
            let old = state.metrics.clone();
            let old_amount = state.amount.clone();
            state.metrics = next_metrics;
            state.amount = next_amount;
            self.visit(&edge.to, target, state)?;
            state.metrics = old;
            state.amount = old_amount;
            state.visited.remove(&edge.to);
            state.path.pop();
        }
        Ok(())
    }

    fn edge_feasible(&self, edge: &ActionEdge, amount: &Quantity) -> Result<bool, LiquidityError> {
        if !self.quantity_matches_node(&edge.from, amount)? {
            return Ok(false);
        }
        if let Some(LiquidityNode::Position(position)) = self.nodes.get(&edge.from) {
            if !quantity_at_least(&position.quantity, amount)? {
                return Ok(false);
            }
            if !edge.allow_encumbered
                && position.is_encumbered()
                && edge.action != ActionKind::ReleaseEncumbrance
            {
                return Ok(false);
            }
            if edge
                .required_permissions
                .iter()
                .any(|permission| !position.permissions.allows(permission))
            {
                return Ok(false);
            }
        }
        if let Some(capacity) = &edge.capacity
            && !quantity_at_least(capacity, amount)?
        {
            return Ok(false);
        }
        if let Some(minimum) = &edge.minimum
            && !quantity_at_least(amount, minimum)?
        {
            return Ok(false);
        }
        if self.output_quantity(edge, amount).is_err() {
            return Ok(false);
        }
        Ok(true)
    }

    fn start_feasible(&self, node: &NodeId, amount: &Quantity) -> Result<bool, LiquidityError> {
        if !self.quantity_matches_node(node, amount)? {
            return Ok(false);
        }
        match self.nodes.get(node) {
            Some(LiquidityNode::Position(position)) => {
                quantity_at_least(&position.quantity, amount)
            }
            _ => Ok(true),
        }
    }

    fn quantity_matches_node(
        &self,
        node: &NodeId,
        amount: &Quantity,
    ) -> Result<bool, LiquidityError> {
        let Some(unit) = self.node_unit(node) else {
            return Ok(true);
        };
        Ok(amount
            .unit
            .as_ref()
            .is_none_or(|value| value.as_str() == unit))
    }

    fn node_unit(&self, node: &NodeId) -> Option<String> {
        match self.nodes.get(node) {
            Some(LiquidityNode::Position(position)) => Some(position.instrument.to_string()),
            Some(LiquidityNode::Instrument(instrument)) => Some(instrument.id.to_string()),
            None => None,
        }
    }

    fn output_quantity(
        &self,
        edge: &ActionEdge,
        amount: &Quantity,
    ) -> Result<Quantity, LiquidityError> {
        let output = match &edge.quantity_effect {
            QuantityEffect::Preserve => amount.clone(),
            QuantityEffect::Convert { output_unit, ratio } => {
                if ratio.is_negative() {
                    return Err(LiquidityError::InvalidConversionRatio);
                }
                let number = amount.number.checked_mul(ratio);
                if !amount.number.is_zero() && (number.is_zero() || number.is_negative()) {
                    return Err(LiquidityError::ZeroConversionOutput);
                }
                Quantity::with_unit(number, output_unit.clone()).map_err(LiquidityError::Model)?
            }
        };
        if !self.quantity_matches_node(&edge.to, &output)? {
            return Err(LiquidityError::ConversionUnitMismatch);
        }
        Ok(output)
    }

    pub fn verify_route<N1: Borrow<NodeId>, N2: Borrow<NodeId>>(
        &self,
        route: &LiquidityRoute,
        from: N1,
        to: N2,
        amount: &Quantity,
    ) -> Result<VerifiedRoute, VerificationError> {
        let from = from.borrow();
        let to = to.borrow();
        if amount.number.is_negative() {
            return Err(VerificationError::Liquidity(
                LiquidityError::NegativeQuantity,
            ));
        }
        if route.from != *from || route.to != *to || route.amount != *amount {
            return Err(VerificationError::RouteIdentity);
        }
        if route.edges.is_empty() && from != to {
            return Err(VerificationError::Disconnected);
        }
        if !self
            .start_feasible(from, amount)
            .map_err(VerificationError::Liquidity)?
        {
            return Err(VerificationError::Infeasible);
        }
        let mut current = from.clone();
        let mut current_amount = amount.clone();
        let mut metrics = RouteMetrics::zero();
        let mut seen = BTreeSet::from([current.clone()]);
        for edge_id in &route.edges {
            let edge = self
                .edges
                .get(edge_id)
                .ok_or(VerificationError::UnknownEdge)?;
            if edge.from != current {
                return Err(VerificationError::Disconnected);
            }
            if !seen.insert(edge.to.clone()) {
                return Err(VerificationError::Cycle);
            }
            if !self
                .edge_feasible(edge, &current_amount)
                .map_err(VerificationError::Liquidity)?
            {
                return Err(VerificationError::Infeasible);
            }
            metrics = metrics.add(edge).map_err(VerificationError::Liquidity)?;
            // Recompute the quantity effect at every edge.  The final amount
            // is deliberately not trusted from the route object.
            current_amount = self
                .output_quantity(edge, &current_amount)
                .map_err(VerificationError::Liquidity)?;
            current = edge.to.clone();
        }
        if current != *to {
            return Err(VerificationError::Disconnected);
        }
        if metrics != route.metrics {
            return Err(VerificationError::MetricMismatch);
        }
        Ok(VerifiedRoute {
            route: route.clone(),
            metrics,
        })
    }

    pub fn verify<N1: Borrow<NodeId>, N2: Borrow<NodeId>>(
        &self,
        route: &LiquidityRoute,
        from: N1,
        to: N2,
        amount: &Quantity,
    ) -> Result<VerifiedRoute, VerificationError> {
        self.verify_route(route, from, to, amount)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SearchState {
    result: SearchResult,
    origin: NodeId,
    expansions: usize,
    requested_amount: Quantity,
    amount: Quantity,
    limits: SearchLimits,
    path: Vec<String>,
    visited: BTreeSet<NodeId>,
    metrics: RouteMetrics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchLimits {
    pub max_expansions: usize,
    pub max_depth: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_expansions: 100_000,
            max_depth: 256,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchCompletion {
    Complete,
    Incomplete { expanded: usize, limit: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub routes: Vec<LiquidityRoute>,
    pub completion: SearchCompletion,
    expanded: usize,
}

impl SearchResult {
    fn complete() -> Self {
        Self {
            routes: Vec::new(),
            completion: SearchCompletion::Complete,
            expanded: 0,
        }
    }
    fn mark_incomplete(&mut self, expanded: usize, limit: usize) {
        self.completion = SearchCompletion::Incomplete { expanded, limit };
    }
    pub fn is_complete(&self) -> bool {
        matches!(self.completion, SearchCompletion::Complete)
    }
    pub fn is_incomplete(&self) -> bool {
        !self.is_complete()
    }
    pub fn incomplete(&self) -> bool {
        self.is_incomplete()
    }
    pub fn expanded(&self) -> usize {
        self.expanded
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiquidityRoute {
    pub from: NodeId,
    pub to: NodeId,
    pub edges: Vec<String>,
    pub metrics: RouteMetrics,
    pub amount: Quantity,
}

impl LiquidityRoute {
    pub fn edge_ids(&self) -> &[String] {
        &self.edges
    }
    pub fn time(&self) -> u64 {
        self.metrics.time
    }
    pub fn fee(&self) -> &Quantity {
        &self.metrics.fee
    }
    pub fn tax(&self) -> &Quantity {
        &self.metrics.tax
    }
    pub fn risk(&self) -> &Exact {
        &self.metrics.risk
    }
}

pub type Route = LiquidityRoute;
pub type ParetoRoutes = SearchResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteMetrics {
    pub time: u64,
    pub fee: Quantity,
    pub tax: Quantity,
    pub risk: Exact,
}

impl RouteMetrics {
    fn zero() -> Self {
        Self {
            time: 0,
            fee: Quantity::zero(),
            tax: Quantity::zero(),
            risk: Exact::integer(0),
        }
    }
    fn add(&self, edge: &ActionEdge) -> Result<Self, LiquidityError> {
        Ok(Self {
            time: self
                .time
                .checked_add(edge.time)
                .ok_or(LiquidityError::Overflow)?,
            fee: self
                .fee
                .checked_add(&edge.fee)
                .map_err(LiquidityError::Model)?,
            tax: self
                .tax
                .checked_add(&edge.tax)
                .map_err(LiquidityError::Model)?,
            risk: self.risk.checked_add(&edge.risk),
        })
    }
    pub fn dominates(&self, other: &Self) -> bool {
        // A single fee/tax scalar is comparable only when both routes use a
        // compatible unit.  Otherwise neither route may discard the other's
        // alternative merely because its numeric spelling is smaller.
        if !metric_units_compatible(&self.fee, &other.fee)
            || !metric_units_compatible(&self.tax, &other.tax)
        {
            return false;
        }
        let no_worse = self.time <= other.time
            && self.fee.number <= other.fee.number
            && self.tax.number <= other.tax.number
            && self.risk <= other.risk;
        let better = self.time < other.time
            || self.fee.number < other.fee.number
            || self.tax.number < other.tax.number
            || self.risk < other.risk;
        no_worse && better
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRoute {
    pub route: LiquidityRoute,
    pub metrics: RouteMetrics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiquidityError {
    DuplicateNode,
    DuplicateEdge,
    UnknownNode,
    Overflow,
    IncompatibleUnit,
    ConversionUnitMismatch,
    InvalidConversionRatio,
    ZeroConversionOutput,
    NegativeQuantity,
    NegativeCost,
    NegativeCapacity,
    PositionUnitMismatch,
    CapacityUnitMismatch,
    MinimumExceedsCapacity,
    Model(crate::model::ModelError),
}

impl fmt::Display for LiquidityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode => f.write_str("duplicate liquidity node"),
            Self::DuplicateEdge => f.write_str("duplicate liquidity edge"),
            Self::UnknownNode => f.write_str("unknown liquidity node"),
            Self::Overflow => f.write_str("liquidity metric overflow"),
            Self::IncompatibleUnit => f.write_str("incompatible liquidity quantity unit"),
            Self::ConversionUnitMismatch => {
                f.write_str("liquidity conversion has incompatible node units")
            }
            Self::InvalidConversionRatio => f.write_str("liquidity conversion ratio is negative"),
            Self::ZeroConversionOutput => {
                f.write_str("positive liquidity input converts to zero output")
            }
            Self::NegativeQuantity => f.write_str("liquidity position quantity is negative"),
            Self::NegativeCost => f.write_str("liquidity fee, tax, or risk is negative"),
            Self::NegativeCapacity => f.write_str("liquidity capacity or minimum is negative"),
            Self::PositionUnitMismatch => {
                f.write_str("position quantity unit differs from instrument")
            }
            Self::CapacityUnitMismatch => {
                f.write_str("capacity unit differs from source instrument")
            }
            Self::MinimumExceedsCapacity => f.write_str("minimum quantity exceeds edge capacity"),
            Self::Model(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for LiquidityError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationError {
    RouteIdentity,
    UnknownEdge,
    Disconnected,
    Cycle,
    Infeasible,
    MetricMismatch,
    Liquidity(LiquidityError),
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RouteIdentity => f.write_str("route identity does not match query"),
            Self::UnknownEdge => f.write_str("route names an unknown edge"),
            Self::Disconnected => f.write_str("route edges are not connected"),
            Self::Cycle => f.write_str("route contains a cycle"),
            Self::Infeasible => f.write_str("route is not exactly feasible"),
            Self::MetricMismatch => f.write_str("route metrics are not canonical for its edges"),
            Self::Liquidity(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for VerificationError {}

fn quantity_at_least(left: &Quantity, right: &Quantity) -> Result<bool, LiquidityError> {
    if left.unit != right.unit && !left.is_zero() && !right.is_zero() {
        return Err(LiquidityError::IncompatibleUnit);
    }
    Ok(left.number >= right.number)
}

fn metric_units_compatible(left: &Quantity, right: &Quantity) -> bool {
    left.is_zero() || right.is_zero() || left.unit == right.unit
}

fn pareto_filter(routes: &mut Vec<LiquidityRoute>) {
    let mut keep = Vec::new();
    for (index, candidate) in routes.iter().enumerate() {
        if routes.iter().enumerate().any(|(other_index, other)| {
            other_index != index && other.metrics.dominates(&candidate.metrics)
        }) {
            continue;
        }
        if !keep.iter().any(|route: &LiquidityRoute| {
            route.metrics == candidate.metrics && route.edges == candidate.edges
        }) {
            keep.push(candidate.clone());
        }
    }
    *routes = keep;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::Exact;

    fn amount(value: &str) -> Quantity {
        Quantity::with_unit(Exact::parse(value).unwrap(), "USD").unwrap()
    }
    fn graph() -> (LiquidityGraph, NodeId, NodeId) {
        let source = NodeId::position("cash");
        let mid = NodeId::instrument("USD");
        let target = NodeId::position("spendable");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new("cash", "USD", amount("100")))
            .unwrap();
        graph.add_instrument(InstrumentNode::new("USD")).unwrap();
        graph
            .add_position(PositionNode::new("spendable", "USD", amount("0")))
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new("slow", source.clone(), mid.clone(), ActionKind::Transfer)
                    .with_time(3)
                    .with_capacity(amount("100"))
                    .with_fee(amount("1")),
            )
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new("fast", source.clone(), target.clone(), ActionKind::Transfer)
                    .with_time(1)
                    .with_capacity(amount("100"))
                    .with_fee(amount("2")),
            )
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new("settle", mid.clone(), target.clone(), ActionKind::Settle)
                    .with_time(3)
                    .with_capacity(amount("100"))
                    .with_fee(amount("0")),
            )
            .unwrap();
        (graph, source, target)
    }

    #[test]
    fn pareto_keeps_cost_time_tradeoff() {
        let (graph, source, target) = graph();
        let result = graph
            .pareto_routes(&source, &target, &amount("10"))
            .unwrap();
        assert!(result.is_complete());
        assert_eq!(result.routes.len(), 2);
    }

    #[test]
    fn encumbrance_and_capacity_are_exactly_blocking() {
        let source = NodeId::position("cash");
        let target = NodeId::position("out");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new("cash", "USD", amount("10")).encumber("pledge"))
            .unwrap();
        graph
            .add_position(PositionNode::new("out", "USD", amount("0")))
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new(
                    "withdraw",
                    source.clone(),
                    target.clone(),
                    ActionKind::Withdraw,
                )
                .with_capacity(amount("5")),
            )
            .unwrap();
        assert!(
            graph
                .pareto_routes(&source, &target, &amount("6"))
                .unwrap()
                .routes
                .is_empty()
        );
        assert!(
            graph
                .pareto_routes(&source, &target, &amount("4"))
                .unwrap()
                .routes
                .is_empty()
        );
    }

    #[test]
    fn resource_limit_is_incomplete_not_false() {
        let (graph, source, target) = graph();
        let result = graph
            .search(
                &source,
                &target,
                &amount("10"),
                SearchLimits {
                    max_expansions: 1,
                    max_depth: 20,
                },
            )
            .unwrap();
        assert!(result.is_incomplete());
    }

    #[test]
    fn verifier_rejects_tampered_metrics() {
        let (graph, source, target) = graph();
        let result = graph
            .pareto_routes(&source, &target, &amount("10"))
            .unwrap();
        let mut route = result.routes[0].clone();
        route.metrics.time += 99;
        assert!(matches!(
            graph.verify_route(&route, &source, &target, &amount("10")),
            Err(VerificationError::MetricMismatch)
        ));
    }

    #[test]
    fn conversion_requires_explicit_ratio_and_preserves_units() {
        let source = NodeId::position("shares");
        let instrument = NodeId::instrument("USD");
        let target = NodeId::position("cash");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new(
                "shares",
                "ABC",
                Quantity::with_unit(Exact::integer(10), "ABC").unwrap(),
            ))
            .unwrap();
        graph.add_instrument(InstrumentNode::new("USD")).unwrap();
        graph
            .add_position(PositionNode::new(
                "cash",
                "USD",
                Quantity::with_unit(Exact::integer(0), "USD").unwrap(),
            ))
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new(
                    "convert",
                    source.clone(),
                    instrument.clone(),
                    ActionKind::Convert,
                )
                .with_conversion("USD", Exact::integer(2)),
            )
            .unwrap();
        graph
            .add_edge(ActionEdge::new(
                "deposit",
                instrument.clone(),
                target.clone(),
                ActionKind::Transfer,
            ))
            .unwrap();
        let result = graph
            .pareto_routes(
                &source,
                &target,
                &Quantity::with_unit(Exact::integer(3), "ABC").unwrap(),
            )
            .unwrap();
        assert_eq!(result.routes.len(), 1);
        let route = &result.routes[0];
        assert!(
            graph
                .verify_route(
                    route,
                    &source,
                    &target,
                    &Quantity::with_unit(Exact::integer(3), "ABC").unwrap(),
                )
                .is_ok()
        );
    }

    #[test]
    fn fee_units_preserve_incomparable_routes() {
        let source = NodeId::position("cash");
        let target = NodeId::position("out");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new("cash", "USD", amount("10")))
            .unwrap();
        graph
            .add_position(PositionNode::new("out", "USD", amount("0")))
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new(
                    "usd-fee",
                    source.clone(),
                    target.clone(),
                    ActionKind::Transfer,
                )
                .with_fee(amount("1")),
            )
            .unwrap();
        graph
            .add_edge(
                ActionEdge::new(
                    "eur-fee",
                    source.clone(),
                    target.clone(),
                    ActionKind::Transfer,
                )
                .with_fee(Quantity::with_unit(Exact::integer(1), "EUR").unwrap()),
            )
            .unwrap();
        let result = graph.pareto_routes(&source, &target, &amount("1")).unwrap();
        assert_eq!(result.routes.len(), 2);
    }

    #[test]
    fn edge_validation_rejects_negative_costs_and_mismatched_capacity() {
        let source = NodeId::position("cash");
        let target = NodeId::position("out");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new("cash", "USD", amount("10")))
            .unwrap();
        graph
            .add_position(PositionNode::new("out", "USD", amount("0")))
            .unwrap();
        assert!(matches!(
            graph.add_edge(
                ActionEdge::new(
                    "negative",
                    source.clone(),
                    target.clone(),
                    ActionKind::Transfer
                )
                .with_fee(Quantity::with_unit(Exact::integer(-1), "USD").unwrap())
            ),
            Err(LiquidityError::NegativeCost)
        ));
        assert!(matches!(
            graph.add_edge(
                ActionEdge::new(
                    "wrong-unit",
                    source.clone(),
                    target.clone(),
                    ActionKind::Transfer
                )
                .with_capacity(Quantity::with_unit(Exact::integer(1), "EUR").unwrap())
            ),
            Err(LiquidityError::CapacityUnitMismatch)
        ));
    }

    #[test]
    fn positive_input_cannot_use_zero_conversion_output() {
        let source = NodeId::position("shares");
        let target = NodeId::instrument("USD");
        let mut graph = LiquidityGraph::new();
        graph
            .add_position(PositionNode::new(
                "shares",
                "ABC",
                Quantity::with_unit(Exact::integer(10), "ABC").unwrap(),
            ))
            .unwrap();
        graph.add_instrument(InstrumentNode::new("USD")).unwrap();
        graph
            .add_edge(
                ActionEdge::new("zero", source.clone(), target.clone(), ActionKind::Convert)
                    .with_conversion("USD", Exact::integer(0)),
            )
            .unwrap();
        let result = graph
            .pareto_routes(
                &source,
                &target,
                &Quantity::with_unit(Exact::integer(1), "ABC").unwrap(),
            )
            .unwrap();
        assert!(result.routes.is_empty());
    }
}
