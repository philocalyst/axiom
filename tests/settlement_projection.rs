use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName, Span,
};
use axiom_ledger::model::{Date, SettlementKind};
use axiom_ledger::ontology::SettlementStateRecord;
use axiom_ledger::package_compiler::{PackageInput, SchemaCapability};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

fn qualified_state() -> QualifiedName {
    QualifiedName {
        module: ModulePath::new(vec![Name::new("types").unwrap()]).unwrap(),
        name: Name::new("SettlementState").unwrap(),
    }
}

fn package(capable: bool) -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new("payments", Version::new(1, 0, 0), "settlement package");
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
    let package = PackageInput::new(manifest.clone(), [axiom_ledger::hir::lower(module)]);
    let package = if capable {
        package.with_schema_capability(qualified_state(), SchemaCapability::SettlementStateV1)
    } else {
        package
    };
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

fn workspace_for(source: &str, capable: bool) -> (Workspace, axiom_ledger::store::CommitId) {
    let (package, lockfile) = package(capable);
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

fn issued_ach_source() -> String {
    r#"
form first/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state issued
  at 2026-01-01
  from alice
  to bank
  instrument USD
  amount 100
"#
    .to_owned()
}

#[test]
fn projects_capable_forms_in_source_order_with_exact_amounts() {
    let source = r#"
form first/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state issued
  at 2026-01-01
  from alice
  to bank
  instrument USD
  amount 100.25

form second/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state presented
  at 2026-01-02
  from alice
  to bank
  instrument USD
  amount 100.25

form other/1 : payments::types::SettlementState
  settlement other-payment
  kind check
  state issued
  at 2026-01-03
  from bob
  to bank
  instrument EUR
  amount 7
"#;
    let (workspace, commit) = workspace_for(source, true);

    let projection = workspace.project_settlement_states(commit).unwrap();
    assert_eq!(projection.source_commit(), commit);
    assert_ne!(
        projection.artifact_hash(),
        axiom_ledger::model::ContentHash::ZERO
    );
    assert_eq!(projection.records().len(), 3);
    assert_eq!(
        projection
            .records()
            .map(|record| record.occurrence.as_str())
            .collect::<Vec<_>>(),
        ["first/1", "second/1", "other/1"]
    );
    let first = projection.records().next().unwrap();
    assert_eq!(first.amount.canonical(), "401/4 USD");
    assert_eq!(first.kind(), Some(SettlementKind::Ach));
    assert_eq!(first.at, Some(Date::new(2026, 1, 1).unwrap()));

    let graph_records = projection
        .graph()
        .records_of::<SettlementStateRecord>()
        .map(|record| record.occurrence.as_str())
        .collect::<Vec<_>>();
    assert_eq!(graph_records, ["first/1", "second/1", "other/1"]);
}

#[test]
fn unmarked_schema_with_the_same_shape_has_no_projection_authority() {
    let source = r#"
form first/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state issued
  at 2026-01-01
  from alice
  to bank
  instrument USD
  amount 100
"#;
    let (workspace, commit) = workspace_for(source, false);
    let error = workspace.project_settlement_states(commit).unwrap_err();
    assert!(matches!(
        error,
        WorkspaceError::SettlementProjection(
            axiom_ledger::settlement_projection::SettlementProjectionError::NoCapableForms
        )
    ));
}

#[test]
fn invalid_history_fails_atomically() {
    let source = r#"
form first/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state issued
  at 2026-01-02
  from alice
  to bank
  instrument USD
  amount 100

form invalid/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state settled
  at 2026-01-01
  from alice
  to bank
  instrument USD
  amount 100
"#;
    let (workspace, commit) = workspace_for(source, true);
    let error = workspace.project_settlement_states(commit).unwrap_err();
    assert!(matches!(
        error,
        WorkspaceError::SettlementProjection(
            axiom_ledger::settlement_projection::SettlementProjectionError::Ontology(_)
        )
    ));
}

#[test]
fn holes_are_rejected_instead_of_becoming_authority() {
    let source = r#"
form first/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state issued
  at 2026-01-01
  from alice
  to bank
  instrument USD
  amount ?amount
"#;
    let (workspace, commit) = workspace_for(source, true);
    let error = workspace.project_settlement_states(commit).unwrap_err();
    assert!(matches!(
        error,
        WorkspaceError::SettlementProjection(
            axiom_ledger::settlement_projection::SettlementProjectionError::WrongType {
                field: "amount",
                actual: "hole",
                ..
            }
        )
    ));
}

#[test]
fn rail_specific_rules_are_stricter_than_the_generic_state_machine() {
    let source = format!(
        "{}\n{}",
        issued_ach_source(),
        r#"
form second/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state authorized
  at 2026-01-02
  from alice
  to bank
  instrument USD
  amount 100
"#
    );
    let (workspace, commit) = workspace_for(&source, true);
    assert!(matches!(
        workspace.project_settlement_states(commit),
        Err(WorkspaceError::SettlementProjection(
            axiom_ledger::settlement_projection::SettlementProjectionError::Ontology(_)
        ))
    ));
}

#[test]
fn one_inconsistent_fact_rejects_the_whole_history() {
    let source = format!(
        "{}\n{}",
        issued_ach_source(),
        r#"
form second/1 : payments::types::SettlementState
  settlement payment
  kind ach
  state presented
  at 2026-01-02
  from alice
  to bank
  instrument USD
  amount 101
"#
    );
    let (workspace, commit) = workspace_for(&source, true);
    assert!(matches!(
        workspace.project_settlement_states(commit),
        Err(WorkspaceError::SettlementProjection(
            axiom_ledger::settlement_projection::SettlementProjectionError::Ontology(_)
        ))
    ));
}

#[test]
fn invalid_scalar_values_never_cross_the_projection_boundary() {
    for source in [
        issued_ach_source().replace("kind ach", "kind wire"),
        issued_ach_source().replace("state issued", "state invented"),
        issued_ach_source().replace("at 2026-01-01", "at 2026-1-1"),
        issued_ach_source().replace("at 2026-01-01", "at 10000-01-01"),
        issued_ach_source().replace("amount 100", "amount 0"),
        issued_ach_source().replace("amount 100", "amount -1"),
    ] {
        let (workspace, commit) = workspace_for(&source, true);
        assert!(
            workspace.project_settlement_states(commit).is_err(),
            "{source}"
        );
    }
}
