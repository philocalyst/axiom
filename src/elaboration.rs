//! Source-form elaboration into package-owned record values.
//!
//! This is deliberately a narrow bridge between the lossless authoring
//! surface and the package compiler.  A source `form` names a schema, but the
//! schema is authoritative only when resolved through a compiled artifact and
//! an explicit package root.  The source itself supplies field occurrences and
//! exact scalar spellings; this module never resolves a schema by its printed
//! name alone and never adds a domain [`crate::model::LedgerForm`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use num_bigint::BigInt;
use num_rational::BigRational;

use crate::hir::{ModulePath, Name, QualifiedName, Type};
use crate::ir::{Record, Sort, Symbol, Term, Var};
use crate::model::{ContentHash, OccurrenceId};
use crate::package_compiler::{
    CompiledArtifact, RecordSchema, RecordSchemaError, RecordValueError, SchemaBoundRecord,
};
use crate::surface::{FormView, NodeId, Severity, Span, Token, TokenKind};

/// The source-local diagnostic classes emitted by [`elaborate_form`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FormDiagnosticCode {
    IncompleteForm,
    InvalidOccurrence,
    MissingSchema,
    InvalidSchema,
    PackageMismatch,
    UnknownSchema,
    NotRecordSchema,
    MalformedField,
    DuplicateField,
    UnknownField,
    MissingField,
    MissingValue,
    ExtraValue,
    InvalidValue,
    TypeMismatch,
    UnsupportedType,
    DuplicateOccurrence,
    InternalValueCheck,
}

/// A diagnostic whose span points into the exact source retained by
/// [`crate::surface::SurfaceFile`].
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FormDiagnostic {
    pub severity: Severity,
    pub code: FormDiagnosticCode,
    pub span: Span,
    pub message: String,
}

impl FormDiagnostic {
    fn error(code: FormDiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for FormDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}, column {}: {}",
            self.span.line, self.span.column, self.message
        )
    }
}

/// The successful result of elaborating one source form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ElaboratedForm {
    node_id: NodeId,
    occurrence: OccurrenceId,
    schema: RecordSchema,
    value: SchemaBoundRecord,
}

impl ElaboratedForm {
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn occurrence(&self) -> &OccurrenceId {
        &self.occurrence
    }

    pub fn schema(&self) -> &RecordSchema {
        &self.schema
    }

    pub fn package_root(&self) -> ContentHash {
        self.schema.package_root()
    }

    pub fn value(&self) -> &SchemaBoundRecord {
        &self.value
    }
}

/// Why source-form elaboration did not produce a bound value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormElaborationError {
    /// The package-root-qualified schema could not be selected.  The
    /// diagnostics retain the source location of the schema token.
    SchemaResolution {
        error: RecordSchemaError,
        diagnostics: Vec<FormDiagnostic>,
    },
    /// One or more source-local diagnostics made the value ineligible for
    /// schema checking.
    Diagnostics(Vec<FormDiagnostic>),
}

impl FormElaborationError {
    pub fn diagnostics(&self) -> &[FormDiagnostic] {
        match self {
            Self::SchemaResolution { diagnostics, .. } | Self::Diagnostics(diagnostics) => {
                diagnostics
            }
        }
    }

    pub fn schema_error(&self) -> Option<&RecordSchemaError> {
        match self {
            Self::SchemaResolution { error, .. } => Some(error),
            Self::Diagnostics(_) => None,
        }
    }
}

impl fmt::Display for FormElaborationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaResolution { error, .. } => {
                write!(formatter, "schema resolution failed: {error}")
            }
            Self::Diagnostics(diagnostics) => {
                write!(
                    formatter,
                    "form has {} source diagnostic(s)",
                    diagnostics.len()
                )
            }
        }
    }
}

impl std::error::Error for FormElaborationError {}

