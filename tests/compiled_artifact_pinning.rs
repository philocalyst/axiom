use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, Span,
};
use axiom_ledger::model::ContentHash;
use axiom_ledger::package_compiler::PackageInput;
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::store::{Commit, CompiledArtifactId, ObjectKind, ObjectStore, StoreError};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

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

fn package(value: i128) -> PackageInput {
    let manifest = PackageManifest::new("example", Version::new(1, 0, 0), "hir-package");
    let module = axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("root").unwrap()),
        declarations: vec![AstDeclaration {
            name: "answer".to_string(),
            kind: AstDeclarationKind::Value {
                ty: AstType::Integer,
                expression: Some(axiom_ledger::hir::AstExpression::Integer(value)),
            },
            span: Span::default(),
        }],
    });
    PackageInput::new(manifest, [module])
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
            dependencies: Vec::new(),
        }],
    }
}

#[test]
fn pin_changes_the_exact_source_and_analysis_inherits_it() {
    let input = package(1);
    let lock = lockfile(&input.manifest);
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([input], &lock)
        .unwrap();

    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .expect("persisted artifact pins to a source child");
    assert_ne!(pinned.commit, source.commit);
    assert_eq!(
        workspace
            .store()
            .commit(pinned.commit)
            .unwrap()
            .compiled_artifact,
        Some(artifact)
    );

    let analysis = workspace.analyze_commit(pinned.commit).unwrap();
    assert_eq!(
        workspace
            .store()
            .commit(analysis.analysis_commit())
            .unwrap()
            .compiled_artifact,
        Some(artifact)
    );

    let corrected = workspace
        .correct_source(pinned.commit, SOURCE.replace("200 USD", "210 USD"))
        .unwrap();
    assert_eq!(
        workspace
            .store()
            .commit(corrected.commit)
            .unwrap()
            .compiled_artifact,
        Some(artifact)
    );
}

#[test]
fn pin_replaces_stale_compiled_context_and_rejects_unusable_ids() {
    let first = package(1);
    let second = package(2);
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (first_id, _) = workspace
        .compile_packages_persisted([first.clone()], &lockfile(&first.manifest))
        .unwrap();
    let (second_id, _) = workspace
        .compile_packages_persisted([second.clone()], &lockfile(&second.manifest))
        .unwrap();
    assert_ne!(first_id, second_id);

    let first_source = workspace
        .commit_with_compiled_artifact(source.commit, first_id)
        .unwrap();
    let second_source = workspace
        .commit_with_compiled_artifact(first_source.commit, second_id)
        .unwrap();
    assert_eq!(
        workspace
            .store()
            .commit(second_source.commit)
            .unwrap()
            .compiled_artifact,
        Some(second_id)
    );

    let missing = CompiledArtifactId::new(ContentHash::domain_separated(
        "test/missing-compiled-artifact",
        b"missing",
    ));
    assert!(matches!(
        workspace.commit_with_compiled_artifact(source.commit, missing),
        Err(WorkspaceError::Store(StoreError::MissingObject(_)))
    ));

    let package_id = workspace
        .put_policy_package(axiom_ledger::store::PolicyPackage::new(
            "wrong-kind",
            "1",
            b"book tax\n".to_vec(),
        ))
        .unwrap();
    let wrong_kind = CompiledArtifactId::new(package_id.hash());
    assert!(matches!(
        workspace.commit_with_compiled_artifact(source.commit, wrong_kind),
        Err(WorkspaceError::Store(StoreError::WrongKind {
            expected: ObjectKind::CompiledArtifact,
            ..
        }))
    ));
}

#[test]
fn changing_legacy_policy_roots_clears_unrelated_compiler_context() {
    let input = package(1);
    let mut workspace = Workspace::new();
    let source = workspace.load_source("ledger", SOURCE).unwrap();
    let (artifact, _) = workspace
        .compile_packages_persisted([input.clone()], &lockfile(&input.manifest))
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source.commit, artifact)
        .unwrap();
    let policy = workspace
        .put_policy_package(axiom_ledger::store::PolicyPackage::new(
            "lots/custom",
            "1",
            b"selector=earliest_acquisition\ntie=ambiguous".to_vec(),
        ))
        .unwrap();

    let changed = workspace
        .commit_with_packages(pinned.commit, [policy])
        .unwrap();
    assert_eq!(
        workspace
            .store()
            .commit(changed.commit)
            .unwrap()
            .compiled_artifact,
        None
    );
}

#[test]
fn merge_rejects_divergent_compiled_package_worlds() {
    let first = package(1);
    let second = package(2);
    let first_artifact =
        axiom_ledger::package_compiler::compile([first.clone()], &lockfile(&first.manifest))
            .unwrap();
    let second_artifact =
        axiom_ledger::package_compiler::compile([second.clone()], &lockfile(&second.manifest))
            .unwrap();
    let mut store = ObjectStore::new();
    let first_id = store.put_compiled_artifact(first_artifact).unwrap();
    let second_id = store.put_compiled_artifact(second_artifact).unwrap();
    let base = store
        .put_commit(Commit::new([], [], [], [], [], [], [], "base"))
        .unwrap();
    let left = store
        .put_commit(
            Commit::new([base], [], [], [], [], [], [], "left").with_compiled_artifact(first_id),
        )
        .unwrap();
    let right = store
        .put_commit(
            Commit::new([base], [], [], [], [], [], [], "right").with_compiled_artifact(second_id),
        )
        .unwrap();

    assert!(matches!(
        store.merge_three_way(base, left, right, "merge"),
        Err(StoreError::InvalidObject(message))
            if message.contains("divergent compiled artifacts")
    ));
}
