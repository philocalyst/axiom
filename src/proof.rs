//! Small, deterministic proof DAGs used by the semantic engine.
//!
//! A proof is deliberately boring: every node is content addressed, all
//! dependencies are explicit, and checking a proof does not call the engine.
//! This is important for a ledger. The engine is allowed to derive a view;
//! the checker independently verifies graph integrity and the exact arithmetic
//! certificates V0 knows how to express.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::exact::Exact;

/// A 256 bit, content-addressed proof node identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProofId(pub [u8; 32]);

impl ProofId {
    pub const ZERO: Self = Self([0; 32]);

    pub fn hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{byte:02x}");
        }
        out
    }
}

impl fmt::Display for ProofId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

/// The part of a statement that is meaningful to a consumer of a proof.
///
/// It is kept as canonical text rather than a Rust enum on purpose.  The
/// statement grammar belongs to the finance engine and may grow without
/// making old proof readers unable to inspect a new statement.  A node's
/// operation below is still typed enough for an independent structural
/// checker to validate its edges.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Statement(pub String);

impl Statement {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A deterministic derivation operation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Operation {
    /// A source ledger fact or other immutable observation.
    Observation { source: String },
    /// A rule application.  The rule name is content-addressed by the
    /// package layer when one exists; the checker treats it as data.
    Derive { rule: String },
    /// An exact subtraction certificate checked without invoking the solver.
    Arithmetic {
        rule: String,
        minuend: Exact,
        subtrahend: Exact,
        result: Exact,
        unit: String,
    },
    /// A selected answer to an explicitly named resolution question.
    Decision { subject: String, answer: String },
    /// A policy application.  Policy and decision results are intentionally
    /// separate so a conflict can be represented rather than hidden.
    Policy {
        subject: String,
        policy: String,
        answer: String,
    },
    /// Explicitly records why a result is blocked or contradictory.
    Conflict { subject: String, reason: String },
}

impl Operation {
    fn tag(&self) -> &'static [u8] {
        match self {
            Self::Observation { .. } => b"observation",
            Self::Derive { .. } => b"derive",
            Self::Arithmetic { .. } => b"arithmetic",
            Self::Decision { .. } => b"decision",
            Self::Policy { .. } => b"policy",
            Self::Conflict { .. } => b"conflict",
        }
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.tag());
        match self {
            Self::Observation { source } => put_string(out, source),
            Self::Derive { rule } => put_string(out, rule),
            Self::Arithmetic {
                rule,
                minuend,
                subtrahend,
                result,
                unit,
            } => {
                put_string(out, rule);
                put_string(out, &minuend.canonical_string());
                put_string(out, &subtrahend.canonical_string());
                put_string(out, &result.canonical_string());
                put_string(out, unit);
            }
            Self::Decision { subject, answer } => {
                put_string(out, subject);
                put_string(out, answer);
            }
            Self::Policy {
                subject,
                policy,
                answer,
            } => {
                put_string(out, subject);
                put_string(out, policy);
                put_string(out, answer);
            }
            Self::Conflict { subject, reason } => {
                put_string(out, subject);
                put_string(out, reason);
            }
        }
    }
}

/// One certificate node.  `id` is checked against all other fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub id: ProofId,
    pub statement: Statement,
    pub operation: Operation,
    pub inputs: Vec<ProofId>,
    /// Stable, sorted metadata (dependency roots, source occurrence IDs,
    /// invalidation roots, and similar).  Values are opaque to this module,
    /// but are included in the node hash and therefore cannot be changed
    /// without invalidating the certificate.
    pub metadata: BTreeMap<String, String>,
}