/// Elaborate one generic [`FormView`] against an exact package root.
///
/// The schema token is parsed only into a [`QualifiedName`].  It is not an
/// authority: [`CompiledArtifact::resolve_record_schema`] receives both that
/// name and the caller-provided package root.  This prevents an identically
/// named record in another package from being selected accidentally.
pub fn elaborate_form(
    form: FormView<'_>,
    artifact: &CompiledArtifact,
    package_root: ContentHash,
) -> Result<ElaboratedForm, FormElaborationError> {
    let mut diagnostics = Vec::new();
    if let Some(error) = form.node().error.as_ref() {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::IncompleteForm,
            error.span,
            error.message.clone(),
        ));
    }
    let occurrence = match form.occurrence() {
        Some(token) => match OccurrenceId::try_new(token.lexeme.clone()) {
            Ok(occurrence) => Some(occurrence),
            Err(error) => {
                diagnostics.push(FormDiagnostic::error(
                    FormDiagnosticCode::InvalidOccurrence,
                    token.span,
                    error.to_string(),
                ));
                None
            }
        },
        None => {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::InvalidOccurrence,
                form.node().span,
                "form header requires an occurrence identifier",
            ));
            None
        }
    };

    let schema_parts = form.schema_parts().collect::<Vec<_>>();
    let schema_span = schema_parts
        .first()
        .map(|token| token.span)
        .unwrap_or_else(|| form.node().span);
    let Some(schema_reference) =
        parse_schema_reference(&schema_parts, schema_span, &mut diagnostics)
    else {
        sort_diagnostics(&mut diagnostics);
        return Err(FormElaborationError::Diagnostics(diagnostics));
    };

    if let Some(package) = artifact
        .packages()
        .iter()
        .find(|package| package.root_hash() == package_root)
        && package.name != schema_reference.package
    {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::PackageMismatch,
            schema_span,
            format!(
                "form names package `{}`, but the pinned package root belongs to `{}`",
                schema_reference.package, package.name
            ),
        ));
        sort_diagnostics(&mut diagnostics);
        return Err(FormElaborationError::Diagnostics(diagnostics));
    }

    let schema =
        match artifact.resolve_record_schema(package_root, &schema_reference.qualified_name) {
            Ok(schema) => schema,
            Err(error) => {
                let code = match &error {
                    RecordSchemaError::NotRecord { .. } => FormDiagnosticCode::NotRecordSchema,
                    _ => FormDiagnosticCode::UnknownSchema,
                };
                diagnostics.push(FormDiagnostic::error(code, schema_span, error.to_string()));
                sort_diagnostics(&mut diagnostics);
                return Err(FormElaborationError::SchemaResolution { error, diagnostics });
            }
        };

    if schema.row().is_open() {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::UnsupportedType,
            schema_span,
            "open record schemas require typed extension fields and cannot yet be authored as a form",
        ));
    }

    let mut fields = BTreeMap::<Symbol, Term>::new();
    let mut source_fields = BTreeMap::<String, Span>::new();
    let mut hole_table = HoleTable::default();
    let schema_fields = schema.row().fields();

    for field in form.fields() {
        let meaningful = field
            .tokens()
            .iter()
            .filter(|token| !token.is_trivia())
            .collect::<Vec<_>>();
        let Some(name_token) = meaningful.first().copied() else {
            continue;
        };
        if name_token.kind != TokenKind::Identifier {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::MalformedField,
                field.span(),
                "form field must begin with an identifier",
            ));
            continue;
        }
        let name = name_token.lexeme.clone();
        let field_span = field.span();
        if source_fields.insert(name.clone(), field_span).is_some() {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::DuplicateField,
                name_token.span,
                format!("duplicate source field `{name}`"),
            ));
            continue;
        }
        let Some(schema_field) = schema_fields
            .iter()
            .find(|schema_field| schema_field.name.as_str() == name)
        else {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::UnknownField,
                name_token.span,
                format!("field `{name}` is not declared by the package record schema"),
            ));
            continue;
        };
        let values = meaningful.into_iter().skip(1).collect::<Vec<_>>();
        if let Some(value) = elaborate_value(
            &schema_field.ty,
            &values,
            field_span,
            &mut hole_table,
            &mut diagnostics,
        ) {
            fields.insert(Symbol::from(name), value);
        }
    }

    for schema_field in schema_fields {
        let name = schema_field.name.as_str().to_owned();
        if !source_fields.contains_key(&name) {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::MissingField,
                form.node().span,
                format!("record is missing field `{name}`"),
            ));
        }
    }

    sort_diagnostics(&mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(FormElaborationError::Diagnostics(diagnostics));
    }

    let record = Record::closed(fields);
    let value = schema.check_concrete_record(&record).map_err(|error| {
        let diagnostic = value_error_diagnostic(&error, &source_fields, form.node().span);
        FormElaborationError::Diagnostics(vec![diagnostic])
    })?;

    Ok(ElaboratedForm {
        node_id: form.node().id,
        occurrence: occurrence.expect("missing occurrence emitted a diagnostic"),
        schema,
        value,
    })
}

