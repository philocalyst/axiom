//! Deterministic semantic analysis for the first Axiom slice.
//!
//! The parser gives us typed, source-located forms.  This module only derives
//! views: lots, conditional answers, recognition, observations, and a
//! balanced journal projection.  It never changes the source ledger and it
//! never silently chooses an answer when more than one answer is admissible.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::exact::Exact;
use crate::model::{self, Date, Ledger, LedgerForm, LotSelector};
use crate::package::{LotCandidate, PolicyRegistry, Selection, SelectionProgram};
use crate::proof::{LotAllocationCertificate, Node, Operation, Proof, ProofId};

pub use crate::model::Quantity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lot {
    pub id: String,
    pub date: Date,
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub consideration: Quantity,
    pub fee: Option<Quantity>,
    pub basis: Quantity,
    pub source_line: usize,
    pub proof: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionalGain {
    pub lot_id: String,
    /// Quantity of the holding represented by this conditional result.  For
    /// an unresolved single-lot alternative this is the sale quantity; for a
    /// recognized allocation it is the exact slice consumed from the lot.
    pub quantity: Quantity,
    pub proceeds: Quantity,
    pub basis: Quantity,
    pub gain: Quantity,
    pub proof: ProofId,
}

/// One exact, recognized slice of a sale.  A sale may consume several lots;
/// keeping each slice explicit makes conservation and audit explanations
/// possible without overloading `selected_lot` (which remains for the common
/// one-lot case).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LotAllocation {
    pub lot_id: String,
    pub quantity: Quantity,
    pub proceeds: Quantity,
    pub basis: Quantity,
    pub gain: Quantity,
    /// The immutable lot observation used by this allocation certificate.
    /// Keeping this edge beside the economic values lets the semantic checker
    /// validate the certificate without interpreting string metadata.
    pub lot_proof: ProofId,
    /// The immutable sale observation used by this allocation certificate.
    pub sale_proof: ProofId,
    /// The typed allocation derivation node. `proof` below is the gain
    /// arithmetic node, so both edges are retained explicitly.
    pub allocation_proof: ProofId,
    /// The inventory-conservation step that consumes this allocation.
    pub conservation_proof: ProofId,
    pub proof: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaleAnalysis {
    pub id: String,
    pub date: Date,
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub proceeds: Quantity,
    pub eligible_lots: Vec<String>,
    pub conditional_gains: Vec<ConditionalGain>,
    /// Recognized allocations, in deterministic economic/policy order.
    pub allocations: Vec<LotAllocation>,
    /// Aggregate recognized arithmetic, when this sale is complete.  The
    /// per-lot values remain in `allocations`.
    pub recognized: Option<ConditionalGain>,
    /// All lots consumed by this sale.  `selected_lot` is retained as a
    /// compatibility convenience and is populated only for one-lot sales.
    pub selected_lots: Vec<String>,
    pub selected_lot: Option<String>,
    pub status: RecognitionStatus,
    pub proof: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecognitionStatus {
    Recognized,
    Ambiguous {
        candidates: Vec<String>,
    },
    Conflict {
        policy_lot: String,
        decision_lot: String,
    },
    MissingLot,
    InvalidAmount,
}

impl RecognitionStatus {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Recognized)
    }

    pub fn is_blocked(&self) -> bool {
        !self.is_complete()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuoteView {
    pub id: String,
    pub date: Date,
    pub base: Quantity,
    pub quote: Quantity,
    pub proof: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuoteStatus {
    Unique,
    Ambiguous { quote_ids: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionView {
    pub account: String,
    pub quantity: Quantity,
    pub proof: ProofId,
    pub status: ObservationStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementView {
    pub reference: String,
    pub quantity: Quantity,
    pub proof: ProofId,
    /// An observed settlement is journal-usable only when it names the
    /// receiving account.  `None` deliberately remains an observed hole.
    pub into: Option<String>,
    pub status: ObservationStatus,
}

/// Evidence observations are not silently promoted to accepted facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationStatus {
    Observed,
    Reconciled,
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Debit,
    Credit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalLine {
    pub side: Side,
    pub account: String,
    pub quantity: Quantity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    pub sale: String,
    pub lines: Vec<JournalLine>,
    pub proof: ProofId,
}

impl JournalEntry {
    pub fn balanced(&self) -> bool {
        let mut totals: BTreeMap<model::Unit, Exact> = BTreeMap::new();
        for line in &self.lines {
            let Some(unit) = line.quantity.unit.as_ref() else {
                if !line.quantity.is_zero() {
                    return false;
                }
                continue;
            };
            let current = totals
                .entry(unit.clone())
                .or_insert_with(|| Exact::from(0i64));
            let amount = match line.side {
                Side::Debit => line.quantity.number.clone(),
                Side::Credit => -line.quantity.number.clone(),
            };
            *current = current.checked_add(&amount);
        }
        totals.values().all(Exact::is_zero)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Issue {
    pub code: IssueCode,
    pub message: String,
    pub sale: Option<String>,
    pub proof: Option<ProofId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IssueCode {
    AmbiguousLot,
    DecisionConflict,
    PolicyDecisionConflict,
    MissingLot,
    InsufficientInventory,
    PositionConflict,
    SettlementConflict,
    AmbiguousQuote,
    UnknownPolicy,
    IncompatibleUnit,
    InvalidAmount,
}

/// Failure from the finance-native semantic pass over an [`Analysis`].
/// Structural proof failures remain represented by [`crate::proof::CheckError`]
/// and are wrapped here only when callers ask for the combined `check_proof`
/// entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisCheckError {
    Proof(crate::proof::CheckError),
    BlockedSaleHasRecognition {
        sale: String,
        proof: ProofId,
    },
    MissingRecognition {
        sale: String,
        proof: ProofId,
    },
    UnknownLot {
        sale: String,
        lot: String,
        proof: ProofId,
    },
    InvalidAllocation {
        sale: String,
        lot: String,
        proof: ProofId,
        reason: String,
    },
    InvalidRecognition {
        sale: String,
        proof: ProofId,
        reason: String,
    },
}

impl AnalysisCheckError {
    fn proof_id(&self) -> Option<ProofId> {
        match self {
            Self::Proof(crate::proof::CheckError::MissingRoot { root }) => Some(*root),
            Self::Proof(crate::proof::CheckError::MissingNode { id }) => Some(*id),
            Self::Proof(crate::proof::CheckError::MapKeyMismatch { expected, .. }) => {
                Some(*expected)
            }
            Self::Proof(crate::proof::CheckError::TamperedNode { id })
            | Self::Proof(crate::proof::CheckError::InvalidArithmetic { id })
            | Self::Proof(crate::proof::CheckError::InvalidLotAllocation { id })
            | Self::Proof(crate::proof::CheckError::InvalidInventoryConservation { id })
            | Self::Proof(crate::proof::CheckError::InvalidRecognition { id })
            | Self::Proof(crate::proof::CheckError::UnreachableCertificate { id })
            | Self::Proof(crate::proof::CheckError::InvalidOperation { id }) => Some(*id),
            Self::Proof(_) => None,
            Self::BlockedSaleHasRecognition { proof, .. }
            | Self::MissingRecognition { proof, .. }
            | Self::UnknownLot { proof, .. }
            | Self::InvalidAllocation { proof, .. }
            | Self::InvalidRecognition { proof, .. } => Some(*proof),
        }
    }
}

impl fmt::Display for AnalysisCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Proof(error) => write!(formatter, "proof check failed: {error}"),
            Self::BlockedSaleHasRecognition { sale, .. } => {
                write!(formatter, "blocked sale `{sale}` carries recognition")
            }
            Self::MissingRecognition { sale, .. } => {
                write!(formatter, "recognized sale `{sale}` has no aggregate")
            }
            Self::UnknownLot { sale, lot, .. } => {
                write!(
                    formatter,
                    "sale `{sale}` allocation names unknown lot `{lot}`"
                )
            }
            Self::InvalidAllocation {
                sale, lot, reason, ..
            } => {
                write!(
                    formatter,
                    "invalid allocation `{lot}` for sale `{sale}`: {reason}"
                )
            }
            Self::InvalidRecognition { sale, reason, .. } => {
                write!(formatter, "invalid recognition for sale `{sale}`: {reason}")
            }
        }
    }
}

impl std::error::Error for AnalysisCheckError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Analysis {
    pub book: String,
    pub policy: Option<String>,
    pub decisions: Vec<String>,
    pub lots: Vec<Lot>,
    pub sales: Vec<SaleAnalysis>,
    pub quotes: Vec<QuoteView>,
    pub quote_status: BTreeMap<String, QuoteStatus>,
    pub positions: Vec<PositionView>,
    pub settlements: Vec<SettlementView>,
    pub journal: Vec<JournalEntry>,
    pub issues: Vec<Issue>,
    pub proof: Proof,
    /// Goal -> source/proof roots.  Consumers can invalidate a view by
    /// replacing only the roots named here, without guessing dependencies.
    pub dependencies: BTreeMap<String, Vec<ProofId>>,
    /// Source occurrence -> goals affected by that occurrence.
    pub invalidations: BTreeMap<String, Vec<String>>,
}

impl Analysis {
    pub fn blocked(&self) -> bool {
        self.issues.iter().any(|issue| {
            // Quote disagreement is a separate valuation concern.  It must
            // not turn a directly evidenced sale/check into a blocked result.
            issue.code != IssueCode::AmbiguousQuote
        }) || self.sales.iter().any(|sale| sale.status.is_blocked())
    }

    pub fn check_proof(&self) -> Result<(), crate::proof::CheckError> {
        match self.check_semantics() {
            Ok(()) => Ok(()),
            Err(AnalysisCheckError::Proof(error)) => Err(error),
            Err(error) => Err(crate::proof::CheckError::InvalidOperation {
                id: error.proof_id().unwrap_or(ProofId::ZERO),
            }),
        }
    }

    /// Independently verify allocation and recognition propositions.
    ///
    /// `Proof::check` validates hashes, edges, cycles, and primitive
    /// arithmetic.  This pass validates the finance-native proposition those
    /// nodes claim: a slice is proportional to the remaining lot, inventory
    /// is consumed once, and a recognized aggregate is exactly the sum of its
    /// slices.  No solver or metadata convention is consulted.
    pub fn check_semantics(&self) -> Result<(), AnalysisCheckError> {
        self.proof.check().map_err(AnalysisCheckError::Proof)?;

        let lots_by_id = self
            .lots
            .iter()
            .map(|lot| (lot.id.as_str(), lot))
            .collect::<BTreeMap<_, _>>();
        let mut remaining_quantity = self
            .lots
            .iter()
            .map(|lot| (lot.id.clone(), lot.quantity.number.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut remaining_basis = self
            .lots
            .iter()
            .map(|lot| (lot.id.clone(), lot.basis.number.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut previous_conservation = BTreeMap::<String, ProofId>::new();

        let mut sales = self.sales.iter().collect::<Vec<_>>();
        sales.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.id.cmp(&right.id))
        });

        for sale in sales {
            check_conditional_gains(
                self,
                sale,
                &lots_by_id,
                &remaining_quantity,
                &remaining_basis,
            )?;
            if !sale.status.is_complete() {
                if sale.recognized.is_some() || !sale.allocations.is_empty() {
                    return Err(AnalysisCheckError::BlockedSaleHasRecognition {
                        sale: sale.id.clone(),
                        proof: sale.proof,
                    });
                }
                continue;
            }

            if sale.allocations.is_empty() || sale.recognized.is_none() {
                return Err(AnalysisCheckError::MissingRecognition {
                    sale: sale.id.clone(),
                    proof: sale.proof,
                });
            }
            let selected_lots = sale
                .allocations
                .iter()
                .map(|allocation| allocation.lot_id.clone())
                .collect::<Vec<_>>();
            let selected_lot = (selected_lots.len() == 1).then(|| selected_lots[0].clone());
            if sale.selected_lots != selected_lots || sale.selected_lot != selected_lot {
                return Err(AnalysisCheckError::InvalidRecognition {
                    sale: sale.id.clone(),
                    proof: sale.proof,
                    reason: "selected lots do not match the recognized allocations".into(),
                });
            }
            let mut total_quantity = Quantity::zero();
            let mut total_proceeds = Quantity::zero();
            let mut total_basis = Quantity::zero();
            let mut total_gain = Quantity::zero();
            let mut gain_proofs = Vec::with_capacity(sale.allocations.len());
            let mut sale_observation = None;

            for allocation in &sale.allocations {
                let Some(lot) = lots_by_id.get(allocation.lot_id.as_str()) else {
                    return Err(AnalysisCheckError::UnknownLot {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                    });
                };
                if allocation.lot_proof != lot.proof
                    || allocation.sale_proof == ProofId::ZERO
                    || allocation.allocation_proof == ProofId::ZERO
                {
                    return Err(AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "certificate proof edges do not identify the source lot and sale"
                            .into(),
                    });
                }
                if let Some(previous) = sale_observation
                    && previous != allocation.sale_proof
                {
                    return Err(AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "allocation slices use different sale observations".into(),
                    });
                }
                sale_observation = Some(allocation.sale_proof);

                let available_quantity = remaining_quantity
                    .get(&allocation.lot_id)
                    .cloned()
                    .unwrap_or_else(|| Exact::from(0i64));
                let available_basis = remaining_basis
                    .get(&allocation.lot_id)
                    .cloned()
                    .unwrap_or_else(|| Exact::from(0i64));
                if allocation.quantity.is_zero()
                    || allocation.quantity.unit != lot.quantity.unit
                    || allocation.quantity.number > available_quantity
                {
                    return Err(AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "allocated quantity exceeds remaining lot inventory".into(),
                    });
                }
                if sale.quantity.number.is_zero()
                    || sale.quantity.unit != allocation.quantity.unit
                    || sale.proceeds.unit != lot.basis.unit
                    || allocation.proceeds.unit != sale.proceeds.unit
                    || allocation.basis.unit != lot.basis.unit
                    || allocation.gain.unit != sale.proceeds.unit
                {
                    return Err(AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "allocation units are incompatible".into(),
                    });
                }

                let expected_proceeds = sale.proceeds.number.checked_mul(
                    &allocation
                        .quantity
                        .number
                        .checked_div(&sale.quantity.number)
                        .map_err(|_| AnalysisCheckError::InvalidAllocation {
                            sale: sale.id.clone(),
                            lot: allocation.lot_id.clone(),
                            proof: allocation.proof,
                            reason: "sale quantity cannot form an exact allocation ratio".into(),
                        })?,
                );
                let expected_basis = available_basis.checked_mul(
                    &allocation
                        .quantity
                        .number
                        .checked_div(&available_quantity)
                        .map_err(|_| AnalysisCheckError::InvalidAllocation {
                            sale: sale.id.clone(),
                            lot: allocation.lot_id.clone(),
                            proof: allocation.proof,
                            reason: "remaining lot quantity cannot form an exact basis ratio"
                                .into(),
                        })?,
                );
                let expected_gain = expected_proceeds.checked_sub(&expected_basis);
                let expected_proceeds =
                    Quantity::new(expected_proceeds, sale.proceeds.unit.clone()).map_err(|_| {
                        AnalysisCheckError::InvalidAllocation {
                            sale: sale.id.clone(),
                            lot: allocation.lot_id.clone(),
                            proof: allocation.proof,
                            reason: "expected proceeds have no valid unit".into(),
                        }
                    })?;
                let expected_basis = Quantity::new(expected_basis, lot.basis.unit.clone())
                    .map_err(|_| AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "expected basis has no valid unit".into(),
                    })?;
                let expected_gain = Quantity::new(expected_gain, sale.proceeds.unit.clone())
                    .map_err(|_| AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "expected gain has no valid unit".into(),
                    })?;
                if allocation.proceeds != expected_proceeds
                    || allocation.basis != expected_basis
                    || allocation.gain != expected_gain
                {
                    return Err(AnalysisCheckError::InvalidAllocation {
                        sale: sale.id.clone(),
                        lot: allocation.lot_id.clone(),
                        proof: allocation.proof,
                        reason: "allocation arithmetic does not reproduce the typed values".into(),
                    });
                }

                check_allocation_nodes(
                    self,
                    sale,
                    lot,
                    allocation,
                    &available_quantity,
                    &available_basis,
                    previous_conservation.get(&allocation.lot_id).copied(),
                )?;
                total_quantity =
                    total_quantity
                        .checked_add(&allocation.quantity)
                        .map_err(|_| AnalysisCheckError::InvalidRecognition {
                            sale: sale.id.clone(),
                            proof: sale.proof,
                            reason: "allocation quantities are not summable".into(),
                        })?;
                total_proceeds =
                    total_proceeds
                        .checked_add(&allocation.proceeds)
                        .map_err(|_| AnalysisCheckError::InvalidRecognition {
                            sale: sale.id.clone(),
                            proof: sale.proof,
                            reason: "allocation proceeds are not summable".into(),
                        })?;
                total_basis = total_basis.checked_add(&allocation.basis).map_err(|_| {
                    AnalysisCheckError::InvalidRecognition {
                        sale: sale.id.clone(),
                        proof: sale.proof,
                        reason: "allocation bases are not summable".into(),
                    }
                })?;
                total_gain = total_gain.checked_add(&allocation.gain).map_err(|_| {
                    AnalysisCheckError::InvalidRecognition {
                        sale: sale.id.clone(),
                        proof: sale.proof,
                        reason: "allocation gains are not summable".into(),
                    }
                })?;
                gain_proofs.push(allocation.proof);
                previous_conservation
                    .insert(allocation.lot_id.clone(), allocation.conservation_proof);

                *remaining_quantity
                    .get_mut(&allocation.lot_id)
                    .expect("lot was present in the initial inventory") =
                    available_quantity.checked_sub(&allocation.quantity.number);
                *remaining_basis
                    .get_mut(&allocation.lot_id)
                    .expect("lot was present in the initial inventory") =
                    available_basis.checked_sub(&allocation.basis.number);
            }

            let aggregate = sale.recognized.as_ref().expect("checked above");
            let aggregate_lots = sale
                .allocations
                .iter()
                .map(|allocation| allocation.lot_id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            if aggregate.lot_id != aggregate_lots
                || aggregate.quantity != total_quantity
                || aggregate.proceeds != total_proceeds
                || aggregate.basis != total_basis
                || aggregate.gain != total_gain
                || aggregate.quantity != sale.quantity
                || aggregate.proceeds != sale.proceeds
                || aggregate.proof != sale.proof
            {
                return Err(AnalysisCheckError::InvalidRecognition {
                    sale: sale.id.clone(),
                    proof: sale.proof,
                    reason: "recognized aggregate is not the exact sum of allocations".into(),
                });
            }
            if !self.proof.roots.contains(&sale.proof) {
                return Err(AnalysisCheckError::InvalidRecognition {
                    sale: sale.id.clone(),
                    proof: sale.proof,
                    reason: "recognition is not a proof root".into(),
                });
            }
            let sale_observation =
                sale_observation.ok_or_else(|| AnalysisCheckError::InvalidRecognition {
                    sale: sale.id.clone(),
                    proof: sale.proof,
                    reason: "recognized sale has no sale observation edge".into(),
                })?;
            let allocation_proofs = sale
                .allocations
                .iter()
                .map(|allocation| allocation.allocation_proof)
                .collect::<Vec<_>>();
            let conservation_proofs = sale
                .allocations
                .iter()
                .map(|allocation| allocation.conservation_proof)
                .collect::<Vec<_>>();
            check_recognition_node(
                self,
                sale,
                sale_observation,
                &allocation_proofs,
                &gain_proofs,
                &conservation_proofs,
            )?;
        }

        Ok(())
    }

    pub fn sale(&self, id: &str) -> Option<&SaleAnalysis> {
        self.sales.iter().find(|sale| sale.id == id)
    }

    pub fn recognized_gain(&self, id: &str) -> Option<&ConditionalGain> {
        let sale = self.sale(id)?;
        sale.recognized.as_ref()
    }

    pub fn dependency_roots(&self, goal: &str) -> &[ProofId] {
        self.dependencies
            .get(goal)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

impl fmt::Display for RecognitionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recognized => f.write_str("recognized"),
            Self::Ambiguous { candidates } => {
                write!(f, "ambiguous lot ({})", candidates.join(", "))
            }
            Self::Conflict {
                policy_lot,
                decision_lot,
            } => {
                write!(
                    f,
                    "policy selects {policy_lot}, decision selects {decision_lot}"
                )
            }
            Self::MissingLot => f.write_str("missing lot"),
            Self::InvalidAmount => f.write_str("invalid amount"),
        }
    }
}

