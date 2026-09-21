//! Immutable source evidence and the observation/import boundary.
//!
//! Evidence is deliberately a fairly boring layer.  It records what a source
//! supplied (including the fact that the source is no longer available), and
//! records links between observations.  It does not resolve links, choose a
//! correction, or create an accepted economic fact.  In particular, equal
//! content is not a reason to collapse occurrences: an occurrence is the
//! identity of one source observation, while [`ContentHash`] identifies its
//! normalized contents.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use crate::exact::ExactNumber;
use crate::model::{ContentHash, ExternalId, Identity, OccurrenceId, SourceId};

const RAW_EVIDENCE_DOMAIN: &str = "axiom.raw-evidence.v1";

/// Whether the bytes/value for a raw observation can currently be read.
///
/// These states are intentionally not folded into `Option<Vec<u8>>`: a source
/// that was never supplied is different from a source that was redacted or a
/// payload that failed integrity checks.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Availability {
    /// The source payload is available and may be inspected.
    Present,
    /// The source was expected but is not available in this evidence set.
    Unavailable,
    /// The source exists, but its content has been intentionally hidden.
    Redacted,
    /// The source was deliberately deleted; the tombstone remains.
    Deleted,
    /// Bytes were supplied but failed an integrity or decoding check.
    Corrupt,
    /// Bytes are available but no supported decoder can interpret them.
    Unsupported,
}

impl Availability {
    pub const fn has_payload(self) -> bool {
        matches!(self, Self::Present | Self::Corrupt | Self::Unsupported)
    }

    pub const fn is_tombstone(self) -> bool {
        matches!(self, Self::Unavailable | Self::Redacted | Self::Deleted)
    }
}

/// A source location retained by an observation.  The locator is intentionally
/// open-ended enough for CSV, JSON, PDF and ordinary byte-oriented adapters.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SpanLocator {
    Bytes {
        start: u64,
        end: u64,
    },
    LineColumn {
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    },
    JsonPointer(String),
    CsvRow {
        row: u64,
        columns: Option<(u32, u32)>,
    },
    PdfRegion {
        page: u32,
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
    },
    Other(String),
}

/// A source span is provenance for a value or observation, not a replacement
/// for the source identity itself.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceSpan {
    pub source: SourceId,
    pub locator: SpanLocator,
}

impl SourceSpan {
    pub fn new(source: impl Into<SourceId>, locator: SpanLocator) -> Self {
        Self {
            source: source.into(),
            locator,
        }
    }

    pub fn bytes(source: impl Into<SourceId>, start: u64, end: u64) -> Self {
        Self::new(source, SpanLocator::Bytes { start, end })
    }

    pub fn json(source: impl Into<SourceId>, pointer: impl Into<String>) -> Self {
        Self::new(source, SpanLocator::JsonPointer(pointer.into()))
    }

    pub fn csv_row(source: impl Into<SourceId>, row: u64) -> Self {
        Self::new(source, SpanLocator::CsvRow { row, columns: None })
    }

    pub fn csv_columns(source: impl Into<SourceId>, row: u64, start: u32, end: u32) -> Self {
        Self::new(
            source,
            SpanLocator::CsvRow {
                row,
                columns: Some((start, end)),
            },
        )
    }

    pub fn pdf(source: impl Into<SourceId>, page: u32, x1: u32, y1: u32, x2: u32, y2: u32) -> Self {
        Self::new(
            source,
            SpanLocator::PdfRegion {
                page,
                x1,
                y1,
                x2,
                y2,
            },
        )
    }
}

/// Versioned information about the code which emitted an observation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AdapterProvenance {
    pub name: String,
    pub version: String,
    pub implementation: Option<ContentHash>,
}

impl AdapterProvenance {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            implementation: None,
        }
    }

    pub fn with_implementation(mut self, implementation: ContentHash) -> Self {
        self.implementation = Some(implementation);
        self
    }
}

/// The source and adapter history for an observation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Provenance {
    pub source: SourceId,
    pub adapter: Option<AdapterProvenance>,
    pub spans: Vec<SourceSpan>,
    /// An adapter may preserve an external observation timestamp without
    /// pretending that this string is a semantic event time.
    pub observed_at: Option<String>,
}

impl Provenance {
    pub fn new(source: impl Into<SourceId>) -> Self {
        Self {
            source: source.into(),
            adapter: None,
            spans: Vec::new(),
            observed_at: None,
        }
    }

    pub fn with_adapter(mut self, adapter: AdapterProvenance) -> Self {
        self.adapter = Some(adapter);
        self
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.spans.push(span);
        self
    }

    pub fn with_spans(mut self, spans: impl IntoIterator<Item = SourceSpan>) -> Self {
        self.spans.extend(spans);
        self
    }

    pub fn observed_at(mut self, value: impl Into<String>) -> Self {
        self.observed_at = Some(value.into());
        self
    }
}

/// The authority attached to an observation or explicit relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AuthorityKind {
    SourceObservation,
    InstitutionalRecord,
    UserAssertion,
    PolicyDerivation,
    SignedDecision,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Authority {
    pub kind: AuthorityKind,
    pub subject: String,
}

impl Authority {
    pub fn new(kind: AuthorityKind, subject: impl Into<String>) -> Self {
        Self {
            kind,
            subject: subject.into(),
        }
    }

    pub fn source(subject: impl Into<String>) -> Self {
        Self::new(AuthorityKind::SourceObservation, subject)
    }

    pub fn institution(subject: impl Into<String>) -> Self {
        Self::new(AuthorityKind::InstitutionalRecord, subject)
    }

    pub fn user(subject: impl Into<String>) -> Self {
        Self::new(AuthorityKind::UserAssertion, subject)
    }
}

/// One immutable source observation.
///
/// `identity.content` is a normalized content address, while
/// `identity.occurrence` and `identity.external` remain independent.  Payload
/// bytes are private and can only be borrowed or cloned; no operation on a
/// `RawEvidence` rewrites it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEvidence {
    identity: Identity,
    provenance: Provenance,
    authority: Authority,
    availability: Availability,
    note: Option<String>,
    payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceError {
    AvailabilityPayloadMismatch {
        availability: Availability,
        has_payload: bool,
    },
    InvalidTombstoneAvailability {
        availability: Availability,
    },
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AvailabilityPayloadMismatch {
                availability,
                has_payload,
            } => write!(
                formatter,
                "availability {availability:?} is inconsistent with payload presence {has_payload}"
            ),
            Self::InvalidTombstoneAvailability { availability } => write!(
                formatter,
                "{availability:?} is not a tombstone availability state"
            ),
        }
    }
}

impl std::error::Error for EvidenceError {}

impl RawEvidence {
    /// Derive a content identity for raw bytes.  Adapters that normalize a
    /// document before hashing should use [`Self::observed_with_content`].
    pub fn content_hash(bytes: &[u8]) -> ContentHash {
        ContentHash::domain_separated(RAW_EVIDENCE_DOMAIN, bytes)
    }

    /// Build a present observation using bytes as both the source payload and
    /// the content identity input.
    pub fn from_bytes(
        source: impl Into<SourceId>,
        occurrence: impl Into<OccurrenceId>,
        external: Option<ExternalId>,
        bytes: impl AsRef<[u8]>,
    ) -> Self {
        let source = source.into();
        let bytes = bytes.as_ref().to_vec();
        Self::observed_with_content(
            source,
            occurrence,
            external,
            Self::content_hash(&bytes),
            bytes,
        )
    }

    /// Build an observation when the adapter has a normalized semantic content
    /// hash distinct from the exact source bytes.
    pub fn observed_with_content(
        source: impl Into<SourceId>,
        occurrence: impl Into<OccurrenceId>,
        external: Option<ExternalId>,
        content: ContentHash,
        bytes: impl AsRef<[u8]>,
    ) -> Self {
        let source = source.into();
        let mut identity = Identity::new(occurrence, content);
        if let Some(external) = external {
            identity = identity.with_external(external);
        }
        Self::from_parts(
            identity,
            Provenance::new(source.clone()),
            Authority::source(source.to_string()),
            Availability::Present,
            Some(bytes.as_ref().to_vec()),
        )
    }

