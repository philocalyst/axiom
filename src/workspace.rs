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
use crate::incremental::{
    IncrementalDb, MemoOutcome, QueryError, QueryKey, SourceKey, TraceEvent, TraceMetrics,
};
use crate::model::{ContentHash, Identity, Ledger, SourceId};
use crate::package::{PolicyPackage as ExecutablePolicyPackage, PolicyRegistry};
use crate::parser::{self, ParseError};
use crate::proof::{Node, Operation, Proof};
use crate::store::{
    Commit, CommitId, Evidence, EvidenceId, EvidenceState, ObjectStore, PackageId,
    PolicyPackage as StoredPolicyPackage, ProofObject, ProofObjectId, StoreError,
};
use crate::surface::SurfaceFile;

const SOURCE_OCCURRENCE_PREFIX: &str = "source/";
const SOURCE_AUTHOR: &str = "workspace/source";
const ANALYSIS_AUTHOR: &str = "workspace/analysis";
const QUOTE_PARTITION: &str = "quotes";
const UNRELATED_PARTITION: &str = "unrelated";

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
    source_commit: CommitId,
    ledger: Ledger,
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
    source_commit: CommitId,
    analysis_commit: CommitId,
    ledger: BoundLedger,
    pub analysis: Analysis,
    /// The executable registry resolved from the source commit's package
    /// roots.  Keeping this alongside the bound result lets callers render
    /// the same package set that was actually evaluated.
    pub policy_registry: PolicyRegistry,
    proof_id: ProofObjectId,
    metadata: BTreeMap<String, String>,
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

    pub fn ledger(&self) -> &BoundLedger {
        &self.ledger
    }

    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    pub fn policy_registry(&self) -> &PolicyRegistry {
        &self.policy_registry
    }

    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    pub fn check_proof(&self) -> Result<(), crate::proof::CheckError> {
        self.analysis.check_proof()?;
        let expected = self.source_commit.hash().to_string();
        let mut bindings = self.analysis.proof.nodes.values().filter(|node| {
            matches!(
                &node.operation,
                Operation::Observation { source } if source.starts_with("commit:")
            )
        });
        let Some(binding) = bindings.next() else {
            return Err(crate::proof::CheckError::InvalidOperation {
                id: crate::proof::ProofId::ZERO,
            });
        };
        let valid_operation = matches!(
            &binding.operation,
            Operation::Observation { source } if source == &format!("commit:{expected}")
        );
        let mut expected_inputs = self.analysis.proof.roots.clone();
        expected_inputs.retain(|root| *root != binding.id);
        if bindings.next().is_some()
            || self.ledger.source_commit != self.source_commit
            || !valid_operation
            || binding.metadata.get("source-commit") != Some(&expected)
            || !self.analysis.proof.roots.contains(&binding.id)
            || binding.inputs != expected_inputs
        {
            return Err(crate::proof::CheckError::InvalidOperation { id: binding.id });
        }
        Ok(())
    }
}

impl Deref for CommitAnalysis {
    type Target = Analysis;

    fn deref(&self) -> &Self::Target {
        &self.analysis
    }
}

/// The typed value retained by [`IncrementalDb`].  Store object IDs are
/// intentionally added only after the memo has been evaluated: persistence
/// is a content-addressed side effect, while this value is the semantic
/// result that can be replayed on a cache hit without invoking the engine.
#[derive(Clone, Debug)]
struct PreparedAnalysis {
    source_commit: CommitId,
    ledger: BoundLedger,
    analysis: Analysis,
    policy_registry: PolicyRegistry,
    metadata: BTreeMap<String, String>,
}

/// A failure at the source/commit/elaboration/evaluation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    Store(StoreError),
    Parse(ParseError),
    Incremental(crate::incremental::DatabaseError),
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
            Self::Incremental(error) => write!(formatter, "workspace incremental error: {error}"),
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

impl From<crate::incremental::DatabaseError> for WorkspaceError {
    fn from(value: crate::incremental::DatabaseError) -> Self {
        Self::Incremental(value)
    }
}

/// The canonical immutable-object boundary for source ledgers, corrections,
/// elaboration, and proof-producing evaluation.
#[derive(Clone, Debug, Default)]
pub struct Workspace {
    store: ObjectStore,
    /// A branch reference is not another evidence store.  It only remembers
    /// which source commit `load_source` should treat as the current head.
    heads: BTreeMap<SourceId, CommitId>,
    incremental: IncrementalDb,
}

