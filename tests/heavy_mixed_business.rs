//! A mixed-business end-to-end ledger that keeps each semantic concern honest:
//! direct gains remain usable beside conflicting valuation evidence, while
//! observed cash and positions must reconcile before a close can be sealed.

use axiom_ledger::engine::{IssueCode, ObservationStatus, QuoteStatus, RecognitionStatus};
use axiom_ledger::store::{ObjectKind, Period, StoreError};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const SOURCE: &str = include_str!("../fixtures/heavy/mixed_business_correction.axm");

fn period() -> Period {
    Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap()
}

fn corrected_settlement(source: &str) -> String {
    source.replace(
        "observe settlement sale/fifo 750 USD into checking",
        "observe settlement sale/fifo 749 USD into checking",
    )
}

fn conflicted_decision(source: &str) -> String {
    format!("{source}\ndecide sale/fifo lot inventory/late\n")
}

fn assert_obligation_network(analysis: &axiom_ledger::engine::Analysis) {
    let obligation = analysis
        .obligations
        .iter()
        .find(|value| value.id == "invoice/card")
        .unwrap();
    assert_eq!(obligation.promised.canonical(), "100 USD");
    assert_eq!(
        obligation.remaining.as_ref().map(|value| value.canonical()),
        Some("0 USD".to_owned())
    );
    assert_eq!(format!("{:?}", obligation.status), "Satisfied");

    let history = analysis
        .settlement_histories
        .iter()
        .find(|value| value.id == "card/payment")
        .unwrap();
    assert_eq!(history.amount.canonical(), "100 USD");
    assert_eq!(format!("{:?}", history.current), "Settled");
    assert!(history.effective);

    let satisfaction = analysis
        .satisfactions
        .iter()
        .find(|value| value.id == "allocation/card")
        .unwrap();
    assert_eq!(satisfaction.amount.canonical(), "100 USD");
    assert!(satisfaction.effective);
}

fn assert_gains_and_observations(
    analysis: &axiom_ledger::engine::Analysis,
    fifo_gain: &str,
    explicit_gain: &str,
    settlements_reconciled: bool,
    expected_blocked: bool,
) {
    let fifo = analysis.sale("sale/fifo").unwrap();
    assert_eq!(fifo.status, RecognitionStatus::Recognized);
    assert_eq!(fifo.allocations.len(), 2);
    assert_eq!(
        fifo.selected_lots,
        vec!["inventory/early", "inventory/late"]
    );
    assert_eq!(
        fifo.recognized.as_ref().unwrap().basis.canonical(),
        "703/2 USD"
    );
    assert_eq!(
        fifo.recognized.as_ref().unwrap().gain.canonical(),
        fifo_gain
    );

    let explicit = analysis.sale("sale/explicit").unwrap();
    assert_eq!(explicit.status, RecognitionStatus::Recognized);
    assert_eq!(explicit.selected_lots, vec!["inventory/late"]);
    assert_eq!(
        explicit.recognized.as_ref().unwrap().basis.canonical(),
        "301/5 USD"
    );
    assert_eq!(
        explicit.recognized.as_ref().unwrap().gain.canonical(),
        explicit_gain
    );

    let position = analysis
        .positions
        .iter()
        .find(|value| value.account == "brokerage")
        .unwrap();
    assert_eq!(position.quantity.canonical(), "3 ABC");
    assert_eq!(position.status, ObservationStatus::Reconciled);

    assert_eq!(analysis.settlements.len(), 2);
    assert_eq!(
        analysis
            .settlements
            .iter()
            .filter(|value| value.status == ObservationStatus::Reconciled)
            .count(),
        if settlements_reconciled { 2 } else { 1 }
    );
    assert_eq!(
        analysis.journal.len(),
        if settlements_reconciled { 2 } else { 1 }
    );
    assert!(analysis.journal.iter().all(|entry| entry.balanced()));
    assert_obligation_network(analysis);

    assert_eq!(analysis.quote_status.len(), 1);
    assert!(matches!(
        analysis.quote_status.values().next().unwrap(),
        QuoteStatus::Ambiguous { .. }
    ));
    assert!(
        analysis
            .issues
            .iter()
            .any(|issue| issue.code == IssueCode::AmbiguousQuote)
    );
    assert_eq!(
        analysis.blocked(),
        expected_blocked,
        "issues: {:?}",
        analysis.issues
    );
}

