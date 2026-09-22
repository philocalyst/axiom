//! Deterministic, content-addressed package resolution.
//!
//! This module is deliberately smaller than a general package manager.  A
//! manifest is immutable data, versions are strict semver-like values, and a
//! resolver produces a canonical lockfile that can be checked without
//! consulting the resolver again.  There is no network or ambient filesystem
//! state in this layer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::ContentHash;

const MANIFEST_DOMAIN: &str = "axiom/package-manifest/v1";
const LOCKFILE_DOMAIN: &str = "axiom/package-lock/v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(source: &str) -> Result<Self, PackageLockError> {
        let source = source.trim();
        let mut parts = source.split('.');
        let major = parse_component(parts.next(), source)?;
        let minor = parse_component(parts.next(), source)?;
        let patch = parse_component(parts.next(), source)?;
        if parts.next().is_some() {
            return Err(PackageLockError::InvalidVersion(source.to_owned()));
        }
        Ok(Self::new(major, minor, patch))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum VersionReq {
    Any,
    Exact(Version),
    Caret(Version),
    Tilde(Version),
}

impl VersionReq {
    pub fn parse(source: &str) -> Result<Self, PackageLockError> {
        let source = source.trim();
        if source == "*" || source.eq_ignore_ascii_case("any") {
            return Ok(Self::Any);
        }
        if let Some(version) = source.strip_prefix('^') {
            return Ok(Self::Caret(Version::parse(version)?));
        }
        if let Some(version) = source.strip_prefix('~') {
            return Ok(Self::Tilde(Version::parse(version)?));
        }
        if let Some(version) = source.strip_prefix('=') {
            return Ok(Self::Exact(Version::parse(version)?));
        }
        Ok(Self::Exact(Version::parse(source)?))
    }

