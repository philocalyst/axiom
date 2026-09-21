//! Immutable, content-addressed semantic objects.
//!
//! The store is intentionally less clever than the semantic engine.  It does
//! not decide which observation is true or which policy should win.  It only
//! gives normalized objects stable identities, keeps their bytes immutable,
//! and provides the small amount of three-way merge policy needed to keep
//! collaboration honest.
//!
//! Every object is hashed from deterministic bytes.  Occurrence identity lives
//! in the object payload; it is therefore never accidentally collapsed merely
//! because two observations have equal contents.  The in-memory implementation
//! is a reference implementation for a future append-only or packed store.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use crate::model::{BookId, ContentHash, Date, ExternalId};
use crate::package_compiler::CompiledArtifact;
use crate::proof::{
    Node as CanonicalNode, Operation as CanonicalOperation, Proof as CanonicalProof,
    ProofId as CanonicalProofId,
};

const SCHEMA_VERSION: &str = "axiom/store/v3";
pub(crate) const ANALYSIS_AUTHOR: &str = "workspace/analysis";
const EVIDENCE_CONTENT_DOMAIN: &str = "axiom/store/evidence-content/v1";

/// The object families that may be addressed by this store.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObjectKind {
    Evidence,
    Statement,
    Decision,
    Completeness,
    Package,
    CompiledArtifact,
    AnalysisArtifact,
    Proof,
    Conflict,
    Commit,
    Close,
}

impl ObjectKind {
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Statement => "statement",
            Self::Decision => "decision",
            Self::Completeness => "completeness",
            Self::Package => "package",
            Self::CompiledArtifact => "compiled-artifact",
            Self::AnalysisArtifact => "analysis-artifact",
            Self::Proof => "proof",
            Self::Conflict => "conflict",
            Self::Commit => "commit",
            Self::Close => "close",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// A type-level marker for one object family.
pub trait Kind: Copy + Clone + Eq + Ord + std::hash::Hash + fmt::Debug + 'static {
    const OBJECT_KIND: ObjectKind;
}

macro_rules! kind_marker {
    ($name:ident, $kind:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name;

        impl Kind for $name {
            const OBJECT_KIND: ObjectKind = ObjectKind::$kind;
        }
    };
}

kind_marker!(EvidenceKind, Evidence);
kind_marker!(StatementKind, Statement);
kind_marker!(DecisionKind, Decision);
kind_marker!(CompletenessKind, Completeness);
kind_marker!(PackageKind, Package);
kind_marker!(CompiledArtifactKind, CompiledArtifact);
kind_marker!(AnalysisArtifactKind, AnalysisArtifact);
kind_marker!(ProofKind, Proof);
kind_marker!(ConflictKind, Conflict);
kind_marker!(CommitKind, Commit);
kind_marker!(CloseKind, Close);

/// A typed content address.  The type parameter prevents a statement address
/// from being passed where a policy package address is required.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectId<K: Kind> {
    hash: ContentHash,
    marker: PhantomData<K>,
}

impl<K: Kind> ObjectId<K> {
    pub const fn new(hash: ContentHash) -> Self {
        Self {
            hash,
            marker: PhantomData,
        }
    }

    pub const fn hash(self) -> ContentHash {
        self.hash
    }

    pub const fn kind(self) -> ObjectKind {
        K::OBJECT_KIND
    }
}

impl<K: Kind> From<ObjectId<K>> for ContentHash {
    fn from(value: ObjectId<K>) -> Self {
        value.hash
    }
}

impl<K: Kind> fmt::Display for ObjectId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.hash.fmt(f)
    }
}

pub type EvidenceId = ObjectId<EvidenceKind>;
pub type StatementId = ObjectId<StatementKind>;
pub type DecisionId = ObjectId<DecisionKind>;
pub type CompletenessId = ObjectId<CompletenessKind>;
pub type PackageId = ObjectId<PackageKind>;
/// The store address of a deterministic compiled package artifact.
pub type CompiledArtifactId = ObjectId<CompiledArtifactKind>;
/// The store address of a sealed, workspace-produced analysis artifact.  The
/// artifact is the only authority a close may cite; unlike a generic proof it
/// carries the complete source/analysis/package/result binding.
pub type AnalysisArtifactId = ObjectId<AnalysisArtifactKind>;
/// The store address of a persisted proof object.  This is deliberately
/// distinct from [`crate::proof::ProofId`], which is the identity of a proof
/// node inside the canonical DAG.  A stored object may contain many nodes and
/// is addressed by the store's typed object hash, while all semantic edges
/// continue to use the canonical proof ID.
pub type ProofObjectId = ObjectId<ProofKind>;
pub type ConflictId = ObjectId<ConflictKind>;
pub type CommitId = ObjectId<CommitKind>;
pub type CloseId = ObjectId<CloseKind>;

/// Lifecycle state for an evidence object.  None of these variants delete
/// historical identity: a correction or tombstone is itself new evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EvidenceState {
    Present,
    Correction {
        supersedes: EvidenceId,
        scope: String,
        reason: String,
        authority: String,
    },
    Tombstone {
        supersedes: EvidenceId,
        reason: String,
        authority: String,
    },
    Unavailable {
        reason: String,
    },
    Redacted {
        reason: String,
    },
}

impl EvidenceState {
    /// Return the immutable object this state supersedes, when it is a
    /// correction/tombstone.  The target is intentionally an object address,
    /// not an occurrence string: a correction must remain auditable even when
    /// another adapter emitted the same occurrence later.
    pub fn supersedes(&self) -> Option<EvidenceId> {
        match self {
            Self::Correction { supersedes, .. } | Self::Tombstone { supersedes, .. } => {
                Some(*supersedes)
            }
            Self::Present | Self::Unavailable { .. } | Self::Redacted { .. } => None,
        }
    }

    fn target(&self) -> Option<EvidenceId> {
        self.supersedes()
    }

    pub const fn is_tombstone(&self) -> bool {
        matches!(self, Self::Tombstone { .. })
    }

    pub const fn is_deleted(&self) -> bool {
        self.is_tombstone()
    }

    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Present | Self::Correction { .. })
    }
}

/// An immutable normalized observation.  Occurrence, normalized content, and
/// external identity are separate fields: equal rows can share the content
/// address without becoming one occurrence, while a source identifier can be
/// retained without being mistaken for either.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Evidence {
    pub occurrence: String,
    pub source: String,
    pub normalized_content: ContentHash,
    pub external: Option<ExternalId>,
    pub content: Vec<u8>,
    pub state: EvidenceState,
}

impl Evidence {
    pub fn new(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<Vec<u8>>,
    ) -> Self {
        let content = content.into();
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: normalized_content_hash(&content),
            external: None,
            content,
            state: EvidenceState::Present,
        }
    }

    pub fn correction(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        content: impl Into<Vec<u8>>,
        supersedes: EvidenceId,
        scope: impl Into<String>,
        reason: impl Into<String>,
        authority: impl Into<String>,
    ) -> Self {
        let content = content.into();
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: normalized_content_hash(&content),
            external: None,
            content,
            state: EvidenceState::Correction {
                supersedes,
                scope: scope.into(),
                reason: reason.into(),
                authority: authority.into(),
            },
        }
    }

    pub fn tombstone(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        supersedes: EvidenceId,
        reason: impl Into<String>,
        authority: impl Into<String>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: ContentHash::ZERO,
            external: None,
            content: Vec::new(),
            state: EvidenceState::Tombstone {
                supersedes,
                reason: reason.into(),
                authority: authority.into(),
            },
        }
    }

    /// Construct a deletion tombstone while retaining the normalized content
    /// address of the superseded observation.  [`Self::tombstone`] remains the
    /// loss-minimizing constructor for callers that are not permitted to
    /// retain the prior hash.
    pub fn deleted(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        content: ContentHash,
        supersedes: EvidenceId,
        reason: impl Into<String>,
        authority: impl Into<String>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: content,
            external: None,
            content: Vec::new(),
            state: EvidenceState::Tombstone {
                supersedes,
                reason: reason.into(),
                authority: authority.into(),
            },
        }
    }

    pub fn unavailable(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: ContentHash::ZERO,
            external: None,
            content: Vec::new(),
            state: EvidenceState::Unavailable {
                reason: reason.into(),
            },
        }
    }

    pub fn redacted(
        occurrence: impl Into<String>,
        source: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            source: source.into(),
            normalized_content: ContentHash::ZERO,
            external: None,
            content: Vec::new(),
            state: EvidenceState::Redacted {
                reason: reason.into(),
            },
        }
    }

    /// Set the normalized content address when an adapter has normalized a
    /// payload before handing it to this small store.
    pub fn with_normalized_content(mut self, content: ContentHash) -> Self {
        self.normalized_content = content;
        self
    }

    pub fn with_external(mut self, external: impl Into<ExternalId>) -> Self {
        self.external = Some(external.into());
        self
    }

    pub fn content_hash(&self) -> ContentHash {
        self.normalized_content
    }

    pub fn external_id(&self) -> Option<&ExternalId> {
        self.external.as_ref()
    }
}

/// A normalized proposition.  The store does not interpret the proposition;
/// it merely preserves polarity and deterministic identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Statement {
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub negative: bool,
}

impl Statement {
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            predicate: predicate.into(),
            value: value.into(),
            negative: false,
        }
    }

    pub fn negative(mut self) -> Self {
        self.negative = true;
        self
    }
}

/// A first-class resolution choice.  A decision is data, not an overwritten
/// field, so concurrent choices remain visible to merge and audit consumers.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Decision {
    pub subject: String,
    pub selected: String,
    pub rejected: Vec<String>,
    pub scope: String,
    pub rationale: Option<String>,
    pub supersedes: Option<DecisionId>,
}

impl Decision {
    pub fn new(subject: impl Into<String>, selected: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            selected: selected.into(),
            rejected: Vec::new(),
            scope: String::new(),
            rationale: None,
            supersedes: None,
        }
    }

    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = scope.into();
        self
    }

    pub fn with_rejected(mut self, rejected: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.rejected = rejected.into_iter().map(Into::into).collect();
        self.rejected.sort();
        self.rejected.dedup();
        self
    }

    pub fn with_rationale(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = Some(rationale.into());
        self
    }

    pub fn superseding(mut self, prior: DecisionId) -> Self {
        self.supersedes = Some(prior);
        self
    }
}

/// A bounded statement of source completeness.  It is a positive assertion,
/// not a global closed-world assumption.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Completeness {
    pub relation: String,
    pub source: String,
    pub scope: String,
    pub from: Option<Date>,
    pub until: Option<Date>,
    pub complete: bool,
    pub supersedes: Option<CompletenessId>,
}

impl Completeness {
    pub fn new(
        relation: impl Into<String>,
        source: impl Into<String>,
        scope: impl Into<String>,
        from: Option<Date>,
        until: Option<Date>,
    ) -> Self {
        Self {
            relation: relation.into(),
            source: source.into(),
            scope: scope.into(),
            from,
            until,
            complete: true,
            supersedes: None,
        }
    }

    pub fn incomplete(mut self) -> Self {
        self.complete = false;
        self
    }

    pub fn superseding(mut self, prior: CompletenessId) -> Self {
        self.supersedes = Some(prior);
        self
    }

    fn semantic_key(&self) -> CompletenessScopeKey {
        (
            self.relation.clone(),
            self.source.clone(),
            self.scope.clone(),
        )
    }
}

/// A versioned policy/rule package. `manifest` is storage metadata while the
/// executable body and dependency set are retained losslessly for the policy
/// layer. The store still accepts malformed bodies so callers can inspect and
/// report them instead of having insertion fabricate executable semantics.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PolicyPackage {
    pub name: String,
    pub version: String,
    pub manifest: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Opaque content-addressed dependency roots. They are part of executable
    /// identity but need not name objects in this local store; a future
    /// lockfile/resolver owns that availability check.
    pub dependencies: Vec<ContentHash>,
    pub supersedes: Option<PackageId>,
}

impl PolicyPackage {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            manifest: Vec::new(),
            body: body.into(),
            dependencies: Vec::new(),
            supersedes: None,
        }
    }

    pub fn with_manifest(
        mut self,
        manifest: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.manifest = manifest
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self.manifest.sort();
        self.manifest.dedup();
        self
    }

    /// Add content-addressed package dependencies.  Dependencies are a
    /// semantic set, so their order and repetition do not affect identity.
    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = ContentHash>,
    ) -> Self {
        self.dependencies = dependencies.into_iter().collect();
        self.dependencies.sort();
        self.dependencies.dedup();
        self
    }

    pub fn superseding(mut self, prior: PackageId) -> Self {
        self.supersedes = Some(prior);
        self
    }

    /// Convert a stored package to the executable package without changing
    /// its body, metadata, or dependency set.  Arbitrary manifest metadata is
    /// rejected because the executable package has no field in which to
    /// retain it; silently dropping it would make two stored packages execute
    /// as if they were the same package.
    pub fn to_executable(&self) -> Result<crate::package::PolicyPackage, StoreError> {
        if !self.manifest.is_empty() {
            return Err(StoreError::InvalidObject(format!(
                "policy package `{}` has metadata that cannot be represented by the executable package",
                self.name
            )));
        }
        let body = String::from_utf8(self.body.clone()).map_err(|_| {
            StoreError::InvalidObject(format!(
                "policy package `{}` body is not valid UTF-8",
                self.name
            ))
        })?;
        let executable =
            crate::package::PolicyPackage::new(self.name.clone(), self.version.clone(), body)
                .with_dependencies(self.dependencies.iter().copied());
        executable.validate_identity().map_err(|error| {
            StoreError::InvalidObject(format!(
                "policy package `{}` is not executable: {error}",
                self.name
            ))
        })?;
        Ok(executable)
    }

    /// Construct the canonical storage envelope for an executable package.
    /// Raw archival envelopes may contain additional metadata and therefore
    /// are intentionally not round-tripped through this narrowing view.
    pub fn from_executable(value: &crate::package::PolicyPackage) -> Self {
        Self::new(
            value.name.clone(),
            value.version.clone(),
            value.canonical_body_text().into_bytes(),
        )
        .with_dependencies(value.dependencies.iter().copied())
    }
}

/// An immutable storage envelope for one compiler artifact.
///
/// The artifact is the sole semantic authority. Its canonical bytes contain
/// the lockfile, compiled packages, modules, and exports; its hash is
/// recomputed whenever the object crosses the store boundary. Stored policy
/// objects are a distinct representation and are not claimed as equivalent
/// without an explicit conversion proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledArtifactObject {
    pub artifact: CompiledArtifact,
}

