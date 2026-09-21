//! The canonical source-ledger workspace boundary.
//!
//! A [`Workspace`] turns exact UTF-8 source bytes into one immutable,
//! content-addressed [`SourceLedger`].  The bytes are first retained as
//! [`RawEvidence`], then represented in the existing [`crate::store::ObjectStore`]
//! as a narrow canonical [`crate::store::Evidence`] conversion.  The
//! conversion preserves occurrence identity, the exact payload, the raw
//! content address, external identity, and the source portion of provenance.
//! `ObjectStore::Evidence` has no fields for adapter provenance, spans,
//! authority, availability notes, or arbitrary evidence relations; those
//! richer fields therefore remain on the returned `SourceLedger`.  This is a
//! deliberate, documented consolidation gap rather than a second evidence
//! store.
//!
//! Exact source bytes are the source identity.  Consequently a comment or
//! whitespace edit creates a different source evidence/commit even when the
//! strict model and analysis are unchanged.  The surface semantic node IDs
//! still ignore trivia, so editor-level semantic identity retains its
//! documented trivia-insensitive behavior.  A repeated byte-identical import
//! is idempotent.  A changed import through [`Workspace::load_source`] is a
//! new correction commit whose parent is the workspace's current source head;
//! no historical object is overwritten.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::ops::Deref;
use std::str;

use crate::engine::{self, Analysis};
use crate::evidence::{Authority, Provenance, RawEvidence};
use crate::model::{ContentHash, Identity, Ledger, SourceId};
use crate::package::PolicyRegistry;
use crate::parser::{self, ParseError};
use crate::proof::{Node, Operation, Proof};
use crate::store::{
    Commit, CommitId, Evidence, EvidenceId, EvidenceState, ObjectStore, PackageId, ProofObject,
    ProofObjectId, StoreError,
};
use crate::surface::SurfaceFile;

const SOURCE_OCCURRENCE_PREFIX: &str = "source/";
const SOURCE_AUTHOR: &str = "workspace/source";
const ANALYSIS_AUTHOR: &str = "workspace/analysis";

/// An immutable source file together with its lossless surface and persisted
/// source commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLedger {
    /// The commit containing this source evidence.
    pub commit: CommitId,
    /// The original observation.  Its payload is exactly the bytes supplied
    /// to [`Workspace::load_source`] or [`Workspace::correct_source`].
    pub evidence: RawEvidence,
    /// The tolerant, lossless authoring surface parsed from the UTF-8 bytes.
    pub surface: SurfaceFile,
}

impl SourceLedger {
    pub fn commit_id(&self) -> CommitId {
        self.commit
    }

    pub fn source(&self) -> &SourceId {
        self.evidence.source()
    }

    pub fn occurrence(&self) -> &crate::model::OccurrenceId {
        self.evidence.occurrence()
    }

    pub fn content(&self) -> ContentHash {
        self.evidence.content()
    }

    pub fn bytes(&self) -> &[u8] {
        self.evidence.payload().unwrap_or_default()
    }

    pub fn lossless_source(&self) -> &str {
        self.surface.lossless()
    }

    pub fn has_surface_errors(&self) -> bool {
        self.surface.errors().next().is_some()
    }
}

/// A strict model ledger with the source commit it was elaborated from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundLedger {
    pub source_commit: CommitId,
    pub ledger: Ledger,
}

impl BoundLedger {
    pub fn commit_id(&self) -> CommitId {
        self.source_commit
    }

    pub fn source_commit(&self) -> CommitId {
        self.source_commit
    }
}

impl Deref for BoundLedger {
    type Target = Ledger;

    fn deref(&self) -> &Self::Target {
        &self.ledger
    }
}