/// Elaborate a document's forms while enforcing ledger-wide occurrence
/// uniqueness. Each form carries the exact package root already selected by
/// the caller's pinned artifact context; no display-name fallback is used.
pub fn elaborate_forms<'a>(
    forms: impl IntoIterator<Item = (FormView<'a>, ContentHash)>,
    artifact: &CompiledArtifact,
) -> Result<Vec<ElaboratedForm>, FormElaborationError> {
    let mut elaborated = Vec::new();
    let mut occurrences = BTreeSet::new();
    for (form, package_root) in forms {
        let occurrence_span = form
            .occurrence()
            .map_or(form.node().span, |token| token.span);
        let value = elaborate_form(form, artifact, package_root)?;
        if !occurrences.insert(value.occurrence.clone()) {
            return Err(FormElaborationError::Diagnostics(vec![
                FormDiagnostic::error(
                    FormDiagnosticCode::DuplicateOccurrence,
                    occurrence_span,
                    format!("duplicate form occurrence `{}`", value.occurrence),
                ),
            ]));
        }
        elaborated.push(value);
    }
    Ok(elaborated)
}

struct SchemaReference {
    package: String,
    qualified_name: QualifiedName,
}

fn parse_schema_reference(
    parts: &[&Token],
    span: Span,
    diagnostics: &mut Vec<FormDiagnostic>,
) -> Option<SchemaReference> {
    if parts.is_empty() {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::MissingSchema,
            span,
            "form header requires a record schema after `:`",
        ));
        return None;
    }
    let raw = parts
        .iter()
        .map(|token| token.lexeme.as_str())
        .collect::<String>();
    if parts
        .iter()
        .any(|token| token.kind != TokenKind::Identifier)
        || parts
            .windows(2)
            .any(|pair| !pair[0].lexeme.ends_with("::") && !pair[1].lexeme.starts_with("::"))
    {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::InvalidSchema,
            span,
            "record schema tokens must be joined by `::`",
        ));
        return None;
    }
    let segments = raw.split("::").collect::<Vec<_>>();
    if segments.len() < 3 || segments.iter().any(|segment| segment.is_empty()) {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::InvalidSchema,
            span,
            format!("schema `{raw}` must use `package::module::Record` spelling"),
        ));
        return None;
    }
    let package = segments[0].to_owned();
    let mut names = Vec::with_capacity(segments.len() - 1);
    for segment in &segments[1..] {
        match Name::new(segment.to_owned()) {
            Ok(name) => names.push(name),
            Err(_) => {
                diagnostics.push(FormDiagnostic::error(
                    FormDiagnosticCode::InvalidSchema,
                    span,
                    format!("invalid schema name `{raw}`"),
                ));
                return None;
            }
        }
    }
    let name = names.pop().expect("schema has at least two segments");
    Some(SchemaReference {
        package,
        qualified_name: QualifiedName {
            module: ModulePath::new(names).expect("schema has a module segment"),
            name,
        },
    })
}

