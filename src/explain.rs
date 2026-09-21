//! Structured, proof-bearing explanations for semantic queries.
//!
//! The solver's [`crate::semantics::Resolution`] is deliberately a compact
//! value.  A client that wants to render a diagnostic, however, needs a
//! little more structure than a list of strings: positive and negative
//! support must remain separate, residual requirements must not be mistaken
//! for refutations, and a change must identify the exact proof roots it can
//! invalidate.  This module is that boundary.
//!
//! The author-facing entry point is [`Explanation::from_resolution`].  It
//! retains the canonical IDs already used by the semantic and proof layers;
//! it does not mint a second proof identity.  Richer callers can use
//! [`ExplanationBuilder`] to attach scoped decisions, completeness claims,
//! and dependency impacts without changing the underlying result.

use std::collections::BTreeSet;
use std::fmt;

use crate::model::DecisionId;
use crate::proof::ProofId;
use crate::semantics::{
    CompletenessId, CompletenessScope, Completion, Conflict, DecisionScope, GoalId, Multiplicity,
    Repair, Requirement, Resolution, Truth,
};

/// A queryable proposition identified by the canonical semantic goal ID.
///
/// There is intentionally no second proposition hash here.  `GoalId` is the
/// canonical identity used by the semantic layer and can therefore be used
/// directly as a cache, navigation, and explanation key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Proposition {
    goal: GoalId,
}

impl Proposition {
    pub fn new(goal: GoalId) -> Self {
        Self { goal }
    }

    pub fn goal(self) -> GoalId {
        self.goal
    }
}

impl From<GoalId> for Proposition {
    fn from(goal: GoalId) -> Self {
        Self::new(goal)
    }
}

/// One semantic dependency edge and its proof roots.
///
/// A dependency is intentionally keyed by an existing [`GoalId`].  The
/// proof roots are sorted and deduplicated so source or solver traversal
/// order cannot change an explanation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Dependency {
    goal: GoalId,
    proof_roots: Vec<ProofId>,
}

impl Dependency {
    pub fn new(
        goal: GoalId,
        proof_roots: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        let proof_roots = canonical_proofs(proof_roots)?;
        Ok(Self { goal, proof_roots })
    }

    pub fn goal(&self) -> GoalId {
        self.goal
    }

    pub fn proof_roots(&self) -> &[ProofId] {
        &self.proof_roots
    }
}

/// A decision reference retained in an explanation with its scope.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopedDecision {
    id: DecisionId,
    scope: DecisionScope,
}

impl ScopedDecision {
    pub fn new(id: impl Into<DecisionId>, scope: DecisionScope) -> Result<Self, ExplainError> {
        let id = id.into();
        if id.as_str().trim().is_empty() {
            return Err(ExplainError::EmptyIdentifier("decision id"));
        }
        Ok(Self { id, scope })
    }

    pub fn id(&self) -> &DecisionId {
        &self.id
    }

    pub fn scope(&self) -> &DecisionScope {
        &self.scope
    }
}

/// A completeness reference retained with its relation/world scope.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopedCompleteness {
    id: CompletenessId,
    scope: CompletenessScope,
}

impl ScopedCompleteness {
    pub fn new(id: CompletenessId, scope: CompletenessScope) -> Self {
        Self { id, scope }
    }

    pub fn id(&self) -> CompletenessId {
        self.id
    }

    pub fn scope(&self) -> &CompletenessScope {
        &self.scope
    }
}

/// The positive part of a proposition's explanation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Why {
    proposition: Proposition,
    proof_ids: Vec<ProofId>,
    dependencies: Vec<Dependency>,
    decisions: Vec<ScopedDecision>,
    completeness: Vec<ScopedCompleteness>,
}

impl Why {
    pub fn new(
        proposition: Proposition,
        proof_ids: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        Self::with_context(proposition, proof_ids, [], [], [])
    }

    pub fn with_context(
        proposition: Proposition,
        proof_ids: impl IntoIterator<Item = ProofId>,
        dependencies: impl IntoIterator<Item = Dependency>,
        decisions: impl IntoIterator<Item = ScopedDecision>,
        completeness: impl IntoIterator<Item = ScopedCompleteness>,
    ) -> Result<Self, ExplainError> {
        Ok(Self {
            proposition,
            proof_ids: canonical_proofs(proof_ids)?,
            dependencies: canonical_dependencies(dependencies),
            decisions: canonical_vec(decisions),
            completeness: canonical_vec(completeness),
        })
    }

    pub fn proposition(&self) -> Proposition {
        self.proposition
    }

    pub fn proof_ids(&self) -> &[ProofId] {
        &self.proof_ids
    }

    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    pub fn decisions(&self) -> &[ScopedDecision] {
        &self.decisions
    }

    pub fn completeness(&self) -> &[ScopedCompleteness] {
        &self.completeness
    }
}

/// Why a proposition is not established.
///
/// `NoSupport` is not a negative proof.  It is used for open-world unknowns
/// and therefore cannot be used as evidence that a proposition is false.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WhyNotReason {
    ExplicitRefutation,
    NoSupport,
    Blocked,
    ResourceIncomplete,
}

