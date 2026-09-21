//! A small, typed high-level intermediate representation.
//!
//! HIR is deliberately boring: it contains names, declarations and types,
//! but no evaluator, policy, solver, or economic interpretation.  This makes
//! it a useful boundary for future language work without claiming that the
//! current surface syntax already has a compiler.  Every value has a stable
//! canonical encoding, and lowering is a deterministic validation pass over a
//! small declaration AST.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use blake3::Hasher;

/// A source location used by HIR diagnostics.  It is intentionally independent
/// of the lossless surface parser so callers may lower generated declarations.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A validated identifier.  Names are compared by their UTF-8 bytes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Name(String);

impl Name {
    pub fn new(value: impl Into<String>) -> Result<Self, NameError> {
        let value = value.into();
        if value.is_empty() {
            return Err(NameError::Empty);
        }
        if !value.chars().enumerate().all(|(index, c)| {
            (index == 0 && (c == '_' || c.is_ascii_alphabetic()))
                || (index > 0 && (c == '_' || c.is_ascii_alphanumeric()))
        }) {
            return Err(NameError::Invalid(value));
        }
        Ok(Self(value))
    }

    /// Construct a name for trusted compiler-generated data.
    fn generated(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Name> for String {
    fn from(value: Name) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NameError {
    Empty,
    Invalid(String),
}

/// A module path.  Its canonical spelling is `segment::segment`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModulePath(Vec<Name>);

impl ModulePath {
    pub fn new(segments: impl IntoIterator<Item = Name>) -> Result<Self, NameError> {
        let segments: Vec<_> = segments.into_iter().collect();
        if segments.is_empty() {
            return Err(NameError::Empty);
        }
        Ok(Self(segments))
    }
    pub fn root(name: Name) -> Self {
        Self(vec![name])
    }
    pub fn segments(&self) -> &[Name] {
        &self.0
    }
    pub fn canonical(&self) -> String {
        self.0
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>()
            .join("::")
    }
}

/// A declaration name qualified by its module.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QualifiedName {
    pub module: ModulePath,
    pub name: Name,
}

impl QualifiedName {
    pub fn canonical(&self) -> String {
        format!("{}::{}", self.module.canonical(), self.name)
    }
}

/// A phase is an uninterpreted, ordered label.  Ordering is explicit in the
/// type, rather than smuggled in through an evaluator or a domain policy.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Phase(Name);

impl Phase {
    pub fn new(name: Name) -> Self {
        Self(name)
    }
    pub fn as_name(&self) -> &Name {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhaseTransition {
    pub from: Phase,
    pub to: Phase,
}

/// A type variable used by open rows and generic declarations.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeVar(pub u32);

/// A stable identity for a typed existential hole.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HoleId(pub u64);

/// A first-order refinement predicate.  It only describes syntax; its truth
/// is intentionally left to a later checker.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Predicate {
    Named(Name),
    Equals(String),
    NotEquals(String),
    InSet(Vec<String>),
}

/// A row is sorted and duplicate-free after lowering.  Open rows carry a
/// tail variable; closed rows do not silently accept unknown fields.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Row {
    Closed(Vec<Field>),
    Open { fields: Vec<Field>, tail: TypeVar },
}

impl Row {
    pub fn fields(&self) -> &[Field] {
        match self {
            Self::Closed(fields) | Self::Open { fields, .. } => fields,
        }
    }
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open { .. })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Field {
    pub name: Name,
    pub ty: Type,
}

/// The phase-aware, refinement-capable type language understood by this HIR.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Type {
    Unit,
    Bool,
    Text,
    Integer,
    Decimal,
    Named(QualifiedName),
    Variable(TypeVar),
    Hole(HoleId),
    Record(Row),
    Function {
        arguments: Vec<Type>,
        result: Box<Type>,
    },
    Refined {
        base: Box<Type>,
        predicate: Predicate,
    },
    AtPhase {
        phase: Phase,
        value: Box<Type>,
    },
}

impl Type {
    pub fn at_phase(self, phase: Phase) -> Self {
        Self::AtPhase {
            phase,
            value: Box::new(self),
        }
    }
    pub fn refined(self, predicate: Predicate) -> Self {
        Self::Refined {
            base: Box::new(self),
            predicate,
        }
    }