/// Analyze one parsed ledger inside the workspace boundary. Economic events
/// are ordered by date, stable identity, and explicit policy; no map iteration
/// or hash-map randomization affects them. External callers must use
/// [`crate::workspace::Workspace::analyze_commit`] so the result is bound to
/// an immutable source commit.
#[cfg(test)]
pub(crate) fn analyze(ledger: &Ledger) -> Analysis {
    analyze_with_registry(ledger, &PolicyRegistry::builtins())
}

/// Analyze a ledger against an explicit package registry. The default
/// [`analyze`] path uses the built-in registry; this entry point is the
/// community-package boundary and makes malformed/unsupported packages
/// testable without adding kernel branches.
pub(crate) fn analyze_with_registry(ledger: &Ledger, registry: &PolicyRegistry) -> Analysis {
    let book = ledger.book.as_str().to_owned();
    let mut proof = Proof::new();
    let mut lots = Vec::new();
    let mut sales_raw = Vec::new();
    let mut quotes = Vec::new();
    let mut positions = Vec::new();
    let mut settlements = Vec::new();
    let mut policy = None;
    let mut policy_proof = None;
    let mut policy_uses = Vec::new();
    let mut decisions = Vec::new();
    let mut decisions_by_sale: BTreeMap<String, Vec<(String, ProofId)>> = BTreeMap::new();
    let mut issues = Vec::new();

    for (index, statement) in ledger.forms.iter().enumerate() {
        match statement {
            LedgerForm::Buy(buy) => {
                let source = source_key("buy", &buy_material(buy));
                let mut metadata = metadata_for(&source, "lot", Some(index + 1));
                metadata.insert("occurrence".into(), buy.occurrence.as_str().into());
                let node = Node::new(
                    format!("lot {}/{}", buy.label, source),
                    Operation::LotObservation {
                        lot: buy.label.clone(),
                        source: source.clone(),
                    },
                    Vec::new(),
                    metadata,
                );
                let proof_id = proof.insert(node);
                if let Some(lot) = make_lot(buy, proof_id, index + 1) {
                    lots.push(lot);
                } else {
                    issues.push(Issue {
                        code: IssueCode::InvalidAmount,
                        message: format!(
                            "buy `{}` has an unresolved or incompatible quantity",
                            buy.label
                        ),
                        sale: None,
                        proof: Some(proof_id),
                    });
                }
            }
            LedgerForm::Sell(sell) => sales_raw.push((index, sell.clone())),
            LedgerForm::Quote(quote) => {
                let source = source_key("quote", &quote_material(quote));
                let node = proof.insert(Node::new(
                    format!("quote {}/{}", quote.label, source),
                    Operation::Observation {
                        source: source.clone(),
                    },
                    Vec::new(),
                    metadata_for(&source, "quote", Some(index + 1)),
                ));
                if let Some(view) = make_quote(quote, node) {
                    quotes.push(view);
                }
            }
            LedgerForm::ObservePosition(observation) => {
                let source = source_key(
                    "position",
                    &format!(
                        "{}|{}",
                        observation.account,
                        canonical_model_quantity(&observation.quantity)
                    ),
                );
                let node = proof.insert(Node::new(
                    format!("observed position {}/{}", observation.account, source),
                    Operation::Observation {
                        source: source.clone(),
                    },
                    Vec::new(),
                    metadata_for(&source, "position", Some(index + 1)),
                ));
                if let Some(quantity) = quantity_of(&observation.quantity) {
                    positions.push(PositionView {
                        account: observation.account.as_str().to_owned(),
                        quantity,
                        proof: node,
                        status: ObservationStatus::Observed,
                    });
                }
            }
            LedgerForm::ObserveSettlement(observation) => {
                let reference = observation.sale.as_str();
                let source = source_key(
                    "settlement",
                    &format!(
                        "{}|{}|{}",
                        reference,
                        canonical_model_quantity(&observation.amount),
                        observation
                            .into
                            .as_ref()
                            .map(|account| account.as_str())
                            .unwrap_or("?")
                    ),
                );
                let node = proof.insert(Node::new(
                    format!("observed settlement {reference}/{source}"),
                    Operation::Observation {
                        source: source.clone(),
                    },
                    Vec::new(),
                    metadata_for(&source, "settlement", Some(index + 1)),
                ));
                if let Some(quantity) = quantity_of(&observation.amount) {
                    settlements.push(SettlementView {
                        reference: reference.to_owned(),
                        quantity,
                        proof: node,
                        into: observation
                            .into
                            .as_ref()
                            .map(|account| account.as_str().to_owned()),
                        status: ObservationStatus::Observed,
                    });
                }
            }
            LedgerForm::UsePolicy(use_policy) => {
                let policy_name = use_policy.policy.as_str().to_owned();
                let source = source_key(
                    "policy",
                    &format!("{}|{}", use_policy.book, use_policy.policy),
                );
                let mut metadata = metadata_for(&source, "policy", Some(index + 1));
                let package = registry.get(&policy_name);
                if let Some(package) = package {
                    metadata.insert("policy-hash".into(), package.hash().to_string());
                    // This records the exact executable body next to its
                    // content address. A proof reader can verify the hash and
                    // independently re-run the typed evaluator.
                    if let Ok(program) = package.compile() {
                        metadata.insert("policy-program".into(), program.to_string());
                    }
                }
                if use_policy.book.as_str() != book {
                    let current_proof = proof.insert(Node::new(
                        format!(
                            "policy {} for wrong book {}",
                            use_policy.policy, use_policy.book
                        ),
                        Operation::Policy {
                            subject: use_policy.book.as_str().to_owned(),
                            policy: policy_name.clone(),
                            answer: "ignored:wrong-book".into(),
                        },
                        Vec::new(),
                        metadata,
                    ));
                    issues.push(Issue {
                        code: IssueCode::UnknownPolicy,
                        message: format!(
                            "policy `{policy_name}` targets book `{}` but analysis is for `{book}`",
                            use_policy.book
                        ),
                        sale: None,
                        proof: Some(current_proof),
                    });
                } else {
                    let current_proof = proof.insert(Node::new(
                        format!("policy {}", use_policy.policy),
                        Operation::Policy {
                            subject: book.clone(),
                            policy: policy_name.clone(),
                            answer: "active".into(),
                        },
                        Vec::new(),
                        metadata,
                    ));
                    policy_uses.push((policy_name, current_proof));
                }
            }
            LedgerForm::Decide(decision) => {
                let value = decision.lot.as_str().to_owned();
                let sale_name = decision.sale.as_str().to_owned();
                decisions.push(value.clone());
                let source = source_key("decision", &format!("{}|{}", decision.sale, decision.lot));
                let current_proof = proof.insert(Node::new(
                    format!("decision {} lot {}", decision.sale, decision.lot),
                    Operation::Decision {
                        subject: sale_name.clone(),
                        answer: value.clone(),
                    },
                    Vec::new(),
                    metadata_for(&source, "decision", Some(index + 1)),
                ));
                let choices = decisions_by_sale.entry(sale_name.clone()).or_default();
                if choices.iter().any(|(answer, _)| answer == &value) {
                    // Repeating the same decision is harmless evidence; it
                    // remains separately locatable through its source
                    // metadata but does not create a false conflict.
                } else {
                    choices.push((value, current_proof));
                }
            }
        }
    }

    // Decisions are set-like evidence. Canonicalize answers before deriving
    // conflicts so their result does not depend on source order, and make a
    // decision for a nonexistent sale an explicit rooted issue rather than
    // silently dropping it.
    for choices in decisions_by_sale.values_mut() {
        choices.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    }
    decisions.sort();
    decisions.dedup();
    // Inventory consumption follows economic time, never textual placement.
    // Labels make independent events deterministic, but may not decide which
    // same-day sale gets scarce shared inventory. Such sales are blocked below
    // unless distinct decisions make their allocations independent.
    sales_raw.sort_by(|left, right| {
        left.1
            .date
            .cmp(&right.1.date)
            .then_with(|| left.1.label.cmp(&right.1.label))
    });
    let sale_names = sales_raw
        .iter()
        .map(|(_, sale)| sale.label.as_str())
        .collect::<BTreeSet<_>>();
    for (sale_name, choices) in &decisions_by_sale {
        if choices.len() > 1 {
            let conflict_proof = proof.insert(Node::new(
                format!("decision conflict {sale_name}"),
                Operation::Conflict {
                    subject: sale_name.clone(),
                    reason: "multiple decisions select different lots".into(),
                },
                choices.iter().map(|(_, proof_id)| *proof_id).collect(),
                metadata_for(sale_name, "decision-conflict", None),
            ));
            issues.push(Issue {
                code: IssueCode::DecisionConflict,
                message: format!(
                    "decisions for sale `{sale_name}` select {}",
                    choices
                        .iter()
                        .map(|(lot, _)| format!("`{lot}`"))
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
                sale: Some(sale_name.clone()),
                proof: Some(conflict_proof),
            });
        }
        if !sale_names.contains(sale_name.as_str()) {
            let target_proof = proof.insert(Node::new(
                format!("unknown decision target {sale_name}"),
                Operation::Conflict {
                    subject: sale_name.clone(),
                    reason: "decision names a sale that is not present".into(),
                },
                choices.iter().map(|(_, proof_id)| *proof_id).collect(),
                metadata_for(sale_name, "unknown-decision-target", None),
            ));
            issues.push(Issue {
                code: IssueCode::PolicyDecisionConflict,
                message: format!("decision targets unknown sale `{sale_name}`"),
                sale: Some(sale_name.clone()),
                proof: Some(target_proof),
            });
        }
    }

    let mut simultaneous_sales = BTreeMap::<(model::Date, String, String), Vec<String>>::new();
    for (_, sale) in &sales_raw {
        if let Some((account, asset, _)) = holding_of_model(sale) {
            simultaneous_sales
                .entry((sale.date, account, asset))
                .or_default()
                .push(sale.label.clone());
        }
    }
    let mut unresolved_simultaneous = BTreeMap::<String, Vec<String>>::new();
    for labels in simultaneous_sales
        .into_values()
        .filter(|labels| labels.len() > 1)
    {
        let selected = labels
            .iter()
            .filter_map(|label| {
                let sale = sales_raw
                    .iter()
                    .find(|(_, sale)| &sale.label == label)
                    .map(|(_, sale)| sale)?;
                if let LotSelector::Explicit(lot) = &sale.lot {
                    return Some(lot.as_str().to_owned());
                }
                let choices = decisions_by_sale.get(label)?;
                (choices.len() == 1).then(|| choices[0].0.clone())
            })
            .collect::<BTreeSet<_>>();
        let explicitly_disjoint = selected.len() == labels.len();
        if !explicitly_disjoint {
            for label in &labels {
                unresolved_simultaneous.insert(label.clone(), labels.clone());
            }
        }
    }

    // Resolve package names through the registry only after collecting all
    // uses. Sorting makes repeated policy declarations source-order
    // invariant, and a conflict disables execution instead of letting the
    // first source line choose a winner.
    policy_uses.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut policy_program: Option<SelectionProgram> = None;
    let mut policy_invalid = false;
    let policy_names = policy_uses
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    if let Some(name) = policy_names.iter().next() {
        policy = Some(name.clone());
        policy_proof = policy_uses
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, proof_id)| *proof_id);
    }
    if policy_names.len() > 1 {
        policy_invalid = true;
        let conflict_proof = proof.insert(Node::new(
            format!("policy conflict for {book}"),
            Operation::Conflict {
                subject: book.clone(),
                reason: "multiple policy packages apply".into(),
            },
            policy_uses.iter().map(|(_, proof_id)| *proof_id).collect(),
            metadata_for(&book, "policy-conflict", None),
        ));
        issues.push(Issue {
            code: IssueCode::PolicyDecisionConflict,
            message: format!(
                "policies {} both apply to book `{book}`",
                policy_names
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            sale: None,
            proof: Some(conflict_proof),
        });
    }
    for name in &policy_names {
        let proof_id = policy_uses
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, proof_id)| *proof_id);
        let Some(package) = registry.get(name) else {
            policy_invalid = true;
            issues.push(Issue {
                code: IssueCode::UnknownPolicy,
                message: format!("policy `{name}` has no registered package"),
                sale: None,
                proof: proof_id,
            });
            continue;
        };
        if let Err(error) = package.validate() {
            policy_invalid = true;
            issues.push(Issue {
                code: IssueCode::UnknownPolicy,
                message: format!(
                    "policy package `{name}` ({}) is not executable: {error}",
                    package.hash()
                ),
                sale: None,
                proof: proof_id,
            });
        }
    }
    if !policy_invalid
        && policy_names.len() == 1
        && let Some(name) = policy_names.iter().next()
    {
        policy_program = registry
            .get(name)
            .and_then(|package| package.compile().ok());
    }

    // Quote statuses are independent from recognition.  A sale in USD does
    // not become blocked merely because a valuation quote is contradictory.
    let mut quote_groups: BTreeMap<String, Vec<&QuoteView>> = BTreeMap::new();
    for quote in &quotes {
        quote_groups
            .entry(quote_group_key(quote))
            .or_default()
            .push(quote);
    }
    let mut quote_status = BTreeMap::new();
    for (group, group_quotes) in quote_groups {
        let mut ids = group_quotes
            .iter()
            .map(|quote| quote.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        // Rates are values, not source spellings.  `1 ABC = 52 USD` and
        // `2 ABC = 104 USD` are the same quote; compare by cross
        // multiplication so the result remains exact for rationals and does
        // not depend on decimal scale.
        let rates_agree = group_quotes.first().is_none_or(|reference| {
            group_quotes.iter().all(|quote| {
                quote.base.number.checked_mul(&reference.quote.number)
                    == reference.base.number.checked_mul(&quote.quote.number)
            })
        });
        if !rates_agree {
            let proof = proof.insert(Node::new(
                format!("quote conflict {group}"),
                Operation::Conflict {
                    subject: group.clone(),
                    reason: "multiple effective rates".into(),
                },
                group_quotes.iter().map(|quote| quote.proof).collect(),
                metadata_for(&group, "quote-conflict", None),
            ));
            quote_status.insert(
                group.clone(),
                QuoteStatus::Ambiguous {
                    quote_ids: ids.clone(),
                },
            );
            issues.push(Issue {
                code: IssueCode::AmbiguousQuote,
                message: format!("quotes {} disagree for {group}", ids.join(", ")),
                sale: None,
                proof: Some(proof),
            });
        } else {
            quote_status.insert(group, QuoteStatus::Unique);
        }
    }

    let mut sales = Vec::new();
    let mut journal = Vec::new();
    let mut dependencies: BTreeMap<String, Vec<ProofId>> = BTreeMap::new();
    let mut invalidations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Inventory is consumed once, in economic event order. Lots themselves remain
    // immutable evidence; this map is the derived remaining quantity shared
    // by every sale in the analysis.
    let mut remaining: BTreeMap<String, Exact> = lots
        .iter()
        .map(|lot| (lot.id.clone(), lot.quantity.number.clone()))
        .collect();
    let mut remaining_basis: BTreeMap<String, Exact> = lots
        .iter()
        .map(|lot| (lot.id.clone(), lot.basis.number.clone()))
        .collect();
    let mut remaining_proofs: BTreeMap<String, Vec<ProofId>> = lots
        .iter()
        .map(|lot| (lot.id.clone(), vec![lot.proof]))
        .collect();
    let mut remaining_conservation: BTreeMap<String, ProofId> = BTreeMap::new();

    // Observations are evidence, not balances.  Preserve every source node,
    // but explicitly surface contradictory values and never let one
    // observation silently win by source order.
    let mut position_groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, position) in positions.iter().enumerate() {
        position_groups
            .entry((
                position.account.clone(),
                position
                    .quantity
                    .unit
                    .as_ref()
                    .map(|unit| unit.as_str().to_owned())
                    .unwrap_or_else(|| "?".into()),
            ))
            .or_default()
            .push(index);
    }
    for ((account, asset), indexes) in position_groups {
        let distinct = indexes
            .iter()
            .map(|index| positions[*index].quantity.canonical())
            .collect::<BTreeSet<_>>();
        if distinct.len() > 1 {
            let conflict_proof = proof.insert(Node::new(
                format!("position conflict {account}:{asset}"),
                Operation::Conflict {
                    subject: format!("{account}:{asset}"),
                    reason: "multiple observed positions disagree".into(),
                },
                indexes
                    .iter()
                    .map(|index| positions[*index].proof)
                    .collect(),
                metadata_for(&account, "position-conflict", None),
            ));
            for index in &indexes {
                positions[*index].status = ObservationStatus::Conflict;
            }
            issues.push(Issue {
                code: IssueCode::PositionConflict,
                message: format!("position observations for `{account}:{asset}` conflict"),
                sale: None,
                proof: Some(conflict_proof),
            });
        }
    }

    let mut settlement_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, settlement) in settlements.iter().enumerate() {
        if !sale_names.contains(settlement.reference.as_str()) {
            issues.push(Issue {
                code: IssueCode::SettlementConflict,
                message: format!(
                    "settlement observation `{}` does not match a sale",
                    settlement.reference
                ),
                sale: Some(settlement.reference.clone()),
                proof: Some(settlement.proof),
            });
        }
        settlement_groups
            .entry(settlement.reference.clone())
            .or_default()
            .push(index);
    }
    let mut settlement_conflicts = BTreeSet::new();
    for (reference, indexes) in settlement_groups {
        let distinct = indexes
            .iter()
            .map(|index| {
                format!(
                    "{}|{}",
                    settlements[*index].quantity.canonical(),
                    settlements[*index].into.as_deref().unwrap_or("?")
                )
            })
            .collect::<BTreeSet<_>>();
        if indexes.len() > 1 {
            settlement_conflicts.insert(reference.clone());
            let reason = if distinct.len() > 1 {
                "multiple observed settlements disagree"
            } else {
                "multiple settlement observations require explicit allocation"
            };
            let conflict_proof = proof.insert(Node::new(
                format!("settlement conflict {reference}"),
                Operation::Conflict {
                    subject: reference.clone(),
                    reason: reason.into(),
                },
                indexes
                    .iter()
                    .map(|index| settlements[*index].proof)
                    .collect(),
                metadata_for(&reference, "settlement-conflict", None),
            ));
            for index in &indexes {
                settlements[*index].status = ObservationStatus::Conflict;
            }
            issues.push(Issue {
                code: IssueCode::SettlementConflict,
                message: format!("settlement observations for `{reference}` conflict"),
                sale: Some(reference),
                proof: Some(conflict_proof),
            });
        }
    }

    for (index, sell) in sales_raw {
        let source = source_key("sell", &sell_material(&sell));
        let sale_proof = proof.insert(Node::new(
            format!("sale {}/{}", sell.label, source),
            Operation::SaleObservation {
                sale: sell.label.clone(),
                source: source.clone(),
            },
            Vec::new(),
            metadata_for(&source, "sale", Some(index + 1)),
        ));
        let Some((account, asset, quantity)) = holding_of_model(&sell) else {
            let status = RecognitionStatus::InvalidAmount;
            issues.push(Issue {
                code: IssueCode::InvalidAmount,
                message: format!("sale `{}` has an unresolved holding", sell.label),
                sale: Some(sell.label.clone()),
                proof: Some(sale_proof),
            });
            sales.push(SaleAnalysis {
                id: sell.label.clone(),
                date: sell.date,
                account: sell.from.as_str().to_owned(),
                asset: "?asset".into(),
                quantity: Quantity::zero(),
                proceeds: quantity_of(&sell.proceeds).unwrap_or_else(zero_quantity),
                eligible_lots: Vec::new(),
                conditional_gains: Vec::new(),
                allocations: Vec::new(),
                recognized: None,
                selected_lots: Vec::new(),
                selected_lot: None,
                status,
                proof: sale_proof,
            });
            continue;
        };
        let proceeds = quantity_of(&sell.proceeds).unwrap_or_else(zero_quantity);
        let mut eligible = lots
            .iter()
            .filter(|lot| {
                lot.date <= sell.date
                    && lot.account == account
                    && lot.asset == asset
                    && lot.quantity.unit == quantity.unit
                    && remaining
                        .get(&lot.id)
                        .is_some_and(|available| !available.is_zero())
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.id.cmp(&right.id))
        });
        let eligible_ids = eligible
            .iter()
            .map(|lot| lot.id.clone())
            .collect::<Vec<_>>();
        let mut conditionals = Vec::new();
        for lot in &eligible {
            let available = remaining
                .get(&lot.id)
                .cloned()
                .unwrap_or_else(|| Exact::from(0i64));
            if available >= quantity.number
                && let Some(available_basis) = remaining_basis.get(&lot.id)
                && let Some(remaining_inputs) = remaining_proofs.get(&lot.id)
                && let Some(gain) = conditional_gain(
                    RemainingLot {
                        lot,
                        quantity: &available,
                        basis: available_basis,
                        proof_inputs: remaining_inputs,
                    },
                    SaleSlice {
                        quantity: &quantity,
                        proceeds: &proceeds,
                        proof: sale_proof,
                    },
                    &mut proof,
                )
            {
                conditionals.push(gain);
            }
        }

        let explicit_lot = match &sell.lot {
            LotSelector::Explicit(lot) => Some(lot.as_str().to_owned()),
            LotSelector::Hole(_) => None,
        };
        let sale_decisions = decisions_by_sale
            .get(&sell.label)
            .cloned()
            .unwrap_or_default();
        let decision_lots = sale_decisions
            .iter()
            .map(|(lot, _)| lot.clone())
            .collect::<Vec<_>>();
        let decision_conflict = decision_lots.len() > 1;
        let decision_lot = (decision_lots.len() == 1).then(|| decision_lots[0].clone());
        let decision_proofs = sale_decisions
            .iter()
            .map(|(_, proof)| *proof)
            .collect::<Vec<_>>();
        let requested_lot = explicit_lot.clone().or_else(|| decision_lot.clone());
        let requested_is_invalid = requested_lot
            .as_ref()
            .is_some_and(|lot| !eligible_ids.contains(lot));
        let policy_selection = policy_program.map(|program| {
            let candidates = eligible
                .iter()
                .map(|lot| LotCandidate::new(lot.id.clone(), lot.date))
                .collect::<Vec<_>>();
            (
                program,
                program.evaluate(&candidates),
                program.ordered(&candidates),
            )
        });
        let policy_lot = policy_selection
            .as_ref()
            .and_then(|(_, selection, _)| match selection {
                Selection::Unique(lot) => Some(lot.clone()),
                Selection::None | Selection::Ambiguous(_) => None,
            });
        let policy_order = policy_selection
            .as_ref()
            .map(|(_, _, ordered)| ordered.iter().map(|lot| lot.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let policy_application_proof = policy_selection.as_ref().map(|(program, selection, _)| {
            let answer = match selection {
                Selection::None => "none".to_owned(),
                Selection::Unique(lot) => lot.clone(),
                Selection::Ambiguous(lots) => format!("ambiguous:{}", lots.join(",")),
            };
            let mut metadata = metadata_for(&source, "policy-selection", None);
            if let Some(name) = policy.as_deref()
                && let Some(package) = registry.get(name)
            {
                metadata.insert("policy-hash".into(), package.hash().to_string());
                metadata.insert("policy-program".into(), program.to_string());
            }
            proof.insert(Node::new(
                format!("policy selection {}/{}", sell.label, source),
                Operation::Policy {
                    subject: sell.label.clone(),
                    policy: policy.clone().unwrap_or_else(|| "unknown".into()),
                    answer,
                },
                std::iter::once(policy_proof)
                    .flatten()
                    .chain(eligible.iter().map(|lot| lot.proof))
                    .collect(),
                metadata,
            ))
        });
        let (mut selected_lot, mut status, mut planned_lots) = if let Some(peers) =
            unresolved_simultaneous.get(&sell.label)
        {
            issues.push(Issue {
                code: IssueCode::AmbiguousLot,
                message: format!(
                    "same-day sales {} compete for shared inventory; add distinct lot selections",
                    peers
                        .iter()
                        .map(|label| format!("`{label}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                sale: Some(sell.label.clone()),
                proof: Some(sale_proof),
            });
            (
                None,
                RecognitionStatus::Ambiguous {
                    candidates: peers.clone(),
                },
                Vec::new(),
            )
        } else if decision_conflict {
            // The conflict proof created while reading decisions is rooted,
            // and no one decision is allowed to leak into selection.
            (
                None,
                RecognitionStatus::Ambiguous {
                    candidates: decision_lots.clone(),
                },
                Vec::new(),
            )
        } else if let Some((explicit_lot, decision_lot)) = explicit_lot
            .clone()
            .zip(decision_lot.clone())
            .filter(|(explicit, decision)| explicit != decision)
        {
            let proof_id = proof.insert(Node::new(
                format!("lot conflict {}/{}", sell.label, source),
                Operation::Conflict {
                    subject: sell.label.clone(),
                    reason: "explicit lot and decision disagree".into(),
                },
                std::iter::once(sale_proof)
                    .chain(decision_proofs.iter().copied())
                    .collect(),
                metadata_for(&source, "lot-conflict", None),
            ));
            issues.push(Issue {
                code: IssueCode::PolicyDecisionConflict,
                message: format!(
                    "explicit lot `{explicit_lot}` and decision `{decision_lot}` disagree for sale `{}`",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
                proof: Some(proof_id),
            });
            (
                None,
                RecognitionStatus::Conflict {
                    policy_lot: explicit_lot,
                    decision_lot,
                },
                Vec::new(),
            )
        } else if policy_invalid && !policy_names.is_empty() && requested_lot.is_none() {
            // An invalid package cannot silently degrade to the unique-lot
            // fallback. The package issue above is global and rooted at the
            // policy declaration; this sale remains explicitly blocked.
            (None, RecognitionStatus::MissingLot, Vec::new())
        } else {
            match (policy_lot.clone(), requested_lot.clone()) {
                (Some(policy_lot), Some(decision_lot)) if policy_lot != decision_lot => {
                    let proof_id = proof.insert(Node::new(
                        format!("lot conflict {}/{}", sell.label, source),
                        Operation::Conflict {
                            subject: sell.label.clone(),
                            reason: "policy and decision disagree".into(),
                        },
                        std::iter::once(sale_proof)
                            .chain(policy_proof)
                            .chain(decision_proofs.iter().copied())
                            .chain(eligible.iter().map(|lot| lot.proof))
                            .collect(),
                        metadata_for(&source, "lot-conflict", None),
                    ));
                    issues.push(Issue { code: IssueCode::PolicyDecisionConflict, message: format!("policy selects `{policy_lot}` but decision selects `{decision_lot}` for sale `{}`", sell.label), sale: Some(sell.label.clone()), proof: Some(proof_id) });
                    (
                        None,
                        RecognitionStatus::Conflict {
                            policy_lot,
                            decision_lot,
                        },
                        Vec::new(),
                    )
                }
                (Some(policy_lot), Some(decision_lot)) if requested_is_invalid => {
                    let proof_id = proof.insert(Node::new(
                        format!("invalid lot decision {}/{}", sell.label, source),
                        Operation::Conflict {
                            subject: sell.label.clone(),
                            reason: "decision names an ineligible lot".into(),
                        },
                        std::iter::once(sale_proof)
                            .chain(policy_proof)
                            .chain(decision_proofs.iter().copied())
                            .collect(),
                        metadata_for(&source, "invalid-decision", None),
                    ));
                    issues.push(Issue {
                        code: IssueCode::PolicyDecisionConflict,
                        message: format!(
                            "lot decision `{decision_lot}` is not eligible for sale `{}`",
                            sell.label
                        ),
                        sale: Some(sell.label.clone()),
                        proof: Some(proof_id),
                    });
                    (
                        None,
                        RecognitionStatus::Conflict {
                            policy_lot,
                            decision_lot,
                        },
                        Vec::new(),
                    )
                }
                (Some(policy_lot), Some(requested_lot)) => (
                    Some(policy_lot),
                    RecognitionStatus::Recognized,
                    vec![requested_lot],
                ),
                (Some(policy_lot), None) => (
                    Some(policy_lot),
                    RecognitionStatus::Recognized,
                    policy_order.clone(),
                ),
                (None, Some(decision_lot)) if requested_is_invalid => {
                    issues.push(Issue {
                        code: IssueCode::MissingLot,
                        message: format!(
                            "lot decision `{decision_lot}` is not eligible for sale `{}`",
                            sell.label
                        ),
                        sale: Some(sell.label.clone()),
                        proof: Some(sale_proof),
                    });
                    (None, RecognitionStatus::MissingLot, Vec::new())
                }
                (None, Some(decision_lot)) => (
                    Some(decision_lot.clone()),
                    RecognitionStatus::Recognized,
                    vec![decision_lot],
                ),
                (None, None) if policy_program.is_some() => {
                    (None, RecognitionStatus::Recognized, policy_order.clone())
                }
                (None, None) if eligible.len() == 1 => (
                    eligible.first().map(|lot| lot.id.clone()),
                    RecognitionStatus::Recognized,
                    eligible
                        .first()
                        .map(|lot| vec![lot.id.clone()])
                        .unwrap_or_default(),
                ),
                (None, None) if eligible.is_empty() => {
                    issues.push(Issue {
                        code: IssueCode::MissingLot,
                        message: format!("no eligible lot for sale `{}`", sell.label),
                        sale: Some(sell.label.clone()),
                        proof: Some(sale_proof),
                    });
                    (None, RecognitionStatus::MissingLot, Vec::new())
                }
                (None, None) => {
                    issues.push(Issue {
                        code: IssueCode::AmbiguousLot,
                        message: format!(
                            "sale `{}` has multiple eligible lots; use a policy or decision",
                            sell.label
                        ),
                        sale: Some(sell.label.clone()),
                        proof: Some(sale_proof),
                    });
                    (
                        None,
                        RecognitionStatus::Ambiguous {
                            candidates: eligible_ids.clone(),
                        },
                        Vec::new(),
                    )
                }
            }
        };

        // A policy supplies an ordered stream, so a sale can consume a
        // partial first lot and continue into later lots.  An explicit
        // decision (or the legacy unique-lot fallback) is intentionally
        // restricted to the named lot.
        let mut allocation_specs = Vec::<(String, Exact)>::new();
        if status.is_complete() {
            let preserve_policy_ties = policy_program.is_some() && requested_lot.is_none();
            match plan_allocations(
                &planned_lots,
                &lots,
                &remaining,
                &quantity.number,
                preserve_policy_ties,
            ) {
                AllocationPlan::Complete(specs) => allocation_specs = specs,
                AllocationPlan::Insufficient { available } => {
                    issues.push(Issue {
                        code: IssueCode::InsufficientInventory,
                        message: format!(
                            "sale `{}` requires {} {}, but only {} {} remains in eligible lots",
                            sell.label, quantity.number, asset, available, asset
                        ),
                        sale: Some(sell.label.clone()),
                        proof: Some(sale_proof),
                    });
                    status = RecognitionStatus::MissingLot;
                    selected_lot = None;
                    planned_lots.clear();
                }
                AllocationPlan::Ambiguous { candidates } => {
                    issues.push(Issue {
                        code: IssueCode::AmbiguousLot,
                        message: format!(
                            "policy reaches tied lots {} for sale `{}`; add a specific decision",
                            candidates.join(", "),
                            sell.label
                        ),
                        sale: Some(sell.label.clone()),
                        proof: policy_application_proof,
                    });
                    status = RecognitionStatus::Ambiguous { candidates };
                    selected_lot = None;
                    planned_lots.clear();
                }
            }
        }
        if status.is_complete() && allocation_specs.is_empty() {
            issues.push(Issue {
                code: IssueCode::IncompatibleUnit,
                message: format!(
                    "sale `{}` proceeds and eligible lot basis use incompatible units",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
                proof: Some(sale_proof),
            });
            selected_lot = None;
            status = RecognitionStatus::InvalidAmount;
        }
        let mut allocations = Vec::new();
        if status.is_complete() {
            let mut valid = true;
            for (lot_id, allocated_quantity) in &allocation_specs {
                let Some(lot) = lots.iter().find(|lot| lot.id == *lot_id) else {
                    valid = false;
                    break;
                };
                let available_quantity = remaining
                    .get(lot_id)
                    .cloned()
                    .unwrap_or_else(|| Exact::from(0i64));
                let available_basis = remaining_basis
                    .get(lot_id)
                    .cloned()
                    .unwrap_or_else(|| Exact::from(0i64));
                let remaining_inputs = remaining_proofs
                    .get(lot_id)
                    .cloned()
                    .unwrap_or_else(|| vec![lot.proof]);
                let Some(allocation) = lot_allocation(
                    RemainingLot {
                        lot,
                        quantity: &available_quantity,
                        basis: &available_basis,
                        proof_inputs: &remaining_inputs,
                    },
                    allocated_quantity,
                    SaleSlice {
                        quantity: &quantity,
                        proceeds: &proceeds,
                        proof: sale_proof,
                    },
                    &sell.label,
                    &mut proof,
                ) else {
                    valid = false;
                    break;
                };
                let predecessor = remaining_conservation.get(lot_id).copied();
                let quantity_unit = lot
                    .quantity
                    .unit
                    .as_ref()
                    .expect("validated lot quantity has a unit")
                    .to_string();
                let mut conservation_inputs = vec![lot.proof, allocation.allocation_proof];
                if let Some(predecessor) = predecessor {
                    conservation_inputs.push(predecessor);
                }
                let mut conservation_metadata =
                    metadata_for(&lot.id, "inventory-conservation", None);
                conservation_metadata.insert("lot".into(), lot.id.clone());
                conservation_metadata
                    .insert("before".into(), available_quantity.canonical_string());
                conservation_metadata
                    .insert("consumed".into(), allocated_quantity.canonical_string());
                let after = available_quantity.checked_sub(allocated_quantity);
                conservation_metadata.insert("after".into(), after.canonical_string());
                conservation_metadata.insert("unit".into(), quantity_unit.clone());
                let conservation = proof.insert(Node::new(
                    format!("remaining {} in {}", after, lot.id),
                    Operation::InventoryConservation {
                        lot: lot.id.clone(),
                        lot_proof: lot.proof,
                        before: available_quantity.clone(),
                        consumed: allocated_quantity.clone(),
                        after,
                        unit: quantity_unit,
                        predecessor,
                        allocation: Some(allocation.allocation_proof),
                    },
                    conservation_inputs,
                    conservation_metadata,
                ));
                remaining_conservation.insert(lot_id.clone(), conservation);
                allocations.push(allocation.finish(conservation));
            }
            if !valid {
                issues.push(Issue {
                    code: IssueCode::IncompatibleUnit,
                    message: format!(
                        "sale `{}` proceeds and eligible lot basis use incompatible units",
                        sell.label
                    ),
                    sale: Some(sell.label.clone()),
                    proof: Some(sale_proof),
                });
                allocations.clear();
                allocation_specs.clear();
                selected_lot = None;
                status = RecognitionStatus::InvalidAmount;
            }
        }
        let selected_lots = allocations
            .iter()
            .map(|allocation| allocation.lot_id.clone())
            .collect::<Vec<_>>();
        if allocations.len() == 1 {
            selected_lot = selected_lots.first().cloned();
        } else if allocations.len() > 1 {
            selected_lot = None;
        }
        for (lot_id, allocated_quantity) in &allocation_specs {
            if let Some(current) = remaining.get_mut(lot_id) {
                *current = current.checked_sub(allocated_quantity);
            }
            if let Some(allocation) = allocations
                .iter()
                .find(|allocation| &allocation.lot_id == lot_id)
                && let Some(current) = remaining_basis.get_mut(lot_id)
            {
                *current = current.checked_sub(&allocation.basis.number);
            }
            if let Some(proofs) = remaining_proofs.get_mut(lot_id)
                && let Some(allocation) = allocations
                    .iter()
                    .find(|allocation| &allocation.lot_id == lot_id)
            {
                proofs.push(allocation.proof);
            }
        }
        let sale_result_proof = if !allocations.is_empty() {
            let mut inputs = vec![sale_proof];
            // The recognition certificate consumes typed allocation nodes
            // directly.  Gain arithmetic nodes remain in the explanation
            // branch, but are not trusted as the aggregate payload.
            inputs.extend(
                allocations
                    .iter()
                    .map(|allocation| allocation.allocation_proof),
            );
            inputs.extend(allocations.iter().map(|allocation| allocation.proof));
            inputs.extend(
                allocations
                    .iter()
                    .map(|allocation| allocation.conservation_proof),
            );
            if let Some(policy_proof) = policy_proof {
                inputs.push(policy_proof);
            }
            if let Some(policy_application_proof) = policy_application_proof {
                inputs.push(policy_application_proof);
            }
            inputs.extend(decision_proofs.iter().copied());
            if let Some(settlement) = matching_settlement(&settlements, &sell.label, &proceeds) {
                inputs.push(settlement.proof);
            }
            let id = proof.insert(Node::new(
                format!("gain {}/{}", sell.label, source),
                Operation::Recognition {
                    sale: sell.label.clone(),
                    sale_proof,
                    quantity: quantity.number.clone(),
                    proceeds: proceeds.number.clone(),
                    basis: allocations
                        .iter()
                        .fold(Exact::from(0i64), |total, allocation| {
                            total.checked_add(&allocation.basis.number)
                        }),
                    gain: allocations
                        .iter()
                        .fold(Exact::from(0i64), |total, allocation| {
                            total.checked_add(&allocation.gain.number)
                        }),
                    quantity_unit: quantity
                        .unit
                        .as_ref()
                        .expect("validated sale quantity has a unit")
                        .to_string(),
                    value_unit: proceeds
                        .unit
                        .as_ref()
                        .expect("validated sale proceeds have a unit")
                        .to_string(),
                },
                inputs,
                metadata_for(&source, "recognition", None),
            ));
            // A journal is an accepted cash projection, not a guess.  The
            // receiving account must come from a matching settlement
            // observation; never invent `proceeds:<sale>`.
            if !policy_invalid
                && let Some(settlement) = matching_settlement(&settlements, &sell.label, &proceeds)
                && let Some(into) = settlement.into.as_deref()
                && let Some(total) = aggregate_allocations(&allocations, id)
            {
                journal.push(make_journal(&sell.label, into, &account, &asset, &total));
            }
            id
        } else {
            sale_proof
        };
        let recognized = if status.is_complete() {
            aggregate_allocations(&allocations, sale_result_proof)
        } else {
            None
        };
        let mut goal_roots = std::iter::once(sale_result_proof)
            .chain(eligible.iter().map(|lot| lot.proof))
            .collect::<Vec<_>>();
        goal_roots.extend(conditionals.iter().map(|conditional| conditional.proof));
        if let Some(policy_proof) = policy_proof {
            goal_roots.push(policy_proof);
        }
        if let Some(policy_application_proof) = policy_application_proof {
            goal_roots.push(policy_application_proof);
        }
        goal_roots.extend(decision_proofs.iter().copied());
        dependencies.insert(format!("gain:{}", sell.label), goal_roots);
        invalidations
            .entry(source)
            .or_default()
            .push(format!("gain:{}", sell.label));
        sales.push(SaleAnalysis {
            id: sell.label,
            date: sell.date,
            account,
            asset,
            quantity,
            proceeds,
            eligible_lots: eligible_ids,
            conditional_gains: conditionals,
            allocations,
            recognized,
            selected_lots,
            selected_lot,
            status,
            proof: sale_result_proof,
        });
    }

    // An observation becomes reconciled only when authored economic events
    // independently reproduce it.
    for position in &mut positions {
        if position.status == ObservationStatus::Conflict {
            continue;
        }
        let mut calculated = Exact::from(0i64);
        let mut inputs = vec![position.proof];
        for lot in &lots {
            if lot.account == position.account
                && position
                    .quantity
                    .unit
                    .as_ref()
                    .is_some_and(|unit| lot.asset == unit.as_str())
            {
                calculated = calculated.checked_add(&lot.quantity.number);
                inputs.push(lot.proof);
            }
        }
        for sale in &sales {
            // A sale that is still ambiguous, conflicted, or otherwise
            // blocked is not an accepted disposal. Subtracting it here would
            // make an observed position look reconciled by an unresolved
            // choice, which is a materially false balance claim.
            if sale.status.is_complete()
                && sale.account == position.account
                && position
                    .quantity
                    .unit
                    .as_ref()
                    .is_some_and(|unit| sale.asset == unit.as_str())
            {
                calculated = calculated.checked_sub(&sale.quantity.number);
                inputs.push(sale.proof);
            }
        }
        if inputs.len() == 1 {
            continue;
        }
        let operation = if calculated == position.quantity.number {
            position.status = ObservationStatus::Reconciled;
            Operation::Derive {
                rule: "reconcile-position".into(),
            }
        } else {
            position.status = ObservationStatus::Conflict;
            issues.push(Issue {
                code: IssueCode::PositionConflict,
                message: format!(
                    "observed position `{}` is {} {}, but authored events imply {} {}",
                    position.account,
                    position.quantity.number,
                    position
                        .quantity
                        .unit
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "?".into()),
                    calculated,
                    position
                        .quantity
                        .unit
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "?".into())
                ),
                sale: None,
                proof: Some(position.proof),
            });
            Operation::Conflict {
                subject: position.account.clone(),
                reason: "observed position disagrees with authored events".into(),
            }
        };
        position.proof = proof.insert(Node::new(
            format!("position reconciliation {}", position.account),
            operation,
            inputs,
            metadata_for(&position.account, "position-reconciliation", None),
        ));
    }

    for settlement in &mut settlements {
        if settlement.status == ObservationStatus::Conflict {
            continue;
        }
        let Some(sale) = sales.iter().find(|sale| sale.id == settlement.reference) else {
            continue;
        };
        if sale.proceeds == settlement.quantity {
            settlement.status = ObservationStatus::Reconciled;
            settlement.proof = proof.insert(Node::new(
                format!("settlement reconciliation {}", settlement.reference),
                Operation::Derive {
                    rule: "reconcile-settlement".into(),
                },
                vec![settlement.proof, sale.proof],
                metadata_for(&settlement.reference, "settlement-reconciliation", None),
            ));
        } else {
            settlement.status = ObservationStatus::Conflict;
            issues.push(Issue {
                code: IssueCode::SettlementConflict,
                message: format!(
                    "observed settlement `{}` is {}, but sale proceeds are {}",
                    settlement.reference,
                    settlement.quantity.canonical(),
                    sale.proceeds.canonical()
                ),
                sale: Some(settlement.reference.clone()),
                proof: Some(settlement.proof),
            });
        }
    }

    for position in &positions {
        dependencies
            .entry(format!("position:{}", position.account))
            .or_default()
            .push(position.proof);
    }
    for settlement in &settlements {
        dependencies
            .entry(format!("settlement:{}", settlement.reference))
            .or_default()
            .push(settlement.proof);
    }
    // Materialize the reverse edge from every stable proof dependency.  A
    // caller can now invalidate a goal from its source occurrence without
    // knowing whether that occurrence was a sale, acquisition, policy, or
    // decision.
    for (goal, roots) in &dependencies {
        for root in roots {
            if let Some(source) = proof
                .node(*root)
                .and_then(|node| node.metadata.get("invalidation"))
            {
                invalidations
                    .entry(source.clone())
                    .or_default()
                    .push(goal.clone());
            }
        }
    }
    for roots in dependencies.values_mut() {
        roots.sort();
        roots.dedup();
    }
    for goals in invalidations.values_mut() {
        goals.sort();
        goals.dedup();
    }
    for roots in dependencies.values() {
        for root in roots {
            proof.root(*root);
        }
    }
    for issue in &issues {
        if let Some(root) = issue.proof {
            proof.root(root);
        }
    }
    issues.sort_by(|left, right| {
        left.sale
            .cmp(&right.sale)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
            .then_with(|| left.proof.cmp(&right.proof))
    });
    Analysis {
        book,
        policy,
        decisions,
        lots,
        sales,
        quotes,
        quote_status,
        positions,
        settlements,
        journal,
        issues,
        proof,
        dependencies,
        invalidations,
    }
}

fn make_lot(buy: &model::Buy, proof: ProofId, source_line: usize) -> Option<Lot> {
    let asset = buy.quantity.unit.as_ref()?.as_str().to_owned();
    let quantity = quantity_of(&buy.quantity)?;
    let account = buy.into.as_str().to_owned();
    let consideration = quantity_of(&buy.cost)?;
    let fee = match &buy.fee {
        Some(fee) => Some(quantity_of(fee)?),
        None => None,
    };
    if consideration.unit
        != fee
            .as_ref()
            .map(|fee| fee.unit.clone())
            .unwrap_or_else(|| consideration.unit.clone())
    {
        return None;
    }
    let basis_amount = consideration.number.checked_add(
        &fee.as_ref()
            .map(|fee| fee.number.clone())
            .unwrap_or_else(|| Exact::from(0i64)),
    );
    Some(Lot {
        id: buy.label.clone(),
        date: buy.date,
        account,
        asset,
        quantity,
        consideration: consideration.clone(),
        fee,
        basis: Quantity::new(basis_amount, consideration.unit).ok()?,
        source_line,
        proof,
    })
}

fn make_quote(quote: &model::Quote, proof: ProofId) -> Option<QuoteView> {
    Some(QuoteView {
        id: quote.label.clone(),
        date: quote.date,
        base: quantity_of(&quote.base)?,
        quote: quantity_of(&quote.counter)?,
        proof,
    })
}

#[derive(Clone, Copy)]
struct RemainingLot<'a> {
    lot: &'a Lot,
    quantity: &'a Exact,
    basis: &'a Exact,
    proof_inputs: &'a [ProofId],
}

#[derive(Clone, Copy)]
struct SaleSlice<'a> {
    quantity: &'a Quantity,
    proceeds: &'a Quantity,
    proof: ProofId,
}

enum AllocationPlan {
    Complete(Vec<(String, Exact)>),
    Insufficient { available: Exact },
    Ambiguous { candidates: Vec<String> },
}

fn plan_allocations(
    ordered_lots: &[String],
    lots: &[Lot],
    remaining: &BTreeMap<String, Exact>,
    required: &Exact,
    preserve_date_ties: bool,
) -> AllocationPlan {
    let available = ordered_lots.iter().fold(Exact::from(0i64), |total, id| {
        total.checked_add(remaining.get(id).unwrap_or(&Exact::from(0i64)))
    });
    if &available < required {
        return AllocationPlan::Insufficient { available };
    }

    let mut specs = Vec::new();
    let mut left = required.clone();
    let mut index = 0;
    while index < ordered_lots.len() && !left.is_zero() {
        let date = lots
            .iter()
            .find(|lot| lot.id == ordered_lots[index])
            .map(|lot| lot.date);
        let mut end = index + 1;
        while preserve_date_ties
            && end < ordered_lots.len()
            && lots
                .iter()
                .find(|lot| lot.id == ordered_lots[end])
                .map(|lot| lot.date)
                == date
        {
            end += 1;
        }
        let group = &ordered_lots[index..end];
        let group_available = group.iter().fold(Exact::from(0i64), |total, id| {
            total.checked_add(remaining.get(id).unwrap_or(&Exact::from(0i64)))
        });
        if preserve_date_ties && group.len() > 1 && left < group_available {
            return AllocationPlan::Ambiguous {
                candidates: group.to_vec(),
            };
        }
        for id in group {
            if left.is_zero() {
                break;
            }
            let available = remaining
                .get(id)
                .cloned()
                .unwrap_or_else(|| Exact::from(0i64));
            if available.is_zero() {
                continue;
            }
            let take = if available < left {
                available
            } else {
                left.clone()
            };
            left = left.checked_sub(&take);
            specs.push((id.clone(), take));
        }
        index = end;
    }
    AllocationPlan::Complete(specs)
}

fn conditional_gain(
    inventory: RemainingLot<'_>,
    sale: SaleSlice<'_>,
    proof: &mut Proof,
) -> Option<ConditionalGain> {
    let RemainingLot {
        lot,
        quantity: available_quantity,
        basis: available_basis,
        proof_inputs: remaining_inputs,
    } = inventory;
    let SaleSlice {
        quantity: sold,
        proceeds,
        proof: sale_proof,
    } = sale;
    if lot.quantity.unit != sold.unit || proceeds.unit.is_none() || lot.basis.unit != proceeds.unit
    {
        return None;
    }
    let ratio = sold.number.checked_div(available_quantity).ok()?;
    let allocated_basis = available_basis.checked_mul(&ratio);
    let gain = proceeds.number.checked_sub(&allocated_basis);
    let basis = Quantity::new(allocated_basis.clone(), lot.basis.unit.clone()).ok()?;
    let gain = Quantity::new(gain.clone(), proceeds.unit.clone()).ok()?;
    let mut metadata = metadata_for(&lot.id, "conditional-gain", None);
    metadata.insert("lot".into(), lot.id.clone());
    let proof_id = proof.insert(Node::new(
        format!("conditional gain {} on {}", lot.id, sale_proof),
        Operation::Arithmetic {
            rule: "gain = proceeds - allocated basis".into(),
            minuend: proceeds.number.clone(),
            subtrahend: allocated_basis.clone(),
            result: gain.number.clone(),
            unit: proceeds.unit.as_ref()?.to_string(),
        },
        remaining_inputs
            .iter()
            .copied()
            .chain(std::iter::once(sale_proof))
            .collect(),
        metadata,
    ));
    Some(ConditionalGain {
        lot_id: lot.id.clone(),
        quantity: sold.clone(),
        proceeds: proceeds.clone(),
        basis,
        gain,
        proof: proof_id,
    })
}

/// Build one exact slice of a recognized sale.  Both the holding basis and
/// the sale proceeds are allocated by the same rational quantity fraction;
/// no decimal rounding is introduced at an allocation boundary.
struct PendingLotAllocation {
    lot_id: String,
    quantity: Quantity,
    proceeds: Quantity,
    basis: Quantity,
    gain: Quantity,
    lot_proof: ProofId,
    sale_proof: ProofId,
    allocation_proof: ProofId,
    proof: ProofId,
}

impl PendingLotAllocation {
    fn finish(self, conservation_proof: ProofId) -> LotAllocation {
        LotAllocation {
            lot_id: self.lot_id,
            quantity: self.quantity,
            proceeds: self.proceeds,
            basis: self.basis,
            gain: self.gain,
            lot_proof: self.lot_proof,
            sale_proof: self.sale_proof,
            allocation_proof: self.allocation_proof,
            conservation_proof,
            proof: self.proof,
        }
    }
}

fn lot_allocation(
    inventory: RemainingLot<'_>,
    allocated_quantity: &Exact,
    sale: SaleSlice<'_>,
    sale_name: &str,
    proof: &mut Proof,
) -> Option<PendingLotAllocation> {
    let RemainingLot {
        lot,
        quantity: available_quantity,
        basis: available_basis,
        proof_inputs: remaining_inputs,
    } = inventory;
    let SaleSlice {
        quantity: sale_quantity,
        proceeds: sale_proceeds,
        proof: sale_proof,
    } = sale;
    if allocated_quantity.is_zero()
        || lot.quantity.unit != sale_quantity.unit
        || sale_quantity.number.is_zero()
        || sale_proceeds.unit.is_none()
        || lot.basis.unit != sale_proceeds.unit
    {
        return None;
    }
    let unit = lot.quantity.unit.as_ref()?.to_string();
    let quantity = Quantity::new(allocated_quantity.clone(), lot.quantity.unit.clone()).ok()?;
    let sale_ratio = allocated_quantity.checked_div(&sale_quantity.number).ok()?;
    let inventory_ratio = allocated_quantity.checked_div(available_quantity).ok()?;
    let allocated_basis_number = available_basis.checked_mul(&inventory_ratio);
    let allocated_proceeds_number = sale_proceeds.number.checked_mul(&sale_ratio);
    let remaining_number = available_quantity.checked_sub(allocated_quantity);
    let basis = Quantity::new(allocated_basis_number.clone(), lot.basis.unit.clone()).ok()?;
    let proceeds = Quantity::new(
        allocated_proceeds_number.clone(),
        sale_proceeds.unit.clone(),
    )
    .ok()?;
    let gain_number = allocated_proceeds_number.checked_sub(&allocated_basis_number);
    let gain = Quantity::new(gain_number.clone(), sale_proceeds.unit.clone()).ok()?;

    let mut allocation_metadata = metadata_for(&lot.id, "lot-allocation", None);
    allocation_metadata.insert(
        "sale-quantity".into(),
        sale_quantity.number.canonical_string(),
    );
    allocation_metadata.insert(
        "allocated-quantity".into(),
        allocated_quantity.canonical_string(),
    );
    allocation_metadata.insert(
        "allocated-proceeds".into(),
        allocated_proceeds_number.canonical_string(),
    );
    allocation_metadata.insert(
        "allocated-basis".into(),
        allocated_basis_number.canonical_string(),
    );
    allocation_metadata.insert("lot".into(), lot.id.clone());
    allocation_metadata.insert("sale".into(), sale_name.into());
    allocation_metadata.insert("available".into(), available_quantity.canonical_string());
    allocation_metadata.insert("allocated".into(), allocated_quantity.canonical_string());
    allocation_metadata.insert("remaining".into(), remaining_number.canonical_string());
    allocation_metadata.insert(
        "sale-quantity".into(),
        sale_quantity.number.canonical_string(),
    );
    allocation_metadata.insert(
        "sale-proceeds".into(),
        sale_proceeds.number.canonical_string(),
    );
    allocation_metadata.insert("available-basis".into(), available_basis.canonical_string());
    allocation_metadata.insert(
        "allocated-proceeds".into(),
        allocated_proceeds_number.canonical_string(),
    );
    allocation_metadata.insert(
        "allocated-basis".into(),
        allocated_basis_number.canonical_string(),
    );
    allocation_metadata.insert("gain".into(), gain_number.canonical_string());
    allocation_metadata.insert("quantity-unit".into(), unit.clone());
    allocation_metadata.insert(
        "value-unit".into(),
        sale_proceeds.unit.as_ref()?.to_string(),
    );
    let allocation_proof = proof.insert(Node::new(
        format!("allocate {} from {}", allocated_quantity, lot.id),
        Operation::LotAllocation(Box::new(LotAllocationCertificate {
            lot: lot.id.clone(),
            sale: sale_name.into(),
            lot_proof: lot.proof,
            sale_proof,
            available: available_quantity.clone(),
            allocated: allocated_quantity.clone(),
            remaining: remaining_number,
            sale_quantity: sale_quantity.number.clone(),
            sale_proceeds: sale_proceeds.number.clone(),
            available_basis: available_basis.clone(),
            allocated_proceeds: allocated_proceeds_number.clone(),
            allocated_basis: allocated_basis_number.clone(),
            gain: gain_number.clone(),
            quantity_unit: unit,
            value_unit: sale_proceeds.unit.as_ref()?.to_string(),
        })),
        remaining_inputs
            .iter()
            .copied()
            .chain(std::iter::once(sale_proof))
            .collect(),
        allocation_metadata,
    ));
    let mut gain_metadata = metadata_for(&lot.id, "allocated-gain", None);
    gain_metadata.insert("lot".into(), lot.id.clone());
    gain_metadata.insert("quantity".into(), allocated_quantity.canonical_string());
    let gain_proof = proof.insert(Node::new(
        format!("allocated gain {} on {}", lot.id, sale_proof),
        Operation::Arithmetic {
            rule: "gain = allocated proceeds - allocated basis".into(),
            minuend: allocated_proceeds_number,
            subtrahend: allocated_basis_number,
            result: gain_number,
            unit: sale_proceeds.unit.as_ref()?.to_string(),
        },
        vec![allocation_proof],
        gain_metadata,
    ));
    Some(PendingLotAllocation {
        lot_id: lot.id.clone(),
        quantity,
        proceeds,
        basis,
        gain,
        lot_proof: lot.proof,
        sale_proof,
        allocation_proof,
        proof: gain_proof,
    })
}

fn aggregate_allocations(allocations: &[LotAllocation], proof: ProofId) -> Option<ConditionalGain> {
    allocations.first()?;
    let mut quantity = Quantity::zero();
    let mut proceeds = Quantity::zero();
    let mut basis = Quantity::zero();
    let mut gain = Quantity::zero();
    for allocation in allocations {
        quantity = quantity.checked_add(&allocation.quantity).ok()?;
        proceeds = proceeds.checked_add(&allocation.proceeds).ok()?;
        basis = basis.checked_add(&allocation.basis).ok()?;
        gain = gain.checked_add(&allocation.gain).ok()?;
    }
    Some(ConditionalGain {
        lot_id: allocations
            .iter()
            .map(|allocation| allocation.lot_id.as_str())
            .collect::<Vec<_>>()
            .join(","),
        quantity,
        proceeds,
        basis,
        gain,
        proof,
    })
}

fn check_conditional_gains(
    analysis: &Analysis,
    sale: &SaleAnalysis,
    lots: &BTreeMap<&str, &Lot>,
    remaining_quantity: &BTreeMap<String, Exact>,
    remaining_basis: &BTreeMap<String, Exact>,
) -> Result<(), AnalysisCheckError> {
    for conditional in &sale.conditional_gains {
        let Some(lot) = lots.get(conditional.lot_id.as_str()) else {
            return Err(AnalysisCheckError::UnknownLot {
                sale: sale.id.clone(),
                lot: conditional.lot_id.clone(),
                proof: conditional.proof,
            });
        };
        let available = remaining_quantity
            .get(&conditional.lot_id)
            .expect("known lot has remaining quantity");
        let available_basis = remaining_basis
            .get(&conditional.lot_id)
            .expect("known lot has remaining basis");
        let expected_basis = sale
            .quantity
            .number
            .checked_div(available)
            .map(|ratio| available_basis.checked_mul(&ratio))
            .map_err(|_| AnalysisCheckError::InvalidAllocation {
                sale: sale.id.clone(),
                lot: conditional.lot_id.clone(),
                proof: conditional.proof,
                reason: "conditional gain cannot form an exact lot ratio".into(),
            })?;
        let expected_gain = sale.proceeds.number.checked_sub(&expected_basis);
        let valid_values = conditional.quantity == sale.quantity
            && conditional.proceeds == sale.proceeds
            && conditional.basis.number == expected_basis
            && conditional.basis.unit == lot.basis.unit
            && conditional.gain.number == expected_gain
            && conditional.gain.unit == sale.proceeds.unit;
        let valid_node = analysis.proof.node(conditional.proof).is_some_and(|node| {
            matches!(
                &node.operation,
                Operation::Arithmetic {
                    minuend,
                    subtrahend,
                    result,
                    unit,
                    ..
                } if *minuend == sale.proceeds.number
                    && *subtrahend == expected_basis
                    && *result == expected_gain
                    && sale.proceeds.unit.as_ref().is_some_and(|expected| unit == expected.as_str())
            ) && node.inputs.contains(&lot.proof)
                && node.inputs.iter().any(|input| {
                    matches!(
                        analysis.proof.node(*input).map(|node| &node.operation),
                        Some(Operation::SaleObservation { sale: observed, .. })
                            if observed == &sale.id
                    )
                })
        });
        if !valid_values || !valid_node || !analysis.proof.roots.contains(&conditional.proof) {
            return Err(AnalysisCheckError::InvalidAllocation {
                sale: sale.id.clone(),
                lot: conditional.lot_id.clone(),
                proof: conditional.proof,
                reason: "conditional gain is not the checked lot alternative".into(),
            });
        }
    }
    Ok(())
}

fn check_allocation_nodes(
    analysis: &Analysis,
    sale: &SaleAnalysis,
    lot: &Lot,
    allocation: &LotAllocation,
    available_quantity: &Exact,
    available_basis: &Exact,
    expected_predecessor: Option<ProofId>,
) -> Result<(), AnalysisCheckError> {
    let Some(allocation_node) = analysis.proof.node(allocation.allocation_proof) else {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "allocation derivation node is missing".into(),
        });
    };
    let typed_allocation = matches!(
        &allocation_node.operation,
        Operation::LotAllocation(certificate) if certificate.lot == allocation.lot_id
            && certificate.sale == sale.id
            && certificate.lot_proof == lot.proof
            && certificate.sale_proof == allocation.sale_proof
            && certificate.allocated == allocation.quantity.number
            && certificate.available == *available_quantity
            && certificate.remaining == available_quantity.checked_sub(&allocation.quantity.number)
            && certificate.sale_quantity == sale.quantity.number
            && certificate.sale_proceeds == sale.proceeds.number
            && certificate.available_basis == *available_basis
            && certificate.allocated_proceeds == allocation.proceeds.number
            && certificate.allocated_basis == allocation.basis.number
            && certificate.gain == allocation.gain.number
            && certificate.quantity_unit == lot.asset
            && allocation.gain.unit.as_ref().is_some_and(|unit| certificate.value_unit == unit.as_str())
    );
    if !typed_allocation
        || !allocation_node.inputs.contains(&lot.proof)
        || !allocation_node.inputs.contains(&allocation.sale_proof)
    {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "allocation node is not rooted in the typed lot and sale observations".into(),
        });
    }
    let Some(gain_node) = analysis.proof.node(allocation.proof) else {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "allocation gain node is missing".into(),
        });
    };
    let valid_gain = matches!(
        &gain_node.operation,
        Operation::Arithmetic {
            minuend,
            subtrahend,
            result,
            unit,
            ..
        } if *minuend == allocation.proceeds.number
            && *subtrahend == allocation.basis.number
            && *result == allocation.gain.number
            && allocation.gain.unit.as_ref().is_some_and(|expected| unit == expected.as_str())
    );
    if !valid_gain || !gain_node.inputs.contains(&allocation.allocation_proof) {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "allocation gain arithmetic is not rooted in its allocation".into(),
        });
    }
    let Some(conservation_node) = analysis.proof.node(allocation.conservation_proof) else {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "inventory-conservation node is missing".into(),
        });
    };
    let valid_conservation = matches!(
        &conservation_node.operation,
        Operation::InventoryConservation {
            lot: operation_lot,
            lot_proof,
            before,
            consumed,
            after,
            unit,
            predecessor,
            allocation: Some(operation_allocation),
            ..
        } if operation_lot == &allocation.lot_id
            && *lot_proof == lot.proof
            && *before == *available_quantity
            && *consumed == allocation.quantity.number
            && *after == available_quantity.checked_sub(&allocation.quantity.number)
            && allocation.quantity.unit.as_ref().is_some_and(|expected| unit == expected.as_str())
            && *predecessor == expected_predecessor
            && *operation_allocation == allocation.allocation_proof
    );
    if !valid_conservation
        || !conservation_node.inputs.contains(&lot.proof)
        || !conservation_node
            .inputs
            .contains(&allocation.allocation_proof)
    {
        return Err(AnalysisCheckError::InvalidAllocation {
            sale: sale.id.clone(),
            lot: allocation.lot_id.clone(),
            proof: allocation.proof,
            reason: "inventory conservation is not bound to the allocation".into(),
        });
    }
    Ok(())
}