    pub fn matches(&self, version: Version) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => *expected == version,
            Self::Caret(base) if base.major > 0 => version >= *base && version.major == base.major,
            Self::Caret(base) if base.minor > 0 => {
                version >= *base && version.major == 0 && version.minor == base.minor
            }
            Self::Caret(base) => version == *base,
            Self::Tilde(base) => {
                version >= *base && version.major == base.major && version.minor == base.minor
            }
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => formatter.write_str("*"),
            Self::Exact(version) => write!(formatter, "={version}"),
            Self::Caret(version) => write!(formatter, "^{version}"),
            Self::Tilde(version) => write!(formatter, "~{version}"),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Dependency {
    pub name: String,
    pub requirement: VersionReq,
}

impl Dependency {
    pub fn new(name: impl Into<String>, requirement: VersionReq) -> Self {
        Self {
            name: name.into(),
            requirement,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageManifest {
    pub name: String,
    pub version: Version,
    pub body: String,
    pub dependencies: Vec<Dependency>,
}

impl PackageManifest {
    pub fn new(name: impl Into<String>, version: Version, body: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version,
            body: body.into(),
            dependencies: Vec::new(),
        }
    }

    pub fn with_dependencies(mut self, dependencies: impl IntoIterator<Item = Dependency>) -> Self {
        self.dependencies = dependencies.into_iter().collect();
        self.dependencies.sort();
        self.dependencies.dedup();
        self
    }

    pub fn validate(&self) -> Result<(), PackageLockError> {
        if self.name.trim().is_empty()
            || self.name.chars().any(char::is_whitespace)
            || self.name.contains("::")
        {
            return Err(PackageLockError::InvalidName(self.name.clone()));
        }
        if self.body.trim().is_empty() {
            return Err(PackageLockError::EmptyBody(self.name.clone()));
        }
        if self.dependencies.iter().any(|dependency| {
            dependency.name.trim().is_empty()
                || dependency.name.chars().any(char::is_whitespace)
                || dependency.name.contains("::")
        }) {
            return Err(PackageLockError::InvalidDependency(self.name.clone()));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        put_text(&mut bytes, &self.name);
        put_text(&mut bytes, &self.version.to_string());
        put_text(&mut bytes, &self.body);
        let mut dependencies = self.dependencies.clone();
        dependencies.sort();
        put_u64(&mut bytes, dependencies.len());
        for dependency in dependencies {
            put_text(&mut bytes, &dependency.name);
            put_text(&mut bytes, &dependency.requirement.to_string());
        }
        bytes
    }

    pub fn hash(&self) -> ContentHash {
        ContentHash::domain_separated(MANIFEST_DOMAIN, &self.canonical_bytes())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageLockError {
    InvalidName(String),
    InvalidVersion(String),
    EmptyBody(String),
    InvalidDependency(String),
    DuplicatePackage(String),
    PackageVersionConflict {
        name: String,
        version: Version,
    },
    MissingPackage(String),
    NoMatchingVersion {
        name: String,
        requirements: Vec<String>,
    },
    IncompatibleRequirements {
        name: String,
        version: Version,
        requirement: String,
    },
    DependencyCycle(Vec<String>),
    DuplicateLockEntry(String),
    LockHashMismatch {
        name: String,
        expected: ContentHash,
        actual: ContentHash,
    },
    LockDependencyMismatch {
        name: String,
        dependency: String,
    },
    UnexpectedLockEntry(String),
    LockRootMismatch,
}

impl fmt::Display for PackageLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(formatter, "invalid package name `{name}`"),
            Self::InvalidVersion(version) => write!(formatter, "invalid version `{version}`"),
            Self::EmptyBody(name) => write!(formatter, "package `{name}` has an empty body"),
            Self::InvalidDependency(name) => {
                write!(formatter, "package `{name}` has an invalid dependency")
            }
            Self::DuplicatePackage(name) => {
                write!(formatter, "package `{name}` is registered twice")
            }
            Self::PackageVersionConflict { name, version } => write!(
                formatter,
                "package `{name}@{version}` has conflicting content"
            ),
            Self::MissingPackage(name) => write!(formatter, "package `{name}` is missing"),
            Self::NoMatchingVersion { name, requirements } => write!(
                formatter,
                "no version of `{name}` matches {}",
                requirements.join(", ")
            ),
            Self::IncompatibleRequirements {
                name,
                version,
                requirement,
            } => write!(
                formatter,
                "selected `{name}@{version}` does not satisfy `{requirement}`"
            ),
            Self::DependencyCycle(path) => {
                write!(formatter, "dependency cycle: {}", path.join(" -> "))
            }
            Self::DuplicateLockEntry(name) => write!(formatter, "lockfile repeats `{name}`"),
            Self::LockHashMismatch {
                name,
                expected,
                actual,
            } => write!(
                formatter,
                "lock hash for `{name}` is {actual}, expected {expected}"
            ),
            Self::LockDependencyMismatch { name, dependency } => write!(
                formatter,
                "lock dependency `{dependency}` is not the resolved dependency of `{name}`"
            ),
            Self::UnexpectedLockEntry(name) => {
                write!(formatter, "lockfile contains unreachable package `{name}`")
            }
            Self::LockRootMismatch => formatter.write_str("lockfile roots do not match"),
        }
    }
}

impl std::error::Error for PackageLockError {}

#[derive(Clone, Debug, Default)]
pub struct PackageRegistry {
    packages: BTreeMap<String, BTreeMap<Version, PackageManifest>>,
}

impl PackageRegistry {
    pub fn insert(&mut self, package: PackageManifest) -> Result<ContentHash, PackageLockError> {
        package.validate()?;
        let hash = package.hash();
        let versions = self.packages.entry(package.name.clone()).or_default();
        if let Some(existing) = versions.get(&package.version) {
            if existing.hash() != hash {
                return Err(PackageLockError::PackageVersionConflict {
                    name: package.name,
                    version: package.version,
                });
            }
            return Ok(hash);
        }
        versions.insert(package.version, package);
        Ok(hash)
    }

    pub fn get(&self, name: &str, version: Version) -> Option<&PackageManifest> {
        self.packages.get(name)?.get(&version)
    }

    fn candidates(&self, name: &str, requirements: &[VersionReq]) -> Vec<&PackageManifest> {
        self.packages
            .get(name)
            .into_iter()
            .flat_map(|versions| versions.values())
            .filter(|package| {
                requirements
                    .iter()
                    .all(|requirement| requirement.matches(package.version))
            })
            .rev()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockedPackage {
    pub name: String,
    pub version: Version,
    pub hash: ContentHash,
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lockfile {
    pub roots: Vec<Dependency>,
    pub packages: Vec<LockedPackage>,
}

impl Lockfile {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut roots = self.roots.clone();
        roots.sort();
        put_u64(&mut bytes, roots.len());
        for root in roots {
            put_dependency(&mut bytes, &root);
        }
        let mut packages = self.packages.clone();
        packages.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.version.cmp(&right.version))
        });
        put_u64(&mut bytes, packages.len());
        for package in packages {
            put_text(&mut bytes, &package.name);
            put_text(&mut bytes, &package.version.to_string());
            bytes.extend_from_slice(package.hash.as_bytes());
            let mut dependencies = package.dependencies.clone();
            dependencies.sort();
            put_u64(&mut bytes, dependencies.len());
            for dependency in dependencies {
                put_dependency(&mut bytes, &dependency);
            }
        }
        bytes
    }

    pub fn hash(&self) -> ContentHash {
        ContentHash::domain_separated(LOCKFILE_DOMAIN, &self.canonical_bytes())
    }

    pub fn verify(&self, registry: &PackageRegistry) -> Result<(), PackageLockError> {
        let mut entries = BTreeMap::new();
        for entry in &self.packages {
            if entries.insert(entry.name.clone(), entry).is_some() {
                return Err(PackageLockError::DuplicateLockEntry(entry.name.clone()));
            }
        }
        for entry in &self.packages {
            let package = registry
                .get(&entry.name, entry.version)
                .ok_or_else(|| PackageLockError::MissingPackage(entry.name.clone()))?;
            let actual = package.hash();
            if actual != entry.hash {
                return Err(PackageLockError::LockHashMismatch {
                    name: entry.name.clone(),
                    expected: entry.hash,
                    actual,
                });
            }
            let mut expected = package.dependencies.clone();
            expected.sort();
            let mut actual_dependencies = entry.dependencies.clone();
            actual_dependencies.sort();
            if expected != actual_dependencies {
                return Err(PackageLockError::LockDependencyMismatch {
                    name: entry.name.clone(),
                    dependency: format!("expected {expected:?}, got {actual_dependencies:?}"),
                });
            }
            for dependency in &entry.dependencies {
                let Some(dependency_entry) = entries.get(&dependency.name) else {
                    return Err(PackageLockError::LockDependencyMismatch {
                        name: entry.name.clone(),
                        dependency: dependency.name.clone(),
                    });
                };
                if !dependency.requirement.matches(dependency_entry.version) {
                    return Err(PackageLockError::LockDependencyMismatch {
                        name: entry.name.clone(),
                        dependency: dependency.name.clone(),
                    });
                }
            }
        }
        for root in &self.roots {
            let Some(entry) = entries.get(&root.name) else {
                return Err(PackageLockError::LockRootMismatch);
            };
            if !root.requirement.matches(entry.version) {
                return Err(PackageLockError::LockRootMismatch);
            }
        }
        let mut visited = BTreeSet::new();
        for root in &self.roots {
            verify_lock_closure(&root.name, &entries, &mut Vec::new(), &mut visited)?;
        }
        if let Some(unreachable) = entries.keys().find(|name| !visited.contains(*name)) {
            return Err(PackageLockError::UnexpectedLockEntry(unreachable.clone()));
        }
        Ok(())
    }
}

fn verify_lock_closure(
    name: &str,
    entries: &BTreeMap<String, &LockedPackage>,
    active: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), PackageLockError> {
    if let Some(start) = active.iter().position(|entry| entry == name) {
        let mut cycle = active[start..].to_vec();
        cycle.push(name.to_owned());
        return Err(PackageLockError::DependencyCycle(cycle));
    }
    if visited.contains(name) {
        return Ok(());
    }
    active.push(name.to_owned());
    let entry = entries
        .get(name)
        .ok_or(PackageLockError::LockRootMismatch)?;
    for dependency in &entry.dependencies {
        verify_lock_closure(&dependency.name, entries, active, visited)?;
    }
    active.pop();
    visited.insert(name.to_owned());
    Ok(())
}

pub fn resolve(
    registry: &PackageRegistry,
    roots: impl IntoIterator<Item = Dependency>,
) -> Result<Lockfile, PackageLockError> {
    let mut roots = roots.into_iter().collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    let mut constraints = BTreeMap::<String, Vec<VersionReq>>::new();
    for root in &roots {
        constraints
            .entry(root.name.clone())
            .or_default()
            .push(root.requirement.clone());
    }
    let mut selected = BTreeMap::new();
    solve(registry, &mut constraints, &mut selected)?;
    detect_cycles(registry, &selected)?;
    let packages = selected
        .into_iter()
        .map(|(name, version)| {
            let package = registry
                .get(&name, version)
                .expect("resolver selected a registered package");
            LockedPackage {
                name,
                version,
                hash: package.hash(),
                dependencies: package.dependencies.clone(),
            }
        })
        .collect();
    let lockfile = Lockfile { roots, packages };
    lockfile.verify(registry)?;
    Ok(lockfile)
}

fn solve(
    registry: &PackageRegistry,
    constraints: &mut BTreeMap<String, Vec<VersionReq>>,
    selected: &mut BTreeMap<String, Version>,
) -> Result<(), PackageLockError> {
    for (name, version) in selected.iter() {
        if let Some(requirements) = constraints.get(name)
            && let Some(requirement) = requirements
                .iter()
                .find(|requirement| !requirement.matches(*version))
        {
            return Err(PackageLockError::IncompatibleRequirements {
                name: name.clone(),
                version: *version,
                requirement: requirement.to_string(),
            });
        }
    }
    let next = constraints
        .keys()
        .find(|name| !selected.contains_key(*name))
        .cloned();
    let Some(name) = next else {
        return Ok(());
    };
    let requirements = constraints.get(&name).cloned().unwrap_or_default();
    let candidates = registry.candidates(&name, &requirements);
    if candidates.is_empty() {
        if registry.packages.contains_key(&name) {
            return Err(PackageLockError::NoMatchingVersion {
                name,
                requirements: requirements.iter().map(ToString::to_string).collect(),
            });
        }
        return Err(PackageLockError::MissingPackage(name));
    }
    let mut last_error = None;
    for candidate in candidates {
        selected.insert(name.clone(), candidate.version);
        let mut added = Vec::new();
        for dependency in &candidate.dependencies {
            constraints
                .entry(dependency.name.clone())
                .or_default()
                .push(dependency.requirement.clone());
            added.push(dependency.name.clone());
        }
        match solve(registry, constraints, selected) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        selected.remove(&name);
        for dependency_name in added {
            let requirements = constraints
                .get_mut(&dependency_name)
                .expect("added constraint");
            requirements.pop();
            if requirements.is_empty() {
                constraints.remove(&dependency_name);
            }
        }
    }
    Err(
        last_error.unwrap_or_else(|| PackageLockError::NoMatchingVersion {
            name,
            requirements: requirements.iter().map(ToString::to_string).collect(),
        }),
    )
}

fn detect_cycles(
    registry: &PackageRegistry,
    selected: &BTreeMap<String, Version>,
) -> Result<(), PackageLockError> {
    fn visit(
        name: &str,
        registry: &PackageRegistry,
        selected: &BTreeMap<String, Version>,
        active: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), PackageLockError> {
        if let Some(start) = active.iter().position(|entry| entry == name) {
            let mut cycle = active[start..].to_vec();
            cycle.push(name.to_owned());
            return Err(PackageLockError::DependencyCycle(cycle));
        }
        if !done.insert(name.to_owned()) {
            return Ok(());
        }
        active.push(name.to_owned());
        let version = *selected.get(name).expect("selected dependency");
        let package = registry.get(name, version).expect("selected package");
        for dependency in &package.dependencies {
            visit(&dependency.name, registry, selected, active, done)?;
        }
        active.pop();
        Ok(())
    }
    let mut done = BTreeSet::new();
    for name in selected.keys() {
        visit(name, registry, selected, &mut Vec::new(), &mut done)?;
    }
    Ok(())
}

fn parse_component(component: Option<&str>, source: &str) -> Result<u64, PackageLockError> {
    let component = component.ok_or_else(|| PackageLockError::InvalidVersion(source.to_owned()))?;
    if component.is_empty() || (component.len() > 1 && component.starts_with('0')) {
        return Err(PackageLockError::InvalidVersion(source.to_owned()));
    }
    component
        .parse()
        .map_err(|_| PackageLockError::InvalidVersion(source.to_owned()))
}

fn put_u64(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_be_bytes());
}
fn put_text(bytes: &mut Vec<u8>, text: &str) {
    put_u64(bytes, text.len());
    bytes.extend_from_slice(text.as_bytes());
}
fn put_dependency(bytes: &mut Vec<u8>, dependency: &Dependency) {
    put_text(bytes, &dependency.name);
    put_text(bytes, &dependency.requirement.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, version: &str, dependencies: &[(&str, &str)]) -> PackageManifest {
        PackageManifest::new(name, Version::parse(version).unwrap(), "body").with_dependencies(
            dependencies.iter().map(|(name, requirement)| {
                Dependency::new(*name, VersionReq::parse(requirement).unwrap())
            }),
        )
    }

    #[test]
    fn package_names_cannot_steal_the_schema_separator() {
        assert!(matches!(
            package("billing::shadow", "1.0.0", &[]).validate(),
            Err(PackageLockError::InvalidName(_))
        ));
        assert!(matches!(
            package("billing", "1.0.0", &[("types::shadow", "=1.0.0")]).validate(),
            Err(PackageLockError::InvalidDependency(_))
        ));
    }

    #[test]
    fn versions_and_ranges_are_strict_and_deterministic() {
        assert!(Version::parse("1.2").is_err());
        assert!(
            VersionReq::parse("^1.2.3")
                .unwrap()
                .matches(Version::new(1, 9, 0))
        );
        assert!(
            !VersionReq::parse("^1.2.3")
                .unwrap()
                .matches(Version::new(2, 0, 0))
        );
        assert!(
            VersionReq::parse("~0.2.3")
                .unwrap()
                .matches(Version::new(0, 2, 99))
        );
        assert!(
            !VersionReq::parse("~0.2.3")
                .unwrap()
                .matches(Version::new(0, 3, 0))
        );
        let maximum = Version::new(u64::MAX, u64::MAX, u64::MAX);
        assert!(VersionReq::Caret(maximum).matches(maximum));
        assert!(VersionReq::Tilde(maximum).matches(maximum));
    }

    #[test]
    fn resolver_picks_highest_compatible_version_and_verifies_lock() {
        let mut registry = PackageRegistry::default();
        registry.insert(package("leaf", "1.0.0", &[])).unwrap();
        registry.insert(package("leaf", "1.2.0", &[])).unwrap();
        registry
            .insert(package("root", "1.0.0", &[("leaf", "^1.0.0")]))
            .unwrap();
        let lock = resolve(
            &registry,
            [Dependency::new(
                "root",
                VersionReq::parse("=1.0.0").unwrap(),
            )],
        )
        .unwrap();
        assert_eq!(
            lock.packages
                .iter()
                .find(|entry| entry.name == "leaf")
                .unwrap()
                .version,
            Version::new(1, 2, 0)
        );
        assert!(lock.verify(&registry).is_ok());
        assert_ne!(lock.hash(), ContentHash::ZERO);
    }

    #[test]
    fn verification_is_independent_of_package_and_dependency_name_order() {
        let mut registry = PackageRegistry::default();
        registry.insert(package("z-leaf", "1.0.0", &[])).unwrap();
        registry
            .insert(package("a-root", "1.0.0", &[("z-leaf", "=1.0.0")]))
            .unwrap();

        let mut lock = resolve(
            &registry,
            [Dependency::new(
                "a-root",
                VersionReq::parse("=1.0.0").unwrap(),
            )],
        )
        .unwrap();
        lock.packages.reverse();

        assert!(lock.verify(&registry).is_ok());
    }

    #[test]
    fn verification_rejects_packages_outside_the_root_closure_and_cycles() {
        let mut registry = PackageRegistry::default();
        registry.insert(package("root", "1.0.0", &[])).unwrap();
        let extra = package("extra", "1.0.0", &[]);
        registry.insert(extra.clone()).unwrap();
        let mut lock = resolve(
            &registry,
            [Dependency::new(
                "root",
                VersionReq::parse("=1.0.0").unwrap(),
            )],
        )
        .unwrap();
        lock.packages.push(LockedPackage {
            name: extra.name.clone(),
            version: extra.version,
            hash: extra.hash(),
            dependencies: extra.dependencies.clone(),
        });
        assert!(matches!(
            lock.verify(&registry),
            Err(PackageLockError::UnexpectedLockEntry(name)) if name == "extra"
        ));

        let mut cyclic_registry = PackageRegistry::default();
        let a = package("a", "1.0.0", &[("b", "=1.0.0")]);
        let b = package("b", "1.0.0", &[("a", "=1.0.0")]);
        cyclic_registry.insert(a.clone()).unwrap();
        cyclic_registry.insert(b.clone()).unwrap();
        let cyclic = Lockfile {
            roots: vec![Dependency::new("a", VersionReq::parse("=1.0.0").unwrap())],
            packages: vec![
                LockedPackage {
                    name: a.name.clone(),
                    version: a.version,
                    hash: a.hash(),
                    dependencies: a.dependencies.clone(),
                },
                LockedPackage {
                    name: b.name.clone(),
                    version: b.version,
                    hash: b.hash(),
                    dependencies: b.dependencies.clone(),
                },
            ],
        };
        assert!(matches!(
            cyclic.verify(&cyclic_registry),
            Err(PackageLockError::DependencyCycle(_))
        ));
    }

    #[test]
    fn missing_and_incompatible_dependencies_are_explicit() {
        let mut registry = PackageRegistry::default();
        registry
            .insert(package("root", "1.0.0", &[("missing", "*")]))
            .unwrap();
        assert!(
            matches!(resolve(&registry, [Dependency::new("root", VersionReq::parse("*").unwrap())]), Err(PackageLockError::MissingPackage(name)) if name == "missing")
        );
        let mut registry = PackageRegistry::default();
        registry.insert(package("a", "1.0.0", &[])).unwrap();
        registry
            .insert(package("root", "1.0.0", &[("a", "^2.0.0")]))
            .unwrap();
        assert!(
            matches!(resolve(&registry, [Dependency::new("root", VersionReq::parse("*").unwrap())]), Err(PackageLockError::NoMatchingVersion { name, .. }) if name == "a")
        );
    }

    #[test]
    fn cycles_are_rejected_and_manifest_hash_binds_dependencies() {
        let mut registry = PackageRegistry::default();
        registry
            .insert(package("a", "1.0.0", &[("b", "*")]))
            .unwrap();
        registry
            .insert(package("b", "1.0.0", &[("a", "*")]))
            .unwrap();
        assert!(matches!(
            resolve(
                &registry,
                [Dependency::new("a", VersionReq::parse("*").unwrap())]
            ),
            Err(PackageLockError::DependencyCycle(_))
        ));
        assert_ne!(
            package("a", "1.0.0", &[]).hash(),
            package("a", "1.0.0", &[("b", "*")]).hash()
        );
    }

    #[test]
    fn duplicate_same_content_is_idempotent_but_conflicting_content_is_rejected() {
        let mut registry = PackageRegistry::default();
        let one = package("a", "1.0.0", &[]);
        registry.insert(one.clone()).unwrap();
        registry.insert(one).unwrap();
        let conflict = PackageManifest::new("a", Version::new(1, 0, 0), "different");
        assert!(matches!(
            registry.insert(conflict),
            Err(PackageLockError::PackageVersionConflict { .. })
        ));
    }
}