    /// Stable, length-delimited encoding of this type, suitable for cache
    /// keys and content-addressed identities.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_type_value(&mut out, self);
        out
    }

    pub fn canonical(&self) -> String {
        String::from_utf8(self.canonical_bytes()).expect("HIR canonical encoding is UTF-8")
    }
}

/// A typed declaration.  The optional value body is represented as a compact
/// declaration expression rather than pretending to parse a full language.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Declaration {
    pub name: Name,
    pub kind: DeclarationKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeclarationKind {
    Type {
        ty: Type,
    },
    Value {
        ty: Type,
        expression: Option<Expression>,
    },
    Rule {
        input: Type,
        output: Type,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Expression {
    Text(String),
    Integer(i128),
    Bool(bool),
    Hole(HoleId),
}

/// Expected constraints attached to a typed hole.  A hole may have multiple
/// expected types as it is encountered through aliases or generic positions.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HoleConstraint {
    pub id: HoleId,
    pub name: Option<Name>,
    pub generic: bool,
    pub expected: Vec<Type>,
    pub span: Span,
}

/// The result of lowering.  Diagnostics are retained even when declarations
/// are usable, making this safe for editor-style incremental clients.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Module {
    pub path: ModulePath,
    pub declarations: Vec<Declaration>,
    pub holes: Vec<HoleConstraint>,
    pub diagnostics: Vec<Diagnostic>,
    content_id: ContentId,
}

