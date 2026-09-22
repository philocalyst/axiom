//! The small, finance-native logical intermediate representation.
//!
//! This module deliberately contains no solver policy.  It is the value layer
//! shared by the logical coordinator and the theory implementations: nominal
//! symbols keep economic identity explicit, quantities keep their units, and
//! open records give extensions a place to live without making the kernel a
//! closed enum.

use std::collections::BTreeMap;
use std::fmt;

use blake3::Hasher;
use num_bigint::{BigInt, Sign};
use num_rational::BigRational;
use num_traits::Zero;

/// A UTF-8 symbol.  Symbols are compared by their bytes and are therefore
/// stable across processes and platforms.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Symbol {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Symbol {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The namespace of a nominal symbol is semantic, not merely cosmetic.
/// `instrument/USD` and `entity/USD` are distinct values even though their
/// printed names happen to agree.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NominalKind {
    Entity,
    Instrument,
    Contract,
    Event,
    Lot,
    Account,
    Venue,
    Policy,
    Predicate,
    Annotation,
    Custom(Symbol),
}

impl NominalKind {
    fn tag(&self) -> (&'static str, Option<&Symbol>) {
        match self {
            Self::Entity => ("entity", None),
            Self::Instrument => ("instrument", None),
            Self::Contract => ("contract", None),
            Self::Event => ("event", None),
            Self::Lot => ("lot", None),
            Self::Account => ("account", None),
            Self::Venue => ("venue", None),
            Self::Policy => ("policy", None),
            Self::Predicate => ("predicate", None),
            Self::Annotation => ("annotation", None),
            Self::Custom(name) => ("custom", Some(name)),
        }
    }
}

/// A typed, optionally namespaced nominal identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Nominal {
    pub kind: NominalKind,
    pub namespace: Option<Symbol>,
    pub name: Symbol,
}

impl Nominal {
    pub fn new(kind: NominalKind, name: impl Into<Symbol>) -> Self {
        Self {
            kind,
            namespace: None,
            name: name.into(),
        }
    }

    pub fn namespaced(
        kind: NominalKind,
        namespace: impl Into<Symbol>,
        name: impl Into<Symbol>,
    ) -> Self {
        Self {
            kind,
            namespace: Some(namespace.into()),
            name: name.into(),
        }
    }
}

impl fmt::Display for Nominal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}/", self.kind)?;
        if let Some(namespace) = &self.namespace {
            write!(formatter, "{namespace}/")?;
        }
        formatter.write_str(self.name.as_str())
    }
}

/// A unit is nominal by design.  A currency, security, hour, or energy unit
/// cannot unify with another unit merely because its spelling is similar.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Unit(Nominal);

impl Unit {
    pub fn new(name: impl Into<Symbol>) -> Self {
        Self(Nominal::new(NominalKind::Instrument, name))
    }

    pub fn nominal(nominal: Nominal) -> Self {
        Self(nominal)
    }

    pub fn as_nominal(&self) -> &Nominal {
        &self.0
    }

    fn canonical_name(&self) -> String {
        let (kind, custom) = self.0.kind.tag();
        format!(
            "{}:{}:{}:{}",
            kind,
            custom.map(Symbol::as_str).unwrap_or(""),
            self.0.namespace.as_ref().map(Symbol::as_str).unwrap_or(""),
            self.0.name
        )
    }
}

impl From<Nominal> for Unit {
    fn from(value: Nominal) -> Self {
        Self::nominal(value)
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.name.as_str())
    }
}

/// An arbitrary-precision exact quantity.  A unit-less quantity is legal only
/// for zero; it is the polymorphic-zero literal used by the type checker.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExactQuantity {
    value: BigRational,
    unit: Option<Unit>,
}

impl ExactQuantity {
    pub fn new(value: BigRational, unit: Option<Unit>) -> Result<Self, QuantityError> {
        if value.denom().is_zero() {
            return Err(QuantityError::ZeroDenominator);
        }
        if value.is_zero() || unit.is_some() {
            Ok(Self { value, unit })
        } else {
            Err(QuantityError::UnitRequired)
        }
    }

