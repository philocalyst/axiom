//! A small, deliberately boring semantic oracle for the V0 ledger.
//!
//! The production analyser is proof-producing and keeps a fair amount of
//! presentation state.  This module intentionally does not use it.  It walks
//! the source [`Ledger`] into a handful of sorted relations and uses only the
//! exact arithmetic in [`crate::exact`].  Keeping this evaluator independent
//! makes it useful as a differential oracle when the production analyser is
//! changed or optimized.
//!
//! The relations are named after the concepts in the semantic design:
//! `candidate_lot`, `selected_lot`, `valuation`, `basis`, `gain`,
//! `balances`/`positions`, `satisfies`, `recognized`, and `available`.
//! Unknowns and disagreements are represented as data; in particular, an
//! unresolved lot and disagreeing quotes never become a guessed answer.

use std::collections::{BTreeMap, BTreeSet};

use crate::exact::Exact;
use crate::model::{self, Date, Ledger, LedgerForm, LotSelector, Unit};

pub use crate::model::Quantity;

/// A source buy lowered to a lot, without any proof or source-location state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceLot {
    pub id: String,
    pub date: Date,
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub consideration: Quantity,
    pub fee: Option<Quantity>,
    pub basis: Quantity,
}

pub type Lot = ReferenceLot;

/// One admissible lot for one sale.  This relation is intentionally retained
/// even when the sale is recognized: it is the conditional result from which
/// the selected result is chosen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateLot {
    pub sale: String,
    pub lot: String,
    pub lot_id: String,
    pub date: Date,
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub basis: Quantity,
}

impl CandidateLot {
    fn key(&self) -> (&str, Date, &str) {
        (&self.sale, self.date, &self.lot)
    }
}

/// The result of lot selection for a sale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedLot {
    pub sale: String,
    pub lot_id: Option<String>,
    /// `selected_lot` is a convenient synonym for `lot_id` in serialized or
    /// ad-hoc consumers.
    pub selected_lot: Option<String>,
    pub candidates: Vec<String>,
    pub policy_lot: Option<String>,
    pub decision_lot: Option<String>,
    pub explicit_lot: Option<String>,
    pub status: SelectionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionStatus {
    Recognized,
    Ambiguous {
        candidates: Vec<String>,
    },
    /// More than one policy applies to the book.  No policy-dependent answer
    /// is allowed to leak through, even when one of the policies happens to
    /// sort first by name.
    PolicyConflict {
        policies: Vec<String>,
    },
    Conflict {
        policy_lot: String,
        decision_lot: String,
    },
    MissingLot,
    InvalidAmount,
}

impl SelectionStatus {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Recognized)
    }

    pub fn is_blocked(&self) -> bool {
        !self.is_complete()
    }
}

/// A conditional exact basis allocation for a sale/lot pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasisFact {
    pub sale: Option<String>,
    pub lot: String,
    pub lot_id: String,
    pub quantity: Quantity,
    pub basis: Quantity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BasisKind {
    Lot,
    Allocated,
}

/// `basis` includes both the acquisition basis (`Lot`) and every conditional
/// allocation (`Allocated`).  The `kind` field makes the distinction explicit
/// without overloading one numeric field with two meanings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasisRelation {
    pub sale: Option<String>,
    pub lot: String,
    pub lot_id: String,
    pub quantity: Quantity,
    pub basis: Quantity,
    pub kind: BasisKind,
}

/// A conditional or recognized exact gain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gain {
    pub sale: String,
    pub lot: String,
    pub lot_id: String,
    pub proceeds: Quantity,
    pub basis: Quantity,
    pub gain: Quantity,
}

pub type GainFact = Gain;

/// A quote as retained by the valuation relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuoteFact {
    pub id: String,
    pub date: Date,
    pub base: Quantity,
    pub quote: Quantity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValuationStatus {
    Unique,
    Conflict { quote_ids: Vec<String> },
    Unavailable,
}

/// Quotes with the same date and units form one valuation group.  A group is
/// unique only when every rate agrees by exact cross multiplication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Valuation {
    pub id: String,
    pub date: Date,
    pub base_unit: String,
    pub quote_unit: String,
    pub quotes: Vec<QuoteFact>,
    pub quote_ids: Vec<String>,
    pub effective: Option<QuoteFact>,
    pub status: ValuationStatus,
}

/// The status of a position observation after comparing it with authored
/// events.  This mirrors the evidence boundary: an observation is not
/// silently accepted just because it was seen first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PositionStatus {
    Observed,
    Reconciled,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub observed: Quantity,
    pub calculated: Quantity,
    pub status: PositionStatus,
}

pub type PositionFact = Position;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Balance {
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub observed: Option<Quantity>,
    pub status: PositionStatus,
}

pub type BalanceFact = Balance;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SatisfactionStatus {
    Satisfied,
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Satisfies {
    pub sale: String,
    pub settlement: Option<String>,
    pub amount: Option<Quantity>,
    pub into: Option<String>,
    pub status: SatisfactionStatus,
}

pub type SatisfiesFact = Satisfies;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recognized {
    pub sale: String,
    pub lot: Option<String>,
    pub basis: Option<Quantity>,
    pub gain: Option<Quantity>,
    pub status: SelectionStatus,
}

pub type Recognition = Recognized;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvailableStatus {
    Available,
    Ambiguous,
    Conflict,
    Unavailable,
}

/// Availability is kept separate from recognition.  A sale can have
/// available candidate lots while still being ambiguous or conflict-blocked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Available {
    pub sale: String,
    pub account: String,
    pub asset: String,
    pub candidates: Vec<String>,
    pub quantity: Quantity,
    pub status: AvailableStatus,
}