/// Engine output bound to one immutable source commit.
///
/// `analysis.proof` is the engine proof plus a deterministic binding root
/// whose operation names the source commit and whose inputs are the original
/// proof roots.  The proof is also persisted in `ObjectStore`, and
/// `analysis_commit` pins it without changing the source commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitAnalysis {
    pub source_commit: CommitId,
    pub analysis_commit: CommitId,
    pub ledger: BoundLedger,
    pub analysis: Analysis,
    /// The executable registry resolved from the source commit's package
    /// roots.  Keeping this alongside the bound result lets callers render
    /// the same package set that was actually evaluated.
    pub policy_registry: PolicyRegistry,
    pub proof_id: ProofObjectId,
    pub metadata: BTreeMap<String, String>,
}

impl CommitAnalysis {
    pub fn source_commit(&self) -> CommitId {
        self.source_commit
    }

    pub fn commit_id(&self) -> CommitId {
        self.source_commit
    }

    pub fn analysis_commit(&self) -> CommitId {
        self.analysis_commit
    }

    pub fn proof_id(&self) -> ProofObjectId {
        self.proof_id
    }

    pub fn proof(&self) -> &Proof {
        &self.analysis.proof
    }

    pub fn check_proof(&self) -> Result<(), crate::proof::CheckError> {
        self.analysis.check_proof()
    }
}

impl Deref for CommitAnalysis {
    type Target = Analysis;

    fn deref(&self) -> &Self::Target {
        &self.analysis
    }
}

