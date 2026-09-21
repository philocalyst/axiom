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

/// Typed source observation for a quote answer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct QuoteObservationCertificate {
    pub quote: String,
    pub date: String,
    pub base: Exact,
    pub base_unit: String,
    pub quote_amount: Exact,
    pub quote_unit: String,
}

/// Typed source observation for a position answer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PositionObservationCertificate {
    pub account: String,
    pub quantity: Exact,
    pub unit: String,
}

/// One independently checked position reconciliation result.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PositionReconciliationCertificate {
    pub account: String,
    pub source_proof: ProofId,
    pub observed: Exact,
    pub calculated: Exact,
    pub result: Exact,
    pub unit: String,
    /// `reconciled` or `conflict`.
    pub status: String,
}

/// Typed source observation for a cash settlement used by a journal.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CashSettlementObservationCertificate {
    pub reference: String,
    pub amount: Exact,
    pub unit: String,
    pub into: Option<String>,
}

/// Proposition-specific public answer for a sale whose recognition is
/// blocked.  The source edge is deliberately typed: a blocked answer cannot
/// borrow a same-shaped source from another sale, and its economic identity
/// is carried in exact fields rather than in statement text or metadata.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BlockedSaleCertificate {
    pub sale: String,
    pub source_proof: ProofId,
    pub quantity: Exact,
    pub proceeds: Exact,
    pub quantity_unit: String,
    pub value_unit: String,
    pub account: String,
    pub asset: String,
    /// Canonical blocker reason, such as `ambiguous-lot:a,b` or
    /// `policy-decision-conflict:a:b`.
    pub reason: String,
}

/// Independent comparison of one cash-settlement observation with the
/// proceeds of its authored sale.  A reconciliation is deliberately a
/// concrete certificate rather than a generic `Derive`: both source leaves,
/// both exact amounts, and the resulting status are checked by the proof
/// reader without consulting the engine.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettlementReconciliationCertificate {
    pub settlement: String,
    pub source_proof: ProofId,
    pub sale: String,
    pub sale_proof: ProofId,
    pub observed: Exact,
    pub expected: Exact,
    pub unit: String,
    /// `reconciled` when observed == expected, otherwise `conflict`.
    pub status: String,
}

/// One exact journal line.  Journal lines are deliberately represented in
/// the proof layer rather than reconstructed from display metadata.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JournalLineCertificate {
    pub side: String,
    pub account: String,
    pub amount: Exact,
    pub unit: String,
}

/// A journal projection tied to one recognition and one cash settlement.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JournalEntryCertificate {
    pub sale: String,
    pub recognition_proof: ProofId,
    pub settlement_proof: ProofId,
    pub inventory_account: String,
    pub asset: String,
    pub lines: Vec<JournalLineCertificate>,
}

/// One ordered state observation in a settlement history.
///
/// This deliberately uses canonical text for the state and date.  The proof
/// module must remain independent from the engine and ontology crates; the
/// engine-facing boundary can serialize its typed state/date values here.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettlementTransition {
    pub state: String,
    pub at: Option<String>,
}

/// Typed source observation for an obligation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObligationObservationCertificate {
    pub obligation: String,
    pub debtor: String,
    pub creditor: String,
    pub promised: Exact,
    pub unit: String,
    pub due: Option<String>,
}

/// Typed source observation for a settlement and its authoritative history.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettlementObservationCertificate {
    pub settlement: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub instrument: String,
    pub amount: Exact,
    pub unit: String,
    pub history: Vec<SettlementTransition>,
}

/// Typed source observation for a satisfaction allocation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SatisfactionObservationCertificate {
    pub satisfaction: String,
    pub obligation: String,
    pub settlement: String,
    pub amount: Exact,
    pub unit: String,
    /// Expected canonical spelling is `proposed`, `applied`, or `reversed`.
    /// The checker accepts case variants at this boundary for ergonomic
    /// engine adapters, while the value remains content-addressed as given.
    pub state: String,
}

/// A self-contained certificate for a settlement's ordered history and its
/// current effectiveness.  `settlement_proof` must point at the matching
/// [`Operation::SettlementObservation`] source leaf.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettlementHistoryCertificate {
    pub settlement: String,
    pub settlement_proof: ProofId,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub instrument: String,
    pub amount: Exact,
    pub unit: String,
    pub history: Vec<SettlementTransition>,
    pub current: String,
    pub effective: bool,
}

/// One typed satisfaction allocation.  The three proof IDs intentionally
/// remain in the payload as well as in `Node::inputs`: this prevents a
/// certificate from borrowing a same-shaped source belonging to a different
/// obligation or settlement.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SatisfactionAllocationCertificate {
    pub satisfaction: String,
    pub satisfaction_proof: ProofId,
    pub obligation: String,
    pub obligation_proof: ProofId,
    pub settlement: String,
    pub settlement_proof: ProofId,
    pub amount: Exact,
    pub unit: String,
    /// Expected canonical spelling is `proposed`, `applied`, or `reversed`.
    pub state: String,
}

/// Conservation certificate for one obligation.  The listed allocation IDs
/// must be exactly all effective applied allocations for this obligation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObligationBalanceCertificate {
    pub obligation: String,
    pub obligation_proof: ProofId,
    pub promised: Exact,
    pub allocated: Exact,
    pub remaining: Exact,
    pub unit: String,
    pub allocations: Vec<ProofId>,
}

/// Conservation certificate for one settlement.  The listed allocation IDs
/// must be exactly all effective applied allocations for this settlement.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SettlementBalanceCertificate {
    pub settlement: String,
    pub settlement_proof: ProofId,
    pub amount: Exact,
    pub allocated: Exact,
    pub unused: Exact,
    pub unit: String,
    pub allocations: Vec<ProofId>,
}

/// Short aliases kept for engine adapters that name the source leaves by
/// their domain object rather than by their proof role.
pub type ObligationCertificate = ObligationObservationCertificate;
pub type SettlementCertificate = SettlementObservationCertificate;
pub type SatisfactionCertificate = SatisfactionObservationCertificate;
pub type SettlementTransitionCertificate = SettlementTransition;