pub type AvailableFact = Available;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ReferenceIssueCode {
    AmbiguousLot,
    DecisionConflict,
    PolicyDecisionConflict,
    MissingLot,
    AmbiguousQuote,
    UnknownPolicy,
    IncompatibleUnit,
    InvalidAmount,
    PositionConflict,
    SettlementConflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceIssue {
    pub code: ReferenceIssueCode,
    pub message: String,
    pub sale: Option<String>,
}

/// The complete deterministic result of [`evaluate`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceResult {
    pub book: String,
    pub policy: Option<String>,
    pub policies: Vec<String>,
    pub decisions: Vec<String>,
    pub lots: Vec<ReferenceLot>,
    pub sales: Vec<ReferenceSale>,
    pub candidate_lot: Vec<CandidateLot>,
    pub selected_lot: Vec<SelectedLot>,
    pub valuation: Vec<Valuation>,
    pub basis: Vec<BasisRelation>,
    pub gain: Vec<Gain>,
    pub balances: Vec<Balance>,
    pub positions: Vec<Position>,
    pub satisfies: Vec<Satisfies>,
    pub recognized: Vec<Recognized>,
    pub available: Vec<Available>,
    pub issues: Vec<ReferenceIssue>,
}

pub type ReferenceAnalysis = ReferenceResult;

/// Sale-level convenience view.  Its shape deliberately follows the stable
/// semantic fields in `engine::SaleAnalysis`, while remaining independent of
/// the engine's proof types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceSale {
    pub id: String,
    pub date: Date,
    pub account: String,
    pub asset: String,
    pub quantity: Quantity,
    pub proceeds: Quantity,
    pub eligible_lots: Vec<String>,
    pub conditional_gains: Vec<Gain>,
    pub selected_lot: Option<String>,
    pub status: SelectionStatus,
}

impl ReferenceResult {
    pub fn blocked(&self) -> bool {
        self.recognized.iter().any(|fact| fact.status.is_blocked())
            || self
                .issues
                .iter()
                .any(|issue| !matches!(issue.code, ReferenceIssueCode::AmbiguousQuote))
    }

    pub fn sale(&self, id: &str) -> Option<&ReferenceSale> {
        self.sales.iter().find(|sale| sale.id == id)
    }

    pub fn selected(&self, id: &str) -> Option<&SelectedLot> {
        self.selected_lot
            .iter()
            .find(|selected| selected.sale == id)
    }

    pub fn selected_lot_id(&self, id: &str) -> Option<&str> {
        self.selected(id)
            .and_then(|selected| selected.lot_id.as_deref())
    }

    pub fn candidates(&self, id: &str) -> Vec<&CandidateLot> {
        self.candidate_lot
            .iter()
            .filter(|candidate| candidate.sale == id)
            .collect()
    }

    pub fn recognized_gain(&self, id: &str) -> Option<&Gain> {
        let selected = self.selected_lot_id(id)?;
        self.gain
            .iter()
            .find(|gain| gain.sale == id && gain.lot_id == selected)
    }

    pub fn valuation_group(&self, id: &str) -> Option<&Valuation> {
        self.valuation.iter().find(|valuation| valuation.id == id)
    }

    /// A stable, relation-only projection convenient for differential tests.
    /// It intentionally excludes vector order, so adding unrelated evidence
    /// or changing source order cannot alter a semantic comparison.
    pub fn relation_key(&self) -> String {
        let mut chunks = Vec::new();
        chunks.push(format!(
            "meta|book={:?}|policy={:?}|policies={:?}|decisions={:?}",
            self.book, self.policy, self.policies, self.decisions
        ));
        for lot in &self.lots {
            chunks.push(format!("lot|{lot:?}"));
        }
        for sale in &self.sales {
            chunks.push(format!("sale|{sale:?}"));
        }
        for candidate in &self.candidate_lot {
            chunks.push(format!("candidate_lot|{candidate:?}"));
        }
        for selected in &self.selected_lot {
            chunks.push(format!("selected_lot|{selected:?}"));
        }
        for valuation in &self.valuation {
            chunks.push(format!("valuation|{valuation:?}"));
        }
        for basis in &self.basis {
            chunks.push(format!("basis|{basis:?}"));
        }
        for gain in &self.gain {
            chunks.push(format!("gain|{gain:?}"));
        }
        for balance in &self.balances {
            chunks.push(format!("balance|{balance:?}"));
        }
        for position in &self.positions {
            chunks.push(format!("position|{position:?}"));
        }
        for satisfies in &self.satisfies {
            chunks.push(format!("satisfies|{satisfies:?}"));
        }
        for recognized in &self.recognized {
            chunks.push(format!("recognized|{recognized:?}"));
        }
        for available in &self.available {
            chunks.push(format!("available|{available:?}"));
        }
        for issue in &self.issues {
            chunks.push(format!("issue|{issue:?}"));
        }
        chunks.sort();
        chunks.join("\n")
    }
}