impl Node {
    pub fn new(
        statement: impl Into<String>,
        operation: Operation,
        mut inputs: Vec<ProofId>,
        metadata: BTreeMap<String, String>,
    ) -> Self {
        // Proofs are set-like at the dependency boundary.  Keeping a sorted,
        // duplicate-free edge list makes the hash independent of traversal
        // order and prevents accidental duplicate invalidation edges.
        inputs.sort();
        inputs.dedup();
        let statement = Statement::new(statement);
        let id = hash_node(&statement, &operation, &inputs, &metadata);
        Self {
            id,
            statement,
            operation,
            inputs,
            metadata,
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.id
            == hash_node(
                &self.statement,
                &self.operation,
                &self.inputs,
                &self.metadata,
            )
            && self.inputs.windows(2).all(|pair| pair[0] < pair[1])
            && self.metadata.keys().all(|key| !key.is_empty())
    }
}

/// A proof DAG.  The map and roots are canonicalized by all constructors.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Proof {
    pub nodes: BTreeMap<ProofId, Node>,
    pub roots: Vec<ProofId>,
}

impl Proof {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, node: Node) -> ProofId {
        let id = node.id;
        match self.nodes.get(&id) {
            Some(_existing) => {
                // Diagnostic source locations are deliberately excluded from
                // the content address, so two equal observations at distinct
                // locations may share one node.  Keep the first location as
                // the stable representative rather than making insertion
                // order observable or panicking in debug builds.
            }
            None => {
                self.nodes.insert(id, node);
            }
        }
        id
    }

    pub fn root(&mut self, id: ProofId) {
        if !self.roots.contains(&id) {
            self.roots.push(id);
            self.roots.sort();
        }
    }

    pub fn node(&self, id: ProofId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Validate the complete bundle and require a particular node to be
    /// present.  Callers at semantic boundaries use this instead of treating
    /// a non-zero hash as proof merely because it has the right width.
    pub fn check_member(&self, id: ProofId) -> Result<&Node, CheckError> {
        self.check()?;
        self.node(id).ok_or(CheckError::MissingNode { id })
    }

    /// Run the independent structural checker.  No engine code is called.
    pub fn check(&self) -> Result<(), CheckError> {
        let mut roots = self.roots.clone();
        roots.sort();
        roots.dedup();
        if roots != self.roots {
            return Err(CheckError::NonCanonicalRoots);
        }
        for (id, node) in &self.nodes {
            if *id != node.id {
                return Err(CheckError::MapKeyMismatch {
                    expected: *id,
                    actual: node.id,
                });
            }
            if !node.is_well_formed() {
                return Err(CheckError::TamperedNode { id: *id });
            }
            if let Operation::Arithmetic {
                minuend,
                subtrahend,
                result,
                unit,
                ..
            } = &node.operation
                && (unit.is_empty() || minuend.checked_sub(subtrahend) != *result)
            {
                return Err(CheckError::InvalidArithmetic { id: *id });
            }
            let invalid_operation = match &node.operation {
                Operation::Observation { source } => source.trim().is_empty(),
                Operation::Derive { rule } => rule.trim().is_empty(),
                Operation::Arithmetic { rule, unit, .. } => {
                    rule.trim().is_empty() || unit.trim().is_empty()
                }
                Operation::Decision { subject, answer } => {
                    subject.trim().is_empty() || answer.trim().is_empty()
                }
                Operation::Policy {
                    subject,
                    policy,
                    answer,
                } => {
                    subject.trim().is_empty()
                        || policy.trim().is_empty()
                        || answer.trim().is_empty()
                }
                Operation::Conflict { subject, reason } => {
                    subject.trim().is_empty() || reason.trim().is_empty()
                }
            };
            if invalid_operation {
                return Err(CheckError::InvalidOperation { id: *id });
            }
            for input in &node.inputs {
                if !self.nodes.contains_key(input) {
                    return Err(CheckError::MissingInput {
                        node: *id,
                        input: *input,
                    });
                }
            }
        }
        for root in &self.roots {
            if !self.nodes.contains_key(root) {
                return Err(CheckError::MissingRoot { root: *root });
            }
        }
        // Kahn's algorithm catches both self-dependencies and longer cycles.
        let mut remaining: BTreeMap<ProofId, usize> = self
            .nodes
            .iter()
            .map(|(id, node)| (*id, node.inputs.len()))
            .collect();
        let mut reverse: BTreeMap<ProofId, Vec<ProofId>> = BTreeMap::new();
        for (id, node) in &self.nodes {
            for input in &node.inputs {
                reverse.entry(*input).or_default().push(*id);
            }
        }
        let mut ready: BTreeSet<ProofId> = remaining
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(*id))
            .collect();
        let mut visited = 0usize;
        while let Some(id) = ready.pop_first() {
            visited += 1;
            if let Some(dependants) = reverse.get(&id) {
                for dependant in dependants {
                    let count = remaining.get_mut(dependant).expect("reverse edge exists");
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(*dependant);
                    }
                }
            }
        }
        if visited != self.nodes.len() {
            return Err(CheckError::Cycle);
        }
        Ok(())
    }

    /// Return the proof's stable content root.  This is useful as a cache key
    /// and deliberately includes roots as well as reachable node content.
    pub fn content_hash(&self) -> ProofId {
        let mut bytes = Vec::new();
        put_bytes(&mut bytes, b"axiom/proof/v1");
        put_u64(&mut bytes, self.roots.len() as u64);
        for root in &self.roots {
            bytes.extend_from_slice(&root.0);
        }
        put_u64(&mut bytes, self.nodes.len() as u64);
        for (id, node) in &self.nodes {
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&node.id.0);
        }
        ProofId(*blake3::hash(&bytes).as_bytes())
    }

    /// Deterministic bytes for persistence and content-addressed envelopes.
    /// Unlike [`Self::content_hash`], which is intentionally a compact root
    /// index, this representation carries every node field so a store can
    /// reload and independently check the complete DAG.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, b"axiom/canonical-proof/v1");
        put_u64(&mut out, self.roots.len() as u64);
        for root in &self.roots {
            out.extend_from_slice(&root.0);
        }
        put_u64(&mut out, self.nodes.len() as u64);
        for (id, node) in &self.nodes {
            out.extend_from_slice(&id.0);
            out.extend_from_slice(&node.id.0);
            put_string(&mut out, node.statement.as_str());
            node.operation.encode_into(&mut out);
            put_u64(&mut out, node.inputs.len() as u64);
            for input in &node.inputs {
                out.extend_from_slice(&input.0);
            }
            put_u64(&mut out, node.metadata.len() as u64);
            for (key, value) in &node.metadata {
                put_string(&mut out, key);
                put_string(&mut out, value);
            }
        }
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckError {
    NonCanonicalRoots,
    MapKeyMismatch { expected: ProofId, actual: ProofId },
    TamperedNode { id: ProofId },
    MissingInput { node: ProofId, input: ProofId },
    MissingRoot { root: ProofId },
    MissingNode { id: ProofId },
    InvalidArithmetic { id: ProofId },
    InvalidOperation { id: ProofId },
    Cycle,
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonCanonicalRoots => f.write_str("proof roots are not canonical"),
            Self::MapKeyMismatch { expected, actual } => {
                write!(
                    f,
                    "proof map key {expected} does not match node id {actual}"
                )
            }
            Self::TamperedNode { id } => write!(f, "proof node {id} failed its content hash"),
            Self::MissingInput { node, input } => {
                write!(f, "proof node {node} refers to missing input {input}")
            }
            Self::MissingRoot { root } => write!(f, "proof root {root} is missing"),
            Self::MissingNode { id } => write!(f, "proof bundle does not contain proof node {id}"),
            Self::InvalidArithmetic { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid arithmetic certificate"
                )
            }
            Self::InvalidOperation { id } => {
                write!(f, "proof node {id} contains an invalid operation")
            }
            Self::Cycle => f.write_str("proof graph contains a cycle"),
        }
    }
}

