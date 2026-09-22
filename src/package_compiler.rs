//! Deterministic coherence and binding for typed package inputs.
//!
//! This module intentionally starts after parsing and lowering.  Callers hand
//! it [`crate::hir::Module`] values, immutable package manifests, and a
//! verified lockfile; it produces only a content-addressed description of the
//! declarations that were compiled.  There is no source parser, evaluator, or
//! language-server state in this boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::hir::{
    Declaration, DeclarationKind, Diagnostic, Module, ModulePath, QualifiedName, Type,
};
use crate::model::ContentHash;
use crate::package_lock::{Lockfile, PackageLockError, PackageManifest, PackageRegistry, Version};

const ARTIFACT_DOMAIN: &str = "axiom/package-artifact/v1";
const CAPABILITY_ARTIFACT_DOMAIN: &str = "axiom/package-artifact/v2";
const RECORD_SCHEMA_DOMAIN: &str = "axiom/package-record-schema/v1";
const CAPABILITY_RECORD_SCHEMA_DOMAIN: &str = "axiom/package-record-schema/v2";
const RECORD_VALUE_DOMAIN: &str = "axiom/package-record-value/v1";

/// The input to one package compilation unit.
///
/// The manifest body is deliberately not interpreted here.  It contributes to
/// the manifest hash through [`PackageManifest::hash`], while the typed HIR is
/// supplied independently by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInput {
    pub manifest: PackageManifest,
    pub modules: Vec<Module>,
    /// Explicit capabilities attached to exported declarations in this
    /// package.  A capability is never inferred from a declaration name or
    /// type shape; the compiler validates each binding below.
    schema_capabilities: Vec<SchemaCapabilityBinding>,
}

impl PackageInput {
    pub fn new(manifest: PackageManifest, modules: impl IntoIterator<Item = Module>) -> Self {
        Self {
            manifest,
            modules: modules.into_iter().collect(),
            schema_capabilities: Vec::new(),
        }
    }

    /// Attach one versioned capability to an exported schema.  Duplicate
    /// bindings are retained until compilation so the compiler can reject
    /// them explicitly rather than silently changing their meaning.
    pub fn with_schema_capability(
        mut self,
        qualified_name: QualifiedName,
        capability: SchemaCapability,
    ) -> Self {
        self.schema_capabilities
            .push(SchemaCapabilityBinding::new(qualified_name, capability));
        self
    }

    pub fn manifest_hash(&self) -> ContentHash {
        self.manifest.hash()
    }

    pub fn schema_capabilities(&self) -> &[SchemaCapabilityBinding] {
        &self.schema_capabilities
    }

    /// Return the content identity of the complete compiler input.
    ///
    /// A manifest hash alone is not sufficient for an incremental package
    /// query: the typed modules are supplied separately from the manifest and
    /// changing one must invalidate the package compilation.  Module order is
    /// intentionally ignored here for the same reason it is ignored by
    /// [`compile`].
    pub fn input_hash(&self) -> ContentHash {
        let mut modules = self.modules.iter().collect::<Vec<_>>();
        modules.sort_by(|left, right| module_order(left, right));
        let mut bytes = Vec::new();
        put_text(&mut bytes, "package-input");
        bytes.extend_from_slice(self.manifest.hash().as_bytes());
        put_u64(&mut bytes, modules.len());
        for module in modules {
            put_text(&mut bytes, &module.path.canonical());
            bytes.extend_from_slice(&module.recomputed_content_id().bytes());
        }
        let domain = if self.schema_capabilities.is_empty() {
            "axiom/package-input/v1"
        } else {
            put_schema_capabilities(&mut bytes, &self.schema_capabilities);
            "axiom/package-input/v2"
        };
        ContentHash::domain_separated(domain, &bytes)
    }
}

/// A versioned semantic capability which a package may explicitly export.
///
/// The version is part of the variant, rather than being inferred from a
/// package name, declaration name, or record shape.  Adding a future version
/// therefore creates a new capability and a new content identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SchemaCapability {
    SettlementStateV1,
}

impl SchemaCapability {
    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::SettlementStateV1 => "settlement-state/v1",
        }
    }
}

impl fmt::Display for SchemaCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.canonical_name())
    }
}

/// An explicit attachment between a package export and a versioned schema
/// capability.  The qualified name keeps the attachment package-local while
/// still making the export identity unambiguous in an artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaCapabilityBinding {
    pub qualified_name: QualifiedName,
    pub capability: SchemaCapability,
}

impl SchemaCapabilityBinding {
    pub fn new(qualified_name: QualifiedName, capability: SchemaCapability) -> Self {
        Self {
            qualified_name,
            capability,
        }
    }
}

/// A module included in a compiled artifact.
///
/// The HIR content ID is retained rather than copying source text or claiming
/// that this layer has emitted executable code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledModule {
    pub path: ModulePath,
    pub content_id: crate::hir::ContentId,
}

/// One declaration exported by a package.
///
/// HIR has no visibility modifier yet, so every declaration in an error-free
/// module is an export at this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledExport {
    pub package: String,
    pub qualified_name: QualifiedName,
    pub kind: DeclarationKind,
}

/// The deterministic output for one package input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledPackage {
    pub name: String,
    pub version: Version,
    pub manifest_hash: ContentHash,
    pub modules: Vec<CompiledModule>,
    schema_capabilities: Vec<SchemaCapabilityBinding>,
}

impl CompiledPackage {
    pub fn schema_capabilities(&self) -> &[SchemaCapabilityBinding] {
        &self.schema_capabilities
    }

    /// Return the content address of this package's complete compiled input.
    ///
    /// The package root is deliberately derived from the normalized compiler
    /// result rather than from an insertion-order-dependent wrapper.
    pub fn root_hash(&self) -> ContentHash {
        let mut modules = self.modules.clone();
        modules.sort_by(|left, right| {
            left.path
                .canonical()
                .cmp(&right.path.canonical())
                .then(left.content_id.cmp(&right.content_id))
        });
        let mut bytes = Vec::new();
        put_text(&mut bytes, "package-root");
        put_text(&mut bytes, &self.name);
        put_text(&mut bytes, &self.version.to_string());
        bytes.extend_from_slice(self.manifest_hash.as_bytes());
        put_u64(&mut bytes, modules.len());
        for module in modules {
            put_text(&mut bytes, &module.path.canonical());
            bytes.extend_from_slice(&module.content_id.bytes());
        }
        let domain = if self.schema_capabilities.is_empty() {
            "axiom/package-root/v1"
        } else {
            put_schema_capabilities(&mut bytes, &self.schema_capabilities);
            "axiom/package-root/v2"
        };
        ContentHash::domain_separated(domain, &bytes)
    }
}

/// A compiled package set bound to exactly one lockfile and its manifests.
///
/// The artifact hash is private so callers cannot construct a seemingly valid
/// hash after mutating an artifact.  [`CompiledArtifact::verify`] recomputes it
/// before checking the artifact against fresh inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledArtifact {
    lockfile_hash: ContentHash,
    packages: Vec<CompiledPackage>,
    exports: Vec<CompiledExport>,
    artifact_hash: ContentHash,
}

impl CompiledArtifact {
    pub fn lockfile_hash(&self) -> ContentHash {
        self.lockfile_hash
    }

    pub fn packages(&self) -> &[CompiledPackage] {
        &self.packages
    }

    pub fn exports(&self) -> &[CompiledExport] {
        &self.exports
    }
    pub fn artifact_hash(&self) -> ContentHash {
        self.artifact_hash
    }

    /// Return the deterministic root for each compiled package in this
    /// artifact. These roots are content identities, not store locations.
    pub fn package_roots(&self) -> Vec<ContentHash> {
        let mut roots = self
            .packages
            .iter()
            .map(CompiledPackage::root_hash)
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        roots
    }