fn check_recognition_node(
    analysis: &Analysis,
    sale: &SaleAnalysis,
    sale_observation: ProofId,
    allocation_proofs: &[ProofId],
    gain_proofs: &[ProofId],
    conservation_proofs: &[ProofId],
) -> Result<(), AnalysisCheckError> {
    let Some(node) = analysis.proof.node(sale.proof) else {
        return Err(AnalysisCheckError::InvalidRecognition {
            sale: sale.id.clone(),
            proof: sale.proof,
            reason: "recognition node is missing".into(),
        });
    };
    let valid_recognition = matches!(
        &node.operation,
        Operation::Recognition {
            sale: operation_sale,
            sale_proof: operation_sale_proof,
            quantity,
            proceeds,
            basis,
            gain,
            quantity_unit,
            value_unit,
        } if operation_sale == &sale.id
            && *operation_sale_proof == sale_observation
            && *quantity == sale.quantity.number
            && *proceeds == sale.proceeds.number
            && *basis == sale.recognized.as_ref().map(|aggregate| aggregate.basis.number.clone()).unwrap_or_else(|| Exact::from(0i64))
            && *gain == sale.recognized.as_ref().map(|aggregate| aggregate.gain.number.clone()).unwrap_or_else(|| Exact::from(0i64))
            && sale.quantity.unit.as_ref().is_some_and(|unit| quantity_unit == unit.as_str())
            && sale.proceeds.unit.as_ref().is_some_and(|unit| value_unit == unit.as_str())
    );
    if !valid_recognition
        || !node.inputs.contains(&sale_observation)
        || allocation_proofs
            .iter()
            .any(|proof| !node.inputs.contains(proof))
        || gain_proofs.iter().any(|proof| !node.inputs.contains(proof))
        || conservation_proofs
            .iter()
            .any(|proof| !node.inputs.contains(proof))
    {
        return Err(AnalysisCheckError::InvalidRecognition {
            sale: sale.id.clone(),
            proof: sale.proof,
            reason: "recognition node does not include its sale and allocation proofs".into(),
        });
    }
    Ok(())
}