impl Module {
    pub fn content_id(&self) -> ContentId {
        self.content_id
    }
    /// Recompute identity from the module's current canonical contents.
    /// Compiler boundaries use this instead of trusting the cached lowering
    /// identity because editor clients can still mutate public HIR fields.
    pub fn recomputed_content_id(&self) -> ContentId {
        module_content_id(self)
    }
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_module(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentId([u8; 32]);

impl ContentId {
    pub fn bytes(self) -> [u8; 32] {
        self.0
    }
    pub fn hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    InvalidName,
    DuplicateDeclaration,
    UnknownType,
    DuplicateField,
    InvalidRow,
    InvalidPhase,
    ExpressionTypeMismatch,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    fn error(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            span,
            message: message.into(),
        }
    }
}

/// Small declaration AST accepted by this module.  It is intentionally not a
/// source parser: callers must construct names and type forms explicitly.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AstModule {
    pub path: ModulePath,
    pub declarations: Vec<AstDeclaration>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AstDeclaration {
    pub name: String,
    pub kind: AstDeclarationKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AstDeclarationKind {
    Type(AstType),
    Value {
        ty: AstType,
        expression: Option<AstExpression>,
    },
    Rule {
        input: AstType,
        output: AstType,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AstExpression {
    Text(String),
    Integer(i128),
    Bool(bool),
    Hole(AstHole),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AstHole {
    pub name: Option<String>,
    pub generic: bool,
    pub expected: Option<Box<AstType>>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AstType {
    Unit,
    Bool,
    Text,
    Integer,
    Decimal,
    Named(String),
    Variable(u32),
    Hole(AstHole),
    Record {
        fields: Vec<(String, AstType)>,
        open_tail: Option<u32>,
    },
    Function {
        arguments: Vec<AstType>,
        result: Box<AstType>,
    },
    Refined {
        base: Box<AstType>,
        predicate: Predicate,
    },
    AtPhase {
        phase: String,
        value: Box<AstType>,
    },
}

/// Lower a declaration AST deterministically.  Declarations are emitted in
/// canonical name order, while diagnostics are sorted by span and code.
pub fn lower(ast: AstModule) -> Module {
    let mut diagnostics = Vec::new();
    let module_path = ast.path.clone();
    let mut names = BTreeMap::<Name, Span>::new();
    let mut type_names = BTreeSet::<Name>::new();
    let mut parsed = Vec::new();
    for declaration in ast.declarations {
        let name = match Name::new(declaration.name.clone()) {
            Ok(name) => name,
            Err(_) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidName,
                    declaration.span,
                    format!("invalid declaration name `{}`", declaration.name),
                ));
                continue;
            }
        };
        if names.insert(name.clone(), declaration.span).is_some() {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::DuplicateDeclaration,
                declaration.span,
                format!("duplicate declaration `{name}`"),
            ));
        }
        if matches!(&declaration.kind, AstDeclarationKind::Type(_)) {
            type_names.insert(name.clone());
        }
        parsed.push((name, declaration));
    }
    let mut holes = BTreeMap::<HoleId, HoleConstraint>::new();
    let mut declarations = Vec::new();
    parsed.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, declaration) in parsed {
        let kind = match declaration.kind {
            AstDeclarationKind::Type(ty) => DeclarationKind::Type {
                ty: lower_type(
                    &module_path,
                    &name,
                    &ty,
                    &type_names,
                    &mut holes,
                    &mut diagnostics,
                    declaration.span,
                ),
            },
            AstDeclarationKind::Value { ty, expression } => {
                let ty = lower_type(
                    &module_path,
                    &name,
                    &ty,
                    &type_names,
                    &mut holes,
                    &mut diagnostics,
                    declaration.span,
                );
                let expression = expression.map(|expression| {
                    lower_expression(
                        &module_path,
                        (&name, declaration.span),
                        expression,
                        &ty,
                        &type_names,
                        &mut holes,
                        &mut diagnostics,
                    )
                });
                DeclarationKind::Value { ty, expression }
            }
            AstDeclarationKind::Rule { input, output } => DeclarationKind::Rule {
                input: lower_type(
                    &module_path,
                    &name,
                    &input,
                    &type_names,
                    &mut holes,
                    &mut diagnostics,
                    declaration.span,
                ),
                output: lower_type(
                    &module_path,
                    &name,
                    &output,
                    &type_names,
                    &mut holes,
                    &mut diagnostics,
                    declaration.span,
                ),
            },
        };
        declarations.push(Declaration {
            name,
            kind,
            span: declaration.span,
        });
    }
    let holes = holes
        .into_values()
        .map(|mut hole| {
            hole.expected.sort();
            hole.expected.dedup();
            hole
        })
        .collect();
    diagnostics.sort();
    let mut module = Module {
        path: module_path,
        declarations,
        holes,
        diagnostics,
        content_id: ContentId([0; 32]),
    };
    module.content_id = module_content_id(&module);
    module
}

fn module_content_id(module: &Module) -> ContentId {
    let mut hasher = Hasher::new();
    hasher.update(b"axiom/hir/module/v1\0");
    hasher.update(&canonical_module(module));
    ContentId(*hasher.finalize().as_bytes())
}