    pub fn integer(value: impl Into<BigInt>, unit: impl Into<Unit>) -> Self {
        Self {
            value: BigRational::from_integer(value.into()),
            unit: Some(unit.into()),
        }
    }

    pub fn rational(
        numerator: impl Into<BigInt>,
        denominator: impl Into<BigInt>,
        unit: impl Into<Unit>,
    ) -> Result<Self, QuantityError> {
        let denominator = denominator.into();
        if denominator.is_zero() {
            return Err(QuantityError::ZeroDenominator);
        }
        Self::new(
            BigRational::new(numerator.into(), denominator),
            Some(unit.into()),
        )
    }

    pub fn zero() -> Self {
        Self {
            value: BigRational::zero(),
            unit: None,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }

    pub fn value(&self) -> &BigRational {
        &self.value
    }

    pub fn unit(&self) -> Option<&Unit> {
        self.unit.as_ref()
    }

    /// Lower this IR-only value into the canonical domain quantity.  The
    /// conversion is checked and crate-private so IR remains an implementation
    /// representation rather than a second public arithmetic API.
    pub(crate) fn to_model_quantity(&self) -> Result<crate::model::Quantity, QuantityError> {
        let value = crate::exact::ExactNumber::parse(&format!(
            "{}/{}",
            self.value.numer(),
            self.value.denom()
        ))
        .map_err(|error| QuantityError::InvalidExact(error.to_string()))?;
        let unit = self
            .unit
            .as_ref()
            .map(|unit| crate::model::Unit::new(unit.canonical_name()))
            .transpose()
            .map_err(|error| QuantityError::InvalidExact(error.to_string()))?;
        crate::model::Quantity::new(value, unit)
            .map_err(|error| QuantityError::InvalidExact(error.to_string()))
    }

    /// Stable, human-independent numeric encoding.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_bytes(&mut bytes, self.value.numer().to_string().as_bytes());
        write_bytes(&mut bytes, self.value.denom().to_string().as_bytes());
        match &self.unit {
            Some(unit) => {
                bytes.push(1);
                write_nominal(&mut bytes, unit.as_nominal());
            }
            None => bytes.push(0),
        }
        bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuantityError {
    UnitRequired,
    ZeroDenominator,
    InvalidExact(String),
}

impl fmt::Display for QuantityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitRequired => formatter.write_str("a nonzero quantity requires a unit"),
            Self::ZeroDenominator => formatter.write_str("a quantity denominator cannot be zero"),
            Self::InvalidExact(error) => write!(formatter, "cannot lower exact quantity: {error}"),
        }
    }
}

impl std::error::Error for QuantityError {}

/// Inference variables carry a kind and an optional expected sort.  Names are
/// only diagnostic sugar; canonicalization intentionally ignores them.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Var {
    pub id: u32,
    pub kind: VarKind,
    pub name: Option<Symbol>,
    pub sort: Sort,
}

impl Var {
    pub fn inference(id: u32) -> Self {
        Self {
            id,
            kind: VarKind::Inference,
            name: None,
            sort: Sort::Any,
        }
    }

    pub fn named(id: u32, name: impl Into<Symbol>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::inference(id)
        }
    }

    pub fn hole(id: u32, name: impl Into<Symbol>, sort: Sort) -> Self {
        Self {
            id,
            kind: VarKind::Hole,
            name: Some(name.into()),
            sort,
        }
    }

    pub fn row(id: u32) -> Self {
        Self {
            id,
            kind: VarKind::Row,
            name: None,
            sort: Sort::Row,
        }
    }

    pub fn is_hole(&self) -> bool {
        self.kind == VarKind::Hole
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VarKind {
    Inference,
    Hole,
    Row,
}

/// A deliberately small sort language.  Sorts constrain what a hole or
/// typed inference variable may be bound to; `Any` is the gradual escape hatch.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Sort {
    Any,
    Bool,
    Text,
    Integer,
    Decimal,
    Nominal(Option<NominalKind>),
    Unit(Option<Unit>),
    Quantity(Option<Unit>),
    Record,
    Row,
    Atom,
    Goal,
}

/// A record is an extensible row: explicit fields plus an optional tail row
/// variable.  Field order is canonicalized by `BTreeMap`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub fields: BTreeMap<Symbol, Term>,
    pub rest: Option<Var>,
}

