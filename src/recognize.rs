//! Book-specific recognition over an immutable accepted world.
//!
//! This module deliberately sits at the boundary between accepted facts and
//! reports.  An accepted fact is referenced, never copied into a recognized
//! fact, and a policy is an input to recognition rather than a property of the
//! source event.  Consequently a cash, accrual, and tax book can interpret
//! one event independently while retaining one source occurrence and one
//! accepted-world commit.
//!
//! The types here are intentionally small and ontology-neutral.  Kinds,
//! classifications, basis labels, valuation labels, and arbitrary attributes
//! are strings.  A later ontology or semantics layer can attach meaning to
//! those labels without making this boundary depend on an account hierarchy.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use crate::model::{BookId, ContentHash, Date, DecisionId, ExternalId, OccurrenceId, PolicyId};
use crate::proof::{CheckError as ProofCheckError, Node, Operation, Proof, ProofId};

/// A fact is either part of the actual accepted world or an explicitly scoped
/// scenario.  Scenarios are useful to callers, but they may not cross into an
/// actual-book recognition.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FactScope {
    Actual,
    Scenario(ExternalId),
}

impl FactScope {
    pub fn is_actual(&self) -> bool {
        matches!(self, Self::Actual)
    }

    pub fn scenario(id: impl Into<String>) -> Result<Self, RecognitionFactError> {
        let id = ExternalId::try_new(id.into())
            .map_err(|_| RecognitionFactError::MissingScenarioIdentity)?;
        Ok(Self::Scenario(id))
    }

    pub fn scenario_id(&self) -> Option<&ExternalId> {
        match self {
            Self::Actual => None,
            Self::Scenario(id) => Some(id),
        }
    }
}

/// A recognition-boundary accepted event.  The event's economic meaning is
/// intentionally opaque here; `kind` and `attributes` are extension points
/// for ontology and semantics packages.  This type is deliberately named
/// with the recognition boundary to avoid competing with the canonical
/// ontology and semantics accepted-fact types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognitionAcceptedFact {
    id: OccurrenceId,
    kind: String,
    occurrence_date: Date,
    settlement_date: Option<Date>,
    scope: FactScope,
    attributes: BTreeMap<String, String>,
    proof: ProofId,
    authority: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecognitionFactError {
    MissingProof { fact: OccurrenceId },
    MissingAuthority { fact: OccurrenceId },
    MissingScenarioIdentity,
    InvalidProof(String),
}

impl fmt::Display for RecognitionFactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProof { fact } => write!(formatter, "accepted fact {fact} has no proof"),
            Self::MissingAuthority { fact } => {
                write!(formatter, "accepted fact {fact} has no authority")
            }
            Self::MissingScenarioIdentity => {
                formatter.write_str("scenario accepted fact has no scenario identity")
            }
            Self::InvalidProof(reason) => {
                write!(formatter, "invalid accepted-fact proof: {reason}")
            }
        }
    }
}

impl std::error::Error for RecognitionFactError {}

impl RecognitionAcceptedFact {
    /// Construct an actual accepted fact.  Acceptance requires both a
    /// non-zero proof reference and a non-empty authority; callers cannot
    /// manufacture an accepted fact from an unproven label alone.
    pub fn actual(
        id: impl Into<OccurrenceId>,
        kind: impl Into<String>,
        date: Date,
        proof: ProofId,
        authority: impl Into<String>,
    ) -> Result<Self, RecognitionFactError> {
        Self::new(id, kind, date, FactScope::Actual, proof, authority)
    }

    /// Construct a scenario fact with explicit provenance.  It remains
    /// ineligible for actual-book recognition even when it is well-proven.
    pub fn scenario(
        id: impl Into<OccurrenceId>,
        kind: impl Into<String>,
        date: Date,
        scenario: impl Into<String>,
        proof: ProofId,
        authority: impl Into<String>,
    ) -> Result<Self, RecognitionFactError> {
        Self::new(
            id,
            kind,
            date,
            FactScope::scenario(scenario)?,
            proof,
            authority,
        )
    }

    pub fn actual_checked(
        id: impl Into<OccurrenceId>,
        kind: impl Into<String>,
        date: Date,
        proof: ProofId,
        authority: impl Into<String>,
        bundle: &Proof,
    ) -> Result<Self, RecognitionFactError> {
        let fact = Self::actual(id, kind, date, proof, authority)?;
        let node = bundle
            .check_member(proof)
            .map_err(|error| RecognitionFactError::InvalidProof(error.to_string()))?;
        if !proof_reaches(bundle, proof) {
            return Err(RecognitionFactError::InvalidProof(
                "proof node is not rooted in the proof bundle".into(),
            ));
        }
        if !node
            .metadata
            .get("accepted-fact")
            .is_some_and(|bound| bound == fact.id.as_str())
        {
            return Err(RecognitionFactError::InvalidProof(
                "proof node is not bound to the accepted fact".into(),
            ));
        }
        Ok(fact)
    }

    pub fn scenario_checked(
        id: impl Into<OccurrenceId>,
        kind: impl Into<String>,
        date: Date,
        scenario: impl Into<String>,
        proof: ProofId,
        authority: impl Into<String>,
        bundle: &Proof,
    ) -> Result<Self, RecognitionFactError> {
        let fact = Self::scenario(id, kind, date, scenario, proof, authority)?;
        let node = bundle
            .check_member(proof)
            .map_err(|error| RecognitionFactError::InvalidProof(error.to_string()))?;
        if !proof_reaches(bundle, proof) {
            return Err(RecognitionFactError::InvalidProof(
                "proof node is not rooted in the proof bundle".into(),
            ));
        }
        if !node
            .metadata
            .get("accepted-fact")
            .is_some_and(|bound| bound == fact.id.as_str())
        {
            return Err(RecognitionFactError::InvalidProof(
                "proof node is not bound to the accepted fact".into(),
            ));
        }
        Ok(fact)
    }

    fn new(
        id: impl Into<OccurrenceId>,
        kind: impl Into<String>,
        date: Date,
        scope: FactScope,
        proof: ProofId,
        authority: impl Into<String>,
    ) -> Result<Self, RecognitionFactError> {
        let id = id.into();
        let authority = authority.into();
        if proof == ProofId::ZERO {
            return Err(RecognitionFactError::MissingProof { fact: id });
        }
        if authority.trim().is_empty() {
            return Err(RecognitionFactError::MissingAuthority { fact: id });
        }
        Ok(Self {
            id,
            kind: kind.into(),
            occurrence_date: date,
            settlement_date: None,
            scope,
            attributes: BTreeMap::new(),
            proof,
            authority,
        })
    }

    pub fn id(&self) -> &OccurrenceId {
        &self.id
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn occurrence_date(&self) -> Date {
        self.occurrence_date
    }

    pub fn settlement_date(&self) -> Option<Date> {
        self.settlement_date
    }

    pub fn scope(&self) -> FactScope {
        self.scope.clone()
    }

    pub fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    pub fn proof(&self) -> ProofId {
        self.proof
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn with_settlement_date(mut self, date: Date) -> Self {
        self.settlement_date = Some(date);
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub fn with_attributes<I, K, V>(mut self, attributes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in attributes {
            self.attributes.insert(key.into(), value.into());
        }
        self
    }
}

/// Errors found while constructing an accepted world.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorldError {
    DuplicateFact { id: OccurrenceId },
    MissingProofBundle,
    InvalidProof(String),
    MissingProofNode { fact: OccurrenceId, proof: ProofId },
    ProofBindingMismatch { fact: OccurrenceId, proof: ProofId },
}

impl fmt::Display for WorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateFact { id } => write!(formatter, "duplicate accepted fact {id}"),
            Self::MissingProofBundle => {
                formatter.write_str("accepted world has no checked proof bundle")
            }
            Self::InvalidProof(reason) => {
                write!(formatter, "invalid accepted-world proof: {reason}")
            }
            Self::MissingProofNode { fact, proof } => {
                write!(
                    formatter,
                    "accepted fact {fact} refers to missing proof node {proof}"
                )
            }
            Self::ProofBindingMismatch { fact, proof } => write!(
                formatter,
                "accepted fact {fact} proof {proof} is not bound to the fact",
            ),
        }
    }
}

impl std::error::Error for WorldError {}

/// An immutable set of accepted facts identified by the accepted-source
/// commit.  `with_fact` returns a new world; it never mutates the receiver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedWorld {
    source_commit: ContentHash,
    world_root: ContentHash,
    facts: Vec<RecognitionAcceptedFact>,
    proof: Option<Proof>,
}

impl AcceptedWorld {
    pub fn new(source_commit: ContentHash) -> Self {
        let facts = Vec::new();
        Self {
            source_commit,
            world_root: world_root(source_commit, &facts, None),
            facts,
            proof: None,
        }
    }

    pub fn from_facts<I>(source_commit: ContentHash, facts: I) -> Result<Self, WorldError>
    where
        I: IntoIterator<Item = RecognitionAcceptedFact>,
    {
        let mut world = Self::new(source_commit);
        for fact in facts {
            world = world.try_with_fact(fact)?;
        }
        Ok(world)
    }

    /// Construct a world from a checked proof bundle and its fact references.
    /// This is the preferred constructor for callers crossing the acceptance
    /// boundary; `from_facts` is retained only as a compatibility-shaped
    /// builder and rejects its first fact because it has no bundle to check.
    pub fn from_facts_checked<I>(
        source_commit: ContentHash,
        facts: I,
        proof: Proof,
    ) -> Result<Self, WorldError>
    where
        I: IntoIterator<Item = RecognitionAcceptedFact>,
    {
        let mut world = Self::new(source_commit).with_proof(proof)?;
        for fact in facts {
            world = world.with_fact(fact)?;
        }
        Ok(world)
    }

    pub fn from_facts_with_proof<I>(
        source_commit: ContentHash,
        facts: I,
        proof: Proof,
    ) -> Result<Self, WorldError>
    where
        I: IntoIterator<Item = RecognitionAcceptedFact>,
    {
        Self::from_facts_checked(source_commit, facts, proof)
    }

    pub fn new_checked(source_commit: ContentHash, proof: Proof) -> Result<Self, WorldError> {
        Self::new(source_commit).with_proof(proof)
    }