fn lower_expression(
    module: &ModulePath,
    site: (&Name, Span),
    expression: AstExpression,
    ty: &Type,
    known: &BTreeSet<Name>,
    holes: &mut BTreeMap<HoleId, HoleConstraint>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Expression {
    let (declaration, span) = site;
    match expression {
        AstExpression::Text(value) => {
            if !matches!(ty, Type::Text | Type::Hole(_)) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ExpressionTypeMismatch,
                    span,
                    format!("text value does not match `{ty:?}`"),
                ));
            }
            constrain_hole(ty, Type::Text, holes);
            Expression::Text(value)
        }
        AstExpression::Integer(value) => {
            if !matches!(ty, Type::Integer | Type::Decimal | Type::Hole(_)) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ExpressionTypeMismatch,
                    span,
                    format!("integer value does not match `{ty:?}`"),
                ));
            }
            constrain_hole(ty, Type::Integer, holes);
            Expression::Integer(value)
        }
        AstExpression::Bool(value) => {
            if !matches!(ty, Type::Bool | Type::Hole(_)) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ExpressionTypeMismatch,
                    span,
                    format!("boolean value does not match `{ty:?}`"),
                ));
            }
            constrain_hole(ty, Type::Bool, holes);
            Expression::Bool(value)
        }
        AstExpression::Hole(hole) => {
            let id = register_hole(module, declaration, &hole, known, holes, diagnostics);
            if !matches!(ty, Type::Hole(_))
                && let Some(constraint) = holes.get_mut(&id)
                && !constraint.expected.contains(ty)
            {
                constraint.expected.push(ty.clone());
            }
            Expression::Hole(id)
        }
    }
}

fn constrain_hole(ty: &Type, expected: Type, holes: &mut BTreeMap<HoleId, HoleConstraint>) {
    if let Type::Hole(id) = ty
        && let Some(constraint) = holes.get_mut(id)
        && !constraint.expected.contains(&expected)
    {
        constraint.expected.push(expected);
    }
}

fn lower_type(
    module: &ModulePath,
    declaration: &Name,
    ast: &AstType,
    known: &BTreeSet<Name>,
    holes: &mut BTreeMap<HoleId, HoleConstraint>,
    diagnostics: &mut Vec<Diagnostic>,
    span: Span,
) -> Type {
    match ast {
        AstType::Unit => Type::Unit,
        AstType::Bool => Type::Bool,
        AstType::Text => Type::Text,
        AstType::Integer => Type::Integer,
        AstType::Decimal => Type::Decimal,
        AstType::Variable(v) => Type::Variable(TypeVar(*v)),
        AstType::Named(raw) => match Name::new(raw.clone()) {
            Ok(name) if known.contains(&name) => Type::Named(QualifiedName {
                module: module.clone(),
                name,
            }),
            Ok(_) | Err(_) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownType,
                    span,
                    format!("unknown type `{raw}`"),
                ));
                Type::Named(QualifiedName {
                    module: module.clone(),
                    name: Name::generated(raw.clone()),
                })
            }
        },
        AstType::Hole(hole) => Type::Hole(register_hole(
            module,
            declaration,
            hole,
            known,
            holes,
            diagnostics,
        )),
        AstType::Record { fields, open_tail } => {
            let mut lowered = Vec::new();
            let mut seen = BTreeSet::new();
            for (field, field_ty) in fields {
                match Name::new(field.clone()) {
                    Ok(name) if seen.insert(name.clone()) => lowered.push(Field {
                        name,
                        ty: lower_type(
                            module,
                            declaration,
                            field_ty,
                            known,
                            holes,
                            diagnostics,
                            span,
                        ),
                    }),
                    Ok(_) => diagnostics.push(Diagnostic::error(
                        DiagnosticCode::DuplicateField,
                        span,
                        format!("duplicate record field `{field}`"),
                    )),
                    Err(_) => diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidRow,
                        span,
                        format!("invalid record field `{field}`"),
                    )),
                }
            }
            lowered.sort_by(|a, b| a.name.cmp(&b.name));
            match open_tail {
                Some(tail) => Type::Record(Row::Open {
                    fields: lowered,
                    tail: TypeVar(*tail),
                }),
                None => Type::Record(Row::Closed(lowered)),
            }
        }
        AstType::Function { arguments, result } => Type::Function {
            arguments: arguments
                .iter()
                .map(|a| lower_type(module, declaration, a, known, holes, diagnostics, span))
                .collect(),
            result: Box::new(lower_type(
                module,
                declaration,
                result,
                known,
                holes,
                diagnostics,
                span,
            )),
        },
        AstType::Refined { base, predicate } => Type::Refined {
            base: Box::new(lower_type(
                module,
                declaration,
                base,
                known,
                holes,
                diagnostics,
                span,
            )),
            predicate: predicate.clone(),
        },
        AstType::AtPhase { phase, value } => match Name::new(phase.clone()) {
            Ok(name) => Type::AtPhase {
                phase: Phase::new(name),
                value: Box::new(lower_type(
                    module,
                    declaration,
                    value,
                    known,
                    holes,
                    diagnostics,
                    span,
                )),
            },
            Err(_) => {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidPhase,
                    span,
                    format!("invalid phase `{phase}`"),
                ));
                lower_type(module, declaration, value, known, holes, diagnostics, span)
            }
        },
    }
}