/// The negative or non-positive side of a proposition's explanation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WhyNot {
    proposition: Proposition,
    reason: WhyNotReason,
    proof_ids: Vec<ProofId>,
    dependencies: Vec<Dependency>,
    decisions: Vec<ScopedDecision>,
    completeness: Vec<ScopedCompleteness>,
}

impl WhyNot {
    pub fn new(
        proposition: Proposition,
        reason: WhyNotReason,
        proof_ids: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        Self::with_context(proposition, reason, proof_ids, [], [], [])
    }

    pub fn with_context(
        proposition: Proposition,
        reason: WhyNotReason,
        proof_ids: impl IntoIterator<Item = ProofId>,
        dependencies: impl IntoIterator<Item = Dependency>,
        decisions: impl IntoIterator<Item = ScopedDecision>,
        completeness: impl IntoIterator<Item = ScopedCompleteness>,
    ) -> Result<Self, ExplainError> {
        let proof_ids = canonical_proofs(proof_ids)?;
        if reason == WhyNotReason::ExplicitRefutation && proof_ids.is_empty() {
            return Err(ExplainError::InvalidWhyNot(
                "an explicit refutation requires a negative proof",
            ));
        }
        if reason != WhyNotReason::ExplicitRefutation && !proof_ids.is_empty() {
            return Err(ExplainError::InvalidWhyNot(
                "only an explicit refutation may carry negative proofs",
            ));
        }
        Ok(Self {
            proposition,
            reason,
            proof_ids,
            dependencies: canonical_dependencies(dependencies),
            decisions: canonical_vec(decisions),
            completeness: canonical_vec(completeness),
        })
    }

    pub fn proposition(&self) -> Proposition {
        self.proposition
    }

    pub fn reason(&self) -> WhyNotReason {
        self.reason
    }

    pub fn proof_ids(&self) -> &[ProofId] {
        &self.proof_ids
    }

    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    pub fn decisions(&self) -> &[ScopedDecision] {
        &self.decisions
    }

    pub fn completeness(&self) -> &[ScopedCompleteness] {
        &self.completeness
    }
}

/// A residual requirement, with the proof roots whose completion depends on
/// it.  The requirement itself remains the canonical semantic enum; this
/// wrapper adds no alternate blocker identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Missing {
    requirement: Requirement,
    required_by: Vec<ProofId>,
    decision: Option<ScopedDecision>,
    completeness: Option<ScopedCompleteness>,
}

impl Missing {
    pub fn new(requirement: Requirement) -> Result<Self, ExplainError> {
        Self::with_required_by(requirement, [])
    }

    pub fn with_required_by(
        requirement: Requirement,
        required_by: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        Ok(Self {
            requirement,
            required_by: canonical_proofs(required_by)?,
            decision: None,
            completeness: None,
        })
    }

    pub fn decision(
        id: impl Into<DecisionId>,
        scope: DecisionScope,
        required_by: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        let id = id.into();
        let scoped = ScopedDecision::new(id.clone(), scope)?;
        let mut value = Self::with_required_by(Requirement::Decision(id), required_by)?;
        value.decision = Some(scoped);
        Ok(value)
    }

    pub fn completeness(
        id: CompletenessId,
        scope: CompletenessScope,
        required_by: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        let scoped = ScopedCompleteness::new(id, scope);
        let mut value = Self::with_required_by(Requirement::Completeness(id), required_by)?;
        value.completeness = Some(scoped);
        Ok(value)
    }

    pub fn requirement(&self) -> &Requirement {
        &self.requirement
    }

    pub fn required_by(&self) -> &[ProofId] {
        &self.required_by
    }

    pub fn decision_scope(&self) -> Option<&ScopedDecision> {
        self.decision.as_ref()
    }

    pub fn completeness_scope(&self) -> Option<&ScopedCompleteness> {
        self.completeness.as_ref()
    }

    pub fn is_resource_boundary(&self) -> bool {
        matches!(self.requirement, Requirement::ResourceBoundary)
    }
}

/// A contradiction retained as data, not collapsed into an arbitrary answer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConflictDetail {
    subject: GoalId,
    positive_proofs: Vec<ProofId>,
    negative_proofs: Vec<ProofId>,
}

impl ConflictDetail {
    pub fn new(
        subject: GoalId,
        positive_proofs: impl IntoIterator<Item = ProofId>,
        negative_proofs: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        let positive_proofs = canonical_proofs(positive_proofs)?;
        let negative_proofs = canonical_proofs(negative_proofs)?;
        if positive_proofs.is_empty() || negative_proofs.is_empty() {
            return Err(ExplainError::InvalidConflict(
                "both positive and negative proofs are required",
            ));
        }
        Ok(Self {
            subject,
            positive_proofs,
            negative_proofs,
        })
    }

    pub fn from_semantic(conflict: &Conflict) -> Result<Self, ExplainError> {
        Self::new(
            conflict.subject(),
            conflict.positive_proofs().iter().copied(),
            conflict.negative_proofs().iter().copied(),
        )
    }