    /// Return a new world containing `fact`, rejecting duplicate occurrences
    /// instead of silently appending a second accepted fact with the same
    /// identity.
    pub fn with_fact(&self, fact: RecognitionAcceptedFact) -> Result<Self, WorldError> {
        if self.facts.iter().any(|existing| existing.id == fact.id) {
            return Err(WorldError::DuplicateFact {
                id: fact.id.clone(),
            });
        }
        let proof = self.proof.as_ref().ok_or(WorldError::MissingProofBundle)?;
        check_accepted_fact_proof(proof, &fact)?;
        let mut next = self.clone();
        next.facts.push(fact);
        next.facts.sort_by(|left, right| left.id.cmp(&right.id));
        next.world_root = world_root(next.source_commit, &next.facts, next.proof.as_ref());
        Ok(next)
    }

    pub fn try_with_fact(&self, fact: RecognitionAcceptedFact) -> Result<Self, WorldError> {
        self.with_fact(fact)
    }

    pub fn commit_hash(&self) -> ContentHash {
        self.source_commit
    }

    pub fn commit(&self) -> ContentHash {
        self.commit_hash()
    }

    pub fn source_commit(&self) -> ContentHash {
        self.source_commit
    }

    pub fn world_root(&self) -> ContentHash {
        self.world_root
    }

    /// Attach the independently checked proof bundle used by accepted facts.
    /// Every existing and subsequently added fact must reference a node in
    /// this bundle; the bundle content is also part of the world root.
    pub fn with_proof(mut self, proof: Proof) -> Result<Self, WorldError> {
        proof
            .check()
            .map_err(|error| WorldError::InvalidProof(error.to_string()))?;
        for fact in &self.facts {
            check_accepted_fact_proof(&proof, fact)?;
        }
        self.proof = Some(proof);
        self.world_root = world_root(self.source_commit, &self.facts, self.proof.as_ref());
        Ok(self)
    }

    pub fn proof_bundle(&self) -> Option<&Proof> {
        self.proof.as_ref()
    }

    /// Independently validate the world before it is consumed by recognition
    /// or close.  A world without a checked proof bundle is not an accepted
    /// world, even if its fact references happen to be non-zero hashes.
    pub fn check(&self) -> Result<(), WorldError> {
        let proof = self.proof.as_ref().ok_or(WorldError::MissingProofBundle)?;
        proof
            .check()
            .map_err(|error| WorldError::InvalidProof(error.to_string()))?;
        for fact in &self.facts {
            check_accepted_fact_proof(proof, fact)?;
        }
        if self.world_root != world_root(self.source_commit, &self.facts, Some(proof)) {
            return Err(WorldError::InvalidProof(
                "accepted-world root does not match its facts and proof bundle".into(),
            ));
        }
        Ok(())
    }

    pub fn with_proof_bundle(self, proof: Proof) -> Result<Self, WorldError> {
        self.with_proof(proof)
    }

    pub fn facts(&self) -> &[RecognitionAcceptedFact] {
        &self.facts
    }

    pub fn fact(&self, id: &OccurrenceId) -> Option<&RecognitionAcceptedFact> {
        self.facts.iter().find(|fact| &fact.id == id)
    }
}

fn check_accepted_fact_proof(
    proof: &Proof,
    fact: &RecognitionAcceptedFact,
) -> Result<(), WorldError> {
    let node = proof
        .check_member(fact.proof)
        .map_err(|error| match error {
            ProofCheckError::MissingNode { .. } => WorldError::MissingProofNode {
                fact: fact.id.clone(),
                proof: fact.proof,
            },
            other => WorldError::InvalidProof(other.to_string()),
        })?;
    if !proof_reaches(proof, fact.proof) {
        return Err(WorldError::InvalidProof(format!(
            "proof {} for accepted fact {} is not rooted",
            fact.proof, fact.id
        )));
    }
    if !node
        .metadata
        .get("accepted-fact")
        .is_some_and(|bound| bound == fact.id.as_str())
    {
        return Err(WorldError::ProofBindingMismatch {
            fact: fact.id.clone(),
            proof: fact.proof,
        });
    }
    Ok(())
}

/// A proof reference is rooted when it is a root or an input reachable from a
/// root.  `Proof::check` validates edge integrity; this traversal validates
/// that an otherwise valid, but orphaned, node cannot be used as evidence.
fn proof_reaches(proof: &Proof, target: ProofId) -> bool {
    let mut pending = proof.roots.clone();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if id == target {
            return true;
        }
        if let Some(node) = proof.node(id) {
            pending.extend(node.inputs.iter().copied());
        }
    }
    false
}

/// A closed-open/inclusive date interval used by policies and reporting
/// periods.  Both endpoints are inclusive.  An absent end is open-ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EffectiveInterval {
    pub start: Date,
    pub end: Option<Date>,
}

impl EffectiveInterval {
    pub const fn new(start: Date, end: Option<Date>) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, date: Date) -> bool {
        date >= self.start && self.end.is_none_or(|end| date <= end)
    }
}

/// A reporting interval.  It is separate from policy effectiveness because a
/// close can report a period under a policy whose version remains effective
/// beyond the period.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ReportingPeriod {
    pub start: Date,
    pub end: Date,
}

pub type ClosePeriod = ReportingPeriod;

impl ReportingPeriod {
    pub const fn new(start: Date, end: Date) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, date: Date) -> bool {
        date >= self.start && date <= self.end
    }
}

/// The date dimension selected by a book.  An attribute selector allows a
/// domain package to add a date without adding an ontology type here.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DateBasis {
    #[default]
    Occurrence,
    Settlement,
    Attribute(String),
}

/// A versioned interpretation of accepted facts.  `classification`, `basis`,
/// and `valuation` are book-local choices and are never written back to the
/// accepted event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookPolicy {
    id: BookId,
    hash: ContentHash,
    effective: EffectiveInterval,
    classification: Option<String>,
    date_basis: DateBasis,
    basis: Option<String>,
    valuation: Option<String>,
    choices: BTreeMap<String, String>,
}

impl BookPolicy {
    /// Build a policy whose package hash is derived from its definition.
    pub fn new(id: impl Into<BookId>, effective_start: Date, effective_end: Option<Date>) -> Self {
        let mut policy = Self {
            id: id.into(),
            effective: EffectiveInterval::new(effective_start, effective_end),
            classification: None,
            date_basis: DateBasis::Occurrence,
            basis: None,
            valuation: None,
            choices: BTreeMap::new(),
            hash: ContentHash::ZERO,
        };
        policy.hash = policy.derived_hash();
        policy
    }

    pub fn from_definition(
        id: impl Into<BookId>,
        effective_start: Date,
        effective_end: Option<Date>,
    ) -> Self {
        Self::new(id, effective_start, effective_end)
    }

    pub fn identity(&self) -> &BookId {
        &self.id
    }

    pub fn policy_id(&self) -> &BookId {
        self.identity()
    }

    pub fn content_hash(&self) -> ContentHash {
        self.hash
    }

    pub fn effective_interval(&self) -> EffectiveInterval {
        self.effective
    }

    pub fn is_effective_on(&self, date: Date) -> bool {
        self.effective.contains(date)
    }

    pub fn with_classification(mut self, classification: impl Into<String>) -> Self {
        self.classification = Some(classification.into());
        self.refresh_hash()
    }

    pub fn with_date_basis(mut self, date_basis: DateBasis) -> Self {
        self.date_basis = date_basis;
        self.refresh_hash()
    }

    pub fn with_basis(mut self, basis: impl Into<String>) -> Self {
        self.basis = Some(basis.into());
        self.refresh_hash()
    }

    pub fn with_valuation(mut self, valuation: impl Into<String>) -> Self {
        self.valuation = Some(valuation.into());
        self.refresh_hash()
    }

    pub fn with_choice(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.choices.insert(key.into(), value.into());
        self.refresh_hash()
    }

    pub fn choice(&self, key: &str) -> Option<&str> {
        self.choices.get(key).map(String::as_str)
    }

    fn refresh_hash(&mut self) -> Self {
        self.hash = self.derived_hash();
        self.clone()
    }

    fn derived_hash(&self) -> ContentHash {
        ContentHash::domain_separated("axiom/book-policy", &self.canonical_bytes(false))
    }

    fn canonical_bytes(&self, include_hash: bool) -> Vec<u8> {
        let mut output = Vec::new();
        put_string(&mut output, self.id.as_str());
        if include_hash {
            put_hash(&mut output, self.hash);
        }
        put_date(&mut output, self.effective.start);
        match self.effective.end {
            Some(end) => {
                output.push(1);
                put_date(&mut output, end);
            }
            None => output.push(0),
        }
        match &self.date_basis {
            DateBasis::Occurrence => {
                output.push(0);
            }
            DateBasis::Settlement => {
                output.push(1);
            }
            DateBasis::Attribute(attribute) => {
                output.push(2);
                put_string(&mut output, attribute);
            }
        }
        put_string(&mut output, self.classification.as_deref().unwrap_or(""));
        put_string(&mut output, self.basis.as_deref().unwrap_or(""));
        put_string(&mut output, self.valuation.as_deref().unwrap_or(""));
        for (key, value) in &self.choices {
            put_string(&mut output, key);
            put_string(&mut output, value);
        }
        output
    }
}

/// A reference from a recognized fact back to accepted evidence and its
/// derived proof node.  It contains no copy of the source economic payload.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ProofReference {
    pub source_fact: OccurrenceId,
    pub accepted_commit: ContentHash,
    pub accepted_world_root: ContentHash,
    pub source_proof: Option<ProofId>,
    pub recognition_proof: ProofId,
}

pub type ProofRef = ProofReference;

/// Book-local fields produced by recognition.  Values are intentionally
/// generic strings; no account ontology is implied by this module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookRecognizedFact {
    pub source_fact: OccurrenceId,
    pub kind: String,
    pub date: Date,
    pub classification: Option<String>,
    pub basis: Option<String>,
    pub valuation: Option<String>,
    pub attributes: BTreeMap<String, String>,
    pub proof: ProofReference,
}

impl BookRecognizedFact {
    pub fn source(&self) -> &OccurrenceId {
        &self.source_fact
    }

    pub fn proof_reference(&self) -> &ProofReference {
        &self.proof
    }
}

/// All facts recognized under one book policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizedFacts {
    pub book: BookId,
    pub policy: PolicyId,
    pub policy_hash: ContentHash,
    pub source_commit: ContentHash,
    pub world_root: ContentHash,
    pub facts: Vec<BookRecognizedFact>,
    pub proofs: Vec<ProofReference>,
    pub recognized_root: ContentHash,
}

