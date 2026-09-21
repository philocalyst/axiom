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

/// Exact, self-contained certificate for one lot slice. Kept behind one box
/// in [`Operation`] so ordinary observation nodes stay compact.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LotAllocationCertificate {
    pub lot: String,
    pub sale: String,
    pub lot_proof: ProofId,
    pub sale_proof: ProofId,
    pub available: Exact,
    pub allocated: Exact,
    pub remaining: Exact,
    pub sale_quantity: Exact,
    pub sale_proceeds: Exact,
    pub available_basis: Exact,
    pub allocated_proceeds: Exact,
    pub allocated_basis: Exact,
    pub gain: Exact,
    pub quantity_unit: String,
    pub value_unit: String,
}

/// A deterministic derivation operation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Operation {
    /// A source ledger fact or other immutable observation.
    Observation { source: String },
    /// Source observation for one named acquisition lot.
    LotObservation { lot: String, source: String },
    /// Source observation for one named disposal.
    SaleObservation { sale: String, source: String },
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
    /// Exact allocation of part of a lot.  `available = allocated +
    /// remaining` is checked independently of the engine.  Keeping the
    /// quantities in the operation (rather than trusting metadata) makes the
    /// proposition self-contained for a proof reader.
    LotAllocation(Box<LotAllocationCertificate>),
    /// One exact step of remaining-inventory conservation.  `predecessor`
    /// points at the previous step, while `allocation` points at the one
    /// typed lot slice consumed by this step.  Both references are also
    /// required to occur in `inputs`; they are not metadata conventions.
    InventoryConservation {
        lot: String,
        lot_proof: ProofId,
        before: Exact,
        consumed: Exact,
        after: Exact,
        unit: String,
        predecessor: Option<ProofId>,
        allocation: Option<ProofId>,
    },
    /// Exact recognized aggregate.  The checker recomputes every total from
    /// direct [`LotAllocation`] inputs, so aggregate fields cannot be forged.
    Recognition {
        sale: String,
        sale_proof: ProofId,
        quantity: Exact,
        proceeds: Exact,
        basis: Exact,
        gain: Exact,
        quantity_unit: String,
        value_unit: String,
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
            Self::LotObservation { .. } => b"lot-observation",
            Self::SaleObservation { .. } => b"sale-observation",
            Self::Derive { .. } => b"derive",
            Self::Arithmetic { .. } => b"arithmetic",
            Self::LotAllocation(..) => b"lot-allocation",
            Self::InventoryConservation { .. } => b"inventory-conservation",
            Self::Recognition { .. } => b"recognition",
            Self::Decision { .. } => b"decision",
            Self::Policy { .. } => b"policy",
            Self::Conflict { .. } => b"conflict",
        }
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.tag());
        match self {
            Self::Observation { source } => put_string(out, source),
            Self::LotObservation { lot, source } => {
                put_string(out, lot);
                put_string(out, source);
            }
            Self::SaleObservation { sale, source } => {
                put_string(out, sale);
                put_string(out, source);
            }
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
            Self::LotAllocation(certificate) => {
                put_string(out, &certificate.lot);
                put_string(out, &certificate.sale);
                put_proof_id(out, &certificate.lot_proof);
                put_proof_id(out, &certificate.sale_proof);
                put_string(out, &certificate.available.canonical_string());
                put_string(out, &certificate.allocated.canonical_string());
                put_string(out, &certificate.remaining.canonical_string());
                put_string(out, &certificate.sale_quantity.canonical_string());
                put_string(out, &certificate.sale_proceeds.canonical_string());
                put_string(out, &certificate.available_basis.canonical_string());
                put_string(out, &certificate.allocated_proceeds.canonical_string());
                put_string(out, &certificate.allocated_basis.canonical_string());
                put_string(out, &certificate.gain.canonical_string());
                put_string(out, &certificate.quantity_unit);
                put_string(out, &certificate.value_unit);
            }
            Self::InventoryConservation {
                lot,
                lot_proof,
                before,
                consumed,
                after,
                unit,
                predecessor,
                allocation,
            } => {
                put_string(out, lot);
                put_proof_id(out, lot_proof);
                put_string(out, &before.canonical_string());
                put_string(out, &consumed.canonical_string());
                put_string(out, &after.canonical_string());
                put_string(out, unit);
                put_optional_proof_id(out, *predecessor);
                put_optional_proof_id(out, *allocation);
            }
            Self::Recognition {
                sale,
                sale_proof,
                quantity,
                proceeds,
                basis,
                gain,
                quantity_unit,
                value_unit,
            } => {
                put_string(out, sale);
                put_proof_id(out, sale_proof);
                put_string(out, &quantity.canonical_string());
                put_string(out, &proceeds.canonical_string());
                put_string(out, &basis.canonical_string());
                put_string(out, &gain.canonical_string());
                put_string(out, quantity_unit);
                put_string(out, value_unit);
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
            match &node.operation {
                Operation::LotAllocation(certificate) => {
                    if !valid_lot_allocation(certificate)
                        || !certificate_metadata_matches(
                            &node.metadata,
                            [
                                ("lot", certificate.lot.clone()),
                                ("sale", certificate.sale.clone()),
                                ("available", certificate.available.canonical_string()),
                                ("allocated", certificate.allocated.canonical_string()),
                                ("remaining", certificate.remaining.canonical_string()),
                                (
                                    "sale-quantity",
                                    certificate.sale_quantity.canonical_string(),
                                ),
                                (
                                    "sale-proceeds",
                                    certificate.sale_proceeds.canonical_string(),
                                ),
                                (
                                    "available-basis",
                                    certificate.available_basis.canonical_string(),
                                ),
                                (
                                    "allocated-proceeds",
                                    certificate.allocated_proceeds.canonical_string(),
                                ),
                                (
                                    "allocated-basis",
                                    certificate.allocated_basis.canonical_string(),
                                ),
                                ("gain", certificate.gain.canonical_string()),
                                ("quantity-unit", certificate.quantity_unit.clone()),
                                ("value-unit", certificate.value_unit.clone()),
                            ],
                        )
                    {
                        return Err(CheckError::InvalidLotAllocation { id: *id });
                    }
                }
                Operation::InventoryConservation {
                    lot,
                    before,
                    consumed,
                    after,
                    unit,
                    ..
                } => {
                    if !valid_inventory_conservation(lot, before, consumed, after, unit)
                        || !certificate_metadata_matches(
                            &node.metadata,
                            [
                                ("lot", lot.clone()),
                                ("before", before.canonical_string()),
                                ("consumed", consumed.canonical_string()),
                                ("after", after.canonical_string()),
                                ("unit", unit.clone()),
                            ],
                        )
                    {
                        return Err(CheckError::InvalidInventoryConservation { id: *id });
                    }
                }
                Operation::Recognition {
                    sale,
                    quantity,
                    proceeds,
                    basis,
                    gain,
                    quantity_unit,
                    value_unit,
                    ..
                } if !valid_recognition(
                    sale,
                    quantity,
                    proceeds,
                    basis,
                    gain,
                    quantity_unit,
                    value_unit,
                ) =>
                {
                    return Err(CheckError::InvalidRecognition { id: *id });
                }
                _ => {}
            }
            let invalid_operation = match &node.operation {
                Operation::Observation { source } => source.trim().is_empty(),
                Operation::LotObservation { lot, source } => {
                    lot.trim().is_empty() || source.trim().is_empty()
                }
                Operation::SaleObservation { sale, source } => {
                    sale.trim().is_empty() || source.trim().is_empty()
                }
                Operation::Derive { rule } => rule.trim().is_empty(),
                Operation::Arithmetic { rule, unit, .. } => {
                    rule.trim().is_empty() || unit.trim().is_empty()
                }
                Operation::LotAllocation(..)
                | Operation::InventoryConservation { .. }
                | Operation::Recognition { .. } => false,
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
        // Proposition-specific edges are checked after all nodes have passed
        // their local payload checks.  The typed pointers below prevent a
        // certificate from borrowing a same-shaped operation in another lot,
        // and reject reusing one allocation in two inventory histories.
        let mut allocation_uses: BTreeMap<ProofId, ProofId> = BTreeMap::new();
        let mut predecessor_uses: BTreeMap<ProofId, ProofId> = BTreeMap::new();
        for (id, node) in &self.nodes {
            match &node.operation {
                Operation::LotAllocation(certificate) => {
                    if !node.inputs.contains(&certificate.lot_proof)
                        || !node.inputs.contains(&certificate.sale_proof)
                        || !is_lot_observation(
                            self.nodes.get(&certificate.lot_proof),
                            &certificate.lot,
                        )
                        || !is_sale_observation(
                            self.nodes.get(&certificate.sale_proof),
                            &certificate.sale,
                        )
                    {
                        return Err(CheckError::InvalidLotAllocation { id: *id });
                    }
                }
                Operation::InventoryConservation {
                    lot,
                    lot_proof,
                    before,
                    consumed,
                    after,
                    unit,
                    predecessor,
                    allocation,
                } => {
                    if !node.inputs.contains(lot_proof)
                        || !is_lot_observation(self.nodes.get(lot_proof), lot)
                    {
                        return Err(CheckError::InvalidInventoryConservation { id: *id });
                    }
                    if let Some(previous_id) = predecessor {
                        if !node.inputs.contains(previous_id)
                            || predecessor_uses.insert(*previous_id, *id).is_some()
                        {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        }
                        let Some(previous_node) = self.nodes.get(previous_id) else {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        };
                        let Operation::InventoryConservation {
                            lot: previous_lot,
                            lot_proof: previous_lot_proof,
                            after: previous_after,
                            unit: previous_unit,
                            ..
                        } = &previous_node.operation
                        else {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        };
                        if previous_lot != lot
                            || previous_lot_proof != lot_proof
                            || previous_unit != unit
                            || previous_after != before
                        {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        }
                    } else if node.inputs.iter().any(|input| {
                        matches!(
                            self.nodes.get(input).map(|node| &node.operation),
                            Some(Operation::InventoryConservation { .. })
                        )
                    }) {
                        return Err(CheckError::InvalidInventoryConservation { id: *id });
                    }

                    if let Some(allocation_id) = allocation {
                        if !node.inputs.contains(allocation_id)
                            || allocation_uses.insert(*allocation_id, *id).is_some()
                        {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        }
                        let Some(allocation_node) = self.nodes.get(allocation_id) else {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        };
                        let Operation::LotAllocation(certificate) = &allocation_node.operation
                        else {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        };
                        if &certificate.lot != lot
                            || &certificate.lot_proof != lot_proof
                            || &certificate.quantity_unit != unit
                            || &certificate.available != before
                            || &certificate.allocated != consumed
                            || &certificate.remaining != after
                        {
                            return Err(CheckError::InvalidInventoryConservation { id: *id });
                        }
                    } else if !consumed.is_zero() {
                        return Err(CheckError::InvalidInventoryConservation { id: *id });
                    }
                }
                Operation::Recognition {
                    sale,
                    sale_proof,
                    quantity,
                    proceeds,
                    basis,
                    gain,
                    quantity_unit,
                    value_unit,
                } => {
                    if !node.inputs.contains(sale_proof)
                        || !is_sale_observation(self.nodes.get(sale_proof), sale)
                    {
                        return Err(CheckError::InvalidRecognition { id: *id });
                    }
                    let mut total_quantity = Exact::from(0i64);
                    let mut total_proceeds = Exact::from(0i64);
                    let mut total_basis = Exact::from(0i64);
                    let mut total_gain = Exact::from(0i64);
                    let mut saw_allocation = false;
                    for input in &node.inputs {
                        let input_node = self.nodes.get(input).expect("inputs checked above");
                        let Operation::LotAllocation(certificate) = &input_node.operation else {
                            continue;
                        };
                        saw_allocation = true;
                        if &certificate.sale != sale
                            || &certificate.sale_proof != sale_proof
                            || &certificate.quantity_unit != quantity_unit
                            || &certificate.value_unit != value_unit
                        {
                            return Err(CheckError::InvalidRecognition { id: *id });
                        }
                        total_quantity = total_quantity.checked_add(&certificate.allocated);
                        total_proceeds =
                            total_proceeds.checked_add(&certificate.allocated_proceeds);
                        total_basis = total_basis.checked_add(&certificate.allocated_basis);
                        total_gain = total_gain.checked_add(&certificate.gain);
                    }
                    if !saw_allocation
                        || total_quantity != *quantity
                        || total_proceeds != *proceeds
                        || total_basis != *basis
                        || total_gain != *gain
                    {
                        return Err(CheckError::InvalidRecognition { id: *id });
                    }
                }
                _ => {}
            }
        }
        for root in &self.roots {
            if !self.nodes.contains_key(root) {
                return Err(CheckError::MissingRoot { root: *root });
            }
        }
        let mut reachable = BTreeSet::new();
        let mut pending = self.roots.clone();
        while let Some(id) = pending.pop() {
            if reachable.insert(id) {
                pending.extend(
                    self.nodes
                        .get(&id)
                        .expect("roots and inputs were checked")
                        .inputs
                        .iter()
                        .copied(),
                );
            }
        }
        for (id, node) in &self.nodes {
            if matches!(
                node.operation,
                Operation::LotAllocation(..)
                    | Operation::InventoryConservation { .. }
                    | Operation::Recognition { .. }
            ) && !reachable.contains(id)
            {
                return Err(CheckError::UnreachableCertificate { id: *id });
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
    InvalidLotAllocation { id: ProofId },
    InvalidInventoryConservation { id: ProofId },
    InvalidRecognition { id: ProofId },
    UnreachableCertificate { id: ProofId },
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
            Self::InvalidLotAllocation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid lot allocation certificate"
                )
            }
            Self::InvalidInventoryConservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid inventory conservation certificate"
                )
            }
            Self::InvalidRecognition { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid recognition certificate"
                )
            }
            Self::UnreachableCertificate { id } => {
                write!(f, "proof certificate {id} is not reachable from any root")
            }
            Self::InvalidOperation { id } => {
                write!(f, "proof node {id} contains an invalid operation")
            }
            Self::Cycle => f.write_str("proof graph contains a cycle"),
        }
    }
}

