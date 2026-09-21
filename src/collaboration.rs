//! Small, explicit collaboration boundaries.
//!
//! This module deliberately does not own decisions, packages, or proof
//! objects.  Those remain the store and proof module's content-addressed
//! values. Collaboration values only carry references to them. Access labels
//! are policy primitives; callers must authorize before invoking the low-level
//! proof projection primitive.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use crate::evidence::{ImportBatch, ObservationAdapter};
use crate::model::ContentHash;
use crate::proof::{CheckError, Node, Proof, ProofId};
use crate::store::{DecisionId, ObjectStore, PackageId, SignatureVerifier, StoreError};

/// The domain used for signed collaboration references.
///
/// A typed object ID is included in the payload rather than merely its raw
/// hash.  Decision and package IDs are both 256-bit values, so binding the
/// object kind prevents a valid signature for one family from being replayed
/// as an attestation for the other family.
pub const ATTESTATION_DOMAIN: &str = "axiom/collaboration/attestation/v1";

/// A content-addressed object which can be attested by a collaborator.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AttestationSubject {
    Decision(DecisionId),
    Package(PackageId),
}

impl AttestationSubject {
    /// Construct an attestation subject for a decision object.
    pub const fn decision(id: DecisionId) -> Self {
        Self::Decision(id)
    }

    /// Construct an attestation subject for a policy package object.
    pub const fn package(id: PackageId) -> Self {
        Self::Package(id)
    }

    /// Return the existing store address without copying or re-hashing the
    /// addressed object.
    pub const fn hash(self) -> ContentHash {
        match self {
            Self::Decision(id) => id.hash(),
            Self::Package(id) => id.hash(),
        }
    }

    pub const fn kind(self) -> AttestationKind {
        match self {
            Self::Decision(_) => AttestationKind::Decision,
            Self::Package(_) => AttestationKind::Package,
        }
    }

    /// The domain-separated message handed to [`SignatureVerifier`].
    ///
    /// The object hash is still the source of identity; the extra kind byte
    /// only prevents cross-family signature replay.
    pub fn signing_payload(self) -> ContentHash {
        let mut bytes = Vec::with_capacity(1 + 32);
        bytes.push(self.kind().tag());
        bytes.extend_from_slice(self.hash().as_bytes());
        ContentHash::domain_separated(ATTESTATION_DOMAIN, &bytes)
    }
}

/// The two store object families accepted by [`AttestationSubject`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AttestationKind {
    Decision,
    Package,
}

impl AttestationKind {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Decision => 0,
            Self::Package => 1,
        }
    }
}

impl fmt::Display for AttestationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Decision => "decision",
            Self::Package => "package",
        })
    }
}

/// A signature over one already-persisted decision or package address.
///
/// This is intentionally not a second decision/package representation.  The
/// signer only attests to a typed content address; consumers resolve the
/// referenced object through the ordinary store API.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SignedAttestation {
    pub subject: AttestationSubject,
    pub signer: String,
    pub algorithm: String,
    pub signature: Vec<u8>,
}