impl RecognizedFacts {
    pub fn book(&self) -> &BookId {
        &self.book
    }

    pub fn policy_hash(&self) -> ContentHash {
        self.policy_hash
    }

    pub fn source_commit(&self) -> ContentHash {
        self.source_commit
    }

    pub fn root(&self) -> ContentHash {
        self.recognized_root
    }

    pub fn fact(&self, source_fact: &OccurrenceId) -> Option<&BookRecognizedFact> {
        self.facts
            .iter()
            .find(|fact| &fact.source_fact == source_fact)
    }
}

/// Book-indexed recognition results.  The map key prevents a result for one
/// book from being accidentally consumed as another book's result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizedBooks {
    pub books: BTreeMap<BookId, RecognizedFacts>,
    pub decisions: Vec<DecisionReference>,
    pub recognized_root: ContentHash,
}

impl Default for RecognizedBooks {
    fn default() -> Self {
        Self {
            books: BTreeMap::new(),
            decisions: Vec::new(),
            recognized_root: ContentHash::ZERO,
        }
    }
}

impl RecognizedBooks {
    pub fn new(books: BTreeMap<BookId, RecognizedFacts>) -> Self {
        let recognized_root = books_root(&books, &[]);
        Self {
            books,
            decisions: Vec::new(),
            recognized_root,
        }
    }

    pub fn with_decisions(mut self, decisions: Vec<DecisionReference>) -> Self {
        self.decisions = decisions;
        self.recognized_root = books_root(&self.books, &self.decisions);
        self
    }

    pub fn get(&self, book: &BookId) -> Option<&RecognizedFacts> {
        self.books.get(book)
    }

    pub fn for_book(&self, book: &BookId) -> Option<&RecognizedFacts> {
        self.get(book)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BookId, &RecognizedFacts)> {
        self.books.iter()
    }

    pub fn root(&self) -> ContentHash {
        self.recognized_root
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }
}

/// A pure recognizer implementation.  Implementors can supply domain-specific
/// semantics while retaining the same source/book/close boundary.
pub trait Recognizer {
    fn recognize(
        &self,
        world: &AcceptedWorld,
        policy: &BookPolicy,
    ) -> Result<RecognizedFacts, RecognitionError>;

    fn recognize_books(
        &self,
        world: &AcceptedWorld,
        policies: &[BookPolicy],
    ) -> Result<RecognizedBooks, RecognitionError> {
        let mut books = BTreeMap::new();
        for policy in policies {
            let book = self.recognize(world, policy)?;
            if books.insert(policy.id.clone(), book).is_some() {
                return Err(RecognitionError::DuplicateBook {
                    book: policy.id.clone(),
                });
            }
        }
        Ok(RecognizedBooks::new(books))
    }
}

/// The generic recognizer applies only book-local field choices; it does not
/// infer accounts, ownership, tax categories, or other ontology facts.
#[derive(Clone, Copy, Debug, Default)]
pub struct GenericRecognizer;

impl Recognizer for GenericRecognizer {
    fn recognize(
        &self,
        world: &AcceptedWorld,
        policy: &BookPolicy,
    ) -> Result<RecognizedFacts, RecognitionError> {
        self.recognize_period(world, policy, None)
    }
}

impl GenericRecognizer {
    fn recognize_period(
        &self,
        world: &AcceptedWorld,
        policy: &BookPolicy,
        period: Option<ReportingPeriod>,
    ) -> Result<RecognizedFacts, RecognitionError> {
        world
            .check()
            .map_err(|error| RecognitionError::UnverifiedWorld(error.to_string()))?;
        let mut facts = Vec::with_capacity(world.facts.len());
        for accepted in &world.facts {
            if !accepted.scope().is_actual() {
                return Err(RecognitionError::ScenarioFact {
                    book: policy.id.clone(),
                    fact: accepted.id().clone(),
                    scenario: accepted
                        .scope()
                        .scenario_id()
                        .expect("scenario scope has identity")
                        .clone(),
                });
            }

            let date = match &policy.date_basis {
                DateBasis::Occurrence => accepted.occurrence_date(),
                DateBasis::Settlement => {
                    accepted
                        .settlement_date()
                        .ok_or_else(|| RecognitionError::MissingDate {
                            book: policy.id.clone(),
                            fact: accepted.id().clone(),
                            basis: DateBasis::Settlement,
                        })?
                }
                DateBasis::Attribute(attribute) => accepted
                    .attributes()
                    .get(attribute)
                    .ok_or_else(|| RecognitionError::MissingAttribute {
                        book: policy.id.clone(),
                        fact: accepted.id().clone(),
                        attribute: attribute.clone(),
                    })
                    .and_then(|text| {
                        text.parse()
                            .map_err(|_| RecognitionError::InvalidDateAttribute {
                                book: policy.id.clone(),
                                fact: accepted.id().clone(),
                                attribute: attribute.clone(),
                                value: text.clone(),
                            })
                    })?,
            };

            if period.is_some_and(|period| !period.contains(date)) {
                continue;
            }

            if !policy.is_effective_on(date) {
                return Err(RecognitionError::PolicyNotEffective {
                    book: policy.id.clone(),
                    fact: accepted.id().clone(),
                    date,
                });
            }

            let classification = policy
                .classification
                .clone()
                .or_else(|| accepted.attributes().get("classification").cloned());
            let basis = policy
                .basis
                .clone()
                .or_else(|| accepted.attributes().get("basis").cloned());
            let valuation = policy
                .valuation
                .clone()
                .or_else(|| accepted.attributes().get("valuation").cloned());

            let proof = ProofReference {
                source_fact: accepted.id().clone(),
                accepted_commit: world.source_commit,
                accepted_world_root: world.world_root,
                source_proof: Some(accepted.proof()),
                recognition_proof: fact_proof(world.world_root, policy, accepted, date),
            };
            facts.push(BookRecognizedFact {
                source_fact: accepted.id().clone(),
                kind: accepted.kind().to_string(),
                date,
                classification,
                basis,
                valuation,
                attributes: accepted.attributes().clone(),
                proof,
            });
        }

        let recognized_root = facts_root(world.world_root, policy, &facts);
        let proofs = facts.iter().map(|fact| fact.proof.clone()).collect();
        Ok(RecognizedFacts {
            book: policy.id.clone(),
            policy: PolicyId::new(policy.id.to_string()),
            policy_hash: policy.hash,
            source_commit: world.source_commit,
            world_root: world.world_root,
            facts,
            proofs,
            recognized_root,
        })
    }
}

/// Recognize one policy over one accepted world using the generic recognizer.
pub fn recognize(
    world: &AcceptedWorld,
    policy: &BookPolicy,
) -> Result<RecognizedFacts, RecognitionError> {
    GenericRecognizer.recognize(world, policy)
}

/// Recognize several books over one accepted world.  Every book receives its
/// own result and proof root while source facts remain shared by reference.
pub fn recognize_books(
    world: &AcceptedWorld,
    policies: &[BookPolicy],
) -> Result<RecognizedBooks, RecognitionError> {
    GenericRecognizer.recognize_books(world, policies)
}

pub fn recognize_all<I>(
    world: &AcceptedWorld,
    policies: I,
) -> Result<RecognizedBooks, RecognitionError>
where
    I: IntoIterator<Item = BookPolicy>,
{
    let policies: Vec<_> = policies.into_iter().collect();
    recognize_books(world, &policies)
}

fn recognize_books_in_period(
    world: &AcceptedWorld,
    policies: &[BookPolicy],
    period: ReportingPeriod,
) -> Result<RecognizedBooks, RecognitionError> {
    let mut books = BTreeMap::new();
    for policy in policies {
        let facts = GenericRecognizer.recognize_period(world, policy, Some(period))?;
        if books.insert(policy.id.clone(), facts).is_some() {
            return Err(RecognitionError::DuplicateBook {
                book: policy.id.clone(),
            });
        }
    }
    Ok(RecognizedBooks::new(books))
}

/// Errors are explicit rather than silently dropping a fact or guessing a
/// book-local date.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecognitionError {
    UnverifiedWorld(String),
    ScenarioFact {
        book: BookId,
        fact: OccurrenceId,
        scenario: ExternalId,
    },
    DuplicateBook {
        book: BookId,
    },
    MissingDate {
        book: BookId,
        fact: OccurrenceId,
        basis: DateBasis,
    },
    MissingAttribute {
        book: BookId,
        fact: OccurrenceId,
        attribute: String,
    },
    InvalidDateAttribute {
        book: BookId,
        fact: OccurrenceId,
        attribute: String,
        value: String,
    },
    PolicyNotEffective {
        book: BookId,
        fact: OccurrenceId,
        date: Date,
    },
}

impl fmt::Display for RecognitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnverifiedWorld(reason) => {
                write!(formatter, "accepted world is unverified: {reason}")
            }
            Self::ScenarioFact {
                book,
                fact,
                scenario,
            } => {
                write!(
                    formatter,
                    "scenario fact {fact} in {scenario} cannot enter actual book {book}"
                )
            }
            Self::DuplicateBook { book } => write!(formatter, "duplicate book {book}"),
            Self::MissingDate { book, fact, basis } => {
                write!(formatter, "book {book} has no {basis:?} date for {fact}")
            }
            Self::MissingAttribute {
                book,
                fact,
                attribute,
            } => write!(
                formatter,
                "book {book} requires attribute {attribute} for {fact}"
            ),
            Self::InvalidDateAttribute {
                book,
                fact,
                attribute,
                value,
            } => write!(
                formatter,
                "book {book} received invalid date {value} in {attribute} for {fact}"
            ),
            Self::PolicyNotEffective { book, fact, date } => {
                write!(
                    formatter,
                    "book {book} is not effective on {date} for {fact}"
                )
            }
        }
    }
}

impl std::error::Error for RecognitionError {}

/// A close completeness assertion.  The key is intentionally opaque: a
/// package can name claims such as "bank-feed-complete" without this module
/// knowing what that claim means.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletenessClaim {
    name: String,
    complete: bool,
    required: bool,
    evidence: Vec<String>,
    source: Option<ContentHash>,
    scope: Option<String>,
    valid_during: Option<ReportingPeriod>,
    provenance: Option<ProofId>,
    revision: Option<ContentHash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct CompletenessClaims {
    claims: BTreeMap<String, CompletenessClaim>,
    proof: Proof,
}