fn make_journal(
    sale: &str,
    proceeds_account: &str,
    account: &str,
    asset: &str,
    gain: &ConditionalGain,
) -> JournalEntry {
    JournalEntry {
        sale: sale.to_owned(),
        lines: vec![
            JournalLine {
                side: Side::Debit,
                account: proceeds_account.to_owned(),
                quantity: gain.proceeds.clone(),
            },
            JournalLine {
                side: Side::Credit,
                account: format!("{account}:{asset}"),
                quantity: gain.basis.clone(),
            },
            JournalLine {
                side: Side::Credit,
                account: "gain:recognized".into(),
                quantity: gain.gain.clone(),
            },
        ],
        proof: gain.proof,
    }
}

fn matching_settlement<'a>(
    settlements: &'a [SettlementView],
    sale: &str,
    proceeds: &Quantity,
) -> Option<&'a SettlementView> {
    let mut matches = settlements.iter().filter(|settlement| {
        settlement.reference == sale
            && matches!(
                settlement.status,
                ObservationStatus::Observed | ObservationStatus::Reconciled
            )
            && settlement.into.is_some()
            && settlement.quantity == *proceeds
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn holding_of_model(sell: &model::Sell) -> Option<(String, String, Quantity)> {
    let account = sell.from.as_str().to_owned();
    let unit = sell.quantity.unit.as_ref()?.as_str().to_owned();
    let quantity = quantity_of(&sell.quantity)?;
    Some((account, unit, quantity))
}

fn quantity_of(quantity: &model::Quantity) -> Option<Quantity> {
    quantity.unit.as_ref()?;
    Some(quantity.clone())
}

fn zero_quantity() -> Quantity {
    Quantity::zero()
}

/// Return the semantic identity of an observed form.  This intentionally does
/// not contain a source line: moving a statement, inserting a comment, or
/// reformatting the file must not manufacture a different economic fact.
fn source_key(kind: &str, material: &str) -> String {
    format!("{kind}:{material}")
}

/// Source locations are useful for diagnostics and invalidation UX, but they
/// are metadata about a semantic observation rather than part of its source
/// identity.  The canonical `dependency`/`invalidation` key above is the
/// stable edge used by proofs and dependency queries.
fn metadata_for(source: &str, kind: &str, line: Option<usize>) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::from([
        ("dependency".into(), source.into()),
        ("kind".into(), kind.into()),
        ("invalidation".into(), source.into()),
    ]);
    if let Some(line) = line {
        metadata.insert("location".into(), format!("line:{line}"));
    }
    metadata
}

fn canonical_model_quantity(quantity: &model::Quantity) -> String {
    quantity.canonical()
}

fn buy_material(buy: &model::Buy) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        buy.label,
        buy.date,
        buy.into,
        canonical_model_quantity(&buy.quantity),
        canonical_model_quantity(&buy.cost),
        buy.fee
            .as_ref()
            .map(canonical_model_quantity)
            .unwrap_or_else(|| "-".into())
    )
}