impl CompiledArtifactObject {
    pub fn new(artifact: CompiledArtifact) -> Self {
        Self { artifact }
    }

    pub fn artifact_hash(&self) -> ContentHash {
        self.artifact.artifact_hash()
    }

    pub fn input_roots(&self) -> Vec<ContentHash> {
        self.artifact.package_roots()
    }

    /// Check the artifact's content hash against its complete canonical body.
    pub fn verify_integrity(&self) -> Result<(), StoreError> {
        let expected = self.artifact.recomputed_hash();
        if expected != self.artifact.artifact_hash() {
            return Err(StoreError::InvalidObject(format!(
                "compiled artifact hash is {}, expected {}",
                self.artifact.artifact_hash(),
                expected
            )));
        }
        Ok(())
    }
}

impl Ord for CompiledArtifactObject {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.artifact_hash()
            .cmp(&other.artifact_hash())
            .then_with(|| {
                self.artifact
                    .canonical_bytes()
                    .cmp(&other.artifact.canonical_bytes())
            })
    }
}

impl PartialOrd for CompiledArtifactObject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Canonical proof content plus references to other store objects pinned by a
/// commit.  The store validates the proof DAG before assigning its own typed
/// object address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofObject {
    /// Canonical proof content.  `put_proof` independently checks this DAG
    /// before accepting it; callers must not be able to persist an arbitrary
    /// graph under an otherwise valid store address.
    pub proof: CanonicalProof,
    /// References to other store objects retained for close/commit pinning.
    /// They are not proof-node identities and are checked as ordinary store
    /// references by `ObjectStore::put_proof`.
    pub roots: Vec<ContentHash>,
}

impl Ord for ProofObject {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.proof
            .content_hash()
            .cmp(&other.proof.content_hash())
            .then_with(|| self.roots.cmp(&other.roots))
    }
}

impl PartialOrd for ProofObject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ProofObject {
    pub fn new(roots: impl IntoIterator<Item = ContentHash>, body: impl Into<Vec<u8>>) -> Self {
        let mut roots: Vec<_> = roots.into_iter().collect();
        roots.sort();
        roots.dedup();
        let body = body.into();
        // Keep the historical constructor useful without allowing an opaque
        // byte blob to masquerade as an unchecked proof.  The envelope is
        // represented by a canonical observation node whose source is bound
        // to the supplied bytes; callers with real solver content should use
        // `from_proof` instead.
        let mut proof = CanonicalProof::new();
        let node = CanonicalNode::new(
            "stored proof envelope",
            CanonicalOperation::Observation {
                source: "stored-proof".into(),
            },
            Vec::new(),
            BTreeMap::from([(
                "payload-hash".to_string(),
                blake3::hash(&body).to_hex().to_string(),
            )]),
        );
        let root = proof.insert(node);
        proof.root(root);
        Self { proof, roots }
    }

    /// Persist canonical proof content in the store envelope.
    pub fn from_proof(proof: CanonicalProof) -> Self {
        let mut proof = proof;
        proof.roots.sort();
        proof.roots.dedup();
        Self {
            proof,
            roots: Vec::new(),
        }
    }

    pub fn canonical_proof(&self) -> &CanonicalProof {
        &self.proof
    }
}

pub type Proof = ProofObject;

/// A signature is opaque to the store, but signer and bytes are included in
/// the object identity and have a stable ordering.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Signature {
    pub signer: String,
    pub algorithm: String,
    pub bytes: Vec<u8>,
}

impl Signature {
    pub fn new(
        signer: impl Into<String>,
        algorithm: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            signer: signer.into(),
            algorithm: algorithm.into(),
            bytes: bytes.into(),
        }
    }
}

/// Verification boundary for signed immutable objects. The store owns the
/// canonical payload; cryptographic key lookup and revocation policy stay
/// outside the semantic kernel.
pub trait SignatureVerifier {
    fn verify(&self, signer: &str, algorithm: &str, payload: ContentHash, signature: &[u8])
    -> bool;
}

/// A reproducible branch snapshot.  Roots are sets in the semantic sense;
/// constructors and insertion both normalize their order and remove repeats.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Commit {
    pub parents: Vec<CommitId>,
    pub evidence: Vec<EvidenceId>,
    pub statements: Vec<StatementId>,
    pub decisions: Vec<DecisionId>,
    pub completeness: Vec<CompletenessId>,
    pub packages: Vec<PackageId>,
    pub proofs: Vec<ProofObjectId>,
    /// Durable semantic conflicts. A later snapshot may replace an unresolved
    /// conflict only with a [`ConflictRecord`] that explicitly resolves it.
    pub conflicts: Vec<ConflictId>,
    pub schema_version: String,
    pub author: String,
    pub signatures: Vec<Signature>,
}

impl Commit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parents: impl IntoIterator<Item = CommitId>,
        evidence: impl IntoIterator<Item = EvidenceId>,
        statements: impl IntoIterator<Item = StatementId>,
        decisions: impl IntoIterator<Item = DecisionId>,
        completeness: impl IntoIterator<Item = CompletenessId>,
        packages: impl IntoIterator<Item = PackageId>,
        proofs: impl IntoIterator<Item = ProofObjectId>,
        author: impl Into<String>,
    ) -> Self {
        Self {
            parents: canonical_set(parents),
            evidence: canonical_set(evidence),
            statements: canonical_set(statements),
            decisions: canonical_set(decisions),
            completeness: canonical_set(completeness),
            packages: canonical_set(packages),
            proofs: canonical_set(proofs),
            conflicts: Vec::new(),
            schema_version: SCHEMA_VERSION.to_string(),
            author: author.into(),
            signatures: Vec::new(),
        }
    }

    pub fn with_schema_version(mut self, version: impl Into<String>) -> Self {
        self.schema_version = version.into();
        self
    }

    pub fn with_signatures(mut self, signatures: impl IntoIterator<Item = Signature>) -> Self {
        self.signatures = signatures.into_iter().collect();
        self.canonicalize();
        self
    }

    pub fn with_conflicts(mut self, conflicts: impl IntoIterator<Item = ConflictId>) -> Self {
        self.conflicts = canonical_set(conflicts);
        self
    }

    pub fn canonicalize(&mut self) {
        canonicalize_vec(&mut self.parents);
        canonicalize_vec(&mut self.evidence);
        canonicalize_vec(&mut self.statements);
        canonicalize_vec(&mut self.decisions);
        canonicalize_vec(&mut self.completeness);
        canonicalize_vec(&mut self.packages);
        canonicalize_vec(&mut self.proofs);
        canonicalize_vec(&mut self.conflicts);
        self.signatures.sort();
        self.signatures.dedup();
    }

    pub fn signing_hash(&self) -> ContentHash {
        let mut unsigned = self.clone();
        unsigned.canonicalize();
        unsigned.signatures.clear();
        let mut bytes = Vec::new();
        encode_commit(&mut bytes, &unsigned);
        ContentHash::domain_separated("axiom/store/commit-signing/v1", &bytes)
    }
}

/// A reporting close pins the source commit, selected policy package roots,
/// a sealed workspace analysis artifact, exceptions, and signatures.  A
/// reopening is a new close with `supersedes` set; no historical object is
/// mutated.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Period {
    pub from: Date,
    pub until: Date,
}

impl Period {
    pub fn new(from: Date, until: Date) -> Result<Self, StoreError> {
        if from > until {
            return Err(StoreError::InvalidObject(
                "period starts after it ends".to_string(),
            ));
        }
        Ok(Self { from, until })
    }
}

/// A sealed, content-addressed sale-ledger analysis result produced by
/// [`crate::workspace::Workspace`].
///
/// This is deliberately not a general-purpose proof wrapper. Its private
/// fields prevent callers from rebinding an analysis to a different book or
/// reporting period. The analysis commit already content-addresses the exact
/// proof and its complete terminal result set, so the artifact does not repeat
/// either. The store checks that immutable commit/proof shape again when this
/// object is inserted; the workspace is the only constructor because it is
/// the only layer that has a checked engine [`crate::engine::Analysis`].
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AnalysisArtifact {
    analysis_commit: CommitId,
    book: BookId,
    period: Period,
}

impl AnalysisArtifact {
    /// This constructor is crate-private on purpose.  External callers must
    /// obtain an artifact through the workspace's checked close flow.
    pub(crate) fn new(analysis_commit: CommitId, book: BookId, period: Period) -> Self {
        Self {
            analysis_commit,
            book,
            period,
        }
    }

    pub fn analysis_commit(&self) -> CommitId {
        self.analysis_commit
    }

    pub fn book(&self) -> &BookId {
        &self.book
    }

    pub fn period(&self) -> &Period {
        &self.period
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Close {
    pub period: Period,
    pub book: BookId,
    pub policies: Vec<PackageId>,
    pub source: CommitId,
    /// Content address of the sealed workspace analysis artifact.  A generic
    /// [`ProofObject`] is intentionally not accepted here.
    pub analysis_artifact: AnalysisArtifactId,
    pub exceptions: Vec<String>,
    pub signatures: Vec<Signature>,
    pub supersedes: Option<CloseId>,
}

impl Close {
    pub fn new(
        period: Period,
        book: impl Into<BookId>,
        policies: impl IntoIterator<Item = PackageId>,
        source: CommitId,
        analysis_artifact: AnalysisArtifactId,
    ) -> Self {
        Self {
            period,
            book: book.into(),
            policies: canonical_set(policies),
            source,
            analysis_artifact,
            exceptions: Vec::new(),
            signatures: Vec::new(),
            supersedes: None,
        }
    }

    pub fn with_exceptions(
        mut self,
        exceptions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.exceptions = exceptions.into_iter().map(Into::into).collect();
        self.exceptions.sort();
        self.exceptions.dedup();
        self
    }

    pub fn with_signatures(mut self, signatures: impl IntoIterator<Item = Signature>) -> Self {
        self.signatures = signatures.into_iter().collect();
        self.signatures.sort();
        self.signatures.dedup();
        self
    }

    pub fn superseding(mut self, prior: CloseId) -> Self {
        self.supersedes = Some(prior);
        self
    }

    /// Domain-separated canonical payload signed by every close signature.
    /// Signatures themselves are excluded, avoiding hash/signature circularity.
    pub fn signing_hash(&self) -> ContentHash {
        let mut unsigned = self.clone();
        unsigned.policies.sort();
        unsigned.policies.dedup();
        unsigned.exceptions.sort();
        unsigned.exceptions.dedup();
        unsigned.signatures.clear();
        let mut bytes = Vec::new();
        encode_close(&mut bytes, &unsigned);
        ContentHash::domain_separated("axiom/store/close-signing/v1", &bytes)
    }
}

/// The concrete object carried by the in-memory store.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StoredObject {
    Evidence(Evidence),
    Statement(Statement),
    Decision(Decision),
    Completeness(Completeness),
    Package(PolicyPackage),
    CompiledArtifact(CompiledArtifactObject),
    AnalysisArtifact(AnalysisArtifact),
    Proof(ProofObject),
    Conflict(ConflictRecord),
    Commit(Commit),
    Close(Close),
}

impl StoredObject {
    pub const fn kind(&self) -> ObjectKind {
        match self {
            Self::Evidence(_) => ObjectKind::Evidence,
            Self::Statement(_) => ObjectKind::Statement,
            Self::Decision(_) => ObjectKind::Decision,
            Self::Completeness(_) => ObjectKind::Completeness,
            Self::Package(_) => ObjectKind::Package,
            Self::CompiledArtifact(_) => ObjectKind::CompiledArtifact,
            Self::AnalysisArtifact(_) => ObjectKind::AnalysisArtifact,
            Self::Proof(_) => ObjectKind::Proof,
            Self::Conflict(_) => ObjectKind::Conflict,
            Self::Commit(_) => ObjectKind::Commit,
            Self::Close(_) => ObjectKind::Close,
        }
    }

    /// Deterministic bytes used for this object's content address.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_string(&mut out, SCHEMA_VERSION);
        put_string(&mut out, self.kind().tag());
        match self {
            Self::Evidence(value) => encode_evidence(&mut out, value),
            Self::Statement(value) => encode_statement(&mut out, value),
            Self::Decision(value) => encode_decision(&mut out, value),
            Self::Completeness(value) => encode_completeness(&mut out, value),
            Self::Package(value) => encode_package(&mut out, value),
            Self::CompiledArtifact(value) => encode_compiled_artifact(&mut out, value),
            Self::AnalysisArtifact(value) => encode_analysis_artifact(&mut out, value),
            Self::Proof(value) => encode_proof(&mut out, value),
            Self::Conflict(value) => encode_conflict_record(&mut out, value),
            Self::Commit(value) => encode_commit(&mut out, value),
            Self::Close(value) => encode_close(&mut out, value),
        }
        out
    }

    pub fn content_hash(&self) -> ContentHash {
        ContentHash::domain_separated("axiom/store/object/v3", &self.canonical_bytes())
    }
}

/// Conflicts are semantic, not storage failures.  The merged commit is still
/// stored, while these diagnostics keep a later close from pretending that
/// two divergent choices were reconciled.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MergeConflict {
    /// Both branches changed the successor of one immutable evidence object
    /// differently.  The merged commit retains every successor; this
    /// diagnostic prevents a consumer from silently treating one as the
    /// accepted correction.
    Evidence {
        supersedes: EvidenceId,
        left: Vec<EvidenceId>,
        right: Vec<EvidenceId>,
    },
    EvidenceIdentity {
        source: String,
        identity: String,
        alternatives: Vec<EvidenceId>,
    },
    Statements {
        subject: String,
        predicate: String,
        value: String,
        left: Vec<StatementId>,
        right: Vec<StatementId>,
    },
    Decisions {
        subject: String,
        scope: String,
        left: Vec<DecisionId>,
        right: Vec<DecisionId>,
    },
    Policies {
        name: String,
        left: Vec<PackageId>,
        right: Vec<PackageId>,
    },
    Completeness {
        relation: String,
        source: String,
        scope: String,
        left: Vec<CompletenessId>,
        right: Vec<CompletenessId>,
        left_bounds: Vec<CompletenessBounds>,
        right_bounds: Vec<CompletenessBounds>,
    },
}

/// Immutable lifecycle record for a collaboration conflict.
///
/// Resolution never mutates or deletes the original record. It creates a new
/// record that names both the unresolved object and the decision authorizing
/// its resolution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ConflictRecord {
    pub conflict: MergeConflict,
    pub supersedes: Option<ConflictId>,
    pub resolution: Option<DecisionId>,
    pub rationale: Option<String>,
}