pub type Completeness = CompletenessClaims;

impl CompletenessClaims {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn claim(mut self, name: impl Into<String>, complete: bool) -> Self {
        let name = name.into();
        self.claims.insert(
            name.clone(),
            CompletenessClaim {
                name,
                complete,
                required: true,
                evidence: Vec::new(),
                source: None,
                scope: None,
                valid_during: None,
                provenance: None,
                revision: None,
            },
        );
        self
    }

    pub fn with_evidence(mut self, name: impl Into<String>, evidence: impl Into<String>) -> Self {
        let name = name.into();
        let claim = self
            .claims
            .entry(name.clone())
            .or_insert_with(|| CompletenessClaim {
                name: name.clone(),
                complete: false,
                required: true,
                evidence: Vec::new(),
                source: None,
                scope: None,
                valid_during: None,
                provenance: None,
                revision: None,
            });
        claim.evidence.push(evidence.into());
        self
    }

    pub fn optional_claim(mut self, name: impl Into<String>, complete: bool) -> Self {
        let name = name.into();
        self.claims.insert(
            name.clone(),
            CompletenessClaim {
                name,
                complete,
                required: false,
                evidence: Vec::new(),
                source: None,
                scope: None,
                valid_during: None,
                provenance: None,
                revision: None,
            },
        );
        self
    }

    pub fn is_complete(&self) -> bool {
        self.is_satisfied()
    }

    pub fn is_satisfied(&self) -> bool {
        !self.claims.is_empty()
            && self.claims.values().any(|claim| claim.required)
            && self
                .claims
                .values()
                .filter(|claim| claim.required)
                .all(|claim| claim.complete && !claim.name.trim().is_empty())
    }

    /// Add a completeness assertion scoped to one accepted-world source,
    /// semantic domain, reporting interval, provenance proof, and revision.
    /// A bare `claim` remains available for legacy value construction, but it
    /// is intentionally not sufficient for a scoped close.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_scoped(
        mut self,
        name: impl Into<String>,
        complete: bool,
        source: ContentHash,
        scope: impl Into<String>,
        valid_during: ReportingPeriod,
        provenance: ProofId,
        revision: ContentHash,
    ) -> Self {
        let name = name.into();
        let scope = scope.into();
        let metadata = BTreeMap::from([
            ("completeness-name".to_string(), name.clone()),
            ("completeness-source".to_string(), source.to_string()),
            ("completeness-scope".to_string(), scope.clone()),
            (
                "completeness-period".to_string(),
                format!("{}..{}", valid_during.start, valid_during.end),
            ),
            ("completeness-revision".to_string(), revision.to_string()),
            (
                "completeness-source-provenance".to_string(),
                provenance.to_string(),
            ),
        ]);
        let node = Node::new(
            format!("completeness {name}"),
            Operation::Derive {
                rule: "completeness".into(),
            },
            Vec::new(),
            metadata,
        );
        let provenance = self.proof.insert(node);
        self.proof.root(provenance);
        self.claims.insert(
            name.clone(),
            CompletenessClaim {
                name,
                complete,
                required: true,
                evidence: Vec::new(),
                source: Some(source),
                scope: Some(scope),
                valid_during: Some(valid_during),
                provenance: Some(provenance),
                revision: Some(revision),
            },
        );
        self
    }

    pub fn is_satisfied_for(
        &self,
        source: ContentHash,
        scope: &str,
        period: ReportingPeriod,
    ) -> bool {
        !scope.trim().is_empty()
            && period.start <= period.end
            && self.is_satisfied()
            && self
                .claims
                .values()
                .filter(|claim| claim.required)
                .all(|claim| {
                    claim.name.trim() != ""
                        && claim.source == Some(source)
                        && claim.scope.as_deref().is_some_and(|claim_scope| {
                            claim_scope == scope && !claim_scope.trim().is_empty()
                        })
                        && claim.valid_during.is_some_and(|valid| {
                            valid.start <= valid.end
                                && valid.start <= period.start
                                && valid.end >= period.end
                        })
                        && claim.provenance.is_some_and(|proof| proof != ProofId::ZERO)
                        && claim
                            .revision
                            .is_some_and(|revision| revision != ContentHash::ZERO)
                        && claim
                            .evidence
                            .iter()
                            .any(|evidence| !evidence.trim().is_empty())
                })
    }
}

/// An accepted decision pinned into a close.  It is a reference, not a
/// mutable instruction to the recognizer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DecisionReference {
    id: DecisionId,
    answer: String,
    proof: Option<ProofId>,
    relevance: DecisionRelevance,
}

pub type CloseDecision = DecisionReference;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DecisionRelevance {
    Unresolved,
    Applied { fact: OccurrenceId },
    Irrelevant { reason: String },
}

impl DecisionReference {
    pub fn new(id: impl Into<DecisionId>, answer: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            answer: answer.into(),
            proof: None,
            relevance: DecisionRelevance::Unresolved,
        }
    }

    pub fn for_fact(
        id: impl Into<DecisionId>,
        answer: impl Into<String>,
        fact: impl Into<OccurrenceId>,
        proof: ProofId,
    ) -> Self {
        Self {
            id: id.into(),
            answer: answer.into(),
            proof: Some(proof),
            relevance: DecisionRelevance::Applied { fact: fact.into() },
        }
    }

    pub fn for_fact_checked(
        id: impl Into<DecisionId>,
        answer: impl Into<String>,
        fact: impl Into<OccurrenceId>,
        proof: ProofId,
        bundle: &Proof,
    ) -> Result<Self, CloseError> {
        let id = id.into();
        let answer = answer.into();
        let fact = fact.into();
        let decision = Self::for_fact(id, answer, fact, proof);
        check_decision_proof(bundle, &decision)?;
        Ok(decision)
    }

    pub fn irrelevant(
        id: impl Into<DecisionId>,
        answer: impl Into<String>,
        reason: impl Into<String>,
        proof: ProofId,
    ) -> Self {
        Self {
            id: id.into(),
            answer: answer.into(),
            proof: Some(proof),
            relevance: DecisionRelevance::Irrelevant {
                reason: reason.into(),
            },
        }
    }

    pub fn irrelevant_checked(
        id: impl Into<DecisionId>,
        answer: impl Into<String>,
        reason: impl Into<String>,
        proof: ProofId,
        bundle: &Proof,
    ) -> Result<Self, CloseError> {
        let decision = Self::irrelevant(id, answer, reason, proof);
        check_decision_proof(bundle, &decision)?;
        Ok(decision)
    }

    pub fn id(&self) -> &DecisionId {
        &self.id
    }

    pub fn answer(&self) -> &str {
        &self.answer
    }

    pub fn proof(&self) -> Option<ProofId> {
        self.proof
    }

    pub fn relevance(&self) -> &DecisionRelevance {
        &self.relevance
    }

    pub fn with_proof(mut self, proof: ProofId) -> Self {
        self.proof = Some(proof);
        self
    }

    pub fn applied_to(mut self, fact: impl Into<OccurrenceId>) -> Self {
        self.relevance = DecisionRelevance::Applied { fact: fact.into() };
        self
    }

    pub fn mark_irrelevant(mut self, reason: impl Into<String>) -> Self {
        self.relevance = DecisionRelevance::Irrelevant {
            reason: reason.into(),
        };
        self
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CloseException {
    pub code: String,
    pub message: String,
    pub blocking: bool,
    resolved: bool,
}

impl CloseException {
    pub fn new(code: impl Into<String>, message: impl Into<String>, blocking: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            blocking,
            resolved: !blocking,
        }
    }

    pub fn resolved(code: impl Into<String>, message: impl Into<String>, blocking: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            blocking,
            resolved: true,
        }
    }

    pub fn resolve(mut self) -> Self {
        self.resolved = true;
        self
    }

    pub fn is_resolved(&self) -> bool {
        self.resolved
    }
}

/// A non-cryptographic signature record.  Verification belongs to a store or
/// trust layer; retaining the record keeps a historical close reproducible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureRecord {
    pub signer: String,
    pub signature: String,
    pub signed_at: Option<String>,
}

impl SignatureRecord {
    pub fn new(signer: impl Into<String>, signature: impl Into<String>) -> Self {
        Self {
            signer: signer.into(),
            signature: signature.into(),
            signed_at: None,
        }
    }

    pub fn with_timestamp(mut self, signed_at: impl Into<String>) -> Self {
        self.signed_at = Some(signed_at.into());
        self
    }
}

/// All inputs to a close are pinned before recognition is finalized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseRequest {
    pub source_commit: ContentHash,
    pub policies: Vec<BookPolicy>,
    pub period: ReportingPeriod,
    /// Explicit semantic scope covered by the completeness assertions.
    pub scope: String,
    pub completeness: CompletenessClaims,
    pub decisions: Vec<DecisionReference>,
    /// If present, the caller expects this exact root.  It is checked rather
    /// than accepted on trust.
    pub recognized_root: Option<ContentHash>,
    pub exceptions: Vec<CloseException>,
    pub supersedes: Option<ContentHash>,
    pub signatures: Vec<SignatureRecord>,
    /// Independently checked proof bundle for decision references.  When
    /// present, every decision proof must be a member and its semantic
    /// metadata must agree with the decision target and answer.
    pub proof: Option<Proof>,
}

impl CloseRequest {
    pub fn new<I>(source_commit: ContentHash, policies: I, period: ReportingPeriod) -> Self
    where
        I: IntoIterator<Item = BookPolicy>,
    {
        Self {
            source_commit,
            policies: policies.into_iter().collect(),
            period,
            scope: String::new(),
            completeness: CompletenessClaims::default(),
            decisions: Vec::new(),
            recognized_root: None,
            exceptions: Vec::new(),
            supersedes: None,
            signatures: Vec::new(),
            proof: None,
        }
    }

    pub fn new_scoped<I>(
        source_commit: ContentHash,
        policies: I,
        period: ReportingPeriod,
        scope: impl Into<String>,
    ) -> Self
    where
        I: IntoIterator<Item = BookPolicy>,
    {
        Self::new(source_commit, policies, period).with_scope(scope)
    }

    pub fn with_completeness(mut self, completeness: CompletenessClaims) -> Self {
        if !completeness.proof.nodes.is_empty() {
            let proof = self.proof.get_or_insert_with(Proof::new);
            for node in completeness.proof.nodes.values().cloned() {
                proof.insert(node);
            }
            for root in &completeness.proof.roots {
                proof.root(*root);
            }
        }
        self.completeness = completeness;
        self
    }

    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = scope.into();
        self
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn with_decision(mut self, decision: DecisionReference) -> Self {
        self.decisions.push(decision);
        self
    }