fn register_hole(
    module: &ModulePath,
    declaration: &Name,
    hole: &AstHole,
    known: &BTreeSet<Name>,
    holes: &mut BTreeMap<HoleId, HoleConstraint>,
    diagnostics: &mut Vec<Diagnostic>,
) -> HoleId {
    if let Some(name) = &hole.name
        && Name::new(name.clone()).is_err()
    {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::InvalidName,
            hole.span,
            format!("invalid hole name `{name}`"),
        ));
    }
    let mut hasher = Hasher::new();
    hasher.update(b"axiom/hir/hole/v1\0");
    hasher.update(module.canonical().as_bytes());
    hasher.update(b"\0");
    hasher.update(declaration.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(hole.name.as_deref().unwrap_or("_").as_bytes());
    hasher.update(b"\0");
    let flavor: &[u8] = if hole.generic {
        b"generic"
    } else {
        b"ordinary"
    };
    hasher.update(flavor);
    if hole.name.is_none() {
        hasher.update(b"\0anonymous-span\0");
        hasher.update(hole.span.start.to_le_bytes().as_slice());
        hasher.update(hole.span.end.to_le_bytes().as_slice());
    }
    let bytes = hasher.finalize();
    let id = HoleId(u64::from_le_bytes(
        bytes.as_bytes()[..8].try_into().expect("digest slice"),
    ));
    let expected = hole
        .expected
        .as_ref()
        .map(|expected| {
            vec![lower_type(
                module,
                declaration,
                expected,
                known,
                holes,
                diagnostics,
                hole.span,
            )]
        })
        .unwrap_or_default();
    if let Some(existing) = holes.get_mut(&id) {
        for ty in &expected {
            if !existing.expected.contains(ty) {
                existing.expected.push(ty.clone());
            }
        }
        existing.generic |= hole.generic;
    } else {
        holes.insert(
            id,
            HoleConstraint {
                id,
                name: hole
                    .name
                    .as_ref()
                    .and_then(|name| Name::new(name.clone()).ok()),
                generic: hole.generic,
                expected,
                span: hole.span,
            },
        );
    }
    id
}

fn canonical_module(module: &Module) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, "module");
    put(&mut out, &module.path.canonical());
    for declaration in &module.declarations {
        put(&mut out, "decl");
        put(&mut out, declaration.name.as_str());
        put_type(&mut out, &declaration.kind);
    }
    for hole in &module.holes {
        put(&mut out, "hole");
        put(&mut out, &hole.id.0.to_string());
        put(
            &mut out,
            hole.name.as_ref().map(Name::as_str).unwrap_or("_"),
        );
        put(&mut out, if hole.generic { "generic" } else { "ordinary" });
        for ty in &hole.expected {
            put_type_value(&mut out, ty);
        }
    }
    for diagnostic in &module.diagnostics {
        put(&mut out, "diagnostic");
        put(&mut out, &format!("{:?}", diagnostic.severity));
        put(&mut out, &format!("{:?}", diagnostic.code));
        put(&mut out, &diagnostic.span.start.to_string());
        put(&mut out, &diagnostic.span.end.to_string());
        put(&mut out, &diagnostic.message);
    }
    out
}