impl std::error::Error for CheckError {}

fn valid_lot_allocation(certificate: &LotAllocationCertificate) -> bool {
    let Some(sale_ratio) = certificate
        .allocated
        .checked_div(&certificate.sale_quantity)
        .ok()
    else {
        return false;
    };
    let Some(inventory_ratio) = certificate
        .allocated
        .checked_div(&certificate.available)
        .ok()
    else {
        return false;
    };
    !certificate.lot.trim().is_empty()
        && !certificate.sale.trim().is_empty()
        && !certificate.quantity_unit.trim().is_empty()
        && !certificate.value_unit.trim().is_empty()
        && !certificate.available.is_negative()
        && !certificate.allocated.is_negative()
        && !certificate.remaining.is_negative()
        && !certificate.sale_quantity.is_negative()
        && !certificate.sale_quantity.is_zero()
        && !certificate.sale_proceeds.is_negative()
        && !certificate.available_basis.is_negative()
        && !certificate.allocated_proceeds.is_negative()
        && !certificate.allocated_basis.is_negative()
        // A zero-sized allocation is not an allocation.  This catches a
        // common forged certificate in which no inventory was actually used.
        && !certificate.allocated.is_zero()
        && certificate.allocated <= certificate.available
        && certificate.allocated <= certificate.sale_quantity
        && certificate.available.checked_sub(&certificate.allocated) == certificate.remaining
        && certificate.sale_proceeds.checked_mul(&sale_ratio) == certificate.allocated_proceeds
        && certificate.available_basis.checked_mul(&inventory_ratio)
            == certificate.allocated_basis
        && certificate
            .allocated_proceeds
            .checked_sub(&certificate.allocated_basis)
            == certificate.gain
}

