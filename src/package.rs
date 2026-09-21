//! Content-addressed, executable policy packages.
//!
//! A policy is data first. The kernel knows how to validate and execute the
//! deliberately small lot-selection language below, but it does not select a
//! rule by matching a package name. This is the important boundary: a new
//! package can be registered without adding another privileged engine branch,
//! while a package outside the supported fragment fails explicitly.

use core::fmt;
use std::collections::BTreeMap;

use crate::model::{ContentHash, Date};

pub const POLICY_DOMAIN: &str = "axiom/policy/v1";
pub const PACKAGE_DOMAIN: &str = "axiom/package/v2";

/// The only policy fragment currently executable by the V0 engine.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AcquisitionOrder {
    Earliest,
    Latest,
}

/// A canonical, typed lot-selection program emitted by [`PolicyPackage::compile`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SelectionProgram {
    order: AcquisitionOrder,
}

impl SelectionProgram {
    pub const fn order(self) -> AcquisitionOrder {
        self.order
    }

    pub fn canonical_body(self) -> &'static str {
        match self.order {
            AcquisitionOrder::Earliest => "selector=earliest_acquisition\ntie=ambiguous",
            AcquisitionOrder::Latest => "selector=latest_acquisition\ntie=ambiguous",
        }
    }

    pub fn evaluate<'a, I>(self, candidates: I) -> Selection
    where
        I: IntoIterator<Item = &'a LotCandidate>,
    {
        let mut candidates = candidates.into_iter().collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.id.cmp(&right.id))
        });
        let Some(extreme) = (match self.order {
            AcquisitionOrder::Earliest => candidates.first(),
            AcquisitionOrder::Latest => candidates.last(),
        }) else {
            return Selection::None;
        };
        let tied = candidates
            .iter()
            .filter(|candidate| candidate.date == extreme.date)
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        if tied.len() == 1 {
            Selection::Unique(tied[0].clone())
        } else {
            Selection::Ambiguous(tied)
        }
    }

    /// Return candidates in the deterministic acquisition order implemented by
    /// this compiled program.  `evaluate` remains the single-answer API used
    /// by callers that need the package's explicit tie rule; allocation uses
    /// this ordered view after checking that the policy has a unique extreme.
    pub fn ordered<'a, I>(self, candidates: I) -> Vec<LotCandidate>
    where
        I: IntoIterator<Item = &'a LotCandidate>,
    {
        let mut candidates = candidates.into_iter().cloned().collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.date
                .cmp(&right.date)
                .then_with(|| left.id.cmp(&right.id))
        });
        if matches!(self.order, AcquisitionOrder::Latest) {
            candidates.reverse();
        }
        candidates
    }
}

impl fmt::Display for SelectionProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.canonical_body())
    }
}

/// The minimum candidate view needed by the selection evaluator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LotCandidate {
    pub id: String,
    pub date: Date,
}

impl LotCandidate {
    pub fn new(id: impl Into<String>, date: Date) -> Self {
        Self {
            id: id.into(),
            date,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    None,
    Unique(String),
    Ambiguous(Vec<String>),
}

/// Errors are deliberately split between malformed syntax and a well-formed
/// but unsupported future feature. Callers must not turn either into a hidden
/// fallback to FIFO.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageError {
    EmptyName,
    EmptyVersion,
    InvalidName(String),
    EmptyBody,
    DuplicateField(String),
    MissingField(String),
    MalformedField(String),
    UnsupportedSelector(String),
    UnsupportedTie(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("policy package name is empty"),
            Self::EmptyVersion => formatter.write_str("policy package version is empty"),
            Self::InvalidName(name) => write!(formatter, "invalid policy package name `{name}`"),
            Self::EmptyBody => formatter.write_str("policy package body is empty"),
            Self::DuplicateField(field) => {
                write!(formatter, "policy package field `{field}` is repeated")
            }
            Self::MissingField(field) => {
                write!(formatter, "policy package field `{field}` is missing")
            }
            Self::MalformedField(field) => {
                write!(formatter, "malformed policy package field `{field}`")
            }
            Self::UnsupportedSelector(selector) => {
                write!(formatter, "unsupported policy selector `{selector}`")
            }
            Self::UnsupportedTie(tie) => {
                write!(formatter, "unsupported policy tie rule `{tie}`")
            }
        }
    }
}

impl std::error::Error for PackageError {}

/// A single executable package. `body` is a tiny canonical manifest body,
/// not Rust code. Dependencies are included in the package address even
/// though V0's selector fragment does not yet consume them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyPackage {
    pub name: String,
    pub version: String,
    pub body: String,
    pub dependencies: Vec<ContentHash>,
}