/// Evaluate a ledger with the independent reference semantics.
pub fn evaluate(ledger: &Ledger) -> ReferenceResult {
    let book = ledger.book.as_str().to_owned();
    let mut lots = Vec::new();
    let mut raw_sales = Vec::new();
    let mut raw_quotes = Vec::new();
    let mut raw_positions = Vec::new();
    let mut raw_settlements = Vec::new();
    let mut policies = Vec::new();
    let mut decisions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut issues = Vec::new();

    for form in &ledger.forms {
        match form {
            LedgerForm::Buy(buy) => match make_lot(buy) {
                Some(lot) => lots.push(lot),
                None => issues.push(ReferenceIssue {
                    code: ReferenceIssueCode::InvalidAmount,
                    message: format!(
                        "buy `{}` has an unresolved or incompatible quantity",
                        buy.label
                    ),
                    sale: None,
                }),
            },
            LedgerForm::Sell(sell) => raw_sales.push(sell.clone()),
            LedgerForm::Quote(quote) => {
                if let Some(value) = make_quote(quote) {
                    raw_quotes.push(value);
                }
            }
            LedgerForm::ObservePosition(position) => raw_positions.push(position.clone()),
            LedgerForm::ObserveSettlement(settlement) => raw_settlements.push(settlement.clone()),
            LedgerForm::UsePolicy(use_policy) if use_policy.book.as_str() == book => {
                let policy = use_policy.policy.as_str().to_owned();
                if !policies.contains(&policy) {
                    policies.push(policy);
                }
            }
            LedgerForm::UsePolicy(_) => {}
            LedgerForm::Decide(decision) => {
                decisions
                    .entry(decision.sale.as_str().to_owned())
                    .or_default()
                    .insert(decision.lot.as_str().to_owned());
            }
        }
    }

    lots.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.account.cmp(&right.account))
            .then_with(|| left.asset.cmp(&right.asset))
    });
    policies.sort();
    let policy_conflict = policies.len() > 1;
    let active_policy = (!policy_conflict)
        .then(|| policies.first().cloned())
        .flatten();
    if policy_conflict {
        issues.push(ReferenceIssue {
            code: ReferenceIssueCode::PolicyDecisionConflict,
            message: format!(
                "policies `{}` both apply to book `{book}`",
                policies.join("`, `")
            ),
            sale: None,
        });
    }
    if let Some(policy) = active_policy.as_deref()
        && policy != "lots/fifo"
    {
        issues.push(ReferenceIssue {
            code: ReferenceIssueCode::UnknownPolicy,
            message: format!("policy `{policy}` has no built-in resolver"),
            sale: None,
        });
    }

    let valuation = build_valuations(raw_quotes);
    for group in &valuation {
        if let ValuationStatus::Conflict { quote_ids } = &group.status {
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::AmbiguousQuote,
                message: format!("quotes {} disagree for {}", quote_ids.join(", "), group.id),
                sale: None,
            });
        }
    }

    let mut candidate_lot = Vec::new();
    let mut selected_lot = Vec::new();
    let mut basis = Vec::new();
    let mut gains = Vec::new();
    let mut sales = Vec::new();
    let sale_count = raw_sales.len();
    let mut sale_set = BTreeSet::new();

    for sell in &raw_sales {
        sale_set.insert(sell.label.clone());
        let quantity = quantity_of(&sell.quantity).unwrap_or_else(zero_quantity);
        let proceeds = quantity_of(&sell.proceeds).unwrap_or_else(zero_quantity);
        let Some(asset) = sell
            .quantity
            .unit
            .as_ref()
            .map(|unit| unit.as_str().to_owned())
        else {
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::InvalidAmount,
                message: format!("sale `{}` has an unresolved holding", sell.label),
                sale: Some(sell.label.clone()),
            });
            let selected = SelectedLot {
                sale: sell.label.clone(),
                lot_id: None,
                selected_lot: None,
                candidates: Vec::new(),
                policy_lot: None,
                decision_lot: None,
                explicit_lot: explicit_lot(sell),
                status: SelectionStatus::InvalidAmount,
            };
            selected_lot.push(selected.clone());
            sales.push(ReferenceSale {
                id: sell.label.clone(),
                date: sell.date,
                account: sell.from.as_str().to_owned(),
                asset: "?asset".into(),
                quantity,
                proceeds,
                eligible_lots: Vec::new(),
                conditional_gains: Vec::new(),
                selected_lot: None,
                status: SelectionStatus::InvalidAmount,
            });
            continue;
        };

        let mut eligible: Vec<&ReferenceLot> = lots
            .iter()
            .filter(|lot| {
                lot.date <= sell.date
                    && lot.account == sell.from.as_str()
                    && lot.asset == asset
                    && lot.quantity.unit == quantity.unit
                    && lot.quantity.number >= quantity.number
            })
            .collect();
        eligible.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.id.cmp(&right.id))
        });
        let eligible_ids = eligible
            .iter()
            .map(|lot| lot.id.clone())
            .collect::<Vec<_>>();

        // Acquisition basis is useful even when a sale is ambiguous.
        for lot in &eligible {
            candidate_lot.push(CandidateLot {
                sale: sell.label.clone(),
                lot: lot.id.clone(),
                lot_id: lot.id.clone(),
                date: lot.date,
                account: lot.account.clone(),
                asset: lot.asset.clone(),
                quantity: lot.quantity.clone(),
                basis: lot.basis.clone(),
            });
        }

        let explicit = explicit_lot(sell);
        let decision_values = decisions
            .get(&sell.label)
            .map(|values| values.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let decision_conflict = decision_values.len() > 1;
        let decision = (decision_values.len() == 1).then(|| decision_values[0].clone());
        let requested = explicit.clone().or_else(|| decision.clone());
        let requested_invalid = requested
            .as_ref()
            .is_some_and(|name| !eligible_ids.iter().any(|id| id == name));
        let fifo_lot = if active_policy.as_deref() == Some("lots/fifo") {
            eligible.first().and_then(|first| {
                let same_day = eligible.iter().filter(|lot| lot.date == first.date).count();
                (same_day == 1).then(|| first.id.clone())
            })
        } else {
            None
        };

        let mut status;
        let mut chosen = None;
        if sale_count > 1 {
            status = SelectionStatus::MissingLot;
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::MissingLot,
                message: format!(
                    "sale `{}` is blocked: V0 does not allocate lots across multiple sales",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
            });
        } else if policy_conflict {
            status = SelectionStatus::PolicyConflict {
                policies: policies.clone(),
            };
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::PolicyDecisionConflict,
                message: format!(
                    "policy selection is ambiguous for sale `{}`: {}",
                    sell.label,
                    policies.join("`, `")
                ),
                sale: Some(sell.label.clone()),
            });
        } else if decision_conflict {
            status = SelectionStatus::Ambiguous {
                candidates: decision_values.clone(),
            };
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::DecisionConflict,
                message: format!(
                    "decisions for sale `{}` select both `{}`",
                    sell.label,
                    decision_values.join("`, `")
                ),
                sale: Some(sell.label.clone()),
            });
        } else if let (Some(explicit_name), Some(decision_name)) =
            (explicit.clone(), decision.clone())
            && explicit_name != decision_name
        {
            status = SelectionStatus::Conflict {
                policy_lot: explicit_name.clone(),
                decision_lot: decision_name.clone(),
            };
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::PolicyDecisionConflict,
                message: format!(
                    "explicit lot `{explicit_name}` and decision `{decision_name}` disagree for sale `{}`",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
            });
        } else if let (Some(policy_name), Some(requested_name)) =
            (fifo_lot.clone(), requested.clone())
            && policy_name != requested_name
        {
            status = SelectionStatus::Conflict {
                policy_lot: policy_name.clone(),
                decision_lot: requested_name.clone(),
            };
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::PolicyDecisionConflict,
                message: format!(
                    "FIFO selects `{policy_name}` but decision selects `{requested_name}` for sale `{}`",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
            });
        } else if let Some(policy_name) = fifo_lot.clone() {
            chosen = Some(policy_name);
            status = SelectionStatus::Recognized;
        } else if let Some(requested_name) = requested.clone() {
            if requested_invalid {
                status = SelectionStatus::MissingLot;
                issues.push(ReferenceIssue {
                    code: ReferenceIssueCode::MissingLot,
                    message: format!(
                        "lot decision `{requested_name}` is not eligible for sale `{}`",
                        sell.label
                    ),
                    sale: Some(sell.label.clone()),
                });
            } else {
                chosen = Some(requested_name);
                status = SelectionStatus::Recognized;
            }
        } else if eligible.len() == 1 {
            chosen = eligible.first().map(|lot| lot.id.clone());
            status = SelectionStatus::Recognized;
        } else if eligible.is_empty() {
            status = SelectionStatus::MissingLot;
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::MissingLot,
                message: format!("no eligible lot for sale `{}`", sell.label),
                sale: Some(sell.label.clone()),
            });
        } else {
            status = SelectionStatus::Ambiguous {
                candidates: eligible_ids.clone(),
            };
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::AmbiguousLot,
                message: format!(
                    "sale `{}` has multiple eligible lots; use a policy or decision",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
            });
        }

        // Conditional arithmetic is independent of selection.  A unit
        // mismatch therefore leaves all conditional gains unavailable rather
        // than converting through an implicit quote.
        let mut conditional_gains = Vec::new();
        for lot in &eligible {
            if let Some(gain) = conditional_gain(&sell.label, lot, &quantity, &proceeds) {
                basis.push(BasisRelation {
                    sale: Some(sell.label.clone()),
                    lot: lot.id.clone(),
                    lot_id: lot.id.clone(),
                    quantity: quantity.clone(),
                    basis: gain.basis.clone(),
                    kind: BasisKind::Allocated,
                });
                gains.push(gain.clone());
                conditional_gains.push(gain);
            }
        }
        if chosen.is_some()
            && !conditional_gains
                .iter()
                .any(|gain| Some(&gain.lot_id) == chosen.as_ref())
        {
            status = SelectionStatus::InvalidAmount;
            chosen = None;
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::IncompatibleUnit,
                message: format!(
                    "sale `{}` proceeds and eligible lot basis use incompatible units",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
            });
        }

        let selected = SelectedLot {
            sale: sell.label.clone(),
            lot_id: chosen.clone(),
            selected_lot: chosen.clone(),
            candidates: match &status {
                SelectionStatus::Ambiguous { candidates } => candidates.clone(),
                _ => eligible_ids.clone(),
            },
            policy_lot: fifo_lot,
            decision_lot: decision,
            explicit_lot: explicit,
            status: status.clone(),
        };
        selected_lot.push(selected);
        sales.push(ReferenceSale {
            id: sell.label.clone(),
            date: sell.date,
            account: sell.from.as_str().to_owned(),
            asset,
            quantity,
            proceeds,
            eligible_lots: eligible_ids,
            conditional_gains,
            selected_lot: chosen,
            status,
        });
    }

    // Acquisition basis is a relation in its own right, even if no sale
    // refers to a lot.
    for lot in &lots {
        basis.push(BasisRelation {
            sale: None,
            lot: lot.id.clone(),
            lot_id: lot.id.clone(),
            quantity: lot.quantity.clone(),
            basis: lot.basis.clone(),
            kind: BasisKind::Lot,
        });
    }

    let (positions, balances) = build_positions_and_balances(&lots, &raw_sales, raw_positions);
    issues.extend(position_issues(&positions));

    let (satisfies, settlement_issues) = build_satisfaction(&sales, raw_settlements, &sale_set);
    issues.extend(settlement_issues);

    let recognized = sales
        .iter()
        .map(|sale| {
            let recognized_gain = sale.selected_lot.as_ref().and_then(|lot| {
                sale.conditional_gains
                    .iter()
                    .find(|gain| &gain.lot_id == lot)
            });
            Recognized {
                sale: sale.id.clone(),
                lot: sale.selected_lot.clone(),
                basis: recognized_gain.map(|gain| gain.basis.clone()),
                gain: recognized_gain.map(|gain| gain.gain.clone()),
                status: sale.status.clone(),
            }
        })
        .collect::<Vec<_>>();

    let available = sales
        .iter()
        .map(|sale| Available {
            sale: sale.id.clone(),
            account: sale.account.clone(),
            asset: sale.asset.clone(),
            candidates: sale.eligible_lots.clone(),
            quantity: sale.quantity.clone(),
            status: match sale.status {
                SelectionStatus::Recognized => AvailableStatus::Available,
                SelectionStatus::Ambiguous { .. } => AvailableStatus::Ambiguous,
                SelectionStatus::PolicyConflict { .. } => AvailableStatus::Conflict,
                SelectionStatus::Conflict { .. } => AvailableStatus::Conflict,
                SelectionStatus::MissingLot | SelectionStatus::InvalidAmount => {
                    AvailableStatus::Unavailable
                }
            },
        })
        .collect::<Vec<_>>();

    lots.sort_by(|left, right| left.id.cmp(&right.id));
    sales.sort_by(|left, right| left.id.cmp(&right.id));
    candidate_lot.sort_by(|left, right| left.key().cmp(&right.key()));
    selected_lot.sort_by(|left, right| left.sale.cmp(&right.sale));
    basis.sort_by(|left, right| {
        left.sale
            .cmp(&right.sale)
            .then_with(|| left.lot_id.cmp(&right.lot_id))
            .then_with(|| (left.kind as u8).cmp(&(right.kind as u8)))
    });
    gains.sort_by(|left, right| {
        left.sale
            .cmp(&right.sale)
            .then_with(|| left.lot_id.cmp(&right.lot_id))
    });
    issues.sort_by(|left, right| {
        left.sale
            .cmp(&right.sale)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });

    ReferenceResult {
        book,
        policy: active_policy,
        policies,
        decisions: decisions
            .values()
            .flat_map(|values| values.iter().cloned())
            .collect(),
        lots,
        sales,
        candidate_lot,
        selected_lot,
        valuation,
        basis,
        gain: gains,
        balances,
        positions,
        satisfies,
        recognized,
        available,
        issues,
    }
}

