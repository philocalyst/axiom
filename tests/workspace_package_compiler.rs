use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, DeclarationKind, Expression, Module,
    ModulePath, Name, Span, Type,
};
use axiom_ledger::incremental::TraceEvent;
use axiom_ledger::package_compiler::PackageInput;
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

fn manifest(body: &str) -> PackageManifest {
    PackageManifest::new("example", Version::new(1, 0, 0), body)
}

#[test]
fn mutable_hir_cannot_replay_its_old_compiled_artifact() {
    let package = manifest("hir-package");
    let lockfile = lockfile(&package);
    let mut input = PackageInput::new(package, [module(1)]);
    let cached_lowering_id = input.modules[0].content_id();
    let mut workspace = Workspace::new();
    let first = workspace
        .compile_packages([input.clone()], &lockfile)
        .unwrap();

    input.modules[0].declarations[0].kind = DeclarationKind::Value {
        ty: Type::Integer,
        expression: Some(Expression::Integer(2)),
    };
    assert_eq!(input.modules[0].content_id(), cached_lowering_id);
    let changed = workspace.compile_packages([input], &lockfile).unwrap();
    assert_ne!(first, changed);
}

fn module(value: i128) -> Module {
    axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("root").unwrap()),
        declarations: vec![AstDeclaration {
            name: "answer".to_string(),
            kind: AstDeclarationKind::Value {
                ty: AstType::Integer,
                expression: Some(axiom_ledger::hir::AstExpression::Integer(value)),
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
fn package_compilation_is_cached_and_module_changes_invalidate_it() {
    let package = manifest("hir-package");
    let lockfile = lockfile(&package);
    let input = PackageInput::new(package.clone(), [module(1)]);
    let mut workspace = Workspace::new();

    let first = workspace
        .compile_packages([input.clone()], &lockfile)
        .unwrap();
    workspace.clear_incremental_trace();
    let replay = workspace.compile_packages([input], &lockfile).unwrap();
    assert_eq!(first, replay);
    assert!(workspace.incremental_trace().iter().any(|event| {
        matches!(event, TraceEvent::CacheHit { query, .. } if query.as_str() == "workspace/package-compile")
    }));

    workspace.clear_incremental_trace();
    let changed = workspace
        .compile_packages([PackageInput::new(package, [module(2)])], &lockfile)
        .unwrap();
    assert_ne!(first, changed);
    assert!(workspace.incremental_trace().iter().any(|event| {
        matches!(event, TraceEvent::Invalidated { query, .. } if query.as_str() == "workspace/package-compile")
    }));
}

#[test]
fn package_compile_errors_cross_the_workspace_boundary() {
    let package = manifest("hir-package");
    let lockfile = lockfile(&package);
    let broken = axiom_ledger::hir::lower(AstModule {
        path: ModulePath::root(Name::new("root").unwrap()),
        declarations: vec![AstDeclaration {
            name: "answer".to_string(),
            kind: AstDeclarationKind::Value {
                ty: AstType::Text,
                expression: Some(axiom_ledger::hir::AstExpression::Integer(1)),
            },
            span: Span::default(),
        }],
    });
    let mut workspace = Workspace::new();
    assert!(matches!(
        workspace.compile_packages([PackageInput::new(package, [broken])], &lockfile),
        Err(WorkspaceError::PackageCompile(_))
    ));
}
