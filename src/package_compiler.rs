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

    /// Recompute the artifact hash from the public artifact contents.
    pub fn recomputed_hash(&self) -> ContentHash {
        artifact_hash(self.lockfile_hash, &self.packages, &self.exports)
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
    DuplicateExportedName {
        qualified_name: String,
        first_package: String,
        second_package: String,
    },
    IncompatibleDeclaration {
        qualified_name: String,
        first_package: String,
        second_package: String,
        first_kind: Box<DeclarationKind>,
        second_kind: Box<DeclarationKind>,
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
            Self::DuplicateExportedName {
                qualified_name,
                first_package,
                second_package,
            } => write!(
                formatter,
                "export `{qualified_name}` is defined by both `{first_package}` and `{second_package}`"
            ),
            Self::IncompatibleDeclaration {
                qualified_name,
                first_package,
                second_package,
                ..
            } => write!(
                formatter,
                "export `{qualified_name}` has incompatible declarations in `{first_package}` and `{second_package}`"
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
    let mut exports = BTreeMap::<String, CompiledExport>::new();

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
                content_id: module.content_id(),
            });

            let mut declarations = module.declarations.clone();
            declarations.sort_by(declaration_order);
            for declaration in declarations {
                let qualified_name = QualifiedName {
                    module: module.path.clone(),
                    name: declaration.name.clone(),
                };
                let key = qualified_name.canonical();
                let export = CompiledExport {
                    package: input.manifest.name.clone(),
                    qualified_name,
                    kind: declaration.kind.clone(),
                };
                if let Some(previous) = exports.get(&key) {
                    if compatible_declaration_kinds(&previous.kind, &export.kind) {
                        return Err(PackageCompileError::DuplicateExportedName {
                            qualified_name: key,
                            first_package: previous.package.clone(),
                            second_package: export.package,
                        });
                    }
                    return Err(PackageCompileError::IncompatibleDeclaration {
                        qualified_name: key,
                        first_package: previous.package.clone(),
                        second_package: export.package,
                        first_kind: Box::new(previous.kind.clone()),
                        second_kind: Box::new(export.kind),
                    });
                }
                exports.insert(key, export);
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
        .map(|module| (module.path.canonical(), module.content_id()))
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn module_order(left: &Module, right: &Module) -> std::cmp::Ordering {
    left.path
        .canonical()
        .cmp(&right.path.canonical())
        .then(left.content_id().cmp(&right.content_id()))
}

fn declaration_order(left: &Declaration, right: &Declaration) -> std::cmp::Ordering {
    left.name
        .cmp(&right.name)
        .then(declaration_kind_bytes(&left.kind).cmp(&declaration_kind_bytes(&right.kind)))
}

fn compatible_declaration_kinds(left: &DeclarationKind, right: &DeclarationKind) -> bool {
    match (left, right) {
        (DeclarationKind::Type { ty: left }, DeclarationKind::Type { ty: right }) => left == right,
        (DeclarationKind::Value { ty: left, .. }, DeclarationKind::Value { ty: right, .. }) => {
            left == right
        }
        (
            DeclarationKind::Rule {
                input: left_input,
                output: left_output,
            },
            DeclarationKind::Rule {
                input: right_input,
                output: right_output,
            },
        ) => left_input == right_input && left_output == right_output,
        _ => false,
    }
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

    ContentHash::domain_separated(ARTIFACT_DOMAIN, &bytes)
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
    use crate::hir::{AstDeclaration, AstDeclarationKind, AstModule, AstType, Name, Span};
    use crate::package_lock::{Dependency, LockedPackage, VersionReq};

    fn manifest(name: &str) -> PackageManifest {
        PackageManifest::new(name, Version::new(1, 0, 0), "hir-package")
    }

    fn module(name: &str, kind: AstDeclarationKind) -> Module {
        crate::hir::lower(AstModule {
            path: ModulePath::root(Name::new("shared").unwrap()),
            declarations: vec![AstDeclaration {
                name: name.to_owned(),
                kind,
                span: Span::default(),
            }],
        })
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
    fn duplicate_and_incompatible_exports_are_rejected() {
        let first = manifest("first");
        let second = manifest("second");
        let lock = lockfile(&[first.clone(), second.clone()]);
        let same = |kind: AstDeclarationKind| {
            compile(
                [
                    PackageInput::new(first.clone(), [module("entry", kind.clone())]),
                    PackageInput::new(second.clone(), [module("entry", kind)]),
                ],
                &lock,
            )
        };
        assert!(matches!(
            same(AstDeclarationKind::Type(AstType::Text)),
            Err(PackageCompileError::DuplicateExportedName { .. })
        ));

        assert!(matches!(
            same(AstDeclarationKind::Type(AstType::Bool)),
            Err(PackageCompileError::DuplicateExportedName { .. })
        ));

        let incompatible = compile(
            [
                PackageInput::new(
                    first,
                    [module("entry", AstDeclarationKind::Type(AstType::Text))],
                ),
                PackageInput::new(
                    second,
                    [module(
                        "entry",
                        AstDeclarationKind::Value {
                            ty: AstType::Text,
                            expression: None,
                        },
                    )],
                ),
            ],
            &lock,
        );
        assert!(matches!(
            incompatible,
            Err(PackageCompileError::IncompatibleDeclaration { .. })
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
}