impl Record {
    pub fn closed<I, K>(fields: I) -> Self
    where
        I: IntoIterator<Item = (K, Term)>,
        K: Into<Symbol>,
    {
        Self {
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            rest: None,
        }
    }

    pub fn open<I, K>(fields: I, rest: Var) -> Self
    where
        I: IntoIterator<Item = (K, Term)>,
        K: Into<Symbol>,
    {
        Self {
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            rest: Some(rest),
        }
    }

    pub fn field(&self, name: impl Into<Symbol>) -> Option<&Term> {
        self.fields.get(&name.into())
    }
}

/// Terms are intentionally finite and boring.  Rich economic constructs are
/// represented by nominal predicates and records rather than kernel enums.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Term {
    Var(Var),
    Nominal(Nominal),
    Unit(Unit),
    Quantity(ExactQuantity),
    /// An exact integer scalar.  Numeric literals stay arbitrary precision at
    /// the IR boundary; there is no lossy machine-number escape hatch.
    Integer(BigInt),
    /// An exact decimal scalar.  The current HIR carries decimal values as
    /// normalized rationals; retaining a distinct term keeps the author's
    /// declared decimal sort visible to schema checking.
    Decimal(BigRational),
    Record(Record),
    Tuple(Vec<Term>),
    App {
        constructor: Symbol,
        arguments: Vec<Term>,
    },
    Bool(bool),
    Text(String),
}

impl Term {
    pub fn var(variable: Var) -> Self {
        Self::Var(variable)
    }

    pub fn nominal(nominal: Nominal) -> Self {
        Self::Nominal(nominal)
    }

    pub fn quantity(quantity: ExactQuantity) -> Self {
        debug_assert!(quantity.to_model_quantity().is_ok());
        Self::Quantity(quantity)
    }

    pub fn integer(value: impl Into<BigInt>) -> Self {
        Self::Integer(value.into())
    }

    pub fn decimal(value: BigRational) -> Self {
        Self::Decimal(normalize_rational(&value))
    }

