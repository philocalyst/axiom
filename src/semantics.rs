//! The small semantic kernel shared by observations, policies, and reports.
//!
//! This module deliberately does not know anything about the journal surface.
//! A statement has independent axes for polarity, world, force, phase, time,
//! provenance, and authority.  Keeping those axes separate is what makes it
//! possible to retain contradictory evidence without turning a contradiction
//! into arbitrary conclusions.

use std::fmt;

use crate::model::{
    AccountId, BookId, ContentHash, Date, DecisionId, EntityId, ExternalId, OccurrenceId, PolicyId,
    SourceId,
};
use crate::proof::{Proof, ProofId};

/// Errors returned by semantic value constructors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticError {
    EmptyField(&'static str),
    ZeroHash(&'static str),
    InvalidTemporalScope,
    DuplicateAnswer,
    SelectedAnswerRejected,
    InvalidProof,
    InvalidProofContext(String),
    RevocationOutOfOrder,
    SelfSupersedingDecision,
    InvalidProvenance,
    PhaseProvenanceMismatch,
    InvalidPhaseTransition,
    InvalidResolution(&'static str),
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(f, "empty semantic {field}"),
            Self::ZeroHash(field) => write!(f, "zero content hash is not a {field}"),
            Self::InvalidTemporalScope => f.write_str("temporal scope starts after it ends"),
            Self::DuplicateAnswer => f.write_str("decision contains a duplicate rejected answer"),
            Self::SelectedAnswerRejected => f.write_str("selected answer is also rejected"),
            Self::InvalidProof => f.write_str("zero proof id is not a proof"),
            Self::InvalidProofContext(reason) => write!(f, "invalid proof context: {reason}"),
            Self::RevocationOutOfOrder => f.write_str("revocations must be chronological"),
            Self::SelfSupersedingDecision => f.write_str("a decision cannot supersede itself"),
            Self::InvalidProvenance => f.write_str("invalid provenance record"),
            Self::PhaseProvenanceMismatch => {
                f.write_str("statement phase and provenance do not agree")
            }
            Self::InvalidPhaseTransition => f.write_str("invalid semantic phase transition"),
            Self::InvalidResolution(reason) => write!(f, "invalid resolution: {reason}"),
        }
    }
}

impl std::error::Error for SemanticError {}

fn require_id(value: &str, field: &'static str) -> Result<(), SemanticError> {
    if value.trim().is_empty() {
        Err(SemanticError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn require_hash(value: ContentHash, field: &'static str) -> Result<(), SemanticError> {
    if value == ContentHash::ZERO {
        Err(SemanticError::ZeroHash(field))
    } else {
        Ok(())
    }
}

fn require_proof(value: ProofId) -> Result<(), SemanticError> {
    if value == ProofId::ZERO {
        Err(SemanticError::InvalidProof)
    } else {
        Ok(())
    }
}

fn canonical_proofs(mut values: Vec<ProofId>) -> Vec<ProofId> {
    values.sort();
    values.dedup();
    values
}

/// Whether a proposition is asserted or explicitly denied.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Polarity {
    Positive,
    Negative,
}

/// The world to which a statement applies.  Scenarios are deliberately
/// named, so actual and hypothetical facts cannot be combined accidentally.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum World {
    Actual,
    Scenario(ExternalId),
}

impl World {
    pub fn actual() -> Self {
        Self::Actual
    }

    pub fn scenario(id: impl Into<String>) -> Result<Self, SemanticError> {
        let id =
            ExternalId::try_new(id.into()).map_err(|_| SemanticError::EmptyField("scenario id"))?;
        Ok(Self::Scenario(id))
    }

    pub fn is_actual(&self) -> bool {
        matches!(self, Self::Actual)
    }

    pub fn scenario_id(&self) -> Option<&ExternalId> {
        match self {
            Self::Actual => None,
            Self::Scenario(id) => Some(id),
        }
    }
}

/// Whether a statement describes or prescribes an economic fact.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Force {
    Descriptive,
    Required,
    Permitted,
    Prohibited,
    Preferred,
}

/// How far a statement has moved through the evidence pipeline.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    Observed,
    Resolved,
    Accepted,
    Recognized(BookId),
}

impl Phase {
    pub fn recognized(book: impl Into<BookId>) -> Result<Self, SemanticError> {
        let book = book.into();
        require_id(book.as_str(), "book id")?;
        Ok(Self::Recognized(book))
    }
}

/// A closed or open-ended inclusive civil-date interval.
///
/// `None` on either side means unbounded in that direction.  `at` is useful
/// for occurrence-time facts and is represented by equal bounds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TemporalScope {
    start: Option<Date>,
    end: Option<Date>,
}

impl TemporalScope {
    pub fn all_time() -> Self {
        Self {
            start: None,
            end: None,
        }
    }

    pub fn at(date: Date) -> Self {
        Self {
            start: Some(date),
            end: Some(date),
        }
    }

    pub fn from(start: Date) -> Self {
        Self {
            start: Some(start),
            end: None,
        }
    }

    pub fn through(end: Date) -> Self {
        Self {
            start: None,
            end: Some(end),
        }
    }

    pub fn between(start: Date, end: Date) -> Result<Self, SemanticError> {
        if start > end {
            return Err(SemanticError::InvalidTemporalScope);
        }
        Ok(Self {
            start: Some(start),
            end: Some(end),
        })
    }

    pub fn start(&self) -> Option<Date> {
        self.start
    }

    pub fn end(&self) -> Option<Date> {
        self.end
    }