#[derive(Default)]
struct HoleTable {
    next_id: u32,
    named: BTreeMap<String, (u32, Sort)>,
}

impl HoleTable {
    fn variable(
        &mut self,
        token: &Token,
        sort: Sort,
        diagnostics: &mut Vec<FormDiagnostic>,
    ) -> Var {
        let hole_name = token.lexeme.strip_prefix('?').unwrap_or(&token.lexeme);
        if token.lexeme == "_" || token.lexeme == "?" {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            return Var::hole(id, format!("_{id}"), sort);
        }
        if let Some((id, previous_sort)) = self.named.get(hole_name).cloned() {
            if previous_sort != sort && previous_sort != Sort::Any && sort != Sort::Any {
                diagnostics.push(FormDiagnostic::error(
                    FormDiagnosticCode::TypeMismatch,
                    token.span,
                    format!(
                        "named hole `?{hole_name}` is used with incompatible types `{previous_sort:?}` and `{sort:?}`"
                    ),
                ));
            }
            return Var::hole(id, hole_name.to_owned(), previous_sort);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.named.insert(hole_name.to_owned(), (id, sort.clone()));
        Var::hole(id, hole_name.to_owned(), sort)
    }
}

fn elaborate_value(
    ty: &Type,
    values: &[&Token],
    span: Span,
    holes: &mut HoleTable,
    diagnostics: &mut Vec<FormDiagnostic>,
) -> Option<Term> {
    if values.is_empty() {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::MissingValue,
            span,
            "record field requires a value",
        ));
        return None;
    }
    if values.len() > 1 {
        diagnostics.push(FormDiagnostic::error(
            FormDiagnosticCode::ExtraValue,
            values[1].span,
            "record field accepts exactly one scalar value",
        ));
        return None;
    }
    let token = values[0];
    match ty {
        Type::Bool => {
            if let Some(var) = typed_hole(token, Sort::Bool, holes, diagnostics) {
                return Some(Term::Var(var));
            }
            match token.lexeme.as_str() {
                "true" => Some(Term::Bool(true)),
                "false" => Some(Term::Bool(false)),
                _ => {
                    diagnostics.push(type_error(token, "bool", &token.lexeme));
                    None
                }
            }
        }
        Type::Text => {
            if let Some(var) = typed_hole(token, Sort::Text, holes, diagnostics) {
                return Some(Term::Var(var));
            }
            match token.kind {
                TokenKind::Identifier | TokenKind::Number => Some(Term::Text(token.lexeme.clone())),
                TokenKind::String => match decode_string(&token.lexeme) {
                    Ok(value) => Some(Term::Text(value)),
                    Err(message) => {
                        diagnostics.push(FormDiagnostic::error(
                            FormDiagnosticCode::InvalidValue,
                            token.span,
                            message,
                        ));
                        None
                    }
                },
                _ => {
                    diagnostics.push(type_error(token, "text", &token.lexeme));
                    None
                }
            }
        }
        Type::Integer => {
            if let Some(var) = typed_hole(token, Sort::Integer, holes, diagnostics) {
                return Some(Term::Var(var));
            }
            match parse_integer(&token.lexeme) {
                Ok(value) => Some(Term::Integer(value)),
                Err(message) => {
                    diagnostics.push(FormDiagnostic::error(
                        FormDiagnosticCode::InvalidValue,
                        token.span,
                        message,
                    ));
                    None
                }
            }
        }
        Type::Decimal => {
            if let Some(var) = typed_hole(token, Sort::Decimal, holes, diagnostics) {
                return Some(Term::Var(var));
            }
            match parse_decimal(&token.lexeme) {
                Ok(value) => Some(Term::Decimal(value)),
                Err(message) => {
                    diagnostics.push(FormDiagnostic::error(
                        FormDiagnosticCode::InvalidValue,
                        token.span,
                        message,
                    ));
                    None
                }
            }
        }
        Type::Record(_) => {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::UnsupportedType,
                token.span,
                "nested record values require a row-qualified nested form",
            ));
            None
        }
        other => {
            diagnostics.push(FormDiagnostic::error(
                FormDiagnosticCode::UnsupportedType,
                token.span,
                format!("source elaboration does not support schema type `{other:?}`"),
            ));
            None
        }
    }
}