    fn from_parts(
        identity: Identity,
        provenance: Provenance,
        authority: Authority,
        availability: Availability,
        payload: Option<Vec<u8>>,
    ) -> Self {
        debug_assert_eq!(availability.has_payload(), payload.is_some());
        Self {
            identity,
            provenance,
            authority,
            availability,
            note: None,
            payload,
        }
    }

    /// Construct from a complete identity.  This is useful for unavailable,
    /// redacted, corrupt, and deleted records for which bytes are absent.
    pub fn new(
        identity: Identity,
        provenance: Provenance,
        authority: Authority,
        availability: Availability,
        payload: Option<Vec<u8>>,
    ) -> Result<Self, EvidenceError> {
        if availability.has_payload() != payload.is_some() {
            return Err(EvidenceError::AvailabilityPayloadMismatch {
                availability,
                has_payload: payload.is_some(),
            });
        }
        Ok(Self::from_parts(
            identity,
            provenance,
            authority,
            availability,
            payload,
        ))
    }

    /// Alias emphasizing that this constructor is an observation, not an
    /// accepted fact.
    pub fn observed(
        source: impl Into<SourceId>,
        occurrence: impl Into<OccurrenceId>,
        external: Option<ExternalId>,
        bytes: impl AsRef<[u8]>,
    ) -> Self {
        Self::from_bytes(source, occurrence, external, bytes)
    }

    pub fn tombstone(
        identity: Identity,
        provenance: Provenance,
        authority: Authority,
        availability: Availability,
    ) -> Result<Self, EvidenceError> {
        if !availability.is_tombstone() {
            return Err(EvidenceError::InvalidTombstoneAvailability { availability });
        }
        Self::new(identity, provenance, authority, availability, None)
    }

    pub fn deleted_tombstone(
        source: impl Into<SourceId>,
        occurrence: impl Into<OccurrenceId>,
        external: Option<ExternalId>,
        content: ContentHash,
    ) -> Result<Self, EvidenceError> {
        let source = source.into();
        let mut identity = Identity::new(occurrence, content);
        if let Some(external) = external {
            identity = identity.with_external(external);
        }
        Self::tombstone(
            identity,
            Provenance::new(source.clone()),
            Authority::source(source.to_string()),
            Availability::Deleted,
        )
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = authority;
        self
    }

    pub fn with_availability(mut self, availability: Availability) -> Result<Self, EvidenceError> {
        if availability.has_payload() != self.payload.is_some() {
            return Err(EvidenceError::AvailabilityPayloadMismatch {
                availability,
                has_payload: self.payload.is_some(),
            });
        }
        self.availability = availability;
        Ok(self)
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn payload(&self) -> Option<&[u8]> {
        self.payload.as_deref()
    }

    pub fn payload_owned(&self) -> Option<Vec<u8>> {
        self.payload.clone()
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    pub const fn availability(&self) -> Availability {
        self.availability
    }

    pub fn occurrence(&self) -> &OccurrenceId {
        &self.identity.occurrence
    }

    pub fn external(&self) -> Option<&ExternalId> {
        self.identity.external.as_ref()
    }

    pub fn content(&self) -> ContentHash {
        self.identity.content
    }

    pub fn source(&self) -> &SourceId {
        &self.provenance.source
    }

    pub fn is_available(&self) -> bool {
        self.availability == Availability::Present && self.payload.is_some()
    }
}

/// A bounded confidence value for a candidate link.  Confidence is metadata
/// for review/ranking, not an accepted probability and never performs a
/// union-find operation on identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Confidence(u16);

impl Confidence {
    pub const ZERO: Self = Self(0);
    pub const CERTAIN: Self = Self(10_000);

    pub const fn from_basis_points(value: u16) -> Self {
        Self(if value > 10_000 { 10_000 } else { value })
    }

    pub const fn from_percent(value: u16) -> Self {
        Self::from_basis_points(value.saturating_mul(100))
    }

    pub const fn basis_points(self) -> u16 {
        self.0
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{:04}", self.0 / 10_000, self.0 % 10_000)
    }
}

/// A field/time/source scope for a correction or supersession.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CorrectionScope {
    /// Empty means the whole observation.  Paths use adapter-defined syntax
    /// (for example `/data/object/amount_due` or `amount`).
    pub fields: Vec<String>,
    pub source: Option<SourceId>,
    pub interval: Option<String>,
    pub rationale: Option<String>,
}

pub type RelationScope = CorrectionScope;

impl CorrectionScope {
    pub fn all() -> Self {
        Self {
            fields: Vec::new(),
            source: None,
            interval: None,
            rationale: None,
        }
    }

    pub fn fields(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            fields: fields.into_iter().map(Into::into).collect(),
            ..Self::all()
        }
    }

    pub fn field(field: impl Into<String>) -> Self {
        Self::fields([field])
    }

    pub fn with_source(mut self, source: impl Into<SourceId>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn during(mut self, interval: impl Into<String>) -> Self {
        self.interval = Some(interval.into());
        self
    }

    pub fn with_rationale(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = Some(rationale.into());
        self
    }

    pub fn is_global(&self) -> bool {
        self.fields.is_empty() && self.source.is_none() && self.interval.is_none()
    }
}

/// One member of a split/merge conservation statement.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConservationLeg {
    pub identity: Identity,
    pub quantity: Option<ExactNumber>,
    /// Unit of the quantity on this leg.  A quantity without a unit is not
    /// safe to compare with another leg, even when the numeric values match.
    pub unit: Option<String>,
}

impl ConservationLeg {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            quantity: None,
            unit: None,
        }
    }

    pub fn with_quantity(mut self, quantity: ExactNumber) -> Self {
        self.quantity = Some(quantity);
        self
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

/// Metadata attached to a split or merge relation.  The relation itself does
/// not assert that quantities are equal; this structure records which members
/// and quantities a later checker must compare.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConservationMetadata {
    pub inputs: Vec<ConservationLeg>,
    pub outputs: Vec<ConservationLeg>,
    pub unit: Option<String>,
    pub note: Option<String>,
}

impl ConservationMetadata {
    pub fn new(
        inputs: impl IntoIterator<Item = ConservationLeg>,
        outputs: impl IntoIterator<Item = ConservationLeg>,
    ) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
            unit: None,
            note: None,
        }
    }

    pub fn for_split(parent: Identity, children: impl IntoIterator<Item = Identity>) -> Self {
        Self::new(
            [ConservationLeg::new(parent)],
            children.into_iter().map(ConservationLeg::new),
        )
    }

    pub fn for_merge(inputs: impl IntoIterator<Item = Identity>, output: Identity) -> Self {
        Self::new(
            inputs.into_iter().map(ConservationLeg::new),
            [ConservationLeg::new(output)],
        )
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Check conservation when every leg has an exact quantity.  Returning
    /// `None` means the metadata is incomplete, not that conservation failed.
    pub fn balances(&self) -> Option<bool> {
        let mut unit: Option<&str> = self.unit.as_deref();
        for leg in self.inputs.iter().chain(self.outputs.iter()) {
            if self.unit.is_none() && leg.unit.is_none() {
                return None;
            }
            let leg_unit = leg.unit.as_deref().or(unit);
            let leg_unit = leg_unit?;
            if let Some(expected) = unit {
                if expected != leg_unit {
                    return Some(false);
                }
            } else {
                unit = Some(leg_unit);
            }
        }
        unit?;
        let mut inputs = num_rational::BigRational::from_integer(0.into());
        let mut outputs = num_rational::BigRational::from_integer(0.into());
        for leg in &self.inputs {
            inputs += leg.quantity.as_ref()?.as_rational();
        }
        for leg in &self.outputs {
            outputs += leg.quantity.as_ref()?.as_rational();
        }
        Some(inputs == outputs)
    }
}