fn sell_material(sell: &model::Sell) -> String {
    let lot = match &sell.lot {
        LotSelector::Explicit(lot) => format!("explicit:{lot}"),
        LotSelector::Hole(hole) => format!("hole:{hole}"),
    };
    format!(
        "{}|{}|{}|{}|{}|{}",
        sell.label,
        sell.date,
        sell.from,
        canonical_model_quantity(&sell.quantity),
        canonical_model_quantity(&sell.proceeds),
        lot,
    )
}

fn quote_material(quote: &model::Quote) -> String {
    format!(
        "{}|{}|{}|{}",
        quote.label,
        quote.date,
        canonical_model_quantity(&quote.base),
        canonical_model_quantity(&quote.counter),
    )
}

fn quote_group_key(quote: &QuoteView) -> String {
    format!(
        "{}:{}:{}",
        quote.date,
        quote
            .base
            .unit
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "?".into()),
        quote
            .quote
            .unit
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "?".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_ledger;

    #[test]
    fn fixture_recognizes_fifo_and_exact_gains() {
        let source = r#"book tax-us

buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD

buy buy/two on 2026-02-04
  10 ABC into brokerage
  for 300 USD
  fee 1 USD

sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot

use lots/fifo for tax-us
decide sell lot buy/two
"#;
        let ledger = parse_ledger(source).unwrap();
        let result = analyze(&ledger);
        let sale = result.sale("sell").unwrap();
        assert_eq!(sale.eligible_lots, vec!["buy/one", "buy/two"]);
        assert_eq!(sale.conditional_gains[0].gain.number.to_string(), "299");
        assert_eq!(sale.conditional_gains[1].gain.number.to_string(), "199");
        assert!(matches!(sale.status, RecognitionStatus::Conflict { .. }));
        assert!(result.journal.is_empty());
        assert!(
            result
                .invalidations
                .iter()
                .any(|(source, goals)| source.starts_with("buy:")
                    && goals.iter().any(|goal| goal == "gain:sell"))
        );
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn fifo_policy_proof_is_bound_to_the_builtin_policy_hash() {
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
        let result = analyze(&parse_ledger(source).unwrap());
        let policy_node = result
            .dependency_roots("gain:sell")
            .iter()
            .filter_map(|root| result.proof.node(*root))
            .find(|node| matches!(&node.operation, Operation::Policy { .. }))
            .expect("active policy is a gain dependency");
        assert_eq!(
            policy_node.metadata.get("policy-hash"),
            crate::package::builtin_policy_hash("lots/fifo")
                .map(|hash| hash.to_string())
                .as_ref()
        );
    }

    #[test]
    fn unique_selection_makes_a_balanced_journal() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
observe position brokerage 0 ABC
observe settlement sell 500 USD into cash
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(!result.blocked());
        assert_eq!(
            result
                .recognized_gain("sell")
                .unwrap()
                .gain
                .number
                .to_string(),
            "299"
        );
        assert!(result.journal[0].balanced());
        assert!(matches!(
            result.positions[0].status,
            ObservationStatus::Reconciled
        ));
        assert!(matches!(
            result.settlements[0].status,
            ObservationStatus::Reconciled
        ));
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn multiple_quotes_are_reported_without_blocking_direct_gain() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
quote quote/one on 2026-09-20
  1 ABC = 52 USD
quote quote/two on 2026-09-20
  1 ABC = 53 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(
            result
                .quote_status
                .values()
                .any(|status| matches!(status, QuoteStatus::Ambiguous { .. }))
        );
        assert_eq!(
            result
                .recognized_gain("sell")
                .unwrap()
                .gain
                .number
                .to_string(),
            "300"
        );
    }

    #[test]
    fn equivalent_quote_rates_are_unique_by_cross_multiplication() {
        let source = r#"book tax-us
quote quote/one on 2026-09-20
  1 ABC = 52 USD
quote quote/two on 2026-09-20
  2 ABC = 104 USD
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(matches!(
            result.quote_status.get("2026-09-20:ABC:USD"),
            Some(QuoteStatus::Unique)
        ));
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.code == IssueCode::AmbiguousQuote)
        );
    }

    #[test]
    fn source_locations_do_not_change_observation_identity() {
        let first = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
"#;
        let second = r#"book tax-us

# a location-only edit

buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
"#;
        let left = analyze(&parse_ledger(first).unwrap());
        let right = analyze(&parse_ledger(second).unwrap());
        assert_eq!(left.lots[0].proof, right.lots[0].proof);
        let left_node = left.proof.node(left.lots[0].proof).unwrap();
        assert!(left_node.metadata.contains_key("location"));
        assert_eq!(
            left_node.metadata.get("dependency"),
            right
                .proof
                .node(right.lots[0].proof)
                .and_then(|node| node.metadata.get("dependency"))
        );
    }

    #[test]
    fn settlement_account_is_the_only_journal_proceeds_account() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
