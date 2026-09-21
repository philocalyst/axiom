use std::collections::BTreeMap;

use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName, Span,
    lower,
};
use axiom_ledger::lsp::{AuthoringSurface, BindingError};
use axiom_ledger::proof::{Node, Operation, Proof};
use axiom_ledger::workspace::Workspace;

fn module(span: Span) -> axiom_ledger::hir::Module {
    lower(AstModule {
        path: ModulePath::root(Name::new("demo").unwrap()),
        declarations: vec![AstDeclaration {
            name: "cash".into(),
            kind: AstDeclarationKind::Value {
                ty: AstType::Decimal,
                expression: None,
            },
            span,
        }],
    })
}

#[test]
fn workspace_source_hir_and_proof_share_one_checked_editor_snapshot() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("demo.axm", "cash\n").unwrap();

    let mut proof = Proof::new();
    let root = proof.insert(Node::new(
        "source",
        Operation::Observation {
            source: "demo.axm".into(),
        },
        vec![],
        BTreeMap::new(),
    ));
    proof.root(root);

    let hir = module(Span::new(0, 4));
    let document = AuthoringSurface::checked(source.clone(), hir.clone(), Some(proof)).unwrap();
    assert_eq!(document.source_commit(), source.commit_id());
    assert_eq!(document.source_content(), source.content());
    assert_eq!(document.module().content_id(), hir.content_id());

    let qualified = QualifiedName {
        module: ModulePath::root(Name::new("demo").unwrap()),
        name: Name::new("cash").unwrap(),
    };
    assert_eq!(document.definition(&qualified), Some(Span::new(0, 4)));
    assert_eq!(
        document
            .completions("cash")
            .into_iter()
            .map(|item| item.label)
            .collect::<Vec<_>>(),
        vec!["cash"]
    );
    assert!(document.diagnostics().is_empty());

    let navigator = document.proof_navigator().unwrap().unwrap();
    assert_eq!(navigator.roots(), &[root]);
    assert_eq!(navigator.dependencies(root), Some([].as_slice()));
}

#[test]
fn binding_rejects_hir_ranges_that_cannot_name_the_source() {
    let mut workspace = Workspace::new();
    let source = workspace.load_source("demo.axm", "book demo\n").unwrap();
    let mismatch =
        AuthoringSurface::checked(source.clone(), module(Span::new(0, 4)), None).unwrap_err();
    assert!(matches!(
        mismatch,
        BindingError::SourceSpellingMismatch { .. }
    ));
    let error = AuthoringSurface::checked(source, module(Span::new(0, 100)), None).unwrap_err();
    assert!(matches!(
        error,
        BindingError::SpanOutOfBounds {
            layer: "HIR declaration",
            ..
        }
    ));
}
