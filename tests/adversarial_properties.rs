//! Deterministic adversarial checks for public economic/proof boundaries.
//!
//! These cases are intentionally small and exact.  They exercise combinations
//! that are easy to accidentally make permissive: fractional conservation,
//! one-quantum overconsumption, payment-rail-specific histories, rehashed but
//! forged certificates, and replayed content-addressed snapshots.

use std::collections::BTreeMap;

use axiom_ledger::exact::ExactNumber;
use axiom_ledger::model::{Quantity, SettlementKind};
use axiom_ledger::ontology::{
    DisposeRecord, Endpoint, Lot, Obligation, OntologyError, SatisfactionAllocation, Settlement,
    SettlementEffectKind, SettlementEffectRecord, SettlementState, validate_lot_inventory,
    validate_satisfaction_network, validate_settlement_effects,
};
use axiom_ledger::proof::{
    CheckError, Node, ObligationBalanceCertificate, Operation, Proof, SettlementHistoryCertificate,
    SettlementObservationCertificate, SettlementTransition,
};
use axiom_ledger::store::{Commit, Evidence, MergeConflict, ObjectStore, Statement};

fn quantity(value: ExactNumber, unit: &str) -> Quantity {
    Quantity::with_unit(value, unit).expect("test quantity has a valid unit")
}

fn integer(value: i64, unit: &str) -> Quantity {
    quantity(ExactNumber::integer(value), unit)
}

fn rational(numerator: i64, denominator: i64, unit: &str) -> Quantity {
    quantity(
        ExactNumber::rational(numerator, denominator).expect("test denominator is nonzero"),
        unit,
    )
}

fn settled_payment(id: &str, amount: Quantity) -> Settlement {
    let mut payment = Settlement::new(
        id,
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        amount,
    )
    .expect("positive payment");
    payment
        .transition(SettlementState::Presented, None, None)
        .expect("issued payments can be presented");
    payment
        .transition(SettlementState::Settled, None, None)
        .expect("presented payments can settle");
    payment
}

#[test]
fn fractional_satisfaction_conserves_both_sides_independent_of_input_order() {
    let obligation = Obligation::transfer(
        "invoice/fractional",
        "payer",
        "merchant",
        "USD",
        integer(1, "USD"),
    )
    .expect("positive obligation");
    let payment = settled_payment("payment/fractional", integer(1, "USD"));
    let first = SatisfactionAllocation::new(
        "allocation/one-third",
        obligation.id.clone(),
        payment.id.clone(),
        rational(1, 3, "USD"),
    )
    .expect("positive allocation")
    .applied();
    let second = SatisfactionAllocation::new(
        "allocation/two-thirds",
        obligation.id.clone(),
        payment.id.clone(),
        rational(2, 3, "USD"),
    )
    .expect("positive allocation")
    .applied();

    let forward = validate_satisfaction_network(
        std::slice::from_ref(&obligation),
        std::slice::from_ref(&payment),
        &[first.clone(), second.clone()],
    )
    .expect("fractional allocations exactly conserve the payment");
    let reverse = validate_satisfaction_network(
        std::slice::from_ref(&obligation),
        std::slice::from_ref(&payment),
        &[second.clone(), first.clone()],
    )
    .expect("allocation input order is not semantic");
    assert_eq!(forward, reverse);
    assert_eq!(
        forward.obligation_remaining[&obligation.id].canonical(),
        "0 USD"
    );
    assert_eq!(forward.settlement_unused[&payment.id].canonical(), "0 USD");

    let over = SatisfactionAllocation::new(
        "allocation/epsilon",
        obligation.id.clone(),
        payment.id.clone(),
        rational(1, 10_000, "USD"),
    )
    .expect("positive epsilon allocation")
    .applied();
    assert!(matches!(
        validate_satisfaction_network(
            std::slice::from_ref(&obligation),
            std::slice::from_ref(&payment),
            &[first, second, over],
        ),
        Err(OntologyError::ObligationOverallocated { .. })
            | Err(OntologyError::SettlementOverallocated { .. })
    ));
}