    pub fn subject(&self) -> GoalId {
        self.subject
    }

    pub fn positive_proofs(&self) -> &[ProofId] {
        &self.positive_proofs
    }

    pub fn negative_proofs(&self) -> &[ProofId] {
        &self.negative_proofs
    }
}

/// Canonically ordered contradiction details.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Conflicts {
    items: Vec<ConflictDetail>,
}

impl Conflicts {
    pub fn new(items: impl IntoIterator<Item = ConflictDetail>) -> Self {
        Self {
            items: canonical_vec(items),
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ConflictDetail> {
        self.items.iter()
    }

    pub fn as_slice(&self) -> &[ConflictDetail] {
        &self.items
    }
}

impl std::ops::Deref for Conflicts {
    type Target = [ConflictDetail];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a Conflicts {
    type Item = &'a ConflictDetail;
    type IntoIter = std::slice::Iter<'a, ConflictDetail>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// Exact dependency delta for one affected goal.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DependencyImpact {
    goal: GoalId,
    before: Vec<ProofId>,
    after: Vec<ProofId>,
    added: Vec<ProofId>,
    removed: Vec<ProofId>,
}

impl DependencyImpact {
    pub fn new(
        goal: GoalId,
        before: impl IntoIterator<Item = ProofId>,
        after: impl IntoIterator<Item = ProofId>,
    ) -> Result<Self, ExplainError> {
        let before = canonical_proofs(before)?;
        let after = canonical_proofs(after)?;
        let before_set = before.iter().copied().collect::<BTreeSet<_>>();
        let after_set = after.iter().copied().collect::<BTreeSet<_>>();
        let added = after_set.difference(&before_set).copied().collect();
        let removed = before_set.difference(&after_set).copied().collect();
        Ok(Self {
            goal,
            before,
            after,
            added,
            removed,
        })
    }

    pub fn goal(&self) -> GoalId {
        self.goal
    }

    pub fn before(&self) -> &[ProofId] {
        &self.before
    }

    pub fn after(&self) -> &[ProofId] {
        &self.after
    }

    pub fn added(&self) -> &[ProofId] {
        &self.added
    }

    pub fn removed(&self) -> &[ProofId] {
        &self.removed
    }

    pub fn is_changed(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

/// A proposition-specific impact report.  It is a description only: creating
/// one never changes a decision, evidence set, or accepted world.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WhatChanges {
    proposition: Proposition,
    impacts: Vec<DependencyImpact>,
}

impl WhatChanges {
    pub fn new(
        proposition: Proposition,
        impacts: impl IntoIterator<Item = DependencyImpact>,
    ) -> Self {
        Self {
            proposition,
            impacts: canonical_vec(impacts),
        }
    }

    pub fn proposition(&self) -> Proposition {
        self.proposition
    }

    pub fn impacts(&self) -> &[DependencyImpact] {
        &self.impacts
    }

    pub fn is_empty(&self) -> bool {
        self.impacts.is_empty()
    }
}

/// A repair is an inert suggestion.  The underlying [`Repair`] enum is kept
/// intact so applying it remains the caller's explicit responsibility.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepairSuggestion {
    action: Repair,
    scope: Option<DecisionScope>,
    completeness: Option<CompletenessScope>,
    affects: Vec<GoalId>,
}

impl RepairSuggestion {
    pub fn new(action: Repair) -> Self {
        Self {
            action,
            scope: None,
            completeness: None,
            affects: Vec::new(),
        }
    }

    pub fn scoped_decision(action: Repair, scope: DecisionScope) -> Result<Self, ExplainError> {
        if !matches!(action, Repair::MakeDecision(_)) {
            return Err(ExplainError::InvalidRepair(
                "a decision scope requires a MakeDecision repair",
            ));
        }
        Ok(Self {
            action,
            scope: Some(scope),
            completeness: None,
            affects: Vec::new(),
        })
    }

    pub fn scoped_completeness(
        action: Repair,
        scope: CompletenessScope,
    ) -> Result<Self, ExplainError> {
        if !matches!(action, Repair::DeclareCompleteness(_)) {
            return Err(ExplainError::InvalidRepair(
                "a completeness scope requires a DeclareCompleteness repair",
            ));
        }
        Ok(Self {
            action,
            scope: None,
            completeness: Some(scope),
            affects: Vec::new(),
        })
    }

    pub fn affecting(mut self, goals: impl IntoIterator<Item = GoalId>) -> Self {
        self.affects = canonical_vec(goals);
        self
    }

    pub fn action(&self) -> &Repair {
        &self.action
    }

    pub fn decision_scope(&self) -> Option<&DecisionScope> {
        self.scope.as_ref()
    }

    pub fn completeness_scope(&self) -> Option<&CompletenessScope> {
        self.completeness.as_ref()
    }

    pub fn affects(&self) -> &[GoalId] {
        &self.affects
    }
}

/// Canonically ordered, inert repairs.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepairPlan {
    suggestions: Vec<RepairSuggestion>,
}

impl RepairPlan {
    pub fn new(items: impl IntoIterator<Item = RepairSuggestion>) -> Self {
        Self {
            suggestions: canonical_vec(items),
        }
    }

    pub fn from_repairs(items: impl IntoIterator<Item = Repair>) -> Self {
        Self::new(items.into_iter().map(RepairSuggestion::new))
    }

    pub fn len(&self) -> usize {
        self.suggestions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &RepairSuggestion> {
        self.suggestions.iter()
    }

    pub fn as_slice(&self) -> &[RepairSuggestion] {
        &self.suggestions
    }
}

impl std::ops::Deref for RepairPlan {
    type Target = [RepairSuggestion];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a> IntoIterator for &'a RepairPlan {
    type Item = &'a RepairSuggestion;
    type IntoIter = std::slice::Iter<'a, RepairSuggestion>;

    fn into_iter(self) -> Self::IntoIter {
        self.suggestions.iter()
    }
}

/// A complete structured explanation for one proposition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Explanation {
    proposition: Proposition,
    truth: Truth,
    /// The answer shape is the canonical semantic multiplicity with values
    /// erased.  It is a projection, not a second answer-state enum.
    multiplicity: Multiplicity<()>,
    completion: Completion,
    conditional_answers: bool,
    why: Option<Why>,
    why_not: Option<WhyNot>,
    missing: Vec<Missing>,
    conflicts: Conflicts,
    what_changes: Vec<WhatChanges>,
    repair_plan: RepairPlan,
}

impl Explanation {
    pub fn for_resolution<T>(
        goal: impl Into<Proposition>,
        resolution: &Resolution<T>,
    ) -> Result<Self, ExplainError> {
        Self::from_resolution(goal, resolution)
    }

    /// Build the standard structured view of a semantic resolution.
    ///
    /// This constructor preserves the resolution axes exactly.  In
    /// particular, `ResourceLimited` remains incomplete even when there is no
    /// negative proof; absence of such a proof is never promoted to a
    /// refutation.
    pub fn from_resolution<T>(
        goal: impl Into<Proposition>,
        resolution: &Resolution<T>,
    ) -> Result<Self, ExplainError> {
        let proposition = goal.into();
        let positive = canonical_proofs(resolution.positive_proofs().iter().copied())?;
        let negative = canonical_proofs(resolution.negative_proofs().iter().copied())?;
        let conflicts = resolution
            .conflicts()
            .iter()
            .map(ConflictDetail::from_semantic)
            .collect::<Result<Vec<_>, _>>()?;
        let missing = resolution
            .blockers()
            .iter()
            .cloned()
            .map(Missing::new)
            .collect::<Result<Vec<_>, _>>()?;
        let repairs = RepairPlan::from_repairs(resolution.repairs().iter().cloned());
        let (multiplicity, conditional_answers) = answer_shape(resolution.answers());
        let why = (!positive.is_empty())
            .then(|| Why::with_context(proposition, positive.clone(), [], [], []))
            .transpose()?;
        let why_not = if !negative.is_empty() {
            Some(WhyNot::new(
                proposition,
                WhyNotReason::ExplicitRefutation,
                negative,
            )?)
        } else if positive.is_empty() {
            let reason = if resolution.completion() == Completion::ResourceLimited {
                WhyNotReason::ResourceIncomplete
            } else if !missing.is_empty() {
                WhyNotReason::Blocked
            } else {
                WhyNotReason::NoSupport
            };
            Some(WhyNot::new(proposition, reason, [])?)
        } else {
            None
        };
        Self::new(
            proposition,
            resolution.truth(),
            multiplicity,
            resolution.completion(),
            conditional_answers,
            why,
            why_not,
            missing,
            Conflicts::new(conflicts),
            Vec::new(),
            repairs,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proposition: Proposition,
        truth: Truth,
        multiplicity: Multiplicity<()>,
        completion: Completion,
        conditional_answers: bool,
        why: Option<Why>,
        why_not: Option<WhyNot>,
        missing: impl IntoIterator<Item = Missing>,
        conflicts: Conflicts,
        what_changes: impl IntoIterator<Item = WhatChanges>,
        repair_plan: RepairPlan,
    ) -> Result<Self, ExplainError> {
        let missing = canonical_vec(missing);
        let what_changes = canonical_vec(what_changes);
        let positive = why
            .as_ref()
            .map(|value| value.proof_ids().to_vec())
            .unwrap_or_default();
        let negative = why_not
            .as_ref()
            .map(|value| value.proof_ids().to_vec())
            .unwrap_or_default();
        let expected_truth = Truth::from_support(!positive.is_empty(), !negative.is_empty());
        if expected_truth != truth {
            return Err(ExplainError::TruthMismatch {
                expected: expected_truth,
                actual: truth,
            });
        }
        if !conflicts.is_empty() && truth != Truth::Both {
            return Err(ExplainError::ConflictWithoutBoth);
        }
        if truth == Truth::Both && conflicts.is_empty() {
            return Err(ExplainError::BothWithoutConflict);
        }
        for conflict in &conflicts {
            if conflict.subject() != proposition.goal() {
                return Err(ExplainError::PropositionMismatch);
            }
            if conflict
                .positive_proofs()
                .iter()
                .any(|id| !positive.contains(id))
                || conflict
                    .negative_proofs()
                    .iter()
                    .any(|id| !negative.contains(id))
            {
                return Err(ExplainError::ConflictProofNotInSupport);
            }
        }
        if completion == Completion::ResourceLimited
            && !missing.iter().any(Missing::is_resource_boundary)
        {
            return Err(ExplainError::ResourceWithoutBlocker);
        }
        if let Some(value) = &why
            && value.proposition() != proposition
        {
            return Err(ExplainError::PropositionMismatch);
        }
        if let Some(value) = &why_not
            && value.proposition() != proposition
        {
            return Err(ExplainError::PropositionMismatch);
        }
        if what_changes
            .iter()
            .any(|value| value.proposition() != proposition)
        {
            return Err(ExplainError::PropositionMismatch);
        }
        Ok(Self {
            proposition,
            truth,
            multiplicity,
            completion,
            conditional_answers,
            why,
            why_not,
            missing,
            conflicts,
            what_changes,
            repair_plan,
        })
    }

    pub fn proposition(&self) -> Proposition {
        self.proposition
    }

    pub fn truth(&self) -> Truth {
        self.truth
    }

    pub fn multiplicity(&self) -> &Multiplicity<()> {
        &self.multiplicity
    }

    pub fn completion(&self) -> Completion {
        self.completion
    }

    pub fn has_conditional_answers(&self) -> bool {
        self.conditional_answers
    }

    pub fn is_ambiguous(&self) -> bool {
        matches!(self.multiplicity, Multiplicity::Multiple(_))
    }

    pub fn why(&self) -> Option<&Why> {
        self.why.as_ref()
    }

    pub fn why_not(&self) -> Option<&WhyNot> {
        self.why_not.as_ref()
    }

    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }

    pub fn conflicts(&self) -> &Conflicts {
        &self.conflicts
    }

    pub fn what_changes(&self) -> &[WhatChanges] {
        &self.what_changes
    }

    pub fn repair_plan(&self) -> &RepairPlan {
        &self.repair_plan
    }

    pub fn is_refuted(&self) -> bool {
        self.completion == Completion::Complete
            && self.truth == Truth::FalseOnly
            && self.missing.is_empty()
            && !self.is_ambiguous()
            && !self.conditional_answers
    }

    pub fn is_incomplete(&self) -> bool {
        self.completion == Completion::ResourceLimited
    }

    /// Add a what-changes report while returning a new immutable explanation.
    /// This is useful for an impact query and intentionally cannot mutate the
    /// source resolution or accepted world.
    pub fn with_what_changes(
        &self,
        changes: impl IntoIterator<Item = WhatChanges>,
    ) -> Result<Self, ExplainError> {
        let mut all = self.what_changes.clone();
        all.extend(changes);
        Self::new(
            self.proposition,
            self.truth,
            self.multiplicity.clone(),
            self.completion,
            self.conditional_answers,
            self.why.clone(),
            self.why_not.clone(),
            self.missing.clone(),
            self.conflicts.clone(),
            all,
            self.repair_plan.clone(),
        )
    }
}

/// A small query selector over an already computed explanation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExplanationQuery {
    Why(Proposition),
    WhyNot(Proposition),
    Missing(Proposition),
    Conflicts(Proposition),
    WhatChanges(Proposition),
    RepairPlan(Proposition),
}

impl ExplanationQuery {
    pub fn why(goal: GoalId) -> Self {
        Self::Why(Proposition::new(goal))
    }