/// Accepted source forms for an executable package body.
///
/// Raw bytes deliberately are not accepted here.  A stored package may carry
/// arbitrary bytes so that malformed input remains inspectable, but crossing
/// into the executable representation must be lossless and therefore requires
/// an explicit UTF-8 conversion in the store/workspace boundary.
pub trait BodySource {
    fn into_body(self) -> String;
}

impl BodySource for String {
    fn into_body(self) -> String {
        self
    }
}

impl BodySource for &str {
    fn into_body(self) -> String {
        self.to_owned()
    }
}

impl PolicyPackage {
    pub fn new<B: BodySource>(
        name: impl Into<String>,
        version: impl Into<String>,
        body: B,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            body: body.into_body(),
            dependencies: Vec::new(),
        }
    }

    pub fn with_dependencies(
        mut self,
        dependencies: impl IntoIterator<Item = ContentHash>,
    ) -> Self {
        self.dependencies = dependencies.into_iter().collect();
        self.dependencies.sort();
        self.dependencies.dedup();
        self
    }

    /// Validate package metadata and body syntax without compiling it.
    pub fn validate(&self) -> Result<(), PackageError> {
        self.validate_identity()?;
        self.parse_body().map(|_| ())
    }

    /// Validate the stable package identity while leaving body diagnostics to
    /// the compiler. This lets malformed community packages remain visible as
    /// blocking ledger evidence instead of disappearing at the store edge.
    pub fn validate_identity(&self) -> Result<(), PackageError> {
        if self.name.trim().is_empty() {
            return Err(PackageError::EmptyName);
        }
        if self.version.trim().is_empty() {
            return Err(PackageError::EmptyVersion);
        }
        if self.name.chars().any(|character| character.is_whitespace()) {
            return Err(PackageError::InvalidName(self.name.clone()));
        }
        Ok(())
    }

    /// Compile the canonical body to the typed evaluator used by `engine`.
    pub fn compile(&self) -> Result<SelectionProgram, PackageError> {
        self.validate()?;
        self.parse_body()
    }

    /// Canonical package bytes bind metadata, dependencies, and canonical body
    /// in one stable representation. Hashing malformed packages remains
    /// deterministic; validation is the separate acceptance gate.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        put_text(&mut bytes, self.name.trim());
        put_text(&mut bytes, self.version.trim());
        let mut dependencies = self.dependencies.clone();
        dependencies.sort();
        dependencies.dedup();
        put_hashes(&mut bytes, &dependencies);
        put_text(&mut bytes, &canonical_body_text(&self.body));
        bytes
    }

    /// Return the stable textual body identity used by [`Self::canonical_bytes`].
    /// Storage uses this only after a package has crossed the validated public
    /// boundary; raw malformed package bodies remain untouched in the store so
    /// they can still be diagnosed by internal tests.
    pub(crate) fn canonical_body_text(&self) -> String {
        canonical_body_text(&self.body)
    }

    pub fn hash(&self) -> ContentHash {
        ContentHash::domain_separated(PACKAGE_DOMAIN, &self.canonical_bytes())
    }

    fn parse_body(&self) -> Result<SelectionProgram, PackageError> {
        let mut fields = BTreeMap::<String, String>::new();
        for line in self.body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(PackageError::MalformedField(line.to_owned()));
            };
            let key = key.trim().to_owned();
            let value = value.trim().to_owned();
            if key.is_empty() || value.is_empty() {
                return Err(PackageError::MalformedField(line.to_owned()));
            }
            if fields.insert(key.clone(), value).is_some() {
                return Err(PackageError::DuplicateField(key));
            }
        }
        if fields.is_empty() {
            return Err(PackageError::EmptyBody);
        }
        let selector = fields
            .remove("selector")
            .ok_or_else(|| PackageError::MissingField("selector".into()))?;
        let tie = fields
            .remove("tie")
            .ok_or_else(|| PackageError::MissingField("tie".into()))?;
        if let Some((key, _)) = fields.into_iter().next() {
            return Err(PackageError::MalformedField(key));
        }
        if tie != "ambiguous" {
            return Err(PackageError::UnsupportedTie(tie));
        }
        let order = match selector.as_str() {
            "earliest_acquisition" => AcquisitionOrder::Earliest,
            "latest_acquisition" => AcquisitionOrder::Latest,
            other => return Err(PackageError::UnsupportedSelector(other.to_owned())),
        };
        Ok(SelectionProgram { order })
    }
}