    /// Return the canonical bytes used to derive [`Self::artifact_hash`].
    ///
    /// Workspace and persistent backends use this as an opaque, stable query
    /// value.  It deliberately contains no source text or insertion-order
    /// state, so equivalent package inputs produce byte-identical values.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        artifact_bytes(self.lockfile_hash, &self.packages, &self.exports)
    }

    /// Recompute the artifact hash from the public artifact contents.
    pub fn recomputed_hash(&self) -> ContentHash {
        artifact_hash(self.lockfile_hash, &self.packages, &self.exports)
    }

    /// Resolve one directly exported record declaration against the exact
    /// package that owns it.
    ///
    /// Package roots are required deliberately: a qualified name is only a
    /// declaration key inside a particular compiled package context.  The
    /// returned value owns its small schema view, so it cannot observe a
    /// mutable HIR value or an artifact constructed later.
    pub fn resolve_record_schema(
        &self,
        package_root: ContentHash,
        qualified_name: &QualifiedName,
    ) -> Result<RecordSchema, RecordSchemaError> {
        let package = self
            .packages
            .iter()
            .find(|package| package.root_hash() == package_root)
            .ok_or(RecordSchemaError::UnknownPackageRoot { package_root })?;

        let export = self
            .exports
            .iter()
            .find(|export| {
                export.package == package.name && export.qualified_name == *qualified_name
            })
            .ok_or_else(|| RecordSchemaError::UnknownExport {
                package_root,
                package: package.name.clone(),
                qualified_name: Box::new(qualified_name.clone()),
            })?;

        let DeclarationKind::Type {
            ty: Type::Record(row),
        } = &export.kind
        else {
            return Err(RecordSchemaError::NotRecord {
                package_root,
                package: package.name.clone(),
                qualified_name: Box::new(qualified_name.clone()),
            });
        };

        let capability = package
            .schema_capabilities
            .iter()
            .find(|binding| binding.qualified_name == *qualified_name)
            .map(|binding| binding.capability);

        Ok(RecordSchema {
            package_root,
            qualified_name: qualified_name.clone(),
            row: row.clone(),
            capability,
            schema_id: record_schema_id(
                self.artifact_hash,
                package_root,
                qualified_name,
                &export.kind,
                capability,
            ),
        })
    }

    /// Verify this artifact against fresh package and lockfile inputs.
    ///
    /// Verification checks both the artifact's internal hash and the complete
    /// deterministic compilation result.  In particular, changing a manifest
    /// body, module HIR, package order, or lockfile changes the expected result
    /// and cannot be hidden by retaining the old artifact hash.
    pub fn verify<I>(&self, packages: I, lockfile: &Lockfile) -> Result<(), PackageCompileError>
    where
        I: IntoIterator<Item = PackageInput>,
    {
        let recomputed = self.recomputed_hash();
        if recomputed != self.artifact_hash {
            return Err(PackageCompileError::ArtifactHashMismatch {
                expected: recomputed,
                actual: self.artifact_hash,
            });
        }

        let expected = compile(packages, lockfile)?;
        if self != &expected {
            return Err(PackageCompileError::ArtifactInputMismatch {
                expected: expected.artifact_hash,
                actual: self.artifact_hash,
            });
        }
        Ok(())
    }

    /// Alias with a name that makes the input-binding check explicit at call
    /// sites.
    pub fn verify_against<I>(
        &self,
        packages: I,
        lockfile: &Lockfile,
    ) -> Result<(), PackageCompileError>
    where
        I: IntoIterator<Item = PackageInput>,
    {
        self.verify(packages, lockfile)
    }
}

/// The immutable schema view returned by [`CompiledArtifact::resolve_record_schema`].
///
/// The row is retained exactly as lowered: [`crate::hir::Row::Closed`] marks
/// undeclared fields invalid during value checking, while
/// [`crate::hir::Row::Open`] carries its explicit tail variable. The schema ID
/// is content-addressed and includes the artifact, package root, qualified
/// name, and complete record type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordSchema {
    package_root: ContentHash,
    qualified_name: QualifiedName,
    row: crate::hir::Row,
    capability: Option<SchemaCapability>,
    schema_id: ContentHash,
}

impl RecordSchema {
    pub fn package_root(&self) -> ContentHash {
        self.package_root
    }

    pub fn qualified_name(&self) -> &QualifiedName {
        &self.qualified_name
    }

    pub fn row(&self) -> &crate::hir::Row {
        &self.row
    }

    /// Return the explicit capability attached to this exported schema.
    /// `None` means the schema is ordinary and carries no semantic projection
    /// authority.
    pub fn capability(&self) -> Option<SchemaCapability> {
        self.capability
    }

    pub fn schema_id(&self) -> ContentHash {
        self.schema_id
    }

    /// Check and schema-guide the normalization of one IR record.
    ///
    /// Decimal fields accept integer literals by exact widening; no other
    /// coercions are performed. Aliases, functions, refinements, and phase
    /// wrappers require a later compiler boundary with their semantics.
    pub fn check_concrete_record(
        &self,
        record: &crate::ir::Record,
    ) -> Result<SchemaBoundRecord, RecordValueError> {
        let normalized = normalize_row(&self.row, record, &mut Vec::new())?;

        let canonical = crate::ir::canonicalize_term(
            &crate::ir::Term::Record(normalized),
            &crate::ir::CanonicalContext::default(),
        );
        let canonical_record = match canonical.value {
            crate::ir::Term::Record(record) => record,
            _ => unreachable!("canonicalizing a record returns a record"),
        };
        let content_hash = record_value_hash(self.schema_id, &canonical.bytes);
        Ok(SchemaBoundRecord {
            schema_id: self.schema_id,
            record: canonical_record,
            content_hash,
        })
    }
}

/// A record that has passed a concrete [`RecordSchema`] check.
///
/// The schema identity and canonical record are private so callers cannot
/// mutate one without invalidating the content hash.  Use [`Self::record`]
/// when handing the value to another IR consumer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaBoundRecord {
    schema_id: ContentHash,
    record: crate::ir::Record,
    content_hash: ContentHash,
}

impl SchemaBoundRecord {
    pub fn schema_id(&self) -> ContentHash {
        self.schema_id
    }

    pub fn record(&self) -> &crate::ir::Record {
        &self.record
    }

    pub fn content_hash(&self) -> ContentHash {
        self.content_hash
    }
}

/// A failure to turn an untrusted IR record into a schema-bound value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordValueError {
    MissingField {
        path: Vec<String>,
    },
    UnexpectedField {
        path: Vec<String>,
    },
    UnexpectedRowTail {
        path: Vec<String>,
    },
    InvalidRowTail {
        path: Vec<String>,
    },
    InvalidDecimal {
        path: Vec<String>,
    },
    RecordHoleNeedsSchema {
        path: Vec<String>,
    },
    DuplicateSchemaField {
        path: Vec<String>,
    },
    UntypedHole {
        path: Vec<String>,
    },
    HoleSortMismatch {
        path: Vec<String>,
        expected: String,
        actual: String,
    },
    TypeMismatch {
        path: Vec<String>,
        expected: String,
        actual: String,
    },
    UnsupportedType {
        path: Vec<String>,
        ty: String,
    },
}

impl fmt::Display for RecordValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { path } => {
                write!(
                    formatter,
                    "record is missing field `{}`",
                    display_path(path)
                )
            }
            Self::UnexpectedField { path } => {
                write!(
                    formatter,
                    "record has unexpected field `{}`",
                    display_path(path)
                )
            }
            Self::UnexpectedRowTail { path } => write!(
                formatter,
                "closed record `{}` cannot carry a row tail",
                display_path(path)
            ),
            Self::InvalidRowTail { path } => write!(
                formatter,
                "open record `{}` requires a row-typed tail",
                display_path(path)
            ),
            Self::InvalidDecimal { path } => write!(
                formatter,
                "record field `{}` has a decimal with a zero denominator",
                display_path(path)
            ),
            Self::RecordHoleNeedsSchema { path } => write!(
                formatter,
                "record field `{}` has a hole without a row schema",
                display_path(path)
            ),
            Self::DuplicateSchemaField { path } => write!(
                formatter,
                "schema declares duplicate field `{}`",
                display_path(path)
            ),
            Self::UntypedHole { path } => write!(
                formatter,
                "record field `{}` requires a typed hole variable",
                display_path(path)
            ),
            Self::HoleSortMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "record field `{}` has hole sort `{actual}`, incompatible with `{expected}`",
                display_path(path)
            ),
            Self::TypeMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "record field `{}` has `{actual}`, expected `{expected}`",
                display_path(path)
            ),
            Self::UnsupportedType { path, ty } => write!(
                formatter,
                "record field `{}` uses unsupported type `{ty}`",
                display_path(path)
            ),
        }
    }
}

impl std::error::Error for RecordValueError {}

fn normalize_row(
    row: &crate::hir::Row,
    record: &crate::ir::Record,
    path: &mut Vec<String>,
) -> Result<crate::ir::Record, RecordValueError> {
    let fields = row.fields();
    let mut known = BTreeSet::new();
    for field in fields {
        if !known.insert(field.name.as_str()) {
            path.push(field.name.as_str().to_owned());
            let error = RecordValueError::DuplicateSchemaField { path: path.clone() };
            path.pop();
            return Err(error);
        }
    }

    let mut normalized = record.clone();
    for field in fields {
        let name = field.name.as_str().to_owned();
        path.push(name.clone());
        let symbol = crate::ir::Symbol::from(name.as_str());
        let result = match record.fields.get(&symbol) {
            Some(value) => normalize_type(&field.ty, value, path),
            None => Err(RecordValueError::MissingField { path: path.clone() }),
        };
        path.pop();
        normalized.fields.insert(symbol, result?);
    }

    if !row.is_open() {
        for name in normalized.fields.keys() {
            if !known.contains(name.as_str()) {
                path.push(name.as_str().to_owned());
                let error = RecordValueError::UnexpectedField { path: path.clone() };
                path.pop();
                return Err(error);
            }
        }
        if normalized.rest.is_some() {
            return Err(RecordValueError::UnexpectedRowTail { path: path.clone() });
        }
    } else if let Some(rest) = &normalized.rest
        && !(matches!(rest.kind, crate::ir::VarKind::Row)
            && matches!(rest.sort, crate::ir::Sort::Row))
        && !(matches!(rest.kind, crate::ir::VarKind::Hole)
            && matches!(rest.sort, crate::ir::Sort::Row))
    {
        return Err(RecordValueError::InvalidRowTail { path: path.clone() });
    }
    Ok(normalized)
}

