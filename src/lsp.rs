//! Transport-independent editor services over checked compiler artifacts.
//!
//! This is deliberately not an LSP server. It is the small deterministic
//! index a JSON-RPC adapter can expose once surface-to-HIR lowering exists.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::hir::{
    Declaration, DeclarationKind, DiagnosticCode, Module, QualifiedName, Severity, Span, Type,
};
use crate::model::ContentHash;
use crate::proof::{CheckError, Node, Proof, ProofId};
use crate::store::CommitId;
use crate::surface;
use crate::workspace::{CommitAnalysis, SourceLedger, Workspace, WorkspaceError};

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

/// Which checked authoring layer produced an editor diagnostic.
///
/// Surface diagnostics deliberately do not pretend to have a HIR diagnostic
/// code.  Keeping their origin explicit prevents a JSON-RPC adapter from
/// accidentally presenting recoverable CST errors as type errors.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticOrigin {
    Surface,
    Hir,
}

/// A diagnostic from the complete source/HIR authoring surface.
///
/// HIR diagnostics retain their typed [`DiagnosticCode`].  Surface
/// diagnostics have no HIR code yet, so `code` is `None`; both kinds use the
/// same byte-offset span and deterministic ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoringDiagnostic {
    pub span: Span,
    pub severity: Severity,
    pub origin: DiagnosticOrigin,
    pub code: Option<DiagnosticCode>,
    pub message: String,
}

impl AuthoringDiagnostic {
    pub fn is_surface(&self) -> bool {
        self.origin == DiagnosticOrigin::Surface
    }

    pub fn is_hir(&self) -> bool {
        self.origin == DiagnosticOrigin::Hir
    }
}

/// Failure while binding source, typed HIR, and a checked proof into one
/// editor document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingError {
    /// A HIR or diagnostic span points outside the exact UTF-8 source bytes.
    SpanOutOfBounds {
        layer: &'static str,
        span: Span,
        source_len: usize,
    },
    /// A byte offset is inside a UTF-8 code point and therefore cannot be an
    /// editor range boundary.
    InvalidUtf8Boundary {
        layer: &'static str,
        span: Span,
    },
    /// A declaration's source range does not contain its declared spelling.
    SourceSpellingMismatch {
        name: String,
        span: Span,
    },
    /// The proof was not independently valid at the point it entered the
    /// authoring surface.
    InvalidProof(CheckError),
    /// The proof came from a workspace analysis for another source commit.
    SourceCommitMismatch {
        source: CommitId,
        analysis: CommitId,
    },
    Workspace(WorkspaceError),
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpanOutOfBounds {
                layer,
                span,
                source_len,
            } => write!(
                formatter,
                "{layer} span {}..{} is outside source length {source_len}",
                span.start, span.end
            ),
            Self::InvalidUtf8Boundary { layer, span } => write!(
                formatter,
                "{layer} span {}..{} is not aligned to UTF-8 boundaries",
                span.start, span.end
            ),
            Self::SourceSpellingMismatch { name, span } => write!(
                formatter,
                "HIR declaration `{name}` is not present at source span {}..{}",
                span.start, span.end
            ),
            Self::InvalidProof(error) => write!(formatter, "invalid proof: {error}"),
            Self::SourceCommitMismatch { source, analysis } => write!(
                formatter,
                "source commit {source} does not match analysis commit {analysis}"
            ),
            Self::Workspace(error) => write!(formatter, "workspace binding failed: {error}"),
        }
    }
}

impl std::error::Error for BindingError {}

impl From<WorkspaceError> for BindingError {
    fn from(value: WorkspaceError) -> Self {
        Self::Workspace(value)
    }
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

/// One immutable, checked document exposed to an editor adapter.
///
/// The source bytes and tolerant CST come from [`SourceLedger`].  The typed
/// module is retained exactly as supplied by the HIR lowering boundary.  An
/// optional proof is accepted only after independent proof checking; when it
/// came from [`CommitAnalysis`], the source commit binding is checked too.
/// All editor queries are consequently answered from one source/HIR/proof
/// snapshot rather than from independently refreshed pieces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoringSurface {
    source: SourceLedger,
    index: DocumentIndex,
    proof: Option<Proof>,
}

impl AuthoringSurface {
    /// Bind a source ledger, lowered HIR module, and optional checked proof.
    pub fn checked(
        source: SourceLedger,
        module: Module,
        proof: Option<Proof>,
    ) -> Result<Self, BindingError> {
        validate_module_spans(&source, &module)?;
        if let Some(proof) = &proof {
            proof.check().map_err(BindingError::InvalidProof)?;
        }
        let index = DocumentIndex::new(module.clone());
        Ok(Self {
            source,
            index,
            proof,
        })
    }

