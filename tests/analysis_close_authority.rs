use axiom_ledger::store::{
    AnalysisArtifactId, Close, ObjectKind, Period, ProofObject, Signature, SignatureVerifier,
    StoreError,
};
use axiom_ledger::workspace::Workspace;

const SOURCE: &str = r#"book tax
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
sell sell/one on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
observe settlement sell/one 500 USD into cash
"#;

const BLOCKED_SOURCE: &str = r#"book tax
sell sell/blocked on 2026-09-20
  1 ABC from brokerage
  for 50 USD
  lot ?lot
"#;

struct EchoVerifier;

impl SignatureVerifier for EchoVerifier {
    fn verify(
        &self,
        _signer: &str,
        algorithm: &str,
        payload: axiom_ledger::model::ContentHash,
        signature: &[u8],
    ) -> bool {
        algorithm == "test-only" && signature == payload.as_bytes()
    }
}

#[test]
fn workspace_close_is_sealed_to_checked_analysis_results() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let mut analysis = workspace.analyze_commit(source.commit).unwrap();
    analysis.analysis.book = "caller/relabelled".into();
    analysis.analysis.sales[0].date = "2030-01-01".parse().unwrap();
    analysis.analysis.journal.clear();
    let period = Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();
    let close_id = workspace
        .close_sale_ledger(source.commit, period.clone())
        .expect("checked sale analysis closes");
    let close = workspace.store().close(close_id).unwrap();
    let artifact = workspace
        .store()
        .analysis_artifact(close.analysis_artifact)
        .unwrap();
    assert_eq!(artifact.analysis_commit(), analysis.analysis_commit());
    let stored_analysis = workspace
        .store()
        .commit(artifact.analysis_commit())
        .unwrap();
    assert_eq!(stored_analysis.parents, vec![source.commit]);
    assert_eq!(stored_analysis.proofs, vec![analysis.proof_id()]);
    assert_eq!(artifact.book().as_str(), "tax");
    assert_eq!(artifact.period(), &period);

    let signed = Close::new(
        period.clone(),
        close.book.clone(),
        close.policies.clone(),
        close.source,
        close.analysis_artifact,
    );
    let payload = signed.signing_hash();
    let signed = signed.with_signatures([Signature::new(
        "alice",
        "test-only",
        payload.as_bytes().to_vec(),
    )]);

    let mut store = workspace.store().clone();
    let signed_id = store
        .put_close_verified(signed, &EchoVerifier)
        .expect("a valid signature authorizes the sealed close");
    assert_eq!(store.close(signed_id).unwrap().signatures.len(), 1);
    let restated = store
        .put_close(
            Close::new(
                period,
                close.book.clone(),
                close.policies.clone(),
                close.source,
                close.analysis_artifact,
            )
            .superseding(close_id),
        )
        .expect("restatement retains the same sealed analysis");
    assert_eq!(store.close(restated).unwrap().supersedes, Some(close_id));
    assert!(store.close(close_id).is_ok());
}

#[test]
fn generic_stale_and_relabelled_authority_are_rejected() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let period = Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();
    let close_id = workspace
        .close_sale_ledger(source.commit, period.clone())
        .unwrap();
    let close = workspace.store().close(close_id).unwrap().clone();

    let changed = workspace
        .load_source("changed-ledger", format!("{SOURCE}\n# later correction\n"))
        .unwrap();
    let mut store = workspace.store().clone();
    let generic = store
        .put_proof(ProofObject::new([], b"forged close"))
        .unwrap();
    assert!(matches!(
        store.put_close(Close::new(
            period.clone(),
            "tax",
            [],
            source.commit,
            AnalysisArtifactId::new(generic.hash()),
        )),
        Err(StoreError::WrongKind {
            expected: ObjectKind::AnalysisArtifact,
            actual: ObjectKind::Proof,
            ..
        })
    ));

    assert!(
        store
            .put_close(Close::new(
                period.clone(),
                close.book.clone(),
                close.policies.clone(),
                changed.commit,
                close.analysis_artifact,
            ))
            .is_err()
    );

    let wrong_period =
        Period::new("2026-02-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();
    let invalid = Close::new(
        wrong_period,
        close.book,
        close.policies,
        close.source,
        close.analysis_artifact,
    );
    let payload = invalid.signing_hash();
    let signed = invalid.with_signatures([Signature::new(
        "alice",
        "test-only",
        payload.as_bytes().to_vec(),
    )]);
    assert!(matches!(
        store.put_close_verified(signed, &EchoVerifier),
        Err(StoreError::InvalidObject(reason))
            if reason.contains("exactly match its sealed analysis artifact")
    ));
}

#[test]
fn close_rejects_empty_blocked_and_out_of_period_ledgers() {
    let period = Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();

    let mut empty = Workspace::new();
    let source = empty.load_source("empty", "book tax\n").unwrap();
    assert!(
        empty
            .close_sale_ledger(source.commit, period.clone())
            .is_err()
    );

    let mut blocked = Workspace::new();
    let source = blocked.load_source("blocked", BLOCKED_SOURCE).unwrap();
    assert!(
        blocked
            .close_sale_ledger(source.commit, period.clone())
            .is_err()
    );

    let mut contradictory = Workspace::new();
    let contradictory_source = SOURCE.replace("500 USD into cash", "499 USD into cash");
    let source = contradictory
        .load_source("contradictory", contradictory_source)
        .unwrap();
    assert!(
        contradictory
            .close_sale_ledger(source.commit, period.clone())
            .is_err()
    );

    let mut outside = Workspace::new();
    let future_source = SOURCE.replace("2026-09-20", "2027-09-20");
    let source = outside.load_source("outside", future_source).unwrap();
    assert!(outside.close_sale_ledger(source.commit, period).is_err());
}