impl ConflictRecord {
    pub fn resolution_subject(conflict: ConflictId) -> String {
        format!("conflict/{conflict}")
    }

    pub fn unresolved(conflict: MergeConflict) -> Self {
        Self {
            conflict,
            supersedes: None,
            resolution: None,
            rationale: None,
        }
    }

    pub fn resolved(
        conflict: MergeConflict,
        supersedes: ConflictId,
        resolution: DecisionId,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            conflict,
            supersedes: Some(supersedes),
            resolution: Some(resolution),
            rationale: Some(rationale.into()),
        }
    }

    pub const fn is_resolved(&self) -> bool {
        self.resolution.is_some()
    }
}

/// The interval portion of a completeness claim retained in a merge
/// diagnostic.  Keeping it next to the object IDs prevents a conflict report
/// from collapsing two claims merely because their relation keys match.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CompletenessBounds {
    pub from: Option<Date>,
    pub until: Option<Date>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub commit: CommitId,
    pub conflicts: Vec<MergeConflict>,
    pub conflict_objects: Vec<ConflictId>,
    pub unresolved_conflicts: Vec<ConflictId>,
}

impl MergeResult {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty() && self.unresolved_conflicts.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    MissingObject(ContentHash),
    WrongKind {
        hash: ContentHash,
        expected: ObjectKind,
        actual: ObjectKind,
    },
    HashCollision(ContentHash),
    CorruptObject(ContentHash),
    InvalidObject(String),
    MergeRequiresCommit(ContentHash),
    BaseNotAncestor {
        base: CommitId,
        branch: CommitId,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingObject(hash) => write!(f, "missing object {hash}"),
            Self::WrongKind {
                hash,
                expected,
                actual,
            } => write!(f, "object {hash} is {actual}, expected {expected}"),
            Self::HashCollision(hash) => write!(f, "content hash collision at {hash}"),
            Self::CorruptObject(hash) => write!(f, "corrupt object {hash}"),
            Self::InvalidObject(reason) => f.write_str(reason),
            Self::MergeRequiresCommit(hash) => write!(f, "merge input {hash} is not a commit"),
            Self::BaseNotAncestor { base, branch } => {
                write!(f, "base commit {base} is not an ancestor of {branch}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// A small immutable-by-API content-addressed store.  There is deliberately
/// no mutable accessor for stored bytes; all writes are insert-only and equal
/// content returns the existing address.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectStore {
    objects: BTreeMap<ContentHash, StoredObject>,
}

impl ObjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    fn insert(&mut self, object: StoredObject) -> Result<ContentHash, StoreError> {
        let hash = object.content_hash();
        if let Some(existing) = self.objects.get(&hash) {
            if existing.kind() != object.kind()
                || existing.canonical_bytes() != object.canonical_bytes()
            {
                return Err(StoreError::HashCollision(hash));
            }
            return Ok(hash);
        }
        self.objects.insert(hash, object);
        Ok(hash)
    }

    pub fn get(&self, hash: ContentHash) -> Result<&StoredObject, StoreError> {
        let object = self
            .objects
            .get(&hash)
            .ok_or(StoreError::MissingObject(hash))?;
        if object.content_hash() != hash {
            return Err(StoreError::CorruptObject(hash));
        }
        if let StoredObject::Proof(value) = object {
            value
                .proof
                .check()
                .map_err(|error| StoreError::InvalidObject(format!("invalid proof: {error}")))?;
        } else if let StoredObject::CompiledArtifact(value) = object {
            value.verify_integrity()?;
        } else if let StoredObject::AnalysisArtifact(value) = object {
            self.validate_analysis_artifact(value)?;
        }
        Ok(object)
    }

    /// Return the verified canonical bytes for an address.  Callers that
    /// persist an object externally can use these bytes as the exact payload
    /// whose hash is committed by the store.
    pub fn canonical_bytes(&self, hash: ContentHash) -> Result<Vec<u8>, StoreError> {
        Ok(self.get(hash)?.canonical_bytes())
    }

    /// Verify every object currently held.  This is intentionally public so a
    /// persistent backend can run the same check after loading a pack.
    pub fn verify(&self) -> Result<(), StoreError> {
        for (hash, object) in &self.objects {
            if object.content_hash() != *hash {
                return Err(StoreError::CorruptObject(*hash));
            }
            if let StoredObject::Proof(value) = object {
                value.proof.check().map_err(|error| {
                    StoreError::InvalidObject(format!("invalid proof {hash}: {error}"))
                })?;
            } else if let StoredObject::CompiledArtifact(value) = object {
                value.verify_integrity()?;
            } else if let StoredObject::AnalysisArtifact(value) = object {
                self.validate_analysis_artifact(value)?;
            }
        }
        Ok(())
    }

    pub fn contains(&self, hash: ContentHash) -> bool {
        self.objects.contains_key(&hash) && self.get(hash).is_ok()
    }

    pub fn put_evidence(&mut self, value: Evidence) -> Result<EvidenceId, StoreError> {
        if matches!(
            &value.state,
            EvidenceState::Tombstone { .. }
                | EvidenceState::Unavailable { .. }
                | EvidenceState::Redacted { .. }
        ) && !value.content.is_empty()
        {
            return Err(StoreError::InvalidObject(
                "evidence without an available payload cannot retain content bytes".to_string(),
            ));
        }
        if let Some(target) = value.state.target() {
            self.require_kind(target.hash(), ObjectKind::Evidence)?;
            let prior = self.evidence(target)?;
            if value.source != prior.source {
                return Err(StoreError::InvalidObject(
                    "evidence supersession must retain its source".to_string(),
                ));
            }
            if value.state.is_tombstone()
                && value.normalized_content != ContentHash::ZERO
                && value.normalized_content != prior.normalized_content
            {
                return Err(StoreError::InvalidObject(
                    "deletion tombstone content hash must match its target".to_string(),
                ));
            }
        }
        Ok(EvidenceId::new(self.insert(StoredObject::Evidence(value))?))
    }

    pub fn put_statement(&mut self, value: Statement) -> Result<StatementId, StoreError> {
        Ok(StatementId::new(
            self.insert(StoredObject::Statement(value))?,
        ))
    }

    pub fn put_decision(&mut self, mut value: Decision) -> Result<DecisionId, StoreError> {
        value.rejected.sort();
        value.rejected.dedup();
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Decision)?;
            let prior_value = self.decision(prior)?;
            if value.subject != prior_value.subject || value.scope != prior_value.scope {
                return Err(StoreError::InvalidObject(
                    "decision supersession must retain subject and scope".to_string(),
                ));
            }
        }
        Ok(DecisionId::new(self.insert(StoredObject::Decision(value))?))
    }