    pub fn record(record: Record) -> Self {
        Self::Record(record)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Atom {
    pub predicate: Nominal,
    pub arguments: Vec<Term>,
}

impl Atom {
    pub fn new(predicate: Nominal, arguments: Vec<Term>) -> Self {
        Self {
            predicate,
            arguments,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Goal {
    True,
    False,
    Atom(Atom),
    Equal(Term, Term),
    NotEqual(Term, Term),
    And(Vec<Goal>),
    Or(Vec<Goal>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Clause {
    pub head: Atom,
    pub body: Goal,
}

impl Clause {
    pub fn new(head: Atom, body: Goal) -> Self {
        Self { head, body }
    }
}

/// Context that prevents a cached logical answer from crossing semantic
/// worlds or policy versions.  The goal's canonical bytes remain context-free;
/// the context is included in the content hash.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CanonicalContext {
    pub accepted_world: Option<[u8; 32]>,
    pub package_hashes: Vec<[u8; 32]>,
    pub book: Option<Symbol>,
    pub scenario: Option<Symbol>,
    pub semantics_version: u32,
}

/// A canonical value and the hash used as an incremental/cache key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Canonical<T> {
    pub value: T,
    pub bytes: Vec<u8>,
    pub hash: [u8; 32],
}

pub type CanonicalGoal = Canonical<Goal>;
pub type CanonicalClause = Canonical<Clause>;

pub fn canonicalize_term(term: &Term, context: &CanonicalContext) -> Canonical<Term> {
    let mut canonicalizer = Canonicalizer::default();
    let value = canonicalizer.term(term);
    let bytes = canonical_bytes(&value);
    finish(value, bytes, context)
}

pub fn canonicalize_atom(atom: &Atom, context: &CanonicalContext) -> Canonical<Atom> {
    let mut canonicalizer = Canonicalizer::default();
    let value = canonicalizer.atom(atom);
    let bytes = canonical_bytes(&value);
    finish(value, bytes, context)
}

pub fn canonicalize_goal(goal: &Goal, context: &CanonicalContext) -> CanonicalGoal {
    let mut canonicalizer = Canonicalizer::default();
    let value = canonicalizer.goal(goal);
    let bytes = canonical_bytes(&value);
    finish(value, bytes, context)
}

pub fn canonicalize_clause(clause: &Clause, context: &CanonicalContext) -> CanonicalClause {
    let mut canonicalizer = Canonicalizer::default();
    let value = canonicalizer.clause(clause);
    let bytes = canonical_bytes(&value);
    finish(value, bytes, context)
}

fn finish<T>(value: T, bytes: Vec<u8>, context: &CanonicalContext) -> Canonical<T> {
    let mut hasher = Hasher::new();
    hasher.update(b"axiom/canonical/v1\0");
    let context_bytes = canonical_context_bytes(context);
    hasher.update(&context_bytes);
    hasher.update(&bytes);
    Canonical {
        value,
        bytes,
        hash: *hasher.finalize().as_bytes(),
    }
}

fn canonical_context_bytes(context: &CanonicalContext) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&context.semantics_version.to_be_bytes());
    match context.accepted_world {
        Some(hash) => {
            bytes.push(1);
            bytes.extend_from_slice(&hash);
        }
        None => bytes.push(0),
    }
    let mut package_hashes = context.package_hashes.clone();
    package_hashes.sort_unstable();
    bytes.extend_from_slice(&(package_hashes.len() as u64).to_be_bytes());
    for hash in package_hashes {
        bytes.extend_from_slice(&hash);
    }
    write_optional_symbol(&mut bytes, context.book.as_ref());
    write_optional_symbol(&mut bytes, context.scenario.as_ref());
    bytes
}

fn canonical_bytes<T: CanonicalEncoding>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.encode(&mut bytes);
    bytes
}

trait CanonicalEncoding {
    fn encode(&self, bytes: &mut Vec<u8>);
}

impl CanonicalEncoding for Term {
    fn encode(&self, bytes: &mut Vec<u8>) {
        encode_term(self, bytes)
    }
}

impl CanonicalEncoding for Atom {
    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.push(b'a');
        write_nominal(bytes, &self.predicate);
        write_len(bytes, self.arguments.len());
        for argument in &self.arguments {
            argument.encode(bytes);
        }
    }
}

impl CanonicalEncoding for Goal {
    fn encode(&self, bytes: &mut Vec<u8>) {
        encode_goal(self, bytes)
    }
}

impl CanonicalEncoding for Clause {
    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.push(b'c');
        self.head.encode(bytes);
        self.body.encode(bytes);
    }
}

fn encode_term(term: &Term, bytes: &mut Vec<u8>) {
    match term {
        Term::Var(variable) => {
            bytes.push(b'v');
            encode_var(variable, bytes);
        }
        Term::Nominal(nominal) => {
            bytes.push(b'n');
            write_nominal(bytes, nominal);
        }
        Term::Unit(unit) => {
            bytes.push(b'u');
            write_nominal(bytes, unit.as_nominal());
        }
        Term::Quantity(quantity) => {
            bytes.push(b'q');
            bytes.extend_from_slice(&quantity.canonical_bytes());
        }
        Term::Integer(value) => {
            bytes.push(b'i');
            write_bigint(bytes, value);
        }
        Term::Decimal(value) => {
            bytes.push(b'd');
            write_normalized_rational(bytes, value);
        }
        Term::Record(record) => {
            bytes.push(b'r');
            write_len(bytes, record.fields.len());
            for (name, value) in &record.fields {
                write_symbol(bytes, name);
                value.encode(bytes);
            }
            write_optional_var(record.rest.as_ref(), bytes);
        }
        Term::Tuple(values) => {
            bytes.push(b't');
            write_len(bytes, values.len());
            for value in values {
                value.encode(bytes);
            }
        }
        Term::App {
            constructor,
            arguments,
        } => {
            bytes.push(b'f');
            write_symbol(bytes, constructor);
            write_len(bytes, arguments.len());
            for argument in arguments {
                argument.encode(bytes);
            }
        }
        Term::Bool(value) => {
            bytes.push(b'b');
            bytes.push(u8::from(*value));
        }
        Term::Text(value) => {
            bytes.push(b'x');
            write_bytes(bytes, value.as_bytes());
        }
    }
}