fn valid_inventory_conservation(
    lot: &str,
    before: &Exact,
    consumed: &Exact,
    after: &Exact,
    unit: &str,
) -> bool {
    !lot.trim().is_empty()
        && !unit.trim().is_empty()
        && !before.is_negative()
        && !consumed.is_negative()
        && !after.is_negative()
        && before.checked_sub(consumed) == *after
}

fn valid_recognition(
    sale: &str,
    quantity: &Exact,
    proceeds: &Exact,
    basis: &Exact,
    gain: &Exact,
    quantity_unit: &str,
    value_unit: &str,
) -> bool {
    !sale.trim().is_empty()
        && !quantity_unit.trim().is_empty()
        && !value_unit.trim().is_empty()
        && !quantity.is_negative()
        && !proceeds.is_negative()
        && !basis.is_negative()
        && proceeds.checked_sub(basis) == *gain
}

fn is_lot_observation(node: Option<&Node>, lot: &str) -> bool {
    matches!(
        node.map(|node| &node.operation),
        Some(Operation::LotObservation { lot: observed, .. }) if observed == lot
    )
}

fn is_sale_observation(node: Option<&Node>, sale: &str) -> bool {
    matches!(
        node.map(|node| &node.operation),
        Some(Operation::SaleObservation { sale: observed, .. }) if observed == sale
    )
}