/// A deterministic name registry. The registry stores raw packages so a
/// caller can diagnose malformed/unsupported community input through the same
/// explicit error path as unknown names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyRegistry {
    packages: BTreeMap<String, PolicyPackage>,
}

impl PolicyRegistry {
    pub fn builtins() -> Self {
        let mut registry = Self::default();
        for package in builtin_policies() {
            registry.insert(package);
        }
        registry
    }

    pub fn insert(&mut self, package: PolicyPackage) {
        self.packages.insert(package.name.clone(), package);
    }

    pub fn get(&self, name: &str) -> Option<&PolicyPackage> {
        self.packages.get(name)
    }

    pub fn packages(&self) -> impl Iterator<Item = &PolicyPackage> {
        self.packages.values()
    }
}

pub fn builtin_policies() -> Vec<PolicyPackage> {
    vec![
        PolicyPackage::new(
            "lots/fifo",
            "0",
            "selector=earliest_acquisition\ntie=ambiguous",
        ),
        PolicyPackage::new(
            "lots/lifo",
            "0",
            "selector=latest_acquisition\ntie=ambiguous",
        ),
    ]
}

pub fn builtin_policy(name: &str) -> Option<PolicyPackage> {
    builtin_policies()
        .into_iter()
        .find(|package| package.name == name)
}

pub fn builtin_policy_hash(name: &str) -> Option<ContentHash> {
    builtin_policy(name).map(|package| package.hash())
}

fn canonical_body_text(body: &str) -> String {
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n")
}

fn put_text(bytes: &mut Vec<u8>, text: &str) {
    let length = u64::try_from(text.len()).expect("policy package text is too large");
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn put_hashes(bytes: &mut Vec<u8>, hashes: &[ContentHash]) {
    let length = u64::try_from(hashes.len()).expect("policy package has too many dependencies");
    bytes.extend_from_slice(&length.to_be_bytes());
    for hash in hashes {
        bytes.extend_from_slice(hash.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u8) -> Date {
        Date::new(2026, 1, day).unwrap()
    }

    #[test]
    fn builtins_compile_through_the_same_program() {
        let fifo = builtin_policy("lots/fifo").unwrap().compile().unwrap();
        let lifo = builtin_policy("lots/lifo").unwrap().compile().unwrap();
        assert_eq!(fifo.order(), AcquisitionOrder::Earliest);
        assert_eq!(lifo.order(), AcquisitionOrder::Latest);
    }

    #[test]
    fn selection_is_source_order_invariant_and_ties_are_explicit() {
        let program = builtin_policy("lots/lifo").unwrap().compile().unwrap();
        let left = vec![
            LotCandidate::new("two", date(2)),
            LotCandidate::new("one", date(1)),
        ];
        let right = vec![
            LotCandidate::new("one", date(1)),
            LotCandidate::new("two", date(2)),
        ];
        assert_eq!(program.evaluate(&left), Selection::Unique("two".into()));
        assert_eq!(program.evaluate(&right), Selection::Unique("two".into()));
        let tie = vec![
            LotCandidate::new("b", date(2)),
            LotCandidate::new("a", date(2)),
        ];
        assert_eq!(
            program.evaluate(&tie),
            Selection::Ambiguous(vec!["a".into(), "b".into()])
        );
        assert_eq!(
            program
                .ordered(&[
                    LotCandidate::new("one", date(1)),
                    LotCandidate::new("two", date(2)),
                ])
                .into_iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec!["two", "one"]
        );
    }

    #[test]
    fn canonical_hash_ignores_body_line_and_dependency_order() {
        let one = ContentHash::domain_separated("test", b"one");
        let two = ContentHash::domain_separated("test", b"two");
        let left = PolicyPackage::new("lots/x", "1", "tie=ambiguous\nselector=latest_acquisition")
            .with_dependencies([one, two]);
        let right = PolicyPackage::new("lots/x", "1", "selector=latest_acquisition\ntie=ambiguous")
            .with_dependencies([two, one]);
        assert_eq!(left.hash(), right.hash());
    }

    #[test]
    fn malformed_and_unsupported_packages_are_not_fallbacks() {
        assert!(matches!(
            PolicyPackage::new("lots/x", "1", "selector=latest_acquisition").compile(),
            Err(PackageError::MissingField(_))
        ));
        assert!(matches!(
            PolicyPackage::new("lots/x", "1", "selector=cost\ntie=ambiguous").compile(),
            Err(PackageError::UnsupportedSelector(_))
        ));
    }
}