fn encode_goal(goal: &Goal, bytes: &mut Vec<u8>) {
    match goal {
        Goal::True => bytes.push(b't'),
        Goal::False => bytes.push(b'f'),
        Goal::Atom(atom) => atom.encode(bytes),
        Goal::Equal(left, right) => {
            bytes.push(b'=');
            left.encode(bytes);
            right.encode(bytes);
        }
        Goal::NotEqual(left, right) => {
            bytes.push(b'!');
            left.encode(bytes);
            right.encode(bytes);
        }
        Goal::And(goals) => {
            bytes.push(b'&');
            write_len(bytes, goals.len());
            for goal in goals {
                goal.encode(bytes);
            }
        }
        Goal::Or(goals) => {
            bytes.push(b'|');
            write_len(bytes, goals.len());
            for goal in goals {
                goal.encode(bytes);
            }
        }
    }
}

fn write_nominal(bytes: &mut Vec<u8>, nominal: &Nominal) {
    let (kind, custom) = nominal.kind.tag();
    write_bytes(bytes, kind.as_bytes());
    write_optional_symbol(bytes, custom);
    write_optional_symbol(bytes, nominal.namespace.as_ref());
    write_symbol(bytes, &nominal.name);
}

fn encode_var(variable: &Var, bytes: &mut Vec<u8>) {
    // `Canonicalizer` has already replaced the source id with the canonical
    // encounter-order id and removed its diagnostic name.
    bytes.push(match variable.kind {
        VarKind::Inference => b'i',
        VarKind::Hole => b'h',
        VarKind::Row => b'r',
    });
    bytes.extend_from_slice(&(variable.id as u64).to_be_bytes());
    encode_sort(&variable.sort, bytes);
}

fn encode_sort(sort: &Sort, bytes: &mut Vec<u8>) {
    match sort {
        Sort::Any => bytes.push(0),
        Sort::Bool => bytes.push(8),
        Sort::Text => bytes.push(9),
        Sort::Integer => bytes.push(10),
        Sort::Decimal => bytes.push(11),
        Sort::Nominal(kind) => {
            bytes.push(1);
            match kind {
                Some(kind) => {
                    bytes.push(1);
                    let (tag, custom) = kind.tag();
                    write_bytes(bytes, tag.as_bytes());
                    write_optional_symbol(bytes, custom);
                }
                None => bytes.push(0),
            }
        }
        Sort::Unit(unit) => {
            bytes.push(2);
            match unit {
                Some(unit) => {
                    bytes.push(1);
                    write_nominal(bytes, unit.as_nominal());
                }
                None => bytes.push(0),
            }
        }
        Sort::Quantity(unit) => {
            bytes.push(3);
            match unit {
                Some(unit) => {
                    bytes.push(1);
                    write_nominal(bytes, unit.as_nominal());
                }
                None => bytes.push(0),
            }
        }
        Sort::Record => bytes.push(4),
        Sort::Row => bytes.push(5),
        Sort::Atom => bytes.push(6),
        Sort::Goal => bytes.push(7),
    }
}

fn write_optional_var(variable: Option<&Var>, bytes: &mut Vec<u8>) {
    match variable {
        Some(variable) => {
            bytes.push(1);
            encode_var(variable, bytes);
        }
        None => bytes.push(0),
    }
}

fn write_optional_symbol(bytes: &mut Vec<u8>, symbol: Option<&Symbol>) {
    match symbol {
        Some(symbol) => {
            bytes.push(1);
            write_symbol(bytes, symbol);
        }
        None => bytes.push(0),
    }
}

fn write_symbol(bytes: &mut Vec<u8>, symbol: &Symbol) {
    write_bytes(bytes, symbol.as_str().as_bytes());
}

fn write_bigint(bytes: &mut Vec<u8>, value: &BigInt) {
    let (tag, magnitude) = match value.sign() {
        Sign::Minus => (0u8, value.magnitude().to_bytes_be()),
        Sign::NoSign => (1u8, Vec::new()),
        Sign::Plus => (2u8, value.magnitude().to_bytes_be()),
    };
    bytes.push(tag);
    write_bytes(bytes, &magnitude);
}

