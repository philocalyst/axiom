use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName, Span,
};
use axiom_ledger::package_compiler::{
    FormFieldMappingV1, FormSurfaceV1, FormTemplateV1, PackageInput, SchemaCapability,
};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::settlement_books::SettlementReportingPeriod;
use axiom_ledger::settlement_proof::{
    SETTLEMENT_STATE_PROOF_VERSION, SETTLEMENT_STATE_PROOF_VERSION_V2, SettlementStateFormOriginV2,
};
use axiom_ledger::store::CommitId;
use axiom_ledger::workspace::Workspace;

fn schema() -> QualifiedName {
    QualifiedName {
        module: ModulePath::new(vec![Name::new("types").unwrap()]).unwrap(),
        name: Name::new("SettlementState").unwrap(),
    }
}

fn template() -> QualifiedName {
    QualifiedName {
        module: ModulePath::new(vec![Name::new("forms").unwrap()]).unwrap(),
        name: Name::new("CompactSettlement").unwrap(),
    }
}

fn package(compact: bool) -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new("payments", Version::new(1, 0, 0), "compact proof");
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
    let mut input = PackageInput::new(manifest.clone(), [axiom_ledger::hir::lower(module)])
        .with_schema_capability(schema(), SchemaCapability::SettlementStateV1);
    if compact {
        input = input.with_form_surface(FormSurfaceV1::new([FormTemplateV1::new(
            template(),
            schema(),
            [
                ("s", "settlement"),
                ("k", "kind"),
                ("x", "state"),
                ("d", "at"),
                ("f", "from"),
                ("t", "to"),
                ("i", "instrument"),
                ("a", "amount"),
            ]
            .into_iter()
            .map(|(source, target)| FormFieldMappingV1::new(source, target)),
        )]));
    }
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
    (input, lockfile)
}

fn package_with_template_name(name: &str) -> (PackageInput, Lockfile) {
    let (input, lockfile) = package(false);
    let target = QualifiedName {
        module: ModulePath::new(vec![Name::new("forms").unwrap()]).unwrap(),
        name: Name::new(name).unwrap(),
    };
    (
        input.with_form_surface(FormSurfaceV1::new([FormTemplateV1::new(
            target,
            schema(),
            [
                ("s", "settlement"),
                ("k", "kind"),
                ("x", "state"),
                ("d", "at"),
                ("f", "from"),
                ("t", "to"),
                ("i", "instrument"),
                ("a", "amount"),
            ]
            .into_iter()
            .map(|(source, target)| FormFieldMappingV1::new(source, target)),
        )])),
        lockfile,
    )
}

fn source(compact: bool) -> String {
    let header = if compact {
        "payments::forms::CompactSettlement"
    } else {
        "payments::types::SettlementState"
    };
    let fields = if compact {
        "s payment\n  k ach\n  x issued\n  d 2026-01-01\n  f alice\n  t bank\n  i USD\n  a 100"
    } else {
        "settlement payment\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100"
    };
    format!("form payment/1 : {header}\n  {fields}\n")
}

fn pinned_workspace(compact: bool) -> (Workspace, CommitId) {
    pinned_workspace_source(compact, &source(compact))
}

fn pinned_workspace_source(compact: bool, source_text: &str) -> (Workspace, CommitId) {
    let (package, lockfile) = package(compact);
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", source_text).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let commit = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap()
        .commit;
    (workspace, commit)
}

fn pinned_workspace_with(
    package: PackageInput,
    lockfile: Lockfile,
    source_text: &str,
) -> (Workspace, CommitId) {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", source_text).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let commit = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap()
        .commit;
    (workspace, commit)
}

fn mixed_source() -> String {
    format!(
        "{}\n{}",
        source(false),
        source(true)
            .replace("payment/1", "payment/2")
            .replace("x issued", "x presented")
            .replace("d 2026-01-01", "d 2026-01-02")
    )
}

