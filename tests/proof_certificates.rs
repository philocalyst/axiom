//! Focused coverage for the concrete public-answer proof certificates.
//!
//! These tests intentionally inspect the persisted proof DAG rather than
//! trusting the rendered `Analysis` fields.  The production engine remains
//! the producer; `Proof::check` is the independent consumer.

use axiom_ledger::model::ContentHash;
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

#[test]
fn commit_binding_rejects_hash_input_root_and_metadata_tampering() {
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
"#,
    );
    let binding = analysis
        .analysis
        .proof
        .nodes
        .values()
        .find(|node| matches!(&node.operation, Operation::CommitBinding(..)))
        .expect("analysis has a typed source binding")
        .id;

    let mut hash_tampered = analysis.analysis.proof.clone();
    let node = hash_tampered.nodes.get_mut(&binding).expect("binding node");
    let Operation::CommitBinding(certificate) = &mut node.operation else {
        panic!("expected commit binding");
    };
    certificate.commit = ContentHash::domain_separated("test/tamper", b"other-source");
    assert!(matches!(
        hash_tampered.check(),
        Err(CheckError::TamperedNode { id }) if id == binding
    ));

    let mut input_tampered = analysis.analysis.proof.clone();
    input_tampered
        .nodes
        .get_mut(&binding)
        .expect("binding node")
        .inputs
        .clear();
    assert!(matches!(
        input_tampered.check(),
        Err(CheckError::TamperedNode { id }) if id == binding
    ));

    let mut root_tampered = analysis.analysis.proof.clone();
    root_tampered.roots.retain(|root| *root != binding);
    assert!(matches!(
        root_tampered.check(),
        Err(CheckError::InvalidCommitBinding { id }) if id == binding
    ));

    let mut metadata_tampered = analysis.analysis.proof.clone();
    metadata_tampered
        .nodes
        .get_mut(&binding)
        .expect("binding node")
        .metadata
        .insert("source-commit".into(), "forged-metadata".into());
    assert!(matches!(
        metadata_tampered.check(),
        Err(CheckError::TamperedNode { id }) if id == binding
    ));
}

#[test]
fn commit_binding_is_unique_and_covers_all_other_roots() {
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
"#,
    );
    let mut duplicate = analysis.analysis.proof.clone();
    let binding = duplicate
        .nodes
        .values()
        .find(|node| matches!(&node.operation, Operation::CommitBinding(..)))
        .cloned()
        .expect("analysis has a typed source binding");
    let extra = axiom_ledger::proof::Node::new(
        "second source commit",
        binding.operation.clone(),
        binding.inputs.clone(),
        binding.metadata.clone(),
    );
    duplicate.insert(extra);
    assert!(matches!(
        duplicate.check(),
        Err(CheckError::InvalidCommitBinding { .. })
    ));

    let mut missing_terminal = analysis.analysis.proof.clone();
    let terminal = missing_terminal
        .roots
        .iter()
        .copied()
        .find(|root| *root != binding.id)
        .expect("analysis has a terminal root");
    missing_terminal.roots.retain(|root| *root != terminal);
    assert!(matches!(
        missing_terminal.check(),
        Err(CheckError::InvalidCommitBinding { .. })
    ));
}

#[test]
fn generic_observation_cannot_spoof_the_reserved_commit_binding_namespace() {
    let mut proof = Proof::new();
    let node = axiom_ledger::proof::Node::new(
        "legacy string binding",
        Operation::Observation {
            source: format!(
                "commit:{}",
                ContentHash::domain_separated("test", b"source")
            ),
        },
        Vec::new(),
        Default::default(),
    );
    let root = proof.insert(node);
    proof.root(root);
    assert!(matches!(
        proof.check(),
        Err(CheckError::InvalidOperation { id }) if id == root
    ));
}