observe settlement sell 500 USD into checking
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert_eq!(result.journal.len(), 1);
        assert_eq!(result.journal[0].lines[0].account, "checking");
        assert!(!result.journal[0].lines[0].account.starts_with("proceeds:"));
    }

    #[test]
    fn conflicting_observations_and_unmatched_settlement_are_not_proven() {
        let source = r#"book tax-us
observe position brokerage 10 ABC
observe position brokerage 11 ABC
observe settlement missing 1 USD into checking
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(
            result
                .positions
                .iter()
                .all(|position| matches!(position.status, ObservationStatus::Conflict))
        );
        assert!(
            result
                .settlements
                .iter()
                .all(|settlement| matches!(settlement.status, ObservationStatus::Observed))
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("position observations"))
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("does not match a sale"))
        );
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn conflicting_decisions_keep_both_proofs_and_block_selection() {
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
decide sell lot buy/one
decide sell lot buy/two
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let sale = result.sale("sell").unwrap();
        assert!(sale.selected_lot.is_none());
        assert!(sale.status.is_blocked());
        let issue = result
            .issues
            .iter()
            .find(|issue| issue.message.contains("decisions for sale"))
            .expect("decision conflict");
        let conflict = result
            .proof
            .node(issue.proof.expect("conflict proof"))
            .unwrap();
        assert_eq!(conflict.inputs.len(), 2);
        assert!(conflict.inputs.iter().all(|input| matches!(
            &result.proof.node(*input).unwrap().operation,
            Operation::Decision { subject, .. } if subject == "sell"
        )));
        assert!(result.journal.is_empty());
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn multiple_sales_deplete_one_lot_without_reuse() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  20 ABC into brokerage
  for 400 USD
sell first on 2026-09-20
  10 ABC from brokerage
  for 250 USD
  lot ?lot
sell second on 2026-09-21
  10 ABC from brokerage
  for 250 USD
  lot ?lot
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert_eq!(result.journal.len(), 0);
        assert_eq!(result.sales.len(), 2);
        assert!(result.sales.iter().all(|sale| sale.status.is_complete()));
        assert!(
            result
                .sales
                .iter()
                .all(|sale| sale.selected_lot.as_deref() == Some("buy/one"))
        );
        assert_eq!(
            result
                .sales
                .iter()
                .map(|sale| sale.allocations[0].quantity.number.to_string())
                .collect::<Vec<_>>(),
            vec!["10", "10"]
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.message.contains("multiple sales"))
        );
        assert!(!result.blocked());
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn policy_allocates_partial_and_cross_lot_sales_exactly() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  3 ABC into brokerage
  for 10 USD
  fee 2 USD