/// A deterministic derivation operation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Operation {
    /// A source ledger fact or other immutable observation.
    Observation { source: String },
    /// Typed source observation for a quote.
    QuoteObservation(QuoteObservationCertificate),
    /// Typed source observation for an observed position.
    PositionObservation(PositionObservationCertificate),
    /// Typed source observation for an observed cash settlement.
    CashSettlementObservation(CashSettlementObservationCertificate),
    /// Proposition-specific public answer for a blocked sale.
    BlockedSale(BlockedSaleCertificate),
    /// Exact comparison between a cash-settlement observation and sale
    /// proceeds, with both typed source proofs retained in the payload.
    SettlementReconciliation(SettlementReconciliationCertificate),
    /// Source observation for one named acquisition lot.
    LotObservation {
        lot: String,
        source: String,
        account: String,
        asset: String,
        quantity: Exact,
    },
    /// Source observation for one named disposal.
    SaleObservation {
        sale: String,
        source: String,
        account: String,
        asset: String,
        quantity: Exact,
        proceeds: Exact,
        quantity_unit: String,
        value_unit: String,
    },
    /// Typed source observation for one obligation.
    ObligationObservation(ObligationObservationCertificate),
    /// Typed source observation for one settlement and its ordered history.
    SettlementObservation(SettlementObservationCertificate),
    /// Typed source observation for one satisfaction allocation.
    SatisfactionObservation(SatisfactionObservationCertificate),
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
        account: String,
        asset: String,
        quantity: Exact,
        proceeds: Exact,
        basis: Exact,
        gain: Exact,
        quantity_unit: String,
        value_unit: String,
    },
    /// Independently checked ordered settlement history and effectiveness.
    SettlementHistory(Box<SettlementHistoryCertificate>),
    /// Independently checked identity and endpoint/unit links for one
    /// satisfaction allocation.
    SatisfactionAllocation(Box<SatisfactionAllocationCertificate>),
    /// `promised = effective allocated + remaining` for one obligation.
    ObligationBalance(Box<ObligationBalanceCertificate>),
    /// `amount = effective allocated + unused` for one settlement.
    SettlementBalance(Box<SettlementBalanceCertificate>),
    /// Independently checked position result, including its exact observed
    /// and calculated values.
    PositionReconciliation(Box<PositionReconciliationCertificate>),
    /// Independently checked journal projection and exact lines.
    JournalEntry(Box<JournalEntryCertificate>),
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
            Self::QuoteObservation(..) => b"quote-observation",
            Self::PositionObservation(..) => b"position-observation",
            Self::CashSettlementObservation(..) => b"cash-settlement-observation",
            Self::BlockedSale(..) => b"blocked-sale",
            Self::SettlementReconciliation(..) => b"settlement-reconciliation",
            Self::LotObservation { .. } => b"lot-observation",
            Self::SaleObservation { .. } => b"sale-observation",
            Self::ObligationObservation(..) => b"obligation-observation",
            Self::SettlementObservation(..) => b"settlement-observation",
            Self::SatisfactionObservation(..) => b"satisfaction-observation",
            Self::Derive { .. } => b"derive",
            Self::Arithmetic { .. } => b"arithmetic",
            Self::LotAllocation(..) => b"lot-allocation",
            Self::InventoryConservation { .. } => b"inventory-conservation",
            Self::Recognition { .. } => b"recognition",
            Self::SettlementHistory(..) => b"settlement-history",
            Self::SatisfactionAllocation(..) => b"satisfaction-allocation",
            Self::ObligationBalance(..) => b"obligation-balance",
            Self::SettlementBalance(..) => b"settlement-balance",
            Self::PositionReconciliation(..) => b"position-reconciliation",
            Self::JournalEntry(..) => b"journal-entry",
            Self::Decision { .. } => b"decision",
            Self::Policy { .. } => b"policy",
            Self::Conflict { .. } => b"conflict",
        }
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.tag());
        match self {
            Self::Observation { source } => put_string(out, source),
            Self::QuoteObservation(certificate) => {
                put_string(out, &certificate.quote);
                put_string(out, &certificate.date);
                put_string(out, &certificate.base.canonical_string());
                put_string(out, &certificate.base_unit);
                put_string(out, &certificate.quote_amount.canonical_string());
                put_string(out, &certificate.quote_unit);
            }
            Self::PositionObservation(certificate) => {
                put_string(out, &certificate.account);
                put_string(out, &certificate.quantity.canonical_string());
                put_string(out, &certificate.unit);
            }
            Self::CashSettlementObservation(certificate) => {
                put_string(out, &certificate.reference);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.unit);
                put_optional_string(out, certificate.into.as_deref());
            }
            Self::BlockedSale(certificate) => {
                put_string(out, &certificate.sale);
                put_proof_id(out, &certificate.source_proof);
                put_string(out, &certificate.quantity.canonical_string());
                put_string(out, &certificate.proceeds.canonical_string());
                put_string(out, &certificate.quantity_unit);
                put_string(out, &certificate.value_unit);
                put_string(out, &certificate.account);
                put_string(out, &certificate.asset);
                put_string(out, &certificate.reason);
            }
            Self::SettlementReconciliation(certificate) => {
                put_string(out, &certificate.settlement);
                put_proof_id(out, &certificate.source_proof);
                put_string(out, &certificate.sale);
                put_proof_id(out, &certificate.sale_proof);
                put_string(out, &certificate.observed.canonical_string());
                put_string(out, &certificate.expected.canonical_string());
                put_string(out, &certificate.unit);
                put_string(out, &certificate.status);
            }
            Self::LotObservation {
                lot,
                source,
                account,
                asset,
                quantity,
            } => {
                put_string(out, lot);
                put_string(out, source);
                put_string(out, account);
                put_string(out, asset);
                put_string(out, &quantity.canonical_string());
            }
            Self::SaleObservation {
                sale,
                source,
                account,
                asset,
                quantity,
                proceeds,
                quantity_unit,
                value_unit,
            } => {
                put_string(out, sale);
                put_string(out, source);
                put_string(out, account);
                put_string(out, asset);
                put_string(out, &quantity.canonical_string());
                put_string(out, &proceeds.canonical_string());
                put_string(out, quantity_unit);
                put_string(out, value_unit);
            }
            Self::ObligationObservation(certificate) => {
                put_string(out, &certificate.obligation);
                put_string(out, &certificate.debtor);
                put_string(out, &certificate.creditor);
                put_string(out, &certificate.promised.canonical_string());
                put_string(out, &certificate.unit);
                put_optional_string(out, certificate.due.as_deref());
            }
            Self::SettlementObservation(certificate) => {
                put_string(out, &certificate.settlement);
                put_string(out, &certificate.kind);
                put_string(out, &certificate.from);
                put_string(out, &certificate.to);
                put_string(out, &certificate.instrument);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.unit);
                put_transitions(out, &certificate.history);
            }
            Self::SatisfactionObservation(certificate) => {
                put_string(out, &certificate.satisfaction);
                put_string(out, &certificate.obligation);
                put_string(out, &certificate.settlement);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.unit);
                put_string(out, &certificate.state);
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
                account,
                asset,
                quantity,
                proceeds,
                basis,
                gain,
                quantity_unit,
                value_unit,
            } => {
                put_string(out, sale);
                put_proof_id(out, sale_proof);
                put_string(out, account);
                put_string(out, asset);
                put_string(out, &quantity.canonical_string());
                put_string(out, &proceeds.canonical_string());
                put_string(out, &basis.canonical_string());
                put_string(out, &gain.canonical_string());
                put_string(out, quantity_unit);
                put_string(out, value_unit);
            }
            Self::SettlementHistory(certificate) => {
                put_string(out, &certificate.settlement);
                put_proof_id(out, &certificate.settlement_proof);
                put_string(out, &certificate.kind);
                put_string(out, &certificate.from);
                put_string(out, &certificate.to);
                put_string(out, &certificate.instrument);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.unit);
                put_transitions(out, &certificate.history);
                put_string(out, &certificate.current);
                out.push(u8::from(certificate.effective));
            }
            Self::SatisfactionAllocation(certificate) => {
                put_string(out, &certificate.satisfaction);
                put_proof_id(out, &certificate.satisfaction_proof);
                put_string(out, &certificate.obligation);
                put_proof_id(out, &certificate.obligation_proof);
                put_string(out, &certificate.settlement);
                put_proof_id(out, &certificate.settlement_proof);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.unit);
                put_string(out, &certificate.state);
            }
            Self::ObligationBalance(certificate) => {
                put_string(out, &certificate.obligation);
                put_proof_id(out, &certificate.obligation_proof);
                put_string(out, &certificate.promised.canonical_string());
                put_string(out, &certificate.allocated.canonical_string());
                put_string(out, &certificate.remaining.canonical_string());
                put_string(out, &certificate.unit);
                put_proof_ids(out, &certificate.allocations);
            }
            Self::SettlementBalance(certificate) => {
                put_string(out, &certificate.settlement);
                put_proof_id(out, &certificate.settlement_proof);
                put_string(out, &certificate.amount.canonical_string());
                put_string(out, &certificate.allocated.canonical_string());
                put_string(out, &certificate.unused.canonical_string());
                put_string(out, &certificate.unit);
                put_proof_ids(out, &certificate.allocations);
            }
            Self::PositionReconciliation(certificate) => {
                put_string(out, &certificate.account);
                put_proof_id(out, &certificate.source_proof);
                put_string(out, &certificate.observed.canonical_string());
                put_string(out, &certificate.calculated.canonical_string());
                put_string(out, &certificate.result.canonical_string());
                put_string(out, &certificate.unit);
                put_string(out, &certificate.status);
            }
            Self::JournalEntry(certificate) => {
                put_string(out, &certificate.sale);
                put_proof_id(out, &certificate.recognition_proof);
                put_proof_id(out, &certificate.settlement_proof);
                put_string(out, &certificate.inventory_account);
                put_string(out, &certificate.asset);
                put_journal_lines(out, &certificate.lines);
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
        if let Err(index) = self.roots.binary_search(&id) {
            self.roots.insert(index, id);
        }
    }

    /// Add a batch of roots with one canonicalization pass. Large analyses
    /// must not repeatedly shift and sort the root vector.
    pub fn root_all(&mut self, roots: impl IntoIterator<Item = ProofId>) {
        self.roots.extend(roots);
        self.roots.sort_unstable();
        self.roots.dedup();
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
                Operation::QuoteObservation(certificate)
                    if !valid_quote_observation(certificate) =>
                {
                    return Err(CheckError::InvalidQuoteObservation { id: *id });
                }
                Operation::PositionObservation(certificate)
                    if !valid_position_observation(certificate) =>
                {
                    return Err(CheckError::InvalidPositionObservation { id: *id });
                }
                Operation::CashSettlementObservation(certificate)
                    if !valid_cash_settlement_observation(certificate) =>
                {
                    return Err(CheckError::InvalidCashSettlementObservation { id: *id });
                }
                Operation::BlockedSale(certificate) if !valid_blocked_sale(certificate) => {
                    return Err(CheckError::InvalidBlockedSale { id: *id });
                }
                Operation::SettlementReconciliation(certificate)
                    if !valid_settlement_reconciliation(certificate) =>
                {
                    return Err(CheckError::InvalidSettlementReconciliation { id: *id });
                }
                Operation::ObligationObservation(certificate)
                    if !valid_obligation_observation(certificate) =>
                {
                    return Err(CheckError::InvalidObligationObservation { id: *id });
                }
                Operation::SettlementObservation(certificate)
                    if !valid_settlement_observation(certificate) =>
                {
                    return Err(CheckError::InvalidSettlementObservation { id: *id });
                }
                Operation::SatisfactionObservation(certificate)
                    if !valid_satisfaction_observation(certificate) =>
                {
                    return Err(CheckError::InvalidSatisfactionObservation { id: *id });
                }
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
                Operation::SettlementHistory(certificate)
                    if !valid_settlement_history_certificate(certificate) =>
                {
                    return Err(CheckError::InvalidSettlementHistory { id: *id });
                }
                Operation::SatisfactionAllocation(certificate)
                    if !valid_satisfaction_allocation(certificate) =>
                {
                    return Err(CheckError::InvalidSatisfactionAllocation { id: *id });
                }
                Operation::ObligationBalance(certificate)
                    if !valid_obligation_balance(certificate) =>
                {
                    return Err(CheckError::InvalidObligationBalance { id: *id });
                }
                Operation::SettlementBalance(certificate)
                    if !valid_settlement_balance(certificate) =>
                {
                    return Err(CheckError::InvalidSettlementBalance { id: *id });
                }
                Operation::PositionReconciliation(certificate)
                    if !valid_position_reconciliation(certificate) =>
                {
                    return Err(CheckError::InvalidPositionReconciliation { id: *id });
                }
                Operation::JournalEntry(certificate) if !valid_journal_entry(certificate) => {
                    return Err(CheckError::InvalidJournalEntry { id: *id });
                }
                _ => {}
            }
            let invalid_operation = match &node.operation {
                Operation::Observation { source } => source.trim().is_empty(),
                Operation::QuoteObservation(..)
                | Operation::PositionObservation(..)
                | Operation::CashSettlementObservation(..)
                | Operation::BlockedSale(..) => false,
                Operation::SettlementReconciliation(..) => false,
                Operation::LotObservation {
                    lot,
                    source,
                    account,
                    asset,
                    quantity,
                } => {
                    lot.trim().is_empty()
                        || source.trim().is_empty()
                        || account.trim().is_empty()
                        || asset.trim().is_empty()
                        || quantity.is_negative()
                        || quantity.is_zero()
                }
                Operation::SaleObservation {
                    sale,
                    source,
                    account,
                    asset,
                    quantity_unit,
                    value_unit,
                    ..
                } => {
                    sale.trim().is_empty()
                        || source.trim().is_empty()
                        || account.trim().is_empty()
                        || asset.trim().is_empty()
                        || quantity_unit.trim().is_empty()
                        || value_unit.trim().is_empty()
                }
                Operation::ObligationObservation(certificate) => {
                    certificate.obligation.trim().is_empty()
                        || certificate.debtor.trim().is_empty()
                        || certificate.creditor.trim().is_empty()
                        || certificate.unit.trim().is_empty()
                }
                Operation::SettlementObservation(certificate) => {
                    certificate.settlement.trim().is_empty()
                        || certificate.from.trim().is_empty()
                        || certificate.to.trim().is_empty()
                        || certificate.instrument.trim().is_empty()
                        || certificate.unit.trim().is_empty()
                }
                Operation::SatisfactionObservation(certificate) => {
                    certificate.satisfaction.trim().is_empty()
                        || certificate.obligation.trim().is_empty()
                        || certificate.settlement.trim().is_empty()
                        || certificate.unit.trim().is_empty()
                        || certificate.state.trim().is_empty()
                }
                Operation::Derive { rule } => rule.trim().is_empty(),
                Operation::Arithmetic { rule, unit, .. } => {
                    rule.trim().is_empty() || unit.trim().is_empty()
                }
                Operation::LotAllocation(..)
                | Operation::InventoryConservation { .. }
                | Operation::Recognition { .. }
                | Operation::SettlementHistory(..)
                | Operation::SatisfactionAllocation(..)
                | Operation::ObligationBalance(..)
                | Operation::SettlementBalance(..) => false,
                Operation::PositionReconciliation(..) | Operation::JournalEntry(..) => false,
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
        let mut obligation_balance_uses: BTreeMap<ProofId, ProofId> = BTreeMap::new();
        let mut settlement_balance_uses: BTreeMap<ProofId, ProofId> = BTreeMap::new();
        let mut obligation_sources: BTreeMap<String, ProofId> = BTreeMap::new();
        let mut settlement_sources: BTreeMap<String, ProofId> = BTreeMap::new();
        let mut satisfaction_sources: BTreeMap<String, ProofId> = BTreeMap::new();
        let mut satisfaction_certificates: BTreeMap<String, ProofId> = BTreeMap::new();
        let effective_allocations = effective_allocation_index(self);
        for (id, node) in &self.nodes {
            match &node.operation {
                Operation::BlockedSale(certificate) => {
                    if !node.inputs.contains(&certificate.source_proof)
                        || !blocked_sale_matches_source(
                            self.nodes.get(&certificate.source_proof),
                            certificate,
                        )
                    {
                        return Err(CheckError::InvalidBlockedSale { id: *id });
                    }
                }
                Operation::SettlementReconciliation(certificate) => {
                    if !node.inputs.contains(&certificate.source_proof)
                        || !node.inputs.contains(&certificate.sale_proof)
                        || !settlement_reconciliation_matches_sources(self, certificate)
                    {
                        return Err(CheckError::InvalidSettlementReconciliation { id: *id });
                    }
                }
                Operation::PositionReconciliation(certificate) => {
                    if !node.inputs.contains(&certificate.source_proof)
                        || !position_observation_matches(
                            self.nodes.get(&certificate.source_proof),
                            certificate,
                        )
                        || !position_reconciliation_matches(self, node, certificate)
                    {
                        return Err(CheckError::InvalidPositionReconciliation { id: *id });
                    }
                }
                Operation::JournalEntry(certificate) => {
                    if !node.inputs.contains(&certificate.recognition_proof)
                        || !node.inputs.contains(&certificate.settlement_proof)
                        || !journal_entry_matches_sources(self, certificate)
                    {
                        return Err(CheckError::InvalidJournalEntry { id: *id });
                    }
                }
                Operation::ObligationObservation(certificate) => {
                    if obligation_sources
                        .insert(certificate.obligation.clone(), *id)
                        .is_some()
                    {
                        return Err(CheckError::DuplicateObligationObservation { id: *id });
                    }
                }
                Operation::SettlementObservation(certificate) => {
                    if settlement_sources
                        .insert(certificate.settlement.clone(), *id)
                        .is_some()
                    {
                        return Err(CheckError::DuplicateSettlementObservation { id: *id });
                    }
                }
                Operation::SatisfactionObservation(certificate) => {
                    if satisfaction_sources
                        .insert(certificate.satisfaction.clone(), *id)
                        .is_some()
                    {
                        return Err(CheckError::DuplicateSatisfactionObservation { id: *id });
                    }
                }
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
                Operation::SettlementHistory(certificate) => {
                    if !node.inputs.contains(&certificate.settlement_proof)
                        || !settlement_history_matches_source(
                            self.nodes.get(&certificate.settlement_proof),
                            certificate,
                        )
                    {
                        return Err(CheckError::InvalidSettlementHistory { id: *id });
                    }
                }
                Operation::SatisfactionAllocation(certificate) => {
                    if satisfaction_certificates
                        .insert(certificate.satisfaction.clone(), *id)
                        .is_some()
                        || !node.inputs.contains(&certificate.satisfaction_proof)
                        || !node.inputs.contains(&certificate.obligation_proof)
                        || !node.inputs.contains(&certificate.settlement_proof)
                        || !satisfaction_allocation_matches_sources(
                            self.nodes.get(&certificate.satisfaction_proof),
                            self.nodes.get(&certificate.obligation_proof),
                            self.nodes.get(&certificate.settlement_proof),
                            certificate,
                        )
                    {
                        return Err(CheckError::InvalidSatisfactionAllocation { id: *id });
                    }
                }
                Operation::ObligationBalance(certificate) => {
                    if !node.inputs.contains(&certificate.obligation_proof)
                        || !obligation_balance_matches(
                            self,
                            *id,
                            certificate,
                            &mut obligation_balance_uses,
                            &effective_allocations,
                        )
                    {
                        return Err(CheckError::InvalidObligationBalance { id: *id });
                    }
                }
                Operation::SettlementBalance(certificate) => {
                    if !node.inputs.contains(&certificate.settlement_proof)
                        || !settlement_balance_matches(
                            self,
                            *id,
                            certificate,
                            &mut settlement_balance_uses,
                            &effective_allocations,
                        )
                    {
                        return Err(CheckError::InvalidSettlementBalance { id: *id });
                    }
                }
                Operation::Recognition {
                    sale,
                    sale_proof,
                    account,
                    asset,
                    quantity,
                    proceeds,
                    basis,
                    gain,
                    quantity_unit,
                    value_unit,
                } => {
                    if !node.inputs.contains(sale_proof)
                        || !sale_observation_matches(
                            self.nodes.get(sale_proof),
                            sale,
                            account,
                            asset,
                        )
                    {
                        return Err(CheckError::InvalidRecognition { id: *id });
                    }
                    let mut total_quantity = Exact::from(0i64);
                    let mut total_proceeds = Exact::from(0i64);
                    let mut total_basis = Exact::from(0i64);
                    let mut total_gain = Exact::from(0i64);
                    let mut saw_allocation = false;
                    for input in &node.inputs {
                        let Some(input_node) = self.nodes.get(input) else {
                            return Err(CheckError::InvalidRecognition { id: *id });
                        };
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
                let Some(node) = self.nodes.get(&id) else {
                    return Err(CheckError::MissingRoot { root: id });
                };
                pending.extend(node.inputs.iter().copied());
            }
        }
        for (id, node) in &self.nodes {
            if matches!(
                node.operation,
                Operation::LotAllocation(..)
                    | Operation::InventoryConservation { .. }
                    | Operation::Recognition { .. }
                    | Operation::SettlementReconciliation(..)
                    | Operation::BlockedSale(..)
                    | Operation::SettlementHistory(..)
                    | Operation::SatisfactionAllocation(..)
                    | Operation::ObligationBalance(..)
                    | Operation::SettlementBalance(..)
                    | Operation::PositionReconciliation(..)
                    | Operation::JournalEntry(..)
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
                    let Some(count) = remaining.get_mut(dependant) else {
                        return Err(CheckError::MissingInput {
                            node: *dependant,
                            input: id,
                        });
                    };
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
        Self::content_hash_from_ids(self.roots.iter().copied(), self.nodes.keys().copied())
    }

    /// Compute the canonical content root from proof roots and node IDs.
    ///
    /// This is the compact commitment used by [`Self::content_hash`].  It is
    /// public so proof projections can authenticate a complete ID set without
    /// reimplementing the byte encoding or relying on hidden node payloads.
    pub fn content_hash_from_ids(
        roots: impl IntoIterator<Item = ProofId>,
        ids: impl IntoIterator<Item = ProofId>,
    ) -> ProofId {
        let mut roots = roots.into_iter().collect::<Vec<_>>();
        roots.sort_unstable();
        roots.dedup();
        let mut ids = ids.into_iter().collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        let mut bytes = Vec::new();
        put_bytes(&mut bytes, b"axiom/proof/v1");
        put_u64(&mut bytes, roots.len() as u64);
        for root in roots {
            bytes.extend_from_slice(&root.0);
        }
        put_u64(&mut bytes, ids.len() as u64);
        for id in ids {
            bytes.extend_from_slice(&id.0);
            bytes.extend_from_slice(&id.0);
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
    InvalidQuoteObservation { id: ProofId },
    InvalidPositionObservation { id: ProofId },
    InvalidCashSettlementObservation { id: ProofId },
    InvalidBlockedSale { id: ProofId },
    InvalidSettlementReconciliation { id: ProofId },
    InvalidObligationObservation { id: ProofId },
    InvalidSettlementObservation { id: ProofId },
    InvalidSatisfactionObservation { id: ProofId },
    InvalidLotAllocation { id: ProofId },
    InvalidInventoryConservation { id: ProofId },
    InvalidRecognition { id: ProofId },
    InvalidSettlementHistory { id: ProofId },
    InvalidSatisfactionAllocation { id: ProofId },
    InvalidObligationBalance { id: ProofId },
    InvalidSettlementBalance { id: ProofId },
    InvalidPositionReconciliation { id: ProofId },
    InvalidJournalEntry { id: ProofId },
    DuplicateObligationObservation { id: ProofId },
    DuplicateSettlementObservation { id: ProofId },
    DuplicateSatisfactionObservation { id: ProofId },
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
            Self::InvalidQuoteObservation { id } => {
                write!(f, "proof node {id} contains an invalid quote observation")
            }
            Self::InvalidPositionObservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid position observation"
                )
            }
            Self::InvalidCashSettlementObservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid cash settlement observation"
                )
            }
            Self::InvalidBlockedSale { id } => {
                write!(f, "proof node {id} contains an invalid blocked sale")
            }
            Self::InvalidSettlementReconciliation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid settlement reconciliation"
                )
            }
            Self::InvalidObligationObservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid obligation observation"
                )
            }
            Self::InvalidSettlementObservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid settlement observation"
                )
            }
            Self::InvalidSatisfactionObservation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid satisfaction observation"
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
            Self::InvalidSettlementHistory { id } => {
                write!(f, "proof node {id} contains an invalid settlement history")
            }
            Self::InvalidSatisfactionAllocation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid satisfaction allocation"
                )
            }
            Self::InvalidObligationBalance { id } => {
                write!(f, "proof node {id} contains an invalid obligation balance")
            }
            Self::InvalidSettlementBalance { id } => {
                write!(f, "proof node {id} contains an invalid settlement balance")
            }
            Self::InvalidPositionReconciliation { id } => {
                write!(
                    f,
                    "proof node {id} contains an invalid position reconciliation"
                )
            }
            Self::InvalidJournalEntry { id } => {
                write!(f, "proof node {id} contains an invalid journal entry")
            }
            Self::DuplicateObligationObservation { id } => {
                write!(
                    f,
                    "proof bundle contains duplicate obligation observation at {id}"
                )
            }
            Self::DuplicateSettlementObservation { id } => {
                write!(
                    f,
                    "proof bundle contains duplicate settlement observation at {id}"
                )
            }
            Self::DuplicateSatisfactionObservation { id } => {
                write!(
                    f,
                    "proof bundle contains duplicate satisfaction observation at {id}"
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

fn valid_obligation_observation(certificate: &ObligationObservationCertificate) -> bool {
    !certificate.obligation.trim().is_empty()
        && !certificate.debtor.trim().is_empty()
        && !certificate.creditor.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.promised.is_negative()
        && !certificate.promised.is_zero()
        && certificate.due.as_deref().is_none_or(valid_canonical_date)
}

fn valid_quote_observation(certificate: &QuoteObservationCertificate) -> bool {
    !certificate.quote.trim().is_empty()
        && valid_canonical_date(&certificate.date)
        && !certificate.base_unit.trim().is_empty()
        && !certificate.quote_unit.trim().is_empty()
        && !certificate.base.is_negative()
        && !certificate.base.is_zero()
        && !certificate.quote_amount.is_negative()
        && !certificate.quote_amount.is_zero()
}

fn valid_position_observation(certificate: &PositionObservationCertificate) -> bool {
    !certificate.account.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.quantity.is_negative()
}

fn valid_cash_settlement_observation(certificate: &CashSettlementObservationCertificate) -> bool {
    !certificate.reference.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.amount.is_negative()
        && !certificate.amount.is_zero()
        && certificate
            .into
            .as_deref()
            .is_none_or(|account| !account.trim().is_empty())
}

fn valid_blocked_sale(certificate: &BlockedSaleCertificate) -> bool {
    !certificate.sale.trim().is_empty()
        && certificate.source_proof != ProofId::ZERO
        && !certificate.quantity_unit.trim().is_empty()
        && !certificate.value_unit.trim().is_empty()
        && !certificate.account.trim().is_empty()
        && !certificate.asset.trim().is_empty()
        && valid_blocked_sale_reason(&certificate.reason)
        && (certificate.reason == "invalid-amount"
            || (!certificate.quantity.is_negative()
                && !certificate.quantity.is_zero()
                && !certificate.proceeds.is_negative()
                && !certificate.proceeds.is_zero()))
}

fn valid_blocked_sale_reason(reason: &str) -> bool {
    let reason = reason.trim();
    let Some((kind, detail)) = reason.split_once(':') else {
        return matches!(
            reason,
            "missing-lot" | "invalid-amount" | "incompatible-unit"
        );
    };
    if detail.trim().is_empty() {
        return false;
    }
    match kind {
        "ambiguous-lot" => detail
            .split(',')
            .all(|candidate| !candidate.trim().is_empty()),
        "policy-decision-conflict" => {
            let mut lots = detail.split(':');
            lots.next().is_some_and(|lot| !lot.trim().is_empty())
                && lots.next().is_some_and(|lot| !lot.trim().is_empty())
                && lots.next().is_none()
        }
        _ => false,
    }
}

fn valid_settlement_reconciliation(certificate: &SettlementReconciliationCertificate) -> bool {
    !certificate.settlement.trim().is_empty()
        && certificate.source_proof != ProofId::ZERO
        && !certificate.sale.trim().is_empty()
        && certificate.sale_proof != ProofId::ZERO
        && !certificate.unit.trim().is_empty()
        && !certificate.observed.is_negative()
        && !certificate.expected.is_negative()
        && matches!(certificate.status.trim(), "reconciled" | "conflict")
        && ((certificate.status == "reconciled" && certificate.observed == certificate.expected)
            || (certificate.status == "conflict" && certificate.observed != certificate.expected))
}

fn valid_position_reconciliation(certificate: &PositionReconciliationCertificate) -> bool {
    !certificate.account.trim().is_empty()
        && certificate.source_proof != ProofId::ZERO
        && !certificate.unit.trim().is_empty()
        && matches!(certificate.status.trim(), "reconciled" | "conflict")
        && certificate.observed == certificate.result
        && ((certificate.status == "reconciled" && certificate.calculated == certificate.result)
            || (certificate.status == "conflict" && certificate.calculated != certificate.result))
}

fn valid_journal_line(certificate: &JournalLineCertificate) -> bool {
    matches!(certificate.side.trim(), "debit" | "credit")
        && !certificate.account.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.amount.is_negative()
}

fn valid_journal_entry(certificate: &JournalEntryCertificate) -> bool {
    !certificate.sale.trim().is_empty()
        && certificate.recognition_proof != ProofId::ZERO
        && certificate.settlement_proof != ProofId::ZERO
        && !certificate.inventory_account.trim().is_empty()
        && !certificate.asset.trim().is_empty()
        && certificate.lines.len() == 3
        && certificate.lines.iter().all(valid_journal_line)
}

fn valid_settlement_observation(certificate: &SettlementObservationCertificate) -> bool {
    !certificate.settlement.trim().is_empty()
        && valid_settlement_kind(&certificate.kind)
        && !certificate.from.trim().is_empty()
        && !certificate.to.trim().is_empty()
        && !certificate.instrument.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && certificate.instrument == certificate.unit
        && !certificate.amount.is_negative()
        && !certificate.amount.is_zero()
        && valid_settlement_history(&certificate.settlement, &certificate.history).is_some()
}

fn valid_satisfaction_observation(certificate: &SatisfactionObservationCertificate) -> bool {
    !certificate.satisfaction.trim().is_empty()
        && !certificate.obligation.trim().is_empty()
        && !certificate.settlement.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.amount.is_negative()
        && !certificate.amount.is_zero()
        && valid_allocation_state(&certificate.state)
}

fn valid_settlement_history_certificate(certificate: &SettlementHistoryCertificate) -> bool {
    !certificate.settlement.trim().is_empty()
        && valid_settlement_kind(&certificate.kind)
        && !certificate.from.trim().is_empty()
        && !certificate.to.trim().is_empty()
        && !certificate.instrument.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && certificate.instrument == certificate.unit
        && !certificate.amount.is_negative()
        && !certificate.amount.is_zero()
        && valid_settlement_history(&certificate.settlement, &certificate.history).is_some_and(
            |(current, effective)| {
                certificate.current == current && certificate.effective == effective
            },
        )
}

fn valid_satisfaction_allocation(certificate: &SatisfactionAllocationCertificate) -> bool {
    !certificate.satisfaction.trim().is_empty()
        && !certificate.obligation.trim().is_empty()
        && !certificate.settlement.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.amount.is_negative()
        && !certificate.amount.is_zero()
        && valid_allocation_state(&certificate.state)
}

fn valid_obligation_balance(certificate: &ObligationBalanceCertificate) -> bool {
    !certificate.obligation.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.promised.is_negative()
        && !certificate.allocated.is_negative()
        && !certificate.remaining.is_negative()
        && certificate.allocated.checked_add(&certificate.remaining) == certificate.promised
        && unique_proof_ids(&certificate.allocations)
}

fn valid_settlement_balance(certificate: &SettlementBalanceCertificate) -> bool {
    !certificate.settlement.trim().is_empty()
        && !certificate.unit.trim().is_empty()
        && !certificate.amount.is_negative()
        && !certificate.allocated.is_negative()
        && !certificate.unused.is_negative()
        && certificate.allocated.checked_add(&certificate.unused) == certificate.amount
        && unique_proof_ids(&certificate.allocations)
}

fn valid_allocation_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "proposed" | "applied" | "reversed"
    )
}