impl SignedAttestation {
    pub fn new(
        subject: AttestationSubject,
        signer: impl Into<String>,
        algorithm: impl Into<String>,
        signature: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            subject,
            signer: signer.into(),
            algorithm: algorithm.into(),
            signature: signature.into(),
        }
    }

    pub fn decision(
        id: DecisionId,
        signer: impl Into<String>,
        algorithm: impl Into<String>,
        signature: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(
            AttestationSubject::Decision(id),
            signer,
            algorithm,
            signature,
        )
    }

    pub fn package(
        id: PackageId,
        signer: impl Into<String>,
        algorithm: impl Into<String>,
        signature: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(
            AttestationSubject::Package(id),
            signer,
            algorithm,
            signature,
        )
    }

    pub const fn kind(&self) -> AttestationKind {
        self.subject.kind()
    }

    pub const fn object_hash(&self) -> ContentHash {
        self.subject.hash()
    }

    pub fn signing_payload(&self) -> ContentHash {
        self.subject.signing_payload()
    }

    /// Verify the cryptographic envelope. This does not claim that the
    /// addressed object is present in a particular store; use
    /// [`Self::verify_persisted`] at that boundary.
    pub fn verify<V: SignatureVerifier>(&self, verifier: &V) -> Result<(), AttestationError> {
        if self.signer.trim().is_empty() {
            return Err(AttestationError::EmptySigner);
        }
        if self.algorithm.trim().is_empty() {
            return Err(AttestationError::EmptyAlgorithm);
        }
        if self.signature.is_empty() {
            return Err(AttestationError::EmptySignature);
        }
        if !verifier.verify(
            &self.signer,
            &self.algorithm,
            self.signing_payload(),
            &self.signature,
        ) {
            return Err(AttestationError::InvalidSignature {
                signer: self.signer.clone(),
                subject: self.subject,
            });
        }
        Ok(())
    }

    /// Alias useful at an authorization boundary where the operation is
    /// described as checking rather than verifying.
    pub fn check<V: SignatureVerifier>(&self, verifier: &V) -> Result<(), AttestationError> {
        self.verify(verifier)
    }

    pub fn verify_persisted<V: SignatureVerifier>(
        &self,
        verifier: &V,
        store: &ObjectStore,
    ) -> Result<(), AttestationError> {
        self.verify(verifier)?;
        match self.subject {
            AttestationSubject::Decision(id) => store.decision(id).map(|_| ()),
            AttestationSubject::Package(id) => store.package(id).map(|_| ()),
        }
        .map_err(AttestationError::MissingSubject)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttestationError {
    EmptySigner,
    EmptyAlgorithm,
    EmptySignature,
    InvalidSignature {
        signer: String,
        subject: AttestationSubject,
    },
    MissingSubject(StoreError),
}

impl fmt::Display for AttestationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySigner => formatter.write_str("attestation signer is empty"),
            Self::EmptyAlgorithm => formatter.write_str("attestation algorithm is empty"),
            Self::EmptySignature => formatter.write_str("attestation signature is empty"),
            Self::InvalidSignature { signer, subject } => write!(
                formatter,
                "signature from {signer} is invalid for {subject:?}"
            ),
            Self::MissingSubject(error) => {
                write!(formatter, "attested object is unavailable: {error}")
            }
        }
    }
}

impl std::error::Error for AttestationError {}

/// A stable principal name used for access checks.
///
/// Principal matching is exact and case-sensitive.  In particular, a label
/// never grants access merely because two names look similar or because a
/// caller has a signature from the same party.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Principal(String);

impl Principal {
    pub fn new(value: impl Into<String>) -> Result<Self, AccessError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(AccessError::EmptyPrincipal)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// An information-flow label.
///
/// `Public` is the only label that grants access without naming a principal.
/// `Restricted` is an explicit allow-list; an empty allow-list denies every
/// principal.  There is no ambient/default principal.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessLabel {
    Public,
    Restricted(BTreeSet<Principal>),
}

impl AccessLabel {
    pub const fn public() -> Self {
        Self::Public
    }

    pub fn restricted<I, P>(principals: I) -> Result<Self, AccessError>
    where
        I: IntoIterator<Item = P>,
        P: Into<String>,
    {
        principals
            .into_iter()
            .map(|principal| Principal::new(principal.into()))
            .collect::<Result<BTreeSet<_>, _>>()
            .map(Self::Restricted)
    }

    pub fn private(principal: impl Into<String>) -> Result<Self, AccessError> {
        Self::restricted([principal])
    }

    pub fn allows(&self, principal: &Principal) -> bool {
        match self {
            Self::Public => true,
            Self::Restricted(principals) => principals.contains(principal),
        }
    }

    pub fn authorize(&self, principal: &Principal) -> Result<(), AccessError> {
        if self.allows(principal) {
            Ok(())
        } else {
            Err(AccessError::Unauthorized {
                principal: principal.clone(),
            })
        }
    }

    pub const fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }

    pub fn principals(&self) -> Option<&BTreeSet<Principal>> {
        match self {
            Self::Public => None,
            Self::Restricted(principals) => Some(principals),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessError {
    EmptyPrincipal,
    Unauthorized { principal: Principal },
}

impl fmt::Display for AccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPrincipal => formatter.write_str("principal is empty"),
            Self::Unauthorized { principal } => {
                write!(formatter, "principal {principal} is not authorized")
            }
        }
    }
}

impl std::error::Error for AccessError {}