    pub fn put_completeness(&mut self, value: Completeness) -> Result<CompletenessId, StoreError> {
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Completeness)?;
            let prior_value = self.completeness(prior)?;
            if value.semantic_key() != prior_value.semantic_key() {
                return Err(StoreError::InvalidObject(
                    "completeness supersession must retain relation, source, and scope".to_string(),
                ));
            }
        }
        Ok(CompletenessId::new(
            self.insert(StoredObject::Completeness(value))?,
        ))
    }

    pub fn put_package(&mut self, mut value: PolicyPackage) -> Result<PackageId, StoreError> {
        value.manifest.sort();
        value.manifest.dedup();
        value.dependencies.sort();
        value.dependencies.dedup();
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Package)?;
            let prior_value = self.package(prior)?;
            if value.name != prior_value.name {
                return Err(StoreError::InvalidObject(
                    "policy package supersession must retain package name".to_string(),
                ));
            }
        }
        Ok(PackageId::new(self.insert(StoredObject::Package(value))?))
    }

    /// Persist a compiler artifact after independently checking its content
    /// hash and complete package-input roots.
    pub fn put_compiled_artifact(
        &mut self,
        artifact: CompiledArtifact,
    ) -> Result<CompiledArtifactId, StoreError> {
        let value = CompiledArtifactObject::new(artifact);
        value.verify_integrity()?;
        Ok(CompiledArtifactId::new(
            self.insert(StoredObject::CompiledArtifact(value))?,
        ))
    }

    /// Persist the only close authority. This boundary is crate-private so
    /// callers cannot manufacture authority around an unchecked analysis;
    /// [`crate::workspace::Workspace`] creates it only after checking engine
    /// output, and the store independently verifies its immutable bindings.
    pub(crate) fn put_analysis_artifact(
        &mut self,
        value: AnalysisArtifact,
    ) -> Result<AnalysisArtifactId, StoreError> {
        self.validate_analysis_artifact(&value)?;
        Ok(AnalysisArtifactId::new(
            self.insert(StoredObject::AnalysisArtifact(value))?,
        ))
    }

    pub fn put_proof(&mut self, value: ProofObject) -> Result<ProofObjectId, StoreError> {
        value
            .proof
            .check()
            .map_err(|error| StoreError::InvalidObject(format!("invalid proof: {error}")))?;
        let commit_bindings = value
            .proof
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    &node.operation,
                    CanonicalOperation::Observation { source }
                        if source.starts_with("commit:")
                )
            })
            .collect::<Vec<_>>();
        if !commit_bindings.is_empty() {
            let binding = commit_bindings[0];
            let expected = value.roots.first().map(ToString::to_string);
            let source_matches = matches!(
                &binding.operation,
                CanonicalOperation::Observation { source }
                    if Some(source.strip_prefix("commit:").unwrap_or_default())
                        == expected.as_deref()
            );
            let mut expected_inputs = value.proof.roots.clone();
            expected_inputs.retain(|root| *root != binding.id);
            if commit_bindings.len() != 1
                || value.roots.len() != 1
                || !source_matches
                || !value.proof.roots.contains(&binding.id)
                || binding.inputs != expected_inputs
            {
                return Err(StoreError::InvalidObject(
                    "proof commit binding does not match its external root".into(),
                ));
            }
        }
        for root in &value.roots {
            if *root != ContentHash::ZERO {
                self.get(*root)?;
            }
        }
        Ok(ProofObjectId::new(self.insert(StoredObject::Proof(value))?))
    }

    fn validate_analysis_artifact(&self, value: &AnalysisArtifact) -> Result<(), StoreError> {
        if value.period.from > value.period.until {
            return Err(StoreError::InvalidObject(
                "analysis artifact period starts after it ends".into(),
            ));
        }
        let analysis = self.commit(value.analysis_commit)?;
        let [source_id] = analysis.parents.as_slice() else {
            return Err(StoreError::InvalidObject(
                "analysis artifact must point to a direct analysis child".into(),
            ));
        };
        let source = self.commit(*source_id)?.clone();
        let [proof_id] = analysis.proofs.as_slice() else {
            return Err(StoreError::InvalidObject(
                "analysis artifact child must pin exactly one proof".into(),
            ));
        };
        if !analysis.evidence.is_empty()
            || !analysis.statements.is_empty()
            || !analysis.completeness.is_empty()
            || analysis.decisions != source.decisions
            || analysis.packages != source.packages
            || analysis.conflicts != source.conflicts
            || analysis.schema_version != source.schema_version
            || analysis.author != ANALYSIS_AUTHOR
            || !analysis.signatures.is_empty()
        {
            return Err(StoreError::InvalidObject(
                "analysis artifact does not point to the deterministic analysis child".into(),
            ));
        }
        let proof = self.proof(*proof_id)?;
        if proof.roots != vec![source_id.hash()] {
            return Err(StoreError::InvalidObject(
                "analysis artifact proof must bind exactly its source commit".into(),
            ));
        }
        let bindings = proof
            .proof
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    &node.operation,
                    CanonicalOperation::Observation { source } if source.starts_with("commit:")
                )
            })
            .collect::<Vec<_>>();
        let Some(binding) = bindings.first() else {
            return Err(StoreError::InvalidObject(
                "analysis artifact proof has no exact source binding".into(),
            ));
        };
        let expected = source_id.hash().to_string();
        let valid_binding = matches!(
            &binding.operation,
            CanonicalOperation::Observation { source } if source == &format!("commit:{expected}")
        );
        let mut expected_inputs = proof.proof.roots.clone();
        expected_inputs.retain(|root| *root != binding.id);
        if bindings.len() != 1
            || !proof.proof.roots.contains(&binding.id)
            || !valid_binding
            || binding.metadata.get("source-commit") != Some(&expected)
            || binding.inputs != expected_inputs
        {
            return Err(StoreError::InvalidObject(
                "analysis artifact proof source binding is not exact".into(),
            ));
        }
        let expected_results = Self::sale_close_result_roots_from(proof);
        if expected_results.is_empty() {
            return Err(StoreError::InvalidObject(
                "analysis artifact proof must expose at least one sale result root".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn sale_close_result_roots(
        &self,
        proof: ProofObjectId,
    ) -> Result<Vec<CanonicalProofId>, StoreError> {
        let proof = self.proof(proof)?;
        Ok(Self::sale_close_result_roots_from(proof))
    }

    fn sale_close_result_roots_from(proof: &ProofObject) -> Vec<CanonicalProofId> {
        canonical_proof_set(proof.proof.roots.iter().copied().filter(|root| {
            proof.proof.node(*root).is_some_and(|node| {
                matches!(
                    node.operation,
                    CanonicalOperation::Recognition { .. } | CanonicalOperation::JournalEntry(..)
                )
            })
        }))
    }

    pub fn put_conflict(&mut self, mut value: ConflictRecord) -> Result<ConflictId, StoreError> {
        if let MergeConflict::Completeness {
            left,
            right,
            left_bounds,
            right_bounds,
            ..
        } = &value.conflict
            && (left.len() != left_bounds.len() || right.len() != right_bounds.len())
        {
            return Err(StoreError::InvalidObject(
                "completeness conflict bounds must match their referenced claims".into(),
            ));
        }
        canonicalize_merge_conflict(&mut value.conflict);
        self.validate_merge_conflict(&value.conflict)?;
        match (value.supersedes, value.resolution) {
            (None, None) => {
                if value.rationale.is_some() {
                    return Err(StoreError::InvalidObject(
                        "unresolved conflict cannot carry a resolution rationale".into(),
                    ));
                }
            }
            (Some(prior), Some(decision)) => {
                let prior_value = self.conflict(prior)?;
                if prior_value.is_resolved() || prior_value.conflict != value.conflict {
                    return Err(StoreError::InvalidObject(
                        "conflict resolution must supersede the same unresolved conflict".into(),
                    ));
                }
                self.require_kind(decision.hash(), ObjectKind::Decision)?;
                if self.decision(decision)?.subject != ConflictRecord::resolution_subject(prior) {
                    return Err(StoreError::InvalidObject(
                        "conflict resolution decision must explicitly name the conflict".into(),
                    ));
                }
                if value
                    .rationale
                    .as_deref()
                    .is_none_or(|rationale| rationale.trim().is_empty())
                {
                    return Err(StoreError::InvalidObject(
                        "conflict resolution requires a rationale".into(),
                    ));
                }
            }
            _ => {
                return Err(StoreError::InvalidObject(
                    "conflict resolution requires both prior conflict and decision".into(),
                ));
            }
        }
        Ok(ConflictId::new(self.insert(StoredObject::Conflict(value))?))
    }

    fn validate_merge_conflict(&self, conflict: &MergeConflict) -> Result<(), StoreError> {
        let invalid = || {
            StoreError::InvalidObject(
                "conflict payload does not match its referenced semantic objects".into(),
            )
        };
        match conflict {
            MergeConflict::Evidence {
                supersedes,
                left,
                right,
            } => {
                self.require_kind(supersedes.hash(), ObjectKind::Evidence)?;
                for id in left.iter().chain(right) {
                    self.require_kind(id.hash(), ObjectKind::Evidence)?;
                    if self.evidence(*id)?.state.supersedes() != Some(*supersedes) {
                        return Err(invalid());
                    }
                }
            }
            MergeConflict::EvidenceIdentity {
                source,
                identity,
                alternatives,
            } => {
                for id in alternatives {
                    self.require_kind(id.hash(), ObjectKind::Evidence)?;
                    let evidence = self.evidence(*id)?;
                    let actual_identity = evidence.external.as_ref().map_or_else(
                        || format!("occurrence:{}", evidence.occurrence),
                        |external| format!("external:{}", external.as_str()),
                    );
                    if evidence.source != *source
                        || actual_identity != *identity
                        || !matches!(evidence.state, EvidenceState::Present)
                    {
                        return Err(invalid());
                    }
                }
            }
            MergeConflict::Statements {
                subject,
                predicate,
                value,
                left,
                right,
            } => {
                for id in left.iter().chain(right) {
                    self.require_kind(id.hash(), ObjectKind::Statement)?;
                    let statement = self.statement(*id)?;
                    if statement.subject != *subject
                        || statement.predicate != *predicate
                        || statement.value != *value
                    {
                        return Err(invalid());
                    }
                }
            }
            MergeConflict::Decisions {
                subject,
                scope,
                left,
                right,
            } => {
                for id in left.iter().chain(right) {
                    self.require_kind(id.hash(), ObjectKind::Decision)?;
                    let decision = self.decision(*id)?;
                    if decision.subject != *subject || decision.scope != *scope {
                        return Err(invalid());
                    }
                }
            }
            MergeConflict::Policies { name, left, right } => {
                for id in left.iter().chain(right) {
                    self.require_kind(id.hash(), ObjectKind::Package)?;
                    if self.package(*id)?.name != *name {
                        return Err(invalid());
                    }
                }
            }
            MergeConflict::Completeness {
                relation,
                source,
                scope,
                left,
                right,
                left_bounds,
                right_bounds,
            } => {
                if left.len() != left_bounds.len() || right.len() != right_bounds.len() {
                    return Err(invalid());
                }
                for (id, bounds) in left
                    .iter()
                    .zip(left_bounds)
                    .chain(right.iter().zip(right_bounds))
                {
                    self.require_kind(id.hash(), ObjectKind::Completeness)?;
                    let claim = self.completeness(*id)?;
                    if claim.relation != *relation
                        || claim.source != *source
                        || claim.scope != *scope
                        || claim.from != bounds.from
                        || claim.until != bounds.until
                        || claim.complete != bounds.complete
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn put_commit(&mut self, value: Commit) -> Result<CommitId, StoreError> {
        if !value.signatures.is_empty() {
            return Err(StoreError::InvalidObject(
                "signed commit requires signature verification".into(),
            ));
        }
        self.put_commit_inner(value)
    }

    pub fn put_commit_verified(
        &mut self,
        value: Commit,
        verifier: &impl SignatureVerifier,
    ) -> Result<CommitId, StoreError> {
        if value.signatures.is_empty() {
            return Err(StoreError::InvalidObject(
                "verified commit requires at least one signature".into(),
            ));
        }
        let payload = value.signing_hash();
        for signature in &value.signatures {
            if signature.signer.trim().is_empty()
                || signature.algorithm.trim().is_empty()
                || signature.bytes.is_empty()
                || !verifier.verify(
                    &signature.signer,
                    &signature.algorithm,
                    payload,
                    &signature.bytes,
                )
            {
                return Err(StoreError::InvalidObject(format!(
                    "commit signature from {} is invalid",
                    signature.signer
                )));
            }
        }
        self.put_commit_inner(value)
    }

    fn put_commit_inner(&mut self, mut value: Commit) -> Result<CommitId, StoreError> {
        value.canonicalize();
        self.validate_commit(&value)?;
        Ok(CommitId::new(self.insert(StoredObject::Commit(value))?))
    }

    /// Store an unsigned close. Signed closes must cross
    /// [`Self::put_close_verified`] so opaque bytes cannot be treated as trust.
    pub fn put_close(&mut self, value: Close) -> Result<CloseId, StoreError> {
        if !value.signatures.is_empty() {
            return Err(StoreError::InvalidObject(
                "signed close requires signature verification".into(),
            ));
        }
        self.put_close_inner(value)
    }

    pub fn put_close_verified(
        &mut self,
        value: Close,
        verifier: &impl SignatureVerifier,
    ) -> Result<CloseId, StoreError> {
        if value.signatures.is_empty() {
            return Err(StoreError::InvalidObject(
                "verified close requires at least one signature".into(),
            ));
        }
        let payload = value.signing_hash();
        for signature in &value.signatures {
            if signature.signer.trim().is_empty()
                || signature.algorithm.trim().is_empty()
                || signature.bytes.is_empty()
                || !verifier.verify(
                    &signature.signer,
                    &signature.algorithm,
                    payload,
                    &signature.bytes,
                )
            {
                return Err(StoreError::InvalidObject(format!(
                    "close signature from {} is invalid",
                    signature.signer
                )));
            }
        }
        self.put_close_inner(value)
    }

    fn put_close_inner(&mut self, mut value: Close) -> Result<CloseId, StoreError> {
        value.policies.sort();
        value.policies.dedup();
        value.exceptions.sort();
        value.exceptions.dedup();
        value.signatures.sort();
        value.signatures.dedup();
        if value.signatures.iter().any(|signature| {
            signature.signer.trim().is_empty()
                || signature.algorithm.trim().is_empty()
                || signature.bytes.is_empty()
        }) {
            return Err(StoreError::InvalidObject(
                "close contains an incomplete signature".into(),
            ));
        }
        self.require_kind(value.source.hash(), ObjectKind::Commit)?;
        let source_commit = self.commit(value.source)?.clone();
        let mut unresolved = Vec::new();
        let mut resolutions = BTreeMap::<ConflictId, DecisionId>::new();
        for id in &source_commit.conflicts {
            let conflict = self.conflict(*id)?;
            if !conflict.is_resolved() {
                unresolved.push(*id);
            } else if let (Some(prior), Some(decision)) = (conflict.supersedes, conflict.resolution)
                && resolutions
                    .insert(prior, decision)
                    .is_some_and(|existing| existing != decision)
            {
                return Err(StoreError::InvalidObject(
                    "close source contains conflicting semantic conflict resolutions".into(),
                ));
            }
        }
        if !unresolved.is_empty() {
            return Err(StoreError::InvalidObject(format!(
                "close source contains {} unresolved semantic conflict(s)",
                unresolved.len()
            )));
        }
        for package in &value.policies {
            self.require_kind(package.hash(), ObjectKind::Package)?;
        }
        if value.policies != source_commit.packages {
            return Err(StoreError::InvalidObject(
                "close policies must exactly match the package roots pinned by its source commit"
                    .into(),
            ));
        }
        if value.analysis_artifact.hash() == ContentHash::ZERO {
            return Err(StoreError::InvalidObject(
                "close must pin a sealed analysis artifact".into(),
            ));
        }
        let artifact = match self.get(value.analysis_artifact.hash())? {
            StoredObject::AnalysisArtifact(artifact) => artifact,
            object => {
                return Err(StoreError::WrongKind {
                    hash: value.analysis_artifact.hash(),
                    expected: ObjectKind::AnalysisArtifact,
                    actual: object.kind(),
                });
            }
        };
        if artifact.book != value.book || artifact.period != value.period {
            return Err(StoreError::InvalidObject(
                "close fields must exactly match its sealed analysis artifact".into(),
            ));
        }
        let analysis_commit = self.commit(artifact.analysis_commit)?;
        if analysis_commit.parents != vec![value.source]
            || analysis_commit.packages != value.policies
            || source_commit.packages != value.policies
        {
            return Err(StoreError::InvalidObject(
                "close artifact is not a child of its exact source commit".into(),
            ));
        }
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Close)?;
            let prior_value = self.close(prior)?;
            if value.period != prior_value.period || value.book != prior_value.book {
                return Err(StoreError::InvalidObject(
                    "close supersession must retain book and reporting period".to_string(),
                ));
            }
            self.require_ancestor(prior_value.source, value.source)
                .map_err(|_| {
                    StoreError::InvalidObject(
                        "close supersession source must descend from the prior close source"
                            .to_string(),
                    )
                })?;
        }
        Ok(CloseId::new(self.insert(StoredObject::Close(value))?))
    }

    pub fn evidence(&self, id: EvidenceId) -> Result<&Evidence, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Evidence(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Evidence,
                actual: object.kind(),
            }),
        }
    }

    pub fn statement(&self, id: StatementId) -> Result<&Statement, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Statement(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Statement,
                actual: object.kind(),
            }),
        }
    }

    pub fn decision(&self, id: DecisionId) -> Result<&Decision, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Decision(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Decision,
                actual: object.kind(),
            }),
        }
    }

    pub fn completeness(&self, id: CompletenessId) -> Result<&Completeness, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Completeness(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Completeness,
                actual: object.kind(),
            }),
        }
    }

    pub fn package(&self, id: PackageId) -> Result<&PolicyPackage, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Package(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Package,
                actual: object.kind(),
            }),
        }
    }

    pub fn compiled_artifact(
        &self,
        id: CompiledArtifactId,
    ) -> Result<&CompiledArtifactObject, StoreError> {
        match self.get(id.hash())? {
            StoredObject::CompiledArtifact(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::CompiledArtifact,
                actual: object.kind(),
            }),
        }
    }

    pub fn analysis_artifact(
        &self,
        id: AnalysisArtifactId,
    ) -> Result<&AnalysisArtifact, StoreError> {
        match self.get(id.hash())? {
            StoredObject::AnalysisArtifact(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::AnalysisArtifact,
                actual: object.kind(),
            }),
        }
    }

    pub fn proof(&self, id: ProofObjectId) -> Result<&ProofObject, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Proof(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Proof,
                actual: object.kind(),
            }),
        }
    }

    pub fn conflict(&self, id: ConflictId) -> Result<&ConflictRecord, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Conflict(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Conflict,
                actual: object.kind(),
            }),
        }
    }

    pub fn commit(&self, id: CommitId) -> Result<&Commit, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Commit(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Commit,
                actual: object.kind(),
            }),
        }
    }

    pub fn close(&self, id: CloseId) -> Result<&Close, StoreError> {
        match self.get(id.hash())? {
            StoredObject::Close(value) => Ok(value),
            object => Err(StoreError::WrongKind {
                hash: id.hash(),
                expected: ObjectKind::Close,
                actual: object.kind(),
            }),
        }
    }

    /// Return the immutable correction chain from `latest` back toward the
    /// original observation.  A cycle is invalid rather than silently looped.
    pub fn evidence_history(&self, latest: EvidenceId) -> Result<Vec<EvidenceId>, StoreError> {
        let mut history = Vec::new();
        let mut current = Some(latest);
        let mut seen = BTreeSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(StoreError::InvalidObject(
                    "evidence correction cycle".to_string(),
                ));
            }
            history.push(id);
            current = self.evidence(id)?.state.target();
        }
        Ok(history)
    }

    /// Return all evidence from `source` that was known at `at`, with
    /// superseded objects removed only when a current snapshot successor
    /// supersedes them.  The result is a set because one source can legitimately contain
    /// many occurrences.  If two branches corrected the same observation in
    /// different ways, both successors are returned; callers must not infer a
    /// winner from object ordering.
    pub fn evidence_as_known_at(
        &self,
        source: impl AsRef<str>,
        at: CommitId,
    ) -> Result<Vec<EvidenceId>, StoreError> {
        self.commit(at)?;
        let source = source.as_ref();
        let mut pending = vec![at];
        let mut visited = BTreeSet::new();
        let mut reachable = BTreeSet::new();
        while let Some(commit_id) = pending.pop() {
            if !visited.insert(commit_id) {
                continue;
            }
            let commit = self.commit(commit_id)?;
            let mut found_source = false;
            for id in &commit.evidence {
                let evidence = self.evidence(*id)?;
                if evidence.source == source {
                    reachable.insert(*id);
                    found_source = true;
                }
            }
            // Commits carry snapshots of their roots.  Once a commit on a
            // path names this source, its parent roots are historical context
            // rather than additional current evidence.  Empty analysis or
            // metadata commits inherit the source snapshot from their parent.
            if !found_source {
                pending.extend(commit.parents.iter().copied());
            }
        }

        let mut superseded = BTreeSet::new();
        for id in &reachable {
            if let Some(target) = self.evidence(*id)?.state.supersedes()
                && reachable.contains(&target)
            {
                superseded.insert(target);
            }
        }
        Ok(reachable.difference(&superseded).copied().collect())
    }

    /// Compatibility spelling for callers that phrase the query as
    /// “as-known-at”.
    pub fn as_known_at(
        &self,
        source: impl AsRef<str>,
        at: CommitId,
    ) -> Result<Vec<EvidenceId>, StoreError> {
        self.evidence_as_known_at(source, at)
    }

    /// Perform a semantic three-way merge.  Evidence is monotone set union;
    /// choices and claims are selected three-way, with divergent changes
    /// retained together and returned as explicit conflicts.
    pub fn merge_three_way(
        &mut self,
        base: CommitId,
        left: CommitId,
        right: CommitId,
        author: impl Into<String>,
    ) -> Result<MergeResult, StoreError> {
        // A three-way merge is only meaningful when the caller supplies one
        // commit that is an ancestor of both heads.  Validate all three
        // addresses before walking ancestry so a forged/mistyped typed ID
        // cannot be mistaken for an empty branch or an implicit base.
        self.validate_merge_inputs(base, left, right)?;
        let base_value = self.commit(base)?.clone();
        let left_value = self.commit(left)?.clone();
        let right_value = self.commit(right)?.clone();

        let (decisions, decision_conflicts) = self.merge_decisions(
            &base_value.decisions,
            &left_value.decisions,
            &right_value.decisions,
        )?;
        let (packages, package_conflicts) = self.merge_packages(
            &base_value.packages,
            &left_value.packages,
            &right_value.packages,
        )?;
        let (completeness, completeness_conflicts) = self.merge_completeness(
            &base_value.completeness,
            &left_value.completeness,
            &right_value.completeness,
        )?;

        let (evidence, evidence_conflicts) = self.merge_evidence(
            &base_value.evidence,
            &left_value.evidence,
            &right_value.evidence,
        )?;

        let (statements, statement_conflicts) = self.merge_statements(
            &base_value.statements,
            &left_value.statements,
            &right_value.statements,
        )?;

        let mut proofs = base_value.proofs.clone();
        proofs.extend(left_value.proofs.iter().copied());
        proofs.extend(right_value.proofs.iter().copied());
        canonicalize_vec(&mut proofs);

        let mut conflicts = evidence_conflicts;
        conflicts.extend(statement_conflicts);
        conflicts.extend(decision_conflicts);
        conflicts.extend(package_conflicts);
        conflicts.extend(completeness_conflicts);
        for conflict in &mut conflicts {
            canonicalize_merge_conflict(conflict);
        }
        conflicts.sort();
        conflicts.dedup();

        let mut conflict_objects = base_value.conflicts.clone();
        conflict_objects.extend(left_value.conflicts.iter().copied());
        conflict_objects.extend(right_value.conflicts.iter().copied());
        for conflict in &conflicts {
            conflict_objects.push(self.put_conflict(ConflictRecord::unresolved(conflict.clone()))?);
        }
        canonicalize_vec(&mut conflict_objects);
        let superseded = conflict_objects
            .iter()
            .filter_map(|id| self.conflict(*id).ok()?.supersedes)
            .collect::<BTreeSet<_>>();
        conflict_objects.retain(|id| !superseded.contains(id));

        let merged = Commit::new(
            [left, right],
            evidence,
            statements,
            decisions,
            completeness,
            packages,
            proofs,
            author,
        )
        .with_conflicts(conflict_objects.clone());
        let commit = self.put_commit(merged)?;
        let mut unresolved_conflicts = Vec::new();
        for id in &conflict_objects {
            if !self.conflict(*id)?.is_resolved() {
                unresolved_conflicts.push(*id);
            }
        }
        Ok(MergeResult {
            commit,
            conflicts,
            conflict_objects,
            unresolved_conflicts,
        })
    }

    pub fn merge(
        &mut self,
        base: CommitId,
        left: CommitId,
        right: CommitId,
        author: impl Into<String>,
    ) -> Result<MergeResult, StoreError> {
        self.merge_three_way(base, left, right, author)
    }

    fn validate_commit(&self, value: &Commit) -> Result<(), StoreError> {
        if value.schema_version != SCHEMA_VERSION {
            return Err(StoreError::InvalidObject(format!(
                "unsupported commit schema version {}",
                value.schema_version
            )));
        }
        for parent in &value.parents {
            self.require_kind(parent.hash(), ObjectKind::Commit)?;
        }
        for id in &value.evidence {
            self.require_kind(id.hash(), ObjectKind::Evidence)?;
        }
        for id in &value.statements {
            self.require_kind(id.hash(), ObjectKind::Statement)?;
        }
        for id in &value.decisions {
            self.require_kind(id.hash(), ObjectKind::Decision)?;
        }
        for id in &value.completeness {
            self.require_kind(id.hash(), ObjectKind::Completeness)?;
        }
        for id in &value.packages {
            self.require_kind(id.hash(), ObjectKind::Package)?;
        }
        for id in &value.proofs {
            self.require_kind(id.hash(), ObjectKind::Proof)?;
        }
        for id in &value.conflicts {
            self.require_kind(id.hash(), ObjectKind::Conflict)?;
            if let Some(decision) = self.conflict(*id)?.resolution
                && !value.decisions.contains(&decision)
            {
                return Err(StoreError::InvalidObject(
                    "conflict resolution decision must be pinned by the same commit".into(),
                ));
            }
        }
        let resolutions = value
            .conflicts
            .iter()
            .filter_map(|id| self.conflict(*id).ok()?.supersedes)
            .collect::<BTreeSet<_>>();
        for parent in &value.parents {
            for inherited in &self.commit(*parent)?.conflicts {
                if !value.conflicts.contains(inherited) && !resolutions.contains(inherited) {
                    return Err(StoreError::InvalidObject(format!(
                        "commit drops unresolved conflict {inherited} without a resolution"
                    )));
                }
            }
        }
        Ok(())
    }

    fn require_kind(&self, hash: ContentHash, expected: ObjectKind) -> Result<(), StoreError> {
        let object = self.get(hash)?;
        if object.kind() == expected {
            Ok(())
        } else {
            Err(StoreError::WrongKind {
                hash,
                expected,
                actual: object.kind(),
            })
        }
    }

    fn validate_merge_inputs(
        &self,
        base: CommitId,
        left: CommitId,
        right: CommitId,
    ) -> Result<(), StoreError> {
        // Resolve every input first.  In particular, require_ancestor must
        // not accept `base == branch` before establishing that the address is
        // actually present and names a commit.
        self.commit(base)?;
        self.commit(left)?;
        self.commit(right)?;
        self.require_ancestor(base, left)?;
        self.require_ancestor(base, right)
    }

    fn require_ancestor(&self, base: CommitId, branch: CommitId) -> Result<(), StoreError> {
        let mut pending = vec![branch];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if current == base {
                return Ok(());
            }
            if !visited.insert(current) {
                continue;
            }
            let commit = self.commit(current)?;
            pending.extend(commit.parents.iter().copied());
        }
        Err(StoreError::BaseNotAncestor { base, branch })
    }

    fn merge_decisions(
        &self,
        base: &[DecisionId],
        left: &[DecisionId],
        right: &[DecisionId],
    ) -> Result<(Vec<DecisionId>, Vec<MergeConflict>), StoreError> {
        let base_groups = self.decision_groups(base)?;
        let left_groups = self.decision_groups(left)?;
        let right_groups = self.decision_groups(right)?;
        let keys = union_keys(&base_groups, &left_groups, &right_groups);
        let mut selected = Vec::new();
        let mut conflicts = Vec::new();
        for (subject, scope) in keys {
            let key = (subject.clone(), scope.clone());
            let b = base_groups.get(&key).cloned().unwrap_or_default();
            let l = left_groups.get(&key).cloned().unwrap_or_default();
            let r = right_groups.get(&key).cloned().unwrap_or_default();
            let chosen = if l == r {
                l.clone()
            } else if l == b {
                r.clone()
            } else if r == b {
                l.clone()
            } else {
                let mut chosen = l.clone();
                chosen.extend(r.iter().copied());
                canonicalize_vec(&mut chosen);
                chosen
            };
            // A delete-versus-edit is a conflict even when the selected
            // result contains only the edited value: the empty branch would
            // otherwise disappear from `chosen` and the divergence would be
            // silently accepted.  IDs may differ for harmless metadata
            // revisions, so compare selected values rather than addresses.
            let both_changed = l != b && r != b;
            let changed_selection =
                self.decision_selections(&l)? != self.decision_selections(&r)?;
            if (self.decision_values_conflict(&chosen)? || (both_changed && changed_selection))
                && (l != b || r != b)
            {
                conflicts.push(MergeConflict::Decisions {
                    subject,
                    scope,
                    left: l,
                    right: r,
                });
            }
            selected.extend(chosen);
        }
        canonicalize_vec(&mut selected);
        Ok((selected, conflicts))
    }

    fn merge_evidence(
        &self,
        base: &[EvidenceId],
        left: &[EvidenceId],
        right: &[EvidenceId],
    ) -> Result<(Vec<EvidenceId>, Vec<MergeConflict>), StoreError> {
        // Evidence is normally monotone set union.  Corrections and
        // tombstones are the exception: two successors of the same target
        // are unresolved semantic alternatives and must be surfaced as a
        // conflict while still retaining both immutable objects.
        let mut selected = base.to_vec();
        selected.extend(left.iter().copied());
        selected.extend(right.iter().copied());
        canonicalize_vec(&mut selected);

        let base_groups = self.evidence_supersession_groups(base)?;
        let left_groups = self.evidence_supersession_groups(left)?;
        let right_groups = self.evidence_supersession_groups(right)?;
        let keys = union_keys(&base_groups, &left_groups, &right_groups);
        let mut conflicts = Vec::new();
        for supersedes in keys {
            let b = base_groups.get(&supersedes).cloned().unwrap_or_default();
            let l = left_groups.get(&supersedes).cloned().unwrap_or_default();
            let r = right_groups.get(&supersedes).cloned().unwrap_or_default();
            if l.len() > 1 || r.len() > 1 || (l != r && l != b && r != b) {
                conflicts.push(MergeConflict::Evidence {
                    supersedes,
                    left: l,
                    right: r,
                });
            }
        }
        let mut identities = BTreeMap::<(String, String), Vec<EvidenceId>>::new();
        for id in &selected {
            let evidence = self.evidence(*id)?;
            if !matches!(evidence.state, EvidenceState::Present) {
                continue;
            }
            let identity = evidence.external.as_ref().map_or_else(
                || format!("occurrence:{}", evidence.occurrence),
                |external| format!("external:{}", external.as_str()),
            );
            identities
                .entry((evidence.source.clone(), identity))
                .or_default()
                .push(*id);
        }
        for ((source, identity), mut alternatives) in identities {
            alternatives.sort();
            alternatives.dedup();
            let contents = alternatives
                .iter()
                .map(|id| self.evidence(*id).map(Evidence::content_hash))
                .collect::<Result<BTreeSet<_>, _>>()?;
            if contents.len() > 1 {
                conflicts.push(MergeConflict::EvidenceIdentity {
                    source,
                    identity,
                    alternatives,
                });
            }
        }
        Ok((selected, conflicts))
    }

    fn merge_statements(
        &self,
        base: &[StatementId],
        left: &[StatementId],
        right: &[StatementId],
    ) -> Result<(Vec<StatementId>, Vec<MergeConflict>), StoreError> {
        let base_groups = self.statement_groups(base)?;
        let left_groups = self.statement_groups(left)?;
        let right_groups = self.statement_groups(right)?;
        let keys = union_keys(&base_groups, &left_groups, &right_groups);
        let mut selected = Vec::new();
        let mut conflicts = Vec::new();
        for (subject, predicate, value) in keys {
            let key = (subject.clone(), predicate.clone(), value.clone());
            let mut chosen = base_groups.get(&key).cloned().unwrap_or_default();
            if let Some(ids) = left_groups.get(&key) {
                chosen.extend(ids.iter().copied());
            }
            if let Some(ids) = right_groups.get(&key) {
                chosen.extend(ids.iter().copied());
            }
            canonicalize_vec(&mut chosen);
            if self.statement_polarities_conflict(&chosen)? {
                conflicts.push(MergeConflict::Statements {
                    subject,
                    predicate,
                    value,
                    left: left_groups.get(&key).cloned().unwrap_or_default(),
                    right: right_groups.get(&key).cloned().unwrap_or_default(),
                });
            }
            selected.extend(chosen);
        }
        canonicalize_vec(&mut selected);
        Ok((selected, conflicts))
    }

    fn statement_groups(
        &self,
        ids: &[StatementId],
    ) -> Result<BTreeMap<StatementKey, Vec<StatementId>>, StoreError> {
        let mut groups: BTreeMap<StatementKey, Vec<StatementId>> = BTreeMap::new();
        for id in ids {
            let statement = self.statement(*id)?;
            groups
                .entry((
                    statement.subject.clone(),
                    statement.predicate.clone(),
                    statement.value.clone(),
                ))
                .or_default()
                .push(*id);
        }
        for group in groups.values_mut() {
            group.sort();
            group.dedup();
        }
        Ok(groups)
    }

    fn statement_polarities_conflict(&self, ids: &[StatementId]) -> Result<bool, StoreError> {
        let mut positive = false;
        let mut negative = false;
        for id in ids {
            if self.statement(*id)?.negative {
                negative = true;
            } else {
                positive = true;
            }
        }
        Ok(positive && negative)
    }

    fn evidence_supersession_groups(
        &self,
        ids: &[EvidenceId],
    ) -> Result<BTreeMap<EvidenceId, Vec<EvidenceId>>, StoreError> {
        let mut groups: BTreeMap<EvidenceId, Vec<EvidenceId>> = BTreeMap::new();
        for id in ids {
            if let Some(target) = self.evidence(*id)?.state.supersedes() {
                groups.entry(target).or_default().push(*id);
            }
        }
        for group in groups.values_mut() {
            group.sort();
            group.dedup();
        }
        Ok(groups)
    }

    fn decision_values_conflict(&self, ids: &[DecisionId]) -> Result<bool, StoreError> {
        Ok(self.decision_selections(ids)?.len() > 1)
    }

    fn decision_selections(&self, ids: &[DecisionId]) -> Result<BTreeSet<String>, StoreError> {
        let mut selected = BTreeSet::new();
        for id in ids {
            selected.insert(self.decision(*id)?.selected.clone());
        }
        Ok(selected)
    }

    fn decision_groups(&self, ids: &[DecisionId]) -> Result<DecisionGroups, StoreError> {
        let mut groups: DecisionGroups = BTreeMap::new();
        for id in ids {
            let decision = self.decision(*id)?;
            groups
                .entry((decision.subject.clone(), decision.scope.clone()))
                .or_default()
                .push(*id);
        }
        for group in groups.values_mut() {
            let mut keyed = Vec::with_capacity(group.len());
            for id in group.drain(..) {
                let decision = self.decision(id)?;
                keyed.push((decision.selected.clone(), id));
            }
            keyed.sort();
            group.extend(keyed.into_iter().map(|(_, id)| id));
        }
        Ok(groups)
    }

    fn merge_packages(
        &self,
        base: &[PackageId],
        left: &[PackageId],
        right: &[PackageId],
    ) -> Result<(Vec<PackageId>, Vec<MergeConflict>), StoreError> {
        let base_groups = self.package_groups(base)?;
        let left_groups = self.package_groups(left)?;
        let right_groups = self.package_groups(right)?;
        let keys = union_keys(&base_groups, &left_groups, &right_groups);
        let mut selected = Vec::new();
        let mut conflicts = Vec::new();
        for name in keys {
            let b = base_groups.get(&name).cloned().unwrap_or_default();
            let l = left_groups.get(&name).cloned().unwrap_or_default();
            let r = right_groups.get(&name).cloned().unwrap_or_default();
            let chosen = if l == r {
                l.clone()
            } else if l == b {
                r.clone()
            } else if r == b {
                l.clone()
            } else {
                let mut chosen = l.clone();
                chosen.extend(r.iter().copied());
                canonicalize_vec(&mut chosen);
                conflicts.push(MergeConflict::Policies {
                    name: name.clone(),
                    left: l.clone(),
                    right: r.clone(),
                });
                chosen
            };
            if self.package_versions_conflict(&chosen)? && (l != b || r != b) {
                conflicts.push(MergeConflict::Policies {
                    name,
                    left: l,
                    right: r,
                });
            }
            selected.extend(chosen);
        }
        canonicalize_vec(&mut selected);
        Ok((selected, conflicts))
    }

    fn package_groups(
        &self,
        ids: &[PackageId],
    ) -> Result<BTreeMap<String, Vec<PackageId>>, StoreError> {
        let mut groups: BTreeMap<String, Vec<PackageId>> = BTreeMap::new();
        for id in ids {
            groups
                .entry(self.package(*id)?.name.clone())
                .or_default()
                .push(*id);
        }
        for group in groups.values_mut() {
            let mut keyed = Vec::with_capacity(group.len());
            for id in group.drain(..) {
                let package = self.package(id)?;
                keyed.push((package.version.clone(), id));
            }
            keyed.sort();
            group.extend(keyed.into_iter().map(|(_, id)| id));
        }
        Ok(groups)
    }

    fn package_versions_conflict(&self, ids: &[PackageId]) -> Result<bool, StoreError> {
        for id in ids {
            // Resolve every address before reporting a conflict, so a
            // malformed commit still fails as a storage error.
            self.package(*id)?;
        }
        Ok(ids.len() > 1)
    }

    fn merge_completeness(
        &self,
        base: &[CompletenessId],
        left: &[CompletenessId],
        right: &[CompletenessId],
    ) -> Result<(Vec<CompletenessId>, Vec<MergeConflict>), StoreError> {
        let base_groups = self.completeness_groups(base)?;
        let left_groups = self.completeness_groups(left)?;
        let right_groups = self.completeness_groups(right)?;
        let keys = union_keys(&base_groups, &left_groups, &right_groups);
        let mut selected = Vec::new();
        let mut conflicts = Vec::new();
        for key in keys {
            let b = base_groups.get(&key).cloned().unwrap_or_default();
            let l = left_groups.get(&key).cloned().unwrap_or_default();
            let r = right_groups.get(&key).cloned().unwrap_or_default();
            let chosen = if l == r {
                l.clone()
            } else if l == b {
                r.clone()
            } else if r == b {
                l.clone()
            } else {
                let mut chosen = l.clone();
                chosen.extend(r.iter().copied());
                canonicalize_vec(&mut chosen);
                chosen
            };
            // Compare the two branch snapshots, rather than the selected
            // result against itself.  When one branch leaves the base claim
            // unchanged and the other replaces it, selecting only the
            // changed branch would otherwise hide an overlapping opposite
            // completeness assertion.
            let both_changed = l != b && r != b;
            let delete_edit_conflict = both_changed && (l.is_empty() != r.is_empty());
            if self.completeness_conflicts(&l, &r)? || delete_edit_conflict {
                conflicts.push(MergeConflict::Completeness {
                    relation: key.0.clone(),
                    source: key.1.clone(),
                    scope: key.2.clone(),
                    left_bounds: self.completeness_bounds(&l)?,
                    right_bounds: self.completeness_bounds(&r)?,
                    left: l,
                    right: r,
                });
            }
            selected.extend(chosen);
        }
        canonicalize_vec(&mut selected);
        Ok((selected, conflicts))
    }

    fn completeness_groups(
        &self,
        ids: &[CompletenessId],
    ) -> Result<CompletenessGroups, StoreError> {
        let mut groups: CompletenessGroups = BTreeMap::new();
        for id in ids {
            let value = self.completeness(*id)?;
            groups.entry(value.semantic_key()).or_default().push(*id);
        }
        for group in groups.values_mut() {
            group.sort();
        }
        Ok(groups)
    }

    fn completeness_conflicts(
        &self,
        left: &[CompletenessId],
        right: &[CompletenessId],
    ) -> Result<bool, StoreError> {
        for left_id in left {
            let left_claim = self.completeness(*left_id)?;
            for right_id in right {
                let right_claim = self.completeness(*right_id)?;
                if left_claim.complete != right_claim.complete
                    && intervals_overlap(left_claim, right_claim)
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn completeness_bounds(
        &self,
        ids: &[CompletenessId],
    ) -> Result<Vec<CompletenessBounds>, StoreError> {
        ids.iter()
            .map(|id| {
                let claim = self.completeness(*id)?;
                Ok(CompletenessBounds {
                    from: claim.from,
                    until: claim.until,
                    complete: claim.complete,
                })
            })
            .collect()
    }
}

fn canonical_set<T: Ord>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort();
    values.dedup();
    values
}

fn canonical_proof_set(
    values: impl IntoIterator<Item = CanonicalProofId>,
) -> Vec<CanonicalProofId> {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort();
    values.dedup();
    values
}

fn canonicalize_merge_conflict(conflict: &mut MergeConflict) {
    match conflict {
        MergeConflict::Evidence { left, right, .. } => {
            canonicalize_vec(left);
            canonicalize_vec(right);
            if right.as_slice() < left.as_slice() {
                std::mem::swap(left, right);
            }
        }
        MergeConflict::EvidenceIdentity { alternatives, .. } => {
            canonicalize_vec(alternatives);
        }
        MergeConflict::Decisions { left, right, .. } => {
            canonicalize_vec(left);
            canonicalize_vec(right);
            if right.as_slice() < left.as_slice() {
                std::mem::swap(left, right);
            }
        }
        MergeConflict::Statements { left, right, .. } => {
            canonicalize_vec(left);
            canonicalize_vec(right);
            if right.as_slice() < left.as_slice() {
                std::mem::swap(left, right);
            }
        }
        MergeConflict::Policies { left, right, .. } => {
            canonicalize_vec(left);
            canonicalize_vec(right);
            if right.as_slice() < left.as_slice() {
                std::mem::swap(left, right);
            }
        }
        MergeConflict::Completeness {
            left,
            right,
            left_bounds,
            right_bounds,
            ..
        } => {
            let canonicalize =
                |ids: &mut Vec<CompletenessId>, bounds: &mut Vec<CompletenessBounds>| {
                    let mut pairs = ids.drain(..).zip(bounds.drain(..)).collect::<Vec<_>>();
                    pairs.sort();
                    pairs.dedup();
                    ids.extend(pairs.iter().map(|(id, _)| *id));
                    bounds.extend(pairs.into_iter().map(|(_, bounds)| bounds));
                };
            canonicalize(left, left_bounds);
            canonicalize(right, right_bounds);
            if (right.as_slice(), right_bounds.as_slice())
                < (left.as_slice(), left_bounds.as_slice())
            {
                std::mem::swap(left, right);
                std::mem::swap(left_bounds, right_bounds);
            }
        }
    }
}

fn canonicalize_vec<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

fn intervals_overlap(left: &Completeness, right: &Completeness) -> bool {
    if let (Some(until), Some(from)) = (left.until, right.from)
        && until < from
    {
        return false;
    }
    if let (Some(until), Some(from)) = (right.until, left.from)
        && until < from
    {
        return false;
    }
    true
}

type DecisionKey = (String, String);
type DecisionGroups = BTreeMap<DecisionKey, Vec<DecisionId>>;
type StatementKey = (String, String, String);
type CompletenessScopeKey = (String, String, String);
type CompletenessGroups = BTreeMap<CompletenessScopeKey, Vec<CompletenessId>>;

fn union_keys<K, V>(a: &BTreeMap<K, V>, b: &BTreeMap<K, V>, c: &BTreeMap<K, V>) -> Vec<K>
where
    K: Clone + Ord,
{
    let mut keys = BTreeSet::new();
    keys.extend(a.keys().cloned());
    keys.extend(b.keys().cloned());
    keys.extend(c.keys().cloned());
    keys.into_iter().collect()
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_hash(out: &mut Vec<u8>, value: ContentHash) {
    out.extend_from_slice(value.as_bytes());
}

fn put_hashes(out: &mut Vec<u8>, values: &[ContentHash]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_hash(out, *value);
    }
}

fn put_date(out: &mut Vec<u8>, value: Date) {
    put_i32(out, value.year);
    out.push(value.month);
    out.push(value.day);
}

fn put_optional_date(out: &mut Vec<u8>, value: Option<Date>) {
    match value {
        Some(value) => {
            out.push(1);
            put_date(out, value);
        }
        None => out.push(0),
    }
}

fn put_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_string(out, value);
        }
        None => out.push(0),
    }
}

