use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, Span,
};
use axiom_ledger::ir::{Symbol, Term};
use axiom_ledger::package_compiler::PackageInput;
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const SOURCE: &str =
    "form invoice/1 : billing::types::Invoice\n  approved true\n  count 7\n  note paid\n";

fn package(name: &str, record: &str) -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new(name, Version::new(1, 0, 0), "record-package");
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