#[test]
fn lot_inventory_accepts_exact_boundary_but_rejects_one_exact_epsilon() {
    let acquired = rational(1, 3, "ABC");
    let lot = Lot::new("lot/boundary", "ABC", acquired.clone(), "buy/boundary")
        .expect("positive fractional lot");
    let acquisition = axiom_ledger::ontology::AcquireRecord::new(
        "buy/boundary",
        Endpoint::entity("brokerage"),
        "ABC",
        acquired.clone(),
    )
    .into_lot("lot/boundary");
    let exact_disposal = DisposeRecord::new(
        "sell/boundary",
        Endpoint::entity("brokerage"),
        "ABC",
        acquired.clone(),
    )
    .from_lot("lot/boundary");
    let exhausted = lot.consumed(&acquired).expect("exact quantity is allowed");
    assert!(exhausted.exhausted());
    validate_lot_inventory(
        &[exhausted],
        std::slice::from_ref(&acquisition),
        std::slice::from_ref(&exact_disposal),
    )
    .expect("acquisition minus exact disposal is zero inventory");

    let epsilon = rational(1, 1_000_000, "ABC");
    let too_much = acquired.checked_add(&epsilon).expect("matching units");
    let forged_disposal = DisposeRecord::new(
        "sell/too-much",
        Endpoint::entity("brokerage"),
        "ABC",
        too_much,
    )
    .from_lot("lot/boundary");
    assert!(matches!(
        lot.consumed(&forged_disposal.quantity),
        Err(OntologyError::LotOverconsumed { .. })
    ));
    assert!(matches!(
        validate_lot_inventory(&[lot], &[acquisition], &[exact_disposal, forged_disposal],),
        Err(OntologyError::LotOverconsumed { .. })
    ));
}

#[test]
fn explicit_payment_rail_rejects_generic_but_rail_illegal_transition() {
    let amount = integer(25, "USD");
    let generic = Settlement::new(
        "payment/generic",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        amount.clone(),
    )
    .expect("positive payment");
    let generic = generic
        .appended(SettlementState::Presented, None, None)
        .expect("generic presented transition")
        .appended(SettlementState::Settled, None, None)
        .expect("generic settled transition")
        .appended(SettlementState::Reversed, None, None)
        .expect("generic reversal transition");
    assert_eq!(generic.latest_state(), Some(&SettlementState::Reversed));

    assert!(matches!(
        Settlement::from_history_with_kind(
            "payment/check",
            SettlementKind::Check,
            Endpoint::entity("payer"),
            Endpoint::entity("merchant"),
            "USD",
            amount,
            vec![
                axiom_ledger::ontology::SettlementTransition {
                    state: SettlementState::Issued,
                    at: None,
                    reason: None,
                },
                axiom_ledger::ontology::SettlementTransition {
                    state: SettlementState::Presented,
                    at: None,
                    reason: None,
                },
                axiom_ledger::ontology::SettlementTransition {
                    state: SettlementState::Settled,
                    at: None,
                    reason: None,
                },
                axiom_ledger::ontology::SettlementTransition {
                    state: SettlementState::Reversed,
                    at: None,
                    reason: None,
                },
            ],
        ),
        Err(OntologyError::InvalidSettlementTransition {
            from: Some(SettlementState::Settled),
            to: SettlementState::Reversed,
        })
    ));
}

#[test]
fn settlement_undo_effects_are_additive_and_bounded_by_original_amount() {
    let payment = settled_payment("payment/effects", integer(10, "USD"))
        .appended(SettlementState::Reversed, None, None)
        .expect("a settled payment can be reversed");
    let first = SettlementEffectRecord::new(
        "effect/reversal/a",
        payment.id.clone(),
        SettlementEffectKind::Reversal,
        integer(6, "USD"),
        "USD",
    );
    let second = SettlementEffectRecord::new(
        "effect/reversal/b",
        payment.id.clone(),
        SettlementEffectKind::Reversal,
        integer(4, "USD"),
        "USD",
    );
    validate_settlement_effects(
        std::slice::from_ref(&payment),
        &[first.clone(), second.clone()],
    )
    .expect("two additive reversals exactly consume original amount");

    let excess = SettlementEffectRecord::new(
        "effect/reversal/excess",
        payment.id.clone(),
        SettlementEffectKind::Reversal,
        rational(1, 10, "USD"),
        "USD",
    );
    assert!(matches!(
        validate_settlement_effects(std::slice::from_ref(&payment), &[first, second, excess],),
        Err(OntologyError::SettlementOverallocated { .. })
    ));
}