    pub fn with_recognized_root(mut self, root: ContentHash) -> Self {
        self.recognized_root = Some(root);
        self
    }

    pub fn with_expected_root(self, root: ContentHash) -> Self {
        self.with_recognized_root(root)
    }

    pub fn with_exception(mut self, exception: CloseException) -> Self {
        self.exceptions.push(exception);
        self
    }

    pub fn superseding(mut self, previous: &CloseResult) -> Self {
        self.supersedes = Some(previous.close_hash);
        self
    }

    pub fn with_supersedes(mut self, previous: ContentHash) -> Self {
        self.supersedes = Some(previous);
        self
    }

    pub fn with_signature(mut self, signature: SignatureRecord) -> Self {
        self.signatures.push(signature);
        self
    }

    pub fn with_proof(mut self, proof: Proof) -> Result<Self, CloseError> {
        proof
            .check()
            .map_err(|error| CloseError::InvalidProofContext(error.to_string()))?;
        self.proof = Some(match self.proof.take() {
            None => proof,
            Some(mut existing) => {
                for node in proof.nodes.into_values() {
                    existing.insert(node);
                }
                for root in proof.roots {
                    existing.root(root);
                }
                existing
            }
        });
        Ok(self)
    }

    pub fn with_proof_bundle(self, proof: Proof) -> Result<Self, CloseError> {
        self.with_proof(proof)
    }
}

fn check_decision_proof(bundle: &Proof, decision: &DecisionReference) -> Result<(), CloseError> {
    let proof = decision
        .proof
        .filter(|proof| *proof != ProofId::ZERO)
        .ok_or_else(|| CloseError::UnverifiedDecision {
            decision: decision.id.clone(),
        })?;
    let node = bundle.check_member(proof).map_err(|error| match error {
        ProofCheckError::MissingNode { .. } => CloseError::UnverifiedDecision {
            decision: decision.id.clone(),
        },
        other => CloseError::InvalidProofContext(other.to_string()),
    })?;
    if !proof_reaches(bundle, proof) {
        return Err(CloseError::UnverifiedDecision {
            decision: decision.id.clone(),
        });
    }
    if !node
        .metadata
        .get("decision-id")
        .is_some_and(|bound| bound == decision.id.as_str())
        || !node
            .metadata
            .get("decision-answer")
            .is_some_and(|bound| bound == &decision.answer)
    {
        return Err(CloseError::UnverifiedDecision {
            decision: decision.id.clone(),
        });
    }
    if !matches!(
        &node.operation,
        Operation::Decision { subject, answer }
            if subject == decision.id.as_str() && answer == &decision.answer
    ) {
        return Err(CloseError::UnverifiedDecision {
            decision: decision.id.clone(),
        });
    }
    match &decision.relevance {
        DecisionRelevance::Applied { fact } => {
            if !node
                .metadata
                .get("decision-fact")
                .or_else(|| node.metadata.get("decision-target"))
                .is_some_and(|bound| bound == fact.as_str())
            {
                return Err(CloseError::UnverifiedDecision {
                    decision: decision.id.clone(),
                });
            }
        }
        DecisionRelevance::Irrelevant { reason } => {
            if !node
                .metadata
                .get("decision-irrelevance")
                .or_else(|| node.metadata.get("decision-reason"))
                .or_else(|| node.metadata.get("decision-relevance"))
                .is_some_and(|bound| bound == reason)
            {
                return Err(CloseError::UnverifiedDecision {
                    decision: decision.id.clone(),
                });
            }
        }
        DecisionRelevance::Unresolved => {
            return Err(CloseError::UnresolvedDecision {
                decision: decision.id.clone(),
            });
        }
    }
    Ok(())
}

fn check_completeness_proofs(
    bundle: &Proof,
    claims: &CompletenessClaims,
) -> Result<(), CloseError> {
    for claim in claims.claims.values().filter(|claim| claim.required) {
        let proof = claim
            .provenance
            .filter(|proof| *proof != ProofId::ZERO)
            .ok_or_else(|| CloseError::UnverifiedCompleteness {
                claim: claim.name.clone(),
            })?;
        let node = bundle
            .check_member(proof)
            .map_err(|_| CloseError::UnverifiedCompleteness {
                claim: claim.name.clone(),
            })?;
        if !proof_reaches(bundle, proof)
            || !matches!(
                &node.operation,
                Operation::Derive { rule } if rule == "completeness"
            )
            || !node
                .metadata
                .get("completeness-name")
                .is_some_and(|value| value == &claim.name)
            || !node
                .metadata
                .get("completeness-source")
                .is_some_and(|value| {
                    claim
                        .source
                        .is_some_and(|source| value == &source.to_string())
                })
            || !node
                .metadata
                .get("completeness-scope")
                .is_some_and(|value| claim.scope.as_ref().is_some_and(|scope| value == scope))
            || !node
                .metadata
                .get("completeness-period")
                .is_some_and(|value| {
                    claim
                        .valid_during
                        .is_some_and(|period| value == &format!("{}..{}", period.start, period.end))
                })
            || !node
                .metadata
                .get("completeness-revision")
                .is_some_and(|value| {
                    claim
                        .revision
                        .is_some_and(|revision| value == &revision.to_string())
                })
        {
            return Err(CloseError::UnverifiedCompleteness {
                claim: claim.name.clone(),
            });
        }
    }
    Ok(())
}

/// An immutable, content-addressed close result.  Restatement creates a new
/// result with `supersedes` set; it never edits the previous result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseResult {
    pub close_hash: ContentHash,
    pub source_commit: ContentHash,
    pub policies: Vec<BookPolicy>,
    pub period: ReportingPeriod,
    pub scope: String,
    pub completeness: CompletenessClaims,
    pub decisions: Vec<DecisionReference>,
    pub recognized_root: ContentHash,
    pub recognized: RecognizedBooks,
    pub exceptions: Vec<CloseException>,
    pub supersedes: Option<ContentHash>,
    pub signatures: Vec<SignatureRecord>,
    pub proof: Option<Proof>,
}

impl CloseResult {
    pub fn id(&self) -> ContentHash {
        self.close_hash
    }

    pub fn close_id(&self) -> ContentHash {
        self.close_hash
    }

    pub fn is_restatement(&self) -> bool {
        self.supersedes.is_some()
    }

    pub fn restates(&self, previous: &CloseResult) -> bool {
        self.supersedes == Some(previous.close_hash)
    }

    pub fn restate(
        &self,
        world: &AcceptedWorld,
        mut request: CloseRequest,
    ) -> Result<Self, CloseError> {
        request.supersedes = Some(self.close_hash);
        close(world, request)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloseError {
    SourceCommitMismatch {
        request: ContentHash,
        world: ContentHash,
    },
    NoPolicies,
    UnverifiedWorld(String),
    Recognition(RecognitionError),
    RecognizedRootMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
    InvalidPeriod {
        start: Date,
        end: Date,
    },
    CompletenessNotSatisfied,
    UnverifiedCompleteness {
        claim: String,
    },
    UnverifiedDecision {
        decision: DecisionId,
    },
    InvalidProofContext(String),
    UnresolvedDecision {
        decision: DecisionId,
    },
    DecisionNotApplicable {
        decision: DecisionId,
        fact: OccurrenceId,
    },
    UnresolvedBlockingException {
        code: String,
    },
}

impl fmt::Display for CloseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceCommitMismatch { request, world } => write!(
                formatter,
                "close pins source commit {request}, but world is {world}"
            ),
            Self::NoPolicies => formatter.write_str("close has no book policies"),
            Self::UnverifiedWorld(reason) => {
                write!(formatter, "close world is unverified: {reason}")
            }
            Self::Recognition(error) => error.fmt(formatter),
            Self::RecognizedRootMismatch { expected, actual } => {
                write!(
                    formatter,
                    "recognized root {actual} does not match expected {expected}"
                )
            }
            Self::InvalidPeriod { start, end } => {
                write!(formatter, "invalid close period {start} through {end}")
            }
            Self::CompletenessNotSatisfied => {
                formatter.write_str("close completeness claims are missing or unsatisfied")
            }
            Self::UnverifiedCompleteness { claim } => {
                write!(
                    formatter,
                    "completeness claim {claim} has no verified proof"
                )
            }
            Self::UnverifiedDecision { decision } => {
                write!(formatter, "decision {decision} has no verified proof")
            }
            Self::InvalidProofContext(reason) => {
                write!(formatter, "invalid close proof context: {reason}")
            }
            Self::UnresolvedDecision { decision } => {
                write!(formatter, "decision {decision} is unresolved")
            }
            Self::DecisionNotApplicable { decision, fact } => {
                write!(
                    formatter,
                    "decision {decision} does not apply to recognized fact {fact}"
                )
            }
            Self::UnresolvedBlockingException { code } => {
                write!(formatter, "blocking close exception {code} is unresolved")
            }
        }
    }
}

impl std::error::Error for CloseError {}

impl From<RecognitionError> for CloseError {
    fn from(error: RecognitionError) -> Self {
        Self::Recognition(error)
    }
}