/// A proof projection containing complete visible nodes and commitments for
/// every hidden node.
///
/// Hidden nodes are not replaced by fabricated placeholders.  Their exact
/// [`ProofId`] values remain in `hidden`, while `frontier` records the
/// commitments at which a visible path reaches hidden material (plus hidden
/// roots). A recipient can therefore validate disclosed node hashes and
/// disclosed/hidden edge boundaries.
///
/// This is selective disclosure, not zero knowledge: exact hidden node IDs
/// are commitments to their payloads and may permit dictionary attacks when a
/// hidden statement has low entropy. Confidential sharing requires a future
/// salted/Merkle or keyed commitment format in addition to authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactedProof {
    /// Visible, complete proof nodes keyed by their content IDs.
    nodes: BTreeMap<ProofId, Node>,
    /// Proof IDs for every source node omitted from `nodes`.
    hidden: BTreeSet<ProofId>,
    /// The source proof's canonical roots.
    roots: Vec<ProofId>,
    /// Hidden IDs at visible/hidden boundaries and hidden roots.
    frontier: Vec<ProofId>,
    /// A compact commitment to the complete hidden-ID set.
    hidden_commitment: ContentHash,
    /// The content root of the complete, checked source proof.
    source_commitment: ProofId,
}

impl RedactedProof {
    /// Project a checked proof by retaining exactly the requested visible
    /// nodes.  The source proof is checked before projection so a projection
    /// cannot launder an invalid source graph. This low-level operation assumes
    /// its caller has already applied the owning [`AccessLabel`].
    pub fn from_proof<I>(proof: &Proof, visible: I) -> Result<Self, ProjectionError>
    where
        I: IntoIterator<Item = ProofId>,
    {
        proof.check().map_err(ProjectionError::SourceProofInvalid)?;
        let visible = visible.into_iter().collect::<BTreeSet<_>>();
        for id in &visible {
            if !proof.nodes.contains_key(id) {
                return Err(ProjectionError::UnknownVisibleNode { id: *id });
            }
        }
        let nodes = visible
            .iter()
            .filter_map(|id| proof.nodes.get(id).cloned().map(|node| (*id, node)))
            .collect::<BTreeMap<_, _>>();
        let hidden = proof
            .nodes
            .keys()
            .filter(|id| !visible.contains(id))
            .copied()
            .collect::<BTreeSet<_>>();
        let roots = proof.roots.clone();
        let frontier = expected_frontier(&nodes, &hidden, &roots)?;
        let hidden_commitment = hidden_commitment_for(&hidden);
        let source_commitment = proof.content_hash();
        let projection = Self {
            nodes,
            hidden,
            roots,
            frontier,
            hidden_commitment,
            source_commitment,
        };
        projection.check()?;
        Ok(projection)
    }

    /// A descriptive alias for [`Self::from_proof`].
    pub fn project<I>(proof: &Proof, visible: I) -> Result<Self, ProjectionError>
    where
        I: IntoIterator<Item = ProofId>,
    {
        Self::from_proof(proof, visible)
    }

