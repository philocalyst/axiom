use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, Span,
};
use axiom_ledger::model::{AccountId, EntityId};
use axiom_ledger::package_compiler::{PackageInput, SchemaCapability};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::settlement_books::{
    SettlementRecognition, SettlementRecognitionPolicy, SettlementReportingPeriod,
};
use axiom_ledger::workspace::Workspace;

fn settlement_package(name: &str) -> (PackageInput, PackageManifest) {
    let manifest = PackageManifest::new(name, Version::new(1, 0, 0), "settlement books");
    let schema = axiom_ledger::hir::QualifiedName {
        module: ModulePath::new(vec![Name::new("types").unwrap()]).unwrap(),
        name: Name::new("SettlementState").unwrap(),
    };
    let module = AstModule {
        path: ModulePath::root(Name::new("types").unwrap()),
        declarations: vec![AstDeclaration {
            name: "SettlementState".into(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: [
                    ("settlement", AstType::Text),
                    ("kind", AstType::Text),
                    ("state", AstType::Text),
                    ("at", AstType::Text),
                    ("from", AstType::Text),
                    ("to", AstType::Text),
                    ("instrument", AstType::Text),
                    ("amount", AstType::Decimal),
                ]
                .into_iter()
                .map(|(field, ty)| (field.into(), ty))
                .collect(),
                open_tail: None,
            }),
            span: Span::default(),
        }],
    };
    let package = PackageInput::new(manifest.clone(), [axiom_ledger::hir::lower(module)])
        .with_schema_capability(schema, SchemaCapability::SettlementStateV1);
    (package, manifest)
}

fn setup(source: &str) -> (Workspace, axiom_ledger::store::CommitId) {
    let (package, manifest) = settlement_package("payments");
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
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", source).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap();
    (workspace, pinned.commit)
}

#[test]
fn accepts_each_settlement_once_and_preserves_ineffective_history() {
    let source = "form first/1 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform first/2 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state presented\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform first/3 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state settled\n  at 2026-01-03\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform second/1 : payments::types::SettlementState\n  settlement q\n  kind card\n  state issued\n  at 2026-01-03\n  from alice\n  to merchant\n  instrument USD\n  amount 12\n\nform second/2 : payments::types::SettlementState\n  settlement q\n  kind card\n  state cancelled\n  at 2026-01-04\n  from alice\n  to merchant\n  instrument USD\n  amount 12\n";
    let (workspace, commit) = setup(source);
    let accepted = workspace.settlement_world(commit).unwrap();
    accepted.check(workspace.store()).unwrap();
    assert_eq!(accepted.histories().len(), 2);
    let observation = accepted
        .recognize(SettlementRecognitionPolicy::observation())
        .unwrap();
    assert!(matches!(
        &observation,
        SettlementRecognition::Observation(_)
    ));
    assert_eq!(observation.facts().len(), 2);
    assert_eq!(
        accepted
            .history("p")
            .unwrap()
            .settlement_date()
            .unwrap()
            .to_string(),
        "2026-01-03"
    );
    assert!(accepted.history("p").unwrap().effective());
    assert!(!accepted.history("q").unwrap().effective());
    assert_eq!(accepted.history("q").unwrap().settlement_date(), None);
    let policy = SettlementRecognitionPolicy::cash([
        (EntityId::new("alice"), AccountId::new("cash")),
        (EntityId::new("bank"), AccountId::new("bank")),
        (EntityId::new("merchant"), AccountId::new("merchant")),
    ])
    .unwrap();
    let cash = accepted.recognize(policy).unwrap();
    assert!(matches!(&cash, SettlementRecognition::Cash(_)));
    assert_eq!(cash.facts().len(), 1);
    assert_eq!(cash.facts()[0].date().to_string(), "2026-01-03");
    assert_eq!(cash.journal().unwrap().entries().len(), 1);
}

#[test]
fn correction_produces_a_new_world_without_invalidating_old_one() {
    let first = "form one/1 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform one/2 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state presented\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform one/3 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state settled\n  at 2026-01-03\n  from alice\n  to bank\n  instrument USD\n  amount 100\n";
    let (mut workspace, commit) = setup(first);
    let old = workspace.settlement_world(commit).unwrap();
    let corrected = workspace
        .correct_source(
            commit,
            first.replace("at 2026-01-03\n  from", "at 2026-01-04\n  from"),
        )
        .unwrap();
    let new = workspace.settlement_world(corrected.commit).unwrap();
    old.check(workspace.store()).unwrap();
    new.check(workspace.store()).unwrap();
    assert_ne!(old.source_commit(), new.source_commit());
    assert_ne!(old.authority_hash(), new.authority_hash());
    assert_eq!(
        old.history("p")
            .unwrap()
            .settlement_date()
            .unwrap()
            .to_string(),
        "2026-01-03"
    );
    assert_eq!(
        new.history("p")
            .unwrap()
            .settlement_date()
            .unwrap()
            .to_string(),
        "2026-01-04"
    );
    let policy = SettlementRecognitionPolicy::cash([
        (EntityId::new("alice"), AccountId::new("cash")),
        (EntityId::new("bank"), AccountId::new("bank")),
    ])
    .unwrap();
    let period = SettlementReportingPeriod::new(
        "2026-01-01".parse().unwrap(),
        "2026-12-31".parse().unwrap(),
    );
    let close = old.close(policy.clone(), period).unwrap();
    close.check(workspace.store()).unwrap();
    let corrected_close = new.close(policy, period).unwrap();
    assert_ne!(close.recognized_root(), corrected_close.recognized_root());
}

