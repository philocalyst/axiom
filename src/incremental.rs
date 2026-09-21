//! A small, deterministic incremental semantic database.
//!
//! This is deliberately a kernel rather than a replacement for the store or
//! the logical engine.  Inputs are immutable content-addressed values, while
//! named source bindings are allowed to move to a new content address.  Query
//! memo entries record the exact input/query edges observed during evaluation;
//! changing one binding therefore invalidates only the transitive dependents
//! of that binding.
//!
//! The evaluator is intentionally conservative.  A cycle is an explicit
//! [`QueryError::Cycle`], and a resource limit produces
//! [`MemoOutcome::Incomplete`].  Neither condition is represented as a false
//! semantic answer, and neither is cached as a successful value.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::model::ContentHash;

const INPUT_DOMAIN: &str = "axiom/incremental/input/v1";
const VALUE_DOMAIN: &str = "axiom/incremental/value/v1";

/// A monotonically increasing database revision.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    pub const INITIAL: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A stable, named source binding such as `quote/abc-usd`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceKey(String);

impl SourceKey {
    pub fn new(name: impl Into<String>) -> Result<Self, DatabaseError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DatabaseError::EmptyName { kind: "source" });
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A named query identity.  Names are semantic keys, not insertion-order
/// handles; two databases evaluating the same name use the same key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueryKey(String);

impl QueryKey {
    pub fn new(name: impl Into<String>) -> Result<Self, DatabaseError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DatabaseError::EmptyName { kind: "query" });
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QueryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The content identity of an immutable normalized input.
pub type InputId = ContentHash;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ContentInput {
    kind: String,
    content: Vec<u8>,
}

/// A read-only input snapshot returned to query code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSnapshot {
    id: InputId,
    source: SourceKey,
    kind: String,
    content: Vec<u8>,
}

impl InputSnapshot {
    pub fn id(&self) -> InputId {
        self.id
    }

    pub fn source(&self) -> &SourceKey {
        &self.source
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }
}

/// An edge recorded by a memoized query.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Dependency {
    /// The source key is always retained.  The optional address makes a read
    /// of a missing input precise too: adding it later invalidates the query.
    Input {
        source: SourceKey,
        content: Option<InputId>,
    },
    Query(QueryKey),
}

/// A content-addressed query result.  The bytes are deliberately opaque to
/// this layer; semantic decoders and proof checkers live above it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoValue {
    id: ContentHash,
    bytes: Vec<u8>,
}

impl MemoValue {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        let id = ContentHash::domain_separated(VALUE_DOMAIN, &bytes);
        Self { id, bytes }
    }

    pub fn id(&self) -> ContentHash {
        self.id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A query result has no implicit false branch.  A semantic false result, if
/// meaningful for a particular query, must be encoded by that query's value
/// format.  Evaluation failures remain visible here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoOutcome {
    Value(MemoValue),
    Error(QueryError),
    Incomplete { reason: String },
}

impl MemoOutcome {
    pub fn value(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Value(MemoValue::from_bytes(bytes))
    }

    pub fn error(error: QueryError) -> Self {
        Self::Error(error)
    }

