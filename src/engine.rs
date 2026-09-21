//! Deterministic semantic analysis for the first Axiom slice.
//!
//! The parser gives us typed, source-located forms.  This module only derives
//! views: lots, conditional answers, recognition, observations, and a
//! balanced journal projection.  It never changes the source ledger and it
//! never silently chooses an answer when more than one answer is admissible.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::exact::Exact;
use crate::model::{self, Date, Ledger, LedgerForm, LotSelector};
use crate::package::{LotCandidate, PolicyRegistry, Selection, SelectionProgram};
use crate::proof::{
    CashSettlementObservationCertificate, JournalEntryCertificate, JournalLineCertificate,
    LotAllocationCertificate, Node, ObligationBalanceCertificate, ObligationObservationCertificate,
    Operation, PositionObservationCertificate, PositionReconciliationCertificate, Proof, ProofId,
    QuoteObservationCertificate, SatisfactionAllocationCertificate,
    SatisfactionObservationCertificate, SettlementBalanceCertificate, SettlementHistoryCertificate,
    SettlementObservationCertificate, SettlementTransition,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObligationStatus {
    Outstanding,
    PartiallySatisfied,
    Satisfied,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObligationView {
    pub id: String,
    pub debtor: String,
    pub creditor: String,
    pub promised: Quantity,
    pub due: Option<model::Date>,
    pub remaining: Option<Quantity>,
    pub status: ObligationStatus,
    pub proof: ProofId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementHistoryView {
    pub id: String,
    pub kind: model::SettlementKind,
    pub amount: Quantity,
    pub current: model::SettlementStateKind,
    pub effective: bool,
    pub unused: Option<Quantity>,
    pub proof: ProofId,
    source: model::SourceSettlement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SatisfactionView {
    pub id: String,
    pub obligation: String,
    pub settlement: String,
    pub amount: Quantity,
    pub state: model::SatisfactionState,
    pub effective: bool,
    pub proof: ProofId,
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
    ObligationConflict,
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
    InvalidSatisfactionResult {
        subject: String,
        proof: ProofId,
        reason: String,
    },
    InvalidQuoteResult {
        quote: String,
        proof: ProofId,
        reason: String,
    },
    InvalidPositionResult {
        account: String,
        proof: ProofId,
        reason: String,
    },
    InvalidSettlementResult {
        settlement: String,
        proof: ProofId,
        reason: String,
    },
    InvalidJournalResult {
        sale: String,
        proof: ProofId,
        reason: String,
    },
    InvalidDependencyIndex {
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
            | Self::Proof(crate::proof::CheckError::InvalidQuoteObservation { id })
            | Self::Proof(crate::proof::CheckError::InvalidPositionObservation { id })
            | Self::Proof(crate::proof::CheckError::InvalidCashSettlementObservation { id })
            | Self::Proof(crate::proof::CheckError::InvalidLotAllocation { id })
            | Self::Proof(crate::proof::CheckError::InvalidInventoryConservation { id })
            | Self::Proof(crate::proof::CheckError::InvalidRecognition { id })
            | Self::Proof(crate::proof::CheckError::InvalidPositionReconciliation { id })
            | Self::Proof(crate::proof::CheckError::InvalidJournalEntry { id })
            | Self::Proof(crate::proof::CheckError::UnreachableCertificate { id })
            | Self::Proof(crate::proof::CheckError::InvalidOperation { id }) => Some(*id),
            Self::Proof(_) => None,
            Self::BlockedSaleHasRecognition { proof, .. }
            | Self::MissingRecognition { proof, .. }
            | Self::UnknownLot { proof, .. }
            | Self::InvalidAllocation { proof, .. }
            | Self::InvalidRecognition { proof, .. }
            | Self::InvalidSatisfactionResult { proof, .. }
            | Self::InvalidQuoteResult { proof, .. }
            | Self::InvalidPositionResult { proof, .. }
            | Self::InvalidSettlementResult { proof, .. }
            | Self::InvalidJournalResult { proof, .. } => Some(*proof),
            Self::InvalidDependencyIndex { .. } => None,
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
            Self::InvalidSatisfactionResult {
                subject, reason, ..
            } => write!(
                formatter,
                "invalid obligation or settlement result `{subject}`: {reason}"
            ),
            Self::InvalidQuoteResult { quote, reason, .. } => {
                write!(formatter, "invalid quote result `{quote}`: {reason}")
            }
            Self::InvalidPositionResult {
                account, reason, ..
            } => write!(formatter, "invalid position result `{account}`: {reason}"),
            Self::InvalidSettlementResult {
                settlement, reason, ..
            } => write!(
                formatter,
                "invalid settlement result `{settlement}`: {reason}"
            ),
            Self::InvalidJournalResult { sale, reason, .. } => {
                write!(formatter, "invalid journal result `{sale}`: {reason}")
            }
            Self::InvalidDependencyIndex { reason } => {
                write!(formatter, "invalid dependency index: {reason}")
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
    pub obligations: Vec<ObligationView>,
    pub settlement_histories: Vec<SettlementHistoryView>,
    pub satisfactions: Vec<SatisfactionView>,
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
        check_dependency_indexes(self)?;
        check_satisfaction_results(self)?;
        check_public_answers(self)?;

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
    let mut source_obligations = Vec::new();
    let mut source_settlements = Vec::new();
    let mut source_satisfactions = Vec::new();
    let mut obligation_proofs = BTreeMap::new();
    let mut settlement_history_proofs = BTreeMap::new();
    let mut satisfaction_proofs = BTreeMap::new();
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
                let operation = quantity_of(&buy.quantity)
                    .map(|quantity| Operation::LotObservation {
                        lot: buy.label.clone(),
                        source: source.clone(),
                        account: buy.into.as_str().to_owned(),
                        asset: quantity
                            .unit
                            .as_ref()
                            .expect("resolved buy quantity has a unit")
                            .to_string(),
                        quantity: quantity.number,
                    })
                    .unwrap_or_else(|| Operation::Observation {
                        source: source.clone(),
                    });
                let node = Node::new(
                    format!("lot {}/{}", buy.label, source),
                    operation,
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
                let operation = match (quantity_of(&quote.base), quantity_of(&quote.counter)) {
                    (Some(base), Some(counter)) => {
                        Operation::QuoteObservation(QuoteObservationCertificate {
                            quote: quote.label.clone(),
                            date: quote.date.to_string(),
                            base: base.number,
                            base_unit: base
                                .unit
                                .as_ref()
                                .expect("quantity_of requires a unit")
                                .to_string(),
                            quote_amount: counter.number,
                            quote_unit: counter
                                .unit
                                .as_ref()
                                .expect("quantity_of requires a unit")
                                .to_string(),
                        })
                    }
                    _ => Operation::Observation {
                        source: source.clone(),
                    },
                };
                let node = proof.insert(Node::new(
                    format!("quote {}/{}", quote.label, source),
                    operation,
                    Vec::new(),
                    metadata_for(&source, "quote", Some(index + 1)),
                ));
                if let Some(view) = make_quote(quote, node) {
                    quotes.push(view);
                }
            }
            LedgerForm::Obligation(obligation) => {
                let source = obligation_material(obligation);
                let id = obligation.occurrence.as_str().to_owned();
                let source_id = source_key("obligation", &source);
                let operation = typed_obligation_certificate(obligation)
                    .map(Operation::ObligationObservation)
                    .unwrap_or_else(|| Operation::Observation {
                        source: source_id.clone(),
                    });
                let proof_id = proof.insert(Node::new(
                    format!("obligation {id}"),
                    operation,
                    Vec::new(),
                    metadata_for(&source_id, "obligation", Some(index + 1)),
                ));
                obligation_proofs.insert(id, proof_id);
                source_obligations.push(obligation.clone());
            }
            LedgerForm::Settlement(settlement) => {
                let source = settlement_history_material(settlement);
                let id = settlement.occurrence.as_str().to_owned();
                let source_id = source_key("settlement-history", &source);
                let operation = typed_settlement_certificate(settlement)
                    .map(Operation::SettlementObservation)
                    .unwrap_or_else(|| Operation::Observation {
                        source: source_id.clone(),
                    });
                let proof_id = proof.insert(Node::new(
                    format!("settlement history {id}"),
                    operation,
                    Vec::new(),
                    metadata_for(&source_id, "settlement-history", Some(index + 1)),
                ));
                settlement_history_proofs.insert(id, proof_id);
                source_settlements.push(settlement.clone());
            }
            LedgerForm::Satisfaction(satisfaction) => {
                let source = satisfaction_material(satisfaction);
                let id = satisfaction.occurrence.as_str().to_owned();
                let source_id = source_key("satisfaction", &source);
                let operation = typed_satisfaction_certificate(satisfaction)
                    .map(Operation::SatisfactionObservation)
                    .unwrap_or_else(|| Operation::Observation {
                        source: source_id.clone(),
                    });
                let proof_id = proof.insert(Node::new(
                    format!("satisfaction {id}"),
                    operation,
                    Vec::new(),
                    metadata_for(&source_id, "satisfaction", Some(index + 1)),
                ));
                satisfaction_proofs.insert(id, proof_id);
                source_satisfactions.push(satisfaction.clone());
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
                let operation = quantity_of(&observation.quantity)
                    .and_then(|quantity| {
                        quantity.unit.as_ref().map(|unit| {
                            Operation::PositionObservation(PositionObservationCertificate {
                                account: observation.account.as_str().to_owned(),
                                quantity: quantity.number,
                                unit: unit.to_string(),
                            })
                        })
                    })
                    .unwrap_or_else(|| Operation::Observation {
                        source: source.clone(),
                    });
                let node = proof.insert(Node::new(
                    format!("observed position {}/{}", observation.account, source),
                    operation,
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
                let operation = quantity_of(&observation.amount)
                    .and_then(|amount| {
                        amount.unit.as_ref().map(|unit| {
                            Operation::CashSettlementObservation(
                                CashSettlementObservationCertificate {
                                    reference: reference.to_owned(),
                                    amount: amount.number,
                                    unit: unit.to_string(),
                                    into: observation
                                        .into
                                        .as_ref()
                                        .map(|account| account.as_str().to_owned()),
                                },
                            )
                        })
                    })
                    .unwrap_or_else(|| Operation::Observation {
                        source: source.clone(),
                    });
                let node = proof.insert(Node::new(
                    format!("observed settlement {reference}/{source}"),
                    operation,
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
    let (obligations, settlement_histories, satisfactions) =
        analyze_satisfaction_sources(SatisfactionContext {
            source_obligations: &source_obligations,
            source_settlements: &source_settlements,
            source_satisfactions: &source_satisfactions,
            obligation_proofs: &obligation_proofs,
            settlement_proofs: &settlement_history_proofs,
            satisfaction_proofs: &satisfaction_proofs,
            proof: &mut proof,
            issues: &mut issues,
            dependencies: &mut dependencies,
            invalidations: &mut invalidations,
        });
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
        let sale_operation = sell
            .quantity
            .unit
            .as_ref()
            .map(|unit| Operation::SaleObservation {
                sale: sell.label.clone(),
                source: source.clone(),
                account: sell.from.as_str().to_owned(),
                asset: unit.as_str().to_owned(),
            })
            .unwrap_or_else(|| Operation::Observation {
                source: source.clone(),
            });
        let sale_proof = proof.insert(Node::new(
            format!("sale {}/{}", sell.label, source),
            sale_operation,
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
        let mut journal_proof = None;
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
                    account: account.clone(),
                    asset: asset.clone(),
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
                let entry = make_journal(
                    &mut proof,
                    &sell.label,
                    JournalAccounts {
                        proceeds: into,
                        inventory: &account,
                        asset: &asset,
                    },
                    &total,
                    id,
                    settlement.proof,
                );
                journal_proof = Some(entry.proof);
                journal.push(entry);
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
        if let Some(journal_proof) = journal_proof {
            goal_roots.push(journal_proof);
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
            Operation::PositionReconciliation(Box::new(PositionReconciliationCertificate {
                account: position.account.clone(),
                source_proof: position.proof,
                observed: position.quantity.number.clone(),
                calculated: calculated.clone(),
                result: position.quantity.number.clone(),
                unit: position
                    .quantity
                    .unit
                    .as_ref()
                    .expect("position quantity has a unit")
                    .to_string(),
                status: "reconciled".into(),
            }))
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
            Operation::PositionReconciliation(Box::new(PositionReconciliationCertificate {
                account: position.account.clone(),
                source_proof: position.proof,
                observed: position.quantity.number.clone(),
                calculated: calculated.clone(),
                result: position.quantity.number.clone(),
                unit: position
                    .quantity
                    .unit
                    .as_ref()
                    .expect("position quantity has a unit")
                    .to_string(),
                status: "conflict".into(),
            }))
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
    for quote in &quotes {
        dependencies
            .entry(format!("quote:{}", quote.id))
            .or_default()
            .push(quote.proof);
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
    let proof_roots = dependencies
        .values()
        .flatten()
        .copied()
        .chain(issues.iter().filter_map(|issue| issue.proof))
        .collect::<Vec<_>>();
    proof.root_all(proof_roots);
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
        obligations,
        settlement_histories,
        satisfactions,
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

fn check_public_answers(analysis: &Analysis) -> Result<(), AnalysisCheckError> {
    for quote in &analysis.quotes {
        let valid = analysis.proof.node(quote.proof).is_some_and(|node| {
            matches!(
                &node.operation,
                Operation::QuoteObservation(certificate)
                    if certificate.quote == quote.id
                        && certificate.date == quote.date.to_string()
                        && certificate.base == quote.base.number
                        && quote
                            .base
                            .unit
                            .as_ref()
                            .is_some_and(|unit| unit.to_string() == certificate.base_unit)
                        && certificate.quote_amount == quote.quote.number
                        && quote
                            .quote
                            .unit
                            .as_ref()
                            .is_some_and(|unit| unit.to_string() == certificate.quote_unit)
            )
        });
        if !valid {
            return Err(AnalysisCheckError::InvalidQuoteResult {
                quote: quote.id.clone(),
                proof: quote.proof,
                reason: "result fields do not match the typed quote source".into(),
            });
        }
    }

    for position in &analysis.positions {
        let Some(node) = analysis.proof.node(position.proof) else {
            return Err(AnalysisCheckError::InvalidPositionResult {
                account: position.account.clone(),
                proof: position.proof,
                reason: "position proof is missing".into(),
            });
        };
        let unit = position
            .quantity
            .unit
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        match &node.operation {
            Operation::PositionObservation(certificate) => {
                if !matches!(
                    position.status,
                    ObservationStatus::Observed | ObservationStatus::Conflict
                ) || certificate.account != position.account
                    || certificate.quantity != position.quantity.number
                    || certificate.unit != unit
                {
                    return Err(AnalysisCheckError::InvalidPositionResult {
                        account: position.account.clone(),
                        proof: position.proof,
                        reason: "result fields do not match the typed position source".into(),
                    });
                }
            }
            Operation::PositionReconciliation(certificate) => {
                if certificate.account != position.account
                    || certificate.observed != position.quantity.number
                    || certificate.result != position.quantity.number
                    || certificate.unit != unit
                    || ((position.status == ObservationStatus::Reconciled
                        && certificate.status != "reconciled")
                        || (position.status == ObservationStatus::Conflict
                            && certificate.status != "conflict"))
                {
                    return Err(AnalysisCheckError::InvalidPositionResult {
                        account: position.account.clone(),
                        proof: position.proof,
                        reason: "result fields do not match the reconciliation certificate".into(),
                    });
                }
                let mut calculated = Exact::from(0i64);
                for lot in &analysis.lots {
                    if lot.account == position.account && lot.asset == unit {
                        calculated = calculated.checked_add(&lot.quantity.number);
                    }
                }
                for sale in &analysis.sales {
                    if sale.status.is_complete()
                        && sale.account == position.account
                        && sale.asset == unit
                    {
                        calculated = calculated.checked_sub(&sale.quantity.number);
                    }
                }
                if certificate.calculated != calculated
                    || ((certificate.status == "reconciled"
                        && calculated != position.quantity.number)
                        || (certificate.status == "conflict"
                            && calculated == position.quantity.number))
                {
                    return Err(AnalysisCheckError::InvalidPositionResult {
                        account: position.account.clone(),
                        proof: position.proof,
                        reason: "calculated position does not reproduce authored events".into(),
                    });
                }
            }
            _ => {
                return Err(AnalysisCheckError::InvalidPositionResult {
                    account: position.account.clone(),
                    proof: position.proof,
                    reason: "position proof is not a typed source or reconciliation".into(),
                });
            }
        }
    }

    for settlement in &analysis.settlements {
        let Some(certificate) = cash_settlement_source(&analysis.proof, settlement.proof) else {
            return Err(AnalysisCheckError::InvalidSettlementResult {
                settlement: settlement.reference.clone(),
                proof: settlement.proof,
                reason: "settlement proof does not reach a typed cash observation".into(),
            });
        };
        let unit = settlement
            .quantity
            .unit
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let matching_settlements = analysis
            .settlements
            .iter()
            .filter(|candidate| candidate.reference == settlement.reference)
            .count();
        let expected_status = if matching_settlements > 1 {
            ObservationStatus::Conflict
        } else if let Some(sale) = analysis
            .sales
            .iter()
            .find(|sale| sale.id == settlement.reference)
        {
            if sale.proceeds == settlement.quantity {
                ObservationStatus::Reconciled
            } else {
                ObservationStatus::Conflict
            }
        } else {
            ObservationStatus::Observed
        };
        if certificate.reference != settlement.reference
            || certificate.amount != settlement.quantity.number
            || certificate.unit != unit
            || certificate.into != settlement.into
            || settlement.status != expected_status
        {
            return Err(AnalysisCheckError::InvalidSettlementResult {
                settlement: settlement.reference.clone(),
                proof: settlement.proof,
                reason: "result fields do not match the typed cash observation".into(),
            });
        }
    }

    for entry in &analysis.journal {
        let valid = analysis.proof.node(entry.proof).is_some_and(|node| {
            let Operation::JournalEntry(certificate) = &node.operation else {
                return false;
            };
            if certificate.sale != entry.sale || certificate.lines.len() != entry.lines.len() {
                return false;
            }
            entry
                .lines
                .iter()
                .zip(&certificate.lines)
                .all(|(line, expected)| {
                    let side = match line.side {
                        Side::Debit => "debit",
                        Side::Credit => "credit",
                    };
                    side == expected.side
                        && line.account == expected.account
                        && line.quantity.number == expected.amount
                        && line
                            .quantity
                            .unit
                            .as_ref()
                            .is_some_and(|unit| unit.to_string() == expected.unit)
                })
                && entry.balanced()
        });
        if !valid {
            return Err(AnalysisCheckError::InvalidJournalResult {
                sale: entry.sale.clone(),
                proof: entry.proof,
                reason: "journal lines do not match the typed journal certificate".into(),
            });
        }
    }
    Ok(())
}

fn cash_settlement_source(
    proof: &Proof,
    root: ProofId,
) -> Option<&CashSettlementObservationCertificate> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let node = proof.node(id)?;
        if let Operation::CashSettlementObservation(certificate) = &node.operation {
            return Some(certificate);
        }
        pending.extend(node.inputs.iter().copied());
    }
    None
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
            account: operation_account,
            asset: operation_asset,
            quantity,
            proceeds,
            basis,
            gain,
            quantity_unit,
            value_unit,
            ..
        } if operation_sale == &sale.id
            && *operation_sale_proof == sale_observation
            && operation_account == &sale.account
            && operation_asset == &sale.asset
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

#[derive(Clone, Copy)]
struct JournalAccounts<'a> {
    proceeds: &'a str,
    inventory: &'a str,
    asset: &'a str,
}

fn make_journal(
    proof: &mut Proof,
    sale: &str,
    accounts: JournalAccounts<'_>,
    gain: &ConditionalGain,
    recognition_proof: ProofId,
    settlement_proof: ProofId,
) -> JournalEntry {
    let gain_line = if gain.gain.number.is_negative() {
        JournalLine {
            side: Side::Debit,
            account: "loss:recognized".into(),
            quantity: Quantity {
                number: gain.gain.number.abs(),
                unit: gain.gain.unit.clone(),
            },
        }
    } else {
        JournalLine {
            side: Side::Credit,
            account: "gain:recognized".into(),
            quantity: gain.gain.clone(),
        }
    };
    let lines = vec![
        JournalLine {
            side: Side::Debit,
            account: accounts.proceeds.to_owned(),
            quantity: gain.proceeds.clone(),
        },
        JournalLine {
            side: Side::Credit,
            account: format!("{}:{}", accounts.inventory, accounts.asset),
            quantity: gain.basis.clone(),
        },
        gain_line,
    ];
    let certificate = JournalEntryCertificate {
        sale: sale.to_owned(),
        recognition_proof,
        settlement_proof,
        inventory_account: accounts.inventory.to_owned(),
        asset: accounts.asset.to_owned(),
        lines: lines
            .iter()
            .map(|line| JournalLineCertificate {
                side: match line.side {
                    Side::Debit => "debit".into(),
                    Side::Credit => "credit".into(),
                },
                account: line.account.clone(),
                amount: line.quantity.number.clone(),
                unit: line
                    .quantity
                    .unit
                    .as_ref()
                    .expect("journal lines have units")
                    .to_string(),
            })
            .collect(),
    };
    let journal_proof = proof.insert(Node::new(
        format!("journal {sale}"),
        Operation::JournalEntry(Box::new(certificate)),
        vec![recognition_proof, settlement_proof],
        metadata_for(sale, "journal", None),
    ));
    JournalEntry {
        sale: sale.to_owned(),
        lines,
        proof: journal_proof,
    }
}

type SatisfactionViews = (
    Vec<ObligationView>,
    Vec<SettlementHistoryView>,
    Vec<SatisfactionView>,
);

struct SatisfactionContext<'a> {
    source_obligations: &'a [model::SourceObligation],
    source_settlements: &'a [model::SourceSettlement],
    source_satisfactions: &'a [model::SourceSatisfaction],
    obligation_proofs: &'a BTreeMap<String, ProofId>,
    settlement_proofs: &'a BTreeMap<String, ProofId>,
    satisfaction_proofs: &'a BTreeMap<String, ProofId>,
    proof: &'a mut Proof,
    issues: &'a mut Vec<Issue>,
    dependencies: &'a mut BTreeMap<String, Vec<ProofId>>,
    invalidations: &'a mut BTreeMap<String, Vec<String>>,
}

struct SatisfactionDisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl SatisfactionDisjointSet {
    fn new() -> Self {
        Self {
            parent: Vec::new(),
            rank: Vec::new(),
        }
    }

    fn add(&mut self) -> usize {
        let node = self.parent.len();
        self.parent.push(node);
        self.rank.push(0);
        node
    }

    fn find(&mut self, node: usize) -> usize {
        if self.parent[node] != node {
            let root = self.find(self.parent[node]);
            self.parent[node] = root;
        }
        self.parent[node]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left = self.find(left);
        let mut right = self.find(right);
        if left == right {
            return;
        }
        if self.rank[left] < self.rank[right] {
            std::mem::swap(&mut left, &mut right);
        }
        self.parent[right] = left;
        if self.rank[left] == self.rank[right] {
            self.rank[left] += 1;
        }
    }
}

fn satisfaction_node(
    nodes: &mut HashMap<String, usize>,
    dsu: &mut SatisfactionDisjointSet,
    id: &str,
) -> usize {
    if let Some(node) = nodes.get(id) {
        return *node;
    }
    let node = dsu.add();
    nodes.insert(id.to_owned(), node);
    node
}

struct SatisfactionComponent {
    obligations: Vec<crate::ontology::Obligation>,
    settlements: Vec<crate::ontology::Settlement>,
    allocations: Vec<crate::ontology::SatisfactionAllocation>,
    summary: Result<crate::ontology::SatisfactionSummary, crate::ontology::OntologyError>,
    proof_inputs: Vec<ProofId>,
}

struct SatisfactionNetwork {
    components: Vec<SatisfactionComponent>,
    obligation_components: HashMap<String, usize>,
    settlement_components: HashMap<String, usize>,
    settlements_by_id: HashMap<String, usize>,
    allocations_by_id: HashMap<String, usize>,
    allocations_by_obligation: HashMap<String, Vec<usize>>,
    allocations_by_settlement: HashMap<String, Vec<usize>>,
}

impl SatisfactionNetwork {
    fn new(
        obligations: &[crate::ontology::Obligation],
        settlements: &[crate::ontology::Settlement],
        allocations: &[crate::ontology::SatisfactionAllocation],
    ) -> Self {
        let mut dsu = SatisfactionDisjointSet::new();
        let mut obligation_nodes = HashMap::with_capacity(obligations.len());
        let mut settlement_nodes = HashMap::with_capacity(settlements.len());
        for obligation in obligations {
            satisfaction_node(&mut obligation_nodes, &mut dsu, obligation.id.as_str());
        }
        for settlement in settlements {
            satisfaction_node(&mut settlement_nodes, &mut dsu, settlement.id.as_str());
        }
        for allocation in allocations {
            let obligation = satisfaction_node(
                &mut obligation_nodes,
                &mut dsu,
                allocation.obligation.as_str(),
            );
            let settlement = satisfaction_node(
                &mut settlement_nodes,
                &mut dsu,
                allocation.settlement.as_str(),
            );
            dsu.union(obligation, settlement);
        }

        let mut component_by_root = HashMap::new();
        let mut component_count = 0;
        for node in 0..dsu.parent.len() {
            let root = dsu.find(node);
            if let std::collections::hash_map::Entry::Vacant(entry) = component_by_root.entry(root)
            {
                entry.insert(component_count);
                component_count += 1;
            }
        }
        let mut components = (0..component_count)
            .map(|_| SatisfactionComponent {
                obligations: Vec::new(),
                settlements: Vec::new(),
                allocations: Vec::new(),
                summary: Err(crate::ontology::OntologyError::InvalidEvent {
                    kind: "satisfaction",
                    reason: "component has no validation result".into(),
                }),
                proof_inputs: Vec::new(),
            })
            .collect::<Vec<_>>();

        let obligation_components = obligation_nodes
            .iter()
            .map(|(id, node)| (id.clone(), component_by_root[&dsu.find(*node)]))
            .collect::<HashMap<_, _>>();
        let settlement_components = settlement_nodes
            .iter()
            .map(|(id, node)| (id.clone(), component_by_root[&dsu.find(*node)]))
            .collect::<HashMap<_, _>>();
        for obligation in obligations {
            let component = obligation_components[obligation.id.as_str()];
            components[component].obligations.push(obligation.clone());
        }
        for settlement in settlements {
            let component = settlement_components[settlement.id.as_str()];
            components[component].settlements.push(settlement.clone());
        }
        let mut allocations_by_id = HashMap::with_capacity(allocations.len());
        let mut allocations_by_obligation = HashMap::<String, Vec<usize>>::new();
        let mut allocations_by_settlement = HashMap::<String, Vec<usize>>::new();
        for (index, allocation) in allocations.iter().enumerate() {
            let component = obligation_components
                .get(allocation.obligation.as_str())
                .copied()
                .or_else(|| {
                    settlement_components
                        .get(allocation.settlement.as_str())
                        .copied()
                })
                .expect("allocation endpoints were indexed above");
            components[component].allocations.push(allocation.clone());
            allocations_by_id
                .entry(allocation.id.as_str().to_owned())
                .or_insert(index);
            allocations_by_obligation
                .entry(allocation.obligation.as_str().to_owned())
                .or_default()
                .push(index);
            allocations_by_settlement
                .entry(allocation.settlement.as_str().to_owned())
                .or_default()
                .push(index);
        }
        for component in &mut components {
            component.summary = crate::ontology::validate_satisfaction_network(
                &component.obligations,
                &component.settlements,
                &component.allocations,
            );
        }

        let mut settlements_by_id = HashMap::with_capacity(settlements.len());
        for (index, settlement) in settlements.iter().enumerate() {
            settlements_by_id
                .entry(settlement.id.as_str().to_owned())
                .or_insert(index);
        }
        Self {
            components,
            obligation_components,
            settlement_components,
            settlements_by_id,
            allocations_by_id,
            allocations_by_obligation,
            allocations_by_settlement,
        }
    }

    fn component_for_obligation(&self, id: &str) -> Option<&SatisfactionComponent> {
        self.obligation_components
            .get(id)
            .and_then(|index| self.components.get(*index))
    }

    fn component_for_settlement(&self, id: &str) -> Option<&SatisfactionComponent> {
        self.settlement_components
            .get(id)
            .and_then(|index| self.components.get(*index))
    }
}

fn satisfaction_component_proof_inputs(
    component: &SatisfactionComponent,
    obligation_proofs: &BTreeMap<String, ProofId>,
    settlement_proofs: &BTreeMap<String, ProofId>,
    satisfaction_proofs: &BTreeMap<String, ProofId>,
) -> Vec<ProofId> {
    let mut inputs = BTreeSet::new();
    for obligation in &component.obligations {
        if let Some(proof) = obligation_proofs.get(obligation.id.as_str()) {
            inputs.insert(*proof);
        }
    }
    for settlement in &component.settlements {
        if let Some(proof) = settlement_proofs.get(settlement.id.as_str()) {
            inputs.insert(*proof);
        }
    }
    for allocation in &component.allocations {
        if let Some(proof) = satisfaction_proofs.get(allocation.id.as_str()) {
            inputs.insert(*proof);
        }
    }
    inputs.into_iter().collect()
}

fn analyze_satisfaction_sources(context: SatisfactionContext<'_>) -> SatisfactionViews {
    use crate::ontology as economic;

    let SatisfactionContext {
        source_obligations,
        source_settlements,
        source_satisfactions,
        obligation_proofs,
        settlement_proofs,
        satisfaction_proofs,
        proof,
        issues,
        dependencies,
        invalidations,
    } = context;
    let mut first_error = None;
    let mut first_error_source = None;

    let mut obligations = Vec::with_capacity(source_obligations.len());
    for source in source_obligations {
        let result = source
            .quantity
            .unit
            .as_ref()
            .map(|unit| unit.as_str())
            .ok_or(economic::OntologyError::InvalidQuantity {
                context: "an obligation needs an instrument unit",
            })
            .and_then(|unit| {
                economic::Obligation::transfer(
                    source.occurrence.as_str(),
                    source.debtor.as_str(),
                    source.creditor.as_str(),
                    unit,
                    source.quantity.clone(),
                )
            });
        match result {
            Ok(obligation) => obligations.push(match source.due {
                Some(due) => obligation.due_on(due),
                None => obligation,
            }),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                    first_error_source = Some(("obligation", source.occurrence.as_str()));
                }
            }
        }
    }

    let mut settlements = Vec::with_capacity(source_settlements.len());
    for source in source_settlements {
        let history = source
            .history
            .iter()
            .map(|transition| economic::SettlementTransition {
                state: ontology_settlement_state(transition.state),
                at: transition.at,
                reason: None,
            })
            .collect();
        let result = economic::Settlement::from_history(
            source.occurrence.as_str(),
            economic::Endpoint::entity(source.from.as_str()),
            economic::Endpoint::entity(source.to.as_str()),
            source.instrument.as_str(),
            source.amount.clone(),
            history,
        );
        match result {
            Ok(settlement) => settlements.push(settlement),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                    first_error_source = Some(("settlement", source.occurrence.as_str()));
                }
            }
        }
    }

    let mut allocations = Vec::with_capacity(source_satisfactions.len());
    for source in source_satisfactions {
        let result = economic::SatisfactionAllocation::new(
            source.occurrence.as_str(),
            source.obligation.as_str(),
            source.settlement.as_str(),
            source.amount.clone(),
        )
        .map(|allocation| match source.state {
            model::SatisfactionState::Proposed => allocation,
            model::SatisfactionState::Applied => allocation.applied(),
            model::SatisfactionState::Reversed => allocation.reversed(),
        });
        match result {
            Ok(allocation) => allocations.push(allocation),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                    first_error_source = Some(("satisfaction", source.occurrence.as_str()));
                }
            }
        }
    }

    let mut network = SatisfactionNetwork::new(&obligations, &settlements, &allocations);
    for component in &mut network.components {
        component.proof_inputs = satisfaction_component_proof_inputs(
            component,
            obligation_proofs,
            settlement_proofs,
            satisfaction_proofs,
        );
    }

    if let Some(reason) = first_error.as_ref().map(ToString::to_string) {
        let inputs = first_error_source
            .and_then(|(kind, id)| match kind {
                "obligation" => obligation_proofs.get(id),
                "settlement" => settlement_proofs.get(id),
                "satisfaction" => satisfaction_proofs.get(id),
                _ => None,
            })
            .copied()
            .into_iter()
            .collect();
        let conflict = proof.insert(Node::new(
            "obligation satisfaction conflict",
            Operation::Conflict {
                subject: "satisfaction-network".into(),
                reason,
            },
            inputs,
            metadata_for("satisfaction-network", "obligation-conflict", None),
        ));
        issues.push(Issue {
            code: IssueCode::ObligationConflict,
            message: first_error.as_ref().unwrap().to_string(),
            sale: None,
            proof: Some(conflict),
        });
    }
    for component in &network.components {
        let Err(error) = &component.summary else {
            continue;
        };
        let reason = error.to_string();
        let conflict = proof.insert(Node::new(
            "obligation satisfaction conflict",
            Operation::Conflict {
                subject: "satisfaction-network".into(),
                reason: reason.clone(),
            },
            component.proof_inputs.clone(),
            metadata_for("satisfaction-network", "obligation-conflict", None),
        ));
        issues.push(Issue {
            code: IssueCode::ObligationConflict,
            message: reason,
            sale: None,
            proof: Some(conflict),
        });
    }

    // Bind every independently valid authored source to a typed proof leaf,
    // then expose the exact settlement/allocation/balance certificates that
    // consumers can check without rerunning the ontology.  Invalid source
    // facts deliberately remain generic observations (created above), so a
    // blocked ledger still has a structurally checkable proof bundle.
    let mut settlement_history_certificate_proofs = BTreeMap::<String, ProofId>::new();
    for source in source_settlements {
        let id = source.occurrence.as_str().to_owned();
        let Some(&settlement_proof) = settlement_proofs.get(&id) else {
            continue;
        };
        let source_is_typed = proof
            .node(settlement_proof)
            .is_some_and(|node| matches!(node.operation, Operation::SettlementObservation(_)));
        let source_is_valid = network.settlements_by_id.contains_key(id.as_str());
        if !source_is_typed || !source_is_valid {
            continue;
        }
        let history = source
            .history
            .iter()
            .map(|transition| SettlementTransition {
                state: settlement_state_text(transition.state).into(),
                at: transition.at.map(|date| date.to_string()),
            })
            .collect::<Vec<_>>();
        let Some(current) = history.last().map(|transition| transition.state.clone()) else {
            continue;
        };
        let effective = network
            .settlements_by_id
            .get(id.as_str())
            .and_then(|index| settlements.get(*index))
            .is_some_and(economic::Settlement::is_effective);
        let certificate = SettlementHistoryCertificate {
            settlement: id.clone(),
            settlement_proof,
            kind: settlement_kind_text(source.kind).into(),
            from: source.from.as_str().to_owned(),
            to: source.to.as_str().to_owned(),
            instrument: source.instrument.as_str().to_owned(),
            amount: source.amount.number.clone(),
            unit: source
                .amount
                .unit
                .as_ref()
                .expect("typed settlement has a unit")
                .as_str()
                .to_owned(),
            history,
            current,
            effective,
        };
        let certificate_proof = proof.insert(Node::new(
            format!("settlement history certificate {id}"),
            Operation::SettlementHistory(Box::new(certificate)),
            vec![settlement_proof],
            metadata_for(
                &source_key("settlement-history", &settlement_history_material(source)),
                "settlement-history-certificate",
                None,
            ),
        ));
        settlement_history_certificate_proofs.insert(id, certificate_proof);
    }

    let mut satisfaction_allocation_proofs = BTreeMap::<String, ProofId>::new();
    for source in source_satisfactions {
        let id = source.occurrence.as_str().to_owned();
        let Some(&satisfaction_proof) = satisfaction_proofs.get(&id) else {
            continue;
        };
        let Some(&obligation_proof) = obligation_proofs.get(source.obligation.as_str()) else {
            continue;
        };
        let Some(&settlement_proof) = settlement_proofs.get(source.settlement.as_str()) else {
            continue;
        };
        let typed_obligation =
            proof
                .node(obligation_proof)
                .and_then(|node| match &node.operation {
                    Operation::ObligationObservation(certificate) => Some(certificate),
                    _ => None,
                });
        let typed_settlement =
            proof
                .node(settlement_proof)
                .and_then(|node| match &node.operation {
                    Operation::SettlementObservation(certificate) => Some(certificate),
                    _ => None,
                });
        let typed_satisfaction = proof
            .node(satisfaction_proof)
            .is_some_and(|node| matches!(node.operation, Operation::SatisfactionObservation(_)));
        let Some(obligation) = typed_obligation else {
            continue;
        };
        let Some(settlement) = typed_settlement else {
            continue;
        };
        if !typed_satisfaction
            || obligation.debtor != settlement.from
            || obligation.creditor != settlement.to
            || obligation.unit != settlement.unit
            || obligation.unit
                != source
                    .amount
                    .unit
                    .as_ref()
                    .map(|unit| unit.as_str())
                    .unwrap_or("")
            || settlement.instrument != obligation.unit
            || !network.allocations_by_id.contains_key(id.as_str())
        {
            continue;
        }
        let certificate = SatisfactionAllocationCertificate {
            satisfaction: id.clone(),
            satisfaction_proof,
            obligation: source.obligation.as_str().to_owned(),
            obligation_proof,
            settlement: source.settlement.as_str().to_owned(),
            settlement_proof,
            amount: source.amount.number.clone(),
            unit: source
                .amount
                .unit
                .as_ref()
                .expect("typed satisfaction has a unit")
                .as_str()
                .to_owned(),
            state: satisfaction_state_text(source.state).into(),
        };
        let certificate_proof = proof.insert(Node::new(
            format!("satisfaction allocation certificate {id}"),
            Operation::SatisfactionAllocation(Box::new(certificate)),
            vec![satisfaction_proof, obligation_proof, settlement_proof],
            metadata_for(
                &source_key("satisfaction", &satisfaction_material(source)),
                "satisfaction-allocation",
                None,
            ),
        ));
        satisfaction_allocation_proofs.insert(id, certificate_proof);
    }

    let mut obligation_balance_proofs = BTreeMap::<String, ProofId>::new();
    for source in source_obligations {
        let id = source.occurrence.as_str().to_owned();
        let Some(&obligation_proof) = obligation_proofs.get(&id) else {
            continue;
        };
        if !proof
            .node(obligation_proof)
            .is_some_and(|node| matches!(node.operation, Operation::ObligationObservation(_)))
        {
            continue;
        }
        let Some(component) = network.component_for_obligation(&id) else {
            continue;
        };
        let Ok(summary) = &component.summary else {
            continue;
        };
        let Some(remaining) = summary
            .obligation_remaining
            .get(&economic::ObligationId::new(id.clone()))
            .cloned()
        else {
            continue;
        };
        let allocation_ids = network
            .allocations_by_obligation
            .get(id.as_str())
            .into_iter()
            .flatten()
            .filter_map(|index| allocations.get(*index))
            .filter(|allocation| {
                allocation.state == economic::AllocationState::Applied
                    && network
                        .settlements_by_id
                        .get(allocation.settlement.as_str())
                        .and_then(|index| settlements.get(*index))
                        .is_some_and(economic::Settlement::is_effective)
            })
            .filter_map(|allocation| satisfaction_allocation_proofs.get(allocation.id.as_str()))
            .copied()
            .collect::<Vec<_>>();
        let mut allocation_ids = allocation_ids;
        allocation_ids.sort();
        allocation_ids.dedup();
        let allocated = allocation_ids
            .iter()
            .fold(Exact::from(0i64), |total, proof_id| {
                proof
                    .node(*proof_id)
                    .and_then(|node| match &node.operation {
                        Operation::SatisfactionAllocation(certificate) => {
                            Some(certificate.amount.clone())
                        }
                        _ => None,
                    })
                    .map(|amount| total.checked_add(&amount))
                    .unwrap_or(total)
            });
        if allocated.checked_add(&remaining.number) != source.quantity.number {
            continue;
        }
        let Some(unit) = source
            .quantity
            .unit
            .as_ref()
            .map(|unit| unit.as_str().to_owned())
        else {
            continue;
        };
        let certificate = ObligationBalanceCertificate {
            obligation: id.clone(),
            obligation_proof,
            promised: source.quantity.number.clone(),
            allocated,
            remaining: remaining.number,
            unit,
            allocations: allocation_ids.clone(),
        };
        let mut inputs = vec![obligation_proof];
        inputs.extend(allocation_ids);
        let balance_proof = proof.insert(Node::new(
            format!("obligation balance certificate {id}"),
            Operation::ObligationBalance(Box::new(certificate)),
            inputs,
            metadata_for(&id, "obligation-balance", None),
        ));
        obligation_balance_proofs.insert(id, balance_proof);
    }

    let mut settlement_balance_proofs = BTreeMap::<String, ProofId>::new();
    for source in source_settlements {
        let id = source.occurrence.as_str().to_owned();
        let Some(&settlement_proof) = settlement_proofs.get(&id) else {
            continue;
        };
        if !settlement_history_certificate_proofs.contains_key(&id) {
            continue;
        }
        let Some(component) = network.component_for_settlement(&id) else {
            continue;
        };
        let Ok(summary) = &component.summary else {
            continue;
        };
        let Some(unused) = summary
            .settlement_unused
            .get(&economic::SettlementId::new(id.clone()))
            .cloned()
        else {
            continue;
        };
        let allocation_ids = network
            .allocations_by_settlement
            .get(id.as_str())
            .into_iter()
            .flatten()
            .filter_map(|index| allocations.get(*index))
            .filter(|allocation| {
                allocation.state == economic::AllocationState::Applied
                    && network
                        .settlements_by_id
                        .get(id.as_str())
                        .and_then(|index| settlements.get(*index))
                        .is_some_and(economic::Settlement::is_effective)
            })
            .filter_map(|allocation| satisfaction_allocation_proofs.get(allocation.id.as_str()))
            .copied()
            .collect::<Vec<_>>();
        let mut allocation_ids = allocation_ids;
        allocation_ids.sort();
        allocation_ids.dedup();
        let allocated = allocation_ids
            .iter()
            .fold(Exact::from(0i64), |total, proof_id| {
                proof
                    .node(*proof_id)
                    .and_then(|node| match &node.operation {
                        Operation::SatisfactionAllocation(certificate) => {
                            Some(certificate.amount.clone())
                        }
                        _ => None,
                    })
                    .map(|amount| total.checked_add(&amount))
                    .unwrap_or(total)
            });
        let Some(unit) = source
            .amount
            .unit
            .as_ref()
            .map(|unit| unit.as_str().to_owned())
        else {
            continue;
        };
        if allocated.checked_add(&unused.number) != source.amount.number {
            continue;
        }
        let certificate = SettlementBalanceCertificate {
            settlement: id.clone(),
            settlement_proof,
            amount: source.amount.number.clone(),
            allocated,
            unused: unused.number,
            unit,
            allocations: allocation_ids.clone(),
        };
        let mut inputs = vec![settlement_proof];
        if let Some(history_proof) = settlement_history_certificate_proofs.get(&id) {
            inputs.push(*history_proof);
        }
        inputs.extend(allocation_ids);
        let balance_proof = proof.insert(Node::new(
            format!("settlement balance certificate {id}"),
            Operation::SettlementBalance(Box::new(certificate)),
            inputs,
            metadata_for(&id, "settlement-balance", None),
        ));
        settlement_balance_proofs.insert(id, balance_proof);
    }

    let obligation_views = source_obligations
        .iter()
        .map(|source| {
            let id = source.occurrence.as_str().to_owned();
            let proof_id = obligation_proofs[&id];
            let remaining = network
                .component_for_obligation(&id)
                .and_then(|component| component.summary.as_ref().ok())
                .and_then(|summary| {
                    summary
                        .obligation_remaining
                        .get(&economic::ObligationId::new(id.clone()))
                        .cloned()
                });
            let status = match &remaining {
                None => ObligationStatus::Invalid,
                Some(value) if value.is_zero() => ObligationStatus::Satisfied,
                Some(value) if *value == source.quantity => ObligationStatus::Outstanding,
                Some(_) => ObligationStatus::PartiallySatisfied,
            };
            let result_proof = obligation_balance_proofs
                .get(&id)
                .copied()
                .unwrap_or(proof_id);
            let mut roots = network
                .component_for_obligation(&id)
                .map(|component| component.proof_inputs.clone())
                .unwrap_or_else(|| vec![proof_id]);
            if !roots.contains(&proof_id) {
                roots.push(proof_id);
            }
            if let Some(balance) = obligation_balance_proofs.get(&id) {
                roots.push(*balance);
            }
            dependencies.insert(format!("obligation:{id}"), roots);
            invalidations
                .entry(source_key("obligation", &obligation_material(source)))
                .or_default()
                .push(format!("obligation:{id}"));
            ObligationView {
                id,
                debtor: source.debtor.as_str().to_owned(),
                creditor: source.creditor.as_str().to_owned(),
                promised: source.quantity.clone(),
                due: source.due,
                remaining,
                status,
                proof: result_proof,
            }
        })
        .collect();
    let settlement_views: Vec<SettlementHistoryView> = source_settlements
        .iter()
        .map(|source| {
            let id = source.occurrence.as_str().to_owned();
            let proof_id = settlement_proofs[&id];
            let mut roots = network
                .component_for_settlement(&id)
                .map(|component| component.proof_inputs.clone())
                .unwrap_or_else(|| vec![proof_id]);
            if !roots.contains(&proof_id) {
                roots.push(proof_id);
            }
            if let Some(history) = settlement_history_certificate_proofs.get(&id) {
                roots.push(*history);
            }
            if let Some(balance) = settlement_balance_proofs.get(&id) {
                roots.push(*balance);
            }
            dependencies.insert(format!("settlement:{id}"), roots);
            invalidations
                .entry(source_key(
                    "settlement-history",
                    &settlement_history_material(source),
                ))
                .or_default()
                .push(format!("settlement:{id}"));
            let current = source
                .history
                .last()
                .map(|state| state.state)
                .unwrap_or(model::SettlementStateKind::Issued);
            let effective = network
                .settlements_by_id
                .get(id.as_str())
                .and_then(|index| settlements.get(*index))
                .is_some_and(economic::Settlement::is_effective);
            let component_summary = network
                .component_for_settlement(&id)
                .and_then(|component| component.summary.as_ref().ok());
            let result_proof = settlement_balance_proofs
                .get(&id)
                .or_else(|| settlement_history_certificate_proofs.get(&id))
                .copied()
                .unwrap_or(proof_id);
            SettlementHistoryView {
                id: id.clone(),
                kind: source.kind,
                amount: source.amount.clone(),
                current,
                effective,
                unused: component_summary.as_ref().and_then(|summary| {
                    summary
                        .settlement_unused
                        .get(&economic::SettlementId::new(id))
                        .cloned()
                }),
                proof: result_proof,
                source: source.clone(),
            }
        })
        .collect();
    let satisfaction_views = source_satisfactions
        .iter()
        .map(|source| {
            let id = source.occurrence.as_str().to_owned();
            let proof_id = satisfaction_proofs[&id];
            let mut roots = vec![proof_id];
            if let Some(allocation) = satisfaction_allocation_proofs.get(&id) {
                roots.push(*allocation);
            }
            dependencies.insert(format!("satisfaction:{id}"), roots);
            invalidations
                .entry(source_key("satisfaction", &satisfaction_material(source)))
                .or_default()
                .push(format!("satisfaction:{id}"));
            let source_allocation_is_valid = network
                .allocations_by_id
                .get(id.as_str())
                .and_then(|index| allocations.get(*index))
                .is_some_and(|allocation| {
                    allocation.obligation.as_str() == source.obligation.as_str()
                        && allocation.settlement.as_str() == source.settlement.as_str()
                        && allocation.quantity == source.amount
                });
            let component_valid = network
                .obligation_components
                .get(source.obligation.as_str())
                .zip(
                    network
                        .settlement_components
                        .get(source.settlement.as_str()),
                )
                .is_some_and(|(obligation, settlement)| {
                    obligation == settlement && network.components[*obligation].summary.is_ok()
                });
            let effective = source.state == model::SatisfactionState::Applied
                && source_allocation_is_valid
                && component_valid
                && network
                    .settlements_by_id
                    .get(source.settlement.as_str())
                    .and_then(|index| settlements.get(*index))
                    .is_some_and(economic::Settlement::is_effective);
            let result_proof = satisfaction_allocation_proofs
                .get(&id)
                .copied()
                .unwrap_or(proof_id);
            SatisfactionView {
                id,
                obligation: source.obligation.as_str().to_owned(),
                settlement: source.settlement.as_str().to_owned(),
                amount: source.amount.clone(),
                state: source.state,
                effective,
                proof: result_proof,
            }
        })
        .collect();
    (obligation_views, settlement_views, satisfaction_views)
}

fn check_satisfaction_results(analysis: &Analysis) -> Result<(), AnalysisCheckError> {
    let expected_obligations = authored_source_ids(&analysis.proof, "obligation", "obligation ");
    let expected_settlements =
        authored_source_ids(&analysis.proof, "settlement-history", "settlement history ");
    let expected_satisfactions =
        authored_source_ids(&analysis.proof, "satisfaction", "satisfaction ");
    require_complete_views(
        "obligation",
        expected_obligations,
        analysis.obligations.iter().map(|view| view.id.as_str()),
    )?;
    require_complete_views(
        "settlement",
        expected_settlements,
        analysis
            .settlement_histories
            .iter()
            .map(|view| view.id.as_str()),
    )?;
    require_complete_views(
        "satisfaction",
        expected_satisfactions,
        analysis.satisfactions.iter().map(|view| view.id.as_str()),
    )?;

    let mut certified_obligations = BTreeSet::new();
    let mut certified_settlements = BTreeSet::new();
    for node in analysis.proof.nodes.values() {
        match &node.operation {
            Operation::ObligationBalance(balance) => {
                certified_obligations.insert(balance.obligation.as_str());
            }
            Operation::SettlementBalance(balance) => {
                certified_settlements.insert(balance.settlement.as_str());
            }
            _ => {}
        }
    }
    for view in &analysis.obligations {
        require_goal_proof(analysis, &format!("obligation:{}", view.id), view.proof)?;
        let Some(remaining) = &view.remaining else {
            if view.status != ObligationStatus::Invalid {
                return Err(invalid_satisfaction_result(
                    &view.id,
                    view.proof,
                    "an invalid obligation must not claim a status",
                ));
            }
            let valid = analysis
                .proof
                .node(view.proof)
                .is_some_and(|node| match &node.operation {
                    Operation::ObligationObservation(source) => {
                        source.obligation == view.id
                            && source.debtor == view.debtor
                            && source.creditor == view.creditor
                            && source.promised == view.promised.number
                            && view.promised.unit.as_ref().map(model::Unit::as_str)
                                == Some(source.unit.as_str())
                            && source.due == view.due.map(|date| date.to_string())
                    }
                    Operation::Observation { source } => {
                        node.statement.as_str() == format!("obligation {}", view.id)
                            && *source == source_key("obligation", &obligation_view_material(view))
                    }
                    _ => false,
                })
                && analysis.proof.roots.contains(&view.proof);
            if !valid {
                return Err(invalid_satisfaction_result(
                    &view.id,
                    view.proof,
                    "invalid obligation view does not match its typed source",
                ));
            }
            continue;
        };
        let Some(node) = analysis.proof.node(view.proof) else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "result proof is missing",
            ));
        };
        let Operation::ObligationBalance(balance) = &node.operation else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "valid obligation does not point at its balance certificate",
            ));
        };
        let Some(source) = analysis.proof.node(balance.obligation_proof) else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "obligation source proof is missing",
            ));
        };
        let Operation::ObligationObservation(source) = &source.operation else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "obligation balance is not bound to a typed source",
            ));
        };
        let expected_status = if remaining.is_zero() {
            ObligationStatus::Satisfied
        } else if *remaining == view.promised {
            ObligationStatus::Outstanding
        } else {
            ObligationStatus::PartiallySatisfied
        };
        if source.obligation != view.id
            || source.debtor != view.debtor
            || source.creditor != view.creditor
            || source.promised != view.promised.number
            || view.promised.unit.as_ref().map(model::Unit::as_str) != Some(source.unit.as_str())
            || source.due != view.due.map(|date| date.to_string())
            || balance.remaining != remaining.number
            || balance.unit != source.unit
            || view.status != expected_status
            || !analysis.proof.roots.contains(&view.proof)
        {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "obligation view does not match its source and balance certificates",
            ));
        }
    }

    for view in &analysis.settlement_histories {
        require_goal_proof(analysis, &format!("settlement:{}", view.id), view.proof)?;
        if !settlement_view_matches_source(view) {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "settlement view does not match its authored source",
            ));
        }
        let Some(unused) = &view.unused else {
            let Some(node) = analysis.proof.node(view.proof) else {
                return Err(invalid_satisfaction_result(
                    &view.id,
                    view.proof,
                    "result proof is missing",
                ));
            };
            match &node.operation {
                Operation::SettlementHistory(history) => {
                    if history.settlement != view.id
                        || history.kind != settlement_kind_text(view.kind)
                        || history.current != settlement_state_text(view.current)
                        || history.effective != view.effective
                        || history.amount != view.amount.number
                        || view.amount.unit.as_ref().map(model::Unit::as_str)
                            != Some(history.unit.as_str())
                        || !analysis.proof.roots.contains(&view.proof)
                    {
                        return Err(invalid_satisfaction_result(
                            &view.id,
                            view.proof,
                            "conflicted settlement view does not match its history certificate",
                        ));
                    }
                }
                Operation::Observation { source }
                    if !view.effective
                        && node.statement.as_str() == format!("settlement history {}", view.id)
                        && *source
                            == source_key(
                                "settlement-history",
                                &settlement_history_material(&view.source),
                            )
                        && analysis.proof.roots.contains(&view.proof) => {}
                _ => {
                    return Err(invalid_satisfaction_result(
                        &view.id,
                        view.proof,
                        "uncertified settlement result",
                    ));
                }
            }
            continue;
        };
        let Some(node) = analysis.proof.node(view.proof) else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "result proof is missing",
            ));
        };
        let Operation::SettlementBalance(balance) = &node.operation else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "valid settlement does not point at its balance certificate",
            ));
        };
        let history = node.inputs.iter().find_map(|input| {
            analysis
                .proof
                .node(*input)
                .and_then(|node| match &node.operation {
                    Operation::SettlementHistory(history) if history.settlement == view.id => {
                        Some(history.as_ref())
                    }
                    _ => None,
                })
        });
        let Some(history) = history else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "settlement balance has no matching history certificate",
            ));
        };
        if balance.settlement != view.id
            || balance.amount != view.amount.number
            || view.amount.unit.as_ref().map(model::Unit::as_str) != Some(balance.unit.as_str())
            || balance.unused != unused.number
            || history.kind != settlement_kind_text(view.kind)
            || history.current != settlement_state_text(view.current)
            || history.effective != view.effective
            || !analysis.proof.roots.contains(&view.proof)
        {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "settlement view does not match its history and balance certificates",
            ));
        }
    }

    for view in &analysis.satisfactions {
        let Some(node) = analysis.proof.node(view.proof) else {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "result proof is missing",
            ));
        };
        let mut expected_dependencies = vec![view.proof];
        if let Operation::SatisfactionAllocation(allocation) = &node.operation {
            expected_dependencies.push(allocation.satisfaction_proof);
        }
        expected_dependencies.sort_unstable();
        expected_dependencies.dedup();
        if analysis
            .dependencies
            .get(&format!("satisfaction:{}", view.id))
            != Some(&expected_dependencies)
        {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "satisfaction dependency set does not match its certificates",
            ));
        }
        let Operation::SatisfactionAllocation(allocation) = &node.operation else {
            let valid_source = match &node.operation {
                Operation::SatisfactionObservation(source) => {
                    source.satisfaction == view.id
                        && source.obligation == view.obligation
                        && source.settlement == view.settlement
                        && source.amount == view.amount.number
                        && view.amount.unit.as_ref().map(model::Unit::as_str)
                            == Some(source.unit.as_str())
                        && source.state == satisfaction_state_text(view.state)
                }
                Operation::Observation { source } => {
                    node.statement.as_str() == format!("satisfaction {}", view.id)
                        && *source == source_key("satisfaction", &satisfaction_view_material(view))
                }
                _ => false,
            };
            let valid_source =
                valid_source && !view.effective && analysis.proof.roots.contains(&view.proof);
            if !valid_source {
                return Err(invalid_satisfaction_result(
                    &view.id,
                    view.proof,
                    "ineffective satisfaction is not bound to its source observation",
                ));
            }
            continue;
        };
        let settlement_effective = analysis
            .proof
            .node(allocation.settlement_proof)
            .and_then(|node| match &node.operation {
                Operation::SettlementObservation(settlement) => settlement.history.last(),
                _ => None,
            })
            .is_some_and(|transition| transition.state == "settled");
        let effective = view.state == model::SatisfactionState::Applied
            && settlement_effective
            && certified_obligations.contains(view.obligation.as_str())
            && certified_settlements.contains(view.settlement.as_str());
        if allocation.satisfaction != view.id
            || allocation.obligation != view.obligation
            || allocation.settlement != view.settlement
            || allocation.amount != view.amount.number
            || view.amount.unit.as_ref().map(model::Unit::as_str) != Some(allocation.unit.as_str())
            || allocation.state != satisfaction_state_text(view.state)
            || view.effective != effective
            || !analysis.proof.roots.contains(&view.proof)
        {
            return Err(invalid_satisfaction_result(
                &view.id,
                view.proof,
                "satisfaction view does not match its allocation certificate",
            ));
        }
    }
    Ok(())
}