buy buy/two on 2026-02-04
  4 ABC into brokerage
  for 20 USD
sell first on 2026-09-20
  1 ABC from brokerage
  for 5 USD
  lot ?lot
sell second on 2026-09-21
  5 ABC from brokerage
  for 25 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let first = result.sale("first").unwrap();
        assert_eq!(first.selected_lot.as_deref(), Some("buy/one"));
        assert_eq!(first.allocations[0].quantity.number.to_string(), "1");
        assert_eq!(first.allocations[0].basis.number.canonical_string(), "4");
        assert_eq!(first.allocations[0].gain.number.canonical_string(), "1");

        let second = result.sale("second").unwrap();
        assert!(second.status.is_complete());
        assert_eq!(second.selected_lot, None);
        assert_eq!(
            second
                .allocations
                .iter()
                .map(|allocation| allocation.lot_id.as_str())
                .collect::<Vec<_>>(),
            vec!["buy/one", "buy/two"]
        );
        assert_eq!(second.allocations[0].quantity.number.to_string(), "2");
        assert_eq!(second.allocations[0].basis.number.canonical_string(), "8");
        assert_eq!(second.allocations[1].quantity.number.to_string(), "3");
        assert_eq!(second.allocations[1].basis.number.canonical_string(), "15");
        assert_eq!(
            result
                .recognized_gain("second")
                .unwrap()
                .gain
                .number
                .canonical_string(),
            "2"
        );
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn lifo_allocates_from_newest_lot_then_crosses_back() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  2 ABC into brokerage
  for 10 USD
buy buy/two on 2026-02-04
  3 ABC into brokerage
  for 30 USD