    /// Validate the projection without access to hidden proof payloads.
    pub fn check(&self) -> Result<(), ProjectionError> {
        let mut roots = self.roots.clone();
        roots.sort();
        roots.dedup();
        if roots != self.roots {
            return Err(ProjectionError::NonCanonicalRoots);
        }
        let mut hidden = self.hidden.iter().copied().collect::<Vec<_>>();
        hidden.sort();
        hidden.dedup();
        if hidden.len() != self.hidden.len() {
            return Err(ProjectionError::NonCanonicalHidden);
        }
        if self.hidden_commitment != hidden_commitment_for(&self.hidden) {
            return Err(ProjectionError::HiddenCommitmentMismatch);
        }
        if self.nodes.keys().any(|id| self.hidden.contains(id)) {
            return Err(ProjectionError::OverlappingPartition);
        }
        if self.source_commitment
            != Proof::content_hash_from_ids(
                self.roots.iter().copied(),
                self.nodes
                    .keys()
                    .copied()
                    .chain(self.hidden.iter().copied()),
            )
        {
            return Err(ProjectionError::SourceCommitmentMismatch);
        }
        for (id, node) in &self.nodes {
            if id != &node.id || !node.is_well_formed() {
                return Err(ProjectionError::TamperedVisibleNode { id: *id });
            }
            for input in &node.inputs {
                if !self.nodes.contains_key(input) && !self.hidden.contains(input) {
                    return Err(ProjectionError::MissingCommitment {
                        node: *id,
                        input: *input,
                    });
                }
            }
        }
        for root in &self.roots {
            if !self.nodes.contains_key(root) && !self.hidden.contains(root) {
                return Err(ProjectionError::MissingRootCommitment { id: *root });
            }
        }
        let expected = expected_frontier(&self.nodes, &self.hidden, &self.roots)?;
        if self.frontier != expected {
            return Err(ProjectionError::InvalidFrontier {
                expected,
                actual: self.frontier.clone(),
            });
        }
        for id in &self.frontier {
            if !self.hidden.contains(id) {
                return Err(ProjectionError::FrontierNotHidden { id: *id });
            }
        }
        check_visible_reachability(&self.nodes, &self.hidden, &self.roots)?;
        check_visible_acyclic(&self.nodes)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ProjectionError> {
        self.check()
    }

    pub fn verify(&self) -> Result<(), ProjectionError> {
        self.check()
    }

    pub fn visible_node(&self, id: ProofId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn roots(&self) -> &[ProofId] {
        &self.roots
    }

    pub fn frontier(&self) -> &[ProofId] {
        &self.frontier
    }

    pub fn hidden_commitment(&self) -> ContentHash {
        self.hidden_commitment
    }

    /// Return the complete checked proof's content root.
    pub const fn source_commitment(&self) -> ProofId {
        self.source_commitment
    }

    /// Verify this projection against an authenticated complete-proof root.
    ///
    /// The projection's own hash checks prove internal consistency. The
    /// expected root must come from the caller's trusted store, signature, or
    /// ledger commit; accepting a root supplied by the projection would not
    /// establish provenance.
    pub fn verify_against_source(&self, expected: ProofId) -> Result<(), ProjectionError> {
        self.check()?;
        if self.source_commitment != expected {
            return Err(ProjectionError::SourceCommitmentMismatch);
        }
        Ok(())
    }
}

fn hidden_commitment_for(hidden: &BTreeSet<ProofId>) -> ContentHash {
    let mut bytes = Vec::with_capacity(8 + hidden.len() * 32);
    bytes.extend_from_slice(b"hidden-proof-ids/v1");
    bytes.extend_from_slice(&(hidden.len() as u64).to_be_bytes());
    for id in hidden {
        bytes.extend_from_slice(&id.0);
    }
    ContentHash::domain_separated("axiom/collaboration/redacted-proof/v1", &bytes)
}

fn expected_frontier(
    nodes: &BTreeMap<ProofId, Node>,
    hidden: &BTreeSet<ProofId>,
    roots: &[ProofId],
) -> Result<Vec<ProofId>, ProjectionError> {
    let mut frontier = BTreeSet::new();
    for root in roots {
        if hidden.contains(root) {
            frontier.insert(*root);
        } else if !nodes.contains_key(root) {
            return Err(ProjectionError::MissingRootCommitment { id: *root });
        }
    }
    for (id, node) in nodes {
        if id != &node.id || !node.is_well_formed() {
            return Err(ProjectionError::TamperedVisibleNode { id: *id });
        }
        for input in &node.inputs {
            if hidden.contains(input) {
                frontier.insert(*input);
            } else if !nodes.contains_key(input) {
                return Err(ProjectionError::MissingCommitment {
                    node: *id,
                    input: *input,
                });
            }
        }
    }
    Ok(frontier.into_iter().collect())
}

fn check_visible_reachability(
    nodes: &BTreeMap<ProofId, Node>,
    hidden: &BTreeSet<ProofId>,
    roots: &[ProofId],
) -> Result<(), ProjectionError> {
    let mut visited = BTreeSet::new();
    let mut stack = roots
        .iter()
        .filter(|root| nodes.contains_key(root))
        .copied()
        .collect::<Vec<_>>();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(&id) else {
            if hidden.contains(&id) {
                continue;
            }
            return Err(ProjectionError::MissingRootCommitment { id });
        };
        for input in &node.inputs {
            if nodes.contains_key(input) {
                stack.push(*input);
            } else if !hidden.contains(input) {
                return Err(ProjectionError::MissingCommitment {
                    node: id,
                    input: *input,
                });
            }
        }
    }
    if visited.len() != nodes.len() {
        let id = nodes
            .keys()
            .find(|id| !visited.contains(id))
            .copied()
            .expect("visited length differs from node length");
        return Err(ProjectionError::UnreachableVisibleNode { id });
    }
    Ok(())
}

fn check_visible_acyclic(nodes: &BTreeMap<ProofId, Node>) -> Result<(), ProjectionError> {
    fn visit(
        id: ProofId,
        nodes: &BTreeMap<ProofId, Node>,
        active: &mut BTreeSet<ProofId>,
        visited: &mut BTreeSet<ProofId>,
    ) -> Result<(), ProjectionError> {
        if active.contains(&id) {
            return Err(ProjectionError::VisibleCycle { id });
        }
        if !visited.insert(id) {
            return Ok(());
        }
        active.insert(id);
        if let Some(node) = nodes.get(&id) {
            for input in &node.inputs {
                if nodes.contains_key(input) {
                    visit(*input, nodes, active, visited)?;
                }
            }
        }
        active.remove(&id);
        Ok(())
    }

    let mut active = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in nodes.keys().copied() {
        visit(id, nodes, &mut active, &mut visited)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    SourceProofInvalid(CheckError),
    UnknownVisibleNode {
        id: ProofId,
    },
    NonCanonicalRoots,
    NonCanonicalHidden,
    TamperedVisibleNode {
        id: ProofId,
    },
    MissingCommitment {
        node: ProofId,
        input: ProofId,
    },
    MissingRootCommitment {
        id: ProofId,
    },
    InvalidFrontier {
        expected: Vec<ProofId>,
        actual: Vec<ProofId>,
    },
    FrontierNotHidden {
        id: ProofId,
    },
    HiddenCommitmentMismatch,
    SourceCommitmentMismatch,
    OverlappingPartition,
    UnreachableVisibleNode {
        id: ProofId,
    },
    VisibleCycle {
        id: ProofId,
    },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceProofInvalid(error) => {
                write!(formatter, "source proof is invalid: {error}")
            }
            Self::UnknownVisibleNode { id } => write!(formatter, "unknown visible proof node {id}"),
            Self::NonCanonicalRoots => {
                formatter.write_str("redacted proof roots are not canonical")
            }
            Self::NonCanonicalHidden => {
                formatter.write_str("redacted proof hidden IDs are not canonical")
            }
            Self::TamperedVisibleNode { id } => {
                write!(formatter, "visible proof node {id} is tampered")
            }
            Self::MissingCommitment { node, input } => {
                write!(
                    formatter,
                    "visible node {node} lacks input commitment {input}"
                )
            }
            Self::MissingRootCommitment { id } => {
                write!(formatter, "root commitment {id} is missing")
            }
            Self::InvalidFrontier { .. } => {
                formatter.write_str("redacted proof frontier is invalid")
            }
            Self::FrontierNotHidden { id } => write!(formatter, "frontier ID {id} is not hidden"),
            Self::HiddenCommitmentMismatch => {
                formatter.write_str("hidden proof commitment mismatches hidden IDs")
            }
            Self::SourceCommitmentMismatch => {
                formatter.write_str("redacted proof does not match the authenticated source proof")
            }
            Self::OverlappingPartition => {
                formatter.write_str("visible and hidden proof partitions overlap")
            }
            Self::UnreachableVisibleNode { id } => {
                write!(formatter, "visible node {id} is unreachable from roots")
            }
            Self::VisibleCycle { id } => {
                write!(formatter, "visible proof graph contains a cycle at {id}")
            }
        }
    }
}