fn put(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(value.as_bytes());
    out.push(b'|');
}
fn put_type(out: &mut Vec<u8>, kind: &DeclarationKind) {
    match kind {
        DeclarationKind::Type { ty } => {
            put(out, "type");
            put_type_value(out, ty);
        }
        DeclarationKind::Value {
            ty,
            expression: None,
        } => {
            put(out, "value");
            put_type_value(out, ty);
        }
        DeclarationKind::Value {
            ty,
            expression: Some(expr),
        } => {
            put(out, "value");
            put_type_value(out, ty);
            put(out, "expr");
            match expr {
                Expression::Text(value) => {
                    put(out, "text");
                    put(out, value);
                }
                Expression::Integer(value) => {
                    put(out, "integer");
                    put(out, &value.to_string());
                }
                Expression::Bool(value) => {
                    put(out, if *value { "true" } else { "false" });
                }
                Expression::Hole(id) => {
                    put(out, "hole");
                    put(out, &id.0.to_string());
                }
            }
        }
        DeclarationKind::Rule { input, output } => {
            put(out, "rule");
            put_type_value(out, input);
            put_type_value(out, output);
        }
    }
}
fn put_type_value(out: &mut Vec<u8>, ty: &Type) {
    match ty {
        Type::Unit => put(out, "unit"),
        Type::Bool => put(out, "bool"),
        Type::Text => put(out, "text"),
        Type::Integer => put(out, "integer"),
        Type::Decimal => put(out, "decimal"),
        Type::Named(name) => {
            put(out, "named");
            put(out, &name.canonical());
        }
        Type::Variable(variable) => {
            put(out, "variable");
            put(out, &variable.0.to_string());
        }
        Type::Hole(id) => {
            put(out, "hole");
            put(out, &id.0.to_string());
        }
        Type::Record(row) => {
            put(
                out,
                if row.is_open() {
                    "open-row"
                } else {
                    "closed-row"
                },
            );
            put(out, &row.fields().len().to_string());
            for field in row.fields() {
                put(out, field.name.as_str());
                put_type_value(out, &field.ty);
            }
            if let Row::Open { tail, .. } = row {
                put(out, &tail.0.to_string());
            }
        }
        Type::Function { arguments, result } => {
            put(out, "function");
            put(out, &arguments.len().to_string());
            for argument in arguments {
                put_type_value(out, argument);
            }
            put(out, "result");
            put_type_value(out, result);
        }
        Type::Refined { base, predicate } => {
            put(out, "refined");
            put_type_value(out, base);
            put_predicate(out, predicate);
        }
        Type::AtPhase { phase, value } => {
            put(out, "at-phase");
            put(out, phase.as_name().as_str());
            put_type_value(out, value);
        }
    }
}

