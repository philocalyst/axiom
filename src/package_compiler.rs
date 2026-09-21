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
const RECORD_SCHEMA_DOMAIN: &str = "axiom/package-record-schema/v1";

/// The input to one package compilation unit.
///
/// The manifest body is deliberately not interpreted here.  It contributes to
/// the manifest hash through [`PackageManifest::hash`], while the typed HIR is
/// supplied independently by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInput {
    pub manifest: PackageManifest,
    pub modules: Vec<Module>,
}

impl PackageInput {
    pub fn new(manifest: PackageManifest, modules: impl IntoIterator<Item = Module>) -> Self {
        Self {
            manifest,
            modules: modules.into_iter().collect(),
        }
    }

    pub fn manifest_hash(&self) -> ContentHash {
        self.manifest.hash()
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
        ContentHash::domain_separated("axiom/package-input/v1", &bytes)
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
}

impl CompiledPackage {
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
        ContentHash::domain_separated("axiom/package-root/v1", &bytes)
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

        Ok(RecordSchema {
            package_root,
            qualified_name: qualified_name.clone(),
            row: row.clone(),
            schema_id: record_schema_id(
                self.artifact_hash,
                package_root,
                qualified_name,
                &export.kind,
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

    pub fn schema_id(&self) -> ContentHash {
        self.schema_id
    }
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

        compiled_packages.push(CompiledPackage {
            name: input.manifest.name.clone(),
            version: input.manifest.version,
            manifest_hash: input.manifest.hash(),
            modules: compiled_modules,
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
    ContentHash::domain_separated(
        ARTIFACT_DOMAIN,
        &artifact_bytes(lockfile_hash, packages, exports),
    )
}

fn record_schema_id(
    artifact_hash: ContentHash,
    package_root: ContentHash,
    qualified_name: &QualifiedName,
    declaration: &DeclarationKind,
) -> ContentHash {
    let mut bytes = Vec::new();
    put_text(&mut bytes, "record-schema");
    bytes.extend_from_slice(artifact_hash.as_bytes());
    bytes.extend_from_slice(package_root.as_bytes());
    put_text(&mut bytes, &qualified_name.canonical());
    bytes.extend_from_slice(&declaration_kind_bytes(declaration));
    ContentHash::domain_separated(RECORD_SCHEMA_DOMAIN, &bytes)
}

fn artifact_bytes(
    lockfile_hash: ContentHash,
    packages: &[CompiledPackage],
    exports: &[CompiledExport],
) -> Vec<u8> {
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
}