fn typed_hole(
    token: &Token,
    sort: Sort,
    holes: &mut HoleTable,
    diagnostics: &mut Vec<FormDiagnostic>,
) -> Option<Var> {
    if !matches!(token.kind, TokenKind::Hole(_)) {
        return None;
    }
    Some(holes.variable(token, sort, diagnostics))
}

fn type_error(token: &Token, expected: &str, actual: &str) -> FormDiagnostic {
    FormDiagnostic::error(
        FormDiagnosticCode::TypeMismatch,
        token.span,
        format!("value `{actual}` is not a valid {expected}"),
    )
}

fn parse_integer(raw: &str) -> Result<BigInt, String> {
    if raw.is_empty()
        || raw == "+"
        || raw == "-"
        || raw.contains('.')
        || !raw.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_digit() || (index == 0 && matches!(byte, b'+' | b'-'))
        })
    {
        return Err(format!("`{raw}` is not an exact integer"));
    }
    BigInt::from_str(raw).map_err(|_| format!("`{raw}` is not an exact integer"))
}

fn parse_decimal(raw: &str) -> Result<BigRational, String> {
    let negative = raw.starts_with('-');
    let unsigned = raw
        .strip_prefix('+')
        .or_else(|| raw.strip_prefix('-'))
        .unwrap_or(raw);
    let (whole, fraction) = unsigned
        .split_once('.')
        .map_or((unsigned, ""), |(whole, fraction)| (whole, fraction));
    if whole.is_empty()
        || fraction.is_empty() && unsigned.contains('.')
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (fraction.is_empty() && !unsigned.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!("`{raw}` is not an exact decimal"));
    }
    let exponent = u32::try_from(fraction.len())
        .map_err(|_| format!("decimal `{raw}` has too many fractional digits"))?;
    let scale = BigInt::from(10u8).pow(exponent);
    let mut digits = String::with_capacity(whole.len() + fraction.len());
    digits.push_str(whole);
    digits.push_str(fraction);
    let mut numerator =
        BigInt::from_str(&digits).map_err(|_| format!("`{raw}` is not an exact decimal"))?;
    if negative {
        numerator = -numerator;
    }
    Ok(BigRational::new(numerator, scale))
}

fn decode_string(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes[0] != *bytes.last().unwrap_or(&0) {
        return Err(format!("unterminated string literal `{raw}`"));
    }
    let mut result = String::new();
    let mut chars = raw[1..raw.len() - 1].chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err("string literal ends with an escape".to_owned());
        };
        result.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            other => {
                return Err(format!("unsupported string escape `\\{other}`"));
            }
        });
    }
    Ok(result)
}

fn value_error_diagnostic(
    error: &RecordValueError,
    fields: &BTreeMap<String, Span>,
    fallback: Span,
) -> FormDiagnostic {
    let path = match error {
        RecordValueError::MissingField { path }
        | RecordValueError::UnexpectedField { path }
        | RecordValueError::UnexpectedRowTail { path }
        | RecordValueError::InvalidRowTail { path }
        | RecordValueError::InvalidDecimal { path }
        | RecordValueError::RecordHoleNeedsSchema { path }
        | RecordValueError::DuplicateSchemaField { path }
        | RecordValueError::UntypedHole { path }
        | RecordValueError::HoleSortMismatch { path, .. }
        | RecordValueError::TypeMismatch { path, .. }
        | RecordValueError::UnsupportedType { path, .. } => path.first(),
    };
    let span = path
        .and_then(|name| fields.get(name))
        .copied()
        .unwrap_or(fallback);
    FormDiagnostic::error(
        FormDiagnosticCode::InternalValueCheck,
        span,
        error.to_string(),
    )
}