#[test]
fn cash_requires_explicit_distinct_endpoint_accounts_and_period() {
    let source = "form one/1 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform one/2 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state presented\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform one/3 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state settled\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n";
    let (workspace, commit) = setup(source);
    let world = workspace.settlement_world(commit).unwrap();
    let missing =
        SettlementRecognitionPolicy::cash([(EntityId::new("alice"), AccountId::new("cash"))])
            .unwrap();
    assert!(matches!(
        world.recognize(missing),
        Err(axiom_ledger::settlement_books::SettlementBookError::MissingAccount { .. })
    ));
    let same = SettlementRecognitionPolicy::cash([
        (EntityId::new("alice"), AccountId::new("cash")),
        (EntityId::new("bank"), AccountId::new("cash")),
    ])
    .unwrap();
    assert!(matches!(
        world.recognize(same),
        Err(axiom_ledger::settlement_books::SettlementBookError::SameAccount { .. })
    ));
    let policy = SettlementRecognitionPolicy::cash([
        (EntityId::new("alice"), AccountId::new("cash")),
        (EntityId::new("bank"), AccountId::new("bank")),
    ])
    .unwrap();
    let close = world
        .close(
            policy,
            SettlementReportingPeriod::new(
                "2026-01-01".parse().unwrap(),
                "2026-01-01".parse().unwrap(),
            ),
        )
        .unwrap();
    assert!(close.recognition().facts().is_empty());
}

#[test]
fn same_settlement_id_across_package_schema_identities_is_rejected() {
    let (payments, payments_manifest) = settlement_package("payments");
    let (other, other_manifest) = settlement_package("other");
    let lockfile = Lockfile {
        roots: vec![
            Dependency::new(
                payments_manifest.name.clone(),
                VersionReq::Exact(payments_manifest.version),
            ),
            Dependency::new(
                other_manifest.name.clone(),
                VersionReq::Exact(other_manifest.version),
            ),
        ],
        packages: vec![
            LockedPackage {
                name: payments_manifest.name.clone(),
                version: payments_manifest.version,
                hash: payments_manifest.hash(),
                dependencies: Vec::new(),
            },
            LockedPackage {
                name: other_manifest.name.clone(),
                version: other_manifest.version,
                hash: other_manifest.hash(),
                dependencies: Vec::new(),
            },
        ],
    };
    let mut workspace = Workspace::new();
    let source = workspace
        .load_source(
            "ledger",
            "form one/1 : payments::types::SettlementState\n  settlement p\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform two/1 : other::types::SettlementState\n  settlement p\n  kind ach\n  state presented\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n",
        )
        .unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([payments, other], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap();
    assert!(matches!(
        workspace.settlement_world(pinned.commit),
        Err(axiom_ledger::workspace::WorkspaceError::SettlementBook(
            axiom_ledger::settlement_books::SettlementBookError::ConflictingSettlementIdentity { .. }
        ))
    ));
}

#[test]
fn close_projects_only_facts_in_the_requested_period() {
    let source = "form january/1 : payments::types::SettlementState\n  settlement jan\n  kind ach\n  state issued\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform january/2 : payments::types::SettlementState\n  settlement jan\n  kind ach\n  state presented\n  at 2026-01-01\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform january/3 : payments::types::SettlementState\n  settlement jan\n  kind ach\n  state settled\n  at 2026-01-02\n  from alice\n  to bank\n  instrument USD\n  amount 100\n\nform february/1 : payments::types::SettlementState\n  settlement feb\n  kind ach\n  state issued\n  at 2026-02-01\n  from alice\n  to bank\n  instrument USD\n  amount 200\n\nform february/2 : payments::types::SettlementState\n  settlement feb\n  kind ach\n  state presented\n  at 2026-02-01\n  from alice\n  to bank\n  instrument USD\n  amount 200\n\nform february/3 : payments::types::SettlementState\n  settlement feb\n  kind ach\n  state settled\n  at 2026-02-02\n  from alice\n  to bank\n  instrument USD\n  amount 200\n";
    let (workspace, commit) = setup(source);
    let world = workspace.settlement_world(commit).unwrap();
    let policy = SettlementRecognitionPolicy::cash([
        (EntityId::new("alice"), AccountId::new("cash")),
        (EntityId::new("bank"), AccountId::new("bank")),
    ])
    .unwrap();
    let direct = world.recognize(policy.clone()).unwrap();
    assert_eq!(direct.facts().len(), 2);
    let january = world
        .close(
            policy,
            SettlementReportingPeriod::new(
                "2026-01-01".parse().unwrap(),
                "2026-01-31".parse().unwrap(),
            ),
        )
        .unwrap();
    assert_eq!(january.recognition().facts().len(), 1);
    assert_eq!(january.recognition().facts()[0].settlement(), "jan");
    january.check(workspace.store()).unwrap();
}