/// Conventional aliases make the oracle pleasant to use from differential
/// tests and from callers that call the production entry point `analyze`.
pub fn analyze(ledger: &Ledger) -> ReferenceResult {
    evaluate(ledger)
}

pub fn reference(ledger: &Ledger) -> ReferenceResult {
    evaluate(ledger)
}

fn make_lot(buy: &model::Buy) -> Option<ReferenceLot> {
    let asset = buy.quantity.unit.as_ref()?.as_str().to_owned();
    let quantity = quantity_of(&buy.quantity)?;
    let consideration = quantity_of(&buy.cost)?;
    let fee = match buy.fee.as_ref() {
        None => None,
        Some(fee) if fee.number.is_zero() && fee.unit.is_none() => Some(Quantity::typed(
            fee.number.clone(),
            consideration.unit.clone()?,
        )),
        Some(fee) => Some(quantity_of(fee)?),
    };
    if fee
        .as_ref()
        .is_some_and(|fee| fee.unit != consideration.unit)
    {
        return None;
    }
    let basis_amount = consideration.number.checked_add(
        &fee.as_ref()
            .map(|fee| fee.number.clone())
            .unwrap_or_else(|| Exact::from(0i64)),
    );
    Some(ReferenceLot {
        id: buy.label.clone(),
        date: buy.date,
        account: buy.into.as_str().to_owned(),
        asset,
        quantity,
        consideration: consideration.clone(),
        fee,
        basis: Quantity::new(basis_amount, consideration.unit).ok()?,
    })
}