fn put_predicate(out: &mut Vec<u8>, predicate: &Predicate) {
    match predicate {
        Predicate::Named(name) => {
            put(out, "named");
            put(out, name.as_str());
        }
        Predicate::Equals(value) => {
            put(out, "equals");
            put(out, value);
        }
        Predicate::NotEquals(value) => {
            put(out, "not-equals");
            put(out, value);
        }
        Predicate::InSet(values) => {
            put(out, "in-set");
            let mut values = values.clone();
            values.sort();
            values.dedup();
            put(out, &values.len().to_string());
            for value in values {
                put(out, &value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module() -> ModulePath {
        ModulePath::root(Name::new("demo").unwrap())
    }
    fn ast_type_hole(name: Option<&str>) -> AstType {
        AstType::Hole(AstHole {
            name: name.map(str::to_owned),
            generic: true,
            expected: Some(Box::new(AstType::Text)),
            span: Span::new(1, 2),
        })
    }

    #[test]
    fn lowering_sorts_declarations_and_rows_and_is_content_stable() {
        let ast = AstModule {
            path: module(),
            declarations: vec![
                AstDeclaration {
                    name: "zeta".into(),
                    kind: AstDeclarationKind::Type(AstType::Record {
                        fields: vec![("b".into(), AstType::Bool), ("a".into(), AstType::Integer)],
                        open_tail: Some(1),
                    }),
                    span: Span::default(),
                },
                AstDeclaration {
                    name: "alpha".into(),
                    kind: AstDeclarationKind::Type(AstType::Named("zeta".into())),
                    span: Span::default(),
                },
            ],
        };
        let first = lower(ast.clone());
        let second = lower(ast);
        assert_eq!(first.declarations[0].name.as_str(), "alpha");
        assert_eq!(first.content_id(), second.content_id());
        assert_eq!(
            first.declarations[1].kind,
            DeclarationKind::Type {
                ty: Type::Record(Row::Open {
                    fields: vec![
                        Field {
                            name: Name::new("a").unwrap(),
                            ty: Type::Integer
                        },
                        Field {
                            name: Name::new("b").unwrap(),
                            ty: Type::Bool
                        }
                    ],
                    tail: TypeVar(1)
                })
            }
        );
    }

    #[test]
    fn declaration_kinds_have_distinct_content_ids() {
        let declaration = |kind| AstModule {
            path: module(),
            declarations: vec![AstDeclaration {
                name: "entry".into(),
                kind,
                span: Span::default(),
            }],
        };
        let type_id = lower(declaration(AstDeclarationKind::Type(AstType::Text))).content_id();
        let value_id = lower(declaration(AstDeclarationKind::Value {
            ty: AstType::Text,
            expression: None,
        }))
        .content_id();

        assert_ne!(type_id, value_id);
    }

    #[test]
    fn holes_are_stable_and_retain_expected_constraints() {
        let ast = AstModule {
            path: module(),
            declarations: vec![AstDeclaration {
                name: "value".into(),
                kind: AstDeclarationKind::Value {
                    ty: ast_type_hole(Some("x")),
                    expression: Some(AstExpression::Hole(AstHole {
                        name: Some("x".into()),
                        generic: true,
                        expected: None,
                        span: Span::new(3, 4),
                    })),
                },
                span: Span::default(),
            }],
        };
        let lowered = lower(ast);
        assert_eq!(lowered.holes.len(), 1);
        assert_eq!(lowered.holes[0].expected, vec![Type::Text]);
        assert!(lowered.holes[0].generic);
        assert!(!lowered.has_errors());
    }

    #[test]
    fn validation_reports_unknown_types_duplicate_fields_and_expression_mismatch() {
        let ast = AstModule {
            path: module(),
            declarations: vec![AstDeclaration {
                name: "v".into(),
                kind: AstDeclarationKind::Value {
                    ty: AstType::Record {
                        fields: vec![
                            ("x".into(), AstType::Named("missing".into())),
                            ("x".into(), AstType::Bool),
                        ],
                        open_tail: None,
                    },
                    expression: Some(AstExpression::Integer(2)),
                },
                span: Span::new(10, 11),
            }],
        };
        let lowered = lower(ast);
        assert!(lowered.has_errors());
        assert!(
            lowered
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnknownType)
        );
        assert!(
            lowered
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::DuplicateField)
        );
        assert!(
            lowered
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ExpressionTypeMismatch)
        );
    }

    #[test]
    fn qualified_names_and_phase_types_are_preserved() {
        let ast = AstModule {
            path: module(),
            declarations: vec![AstDeclaration {
                name: "amount".into(),
                kind: AstDeclarationKind::Type(AstType::AtPhase {
                    phase: "posted".into(),
                    value: Box::new(AstType::Decimal),
                }),
                span: Span::default(),
            }],
        };
        let lowered = lower(ast);
        assert_eq!(
            lowered.declarations[0].kind,
            DeclarationKind::Type {
                ty: Type::AtPhase {
                    phase: Phase::new(Name::new("posted").unwrap()),
                    value: Box::new(Type::Decimal)
                }
            }
        );
    }
}
