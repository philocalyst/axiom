use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName, Span,
};
use axiom_ledger::package_compiler::{PackageInput, SchemaCapability};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::workspace::Workspace;

fn schema() -> QualifiedName {
    QualifiedName {
        module: ModulePath::new(vec![Name::new("types").unwrap()]).unwrap(),
        name: Name::new("SettlementState").unwrap(),
    }
}

fn package() -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new("payments", Version::new(1, 0, 0), "settlement proof");
    let module = AstModule {
        path: ModulePath::root(Name::new("types").unwrap()),
        declarations: vec![AstDeclaration {
            name: "SettlementState".to_owned(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: vec![
                    ("settlement".to_owned(), AstType::Text),
                    ("kind".to_owned(), AstType::Text),
                    ("state".to_owned(), AstType::Text),
                    ("at".to_owned(), AstType::Text),
                    ("from".to_owned(), AstType::Text),
                    ("to".to_owned(), AstType::Text),
                    ("instrument".to_owned(), AstType::Text),
                    ("amount".to_owned(), AstType::Decimal),
                ],
                open_tail: None,
            }),
            span: Span::default(),
        }],
    };
    let package = PackageInput::new(manifest.clone(), [axiom_ledger::hir::lower(module)])
        .with_schema_capability(schema(), SchemaCapability::SettlementStateV1);
    let lockfile = Lockfile {
        roots: vec![Dependency::new(
            manifest.name.clone(),
            VersionReq::Exact(manifest.version),
        )],
        packages: vec![LockedPackage {
            name: manifest.name.clone(),
            version: manifest.version,
            hash: manifest.hash(),
            dependencies: Vec::new(),
        }],
    };
    (package, lockfile)
}

fn workspace_for(source_text: &str) -> (Workspace, axiom_ledger::store::CommitId) {
    let (package, lockfile) = package();
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", source_text).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap();
    (workspace, pinned.commit)
}

fn source_text() -> &'static str {
    "form first/1 : payments::types::SettlementState\n  settlement payment\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform second/1 : payments::types::SettlementState\n  settlement payment\n  kind ach\n  state presented\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n"
}

fn workspace() -> (Workspace, axiom_ledger::store::CommitId) {
    workspace_for(source_text())
}

fn assert_entry_tamper_rejected<F>(
    proof: &axiom_ledger::settlement_proof::SettlementStateV1Proof,
    workspace: &Workspace,
    mutate: F,
) where
    F: FnOnce(&mut axiom_ledger::settlement_proof::SettlementStateV1Entry),
{
    let mut forged = proof.clone();
    mutate(&mut forged.coverage[0]);
    forged.coverage_hash = forged.recompute_coverage_hash();
    assert!(forged.check(workspace.store()).is_err());
}

#[test]
fn persists_each_capable_schema_binding_in_one_ordered_proof() {
    let (first, _) = package();
    let mut second = first.clone();
    second.manifest = PackageManifest::new(
        "other",
        Version::new(1, 0, 0),
        "second settlement proof package",
    );
    let lockfile = Lockfile {
        roots: vec![
            Dependency::new(
                first.manifest.name.clone(),
                VersionReq::Exact(first.manifest.version),
            ),
            Dependency::new(
                second.manifest.name.clone(),
                VersionReq::Exact(second.manifest.version),
            ),
        ],
        packages: vec![
            LockedPackage {
                name: first.manifest.name.clone(),
                version: first.manifest.version,
                hash: first.manifest.hash(),
                dependencies: Vec::new(),
            },
            LockedPackage {
                name: second.manifest.name.clone(),
                version: second.manifest.version,
                hash: second.manifest.hash(),
                dependencies: Vec::new(),
            },
        ],
    };
    let mut workspace = Workspace::new();
    let source = workspace
        .load_source(
            "ledger",
            "form first/1 : payments::types::SettlementState\n  settlement payment\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform second/1 : other::types::SettlementState\n  settlement other-payment\n  kind ach\n  state issued\n  at 2026-01-01\n  from bob\n  to bank\n  instrument EUR\n  amount 7\n",
        )
        .unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([first, second], &lockfile)
        .unwrap();
    let commit = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap()
        .commit;
    let result = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap();
    assert_eq!(proof.coverage.len(), 2);
    assert_ne!(
        proof.coverage[0].package_root,
        proof.coverage[1].package_root
    );
    assert_eq!(proof.coverage[0].qualified_schema, "types::SettlementState");
    assert_eq!(proof.coverage[1].qualified_schema, "types::SettlementState");
    proof.check(workspace.store()).unwrap();
}

#[test]
fn persists_a_typed_settlement_proof_and_rechecks_it() {
    let (mut workspace, commit) = workspace();
    let result = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap();
    assert_eq!(proof.source_commit, commit);
    assert_eq!(proof.coverage.len(), 2);
    assert_eq!(proof.coverage[0].occurrence.as_str(), "first/1");
    assert_eq!(proof.coverage[1].occurrence.as_str(), "second/1");
    proof.check(workspace.store()).unwrap();
    assert_eq!(
        result.proof_id.kind(),
        axiom_ledger::store::ObjectKind::SettlementStateProof
    );
}