#[test]
fn proof_checker_rejects_rehashed_forged_settlement_and_balance_certificates() {
    let source = Node::new(
        "settlement source",
        Operation::SettlementObservation(SettlementObservationCertificate {
            settlement: "payment/proof".into(),
            kind: "ach".into(),
            from: "payer".into(),
            to: "merchant".into(),
            instrument: "USD".into(),
            amount: ExactNumber::integer(10),
            unit: "USD".into(),
            history: vec![
                SettlementTransition {
                    state: "issued".into(),
                    at: None,
                },
                SettlementTransition {
                    state: "presented".into(),
                    at: None,
                },
                SettlementTransition {
                    state: "settled".into(),
                    at: Some("2026-09-21".into()),
                },
            ],
        }),
        Vec::new(),
        BTreeMap::new(),
    );
    let forged_history = Node::new(
        "forged history",
        Operation::SettlementHistory(Box::new(SettlementHistoryCertificate {
            settlement: "payment/proof".into(),
            settlement_proof: source.id,
            kind: "ach".into(),
            from: "payer".into(),
            to: "merchant".into(),
            instrument: "USD".into(),
            amount: ExactNumber::integer(10),
            unit: "USD".into(),
            history: match &source.operation {
                Operation::SettlementObservation(observation) => observation.history.clone(),
                _ => unreachable!(),
            },
            current: "returned".into(),
            effective: false,
        })),
        vec![source.id],
        BTreeMap::new(),
    );
    let mut history_proof = Proof::new();
    history_proof.insert(source.clone());
    history_proof.insert(forged_history.clone());
    history_proof.root(forged_history.id);
    assert!(matches!(
        history_proof.check(),
        Err(CheckError::InvalidSettlementHistory { id }) if id == forged_history.id
    ));

    let obligation = Node::new(
        "obligation source",
        Operation::ObligationObservation(axiom_ledger::proof::ObligationObservationCertificate {
            obligation: "invoice/proof".into(),
            debtor: "payer".into(),
            creditor: "merchant".into(),
            promised: ExactNumber::integer(10),
            unit: "USD".into(),
            due: None,
        }),
        Vec::new(),
        BTreeMap::new(),
    );
    let forged_balance = Node::new(
        "forged balance",
        Operation::ObligationBalance(Box::new(ObligationBalanceCertificate {
            obligation: "invoice/proof".into(),
            obligation_proof: obligation.id,
            promised: ExactNumber::integer(10),
            allocated: ExactNumber::integer(1),
            remaining: ExactNumber::integer(9),
            unit: "USD".into(),
            allocations: Vec::new(),
        })),
        vec![obligation.id],
        BTreeMap::new(),
    );
    let mut balance_proof = Proof::new();
    balance_proof.insert(obligation);
    balance_proof.insert(forged_balance.clone());
    balance_proof.root(forged_balance.id);
    assert!(matches!(
        balance_proof.check(),
        Err(CheckError::InvalidObligationBalance { id }) if id == forged_balance.id
    ));
}

#[test]
fn replayed_commit_id_is_stable_for_reordered_roots() {
    let mut left = ObjectStore::new();
    let first = left
        .put_evidence(Evidence::new("occurrence/a", "bank", b"a".to_vec()))
        .expect("first evidence");
    let second = left
        .put_evidence(Evidence::new("occurrence/b", "bank", b"b".to_vec()))
        .expect("second evidence");
    let commit_left = left
        .put_commit(Commit::new(
            [],
            [first, second],
            [],
            [],
            [],
            [],
            [],
            "replay",
        ))
        .expect("first replay commit");

    let mut right = ObjectStore::new();
    let second_again = right
        .put_evidence(Evidence::new("occurrence/b", "bank", b"b".to_vec()))
        .expect("second evidence replay");
    let first_again = right
        .put_evidence(Evidence::new("occurrence/a", "bank", b"a".to_vec()))
        .expect("first evidence replay");
    let commit_right = right
        .put_commit(Commit::new(
            [],
            [second_again, first_again, first_again],
            [],
            [],
            [],
            [],
            [],
            "replay",
        ))
        .expect("reordered replay commit");
    assert_eq!(commit_left, commit_right);
    assert_eq!(
        left.get(commit_left.into()).unwrap(),
        right.get(commit_right.into()).unwrap()
    );
}

#[test]
fn statement_conflicts_survive_merge_in_either_branch_order() {
    let mut store = ObjectStore::new();
    let base = store
        .put_commit(Commit::new([], [], [], [], [], [], [], "base"))
        .expect("base commit");
    let left_statement = store
        .put_statement(Statement::new("sale/1", "status", "recognized"))
        .expect("left statement");
    let right_statement = store
        .put_statement(Statement::new("sale/1", "status", "recognized").negative())
        .expect("right statement");
    let left = store
        .put_commit(Commit::new(
            [base],
            [],
            [left_statement],
            [],
            [],
            [],
            [],
            "left",
        ))
        .expect("left commit");
    let right = store
        .put_commit(Commit::new(
            [base],
            [],
            [right_statement],
            [],
            [],
            [],
            [],
            "right",
        ))
        .expect("right commit");

    let forward = store.merge(base, left, right, "merge").expect("merge");
    let reverse = store
        .merge(base, right, left, "merge")
        .expect("reverse merge");
    assert_eq!(forward.commit, reverse.commit);
    assert_eq!(forward.conflicts, reverse.conflicts);
    assert!(!forward.is_clean());
    assert!(matches!(
        forward.conflicts.as_slice(),
        [MergeConflict::Statements {
            subject,
            predicate,
            value,
            ..
        }] if subject == "sale/1" && predicate == "status" && value == "recognized"
    ));
}
