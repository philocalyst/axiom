use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstExpression, AstModule, Module, ModulePath, Name, Span,
};
use axiom_ledger::model::ContentHash;
use axiom_ledger::package_compiler::PackageInput;
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::workspace::Workspace;

fn manifest() -> PackageManifest {
    PackageManifest::new("example", Version::new(1, 0, 0), "hir-package")
}

fn module(value: i128) -> Module {
    axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("root").unwrap()),
        declarations: vec![AstDeclaration {
            name: "answer".to_owned(),
            kind: AstDeclarationKind::Value {
                ty: axiom_ledger::hir::AstType::Integer,
                expression: Some(AstExpression::Integer(value)),
            },
            span: Span::default(),
        }],
    })
}

fn lockfile(package: &PackageManifest) -> Lockfile {
    Lockfile {
        roots: vec![Dependency::new(
            package.name.clone(),
            VersionReq::Exact(package.version),
        )],
        packages: vec![LockedPackage {
            name: package.name.clone(),
            version: package.version,
            hash: package.hash(),
            dependencies: package.dependencies.clone(),
        }],
    }
}

#[test]
fn workspace_persists_verified_artifact_and_input_roots() {
    let package = manifest();
    let lockfile = lockfile(&package);
    let mut workspace = Workspace::new();
    let (artifact_id, artifact) = workspace
        .compile_packages_persisted([PackageInput::new(package.clone(), [module(1)])], &lockfile)
        .unwrap();
    let stored = workspace.compiled_artifact(artifact_id).unwrap();

    assert_eq!(stored.artifact, artifact);
    assert_eq!(stored.input_roots(), artifact.package_roots());
    assert_eq!(
        stored.artifact.artifact_hash(),
        stored.artifact.recomputed_hash()
    );
    assert_eq!(
        workspace
            .store()
            .get(artifact_id.hash())
            .unwrap()
            .content_hash(),
        artifact_id.hash()
    );
    workspace.store().verify().unwrap();

    let independent =
        ContentHash::domain_separated("axiom/package-artifact/v1", &artifact.canonical_bytes());
    assert_eq!(independent, artifact.artifact_hash());
}

#[test]
fn equivalent_inputs_are_idempotent_but_artifact_changes_are_addressed() {
    let package = manifest();
    let lockfile = lockfile(&package);
    let mut workspace = Workspace::new();
    let first = workspace
        .compile_packages_persisted([PackageInput::new(package.clone(), [module(1)])], &lockfile)
        .unwrap();
    let same = workspace
        .compile_packages_persisted([PackageInput::new(package.clone(), [module(1)])], &lockfile)
        .unwrap();
    assert_eq!(first, same);

    let changed = workspace
        .compile_packages_persisted([PackageInput::new(package, [module(2)])], &lockfile)
        .unwrap();
    assert_ne!(first.0, changed.0);
    assert_ne!(first.1.artifact_hash(), changed.1.artifact_hash());
}