fn valid_settlement_kind(kind: &str) -> bool {
    matches!(kind.trim(), "ach" | "card" | "check")
}

fn valid_canonical_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        && parse_date_key(value).is_some()
}

fn is_applied(state: &str) -> bool {
    state.trim().eq_ignore_ascii_case("applied")
}

fn unique_proof_ids(ids: &[ProofId]) -> bool {
    ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
}

/// Validate a source settlement history without calling the ontology.  The
/// return value is the independently derived current state and effectiveness.
fn valid_settlement_history(
    settlement: &str,
    history: &[SettlementTransition],
) -> Option<(String, bool)> {
    if settlement.trim().is_empty() || history.is_empty() {
        return None;
    }
    let mut previous_state: Option<&str> = None;
    let mut previous_at = None;
    for transition in history {
        if transition.state.trim().is_empty() || !valid_settlement_state(&transition.state) {
            return None;
        }
        let state = transition.state.trim();
        if !settlement_transition_is_legal(previous_state, state) {
            return None;
        }
        if let Some(at) = transition.at.as_deref() {
            if !valid_canonical_date(at) {
                return None;
            }
            let current_at = parse_date_key(at)?;
            if previous_at.is_some_and(|previous| current_at < previous) {
                return None;
            }
            previous_at = Some(current_at);
        }
        previous_state = Some(state);
    }
    let current = previous_state?.to_string();
    // `resolved` records that a dispute ended, but does not say who prevailed.
    // Only an explicit settled state proves effective payment.
    let effective = current.eq_ignore_ascii_case("settled");
    Some((current, effective))
}