fn make_quote(quote: &model::Quote) -> Option<QuoteFact> {
    Some(QuoteFact {
        id: quote.label.clone(),
        date: quote.date,
        base: quantity_of(&quote.base)?,
        quote: quantity_of(&quote.counter)?,
    })
}

fn quantity_of(quantity: &model::Quantity) -> Option<Quantity> {
    quantity.unit.as_ref()?;
    Some(quantity.clone())
}

fn unit_name(quantity: &Quantity) -> String {
    quantity
        .unit
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "?".into())
}

fn zero_quantity() -> Quantity {
    Quantity::zero()
}

fn explicit_lot(sell: &model::Sell) -> Option<String> {
    match &sell.lot {
        LotSelector::Explicit(lot) => Some(lot.as_str().to_owned()),
        LotSelector::Hole(_) => None,
    }
}

fn conditional_gain(
    sale: &str,
    lot: &ReferenceLot,
    sold: &Quantity,
    proceeds: &Quantity,
) -> Option<Gain> {
    if lot.quantity.unit != sold.unit || proceeds.unit != lot.basis.unit || proceeds.unit.is_none()
    {
        return None;
    }
    let ratio = sold.number.checked_div(&lot.quantity.number).ok()?;
    let allocated_basis = lot.basis.number.checked_mul(&ratio);
    let gain = proceeds.number.checked_sub(&allocated_basis);
    Some(Gain {
        sale: sale.to_owned(),
        lot: lot.id.clone(),
        lot_id: lot.id.clone(),
        proceeds: proceeds.clone(),
        basis: Quantity::new(allocated_basis, lot.basis.unit.clone()).ok()?,
        gain: Quantity::new(gain, proceeds.unit.clone()).ok()?,
    })
}

