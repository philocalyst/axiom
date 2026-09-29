use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, Span,
};
use axiom_ledger::ir::{Symbol, Term};
use axiom_ledger::package_compiler::PackageInput;
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::store::Period;
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const SOURCE: &str =
    "form invoice/1 : billing::types::Invoice\n  approved true\n  count 7\n  note paid\n";

const MIXED_SOURCE: &str = r#"book brokerage
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
form invoice/1 : billing::types::Invoice
  approved true
  count 7
  note paid
sell sell/one on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
"#;

fn package(name: &str, record: &str) -> (PackageInput, Lockfile) {
    package_version(name, record, Version::new(1, 0, 0))
}

fn package_version(name: &str, record: &str, version: Version) -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new(name, version, "record-package");
    let module = axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("types").unwrap()),
        declarations: vec![AstDeclaration {
            name: record.to_owned(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: vec![
                    ("approved".to_owned(), AstType::Bool),
                    ("count".to_owned(), AstType::Integer),
                    ("note".to_owned(), AstType::Text),
                ],
                open_tail: None,
            }),
            span: Span::default(),
        }],
    });
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
    (PackageInput::new(manifest, [module]), lockfile)
}

#[test]
fn package_forms_bind_exact_source_commit_and_pinned_artifact() {
    let (package, lockfile) = package("billing", "Invoice");
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (artifact_id, artifact) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact_id)
        .unwrap();

    let forms = workspace.elaborate_package_forms(pinned.commit).unwrap();
    assert_eq!(forms.source_commit(), pinned.commit);
    assert_eq!(forms.compiled_artifact(), artifact_id);
    assert_eq!(forms.artifact_hash(), artifact.artifact_hash());
    assert_eq!(forms.forms().len(), 1);
    assert_eq!(forms.forms()[0].package_root(), artifact.package_roots()[0]);
    assert_eq!(
        forms.forms()[0].value().record().field("count"),
        Some(&Term::Integer(7.into()))
    );
    assert_eq!(
        forms.forms()[0].value().record().field("approved"),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        forms.forms()[0]
            .value()
            .record()
            .field(Symbol::from("note")),
        Some(&Term::Text("paid".to_owned()))
    );
}

#[test]
fn package_forms_reload_historical_bytes_and_never_use_ambient_policy_registry() {
    let (package, lockfile) = package("billing", "Invoice");
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (artifact_id, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact_id)
        .unwrap();

    // A later correction changes the source head, but does not rewrite the
    // historical source bytes used by the first pinned commit.
    let corrected = workspace
        .correct_source(pinned.commit, SOURCE.replace("count 7", "count 8"))
        .unwrap();
    let historical = workspace.elaborate_package_forms(pinned.commit).unwrap();
    let current = workspace.elaborate_package_forms(corrected.commit).unwrap();
    assert_ne!(pinned.commit, corrected.commit);
    assert_ne!(pinned.content(), corrected.content());
    assert_ne!(pinned.bytes(), corrected.bytes());
    assert_eq!(historical.compiled_artifact(), artifact_id);
    assert_eq!(current.compiled_artifact(), artifact_id);
    assert_eq!(
        historical.forms()[0].schema().schema_id(),
        current.forms()[0].schema().schema_id()
    );
    assert_ne!(
        historical.forms()[0].value().content_hash(),
        current.forms()[0].value().content_hash()
    );
    assert_eq!(
        historical.forms()[0].value().record().field("count"),
        Some(&Term::Integer(7.into()))
    );
    assert_eq!(
        current.forms()[0].value().record().field("count"),
        Some(&Term::Integer(8.into()))
    );
    assert_ne!(historical.source_commit(), current.source_commit());

    let historical_again = workspace.elaborate_package_forms(pinned.commit).unwrap();
    assert_eq!(historical, historical_again);

    // Legacy policy packages are not an ambient fallback for package forms.
    let no_artifact = workspace.load_source("unbound", SOURCE).unwrap();
    workspace
        .put_policy_package(axiom_ledger::store::PolicyPackage::new(
            "billing",
            "1",
            b"not a compiler artifact".to_vec(),
        ))
        .unwrap();
    assert!(matches!(
        workspace.elaborate_package_forms(no_artifact.commit),
        Err(WorkspaceError::MissingCompiledArtifact { commit }) if commit == no_artifact.commit
    ));
}

#[test]
fn package_forms_reject_an_artifact_without_the_named_schema() {
    let (package, lockfile) = package("other", "Other");
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (artifact_id, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact_id)
        .unwrap();

    let error = workspace
        .elaborate_package_forms(pinned.commit)
        .unwrap_err();
    assert!(matches!(error, WorkspaceError::PackageFormElaboration(_)));
}

#[test]
fn package_forms_reject_a_source_commit_without_a_pinned_artifact() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    assert!(matches!(
        workspace.elaborate_package_forms(source.commit),
        Err(WorkspaceError::MissingCompiledArtifact { commit }) if commit == source.commit
    ));
}