fn sort_diagnostics(diagnostics: &mut [FormDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.span
            .cmp(&right.span)
            .then(left.code.cmp(&right.code))
            .then(left.message.cmp(&right.message))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{AstDeclaration, AstDeclarationKind, AstModule, AstType, Span as HirSpan};
    use crate::package_compiler::{PackageInput, compile};
    use crate::package_lock::{
        Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
    };
    use crate::surface::SurfaceFile;

    fn artifact() -> (CompiledArtifact, ContentHash) {
        artifact_with_tail(None)
    }

    fn artifact_with_tail(open_tail: Option<u32>) -> (CompiledArtifact, ContentHash) {
        let manifest = PackageManifest::new("billing", Version::new(1, 0, 0), "types");
        let lock = Lockfile {
            roots: vec![Dependency::new(
                "billing",
                VersionReq::Exact(manifest.version),
            )],
            packages: vec![LockedPackage {
                name: manifest.name.clone(),
                version: manifest.version,
                hash: manifest.hash(),
                dependencies: Vec::new(),
            }],
        };
        let module_path = ModulePath::root(Name::new("types").unwrap());
        let module = crate::hir::lower(AstModule {
            path: module_path,
            declarations: vec![AstDeclaration {
                name: "Invoice".to_owned(),
                kind: AstDeclarationKind::Type(AstType::Record {
                    fields: vec![
                        ("approved".to_owned(), AstType::Bool),
                        ("count".to_owned(), AstType::Integer),
                        ("total".to_owned(), AstType::Decimal),
                        ("note".to_owned(), AstType::Text),
                    ],
                    open_tail,
                }),
                span: HirSpan::new(0, 7),
            }],
        });
        let artifact = compile([PackageInput::new(manifest, [module])], &lock).unwrap();
        let root = artifact.package_roots()[0];
        (artifact, root)
    }

    #[test]
    fn elaborates_exact_scalars_and_typed_holes() {
        let (artifact, root) = artifact();
        let file = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved true\n  count -7\n  total 12.50\n  note \"ok\"\n",
        );
        let result = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap();
        let fields = &result.value().record().fields;
        assert_eq!(fields[&Symbol::from("approved")], Term::Bool(true));
        assert_eq!(
            fields[&Symbol::from("count")],
            Term::Integer(BigInt::from(-7))
        );
        assert_eq!(
            fields[&Symbol::from("total")],
            Term::Decimal(BigRational::new(BigInt::from(1250), BigInt::from(100)))
        );
        assert_eq!(fields[&Symbol::from("note")], Term::Text("ok".to_owned()));

        let holes = SurfaceFile::parse(
            "form invoice/2 : billing::types::Invoice\n  approved ?ok\n  count _\n  total 1\n  note ?note\n",
        );
        let result = elaborate_form(holes.forms().next().unwrap(), &artifact, root).unwrap();
        assert!(matches!(
            result.value().record().field("approved"),
            Some(Term::Var(var)) if var.sort == Sort::Bool
        ));
        assert!(matches!(
            result.value().record().field("count"),
            Some(Term::Var(var)) if var.sort == Sort::Integer
        ));
    }

    #[test]
    fn reports_duplicate_and_unknown_fields_at_source_spans() {
        let (artifact, root) = artifact();
        let file = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved true\n  approved false\n  mystery value\n  count 1\n  total 2\n  note ok\n",
        );
        let error = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap_err();
        let diagnostics = error.diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == FormDiagnosticCode::DuplicateField && diagnostic.span.line == 3
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == FormDiagnosticCode::UnknownField && diagnostic.span.line == 4
        }));
    }

    #[test]
    fn schema_root_is_authoritative() {
        let (artifact, root) = artifact();
        let file = SurfaceFile::parse(
            "form invoice/1 : billing::types::Missing\n  approved true\n  count 1\n  total 1\n  note ok\n",
        );
        let error = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap_err();
        assert!(matches!(
            error,
            FormElaborationError::SchemaResolution { .. }
        ));

        let file = SurfaceFile::parse(
            "form invoice/1 : other::types::Invoice\n  approved true\n  count 1\n  total 1\n  note ok\n",
        );
        let error = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == FormDiagnosticCode::PackageMismatch)
        );

        let file = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved true\n  count 1\n  total 1\n  note ok\n",
        );
        let error = elaborate_form(
            file.forms().next().unwrap(),
            &artifact,
            ContentHash::domain_separated("test/wrong-root", b"billing"),
        )
        .unwrap_err();
        assert!(matches!(
            error.schema_error(),
            Some(RecordSchemaError::UnknownPackageRoot { .. })
        ));
    }

    #[test]
    fn schema_spacing_and_field_order_do_not_change_the_bound_value() {
        let (artifact, root) = artifact();
        let first = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved true\n  count 7\n  total 7.0\n  note ok\n",
        );
        let second = SurfaceFile::parse(
            "form invoice/1:billing :: types :: Invoice\n  note ok\n  total 7\n  count 7\n  approved true\n",
        );
        let first = elaborate_form(first.forms().next().unwrap(), &artifact, root).unwrap();
        let second = elaborate_form(second.forms().next().unwrap(), &artifact, root).unwrap();
        assert_eq!(first.value(), second.value());
    }

    #[test]
    fn malformed_occurrences_and_incompatible_named_holes_are_source_errors() {
        let (artifact, root) = artifact();
        let malformed = SurfaceFile::parse(
            "form ??? : billing::types::Invoice\n  approved true\n  count 1\n  total 1\n  note ok\n",
        );
        let error = elaborate_form(malformed.forms().next().unwrap(), &artifact, root).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == FormDiagnosticCode::InvalidOccurrence)
        );

        let holes = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved ?same\n  count 1\n  total 1\n  note ?same\n",
        );
        let error = elaborate_form(holes.forms().next().unwrap(), &artifact, root).unwrap_err();
        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.code == FormDiagnosticCode::TypeMismatch && diagnostic.span.line == 5
        }));
    }

    #[test]
    fn malformed_schema_boundaries_cannot_fuse_into_an_authoritative_name() {
        let (artifact, root) = artifact();
        for schema in [
            "billing types::Invoice",
            "billing::types Invoice",
            "billing :: types Invoice",
        ] {
            let file = SurfaceFile::parse(format!(
                "form invoice/1 : {schema}\n  approved true\n  count 1\n  total 1\n  note ok\n"
            ));
            let error = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap_err();
            assert!(
                error
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code == FormDiagnosticCode::InvalidSchema)
            );
        }
    }

    #[test]
    fn batch_elaboration_rejects_duplicate_occurrences_and_open_source_schemas() {
        let (artifact, root) = artifact();
        let file = SurfaceFile::parse(
            "form invoice/1 : billing::types::Invoice\n  approved true\n  count 1\n  total 1\n  note first\nform invoice/1 : billing::types::Invoice\n  approved false\n  count 2\n  total 2\n  note second\n",
        );
        let error = elaborate_forms(file.forms().map(|form| (form, root)), &artifact).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == FormDiagnosticCode::DuplicateOccurrence)
        );

        let (artifact, root) = artifact_with_tail(Some(0));
        let file = SurfaceFile::parse(
            "form invoice/2 : billing::types::Invoice\n  approved true\n  count 1\n  total 1\n  note open\n",
        );
        let error = elaborate_form(file.forms().next().unwrap(), &artifact, root).unwrap_err();
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == FormDiagnosticCode::UnsupportedType)
        );
    }
}