fn put_optional_hash(out: &mut Vec<u8>, value: Option<ContentHash>) {
    match value {
        Some(value) => {
            out.push(1);
            put_hash(out, value);
        }
        None => out.push(0),
    }
}

fn put_ids<K: Kind>(out: &mut Vec<u8>, values: &[ObjectId<K>]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_hash(out, value.hash());
    }
}

fn put_strings(out: &mut Vec<u8>, values: &[String]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_string(out, value);
    }
}

fn put_signatures(out: &mut Vec<u8>, values: &[Signature]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_string(out, &value.signer);
        put_string(out, &value.algorithm);
        put_bytes(out, &value.bytes);
    }
}

fn normalized_content_hash(content: &[u8]) -> ContentHash {
    ContentHash::domain_separated(EVIDENCE_CONTENT_DOMAIN, content)
}

fn encode_evidence(out: &mut Vec<u8>, value: &Evidence) {
    put_string(out, &value.occurrence);
    put_string(out, &value.source);
    put_hash(out, value.normalized_content);
    put_optional_string(out, value.external.as_ref().map(ExternalId::as_str));
    put_bytes(out, &value.content);
    match &value.state {
        EvidenceState::Present => out.push(0),
        EvidenceState::Correction {
            supersedes,
            scope,
            reason,
            authority,
        } => {
            out.push(1);
            put_hash(out, supersedes.hash());
            put_string(out, scope);
            put_string(out, reason);
            put_string(out, authority);
        }
        EvidenceState::Tombstone {
            supersedes,
            reason,
            authority,
        } => {
            out.push(2);
            put_hash(out, supersedes.hash());
            put_string(out, reason);
            put_string(out, authority);
        }
        EvidenceState::Unavailable { reason } => {
            out.push(3);
            put_string(out, reason);
        }
        EvidenceState::Redacted { reason } => {
            out.push(4);
            put_string(out, reason);
        }
    }
}