fn valid_settlement_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "issued"
            | "authorized"
            | "presented"
            | "pending"
            | "settled"
            | "returned"
            | "reversed"
            | "rejected"
            | "cancelled"
            | "refunded"
            | "disputed"
            | "charged-back"
            | "represented"
            | "resolved"
    )
}

fn settlement_transition_is_legal(previous: Option<&str>, next: &str) -> bool {
    let next = next.trim().to_ascii_lowercase();
    let previous = previous.map(|state| state.trim().to_ascii_lowercase());
    matches!(
        (previous.as_deref(), next.as_str()),
        (None, "issued")
            | (Some("issued"), "authorized" | "presented" | "cancelled")
            | (Some("authorized"), "presented" | "cancelled" | "rejected")
            | (
                Some("presented"),
                "pending" | "settled" | "returned" | "rejected" | "cancelled"
            )
            | (
                Some("pending"),
                "settled" | "returned" | "rejected" | "cancelled"
            )
            | (
                Some("settled"),
                "returned" | "reversed" | "refunded" | "disputed" | "charged-back"
            )
            | (Some("disputed"), "resolved" | "charged-back")
            | (Some("charged-back"), "represented")
            | (Some("represented"), "pending" | "settled" | "rejected")
            | (Some("returned"), "presented" | "cancelled")
            | (Some("reversed"), "presented" | "cancelled")
            | (Some("rejected"), "presented" | "cancelled")
    )
}