    pub fn why_not(goal: GoalId) -> Self {
        Self::WhyNot(Proposition::new(goal))
    }

    pub fn missing(goal: GoalId) -> Self {
        Self::Missing(Proposition::new(goal))
    }

    pub fn conflicts(goal: GoalId) -> Self {
        Self::Conflicts(Proposition::new(goal))
    }

    pub fn what_changes(goal: GoalId) -> Self {
        Self::WhatChanges(Proposition::new(goal))
    }

    pub fn repair_plan(goal: GoalId) -> Self {
        Self::RepairPlan(Proposition::new(goal))
    }

    fn proposition(self) -> Proposition {
        match self {
            Self::Why(value)
            | Self::WhyNot(value)
            | Self::Missing(value)
            | Self::Conflicts(value)
            | Self::WhatChanges(value)
            | Self::RepairPlan(value) => value,
        }
    }
}

/// The typed answer to one [`ExplanationQuery`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExplanationAnswer<'a> {
    Why(Option<&'a Why>),
    WhyNot(Option<&'a WhyNot>),
    Missing(&'a [Missing]),
    Conflicts(&'a Conflicts),
    WhatChanges(&'a [WhatChanges]),
    RepairPlan(&'a RepairPlan),
}

impl Explanation {
    pub fn answer(&self, query: ExplanationQuery) -> Result<ExplanationAnswer<'_>, ExplainError> {
        if query.proposition() != self.proposition {
            return Err(ExplainError::PropositionMismatch);
        }
        Ok(match query {
            ExplanationQuery::Why(_) => ExplanationAnswer::Why(self.why()),
            ExplanationQuery::WhyNot(_) => ExplanationAnswer::WhyNot(self.why_not()),
            ExplanationQuery::Missing(_) => ExplanationAnswer::Missing(self.missing()),
            ExplanationQuery::Conflicts(_) => ExplanationAnswer::Conflicts(self.conflicts()),
            ExplanationQuery::WhatChanges(_) => ExplanationAnswer::WhatChanges(self.what_changes()),
            ExplanationQuery::RepairPlan(_) => ExplanationAnswer::RepairPlan(self.repair_plan()),
        })
    }
}

/// A convenient report builder for callers that need the rich context fields.
#[derive(Clone, Debug)]
pub struct ExplanationBuilder {
    proposition: Proposition,
    positive_proofs: Vec<ProofId>,
    negative_proofs: Vec<ProofId>,
    multiplicity: Multiplicity<()>,
    completion: Completion,
    conditional_answers: bool,
    missing: Vec<Missing>,
    conflicts: Vec<ConflictDetail>,
    changes: Vec<WhatChanges>,
    repairs: Vec<RepairSuggestion>,
    decisions: Vec<ScopedDecision>,
    completeness: Vec<ScopedCompleteness>,
}

impl ExplanationBuilder {
    pub fn new(proposition: impl Into<Proposition>) -> Self {
        Self {
            proposition: proposition.into(),
            positive_proofs: Vec::new(),
            negative_proofs: Vec::new(),
            multiplicity: Multiplicity::none(),
            completion: Completion::Complete,
            conditional_answers: false,
            missing: Vec::new(),
            conflicts: Vec::new(),
            changes: Vec::new(),
            repairs: Vec::new(),
            decisions: Vec::new(),
            completeness: Vec::new(),
        }
    }

    pub fn positive_proofs(mut self, proofs: impl IntoIterator<Item = ProofId>) -> Self {
        self.positive_proofs.extend(proofs);
        self
    }

    pub fn negative_proofs(mut self, proofs: impl IntoIterator<Item = ProofId>) -> Self {
        self.negative_proofs.extend(proofs);
        self
    }

    pub fn multiplicity(mut self, multiplicity: Multiplicity<()>) -> Self {
        self.multiplicity = multiplicity;
        self
    }

    pub fn completion(mut self, completion: Completion) -> Self {
        self.completion = completion;
        self
    }

    pub fn conditional_answers(mut self, conditional: bool) -> Self {
        self.conditional_answers = conditional;
        self
    }

    pub fn missing(mut self, missing: impl IntoIterator<Item = Missing>) -> Self {
        self.missing.extend(missing);
        self
    }

    pub fn conflict(mut self, conflict: ConflictDetail) -> Self {
        self.conflicts.push(conflict);
        self
    }

    pub fn what_changes(mut self, changes: impl IntoIterator<Item = WhatChanges>) -> Self {
        self.changes.extend(changes);
        self
    }

    pub fn repair(mut self, repair: RepairSuggestion) -> Self {
        self.repairs.push(repair);
        self
    }

    pub fn decision(mut self, decision: ScopedDecision) -> Self {
        self.decisions.push(decision);
        self
    }

    pub fn completeness(mut self, completeness: ScopedCompleteness) -> Self {
        self.completeness.push(completeness);
        self
    }

    pub fn finish(self) -> Result<Explanation, ExplainError> {
        let positive = canonical_proofs(self.positive_proofs)?;
        let negative = canonical_proofs(self.negative_proofs)?;
        let truth = Truth::from_support(!positive.is_empty(), !negative.is_empty());
        let why = (!positive.is_empty())
            .then(|| {
                Why::with_context(
                    self.proposition,
                    positive,
                    [],
                    self.decisions.clone(),
                    self.completeness.clone(),
                )
            })
            .transpose()?;
        let why_not = if !negative.is_empty() {
            Some(WhyNot::with_context(
                self.proposition,
                WhyNotReason::ExplicitRefutation,
                negative,
                [],
                self.decisions,
                self.completeness,
            )?)
        } else if why.is_none() {
            let reason = if self.completion == Completion::ResourceLimited {
                WhyNotReason::ResourceIncomplete
            } else if !self.missing.is_empty() {
                WhyNotReason::Blocked
            } else {
                WhyNotReason::NoSupport
            };
            Some(WhyNot::with_context(
                self.proposition,
                reason,
                [],
                [],
                self.decisions,
                self.completeness,
            )?)
        } else {
            None
        };
        Explanation::new(
            self.proposition,
            truth,
            self.multiplicity,
            self.completion,
            self.conditional_answers,
            why,
            why_not,
            self.missing,
            Conflicts::new(self.conflicts),
            self.changes,
            RepairPlan::new(self.repairs),
        )
    }
}

/// Author-facing convenience function for the common query path.
pub fn explain<T>(
    goal: impl Into<Proposition>,
    resolution: &Resolution<T>,
) -> Result<Explanation, ExplainError> {
    Explanation::from_resolution(goal, resolution)
}

/// Errors raised when a structured explanation would violate one of its
/// invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExplainError {
    InvalidProof(ProofId),
    EmptyIdentifier(&'static str),
    InvalidWhyNot(&'static str),
    InvalidConflict(&'static str),
    InvalidRepair(&'static str),
    TruthMismatch { expected: Truth, actual: Truth },
    ConflictWithoutBoth,
    BothWithoutConflict,
    ConflictProofNotInSupport,
    ResourceWithoutBlocker,
    PropositionMismatch,
}

impl fmt::Display for ExplainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProof(id) => write!(formatter, "zero proof id is not allowed: {id}"),
            Self::EmptyIdentifier(field) => write!(formatter, "empty {field}"),
            Self::InvalidWhyNot(message) => formatter.write_str(message),
            Self::InvalidConflict(message) => formatter.write_str(message),
            Self::InvalidRepair(message) => formatter.write_str(message),
            Self::TruthMismatch { expected, actual } => {
                write!(
                    formatter,
                    "truth mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::ConflictWithoutBoth => {
                formatter.write_str("conflict details require both positive and negative support")
            }
            Self::BothWithoutConflict => {
                formatter.write_str("both-sided support requires conflict details")
            }
            Self::ConflictProofNotInSupport => {
                formatter.write_str("conflict proof is absent from proposition support")
            }
            Self::ResourceWithoutBlocker => {
                formatter.write_str("resource-limited completion requires a resource blocker")
            }
            Self::PropositionMismatch => {
                formatter.write_str("explanation detail belongs to another proposition")
            }
        }
    }
}

impl std::error::Error for ExplainError {}

fn canonical_proofs(
    proofs: impl IntoIterator<Item = ProofId>,
) -> Result<Vec<ProofId>, ExplainError> {
    let mut values = proofs.into_iter().collect::<Vec<_>>();
    if let Some(id) = values.iter().copied().find(|id| *id == ProofId::ZERO) {
        return Err(ExplainError::InvalidProof(id));
    }
    values.sort();
    values.dedup();
    Ok(values)
}

fn canonical_vec<T: Ord>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn canonical_dependencies(values: impl IntoIterator<Item = Dependency>) -> Vec<Dependency> {
    canonical_vec(values)
}

fn answer_shape<T>(
    answers: &Multiplicity<crate::semantics::Conditional<T>>,
) -> (Multiplicity<()>, bool) {
    let (multiplicity, values) = match answers {
        Multiplicity::None => (Multiplicity::none(), Vec::new()),
        Multiplicity::Unique(value) => (Multiplicity::unique(()), vec![value]),
        Multiplicity::Multiple(values) => (
            Multiplicity::multiple((0..values.len()).map(|_| ()).collect())
                .expect("a canonical multiple answer has at least two values"),
            values.iter().collect(),
        ),
    };
    let conditional = values.iter().any(|value| !value.obligations().is_empty());
    (multiplicity, conditional)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentHash;
    use crate::proof::{Node, Operation, Proof};
    use std::collections::BTreeMap;

    fn goal(seed: u8) -> GoalId {
        GoalId::new(ContentHash::from_bytes([seed; 32])).expect("goal")
    }

    fn proof(seed: u8) -> ProofId {
        proof_node(seed).id
    }

    fn proof_node(seed: u8) -> Node {
        Node::new(
            format!("proof/{seed}"),
            Operation::Observation {
                source: format!("source/{seed}"),
            },
            Vec::new(),
            BTreeMap::new(),
        )
    }

    fn proof_bundle(ids: impl IntoIterator<Item = ProofId>) -> Proof {
        let ids = ids.into_iter().collect::<Vec<_>>();
        let mut bundle = Proof::new();
        for id in ids {
            let node = (0..=u8::MAX)
                .map(proof_node)
                .find(|node| node.id == id)
                .expect("test proof id has a corresponding node");
            bundle.insert(node);
            bundle.root(id);
        }
        bundle
    }

    fn resolution<T>(
        positive: Vec<ProofId>,
        negative: Vec<ProofId>,
        completion: Completion,
        blockers: Vec<Requirement>,
        conflicts: Vec<Conflict>,
        repairs: Vec<Repair>,
        answers: Multiplicity<crate::semantics::Conditional<T>>,
    ) -> Resolution<T> {
        let proof_ids = positive
            .iter()
            .chain(&negative)
            .copied()
            .chain(conflicts.iter().flat_map(|conflict| {
                conflict
                    .positive_proofs()
                    .iter()
                    .chain(conflict.negative_proofs())
                    .copied()
            }))
            .collect::<Vec<_>>();
        let bundle = proof_bundle(proof_ids);
        Resolution::new_checked(
            &bundle, positive, negative, answers, completion, blockers, conflicts, repairs,
        )
        .expect("valid resolution")
    }

    #[test]
    fn resolution_maps_to_proposition_specific_sections() {
        let subject = goal(1);
        let value = resolution(
            vec![proof(2)],
            vec![],
            Completion::Complete,
            vec![Requirement::Decision(DecisionId::from("choose/lot"))],
            vec![],
            vec![Repair::MakeDecision(DecisionId::from("choose/lot"))],
            Multiplicity::unique(crate::semantics::Conditional::new(
                "lot/one",
                vec![Requirement::Decision(DecisionId::from("choose/lot"))],
            )),
        );
        let explanation = Explanation::from_resolution(subject, &value).expect("explanation");
        assert_eq!(explanation.proposition().goal(), subject);
        assert_eq!(explanation.truth(), Truth::TrueOnly);
        assert!(!explanation.is_refuted());
        assert_eq!(explanation.missing().len(), 1);
        assert_eq!(explanation.why().expect("why").proof_ids(), &[proof(2)]);
        assert_eq!(explanation.missing().len(), 1);
        assert_eq!(explanation.repair_plan().len(), 1);
        assert!(matches!(
            explanation.answer(ExplanationQuery::why(subject)),
            Ok(ExplanationAnswer::Why(Some(_)))
        ));
    }

    #[test]
    fn unknown_is_not_refuted_and_resource_limits_are_incomplete() {
        let subject = goal(3);
        let open = resolution::<&str>(
            vec![],
            vec![],
            Completion::OpenWorld,
            vec![],
            vec![],
            vec![],
            Multiplicity::none(),
        );
        let unknown = Explanation::from_resolution(subject, &open).expect("unknown explanation");
        assert_eq!(unknown.truth(), Truth::Neither);
        assert!(!unknown.is_refuted());
        assert_eq!(
            unknown.why_not().expect("why-not").reason(),
            WhyNotReason::NoSupport
        );

        let limited = resolution::<&str>(
            vec![],
            vec![],
            Completion::ResourceLimited,
            vec![Requirement::ResourceBoundary],
            vec![],
            vec![],
            Multiplicity::none(),
        );
        let incomplete =
            Explanation::from_resolution(subject, &limited).expect("limited explanation");
        assert_eq!(incomplete.completion(), Completion::ResourceLimited);
        assert!(!incomplete.is_refuted());
        assert_eq!(
            incomplete.why_not().expect("why-not").reason(),
            WhyNotReason::ResourceIncomplete
        );
    }

    #[test]
    fn conflict_retains_both_sides_and_rejects_unrelated_proofs() {
        let subject = goal(4);
        let conflict = Conflict::new(subject, vec![proof(5)], vec![proof(6)]).expect("conflict");
        let value = resolution::<&str>(
            vec![proof(5)],
            vec![proof(6)],
            Completion::Complete,
            vec![],
            vec![conflict],
            vec![],
            Multiplicity::none(),
        );
        let explanation = Explanation::from_resolution(subject, &value).expect("explanation");
        assert_eq!(explanation.truth(), Truth::Both);
        assert_eq!(explanation.conflicts().len(), 1);
        assert_eq!(explanation.why().expect("why").proof_ids(), &[proof(5)]);
        assert_eq!(
            explanation.why_not().expect("why-not").proof_ids(),
            &[proof(6)]
        );

        let unrelated = ConflictDetail::new(subject, vec![proof(5)], vec![proof(7)]);
        let invalid = ExplanationBuilder::new(subject)
            .positive_proofs([proof(5)])
            .negative_proofs([proof(6)])
            .conflict(unrelated.expect("detail"))
            .finish();
        assert_eq!(invalid, Err(ExplainError::ConflictProofNotInSupport));
    }

    #[test]
    fn canonical_order_and_exact_dependency_delta_are_stable() {
        let subject = goal(8);
        let impact = DependencyImpact::new(subject, [proof(3), proof(1)], [proof(4), proof(1)])
            .expect("impact");
        assert_eq!(impact.before(), &[proof(1), proof(3)]);
        assert_eq!(impact.after(), &[proof(1), proof(4)]);
        assert_eq!(impact.added(), &[proof(4)]);
        assert_eq!(impact.removed(), &[proof(3)]);

        let changes = WhatChanges::new(Proposition::new(subject), [impact]);
        let built = ExplanationBuilder::new(subject)
            .positive_proofs([proof(1), proof(2)])
            .what_changes([changes])
            .finish()
            .expect("explanation");
        assert_eq!(built.what_changes()[0].impacts()[0].added(), &[proof(4)]);
        assert_eq!(built.why().expect("why").proof_ids(), &[proof(1), proof(2)]);
    }

    #[test]
    fn repairs_are_inert_and_scoped_context_is_retained() {
        let scope = DecisionScope::occurrence("sale/one").expect("scope");
        let action = RepairSuggestion::scoped_decision(
            Repair::MakeDecision(DecisionId::from("decision/one")),
            scope.clone(),
        )
        .expect("repair");
        assert_eq!(action.decision_scope(), Some(&scope));

        let subject = goal(9);
        let built = ExplanationBuilder::new(subject)
            .decision(ScopedDecision::new("decision/one", scope).expect("decision"))
            .repair(action)
            .finish()
            .expect("explanation");
        assert_eq!(
            built.why_not().expect("why-not").reason(),
            WhyNotReason::NoSupport
        );
        assert_eq!(built.repair_plan().len(), 1);
    }
}