fn require_goal_proof(
    analysis: &Analysis,
    goal: &str,
    proof: ProofId,
) -> Result<(), AnalysisCheckError> {
    if !analysis
        .dependencies
        .get(goal)
        .is_some_and(|roots| roots.binary_search(&proof).is_ok())
    {
        return Err(AnalysisCheckError::InvalidDependencyIndex {
            reason: format!("goal `{goal}` does not contain its result proof"),
        });
    }
    Ok(())
}

fn authored_source_ids<'a>(
    proof: &'a Proof,
    kind: &str,
    statement_prefix: &str,
) -> BTreeSet<&'a str> {
    proof
        .nodes
        .values()
        .filter(|node| node.metadata.get("kind").is_some_and(|value| value == kind))
        .filter_map(|node| node.statement.as_str().strip_prefix(statement_prefix))
        .collect()
}

fn require_complete_views<'a>(
    kind: &str,
    expected: BTreeSet<&'a str>,
    actual: impl Iterator<Item = &'a str>,
) -> Result<(), AnalysisCheckError> {
    let actual = actual.collect::<Vec<_>>();
    let unique = actual.iter().copied().collect::<BTreeSet<_>>();
    if actual.len() != unique.len() || unique != expected {
        return Err(AnalysisCheckError::InvalidSatisfactionResult {
            subject: kind.into(),
            proof: ProofId::ZERO,
            reason: format!("{kind} views do not exactly cover authored sources"),
        });
    }
    Ok(())
}

