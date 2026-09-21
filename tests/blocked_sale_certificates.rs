//! Public blocked-sale answers must carry an independently checked proposition.

use axiom_ledger::engine::RecognitionStatus;
use axiom_ledger::proof::Operation;
use axiom_ledger::workspace::Workspace;

fn analyzed(source: &str) -> axiom_ledger::workspace::CommitAnalysis {
    let mut workspace = Workspace::new();
    let source = workspace
        .load_source("blocked-sale-tests", source)
        .expect("source loads");
    workspace
        .analyze_commit(source.commit)
        .expect("source analysis succeeds")
}

#[test]
fn blocked_sale_answer_binds_exact_sale_identity_and_reason() {
    let analysis = analyzed(
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
    );
    analysis.check_proof().expect("blocked proof checks");

    let sale = &analysis.analysis.sales[0];
    assert!(matches!(sale.status, RecognitionStatus::Ambiguous { .. }));
    let node = analysis
        .analysis
        .proof
        .node(sale.proof)
        .expect("blocked sale node exists");
    assert!(matches!(node.operation, Operation::BlockedSale(_)));
    if let Operation::BlockedSale(certificate) = &node.operation {
        assert_eq!(certificate.sale, sale.id);
        assert_eq!(certificate.quantity, sale.quantity.number);
        assert_eq!(certificate.proceeds, sale.proceeds.number);
        assert_eq!(certificate.quantity_unit, "ABC");
        assert_eq!(certificate.value_unit, "USD");
        assert_eq!(certificate.account, "brokerage");
        assert_eq!(certificate.asset, "ABC");
        assert_eq!(certificate.reason, "ambiguous-lot:buy/a,buy/b");
        assert!(node.inputs.contains(&certificate.source_proof));
    }
}

#[test]
fn blocked_sale_answer_rejects_public_field_tampering() {
    let source = r#"book tax-us
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
"#;
    let mut analysis = analyzed(source);
    analysis.analysis.sales[0].account = "other-brokerage".into();
    assert!(matches!(
        analysis.analysis.check_semantics(),
        Err(axiom_ledger::engine::AnalysisCheckError::InvalidBlockedSaleResult { .. })
    ));

    let mut analysis = analyzed(source);
    analysis.analysis.sales[0].proceeds.number = 81_i64.into();
    assert!(matches!(
        analysis.analysis.check_semantics(),
        Err(axiom_ledger::engine::AnalysisCheckError::InvalidBlockedSaleResult { .. })
    ));

    let mut analysis = analyzed(source);
    analysis.analysis.sales[0].status = RecognitionStatus::MissingLot;
    assert!(matches!(
        analysis.analysis.check_semantics(),
        Err(axiom_ledger::engine::AnalysisCheckError::InvalidBlockedSaleResult { .. })
    ));
}