/// Metadata remains diagnostic and extensible, but fields that duplicate a
/// proposition's typed payload must agree when present.  In particular this
/// prevents a caller from re-hashing a node with a forged `lot` or amount in
/// metadata and presenting it as an explanation for a different proposition.
fn certificate_metadata_matches<I>(metadata: &BTreeMap<String, String>, expected: I) -> bool
where
    I: IntoIterator<Item = (&'static str, String)>,
{
    expected.into_iter().all(|(key, value)| {
        metadata
            .get(key)
            .map(|actual| actual == &value)
            .unwrap_or(true)
    })
}

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

fn put_proof_id(out: &mut Vec<u8>, value: &ProofId) {
    out.extend_from_slice(&value.0);
}

fn put_optional_proof_id(out: &mut Vec<u8>, value: Option<ProofId>) {
    match value {
        Some(value) => {
            out.push(1);
            put_proof_id(out, &value);
        }
        None => out.push(0),
    }
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

    fn allocation_fixture(allocation_remaining: i64, conservation_consumed: i64) -> Proof {
        let lot = Node::new(
            "lot/one",
            Operation::LotObservation {
                lot: "lot/one".into(),
                source: "buy lot/one".into(),
            },
            vec![],
            BTreeMap::new(),
        );
        let sale = Node::new(
            "sale/one",
            Operation::SaleObservation {
                sale: "sale/one".into(),
                source: "sell sale/one".into(),
            },
            vec![],
            BTreeMap::new(),
        );
        let allocation = Node::new(
            "allocate 3 ABC from lot/one",
            Operation::LotAllocation(Box::new(LotAllocationCertificate {
                lot: "lot/one".into(),
                sale: "sale/one".into(),
                lot_proof: lot.id,
                sale_proof: sale.id,
                available: Exact::from(10i64),
                allocated: Exact::from(3i64),
                remaining: Exact::from(allocation_remaining),
                sale_quantity: Exact::from(5i64),
                sale_proceeds: Exact::from(50i64),
                available_basis: Exact::from(40i64),
                allocated_proceeds: Exact::from(30i64),
                allocated_basis: Exact::from(12i64),
                gain: Exact::from(18i64),
                quantity_unit: "ABC".into(),
                value_unit: "USD".into(),
            })),
            vec![lot.id, sale.id],
            BTreeMap::new(),
        );
        let conservation = Node::new(
            "remaining ABC in lot/one",
            Operation::InventoryConservation {
                lot: "lot/one".into(),
                lot_proof: lot.id,
                before: Exact::from(10i64),
                consumed: Exact::from(conservation_consumed),
                after: Exact::from(10i64).checked_sub(&Exact::from(conservation_consumed)),
                unit: "ABC".into(),
                predecessor: None,
                allocation: Some(allocation.id),
            },
            vec![lot.id, allocation.id],
            BTreeMap::new(),
        );
        let recognition = Node::new(
            "recognize sale/one",
            Operation::Recognition {
                sale: "sale/one".into(),
                sale_proof: sale.id,
                quantity: Exact::from(3i64),
                proceeds: Exact::from(30i64),
                basis: Exact::from(12i64),
                gain: Exact::from(18i64),
                quantity_unit: "ABC".into(),
                value_unit: "USD".into(),
            },
            vec![sale.id, allocation.id],
            BTreeMap::new(),
        );
        let mut proof = Proof::new();
        proof.insert(lot);
        proof.insert(sale);
        proof.insert(allocation);
        let conservation = proof.insert(conservation);
        let recognition = proof.insert(recognition);
        proof.root(conservation);
        proof.root(recognition);
        proof
    }

    #[test]
    fn checker_verifies_typed_allocation_conservation_and_recognition() {
        assert!(allocation_fixture(7, 3).check().is_ok());
    }

    #[test]
    fn checker_rejects_rehashed_forged_certificates() {
        assert!(matches!(
            allocation_fixture(8, 3).check(),
            Err(CheckError::InvalidLotAllocation { .. })
        ));
        assert!(matches!(
            allocation_fixture(7, 4).check(),
            Err(CheckError::InvalidInventoryConservation { .. })
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