fn obligation_view_material(view: &ObligationView) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        view.id,
        view.debtor,
        view.creditor,
        view.promised.canonical(),
        view.due
            .map(|date| date.to_string())
            .unwrap_or_else(|| "-".into())
    )
}

fn satisfaction_view_material(view: &SatisfactionView) -> String {
    format!(
        "{}|{}|{}|{}|{:?}",
        view.id,
        view.obligation,
        view.settlement,
        view.amount.canonical(),
        view.state
    )
}

fn settlement_view_matches_source(view: &SettlementHistoryView) -> bool {
    view.id == view.source.occurrence.as_str()
        && view.kind == view.source.kind
        && view.amount == view.source.amount
        && view.current
            == view
                .source
                .history
                .last()
                .map(|transition| transition.state)
                .unwrap_or(model::SettlementStateKind::Issued)
}

fn check_dependency_indexes(analysis: &Analysis) -> Result<(), AnalysisCheckError> {
    let mut expected_invalidations = BTreeMap::<String, Vec<String>>::new();
    for (goal, roots) in &analysis.dependencies {
        if roots.is_empty() || !roots.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(AnalysisCheckError::InvalidDependencyIndex {
                reason: format!("goal `{goal}` does not have a canonical dependency set"),
            });
        }
        for root in roots {
            if !analysis.proof.roots.contains(root) {
                return Err(AnalysisCheckError::InvalidDependencyIndex {
                    reason: format!("goal `{goal}` names a proof that is not a root"),
                });
            }
            let node = analysis.proof.node(*root).ok_or_else(|| {
                AnalysisCheckError::InvalidDependencyIndex {
                    reason: format!("goal `{goal}` names a missing proof"),
                }
            })?;
            if let Some(source) = node.metadata.get("invalidation") {
                expected_invalidations
                    .entry(source.clone())
                    .or_default()
                    .push(goal.clone());
            }
        }
    }
    for goals in expected_invalidations.values_mut() {
        goals.sort();
        goals.dedup();
    }
    if analysis.invalidations != expected_invalidations {
        return Err(AnalysisCheckError::InvalidDependencyIndex {
            reason: "reverse invalidations do not match proof dependencies".into(),
        });
    }
    Ok(())
}