impl std::error::Error for ProjectionError {}

/// Re-export the already narrow evidence adapter contract at the
/// collaboration boundary.  Its result is [`ImportBatch`], which can carry
/// raw observations only and has no representation for decisions or policy
/// packages.  The adapter cannot access an `EvidenceStore` through this API.
pub use crate::evidence::ObservationAdapter as ObservationOnlyAdapter;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof::{Operation, Statement};

    struct EchoVerifier;

    impl SignatureVerifier for EchoVerifier {
        fn verify(
            &self,
            signer: &str,
            algorithm: &str,
            payload: ContentHash,
            signature: &[u8],
        ) -> bool {
            signer == "alice" && algorithm == "echo" && signature == payload.as_bytes()
        }
    }

    fn decision(seed: u8) -> DecisionId {
        DecisionId::new(ContentHash::from_bytes([seed; 32]))
    }

    fn tiny_proof() -> (Proof, ProofId, ProofId) {
        let mut proof = Proof::new();
        let leaf = proof.insert(Node::new(
            "private fact",
            Operation::Observation {
                source: "hidden source".into(),
            },
            Vec::new(),
            BTreeMap::new(),
        ));
        let root = proof.insert(Node::new(
            "public result",
            Operation::Derive {
                rule: "public-rule".into(),
            },
            vec![leaf],
            BTreeMap::new(),
        ));
        proof.root(root);
        (proof, leaf, root)
    }

    #[test]
    fn attestation_binds_subject_kind_and_id() {
        let id = decision(1);
        let subject = AttestationSubject::decision(id);
        let attestation = SignedAttestation::new(
            subject,
            "alice",
            "echo",
            subject.signing_payload().as_bytes().to_vec(),
        );
        assert!(attestation.verify(&EchoVerifier).is_ok());

        let wrong_subject = SignedAttestation::new(
            AttestationSubject::decision(decision(2)),
            "alice",
            "echo",
            attestation.signature.clone(),
        );
        assert!(matches!(
            wrong_subject.verify(&EchoVerifier),
            Err(AttestationError::InvalidSignature { .. })
        ));

        let wrong_kind = SignedAttestation::new(
            AttestationSubject::package(PackageId::new(id.hash())),
            "alice",
            "echo",
            attestation.signature,
        );
        assert!(wrong_kind.verify(&EchoVerifier).is_err());
    }

    #[test]
    fn restricted_label_requires_exact_principal() {
        let label = AccessLabel::private("alice").unwrap();
        assert!(label.authorize(&Principal::new("alice").unwrap()).is_ok());
        assert!(matches!(
            label.authorize(&Principal::new("bob").unwrap()),
            Err(AccessError::Unauthorized { .. })
        ));
        assert!(
            !AccessLabel::restricted(std::iter::empty::<&str>())
                .unwrap()
                .allows(&Principal::new("alice").unwrap())
        );
    }

    #[test]
    fn redacted_projection_keeps_hidden_commitments_and_frontier() {
        let (proof, leaf, root) = tiny_proof();
        let projection = RedactedProof::from_proof(&proof, [root]).unwrap();
        assert_eq!(
            projection.hidden.iter().copied().collect::<Vec<_>>(),
            vec![leaf]
        );
        assert_eq!(projection.frontier, vec![leaf]);
        assert_eq!(projection.source_commitment(), proof.content_hash());
        assert!(
            projection
                .verify_against_source(proof.content_hash())
                .is_ok()
        );
        assert!(matches!(
            projection.verify_against_source(ProofId([99; 32])),
            Err(ProjectionError::SourceCommitmentMismatch)
        ));
        assert!(projection.check().is_ok());
    }

    #[test]
    fn redacted_projection_rejects_tampered_visible_node_and_frontier() {
        let (proof, _leaf, root) = tiny_proof();
        let mut projection = RedactedProof::from_proof(&proof, [root]).unwrap();
        projection
            .nodes
            .get_mut(&root)
            .expect("root is visible")
            .statement = Statement::new("forged result");
        assert!(matches!(
            projection.check(),
            Err(ProjectionError::TamperedVisibleNode { id }) if id == root
        ));

        let (proof, _leaf, root) = tiny_proof();
        let mut projection = RedactedProof::from_proof(&proof, [root]).unwrap();
        projection.frontier.clear();
        assert!(matches!(
            projection.check(),
            Err(ProjectionError::InvalidFrontier { .. })
        ));
        projection.frontier = vec![ProofId([99; 32])];

        let (proof, leaf, root) = tiny_proof();
        let mut projection = RedactedProof::from_proof(&proof, [root]).unwrap();
        projection.hidden.remove(&leaf);
        assert!(matches!(
            projection.check(),
            Err(ProjectionError::HiddenCommitmentMismatch)
                | Err(ProjectionError::SourceCommitmentMismatch)
        ));

        let (proof, leaf, root) = tiny_proof();
        let mut projection = RedactedProof::from_proof(&proof, [root]).unwrap();
        projection.hidden.insert(root);
        projection.hidden_commitment = hidden_commitment_for(&projection.hidden);
        assert!(matches!(
            projection.check(),
            Err(ProjectionError::OverlappingPartition)
        ));
        assert!(projection.hidden.contains(&leaf));
    }
}
