//! Transport-independent editor services over checked compiler artifacts.
//!
//! This is deliberately not an LSP server. It is the small deterministic
//! index a JSON-RPC adapter can expose once surface-to-HIR lowering exists.

use std::collections::{BTreeMap, BTreeSet};

use crate::hir::{
    Declaration, DeclarationKind, DiagnosticCode, Module, QualifiedName, Severity, Span, Type,
};
use crate::proof::{CheckError, Node, Proof, ProofId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompletionKind {
    Keyword,
    Type,
    Value,
    Rule,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorDiagnostic {
    pub span: Span,
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
}

/// Immutable index for one lowered module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentIndex {
    module: Module,
    definitions: BTreeMap<String, Span>,
    completions: Vec<CompletionItem>,
}

impl DocumentIndex {
    pub fn new(module: Module) -> Self {
        let mut definitions = BTreeMap::new();
        let mut completions = keyword_completions();
        for declaration in &module.declarations {
            definitions.insert(declaration.name.as_str().to_owned(), declaration.span);
            completions.push(completion(declaration));
        }
        completions.sort();
        completions.dedup();
        Self {
            module,
            definitions,
            completions,
        }
    }

    pub fn module(&self) -> &Module {
        &self.module
    }

    pub fn diagnostics(&self) -> Vec<EditorDiagnostic> {
        self.module
            .diagnostics
            .iter()
            .map(|diagnostic| EditorDiagnostic {
                span: diagnostic.span,
                severity: diagnostic.severity,
                code: diagnostic.code,
                message: diagnostic.message.clone(),
            })
            .collect()
    }

    pub fn completions(&self, prefix: &str) -> Vec<CompletionItem> {
        self.completions
            .iter()
            .filter(|item| item.label.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub fn definition(&self, name: &QualifiedName) -> Option<Span> {
        (name.module == self.module.path)
            .then(|| self.definitions.get(name.name.as_str()).copied())
            .flatten()
    }
}

fn completion(declaration: &Declaration) -> CompletionItem {
    let (kind, detail) = match &declaration.kind {
        DeclarationKind::Type { ty } => (CompletionKind::Type, type_detail(ty)),
        DeclarationKind::Value { ty, .. } => (CompletionKind::Value, type_detail(ty)),
        DeclarationKind::Rule { input, output } => (
            CompletionKind::Rule,
            format!("{} -> {}", type_detail(input), type_detail(output)),
        ),
    };
    CompletionItem {
        label: declaration.name.as_str().to_owned(),
        kind,
        detail,
    }
}

fn keyword_completions() -> Vec<CompletionItem> {
    ["type", "value", "rule", "complete", "decide"]
        .into_iter()
        .map(|keyword| CompletionItem {
            label: keyword.to_owned(),
            kind: CompletionKind::Keyword,
            detail: "keyword".to_owned(),
        })
        .collect()
}

fn type_detail(ty: &Type) -> String {
    match ty {
        Type::Unit => "Unit".into(),
        Type::Bool => "Bool".into(),
        Type::Text => "Text".into(),
        Type::Integer => "Integer".into(),
        Type::Decimal => "Decimal".into(),
        Type::Named(name) => name.canonical(),
        Type::Variable(variable) => format!("?T{}", variable.0),
        Type::Hole(hole) => format!("?{}", hole.0),
        Type::Record(row) if row.is_open() => "open record".into(),
        Type::Record(_) => "record".into(),
        Type::Function { .. } => "function".into(),
        Type::Refined { .. } => "refined value".into(),
        Type::AtPhase { phase, .. } => format!("value at {}", phase.as_name()),
    }
}

/// Checked bidirectional navigation over a proof DAG.
#[derive(Clone, Debug)]
pub struct ProofNavigator<'a> {
    proof: &'a Proof,
    dependents: BTreeMap<ProofId, Vec<ProofId>>,
}

impl<'a> ProofNavigator<'a> {
    pub fn checked(proof: &'a Proof) -> Result<Self, CheckError> {
        proof.check()?;
        let mut dependents = BTreeMap::<ProofId, BTreeSet<ProofId>>::new();
        for node in proof.nodes.values() {
            for input in &node.inputs {
                dependents.entry(*input).or_default().insert(node.id);
            }
        }
        Ok(Self {
            proof,
            dependents: dependents
                .into_iter()
                .map(|(id, values)| (id, values.into_iter().collect()))
                .collect(),
        })
    }

    pub fn node(&self, id: ProofId) -> Option<&'a Node> {
        self.proof.node(id)
    }

    pub fn roots(&self) -> &[ProofId] {
        &self.proof.roots
    }

    pub fn dependencies(&self, id: ProofId) -> Option<&'a [ProofId]> {
        self.node(id).map(|node| node.inputs.as_slice())
    }

    pub fn dependents(&self, id: ProofId) -> &[ProofId] {
        self.dependents.get(&id).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{
        AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, lower,
    };
    use crate::proof::{Operation, Proof};

    fn module() -> Module {
        lower(AstModule {
            path: ModulePath::root(Name::new("book").unwrap()),
            declarations: vec![
                AstDeclaration {
                    name: "cash".into(),
                    kind: AstDeclarationKind::Value {
                        ty: AstType::Decimal,
                        expression: None,
                    },
                    span: Span::new(10, 14),
                },
                AstDeclaration {
                    name: "cash_type".into(),
                    kind: AstDeclarationKind::Type(AstType::Decimal),
                    span: Span::new(0, 9),
                },
            ],
        })
    }

    #[test]
    fn index_exposes_deterministic_definitions_completions_and_diagnostics() {
        let module = module();
        let qualified = QualifiedName {
            module: module.path.clone(),
            name: Name::new("cash").unwrap(),
        };
        let index = DocumentIndex::new(module);
        assert_eq!(index.definition(&qualified), Some(Span::new(10, 14)));
        assert_eq!(
            index
                .completions("cash")
                .into_iter()
                .map(|item| item.label)
                .collect::<Vec<_>>(),
            vec!["cash", "cash_type"]
        );
        assert!(index.diagnostics().is_empty());
    }

    #[test]
    fn navigator_rejects_bad_graphs_and_walks_both_directions() {
        let mut proof = Proof::new();
        let source = proof.insert(Node::new(
            "source",
            Operation::Observation {
                source: "ledger".into(),
            },
            vec![],
            BTreeMap::new(),
        ));
        let result = proof.insert(Node::new(
            "result",
            Operation::Derive {
                rule: "test/rule".into(),
            },
            vec![source],
            BTreeMap::new(),
        ));
        proof.root(result);
        let navigator = ProofNavigator::checked(&proof).unwrap();
        assert_eq!(navigator.dependencies(result), Some([source].as_slice()));
        assert_eq!(navigator.dependents(source), &[result]);

        let mut tampered = proof;
        tampered.nodes.get_mut(&result).unwrap().statement.0 = "forged".into();
        assert!(ProofNavigator::checked(&tampered).is_err());
    }
}