fn write_normalized_rational(bytes: &mut Vec<u8>, value: &BigRational) {
    let value = normalize_rational(value);
    write_bigint(bytes, value.numer());
    write_bigint(bytes, value.denom());
}

fn normalize_rational(value: &BigRational) -> BigRational {
    if value.denom().is_zero() {
        value.clone()
    } else {
        BigRational::new(value.numer().clone(), value.denom().clone())
    }
}

fn write_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    write_len(bytes, value.len());
    bytes.extend_from_slice(value);
}

fn write_len(bytes: &mut Vec<u8>, length: usize) {
    bytes.extend_from_slice(&(length as u64).to_be_bytes());
}

#[derive(Default)]
struct Canonicalizer {
    variables: BTreeMap<Var, usize>,
}

impl Canonicalizer {
    fn variable(&mut self, variable: &Var) -> CanonicalVar {
        let next = self.variables.len();
        let index = *self.variables.entry(variable.clone()).or_insert(next);
        CanonicalVar {
            kind: variable.kind.clone(),
            sort: variable.sort.clone(),
            index,
        }
    }

    fn term(&mut self, term: &Term) -> Term {
        match term {
            Term::Var(variable) => {
                let canonical = self.variable(variable);
                Term::Var(Var {
                    id: canonical.index as u32,
                    kind: canonical.kind,
                    name: None,
                    sort: canonical.sort,
                })
            }
            Term::Nominal(nominal) => Term::Nominal(nominal.clone()),
            Term::Unit(unit) => Term::Unit(unit.clone()),
            Term::Quantity(quantity) => Term::Quantity(quantity.clone()),
            Term::Integer(value) => Term::Integer(value.clone()),
            Term::Decimal(value) => Term::Decimal(normalize_rational(value)),
            Term::Record(record) => Term::Record(Record {
                fields: record
                    .fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.term(value)))
                    .collect(),
                rest: record.rest.as_ref().map(|variable| {
                    let canonical = self.variable(variable);
                    Var {
                        id: canonical.index as u32,
                        kind: canonical.kind,
                        name: None,
                        sort: canonical.sort,
                    }
                }),
            }),
            Term::Tuple(values) => {
                Term::Tuple(values.iter().map(|value| self.term(value)).collect())
            }
            Term::App {
                constructor,
                arguments,
            } => Term::App {
                constructor: constructor.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.term(argument))
                    .collect(),
            },
            Term::Bool(value) => Term::Bool(*value),
            Term::Text(value) => Term::Text(value.clone()),
        }
    }

    fn atom(&mut self, atom: &Atom) -> Atom {
        Atom {
            predicate: atom.predicate.clone(),
            arguments: atom
                .arguments
                .iter()
                .map(|argument| self.term(argument))
                .collect(),
        }
    }

    fn goal(&mut self, goal: &Goal) -> Goal {
        match goal {
            Goal::True => Goal::True,
            Goal::False => Goal::False,
            Goal::Atom(atom) => Goal::Atom(self.atom(atom)),
            Goal::Equal(left, right) => Goal::Equal(self.term(left), self.term(right)),
            Goal::NotEqual(left, right) => Goal::NotEqual(self.term(left), self.term(right)),
            Goal::And(goals) => Goal::And(goals.iter().map(|goal| self.goal(goal)).collect()),
            Goal::Or(goals) => Goal::Or(goals.iter().map(|goal| self.goal(goal)).collect()),
        }
    }

    fn clause(&mut self, clause: &Clause) -> Clause {
        Clause {
            head: self.atom(&clause.head),
            body: self.goal(&clause.body),
        }
    }
}

struct CanonicalVar {
    kind: VarKind,
    sort: Sort,
    index: usize,
}

