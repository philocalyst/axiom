//! Content-addressed package metadata.
//!
//! Packages are data, not privileged Rust implementations.  A definition or
//! policy can be authored by the community and pinned by its canonical hash;
//! the kernel only needs to know the domain and dependency roots.  Definition
//! and policy hashes use different domains so one can never be replayed as the
//! other kind by accident.

use core::fmt;

pub use crate::model::ContentHash;

pub type Hash = ContentHash;

pub const DEFINITION_DOMAIN: &str = "axiom/definition/v1";
pub const POLICY_DOMAIN: &str = "axiom/policy/v1";
pub const PACKAGE_DOMAIN: &str = "axiom/package/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestKind {
    Definition,
    Policy,
}

impl ManifestKind {
    pub const fn domain(self) -> &'static str {
        match self {
            Self::Definition => DEFINITION_DOMAIN,
            Self::Policy => POLICY_DOMAIN,
        }
    }
}

/// A named definition and its canonical semantic content.
///
/// The name is useful to humans; the hash, including dependencies, is what a
/// lock file and proof should refer to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionManifest {
    pub name: String,
    pub version: String,
    pub definition: String,
    pub dependencies: Vec<ContentHash>,
}

impl DefinitionManifest {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        definition: impl Into<String>,
        dependencies: impl IntoIterator<Item = ContentHash>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            definition: definition.into(),
            dependencies: dependencies.into_iter().collect(),
        }
    }

    pub fn hash(&self) -> ContentHash {
        hash_manifest(
            ManifestKind::Definition,
            &self.name,
            &self.version,
            &self.definition,
            &self.dependencies,
        )
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_manifest_bytes(
            &self.name,
            &self.version,
            &self.definition,
            &self.dependencies,
        )
    }
}

/// A policy is content-addressed in exactly the same way as a definition, but
/// in a separate hash domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyManifest {
    pub name: String,
    pub version: String,
    pub definition: String,
    pub dependencies: Vec<ContentHash>,
}

impl PolicyManifest {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        definition: impl Into<String>,
        dependencies: impl IntoIterator<Item = ContentHash>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            definition: definition.into(),
            dependencies: dependencies.into_iter().collect(),
        }
    }

    pub fn hash(&self) -> ContentHash {
        hash_manifest(
            ManifestKind::Policy,
            &self.name,
            &self.version,
            &self.definition,
            &self.dependencies,
        )
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_manifest_bytes(
            &self.name,
            &self.version,
            &self.definition,
            &self.dependencies,
        )
    }
}

/// The package root binds its children in sorted hash order.  Reordering a
/// source manifest cannot create a new package identity; changing any child
/// or its declared metadata always does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub definitions: Vec<DefinitionManifest>,
    pub policies: Vec<PolicyManifest>,
}

impl PackageManifest {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            definitions: Vec::new(),
            policies: Vec::new(),
        }
    }

    pub fn with_definition(mut self, definition: DefinitionManifest) -> Self {
        self.definitions.push(definition);
        self
    }

    pub fn with_policy(mut self, policy: PolicyManifest) -> Self {
        self.policies.push(policy);
        self
    }

    pub fn hash(&self) -> ContentHash {
        let mut definitions: Vec<_> = self
            .definitions
            .iter()
            .map(DefinitionManifest::hash)
            .collect();
        let mut policies: Vec<_> = self.policies.iter().map(PolicyManifest::hash).collect();
        definitions.sort();
        policies.sort();
        let mut bytes = Vec::new();
        put_text(&mut bytes, &self.name);
        put_text(&mut bytes, &self.version);
        put_hashes(&mut bytes, &definitions);
        put_hashes(&mut bytes, &policies);
        ContentHash::domain_separated(PACKAGE_DOMAIN, &bytes)
    }
}

pub fn builtin_policies() -> Vec<PolicyManifest> {
    vec![PolicyManifest::new(
        "lots/fifo",
        "0",
        "select the earliest eligible acquisition lot; unresolved ties remain multiple",
        [],
    )]
}

pub fn builtin_policy(name: &str) -> Option<PolicyManifest> {
    builtin_policies()
        .into_iter()
        .find(|manifest| manifest.name == name)
}

pub fn builtin_policy_hash(name: &str) -> Option<ContentHash> {
    builtin_policy(name).map(|manifest| manifest.hash())
}

/// Hash arbitrary canonical bytes in one of the package domains.
pub fn hash_definition(kind: ManifestKind, bytes: &[u8]) -> ContentHash {
    ContentHash::domain_separated(kind.domain(), bytes)
}

fn hash_manifest(
    kind: ManifestKind,
    name: &str,
    version: &str,
    definition: &str,
    dependencies: &[ContentHash],
) -> ContentHash {
    let bytes = canonical_manifest_bytes(name, version, definition, dependencies);
    ContentHash::domain_separated(kind.domain(), &bytes)
}

fn canonical_manifest_bytes(
    name: &str,
    version: &str,
    definition: &str,
    dependencies: &[ContentHash],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    put_text(&mut bytes, name);
    put_text(&mut bytes, version);
    put_text(&mut bytes, definition);
    let mut dependencies = dependencies.to_vec();
    dependencies.sort();
    put_hashes(&mut bytes, &dependencies);
    bytes
}

fn put_text(bytes: &mut Vec<u8>, text: &str) {
    let length = u64::try_from(text.len()).expect("manifest text is too large");
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn put_hashes(bytes: &mut Vec<u8>, hashes: &[ContentHash]) {
    let length = u64::try_from(hashes.len()).expect("manifest has too many dependencies");
    bytes.extend_from_slice(&length.to_be_bytes());
    for hash in hashes {
        bytes.extend_from_slice(hash.as_bytes());
    }
}

impl fmt::Display for ManifestKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Definition => "definition",
            Self::Policy => "policy",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_order_is_not_semantic() {
        let one = ContentHash::domain_separated("test", b"one");
        let two = ContentHash::domain_separated("test", b"two");
        let left = DefinitionManifest::new("x", "1", "body", [one, two]);
        let right = DefinitionManifest::new("x", "1", "body", [two, one]);
        assert_eq!(left.hash(), right.hash());
    }

    #[test]
    fn domains_keep_definition_and_policy_hashes_apart() {
        let definition = DefinitionManifest::new("same", "1", "body", []).hash();
        let policy = PolicyManifest::new("same", "1", "body", []).hash();
        assert_ne!(definition, policy);
    }

    #[test]
    fn fifo_is_a_regular_content_addressed_policy() {
        assert!(builtin_policy_hash("lots/fifo").is_some());
    }
}
