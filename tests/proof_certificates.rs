//! Focused coverage for the concrete public-answer proof certificates.
//!
//! These tests intentionally inspect the persisted proof DAG rather than
//! trusting the rendered `Analysis` fields.  The production engine remains
//! the producer; `Proof::check` is the independent consumer.

use axiom_ledger::proof::{CheckError, Operation, Proof, ProofId};
use axiom_ledger::workspace::Workspace;

fn analyzed(source: &str) -> axiom_ledger::workspace::CommitAnalysis {
    let mut workspace = Workspace::new();
    let source = workspace
        .load_source("proof-tests", source)
        .expect("source loads");
    workspace
        .analyze_commit(source.commit)
        .expect("source analysis succeeds")
}

#[test]
fn settlement_reconciliation_is_a_typed_source_bound_certificate() {
    let analysis = analyzed(
        r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell/one on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
decide sell/one lot buy/one
observe settlement sell/one 500 USD into checking
"#,
    );
    analysis.check_proof().expect("bound proof checks");

    let settlement = &analysis.analysis.settlements[0];
    let node = analysis
        .analysis
        .proof
        .node(settlement.proof)
        .expect("settlement result node exists");
    let Operation::SettlementReconciliation(certificate) = &node.operation else {
        panic!("settlement result is not a typed reconciliation");
    };
    assert_eq!(certificate.settlement, "sell/one");
    assert_eq!(certificate.sale, "sell/one");
    assert_eq!(certificate.observed, certificate.expected);
    assert_eq!(certificate.status, "reconciled");
    assert!(node.inputs.contains(&certificate.source_proof));
    assert!(node.inputs.contains(&certificate.sale_proof));
}

#[test]
fn settlement_reconciliation_tampering_is_rejected_without_engine_execution() {
    let analysis = analyzed(
        r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell/one on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
decide sell/one lot buy/one
observe settlement sell/one 500 USD into checking
"#,
    );
    let reconciliation = analysis.analysis.settlements[0].proof;
    let mut proof = analysis.analysis.proof.clone();
    let node = proof
        .nodes
        .get_mut(&reconciliation)
        .expect("certificate exists");
    let Operation::SettlementReconciliation(certificate) = &mut node.operation else {
        panic!("expected typed reconciliation");
    };
    certificate.expected = certificate.expected.checked_add(&1_i64.into());
    assert!(matches!(proof.check(), Err(CheckError::TamperedNode { id }) if id == reconciliation));
}

#[test]
fn malformed_proof_check_returns_an_error_instead_of_panicking() {
    let mut proof = Proof::new();
    proof.roots.push(ProofId::ZERO);
    assert!(
        matches!(proof.check(), Err(CheckError::MissingRoot { root }) if root == ProofId::ZERO)
    );
}