impl CanonicalEncoding for CanonicalVar {
    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.push(match self.kind {
            VarKind::Inference => b'i',
            VarKind::Hole => b'h',
            VarKind::Row => b'r',
        });
        bytes.extend_from_slice(&(self.index as u64).to_be_bytes());
        encode_sort(&self.sort, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::One;

    fn predicate(name: &str) -> Nominal {
        Nominal::new(NominalKind::Predicate, name)
    }

    #[test]
    fn alpha_equivalent_goals_have_identical_canonical_forms() {
        let left = Var::named(17, "amount");
        let right = Var::named(99, "renamed");
        let first = Goal::Atom(Atom::new(predicate("eligible"), vec![Term::Var(left)]));
        let second = Goal::Atom(Atom::new(predicate("eligible"), vec![Term::Var(right)]));
        let context = CanonicalContext::default();
        let first = canonicalize_goal(&first, &context);
        let second = canonicalize_goal(&second, &context);
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(first.hash, second.hash);
    }

    #[test]
    fn context_changes_hash_but_not_canonical_bytes() {
        let goal = Goal::True;
        let plain = canonicalize_goal(&goal, &CanonicalContext::default());
        let contextual = canonicalize_goal(
            &goal,
            &CanonicalContext {
                semantics_version: 1,
                ..CanonicalContext::default()
            },
        );
        assert_eq!(plain.bytes, contextual.bytes);
        assert_ne!(plain.hash, contextual.hash);
    }

    #[test]
    fn quantities_require_units_except_for_zero() {
        assert!(ExactQuantity::new(BigRational::one(), None).is_err());
        assert!(ExactQuantity::new(BigRational::zero(), None).is_ok());
        let zero = ExactQuantity::zero();
        assert!(zero.value().is_zero());
        assert!(zero.unit().is_none());
        assert_eq!(
            ExactQuantity::new(
                BigRational::new_raw(BigInt::from(1), BigInt::zero()),
                Some(Unit::new("USD")),
            ),
            Err(QuantityError::ZeroDenominator)
        );
    }

    #[test]
    fn scalar_terms_keep_arbitrary_precision_without_floats() {
        let integer = BigInt::from(10u8).pow(128);
        let integer_term = Term::integer(integer.clone());
        assert_eq!(integer_term, Term::Integer(integer));

        let decimal = BigRational::new(BigInt::from(125u8), BigInt::from(100u8));
        let decimal_term = Term::decimal(decimal.clone());
        assert_eq!(decimal_term, Term::Decimal(decimal));
    }

    #[test]
    fn scalar_canonical_encoding_has_distinct_tags_and_normalized_rationals() {
        let context = CanonicalContext::default();
        let integer = canonicalize_term(&Term::integer(7), &context);
        let decimal = canonicalize_term(
            &Term::decimal(BigRational::from_integer(BigInt::from(7))),
            &context,
        );
        assert_ne!(decimal.bytes, integer.bytes);
        assert_eq!(integer.bytes[0], b'i');
        assert_eq!(decimal.bytes[0], b'd');

        let reduced = canonicalize_term(
            &Term::decimal(BigRational::new_raw(BigInt::from(2), BigInt::from(4))),
            &context,
        );
        let canonical = canonicalize_term(
            &Term::decimal(BigRational::new(BigInt::from(1), BigInt::from(2))),
            &context,
        );
        assert_eq!(
            reduced.value,
            Term::decimal(BigRational::new(BigInt::from(1), BigInt::from(2)))
        );
        assert_eq!(reduced.bytes, canonical.bytes);
        assert_eq!(reduced.hash, canonical.hash);
    }

    #[test]
    fn scalar_canonical_encoding_orders_sign_and_magnitude_unambiguously() {
        let context = CanonicalContext::default();
        let negative = canonicalize_term(&Term::integer(-7), &context);
        let zero = canonicalize_term(&Term::integer(0), &context);
        let positive = canonicalize_term(&Term::integer(7), &context);
        assert_ne!(negative.bytes, zero.bytes);
        assert_ne!(zero.bytes, positive.bytes);
        assert_ne!(negative.bytes, positive.bytes);
    }

    #[test]
    fn record_fields_are_canonicalized_by_name() {
        let variable = Var::inference(1);
        let one = Term::record(Record::closed([
            ("b", Term::Var(variable.clone())),
            ("a", Term::Bool(true)),
        ]));
        let two = Term::record(Record::closed([
            ("a", Term::Bool(true)),
            ("b", Term::Var(Var::named(100, "x"))),
        ]));
        assert_eq!(
            canonicalize_term(&one, &Default::default()).bytes,
            canonicalize_term(&two, &Default::default()).bytes
        );
    }
}