/// Explicit relations between evidence or observations.  A relation is a
/// fact about evidence; it is never a command to delete or merge either end.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EvidenceRelationKind {
    SameAs,
    PossiblySameAs,
    DerivedFrom,
    Corrects,
    Supersedes,
    Splits,
    Merges,
    Settles,
    Satisfies,
    Reverses,
    Reclassifies,
}

/// Common short alias for callers that refer to relation values directly.
pub type RelationKind = EvidenceRelationKind;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EvidenceRelation {
    pub kind: EvidenceRelationKind,
    /// For ordinary relations this is the left/source endpoint.  For a split
    /// it is the one parent; for a merge it contains the first input.
    pub source: Identity,
    /// For ordinary relations this is the right/target endpoint.  Additional
    /// split/merge members are retained in `related`.
    pub target: Identity,
    pub related: Vec<Identity>,
    pub scope: Option<CorrectionScope>,
    pub conservation: Option<ConservationMetadata>,
    pub provenance: Option<Provenance>,
    pub authority: Option<Authority>,
}

impl EvidenceRelation {
    pub fn new(kind: EvidenceRelationKind, source: Identity, target: Identity) -> Self {
        Self {
            kind,
            source,
            target,
            related: Vec::new(),
            scope: None,
            conservation: None,
            provenance: None,
            authority: None,
        }
    }

    pub fn same_as(left: Identity, right: Identity) -> Self {
        Self::new(EvidenceRelationKind::SameAs, left, right)
    }

    pub fn possibly_same_as(left: Identity, right: Identity) -> Self {
        Self::new(EvidenceRelationKind::PossiblySameAs, left, right)
    }

    /// `derived` is the new observation and `source` is the observation it
    /// was derived from.
    pub fn derived_from(derived: Identity, source: Identity) -> Self {
        Self::new(EvidenceRelationKind::DerivedFrom, derived, source)
    }

    /// The first argument is the newer/correcting assertion.  The old
    /// assertion remains present in the store.
    pub fn corrects(corrected: Identity, old: Identity, scope: CorrectionScope) -> Self {
        let mut relation = Self::new(EvidenceRelationKind::Corrects, corrected, old);
        relation.scope = Some(scope);
        relation
    }

    pub fn supersedes(newer: Identity, older: Identity, scope: CorrectionScope) -> Self {
        let mut relation = Self::new(EvidenceRelationKind::Supersedes, newer, older);
        relation.scope = Some(scope);
        relation
    }

    pub fn settles(settlement: Identity, obligation: Identity) -> Self {
        Self::new(EvidenceRelationKind::Settles, settlement, obligation)
    }

    pub fn satisfies(payment: Identity, obligation: Identity) -> Self {
        Self::new(EvidenceRelationKind::Satisfies, payment, obligation)
    }

    pub fn reverses(reversal: Identity, original: Identity) -> Self {
        Self::new(EvidenceRelationKind::Reverses, reversal, original)
    }

    pub fn reclassifies(reclassification: Identity, prior: Identity) -> Self {
        Self::new(EvidenceRelationKind::Reclassifies, reclassification, prior)
    }

    pub fn splits(
        parent: Identity,
        children: impl IntoIterator<Item = Identity>,
        conservation: ConservationMetadata,
    ) -> Option<Self> {
        let mut children = children.into_iter();
        let target = children.next()?;
        let mut relation = Self::new(EvidenceRelationKind::Splits, parent, target);
        relation.related = children.collect();
        relation.conservation = Some(conservation);
        Some(relation)
    }

    pub fn merges(
        inputs: impl IntoIterator<Item = Identity>,
        result: Identity,
        conservation: ConservationMetadata,
    ) -> Option<Self> {
        let mut inputs = inputs.into_iter();
        let source = inputs.next()?;
        let mut relation = Self::new(EvidenceRelationKind::Merges, source, result);
        relation.related = inputs.collect();
        relation.conservation = Some(conservation);
        Some(relation)
    }

    pub fn with_scope(mut self, scope: CorrectionScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = Some(authority);
        self
    }

    pub fn with_conservation(mut self, conservation: ConservationMetadata) -> Self {
        self.conservation = Some(conservation);
        self
    }

    pub fn from(&self) -> &Identity {
        &self.source
    }

    pub fn to(&self) -> &Identity {
        &self.target
    }

    pub fn sources(&self) -> impl Iterator<Item = &Identity> {
        std::iter::once(&self.source).chain(
            self.related
                .iter()
                .filter(move |_| self.kind == EvidenceRelationKind::Merges),
        )
    }

    pub fn targets(&self) -> impl Iterator<Item = &Identity> {
        std::iter::once(&self.target).chain(
            self.related
                .iter()
                .filter(move |_| self.kind == EvidenceRelationKind::Splits),
        )
    }
}

/// A proposed identity match.  It is intentionally separate from an
/// accepted `same_as` relation, and retaining it never removes either leaf.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CandidateIdentityLink {
    pub left: Identity,
    pub right: Identity,
    pub confidence: Confidence,
    pub provenance: Provenance,
    pub authority: Authority,
    pub rationale: Option<String>,
}

impl CandidateIdentityLink {
    pub fn new(
        left: Identity,
        right: Identity,
        confidence: Confidence,
        provenance: Provenance,
        authority: Authority,
    ) -> Self {
        Self {
            left,
            right,
            confidence,
            provenance,
            authority,
            rationale: None,
        }
    }

    pub fn with_rationale(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = Some(rationale.into());
        self
    }

    pub fn kind(&self) -> EvidenceRelationKind {
        EvidenceRelationKind::PossiblySameAs
    }

    pub fn relation(&self) -> EvidenceRelation {
        EvidenceRelation::possibly_same_as(self.left.clone(), self.right.clone())
            .with_provenance(self.provenance.clone())
            .with_authority(self.authority.clone())
    }
}

/// Observations emitted by an adapter.  An adapter can create this value, but
/// cannot access or mutate an [`EvidenceStore`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportBatch {
    pub source: SourceId,
    pub adapter: Option<AdapterProvenance>,
    pub observations: Vec<RawEvidence>,
}

impl ImportBatch {
    pub fn new(source: impl Into<SourceId>) -> Self {
        Self {
            source: source.into(),
            adapter: None,
            observations: Vec::new(),
        }
    }

    pub fn with_adapter(mut self, adapter: AdapterProvenance) -> Self {
        self.adapter = Some(adapter);
        self
    }

    pub fn push(&mut self, observation: RawEvidence) {
        self.observations.push(observation);
    }

    pub fn observe(mut self, observation: RawEvidence) -> Self {
        self.push(observation);
        self
    }

    pub fn from_observations(
        source: impl Into<SourceId>,
        observations: impl IntoIterator<Item = RawEvidence>,
    ) -> Self {
        let mut batch = Self::new(source);
        batch.observations.extend(observations);
        batch
    }

    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// A deterministic batch identity independent of adapter emission order.
    pub fn content_hash(&self) -> ContentHash {
        let mut keys: Vec<_> = self
            .observations
            .iter()
            .map(|evidence| {
                (
                    ImportKey::from_evidence(evidence, self.adapter.as_ref()),
                    EvidenceDerivation::from_evidence(evidence, self.adapter.as_ref()),
                )
            })
            .collect();
        keys.sort();
        let mut bytes = Vec::new();
        put_string(&mut bytes, self.source.as_str());
        if let Some(adapter) = &self.adapter {
            put_string(&mut bytes, &adapter.name);
            put_string(&mut bytes, &adapter.version);
            if let Some(hash) = adapter.implementation {
                bytes.extend_from_slice(hash.as_bytes());
            }
        }
        for (key, derivation) in keys {
            key.encode(&mut bytes);
            derivation.encode(&mut bytes);
        }
        ContentHash::domain_separated("axiom.import-batch.v1", &bytes)
    }
}