fn normalize_type(
    ty: &Type,
    value: &crate::ir::Term,
    path: &mut Vec<String>,
) -> Result<crate::ir::Term, RecordValueError> {
    match ty {
        Type::Bool => {
            normalize_primitive(ty, value, matches!(value, crate::ir::Term::Bool(_)), path)
        }
        Type::Text => {
            normalize_primitive(ty, value, matches!(value, crate::ir::Term::Text(_)), path)
        }
        Type::Integer => normalize_primitive(
            ty,
            value,
            matches!(value, crate::ir::Term::Integer(_)),
            path,
        ),
        Type::Decimal => match value {
            crate::ir::Term::Decimal(value) if !num_traits::Zero::is_zero(value.denom()) => {
                Ok(crate::ir::Term::decimal(value.clone()))
            }
            crate::ir::Term::Decimal(_) => {
                Err(RecordValueError::InvalidDecimal { path: path.clone() })
            }
            crate::ir::Term::Integer(value) => Ok(crate::ir::Term::Decimal(
                num_rational::BigRational::from_integer(value.clone()),
            )),
            crate::ir::Term::Var(variable) if variable.kind == crate::ir::VarKind::Hole => {
                normalize_hole(ty, variable, path)
            }
            _ => Err(type_mismatch(ty, value, path)),
        },
        Type::Record(row) => match value {
            crate::ir::Term::Record(record) => {
                normalize_row(row, record, path).map(crate::ir::Term::Record)
            }
            crate::ir::Term::Var(variable) if variable.kind == crate::ir::VarKind::Hole => {
                Err(RecordValueError::RecordHoleNeedsSchema { path: path.clone() })
            }
            _ => Err(type_mismatch(ty, value, path)),
        },
        Type::Unit
        | Type::Hole(_)
        | Type::Named(_)
        | Type::Variable(_)
        | Type::Function { .. }
        | Type::Refined { .. }
        | Type::AtPhase { .. } => Err(RecordValueError::UnsupportedType {
            path: path.clone(),
            ty: ty.canonical(),
        }),
    }
}

fn normalize_primitive(
    expected: &Type,
    value: &crate::ir::Term,
    matches: bool,
    path: &[String],
) -> Result<crate::ir::Term, RecordValueError> {
    if matches {
        Ok(value.clone())
    } else if let crate::ir::Term::Var(variable) = value
        && variable.kind == crate::ir::VarKind::Hole
    {
        normalize_hole(expected, variable, path)
    } else if let crate::ir::Term::Var(_) = value {
        Err(RecordValueError::UntypedHole {
            path: path.to_vec(),
        })
    } else {
        Err(type_mismatch(expected, value, path))
    }
}

fn normalize_hole(
    expected: &Type,
    variable: &crate::ir::Var,
    path: &[String],
) -> Result<crate::ir::Term, RecordValueError> {
    if hole_sort_accepts(expected, &variable.sort) {
        Ok(crate::ir::Term::Var(variable.clone()))
    } else {
        Err(RecordValueError::HoleSortMismatch {
            path: path.to_vec(),
            expected: expected.canonical(),
            actual: format!("{:?}", variable.sort),
        })
    }
}

fn hole_sort_accepts(expected: &Type, sort: &crate::ir::Sort) -> bool {
    match expected {
        Type::Unit => false,
        Type::Bool => matches!(sort, crate::ir::Sort::Bool),
        Type::Text => matches!(sort, crate::ir::Sort::Text),
        Type::Integer => matches!(sort, crate::ir::Sort::Integer),
        Type::Decimal => matches!(sort, crate::ir::Sort::Integer | crate::ir::Sort::Decimal),
        Type::Record(_) => matches!(sort, crate::ir::Sort::Record),
        Type::Hole(_) => false,
        Type::Named(_)
        | Type::Variable(_)
        | Type::Function { .. }
        | Type::Refined { .. }
        | Type::AtPhase { .. } => false,
    }
}

fn type_mismatch(expected: &Type, value: &crate::ir::Term, path: &[String]) -> RecordValueError {
    RecordValueError::TypeMismatch {
        path: path.to_vec(),
        expected: expected.canonical(),
        actual: term_kind(value).to_owned(),
    }
}

fn term_kind(value: &crate::ir::Term) -> &'static str {
    match value {
        crate::ir::Term::Var(_) => "variable",
        crate::ir::Term::Nominal(_) => "nominal",
        crate::ir::Term::Unit(_) => "unit",
        crate::ir::Term::Quantity(_) => "quantity",
        crate::ir::Term::Record(_) => "record",
        crate::ir::Term::Tuple(_) => "tuple",
        crate::ir::Term::App { .. } => "application",
        crate::ir::Term::Bool(_) => "boolean",
        crate::ir::Term::Text(_) => "text",
        crate::ir::Term::Integer(_) => "integer",
        crate::ir::Term::Decimal(_) => "decimal",
    }
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<record>".to_owned()
    } else {
        path.join(".")
    }
}

fn record_value_hash(schema_id: ContentHash, canonical_bytes: &[u8]) -> ContentHash {
    let mut bytes = Vec::new();
    put_text(&mut bytes, "checked-record");
    bytes.extend_from_slice(schema_id.as_bytes());
    put_u64(&mut bytes, canonical_bytes.len());
    bytes.extend_from_slice(canonical_bytes);
    ContentHash::domain_separated(RECORD_VALUE_DOMAIN, &bytes)
}

/// Failure to resolve a record schema without falling back to a name-only
/// lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordSchemaError {
    UnknownPackageRoot {
        package_root: ContentHash,
    },
    UnknownExport {
        package_root: ContentHash,
        package: String,
        qualified_name: Box<QualifiedName>,
    },
    NotRecord {
        package_root: ContentHash,
        package: String,
        qualified_name: Box<QualifiedName>,
    },
}

impl fmt::Display for RecordSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPackageRoot { package_root } => {
                write!(
                    formatter,
                    "compiled package root `{package_root}` is unknown"
                )
            }
            Self::UnknownExport {
                package_root,
                package,
                qualified_name,
            } => write!(
                formatter,
                "export `{}` is unknown in package `{package}` rooted at `{package_root}`",
                qualified_name.canonical()
            ),
            Self::NotRecord {
                package_root,
                package,
                qualified_name,
            } => write!(
                formatter,
                "export `{}` in package `{package}` rooted at `{package_root}` is not a direct record type",
                qualified_name.canonical()
            ),
        }
    }
}

impl std::error::Error for RecordSchemaError {}

/// Errors raised while validating package inputs or constructing coherence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageCompileError {
    Manifest(PackageLockError),
    Lockfile(PackageLockError),
    DuplicatePackageInput {
        name: String,
    },
    PackageNotLocked {
        name: String,
        version: Version,
    },
    ModuleHasErrors {
        package: String,
        module: String,
        diagnostics: Vec<Diagnostic>,
    },
    DuplicateModule {
        package: String,
        module: String,
    },
    DuplicateExport {
        package: String,
        qualified_name: String,
    },
    DuplicateSchemaCapability {
        package: String,
        qualified_name: String,
    },
    UnknownSchemaCapabilityExport {
        package: String,
        qualified_name: String,
        capability: SchemaCapability,
    },
    InvalidSchemaCapability {
        package: String,
        qualified_name: String,
        capability: SchemaCapability,
        reason: String,
    },
    InvalidExportType {
        package: String,
        qualified_name: String,
        reason: String,
    },
    ArtifactHashMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
    ArtifactInputMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
}

impl fmt::Display for PackageCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => write!(formatter, "invalid package manifest: {error}"),
            Self::Lockfile(error) => write!(formatter, "invalid package lockfile: {error}"),
            Self::DuplicatePackageInput { name } => {
                write!(formatter, "package `{name}` is supplied more than once")
            }
            Self::PackageNotLocked { name, version } => {
                write!(
                    formatter,
                    "package `{name}@{version}` is not in the lockfile"
                )
            }
            Self::ModuleHasErrors {
                package,
                module,
                diagnostics,
            } => write!(
                formatter,
                "package `{package}` module `{module}` has {} error diagnostic(s)",
                diagnostics.len()
            ),
            Self::DuplicateModule { package, module } => write!(
                formatter,
                "package `{package}` supplies module `{module}` more than once"
            ),
            Self::DuplicateExport {
                package,
                qualified_name,
            } => write!(
                formatter,
                "package `{package}` exports `{qualified_name}` more than once"
            ),
            Self::DuplicateSchemaCapability {
                package,
                qualified_name,
            } => write!(
                formatter,
                "package `{package}` attaches more than one schema capability to `{qualified_name}`"
            ),
            Self::UnknownSchemaCapabilityExport {
                package,
                qualified_name,
                capability,
            } => write!(
                formatter,
                "package `{package}` attaches capability `{capability}` to unknown export `{qualified_name}`"
            ),
            Self::InvalidSchemaCapability {
                package,
                qualified_name,
                capability,
                reason,
            } => write!(
                formatter,
                "package `{package}` export `{qualified_name}` cannot carry capability `{capability}`: {reason}"
            ),
            Self::InvalidExportType {
                package,
                qualified_name,
                reason,
            } => write!(
                formatter,
                "package `{package}` export `{qualified_name}` has an invalid type: {reason}"
            ),
            Self::ArtifactHashMismatch { expected, actual } => write!(
                formatter,
                "compiled artifact hash is {actual}, expected {expected}"
            ),
            Self::ArtifactInputMismatch { expected, actual } => write!(
                formatter,
                "compiled artifact {actual} does not match inputs (expected {expected})"
            ),
        }
    }
}