fn invalid_satisfaction_result(subject: &str, proof: ProofId, reason: &str) -> AnalysisCheckError {
    AnalysisCheckError::InvalidSatisfactionResult {
        subject: subject.to_owned(),
        proof,
        reason: reason.to_owned(),
    }
}

fn ontology_settlement_state(
    state: model::SettlementStateKind,
) -> crate::ontology::SettlementState {
    use crate::ontology::SettlementState as Target;
    match state {
        model::SettlementStateKind::Issued => Target::Issued,
        model::SettlementStateKind::Authorized => Target::Authorized,
        model::SettlementStateKind::Presented => Target::Presented,
        model::SettlementStateKind::Pending => Target::Pending,
        model::SettlementStateKind::Settled => Target::Settled,
        model::SettlementStateKind::Returned => Target::Returned,
        model::SettlementStateKind::Reversed => Target::Reversed,
        model::SettlementStateKind::Rejected => Target::Rejected,
        model::SettlementStateKind::Cancelled => Target::Cancelled,
        model::SettlementStateKind::Refunded => Target::Refunded,
        model::SettlementStateKind::Disputed => Target::Disputed,
        model::SettlementStateKind::ChargedBack => Target::ChargedBack,
        model::SettlementStateKind::Represented => Target::Represented,
        model::SettlementStateKind::Resolved => Target::Resolved,
    }
}