/// A narrow observation-only adapter contract.  It has no method that can
/// insert, replace, accept, or reconcile evidence.
pub trait ObservationAdapter {
    type Error;

    fn observe(&self, source: &SourceId, bytes: &[u8]) -> Result<ImportBatch, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportError {
    /// One occurrence ID cannot identify two different immutable observations.
    OccurrenceConflict {
        occurrence: OccurrenceId,
        existing: ContentHash,
        incoming: ContentHash,
    },
    /// The batch metadata and an observation disagree about its source.
    SourceMismatch {
        expected: SourceId,
        actual: SourceId,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OccurrenceConflict {
                occurrence,
                existing,
                incoming,
            } => write!(
                formatter,
                "occurrence {occurrence} already has content {existing}, incoming content is {incoming}"
            ),
            Self::SourceMismatch { expected, actual } => {
                write!(
                    formatter,
                    "batch source {expected} does not match observation source {actual}"
                )
            }
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportDisposition {
    Inserted,
    AlreadyPresent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportItem {
    pub occurrence: OccurrenceId,
    pub disposition: ImportDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReport {
    pub batch: ContentHash,
    pub items: Vec<ImportItem>,
}

impl ImportReport {
    pub fn inserted_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.disposition == ImportDisposition::Inserted)
            .count()
    }

    pub fn existing_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.disposition == ImportDisposition::AlreadyPresent)
            .count()
    }

    pub fn inserted(&self) -> impl Iterator<Item = &ImportItem> {
        self.items
            .iter()
            .filter(|item| item.disposition == ImportDisposition::Inserted)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct ImportKey {
    source: SourceId,
    adapter: Option<AdapterProvenance>,
    external: Option<ExternalId>,
    occurrence: Option<OccurrenceId>,
    content: ContentHash,
    availability: Availability,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct StoredKey {
    import: ImportKey,
    occurrence: OccurrenceId,
}

impl ImportKey {
    fn from_evidence(evidence: &RawEvidence, batch_adapter: Option<&AdapterProvenance>) -> Self {
        Self {
            source: evidence.provenance.source.clone(),
            adapter: evidence
                .provenance
                .adapter
                .clone()
                .or_else(|| batch_adapter.cloned()),
            external: evidence.identity.external.clone(),
            // An external source ID is the source occurrence identity.  For
            // source records without one, the local occurrence is necessary
            // to preserve identical duplicate rows.
            occurrence: if evidence.identity.external.is_none() {
                Some(evidence.identity.occurrence.clone())
            } else {
                None
            },
            content: evidence.identity.content,
            availability: evidence.availability,
        }
    }

    fn encode(&self, bytes: &mut Vec<u8>) {
        put_string(bytes, self.source.as_str());
        put_optional_adapter(bytes, self.adapter.as_ref());
        put_optional_string(bytes, self.external.as_ref().map(ExternalId::as_str));
        put_optional_string(bytes, self.occurrence.as_ref().map(OccurrenceId::as_str));
        bytes.extend_from_slice(self.content.as_bytes());
        bytes.push(self.availability as u8);
    }
}

/// One recorded derivation of a semantic source observation.
///
/// Derivation details are audit history, not source-row identity. Replaying
/// the same row with a new span, timestamp, note, or generated occurrence is
/// idempotent while every distinct derivation remains inspectable here.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EvidenceDerivation {
    pub occurrence: OccurrenceId,
    pub provenance: Provenance,
    pub authority: Authority,
    pub note: Option<String>,
}

impl EvidenceDerivation {
    fn from_evidence(evidence: &RawEvidence, batch_adapter: Option<&AdapterProvenance>) -> Self {
        let mut provenance = evidence.provenance.clone();
        if provenance.adapter.is_none() {
            provenance.adapter = batch_adapter.cloned();
        }
        Self {
            occurrence: evidence.identity.occurrence.clone(),
            provenance,
            authority: evidence.authority.clone(),
            note: evidence.note.clone(),
        }
    }

    fn encode(&self, bytes: &mut Vec<u8>) {
        put_string(bytes, self.occurrence.as_str());
        put_string(bytes, self.provenance.source.as_str());
        put_optional_adapter(bytes, self.provenance.adapter.as_ref());
        put_spans(bytes, &self.provenance.spans);
        put_optional_string(bytes, self.provenance.observed_at.as_deref());
        put_authority(bytes, &self.authority);
        put_optional_string(bytes, self.note.as_deref());
    }
}

fn put_optional_adapter(bytes: &mut Vec<u8>, adapter: Option<&AdapterProvenance>) {
    match adapter {
        Some(adapter) => {
            bytes.push(1);
            put_string(bytes, &adapter.name);
            put_string(bytes, &adapter.version);
            match adapter.implementation {
                Some(hash) => {
                    bytes.push(1);
                    bytes.extend_from_slice(hash.as_bytes());
                }
                None => bytes.push(0),
            }
        }
        None => bytes.push(0),
    }
}

fn put_spans(bytes: &mut Vec<u8>, spans: &[SourceSpan]) {
    bytes.extend_from_slice(&(spans.len() as u64).to_be_bytes());
    for span in spans {
        put_string(bytes, span.source.as_str());
        match &span.locator {
            SpanLocator::Bytes { start, end } => {
                bytes.push(0);
                bytes.extend_from_slice(&start.to_be_bytes());
                bytes.extend_from_slice(&end.to_be_bytes());
            }
            SpanLocator::LineColumn {
                start_line,
                start_column,
                end_line,
                end_column,
            } => {
                bytes.push(1);
                bytes.extend_from_slice(&start_line.to_be_bytes());
                bytes.extend_from_slice(&start_column.to_be_bytes());
                bytes.extend_from_slice(&end_line.to_be_bytes());
                bytes.extend_from_slice(&end_column.to_be_bytes());
            }
            SpanLocator::JsonPointer(pointer) => {
                bytes.push(2);
                put_string(bytes, pointer);
            }
            SpanLocator::CsvRow { row, columns } => {
                bytes.push(3);
                bytes.extend_from_slice(&row.to_be_bytes());
                match columns {
                    Some((start, end)) => {
                        bytes.push(1);
                        bytes.extend_from_slice(&start.to_be_bytes());
                        bytes.extend_from_slice(&end.to_be_bytes());
                    }
                    None => bytes.push(0),
                }
            }
            SpanLocator::PdfRegion {
                page,
                x1,
                y1,
                x2,
                y2,
            } => {
                bytes.push(4);
                bytes.extend_from_slice(&page.to_be_bytes());
                bytes.extend_from_slice(&x1.to_be_bytes());
                bytes.extend_from_slice(&y1.to_be_bytes());
                bytes.extend_from_slice(&x2.to_be_bytes());
                bytes.extend_from_slice(&y2.to_be_bytes());
            }
            SpanLocator::Other(value) => {
                bytes.push(5);
                put_string(bytes, value);
            }
        }
    }
}

fn put_authority(bytes: &mut Vec<u8>, authority: &Authority) {
    bytes.push(match authority.kind {
        AuthorityKind::SourceObservation => 0,
        AuthorityKind::InstitutionalRecord => 1,
        AuthorityKind::UserAssertion => 2,
        AuthorityKind::PolicyDerivation => 3,
        AuthorityKind::SignedDecision => 4,
        AuthorityKind::Unknown => 5,
    });
    put_string(bytes, &authority.subject);
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn put_optional_string(bytes: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            bytes.push(1);
            put_string(bytes, value);
        }
        None => bytes.push(0),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelationError {
    MissingEndpoint {
        identity: Identity,
    },
    MissingScope {
        relation: EvidenceRelationKind,
    },
    InvalidEndpoint {
        relation: EvidenceRelationKind,
    },
    UnbalancedConservation {
        relation: EvidenceRelationKind,
        balances: Option<bool>,
    },
    ConservationMembersMismatch {
        relation: EvidenceRelationKind,
    },
}

impl fmt::Display for RelationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEndpoint { identity } => write!(
                formatter,
                "relation endpoint occurrence {} is not present in the evidence store",
                identity.occurrence
            ),
            Self::MissingScope { relation } => {
                write!(
                    formatter,
                    "{relation:?} relation requires a correction scope"
                )
            }
            Self::InvalidEndpoint { relation } => {
                write!(formatter, "{relation:?} relation has identical endpoints")
            }
            Self::UnbalancedConservation { relation, balances } => write!(
                formatter,
                "{relation:?} relation requires explicit balanced conservation (got {balances:?})"
            ),
            Self::ConservationMembersMismatch { relation } => write!(
                formatter,
                "{relation:?} conservation metadata does not match relation members"
            ),
        }
    }
}

impl std::error::Error for RelationError {}

/// Lookup by occurrence without silently selecting one adapter derivation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceLookup<'a> {
    Missing,
    Unique(&'a RawEvidence),
    Multiple(Vec<&'a RawEvidence>),
}

pub type OccurrenceLookup<'a> = EvidenceLookup<'a>;

impl<'a> EvidenceLookup<'a> {
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    pub fn is_unique(&self) -> bool {
        matches!(self, Self::Unique(_))
    }

    pub fn is_multiple(&self) -> bool {
        matches!(self, Self::Multiple(_))
    }

    pub fn unique(self) -> Option<&'a RawEvidence> {
        match self {
            Self::Unique(evidence) => Some(evidence),
            Self::Missing | Self::Multiple(_) => None,
        }
    }

    pub fn all(self) -> Vec<&'a RawEvidence> {
        match self {
            Self::Missing => Vec::new(),
            Self::Unique(evidence) => vec![evidence],
            Self::Multiple(evidence) => evidence,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Missing => 0,
            Self::Unique(_) => 1,
            Self::Multiple(evidence) => evidence.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// An in-memory immutable-evidence index.  The index is mutable as a store,
/// but every inserted [`RawEvidence`] remains immutable and old observations
/// are never replaced by imports or corrections.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EvidenceStore {
    evidence: BTreeMap<StoredKey, RawEvidence>,
    imports: BTreeMap<ImportKey, StoredKey>,
    derivations: BTreeMap<StoredKey, Vec<EvidenceDerivation>>,
    relations: Vec<EvidenceRelation>,
    candidates: Vec<CandidateIdentityLink>,
}

impl EvidenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.evidence.len()
    }