fn encode_statement(out: &mut Vec<u8>, value: &Statement) {
    put_string(out, &value.subject);
    put_string(out, &value.predicate);
    put_string(out, &value.value);
    out.push(u8::from(value.negative));
}

fn encode_decision(out: &mut Vec<u8>, value: &Decision) {
    put_string(out, &value.subject);
    put_string(out, &value.selected);
    put_strings(out, &value.rejected);
    put_string(out, &value.scope);
    put_optional_string(out, value.rationale.as_deref());
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
}

fn encode_completeness(out: &mut Vec<u8>, value: &Completeness) {
    put_string(out, &value.relation);
    put_string(out, &value.source);
    put_string(out, &value.scope);
    put_optional_date(out, value.from);
    put_optional_date(out, value.until);
    out.push(u8::from(value.complete));
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
}

fn encode_package(out: &mut Vec<u8>, value: &PolicyPackage) {
    put_string(out, &value.name);
    put_string(out, &value.version);
    put_u64(out, value.manifest.len() as u64);
    for (key, item) in &value.manifest {
        put_string(out, key);
        put_string(out, item);
    }
    put_bytes(out, &value.body);
    let mut dependencies = value.dependencies.clone();
    dependencies.sort();
    dependencies.dedup();
    put_hashes(out, &dependencies);
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
}

fn encode_compiled_artifact(out: &mut Vec<u8>, value: &CompiledArtifactObject) {
    put_hash(out, value.artifact.artifact_hash());
    put_bytes(out, &value.artifact.canonical_bytes());
}

fn encode_proof(out: &mut Vec<u8>, value: &ProofObject) {
    encode_canonical_proof(out, &value.proof);
    put_u64(out, value.roots.len() as u64);
    for root in &value.roots {
        put_hash(out, *root);
    }
}

fn encode_conflict_record(out: &mut Vec<u8>, value: &ConflictRecord) {
    encode_merge_conflict(out, &value.conflict);
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
    put_optional_hash(out, value.resolution.map(ObjectId::hash));
    put_optional_string(out, value.rationale.as_deref());
}

fn encode_merge_conflict(out: &mut Vec<u8>, value: &MergeConflict) {
    match value {
        MergeConflict::Evidence {
            supersedes,
            left,
            right,
        } => {
            out.push(0);
            put_hash(out, supersedes.hash());
            put_ids(out, left);
            put_ids(out, right);
        }
        MergeConflict::EvidenceIdentity {
            source,
            identity,
            alternatives,
        } => {
            out.push(1);
            put_string(out, source);
            put_string(out, identity);
            put_ids(out, alternatives);
        }
        MergeConflict::Statements {
            subject,
            predicate,
            value,
            left,
            right,
        } => {
            out.push(2);
            put_string(out, subject);
            put_string(out, predicate);
            put_string(out, value);
            put_ids(out, left);
            put_ids(out, right);
        }
        MergeConflict::Decisions {
            subject,
            scope,
            left,
            right,
        } => {
            out.push(3);
            put_string(out, subject);
            put_string(out, scope);
            put_ids(out, left);
            put_ids(out, right);
        }
        MergeConflict::Policies { name, left, right } => {
            out.push(4);
            put_string(out, name);
            put_ids(out, left);
            put_ids(out, right);
        }
        MergeConflict::Completeness {
            relation,
            source,
            scope,
            left,
            right,
            left_bounds,
            right_bounds,
        } => {
            out.push(5);
            put_string(out, relation);
            put_string(out, source);
            put_string(out, scope);
            put_ids(out, left);
            put_ids(out, right);
            encode_completeness_bounds(out, left_bounds);
            encode_completeness_bounds(out, right_bounds);
        }
    }
}