sell sell on 2026-09-20
  4 ABC from brokerage
  for 40 USD
  lot ?lot
use lots/lifo for tax-us
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let sale = result.sale("sell").unwrap();
        assert!(sale.status.is_complete());
        assert_eq!(
            sale.allocations
                .iter()
                .map(|allocation| allocation.lot_id.as_str())
                .collect::<Vec<_>>(),
            vec!["buy/two", "buy/one"]
        );
        assert_eq!(sale.allocations[0].quantity.number.to_string(), "3");
        assert_eq!(sale.allocations[1].quantity.number.to_string(), "1");
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn explicit_decision_conflict_remains_blocked_before_allocation() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  2 ABC into brokerage
  for 10 USD
buy buy/two on 2026-02-04
  2 ABC into brokerage
  for 20 USD
sell sell on 2026-09-20
  3 ABC from brokerage
  for 30 USD
  lot ?lot
use lots/fifo for tax-us
decide sell lot buy/two
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let sale = result.sale("sell").unwrap();
        assert!(matches!(sale.status, RecognitionStatus::Conflict { .. }));
        assert!(sale.allocations.is_empty());
        assert!(result.issues.iter().any(|issue| {
            issue.code == IssueCode::PolicyDecisionConflict
                && issue.message.contains("policy selects")
        }));
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn economic_date_controls_shared_inventory_not_source_order() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  3 ABC into brokerage
  for 3 USD
sell second on 2026-09-21
  2 ABC from brokerage
  for 2 USD
  lot ?lot
sell first on 2026-09-20
  2 ABC from brokerage
  for 2 USD
  lot ?lot
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(result.sale("first").unwrap().status.is_complete());
        assert!(matches!(
            result.sale("second").unwrap().status,
            RecognitionStatus::MissingLot
        ));
        assert_eq!(
            result.sale("first").unwrap().allocations[0]
                .quantity
                .number
                .to_string(),
            "2"
        );
        assert!(result.sale("second").unwrap().allocations.is_empty());
        assert!(result.issues.iter().any(|issue| {
            issue.sale.as_deref() == Some("second")
                && issue.code == IssueCode::InsufficientInventory
        }));
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn same_day_sales_need_disjoint_ledger_decisions_not_label_order() {
        let unresolved = r#"book tax-us
buy buy/one on 2026-01-04
  3 ABC into brokerage
  for 3 USD
sell alpha on 2026-09-20
  2 ABC from brokerage
  for 2 USD
  lot ?lot
sell zulu on 2026-09-20
  2 ABC from brokerage
  for 2 USD
  lot ?lot
"#;
        let blocked = analyze(&parse_ledger(unresolved).unwrap());
        assert!(
            blocked
                .sales
                .iter()
                .all(|sale| matches!(sale.status, RecognitionStatus::Ambiguous { .. }))
        );
        assert!(blocked.sales.iter().all(|sale| sale.allocations.is_empty()));

        let resolved = r#"book tax-us
buy buy/one on 2026-01-04
  2 ABC into brokerage
  for 2 USD
buy buy/two on 2026-01-05
  2 ABC into brokerage
  for 4 USD
sell alpha on 2026-09-20
  2 ABC from brokerage
  for 3 USD
  lot ?lot
sell zulu on 2026-09-20
  2 ABC from brokerage
  for 5 USD
  lot ?lot
decide alpha lot buy/two
decide zulu lot buy/one
"#;
        let decided = analyze(&parse_ledger(resolved).unwrap());
        assert_eq!(
            decided.sale("alpha").unwrap().selected_lot.as_deref(),
            Some("buy/two")
        );
        assert_eq!(
            decided.sale("zulu").unwrap().selected_lot.as_deref(),
            Some("buy/one")
        );
        assert!(decided.sales.iter().all(|sale| sale.status.is_complete()));
        assert!(decided.check_proof().is_ok());
    }

    #[test]
    fn policy_ties_require_a_decision_only_when_partially_consumed() {
        let tied_first = r#"book tax-us
buy buy/a on 2026-01-04
  1 ABC into brokerage
  for 10 USD
buy buy/b on 2026-01-04
  1 ABC into brokerage
  for 20 USD
sell sell on 2026-09-20
  1 ABC from brokerage
  for 30 USD
  lot ?lot
use lots/fifo for tax-us
decide sell lot buy/b
"#;
        let decided = analyze(&parse_ledger(tied_first).unwrap());
        assert_eq!(
            decided.sale("sell").unwrap().selected_lot.as_deref(),
            Some("buy/b")
        );

        let later_tie = r#"book tax-us
buy buy/a on 2026-01-03
  1 ABC into brokerage
  for 10 USD
buy buy/b on 2026-01-04
  1 ABC into brokerage
  for 20 USD
buy buy/c on 2026-01-04
  1 ABC into brokerage
  for 30 USD
sell sell on 2026-09-20
  2 ABC from brokerage
  for 60 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let ambiguous = analyze(&parse_ledger(later_tie).unwrap());
        assert!(matches!(
            ambiguous.sale("sell").unwrap().status,
            RecognitionStatus::Ambiguous { .. }
        ));
        assert!(ambiguous.sale("sell").unwrap().allocations.is_empty());

        let all_tied = later_tie.replace("2 ABC from", "3 ABC from");
        let complete = analyze(&parse_ledger(&all_tied).unwrap());
        assert!(complete.sale("sell").unwrap().status.is_complete());
        assert_eq!(complete.sale("sell").unwrap().allocations.len(), 3);
    }

    #[test]
    fn insufficient_inventory_does_not_consume_any_lot() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  3 ABC into brokerage
  for 10 USD
sell first on 2026-09-20
  2 ABC from brokerage
  for 8 USD
  lot ?lot
sell second on 2026-09-21
  2 ABC from brokerage
  for 8 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(result.sale("first").unwrap().status.is_complete());
        let second = result.sale("second").unwrap();
        assert!(matches!(second.status, RecognitionStatus::MissingLot));
        assert!(second.allocations.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("only 1 ABC remains"))
        );
        assert!(result.check_proof().is_ok());
    }

    #[test]
    fn workspace_source_path_runs_multi_lot_fixture_and_binds_proof() {
        use crate::workspace::Workspace;

        let source = r#"book tax-us
buy buy/one on 2026-01-04
  2 ABC into brokerage
  for 10 USD
buy buy/two on 2026-02-04
  2 ABC into brokerage
  for 20 USD
sell first on 2026-09-20
  1 ABC from brokerage
  for 6 USD
  lot ?lot
sell second on 2026-09-21
  3 ABC from brokerage
  for 18 USD
  lot ?lot
sell third on 2026-09-22
  1 ABC from brokerage
  for 6 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let mut workspace = Workspace::new();
        let source = workspace.load_source("tax-us", source).unwrap();
        let result = workspace.analyze_commit(source.commit).unwrap();
        assert_eq!(result.sales.len(), 3);
        assert_eq!(result.sale("first").unwrap().allocations.len(), 1);
        assert_eq!(result.sale("second").unwrap().allocations.len(), 2);
        assert!(matches!(
            result.sale("third").unwrap().status,
            RecognitionStatus::MissingLot
        ));
        result.check_proof().unwrap();
    }

    #[test]
    fn semantic_checker_rejects_forged_public_results_and_proof_bindings() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  2 ABC into brokerage
  for 10 USD
buy buy/two on 2026-02-04
  2 ABC into brokerage
  for 20 USD
sell sale/one on 2026-09-20
  3 ABC from brokerage
  for 18 USD
  lot ?lot
use lots/fifo for tax-us
"#;
        let analysis = analyze(&parse_ledger(source).unwrap());
        analysis.check_semantics().unwrap();

        let mut forged = analysis.clone();
        forged.sales[0].allocations[0].basis.number = Exact::from(99i64);
        assert!(matches!(
            forged.check_semantics(),
            Err(AnalysisCheckError::InvalidAllocation { .. })
        ));

        let mut forged = analysis.clone();
        forged.sales[0].selected_lots.reverse();
        assert!(matches!(
            forged.check_semantics(),
            Err(AnalysisCheckError::InvalidRecognition { .. })
        ));

        let mut forged = analysis.clone();
        let allocation_proof = forged.sales[0].allocations[0].allocation_proof;
        forged.sales[0].allocations[0].conservation_proof = allocation_proof;
        assert!(matches!(
            forged.check_semantics(),
            Err(AnalysisCheckError::InvalidAllocation { .. })
        ));

        let mut forged = analysis;
        forged.sales[0].recognized.as_mut().unwrap().gain.number = Exact::from(99i64);
        assert!(matches!(
            forged.check_semantics(),
            Err(AnalysisCheckError::InvalidRecognition { .. })
        ));

        let ambiguous = analyze(
            &parse_ledger(
                r#"book tax-us
buy buy/a on 2026-01-01
  10 ABC into brokerage
  for 100 USD
buy buy/b on 2026-01-02
  10 ABC into brokerage
  for 120 USD
sell sale/a on 2026-02-01
  5 ABC from brokerage
  for 80 USD
  lot ?lot
"#,
            )
            .unwrap(),
        );
        ambiguous.check_semantics().unwrap();
        let mut forged = ambiguous;
        forged.sales[0].conditional_gains[0].gain.number = Exact::from(999i64);
        assert!(matches!(
            forged.check_semantics(),
            Err(AnalysisCheckError::InvalidAllocation { .. })
        ));
    }

    #[test]
    fn incompatible_proceeds_block_recognition_instead_of_guessing() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 EUR
  lot ?lot
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let sale = result.sale("sell").unwrap();
        assert_eq!(sale.selected_lot, None);
        assert!(matches!(sale.status, RecognitionStatus::InvalidAmount));
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == IssueCode::IncompatibleUnit)
        );
    }

    #[test]
    fn lifo_uses_the_same_compiled_evaluator_as_fifo() {
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
use lots/lifo for tax-us
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let sale = result.sale("sell").unwrap();
        assert_eq!(sale.selected_lot.as_deref(), Some("buy/two"));
        assert!(sale.status.is_complete());
        let policy = result
            .proof
            .nodes
            .values()
            .find(|node| matches!(node.operation, Operation::Policy { ref policy, .. } if policy == "lots/lifo"))
            .expect("lifo policy proof");
        let expected_hash = builtin_policy_hash_for_test("lots/lifo").to_string();
        assert_eq!(policy.metadata.get("policy-hash"), Some(&expected_hash));
        assert!(policy.metadata.contains_key("policy-program"));
    }

    #[test]
    fn unknown_and_malformed_packages_do_not_fallback_or_emit_a_journal() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
observe settlement sell 500 USD into checking
use lots/unknown for tax-us
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(result.sale("sell").unwrap().selected_lot.is_none());
        assert!(result.journal.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.code == IssueCode::UnknownPolicy && issue.proof.is_some())
        );

        let mut registry = PolicyRegistry::builtins();
        registry.insert(crate::package::PolicyPackage::new(
            "lots/broken",
            "0",
            "selector=not_implemented\ntie=ambiguous",
        ));
        let broken = source.replace("lots/unknown", "lots/broken");
        let result = analyze_with_registry(&parse_ledger(&broken).unwrap(), &registry);
        assert!(result.sale("sell").unwrap().selected_lot.is_none());
        assert!(result.journal.is_empty());
        assert!(result.issues.iter().any(|issue| {
            issue.code == IssueCode::UnknownPolicy
                && issue.message.contains("not executable")
                && issue.proof.is_some()
        }));
    }

    #[test]
    fn policy_conflicts_are_order_invariant_and_rooted() {
        let body = r#"book tax-us
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
"#;
        let left = format!("{body}use lots/fifo for tax-us\nuse lots/lifo for tax-us\n");
        let right = format!("{body}use lots/lifo for tax-us\nuse lots/fifo for tax-us\n");
        let left = analyze(&parse_ledger(&left).unwrap());
        let right = analyze(&parse_ledger(&right).unwrap());
        assert_eq!(left.policy, right.policy);
        assert_eq!(left.sale("sell").unwrap().selected_lot, None);
        assert_eq!(right.sale("sell").unwrap().selected_lot, None);
        assert!(
            left.issues
                .iter()
                .any(|issue| { issue.message.contains("both apply") && issue.proof.is_some() })
        );
        assert!(
            right
                .issues
                .iter()
                .any(|issue| { issue.message.contains("both apply") && issue.proof.is_some() })
        );
    }

    #[test]
    fn wrong_book_policy_and_unknown_decision_targets_are_rooted_issues() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
use lots/fifo for other-book
decide missing-sale lot buy/one
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        let mut messages = result.issues.iter().map(|issue| issue.message.as_str());
        assert!(
            messages
                .clone()
                .any(|message| message.contains("targets book"))
        );
        assert!(messages.any(|message| message.contains("unknown sale")));
        assert!(
            result
                .issues
                .iter()
                .filter(|issue| {
                    issue.message.contains("targets book") || issue.message.contains("unknown sale")
                })
                .all(|issue| issue.proof.is_some())
        );
    }

    #[test]
    fn blocked_sale_does_not_reconcile_a_post_disposal_position() {
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
observe position brokerage 10 ABC
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(matches!(
            result.positions[0].status,
            ObservationStatus::Conflict
        ));
    }

    #[test]
    fn position_conflicts_are_scoped_by_account_and_asset() {
        let source = r#"book tax-us
observe position brokerage 10 ABC
observe position brokerage 500 USD
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(
            result
                .positions
                .iter()
                .all(|position| position.status == ObservationStatus::Observed)
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.message.contains("position observations"))
        );
    }

    #[test]
    fn duplicate_settlements_never_choose_the_first_occurrence() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
observe settlement sell 500 USD into checking
observe settlement sell 500 USD into checking
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert!(
            result
                .settlements
                .iter()
                .all(|settlement| settlement.status == ObservationStatus::Conflict)
        );
        assert!(result.journal.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("settlement observations"))
        );
    }

    fn builtin_policy_hash_for_test(name: &str) -> crate::model::ContentHash {
        crate::package::builtin_policy_hash(name).expect("test builtin")
    }
}