    pub fn is_empty(&self) -> bool {
        self.evidence.is_empty()
    }

    pub fn get(&self, occurrence: &OccurrenceId) -> EvidenceLookup<'_> {
        let evidence: Vec<_> = self
            .evidence
            .iter()
            .filter(|(key, _)| &key.occurrence == occurrence)
            .map(|(_, evidence)| evidence)
            .collect();
        match evidence.len() {
            0 => EvidenceLookup::Missing,
            1 => EvidenceLookup::Unique(evidence[0]),
            _ => EvidenceLookup::Multiple(evidence),
        }
    }

    pub fn all_for_occurrence(&self, occurrence: &OccurrenceId) -> Vec<&RawEvidence> {
        self.get(occurrence).all()
    }

    pub fn contains(&self, occurrence: &OccurrenceId) -> bool {
        self.evidence
            .keys()
            .any(|key| &key.occurrence == occurrence)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RawEvidence> {
        self.evidence.values()
    }

    pub fn evidence(&self) -> impl Iterator<Item = &RawEvidence> {
        self.iter()
    }

    /// Return every distinct derivation recorded for a stored occurrence.
    pub fn derivations_for(&self, occurrence: &OccurrenceId) -> Vec<&EvidenceDerivation> {
        self.derivations
            .iter()
            .filter(|(key, _)| &key.occurrence == occurrence)
            .flat_map(|(_, derivations)| derivations)
            .collect()
    }

    pub fn relations(&self) -> impl Iterator<Item = &EvidenceRelation> {
        self.relations.iter()
    }

    pub fn candidate_links(&self) -> impl Iterator<Item = &CandidateIdentityLink> {
        self.candidates.iter()
    }

    /// Insert one observation using the same deterministic key as batch
    /// import.  Exact re-imports are idempotent; changed content is new
    /// evidence, even when it carries the same external source identifier.
    pub fn insert(&mut self, evidence: RawEvidence) -> Result<ImportDisposition, ImportError> {
        let occurrence = evidence.identity.occurrence.clone();
        let key = ImportKey::from_evidence(&evidence, None);
        let derivation = EvidenceDerivation::from_evidence(&evidence, None);
        if let Some(stored_key) = self.imports.get(&key).cloned() {
            // The exact source observation already exists, even when a caller
            // regenerated a local occurrence ID around the same external row.
            if self.evidence.contains_key(&stored_key) {
                let history = self.derivations.entry(stored_key).or_default();
                if !history.contains(&derivation) {
                    history.push(derivation);
                    history.sort();
                }
                return Ok(ImportDisposition::AlreadyPresent);
            }
        }

        // A source occurrence may be re-derived by a new adapter version.  It
        // is a distinct immutable derivation, not a conflict; the adapter is
        // already part of `key`.  The same occurrence under the same adapter
        // remains protected from accidental replacement.
        if let Some(existing) = self.evidence.values().find(|existing| {
            existing.identity.occurrence == occurrence
                && existing.provenance.adapter == evidence.provenance.adapter
        }) {
            if existing == &evidence {
                return Ok(ImportDisposition::AlreadyPresent);
            }
            return Err(ImportError::OccurrenceConflict {
                occurrence,
                existing: existing.identity.content,
                incoming: evidence.identity.content,
            });
        }

        let stored_key = StoredKey {
            import: key.clone(),
            occurrence,
        };
        self.imports.insert(key, stored_key.clone());
        self.derivations
            .insert(stored_key.clone(), vec![derivation]);
        self.evidence.insert(stored_key, evidence);
        Ok(ImportDisposition::Inserted)
    }

    pub fn import_batch(&mut self, batch: ImportBatch) -> Result<ImportReport, ImportError> {
        let batch_hash = batch.content_hash();
        let batch_adapter = batch.adapter.clone();
        let mut observations: Vec<_> = batch
            .observations
            .into_iter()
            .map(|observation| {
                if observation.provenance.adapter.is_none()
                    && let Some(adapter) = &batch_adapter
                {
                    let provenance = observation.provenance.clone().with_adapter(adapter.clone());
                    return observation.with_provenance(provenance);
                }
                observation
            })
            .collect();
        observations.sort_by(|left, right| {
            ImportKey::from_evidence(left, None).cmp(&ImportKey::from_evidence(right, None))
        });

        // Validate and apply against a clone so a malformed batch cannot
        // leave a prefix of its rows visible.  Import is a single immutable
        // boundary: callers either see every observation or none of them.
        let mut staged = self.clone();
        let mut items = Vec::with_capacity(observations.len());
        for observation in observations {
            if observation.provenance.source != batch.source {
                return Err(ImportError::SourceMismatch {
                    expected: batch.source.clone(),
                    actual: observation.provenance.source.clone(),
                });
            }
            let occurrence = observation.identity.occurrence.clone();
            let disposition = staged.insert(observation)?;
            items.push(ImportItem {
                occurrence,
                disposition,
            });
        }
        items.sort_by(|left, right| left.occurrence.cmp(&right.occurrence));
        *self = staged;
        Ok(ImportReport {
            batch: batch_hash,
            items,
        })
    }

    /// Add an explicit relation.  Duplicate relation insertion is harmless;
    /// no relation insertion alters the evidence set.
    pub fn add_relation(&mut self, relation: EvidenceRelation) -> Result<bool, RelationError> {
        if matches!(
            relation.kind,
            EvidenceRelationKind::Corrects | EvidenceRelationKind::Supersedes
        ) && relation.scope.is_none()
        {
            return Err(RelationError::MissingScope {
                relation: relation.kind,
            });
        }
        if relation.source == relation.target {
            return Err(RelationError::InvalidEndpoint {
                relation: relation.kind,
            });
        }
        for identity in relation.sources().chain(relation.targets()) {
            if !self.contains_identity(identity) {
                return Err(RelationError::MissingEndpoint {
                    identity: identity.clone(),
                });
            }
        }
        if matches!(
            relation.kind,
            EvidenceRelationKind::Splits | EvidenceRelationKind::Merges
        ) {
            let Some(conservation) = relation.conservation.as_ref() else {
                return Err(RelationError::UnbalancedConservation {
                    relation: relation.kind,
                    balances: None,
                });
            };
            let mut expected_inputs: Vec<_> = relation.sources().cloned().collect();
            let mut expected_outputs: Vec<_> = relation.targets().cloned().collect();
            let mut actual_inputs: Vec<_> = conservation
                .inputs
                .iter()
                .map(|leg| leg.identity.clone())
                .collect();
            let mut actual_outputs: Vec<_> = conservation
                .outputs
                .iter()
                .map(|leg| leg.identity.clone())
                .collect();
            expected_inputs.sort_by(identity_order);
            expected_outputs.sort_by(identity_order);
            actual_inputs.sort_by(identity_order);
            actual_outputs.sort_by(identity_order);
            if expected_inputs != actual_inputs || expected_outputs != actual_outputs {
                return Err(RelationError::ConservationMembersMismatch {
                    relation: relation.kind,
                });
            }
            let balances = relation
                .conservation
                .as_ref()
                .and_then(ConservationMetadata::balances);
            if balances != Some(true) {
                return Err(RelationError::UnbalancedConservation {
                    relation: relation.kind,
                    balances,
                });
            }
        }
        if self.relations.contains(&relation) {
            Ok(false)
        } else {
            self.relations.push(relation);
            self.relations.sort_by(relation_order);
            Ok(true)
        }
    }

    fn contains_identity(&self, identity: &Identity) -> bool {
        self.evidence
            .values()
            .any(|evidence| evidence.identity == *identity)
    }

    /// Propose a possible identity match.  This stores both leaves unchanged
    /// and intentionally does not add a `SameAs` relation.
    pub fn propose_identity_link(
        &mut self,
        left: Identity,
        right: Identity,
        confidence: Confidence,
        provenance: Provenance,
        authority: Authority,
    ) -> Result<bool, RelationError> {
        if !self.contains_identity(&left) {
            return Err(RelationError::MissingEndpoint { identity: left });
        }
        if !self.contains_identity(&right) {
            return Err(RelationError::MissingEndpoint { identity: right });
        }
        self.add_candidate_link(CandidateIdentityLink::new(
            left, right, confidence, provenance, authority,
        ))
    }

    pub fn add_candidate_link(
        &mut self,
        candidate: CandidateIdentityLink,
    ) -> Result<bool, RelationError> {
        if !self.contains_identity(&candidate.left) {
            return Err(RelationError::MissingEndpoint {
                identity: candidate.left.clone(),
            });
        }
        if !self.contains_identity(&candidate.right) {
            return Err(RelationError::MissingEndpoint {
                identity: candidate.right.clone(),
            });
        }
        if self.candidates.contains(&candidate) {
            Ok(false)
        } else {
            self.candidates.push(candidate);
            self.candidates.sort_by(candidate_order);
            Ok(true)
        }
    }

    pub fn links_for(
        &self,
        occurrence: &OccurrenceId,
    ) -> impl Iterator<Item = &CandidateIdentityLink> {
        self.candidates.iter().filter(move |candidate| {
            candidate.left.occurrence == *occurrence || candidate.right.occurrence == *occurrence
        })
    }

    pub fn relations_for(
        &self,
        occurrence: &OccurrenceId,
    ) -> impl Iterator<Item = &EvidenceRelation> {
        self.relations.iter().filter(move |relation| {
            relation.source.occurrence == *occurrence
                || relation.target.occurrence == *occurrence
                || relation
                    .related
                    .iter()
                    .any(|identity| identity.occurrence == *occurrence)
        })
    }
}