fn typed_obligation_certificate(
    source: &model::SourceObligation,
) -> Option<ObligationObservationCertificate> {
    let unit = source.quantity.unit.as_ref()?.as_str().to_owned();
    if source.quantity.number.is_negative() || source.quantity.number.is_zero() {
        return None;
    }
    Some(ObligationObservationCertificate {
        obligation: source.occurrence.as_str().to_owned(),
        debtor: source.debtor.as_str().to_owned(),
        creditor: source.creditor.as_str().to_owned(),
        promised: source.quantity.number.clone(),
        unit,
        due: source.due.map(|date| date.to_string()),
    })
}

fn typed_settlement_certificate(
    source: &model::SourceSettlement,
) -> Option<SettlementObservationCertificate> {
    let unit = source.amount.unit.as_ref()?.as_str().to_owned();
    if source.amount.number.is_negative()
        || source.amount.number.is_zero()
        || source.instrument.as_str().is_empty()
        || !source_settlement_history_is_valid(source)
    {
        return None;
    }
    let history = source
        .history
        .iter()
        .map(|transition| SettlementTransition {
            state: settlement_state_text(transition.state).into(),
            at: transition.at.map(|date| date.to_string()),
        })
        .collect();
    Some(SettlementObservationCertificate {
        settlement: source.occurrence.as_str().to_owned(),
        kind: settlement_kind_text(source.kind).into(),
        from: source.from.as_str().to_owned(),
        to: source.to.as_str().to_owned(),
        instrument: source.instrument.as_str().to_owned(),
        amount: source.amount.number.clone(),
        unit,
        history,
    })
}

