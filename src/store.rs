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
use crate::proof::{
    Node as CanonicalNode, Operation as CanonicalOperation, Proof as CanonicalProof,
};

const SCHEMA_VERSION: &str = "axiom/store/v1";
const EVIDENCE_CONTENT_DOMAIN: &str = "axiom/store/evidence-content/v1";

/// The object families that may be addressed by this store.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObjectKind {
    Evidence,
    Statement,
    Decision,
    Completeness,
    Package,
    Proof,
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
            Self::Proof => "proof",
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
kind_marker!(ProofKind, Proof);
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
/// The store address of a persisted proof object.  This is deliberately
/// distinct from [`crate::proof::ProofId`], which is the identity of a proof
/// node inside the canonical DAG.  A stored object may contain many nodes and
/// is addressed by the store's typed object hash, while all semantic edges
/// continue to use the canonical proof ID.
pub type ProofObjectId = ObjectId<ProofKind>;
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
    fn target(&self) -> Option<EvidenceId> {
        match self {
            Self::Correction { supersedes, .. } | Self::Tombstone { supersedes, .. } => {
                Some(*supersedes)
            }
            Self::Present | Self::Unavailable { .. } | Self::Redacted { .. } => None,
        }
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

/// A versioned policy/rule package.  `manifest` is intentionally just
/// canonical metadata in this layer; package semantics belong elsewhere.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PolicyPackage {
    pub name: String,
    pub version: String,
    pub manifest: Vec<(String, String)>,
    pub body: Vec<u8>,
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

    pub fn superseding(mut self, prior: PackageId) -> Self {
        self.supersedes = Some(prior);
        self
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
        Self::envelope(
            roots,
            body,
            "generic",
            CanonicalOperation::Observation {
                source: "stored-proof".into(),
            },
        )
    }

    /// Construct the typed proof envelope required by a close's recognized
    /// root. Generic proof objects remain valid store values, but cannot be
    /// mistaken for recognition results by [`ObjectStore::put_close`].
    pub fn recognized(
        roots: impl IntoIterator<Item = ContentHash>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self::envelope(
            roots,
            body,
            "recognized",
            CanonicalOperation::Derive {
                rule: "book-recognition".into(),
            },
        )
    }

    fn envelope(
        roots: impl IntoIterator<Item = ContentHash>,
        body: impl Into<Vec<u8>>,
        purpose: &str,
        operation: CanonicalOperation,
    ) -> Self {
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
            operation,
            Vec::new(),
            BTreeMap::from([
                ("proof-purpose".to_string(), purpose.to_string()),
                (
                    "payload-hash".to_string(),
                    blake3::hash(&body).to_hex().to_string(),
                ),
            ]),
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

    pub fn canonicalize(&mut self) {
        canonicalize_vec(&mut self.parents);
        canonicalize_vec(&mut self.evidence);
        canonicalize_vec(&mut self.statements);
        canonicalize_vec(&mut self.decisions);
        canonicalize_vec(&mut self.completeness);
        canonicalize_vec(&mut self.packages);
        canonicalize_vec(&mut self.proofs);
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
/// recognized result root, exceptions, and signatures.  A reopening is a new
/// close with `supersedes` set; no historical object is mutated.
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Close {
    pub period: Period,
    pub book: BookId,
    pub policies: Vec<PackageId>,
    pub source: CommitId,
    pub recognized_root: ContentHash,
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
        recognized_root: ContentHash,
    ) -> Self {
        Self {
            period,
            book: book.into(),
            policies: canonical_set(policies),
            source,
            recognized_root,
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
    Proof(ProofObject),
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
            Self::Proof(_) => ObjectKind::Proof,
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
            Self::Proof(value) => encode_proof(&mut out, value),
            Self::Commit(value) => encode_commit(&mut out, value),
            Self::Close(value) => encode_close(&mut out, value),
        }
        out
    }

    pub fn content_hash(&self) -> ContentHash {
        ContentHash::domain_separated("axiom/store/object/v1", &self.canonical_bytes())
    }
}

/// Conflicts are semantic, not storage failures.  The merged commit is still
/// stored, while these diagnostics keep a later close from pretending that
/// two divergent choices were reconciled.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MergeConflict {
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
}

impl MergeResult {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
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
            }
        }
        Ok(())
    }

    pub fn contains(&self, hash: ContentHash) -> bool {
        self.objects.contains_key(&hash) && self.get(hash).is_ok()
    }

    pub fn put_evidence(&mut self, value: Evidence) -> Result<EvidenceId, StoreError> {
        if let Some(target) = value.state.target() {
            self.require_kind(target.hash(), ObjectKind::Evidence)?;
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
        }
        Ok(DecisionId::new(self.insert(StoredObject::Decision(value))?))
    }

    pub fn put_completeness(&mut self, value: Completeness) -> Result<CompletenessId, StoreError> {
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Completeness)?;
        }
        Ok(CompletenessId::new(
            self.insert(StoredObject::Completeness(value))?,
        ))
    }

    pub fn put_package(&mut self, mut value: PolicyPackage) -> Result<PackageId, StoreError> {
        value.manifest.sort();
        value.manifest.dedup();
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Package)?;
        }
        Ok(PackageId::new(self.insert(StoredObject::Package(value))?))
    }

    pub fn put_proof(&mut self, value: ProofObject) -> Result<ProofObjectId, StoreError> {
        value
            .proof
            .check()
            .map_err(|error| StoreError::InvalidObject(format!("invalid proof: {error}")))?;
        for root in &value.roots {
            if *root != ContentHash::ZERO {
                self.get(*root)?;
            }
        }
        Ok(ProofObjectId::new(self.insert(StoredObject::Proof(value))?))
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
        for package in &value.policies {
            self.require_kind(package.hash(), ObjectKind::Package)?;
        }
        if value.recognized_root == ContentHash::ZERO {
            return Err(StoreError::InvalidObject(
                "close must pin a recognized proof root".into(),
            ));
        }
        match self.get(value.recognized_root)? {
            StoredObject::Proof(proof) => {
                proof.proof.check().map_err(|error| {
                    StoreError::InvalidObject(format!("invalid recognized proof: {error}"))
                })?;
                let typed_root = proof.proof.roots.iter().any(|root| {
                    proof.proof.nodes.get(root).is_some_and(|node| {
                        matches!(
                            node.operation,
                            CanonicalOperation::Derive { ref rule }
                                if rule == "book-recognition"
                        ) && node
                            .metadata
                            .get("proof-purpose")
                            .is_some_and(|purpose| purpose == "recognized")
                    })
                });
                if !typed_root {
                    return Err(StoreError::InvalidObject(
                        "close recognized root is not a typed recognition proof".into(),
                    ));
                }
            }
            object => {
                return Err(StoreError::WrongKind {
                    hash: value.recognized_root,
                    expected: ObjectKind::Proof,
                    actual: object.kind(),
                });
            }
        }
        if !source_commit
            .proofs
            .iter()
            .any(|proof| proof.hash() == value.recognized_root)
        {
            return Err(StoreError::InvalidObject(
                "recognized proof root is not pinned by the source commit".into(),
            ));
        }
        if let Some(prior) = value.supersedes {
            self.require_kind(prior.hash(), ObjectKind::Close)?;
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
        self.require_ancestor(base, left)?;
        self.require_ancestor(base, right)?;
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

        let mut evidence = base_value.evidence.clone();
        evidence.extend(left_value.evidence.iter().copied());
        evidence.extend(right_value.evidence.iter().copied());
        canonicalize_vec(&mut evidence);

        let mut statements = base_value.statements.clone();
        statements.extend(left_value.statements.iter().copied());
        statements.extend(right_value.statements.iter().copied());
        canonicalize_vec(&mut statements);

        let mut proofs = base_value.proofs.clone();
        proofs.extend(left_value.proofs.iter().copied());
        proofs.extend(right_value.proofs.iter().copied());
        canonicalize_vec(&mut proofs);

        let mut conflicts = decision_conflicts;
        conflicts.extend(package_conflicts);
        conflicts.extend(completeness_conflicts);
        conflicts.sort();
        conflicts.dedup();

        let merged = Commit::new(
            [left, right],
            evidence,
            statements,
            decisions,
            completeness,
            packages,
            proofs,
            author,
        );
        let commit = self.put_commit(merged)?;
        Ok(MergeResult { commit, conflicts })
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
            if l == r {
                selected.extend(l);
            } else if l == b {
                selected.extend(r);
            } else if r == b {
                selected.extend(l);
            } else {
                selected.extend(l.iter().copied());
                selected.extend(r.iter().copied());
                conflicts.push(MergeConflict::Decisions {
                    subject,
                    scope,
                    left: l,
                    right: r,
                });
            }
        }
        canonicalize_vec(&mut selected);
        Ok((selected, conflicts))
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
            if l == r {
                selected.extend(l);
            } else if l == b {
                selected.extend(r);
            } else if r == b {
                selected.extend(l);
            } else {
                selected.extend(l.iter().copied());
                selected.extend(r.iter().copied());
                conflicts.push(MergeConflict::Policies {
                    name,
                    left: l,
                    right: r,
                });
            }
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
            if l == r {
                selected.extend(l);
            } else if l == b {
                selected.extend(r);
            } else if r == b {
                selected.extend(l);
            } else {
                selected.extend(l.iter().copied());
                selected.extend(r.iter().copied());
                if self.completeness_conflicts(&l, &r)? {
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
            }
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
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
}

fn encode_proof(out: &mut Vec<u8>, value: &ProofObject) {
    encode_canonical_proof(out, &value.proof);
    put_u64(out, value.roots.len() as u64);
    for root in &value.roots {
        put_hash(out, *root);
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
    put_hash(out, value.recognized_root);
    put_strings(out, &value.exceptions);
    put_signatures(out, &value.signatures);
    put_optional_hash(out, value.supersedes.map(ObjectId::hash));
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
            store.put_close(Close::new(period, "tax", [], source, missing)),
            Err(StoreError::MissingObject(hash)) if hash == missing
        ));
    }

    #[test]
    fn close_rejects_zero_and_wrong_kind_recognized_roots() {
        let mut store = ObjectStore::new();
        let source = root_commit(&mut store, Vec::new());
        let period = Period::new(date("2026-01-01"), date("2026-01-31")).unwrap();
        assert!(matches!(
            store.put_close(Close::new(
                period.clone(),
                "tax",
                [],
                source,
                ContentHash::ZERO,
            )),
            Err(StoreError::InvalidObject(_))
        ));
        let statement = store
            .put_statement(Statement::new("recognized", "root", "proof"))
            .expect("statement");
        assert!(matches!(
            store.put_close(Close::new(period, "tax", [], source, statement.hash())),
            Err(StoreError::WrongKind {
                expected: ObjectKind::Proof,
                ..
            })
        ));
    }

    #[test]
    fn close_pins_source_policy_root_and_supersession() {
        let mut store = ObjectStore::new();
        let package = store
            .put_package(PolicyPackage::new("fifo", "1", b"rules".to_vec()))
            .unwrap();
        let proof = store
            .put_proof(ProofObject::recognized([], b"recognized"))
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
                generic_proof.hash(),
            )),
            Err(StoreError::InvalidObject(_))
        ));
        let first = store
            .put_close(Close::new(
                period.clone(),
                "tax",
                [package],
                source,
                proof.hash(),
            ))
            .unwrap();
        let second = store
            .put_close(
                Close::new(period, "tax", [package], source, proof.hash()).superseding(first),
            )
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(store.close(second).unwrap().supersedes, Some(first));

        let unsigned = Close::new(
            Period::new(date("2027-01-01"), date("2027-12-31")).unwrap(),
            "tax",
            [package],
            source,
            proof.hash(),
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
        let signed_id = store
            .put_close_verified(signed, &EchoVerifier)
            .expect("canonical payload signature verifies");
        assert_eq!(store.close(signed_id).unwrap().signatures.len(), 1);

        let invalid = Close::new(
            Period::new(date("2028-01-01"), date("2028-12-31")).unwrap(),
            "tax",
            [package],
            source,
            proof.hash(),
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