#[test]
fn mixed_business_close_and_corrections_preserve_independent_truths() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("mixed-business", SOURCE).unwrap();
    let clean = workspace.analyze_commit(source.commit).unwrap();
    clean.check_proof().unwrap();
    assert_gains_and_observations(&clean.analysis, "797/2 USD", "399/5 USD", true, false);

    let close_id = workspace
        .close_sale_ledger(source.commit, period())
        .expect("clean mixed ledger closes");
    let close = workspace.store().close(close_id).unwrap();
    assert_eq!(close.source, source.commit);
    let artifact = workspace
        .store()
        .analysis_artifact(close.analysis_artifact)
        .unwrap();
    assert_eq!(artifact.period(), &period());
    assert_eq!(
        artifact.analysis_commit().hash(),
        clean.analysis_commit().hash()
    );
    let analysis_commit = workspace
        .store()
        .commit(artifact.analysis_commit())
        .unwrap();
    assert_eq!(analysis_commit.parents, vec![source.commit]);
    assert_eq!(analysis_commit.proofs.len(), 1);
    let proof = workspace.store().proof(analysis_commit.proofs[0]).unwrap();
    assert_eq!(proof.roots, vec![source.commit.hash()]);
    assert_eq!(clean.proof().bound_commit(), Ok(Some(source.commit.hash())));
    assert_eq!(clean.proof_id().kind(), ObjectKind::Proof);
    workspace.store().verify().unwrap();

    let corrected_text = corrected_settlement(SOURCE);
    let corrected_source = workspace
        .correct_source(source.commit, &corrected_text)
        .unwrap();
    let before_failed_close = workspace.store().len();
    assert!(matches!(
        workspace.close_sale_ledger(corrected_source.commit, period()),
        Err(WorkspaceError::Store(StoreError::InvalidObject(message)))
            if message == "sale-ledger close requires an unblocked analysis"
    ));
    assert_eq!(workspace.store().len(), before_failed_close);

    let corrected = workspace.analyze_commit(corrected_source.commit).unwrap();
    corrected.check_proof().unwrap();
    assert_gains_and_observations(&corrected.analysis, "797/2 USD", "399/5 USD", false, true);
    let fifo_settlement = corrected
        .analysis
        .settlements
        .iter()
        .find(|value| value.reference == "sale/fifo")
        .unwrap();
    assert_eq!(fifo_settlement.quantity.canonical(), "749 USD");
    assert_eq!(fifo_settlement.status, ObservationStatus::Conflict);
    assert_eq!(corrected.analysis.journal.len(), 1);
    assert!(corrected.analysis.journal[0].sale == "sale/explicit");
    assert!(corrected.analysis.issues.iter().any(|issue| {
        issue.code == IssueCode::SettlementConflict && issue.sale.as_deref() == Some("sale/fifo")
    }));
    workspace.store().close(close_id).unwrap();

    let conflict_source = workspace
        .correct_source(
            corrected_source.commit,
            conflicted_decision(&corrected_text),
        )
        .unwrap();
    let before_conflict_close = workspace.store().len();
    assert!(
        workspace
            .close_sale_ledger(conflict_source.commit, period())
            .is_err()
    );
    assert_eq!(workspace.store().len(), before_conflict_close);
    let conflict = workspace.analyze_commit(conflict_source.commit).unwrap();
    conflict.check_proof().unwrap();
    let conflicted_fifo = conflict.analysis.sale("sale/fifo").unwrap();
    assert!(matches!(
        conflicted_fifo.status,
        RecognitionStatus::Conflict { .. }
    ));
    assert!(
        conflict
            .analysis
            .sale("sale/explicit")
            .unwrap()
            .status
            .is_blocked()
    );
    assert!(conflict.analysis.blocked());
    assert!(conflict.analysis.positions.iter().any(|value| {
        value.account == "brokerage" && value.status == ObservationStatus::Conflict
    }));
    assert!(conflict.analysis.settlements.iter().any(|value| {
        value.reference == "sale/fifo" && value.status == ObservationStatus::Observed
    }));
    assert_obligation_network(&conflict.analysis);
    let restored_source = workspace
        .correct_source(conflict_source.commit, SOURCE)
        .unwrap();
    let restored = workspace.analyze_commit(restored_source.commit).unwrap();
    restored.check_proof().unwrap();
    assert_ne!(restored.proof_id(), clean.proof_id());
    assert_ne!(restored.analysis_commit(), clean.analysis_commit());
    assert_gains_and_observations(&restored.analysis, "797/2 USD", "399/5 USD", true, false);
    let restored_close_id = workspace
        .close_sale_ledger(restored_source.commit, period())
        .expect("restored clean source closes");
    assert_ne!(restored_close_id, close_id);
    let restored_close = workspace.store().close(restored_close_id).unwrap();
    assert_eq!(restored_close.source, restored_source.commit);
    assert_eq!(restored_close.supersedes, None);
    workspace.store().close(close_id).unwrap();
    workspace.store().verify().unwrap();

    // The source correction changes the exact source commit and therefore the
    // proof binding, while the economic truth untouched by the correction is
    // byte-for-byte stable.
    assert_ne!(source.commit.hash(), corrected_source.commit.hash());
    assert_ne!(
        corrected_source.commit.hash(),
        conflict_source.commit.hash()
    );
    assert_ne!(conflict_source.commit.hash(), restored_source.commit.hash());
    assert_ne!(
        clean.proof().bound_commit(),
        restored.proof().bound_commit()
    );
    assert_eq!(clean.analysis.obligations, restored.analysis.obligations);
    assert_eq!(
        clean.analysis.settlement_histories,
        restored.analysis.settlement_histories
    );
    assert_eq!(
        clean.analysis.satisfactions,
        restored.analysis.satisfactions
    );
}