fn encode_completeness_bounds(out: &mut Vec<u8>, values: &[CompletenessBounds]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_optional_date(out, value.from);
        put_optional_date(out, value.until);
        out.push(u8::from(value.complete));
    }
}

/// Encode the complete canonical proof graph, rather than only its root IDs.
/// Node IDs are content addresses, but retaining the fields here makes a
/// persisted proof independently reloadable and lets the store detect any
/// mismatch between an address and its node content.
fn encode_canonical_proof(out: &mut Vec<u8>, proof: &CanonicalProof) {
    put_bytes(out, &proof.canonical_bytes());
}

fn encode_commit(out: &mut Vec<u8>, value: &Commit) {
    put_ids(out, &value.parents);
    put_ids(out, &value.evidence);
    put_ids(out, &value.statements);
    put_ids(out, &value.decisions);
    put_ids(out, &value.completeness);
    put_ids(out, &value.packages);
    put_ids(out, &value.proofs);
    put_ids(out, &value.conflicts);
    put_string(out, &value.schema_version);
    put_string(out, &value.author);
    put_signatures(out, &value.signatures);
}

fn encode_close(out: &mut Vec<u8>, value: &Close) {
    put_date(out, value.period.from);
    put_date(out, value.period.until);
    put_string(out, value.book.as_str());
    put_ids(out, &value.policies);
    put_hash(out, value.source.hash());
    put_hash(out, value.analysis_artifact.hash());
    put_strings(out, &value.exceptions);
    put_signatures(out, &value.signatures);
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
}