#[test]
fn compact_settlement_proof_is_v2_and_reloads_through_world_and_close() {
    let (mut workspace, commit) = pinned_workspace(true);
    let persisted = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(persisted.proof_id)
        .unwrap();
    assert_eq!(proof.version, SETTLEMENT_STATE_PROOF_VERSION_V2);
    assert!(matches!(
        proof.coverage[0].origin,
        Some(SettlementStateFormOriginV2::Compact { .. })
    ));
    let world = workspace.settlement_world(commit).unwrap();
    assert_eq!(world.settlement_proof(), proof);
    let close = workspace
        .persist_settlement_close(
            persisted.projection_commit,
            axiom_ledger::settlement_books::SettlementRecognitionPolicy::Observation,
            SettlementReportingPeriod::new(
                "2026-01-01".parse().unwrap(),
                "2026-01-01".parse().unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(
        workspace
            .store()
            .settlement_close(close)
            .unwrap()
            .proof_commit(),
        persisted.projection_commit
    );
}

#[test]
fn direct_settlement_proof_retains_v1_identity_and_compact_changes_artifact_identity() {
    let (direct_package, direct_lockfile) = package(false);
    let (compact_package, compact_lockfile) = package(true);
    let direct_artifact =
        axiom_ledger::package_compiler::compile([direct_package], &direct_lockfile).unwrap();
    let compact_artifact =
        axiom_ledger::package_compiler::compile([compact_package], &compact_lockfile).unwrap();
    assert_ne!(
        direct_artifact.artifact_hash(),
        compact_artifact.artifact_hash()
    );

    let (mut workspace, commit) = pinned_workspace(false);
    let persisted = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(persisted.proof_id)
        .unwrap();
    assert_eq!(proof.version, SETTLEMENT_STATE_PROOF_VERSION);
    assert_eq!(
        persisted.proof_id.to_string(),
        "b191c78f841b7dd2ae4cb5445b13fb1328194434663c5abb4c787011c5635106"
    );
    assert_eq!(
        axiom_ledger::model::ContentHash::from_digest(blake3::hash(&proof.canonical_bytes()))
            .to_string(),
        "f19cf1b2975b12e9213599968d84d1e33b7a9b4444150f71029997f6880af9a7"
    );
    assert_eq!(proof.canonical_bytes(), {
        let reloaded = workspace
            .store()
            .settlement_state_proof(persisted.proof_id)
            .unwrap();
        reloaded.canonical_bytes()
    });
}

#[test]
fn mixed_direct_and_compact_source_order_is_preserved_in_v2_coverage() {
    let (mut workspace, commit) = pinned_workspace_source(true, &mixed_source());
    let persisted = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(persisted.proof_id)
        .unwrap();
    assert_eq!(proof.version, SETTLEMENT_STATE_PROOF_VERSION_V2);
    assert_eq!(proof.coverage.len(), 2);
    assert!(matches!(
        proof.coverage[0].origin,
        Some(SettlementStateFormOriginV2::Direct)
    ));
    assert!(matches!(
        proof.coverage[1].origin,
        Some(SettlementStateFormOriginV2::Compact { .. })
    ));
}

#[test]
fn v2_origin_tampering_is_rejected_even_after_rehashing_coverage() {
    let (mut workspace, commit) = pinned_workspace(true);
    let persisted = workspace.persist_settlement_state_proof(commit).unwrap();
    let proof = workspace
        .store()
        .settlement_state_proof(persisted.proof_id)
        .unwrap();
    for field in ["template", "target", "mapping-order", "origin-tag"] {
        let mut forged = proof.clone();
        match (field, forged.coverage[0].origin.as_mut()) {
            ("template", Some(SettlementStateFormOriginV2::Compact { template, .. })) => {
                template.push_str("-forged")
            }
            ("target", Some(SettlementStateFormOriginV2::Compact { target, .. })) => {
                target.push_str("-forged");
            }
            ("mapping-order", Some(SettlementStateFormOriginV2::Compact { mappings, .. })) => {
                mappings.reverse()
            }
            ("origin-tag", Some(origin @ SettlementStateFormOriginV2::Compact { .. })) => {
                *origin = SettlementStateFormOriginV2::Direct;
            }
            _ => panic!("expected compact origin"),
        }
        forged.coverage_hash = forged.recompute_coverage_hash();
        assert!(
            forged.check(workspace.store()).is_err(),
            "forged {field} was accepted"
        );
    }

    let mut downgraded = proof.clone();
    downgraded.version = SETTLEMENT_STATE_PROOF_VERSION.to_owned();
    for entry in &mut downgraded.coverage {
        entry.origin = None;
    }
    downgraded.coverage_hash = downgraded.recompute_coverage_hash();
    assert!(
        downgraded.check(workspace.store()).is_err(),
        "a compact source was accepted through the v1 proof path"
    );
}

#[test]
fn changing_a_template_changes_the_new_artifact_while_the_old_proof_remains_valid() {
    let (mut old_workspace, old_commit) = pinned_workspace(true);
    let old_persisted = old_workspace
        .persist_settlement_state_proof(old_commit)
        .unwrap();
    let old_proof = old_workspace
        .store()
        .settlement_state_proof(old_persisted.proof_id)
        .unwrap()
        .clone();

    let (new_package, new_lockfile) = package_with_template_name("CompactSettlementV2");
    let new_source = source(true).replace("CompactSettlement", "CompactSettlementV2");
    let (mut new_workspace, new_commit) =
        pinned_workspace_with(new_package, new_lockfile, &new_source);
    let new_persisted = new_workspace
        .persist_settlement_state_proof(new_commit)
        .unwrap();
    let new_proof = new_workspace
        .store()
        .settlement_state_proof(new_persisted.proof_id)
        .unwrap();
    assert_ne!(old_proof.compiled_artifact, new_proof.compiled_artifact);
    assert_ne!(old_proof.canonical_bytes(), new_proof.canonical_bytes());
    old_proof.check(old_workspace.store()).unwrap();
}

#[test]
fn failed_compact_persistence_is_atomic() {
    let invalid_source = format!(
        "{}\n{}",
        source(false),
        source(true).replace("payment/1", "payment/2")
    );
    let (mut workspace, commit) = pinned_workspace_source(true, &invalid_source);
    let before = workspace.store().clone();
    assert!(workspace.persist_settlement_state_proof(commit).is_err());
    assert_eq!(workspace.store(), &before);
}