fn build_valuations(quotes: Vec<QuoteFact>) -> Vec<Valuation> {
    let mut groups: BTreeMap<String, Vec<QuoteFact>> = BTreeMap::new();
    for quote in quotes {
        let id = format!(
            "{}:{}:{}",
            quote.date,
            unit_name(&quote.base),
            unit_name(&quote.quote)
        );
        groups.entry(id).or_default().push(quote);
    }
    let mut result = groups
        .into_iter()
        .filter_map(|(id, mut quotes)| {
            quotes.sort_by(|left, right| left.id.cmp(&right.id));
            let first = quotes.first()?.clone();
            let quote_ids = quotes
                .iter()
                .map(|quote| quote.id.clone())
                .collect::<Vec<_>>();
            let rates_agree = quotes.iter().all(|quote| {
                quote.base.number.checked_mul(&first.quote.number)
                    == first.base.number.checked_mul(&quote.quote.number)
            });
            Some(Valuation {
                id,
                date: first.date,
                base_unit: unit_name(&first.base),
                quote_unit: unit_name(&first.quote),
                quotes,
                quote_ids: quote_ids.clone(),
                effective: rates_agree.then_some(first),
                status: if rates_agree {
                    ValuationStatus::Unique
                } else {
                    ValuationStatus::Conflict { quote_ids }
                },
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.id.cmp(&right.id));
    result
}

#[derive(Clone)]
struct ObservedPosition {
    account: String,
    quantity: Quantity,
}

fn build_positions_and_balances(
    lots: &[ReferenceLot],
    sales: &[model::Sell],
    observations: Vec<model::PositionObservation>,
) -> (Vec<Position>, Vec<Balance>) {
    let mut observed = observations
        .into_iter()
        .filter_map(|observation| {
            quantity_of(&observation.quantity).map(|quantity| ObservedPosition {
                account: observation.account.as_str().to_owned(),
                quantity,
            })
        })
        .collect::<Vec<_>>();
    observed.sort_by(|left, right| {
        left.account
            .cmp(&right.account)
            .then_with(|| unit_name(&left.quantity).cmp(&unit_name(&right.quantity)))
            .then_with(|| left.quantity.number.cmp(&right.quantity.number))
    });

    let mut keys = BTreeSet::new();
    for lot in lots {
        keys.insert((lot.account.clone(), lot.asset.clone()));
    }
    for sale in sales {
        if let Some(unit) = sale.quantity.unit.as_ref() {
            keys.insert((sale.from.as_str().to_owned(), unit.as_str().to_owned()));
        }
    }
    for position in &observed {
        keys.insert((position.account.clone(), unit_name(&position.quantity)));
    }

    let mut positions = Vec::new();
    for position in observed {
        let asset = unit_name(&position.quantity);
        let calculated_amount = calculated_amount(lots, sales, &position.account, &asset);
        let calculated = Quantity::typed(
            calculated_amount,
            position
                .quantity
                .unit
                .clone()
                .expect("observed quantities have units"),
        );
        let same_observation = keys.contains(&(position.account.clone(), asset.clone()));
        let has_authored = lots
            .iter()
            .any(|lot| lot.account == position.account && lot.asset == asset)
            || sales.iter().any(|sale| {
                sale.from.as_str() == position.account
                    && sale
                        .quantity
                        .unit
                        .as_ref()
                        .is_some_and(|unit| unit.as_str() == asset)
            });
        let status = if !same_observation || !has_authored {
            PositionStatus::Observed
        } else if calculated == position.quantity {
            PositionStatus::Reconciled
        } else {
            PositionStatus::Conflict
        };
        positions.push(Position {
            account: position.account,
            asset,
            quantity: position.quantity.clone(),
            observed: position.quantity,
            calculated,
            status,
        });
    }
    positions.sort_by(|left, right| {
        left.account
            .cmp(&right.account)
            .then_with(|| left.asset.cmp(&right.asset))
            .then_with(|| left.quantity.number.cmp(&right.quantity.number))
    });

    let mut balances = keys
        .into_iter()
        .map(|(account, asset)| {
            let unit = Unit::new(asset.clone()).expect("asset keys are nonempty");
            let quantity = Quantity::typed(calculated_amount(lots, sales, &account, &asset), unit);
            let observations = positions
                .iter()
                .filter(|position| position.account == account && position.asset == asset)
                .collect::<Vec<_>>();
            let observed_value = observations
                .first()
                .map(|position| position.observed.clone());
            let status = if observations
                .iter()
                .any(|position| matches!(position.status, PositionStatus::Conflict))
            {
                PositionStatus::Conflict
            } else if observations
                .iter()
                .any(|position| matches!(position.status, PositionStatus::Reconciled))
            {
                PositionStatus::Reconciled
            } else {
                PositionStatus::Observed
            };
            Balance {
                account,
                asset,
                quantity,
                observed: observed_value,
                status,
            }
        })
        .collect::<Vec<_>>();
    balances.sort_by(|left, right| {
        left.account
            .cmp(&right.account)
            .then_with(|| left.asset.cmp(&right.asset))
    });
    (positions, balances)
}

fn calculated_amount(
    lots: &[ReferenceLot],
    sales: &[model::Sell],
    account: &str,
    asset: &str,
) -> Exact {
    let acquired = lots
        .iter()
        .filter(|lot| lot.account == account && lot.asset == asset)
        .fold(Exact::from(0i64), |sum, lot| {
            sum.checked_add(&lot.quantity.number)
        });
    sales
        .iter()
        .filter(|sale| {
            sale.from.as_str() == account
                && sale
                    .quantity
                    .unit
                    .as_ref()
                    .is_some_and(|unit| unit.as_str() == asset)
        })
        .fold(acquired, |sum, sale| sum.checked_sub(&sale.quantity.number))
}

fn position_issues(positions: &[Position]) -> Vec<ReferenceIssue> {
    let mut grouped: BTreeMap<(String, String), Vec<&Position>> = BTreeMap::new();
    for position in positions {
        grouped
            .entry((position.account.clone(), position.asset.clone()))
            .or_default()
            .push(position);
    }
    grouped
        .into_iter()
        .filter_map(|((account, asset), positions)| {
            let distinct = positions
                .iter()
                .map(|position| position.observed.canonical())
                .collect::<BTreeSet<_>>();
            if distinct.len() > 1
                || positions
                    .iter()
                    .any(|position| matches!(position.status, PositionStatus::Conflict))
            {
                Some(ReferenceIssue {
                    code: ReferenceIssueCode::PositionConflict,
                    message: format!(
                        "position observations for `{account}` and `{asset}` conflict"
                    ),
                    sale: None,
                })
            } else {
                None
            }
        })
        .collect()
}

fn build_satisfaction(
    sales: &[ReferenceSale],
    observations: Vec<model::SettlementObservation>,
    sale_set: &BTreeSet<String>,
) -> (Vec<Satisfies>, Vec<ReferenceIssue>) {
    let mut raw = observations
        .into_iter()
        .filter_map(|observation| {
            quantity_of(&observation.amount).map(|amount| {
                (
                    observation.sale.as_str().to_owned(),
                    amount,
                    observation.into.map(|account| account.as_str().to_owned()),
                )
            })
        })
        .collect::<Vec<_>>();
    raw.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.canonical().cmp(&right.1.canonical()))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut issues = Vec::new();
    for (reference, _, _) in &raw {
        if !sale_set.contains(reference) {
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::MissingLot,
                message: format!("settlement observation `{reference}` does not match a sale"),
                sale: Some(reference.clone()),
            });
        }
    }

    let mut result = Vec::new();
    for sale in sales {
        let matching = raw
            .iter()
            .filter(|(reference, amount, into)| {
                reference == &sale.id && amount == &sale.proceeds && into.is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        let all_for_sale = raw
            .iter()
            .filter(|(reference, _, _)| reference == &sale.id)
            .cloned()
            .collect::<Vec<_>>();
        let distinct = all_for_sale
            .iter()
            .map(|(_, amount, into)| {
                format!("{}|{}", amount.canonical(), into.as_deref().unwrap_or("?"))
            })
            .collect::<BTreeSet<_>>();
        if distinct.len() > 1 {
            issues.push(ReferenceIssue {
                code: ReferenceIssueCode::SettlementConflict,
                message: format!("settlement observations for `{}` conflict", sale.id),
                sale: Some(sale.id.clone()),
            });
        }
        let (settlement, status) = if let Some((_, amount, _)) = matching.first() {
            (
                Some(format!("{}:{}", sale.id, amount.canonical())),
                SatisfactionStatus::Satisfied,
            )
        } else if all_for_sale.is_empty() {
            (None, SatisfactionStatus::Unavailable)
        } else {
            (None, SatisfactionStatus::Conflict)
        };
        let (amount, into) = matching
            .first()
            .map(|(_, amount, into)| (Some(amount.clone()), into.clone()))
            .unwrap_or((None, None));
        result.push(Satisfies {
            sale: sale.id.clone(),
            settlement,
            amount,
            into,
            status,
        });
    }
    result.sort_by(|left, right| left.sale.cmp(&right.sale));
    (result, issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use crate::parser::parse_ledger;

    fn fifo_fixture() -> &'static str {
        r#"book tax-us
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
quote quote/one on 2026-09-20
  1 ABC = 52 USD
quote quote/two on 2026-09-20
  1 ABC = 53 USD
use lots/fifo for tax-us
"#
    }

    #[test]
    fn reference_preserves_fifo_candidates_and_quote_conflict() {
        let ledger = parse_ledger(fifo_fixture()).unwrap();
        let reference = evaluate(&ledger);
        let production = engine::analyze(&ledger);
        let sale = reference.sale("sell").unwrap();
        let production_sale = production.sale("sell").unwrap();
        assert_eq!(sale.eligible_lots, production_sale.eligible_lots);
        assert_eq!(sale.selected_lot, production_sale.selected_lot);
        assert_eq!(
            sale.conditional_gains[0].gain.number,
            production_sale.conditional_gains[0].gain.number
        );
        assert!(matches!(sale.status, SelectionStatus::Recognized));
        assert!(
            reference
                .valuation
                .iter()
                .any(|valuation| matches!(valuation.status, ValuationStatus::Conflict { .. }))
        );
    }

    #[test]
    fn reference_keeps_policy_decision_conflict_explicit() {
        let source = format!("{}decide sell lot buy/two\n", fifo_fixture());
        let ledger = parse_ledger(&source).unwrap();
        let reference = evaluate(&ledger);
        let production = engine::analyze(&ledger);
        let sale = reference.sale("sell").unwrap();
        let production_sale = production.sale("sell").unwrap();
        assert_eq!(sale.selected_lot, production_sale.selected_lot);
        assert!(matches!(sale.status, SelectionStatus::Conflict { .. }));
        assert!(reference.gain.len() >= 2);
        assert!(production_sale.conditional_gains.len() >= 2);
    }

    #[test]
    fn source_order_and_unrelated_evidence_do_not_change_relations() {
        let source = fifo_fixture();
        let reordered = r#"book tax-us
quote quote/two on 2026-09-20
  1 ABC = 53 USD
buy buy/two on 2026-02-04
  10 ABC into brokerage
  for 300 USD
  fee 1 USD
use lots/fifo for tax-us
quote quote/one on 2026-09-20
  1 ABC = 52 USD
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
"#;
        let base = evaluate(&parse_ledger(source).unwrap());
        let permuted = evaluate(&parse_ledger(reordered).unwrap());
        assert_eq!(base.relation_key(), permuted.relation_key());

        let unrelated = format!(
            "book tax-us\nbuy other on 2025-01-01\n  7 XYZ into other\n  for 11 EUR\n{}",
            source.strip_prefix("book tax-us\n").unwrap()
        );
        let with_unrelated = evaluate(&parse_ledger(&unrelated).unwrap());
        assert_eq!(base.sale("sell"), with_unrelated.sale("sell"));
        assert_eq!(base.valuation, with_unrelated.valuation);
    }

    #[test]
    fn conflicting_policies_are_order_independent_and_block_selection() {
        let body = fifo_fixture().replace("use lots/fifo for tax-us\n", "");
        let first = format!("{body}use lots/community for tax-us\nuse lots/fifo for tax-us\n");
        let second = format!("{body}use lots/fifo for tax-us\nuse lots/community for tax-us\n");
        let left = evaluate(&parse_ledger(&first).unwrap());
        let right = evaluate(&parse_ledger(&second).unwrap());
        assert_eq!(left.relation_key(), right.relation_key());
        let sale = left.sale("sell").unwrap();
        assert!(sale.selected_lot.is_none());
        assert!(matches!(
            sale.status,
            SelectionStatus::PolicyConflict { .. }
        ));
        assert!(left.recognized_gain("sell").is_none());
        assert!(left.issues.iter().any(|issue| {
            issue.code == ReferenceIssueCode::PolicyDecisionConflict
                && issue.message.contains("policy selection is ambiguous")
        }));
    }

    #[test]
    fn multiple_sales_keep_the_v0_allocation_limit_explicit() {
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
        let result = evaluate(&parse_ledger(source).unwrap());
        assert_eq!(result.sales.len(), 2);
        assert!(result.sales.iter().all(|sale| {
            sale.selected_lot.is_none() && matches!(sale.status, SelectionStatus::MissingLot)
        }));
        assert_eq!(
            result
                .issues
                .iter()
                .filter(|issue| issue.message.contains("multiple sales"))
                .count(),
            2
        );
        // Conditional arithmetic remains available for inspection, but it
        // never leaks into a recognized result while allocation is unsupported.
        assert_eq!(result.gain.len(), 2);
        assert!(result.recognized.iter().all(|fact| fact.gain.is_none()));
    }

    #[test]
    fn exact_partial_allocation_stays_rational() {
        let source = r#"book tax-us
buy buy/one on 2026-01-04
  3 ABC into brokerage
  for 10 USD
sell sell on 2026-09-20
  1 ABC from brokerage
  for 5 USD
  lot ?lot
"#;
        let result = evaluate(&parse_ledger(source).unwrap());
        assert_eq!(result.gain[0].basis.number.canonical_string(), "10/3");
        assert_eq!(result.gain[0].gain.number.canonical_string(), "5/3");
    }
}