    pub fn contains(&self, date: Date) -> bool {
        self.start.is_none_or(|start| start <= date) && self.end.is_none_or(|end| date <= end)
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.start
            .is_none_or(|start| other.end.is_none_or(|end| start <= end))
            && other
                .start
                .is_none_or(|start| self.end.is_none_or(|end| start <= end))
    }
}

/// A compact reference to immutable source material.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EvidenceRef {
    source: SourceId,
    content: ContentHash,
}

impl EvidenceRef {
    pub fn new(source: impl Into<SourceId>, content: ContentHash) -> Result<Self, SemanticError> {
        let source = source.into();
        require_id(source.as_str(), "source id")?;
        require_hash(content, "evidence content")?;
        Ok(Self { source, content })
    }

    pub fn source(&self) -> &SourceId {
        &self.source
    }

    pub fn content(&self) -> ContentHash {
        self.content
    }
}

/// A proof-bearing provenance record.  The categories are intentionally
/// explicit: a policy result and a signed decision are not interchangeable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Provenance {
    SourceObservation(EvidenceRef),
    UserAssertion {
        author: EntityId,
        content: ContentHash,
    },
    PolicyDerivation {
        policy: PolicyId,
        proof: ProofId,
    },
    InstitutionalRecord(EvidenceRef),
    SignedDecision {
        decision: DecisionId,
        proof: ProofId,
    },
}

impl Provenance {
    pub fn source_observation(
        source: impl Into<SourceId>,
        content: ContentHash,
    ) -> Result<Self, SemanticError> {
        Ok(Self::SourceObservation(EvidenceRef::new(source, content)?))
    }

    pub fn user_assertion(
        author: impl Into<EntityId>,
        content: ContentHash,
    ) -> Result<Self, SemanticError> {
        let author = author.into();
        require_id(author.as_str(), "author id")?;
        require_hash(content, "assertion content")?;
        Ok(Self::UserAssertion { author, content })
    }

    pub fn policy_derivation(
        policy: impl Into<PolicyId>,
        proof: ProofId,
    ) -> Result<Self, SemanticError> {
        let policy = policy.into();
        require_id(policy.as_str(), "policy id")?;
        require_proof(proof)?;
        Ok(Self::PolicyDerivation { policy, proof })
    }

    pub fn institutional_record(
        source: impl Into<SourceId>,
        content: ContentHash,
    ) -> Result<Self, SemanticError> {
        Ok(Self::InstitutionalRecord(EvidenceRef::new(
            source, content,
        )?))
    }

    pub fn signed_decision(
        decision: impl Into<DecisionId>,
        proof: ProofId,
    ) -> Result<Self, SemanticError> {
        let decision = decision.into();
        require_id(decision.as_str(), "decision id")?;
        require_proof(proof)?;
        Ok(Self::SignedDecision { decision, proof })
    }

    fn validate(&self) -> Result<(), SemanticError> {
        let result = match self {
            Self::SourceObservation(_) | Self::InstitutionalRecord(_) => Ok(()),
            Self::UserAssertion { author, content } => require_id(author.as_str(), "author id")
                .and_then(|()| require_hash(*content, "assertion content")),
            Self::PolicyDerivation { policy, proof } => {
                require_id(policy.as_str(), "policy id").and_then(|()| require_proof(*proof))
            }
            Self::SignedDecision { decision, proof } => {
                require_id(decision.as_str(), "decision id").and_then(|()| require_proof(*proof))
            }
        };
        result.map_err(|_| SemanticError::InvalidProvenance)
    }

    fn matches_phase(&self, phase: &Phase) -> bool {
        match phase {
            Phase::Observed => matches!(
                self,
                Self::SourceObservation(_)
                    | Self::UserAssertion { .. }
                    | Self::InstitutionalRecord(_)
            ),
            Phase::Resolved | Phase::Recognized(_) => {
                matches!(self, Self::PolicyDerivation { .. })
            }
            Phase::Accepted => matches!(self, Self::SignedDecision { .. }),
        }
    }
}

/// The entitlement behind a statement.  This is separate from provenance:
/// the same source may be quoted by a user, a policy, or an institution.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Authority {
    Source(SourceId),
    User(EntityId),
    Policy(PolicyId),
    Institution(SourceId),
    SignedDecision {
        signer: EntityId,
        decision: DecisionId,
    },
}

impl Authority {
    pub fn source(source: impl Into<SourceId>) -> Result<Self, SemanticError> {
        let source = source.into();
        require_id(source.as_str(), "source authority")?;
        Ok(Self::Source(source))
    }

    pub fn user(entity: impl Into<EntityId>) -> Result<Self, SemanticError> {
        let entity = entity.into();
        require_id(entity.as_str(), "user authority")?;
        Ok(Self::User(entity))
    }

    pub fn policy(policy: impl Into<PolicyId>) -> Result<Self, SemanticError> {
        let policy = policy.into();
        require_id(policy.as_str(), "policy authority")?;
        Ok(Self::Policy(policy))
    }

    pub fn institution(source: impl Into<SourceId>) -> Result<Self, SemanticError> {
        let source = source.into();
        require_id(source.as_str(), "institution authority")?;
        Ok(Self::Institution(source))
    }

    pub fn signed_decision(
        signer: impl Into<EntityId>,
        decision: impl Into<DecisionId>,
    ) -> Result<Self, SemanticError> {
        let signer = signer.into();
        let decision = decision.into();
        require_id(signer.as_str(), "decision signer")?;
        require_id(decision.as_str(), "decision id")?;
        Ok(Self::SignedDecision { signer, decision })
    }
}