fn relation_order(left: &EvidenceRelation, right: &EvidenceRelation) -> Ordering {
    left.kind
        .cmp(&right.kind)
        .then_with(|| identity_order(&left.source, &right.source))
        .then_with(|| identity_order(&left.target, &right.target))
        .then_with(|| {
            left.related
                .iter()
                .map(identity_sort_key)
                .cmp(right.related.iter().map(identity_sort_key))
        })
}

fn identity_sort_key(identity: &Identity) -> (&OccurrenceId, &ContentHash, Option<&ExternalId>) {
    (
        &identity.occurrence,
        &identity.content,
        identity.external.as_ref(),
    )
}

fn identity_order(left: &Identity, right: &Identity) -> Ordering {
    identity_sort_key(left).cmp(&identity_sort_key(right))
}

fn candidate_order(left: &CandidateIdentityLink, right: &CandidateIdentityLink) -> Ordering {
    left.left
        .occurrence
        .cmp(&right.left.occurrence)
        .then_with(|| left.right.occurrence.cmp(&right.right.occurrence))
        .then_with(|| right.confidence.cmp(&left.confidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(source: &str, occurrence: &str, external: &str, bytes: &[u8]) -> RawEvidence {
        RawEvidence::from_bytes(source, occurrence, Some(ExternalId::new(external)), bytes)
    }

    fn batch(source: &str, rows: impl IntoIterator<Item = RawEvidence>) -> ImportBatch {
        ImportBatch::from_observations(source, rows)
    }

    #[test]
    fn repeating_same_external_row_is_idempotent() {
        let observation = row("bank", "occurrence-a", "row-184", b"2026-09-18,20.00");
        let mut store = EvidenceStore::new();
        let first = store
            .import_batch(batch("bank", [observation.clone()]))
            .unwrap();
        let second = store
            .import_batch(batch(
                "bank",
                [
                    // A regenerated local occurrence still denotes the same
                    // external row and content.
                    row(
                        "bank",
                        "regenerated-occurrence",
                        "row-184",
                        b"2026-09-18,20.00",
                    ),
                ],
            ))
            .unwrap();
        assert_eq!(first.inserted_count(), 1);
        assert_eq!(second.existing_count(), 1);
        assert_eq!(store.len(), 1);
        assert!(store.contains(&OccurrenceId::new("occurrence-a")));
    }

    #[test]
    fn failed_batch_is_atomic_and_does_not_leave_a_prefix() {
        let first = row("bank", "first", "row-1", b"first");
        let wrong_source = row("other", "second", "row-2", b"second");
        let mut store = EvidenceStore::new();
        let result = store.import_batch(batch("bank", [first, wrong_source]));
        assert!(matches!(result, Err(ImportError::SourceMismatch { .. })));
        assert!(store.is_empty());
    }

    #[test]
    fn changed_provenance_is_retained_without_duplicating_the_source_row() {
        let first =
            row("bank", "occurrence-a", "row-184", b"same").with_note("first adapter observation");
        let second =
            row("bank", "regenerated", "row-184", b"same").with_note("reviewed after import");
        let mut store = EvidenceStore::new();
        assert_eq!(store.insert(first).unwrap(), ImportDisposition::Inserted);
        assert_eq!(
            store.insert(second).unwrap(),
            ImportDisposition::AlreadyPresent
        );
        assert_eq!(store.len(), 1);
        let derivations = store.derivations_for(&OccurrenceId::new("occurrence-a"));
        assert_eq!(derivations.len(), 2);
        assert_eq!(
            derivations
                .iter()
                .filter_map(|derivation| derivation.note.as_deref())
                .collect::<Vec<_>>(),
            vec!["first adapter observation", "reviewed after import"]
        );
    }

    #[test]
    fn same_row_under_new_adapter_version_retains_derivation_history() {
        let observation = row("bank", "occurrence-a", "row-184", b"same");
        let mut store = EvidenceStore::new();
        let v1 = ImportBatch::from_observations("bank", [observation.clone()])
            .with_adapter(AdapterProvenance::new("bank-csv", "1"));
        let v1_repeat = ImportBatch::from_observations("bank", [observation.clone()])
            .with_adapter(AdapterProvenance::new("bank-csv", "1"));
        let v2 = ImportBatch::from_observations("bank", [observation])
            .with_adapter(AdapterProvenance::new("bank-csv", "2"));

        assert_eq!(store.import_batch(v1).unwrap().inserted_count(), 1);
        assert_eq!(store.import_batch(v1_repeat).unwrap().existing_count(), 1);
        assert_eq!(store.import_batch(v2).unwrap().inserted_count(), 1);
        assert_eq!(store.len(), 2);
        let adapters: Vec<_> = store
            .iter()
            .filter_map(|evidence| evidence.provenance().adapter.as_ref())
            .map(|adapter| adapter.version.as_str())
            .collect();
        assert_eq!(adapters, vec!["1", "2"]);
    }

    #[test]
    fn occurrence_lookup_preserves_adapter_derivation_ambiguity() {
        let observation = row("bank", "occurrence-a", "row-184", b"same");
        let mut store = EvidenceStore::new();
        store
            .import_batch(
                ImportBatch::from_observations("bank", [observation.clone()])
                    .with_adapter(AdapterProvenance::new("bank-csv", "1")),
            )
            .unwrap();
        store
            .import_batch(
                ImportBatch::from_observations("bank", [observation])
                    .with_adapter(AdapterProvenance::new("bank-csv", "2")),
            )
            .unwrap();
        let lookup = store.get(&OccurrenceId::new("occurrence-a"));
        assert!(lookup.is_multiple());
        assert_eq!(lookup.len(), 2);
        assert!(lookup.unique().is_none());
        assert_eq!(
            store
                .all_for_occurrence(&OccurrenceId::new("occurrence-a"))
                .len(),
            2
        );
    }

    #[test]
    fn occurrence_conflict_is_checked_within_each_adapter_version() {
        let original = row("bank", "occurrence-a", "row-184", b"same");
        let changed = row("bank", "occurrence-a", "row-184", b"changed");
        let mut store = EvidenceStore::new();
        store
            .import_batch(
                ImportBatch::from_observations("bank", [original.clone()])
                    .with_adapter(AdapterProvenance::new("bank-csv", "1")),
            )
            .unwrap();
        store
            .import_batch(
                ImportBatch::from_observations("bank", [original])
                    .with_adapter(AdapterProvenance::new("bank-csv", "2")),
            )
            .unwrap();

        let result = store.import_batch(
            ImportBatch::from_observations("bank", [changed])
                .with_adapter(AdapterProvenance::new("bank-csv", "2")),
        );
        assert!(matches!(
            result,
            Err(ImportError::OccurrenceConflict { .. })
        ));
        assert_eq!(store.len(), 2);
        assert_eq!(
            store
                .derivations_for(&OccurrenceId::new("occurrence-a"))
                .len(),
            2
        );
    }

    #[test]
    fn identical_distinct_source_rows_remain_distinct_occurrences() {
        let first = row("bank", "occurrence-a", "row-184", b"same");
        let second = row("bank", "occurrence-b", "row-185", b"same");
        let mut store = EvidenceStore::new();
        let report = store.import_batch(batch("bank", [first, second])).unwrap();
        assert_eq!(report.inserted_count(), 2);
        assert_eq!(store.len(), 2);
        assert_eq!(
            store
                .get(&OccurrenceId::new("occurrence-a"))
                .unique()
                .unwrap()
                .content(),
            store
                .get(&OccurrenceId::new("occurrence-b"))
                .unique()
                .unwrap()
                .content()
        );
    }

    #[test]
    fn receipt_and_bank_candidate_link_retains_both_leaves() {
        let receipt = row("receipts", "receipt-1", "receipt-1", b"coffee 4.00");
        let bank = row("bank", "bank-1", "row-1", b"CARD COFFEE 4.00");
        let receipt_id = receipt.identity().clone();
        let bank_id = bank.identity().clone();
        let mut store = EvidenceStore::new();
        store.import_batch(batch("receipts", [receipt])).unwrap();
        store.import_batch(batch("bank", [bank])).unwrap();
        assert!(
            store
                .propose_identity_link(
                    receipt_id.clone(),
                    bank_id.clone(),
                    Confidence::from_basis_points(9_100),
                    Provenance::new("matcher"),
                    Authority::user("reviewer"),
                )
                .unwrap()
        );
        assert_eq!(store.len(), 2);
        assert_eq!(store.candidate_links().count(), 1);
        assert!(!store.get(&receipt_id.occurrence).is_missing());
        assert!(!store.get(&bank_id.occurrence).is_missing());
        assert!(store.relations().next().is_none());
    }

    #[test]
    fn candidate_link_rejects_missing_endpoint_without_mutating_store() {
        let present = row("bank", "present", "row-1", b"present");
        let present_id = present.identity().clone();
        let missing_id = Identity::new(
            "missing",
            ContentHash::domain_separated("test", b"missing-candidate"),
        );
        let mut store = EvidenceStore::new();
        store.import_batch(batch("bank", [present])).unwrap();
        let result = store.propose_identity_link(
            present_id,
            missing_id.clone(),
            Confidence::from_basis_points(5_000),
            Provenance::new("matcher"),
            Authority::user("reviewer"),
        );
        assert_eq!(
            result,
            Err(RelationError::MissingEndpoint {
                identity: missing_id,
            })
        );
        assert_eq!(store.candidate_links().count(), 0);
    }

    #[test]
    fn corrected_statement_keeps_old_evidence_and_scope() {
        let old = row(
            "statement",
            "statement-old",
            "statement-2026-09",
            b"balance=100",
        );
        let corrected = row(
            "statement",
            "statement-corrected",
            "statement-2026-09",
            b"balance=110",
        );
        let old_id = old.identity().clone();
        let corrected_id = corrected.identity().clone();
        let mut store = EvidenceStore::new();
        store.import_batch(batch("statement", [old])).unwrap();
        store.import_batch(batch("statement", [corrected])).unwrap();
        let relation = EvidenceRelation::supersedes(
            corrected_id,
            old_id,
            CorrectionScope::field("balance").with_rationale("issuer correction"),
        );
        assert!(store.add_relation(relation).unwrap());
        assert_eq!(store.len(), 2);
        let relation = store.relations().next().unwrap();
        assert_eq!(relation.kind, EvidenceRelationKind::Supersedes);
        assert_eq!(relation.scope.as_ref().unwrap().fields, vec!["balance"]);
    }

    #[test]
    fn relation_insertion_rejects_missing_endpoints_without_mutating_store() {
        let present = row("bank", "present", "row-1", b"present");
        let present_id = present.identity().clone();
        let missing_id =
            Identity::new("missing", ContentHash::domain_separated("test", b"missing"));
        let mut store = EvidenceStore::new();
        store.import_batch(batch("bank", [present])).unwrap();
        let relation = EvidenceRelation::same_as(present_id, missing_id.clone());
        assert_eq!(
            store.add_relation(relation),
            Err(RelationError::MissingEndpoint {
                identity: missing_id,
            })
        );
        assert_eq!(store.relations().count(), 0);
    }

    #[test]
    fn split_relation_checks_every_child_endpoint() {
        let parent = row("bank", "parent", "row-parent", b"parent");
        let child = row("bank", "child", "row-child", b"child");
        let parent_id = parent.identity().clone();
        let child_id = child.identity().clone();
        let missing = Identity::new(
            "missing-child",
            ContentHash::domain_separated("test", b"missing-child"),
        );
        let mut store = EvidenceStore::new();
        store.import_batch(batch("bank", [parent, child])).unwrap();
        let relation = EvidenceRelation::splits(
            parent_id,
            [child_id, missing.clone()],
            ConservationMetadata::for_split(
                Identity::new("parent", ContentHash::domain_separated("test", b"parent")),
                [missing.clone()],
            ),
        )
        .unwrap();
        assert_eq!(
            store.add_relation(relation),
            Err(RelationError::MissingEndpoint { identity: missing })
        );
        assert_eq!(store.relations().count(), 0);
    }

    #[test]
    fn tombstone_constructor_rejects_present_availability_without_panicking() {
        let identity = Identity::new("occurrence", ContentHash::domain_separated("test", b"x"));
        let result = RawEvidence::tombstone(
            identity,
            Provenance::new("source"),
            Authority::source("source"),
            Availability::Present,
        );
        assert_eq!(
            result,
            Err(EvidenceError::InvalidTombstoneAvailability {
                availability: Availability::Present,
            })
        );
    }

    #[test]
    fn split_and_merge_carry_conservation_metadata() {
        let parent = Identity::new("parent", ContentHash::domain_separated("test", b"parent"));
        let child_a = Identity::new("child-a", ContentHash::domain_separated("test", b"a"));
        let child_b = Identity::new("child-b", ContentHash::domain_separated("test", b"b"));
        let conservation = ConservationMetadata::new(
            [ConservationLeg::new(parent.clone())
                .with_quantity(ExactNumber::integer(10))
                .with_unit("USD")],
            [
                ConservationLeg::new(child_a.clone())
                    .with_quantity(ExactNumber::integer(4))
                    .with_unit("USD"),
                ConservationLeg::new(child_b.clone())
                    .with_quantity(ExactNumber::integer(6))
                    .with_unit("USD"),
            ],
        );
        assert_eq!(conservation.balances(), Some(true));
        let split = EvidenceRelation::splits(
            parent.clone(),
            [child_a.clone(), child_b.clone()],
            conservation,
        )
        .unwrap();
        assert_eq!(split.kind, EvidenceRelationKind::Splits);
        assert_eq!(split.targets().count(), 2);
        assert!(split.conservation.is_some());

        let merged = EvidenceRelation::merges(
            [child_a.clone(), child_b.clone()],
            parent.clone(),
            ConservationMetadata::for_merge([child_a, child_b], parent),
        )
        .unwrap();
        assert_eq!(merged.kind, EvidenceRelationKind::Merges);
        assert_eq!(merged.sources().count(), 2);
        assert_eq!(merged.related.len(), 1);
        assert!(merged.conservation.is_some());
    }

    #[test]
    fn split_insertion_rejects_unknown_or_unbalanced_conservation() {
        let parent = row("bank", "parent", "row-parent", b"parent");
        let child = row("bank", "child", "row-child", b"child");
        let parent_id = parent.identity().clone();
        let child_id = child.identity().clone();
        let mut store = EvidenceStore::new();
        store.import_batch(batch("bank", [parent, child])).unwrap();

        let unknown = EvidenceRelation::splits(
            parent_id.clone(),
            [child_id.clone()],
            ConservationMetadata::for_split(parent_id.clone(), [child_id.clone()]),
        )
        .unwrap();
        assert_eq!(
            store.add_relation(unknown),
            Err(RelationError::UnbalancedConservation {
                relation: EvidenceRelationKind::Splits,
                balances: None,
            })
        );

        let unbalanced = EvidenceRelation::splits(
            parent_id.clone(),
            [child_id.clone()],
            ConservationMetadata::new(
                [ConservationLeg::new(parent_id.clone())
                    .with_quantity(ExactNumber::integer(10))
                    .with_unit("USD")],
                [ConservationLeg::new(child_id.clone())
                    .with_quantity(ExactNumber::integer(9))
                    .with_unit("USD")],
            ),
        )
        .unwrap();
        assert_eq!(
            store.add_relation(unbalanced),
            Err(RelationError::UnbalancedConservation {
                relation: EvidenceRelationKind::Splits,
                balances: Some(false),
            })
        );
        assert_eq!(store.relations().count(), 0);
    }

    #[test]
    fn correction_relations_require_scope_and_split_metadata_members() {
        let left = row("bank", "left", "row-left", b"left");
        let right = row("bank", "right", "row-right", b"right");
        let left_id = left.identity().clone();
        let right_id = right.identity().clone();
        let mut store = EvidenceStore::new();
        store.import_batch(batch("bank", [left, right])).unwrap();
        let missing_scope = EvidenceRelation::new(
            EvidenceRelationKind::Corrects,
            left_id.clone(),
            right_id.clone(),
        );
        assert_eq!(
            store.add_relation(missing_scope),
            Err(RelationError::MissingScope {
                relation: EvidenceRelationKind::Corrects,
            })
        );
        let forged_metadata = EvidenceRelation::splits(
            left_id.clone(),
            [right_id.clone()],
            ConservationMetadata::for_split(
                Identity::new(
                    "not-left",
                    ContentHash::domain_separated("test", b"not-left"),
                ),
                [right_id],
            ),
        )
        .unwrap();
        assert_eq!(
            store.add_relation(forged_metadata),
            Err(RelationError::ConservationMembersMismatch {
                relation: EvidenceRelationKind::Splits,
            })
        );
    }

    #[test]
    fn conservation_rejects_cross_unit_pseudo_balance() {
        let input = Identity::new("input", ContentHash::domain_separated("test", b"input"));
        let output = Identity::new("output", ContentHash::domain_separated("test", b"output"));
        let metadata = ConservationMetadata::new(
            [ConservationLeg::new(input)
                .with_quantity(ExactNumber::integer(10))
                .with_unit("USD")],
            [ConservationLeg::new(output)
                .with_quantity(ExactNumber::integer(10))
                .with_unit("EUR")],
        );
        assert_eq!(metadata.balances(), Some(false));
    }

    #[test]
    fn raw_evidence_rejects_inconsistent_availability_and_payload() {
        let identity = Identity::new("occurrence", ContentHash::domain_separated("test", b"x"));
        let result = RawEvidence::new(
            identity.clone(),
            Provenance::new("source"),
            Authority::source("source"),
            Availability::Deleted,
            Some(b"must not be retained".to_vec()),
        );
        assert!(matches!(
            result,
            Err(EvidenceError::AvailabilityPayloadMismatch {
                availability: Availability::Deleted,
                has_payload: true
            })
        ));
        let present = RawEvidence::new(
            identity,
            Provenance::new("source"),
            Authority::source("source"),
            Availability::Present,
            None,
        );
        assert!(present.is_err());
    }

    #[test]
    fn deletion_is_a_tombstone_and_preserves_identity() {
        let present = row("documents", "doc-old", "doc-1", b"secret");
        let identity = present.identity().clone();
        let tombstone = RawEvidence::deleted_tombstone(
            "documents",
            "doc-deletion",
            Some(ExternalId::new("doc-1")),
            identity.content,
        )
        .unwrap();
        assert_eq!(tombstone.availability(), Availability::Deleted);
        assert!(tombstone.payload().is_none());
        assert_eq!(tombstone.content(), identity.content);
        let mut store = EvidenceStore::new();
        store.import_batch(batch("documents", [present])).unwrap();
        store
            .import_batch(batch("documents", [tombstone.clone()]))
            .unwrap();
        assert_eq!(store.len(), 2);
        assert_eq!(
            store
                .get(&OccurrenceId::new("doc-old"))
                .unique()
                .unwrap()
                .availability(),
            Availability::Present
        );
        assert_eq!(
            store
                .get(&OccurrenceId::new("doc-deletion"))
                .unique()
                .unwrap()
                .availability(),
            Availability::Deleted
        );
    }
}