fn parse_date_key(value: &str) -> Option<(i32, u8, u8)> {
    let mut pieces = value.split('-');
    let year = pieces.next()?.parse::<i32>().ok()?;
    let month = pieces.next()?.parse::<u8>().ok()?;
    let day = pieces.next()?.parse::<u8>().ok()?;
    if pieces.next().is_some()
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some((year, month, day))
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn settlement_history_matches_source(
    node: Option<&Node>,
    certificate: &SettlementHistoryCertificate,
) -> bool {
    let Some(Node {
        operation: Operation::SettlementObservation(source),
        ..
    }) = node
    else {
        return false;
    };
    source.settlement == certificate.settlement
        && source.kind == certificate.kind
        && source.from == certificate.from
        && source.to == certificate.to
        && source.instrument == certificate.instrument
        && source.amount == certificate.amount
        && source.unit == certificate.unit
        && source.history == certificate.history
        && valid_settlement_history(&source.settlement, &source.history).is_some_and(
            |(current, effective)| {
                certificate.current == current && certificate.effective == effective
            },
        )
}

fn position_observation_matches(
    node: Option<&Node>,
    certificate: &PositionReconciliationCertificate,
) -> bool {
    let Some(Node {
        operation: Operation::PositionObservation(source),
        ..
    }) = node
    else {
        return false;
    };
    source.account == certificate.account
        && source.quantity == certificate.observed
        && source.unit == certificate.unit
}

fn position_reconciliation_matches(
    proof: &Proof,
    node: &Node,
    certificate: &PositionReconciliationCertificate,
) -> bool {
    let mut calculated = Exact::from(0i64);
    let mut saw_event = false;
    for input in &node.inputs {
        if *input == certificate.source_proof {
            continue;
        }
        let Some(input_node) = proof.nodes.get(input) else {
            return false;
        };
        match &input_node.operation {
            Operation::LotObservation {
                account,
                asset,
                quantity,
                ..
            } if account == &certificate.account && asset == &certificate.unit => {
                calculated = calculated.checked_add(quantity);
                saw_event = true;
            }
            Operation::Recognition {
                account,
                asset,
                quantity,
                quantity_unit,
                ..
            } if account == &certificate.account
                && asset == &certificate.unit
                && quantity_unit == &certificate.unit =>
            {
                calculated = calculated.checked_sub(quantity);
                saw_event = true;
            }
            _ => return false,
        }
    }
    saw_event && calculated == certificate.calculated
}

fn journal_entry_matches_sources(proof: &Proof, certificate: &JournalEntryCertificate) -> bool {
    let Some(Node {
        operation:
            Operation::Recognition {
                sale,
                account,
                asset,
                proceeds,
                basis,
                gain,
                value_unit,
                ..
            },
        ..
    }) = proof.nodes.get(&certificate.recognition_proof)
    else {
        return false;
    };
    let Some((reference, amount, unit, into)) =
        find_cash_settlement_source(proof, certificate.settlement_proof)
    else {
        return false;
    };
    if sale != &certificate.sale
        || account != &certificate.inventory_account
        || asset != &certificate.asset
        || reference != certificate.sale
        || amount != *proceeds
        || unit != *value_unit
        || into.trim().is_empty()
    {
        return false;
    }
    let gain_line = if gain.is_negative() {
        JournalLineCertificate {
            side: "debit".into(),
            account: "loss:recognized".into(),
            amount: gain.abs(),
            unit: value_unit.clone(),
        }
    } else {
        JournalLineCertificate {
            side: "credit".into(),
            account: "gain:recognized".into(),
            amount: gain.clone(),
            unit: value_unit.clone(),
        }
    };
    let expected = [
        JournalLineCertificate {
            side: "debit".into(),
            account: into,
            amount: proceeds.clone(),
            unit: value_unit.clone(),
        },
        JournalLineCertificate {
            side: "credit".into(),
            account: format!("{account}:{asset}"),
            amount: basis.clone(),
            unit: value_unit.clone(),
        },
        gain_line,
    ];
    certificate.lines == expected
}

fn settlement_reconciliation_matches_sources(
    proof: &Proof,
    certificate: &SettlementReconciliationCertificate,
) -> bool {
    let Some(Node {
        operation: Operation::CashSettlementObservation(source),
        ..
    }) = proof.nodes.get(&certificate.source_proof)
    else {
        return false;
    };
    let Some(Node {
        operation:
            Operation::Recognition {
                sale,
                proceeds,
                value_unit,
                ..
            },
        ..
    }) = proof.nodes.get(&certificate.sale_proof)
    else {
        return false;
    };
    source.reference == certificate.settlement
        && source.reference == certificate.sale
        && source.amount == certificate.observed
        && source.unit == certificate.unit
        && sale == &certificate.sale
        && proceeds == &certificate.expected
        && value_unit == &certificate.unit
}

fn find_cash_settlement_source(
    proof: &Proof,
    root: ProofId,
) -> Option<(String, Exact, String, String)> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let node = proof.nodes.get(&id)?;
        match &node.operation {
            Operation::CashSettlementObservation(source) => {
                let into = source.into.clone()?;
                return Some((
                    source.reference.clone(),
                    source.amount.clone(),
                    source.unit.clone(),
                    into,
                ));
            }
            Operation::Derive { .. } => pending.extend(node.inputs.iter().copied()),
            Operation::SettlementReconciliation(certificate) => {
                pending.push(certificate.source_proof)
            }
            _ => {}
        }
    }
    None
}