/// A package accepted by the public workspace insertion boundary.
///
/// Both package representations cross this authoring boundary through one
/// explicit normalization. Executable text is canonicalized before commit;
/// callers that need byte-preserving archival envelopes use `ObjectStore`
/// directly. A stored package with arbitrary manifest metadata or non-UTF-8
/// bytes is rejected instead of being silently narrowed.
pub trait WorkspacePolicyPackage {
    fn into_stored_policy_package(self) -> Result<StoredPolicyPackage, WorkspaceError>;
}

impl WorkspacePolicyPackage for ExecutablePolicyPackage {
    fn into_stored_policy_package(self) -> Result<StoredPolicyPackage, WorkspaceError> {
        Ok(StoredPolicyPackage::from_executable(&self))
    }
}

impl WorkspacePolicyPackage for StoredPolicyPackage {
    fn into_stored_policy_package(self) -> Result<StoredPolicyPackage, WorkspaceError> {
        // Validate through the same path used when resolving a committed
        // root, then normalize executable text while retaining lineage.
        let executable = self.to_executable().map_err(WorkspaceError::Store)?;
        let mut stored = self;
        stored.body = executable.canonical_body_text().into_bytes();
        stored.dependencies = executable.dependencies;
        Ok(stored)
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_store(store: ObjectStore) -> Self {
        Self {
            store,
            heads: BTreeMap::new(),
            incremental: IncrementalDb::new(),
        }
    }

    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    #[cfg(test)]
    pub(crate) fn store_mut(&mut self) -> &mut ObjectStore {
        &mut self.store
    }

    /// Add an immutable policy package without exposing mutation of the
    /// ledger's canonical object store.
    pub fn put_policy_package<P>(&mut self, package: P) -> Result<PackageId, WorkspaceError>
    where
        P: WorkspacePolicyPackage,
    {
        Ok(self
            .store
            .put_package(package.into_stored_policy_package()?)?)
    }

    /// The canonical incremental database used by all workspace analysis.
    /// Its trace is intentionally exposed as a read-only observation surface
    /// for editors and benchmark instrumentation.
    pub fn incremental_db(&self) -> &IncrementalDb {
        &self.incremental
    }

    pub fn incremental_trace(&self) -> &[TraceEvent] {
        self.incremental.trace()
    }

    pub fn incremental_metrics(&self) -> TraceMetrics {
        self.incremental.trace_metrics()
    }

    pub fn clear_incremental_trace(&mut self) {
        self.incremental.clear_trace();
    }

    pub fn set_resource_limit(&mut self, limit: Option<u64>) {
        self.incremental.set_resource_limit(limit);
    }

    pub fn resource_limit(&self) -> Option<u64> {
        self.incremental.resource_limit()
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

        self.sync_source_input(&source, &bytes)?;
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
        let packages: Vec<_> = packages.into_iter().collect();
        let commit = self.store.put_commit(
            Commit::new(
                [source],
                source_value.evidence,
                [],
                source_value.decisions,
                [],
                packages.clone(),
                [],
                SOURCE_AUTHOR,
            )
            .with_conflicts(source_value.conflicts),
        )?;
        self.sync_package_inputs(source_ledger.source(), source_ledger.bytes(), &packages)?;
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
        let source = self.source_ledger(commit)?;
        let source_commit_value = self.store.commit(commit)?.clone();
        self.sync_source_input(source.source(), source.bytes())?;
        let package_names = source_package_names(source.bytes());
        self.sync_package_inputs(
            source.source(),
            source.bytes(),
            &source_commit_value.packages,
        )?;
        let policy_registry = self.policy_registry(commit)?;
        let source_key = source_key(source.source());
        let commit_key = SourceKey::new(format!("workspace/commit/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        self.incremental.upsert_input(
            commit_key.clone(),
            "commit",
            commit.hash().as_bytes().to_vec(),
        )?;
        let analysis_key = QueryKey::new(format!("workspace/analyze/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let elaboration_key = QueryKey::new(format!("workspace/elaborate/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        // Keep the semantic stages visible to the incremental database.  The
        // engine still owns the authoritative proof-producing analysis, but
        // these stage queries make the quote -> valuation -> recognition ->
        // report dependency path explicit at the source/analysis boundary.
        // In particular, a quote correction must not evict derivations that
        // only consume the non-quote partition of the source.
        let valuation_key = QueryKey::new(format!("workspace/valuation/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let recognition_key = QueryKey::new(format!("workspace/recognition/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let report_key = QueryKey::new(format!("workspace/report/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let position_key = QueryKey::new(format!("workspace/position/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let settlement_key = QueryKey::new(format!("workspace/settlement/{}", source.source()))
            .map_err(WorkspaceError::Incremental)?;
        let source_bytes = source.bytes().to_vec();
        let source_for_query = source.clone();
        let source_for_elaboration = source.clone();
        let package_source_for_query = source.source().clone();
        let registry_for_query = policy_registry.clone();
        let package_names_for_query = package_names.clone();
        let package_hashes = source_commit_value
            .packages
            .iter()
            .map(|package| package.hash().to_string())
            .collect::<Vec<_>>();
        let quote_partition_key = source_partition_key(source.source(), QUOTE_PARTITION);
        let unrelated_partition_key = source_partition_key(source.source(), UNRELATED_PARTITION);
        let prepared = self
            .incremental
            .evaluate_typed(analysis_key, move |context| {
                let _ = context.input(&commit_key);
                for package_name in &package_names_for_query {
                    let key =
                        SourceKey::new(package_input_name(&package_source_for_query, package_name))
                            .map_err(|error| {
                                MemoOutcome::error(QueryError::explicit("input", error.to_string()))
                            })?;
                    let _ = context.input(&key);
                }
                let ledger = context.query_typed(elaboration_key, move |context| {
                    let Some(input) = context.input(&source_key) else {
                        return Err(MemoOutcome::incomplete("source input is missing"));
                    };
                    if input.content() != source_bytes.as_slice() {
                        return Err(MemoOutcome::error(QueryError::explicit(
                            "source-mismatch",
                            "source binding does not match the requested commit",
                        )));
                    }
                    let source_text = str::from_utf8(input.content()).map_err(|_| {
                        MemoOutcome::error(QueryError::explicit(
                            "utf8",
                            "source bytes are not valid UTF-8",
                        ))
                    })?;
                    let surface = SurfaceFile::parse(source_text);
                    parser::parse_surface_ledger(&surface)
                        .map_err(|error| {
                            MemoOutcome::error(QueryError::explicit("parse", error.to_string()))
                        })
                        .map(|ledger| {
                            (ledger, source_for_elaboration.content().as_bytes().to_vec())
                        })
                })?;
                let ledger_for_report = ledger.clone();
                let package_source_for_report = package_source_for_query.clone();
                let package_names_for_report = package_names_for_query.clone();
                let package_hashes_for_report = package_hashes.clone();
                let registry_for_report = registry_for_query.clone();
                let report = context.query_typed(report_key, move |context| {
                    let _ = context.query(valuation_key.clone(), |context| {
                        let Some(input) = context.input(&quote_partition_key) else {
                            return MemoOutcome::incomplete("quote partition is missing");
                        };
                        // This value is the exact quote-sensitive source
                        // partition consumed by the valuation stage.
                        MemoOutcome::value(input.content().to_vec())
                    });
                    let _ = context.query(position_key.clone(), |context| {
                        let Some(input) = context.input(&unrelated_partition_key) else {
                            return MemoOutcome::incomplete(
                                "unrelated source partition is missing",
                            );
                        };
                        MemoOutcome::value(input.content().to_vec())
                    });
                    let _ = context.query(settlement_key.clone(), |context| {
                        let Some(input) = context.input(&unrelated_partition_key) else {
                            return MemoOutcome::incomplete(
                                "unrelated source partition is missing",
                            );
                        };
                        MemoOutcome::value(input.content().to_vec())
                    });
                    let recognition = context.query(recognition_key.clone(), |context| {
                        let valuation = context.query(valuation_key.clone(), |_context| {
                            MemoOutcome::incomplete("valuation stage was not evaluated")
                        });
                        if !valuation.is_value() {
                            return valuation;
                        }
                        let position = context.query(position_key.clone(), |_context| {
                            MemoOutcome::incomplete("position stage was not evaluated")
                        });
                        if !position.is_value() {
                            return position;
                        }
                        let settlement = context.query(settlement_key.clone(), |_context| {
                            MemoOutcome::incomplete("settlement stage was not evaluated")
                        });
                        if !settlement.is_value() {
                            return settlement;
                        }
                        let Some(input) = context.input(&unrelated_partition_key) else {
                            return MemoOutcome::incomplete(
                                "unrelated source partition is missing",
                            );
                        };
                        MemoOutcome::value(input.content().to_vec())
                    });
                    if !recognition.is_value() {
                        return Err(recognition);
                    }
                    let report_position = context.query(position_key.clone(), |_context| {
                        MemoOutcome::incomplete("position stage was not evaluated")
                    });
                    if !report_position.is_value() {
                        return Err(report_position);
                    }
                    let report_settlement = context.query(settlement_key.clone(), |_context| {
                        MemoOutcome::incomplete("settlement stage was not evaluated")
                    });
                    if !report_settlement.is_value() {
                        return Err(report_settlement);
                    }
                    for package_name in &package_names_for_report {
                        let key = SourceKey::new(package_input_name(
                            &package_source_for_report,
                            package_name,
                        ))
                        .map_err(|error| {
                            MemoOutcome::error(QueryError::explicit("input", error.to_string()))
                        })?;
                        let _ = context.input(&key);
                    }
                    let analysis =
                        engine::analyze_with_registry(&ledger_for_report, &registry_for_report);
                    let mut bytes = Vec::new();
                    for root in &analysis.proof.roots {
                        bytes.extend_from_slice(&root.0);
                    }
                    if !package_hashes_for_report.is_empty() {
                        bytes.extend_from_slice(package_hashes_for_report.join(",").as_bytes());
                    }
                    Ok((analysis, bytes))
                })?;
                let mut analysis = report;
                let mut metadata = BTreeMap::from([
                    (
                        "source-commit".to_string(),
                        source_for_query.commit.hash().to_string(),
                    ),
                    (
                        "source-evidence".to_string(),
                        source_for_query.evidence.content().to_string(),
                    ),
                    ("source".to_string(), source_for_query.source().to_string()),
                ]);
                if !package_hashes.is_empty() {
                    metadata.insert("policy-packages".to_string(), package_hashes.join(","));
                }
                let original_roots = analysis.proof.roots.clone();
                let binding = analysis.proof.insert(Node::new(
                    format!("source commit {}", source_for_query.commit.hash()),
                    Operation::Observation {
                        source: format!("commit:{}", source_for_query.commit.hash()),
                    },
                    original_roots,
                    metadata.clone(),
                ));
                analysis.proof.root(binding);
                analysis.proof.check().map_err(|error| {
                    MemoOutcome::error(QueryError::explicit(
                        "proof",
                        format!("bound analysis proof is invalid: {error}"),
                    ))
                })?;
                let prepared = PreparedAnalysis {
                    source_commit: source_for_query.commit,
                    ledger: BoundLedger {
                        source_commit: source_for_query.commit,
                        ledger,
                    },
                    analysis,
                    policy_registry: registry_for_query,
                    metadata,
                };
                let mut bytes = Vec::new();
                bytes.extend_from_slice(prepared.source_commit.hash().as_bytes());
                for root in &prepared.analysis.proof.roots {
                    bytes.extend_from_slice(&root.0);
                }
                Ok((prepared, bytes))
            })
            .map_err(|outcome| self.workspace_error_from_memo(commit, outcome))?;
        self.persist_prepared(prepared)
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
        let visible = self.store.evidence_as_known_at(expected.as_str(), at)?;
        if visible.len() > 1 {
            return Err(WorkspaceError::NotSourceCommit {
                commit: at,
                reason: format!(
                    "source {expected} has {} unresolved evidence alternatives",
                    visible.len()
                ),
            });
        }
        let Some(visible) = visible.first().copied() else {
            return Ok(None);
        };
        for commit in self.history(at)? {
            let value = self.store.commit(commit)?;
            if value.evidence.len() != 1 {
                continue;
            }
            if value.evidence[0] == visible {
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

    fn persist_prepared(
        &mut self,
        prepared: PreparedAnalysis,
    ) -> Result<CommitAnalysis, WorkspaceError> {
        let source_commit = prepared.source_commit;
        let source_commit_value = self.store.commit(source_commit)?.clone();
        let proof_id = self.store.put_proof(ProofObject {
            proof: prepared.analysis.proof.clone(),
            roots: vec![source_commit.hash()],
        })?;
        let analysis_commit = self.store.put_commit(Commit {
            parents: vec![source_commit],
            evidence: Vec::new(),
            statements: Vec::new(),
            decisions: source_commit_value.decisions,
            completeness: Vec::new(),
            packages: source_commit_value.packages,
            proofs: vec![proof_id],
            conflicts: source_commit_value.conflicts,
            schema_version: source_commit_value.schema_version,
            author: ANALYSIS_AUTHOR.to_string(),
            signatures: Vec::new(),
        })?;
        let mut metadata = prepared.metadata;
        metadata.insert(
            "analysis-commit".to_string(),
            analysis_commit.hash().to_string(),
        );
        Ok(CommitAnalysis {
            source_commit,
            analysis_commit,
            ledger: prepared.ledger,
            analysis: prepared.analysis,
            policy_registry: prepared.policy_registry,
            proof_id,
            metadata,
        })
    }

    fn workspace_error_from_memo(&self, commit: CommitId, outcome: MemoOutcome) -> WorkspaceError {
        if let Err(error) = self.elaborate_commit(commit) {
            return error;
        }
        let message = match outcome {
            MemoOutcome::Error(error) => error.to_string(),
            MemoOutcome::Incomplete { reason } => reason,
            MemoOutcome::Value(_) => "typed memo state mismatch".to_string(),
        };
        WorkspaceError::Store(StoreError::InvalidObject(format!(
            "incremental analysis failed: {message}"
        )))
    }

    fn sync_source_input(&mut self, source: &SourceId, bytes: &[u8]) -> Result<(), WorkspaceError> {
        self.incremental
            .upsert_input(source_key(source), "source", bytes.to_vec())?;
        let (quotes, unrelated) = source_partitions(bytes);
        self.incremental.upsert_input(
            source_partition_key(source, QUOTE_PARTITION),
            "source-partition",
            quotes,
        )?;
        self.incremental.upsert_input(
            source_partition_key(source, UNRELATED_PARTITION),
            "source-partition",
            unrelated,
        )?;
        Ok(())
    }

    fn sync_package_inputs(
        &mut self,
        source: &SourceId,
        bytes: &[u8],
        packages: &[PackageId],
    ) -> Result<(), WorkspaceError> {
        let names = source_package_names(bytes);
        for name in names {
            let mut matching = packages
                .iter()
                .filter_map(|id| self.store.package(*id).ok().map(|package| (*id, package)))
                .filter(|(_, package)| package.name == name)
                .collect::<Vec<_>>();
            matching.sort_by_key(|(id, _)| *id);
            let content = if matching.is_empty() {
                if let Some(builtin) = crate::package::builtin_policy(&name) {
                    builtin.canonical_bytes()
                } else {
                    format!("missing:{name}").into_bytes()
                }
            } else {
                let mut content = Vec::new();
                for (id, package) in matching {
                    content.extend_from_slice(id.hash().as_bytes());
                    content.extend_from_slice(&package.body);
                    content.extend_from_slice(package.name.as_bytes());
                    content.extend_from_slice(package.version.as_bytes());
                    for (key, value) in &package.manifest {
                        content.extend_from_slice(key.as_bytes());
                        content.push(0);
                        content.extend_from_slice(value.as_bytes());
                        content.push(0xff);
                    }
                    for dependency in &package.dependencies {
                        content.extend_from_slice(dependency.as_bytes());
                    }
                }
                content
            };
            self.incremental.upsert_input(
                SourceKey::new(package_input_name(source, &name))?,
                "package",
                content,
            )?;
        }
        Ok(())
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
        let old = self.store.evidence(prior_evidence)?.clone();
        let occurrence = old.occurrence.clone();
        self.sync_source_input(&source, &bytes)?;
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
        let commit = self.store.put_commit(
            Commit::new(
                [prior],
                [corrected_id],
                [],
                prior_value.decisions,
                [],
                prior_value.packages,
                [],
                SOURCE_AUTHOR,
            )
            .with_conflicts(prior_value.conflicts),
        )?;
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
        let mut names = BTreeMap::<String, ContentHash>::new();
        for id in &value.packages {
            let package = self.store.package(*id)?;
            let executable = package.to_executable().map_err(WorkspaceError::Store)?;
            let executable_hash = executable.hash();
            if let Some(previous) = names.insert(executable.name.clone(), executable_hash)
                && previous != executable_hash
            {
                return Err(WorkspaceError::PackageConflict {
                    commit,
                    name: executable.name.clone(),
                });
            }
            registry.insert(executable);
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
        let mut resolution_decisions = BTreeSet::new();
        let mut has_unresolved_conflict = false;
        for id in &value.conflicts {
            let conflict = self.store.conflict(*id)?;
            if let Some(decision) = conflict.resolution {
                resolution_decisions.insert(decision);
            } else {
                has_unresolved_conflict = true;
            }
        }
        if has_unresolved_conflict {
            return Err(WorkspaceError::NotSourceCommit {
                commit,
                reason: "source contains unresolved semantic conflicts".to_string(),
            });
        }
        if !value.statements.is_empty()
            || value
                .decisions
                .iter()
                .any(|decision| !resolution_decisions.contains(decision))
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

fn source_key(source: &SourceId) -> SourceKey {
    SourceKey::new(format!("workspace/source/{source}"))
        .expect("workspace source keys are never empty")
}

fn source_partition_key(source: &SourceId, partition: &str) -> SourceKey {
    SourceKey::new(format!("workspace/source/{source}/{partition}"))
        .expect("workspace source partition keys are never empty")
}

/// Split a source into the quote-sensitive and remaining semantic partitions.
///
/// This intentionally works at the tolerant surface boundary rather than the
/// strict parser boundary: loading a source retains editor-invalid text, but
/// a quote-only correction can still invalidate only valuation consumers.
/// Node spans include their indented block and exclude surrounding trivia, so
/// comments and blank lines do not accidentally become quote dependencies.
fn source_partitions(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let Ok(source) = str::from_utf8(bytes) else {
        return (Vec::new(), bytes.to_vec());
    };
    let surface = SurfaceFile::parse(source);
    let mut quote_ranges = surface
        .nodes()
        .iter()
        .filter(|node| node.head.as_deref() == Some("quote"))
        .map(|node| node.span)
        .collect::<Vec<_>>();
    quote_ranges.sort_by_key(|span| span.start);

    let mut quotes = Vec::new();
    let mut unrelated = Vec::new();
    let mut cursor = 0;
    for span in quote_ranges {
        if span.start > cursor {
            unrelated.extend_from_slice(&bytes[cursor..span.start]);
        }
        if span.end > span.start && span.end <= bytes.len() {
            quotes.extend_from_slice(&bytes[span.start..span.end]);
            quotes.push(b'\n');
        }
        cursor = cursor.max(span.end);
    }
    if cursor < bytes.len() {
        unrelated.extend_from_slice(&bytes[cursor..]);
    }
    (quotes, unrelated)
}

fn package_input_name(source: &SourceId, name: &str) -> String {
    format!("workspace/package/{source}/{name}")
}

fn source_package_names(bytes: &[u8]) -> BTreeSet<String> {
    let Ok(source) = str::from_utf8(bytes) else {
        return BTreeSet::new();
    };
    source
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("use")).then(|| words.next().map(str::to_owned))?
        })
        .collect()
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
    use crate::package::PolicyPackage as ExecutablePolicyPackage;
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

        let other = workspace
            .load_source(
                "other-book",
                "book other\nbuy lot-b on 2026-01-02\n  1 XYZ into checking\n  for 2 USD\n",
            )
            .unwrap();
        let mut forged = evaluated.clone();
        forged.source_commit = other.commit;
        forged.ledger.source_commit = other.commit;
        assert!(forged.check_proof().is_err());

        let mut missing_binding = evaluated.clone();
        let binding = missing_binding
            .analysis
            .proof
            .nodes
            .values()
            .find(|node| {
                matches!(
                    &node.operation,
                    Operation::Observation { source } if source.starts_with("commit:")
                )
            })
            .unwrap()
            .id;
        missing_binding
            .analysis
            .proof
            .roots
            .retain(|root| *root != binding);
        assert!(missing_binding.check_proof().is_err());
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

    #[test]
    fn unchanged_analysis_is_a_workspace_cache_hit_and_changed_output_matches_clean_run() {
        let mut workspace = Workspace::new();
        let first = workspace.load_source("book", SOURCE).unwrap();
        let initial = workspace.analyze_commit(first.commit).unwrap();
        workspace.clear_incremental_trace();
        let replay = workspace.analyze_commit(first.commit).unwrap();
        assert_eq!(replay, initial);
        assert!(
            workspace
                .incremental_trace()
                .iter()
                .any(|event| matches!(event, TraceEvent::CacheHit { .. }))
        );

        let changed = workspace
            .load_source("book", SOURCE.replace("100 USD", "110 USD"))
            .unwrap();
        let incremental = workspace.analyze_commit(changed.commit).unwrap();
        let invalidated = workspace.incremental_metrics().invalidated_queries;
        assert!(
            invalidated >= 2,
            "source and analysis stages are invalidated"
        );

        let mut clean = Workspace::from_store(workspace.store().clone());
        let full = clean.analyze_commit(changed.commit).unwrap();
        assert_eq!(incremental.analysis, full.analysis);
        assert_eq!(incremental.proof(), full.proof());
        assert_eq!(incremental.metadata, full.metadata);
    }

    #[test]
    fn real_quote_change_recomputes_valuation_recognition_report_and_reuses_unrelated_derivations()
    {
        let source_text = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
quote close on 2026-09-20
  1 ABC = 52 USD
observe position brokerage 10 ABC
observe settlement sell 500 USD into checking
"#;
        let mut workspace = Workspace::new();
        let first = workspace.load_source("book", source_text).unwrap();
        let initial = workspace.analyze_commit(first.commit).unwrap();
        assert_eq!(initial.quote_status.len(), 1);

        workspace.clear_incremental_trace();
        let changed = workspace
            .load_source(
                "book",
                source_text.replace("1 ABC = 52 USD", "1 ABC = 53 USD"),
            )
            .unwrap();
        let incremental = workspace.analyze_commit(changed.commit).unwrap();
        let trace = workspace.incremental_trace().to_vec();

        let recomputed = trace
            .iter()
            .filter_map(|event| match event {
                TraceEvent::Recomputed { query, .. } => Some(query.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert!(recomputed.contains("workspace/valuation/book"));
        assert!(recomputed.contains("workspace/recognition/book"));
        assert!(recomputed.contains("workspace/report/book"));
        assert!(!recomputed.contains("workspace/position/book"));
        assert!(!recomputed.contains("workspace/settlement/book"));
        assert!(recomputed.iter().all(|query| {
            matches!(
                *query,
                "workspace/valuation/book"
                    | "workspace/recognition/book"
                    | "workspace/report/book"
                    | "workspace/elaborate/book"
                    | "workspace/analyze/book"
            )
        }));

        assert!(trace.iter().any(|event| {
            matches!(
                event,
                TraceEvent::CacheHit { query, .. }
                    if query.as_str() == "workspace/position/book"
            )
        }));
        assert!(trace.iter().any(|event| {
            matches!(
                event,
                TraceEvent::CacheHit { query, .. }
                    if query.as_str() == "workspace/settlement/book"
            )
        }));
        let invalidated = trace
            .iter()
            .filter_map(|event| match event {
                TraceEvent::Invalidated { query, .. } => Some(query.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert!(invalidated.contains("workspace/valuation/book"));
        assert!(invalidated.contains("workspace/recognition/book"));
        assert!(invalidated.contains("workspace/report/book"));
        assert!(!invalidated.contains("workspace/position/book"));
        assert!(!invalidated.contains("workspace/settlement/book"));
        assert!(invalidated.iter().all(|query| {
            matches!(
                *query,
                "workspace/valuation/book"
                    | "workspace/recognition/book"
                    | "workspace/report/book"
                    | "workspace/elaborate/book"
                    | "workspace/analyze/book"
            )
        }));

        // The real source-to-analysis result remains identical to an
        // independent clean evaluation, including its proof and metadata.
        let mut clean = Workspace::from_store(workspace.store().clone());
        let full = clean.analyze_commit(changed.commit).unwrap();
        assert_eq!(incremental.analysis, full.analysis);
        assert_eq!(incremental.proof(), full.proof());
        assert_eq!(incremental.metadata, full.metadata);
        assert_eq!(initial.sales, incremental.sales);
        assert_eq!(initial.positions, incremental.positions);
        assert_eq!(initial.settlements, incremental.settlements);
        assert_ne!(initial.quotes, incremental.quotes);
        assert_ne!(initial.analysis, incremental.analysis);
    }

    #[test]
    fn package_upgrade_invalidates_analysis_but_reuses_elaboration() {
        let source_text = r#"book tax-us
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
        let source = workspace.load_source("book", source_text).unwrap();
        let first_package = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "1",
                b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let first = workspace
            .commit_with_packages(source.commit, [first_package])
            .unwrap();
        let _ = workspace.analyze_commit(first.commit).unwrap();

        let upgraded_package = workspace
            .store_mut()
            .put_package(StorePolicyPackage::new(
                "lots/fifo",
                "2",
                b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        workspace.clear_incremental_trace();
        let upgraded = workspace
            .commit_with_packages(first.commit, [upgraded_package])
            .unwrap();
        let changed = workspace.analyze_commit(upgraded.commit).unwrap();
        let trace = workspace.incremental_trace();
        assert!(trace.iter().any(|event| matches!(event, TraceEvent::Invalidated { query, .. } if query.as_str().contains("analyze"))));
        assert!(trace.iter().any(|event| matches!(event, TraceEvent::CacheHit { query, .. } if query.as_str().contains("elaborate"))));
        assert_eq!(
            changed.sale("sell").unwrap().selected_lot.as_deref(),
            Some("buy/two")
        );
    }

    #[test]
    fn package_inputs_are_isolated_by_source_workspace() {
        let source_text = r#"book tax-us
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
        let first_package = workspace
            .put_policy_package(StorePolicyPackage::new(
                "lots/fifo",
                "1",
                b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let second_package = workspace
            .put_policy_package(StorePolicyPackage::new(
                "lots/fifo",
                "2",
                b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
            ))
            .unwrap();
        let first = workspace.load_source("first", source_text).unwrap();
        let first = workspace
            .commit_with_packages(first.commit, [first_package])
            .unwrap();
        let second = workspace.load_source("second", source_text).unwrap();
        let second = workspace
            .commit_with_packages(second.commit, [second_package])
            .unwrap();

        let first_analysis = workspace.analyze_commit(first.commit).unwrap();
        let _ = workspace.analyze_commit(second.commit).unwrap();
        workspace.clear_incremental_trace();
        let replay = workspace.analyze_commit(first.commit).unwrap();

        assert_eq!(replay, first_analysis);
        let metrics = workspace.incremental_metrics();
        assert!(metrics.cache_hits >= 1);
        assert_eq!(metrics.cache_misses, 0);
        assert_eq!(metrics.invalidated_queries, 0);
    }

    #[test]
    fn public_package_boundary_canonicalizes_body_and_preserves_dependencies() {
        let first = ContentHash::domain_separated("test/workspace-package", b"first");
        let second = ContentHash::domain_separated("test/workspace-package", b"second");
        let package = ExecutablePolicyPackage::new(
            "lots/custom",
            "1",
            "tie=ambiguous\nselector=latest_acquisition",
        )
        .with_dependencies([second, first, first]);
        let mut workspace = Workspace::new();
        let id = workspace.put_policy_package(package.clone()).unwrap();
        let stored = workspace.store().package(id).unwrap();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(stored.dependencies, expected);
        assert_eq!(
            stored.body,
            b"selector=latest_acquisition\ntie=ambiguous".to_vec()
        );
        assert_eq!(stored.to_executable().unwrap().hash(), package.hash());

        let equivalent = StorePolicyPackage::new(
            "lots/custom",
            "1",
            b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
        )
        .with_dependencies([first, second]);
        assert_eq!(workspace.put_policy_package(equivalent).unwrap(), id);
        assert!(
            workspace
                .put_policy_package(StorePolicyPackage::new("lots/custom", "1", vec![0xff],))
                .is_err()
        );
        assert!(
            workspace
                .put_policy_package(
                    StorePolicyPackage::new(
                        "lots/custom",
                        "1",
                        b"selector=latest_acquisition\ntie=ambiguous".to_vec(),
                    )
                    .with_manifest([("scope", "tax")]),
                )
                .is_err()
        );
    }
}