/// Finalize a close against the exact accepted-world commit named by the
/// request.  This is pure: it returns a new immutable result.
pub fn close(world: &AcceptedWorld, request: CloseRequest) -> Result<CloseResult, CloseError> {
    if request.source_commit != world.source_commit {
        return Err(CloseError::SourceCommitMismatch {
            request: request.source_commit,
            world: world.source_commit,
        });
    }
    if request.policies.is_empty() {
        return Err(CloseError::NoPolicies);
    }
    world
        .check()
        .map_err(|error| CloseError::UnverifiedWorld(error.to_string()))?;
    if request.period.start > request.period.end {
        return Err(CloseError::InvalidPeriod {
            start: request.period.start,
            end: request.period.end,
        });
    }
    if !request
        .completeness
        .is_satisfied_for(world.source_commit, request.scope(), request.period)
    {
        return Err(CloseError::CompletenessNotSatisfied);
    }
    let proof = request
        .proof
        .as_ref()
        .ok_or_else(|| CloseError::UnverifiedCompleteness {
            claim: request.scope.clone(),
        })?;
    proof
        .check()
        .map_err(|error| CloseError::InvalidProofContext(error.to_string()))?;
    check_completeness_proofs(proof, &request.completeness)?;
    for exception in &request.exceptions {
        if exception.blocking && !exception.is_resolved() {
            return Err(CloseError::UnresolvedBlockingException {
                code: exception.code.clone(),
            });
        }
    }
    for decision in &request.decisions {
        if decision.id.is_empty() || decision.answer.trim().is_empty() {
            return Err(CloseError::UnverifiedDecision {
                decision: decision.id.clone(),
            });
        }
        match &decision.relevance {
            DecisionRelevance::Unresolved => {
                return Err(CloseError::UnresolvedDecision {
                    decision: decision.id.clone(),
                });
            }
            DecisionRelevance::Applied { fact } if fact.is_empty() => {
                return Err(CloseError::UnresolvedDecision {
                    decision: decision.id.clone(),
                });
            }
            DecisionRelevance::Irrelevant { reason } if reason.trim().is_empty() => {
                return Err(CloseError::UnresolvedDecision {
                    decision: decision.id.clone(),
                });
            }
            DecisionRelevance::Applied { .. } | DecisionRelevance::Irrelevant { .. } => {}
        }
        let bundle = request
            .proof
            .as_ref()
            .ok_or_else(|| CloseError::UnverifiedDecision {
                decision: decision.id.clone(),
            })?;
        check_decision_proof(bundle, decision)?;
    }

    let recognized = recognize_books_in_period(world, &request.policies, request.period)?;
    for decision in &request.decisions {
        if let DecisionRelevance::Applied { fact } = &decision.relevance
            && !recognized
                .books
                .values()
                .any(|book| book.fact(fact).is_some())
        {
            return Err(CloseError::DecisionNotApplicable {
                decision: decision.id.clone(),
                fact: fact.clone(),
            });
        }
    }

    let recognized = recognized.with_decisions(request.decisions.clone());
    let recognized_root = recognized.root();
    if let Some(expected) = request.recognized_root
        && expected != recognized_root
    {
        return Err(CloseError::RecognizedRootMismatch {
            expected,
            actual: recognized_root,
        });
    }
    let close_hash = close_hash(&request, recognized_root);
    Ok(CloseResult {
        close_hash,
        source_commit: request.source_commit,
        policies: request.policies,
        period: request.period,
        scope: request.scope,
        completeness: request.completeness,
        decisions: request.decisions,
        recognized_root,
        recognized,
        exceptions: request.exceptions,
        supersedes: request.supersedes,
        signatures: request.signatures,
        proof: request.proof,
    })
}

pub fn finalize_close(
    world: &AcceptedWorld,
    request: CloseRequest,
) -> Result<CloseResult, CloseError> {
    close(world, request)
}

pub fn restate_close(
    previous: &CloseResult,
    world: &AcceptedWorld,
    request: CloseRequest,
) -> Result<CloseResult, CloseError> {
    previous.restate(world, request)
}

fn world_root(
    source_commit: ContentHash,
    facts: &[RecognitionAcceptedFact],
    proof: Option<&Proof>,
) -> ContentHash {
    let mut bytes = Vec::new();
    // The same fact set in two accepted commits is not the same world.  Bind
    // the source commit before encoding facts so provenance cannot be
    // detached by reconstructing an equivalent vector.
    put_hash(&mut bytes, source_commit);
    if let Some(proof) = proof {
        put_hash(&mut bytes, ContentHash::from_bytes(proof.content_hash().0));
    }
    for fact in facts {
        put_string(&mut bytes, fact.id.as_str());
        put_string(&mut bytes, &fact.kind);
        put_date(&mut bytes, fact.occurrence_date);
        match fact.settlement_date {
            Some(date) => {
                bytes.push(1);
                put_date(&mut bytes, date);
            }
            None => bytes.push(0),
        }
        match &fact.scope {
            FactScope::Actual => bytes.push(0),
            FactScope::Scenario(scenario) => {
                bytes.push(1);
                put_string(&mut bytes, scenario.as_str());
            }
        }
        put_proof_id(&mut bytes, fact.proof);
        put_string(&mut bytes, &fact.authority);
        for (key, value) in &fact.attributes {
            put_string(&mut bytes, key);
            put_string(&mut bytes, value);
        }
    }
    ContentHash::domain_separated("axiom/accepted-world", &bytes)
}

fn fact_proof(
    accepted_world_root: ContentHash,
    policy: &BookPolicy,
    fact: &RecognitionAcceptedFact,
    date: Date,
) -> ProofId {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, accepted_world_root);
    put_hash(&mut bytes, policy.hash);
    put_string(&mut bytes, policy.id.as_str());
    put_string(&mut bytes, fact.id.as_str());
    put_date(&mut bytes, date);
    put_proof_id(&mut bytes, fact.proof);
    put_string(&mut bytes, &fact.authority);
    ContentHash::domain_separated("axiom/recognition/proof", &bytes).into()
}

fn facts_root(
    accepted_world_root: ContentHash,
    policy: &BookPolicy,
    facts: &[BookRecognizedFact],
) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, accepted_world_root);
    bytes.extend(policy.canonical_bytes(true));
    for fact in facts {
        put_string(&mut bytes, fact.source_fact.as_str());
        put_string(&mut bytes, &fact.kind);
        put_date(&mut bytes, fact.date);
        put_string(&mut bytes, fact.classification.as_deref().unwrap_or(""));
        put_string(&mut bytes, fact.basis.as_deref().unwrap_or(""));
        put_string(&mut bytes, fact.valuation.as_deref().unwrap_or(""));
        for (key, value) in &fact.attributes {
            put_string(&mut bytes, key);
            put_string(&mut bytes, value);
        }
        put_proof_id(&mut bytes, fact.proof.recognition_proof);
    }
    ContentHash::domain_separated("axiom/recognized-facts", &bytes)
}

fn books_root(
    books: &BTreeMap<BookId, RecognizedFacts>,
    decisions: &[DecisionReference],
) -> ContentHash {
    let mut bytes = Vec::new();
    for (book, facts) in books {
        put_string(&mut bytes, book.as_str());
        put_hash(&mut bytes, facts.policy_hash);
        put_hash(&mut bytes, facts.recognized_root);
    }
    let mut decisions = decisions.iter().collect::<Vec<_>>();
    decisions.sort();
    for decision in decisions {
        put_decision(&mut bytes, decision);
    }
    ContentHash::domain_separated("axiom/recognized-books", &bytes)
}

fn close_hash(request: &CloseRequest, recognized_root: ContentHash) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, request.source_commit);
    put_string(&mut bytes, &request.scope);
    let mut policies = request.policies.iter().collect::<Vec<_>>();
    policies.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.hash.cmp(&right.hash))
    });
    for policy in policies {
        bytes.extend(policy.canonical_bytes(true));
    }
    put_date(&mut bytes, request.period.start);
    put_date(&mut bytes, request.period.end);
    for (name, claim) in &request.completeness.claims {
        put_string(&mut bytes, name);
        bytes.push(u8::from(claim.complete));
        bytes.push(u8::from(claim.required));
        match claim.source {
            Some(source) => {
                bytes.push(1);
                put_hash(&mut bytes, source);
            }
            None => bytes.push(0),
        }
        match &claim.scope {
            Some(scope) => {
                bytes.push(1);
                put_string(&mut bytes, scope);
            }
            None => bytes.push(0),
        }
        match claim.valid_during {
            Some(period) => {
                bytes.push(1);
                put_date(&mut bytes, period.start);
                put_date(&mut bytes, period.end);
            }
            None => bytes.push(0),
        }
        match claim.provenance {
            Some(provenance) => {
                bytes.push(1);
                put_proof_id(&mut bytes, provenance);
            }
            None => bytes.push(0),
        }
        match claim.revision {
            Some(revision) => {
                bytes.push(1);
                put_hash(&mut bytes, revision);
            }
            None => bytes.push(0),
        }
        let mut evidence = claim.evidence.iter().collect::<Vec<_>>();
        evidence.sort();
        for evidence in evidence {
            put_string(&mut bytes, evidence);
        }
    }
    let mut decisions = request.decisions.iter().collect::<Vec<_>>();
    decisions.sort();
    for decision in decisions {
        put_decision(&mut bytes, decision);
    }
    put_hash(&mut bytes, recognized_root);
    if let Some(proof) = &request.proof {
        bytes.push(1);
        put_hash(&mut bytes, ContentHash::from_bytes(proof.content_hash().0));
    } else {
        bytes.push(0);
    }
    let mut exceptions = request.exceptions.iter().collect::<Vec<_>>();
    exceptions.sort();
    for exception in exceptions {
        put_string(&mut bytes, &exception.code);
        put_string(&mut bytes, &exception.message);
        bytes.push(u8::from(exception.blocking));
        bytes.push(u8::from(exception.resolved));
    }
    match request.supersedes {
        Some(previous) => {
            bytes.push(1);
            put_hash(&mut bytes, previous);
        }
        None => bytes.push(0),
    }
    ContentHash::domain_separated("axiom/close", &bytes)
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    output.extend((value.len() as u64).to_be_bytes());
    output.extend(value.as_bytes());
}

fn put_hash(output: &mut Vec<u8>, hash: ContentHash) {
    output.extend(hash.as_bytes());
}

fn put_decision(output: &mut Vec<u8>, decision: &DecisionReference) {
    put_string(output, decision.id.as_str());
    put_string(output, &decision.answer);
    match &decision.relevance {
        DecisionRelevance::Unresolved => output.push(0),
        DecisionRelevance::Applied { fact } => {
            output.push(1);
            put_string(output, fact.as_str());
        }
        DecisionRelevance::Irrelevant { reason } => {
            output.push(2);
            put_string(output, reason);
        }
    }
    match decision.proof {
        Some(proof) => {
            output.push(1);
            put_proof_id(output, proof);
        }
        None => output.push(0),
    }
}

fn put_proof_id(output: &mut Vec<u8>, proof: ProofId) {
    output.extend(proof.0);
}

fn put_date(output: &mut Vec<u8>, date: Date) {
    output.extend(date.year.to_be_bytes());
    output.push(date.month);
    output.push(date.day);
}

impl From<ContentHash> for ProofId {
    fn from(hash: ContentHash) -> Self {
        Self(*hash.as_bytes())
    }
}