    /// Bind a workspace analysis to its exact source ledger and HIR module.
    /// The analysis checker verifies the commit observation and the proof DAG
    /// before this document is made visible to editor queries.
    pub fn from_analysis(
        source: SourceLedger,
        module: Module,
        analysis: &CommitAnalysis,
    ) -> Result<Self, BindingError> {
        if source.commit_id() != analysis.source_commit() {
            return Err(BindingError::SourceCommitMismatch {
                source: source.commit_id(),
                analysis: analysis.source_commit(),
            });
        }
        analysis.check_proof().map_err(BindingError::InvalidProof)?;
        Self::checked(source, module, Some(analysis.proof().clone()))
    }

    /// Analyze a source commit through the canonical workspace boundary and
    /// return the resulting checked authoring document.
    pub fn from_workspace(
        workspace: &mut Workspace,
        source: SourceLedger,
        module: Module,
    ) -> Result<Self, BindingError> {
        let analysis = workspace.analyze_commit(source.commit_id())?;
        Self::from_analysis(source, module, &analysis)
    }

    pub fn source(&self) -> &SourceLedger {
        &self.source
    }

    pub fn source_commit(&self) -> CommitId {
        self.source.commit_id()
    }

    pub fn source_content(&self) -> ContentHash {
        self.source.content()
    }

    pub fn module(&self) -> &Module {
        self.index.module()
    }

    pub fn index(&self) -> &DocumentIndex {
        &self.index
    }

    pub fn proof(&self) -> Option<&Proof> {
        self.proof.as_ref()
    }

    /// Recreate a borrowing navigator over the proof retained by this
    /// document.  Rechecking is cheap and keeps this API safe if the
    /// implementation later changes how proofs are stored.
    pub fn proof_navigator(&self) -> Result<Option<ProofNavigator<'_>>, CheckError> {
        Ok(self.proof.as_ref().map(ProofNavigator::from_valid))
    }

    /// Unified diagnostics from both the recoverable source surface and HIR,
    /// sorted by source span and then by origin/code/message.
    pub fn diagnostics(&self) -> Vec<AuthoringDiagnostic> {
        let mut diagnostics = self
            .source
            .surface()
            .diagnostics()
            .iter()
            .map(|diagnostic| AuthoringDiagnostic {
                span: Span::new(diagnostic.span.start, diagnostic.span.end),
                severity: match diagnostic.severity {
                    surface::Severity::Warning => Severity::Warning,
                    surface::Severity::Error => Severity::Error,
                },
                origin: DiagnosticOrigin::Surface,
                code: None,
                message: diagnostic.message.clone(),
            })
            .chain(
                self.module()
                    .diagnostics
                    .iter()
                    .map(|diagnostic| AuthoringDiagnostic {
                        span: diagnostic.span,
                        severity: diagnostic.severity,
                        origin: DiagnosticOrigin::Hir,
                        code: Some(diagnostic.code),
                        message: diagnostic.message.clone(),
                    }),
            )
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            (
                left.span,
                left.severity,
                left.origin,
                left.code,
                &left.message,
            )
                .cmp(&(
                    right.span,
                    right.severity,
                    right.origin,
                    right.code,
                    &right.message,
                ))
        });
        diagnostics
    }

    pub fn completions(&self, prefix: &str) -> Vec<CompletionItem> {
        self.index.completions(prefix)
    }

    pub fn definition(&self, name: &QualifiedName) -> Option<Span> {
        self.index.definition(name)
    }
}

fn validate_module_spans(source: &SourceLedger, module: &Module) -> Result<(), BindingError> {
    let source_text = source.lossless_source();
    let source_len = source_text.len();
    for declaration in &module.declarations {
        validate_span("HIR declaration", declaration.span, source_text, source_len)?;
        let spelling = &source_text[declaration.span.start..declaration.span.end];
        if !spelling.contains(declaration.name.as_str()) {
            return Err(BindingError::SourceSpellingMismatch {
                name: declaration.name.as_str().to_owned(),
                span: declaration.span,
            });
        }
    }
    for hole in &module.holes {
        validate_span("HIR hole", hole.span, source_text, source_len)?;
    }
    for diagnostic in &module.diagnostics {
        validate_span("HIR diagnostic", diagnostic.span, source_text, source_len)?;
    }
    Ok(())
}

fn validate_span(
    layer: &'static str,
    span: Span,
    source: &str,
    source_len: usize,
) -> Result<(), BindingError> {
    if span.start > span.end || span.end > source_len {
        return Err(BindingError::SpanOutOfBounds {
            layer,
            span,
            source_len,
        });
    }
    if !source.is_char_boundary(span.start) || !source.is_char_boundary(span.end) {
        return Err(BindingError::InvalidUtf8Boundary { layer, span });
    }
    Ok(())
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
        Ok(Self::from_valid(proof))
    }

    fn from_valid(proof: &'a Proof) -> Self {
        let mut dependents = BTreeMap::<ProofId, BTreeSet<ProofId>>::new();
        for node in proof.nodes.values() {
            for input in &node.inputs {
                dependents.entry(*input).or_default().insert(node.id);
            }
        }
        Self {
            proof,
            dependents: dependents
                .into_iter()
                .map(|(id, values)| (id, values.into_iter().collect()))
                .collect(),
        }
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