/// A failure at the source/commit/elaboration/evaluation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    Store(StoreError),
    Parse(ParseError),
    InvalidUtf8,
    EmptySource,
    NotSourceCommit {
        commit: CommitId,
        reason: String,
    },
    MissingPayload {
        commit: CommitId,
    },
    SourceMismatch {
        expected: SourceId,
        actual: SourceId,
    },
    PackageConflict {
        commit: CommitId,
        name: String,
    },
    HistoryCycle {
        commit: CommitId,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "workspace store error: {error}"),
            Self::Parse(error) => write!(formatter, "source parse error: {error}"),
            Self::InvalidUtf8 => formatter.write_str("source bytes are not valid UTF-8"),
            Self::EmptySource => formatter.write_str("source identifier cannot be empty"),
            Self::NotSourceCommit { commit, reason } => {
                write!(
                    formatter,
                    "commit {commit} is not a source commit: {reason}"
                )
            }
            Self::MissingPayload { commit } => {
                write!(formatter, "source commit {commit} has no available payload")
            }
            Self::SourceMismatch { expected, actual } => {
                write!(
                    formatter,
                    "source mismatch: expected {expected}, found {actual}"
                )
            }
            Self::PackageConflict { commit, name } => write!(
                formatter,
                "source commit {commit} pins multiple policy packages named `{name}`"
            ),
            Self::HistoryCycle { commit } => write!(formatter, "commit history cycle at {commit}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<StoreError> for WorkspaceError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ParseError> for WorkspaceError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}

/// The canonical immutable-object boundary for source ledgers, corrections,
/// elaboration, and proof-producing evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Workspace {
    store: ObjectStore,
    /// A branch reference is not another evidence store.  It only remembers
    /// which source commit `load_source` should treat as the current head.
    heads: BTreeMap<SourceId, CommitId>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_store(store: ObjectStore) -> Self {
        Self {
            store,
            heads: BTreeMap::new(),
        }
    }

    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut ObjectStore {
        &mut self.store
    }

    /// Return the current branch head remembered for a source, if this
    /// workspace has loaded that source on this branch.
    pub fn head(&self, source: impl AsRef<str>) -> Option<CommitId> {
        self.heads
            .iter()
            .find(|(known, _)| known.as_str() == source.as_ref())
            .map(|(_, commit)| *commit)
    }

    /// Load exact source bytes into an immutable source evidence/commit.
    ///
    /// The first load creates a root source commit.  Later loads for the same
    /// source are idempotent when bytes are equal and become correction
    /// commits when bytes differ.
    pub fn load_source(
        &mut self,
        source: impl Into<String>,
        bytes: impl AsRef<[u8]>,
    ) -> Result<SourceLedger, WorkspaceError> {
        let source = SourceId::try_new(source.into()).map_err(|_| WorkspaceError::EmptySource)?;
        let bytes = bytes.as_ref().to_vec();
        ensure_utf8(&bytes)?;

        if let Some(head) = self.heads.get(&source).copied() {
            let current = self.source_ledger(head)?;
            if current.bytes() == bytes.as_slice() {
                return Ok(current);
            }
            return self.correct_source_inner(head, source, bytes, "source bytes changed");
        }

        let raw = raw_source_evidence(source.clone(), bytes);
        let evidence = canonical_evidence(&raw);
        let evidence_id = self.store.put_evidence(evidence)?;
        let commit = self.store.put_commit(Commit::new(
            [],
            [evidence_id],
            [],
            [],
            [],
            [],
            [],
            SOURCE_AUTHOR,
        ))?;
        self.heads.insert(source, commit);
        self.materialize_source_commit(commit)
    }

    /// Create a correction commit explicitly.  The prior source commit and
    /// its evidence remain untouched and are reachable through history.
    pub fn correct_source(
        &mut self,
        prior: CommitId,
        bytes: impl AsRef<[u8]>,
    ) -> Result<SourceLedger, WorkspaceError> {
        let prior_source = self.source_ledger(prior)?;
        let source = prior_source.evidence.source().clone();
        let bytes = bytes.as_ref().to_vec();
        ensure_utf8(&bytes)?;
        if prior_source.bytes() == bytes.as_slice() {
            return Ok(prior_source);
        }
        self.correct_source_inner(prior, source, bytes, "source correction")
    }

    /// Correction variant retaining a caller-supplied audit reason in the
    /// persisted store evidence state.
    pub fn correct_source_with_reason(
        &mut self,
        prior: CommitId,
        bytes: impl AsRef<[u8]>,
        reason: impl Into<String>,
    ) -> Result<SourceLedger, WorkspaceError> {
        let prior_source = self.source_ledger(prior)?;
        let source = prior_source.evidence.source().clone();
        let bytes = bytes.as_ref().to_vec();
        ensure_utf8(&bytes)?;
        if prior_source.bytes() == bytes.as_slice() {
            return Ok(prior_source);
        }
        self.correct_source_inner(prior, source, bytes, reason.into())
    }

    /// Create a new source commit that carries an explicit policy-package
    /// context.  The source evidence remains the same immutable object; only
    /// the commit context changes.  This is the supported bridge from the
    /// store's package roots into [`Workspace::analyze_commit`].
    pub fn commit_with_packages(
        &mut self,
        source: CommitId,
        packages: impl IntoIterator<Item = PackageId>,
    ) -> Result<SourceLedger, WorkspaceError> {
        let source_ledger = self.source_ledger(source)?;
        let source_value = self.store.commit(source)?.clone();
        let commit = self.store.put_commit(Commit::new(
            [source],
            source_value.evidence,
            [],
            [],
            [],
            packages,
            [],
            SOURCE_AUTHOR,
        ))?;
        self.heads
            .insert(source_ledger.evidence.source().clone(), commit);
        self.materialize_source_commit(commit)
    }

    /// Strictly elaborate the source evidence pinned by `commit`.
    pub fn elaborate_commit(&self, commit: CommitId) -> Result<BoundLedger, WorkspaceError> {
        let source = self.source_ledger(commit)?;
        let ledger = parser::parse_surface_ledger(&source.surface)?;
        Ok(BoundLedger {
            source_commit: commit,
            ledger,
        })
    }

    /// Compatibility spelling for callers that use “elaborate” as the phase
    /// name rather than “elaborate_commit”.
    pub fn elaborate(&self, commit: CommitId) -> Result<BoundLedger, WorkspaceError> {
        self.elaborate_commit(commit)
    }

    /// Analyze one source commit, persist a commit-binding proof, and return
    /// engine output whose proof and metadata are bound to `commit`.
    pub fn analyze_commit(&mut self, commit: CommitId) -> Result<CommitAnalysis, WorkspaceError> {
        let ledger = self.elaborate_commit(commit)?;
        let source = self.source_ledger(commit)?;
        let source_commit_value = self.store.commit(commit)?.clone();
        let policy_registry = self.policy_registry(commit)?;
        let mut analysis = engine::analyze_with_registry(&ledger.ledger, &policy_registry);
        let mut metadata = BTreeMap::from([
            ("source-commit".to_string(), commit.hash().to_string()),
            (
                "source-evidence".to_string(),
                source.evidence.content().to_string(),
            ),
            ("source".to_string(), source.source().to_string()),
        ]);
        if !source_commit_value.packages.is_empty() {
            metadata.insert(
                "policy-packages".to_string(),
                source_commit_value
                    .packages
                    .iter()
                    .map(|package| package.hash().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }

        let original_roots = analysis.proof.roots.clone();
        let binding = analysis.proof.insert(Node::new(
            format!("source commit {}", commit.hash()),
            Operation::Observation {
                source: format!("commit:{}", commit.hash()),
            },
            original_roots,
            metadata.clone(),
        ));
        analysis.proof.root(binding);
        analysis.proof.check().map_err(|error| {
            WorkspaceError::Store(StoreError::InvalidObject(format!(
                "bound analysis proof is invalid: {error}"
            )))
        })?;

        let proof_object = ProofObject {
            proof: analysis.proof.clone(),
            roots: vec![commit.hash()],
        };
        let proof_id = self.store.put_proof(proof_object)?;

        // A source commit is immutable.  The proof is pinned by a new child
        // commit, making the evaluation result itself content addressed.
        let analysis_commit = self.store.put_commit(Commit {
            parents: vec![commit],
            // The source roots are inherited through `parents`; copying them
            // here would make the analysis commit look like a second source
            // observation to history/as-known-at queries.
            evidence: Vec::new(),
            statements: Vec::new(),
            decisions: Vec::new(),
            completeness: Vec::new(),
            // The analysis result remains tied to the exact policy roots that
            // were evaluated.  They are context, not a second source
            // observation, so they are intentionally copied without copying
            // the source evidence roots.
            packages: source_commit_value.packages.clone(),
            proofs: vec![proof_id],
            schema_version: source_commit_value.schema_version,
            author: ANALYSIS_AUTHOR.to_string(),
            signatures: Vec::new(),
        })?;

        metadata.insert(
            "analysis-commit".to_string(),
            analysis_commit.hash().to_string(),
        );
        Ok(CommitAnalysis {
            source_commit: commit,
            analysis_commit,
            ledger,
            analysis,
            policy_registry,
            proof_id,
            metadata,
        })
    }

    /// Return all commits reachable from `latest`, newest first.  This is a
    /// graph history rather than a mutable “current value” lookup.
    pub fn history(&self, latest: CommitId) -> Result<Vec<CommitId>, WorkspaceError> {
        self.store.commit(latest)?;
        let mut result = Vec::new();
        let mut pending = VecDeque::from([latest]);
        let mut seen = BTreeSet::new();
        while let Some(commit) = pending.pop_front() {
            if !seen.insert(commit) {
                continue;
            }
            let value = self.store.commit(commit)?;
            result.push(commit);
            for parent in &value.parents {
                if !seen.contains(parent) {
                    pending.push_back(*parent);
                }
            }
        }
        Ok(result)
    }

    /// Return source commits for one source, newest first, as immutable
    /// source-ledger objects.
    pub fn source_history(
        &self,
        source: impl Into<String>,
        latest: CommitId,
    ) -> Result<Vec<SourceLedger>, WorkspaceError> {
        let expected = SourceId::try_new(source.into()).map_err(|_| WorkspaceError::EmptySource)?;
        let mut result = Vec::new();
        for commit in self.history(latest)? {
            let value = self.store.commit(commit)?;
            if value.evidence.len() != 1 {
                continue;
            }
            let evidence = self.store.evidence(value.evidence[0])?;
            if evidence.source == expected.to_string() {
                result.push(self.materialize_source_commit(commit)?);
            }
        }
        Ok(result)
    }

    /// Look up the newest evidence for `source` visible from `at`.
    pub fn as_known_at(
        &self,
        source: impl Into<String>,
        at: CommitId,
    ) -> Result<Option<SourceLedger>, WorkspaceError> {
        let expected = SourceId::try_new(source.into()).map_err(|_| WorkspaceError::EmptySource)?;
        for commit in self.history(at)? {
            let value = self.store.commit(commit)?;
            if value.evidence.len() != 1 {
                continue;
            }
            let evidence = self.store.evidence(value.evidence[0])?;
            if evidence.source == expected.to_string() {
                return Ok(Some(self.materialize_source_commit(commit)?));
            }
        }
        Ok(None)
    }

    /// Explicit commit-first spelling for callers that naturally start from
    /// a time-travel point.
    pub fn as_known_at_commit(
        &self,
        at: CommitId,
        source: impl Into<String>,
    ) -> Result<Option<SourceLedger>, WorkspaceError> {
        self.as_known_at(source, at)
    }

    /// Convenience wrapper over the store's evidence correction chain.
    pub fn evidence_history(&self, latest: EvidenceId) -> Result<Vec<EvidenceId>, WorkspaceError> {
        Ok(self.store.evidence_history(latest)?)
    }

    fn correct_source_inner(
        &mut self,
        prior: CommitId,
        source: SourceId,
        bytes: Vec<u8>,
        reason: impl Into<String>,
    ) -> Result<SourceLedger, WorkspaceError> {
        let prior_source = self.source_ledger(prior)?;
        if prior_source.evidence.source() != &source {
            return Err(WorkspaceError::SourceMismatch {
                expected: source,
                actual: prior_source.evidence.source().clone(),
            });
        }
        let prior_value = self.store.commit(prior)?.clone();
        let prior_evidence =
            *prior_value
                .evidence
                .first()
                .ok_or_else(|| WorkspaceError::NotSourceCommit {
                    commit: prior,
                    reason: "source commit has no evidence".to_string(),
                })?;
        let old = self.store.evidence(prior_evidence)?;
        let occurrence = old.occurrence.clone();
        let mut corrected = Evidence::correction(
            occurrence,
            source.to_string(),
            bytes,
            prior_evidence,
            "whole-source",
            reason,
            SOURCE_AUTHOR,
        );
        // `Evidence::correction` computes a store-domain hash.  Replace it
        // with the raw-evidence domain hash so the conversion keeps the
        // immutable RawEvidence identity exactly.
        corrected.normalized_content = RawEvidence::content_hash(&corrected.content);
        if let Some(external) = old.external.clone() {
            corrected = corrected.with_external(external);
        }
        let corrected_id = self.store.put_evidence(corrected)?;
        let commit = self.store.put_commit(Commit::new(
            [prior],
            [corrected_id],
            [],
            [],
            [],
            prior_value.packages,
            [],
            SOURCE_AUTHOR,
        ))?;
        self.heads.insert(source, commit);
        self.materialize_source_commit(commit)
    }

    fn source_ledger(&self, commit: CommitId) -> Result<SourceLedger, WorkspaceError> {
        self.validate_source_commit(commit)?;
        self.materialize_source_commit(commit)
    }

    /// Resolve the executable policy registry from the package roots pinned
    /// by one source commit.  Builtins are the baseline vocabulary; a
    /// committed package of the same name is an explicit, content-addressed
    /// override.  Distinct roots with one name are rejected rather than
    /// silently selected by object ordering.
    fn policy_registry(&self, commit: CommitId) -> Result<PolicyRegistry, WorkspaceError> {
        let value = self.store.commit(commit)?;
        let mut registry = PolicyRegistry::builtins();
        let mut names = BTreeMap::<String, PackageId>::new();
        for id in &value.packages {
            let package = self.store.package(*id)?;
            if let Some(previous) = names.insert(package.name.clone(), *id)
                && previous != *id
            {
                return Err(WorkspaceError::PackageConflict {
                    commit,
                    name: package.name.clone(),
                });
            }
            registry.insert(crate::package::PolicyPackage::new(
                package.name.clone(),
                package.version.clone(),
                package.body.clone(),
            ));
        }
        Ok(registry)
    }

    fn validate_source_commit(&self, commit: CommitId) -> Result<(), WorkspaceError> {
        let value = self.store.commit(commit)?;
        if value.evidence.len() != 1 {
            return Err(WorkspaceError::NotSourceCommit {
                commit,
                reason: "expected exactly one source evidence root".to_string(),
            });
        }
        if value.parents.len() > 1 {
            return Err(WorkspaceError::NotSourceCommit {
                commit,
                reason: "source commits cannot be merge commits".to_string(),
            });
        }
        if !value.statements.is_empty()
            || !value.decisions.is_empty()
            || !value.completeness.is_empty()
            || !value.proofs.is_empty()
        {
            return Err(WorkspaceError::NotSourceCommit {
                commit,
                reason: "commit contains non-source roots other than policy packages".to_string(),
            });
        }
        Ok(())
    }

    fn materialize_source_commit(&self, commit: CommitId) -> Result<SourceLedger, WorkspaceError> {
        self.validate_source_commit(commit)?;
        let value = self.store.commit(commit)?;
        let evidence_id = value.evidence[0];
        let evidence = self.store.evidence(evidence_id)?;
        if !matches!(
            evidence.state,
            EvidenceState::Present | EvidenceState::Correction { .. }
        ) {
            return Err(WorkspaceError::MissingPayload { commit });
        }
        let raw = raw_from_store_evidence(evidence)?;
        let source = str::from_utf8(
            raw.payload()
                .ok_or(WorkspaceError::MissingPayload { commit })?,
        )
        .map_err(|_| WorkspaceError::InvalidUtf8)?
        .to_owned();
        Ok(SourceLedger {
            commit,
            evidence: raw,
            surface: SurfaceFile::parse(source),
        })
    }
}

fn ensure_utf8(bytes: &[u8]) -> Result<(), WorkspaceError> {
    str::from_utf8(bytes)
        .map(|_| ())
        .map_err(|_| WorkspaceError::InvalidUtf8)
}

fn source_occurrence(source: &SourceId) -> String {
    format!("{SOURCE_OCCURRENCE_PREFIX}{source}")
}

fn raw_source_evidence(source: SourceId, bytes: Vec<u8>) -> RawEvidence {
    RawEvidence::from_bytes(source.clone(), source_occurrence(&source), None, bytes)
        .with_provenance(Provenance::new(source.clone()))
        .with_authority(Authority::source(source.to_string()))
}

/// Canonical conversion into the existing ObjectStore evidence family.
fn canonical_evidence(raw: &RawEvidence) -> Evidence {
    let content = raw.payload_owned().unwrap_or_default();
    let mut evidence = Evidence::new(
        raw.occurrence().to_string(),
        raw.source().to_string(),
        content,
    )
    .with_normalized_content(raw.content());
    if let Some(external) = raw.external().cloned() {
        evidence = evidence.with_external(external);
    }
    evidence
}

fn raw_from_store_evidence(evidence: &Evidence) -> Result<RawEvidence, WorkspaceError> {
    let identity = {
        let base = Identity::new(evidence.occurrence.clone(), evidence.normalized_content);
        match evidence.external.clone() {
            Some(external) => base.with_external(external),
            None => base,
        }
    };
    let (availability, payload) = match &evidence.state {
        EvidenceState::Present | EvidenceState::Correction { .. } => (
            crate::evidence::Availability::Present,
            Some(evidence.content.clone()),
        ),
        EvidenceState::Tombstone { .. } => (crate::evidence::Availability::Deleted, None),
        EvidenceState::Unavailable { .. } => (crate::evidence::Availability::Unavailable, None),
        EvidenceState::Redacted { .. } => (crate::evidence::Availability::Redacted, None),
    };
    RawEvidence::new(
        identity,
        Provenance::new(evidence.source.clone()),
        Authority::source(evidence.source.clone()),
        availability,
        payload,
    )
    .map_err(|error| {
        WorkspaceError::Store(StoreError::InvalidObject(format!(
            "cannot reconstruct raw source evidence: {error}"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentHash;
    use crate::store::{Commit, Evidence as StoreEvidence, PolicyPackage as StorePolicyPackage};

    const SOURCE: &str =
        "book tax\nbuy lot-a on 2026-01-01\n  1 ABC into checking\n  for 100 USD\n";

    #[test]
    fn exact_source_is_surface_commit_ledger_analysis_and_checked_proof() {
        let mut workspace = Workspace::new();
        let source = workspace.load_source("book", SOURCE).unwrap();
        assert_eq!(source.lossless_source(), SOURCE);
        assert_eq!(source.evidence.payload(), Some(SOURCE.as_bytes()));
        let again = workspace.load_source("book", SOURCE).unwrap();
        assert_eq!(source.commit, again.commit);
        assert_eq!(workspace.store().len(), 2, "one evidence and one commit");

        let bound = workspace.elaborate_commit(source.commit).unwrap();
        assert_eq!(bound.source_commit, source.commit);
        assert_eq!(bound.book.as_str(), "tax");

        let evaluated = workspace.analyze_commit(source.commit).unwrap();
        assert_eq!(evaluated.source_commit, source.commit);
        assert_eq!(evaluated.ledger.source_commit, source.commit);
        assert_eq!(
            evaluated.metadata.get("source-commit").map(String::as_str),
            Some(source.commit.hash().to_string().as_str())
        );
        evaluated.check_proof().unwrap();
        assert!(workspace.store().proof(evaluated.proof_id).is_ok());
        assert!(workspace.store().commit(evaluated.analysis_commit).is_ok());
        assert_ne!(evaluated.analysis_commit, source.commit);
    }

    #[test]
    fn trivia_changes_source_identity_but_not_strict_semantics() {
        let mut workspace = Workspace::new();
        let first = workspace.load_source("book", SOURCE).unwrap();
        let changed = workspace
            .load_source("book", format!("# comment\n{SOURCE}"))
            .unwrap();
        assert_ne!(first.commit, changed.commit);
        assert_ne!(first.content(), changed.content());
        let first_ledger = workspace.elaborate_commit(first.commit).unwrap();
        let changed_ledger = workspace.elaborate_commit(changed.commit).unwrap();
        assert_eq!(first_ledger.ledger, changed_ledger.ledger);
    }

    #[test]
    fn correction_is_new_evidence_and_history_is_as_known_at() {
        let mut workspace = Workspace::new();
        let first = workspace.load_source("book", SOURCE).unwrap();
        let corrected_text = SOURCE.replace("100 USD", "110 USD");
        let corrected = workspace
            .correct_source_with_reason(
                first.commit,
                corrected_text.as_bytes(),
                "issuer correction",
            )
            .unwrap();
        assert_ne!(first.commit, corrected.commit);
        assert_ne!(first.evidence.content(), corrected.evidence.content());
        assert_eq!(workspace.store().len(), 4, "two evidences and two commits");

        let history = workspace.source_history("book", corrected.commit).unwrap();
        assert_eq!(
            history.iter().map(|item| item.commit).collect::<Vec<_>>(),
            vec![corrected.commit, first.commit]
        );
        assert_eq!(
            workspace
                .as_known_at("book", first.commit)
                .unwrap()
                .unwrap()
                .commit,
            first.commit
        );
        assert_eq!(
            workspace
                .as_known_at("book", corrected.commit)
                .unwrap()
                .unwrap()
                .commit,
            corrected.commit
        );
        let corrected_evidence = workspace.store().commit(corrected.commit).unwrap().evidence[0];
        let first_evidence = workspace.store().commit(first.commit).unwrap().evidence[0];
        assert_eq!(
            workspace.evidence_history(corrected_evidence).unwrap(),
            vec![corrected_evidence, first_evidence]
        );
    }

    #[test]
    fn wrong_commit_rejected_without_confusing_analysis_bindings() {
        let mut workspace = Workspace::new();
        let source = workspace.load_source("book", SOURCE).unwrap();
        let analysis = workspace.analyze_commit(source.commit).unwrap();
        let error = workspace
            .analyze_commit(analysis.analysis_commit)
            .unwrap_err();
        assert!(matches!(error, WorkspaceError::NotSourceCommit { .. }));

        let evidence = workspace
            .store_mut()
            .put_evidence(StoreEvidence::new(
                "other",
                "other",
                b"not a ledger".to_vec(),
            ))
            .unwrap();
        let wrong = workspace
            .store_mut()
            .put_commit(Commit::new([], [evidence], [], [], [], [], [], "test"))
            .unwrap();
        let error = workspace.analyze_commit(wrong).unwrap_err();
        assert!(matches!(error, WorkspaceError::Parse(_)));
    }

    #[test]
    fn analysis_uses_policy_roots_pinned_by_the_source_commit() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
buy buy/two on 2026-02-04
  10 ABC into brokerage
  for 300 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let mut workspace = Workspace::new();
        let package = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "1",
                b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let source = workspace.load_source("book", source).unwrap();
        let source = workspace
            .commit_with_packages(source.commit, [package])
            .unwrap();

        let analysis = workspace.analyze_commit(source.commit).unwrap();
        assert_eq!(analysis.policy.as_deref(), Some("lots/fifo"));
        assert_eq!(
            analysis.sale("sell").unwrap().selected_lot.as_deref(),
            Some("buy/two")
        );
        assert_eq!(
            analysis.metadata.get("policy-packages").map(String::as_str),
            Some(package.hash().to_string().as_str())
        );
        assert_eq!(
            workspace
                .store()
                .commit(analysis.analysis_commit)
                .unwrap()
                .packages,
            vec![package]
        );
        let packages = crate::render::render_packages_with_registry(
            "tax-us",
            analysis.policy.as_deref(),
            &analysis.policy_registry,
        );
        assert!(packages.contains("lots/fifo@1"));
        assert!(packages.contains("[active]"));
        analysis.check_proof().unwrap();
    }

    #[test]
    fn malformed_policy_root_is_a_blocking_analysis_result() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let mut workspace = Workspace::new();
        let package = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "1",
                b"selector=not-supported\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let source = workspace.load_source("book", source).unwrap();
        let source = workspace
            .commit_with_packages(source.commit, [package])
            .unwrap();

        let analysis = workspace.analyze_commit(source.commit).unwrap();
        assert!(analysis.blocked());
        assert!(analysis.issues.iter().any(|issue| {
            issue.code == crate::engine::IssueCode::UnknownPolicy
                && issue.message.contains("not executable")
        }));
        assert!(analysis.sale("sell").unwrap().selected_lot.is_none());
        analysis.check_proof().unwrap();
    }

    #[test]
    fn distinct_policy_roots_with_one_name_are_not_silently_selected() {
        let mut workspace = Workspace::new();
        let first = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "1",
                b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let second = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "2",
                b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let source = workspace.load_source("book", SOURCE).unwrap();
        let source = workspace
            .commit_with_packages(source.commit, [first, second])
            .unwrap();
        assert!(matches!(
            workspace.analyze_commit(source.commit),
            Err(WorkspaceError::PackageConflict { name, .. }) if name == "lots/fifo"
        ));
    }

    #[test]
    fn content_conversion_keeps_raw_hash_and_external_identity() {
        let raw = RawEvidence::from_bytes("bank", "row-1", Some("external-1".into()), b"x");
        let canonical = canonical_evidence(&raw);
        assert_eq!(canonical.occurrence, "row-1");
        assert_eq!(canonical.normalized_content, raw.content());
        assert_eq!(canonical.external_id().unwrap().as_str(), "external-1");
        assert_eq!(canonical.content, b"x");
        assert_ne!(raw.content(), ContentHash::ZERO);
    }
}