/// A proposition carrying independent semantic axes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Statement<P> {
    proposition: P,
    polarity: Polarity,
    world: World,
    force: Force,
    phase: Phase,
    temporal: TemporalScope,
    provenance: Provenance,
    authority: Authority,
}

impl<P> Statement<P> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proposition: P,
        polarity: Polarity,
        world: World,
        force: Force,
        phase: Phase,
        temporal: TemporalScope,
        provenance: Provenance,
        authority: Authority,
    ) -> Result<Self, SemanticError> {
        if let Phase::Recognized(book) = &phase {
            require_id(book.as_str(), "book id")?;
        }
        provenance.validate()?;
        if !provenance.matches_phase(&phase) {
            return Err(SemanticError::PhaseProvenanceMismatch);
        }
        Ok(Self {
            proposition,
            polarity,
            world,
            force,
            phase,
            temporal,
            provenance,
            authority,
        })
    }

    pub fn proposition(&self) -> &P {
        &self.proposition
    }

    pub fn polarity(&self) -> Polarity {
        self.polarity
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn force(&self) -> Force {
        self.force
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn temporal(&self) -> TemporalScope {
        self.temporal
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    pub fn map<U>(self, map: impl FnOnce(P) -> U) -> Statement<U> {
        Statement {
            proposition: map(self.proposition),
            polarity: self.polarity,
            world: self.world,
            force: self.force,
            phase: self.phase,
            temporal: self.temporal,
            provenance: self.provenance,
            authority: self.authority,
        }
    }

    /// Move an observed statement into a policy-derived resolved statement.
    /// The old statement remains available to the caller; this is a pure
    /// value transition, not mutation of evidence.
    pub fn resolve(
        self,
        provenance: Provenance,
        authority: Authority,
    ) -> Result<Self, SemanticError> {
        if !matches!(self.phase, Phase::Observed) {
            return Err(SemanticError::InvalidPhaseTransition);
        }
        Self::new(
            self.proposition,
            self.polarity,
            self.world,
            self.force,
            Phase::Resolved,
            self.temporal,
            provenance,
            authority,
        )
    }

    /// Accept a resolved candidate through an explicit signed decision.
    pub fn accept(
        self,
        provenance: Provenance,
        authority: Authority,
    ) -> Result<Self, SemanticError> {
        if !matches!(self.phase, Phase::Resolved) {
            return Err(SemanticError::InvalidPhaseTransition);
        }
        Self::new(
            self.proposition,
            self.polarity,
            self.world,
            self.force,
            Phase::Accepted,
            self.temporal,
            provenance,
            authority,
        )
    }

    /// Project an accepted fact into one book's recognition phase.
    pub fn recognize(
        self,
        book: impl Into<BookId>,
        provenance: Provenance,
        authority: Authority,
    ) -> Result<Self, SemanticError> {
        if !matches!(self.phase, Phase::Accepted) {
            return Err(SemanticError::InvalidPhaseTransition);
        }
        Self::new(
            self.proposition,
            self.polarity,
            self.world,
            self.force,
            Phase::recognized(book)?,
            self.temporal,
            provenance,
            authority,
        )
    }
}

/// The four paraconsistent support states for one proposition/world/scope.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Truth {
    Neither,
    TrueOnly,
    FalseOnly,
    Both,
}

impl Truth {
    pub fn from_support(positive: bool, negative: bool) -> Self {
        match (positive, negative) {
            (false, false) => Self::Neither,
            (true, false) => Self::TrueOnly,
            (false, true) => Self::FalseOnly,
            (true, true) => Self::Both,
        }
    }

    pub fn has_positive(self) -> bool {
        matches!(self, Self::TrueOnly | Self::Both)
    }

    pub fn has_negative(self) -> bool {
        matches!(self, Self::FalseOnly | Self::Both)
    }

    pub fn is_conflict(self) -> bool {
        matches!(self, Self::Both)
    }
}

/// A non-empty finite collection used for the `Multiple` case.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NonEmpty<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmpty<T> {
    pub fn one(first: T) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    pub fn from_vec(mut values: Vec<T>) -> Option<Self> {
        if values.is_empty() {
            None
        } else {
            let first = values.remove(0);
            Some(Self {
                first,
                rest: values,
            })
        }
    }

    pub fn len(&self) -> usize {
        1 + self.rest.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn first(&self) -> &T {
        &self.first
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    pub fn into_vec(self) -> Vec<T> {
        let mut values = Vec::with_capacity(self.len());
        values.push(self.first);
        values.extend(self.rest);
        values
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Multiplicity<T> {
    None,
    Unique(T),
    Multiple(NonEmpty<T>),
}

impl<T> Multiplicity<T> {
    pub fn none() -> Self {
        Self::None
    }

    pub fn unique(value: T) -> Self {
        Self::Unique(value)
    }

    pub fn multiple(mut values: Vec<T>) -> Option<Self> {
        match values.len() {
            0 => None,
            1 => Some(Self::Unique(values.remove(0))),
            _ => NonEmpty::from_vec(values).map(Self::Multiple),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Unique(_) => 1,
            Self::Multiple(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Whether the solver exhausted the relevant search space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Completion {
    Complete,
    OpenWorld,
    ResourceLimited,
}

/// A stable content-addressed goal identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GoalId(ContentHash);

impl GoalId {
    pub fn new(hash: ContentHash) -> Result<Self, SemanticError> {
        require_hash(hash, "goal id")?;
        Ok(Self(hash))
    }

    pub fn hash(self) -> ContentHash {
        self.0
    }
}

/// A stable identity for one answer to a resolution goal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AnswerId(ContentHash);

impl AnswerId {
    pub fn new(hash: ContentHash) -> Result<Self, SemanticError> {
        require_hash(hash, "answer id")?;
        Ok(Self(hash))
    }

    pub fn hash(self) -> ContentHash {
        self.0
    }
}

/// A stable identity for a completeness claim.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompletenessId(ContentHash);

impl CompletenessId {
    pub fn new(hash: ContentHash) -> Result<Self, SemanticError> {
        require_hash(hash, "completeness id")?;
        Ok(Self(hash))
    }

    pub fn hash(self) -> ContentHash {
        self.0
    }
}

/// A candidate answer plus residual obligations.  An empty obligation list
/// means the answer is unconditional; it does not by itself claim truth.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Conditional<T> {
    value: T,
    obligations: Vec<Requirement>,
}

impl<T> Conditional<T> {
    pub fn new(value: T, obligations: Vec<Requirement>) -> Self {
        Self { value, obligations }
    }

    pub fn unconditional(value: T) -> Self {
        Self::new(value, Vec::new())
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn obligations(&self) -> &[Requirement] {
        &self.obligations
    }

    pub fn into_value(self) -> T {
        self.value
    }
}

/// A residual obligation that prevents an otherwise useful candidate from
/// becoming an unconditional answer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Requirement {
    Evidence(ContentHash),
    Decision(DecisionId),
    Completeness(CompletenessId),
    ResourceBoundary,
    Theory(ContentHash),
}

/// A focused contradiction between positive and negative support.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Conflict {
    subject: GoalId,
    positive_proofs: Vec<ProofId>,
    negative_proofs: Vec<ProofId>,
}

impl Conflict {
    pub fn new(
        subject: GoalId,
        positive_proofs: Vec<ProofId>,
        negative_proofs: Vec<ProofId>,
    ) -> Result<Self, SemanticError> {
        if positive_proofs.is_empty() || negative_proofs.is_empty() {
            return Err(SemanticError::InvalidProof);
        }
        if positive_proofs.contains(&ProofId::ZERO) || negative_proofs.contains(&ProofId::ZERO) {
            return Err(SemanticError::InvalidProof);
        }
        Ok(Self {
            subject,
            positive_proofs: canonical_proofs(positive_proofs),
            negative_proofs: canonical_proofs(negative_proofs),
        })
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

/// A machine-readable next action.  Repairs change the unresolved state; they
/// do not silently choose an answer.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Repair {
    SupplyEvidence(ContentHash),
    MakeDecision(DecisionId),
    DeclareCompleteness(CompletenessId),
    ResolveConflict(GoalId),
}

/// A generic result of solving a semantic goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution<T> {
    truth: Truth,
    answers: Multiplicity<Conditional<T>>,
    completion: Completion,
    /// The checked proof context that owns every proof id in this result.
    /// Keeping the bundle with the resolution prevents a value from becoming
    /// detached from the certificate it was checked against.
    proof_context: Proof,
    positive_proofs: Vec<ProofId>,
    negative_proofs: Vec<ProofId>,
    blockers: Vec<Requirement>,
    conflicts: Vec<Conflict>,
    repairs: Vec<Repair>,
}

impl<T> Resolution<T> {
    #[allow(clippy::too_many_arguments)]
    /// Build a value from semantic parts without a proof context.
    ///
    /// This constructor is intentionally private.  Callers outside this
    /// module must use [`Resolution::new_checked`], which verifies a complete
    /// canonical proof bundle and requires every referenced id to be present.
    fn new(
        positive_proofs: Vec<ProofId>,
        negative_proofs: Vec<ProofId>,
        answers: Multiplicity<Conditional<T>>,
        completion: Completion,
        blockers: Vec<Requirement>,
        conflicts: Vec<Conflict>,
        repairs: Vec<Repair>,
    ) -> Result<Self, SemanticError> {
        if positive_proofs.contains(&ProofId::ZERO) || negative_proofs.contains(&ProofId::ZERO) {
            return Err(SemanticError::InvalidProof);
        }
        let truth = Truth::from_support(!positive_proofs.is_empty(), !negative_proofs.is_empty());

        if matches!(answers, Multiplicity::Multiple(ref values) if values.len() < 2) {
            return Err(SemanticError::InvalidResolution(
                "multiple answers require at least two values",
            ));
        }

        let has_unconditional_answer = match &answers {
            Multiplicity::None => false,
            Multiplicity::Unique(answer) => answer.obligations.is_empty(),
            Multiplicity::Multiple(values) => {
                values.iter().any(|answer| answer.obligations.is_empty())
            }
        };
        if has_unconditional_answer && positive_proofs.is_empty() {
            return Err(SemanticError::InvalidResolution(
                "an unconditional answer requires positive support",
            ));
        }

        if truth == Truth::Both && conflicts.is_empty() {
            return Err(SemanticError::InvalidResolution(
                "both-sided support requires a conflict diagnostic",
            ));
        }
        if truth != Truth::Both && !conflicts.is_empty() {
            return Err(SemanticError::InvalidResolution(
                "a conflict diagnostic requires both-sided support",
            ));
        }
        for conflict in &conflicts {
            if conflict
                .positive_proofs
                .iter()
                .any(|proof| !positive_proofs.contains(proof))
                || conflict
                    .negative_proofs
                    .iter()
                    .any(|proof| !negative_proofs.contains(proof))
            {
                return Err(SemanticError::InvalidResolution(
                    "conflict proofs must be present in resolution support",
                ));
            }
        }

        let has_resource_boundary = blockers.contains(&Requirement::ResourceBoundary);
        if has_resource_boundary != matches!(completion, Completion::ResourceLimited) {
            return Err(SemanticError::InvalidResolution(
                "resource-limited completion must carry a resource blocker",
            ));
        }
        Ok(Self {
            truth,
            answers,
            completion,
            proof_context: Proof::new(),
            positive_proofs: canonical_proofs(positive_proofs),
            negative_proofs: canonical_proofs(negative_proofs),
            blockers,
            conflicts,
            repairs,
        })
    }

    /// Construct a resolution only after checking that every support and
    /// conflict proof is a member of one independently checked canonical DAG.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        proof: &Proof,
        positive_proofs: Vec<ProofId>,
        negative_proofs: Vec<ProofId>,
        answers: Multiplicity<Conditional<T>>,
        completion: Completion,
        blockers: Vec<Requirement>,
        conflicts: Vec<Conflict>,
        repairs: Vec<Repair>,
    ) -> Result<Self, SemanticError> {
        proof
            .check()
            .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
        for id in positive_proofs.iter().chain(&negative_proofs) {
            proof
                .check_member(*id)
                .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
        }
        for conflict in &conflicts {
            for id in conflict
                .positive_proofs()
                .iter()
                .chain(conflict.negative_proofs())
            {
                proof
                    .check_member(*id)
                    .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
            }
        }
        let mut resolution = Self::new(
            positive_proofs,
            negative_proofs,
            answers,
            completion,
            blockers,
            conflicts,
            repairs,
        )?;
        resolution.proof_context = proof.clone();
        Ok(resolution)
    }

    pub fn validate_proof_context(&self, proof: &Proof) -> Result<(), SemanticError> {
        proof
            .check()
            .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
        for id in self.positive_proofs.iter().chain(&self.negative_proofs) {
            proof
                .check_member(*id)
                .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
        }
        for conflict in &self.conflicts {
            for id in conflict
                .positive_proofs()
                .iter()
                .chain(conflict.negative_proofs())
            {
                proof
                    .check_member(*id)
                    .map_err(|error| SemanticError::InvalidProofContext(error.to_string()))?;
            }
        }
        Ok(())
    }

    pub fn truth(&self) -> Truth {
        self.truth
    }

    pub fn answers(&self) -> &Multiplicity<Conditional<T>> {
        &self.answers
    }

    pub fn completion(&self) -> Completion {
        self.completion
    }

    /// The canonical proof bundle checked when this resolution was built.
    pub fn proof_context(&self) -> &Proof {
        &self.proof_context
    }

    pub fn positive_proofs(&self) -> &[ProofId] {
        &self.positive_proofs
    }

    pub fn negative_proofs(&self) -> &[ProofId] {
        &self.negative_proofs
    }

    pub fn blockers(&self) -> &[Requirement] {
        &self.blockers
    }

    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    pub fn repairs(&self) -> &[Repair] {
        &self.repairs
    }

    pub fn is_blocked(&self) -> bool {
        !self.blockers.is_empty()
    }

    pub fn is_ambiguous(&self) -> bool {
        matches!(self.answers, Multiplicity::Multiple(_))
    }
}

/// A scope to which a decision applies.  Scope is explicit and orthogonal to
/// the selected answer, so the same answer can safely be chosen elsewhere.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecisionScope {
    Occurrence(OccurrenceId),
    Account(AccountId),
    Book(BookId),
    Jurisdiction(ExternalId),
    Scenario(ExternalId),
    Relation(ContentHash),
}

impl DecisionScope {
    pub fn occurrence(id: impl Into<OccurrenceId>) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "occurrence scope")?;
        Ok(Self::Occurrence(id))
    }

    pub fn account(id: impl Into<AccountId>) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "account scope")?;
        Ok(Self::Account(id))
    }

    pub fn book(id: impl Into<BookId>) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "book scope")?;
        Ok(Self::Book(id))
    }

    pub fn jurisdiction(id: impl Into<ExternalId>) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "jurisdiction scope")?;
        Ok(Self::Jurisdiction(id))
    }

    pub fn scenario(id: impl Into<ExternalId>) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "scenario scope")?;
        Ok(Self::Scenario(id))
    }

    pub fn relation(hash: ContentHash) -> Result<Self, SemanticError> {
        require_hash(hash, "relation scope")?;
        Ok(Self::Relation(hash))
    }
}