fn typed_satisfaction_certificate(
    source: &model::SourceSatisfaction,
) -> Option<SatisfactionObservationCertificate> {
    let unit = source.amount.unit.as_ref()?.as_str().to_owned();
    if source.amount.number.is_negative() || source.amount.number.is_zero() {
        return None;
    }
    Some(SatisfactionObservationCertificate {
        satisfaction: source.occurrence.as_str().to_owned(),
        obligation: source.obligation.as_str().to_owned(),
        settlement: source.settlement.as_str().to_owned(),
        amount: source.amount.number.clone(),
        unit,
        state: satisfaction_state_text(source.state).into(),
    })
}

fn settlement_kind_text(kind: model::SettlementKind) -> &'static str {
    match kind {
        model::SettlementKind::Ach => "ach",
        model::SettlementKind::Card => "card",
        model::SettlementKind::Check => "check",
    }
}

fn settlement_state_text(state: model::SettlementStateKind) -> &'static str {
    match state {
        model::SettlementStateKind::Issued => "issued",
        model::SettlementStateKind::Authorized => "authorized",
        model::SettlementStateKind::Presented => "presented",
        model::SettlementStateKind::Pending => "pending",
        model::SettlementStateKind::Settled => "settled",
        model::SettlementStateKind::Returned => "returned",
        model::SettlementStateKind::Reversed => "reversed",
        model::SettlementStateKind::Rejected => "rejected",
        model::SettlementStateKind::Cancelled => "cancelled",
        model::SettlementStateKind::Refunded => "refunded",
        model::SettlementStateKind::Disputed => "disputed",
        model::SettlementStateKind::ChargedBack => "charged-back",
        model::SettlementStateKind::Represented => "represented",
        model::SettlementStateKind::Resolved => "resolved",
    }
}