impl std::error::Error for CheckError {}

fn hash_node(
    statement: &Statement,
    operation: &Operation,
    inputs: &[ProofId],
    metadata: &BTreeMap<String, String>,
) -> ProofId {
    let mut bytes = Vec::new();
    put_bytes(&mut bytes, b"axiom/proof-node/v1");
    put_string(&mut bytes, statement.as_str());
    operation.encode_into(&mut bytes);
    put_u64(&mut bytes, inputs.len() as u64);
    for input in inputs {
        bytes.extend_from_slice(&input.0);
    }
    // Source locations are diagnostic metadata, not semantic identity.  They
    // intentionally do not participate in the content address: inserting a
    // comment or moving a form must not create a new observation node.  All
    // stable dependency metadata remains covered by the hash.
    let hashed_metadata = metadata
        .iter()
        .filter(|(key, _)| key.as_str() != "location")
        .collect::<Vec<_>>();
    put_u64(&mut bytes, hashed_metadata.len() as u64);
    for (key, value) in hashed_metadata {
        put_string(&mut bytes, key);
        put_string(&mut bytes, value);
    }
    ProofId(*blake3::hash(&bytes).as_bytes())
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn node_hash_is_order_independent_for_inputs_and_metadata() {
        let a = Node::new(
            "a",
            Operation::Observation {
                source: "ledger:1".into(),
            },
            vec![],
            metadata(&[("z", "2"), ("a", "1")]),
        );
        let b = Node::new(
            "b",
            Operation::Derive { rule: "r".into() },
            vec![a.id],
            BTreeMap::new(),
        );
        let mut proof = Proof::new();
        proof.insert(b.clone());
        proof.insert(a.clone());
        proof.root(b.id);
        assert!(proof.check().is_ok());
        assert_eq!(b.inputs, vec![a.id]);
    }

    #[test]
    fn checker_rejects_tampering_missing_edges_and_cycles() {
        let a = Node::new(
            "a",
            Operation::Observation {
                source: "ledger:1".into(),
            },
            vec![],
            BTreeMap::new(),
        );
        let b = Node::new(
            "b",
            Operation::Derive { rule: "r".into() },
            vec![a.id],
            BTreeMap::new(),
        );
        let mut tampered = b.clone();
        tampered.statement = Statement::new("changed");
        let mut proof = Proof {
            nodes: BTreeMap::new(),
            roots: vec![b.id],
        };
        proof.nodes.insert(a.id, a);
        proof.nodes.insert(b.id, tampered);
        assert!(matches!(
            proof.check(),
            Err(CheckError::TamperedNode { .. })
        ));

        let mut missing = Proof::new();
        missing.insert(b.clone());
        missing.root(b.id);
        assert!(matches!(
            missing.check(),
            Err(CheckError::MissingInput { .. })
        ));

        let mut cyclic_node = b;
        cyclic_node.inputs = vec![cyclic_node.id];
        let mut cycle = Proof::new();
        cycle.nodes.insert(cyclic_node.id, cyclic_node);
        cycle.root(cycle.nodes.keys().next().copied().unwrap());
        assert!(matches!(
            cycle.check(),
            Err(CheckError::TamperedNode { .. })
        ));
    }

    #[test]
    fn checker_verifies_exact_arithmetic_certificates() {
        let node = Node::new(
            "gain",
            Operation::Arithmetic {
                rule: "gain = proceeds - basis".into(),
                minuend: Exact::from(500i64),
                subtrahend: Exact::from(201i64),
                result: Exact::from(298i64),
                unit: "USD".into(),
            },
            vec![],
            BTreeMap::new(),
        );
        let mut proof = Proof::new();
        let id = proof.insert(node);
        proof.root(id);
        assert!(matches!(
            proof.check(),
            Err(CheckError::InvalidArithmetic { .. })
        ));
    }

    #[test]
    fn diagnostic_locations_do_not_change_content_address() {
        let first = Node::new(
            "observed buy",
            Operation::Observation {
                source: "buy:canonical-material".into(),
            },
            vec![],
            metadata(&[("kind", "lot"), ("location", "line:2")]),
        );
        let second = Node::new(
            "observed buy",
            Operation::Observation {
                source: "buy:canonical-material".into(),
            },
            vec![],
            metadata(&[("kind", "lot"), ("location", "line:8")]),
        );
        assert_eq!(first.id, second.id);
        let second_id = second.id;
        let mut proof = Proof::new();
        proof.insert(first);
        proof.insert(second);
        proof.root(second_id);
        assert!(proof.check().is_ok());
    }
}