#[test]
fn settlement_proof_is_discoverable_from_its_projection_commit() {
    let (mut workspace, source_commit) = workspace();
    let result = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    let source = workspace.store().commit(result.source_commit).unwrap();
    let projection = workspace.store().commit(result.projection_commit).unwrap();

    assert_eq!(projection.parents, vec![result.source_commit]);
    assert_eq!(projection.settlement_proofs, vec![result.proof_id]);
    assert!(projection.evidence.is_empty());
    assert!(projection.statements.is_empty());
    assert!(projection.decisions.is_empty());
    assert!(projection.completeness.is_empty());
    assert!(projection.proofs.is_empty());
    assert_eq!(projection.packages, source.packages);
    assert_eq!(projection.compiled_artifact, source.compiled_artifact);
    assert_eq!(projection.conflicts, source.conflicts);
    assert_eq!(projection.author, "workspace/settlement-proof");
    assert_eq!(
        workspace
            .store()
            .settlement_state_proof(result.proof_id)
            .unwrap()
            .source_commit,
        source_commit
    );
}

#[test]
fn settlement_proof_roots_cannot_be_attached_to_source_or_unfixed_children() {
    let (mut workspace, source_commit) = workspace();
    let result = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    let source = workspace.store().commit(source_commit).unwrap();

    let mut source_forgery = axiom_ledger::store::Commit::new(
        [source_commit],
        source.evidence.clone(),
        [],
        [],
        [],
        source.packages.clone(),
        [],
        "workspace/source",
    )
    .with_settlement_proofs([result.proof_id]);
    if let Some(artifact) = source.compiled_artifact {
        source_forgery = source_forgery.with_compiled_artifact(artifact);
    }
    source_forgery = source_forgery.with_conflicts(source.conflicts.clone());
    let mut store = workspace.store().clone();
    assert!(store.put_commit(source_forgery).is_err());

    let invalid_author = axiom_ledger::store::Commit::new(
        [source_commit],
        [],
        [],
        [],
        [],
        source.packages.clone(),
        [],
        "caller",
    )
    .with_settlement_proofs([result.proof_id]);
    assert!(store.put_commit(invalid_author).is_err());

    let left = store
        .put_commit(axiom_ledger::store::Commit::new(
            [source_commit],
            [],
            [],
            [],
            [],
            [],
            [],
            "left-derived",
        ))
        .unwrap();
    let right = store
        .put_commit(axiom_ledger::store::Commit::new(
            [source_commit],
            [],
            [],
            [],
            [],
            [],
            [],
            "right-derived",
        ))
        .unwrap();
    let forged_merge =
        axiom_ledger::store::Commit::new([left, right], [], [], [], [], [], [], "merge")
            .with_settlement_proofs([result.proof_id]);
    assert!(store.put_commit(forged_merge).is_err());
}

#[test]
fn proof_checker_rejects_a_commit_with_non_source_roots() {
    let (mut workspace, source_commit) = workspace();
    let result = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    let original = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap()
        .clone();
    let source = workspace.store().commit(source_commit).unwrap().clone();
    let mut store = workspace.store().clone();
    let statement = store
        .put_statement(axiom_ledger::store::Statement::new(
            "not", "a-source", "root",
        ))
        .unwrap();
    let mut fake = axiom_ledger::store::Commit::new(
        [source_commit],
        source.evidence,
        [statement],
        source.decisions,
        [],
        source.packages,
        [],
        "workspace/source",
    )
    .with_conflicts(source.conflicts);
    if let Some(artifact) = source.compiled_artifact {
        fake = fake.with_compiled_artifact(artifact);
    }
    let fake = store.put_commit(fake).unwrap();
    let mut forged = original;
    forged.source_commit = fake;

    assert!(forged.check(&store).is_err());
    assert!(store.put_settlement_state_proof(forged).is_err());
}

#[test]
#[ignore = "release stress gate for 1,000 independently checked capable forms"]
fn stress_thousand_settlement_forms_project_prove_anchor_and_verify() {
    let mut source = String::new();
    for index in 0..1_000 {
        source.push_str(&format!(
            "form payment/{index} : payments::types::SettlementState\n  settlement payment-{index}\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100.25\n\n"
        ));
    }
    let (mut workspace, source_commit) = workspace_for(&source);
    let result = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap();
    assert_eq!(proof.coverage().len(), 1_000);
    assert_eq!(
        workspace
            .store()
            .commit(result.projection_commit)
            .unwrap()
            .settlement_proofs,
        vec![result.proof_id]
    );
    workspace.store().verify().unwrap();
}

#[test]
fn settlement_proof_roots_propagate_through_merge_snapshots() {
    let (mut workspace, source_commit) = workspace();
    let result = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    let mut store = workspace.store().clone();
    let merged = store
        .merge(
            source_commit,
            result.projection_commit,
            source_commit,
            "merge",
        )
        .unwrap();
    assert_eq!(
        store.commit(merged.commit).unwrap().settlement_proofs,
        vec![result.proof_id]
    );
    store.verify().unwrap();
}