/// A signed or authored answer to a resolution goal.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Decision {
    id: DecisionId,
    subject: GoalId,
    selected: AnswerId,
    rejected: Vec<AnswerId>,
    scope: DecisionScope,
    authority: Authority,
    rationale: Option<String>,
    made_at: Date,
    effective_during: TemporalScope,
    supersedes: Option<DecisionId>,
}

impl Decision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<DecisionId>,
        subject: GoalId,
        selected: AnswerId,
        rejected: Vec<AnswerId>,
        scope: DecisionScope,
        authority: Authority,
        rationale: Option<String>,
        made_at: Date,
        effective_during: TemporalScope,
        supersedes: Option<DecisionId>,
    ) -> Result<Self, SemanticError> {
        let id = id.into();
        require_id(id.as_str(), "decision id")?;
        if rejected.contains(&selected) {
            return Err(SemanticError::SelectedAnswerRejected);
        }
        for (index, answer) in rejected.iter().enumerate() {
            if rejected[index + 1..].contains(answer) {
                return Err(SemanticError::DuplicateAnswer);
            }
        }
        if supersedes.as_ref().is_some_and(|previous| previous == &id) {
            return Err(SemanticError::SelfSupersedingDecision);
        }
        if rationale
            .as_ref()
            .is_some_and(|text| text.trim().is_empty())
        {
            return Err(SemanticError::EmptyField("decision rationale"));
        }
        Ok(Self {
            id,
            subject,
            selected,
            rejected,
            scope,
            authority,
            rationale,
            made_at,
            effective_during,
            supersedes,
        })
    }

    pub fn id(&self) -> &DecisionId {
        &self.id
    }

    pub fn subject(&self) -> GoalId {
        self.subject
    }

    pub fn selected(&self) -> AnswerId {
        self.selected
    }

    pub fn rejected(&self) -> &[AnswerId] {
        &self.rejected
    }

    pub fn scope(&self) -> &DecisionScope {
        &self.scope
    }

    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    pub fn rationale(&self) -> Option<&str> {
        self.rationale.as_deref()
    }

    pub fn made_at(&self) -> Date {
        self.made_at
    }

    pub fn effective_during(&self) -> TemporalScope {
        self.effective_during
    }

    pub fn supersedes(&self) -> Option<&DecisionId> {
        self.supersedes.as_ref()
    }

    pub fn applies_at(&self, date: Date) -> bool {
        self.effective_during.contains(date)
    }
}