impl std::error::Error for PackageCompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(error) | Self::Lockfile(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_declaration_type(kind: &DeclarationKind) -> Result<(), String> {
    match kind {
        DeclarationKind::Type { ty } | DeclarationKind::Value { ty, .. } => validate_type(ty),
        DeclarationKind::Rule { input, output } => {
            validate_type(input)?;
            validate_type(output)
        }
    }
}

fn validate_type(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Record(row) => {
            let mut names = BTreeSet::new();
            let mut previous = None;
            for field in row.fields() {
                if !names.insert(field.name.as_str()) {
                    return Err(format!("duplicate record field `{}`", field.name));
                }
                if previous.is_some_and(|name| name >= &field.name) {
                    return Err(format!(
                        "record field `{}` is not in canonical order",
                        field.name
                    ));
                }
                previous = Some(&field.name);
                validate_type(&field.ty)?;
            }
            Ok(())
        }
        Type::Function { arguments, result } => {
            for argument in arguments {
                validate_type(argument)?;
            }
            validate_type(result)
        }
        Type::Refined { base, .. } | Type::AtPhase { value: base, .. } => validate_type(base),
        Type::Unit
        | Type::Bool
        | Type::Text
        | Type::Integer
        | Type::Decimal
        | Type::Named(_)
        | Type::Variable(_)
        | Type::Hole(_) => Ok(()),
    }
}