fn satisfaction_allocation_matches_sources(
    satisfaction_node: Option<&Node>,
    obligation_node: Option<&Node>,
    settlement_node: Option<&Node>,
    certificate: &SatisfactionAllocationCertificate,
) -> bool {
    let Some(Node {
        operation: Operation::SatisfactionObservation(satisfaction),
        ..
    }) = satisfaction_node
    else {
        return false;
    };
    let Some(Node {
        operation: Operation::ObligationObservation(obligation),
        ..
    }) = obligation_node
    else {
        return false;
    };
    let Some(Node {
        operation: Operation::SettlementObservation(settlement),
        ..
    }) = settlement_node
    else {
        return false;
    };
    satisfaction.satisfaction == certificate.satisfaction
        && satisfaction.obligation == certificate.obligation
        && satisfaction.settlement == certificate.settlement
        && satisfaction.amount == certificate.amount
        && satisfaction.unit == certificate.unit
        && satisfaction.state == certificate.state
        && obligation.obligation == certificate.obligation
        && settlement.settlement == certificate.settlement
        && obligation.debtor == settlement.from
        && obligation.creditor == settlement.to
        && obligation.unit == settlement.unit
        && obligation.unit == certificate.unit
        && settlement.instrument == certificate.unit
}

fn settlement_is_effective(node: Option<&Node>) -> bool {
    let Some(Node {
        operation: Operation::SettlementObservation(settlement),
        ..
    }) = node
    else {
        return false;
    };
    valid_settlement_history(&settlement.settlement, &settlement.history)
        .is_some_and(|(_, effective)| effective)
}