/// The relation/domain and world covered by a completeness claim.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompletenessScope {
    relation: String,
    subject: Option<ContentHash>,
    world: World,
}

impl CompletenessScope {
    pub fn new(relation: impl Into<String>, world: World) -> Result<Self, SemanticError> {
        let relation = relation.into();
        require_id(&relation, "completeness relation")?;
        Ok(Self {
            relation,
            subject: None,
            world,
        })
    }

    pub fn for_subject(
        relation: impl Into<String>,
        subject: ContentHash,
        world: World,
    ) -> Result<Self, SemanticError> {
        let mut scope = Self::new(relation, world)?;
        require_hash(subject, "completeness subject")?;
        scope.subject = Some(subject);
        Ok(scope)
    }

    pub fn relation(&self) -> &str {
        &self.relation
    }

    pub fn subject(&self) -> Option<ContentHash> {
        self.subject
    }

    pub fn world(&self) -> &World {
        &self.world
    }
}

/// A dated revocation retained as part of the claim's audit history.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revocation {
    at: Date,
    authority: Authority,
}

impl Revocation {
    pub fn new(at: Date, authority: Authority) -> Self {
        Self { at, authority }
    }

    pub fn at(&self) -> Date {
        self.at
    }

    pub fn authority(&self) -> &Authority {
        &self.authority
    }
}