#[test]
fn correction_creates_new_authority_without_rewriting_history() {
    let (mut workspace, original_commit) = workspace();
    let original = workspace
        .persist_settlement_state_proof(original_commit)
        .unwrap();
    let corrected_source = source_text().replace("amount 100", "amount 101");
    let corrected_commit = workspace
        .correct_source(original_commit, corrected_source)
        .unwrap()
        .commit;
    let corrected = workspace
        .persist_settlement_state_proof(corrected_commit)
        .unwrap();

    assert_ne!(original.source_commit, corrected.source_commit);
    assert_ne!(original.projection_commit, corrected.projection_commit);
    assert_ne!(original.proof_id, corrected.proof_id);
    let old_proof = workspace
        .store()
        .settlement_state_proof(original.proof_id)
        .unwrap();
    let new_proof = workspace
        .store()
        .settlement_state_proof(corrected.proof_id)
        .unwrap();
    assert_eq!(old_proof.coverage[0].amount.canonical(), "100 USD");
    assert_eq!(new_proof.coverage[0].amount.canonical(), "101 USD");
    old_proof.check(workspace.store()).unwrap();
    new_proof.check(workspace.store()).unwrap();
    workspace.store().verify().unwrap();
}

#[test]
fn checker_rejects_reordered_or_rehashed_coverage() {
    let (mut workspace, commit) = workspace();
    let result = workspace.persist_settlement_state_proof(commit).unwrap();
    let original = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap()
        .clone();

    let mut reordered = original.clone();
    reordered.coverage.swap(0, 1);
    let mut store = workspace.store().clone();
    assert!(store.put_settlement_state_proof(reordered).is_err());

    let mut forged = original;
    forged.coverage[0].value_hash = axiom_ledger::model::ContentHash::ZERO;
    let mut store = workspace.store().clone();
    assert!(store.put_settlement_state_proof(forged).is_err());

    let mut evidence_forged = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap()
        .clone();
    evidence_forged.source_evidence.content_hash = axiom_ledger::model::ContentHash::ZERO;
    let mut store = workspace.store().clone();
    assert!(store.put_settlement_state_proof(evidence_forged).is_err());
}

#[test]
fn checker_rejects_tampered_entry_bindings_and_semantics() {
    let (mut workspace, commit) = workspace();
    let result = workspace.persist_settlement_state_proof(commit).unwrap();
    let original = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap()
        .clone();

    // Package/schema encodings and source/value identities are independently
    // rebound to the exact re-elaborated form by the checker.
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.package_root = axiom_ledger::model::ContentHash::ZERO;
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.qualified_schema = "types::NotSettlementState".to_owned();
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.schema_id = axiom_ledger::model::ContentHash::ZERO;
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.occurrence = axiom_ledger::model::OccurrenceId::new("forged/1");
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.value_hash = axiom_ledger::model::ContentHash::domain_separated(
            "test/settlement-proof-tamper",
            b"value",
        );
    });

    // Every semantic scalar is source-bound as well as checked locally.
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.settlement = "other-payment".to_owned();
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.rail = axiom_ledger::model::SettlementKind::Card;
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.state = axiom_ledger::ontology::SettlementState::Authorized;
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.date = axiom_ledger::model::Date::new(2026, 1, 3).unwrap();
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.from = axiom_ledger::model::EntityId::new("forged-sender");
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.to = axiom_ledger::model::EntityId::new("forged-recipient");
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.instrument = axiom_ledger::model::InstrumentId::new("EUR");
    });
    assert_entry_tamper_rejected(&original, &workspace, |entry| {
        entry.amount = axiom_ledger::model::Quantity::with_unit(
            axiom_ledger::exact::ExactNumber::parse("101").unwrap(),
            "USD",
        )
        .unwrap();
    });
}

#[test]
fn failed_persistence_does_not_change_workspace_store() {
    let (mut workspace, commit) = workspace_for(
        "form first/1 : payments::types::SettlementState\n  settlement payment\n  kind ach\n  state issued\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform second/1 : payments::types::SettlementState\n  settlement payment\n  kind ach\n  state presented\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n",
    );
    let before = workspace.store().len();
    assert!(workspace.persist_settlement_state_proof(commit).is_err());
    assert_eq!(workspace.store().len(), before);
}

#[test]
fn checker_rejects_oversized_typed_payload_before_persistence() {
    let (mut workspace, commit) = workspace();
    let result = workspace.persist_settlement_state_proof(commit).unwrap();
    let mut forged = workspace
        .store()
        .settlement_state_proof(result.proof_id)
        .unwrap()
        .clone();
    forged.coverage[0].settlement =
        "x".repeat(axiom_ledger::settlement_proof::MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES + 1);
    let before = workspace.store().len();
    assert!(
        workspace
            .store()
            .clone()
            .put_settlement_state_proof(forged)
            .is_err()
    );
    assert_eq!(workspace.store().len(), before);
}