fn allocation_is_effective(proof: &Proof, node: &Node) -> bool {
    let Operation::SatisfactionAllocation(certificate) = &node.operation else {
        return false;
    };
    is_applied(&certificate.state)
        && settlement_is_effective(proof.nodes.get(&certificate.settlement_proof))
}

/// Effective satisfaction allocations are shared by obligation and
/// settlement balance certificates.  Building these indexes once avoids
/// rescanning the complete proof DAG for every balance node.
struct EffectiveAllocationIndex {
    by_obligation: BTreeMap<String, (BTreeSet<ProofId>, Exact)>,
    by_settlement: BTreeMap<String, (BTreeSet<ProofId>, Exact)>,
}

fn effective_allocation_index(proof: &Proof) -> EffectiveAllocationIndex {
    let mut index = EffectiveAllocationIndex {
        by_obligation: BTreeMap::new(),
        by_settlement: BTreeMap::new(),
    };
    for (id, node) in &proof.nodes {
        let Operation::SatisfactionAllocation(certificate) = &node.operation else {
            continue;
        };
        if !allocation_is_effective(proof, node) {
            continue;
        }
        let obligation = index
            .by_obligation
            .entry(certificate.obligation.clone())
            .or_insert_with(|| (BTreeSet::new(), Exact::from(0i64)));
        obligation.0.insert(*id);
        obligation.1 = obligation.1.checked_add(&certificate.amount);

        let settlement = index
            .by_settlement
            .entry(certificate.settlement.clone())
            .or_insert_with(|| (BTreeSet::new(), Exact::from(0i64)));
        settlement.0.insert(*id);
        settlement.1 = settlement.1.checked_add(&certificate.amount);
    }
    index
}

fn obligation_balance_matches(
    proof: &Proof,
    node_id: ProofId,
    certificate: &ObligationBalanceCertificate,
    uses: &mut BTreeMap<ProofId, ProofId>,
    index: &EffectiveAllocationIndex,
) -> bool {
    if uses.insert(certificate.obligation_proof, node_id).is_some() {
        return false;
    }
    let Some(Node {
        operation: Operation::ObligationObservation(obligation),
        ..
    }) = proof.nodes.get(&certificate.obligation_proof)
    else {
        return false;
    };
    if obligation.obligation != certificate.obligation
        || obligation.promised != certificate.promised
        || obligation.unit != certificate.unit
    {
        return false;
    }
    let Some(balance) = proof.nodes.get(&node_id) else {
        return false;
    };
    let listed = certificate
        .allocations
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected = index.by_obligation.get(&certificate.obligation);
    let allocations_match = match expected {
        Some((expected_ids, expected_total)) => {
            &listed == expected_ids && &certificate.allocated == expected_total
        }
        None => listed.is_empty() && certificate.allocated.is_zero(),
    };
    allocations_match
        && certificate.allocated.checked_add(&certificate.remaining) == certificate.promised
        && certificate.allocations.iter().all(|allocation| {
            balance.inputs.binary_search(allocation).is_ok()
                && proof.nodes.get(allocation).is_some_and(|node| {
                    matches!(node.operation, Operation::SatisfactionAllocation(_))
                })
        })
}

fn settlement_balance_matches(
    proof: &Proof,
    node_id: ProofId,
    certificate: &SettlementBalanceCertificate,
    uses: &mut BTreeMap<ProofId, ProofId>,
    index: &EffectiveAllocationIndex,
) -> bool {
    if uses.insert(certificate.settlement_proof, node_id).is_some() {
        return false;
    }
    let Some(Node {
        operation: Operation::SettlementObservation(settlement),
        ..
    }) = proof.nodes.get(&certificate.settlement_proof)
    else {
        return false;
    };
    if settlement.settlement != certificate.settlement
        || settlement.amount != certificate.amount
        || settlement.unit != certificate.unit
    {
        return false;
    }
    let Some(balance) = proof.nodes.get(&node_id) else {
        return false;
    };
    let listed = certificate
        .allocations
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected = index.by_settlement.get(&certificate.settlement);
    let allocations_match = match expected {
        Some((expected_ids, expected_total)) => {
            &listed == expected_ids && &certificate.allocated == expected_total
        }
        None => listed.is_empty() && certificate.allocated.is_zero(),
    };
    allocations_match
        && certificate.allocated.checked_add(&certificate.unused) == certificate.amount
        && certificate.allocations.iter().all(|allocation| {
            balance.inputs.binary_search(allocation).is_ok()
                && proof.nodes.get(allocation).is_some_and(|node| {
                    matches!(node.operation, Operation::SatisfactionAllocation(_))
                })
        })
}

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

fn sale_observation_matches(node: Option<&Node>, sale: &str, account: &str, asset: &str) -> bool {
    matches!(
        node.map(|node| &node.operation),
        Some(Operation::SaleObservation {
            sale: observed,
            account: observed_account,
            asset: observed_asset,
            ..
        }) if observed == sale && observed_account == account && observed_asset == asset
    )
}

fn blocked_sale_matches_source(node: Option<&Node>, certificate: &BlockedSaleCertificate) -> bool {
    matches!(
        node.map(|node| &node.operation),
        Some(Operation::SaleObservation {
            sale: observed,
            account,
            asset,
            quantity,
            proceeds,
            quantity_unit,
            value_unit,
            ..
        }) if observed == &certificate.sale
            && account == &certificate.account
            && asset == &certificate.asset
            && quantity == &certificate.quantity
            && proceeds == &certificate.proceeds
            && quantity_unit == &certificate.quantity_unit
            && value_unit == &certificate.value_unit
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

fn put_proof_ids(out: &mut Vec<u8>, values: &[ProofId]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_proof_id(out, value);
    }
}

fn put_transitions(out: &mut Vec<u8>, values: &[SettlementTransition]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_string(out, &value.state);
        match &value.at {
            Some(at) => {
                out.push(1);
                put_string(out, at);
            }
            None => out.push(0),
        }
    }
}