    pub fn incomplete(reason: impl Into<String>) -> Self {
        Self::Incomplete {
            reason: reason.into(),
        }
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    pub fn is_incomplete(&self) -> bool {
        matches!(self, Self::Incomplete { .. })
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    pub fn as_value(&self) -> Option<&MemoValue> {
        match self {
            Self::Value(value) => Some(value),
            Self::Error(_) | Self::Incomplete { .. } => None,
        }
    }
}

/// Explicit failures from query evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    Cycle { path: Vec<QueryKey> },
    ResourceExhausted { limit: u64, used: u64 },
    EvaluationState { query: QueryKey },
    Explicit { code: String, message: String },
}

impl QueryError {
    pub fn explicit(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Explicit {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle { path } => {
                formatter.write_str("query cycle")?;
                if !path.is_empty() {
                    formatter.write_str(": ")?;
                    for (index, key) in path.iter().enumerate() {
                        if index > 0 {
                            formatter.write_str(" -> ")?;
                        }
                        formatter.write_str(key.as_str())?;
                    }
                }
                Ok(())
            }
            Self::ResourceExhausted { limit, used } => {
                write!(formatter, "resource limit exhausted ({used}/{limit})")
            }
            Self::EvaluationState { query } => {
                write!(formatter, "evaluation state mismatch for query {query}")
            }
            Self::Explicit { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Errors that preserve the database's invariants rather than guessing a
/// repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatabaseError {
    EmptyName { kind: &'static str },
    RevisionExhausted,
    InvalidInputKind,
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName { kind } => write!(formatter, "empty {kind} name"),
            Self::RevisionExhausted => formatter.write_str("database revision exhausted"),
            Self::InvalidInputKind => formatter.write_str("empty input kind"),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// A compact recomputation trace.  Entries are emitted in deterministic
/// execution order; invalidations themselves are emitted in key order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceEvent {
    Invalidated {
        query: QueryKey,
        revision: Revision,
    },
    Evaluating {
        query: QueryKey,
        revision: Revision,
    },
    CacheHit {
        query: QueryKey,
        revision: Revision,
    },
    Recomputed {
        query: QueryKey,
        revision: Revision,
        dependencies: Vec<Dependency>,
        outcome: MemoOutcome,
    },
    Cycle {
        path: Vec<QueryKey>,
        revision: Revision,
    },
    ResourceExhausted {
        query: QueryKey,
        revision: Revision,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MemoRecord {
    revision: Revision,
    dependencies: Vec<Dependency>,
    outcome: MemoOutcome,
    /// Only successful values are cacheable.  Errors and incomplete results
    /// are retained for diagnostics but must be retried on the next request.
    cacheable: bool,
}

/// A public, immutable view of a memo record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoSnapshot {
    revision: Revision,
    dependencies: Vec<Dependency>,
    outcome: MemoOutcome,
    valid: bool,
}

impl MemoSnapshot {
    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    pub fn outcome(&self) -> &MemoOutcome {
        &self.outcome
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

/// Query evaluation context.  The context is the only way a query declares
/// dependencies, making the reverse invalidation graph exact by construction.
pub struct QueryContext<'db> {
    db: &'db mut IncrementalDb,
    query: QueryKey,
    dependencies: BTreeSet<Dependency>,
}

impl<'db> QueryContext<'db> {
    fn depend_on_input(&mut self, source: SourceKey) {
        let content = self.db.bindings.get(&source).copied();
        self.dependencies
            .insert(Dependency::Input { source, content });
    }

    fn depend_on_query(&mut self, key: QueryKey) {
        self.dependencies.insert(Dependency::Query(key));
    }

    /// Read a named input and record the read even when it is currently
    /// missing.  A later insertion then invalidates this query.
    pub fn input(&mut self, source: &SourceKey) -> Option<InputSnapshot> {
        self.depend_on_input(source.clone());
        let current = self.db.bindings.get(source).copied();
        current.and_then(|id| {
            self.db.inputs.get(&id).map(|input| InputSnapshot {
                id,
                source: source.clone(),
                kind: input.kind.clone(),
                content: input.content.clone(),
            })
        })
    }

    /// Evaluate a child query and record the query edge regardless of whether
    /// the child yields a value, an error, or an incomplete result.
    pub fn query<F>(&mut self, key: QueryKey, compute: F) -> MemoOutcome
    where
        F: FnOnce(&mut QueryContext<'_>) -> MemoOutcome,
    {
        self.depend_on_query(key.clone());
        self.db.evaluate_inner(key, compute)
    }

    /// Borrow a narrow adapter for an existing semantic engine.  The adapter
    /// only registers dependency edges; it does not create a second cache or
    /// copy engine results into this database.
    pub fn engine_adapter(&mut self) -> EngineDependencyAdapter<'_, 'db> {
        EngineDependencyAdapter { context: self }
    }

    pub fn query_key(&self) -> &QueryKey {
        &self.query
    }
}

/// Dependency-registration bridge for the current proof-producing engine.
///
/// Existing engine code can feed source occurrence names and named semantic
/// goals into this adapter while it computes its own value.  The incremental
/// database stores only the resulting edges and memo value; it does not own or
/// duplicate the engine's analysis cache.
pub struct EngineDependencyAdapter<'context, 'db> {
    context: &'context mut QueryContext<'db>,
}

impl<'context, 'db> EngineDependencyAdapter<'context, 'db> {
    pub fn depend_on_input(&mut self, source: impl Into<String>) -> Result<(), DatabaseError> {
        let source = SourceKey::new(source)?;
        self.context.depend_on_input(source);
        Ok(())
    }

    pub fn depend_on_query(&mut self, query: impl Into<String>) -> Result<(), DatabaseError> {
        let query = QueryKey::new(query)?;
        self.context.depend_on_query(query);
        Ok(())
    }

    pub fn depend_on_inputs<I, S>(&mut self, sources: I) -> Result<(), DatabaseError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for source in sources {
            self.depend_on_input(source)?;
        }
        Ok(())
    }

    pub fn depend_on_queries<I, S>(&mut self, queries: I) -> Result<(), DatabaseError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for query in queries {
            self.depend_on_query(query)?;
        }
        Ok(())
    }
}

/// A compact exact incremental semantic database.
#[derive(Clone, Debug, Default)]
pub struct IncrementalDb {
    revision: Revision,
    inputs: BTreeMap<InputId, ContentInput>,
    bindings: BTreeMap<SourceKey, InputId>,
    memos: BTreeMap<QueryKey, MemoRecord>,
    reverse_inputs: BTreeMap<SourceKey, BTreeSet<QueryKey>>,
    reverse_queries: BTreeMap<QueryKey, BTreeSet<QueryKey>>,
    active: Vec<QueryKey>,
    resource_limit: Option<u64>,
    work_used: u64,
    trace: Vec<TraceEvent>,
}

impl IncrementalDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Set or replace a source binding.  The content object is immutable and
    /// deduplicated by its address; only the named binding and revision move.
    pub fn upsert_input(
        &mut self,
        source: SourceKey,
        kind: impl Into<String>,
        content: impl Into<Vec<u8>>,
    ) -> Result<InputId, DatabaseError> {
        let kind = kind.into();
        if kind.trim().is_empty() {
            return Err(DatabaseError::InvalidInputKind);
        }
        let content = content.into();
        let id = hash_input(&kind, &content);
        self.inputs
            .entry(id)
            .or_insert_with(|| ContentInput { kind, content });

        if self.bindings.get(&source).copied() == Some(id) {
            return Ok(id);
        }
        self.advance_revision()?;
        self.bindings.insert(source.clone(), id);
        self.invalidate_source(&source);
        Ok(id)
    }

    pub fn remove_input(&mut self, source: &SourceKey) -> Result<bool, DatabaseError> {
        if self.bindings.remove(source).is_none() {
            return Ok(false);
        }
        self.advance_revision()?;
        self.invalidate_source(source);
        Ok(true)
    }

    pub fn input(&self, source: &SourceKey) -> Option<InputSnapshot> {
        let id = self.bindings.get(source).copied()?;
        let input = self.inputs.get(&id)?;
        Some(InputSnapshot {
            id,
            source: source.clone(),
            kind: input.kind.clone(),
            content: input.content.clone(),
        })
    }

    pub fn input_id(&self, source: &SourceKey) -> Option<InputId> {
        self.bindings.get(source).copied()
    }

    pub fn content_count(&self) -> usize {
        self.inputs.len()
    }

    pub fn set_resource_limit(&mut self, limit: Option<u64>) {
        // The available resource profile affects whether a query may be
        // evaluated at all.  A complete value from a generous profile cannot
        // be replayed as though a stricter profile had produced it.
        let profile_changed = self.resource_limit != limit || self.work_used != 0;
        if !profile_changed {
            return;
        }
        self.resource_limit = limit;
        self.work_used = 0;
        self.invalidate_resource_profile();
    }

    pub fn resource_limit(&self) -> Option<u64> {
        self.resource_limit
    }

    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    pub fn trace(&self) -> &[TraceEvent] {
        &self.trace
    }

    pub fn memo(&self, key: &QueryKey) -> Option<MemoSnapshot> {
        let memo = self.memos.get(key)?;
        Some(MemoSnapshot {
            revision: memo.revision,
            dependencies: memo.dependencies.clone(),
            outcome: memo.outcome.clone(),
            // Revision is provenance for when this value was computed, not a
            // coarse invalidation fence.  Unrelated input changes must leave
            // this memo usable; precise reverse edges mark affected memos.
            valid: memo.cacheable,
        })
    }

    pub fn is_valid(&self, key: &QueryKey) -> bool {
        self.memo(key).is_some_and(|memo| memo.is_valid())
    }

    pub fn stale_queries(&self) -> BTreeSet<QueryKey> {
        self.memos
            .iter()
            .filter_map(|(key, memo)| {
                if memo.cacheable {
                    None
                } else {
                    Some(key.clone())
                }
            })
            .collect()
    }

    /// Evaluate a named query.  A valid value memo is returned without
    /// executing `compute`; a stale/missing entry records fresh dependencies.
    pub fn evaluate<F>(&mut self, key: QueryKey, compute: F) -> MemoOutcome
    where
        F: FnOnce(&mut QueryContext<'_>) -> MemoOutcome,
    {
        self.evaluate_inner(key, compute)
    }

    fn evaluate_inner<F>(&mut self, key: QueryKey, compute: F) -> MemoOutcome
    where
        F: FnOnce(&mut QueryContext<'_>) -> MemoOutcome,
    {
        if let Some(index) = self.active.iter().position(|active| active == &key) {
            let mut path = self.active[index..].to_vec();
            path.push(key.clone());
            self.trace.push(TraceEvent::Cycle {
                path: path.clone(),
                revision: self.revision,
            });
            return MemoOutcome::Error(QueryError::Cycle { path });
        }

        if let Some(memo) = self.memos.get(&key)
            && memo.cacheable
        {
            self.trace.push(TraceEvent::CacheHit {
                query: key,
                revision: self.revision,
            });
            return memo.outcome.clone();
        }

        if let Some(limit) = self.resource_limit {
            if self.work_used >= limit {
                self.trace.push(TraceEvent::ResourceExhausted {
                    query: key.clone(),
                    revision: self.revision,
                });
                let outcome = MemoOutcome::Incomplete {
                    reason: format!(
                        "query resource limit exhausted ({}/{limit})",
                        self.work_used
                    ),
                };
                self.record_memo(key, BTreeSet::new(), outcome.clone());
                return outcome;
            }
            self.work_used = self.work_used.saturating_add(1);
        }

        self.trace.push(TraceEvent::Evaluating {
            query: key.clone(),
            revision: self.revision,
        });
        self.active.push(key.clone());
        let mut context = QueryContext {
            db: self,
            query: key.clone(),
            dependencies: BTreeSet::new(),
        };
        let outcome = compute(&mut context);
        let dependencies = context.dependencies.clone();
        let db = context.db;
        let popped = db.active.pop();
        if popped.as_ref() != Some(&key) {
            let state_error =
                MemoOutcome::Error(QueryError::EvaluationState { query: key.clone() });
            db.record_memo(key.clone(), dependencies, state_error.clone());
            return state_error;
        }
        db.record_memo(key, dependencies, outcome.clone());
        outcome
    }

    fn record_memo(
        &mut self,
        key: QueryKey,
        dependencies: BTreeSet<Dependency>,
        outcome: MemoOutcome,
    ) {
        self.remove_reverse_edges(&key);
        let dependencies: Vec<_> = dependencies.into_iter().collect();
        for dependency in &dependencies {
            match dependency {
                Dependency::Input { source, .. } => {
                    self.reverse_inputs
                        .entry(source.clone())
                        .or_default()
                        .insert(key.clone());
                }
                Dependency::Query(dependency) => {
                    self.reverse_queries
                        .entry(dependency.clone())
                        .or_default()
                        .insert(key.clone());
                }
            }
        }
        let cacheable = outcome.is_value();
        let revision = self.revision;
        self.memos.insert(
            key.clone(),
            MemoRecord {
                revision,
                dependencies: dependencies.clone(),
                outcome: outcome.clone(),
                cacheable,
            },
        );
        self.trace.push(TraceEvent::Recomputed {
            query: key,
            revision,
            dependencies,
            outcome,
        });
    }

    fn remove_reverse_edges(&mut self, key: &QueryKey) {
        for dependents in self.reverse_inputs.values_mut() {
            dependents.remove(key);
        }
        for dependents in self.reverse_queries.values_mut() {
            dependents.remove(key);
        }
    }

    fn invalidate_source(&mut self, source: &SourceKey) {
        let mut pending: VecDeque<QueryKey> = self
            .reverse_inputs
            .get(source)
            .into_iter()
            .flat_map(|queries| queries.iter().cloned())
            .collect();
        let mut seen = BTreeSet::new();
        while let Some(query) = pending.pop_front() {
            if !seen.insert(query.clone()) {
                continue;
            }
            let had_memo = self.memos.contains_key(&query);
            if let Some(memo) = self.memos.get_mut(&query) {
                memo.cacheable = false;
                // Keep the old revision and edges for explainability and for
                // continuing the reverse walk through this stale node.
                if had_memo {
                    self.trace.push(TraceEvent::Invalidated {
                        query: query.clone(),
                        revision: self.revision,
                    });
                }
            }
            if let Some(dependents) = self.reverse_queries.get(&query) {
                pending.extend(dependents.iter().cloned());
            }
        }
    }

    fn invalidate_resource_profile(&mut self) {
        let keys: Vec<_> = self.memos.keys().cloned().collect();
        for query in keys {
            if let Some(memo) = self.memos.get_mut(&query)
                && memo.cacheable
            {
                memo.cacheable = false;
                self.trace.push(TraceEvent::Invalidated {
                    query,
                    revision: self.revision,
                });
            }
        }
    }

    fn advance_revision(&mut self) -> Result<(), DatabaseError> {
        self.revision.0 = self
            .revision
            .0
            .checked_add(1)
            .ok_or(DatabaseError::RevisionExhausted)?;
        self.work_used = 0;
        Ok(())
    }
}

fn hash_input(kind: &str, content: &[u8]) -> InputId {
    let mut canonical = Vec::with_capacity(kind.len() + content.len() + 16);
    put_bytes(&mut canonical, kind.as_bytes());
    put_bytes(&mut canonical, content);
    ContentHash::domain_separated(INPUT_DOMAIN, &canonical)
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    output.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> SourceKey {
        SourceKey::new(name).expect("test source key")
    }

    fn query(name: &str) -> QueryKey {
        QueryKey::new(name).expect("test query key")
    }

    fn read_value(ctx: &mut QueryContext<'_>, source: &SourceKey) -> MemoOutcome {
        let input = ctx.input(source);
        match input {
            Some(input) => MemoOutcome::value(input.content().to_vec()),
            None => MemoOutcome::incomplete("missing input"),
        }
    }

    #[test]
    fn content_is_idempotent_and_source_order_does_not_change_keys() {
        let quote = source("quote/abc-usd");
        let report = source("report/monthly");
        let mut left = IncrementalDb::new();
        let left_quote = left
            .upsert_input(quote.clone(), "quote", b"1 ABC = 52.14 USD".to_vec())
            .expect("quote");
        let left_report = left
            .upsert_input(report.clone(), "report", b"september".to_vec())
            .expect("report");
        let revision = left.revision();
        let duplicate = left
            .upsert_input(quote.clone(), "quote", b"1 ABC = 52.14 USD".to_vec())
            .expect("idempotent quote");
        assert_eq!(left_quote, duplicate);
        assert_eq!(left.revision(), revision);
        assert_eq!(left.content_count(), 2);

        let mut right = IncrementalDb::new();
        let right_report = right
            .upsert_input(report, "report", b"september".to_vec())
            .expect("report");
        let right_quote = right
            .upsert_input(quote, "quote", b"1 ABC = 52.14 USD".to_vec())
            .expect("quote");
        assert_eq!(left_quote, right_quote);
        assert_eq!(left_report, right_report);
    }

    #[test]
    fn quote_change_invalidates_only_valuation_recognition_report_chain() {
        let quote = source("quote/abc-usd");
        let bank = source("bank/checking");
        let mut db = IncrementalDb::new();
        db.upsert_input(quote.clone(), "quote", b"52.14".to_vec())
            .expect("quote");
        db.upsert_input(bank.clone(), "statement", b"500".to_vec())
            .expect("bank");

        let valuation = query("valuation/portfolio");
        let recognition = query("recognition/tax");
        let report = query("report/monthly");
        let unrelated = query("position/cash");
        let build_valuation = |ctx: &mut QueryContext<'_>| read_value(ctx, &quote);
        db.evaluate(valuation.clone(), build_valuation);
        db.evaluate(recognition.clone(), |ctx| {
            ctx.query(valuation.clone(), build_valuation)
        });
        db.evaluate(report.clone(), |ctx| {
            ctx.query(recognition.clone(), |ctx| {
                ctx.query(valuation.clone(), build_valuation)
            })
        });
        db.evaluate(unrelated.clone(), |ctx| read_value(ctx, &bank));
        db.clear_trace();

        db.upsert_input(quote.clone(), "quote", b"52.15".to_vec())
            .expect("changed quote");
        assert!(db.memo(&valuation).is_some_and(|memo| !memo.is_valid()));
        assert!(db.memo(&recognition).is_some_and(|memo| !memo.is_valid()));
        assert!(db.memo(&report).is_some_and(|memo| !memo.is_valid()));
        assert!(db.memo(&unrelated).is_some_and(|memo| memo.is_valid()));
        let invalidated: BTreeSet<_> = db
            .trace()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::Invalidated { query, .. } => Some(query.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            invalidated,
            BTreeSet::from([valuation, recognition, report])
        );
    }

    #[test]
    fn recomputation_trace_has_sorted_edges_and_cache_hits() {
        let quote = source("quote");
        let bank = source("bank");
        let root = query("report");
        let mut db = IncrementalDb::new();
        db.upsert_input(quote.clone(), "quote", b"q".to_vec())
            .expect("quote");
        db.upsert_input(bank.clone(), "statement", b"b".to_vec())
            .expect("bank");
        db.evaluate(root.clone(), |ctx| {
            let _ = ctx.input(&bank);
            let _ = ctx.input(&quote);
            MemoOutcome::value(b"ok".to_vec())
        });
        let memo = db.memo(&root).expect("memo");
        assert!(memo.is_valid());
        assert!(
            memo.dependencies()
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
        db.clear_trace();
        let first = db.evaluate(root.clone(), |_ctx| {
            panic!("a valid value must be a cache hit")
        });
        assert!(first.is_value());
        assert!(matches!(db.trace(), [TraceEvent::CacheHit { .. }]));
    }

    #[test]
    fn cycles_are_errors_and_resource_exhaustion_is_incomplete() {
        let mut db = IncrementalDb::new();
        let first = query("first");
        let second = query("second");
        let outcome = db.evaluate(first.clone(), |ctx| {
            ctx.query(second.clone(), |ctx| {
                ctx.query(first.clone(), |_ctx| MemoOutcome::value(b"never".to_vec()))
            })
        });
        match outcome {
            MemoOutcome::Error(QueryError::Cycle { path }) => {
                assert_eq!(path, vec![first.clone(), second.clone(), first.clone()]);
            }
            other => panic!("expected explicit cycle, got {other:?}"),
        }
        assert!(!db.is_valid(&first));
        assert!(!db.is_valid(&second));

        db.set_resource_limit(Some(0));
        let limited = db.evaluate(query("limited"), |_ctx| {
            MemoOutcome::value(b"false".to_vec())
        });
        assert!(matches!(limited, MemoOutcome::Incomplete { .. }));
    }

    #[test]
    fn missing_input_is_invalidated_when_later_supplied() {
        let quote = source("quote");
        let valuation = query("valuation");
        let mut db = IncrementalDb::new();
        let initial = db.evaluate(valuation.clone(), |ctx| read_value(ctx, &quote));
        assert!(initial.is_incomplete());
        db.upsert_input(quote.clone(), "quote", b"52.14".to_vec())
            .expect("quote");
        assert!(!db.is_valid(&valuation));
        let updated = db.evaluate(valuation.clone(), |ctx| read_value(ctx, &quote));
        assert!(updated.is_value());
    }

    #[test]
    fn permutations_of_dependency_declaration_have_equal_memos() {
        let quote = source("quote");
        let bank = source("bank");
        let key = query("report");
        let mut first = IncrementalDb::new();
        first
            .upsert_input(quote.clone(), "quote", b"q".to_vec())
            .expect("quote");
        first
            .upsert_input(bank.clone(), "statement", b"b".to_vec())
            .expect("bank");
        first.evaluate(key.clone(), |ctx| {
            let _ = ctx.input(&quote);
            let _ = ctx.input(&bank);
            MemoOutcome::value(b"same".to_vec())
        });

        let mut second = IncrementalDb::new();
        second
            .upsert_input(bank.clone(), "statement", b"b".to_vec())
            .expect("bank");
        second
            .upsert_input(quote.clone(), "quote", b"q".to_vec())
            .expect("quote");
        second.evaluate(key.clone(), |ctx| {
            let _ = ctx.input(&bank);
            let _ = ctx.input(&quote);
            MemoOutcome::value(b"same".to_vec())
        });
        assert_eq!(
            first.memo(&key).expect("first memo").dependencies(),
            second.memo(&key).expect("second memo").dependencies()
        );
        assert_eq!(
            first.memo(&key).expect("first memo").outcome(),
            second.memo(&key).expect("second memo").outcome()
        );
    }

    #[test]
    fn changing_resource_profile_cannot_replay_a_complete_value() {
        let key = query("report");
        let mut db = IncrementalDb::new();
        db.set_resource_limit(Some(100));
        let complete = db.evaluate(key.clone(), |_ctx| MemoOutcome::value(b"complete".to_vec()));
        assert!(complete.is_value());
        assert!(db.is_valid(&key));

        db.clear_trace();
        db.set_resource_limit(Some(0));
        assert!(!db.is_valid(&key));
        let limited = db.evaluate(key.clone(), |_ctx| {
            // A zero budget must stop before this closure is called.  The
            // value here is deliberately opposite to the incomplete result.
            MemoOutcome::value(b"must-not-be-replayed".to_vec())
        });
        assert!(matches!(limited, MemoOutcome::Incomplete { .. }));
        assert!(db.trace().iter().any(|event| matches!(
            event,
            TraceEvent::Invalidated { query, .. } if query == &key
        )));
        assert!(db.trace().iter().any(|event| matches!(
            event,
            TraceEvent::ResourceExhausted { query, .. } if query == &key
        )));

        db.clear_trace();
        db.set_resource_limit(Some(100));
        let restored = db.evaluate(key.clone(), |_ctx| MemoOutcome::value(b"restored".to_vec()));
        assert_eq!(
            restored.as_value().map(MemoValue::bytes),
            Some(&b"restored"[..])
        );
        assert!(matches!(
            db.trace(),
            [TraceEvent::Evaluating { .. }, TraceEvent::Recomputed { .. }]
        ));

        // Replaying the same limit transition yields byte-for-byte equal
        // trace events, including the reverse transition back to a complete
        // result.
        let expected_trace = db.trace().to_vec();
        let mut replay = IncrementalDb::new();
        replay.set_resource_limit(Some(100));
        replay.evaluate(key.clone(), |_ctx| MemoOutcome::value(b"complete".to_vec()));
        replay.clear_trace();
        replay.set_resource_limit(Some(0));
        replay.evaluate(key.clone(), |_ctx| {
            MemoOutcome::value(b"must-not-run".to_vec())
        });
        replay.clear_trace();
        replay.set_resource_limit(Some(100));
        replay.evaluate(key, |_ctx| MemoOutcome::value(b"restored".to_vec()));
        assert_eq!(expected_trace, replay.trace());
    }

    #[test]
    fn engine_adapter_registers_edges_without_another_cache() {
        let ledger = source("ledger/main");
        let report = query("report/close");
        let mut db = IncrementalDb::new();
        db.upsert_input(ledger.clone(), "ledger", b"source".to_vec())
            .expect("ledger");
        db.evaluate(report.clone(), |context| {
            let mut adapter = context.engine_adapter();
            adapter
                .depend_on_inputs(["ledger/main"])
                .expect("input edge");
            adapter
                .depend_on_queries(["recognition/close"])
                .expect("query edge");
            MemoOutcome::value(b"report".to_vec())
        });
        let memo = db.memo(&report).expect("memo");
        assert_eq!(
            memo.dependencies(),
            &[
                Dependency::Input {
                    source: ledger,
                    content: db.input_id(&source("ledger/main")),
                },
                Dependency::Query(query("recognition/close")),
            ]
        );
    }
}