fn satisfaction_state_text(state: model::SatisfactionState) -> &'static str {
    match state {
        model::SatisfactionState::Proposed => "proposed",
        model::SatisfactionState::Applied => "applied",
        model::SatisfactionState::Reversed => "reversed",
    }
}

fn source_settlement_history_is_valid(source: &model::SourceSettlement) -> bool {
    if source.history.is_empty() {
        return false;
    }
    let mut previous = None;
    let mut previous_at = None;
    for transition in &source.history {
        if !settlement_transition_is_legal(previous, transition.state) {
            return false;
        }
        if let Some(at) = transition.at {
            if previous_at.is_some_and(|previous| at < previous) {
                return false;
            }
            previous_at = Some(at);
        }
        previous = Some(transition.state);
    }
    true
}

fn settlement_transition_is_legal(
    previous: Option<model::SettlementStateKind>,
    next: model::SettlementStateKind,
) -> bool {
    use model::SettlementStateKind::*;
    matches!(
        (previous, next),
        (None, Issued)
            | (Some(Issued), Authorized | Presented | Cancelled)
            | (Some(Authorized), Presented | Cancelled | Rejected)
            | (
                Some(Presented),
                Pending | Settled | Returned | Rejected | Cancelled
            )
            | (Some(Pending), Settled | Returned | Rejected | Cancelled)
            | (
                Some(Settled),
                Returned | Reversed | Refunded | Disputed | ChargedBack
            )
            | (Some(Disputed), Resolved | ChargedBack)
            | (Some(ChargedBack), Represented)
            | (Some(Represented), Pending | Settled | Rejected)
            | (Some(Returned), Presented | Cancelled)
            | (Some(Reversed), Presented | Cancelled)
            | (Some(Rejected), Presented | Cancelled)
    )
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

fn obligation_material(obligation: &model::SourceObligation) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        obligation.occurrence,
        obligation.debtor,
        obligation.creditor,
        obligation.quantity.canonical(),
        obligation
            .due
            .map(|date| date.to_string())
            .unwrap_or_else(|| "-".into())
    )
}

