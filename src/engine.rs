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
use crate::package::builtin_policy_hash;
use crate::proof::{Node, Operation, Proof, ProofId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    pub amount: Exact,
    pub unit: String,
}

impl Quantity {
    fn canonical(&self) -> String {
        format!("{} {}", self.amount.canonical_string(), self.unit)
    }
}

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
    pub proceeds: Quantity,
    pub basis: Quantity,
    pub gain: Quantity,
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
        let mut totals: BTreeMap<&str, Exact> = BTreeMap::new();
        for line in &self.lines {
            let current = totals
                .entry(line.quantity.unit.as_str())
                .or_insert_with(|| Exact::from(0i64));
            let amount = match line.side {
                Side::Debit => line.quantity.amount.clone(),
                Side::Credit => -line.quantity.amount.clone(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IssueCode {
    AmbiguousLot,
    PolicyDecisionConflict,
    MissingLot,
    AmbiguousQuote,
    UnknownPolicy,
    IncompatibleUnit,
    InvalidAmount,
}

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
        self.proof.check()
    }

    pub fn sale(&self, id: &str) -> Option<&SaleAnalysis> {
        self.sales.iter().find(|sale| sale.id == id)
    }

    pub fn recognized_gain(&self, id: &str) -> Option<&ConditionalGain> {
        let sale = self.sale(id)?;
        let lot = sale.selected_lot.as_ref()?;
        sale.conditional_gains
            .iter()
            .find(|gain| &gain.lot_id == lot)
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

/// Analyze one parsed ledger.  All ordering is derived from source order and
/// explicit policy; no map iteration or hash-map randomization affects it.
pub fn analyze(ledger: &Ledger) -> Analysis {
    let book = ledger.book.as_str().to_owned();
    let mut proof = Proof::new();
    let mut lots = Vec::new();
    let mut sales_raw = Vec::new();
    let mut quotes = Vec::new();
    let mut positions = Vec::new();
    let mut settlements = Vec::new();
    let mut policy = None;
    let mut policy_proof = None;
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
                    Operation::Observation {
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
                if use_policy.book.as_str() == book {
                    let policy_name = use_policy.policy.as_str().to_owned();
                    let source = source_key(
                        "policy",
                        &format!("{}|{}", use_policy.book, use_policy.policy),
                    );
                    let mut metadata = metadata_for(&source, "policy", Some(index + 1));
                    if let Some(hash) = builtin_policy_hash(&policy_name) {
                        metadata.insert("policy-hash".into(), hash.to_string());
                    }
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
                    if let Some(previous) = policy.as_ref() {
                        if previous != &policy_name {
                            issues.push(Issue {
                                code: IssueCode::PolicyDecisionConflict,
                                message: format!(
                                    "policies `{previous}` and `{policy_name}` both apply to book `{book}`"
                                ),
                                sale: None,
                                proof: Some(current_proof),
                            });
                        }
                    } else {
                        policy = Some(policy_name);
                        policy_proof = Some(current_proof);
                    }
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
                    if let Some((previous, previous_proof)) = choices.first() {
                        let conflict_proof = proof.insert(Node::new(
                            format!("decision conflict {sale_name}"),
                            Operation::Conflict {
                                subject: sale_name.clone(),
                                reason: "multiple decisions select different lots".into(),
                            },
                            vec![*previous_proof, current_proof],
                            metadata_for(&source, "decision-conflict", None),
                        ));
                        issues.push(Issue {
                            code: IssueCode::PolicyDecisionConflict,
                            message: format!(
                                "decisions for sale `{sale_name}` select both `{previous}` and `{value}`"
                            ),
                            sale: Some(sale_name.clone()),
                            proof: Some(conflict_proof),
                        });
                    }
                    choices.push((value, current_proof));
                }
            }
        }
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
                quote.base.amount.checked_mul(&reference.quote.amount)
                    == reference.base.amount.checked_mul(&quote.quote.amount)
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

    let fifo = policy.as_deref() == Some("lots/fifo");
    if let Some(name) = policy.as_deref()
        && name != "lots/fifo"
    {
        issues.push(Issue {
            code: IssueCode::UnknownPolicy,
            message: format!("policy `{name}` has no built-in resolver"),
            sale: None,
            proof: None,
        });
    }

    let mut sales = Vec::new();
    let mut journal = Vec::new();
    let mut dependencies: BTreeMap<String, Vec<ProofId>> = BTreeMap::new();
    let mut invalidations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let multi_sale_unsupported = sales_raw.len() > 1;

    // Observations are evidence, not balances.  Preserve every source node,
    // but explicitly surface contradictory values and never let one
    // observation silently win by source order.
    let mut position_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, position) in positions.iter().enumerate() {
        position_groups
            .entry(position.account.clone())
            .or_default()
            .push(index);
    }
    for (account, indexes) in position_groups {
        let distinct = indexes
            .iter()
            .map(|index| positions[*index].quantity.canonical())
            .collect::<BTreeSet<_>>();
        if distinct.len() > 1 {
            let conflict_proof = proof.insert(Node::new(
                format!("position conflict {account}"),
                Operation::Conflict {
                    subject: account.clone(),
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
                code: IssueCode::PolicyDecisionConflict,
                message: format!("position observations for `{account}` conflict"),
                sale: None,
                proof: Some(conflict_proof),
            });
        }
    }

    let sale_names = sales_raw
        .iter()
        .map(|(_, sale)| sale.label.as_str())
        .collect::<BTreeSet<_>>();
    let mut settlement_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, settlement) in settlements.iter().enumerate() {
        if !sale_names.contains(settlement.reference.as_str()) {
            issues.push(Issue {
                code: IssueCode::MissingLot,
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
        if distinct.len() > 1 {
            settlement_conflicts.insert(reference.clone());
            let conflict_proof = proof.insert(Node::new(
                format!("settlement conflict {reference}"),
                Operation::Conflict {
                    subject: reference.clone(),
                    reason: "multiple observed settlements disagree".into(),
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
                code: IssueCode::PolicyDecisionConflict,
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
            Operation::Observation {
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
                quantity: Quantity {
                    amount: Exact::from(0i64),
                    unit: "?unit".into(),
                },
                proceeds: quantity_of(&sell.proceeds).unwrap_or_else(zero_quantity),
                eligible_lots: Vec::new(),
                conditional_gains: Vec::new(),
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
                    && lot.quantity.amount >= quantity.amount
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
            if let Some(gain) = conditional_gain(lot, &quantity, &proceeds, &mut proof, sale_proof)
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
        let policy_lot = if fifo {
            eligible.first().and_then(|first| {
                let earliest = eligible
                    .iter()
                    .filter(|lot| lot.date == first.date)
                    .collect::<Vec<_>>();
                (earliest.len() == 1).then(|| first.id.clone())
            })
        } else {
            None
        };
        let (mut selected_lot, mut status) = if multi_sale_unsupported {
            issues.push(Issue {
                code: IssueCode::MissingLot,
                message: format!(
                    "sale `{}` is blocked: V0 does not allocate lots across multiple sales",
                    sell.label
                ),
                sale: Some(sell.label.clone()),
                proof: Some(sale_proof),
            });
            (None, RecognitionStatus::MissingLot)
        } else if decision_conflict {
            // The conflict proof created while reading decisions is rooted,
            // and no one decision is allowed to leak into selection.
            (
                None,
                RecognitionStatus::Ambiguous {
                    candidates: decision_lots.clone(),
                },
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
            )
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
                    issues.push(Issue { code: IssueCode::PolicyDecisionConflict, message: format!("FIFO selects `{policy_lot}` but decision selects `{decision_lot}` for sale `{}`", sell.label), sale: Some(sell.label.clone()), proof: Some(proof_id) });
                    (
                        None,
                        RecognitionStatus::Conflict {
                            policy_lot,
                            decision_lot,
                        },
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
                    )
                }
                (Some(policy_lot), _) => (Some(policy_lot), RecognitionStatus::Recognized),
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
                    (None, RecognitionStatus::MissingLot)
                }
                (None, Some(decision_lot)) => (Some(decision_lot), RecognitionStatus::Recognized),
                (None, None) if eligible.len() == 1 => (
                    eligible.first().map(|lot| lot.id.clone()),
                    RecognitionStatus::Recognized,
                ),
                (None, None) if eligible.is_empty() => {
                    issues.push(Issue {
                        code: IssueCode::MissingLot,
                        message: format!("no eligible lot for sale `{}`", sell.label),
                        sale: Some(sell.label.clone()),
                        proof: Some(sale_proof),
                    });
                    (None, RecognitionStatus::MissingLot)
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
                    )
                }
            }
        };
        let selected_gain = selected_lot
            .as_ref()
            .and_then(|lot| conditionals.iter().find(|gain| &gain.lot_id == lot));
        if selected_lot.is_some() && selected_gain.is_none() {
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
        let sale_result_proof = if let Some(gain) = selected_gain {
            let mut inputs = vec![sale_proof, gain.proof];
            if let Some(policy_proof) = policy_proof {
                inputs.push(policy_proof);
            }
            inputs.extend(decision_proofs.iter().copied());
            if let Some(settlement) = matching_settlement(&settlements, &sell.label, &proceeds) {
                inputs.push(settlement.proof);
            }
            let id = proof.insert(Node::new(
                format!("gain {}/{}", sell.label, source),
                Operation::Derive {
                    rule: "recognize-sale".into(),
                },
                inputs,
                metadata_for(&source, "recognition", None),
            ));
            // A journal is an accepted cash projection, not a guess.  The
            // receiving account must come from a matching settlement
            // observation; never invent `proceeds:<sale>`.
            if let Some(settlement) = matching_settlement(&settlements, &sell.label, &proceeds)
                && let Some(into) = settlement.into.as_deref()
            {
                journal.push(make_journal(&sell.label, into, &account, &asset, gain, id));
            }
            id
        } else {
            sale_proof
        };
        let mut goal_roots = std::iter::once(sale_result_proof)
            .chain(eligible.iter().map(|lot| lot.proof))
            .collect::<Vec<_>>();
        if let Some(policy_proof) = policy_proof {
            goal_roots.push(policy_proof);
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
            if lot.account == position.account && lot.asset == position.quantity.unit {
                calculated = calculated.checked_add(&lot.quantity.amount);
                inputs.push(lot.proof);
            }
        }
        for sale in &sales {
            if sale.account == position.account && sale.asset == position.quantity.unit {
                calculated = calculated.checked_sub(&sale.quantity.amount);
                inputs.push(sale.proof);
            }
        }
        if inputs.len() == 1 {
            continue;
        }
        let operation = if calculated == position.quantity.amount {
            position.status = ObservationStatus::Reconciled;
            Operation::Derive {
                rule: "reconcile-position".into(),
            }
        } else {
            position.status = ObservationStatus::Conflict;
            issues.push(Issue {
                code: IssueCode::PolicyDecisionConflict,
                message: format!(
                    "observed position `{}` is {} {}, but authored events imply {} {}",
                    position.account,
                    position.quantity.amount,
                    position.quantity.unit,
                    calculated,
                    position.quantity.unit
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
                code: IssueCode::PolicyDecisionConflict,
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
    let basis_amount = consideration.amount.checked_add(
        &fee.as_ref()
            .map(|fee| fee.amount.clone())
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
        basis: Quantity {
            amount: basis_amount,
            unit: consideration.unit,
        },
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

fn conditional_gain(
    lot: &Lot,
    sold: &Quantity,
    proceeds: &Quantity,
    proof: &mut Proof,
    sale_proof: ProofId,
) -> Option<ConditionalGain> {
    if lot.quantity.unit != sold.unit || proceeds.unit.is_empty() || lot.basis.unit != proceeds.unit
    {
        return None;
    }
    let ratio = sold.amount.checked_div(&lot.quantity.amount).ok()?;
    let allocated_basis = lot.basis.amount.checked_mul(&ratio);
    let gain = proceeds.amount.checked_sub(&allocated_basis);
    let mut metadata = metadata_for(&lot.id, "conditional-gain", None);
    metadata.insert("lot".into(), lot.id.clone());
    let proof_id = proof.insert(Node::new(
        format!("conditional gain {} on {}", lot.id, sale_proof),
        Operation::Arithmetic {
            rule: "gain = proceeds - allocated basis".into(),
            minuend: proceeds.amount.clone(),
            subtrahend: allocated_basis.clone(),
            result: gain.clone(),
            unit: proceeds.unit.clone(),
        },
        vec![lot.proof, sale_proof],
        metadata,
    ));
    Some(ConditionalGain {
        lot_id: lot.id.clone(),
        proceeds: proceeds.clone(),
        basis: Quantity {
            amount: allocated_basis,
            unit: lot.basis.unit.clone(),
        },
        gain: Quantity {
            amount: gain,
            unit: proceeds.unit.clone(),
        },
        proof: proof_id,
    })
}

fn make_journal(
    sale: &str,
    proceeds_account: &str,
    account: &str,
    asset: &str,
    gain: &ConditionalGain,
    proof: ProofId,
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
        proof,
    }
}

fn matching_settlement<'a>(
    settlements: &'a [SettlementView],
    sale: &str,
    proceeds: &Quantity,
) -> Option<&'a SettlementView> {
    settlements.iter().find(|settlement| {
        settlement.reference == sale
            && matches!(
                settlement.status,
                ObservationStatus::Observed | ObservationStatus::Reconciled
            )
            && settlement.into.is_some()
            && settlement.quantity == *proceeds
    })
}

fn holding_of_model(sell: &model::Sell) -> Option<(String, String, Quantity)> {
    let account = sell.from.as_str().to_owned();
    let unit = sell.quantity.unit.as_ref()?.as_str().to_owned();
    let quantity = quantity_of(&sell.quantity)?;
    Some((account, unit, quantity))
}

fn quantity_of(quantity: &model::Quantity) -> Option<Quantity> {
    let unit = quantity.unit.as_ref()?.as_str().to_owned();
    Some(Quantity {
        amount: quantity.number.clone(),
        unit,
    })
}

fn zero_quantity() -> Quantity {
    Quantity {
        amount: Exact::from(0i64),
        unit: "?unit".into(),
    }
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
    let unit = quantity
        .unit
        .as_ref()
        .map(|unit| unit.as_str())
        .unwrap_or("?");
    format!("{} {unit}", quantity.number.canonical_string())
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
    format!(
        "{}|{}|{}|{}|{}",
        sell.label,
        sell.date,
        sell.from,
        canonical_model_quantity(&sell.quantity),
        canonical_model_quantity(&sell.proceeds),
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
    format!("{}:{}:{}", quote.date, quote.base.unit, quote.quote.unit)
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
        assert_eq!(sale.conditional_gains[0].gain.amount.to_string(), "299");
        assert_eq!(sale.conditional_gains[1].gain.amount.to_string(), "199");
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
                .amount
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
                .amount
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
    fn v0_blocks_multiple_sales_instead_of_reusing_a_lot() {
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
        assert!(result.sales.iter().all(|sale| sale.selected_lot.is_none()));
        assert_eq!(
            result
                .issues
                .iter()
                .filter(|issue| issue.message.contains("multiple sales"))
                .count(),
            2
        );
        assert!(result.blocked());
        assert!(result.check_proof().is_ok());
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
}