fn validate_schema_capability(
    capability: SchemaCapability,
    kind: &DeclarationKind,
) -> Result<(), String> {
    let DeclarationKind::Type {
        ty: Type::Record(row),
    } = kind
    else {
        return Err("capability requires a direct exported record type".to_owned());
    };

    if row.is_open() {
        return Err("capability requires a closed record row".to_owned());
    }

    match capability {
        SchemaCapability::SettlementStateV1 => {
            // HIR rows are canonicalized by field name.  Keep this list in
            // canonical order so the capability cannot accidentally accept a
            // declaration whose ordering was changed by a public HIR caller.
            let expected = [
                ("amount", Type::Decimal),
                ("at", Type::Text),
                ("from", Type::Text),
                ("instrument", Type::Text),
                ("kind", Type::Text),
                ("settlement", Type::Text),
                ("state", Type::Text),
                ("to", Type::Text),
            ];
            let fields = row.fields();
            if fields.len() != expected.len() {
                return Err(format!(
                    "SettlementStateV1 requires exactly these {} fields: settlement Text, kind Text, state Text, at Text, from Text, to Text, instrument Text, amount Decimal",
                    expected.len()
                ));
            }
            for (field, (expected_name, expected_type)) in fields.iter().zip(expected) {
                if field.name.as_str() != expected_name {
                    return Err(format!(
                        "SettlementStateV1 requires field `{expected_name}`, found `{}`",
                        field.name
                    ));
                }
                if field.ty != expected_type {
                    return Err(format!(
                        "SettlementStateV1 field `{expected_name}` must have primitive type `{expected_type:?}`"
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Compile a deterministic package set from validated HIR modules.
///
/// Package inputs may arrive in any order.  Package names, module paths, and
/// declaration names are canonicalized before validation and hashing, so equal
/// inputs produce equal artifacts regardless of their input order.
pub fn compile<I>(packages: I, lockfile: &Lockfile) -> Result<CompiledArtifact, PackageCompileError>
where
    I: IntoIterator<Item = PackageInput>,
{
    let packages = normalize_inputs_with_lockfile(packages, lockfile)?;
    let lockfile_hash = lockfile.hash();

    let mut compiled_packages = Vec::with_capacity(packages.len());
    let mut exports = BTreeMap::<(String, String), CompiledExport>::new();

    for input in &packages {
        let mut modules = input.modules.clone();
        modules.sort_by(module_order);
        let mut compiled_modules = Vec::with_capacity(modules.len());
        let mut module_paths = BTreeSet::new();

        for module in modules {
            let module_path = module.path.canonical();
            if !module_paths.insert(module_path.clone()) {
                return Err(PackageCompileError::DuplicateModule {
                    package: input.manifest.name.clone(),
                    module: module_path,
                });
            }
            if module.has_errors() {
                return Err(PackageCompileError::ModuleHasErrors {
                    package: input.manifest.name.clone(),
                    module: module.path.canonical(),
                    diagnostics: module.diagnostics.clone(),
                });
            }

            compiled_modules.push(CompiledModule {
                path: module.path.clone(),
                content_id: module.recomputed_content_id(),
            });

            let mut declarations = module.declarations.clone();
            declarations.sort_by(declaration_order);
            for declaration in declarations {
                let qualified_name = QualifiedName {
                    module: module.path.clone(),
                    name: declaration.name.clone(),
                };
                let canonical_name = qualified_name.canonical();
                validate_declaration_type(&declaration.kind).map_err(|reason| {
                    PackageCompileError::InvalidExportType {
                        package: input.manifest.name.clone(),
                        qualified_name: canonical_name.clone(),
                        reason,
                    }
                })?;
                let key = (input.manifest.name.clone(), canonical_name.clone());
                let export = CompiledExport {
                    package: input.manifest.name.clone(),
                    qualified_name,
                    kind: declaration.kind.clone(),
                };
                if exports.insert(key, export).is_some() {
                    return Err(PackageCompileError::DuplicateExport {
                        package: input.manifest.name.clone(),
                        qualified_name: canonical_name,
                    });
                }
            }
        }

        let mut schema_capabilities = input.schema_capabilities.clone();
        schema_capabilities.sort();
        let mut capability_names = BTreeSet::new();
        for binding in &schema_capabilities {
            let canonical_name = binding.qualified_name.canonical();
            if !capability_names.insert(canonical_name.clone()) {
                return Err(PackageCompileError::DuplicateSchemaCapability {
                    package: input.manifest.name.clone(),
                    qualified_name: canonical_name,
                });
            }

            let key = (input.manifest.name.clone(), canonical_name.clone());
            let Some(export) = exports.get(&key) else {
                return Err(PackageCompileError::UnknownSchemaCapabilityExport {
                    package: input.manifest.name.clone(),
                    qualified_name: canonical_name,
                    capability: binding.capability,
                });
            };
            validate_schema_capability(binding.capability, &export.kind).map_err(|reason| {
                PackageCompileError::InvalidSchemaCapability {
                    package: input.manifest.name.clone(),
                    qualified_name: binding.qualified_name.canonical(),
                    capability: binding.capability,
                    reason,
                }
            })?;
        }

        compiled_packages.push(CompiledPackage {
            name: input.manifest.name.clone(),
            version: input.manifest.version,
            manifest_hash: input.manifest.hash(),
            modules: compiled_modules,
            schema_capabilities,
        });
    }

    let exports = exports.into_values().collect::<Vec<_>>();
    let artifact_hash = artifact_hash(lockfile_hash, &compiled_packages, &exports);
    Ok(CompiledArtifact {
        lockfile_hash,
        packages: compiled_packages,
        exports,
        artifact_hash,
    })
}

/// Explicitly named convenience wrapper for callers that prefer package
/// terminology over the short [`compile`] function.
pub fn compile_packages<I>(
    packages: I,
    lockfile: &Lockfile,
) -> Result<CompiledArtifact, PackageCompileError>
where
    I: IntoIterator<Item = PackageInput>,
{
    compile(packages, lockfile)
}

/// Stateless compiler facade for callers that want a named service boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackageCompiler;

impl PackageCompiler {
    pub fn compile<I>(
        packages: I,
        lockfile: &Lockfile,
    ) -> Result<CompiledArtifact, PackageCompileError>
    where
        I: IntoIterator<Item = PackageInput>,
    {
        compile(packages, lockfile)
    }
}

// This private entry point carries the lockfile through normalization.  It is
// split out so all validation still happens before any HIR export is consumed.
fn normalize_inputs_with_lockfile<I>(
    packages: I,
    lockfile: &Lockfile,
) -> Result<Vec<PackageInput>, PackageCompileError>
where
    I: IntoIterator<Item = PackageInput>,
{
    let mut packages: Vec<_> = packages.into_iter().collect();
    packages.sort_by(package_input_order);

    let mut seen = BTreeSet::new();
    let mut registry = PackageRegistry::default();
    for input in &packages {
        if !seen.insert(input.manifest.name.clone()) {
            return Err(PackageCompileError::DuplicatePackageInput {
                name: input.manifest.name.clone(),
            });
        }
        input
            .manifest
            .validate()
            .map_err(PackageCompileError::Manifest)?;
        registry
            .insert(input.manifest.clone())
            .map_err(PackageCompileError::Manifest)?;
    }

    let canonical_lockfile = canonical_lockfile_for_validation(lockfile);
    canonical_lockfile
        .verify(&registry)
        .map_err(PackageCompileError::Lockfile)?;

    for input in &packages {
        let Some(entry) = canonical_lockfile.packages.iter().find(|entry| {
            entry.name == input.manifest.name && entry.version == input.manifest.version
        }) else {
            return Err(PackageCompileError::PackageNotLocked {
                name: input.manifest.name.clone(),
                version: input.manifest.version,
            });
        };
        // `Lockfile::verify` checked this already.  Keep the explicit check at
        // the package boundary so the binding remains visible and robust if
        // lockfile verification grows additional modes later.
        if entry.hash != input.manifest.hash() {
            return Err(PackageCompileError::Lockfile(
                PackageLockError::LockHashMismatch {
                    name: entry.name.clone(),
                    expected: entry.hash,
                    actual: input.manifest.hash(),
                },
            ));
        }
    }
    Ok(packages)
}

fn package_input_order(left: &PackageInput, right: &PackageInput) -> std::cmp::Ordering {
    left.manifest
        .name
        .cmp(&right.manifest.name)
        .then(left.manifest.version.cmp(&right.manifest.version))
        .then(left.manifest.hash().cmp(&right.manifest.hash()))
        .then_with(|| {
            let left_modules = sorted_module_keys(&left.modules);
            let right_modules = sorted_module_keys(&right.modules);
            left_modules.cmp(&right_modules)
        })
}

fn sorted_module_keys(modules: &[Module]) -> Vec<(String, crate::hir::ContentId)> {
    let mut keys = modules
        .iter()
        .map(|module| (module.path.canonical(), module.recomputed_content_id()))
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn module_order(left: &Module, right: &Module) -> std::cmp::Ordering {
    left.path.canonical().cmp(&right.path.canonical()).then(
        left.recomputed_content_id()
            .cmp(&right.recomputed_content_id()),
    )
}

fn declaration_order(left: &Declaration, right: &Declaration) -> std::cmp::Ordering {
    left.name
        .cmp(&right.name)
        .then(declaration_kind_bytes(&left.kind).cmp(&declaration_kind_bytes(&right.kind)))
}

fn canonical_lockfile_for_validation(lockfile: &Lockfile) -> Lockfile {
    let mut canonical = lockfile.clone();
    canonical.roots.sort();
    canonical.packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
    });
    for package in &mut canonical.packages {
        package.dependencies.sort();
    }
    canonical
}

fn artifact_hash(
    lockfile_hash: ContentHash,
    packages: &[CompiledPackage],
    exports: &[CompiledExport],
) -> ContentHash {
    let domain = if packages
        .iter()
        .all(|package| package.schema_capabilities.is_empty())
    {
        ARTIFACT_DOMAIN
    } else {
        CAPABILITY_ARTIFACT_DOMAIN
    };
    ContentHash::domain_separated(domain, &artifact_bytes(lockfile_hash, packages, exports))
}

fn record_schema_id(
    artifact_hash: ContentHash,
    package_root: ContentHash,
    qualified_name: &QualifiedName,
    declaration: &DeclarationKind,
    capability: Option<SchemaCapability>,
) -> ContentHash {
    let mut bytes = Vec::new();
    put_text(&mut bytes, "record-schema");
    bytes.extend_from_slice(artifact_hash.as_bytes());
    bytes.extend_from_slice(package_root.as_bytes());
    put_text(&mut bytes, &qualified_name.canonical());
    bytes.extend_from_slice(&declaration_kind_bytes(declaration));
    let domain = if let Some(capability) = capability {
        put_optional_schema_capability(&mut bytes, Some(capability));
        CAPABILITY_RECORD_SCHEMA_DOMAIN
    } else {
        RECORD_SCHEMA_DOMAIN
    };
    ContentHash::domain_separated(domain, &bytes)
}

fn artifact_bytes(
    lockfile_hash: ContentHash,
    packages: &[CompiledPackage],
    exports: &[CompiledExport],
) -> Vec<u8> {
    let has_capabilities = packages
        .iter()
        .any(|package| !package.schema_capabilities.is_empty());
    let mut bytes = Vec::new();
    put_text(&mut bytes, "artifact");
    bytes.extend_from_slice(lockfile_hash.as_bytes());

    let mut packages = packages.to_vec();
    packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.version.cmp(&right.version))
    });
    put_u64(&mut bytes, packages.len());
    for package in packages {
        put_text(&mut bytes, &package.name);
        put_text(&mut bytes, &package.version.to_string());
        bytes.extend_from_slice(package.manifest_hash.as_bytes());
        let mut modules = package.modules;
        modules.sort_by(|left, right| {
            left.path
                .canonical()
                .cmp(&right.path.canonical())
                .then(left.content_id.cmp(&right.content_id))
        });
        put_u64(&mut bytes, modules.len());
        for module in modules {
            put_text(&mut bytes, &module.path.canonical());
            bytes.extend_from_slice(&module.content_id.bytes());
        }
        if has_capabilities {
            put_schema_capabilities(&mut bytes, &package.schema_capabilities);
        }
    }

    let mut exports = exports.to_vec();
    exports.sort_by(|left, right| {
        left.qualified_name
            .canonical()
            .cmp(&right.qualified_name.canonical())
            .then(left.package.cmp(&right.package))
    });
    put_u64(&mut bytes, exports.len());
    for export in exports {
        put_text(&mut bytes, &export.package);
        put_text(&mut bytes, &export.qualified_name.canonical());
        bytes.extend_from_slice(&declaration_kind_bytes(&export.kind));
    }

    bytes
}

fn declaration_kind_bytes(kind: &DeclarationKind) -> Vec<u8> {
    let mut bytes = Vec::new();
    match kind {
        DeclarationKind::Type { ty } => {
            put_text(&mut bytes, "type");
            put_type(&mut bytes, ty);
        }
        DeclarationKind::Value { ty, expression } => {
            put_text(&mut bytes, "value");
            put_type(&mut bytes, ty);
            match expression {
                None => put_text(&mut bytes, "none"),
                Some(expression) => {
                    put_text(&mut bytes, "expression");
                    match expression {
                        crate::hir::Expression::Text(value) => {
                            put_text(&mut bytes, "text");
                            put_text(&mut bytes, value);
                        }
                        crate::hir::Expression::Integer(value) => {
                            put_text(&mut bytes, "integer");
                            put_text(&mut bytes, &value.to_string());
                        }
                        crate::hir::Expression::Bool(value) => {
                            put_text(&mut bytes, if *value { "true" } else { "false" });
                        }
                        crate::hir::Expression::Hole(id) => {
                            put_text(&mut bytes, "hole");
                            put_text(&mut bytes, &id.0.to_string());
                        }
                    }
                }
            }
        }
        DeclarationKind::Rule { input, output } => {
            put_text(&mut bytes, "rule");
            put_type(&mut bytes, input);
            put_type(&mut bytes, output);
        }
    }
    bytes
}

fn put_type(bytes: &mut Vec<u8>, ty: &Type) {
    let canonical = ty.canonical_bytes();
    put_u64(bytes, canonical.len());
    bytes.extend_from_slice(&canonical);
}

fn put_schema_capabilities(bytes: &mut Vec<u8>, capabilities: &[SchemaCapabilityBinding]) {
    let mut capabilities = capabilities.to_vec();
    capabilities.sort();
    put_u64(bytes, capabilities.len());
    for binding in capabilities {
        put_text(bytes, &binding.qualified_name.canonical());
        put_text(bytes, binding.capability.canonical_name());
    }
}

fn put_optional_schema_capability(bytes: &mut Vec<u8>, capability: Option<SchemaCapability>) {
    match capability {
        Some(capability) => {
            put_text(bytes, "capability");
            put_text(bytes, capability.canonical_name());
        }
        None => put_text(bytes, "no-capability"),
    }
}

fn put_u64(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_be_bytes());
}

fn put_text(bytes: &mut Vec<u8>, value: &str) {
    put_u64(bytes, value.len());
    bytes.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{
        AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, Row, Span,
        TypeVar,
    };
    use crate::package_lock::{Dependency, LockedPackage, VersionReq};

    fn manifest(name: &str) -> PackageManifest {
        PackageManifest::new(name, Version::new(1, 0, 0), "hir-package")
    }

    fn module(name: &str, kind: AstDeclarationKind) -> Module {
        module_at("shared", name, kind)
    }

    fn module_at(path: &str, name: &str, kind: AstDeclarationKind) -> Module {
        crate::hir::lower(AstModule {
            path: ModulePath::root(Name::new(path).unwrap()),
            declarations: vec![AstDeclaration {
                name: name.to_owned(),
                kind,
                span: Span::default(),
            }],
        })
    }

    fn record_module(
        path: &str,
        name: &str,
        fields: &[(&str, AstType)],
        open_tail: Option<u32>,
    ) -> Module {
        module_at(
            path,
            name,
            AstDeclarationKind::Type(AstType::Record {
                fields: fields
                    .iter()
                    .map(|(name, ty)| ((*name).to_owned(), ty.clone()))
                    .collect(),
                open_tail,
            }),
        )
    }

    fn settlement_state_fields() -> Vec<(&'static str, AstType)> {
        vec![
            ("settlement", AstType::Text),
            ("kind", AstType::Text),
            ("state", AstType::Text),
            ("at", AstType::Text),
            ("from", AstType::Text),
            ("to", AstType::Text),
            ("instrument", AstType::Text),
            ("amount", AstType::Decimal),
        ]
    }

    fn settlement_state_input() -> (PackageInput, QualifiedName) {
        let package = manifest("settlement-capability");
        let schema = qualified_name("types", "SettlementState");
        let mut input = PackageInput::new(
            package,
            [record_module(
                "types",
                "SettlementState",
                &settlement_state_fields(),
                None,
            )],
        );
        input = input.with_schema_capability(schema.clone(), SchemaCapability::SettlementStateV1);
        (input, schema)
    }

    fn qualified_name(path: &str, name: &str) -> QualifiedName {
        QualifiedName {
            module: ModulePath::root(Name::new(path).unwrap()),
            name: Name::new(name).unwrap(),
        }
    }

    fn lockfile(manifests: &[PackageManifest]) -> Lockfile {
        Lockfile {
            roots: manifests
                .iter()
                .map(|manifest| {
                    Dependency::new(manifest.name.clone(), VersionReq::Exact(manifest.version))
                })
                .collect(),
            packages: manifests
                .iter()
                .map(|manifest| LockedPackage {
                    name: manifest.name.clone(),
                    version: manifest.version,
                    hash: manifest.hash(),
                    dependencies: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn input_order_does_not_change_artifact() {
        let first = manifest("first");
        let second = manifest("second");
        let lock = lockfile(&[first.clone(), second.clone()]);
        let left = compile(
            [
                PackageInput::new(
                    first.clone(),
                    [module("a", AstDeclarationKind::Type(AstType::Text))],
                ),
                PackageInput::new(
                    second.clone(),
                    [module("b", AstDeclarationKind::Type(AstType::Bool))],
                ),
            ],
            &lock,
        )
        .unwrap();
        let right = compile(
            [
                PackageInput::new(
                    second,
                    [module("b", AstDeclarationKind::Type(AstType::Bool))],
                ),
                PackageInput::new(
                    first,
                    [module("a", AstDeclarationKind::Type(AstType::Text))],
                ),
            ],
            &lock,
        )
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.artifact_hash(), left.recomputed_hash());
    }

    #[test]
    fn errorful_hir_is_not_compiled() {
        let package = manifest("broken");
        let lock = lockfile(std::slice::from_ref(&package));
        let broken = module(
            "value",
            AstDeclarationKind::Value {
                ty: AstType::Text,
                expression: Some(crate::hir::AstExpression::Integer(1)),
            },
        );
        assert!(matches!(
            compile([PackageInput::new(package, [broken])], &lock),
            Err(PackageCompileError::ModuleHasErrors { .. })
        ));
    }

    #[test]
    fn compiler_revalidates_public_hir_row_invariants() {
        let package = manifest("mutated");
        let lock = lockfile(std::slice::from_ref(&package));
        let mut mutated = record_module("types", "Record", &[("amount", AstType::Integer)], None);
        let DeclarationKind::Type {
            ty: Type::Record(Row::Closed(fields)),
        } = &mut mutated.declarations[0].kind
        else {
            panic!("fixture must lower to a closed record");
        };
        fields.push(fields[0].clone());

        assert!(matches!(
            compile([PackageInput::new(package, [mutated])], &lock),
            Err(PackageCompileError::InvalidExportType { reason, .. })
                if reason.contains("duplicate record field")
        ));

        let package = manifest("unsorted");
        let lock = lockfile(std::slice::from_ref(&package));
        let mut mutated = record_module(
            "types",
            "Record",
            &[("amount", AstType::Integer), ("note", AstType::Text)],
            None,
        );
        let DeclarationKind::Type {
            ty: Type::Record(Row::Closed(fields)),
        } = &mut mutated.declarations[0].kind
        else {
            panic!("fixture must lower to a closed record");
        };
        fields.reverse();
        assert!(matches!(
            compile([PackageInput::new(package, [mutated])], &lock),
            Err(PackageCompileError::InvalidExportType { reason, .. })
                if reason.contains("canonical order")
        ));
    }

    #[test]
    fn settlement_state_capability_is_explicit_and_hash_bound() {
        let (bound, schema) = settlement_state_input();
        let package = bound.manifest.clone();
        let lock = lockfile(std::slice::from_ref(&package));
        let unbound = PackageInput::new(
            package.clone(),
            [record_module(
                "types",
                "SettlementState",
                &settlement_state_fields(),
                None,
            )],
        );
        let unbound_artifact = compile([unbound.clone()], &lock).unwrap();
        let bound_artifact = compile([bound.clone()], &lock).unwrap();
        let root = bound_artifact.package_roots()[0];
        let resolved = bound_artifact.resolve_record_schema(root, &schema).unwrap();

        assert_eq!(
            resolved.capability(),
            Some(SchemaCapability::SettlementStateV1)
        );
        assert_eq!(
            unbound_artifact
                .resolve_record_schema(unbound_artifact.package_roots()[0], &schema)
                .unwrap()
                .capability(),
            None
        );
        assert_ne!(unbound.input_hash(), bound.input_hash());
        assert_ne!(
            unbound_artifact.package_roots(),
            bound_artifact.package_roots()
        );
        assert_ne!(
            unbound_artifact.artifact_hash(),
            bound_artifact.artifact_hash()
        );
        assert_eq!(
            unbound_artifact.artifact_hash(),
            ContentHash::domain_separated(ARTIFACT_DOMAIN, &unbound_artifact.canonical_bytes())
        );
        assert_eq!(
            bound_artifact.artifact_hash(),
            ContentHash::domain_separated(
                CAPABILITY_ARTIFACT_DOMAIN,
                &bound_artifact.canonical_bytes()
            )
        );
        bound_artifact.verify([bound], &lock).unwrap();
        assert!(
            unbound_artifact
                .verify([bound_artifact_input()], &lock)
                .is_err()
        );

        fn bound_artifact_input() -> PackageInput {
            settlement_state_input().0
        }
    }

    #[test]
    fn settlement_state_capability_rejects_duplicate_and_unknown_bindings() {
        let (bound, schema) = settlement_state_input();
        let package = bound.manifest.clone();
        let lock = lockfile(std::slice::from_ref(&package));
        let duplicate = bound
            .clone()
            .with_schema_capability(schema.clone(), SchemaCapability::SettlementStateV1);
        assert!(matches!(
            compile([duplicate], &lock),
            Err(PackageCompileError::DuplicateSchemaCapability { .. })
        ));

        let unknown_name = qualified_name("types", "Missing");
        let unknown = PackageInput::new(
            package,
            [record_module(
                "types",
                "SettlementState",
                &settlement_state_fields(),
                None,
            )],
        )
        .with_schema_capability(unknown_name, SchemaCapability::SettlementStateV1);
        assert!(matches!(
            compile([unknown], &lock),
            Err(PackageCompileError::UnknownSchemaCapabilityExport { .. })
        ));
    }

    #[test]
    fn settlement_state_capability_requires_exact_closed_primitive_schema() {
        let cases = [
            ("open", Some(7), settlement_state_fields()),
            (
                "missing",
                None,
                settlement_state_fields()
                    .into_iter()
                    .filter(|(name, _)| *name != "amount")
                    .collect(),
            ),
            (
                "extra",
                None,
                settlement_state_fields()
                    .into_iter()
                    .chain([("extra", AstType::Text)])
                    .collect(),
            ),
        ];
        for (name, open_tail, fields) in cases {
            let package = manifest("settlement-capability");
            let schema = qualified_name("types", name);
            let lock = lockfile(std::slice::from_ref(&package));
            let input =
                PackageInput::new(package, [record_module("types", name, &fields, open_tail)])
                    .with_schema_capability(schema, SchemaCapability::SettlementStateV1);
            assert!(matches!(
                compile([input], &lock),
                Err(PackageCompileError::InvalidSchemaCapability { .. })
            ));
        }

        for replacement in [
            Type::Integer,
            Type::Named(QualifiedName {
                module: ModulePath::root(Name::new("types").unwrap()),
                name: Name::new("Amount").unwrap(),
            }),
            Type::Refined {
                base: Box::new(Type::Decimal),
                predicate: crate::hir::Predicate::Named(Name::new("positive").unwrap()),
            },
        ] {
            let package = manifest("settlement-capability");
            let schema = qualified_name("types", "SettlementState");
            let lock = lockfile(std::slice::from_ref(&package));
            let mut module =
                record_module("types", "SettlementState", &settlement_state_fields(), None);
            let DeclarationKind::Type {
                ty: Type::Record(Row::Closed(fields)),
            } = &mut module.declarations[0].kind
            else {
                panic!("fixture must lower to a closed record");
            };
            fields[0].ty = replacement;
            let input = PackageInput::new(package, [module])
                .with_schema_capability(schema, SchemaCapability::SettlementStateV1);
            assert!(matches!(
                compile([input], &lock),
                Err(PackageCompileError::InvalidSchemaCapability { .. })
            ));
        }

        let package = manifest("settlement-capability");
        let lock = lockfile(std::slice::from_ref(&package));
        let value = module_at(
            "types",
            "SettlementState",
            AstDeclarationKind::Value {
                ty: AstType::Text,
                expression: Some(crate::hir::AstExpression::Text("not-a-record".to_owned())),
            },
        );
        let input = PackageInput::new(package, [value]).with_schema_capability(
            qualified_name("types", "SettlementState"),
            SchemaCapability::SettlementStateV1,
        );
        assert!(matches!(
            compile([input], &lock),
            Err(PackageCompileError::InvalidSchemaCapability { .. })
        ));
    }

    #[test]
    fn capability_binding_order_is_not_identity() {
        let package = manifest("settlement-capability");
        let first_name = qualified_name("first", "SettlementState");
        let second_name = qualified_name("second", "SettlementState");
        let modules = [
            record_module("first", "SettlementState", &settlement_state_fields(), None),
            record_module(
                "second",
                "SettlementState",
                &settlement_state_fields(),
                None,
            ),
        ];
        let left = PackageInput::new(package.clone(), modules.clone())
            .with_schema_capability(first_name.clone(), SchemaCapability::SettlementStateV1)
            .with_schema_capability(second_name.clone(), SchemaCapability::SettlementStateV1);
        let right = PackageInput::new(package.clone(), modules)
            .with_schema_capability(second_name, SchemaCapability::SettlementStateV1)
            .with_schema_capability(first_name, SchemaCapability::SettlementStateV1);
        let lock = lockfile(std::slice::from_ref(&package));

        assert_eq!(left.input_hash(), right.input_hash());
        assert_eq!(
            compile([left], &lock).unwrap(),
            compile([right], &lock).unwrap()
        );
    }

    #[test]
    fn exports_are_scoped_by_package() {
        let first = manifest("first");
        let second = manifest("second");
        let lock = lockfile(&[first.clone(), second.clone()]);
        let artifact = compile(
            [
                PackageInput::new(
                    first,
                    [record_module(
                        "types",
                        "Payment",
                        &[("amount", AstType::Integer)],
                        None,
                    )],
                ),
                PackageInput::new(
                    second,
                    [record_module(
                        "types",
                        "Payment",
                        &[("amount", AstType::Text)],
                        None,
                    )],
                ),
            ],
            &lock,
        )
        .unwrap();
        let name = qualified_name("types", "Payment");
        let first_root = artifact
            .packages()
            .iter()
            .find(|package| package.name == "first")
            .unwrap()
            .root_hash();
        let second_root = artifact
            .packages()
            .iter()
            .find(|package| package.name == "second")
            .unwrap()
            .root_hash();

        let first_schema = artifact.resolve_record_schema(first_root, &name).unwrap();
        let second_schema = artifact.resolve_record_schema(second_root, &name).unwrap();
        assert_eq!(
            first_schema.qualified_name(),
            second_schema.qualified_name()
        );
        assert_ne!(first_schema.schema_id(), second_schema.schema_id());
        assert_ne!(first_schema.row(), second_schema.row());
    }

    #[test]
    fn duplicate_export_within_a_package_is_rejected() {
        let package = manifest("duplicate");
        let lock = lockfile(std::slice::from_ref(&package));
        let mut duplicate = module("entry", AstDeclarationKind::Type(AstType::Text));
        duplicate
            .declarations
            .push(duplicate.declarations[0].clone());

        assert!(matches!(
            compile([PackageInput::new(package, [duplicate])], &lock),
            Err(PackageCompileError::DuplicateExport { .. })
        ));
    }

    #[test]
    fn verification_binds_manifest_and_lockfile() {
        let package = manifest("bound");
        let lock = lockfile(std::slice::from_ref(&package));
        let input = PackageInput::new(
            package.clone(),
            [module("value", AstDeclarationKind::Type(AstType::Text))],
        );
        let artifact = compile([input.clone()], &lock).unwrap();
        artifact.verify([input.clone()], &lock).unwrap();

        let changed_manifest = PackageManifest::new("bound", Version::new(1, 0, 0), "changed");
        let changed_input = PackageInput::new(
            changed_manifest,
            [module("value", AstDeclarationKind::Type(AstType::Text))],
        );
        assert!(matches!(
            artifact.verify([changed_input], &lock),
            Err(PackageCompileError::Lockfile(_))
        ));

        let changed_lock = Lockfile {
            roots: lock.roots.clone(),
            packages: vec![LockedPackage {
                name: package.name.clone(),
                version: package.version,
                hash: ContentHash::ZERO,
                dependencies: Vec::new(),
            }],
        };
        assert!(artifact.verify([input], &changed_lock).is_err());
    }

    #[test]
    fn record_schema_resolution_is_root_qualified_and_supports_same_short_names() {
        let first = manifest("first");
        let second = manifest("second");
        let lock = lockfile(&[first.clone(), second.clone()]);
        let artifact = compile(
            [
                PackageInput::new(
                    first,
                    [record_module(
                        "first_types",
                        "Record",
                        &[("amount", AstType::Integer)],
                        None,
                    )],
                ),
                PackageInput::new(
                    second,
                    [record_module(
                        "second_types",
                        "Record",
                        &[("amount", AstType::Text)],
                        None,
                    )],
                ),
            ],
            &lock,
        )
        .unwrap();

        let first_root = artifact
            .packages()
            .iter()
            .find(|package| package.name == "first")
            .unwrap()
            .root_hash();
        let second_root = artifact
            .packages()
            .iter()
            .find(|package| package.name == "second")
            .unwrap()
            .root_hash();
        let first_name = qualified_name("first_types", "Record");
        let second_name = qualified_name("second_types", "Record");
        let first_schema = artifact
            .resolve_record_schema(first_root, &first_name)
            .unwrap();
        let second_schema = artifact
            .resolve_record_schema(second_root, &second_name)
            .unwrap();

        assert!(!first_schema.row().is_open());
        assert!(!second_schema.row().is_open());
        assert_ne!(first_schema.schema_id(), second_schema.schema_id());
        assert!(matches!(
            artifact.resolve_record_schema(second_root, &first_name),
            Err(RecordSchemaError::UnknownExport { .. })
        ));
    }

    #[test]
    fn record_schema_rejects_unknown_root_export_and_non_record() {
        let package = manifest("schemas");
        let lock = lockfile(std::slice::from_ref(&package));
        let artifact = compile(
            [PackageInput::new(
                package,
                [
                    record_module("types", "Record", &[("amount", AstType::Integer)], None),
                    module("Scalar", AstDeclarationKind::Type(AstType::Text)),
                ],
            )],
            &lock,
        )
        .unwrap();
        let root = artifact.package_roots()[0];

        assert!(matches!(
            artifact.resolve_record_schema(
                ContentHash::domain_separated("test/missing-root", b"missing"),
                &qualified_name("types", "Record")
            ),
            Err(RecordSchemaError::UnknownPackageRoot { .. })
        ));
        assert!(matches!(
            artifact.resolve_record_schema(root, &qualified_name("types", "Missing")),
            Err(RecordSchemaError::UnknownExport { .. })
        ));
        assert!(matches!(
            artifact.resolve_record_schema(root, &qualified_name("shared", "Scalar")),
            Err(RecordSchemaError::NotRecord { .. })
        ));
    }

    #[test]
    fn record_schema_id_tracks_complete_type_context_and_rows() {
        let make = |path: &str, fields: &[(&str, AstType)], open_tail| {
            let package = manifest("schemas");
            let lock = lockfile(std::slice::from_ref(&package));
            let artifact = compile(
                [PackageInput::new(
                    package,
                    [record_module(path, "Record", fields, open_tail)],
                )],
                &lock,
            )
            .unwrap();
            let root = artifact.package_roots()[0];
            let name = qualified_name(path, "Record");
            let schema = artifact.resolve_record_schema(root, &name).unwrap();
            (artifact, schema)
        };

        let (base_artifact, base) = make("types", &[("amount", AstType::Integer)], None);
        let (_, changed_field) = make("types", &[("amount", AstType::Text)], None);
        let (_, open) = make("types", &[("amount", AstType::Integer)], Some(7));
        let (_, changed_module) = make("other_types", &[("amount", AstType::Integer)], None);
        assert!(!base.row().is_open());
        assert!(open.row().is_open());
        assert_ne!(base.schema_id(), changed_field.schema_id());
        assert_ne!(base.schema_id(), open.schema_id());
        assert_ne!(base.schema_id(), changed_module.schema_id());
        assert_ne!(base_artifact.artifact_hash(), changed_field.schema_id());
    }

    #[test]
    fn record_schema_resolution_and_ids_are_order_invariant() {
        let package = manifest("ordered");
        let lock = lockfile(std::slice::from_ref(&package));
        let first = compile(
            [PackageInput::new(
                package.clone(),
                [
                    record_module("a_types", "Record", &[("a", AstType::Integer)], None),
                    module_at("b_types", "Other", AstDeclarationKind::Type(AstType::Bool)),
                ],
            )],
            &lock,
        )
        .unwrap();
        let second = compile(
            [PackageInput::new(
                package,
                [
                    module_at("b_types", "Other", AstDeclarationKind::Type(AstType::Bool)),
                    record_module("a_types", "Record", &[("a", AstType::Integer)], None),
                ],
            )],
            &lock,
        )
        .unwrap();
        let name = qualified_name("a_types", "Record");
        let first_schema = first
            .resolve_record_schema(first.package_roots()[0], &name)
            .unwrap();
        let second_schema = second
            .resolve_record_schema(second.package_roots()[0], &name)
            .unwrap();
        assert_eq!(first.artifact_hash(), second.artifact_hash());
        assert_eq!(first_schema.schema_id(), second_schema.schema_id());
        assert_eq!(
            first_schema.row(),
            &Row::Closed(first_schema.row().fields().to_vec())
        );
    }

    fn direct_schema(row: Row) -> RecordSchema {
        RecordSchema {
            package_root: ContentHash::domain_separated("test/package-root", b"records"),
            qualified_name: qualified_name("types", "Record"),
            row,
            capability: None,
            schema_id: ContentHash::domain_separated("test/schema", b"records"),
        }
    }

    #[test]
    fn checked_records_are_order_invariant_and_schema_bound() {
        let schema = direct_schema(Row::Closed(vec![
            crate::hir::Field {
                name: Name::new("amount").unwrap(),
                ty: Type::Integer,
            },
            crate::hir::Field {
                name: Name::new("note").unwrap(),
                ty: Type::Text,
            },
        ]));
        let left = crate::ir::Record::closed([
            ("note", crate::ir::Term::Text("ok".to_owned())),
            (
                "amount",
                crate::ir::Term::Integer(num_bigint::BigInt::from(7)),
            ),
        ]);
        let right = crate::ir::Record::closed([
            (
                "amount",
                crate::ir::Term::Integer(num_bigint::BigInt::from(7)),
            ),
            ("note", crate::ir::Term::Text("ok".to_owned())),
        ]);
        let first = schema.check_concrete_record(&left).unwrap();
        let second = schema.check_concrete_record(&right).unwrap();
        assert_eq!(first.record(), second.record());
        assert_eq!(first.content_hash(), second.content_hash());
        assert_eq!(first.schema_id(), schema.schema_id());

        let mut other_schema = schema.clone();
        other_schema.schema_id = ContentHash::domain_separated("test/schema", b"other");
        assert_ne!(
            first.content_hash(),
            other_schema
                .check_concrete_record(&left)
                .unwrap()
                .content_hash()
        );
    }

    #[test]
    fn schema_bound_records_reject_missing_extra_and_wrong_numeric_fields() {
        let schema = direct_schema(Row::Closed(vec![crate::hir::Field {
            name: Name::new("amount").unwrap(),
            ty: Type::Integer,
        }]));
        let missing = crate::ir::Record::closed(std::iter::empty::<(&str, crate::ir::Term)>());
        assert!(matches!(
            schema.check_concrete_record(&missing),
            Err(RecordValueError::MissingField { .. })
        ));
        let extra = crate::ir::Record::closed([
            (
                "amount",
                crate::ir::Term::Integer(num_bigint::BigInt::from(1)),
            ),
            ("surprise", crate::ir::Term::Bool(true)),
        ]);
        assert!(matches!(
            schema.check_concrete_record(&extra),
            Err(RecordValueError::UnexpectedField { .. })
        ));
        let decimal = crate::ir::Record::closed([(
            "amount",
            crate::ir::Term::Decimal(num_rational::BigRational::from_integer(
                num_bigint::BigInt::from(1),
            )),
        )]);
        assert!(matches!(
            schema.check_concrete_record(&decimal),
            Err(RecordValueError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn decimal_schema_normalizes_integer_and_decimal_spellings_to_one_value() {
        let schema = direct_schema(Row::Closed(vec![crate::hir::Field {
            name: Name::new("amount").unwrap(),
            ty: Type::Decimal,
        }]));
        let integer = crate::ir::Record::closed([("amount", crate::ir::Term::integer(5))]);
        let decimal = crate::ir::Record::closed([(
            "amount",
            crate::ir::Term::decimal(num_rational::BigRational::from_integer(
                num_bigint::BigInt::from(5),
            )),
        )]);

        let integer = schema.check_concrete_record(&integer).unwrap();
        let decimal = schema.check_concrete_record(&decimal).unwrap();
        assert_eq!(integer.record(), decimal.record());
        assert_eq!(integer.content_hash(), decimal.content_hash());

        let invalid = crate::ir::Record::closed([(
            "amount",
            crate::ir::Term::Decimal(num_rational::BigRational::new_raw(
                num_bigint::BigInt::from(1),
                num_bigint::BigInt::from(0),
            )),
        )]);
        assert!(matches!(
            schema.check_concrete_record(&invalid),
            Err(RecordValueError::InvalidDecimal { .. })
        ));
    }

    #[test]
    fn checked_records_handle_nested_open_rows_and_typed_holes() {
        let schema = direct_schema(Row::Open {
            fields: vec![
                crate::hir::Field {
                    name: Name::new("child").unwrap(),
                    ty: Type::Record(Row::Closed(vec![crate::hir::Field {
                        name: Name::new("label").unwrap(),
                        ty: Type::Text,
                    }])),
                },
                crate::hir::Field {
                    name: Name::new("hole").unwrap(),
                    ty: Type::Text,
                },
            ],
            tail: TypeVar(0),
        });
        let record = crate::ir::Record::closed([
            (
                "child",
                crate::ir::Term::Record(crate::ir::Record::closed([(
                    "label",
                    crate::ir::Term::Text("nested".to_owned()),
                )])),
            ),
            (
                "hole",
                crate::ir::Term::Var(crate::ir::Var::hole(4, "value", crate::ir::Sort::Text)),
            ),
            ("extension", crate::ir::Term::Bool(true)),
        ]);
        assert!(schema.check_concrete_record(&record).is_ok());

        let bad_hole = crate::ir::Record::closed([
            (
                "child",
                crate::ir::Term::Record(crate::ir::Record::closed([(
                    "label",
                    crate::ir::Term::Text("nested".to_owned()),
                )])),
            ),
            ("hole", crate::ir::Term::Var(crate::ir::Var::inference(4))),
        ]);
        assert!(matches!(
            schema.check_concrete_record(&bad_hole),
            Err(RecordValueError::UntypedHole { .. })
        ));

        let any_hole = crate::ir::Record::closed([
            (
                "child",
                crate::ir::Term::Record(crate::ir::Record::closed([(
                    "label",
                    crate::ir::Term::Text("nested".to_owned()),
                )])),
            ),
            (
                "hole",
                crate::ir::Term::Var(crate::ir::Var::hole(5, "untyped", crate::ir::Sort::Any)),
            ),
        ]);
        assert!(matches!(
            schema.check_concrete_record(&any_hole),
            Err(RecordValueError::HoleSortMismatch { .. })
        ));

        let record_hole = crate::ir::Record::closed([
            (
                "child",
                crate::ir::Term::Var(crate::ir::Var::hole(6, "record", crate::ir::Sort::Record)),
            ),
            ("hole", crate::ir::Term::Text("resolved".to_owned())),
        ]);
        assert!(matches!(
            schema.check_concrete_record(&record_hole),
            Err(RecordValueError::RecordHoleNeedsSchema { .. })
        ));
    }

    #[test]
    fn checked_records_reject_aliases_functions_and_invalid_tails_explicitly() {
        let alias = direct_schema(Row::Closed(vec![crate::hir::Field {
            name: Name::new("value").unwrap(),
            ty: Type::Named(qualified_name("types", "Alias")),
        }]));
        let value = crate::ir::Record::closed([("value", crate::ir::Term::Text("x".into()))]);
        assert!(matches!(
            alias.check_concrete_record(&value),
            Err(RecordValueError::UnsupportedType { .. })
        ));

        let open = direct_schema(Row::Open {
            fields: vec![],
            tail: TypeVar(0),
        });
        let invalid_tail = crate::ir::Record::open(
            std::iter::empty::<(&str, crate::ir::Term)>(),
            crate::ir::Var::inference(1),
        );
        assert!(matches!(
            open.check_concrete_record(&invalid_tail),
            Err(RecordValueError::InvalidRowTail { .. })
        ));
    }
}