fn put_journal_lines(out: &mut Vec<u8>, values: &[JournalLineCertificate]) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_string(out, &value.side);
        put_string(out, &value.account);
        put_string(out, &value.amount.canonical_string());
        put_string(out, &value.unit);
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
                account: "brokerage".into(),
                asset: "ABC".into(),
                quantity: Exact::from(10i64),
            },
            vec![],
            BTreeMap::new(),
        );
        let sale = Node::new(
            "sale/one",
            Operation::SaleObservation {
                sale: "sale/one".into(),
                source: "sell sale/one".into(),
                account: "brokerage".into(),
                asset: "ABC".into(),
                quantity: Exact::from(3_i64),
                proceeds: Exact::from(30_i64),
                quantity_unit: "ABC".into(),
                value_unit: "USD".into(),
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
                account: "brokerage".into(),
                asset: "ABC".into(),
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

    fn satisfaction_fixture(
        current: &str,
        effective: bool,
        allocation_amount: i64,
        remaining: i64,
        unused: i64,
    ) -> Proof {
        let obligation = Node::new(
            "obligation invoice",
            Operation::ObligationObservation(ObligationObservationCertificate {
                obligation: "invoice".into(),
                debtor: "alice".into(),
                creditor: "bob".into(),
                promised: Exact::from(100i64),
                unit: "USD".into(),
                due: Some("2026-12-31".into()),
            }),
            vec![],
            BTreeMap::new(),
        );
        let settlement_source = Node::new(
            "settlement payment",
            Operation::SettlementObservation(SettlementObservationCertificate {
                settlement: "payment".into(),
                kind: "ach".into(),
                from: "alice".into(),
                to: "bob".into(),
                instrument: "USD".into(),
                amount: Exact::from(60i64),
                unit: "USD".into(),
                history: vec![
                    SettlementTransition {
                        state: "issued".into(),
                        at: Some("2026-01-01".into()),
                    },
                    SettlementTransition {
                        state: "presented".into(),
                        at: Some("2026-01-02".into()),
                    },
                    SettlementTransition {
                        state: "settled".into(),
                        at: Some("2026-01-03".into()),
                    },
                ],
            }),
            vec![],
            BTreeMap::new(),
        );
        let satisfaction = Node::new(
            "satisfaction allocation-1",
            Operation::SatisfactionObservation(SatisfactionObservationCertificate {
                satisfaction: "allocation-1".into(),
                obligation: "invoice".into(),
                settlement: "payment".into(),
                amount: Exact::from(allocation_amount),
                unit: "USD".into(),
                state: "applied".into(),
            }),
            vec![],
            BTreeMap::new(),
        );
        let history = Node::new(
            "settlement history payment",
            Operation::SettlementHistory(Box::new(SettlementHistoryCertificate {
                settlement: "payment".into(),
                settlement_proof: settlement_source.id,
                kind: "ach".into(),
                from: "alice".into(),
                to: "bob".into(),
                instrument: "USD".into(),
                amount: Exact::from(60i64),
                unit: "USD".into(),
                history: match &settlement_source.operation {
                    Operation::SettlementObservation(source) => source.history.clone(),
                    _ => unreachable!(),
                },
                current: current.into(),
                effective,
            })),
            vec![settlement_source.id],
            BTreeMap::new(),
        );
        let allocation = Node::new(
            "apply allocation-1",
            Operation::SatisfactionAllocation(Box::new(SatisfactionAllocationCertificate {
                satisfaction: "allocation-1".into(),
                satisfaction_proof: satisfaction.id,
                obligation: "invoice".into(),
                obligation_proof: obligation.id,
                settlement: "payment".into(),
                settlement_proof: settlement_source.id,
                amount: Exact::from(allocation_amount),
                unit: "USD".into(),
                state: "applied".into(),
            })),
            vec![satisfaction.id, obligation.id, settlement_source.id],
            BTreeMap::new(),
        );
        let obligation_balance = Node::new(
            "balance invoice",
            Operation::ObligationBalance(Box::new(ObligationBalanceCertificate {
                obligation: "invoice".into(),
                obligation_proof: obligation.id,
                promised: Exact::from(100i64),
                allocated: Exact::from(allocation_amount),
                remaining: Exact::from(remaining),
                unit: "USD".into(),
                allocations: vec![allocation.id],
            })),
            vec![obligation.id, allocation.id],
            BTreeMap::new(),
        );
        let settlement_balance = Node::new(
            "balance payment",
            Operation::SettlementBalance(Box::new(SettlementBalanceCertificate {
                settlement: "payment".into(),
                settlement_proof: settlement_source.id,
                amount: Exact::from(60i64),
                allocated: Exact::from(allocation_amount),
                unused: Exact::from(unused),
                unit: "USD".into(),
                allocations: vec![allocation.id],
            })),
            vec![settlement_source.id, allocation.id],
            BTreeMap::new(),
        );
        let obligation_balance_id = obligation_balance.id;
        let settlement_balance_id = settlement_balance.id;
        let history_id = history.id;
        let mut proof = Proof::new();
        for node in [
            obligation,
            settlement_source,
            satisfaction,
            history,
            allocation,
            obligation_balance,
            settlement_balance,
        ] {
            proof.insert(node);
        }
        proof.root(obligation_balance_id);
        proof.root(settlement_balance_id);
        proof.root(history_id);
        proof
    }

    #[test]
    fn checker_verifies_typed_satisfaction_balances_and_history() {
        assert!(
            satisfaction_fixture("settled", true, 60, 40, 0)
                .check()
                .is_ok()
        );
    }

    #[test]
    fn checker_rejects_tampered_satisfaction_certificates() {
        assert!(matches!(
            satisfaction_fixture("issued", true, 60, 40, 0).check(),
            Err(CheckError::InvalidSettlementHistory { .. })
        ));
        assert!(
            satisfaction_fixture("settled", true, 61, 39, 0)
                .check()
                .is_err()
        );
        assert!(
            satisfaction_fixture("settled", true, 60, 41, 0)
                .check()
                .is_err()
        );
        assert!(
            satisfaction_fixture("settled", true, 60, 40, 1)
                .check()
                .is_err()
        );
    }

    #[test]
    fn typed_source_payload_includes_due_and_settlement_kind() {
        let obligation = |due| {
            Node::new(
                "obligation invoice",
                Operation::ObligationObservation(ObligationObservationCertificate {
                    obligation: "invoice".into(),
                    debtor: "alice".into(),
                    creditor: "bob".into(),
                    promised: Exact::from(100i64),
                    unit: "USD".into(),
                    due,
                }),
                vec![],
                BTreeMap::new(),
            )
        };
        assert_ne!(
            obligation(Some("2026-12-31".into())).id,
            obligation(Some("2027-01-01".into())).id
        );

        let settlement = |kind| {
            Node::new(
                "settlement payment",
                Operation::SettlementObservation(SettlementObservationCertificate {
                    settlement: "payment".into(),
                    kind,
                    from: "alice".into(),
                    to: "bob".into(),
                    instrument: "USD".into(),
                    amount: Exact::from(60i64),
                    unit: "USD".into(),
                    history: vec![SettlementTransition {
                        state: "issued".into(),
                        at: None,
                    }],
                }),
                vec![],
                BTreeMap::new(),
            )
        };
        assert_ne!(settlement("ach".into()).id, settlement("card".into()).id);
    }
}