fn encode_analysis_artifact(out: &mut Vec<u8>, value: &AnalysisArtifact) {
    put_hash(out, value.analysis_commit.hash());
    put_string(out, value.book.as_str());
    put_date(out, value.period.from);
    put_date(out, value.period.until);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoVerifier;

    impl SignatureVerifier for EchoVerifier {
        fn verify(
            &self,
            _signer: &str,
            algorithm: &str,
            payload: ContentHash,
            signature: &[u8],
        ) -> bool {
            algorithm == "test-only" && signature == payload.as_bytes()
        }
    }

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn root_commit(store: &mut ObjectStore, evidence: Vec<EvidenceId>) -> CommitId {
        store
            .put_commit(Commit::new([], evidence, [], [], [], [], [], "test"))
            .unwrap()
    }

    #[test]
    fn equal_content_deduplicates_but_equal_rows_do_not_collapse() {
        let mut store = ObjectStore::new();
        let first = store
            .put_evidence(Evidence::new("row/1", "bank", b"20 USD".to_vec()))
            .unwrap();
        let same = store
            .put_evidence(Evidence::new("row/1", "bank", b"20 USD".to_vec()))
            .unwrap();
        let second = store
            .put_evidence(Evidence::new("row/2", "bank", b"20 USD".to_vec()))
            .unwrap();
        assert_eq!(first, same);
        assert_ne!(first, second);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn package_conversion_preserves_dependencies_and_rejects_lossy_fields() {
        let first = ContentHash::domain_separated("test/package-dependency", b"first");
        let second = ContentHash::domain_separated("test/package-dependency", b"second");
        let stored = PolicyPackage::new(
            "lots/x",
            "1",
            b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
        )
        .with_dependencies([second, first, first]);
        let executable = stored.to_executable().unwrap();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(executable.dependencies, expected);
        assert_eq!(
            executable.body,
            String::from_utf8(stored.body.clone()).unwrap()
        );

        let metadata = stored.clone().with_manifest([("scope", "tax")]);
        assert!(matches!(
            metadata.to_executable(),
            Err(StoreError::InvalidObject(reason))
                if reason.contains("cannot be represented")
        ));
        let malformed = PolicyPackage::new("lots/x", "1", vec![0xff]);
        assert!(matches!(
            malformed.to_executable(),
            Err(StoreError::InvalidObject(reason)) if reason.contains("valid UTF-8")
        ));
    }

    #[test]
    fn commit_roots_are_sorted_deduplicated_and_reproducible() {
        let mut store = ObjectStore::new();
        let a = store
            .put_evidence(Evidence::new("a", "source", b"a".to_vec()))
            .unwrap();
        let b = store
            .put_evidence(Evidence::new("b", "source", b"b".to_vec()))
            .unwrap();
        let first = Commit::new([], [b, a, b], [], [], [], [], [], "author");
        let second = Commit::new([], [a, b], [], [], [], [], [], "author");
        assert_eq!(first, second);
        let first_id = store.put_commit(first).unwrap();
        let second_id = store.put_commit(second).unwrap();
        assert_eq!(first_id, second_id);
        assert_eq!(
            store.commit(first_id).unwrap().evidence,
            vec![a.min(b), a.max(b)]
        );

        let unsigned = Commit::new([], [a], [], [], [], [], [], "signed-author");
        let mut noncanonical = unsigned.clone();
        noncanonical.evidence = vec![a, a];
        assert_eq!(noncanonical.signing_hash(), unsigned.signing_hash());
        let signed = unsigned.clone().with_signatures([Signature::new(
            "alice",
            "test-only",
            unsigned.signing_hash().as_bytes().to_vec(),
        )]);
        assert!(matches!(
            store.put_commit(signed.clone()),
            Err(StoreError::InvalidObject(_))
        ));
        store
            .put_commit_verified(signed, &EchoVerifier)
            .expect("canonical commit signature verifies");
    }

    #[test]
    fn corrections_are_new_objects_and_history_is_walkable() {
        let mut store = ObjectStore::new();
        let original = store
            .put_evidence(Evidence::new("statement/1", "bank", b"wrong".to_vec()))
            .unwrap();
        let corrected = store
            .put_evidence(Evidence::correction(
                "statement/1",
                "bank",
                b"right".to_vec(),
                original,
                "balance",
                "source correction",
                "bank",
            ))
            .unwrap();
        let tombstone = store
            .put_evidence(Evidence::tombstone(
                "statement/1",
                "bank",
                corrected,
                "privacy deletion",
                "data subject",
            ))
            .unwrap();
        assert_ne!(original, corrected);
        assert_ne!(corrected, tombstone);
        assert_eq!(
            store.evidence_history(tombstone).unwrap(),
            vec![tombstone, corrected, original]
        );
        assert!(matches!(
            store.evidence(corrected).unwrap().state,
            EvidenceState::Correction { .. }
        ));
    }

    #[test]
    fn as_known_at_removes_only_reachable_superseded_evidence() {
        let mut store = ObjectStore::new();
        let original = store
            .put_evidence(Evidence::new("source/row", "bank", b"old".to_vec()))
            .unwrap();
        let unrelated = store
            .put_evidence(Evidence::new("source/other", "bank", b"other".to_vec()))
            .unwrap();
        let base = root_commit(&mut store, vec![original, unrelated]);
        let corrected = store
            .put_evidence(Evidence::correction(
                "source/row",
                "bank",
                b"new".to_vec(),
                original,
                "whole",
                "issuer correction",
                "bank",
            ))
            .unwrap();
        let latest = store
            .put_commit(Commit::new(
                [base],
                [corrected, unrelated],
                [],
                [],
                [],
                [],
                [],
                "correction",
            ))
            .unwrap();
        let mut expected_latest = vec![corrected, unrelated];
        expected_latest.sort();
        assert_eq!(
            store.evidence_as_known_at("bank", latest).unwrap(),
            expected_latest
        );
        let mut expected_base = vec![original, unrelated];
        expected_base.sort();
        assert_eq!(
            store.evidence_as_known_at("bank", base).unwrap(),
            expected_base
        );

        let reverted = store
            .put_commit(Commit::new(
                [latest],
                [original, unrelated],
                [],
                [],
                [],
                [],
                [],
                "revert",
            ))
            .unwrap();
        assert_eq!(
            store.evidence_as_known_at("bank", reverted).unwrap(),
            expected_base
        );
    }

    #[test]
    fn divergent_evidence_corrections_are_conflicts_and_both_survive_merge() {
        let mut store = ObjectStore::new();
        let original = store
            .put_evidence(Evidence::new("source/row", "bank", b"old".to_vec()))
            .unwrap();
        let base = root_commit(&mut store, vec![original]);
        let left_evidence = store
            .put_evidence(Evidence::correction(
                "source/row",
                "bank",
                b"left".to_vec(),
                original,
                "amount",
                "left correction",
                "left",
            ))
            .unwrap();
        let right_evidence = store
            .put_evidence(Evidence::tombstone(
                "source/row",
                "bank",
                original,
                "right deletion",
                "right",
            ))
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [left_evidence],
                [],
                [],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [right_evidence],
                [],
                [],
                [],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.conflicts.iter().any(|conflict| matches!(
            conflict,
            MergeConflict::Evidence {
                supersedes,
                left,
                right
            } if *supersedes == original
                && ((left == &vec![left_evidence] && right == &vec![right_evidence])
                    || (left == &vec![right_evidence] && right == &vec![left_evidence]))
        )));
        let merged_value = store.commit(merged.commit).unwrap();
        assert!(merged_value.evidence.contains(&left_evidence));
        assert!(merged_value.evidence.contains(&right_evidence));
    }

    #[test]
    fn divergent_present_rows_with_one_source_identity_are_merge_conflicts() {
        let mut store = ObjectStore::new();
        let base = root_commit(&mut store, Vec::new());
        let left_evidence = store
            .put_evidence(Evidence::new("row/1", "bank", b"left".to_vec()))
            .unwrap();
        let right_evidence = store
            .put_evidence(Evidence::new("row/1", "bank", b"right".to_vec()))
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [left_evidence],
                [],
                [],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [right_evidence],
                [],
                [],
                [],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.conflicts.iter().any(|conflict| matches!(
            conflict,
            MergeConflict::EvidenceIdentity {
                source,
                identity,
                alternatives,
            } if source == "bank"
                && identity == "occurrence:row/1"
                && alternatives.len() == 2
                && alternatives.contains(&left_evidence)
                && alternatives.contains(&right_evidence)
        )));
    }

    #[test]
    fn supersession_cannot_cross_semantic_scope() {
        let mut store = ObjectStore::new();
        let prior_decision = store
            .put_decision(Decision::new("sale/1", "lot/a").with_scope("tax"))
            .unwrap();
        assert!(matches!(
            store.put_decision(
                Decision::new("sale/2", "lot/b")
                    .with_scope("tax")
                    .superseding(prior_decision)
            ),
            Err(StoreError::InvalidObject(reason)) if reason.contains("subject and scope")
        ));
    }

    #[test]
    fn unavailable_redacted_and_deleted_states_remain_distinct() {
        let mut store = ObjectStore::new();
        let unavailable = store
            .put_evidence(Evidence::unavailable("unavailable", "bank", "offline"))
            .unwrap();
        let redacted = store
            .put_evidence(Evidence::redacted("redacted", "bank", "privacy"))
            .unwrap();
        let prior = store
            .put_evidence(Evidence::new("deleted", "bank", b"secret".to_vec()))
            .unwrap();
        let prior_hash = store.evidence(prior).unwrap().content_hash();
        let deleted = store
            .put_evidence(Evidence::deleted(
                "deleted",
                "bank",
                prior_hash,
                prior,
                "privacy deletion",
                "subject",
            ))
            .unwrap();
        assert_ne!(unavailable, redacted);
        assert_ne!(redacted, deleted);
        assert!(matches!(
            store.evidence(unavailable).unwrap().state,
            EvidenceState::Unavailable { .. }
        ));
        assert!(matches!(
            store.evidence(redacted).unwrap().state,
            EvidenceState::Redacted { .. }
        ));
        assert!(store.evidence(deleted).unwrap().state.is_tombstone());
        assert_eq!(store.evidence(deleted).unwrap().content_hash(), prior_hash);

        let false_hash = ContentHash::domain_separated("test", b"not the prior content");
        assert!(matches!(
            store.put_evidence(Evidence::deleted(
                "deleted",
                "bank",
                false_hash,
                prior,
                "bad deletion",
                "subject",
            )),
            Err(StoreError::InvalidObject(reason)) if reason.contains("must match its target")
        ));
    }

    #[test]
    fn evidence_keeps_occurrence_content_and_external_identity_separate() {
        let mut store = ObjectStore::new();
        let first = store
            .put_evidence(
                Evidence::new("row/1", "bank", b"20 USD".to_vec()).with_external("row-184"),
            )
            .unwrap();
        let second = store
            .put_evidence(
                Evidence::new("row/2", "bank", b"20 USD".to_vec()).with_external("row-185"),
            )
            .unwrap();
        let first_value = store.evidence(first).unwrap();
        let second_value = store.evidence(second).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            first_value.content_hash(),
            second_value.content_hash(),
            "normalized content can be shared without collapsing occurrences"
        );
        assert_eq!(first_value.external_id().unwrap().as_str(), "row-184");
        assert_eq!(second_value.external_id().unwrap().as_str(), "row-185");
    }

    #[test]
    fn divergent_decisions_merge_as_conflict_and_preserve_both() {
        let mut store = ObjectStore::new();
        let base_decision = store
            .put_decision(Decision::new("sale/1", "lot/a"))
            .unwrap();
        let left_decision = store
            .put_decision(Decision::new("sale/1", "lot/b"))
            .unwrap();
        let right_decision = store
            .put_decision(Decision::new("sale/1", "lot/c"))
            .unwrap();
        let base = store
            .put_commit(Commit::new([], [], [], [base_decision], [], [], [], "base"))
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [left_decision],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [right_decision],
                [],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(!merged.is_clean());
        assert!(matches!(
            merged.conflicts.first(),
            Some(MergeConflict::Decisions { subject, .. }) if subject == "sale/1"
        ));
        let merged_commit = store.commit(merged.commit).unwrap();
        assert_eq!(merged_commit.decisions.len(), 2);
        assert!(merged_commit.decisions.contains(&left_decision));
        assert!(merged_commit.decisions.contains(&right_decision));
        assert!(!merged_commit.decisions.contains(&base_decision));
        assert_eq!(merged.conflict_objects, merged_commit.conflicts);
        assert_eq!(merged.conflict_objects.len(), 1);
        assert!(
            !store
                .conflict(merged.conflict_objects[0])
                .unwrap()
                .is_resolved()
        );
    }

    #[test]
    fn unresolved_merge_conflict_blocks_close_until_explicit_resolution() {
        let mut store = ObjectStore::new();
        let proof = store
            .put_proof(ProofObject::new([], b"generic proof"))
            .unwrap();
        let base_decision = store
            .put_decision(Decision::new("sale/1", "lot/a"))
            .unwrap();
        let left_decision = store
            .put_decision(Decision::new("sale/1", "lot/b"))
            .unwrap();
        let right_decision = store
            .put_decision(Decision::new("sale/1", "lot/c"))
            .unwrap();
        let base = store
            .put_commit(Commit::new(
                [],
                [],
                [],
                [base_decision],
                [],
                [],
                [proof],
                "base",
            ))
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [left_decision],
                [],
                [],
                [proof],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [right_decision],
                [],
                [],
                [proof],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        let period = Period::new(date("2026-01-01"), date("2026-12-31")).unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period.clone(),
                "tax",
                [],
                merged.commit,
                AnalysisArtifactId::new(proof.hash()),
            )),
            Err(StoreError::InvalidObject(reason)) if reason.contains("unresolved semantic conflict")
        ));
        let merged_snapshot = store.commit(merged.commit).unwrap().clone();
        let descendant = |author: &str| {
            Commit::new(
                [merged.commit],
                merged_snapshot.evidence.clone(),
                merged_snapshot.statements.clone(),
                merged_snapshot.decisions.clone(),
                merged_snapshot.completeness.clone(),
                merged_snapshot.packages.clone(),
                merged_snapshot.proofs.clone(),
                author,
            )
            .with_conflicts(merged_snapshot.conflicts.clone())
        };
        let descendant_left = store.put_commit(descendant("left descendant")).unwrap();
        let descendant_right = store.put_commit(descendant("right descendant")).unwrap();
        let inherited = store
            .merge(merged.commit, descendant_left, descendant_right, "inherit")
            .unwrap();
        assert!(!inherited.is_clean());
        assert!(!inherited.unresolved_conflicts.is_empty());
        assert!(matches!(
            store.put_conflict(ConflictRecord::unresolved(MergeConflict::Decisions {
                subject: "wrong-subject".into(),
                scope: String::new(),
                left: vec![left_decision],
                right: vec![right_decision],
            })),
            Err(StoreError::InvalidObject(reason))
                if reason.contains("does not match")
        ));

        let conflict_id = merged.conflict_objects[0];
        let conflict = store.conflict(conflict_id).unwrap().conflict.clone();
        let unrelated = store
            .put_decision(Decision::new("unrelated/question", "yes"))
            .unwrap();
        assert!(matches!(
            store.put_conflict(ConflictRecord::resolved(
                conflict.clone(),
                conflict_id,
                unrelated,
                "not actually related",
            )),
            Err(StoreError::InvalidObject(reason))
                if reason.contains("explicitly name the conflict")
        ));
        let resolution_decision = store
            .put_decision(Decision::new(
                ConflictRecord::resolution_subject(conflict_id),
                "left branch",
            ))
            .unwrap();
        let resolved_id = store
            .put_conflict(ConflictRecord::resolved(
                conflict,
                conflict_id,
                resolution_decision,
                "review selected the left branch",
            ))
            .unwrap();
        let merged_value = store.commit(merged.commit).unwrap().clone();
        let unpinned_resolution = Commit::new(
            [merged.commit],
            merged_value.evidence.clone(),
            merged_value.statements.clone(),
            merged_value.decisions.clone(),
            merged_value.completeness.clone(),
            merged_value.packages.clone(),
            merged_value.proofs.clone(),
            "resolver",
        )
        .with_conflicts([resolved_id]);
        assert!(matches!(
            store.put_commit(unpinned_resolution),
            Err(StoreError::InvalidObject(reason))
                if reason.contains("must be pinned by the same commit")
        ));
        let resolved_commit = store
            .put_commit(
                Commit::new(
                    [merged.commit],
                    merged_value.evidence,
                    merged_value.statements,
                    merged_value
                        .decisions
                        .into_iter()
                        .chain([resolution_decision]),
                    merged_value.completeness,
                    merged_value.packages,
                    merged_value.proofs,
                    "resolver",
                )
                .with_conflicts([resolved_id]),
            )
            .unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period,
                "tax",
                [],
                resolved_commit,
                AnalysisArtifactId::new(proof.hash()),
            )),
            Err(StoreError::WrongKind {
                expected: ObjectKind::AnalysisArtifact,
                actual: ObjectKind::Proof,
                ..
            })
        ));
    }

    #[test]
    fn preexisting_branch_decision_conflict_is_not_hidden_by_merge() {
        let mut store = ObjectStore::new();
        let base_decision = store
            .put_decision(Decision::new("sale/1", "lot/a"))
            .unwrap();
        let branch_decision = store
            .put_decision(Decision::new("sale/1", "lot/b"))
            .unwrap();
        let base = root_commit(&mut store, Vec::new());
        let left = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [base_decision, branch_decision],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [base_decision],
                [],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(!merged.is_clean());
        assert_eq!(store.commit(merged.commit).unwrap().decisions.len(), 2);
    }

    #[test]
    fn contradictory_statement_polarities_are_preserved_as_merge_conflict() {
        let mut store = ObjectStore::new();
        let positive = store
            .put_statement(Statement::new("account/1", "open", "true"))
            .unwrap();
        let negative = store
            .put_statement(Statement::new("account/1", "open", "true").negative())
            .unwrap();
        let base = root_commit(&mut store, Vec::new());
        let left = store
            .put_commit(Commit::new([base], [], [positive], [], [], [], [], "left"))
            .unwrap();
        let right = store
            .put_commit(Commit::new([base], [], [negative], [], [], [], [], "right"))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.conflicts.iter().any(|conflict| matches!(
            conflict,
            MergeConflict::Statements { subject, predicate, value, .. }
                if subject == "account/1" && predicate == "open" && value == "true"
        )));
        let merged_value = store.commit(merged.commit).unwrap();
        assert!(merged_value.statements.contains(&positive));
        assert!(merged_value.statements.contains(&negative));
    }

    #[test]
    fn evidence_union_does_not_need_a_decision_conflict() {
        let mut store = ObjectStore::new();
        let first = store
            .put_evidence(Evidence::new("row/1", "bank", b"1".to_vec()))
            .unwrap();
        let second = store
            .put_evidence(Evidence::new("row/2", "bank", b"2".to_vec()))
            .unwrap();
        let base = root_commit(&mut store, vec![first]);
        let left = store
            .put_commit(Commit::new(
                [base],
                [first, second],
                [],
                [],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new([base], [first], [], [], [], [], [], "right"))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.is_clean());
        assert_eq!(
            store.commit(merged.commit).unwrap().evidence,
            vec![first.min(second), first.max(second)]
        );
    }

    #[test]
    fn decisions_in_disjoint_scopes_merge_without_conflict() {
        let mut store = ObjectStore::new();
        let base = root_commit(&mut store, Vec::new());
        let left_decision = store
            .put_decision(Decision::new("sale/1", "lot/a").with_scope("this disposal"))
            .unwrap();
        let right_decision = store
            .put_decision(Decision::new("sale/1", "lot/b").with_scope("2026 disposals"))
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [left_decision],
                [],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [right_decision],
                [],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.is_clean());
        assert_eq!(store.commit(merged.commit).unwrap().decisions.len(), 2);
    }

    #[test]
    fn completeness_reports_overlapping_divergent_bounds() {
        let mut store = ObjectStore::new();
        let base = root_commit(&mut store, Vec::new());
        let left_claim = store
            .put_completeness(Completeness::new(
                "bank_activity",
                "statement/september",
                "checking",
                Some(date("2026-09-01")),
                Some(date("2026-09-30")),
            ))
            .unwrap();
        let right_claim = store
            .put_completeness(
                Completeness::new(
                    "bank_activity",
                    "statement/september",
                    "checking",
                    Some(date("2026-09-15")),
                    Some(date("2026-10-15")),
                )
                .incomplete(),
            )
            .unwrap();
        let left = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [],
                [left_claim],
                [],
                [],
                "left",
            ))
            .unwrap();
        let right = store
            .put_commit(Commit::new(
                [base],
                [],
                [],
                [],
                [right_claim],
                [],
                [],
                "right",
            ))
            .unwrap();
        let merged = store.merge(base, left, right, "merge").unwrap();
        assert!(merged.conflicts.iter().any(|conflict| matches!(
            conflict,
            MergeConflict::Completeness {
                left,
                right,
                left_bounds,
                right_bounds,
                ..
            } if left.contains(&left_claim)
                && right.contains(&right_claim)
                && left_bounds[0].from == Some(date("2026-09-01"))
                && right_bounds[0].until == Some(date("2026-10-15"))
        )));
        assert_eq!(store.commit(merged.commit).unwrap().completeness.len(), 2);
    }

    #[test]
    fn merge_rejects_a_base_that_is_not_an_ancestor() {
        let mut store = ObjectStore::new();
        let base = root_commit(&mut store, Vec::new());
        let left = store
            .put_commit(Commit::new([], [], [], [], [], [], [], "unrelated-left"))
            .unwrap();
        let right = store
            .put_commit(Commit::new([], [], [], [], [], [], [], "unrelated-right"))
            .unwrap();
        assert!(matches!(
            store.merge(base, left, right, "merge"),
            Err(StoreError::BaseNotAncestor { base: found, branch })
                if found == base && branch == left
        ));
    }

    #[test]
    fn referenced_roots_must_be_present_before_proof_or_close() {
        let mut store = ObjectStore::new();
        let missing = ContentHash::domain_separated("test/missing", b"root");
        assert!(matches!(
            store.put_proof(ProofObject::new([missing], b"proof")),
            Err(StoreError::MissingObject(hash)) if hash == missing
        ));

        let source = root_commit(&mut store, Vec::new());
        let period = Period::new(date("2026-01-01"), date("2026-01-31")).unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period,
                "tax",
                [],
                source,
                AnalysisArtifactId::new(missing),
            )),
            Err(StoreError::MissingObject(hash)) if hash == missing
        ));
    }

    #[test]
    fn close_rejects_zero_and_wrong_kind_analysis_artifacts() {
        let mut store = ObjectStore::new();
        let source = root_commit(&mut store, Vec::new());
        let period = Period::new(date("2026-01-01"), date("2026-01-31")).unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period.clone(),
                "tax",
                [],
                source,
                AnalysisArtifactId::new(ContentHash::ZERO),
            )),
            Err(StoreError::InvalidObject(_))
        ));
        let statement = store
            .put_statement(Statement::new("recognized", "root", "proof"))
            .expect("statement");
        assert!(matches!(
            store.put_close(Close::new(
                period,
                "tax",
                [],
                source,
                AnalysisArtifactId::new(statement.hash()),
            )),
            Err(StoreError::WrongKind {
                expected: ObjectKind::AnalysisArtifact,
                ..
            })
        ));
    }

    #[test]
    fn generic_proofs_and_signatures_cannot_bypass_analysis_artifacts() {
        let mut store = ObjectStore::new();
        let package = store
            .put_package(PolicyPackage::new("fifo", "1", b"rules".to_vec()))
            .unwrap();
        let proof = store
            .put_proof(ProofObject::new([], b"generic proof"))
            .unwrap();
        let generic_proof = store
            .put_proof(ProofObject::new([], b"not recognition"))
            .unwrap();
        let source = store
            .put_commit(Commit::new(
                [],
                [],
                [],
                [],
                [],
                [package],
                [proof, generic_proof],
                "author",
            ))
            .unwrap();
        let period = Period::new(date("2026-01-01"), date("2026-12-31")).unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period.clone(),
                "tax",
                [package],
                source,
                AnalysisArtifactId::new(generic_proof.hash()),
            )),
            Err(StoreError::WrongKind {
                expected: ObjectKind::AnalysisArtifact,
                actual: ObjectKind::Proof,
                ..
            })
        ));
        let unsigned = Close::new(
            period,
            "tax",
            [package],
            source,
            AnalysisArtifactId::new(proof.hash()),
        );
        let mut noncanonical = unsigned.clone();
        noncanonical.policies = vec![package, package];
        noncanonical.exceptions = vec!["z".into(), "a".into(), "z".into()];
        let canonical = unsigned.clone().with_exceptions(["a", "z"]);
        assert_eq!(noncanonical.signing_hash(), canonical.signing_hash());
        let payload = unsigned.signing_hash();
        let signed = unsigned.with_signatures([Signature::new(
            "alice",
            "test-only",
            payload.as_bytes().to_vec(),
        )]);
        assert!(matches!(
            store.put_close(signed.clone()),
            Err(StoreError::InvalidObject(_))
        ));
        assert!(matches!(
            store.put_close_verified(signed, &EchoVerifier),
            Err(StoreError::WrongKind {
                expected: ObjectKind::AnalysisArtifact,
                actual: ObjectKind::Proof,
                ..
            })
        ));

        let invalid = Close::new(
            Period::new(date("2026-01-01"), date("2026-12-31")).unwrap(),
            "tax",
            [package],
            source,
            AnalysisArtifactId::new(proof.hash()),
        )
        .with_signatures([Signature::new("alice", "test-only", b"forged".to_vec())]);
        assert!(matches!(
            store.put_close_verified(invalid, &EchoVerifier),
            Err(StoreError::InvalidObject(_))
        ));
    }

    #[test]
    fn solver_proof_is_checked_and_round_trips_as_canonical_content() {
        use crate::ir::{Atom, Nominal, NominalKind, Term};
        use crate::logic::{Goal, Literal, Program, SemanticContext, Solver};

        let present = Atom::new(
            Nominal::new(NominalKind::Predicate, "present"),
            vec![Term::Text("x".into())],
        );
        let mut program = Program::new();
        program
            .add_fact(Literal::positive(present.clone()))
            .expect("ground fact");
        let result = Solver::new().solve(
            &program,
            &Goal::atom(Literal::positive(present)),
            &SemanticContext::default(),
        );
        result
            .proof_graph()
            .check()
            .expect("canonical production checker accepts solver proof");

        let mut store = ObjectStore::new();
        let address = store
            .put_proof(ProofObject::from_proof(result.proof_graph().clone()))
            .expect("canonical proof persists");
        let loaded = store.proof(address).expect("proof reloads");
        loaded
            .canonical_proof()
            .check()
            .expect("reloaded proof remains independently checkable");
        assert_eq!(
            loaded.canonical_proof().content_hash(),
            result.proof_graph().content_hash()
        );
    }
}