/// Explicit permission to treat absence as meaningful in one bounded domain.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompletenessClaim {
    id: CompletenessId,
    scope: CompletenessScope,
    valid_during: TemporalScope,
    sources: Vec<SourceId>,
    provenance: Provenance,
    revocations: Vec<Revocation>,
}

impl CompletenessClaim {
    pub fn new(
        id: CompletenessId,
        scope: CompletenessScope,
        valid_during: TemporalScope,
        sources: Vec<SourceId>,
        provenance: Provenance,
    ) -> Result<Self, SemanticError> {
        provenance.validate()?;
        if sources.is_empty() {
            return Err(SemanticError::EmptyField("completeness source set"));
        }
        for source in &sources {
            require_id(source.as_str(), "completeness source")?;
        }
        for (index, source) in sources.iter().enumerate() {
            if sources[index + 1..].contains(source) {
                return Err(SemanticError::DuplicateAnswer);
            }
        }
        Ok(Self {
            id,
            scope,
            valid_during,
            sources,
            provenance,
            revocations: Vec::new(),
        })
    }

    pub fn id(&self) -> CompletenessId {
        self.id
    }

    pub fn scope(&self) -> &CompletenessScope {
        &self.scope
    }

    pub fn valid_during(&self) -> TemporalScope {
        self.valid_during
    }