// Keep the book marker visible to downstream users that want a typed wrapper
// around a recognized result without imposing a particular book enum.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizedFor<Book> {
    pub facts: RecognizedFacts,
    marker: PhantomData<Book>,
}

impl<Book> RecognizedFor<Book> {
    pub fn new(facts: RecognizedFacts) -> Self {
        Self {
            facts,
            marker: PhantomData,
        }
    }

    pub fn as_facts(&self) -> &RecognizedFacts {
        &self.facts
    }

    pub fn into_facts(self) -> RecognizedFacts {
        self.facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof::{Node, Operation};

    fn date(value: &str) -> Date {
        value.parse().expect("test date is valid")
    }

    fn proof(label: &str) -> ProofId {
        ContentHash::domain_separated("test/proof", label.as_bytes()).into()
    }

    fn world_with_fact(fact: RecognitionAcceptedFact) -> (ContentHash, AcceptedWorld) {
        let commit =
            ContentHash::domain_separated("test/accepted-world", fact.id().as_str().as_bytes());
        let (fact, bundle) = checked_fact(fact);
        let world = AcceptedWorld::new(commit)
            .with_proof(bundle)
            .expect("checked proof bundle")
            .with_fact(fact)
            .expect("unique accepted fact");
        (commit, world)
    }

    fn checked_fact(fact: RecognitionAcceptedFact) -> (RecognitionAcceptedFact, Proof) {
        let mut metadata = BTreeMap::new();
        metadata.insert("accepted-fact".into(), fact.id().to_string());
        let node = Node::new(
            format!("accepted fact {}", fact.id()),
            Operation::Observation {
                source: fact.id().to_string(),
            },
            vec![],
            metadata,
        );
        let proof_id = node.id;
        let checked = match fact.scope() {
            FactScope::Actual => RecognitionAcceptedFact::actual(
                fact.id().clone(),
                fact.kind().to_string(),
                fact.occurrence_date(),
                proof_id,
                fact.authority().to_string(),
            ),
            FactScope::Scenario(scenario) => RecognitionAcceptedFact::scenario(
                fact.id().clone(),
                fact.kind().to_string(),
                fact.occurrence_date(),
                scenario.to_string(),
                proof_id,
                fact.authority().to_string(),
            ),
        }
        .expect("test fact is valid")
        .with_attributes(fact.attributes().clone());
        let checked = match fact.settlement_date() {
            Some(date) => checked.with_settlement_date(date),
            None => checked,
        };
        let mut bundle = Proof::new();
        bundle.insert(node);
        bundle.root(proof_id);
        (checked, bundle)
    }

    fn world_from_facts(
        source_commit: ContentHash,
        facts: impl IntoIterator<Item = RecognitionAcceptedFact>,
    ) -> AcceptedWorld {
        let mut checked_facts = Vec::new();
        let mut bundle = Proof::new();
        for fact in facts {
            let (fact, fact_bundle) = checked_fact(fact);
            checked_facts.push(fact);
            for node in fact_bundle.nodes.into_values() {
                let id = bundle.insert(node);
                bundle.root(id);
            }
        }
        AcceptedWorld::from_facts_checked(source_commit, checked_facts, bundle)
            .expect("checked accepted world")
    }

    fn checked_decision_for_fact(id: &str, answer: &str, fact: &str) -> (DecisionReference, Proof) {
        let mut metadata = BTreeMap::new();
        metadata.insert("decision-id".into(), id.into());
        metadata.insert("decision-answer".into(), answer.into());
        metadata.insert("decision-fact".into(), fact.into());
        let node = Node::new(
            format!("decision {id}"),
            Operation::Decision {
                subject: id.into(),
                answer: answer.into(),
            },
            vec![],
            metadata,
        );
        let mut bundle = Proof::new();
        let proof_id = bundle.insert(node);
        bundle.root(proof_id);
        (
            DecisionReference::for_fact(id, answer, fact, proof_id),
            bundle,
        )
    }

    fn checked_irrelevant_decision(
        id: &str,
        answer: &str,
        reason: &str,
    ) -> (DecisionReference, Proof) {
        let mut metadata = BTreeMap::new();
        metadata.insert("decision-id".into(), id.into());
        metadata.insert("decision-answer".into(), answer.into());
        metadata.insert("decision-irrelevance".into(), reason.into());
        let node = Node::new(
            format!("decision {id}"),
            Operation::Decision {
                subject: id.into(),
                answer: answer.into(),
            },
            vec![],
            metadata,
        );
        let mut bundle = Proof::new();
        let proof_id = bundle.insert(node);
        bundle.root(proof_id);
        (
            DecisionReference::irrelevant(id, answer, reason, proof_id),
            bundle,
        )
    }

    fn merge_proof(mut target: Proof, addition: Proof) -> Proof {
        for node in addition.nodes.into_values() {
            target.insert(node);
        }
        for root in addition.roots {
            target.root(root);
        }
        target
    }

    fn policy(book: &str) -> BookPolicy {
        BookPolicy::from_definition(book, date("2026-01-01"), Some(date("2026-12-31")))
    }

    fn complete_for(source: ContentHash, period: ReportingPeriod) -> CompletenessClaims {
        CompletenessClaims::new()
            .claim_scoped(
                "accepted-source",
                true,
                source,
                "accepted-source",
                period,
                proof("completeness/accepted-source"),
                ContentHash::domain_separated("test/revision", b"accepted-source"),
            )
            .with_evidence("accepted-source", "test-evidence")
    }

    #[allow(dead_code)]
    fn complete() -> CompletenessClaims {
        CompletenessClaims::new().claim("accepted-source", true)
    }

    #[test]
    fn unscoped_completeness_cannot_satisfy_a_scoped_close() {
        let source = ContentHash::domain_separated("test/source", b"unscoped");
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));
        let claims = CompletenessClaims::new()
            .claim("accepted-source", true)
            .with_evidence("accepted-source", "bank-feed");
        assert!(claims.is_satisfied());
        assert!(!claims.is_satisfied_for(source, "accepted-source", period));
    }

    #[test]
    fn one_accepted_fact_can_differ_by_book_without_duplication() {
        let fact = RecognitionAcceptedFact::actual(
            "event/one",
            "cash-receipt",
            date("2026-01-01"),
            proof("event/one"),
            "test-authority",
        )
        .unwrap()
        .with_settlement_date(date("2026-01-05"));
        let (commit, world) = world_with_fact(fact);
        let cash = policy("cash")
            .with_date_basis(DateBasis::Settlement)
            .with_classification("realized-cash");
        let accrual = policy("accrual")
            .with_date_basis(DateBasis::Occurrence)
            .with_classification("earned-accrual");

        let books = recognize_books(&world, &[cash, accrual]).expect("both books recognize");
        assert_eq!(world.facts().len(), 1);
        assert_eq!(books.books.len(), 2);

        let cash_fact = &books.get(&BookId::new("cash")).unwrap().facts[0];
        let accrual_fact = &books.get(&BookId::new("accrual")).unwrap().facts[0];
        assert_eq!(cash_fact.source_fact, accrual_fact.source_fact);
        assert_eq!(cash_fact.proof.accepted_commit, commit);
        assert_eq!(cash_fact.date, date("2026-01-05"));
        assert_eq!(accrual_fact.date, date("2026-01-01"));
        assert_ne!(cash_fact.classification, accrual_fact.classification);
        assert_ne!(
            cash_fact.proof.recognition_proof,
            accrual_fact.proof.recognition_proof
        );
    }

    #[test]
    fn scenario_fact_is_rejected_from_actual_book() {
        let (_, world) = world_with_fact(
            RecognitionAcceptedFact::scenario(
                "forecast/one",
                "planned-receipt",
                date("2026-01-01"),
                "scenario/base",
                proof("forecast/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let error = recognize(&world, &policy("cash")).expect_err("scenario must not be actual");
        assert!(matches!(
            error,
            RecognitionError::ScenarioFact {
                book,
                fact,
                scenario,
            }
                if book == BookId::new("cash") && fact == OccurrenceId::new("forecast/one")
                    && scenario == ExternalId::new("scenario/base")
        ));
    }

    #[test]
    fn changing_policy_hash_changes_recognition_root() {
        let (_, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let original = recognize(&world, &policy("tax")).expect("original policy recognizes");
        let revised = recognize(&world, &policy("tax").with_valuation("fair-value-at-close"))
            .expect("revised policy recognizes");

        assert_ne!(original.policy_hash, revised.policy_hash);
        assert_ne!(original.recognized_root, revised.recognized_root);
    }

    #[test]
    fn close_restatement_supersedes_without_mutating_previous_close() {
        let (commit, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));
        let first_request = CloseRequest::new(commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(commit, period));
        let first = close(&world, first_request).expect("initial close");
        let first_id = first.close_id();
        let first_root = first.recognized_root;

        let second_request = CloseRequest::new(commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(commit, period))
            .with_exception(CloseException::new("late-evidence", "late evidence", false));
        let second = restate_close(&first, &world, second_request).expect("restatement");

        assert_eq!(first.supersedes, None);
        assert_eq!(first.close_id(), first_id);
        assert_eq!(first.recognized_root, first_root);
        assert_eq!(second.supersedes, Some(first_id));
        assert!(second.is_restatement());
        assert_ne!(second.close_id(), first_id);
        assert_eq!(second.recognized_root, first_root);
    }

    #[test]
    fn close_excludes_facts_outside_period_using_selected_date() {
        let in_period = RecognitionAcceptedFact::actual(
            "event/in-period",
            "receipt",
            date("2026-01-01"),
            proof("event/in-period"),
            "test-authority",
        )
        .unwrap()
        .with_settlement_date(date("2026-01-10"));
        let out_of_period = RecognitionAcceptedFact::actual(
            "event/out-of-period",
            "receipt",
            date("2026-01-02"),
            proof("event/out-of-period"),
            "test-authority",
        )
        .unwrap()
        .with_settlement_date(date("2026-02-01"));
        let source_commit = ContentHash::domain_separated("test/source", b"period");
        let world = world_from_facts(source_commit, [in_period, out_of_period]);
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-01-31"));
        let request = CloseRequest::new(
            source_commit,
            [policy("cash").with_date_basis(DateBasis::Settlement)],
            period,
        )
        .with_scope("accepted-source")
        .with_completeness(complete_for(source_commit, period));

        let result = close(&world, request).expect("period close");
        let facts = &result.recognized.get(&BookId::new("cash")).unwrap().facts;
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].source_fact, OccurrenceId::new("event/in-period"));
        assert_eq!(facts[0].date, date("2026-01-10"));
    }

    #[test]
    fn world_root_is_content_derived_and_updates_without_mutation() {
        let source_commit = ContentHash::domain_separated("test/source", b"stable");
        let empty = AcceptedWorld::new_checked(source_commit, Proof::new()).unwrap();
        let first_fact = RecognitionAcceptedFact::actual(
            "event/one",
            "receipt",
            date("2026-01-01"),
            proof("event/one"),
            "test-authority",
        )
        .unwrap();
        let (first_fact, first_bundle) = checked_fact(first_fact);
        let one_fact = empty
            .clone()
            .with_proof(first_bundle)
            .unwrap()
            .with_fact(first_fact)
            .expect("first fact");
        let second_fact = RecognitionAcceptedFact::actual(
            "event/two",
            "receipt",
            date("2026-01-02"),
            proof("event/two"),
            "test-authority",
        )
        .unwrap();
        let (second_fact, second_bundle) = checked_fact(second_fact);
        let mut combined_bundle = one_fact.proof_bundle().unwrap().clone();
        combined_bundle.nodes.extend(second_bundle.nodes);
        combined_bundle.roots.extend(second_bundle.roots);
        combined_bundle.roots.sort();
        combined_bundle.roots.dedup();
        let two_facts = one_fact
            .clone()
            .with_proof(combined_bundle)
            .expect("combined proof bundle")
            .with_fact(second_fact)
            .expect("second fact");

        assert_eq!(empty.source_commit(), source_commit);
        assert_eq!(one_fact.source_commit(), source_commit);
        assert_ne!(empty.world_root(), one_fact.world_root());
        assert_ne!(one_fact.world_root(), two_facts.world_root());
        let other_source = ContentHash::domain_separated("test/source", b"other");
        let same_facts_other_source =
            AcceptedWorld::new_checked(other_source, one_fact.proof_bundle().unwrap().clone())
                .unwrap()
                .with_fact(one_fact.facts()[0].clone())
                .expect("fact in other world");
        assert_ne!(
            one_fact.source_commit(),
            same_facts_other_source.source_commit()
        );
        assert_ne!(one_fact.world_root(), same_facts_other_source.world_root());
        assert!(empty.facts().is_empty());
        assert_eq!(one_fact.facts().len(), 1);
        assert_eq!(two_facts.facts().len(), 2);
        assert!(matches!(
            one_fact.with_fact(one_fact.facts()[0].clone()),
            Err(WorldError::DuplicateFact { .. })
        ));
    }

    #[test]
    fn accepted_fact_requires_nonzero_proof_and_authority() {
        assert!(matches!(
            FactScope::scenario("   "),
            Err(RecognitionFactError::MissingScenarioIdentity)
        ));

        let missing_proof = RecognitionAcceptedFact::actual(
            "event/one",
            "receipt",
            date("2026-01-01"),
            ProofId::ZERO,
            "test-authority",
        )
        .expect_err("an accepted fact needs a proof");
        assert!(matches!(
            missing_proof,
            RecognitionFactError::MissingProof { .. }
        ));

        let missing_authority = RecognitionAcceptedFact::actual(
            "event/one",
            "receipt",
            date("2026-01-01"),
            proof("event/one"),
            "   ",
        )
        .expect_err("an accepted fact needs an authority");
        assert!(matches!(
            missing_authority,
            RecognitionFactError::MissingAuthority { .. }
        ));
    }

    #[test]
    fn canonical_hashes_distinguish_variants_sort_close_sets_and_exclude_signatures() {
        let occurrence = policy("tax");
        let attribute = policy("tax").with_date_basis(DateBasis::Attribute("occurrence".into()));
        assert_ne!(occurrence.content_hash(), attribute.content_hash());

        let source_commit = ContentHash::domain_separated("test/source", b"canonical-close");
        let fact = RecognitionAcceptedFact::actual(
            "event/one",
            "receipt",
            date("2026-01-01"),
            proof("event/one"),
            "test-authority",
        )
        .unwrap();
        let (fact, fact_bundle) = checked_fact(fact);
        let world = AcceptedWorld::new_checked(source_commit, fact_bundle)
            .unwrap()
            .with_fact(fact)
            .unwrap();
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));
        let (decision_b, proof_b) = checked_decision_for_fact("decision/b", "b", "event/one");
        let (decision_a, proof_a) = checked_decision_for_fact("decision/a", "a", "event/one");
        let decision_proof = merge_proof(proof_b, proof_a);
        let first = close(
            &world,
            CloseRequest::new(source_commit, [occurrence.clone()], period)
                .with_scope("accepted-source")
                .with_completeness(complete_for(source_commit, period))
                .with_decision(decision_b)
                .with_decision(decision_a)
                .with_exception(CloseException::new("z", "z", false))
                .with_exception(CloseException::new("a", "a", false))
                .with_proof(decision_proof.clone())
                .unwrap(),
        )
        .unwrap();
        let (decision_a, proof_a) = checked_decision_for_fact("decision/a", "a", "event/one");
        let (decision_b, proof_b) = checked_decision_for_fact("decision/b", "b", "event/one");
        let second = close(
            &world,
            CloseRequest::new(source_commit, [occurrence], period)
                .with_scope("accepted-source")
                .with_completeness(complete_for(source_commit, period))
                .with_decision(decision_a)
                .with_decision(decision_b)
                .with_exception(CloseException::new("a", "a", false))
                .with_exception(CloseException::new("z", "z", false))
                .with_signature(SignatureRecord::new("reviewer", "opaque-signature"))
                .with_proof(merge_proof(proof_a, proof_b))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(first.close_id(), second.close_id());
    }

    #[test]
    fn close_rejects_missing_completeness() {
        let (source_commit, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let request = CloseRequest::new(
            source_commit,
            [policy("tax")],
            ReportingPeriod::new(date("2026-01-01"), date("2026-12-31")),
        );
        assert!(matches!(
            close(&world, request),
            Err(CloseError::CompletenessNotSatisfied)
        ));

        let request = CloseRequest::new(
            source_commit,
            [policy("tax")],
            ReportingPeriod::new(date("2026-01-01"), date("2026-12-31")),
        )
        .with_completeness(CompletenessClaims::new().claim("", true));
        assert!(matches!(
            close(&world, request),
            Err(CloseError::CompletenessNotSatisfied)
        ));

        let request = CloseRequest::new(
            source_commit,
            [policy("tax")],
            ReportingPeriod::new(date("2026-01-01"), date("2026-12-31")),
        )
        .with_completeness(CompletenessClaims::new().claim("accepted-source", false));
        assert!(matches!(
            close(&world, request),
            Err(CloseError::CompletenessNotSatisfied)
        ));
    }

    #[test]
    fn close_rejects_unverified_or_unresolved_decisions() {
        let (source_commit, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));
        let unresolved = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(DecisionReference::new("decision/unresolved", "answer"));
        assert!(matches!(
            close(&world, unresolved),
            Err(CloseError::UnresolvedDecision { .. })
        ));

        let unscoped = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(DecisionReference::irrelevant(
                "decision/unscoped",
                "answer",
                "outside this book",
                proof("decision/unscoped"),
            ));
        assert!(matches!(
            close(&world, unscoped),
            Err(CloseError::UnverifiedDecision { .. })
        ));

        let (irrelevant_decision, irrelevant_proof) = checked_irrelevant_decision(
            "decision/irrelevant",
            "answer",
            "verified outside this book",
        );
        let irrelevant = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(irrelevant_decision)
            .with_proof(irrelevant_proof)
            .unwrap();
        assert!(close(&world, irrelevant).is_ok());

        let empty_irrelevance = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(DecisionReference::irrelevant(
                "decision/empty-reason",
                "answer",
                "",
                proof("decision/empty-reason"),
            ));
        assert!(matches!(
            close(&world, empty_irrelevance),
            Err(CloseError::UnresolvedDecision { .. })
        ));
    }

    #[test]
    fn close_rejects_forged_decision_operation_and_completeness_certificate() {
        let (source_commit, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));

        let mut missing_certificate = complete_for(source_commit, period);
        missing_certificate.proof = Proof::new();
        let request = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(missing_certificate);
        assert!(matches!(
            close(&world, request),
            Err(CloseError::UnverifiedCompleteness { .. })
        ));

        let id = "decision/forged-operation";
        let answer = "answer";
        let reason = "outside this book";
        let node = Node::new(
            "metadata forgery",
            Operation::Observation {
                source: "attacker".into(),
            },
            vec![],
            BTreeMap::from([
                ("decision-id".into(), id.into()),
                ("decision-answer".into(), answer.into()),
                ("decision-irrelevance".into(), reason.into()),
            ]),
        );
        let mut forged = Proof::new();
        let forged_id = forged.insert(node);
        forged.root(forged_id);
        let request = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(DecisionReference::irrelevant(id, answer, reason, forged_id))
            .with_proof(forged)
            .unwrap();
        assert!(matches!(
            close(&world, request),
            Err(CloseError::UnverifiedDecision { .. })
        ));
    }

    #[test]
    fn close_rejects_decision_without_recognized_target_and_blocking_exception() {
        let (source_commit, world) = world_with_fact(
            RecognitionAcceptedFact::actual(
                "event/one",
                "receipt",
                date("2026-01-01"),
                proof("event/one"),
                "test-authority",
            )
            .unwrap(),
        );
        let period = ReportingPeriod::new(date("2026-01-01"), date("2026-12-31"));
        let (wrong_target_decision, wrong_target_proof) =
            checked_decision_for_fact("decision/wrong", "answer", "event/missing");
        let wrong_target = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_decision(wrong_target_decision)
            .with_proof(wrong_target_proof)
            .unwrap();
        assert!(matches!(
            close(&world, wrong_target),
            Err(CloseError::DecisionNotApplicable { .. })
        ));

        let unresolved_exception = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_exception(CloseException::new("blocking-hole", "needs review", true));
        assert!(matches!(
            close(&world, unresolved_exception),
            Err(CloseError::UnresolvedBlockingException { .. })
        ));

        let resolved_exception = CloseRequest::new(source_commit, [policy("tax")], period)
            .with_scope("accepted-source")
            .with_completeness(complete_for(source_commit, period))
            .with_exception(CloseException::new("blocking-hole", "needs review", true).resolve());
        assert!(close(&world, resolved_exception).is_ok());
    }
}