fn settlement_history_material(settlement: &model::SourceSettlement) -> String {
    let history = settlement
        .history
        .iter()
        .map(|transition| {
            format!(
                "{:?}@{}",
                transition.state,
                transition
                    .at
                    .map(|date| date.to_string())
                    .unwrap_or_else(|| "-".into())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}|{:?}|{}|{}|{}|{}|{}",
        settlement.occurrence,
        settlement.kind,
        settlement.from,
        settlement.to,
        settlement.instrument,
        settlement.amount.canonical(),
        history
    )
}

fn satisfaction_material(satisfaction: &model::SourceSatisfaction) -> String {
    format!(
        "{}|{}|{}|{}|{:?}",
        satisfaction.occurrence,
        satisfaction.obligation,
        satisfaction.settlement,
        satisfaction.amount.canonical(),
        satisfaction.state
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
    fn tampered_quote_answer_is_rejected_by_its_typed_source_certificate() {
        let source = r#"book tax-us
quote quote/one on 2026-09-20
  1 ABC = 52 USD
"#;
        let mut result = analyze(&parse_ledger(source).unwrap());
        result.quotes[0].quote.number = Exact::from(53_i64);
        assert!(matches!(
            result.check_semantics(),
            Err(AnalysisCheckError::InvalidQuoteResult { .. })
        ));
    }

    #[test]
    fn tampered_position_answer_is_rejected_by_its_typed_source_certificate() {
        let source = r#"book tax-us
observe position brokerage 10 ABC
"#;
        let mut result = analyze(&parse_ledger(source).unwrap());
        result.positions[0].quantity.number = Exact::from(11_i64);
        assert!(matches!(
            result.check_semantics(),
            Err(AnalysisCheckError::InvalidPositionResult { .. })
        ));
    }

    #[test]
    fn tampered_settlement_answer_is_rejected_by_its_typed_source_certificate() {
        let source = r#"book tax-us
observe settlement unknown-sale 100 USD into cash
"#;
        let mut result = analyze(&parse_ledger(source).unwrap());
        result.settlements[0].quantity.number = Exact::from(999_i64);
        assert!(matches!(
            result.check_semantics(),
            Err(AnalysisCheckError::InvalidSettlementResult { .. })
        ));
    }

    #[test]
    fn tampered_journal_line_is_rejected_by_its_typed_certificate() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
decide sell lot buy/one
observe settlement sell 500 USD into cash
"#;
        let mut result = analyze(&parse_ledger(source).unwrap());
        result.journal[0].lines[0].quantity.number = Exact::from(501_i64);
        assert!(matches!(
            result.check_semantics(),
            Err(AnalysisCheckError::InvalidJournalResult { .. })
        ));
    }

    #[test]
    fn settled_loss_uses_a_positive_debit_and_has_a_valid_proof() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 100 USD
  lot ?lot
decide sell lot buy/one
observe settlement sell 100 USD into cash
"#;
        let result = analyze(&parse_ledger(source).unwrap());
        assert_eq!(result.journal.len(), 1);
        assert_eq!(result.journal[0].lines[2].side, Side::Debit);
        assert_eq!(result.journal[0].lines[2].account, "loss:recognized");
        assert_eq!(
            result.journal[0].lines[2].quantity.number,
            Exact::from(100_i64)
        );
        result.check_proof().unwrap();
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
    fn authored_obligation_network_tracks_partial_returned_and_overallocated_payments() {
        let partial = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
settlement payment/a
  kind ach
  from customer
  to vendor
  instrument USD
  amount 60 USD
  state issued at 2026-01-01
  state presented at 2026-01-02
  state settled at 2026-01-03
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 60 USD
  state applied
"#,
            )
            .unwrap(),
        );
        assert_eq!(partial.obligations.len(), 1);
        assert_eq!(
            partial.obligations[0].status,
            ObligationStatus::PartiallySatisfied
        );
        assert_eq!(
            partial.obligations[0].remaining.as_ref().unwrap().number,
            Exact::from(40i64)
        );
        assert_eq!(
            partial.settlement_histories[0].current,
            model::SettlementStateKind::Settled
        );
        assert!(partial.settlement_histories[0].effective);
        assert_eq!(
            partial.settlement_histories[0]
                .unused
                .as_ref()
                .unwrap()
                .number,
            Exact::from(0i64)
        );
        assert!(partial.satisfactions[0].effective);
        partial.check_proof().unwrap();

        let returned = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
settlement payment/a
  kind check
  from customer
  to vendor
  instrument USD
  amount 100 USD
  state issued
  state presented
  state settled
  state returned
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 100 USD
  state applied
"#,
            )
            .unwrap(),
        );
        assert_eq!(
            returned.obligations[0].status,
            ObligationStatus::Outstanding
        );
        assert!(!returned.settlement_histories[0].effective);
        assert!(!returned.satisfactions[0].effective);
        assert_eq!(
            returned.settlement_histories[0]
                .unused
                .as_ref()
                .unwrap()
                .number,
            Exact::from(100i64)
        );

        let overallocated = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
obligation invoice/b
  debtor customer
  creditor vendor
  performance transfer 100 USD
obligation invoice/c
  debtor customer
  creditor vendor
  performance transfer 30 USD
settlement payment/a
  kind ach
  from customer
  to vendor
  instrument USD
  amount 100 USD
  state issued
  state presented
  state settled
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 60 USD
  state applied
satisfy allocation/b
  obligation invoice/b
  settlement payment/a
  amount 60 USD
  state applied
settlement payment/c
  kind ach
  from customer
  to vendor
  instrument USD
  amount 30 USD
  state issued
  state presented
  state settled
satisfy allocation/c
  obligation invoice/c
  settlement payment/c
  amount 30 USD
  state applied
"#,
            )
            .unwrap(),
        );
        assert!(overallocated.blocked());
        assert!(
            overallocated
                .issues
                .iter()
                .any(|issue| issue.code == IssueCode::ObligationConflict)
        );
        assert_eq!(overallocated.obligations.len(), 3);
        assert_eq!(
            overallocated.obligations[0].status,
            ObligationStatus::Invalid
        );
        assert_eq!(
            overallocated.obligations[1].status,
            ObligationStatus::Invalid
        );
        assert_eq!(
            overallocated.obligations[2].status,
            ObligationStatus::Satisfied
        );
        assert_eq!(overallocated.settlement_histories.len(), 2);
        assert!(
            overallocated
                .settlement_histories
                .iter()
                .find(|settlement| settlement.id == "payment/a")
                .unwrap()
                .unused
                .is_none()
        );
        assert_eq!(
            overallocated
                .settlement_histories
                .iter()
                .find(|settlement| settlement.id == "payment/c")
                .unwrap()
                .unused
                .as_ref()
                .unwrap()
                .number,
            Exact::from(0i64)
        );
        assert!(
            !overallocated
                .satisfactions
                .iter()
                .find(|satisfaction| satisfaction.id == "allocation/a")
                .unwrap()
                .effective
        );
        assert!(
            !overallocated
                .satisfactions
                .iter()
                .find(|satisfaction| satisfaction.id == "allocation/b")
                .unwrap()
                .effective
        );
        assert!(
            overallocated
                .satisfactions
                .iter()
                .find(|satisfaction| satisfaction.id == "allocation/c")
                .unwrap()
                .effective
        );
        overallocated.check_proof().unwrap();

        let mut malformed_ledger = parse_ledger(
            r#"book receivables
obligation invoice/invalid
  debtor customer
  creditor vendor
  performance transfer 100 USD
"#,
        )
        .unwrap();
        let LedgerForm::Obligation(obligation) = &mut malformed_ledger.forms[0] else {
            panic!("expected obligation source");
        };
        obligation.quantity.number = Exact::from(-1i64);
        let malformed = analyze(&malformed_ledger);
        assert_eq!(malformed.obligations.len(), 1);
        assert_eq!(malformed.obligations[0].status, ObligationStatus::Invalid);
        assert_eq!(malformed.obligations[0].remaining, None);
        let issue = malformed
            .issues
            .iter()
            .find(|issue| issue.code == IssueCode::ObligationConflict)
            .expect("invalid obligation constructor is surfaced");
        assert_eq!(
            issue.message,
            "invalid quantity: an obligation must promise a positive quantity"
        );
        malformed.check_proof().unwrap();
    }

    #[test]
    fn typed_satisfaction_certificates_reject_tampering() {
        let mut analysis = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 10 USD
settlement payment/a
  kind ach
  from customer
  to vendor
  instrument USD
  amount 10 USD
  state issued
  state presented
  state settled
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 10 USD
  state applied
"#,
            )
            .unwrap(),
        );
        assert!(
            analysis
                .proof
                .nodes
                .values()
                .any(|node| { matches!(node.operation, Operation::ObligationObservation(_)) })
        );
        assert!(
            analysis
                .proof
                .nodes
                .values()
                .any(|node| { matches!(node.operation, Operation::SettlementObservation(_)) })
        );
        let mut forged = analysis.clone();
        forged.obligations[0].status = ObligationStatus::Outstanding;
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged.settlement_histories[0].effective = false;
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged.satisfactions[0].effective = false;
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged
            .dependencies
            .get_mut("obligation:invoice/a")
            .unwrap()
            .pop();
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged
            .dependencies
            .get_mut("satisfaction:allocation/a")
            .unwrap()
            .pop();
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged.invalidations.clear();
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged.obligations.clear();
        forged.settlement_histories.clear();
        forged.satisfactions.clear();
        assert!(forged.check_proof().is_err());
        let mut forged = analysis.clone();
        forged.satisfactions[0].effective = false;
        forged.satisfactions[0].proof = forged.obligations[0].proof;
        assert!(forged.check_proof().is_err());

        let allocation = analysis
            .proof
            .nodes
            .values()
            .find(|node| matches!(node.operation, Operation::SatisfactionAllocation(_)))
            .expect("typed satisfaction allocation certificate");
        let allocation_id = allocation.id;
        let node = analysis.proof.nodes.get_mut(&allocation_id).unwrap();
        let Operation::SatisfactionAllocation(certificate) = &mut node.operation else {
            unreachable!();
        };
        certificate.amount = Exact::from(9i64);
        assert!(analysis.check_proof().is_err());

        let mut malformed_ledger = parse_ledger(
            r#"book receivables
settlement payment/invalid
  kind ach
  from customer
  to vendor
  amount 10 USD
  state issued
  state presented
  state settled
"#,
        )
        .unwrap();
        let LedgerForm::Settlement(settlement) = &mut malformed_ledger.forms[0] else {
            unreachable!();
        };
        settlement.amount.unit = None;
        let malformed = analyze(&malformed_ledger);
        malformed.check_proof().unwrap();
        let mut forged = malformed.clone();
        forged.settlement_histories[0].amount.number = Exact::from(11i64);
        assert!(forged.check_proof().is_err());
    }

    #[test]
    fn authored_settlement_history_preserves_order_and_rejects_illegal_transitions() {
        let analysis = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 10 USD
settlement payment/a
  kind card
  from customer
  to vendor
  instrument USD
  amount 10 USD
  state issued at 2026-01-03
  state settled at 2026-01-02
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 10 USD
  state applied
"#,
            )
            .unwrap(),
        );
        assert!(analysis.blocked());
        assert_eq!(analysis.obligations.len(), 1);
        assert_eq!(analysis.settlement_histories.len(), 1);
        assert_eq!(analysis.satisfactions.len(), 1);
        assert_eq!(analysis.obligations[0].status, ObligationStatus::Invalid);
        assert_eq!(
            analysis.settlement_histories[0].current,
            model::SettlementStateKind::Settled
        );
        assert!(!analysis.settlement_histories[0].effective);
        assert!(analysis.settlement_histories[0].unused.is_none());
        assert!(!analysis.satisfactions[0].effective);
        let issue = analysis
            .issues
            .iter()
            .find(|issue| issue.code == IssueCode::ObligationConflict)
            .expect("invalid settlement history is surfaced");
        assert_eq!(
            issue.message,
            "settlement payment/a moves backward from 2026-01-03 to 2026-01-02"
        );
        analysis.check_proof().unwrap();
    }

    #[test]
    fn charged_back_history_uses_the_canonical_source_spelling_in_proofs() {
        let analysis = analyze(
            &parse_ledger(
                r#"book receivables
settlement payment/a
  kind card
  from customer
  to vendor
  amount 10 USD
  state issued
  state presented
  state settled
  state charged-back
"#,
            )
            .unwrap(),
        );
        assert_eq!(
            analysis.settlement_histories[0].current,
            model::SettlementStateKind::ChargedBack
        );
        assert!(!analysis.settlement_histories[0].effective);
        analysis.check_proof().unwrap();
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