    pub fn sources(&self) -> &[SourceId] {
        &self.sources
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn revocations(&self) -> &[Revocation] {
        &self.revocations
    }

    pub fn revoke(&self, revocation: Revocation) -> Result<Self, SemanticError> {
        if self
            .revocations
            .last()
            .is_some_and(|previous| revocation.at < previous.at)
        {
            return Err(SemanticError::RevocationOutOfOrder);
        }
        let mut next = self.clone();
        next.revocations.push(revocation);
        Ok(next)
    }

    pub fn is_active_at(&self, date: Date) -> bool {
        self.valid_during.contains(date)
            && self
                .revocations
                .iter()
                .all(|revocation| revocation.at > date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Date;

    fn date(text: &str) -> Date {
        text.parse().expect("valid date")
    }

    fn hash(seed: u8) -> ContentHash {
        ContentHash::from_bytes([seed; 32])
    }

    fn proof(seed: u8) -> ProofId {
        ProofId([seed; 32])
    }

    fn authority() -> Authority {
        Authority::user("alice").expect("authority")
    }

    fn provenance() -> Provenance {
        Provenance::source_observation("bank", hash(1)).expect("provenance")
    }

    #[test]
    fn statement_keeps_axes_orthogonal() {
        let statement = Statement::new(
            "payment",
            Polarity::Negative,
            World::scenario("forecast").expect("scenario"),
            Force::Required,
            Phase::recognized("tax").expect("phase"),
            TemporalScope::at(date("2026-09-21")),
            Provenance::policy_derivation("tax-policy", proof(3)).expect("provenance"),
            authority(),
        )
        .expect("statement");

        assert_eq!(statement.polarity(), Polarity::Negative);
        assert_eq!(statement.force(), Force::Required);
        assert_eq!(
            statement.world().scenario_id().expect("scenario"),
            &ExternalId::from("forecast")
        );
        assert!(matches!(statement.phase(), Phase::Recognized(book) if book.as_str() == "tax"));
        assert!(statement.temporal().contains(date("2026-09-21")));
    }

    #[test]
    fn paraconsistent_truth_has_no_explosion() {
        assert_eq!(Truth::from_support(false, false), Truth::Neither);
        assert_eq!(Truth::from_support(true, false), Truth::TrueOnly);
        assert_eq!(Truth::from_support(false, true), Truth::FalseOnly);
        assert_eq!(Truth::from_support(true, true), Truth::Both);

        let subject = GoalId::new(hash(9)).expect("goal");
        let conflict = Conflict::new(subject, vec![proof(1)], vec![proof(2)]).expect("conflict");
        let result = Resolution::<&str>::new(
            vec![proof(1)],
            vec![proof(2)],
            Multiplicity::none(),
            Completion::Complete,
            Vec::new(),
            vec![conflict],
            Vec::new(),
        )
        .expect("resolution");
        assert_eq!(result.truth(), Truth::Both);
        assert!(!result.is_blocked());
        // Contradiction is a local support state, not permission to invent an
        // unrelated answer (the paraconsistent "no explosion" property).
        assert!(result.answers().is_empty());
    }

    #[test]
    fn actual_and_scenario_worlds_do_not_equal() {
        let actual = World::actual();
        let scenario = World::scenario("budget").expect("scenario");
        assert_ne!(actual, scenario);

        let actual_scope = CompletenessScope::new("bank_activity", actual).expect("scope");
        let scenario_scope = CompletenessScope::new("bank_activity", scenario).expect("scope");
        assert_ne!(actual_scope, scenario_scope);
    }

    #[test]
    fn decisions_are_scoped_and_rejections_are_explicit() {
        let goal = GoalId::new(hash(3)).expect("goal");
        let selected = AnswerId::new(hash(4)).expect("answer");
        let rejected = AnswerId::new(hash(5)).expect("answer");
        let decision = Decision::new(
            "decide/one",
            goal,
            selected,
            vec![rejected],
            DecisionScope::occurrence("sale/one").expect("scope"),
            authority(),
            Some("receipt identifies the lot".to_owned()),
            date("2026-09-21"),
            TemporalScope::through(date("2026-12-31")),
            None,
        )
        .expect("decision");

        assert_eq!(
            decision.scope(),
            &DecisionScope::Occurrence(OccurrenceId::from("sale/one"))
        );
        assert_eq!(decision.selected(), selected);
        assert_eq!(decision.rejected(), &[rejected]);
        assert!(decision.applies_at(date("2026-10-01")));
        assert!(!decision.applies_at(date("2027-01-01")));
    }

    #[test]
    fn completeness_is_bounded_and_revocable() {
        let claim = CompletenessClaim::new(
            CompletenessId::new(hash(6)).expect("claim id"),
            CompletenessScope::new("bank_activity", World::actual()).expect("scope"),
            TemporalScope::between(date("2026-09-01"), date("2026-09-30")).expect("interval"),
            vec![SourceId::from("statement/september")],
            provenance(),
        )
        .expect("claim");

        assert!(claim.is_active_at(date("2026-09-15")));
        assert!(!claim.is_active_at(date("2026-10-01")));

        let revoked = claim
            .revoke(Revocation::new(date("2026-09-20"), authority()))
            .expect("revocation");
        assert!(revoked.is_active_at(date("2026-09-19")));
        assert!(!revoked.is_active_at(date("2026-09-20")));
        assert_eq!(revoked.revocations().len(), 1);
        assert_eq!(
            revoked.revoke(Revocation::new(date("2026-09-19"), authority())),
            Err(SemanticError::RevocationOutOfOrder)
        );
    }

    #[test]
    fn invariants_reject_invalid_values() {
        assert_eq!(
            World::scenario(" "),
            Err(SemanticError::EmptyField("scenario id"))
        );
        assert_eq!(
            TemporalScope::between(date("2026-09-02"), date("2026-09-01")),
            Err(SemanticError::InvalidTemporalScope)
        );
        assert_eq!(
            GoalId::new(ContentHash::ZERO),
            Err(SemanticError::ZeroHash("goal id"))
        );

        let goal = GoalId::new(hash(7)).expect("goal");
        let answer = AnswerId::new(hash(8)).expect("answer");
        assert_eq!(
            Decision::new(
                "d",
                goal,
                answer,
                vec![answer],
                DecisionScope::book("tax").expect("scope"),
                authority(),
                None,
                date("2026-09-21"),
                TemporalScope::all_time(),
                None,
            ),
            Err(SemanticError::SelectedAnswerRejected)
        );
    }

    #[test]
    fn resolution_retains_conditional_answers_and_blockers() {
        let requirement = Requirement::Decision(DecisionId::from("decide/lot"));
        let answer = Conditional::new("lot/one", vec![requirement.clone()]);
        let result = Resolution::new(
            Vec::new(),
            Vec::new(),
            Multiplicity::unique(answer),
            Completion::OpenWorld,
            vec![requirement],
            Vec::new(),
            vec![Repair::MakeDecision(DecisionId::from("decide/lot"))],
        )
        .expect("resolution");

        assert_eq!(result.truth(), Truth::Neither);
        assert!(result.is_blocked());
        assert_eq!(result.answers().len(), 1);
        assert_eq!(result.repairs().len(), 1);
    }

    #[test]
    fn singleton_multiple_becomes_unique() {
        let answer = Conditional::unconditional("only");
        assert_eq!(
            Multiplicity::multiple(vec![answer]),
            Some(Multiplicity::Unique(Conditional::unconditional("only"),))
        );
        assert!(Multiplicity::<Conditional<&str>>::multiple(Vec::new()).is_none());
        assert_eq!(
            Multiplicity::multiple(vec![1, 2]),
            Some(Multiplicity::Multiple(
                NonEmpty::from_vec(vec![1, 2]).expect("two values"),
            ))
        );
    }

    #[test]
    fn resolution_rejects_incoherent_diagnostics_but_accepts_conditionals() {
        let conditional = Conditional::new(
            "candidate",
            vec![Requirement::Decision(DecisionId::from("choose"))],
        );
        let accepted = Resolution::new(
            Vec::new(),
            Vec::new(),
            Multiplicity::unique(conditional),
            Completion::OpenWorld,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("conditional candidate is legitimate");
        assert_eq!(accepted.truth(), Truth::Neither);

        let both_without_conflict = Resolution::<&str>::new(
            vec![proof(10)],
            vec![proof(11)],
            Multiplicity::none(),
            Completion::Complete,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            both_without_conflict,
            Err(SemanticError::InvalidResolution(
                "both-sided support requires a conflict diagnostic",
            ))
        );

        let unconditional_without_support = Resolution::new(
            Vec::new(),
            Vec::new(),
            Multiplicity::unique(Conditional::unconditional("unsupported")),
            Completion::Complete,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            unconditional_without_support,
            Err(SemanticError::InvalidResolution(
                "an unconditional answer requires positive support",
            ))
        );
    }

    #[test]
    fn checked_resolution_rejects_forged_support_ids() {
        use crate::proof::{Node, Operation, Proof};
        use std::collections::BTreeMap;

        let mut bundle = Proof::new();
        let root = bundle.insert(Node::new(
            "answer",
            Operation::Observation {
                source: "test".into(),
            },
            Vec::new(),
            BTreeMap::new(),
        ));
        bundle.root(root);
        let valid = Resolution::new_checked(
            &bundle,
            vec![root],
            Vec::new(),
            Multiplicity::unique(Conditional::unconditional("answer")),
            Completion::Complete,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("bundle member is accepted");
        assert_eq!(valid.positive_proofs(), &[root]);
        assert!(matches!(
            Resolution::<&str>::new_checked(
                &bundle,
                vec![ProofId([9; 32])],
                Vec::new(),
                Multiplicity::none(),
                Completion::Complete,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            Err(SemanticError::InvalidProofContext(_))
        ));
    }

    #[test]
    fn phase_transitions_require_matching_provenance() {
        let observed = Statement::new(
            "payment",
            Polarity::Positive,
            World::actual(),
            Force::Descriptive,
            Phase::Observed,
            TemporalScope::at(date("2026-09-21")),
            provenance(),
            authority(),
        )
        .expect("observed");
        let resolved = observed
            .resolve(
                Provenance::policy_derivation("policy", proof(12)).expect("policy provenance"),
                Authority::policy("policy").expect("policy authority"),
            )
            .expect("resolved");
        let accepted = resolved
            .accept(
                Provenance::signed_decision("decision/one", proof(13))
                    .expect("decision provenance"),
                Authority::signed_decision("alice", "decision/one").expect("authority"),
            )
            .expect("accepted");
        let recognized = accepted
            .recognize(
                "tax",
                Provenance::policy_derivation("tax-policy", proof(14)).expect("policy provenance"),
                Authority::policy("tax-policy").expect("policy authority"),
            )
            .expect("recognized");
        assert!(matches!(recognized.phase(), Phase::Recognized(book) if book.as_str() == "tax"));

        let invalid_observed = Statement::new(
            "payment",
            Polarity::Positive,
            World::actual(),
            Force::Descriptive,
            Phase::Observed,
            TemporalScope::all_time(),
            Provenance::policy_derivation("policy", proof(15)).expect("policy provenance"),
            authority(),
        );
        assert_eq!(
            invalid_observed,
            Err(SemanticError::PhaseProvenanceMismatch)
        );

        let malformed_provenance = Statement::new(
            "payment",
            Polarity::Positive,
            World::actual(),
            Force::Descriptive,
            Phase::Observed,
            TemporalScope::all_time(),
            Provenance::UserAssertion {
                author: EntityId::try_new("test-author").expect("valid author id"),
                content: ContentHash::ZERO,
            },
            authority(),
        );
        assert_eq!(malformed_provenance, Err(SemanticError::InvalidProvenance));

        let invalid_transition = recognized.accept(
            Provenance::signed_decision("decision/two", proof(16)).expect("decision provenance"),
            authority(),
        );
        assert_eq!(
            invalid_transition,
            Err(SemanticError::InvalidPhaseTransition)
        );
    }
}
