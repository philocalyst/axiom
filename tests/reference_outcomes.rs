//! Gate 1 structured outcomes for the independent reference evaluator.
//!
//! These tests intentionally parse and evaluate the source directly.  They do
//! not construct a workspace or call the proof-producing engine, so they keep
//! the oracle's support/completeness boundary independently executable.

use axiom_ledger::parser::parse_ledger;
use axiom_ledger::reference::{
    ReferenceCompletion, ReferenceOutcomeStatus, ReferenceProof, ReferenceQuery, ReferenceRepair,
    ReferenceResult, ReferenceTruth,
};

fn evaluate(source: &str) -> ReferenceResult {
    axiom_ledger::reference::evaluate(&parse_ledger(source).expect("source parses"))
}

fn fifo_source() -> &'static str {
    r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
use lots/fifo for tax-us
"#
}

#[test]
fn recognized_reference_relation_has_positive_proof() {
    let result = evaluate(fifo_source());
    let outcome = result.resolve(
        ReferenceQuery::Recognition("sell".into()),
        ReferenceCompletion::Complete,
    );

    assert_eq!(outcome.status(), ReferenceOutcomeStatus::Proven);
    assert_eq!(outcome.truth(), ReferenceTruth::TrueOnly);
    assert!(outcome.answer().is_some());
    assert_eq!(outcome.positive_proofs().len(), 1);
    assert!(matches!(
        &outcome.positive_proofs()[0],
        ReferenceProof::Fact { relation, subject }
            if relation == "recognized" && subject == "sell"
    ));
    assert!(outcome.negative_proofs().is_empty());
}

#[test]
fn negative_proof_requires_explicit_completeness() {
    let result = evaluate("book tax-us\n");

    let open = result.resolve(
        ReferenceQuery::Recognition("missing".into()),
        ReferenceCompletion::OpenWorld,
    );
    assert_eq!(open.truth(), ReferenceTruth::Neither);
    assert!(open.negative_proofs().is_empty());
    assert!(open.blockers().iter().any(|blocker| {
        matches!(
            blocker,
            axiom_ledger::reference::ReferenceBlocker::Completeness { .. }
        )
    }));
    assert!(
        open.repairs()
            .iter()
            .any(|repair| { matches!(repair, ReferenceRepair::DeclareCompleteness { .. }) })
    );

    let complete = result.resolve(
        ReferenceQuery::Recognition("missing".into()),
        ReferenceCompletion::Complete,
    );
    assert_eq!(complete.status(), ReferenceOutcomeStatus::Refuted);
    assert_eq!(complete.truth(), ReferenceTruth::FalseOnly);
    assert_eq!(complete.negative_proofs().len(), 1);
    assert!(matches!(
        &complete.negative_proofs()[0],
        ReferenceProof::CompleteAbsence { relation, subject, .. }
            if relation == "recognized" && subject == "missing"
    ));

    let limited = result.resolve(
        ReferenceQuery::Recognition("missing".into()),
        ReferenceCompletion::ResourceLimited,
    );
    assert_eq!(limited.status(), ReferenceOutcomeStatus::Incomplete);
    assert!(limited.negative_proofs().is_empty());
}

#[test]
fn unresolved_selection_is_blocked_with_scoped_repairs() {
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
"#;
    let result = evaluate(source);
    let outcome = result.recognition_outcome("sell", ReferenceCompletion::OpenWorld);

    assert_eq!(outcome.status(), ReferenceOutcomeStatus::Blocked);
    assert_eq!(outcome.truth(), ReferenceTruth::Neither);
    assert!(outcome.positive_proofs().is_empty());
    assert!(outcome.negative_proofs().is_empty());
    assert_eq!(outcome.repairs().len(), 2);
    assert!(outcome.repairs().iter().all(|repair| {
        matches!(repair, ReferenceRepair::SelectLot { sale, .. } if sale == "sell")
    }));
}

#[test]
fn incompatible_selection_evidence_is_a_conflict_not_a_guess() {
    let source = format!("{}use lots/lifo for tax-us\n", fifo_source());
    let result = evaluate(&source);
    let outcome = result.recognition_outcome("sell", ReferenceCompletion::Complete);

    assert_eq!(outcome.status(), ReferenceOutcomeStatus::Conflict);
    assert_eq!(outcome.truth(), ReferenceTruth::Both);
    assert!(!outcome.positive_proofs().is_empty());
    assert!(!outcome.negative_proofs().is_empty());
    assert_eq!(outcome.conflicts().len(), 1);
    assert!(outcome.repairs().iter().any(|repair| {
        matches!(repair, ReferenceRepair::ResolveConflict { subject } if subject == "sell")
    }));
}

#[test]
fn repairs_are_inert_and_do_not_change_reference_relations() {
    let result = evaluate(
        r#"book tax-us
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
"#,
    );
    let before = result.relation_key();
    let outcome = result.recognition_outcome("sell", ReferenceCompletion::OpenWorld);
    assert!(!outcome.repairs().is_empty());
    assert_eq!(before, result.relation_key());
    assert_eq!(outcome.status(), ReferenceOutcomeStatus::Blocked);
}