#[test]
fn analysis_and_close_accept_artifact_bound_forms_between_builtin_blocks() {
    let (package, lockfile) = package("billing", "Invoice");
    let mut workspace = Workspace::new();
    let source = workspace.load_source("mixed-ledger", MIXED_SOURCE).unwrap();
    let (artifact_id, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact_id)
        .unwrap();

    // Public strict parsing remains unchanged. Only the workspace path that
    // first binds the form to this commit's artifact accepts the mixed file.
    assert!(axiom_ledger::parser::parse_ledger(MIXED_SOURCE).is_err());
    let analysis = workspace.analyze_commit(pinned.commit).unwrap();
    assert_eq!(analysis.ledger().forms.len(), 2);
    assert!(matches!(
        analysis.ledger().forms[0],
        axiom_ledger::model::LedgerForm::Buy(_)
    ));
    assert!(matches!(
        analysis.ledger().forms[1],
        axiom_ledger::model::LedgerForm::Sell(_)
    ));
    let package_forms = workspace.elaborate_package_forms(pinned.commit).unwrap();
    assert_eq!(package_forms.forms().len(), 1);
    assert_eq!(
        package_forms.forms()[0].value().record().field("count"),
        Some(&Term::Integer(7.into()))
    );

    let period = Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();
    assert!(workspace.close_sale_ledger(pinned.commit, period).is_ok());
}

#[test]
fn artifact_repin_invalidates_mixed_surface_elaboration() {
    let (package_a, lockfile_a) = package_version("billing", "Invoice", Version::new(1, 0, 0));
    let (package_b, lockfile_b) = package_version("billing", "Invoice", Version::new(2, 0, 0));
    let mut workspace = Workspace::new();
    let source = workspace
        .load_source("repinned-ledger", MIXED_SOURCE)
        .unwrap();
    let (artifact_a, _) = workspace
        .compile_packages_persisted([package_a], &lockfile_a)
        .unwrap();
    let pinned_a = workspace
        .commit_with_compiled_artifact(source.commit, artifact_a)
        .unwrap();
    workspace.analyze_commit(pinned_a.commit).unwrap();

    let (artifact_b, _) = workspace
        .compile_packages_persisted([package_b], &lockfile_b)
        .unwrap();
    let pinned_b = workspace
        .commit_with_compiled_artifact(pinned_a.commit, artifact_b)
        .unwrap();
    assert_eq!(pinned_a.bytes(), pinned_b.bytes());
    assert_ne!(pinned_a.commit, pinned_b.commit);

    workspace.clear_incremental_trace();
    workspace.analyze_commit(pinned_b.commit).unwrap();
    assert!(workspace.incremental_trace().iter().any(|event| matches!(
        event,
        axiom_ledger::incremental::TraceEvent::Invalidated { query, .. }
            if query.as_str() == "workspace/elaborate/repinned-ledger"
    )));
}

#[test]
fn invalid_package_forms_block_analysis_and_close_without_persisting() {
    let (package, lockfile) = package("billing", "Invoice");
    let mut workspace = Workspace::new();
    let (artifact_id, _) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();

    let unpinned = workspace
        .load_source("unpinned-form", MIXED_SOURCE)
        .unwrap();
    let before_unpinned = workspace.store().len();
    assert!(matches!(
        workspace.analyze_commit(unpinned.commit),
        Err(WorkspaceError::MissingCompiledArtifact { commit }) if commit == unpinned.commit
    ));
    let period = Period::new("2026-01-01".parse().unwrap(), "2026-12-31".parse().unwrap()).unwrap();
    assert!(matches!(
        workspace.close_sale_ledger(unpinned.commit, period.clone()),
        Err(WorkspaceError::MissingCompiledArtifact { commit }) if commit == unpinned.commit
    ));
    assert_eq!(workspace.store().len(), before_unpinned);

    let unknown = workspace
        .load_source(
            "unknown-form-package",
            MIXED_SOURCE.replace("billing::types::Invoice", "elsewhere::types::Invoice"),
        )
        .unwrap();
    let unknown = workspace
        .commit_with_compiled_artifact(unknown.commit, artifact_id)
        .unwrap();
    let before_unknown = workspace.store().len();
    assert!(matches!(
        workspace.analyze_commit(unknown.commit),
        Err(WorkspaceError::PackageFormElaboration(_))
    ));
    assert!(matches!(
        workspace.close_sale_ledger(unknown.commit, period.clone()),
        Err(WorkspaceError::PackageFormElaboration(_))
    ));
    assert_eq!(workspace.store().len(), before_unknown);

    let malformed = workspace
        .load_source(
            "malformed-form",
            MIXED_SOURCE.replace("  note paid", "  note paid extra"),
        )
        .unwrap();
    let malformed = workspace
        .commit_with_compiled_artifact(malformed.commit, artifact_id)
        .unwrap();
    let before_malformed = workspace.store().len();
    assert!(matches!(
        workspace.analyze_commit(malformed.commit),
        Err(WorkspaceError::PackageFormElaboration(_))
    ));
    assert!(matches!(
        workspace.close_sale_ledger(malformed.commit, period),
        Err(WorkspaceError::PackageFormElaboration(_))
    ));
    assert_eq!(workspace.store().len(), before_malformed);
}
