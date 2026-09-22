//! Independently checked proof and persistence payload for SettlementStateV1.
//!
//! This module is intentionally separate from `proof.rs`.  The latter is the
//! closed proof-v1 DAG used by the sale engine; settlement-state forms have a
//! different source binding and must not acquire authority by looking like a
//! sale proof node.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::elaboration;
use crate::exact::ExactNumber;
use crate::ir::{Record, Term};
use crate::model::{
    ContentHash, Date, EntityId, ExternalId, InstrumentId, OccurrenceId, Quantity, SettlementKind,
    SourceId,
};
use crate::ontology::SettlementState;
use crate::package_compiler::{CompiledArtifact, RecordSchema, SchemaCapability};
use crate::settlement_projection::{
    CapableSettlementForm, SettlementProjection, SettlementProjectionError,
};
use crate::store::{CommitId, CompiledArtifactId, EvidenceId, ObjectStore, StoreError};
use crate::surface::SurfaceFile;
use crate::workspace::BoundPackageForms;

/// The independent settlement proof format.  This is not proof-format-v1.
pub const SETTLEMENT_STATE_PROOF_VERSION: &str = "axiom/settlement-state-proof/v1";
/// Conservative fixed limits for an in-memory proof object.  These are part
/// of the checker boundary, not a generic resource framework.
pub const MAX_SETTLEMENT_PROOF_ROWS: usize = 4096;
pub const MAX_SETTLEMENT_PROOF_SOURCE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES: usize = 256;
pub const MAX_SETTLEMENT_PROOF_EXACT_BYTES: usize = 4096;
pub const MAX_SETTLEMENT_PROOF_CANONICAL_BYTES: usize = 1_048_576;
const OCCURRENCE_HASH_DOMAIN: &str = "axiom/settlement-state-proof/occurrence/v1";
const COVERAGE_HASH_DOMAIN: &str = "axiom/settlement-state-proof/coverage/v1";

/// The exact source evidence identity from which the proof's forms were
/// elaborated.  Occurrence and content remain separate identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEvidenceIdentity {
    pub evidence: EvidenceId,
    pub source: SourceId,
    pub occurrence: OccurrenceId,
    pub content_hash: ContentHash,
    pub external: Option<ExternalId>,
}

/// One ordered, typed capable-form entry in a SettlementStateV1 proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementStateV1Entry {
    pub occurrence: OccurrenceId,
    pub occurrence_hash: ContentHash,
    pub value_hash: ContentHash,
    pub package_root: ContentHash,
    pub qualified_schema: String,
    pub schema_id: ContentHash,
    pub settlement: String,
    pub rail: SettlementKind,
    pub state: SettlementState,
    pub date: Date,
    pub from: EntityId,
    pub to: EntityId,
    pub instrument: InstrumentId,
    pub amount: Quantity,
}

/// A self-contained proof of one exact source/artifact/schema capable-form
/// projection.  `coverage` is source order and is never sorted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementStateV1Proof {
    pub version: String,
    pub source_commit: CommitId,
    pub compiled_artifact: CompiledArtifactId,
    pub compiled_artifact_hash: ContentHash,
    pub source_evidence: SourceEvidenceIdentity,
    pub coverage: Vec<SettlementStateV1Entry>,
    pub coverage_hash: ContentHash,
}

impl Ord for SettlementStateV1Proof {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical_bytes().cmp(&other.canonical_bytes())
    }
}

impl PartialOrd for SettlementStateV1Proof {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Errors returned by construction and independent checking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementProofError {
    Store(StoreError),
    Projection(SettlementProjectionError),
    Invalid(String),
}

impl fmt::Display for SettlementProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "settlement proof store error: {error}"),
            Self::Projection(error) => {
                write!(formatter, "settlement proof projection error: {error}")
            }
            Self::Invalid(reason) => write!(formatter, "invalid SettlementStateV1 proof: {reason}"),
        }
    }
}

impl std::error::Error for SettlementProofError {}

impl From<StoreError> for SettlementProofError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<SettlementProjectionError> for SettlementProofError {
    fn from(error: SettlementProjectionError) -> Self {
        Self::Projection(error)
    }
}

impl SettlementStateV1Proof {
    /// Build a persistable proof with the exact EvidenceId pinned by the
    /// source commit.  RawEvidence intentionally does not expose a store
    /// address, so workspace callers provide it at this boundary.
    pub(crate) fn from_projection(
        projection: &SettlementProjection,
        source: &crate::evidence::RawEvidence,
        evidence: EvidenceId,
    ) -> Result<Self, SettlementProofError> {
        let forms = projection.capable_forms();
        if forms.is_empty() {
            return Err(invalid("capable-form coverage cannot be empty"));
        }
        if forms.len() > MAX_SETTLEMENT_PROOF_ROWS {
            return Err(invalid("resource limit: too many settlement proof rows"));
        }
        let coverage = forms.iter().map(entry_from_form).collect::<Vec<_>>();
        let source_evidence = SourceEvidenceIdentity {
            evidence,
            source: source.source().clone(),
            occurrence: source.occurrence().clone(),
            content_hash: source.content(),
            external: source.identity().external.clone(),
        };
        let mut proof = Self {
            version: SETTLEMENT_STATE_PROOF_VERSION.to_owned(),
            source_commit: projection.source_commit(),
            compiled_artifact: projection.compiled_artifact(),
            compiled_artifact_hash: projection.artifact_hash(),
            source_evidence,
            coverage,
            coverage_hash: ContentHash::ZERO,
        };
        proof.coverage_hash = proof.recompute_coverage_hash();
        proof.check_payload()?;
        Ok(proof)
    }

    pub fn coverage(&self) -> &[SettlementStateV1Entry] {
        &self.coverage
    }

    pub fn recompute_coverage_hash(&self) -> ContentHash {
        let mut bytes = Vec::new();
        put_u64(&mut bytes, self.coverage.len() as u64);
        for entry in &self.coverage {
            encode_entry(&mut bytes, entry);
        }
        ContentHash::domain_separated(COVERAGE_HASH_DOMAIN, &bytes)
    }

    /// Canonical payload used by the dedicated store object family.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_string(&mut out, &self.version);
        put_hash(&mut out, self.source_commit.hash());
        put_hash(&mut out, self.compiled_artifact.hash());
        put_hash(&mut out, self.compiled_artifact_hash);
        put_hash(&mut out, self.source_evidence.evidence.hash());
        put_string(&mut out, self.source_evidence.source.as_str());
        put_string(&mut out, self.source_evidence.occurrence.as_str());
        put_hash(&mut out, self.source_evidence.content_hash);
        put_optional_string(
            &mut out,
            self.source_evidence
                .external
                .as_ref()
                .map(ExternalId::as_str),
        );
        put_u64(&mut out, self.coverage.len() as u64);
        for entry in &self.coverage {
            encode_entry(&mut out, entry);
        }
        put_hash(&mut out, self.coverage_hash);
        out
    }

    /// Check the payload's local invariants, then independently re-elaborate
    /// the exact source bytes and artifact pinned by the store.  This makes a
    /// reordered, omitted, or forged capable-form entry fail even when an
    /// attacker recomputes the certificate's content hash.
    pub fn check(&self, store: &ObjectStore) -> Result<(), SettlementProofError> {
        self.check_payload()?;
        store.validate_source_snapshot(self.source_commit)?;
        let commit = store.commit(self.source_commit)?;
        if commit.compiled_artifact != Some(self.compiled_artifact) {
            return Err(invalid("source commit does not bind the compiled artifact"));
        }
        if commit.evidence.len() != 1 {
            return Err(invalid(
                "source commit must bind exactly one source evidence object",
            ));
        }
        if commit.evidence[0] != self.source_evidence.evidence {
            return Err(invalid(
                "source evidence object address does not match the certificate",
            ));
        }
        let evidence = store.evidence(commit.evidence[0])?;
        if evidence.normalized_content
            != crate::evidence::RawEvidence::content_hash(&evidence.content)
            || evidence.source != self.source_evidence.source.as_str()
            || evidence.occurrence != self.source_evidence.occurrence.as_str()
            || evidence.normalized_content != self.source_evidence.content_hash
            || evidence.external != self.source_evidence.external
            || !matches!(
                evidence.state,
                crate::store::EvidenceState::Present
                    | crate::store::EvidenceState::Correction { .. }
            )
        {
            return Err(invalid(
                "source evidence identity does not match the certificate",
            ));
        }
        if evidence.content.is_empty() {
            return Err(invalid("source evidence has no payload"));
        }
        if evidence.content.len() > MAX_SETTLEMENT_PROOF_CANONICAL_BYTES {
            return Err(invalid(
                "resource limit: source evidence payload is too large",
            ));
        }

        let artifact_object = store.compiled_artifact(self.compiled_artifact)?;
        if artifact_object.artifact_hash() != self.compiled_artifact_hash {
            return Err(invalid(
                "compiled artifact hash does not match the certificate",
            ));
        }
        for entry in &self.coverage {
            let schema = resolve_schema(
                &artifact_object.artifact,
                entry.package_root,
                &entry.qualified_schema,
            )?;
            if schema.schema_id() != entry.schema_id
                || schema.capability() != Some(SchemaCapability::SettlementStateV1)
            {
                return Err(invalid(
                    "schema identity or capability does not match the certificate",
                ));
            }
        }

        let source = std::str::from_utf8(&evidence.content)
            .map_err(|_| invalid("source evidence payload is not UTF-8"))?;
        let forms =
            elaboration::elaborate_document(&SurfaceFile::parse(source), &artifact_object.artifact)
                .map_err(|error| {
                    invalid(format!("source forms cannot be re-elaborated: {error}"))
                })?;
        let bound = BoundPackageForms {
            source_commit: self.source_commit,
            compiled_artifact: self.compiled_artifact,
            artifact_hash: self.compiled_artifact_hash,
            forms,
        };
        let mut expected = Vec::new();
        for form in bound.forms() {
            if form.schema().capability() != Some(SchemaCapability::SettlementStateV1) {
                continue;
            }
            let qualified_schema = form.schema().qualified_name().canonical();
            let schema = resolve_schema(
                &artifact_object.artifact,
                form.package_root(),
                &qualified_schema,
            )?;
            if schema.schema_id() != form.schema().schema_id()
                || schema.capability() != Some(SchemaCapability::SettlementStateV1)
                || form.value().schema_id() != form.schema().schema_id()
            {
                return Err(invalid(
                    "re-elaborated form schema identity is inconsistent",
                ));
            }
            expected.push(entry_from_elaborated_form(form)?);
        }
        validate_independent_history(&expected)?;
        if expected.len() != self.coverage.len() {
            return Err(invalid("certificate coverage is not exact"));
        }
        for (expected, actual) in expected.iter().zip(&self.coverage) {
            if expected != actual {
                return Err(invalid("certificate coverage entry is reordered or forged"));
            }
        }
        Ok(())
    }

    fn check_payload(&self) -> Result<(), SettlementProofError> {
        self.check_resource_limits()?;
        if self.version != SETTLEMENT_STATE_PROOF_VERSION {
            return Err(invalid("unsupported proof version"));
        }
        if self.coverage.is_empty() {
            return Err(invalid("coverage cannot be empty"));
        }
        if self.coverage_hash != self.recompute_coverage_hash() {
            return Err(invalid("coverage hash does not match ordered coverage"));
        }
        if self.source_evidence.evidence.hash() == ContentHash::ZERO
            || self.source_evidence.content_hash == ContentHash::ZERO
            || !canonical_identifier(self.source_evidence.source.as_str())
            || !canonical_identifier(self.source_evidence.occurrence.as_str())
        {
            return Err(invalid("source evidence identity is not canonical"));
        }
        let mut occurrences = BTreeSet::new();
        for entry in &self.coverage {
            if !occurrences.insert(entry.occurrence.clone()) {
                return Err(invalid("coverage contains a duplicate occurrence"));
            }
            if entry.occurrence_hash != occurrence_hash(&entry.occurrence) {
                return Err(invalid("occurrence hash does not match occurrence"));
            }
            if !canonical_identifier(entry.occurrence.as_str()) {
                return Err(invalid("form occurrence is not canonical"));
            }
            if entry.value_hash == ContentHash::ZERO {
                return Err(invalid("value hash cannot be zero"));
            }
            if Date::new(entry.date.year, entry.date.month, entry.date.day).is_err() {
                return Err(invalid("form date is not a valid civil date"));
            }
            if !canonical_identifier(&entry.settlement)
                || !canonical_identifier(entry.from.as_str())
                || !canonical_identifier(entry.to.as_str())
                || !canonical_identifier(entry.instrument.as_str())
            {
                return Err(invalid("typed settlement identity cannot be empty"));
            }
            if entry.amount.number.is_negative()
                || entry.amount.number.is_zero()
                || entry.amount.unit().map(ToString::to_string).as_deref()
                    != Some(entry.instrument.as_str())
            {
                return Err(invalid(
                    "amount must be positive and denominated by instrument",
                ));
            }
        }
        validate_independent_history(&self.coverage)
    }

    fn check_resource_limits(&self) -> Result<(), SettlementProofError> {
        if self.coverage.len() > MAX_SETTLEMENT_PROOF_ROWS {
            return Err(invalid("resource limit: too many settlement proof rows"));
        }
        for value in [
            self.version.as_str(),
            self.source_evidence.source.as_str(),
            self.source_evidence.occurrence.as_str(),
        ] {
            if value.len() > MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES {
                return Err(invalid("resource limit: identifier is too large"));
            }
        }
        if self
            .source_evidence
            .external
            .as_ref()
            .is_some_and(|value| value.as_str().len() > MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES)
        {
            return Err(invalid("resource limit: external identity is too large"));
        }
        for entry in &self.coverage {
            for value in [
                entry.occurrence.as_str(),
                entry.qualified_schema.as_str(),
                entry.settlement.as_str(),
                entry.from.as_str(),
                entry.to.as_str(),
                entry.instrument.as_str(),
            ] {
                if value.len() > MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES {
                    return Err(invalid("resource limit: typed identity is too large"));
                }
            }
            if entry.amount.number.canonical_string().len() > MAX_SETTLEMENT_PROOF_EXACT_BYTES {
                return Err(invalid("resource limit: exact amount is too large"));
            }
            if entry.amount.unit().is_some_and(|value| {
                value.to_string().len() > MAX_SETTLEMENT_PROOF_IDENTIFIER_BYTES
            }) {
                return Err(invalid("resource limit: amount unit is too large"));
            }
        }
        if self.canonical_bytes().len() > MAX_SETTLEMENT_PROOF_CANONICAL_BYTES {
            return Err(invalid(
                "resource limit: canonical proof payload is too large",
            ));
        }
        Ok(())
    }
}

fn resolve_schema(
    artifact: &CompiledArtifact,
    package_root: ContentHash,
    qualified_schema: &str,
) -> Result<RecordSchema, SettlementProofError> {
    let qualified_name = parse_qualified_schema(qualified_schema)?;
    artifact
        .resolve_record_schema(package_root, &qualified_name)
        .map_err(|error| invalid(format!("schema binding cannot be resolved: {error}")))
}

fn parse_qualified_schema(source: &str) -> Result<crate::hir::QualifiedName, SettlementProofError> {
    let parts = source.split("::").collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(invalid("qualified schema must contain a module and name"));
    }
    let name = crate::hir::Name::new(parts[parts.len() - 1].to_owned())
        .map_err(|_| invalid("qualified schema contains an invalid name"))?;
    let modules = parts[..parts.len() - 1]
        .iter()
        .map(|part| crate::hir::Name::new((*part).to_owned()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid("qualified schema contains an invalid module"))?;
    let module = crate::hir::ModulePath::new(modules)
        .map_err(|_| invalid("qualified schema must contain a module"))?;
    Ok(crate::hir::QualifiedName { module, name })
}

/// Decode a source form for checking without going through the production
/// settlement projection boundary.  The compiled capability already proves
/// the schema shape; these checks deliberately repeat the eight-field
/// contract at the proof authority boundary.
fn entry_from_elaborated_form(
    form: &elaboration::ElaboratedForm,
) -> Result<SettlementStateV1Entry, SettlementProofError> {
    let occurrence = form.occurrence().clone();
    let occurrence_text = occurrence.as_str();
    if !canonical_identifier(occurrence_text) {
        return Err(invalid("re-elaborated form occurrence is not canonical"));
    }

    let record = form.value().record();
    const FIELDS: [&str; 8] = [
        "settlement",
        "kind",
        "state",
        "at",
        "from",
        "to",
        "instrument",
        "amount",
    ];
    if record.rest.is_some()
        || record.fields.len() != FIELDS.len()
        || FIELDS.iter().any(|field| record.field(*field).is_none())
        || record
            .fields
            .keys()
            .any(|field| !FIELDS.contains(&field.as_str()))
    {
        return Err(invalid(format!(
            "capable form `{occurrence_text}` does not contain exactly eight scalar fields"
        )));
    }

    let settlement = form_text(record, occurrence_text, "settlement")?.to_owned();
    if !canonical_identifier(&settlement) {
        return Err(invalid(format!(
            "capable form `{occurrence_text}` has a non-canonical settlement identifier"
        )));
    }
    let rail = parse_proof_kind(form_text(record, occurrence_text, "kind")?, occurrence_text)?;
    let state = parse_proof_state(
        form_text(record, occurrence_text, "state")?,
        occurrence_text,
    )?;
    let date = parse_proof_date(form_text(record, occurrence_text, "at")?, occurrence_text)?;

    let from_text = form_text(record, occurrence_text, "from")?;
    let from = parse_proof_entity(from_text, occurrence_text, "from")?;
    let to_text = form_text(record, occurrence_text, "to")?;
    let to = parse_proof_entity(to_text, occurrence_text, "to")?;
    let instrument_text = form_text(record, occurrence_text, "instrument")?;
    let instrument = parse_proof_instrument(instrument_text, occurrence_text)?;

    let amount_term = record.field("amount").ok_or_else(|| {
        invalid(format!(
            "capable form `{occurrence_text}` is missing `amount`"
        ))
    })?;
    let decimal = match amount_term {
        Term::Decimal(value) => value,
        _ => {
            return Err(invalid(format!(
                "capable form `{occurrence_text}` amount is not an exact decimal"
            )));
        }
    };
    let exact = ExactNumber::rational(decimal.numer().clone(), decimal.denom().clone()).map_err(
        |error| {
            invalid(format!(
                "capable form `{occurrence_text}` amount is invalid: {error}"
            ))
        },
    )?;
    if exact.is_negative() || exact.is_zero() {
        return Err(invalid(format!(
            "capable form `{occurrence_text}` amount must be positive"
        )));
    }
    let amount = Quantity::with_unit(exact, instrument.as_str()).map_err(|error| {
        invalid(format!(
            "capable form `{occurrence_text}` amount unit is invalid: {error}"
        ))
    })?;

    let qualified_schema = form.schema().qualified_name().canonical();
    if !canonical_identifier(&qualified_schema)
        || form.package_root() == ContentHash::ZERO
        || form.schema().schema_id() == ContentHash::ZERO
        || form.value().content_hash() == ContentHash::ZERO
    {
        return Err(invalid(format!(
            "capable form `{occurrence_text}` has a non-canonical schema or value identity"
        )));
    }
    Ok(SettlementStateV1Entry {
        occurrence: occurrence.clone(),
        occurrence_hash: occurrence_hash(&occurrence),
        value_hash: form.value().content_hash(),
        package_root: form.package_root(),
        qualified_schema,
        schema_id: form.schema().schema_id(),
        settlement,
        rail,
        state,
        date,
        from,
        to,
        instrument,
        amount,
    })
}

fn form_text<'a>(
    record: &'a Record,
    occurrence: &str,
    field: &str,
) -> Result<&'a str, SettlementProofError> {
    match record.field(field) {
        Some(Term::Text(value)) => Ok(value),
        Some(_) => Err(invalid(format!(
            "capable form `{occurrence}` field `{field}` is not text"
        ))),
        None => Err(invalid(format!(
            "capable form `{occurrence}` is missing `{field}`"
        ))),
    }
}

fn parse_proof_kind(value: &str, occurrence: &str) -> Result<SettlementKind, SettlementProofError> {
    match value {
        "ach" => Ok(SettlementKind::Ach),
        "card" => Ok(SettlementKind::Card),
        "check" => Ok(SettlementKind::Check),
        _ => Err(invalid(format!(
            "capable form `{occurrence}` has an unknown settlement rail"
        ))),
    }
}

fn parse_proof_state(
    value: &str,
    occurrence: &str,
) -> Result<SettlementState, SettlementProofError> {
    let state = match value {
        "issued" => SettlementState::Issued,
        "authorized" => SettlementState::Authorized,
        "presented" => SettlementState::Presented,
        "pending" => SettlementState::Pending,
        "settled" => SettlementState::Settled,
        "returned" => SettlementState::Returned,
        "reversed" => SettlementState::Reversed,
        "rejected" => SettlementState::Rejected,
        "cancelled" => SettlementState::Cancelled,
        "refunded" => SettlementState::Refunded,
        "disputed" => SettlementState::Disputed,
        "charged-back" => SettlementState::ChargedBack,
        "represented" => SettlementState::Represented,
        "resolved" => SettlementState::Resolved,
        _ => {
            return Err(invalid(format!(
                "capable form `{occurrence}` has an unknown settlement state"
            )));
        }
    };
    Ok(state)
}

fn parse_proof_date(value: &str, occurrence: &str) -> Result<Date, SettlementProofError> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| (index != 4 && index != 7) && !byte.is_ascii_digit())
    {
        return Err(invalid(format!(
            "capable form `{occurrence}` date is not canonical YYYY-MM-DD"
        )));
    }
    let date = value.parse::<Date>().map_err(|error| {
        invalid(format!(
            "capable form `{occurrence}` date is invalid: {error}"
        ))
    })?;
    if date.to_string() != value {
        return Err(invalid(format!(
            "capable form `{occurrence}` date is not canonical YYYY-MM-DD"
        )));
    }
    Ok(date)
}

fn parse_proof_entity(
    value: &str,
    occurrence: &str,
    field: &str,
) -> Result<EntityId, SettlementProofError> {
    if !canonical_identifier(value) {
        return Err(invalid(format!(
            "capable form `{occurrence}` field `{field}` is not canonical"
        )));
    }
    EntityId::try_new(value).map_err(|error| {
        invalid(format!(
            "capable form `{occurrence}` field `{field}` is invalid: {error}"
        ))
    })
}

fn parse_proof_instrument(
    value: &str,
    occurrence: &str,
) -> Result<InstrumentId, SettlementProofError> {
    if !canonical_identifier(value) {
        return Err(invalid(format!(
            "capable form `{occurrence}` instrument is not canonical"
        )));
    }
    InstrumentId::try_new(value).map_err(|error| {
        invalid(format!(
            "capable form `{occurrence}` instrument is invalid: {error}"
        ))
    })
}

fn validate_independent_history(
    entries: &[SettlementStateV1Entry],
) -> Result<(), SettlementProofError> {
    let mut histories: BTreeMap<String, Vec<&SettlementStateV1Entry>> = BTreeMap::new();
    for entry in entries {
        histories
            .entry(entry.settlement.clone())
            .or_default()
            .push(entry);
    }
    for (settlement, history) in histories {
        let first = history[0];
        let mut previous_state = None;
        let mut previous_date = None;
        for entry in history {
            if entry.amount != first.amount
                || entry.instrument != first.instrument
                || entry.from != first.from
                || entry.to != first.to
                || entry.rail != first.rail
            {
                return Err(invalid(format!(
                    "settlement `{settlement}` changes static facts"
                )));
            }
            if previous_date.is_some_and(|previous| entry.date < previous) {
                return Err(invalid(format!(
                    "settlement `{settlement}` dates are not monotonic"
                )));
            }
            if !independent_transition_is_legal(entry.rail, previous_state.as_ref(), &entry.state) {
                return Err(invalid(format!(
                    "settlement `{settlement}` has an illegal {:?} transition",
                    entry.rail
                )));
            }
            previous_date = Some(entry.date);
            previous_state = Some(entry.state.clone());
        }
    }
    Ok(())
}

fn independent_transition_is_legal(
    rail: SettlementKind,
    previous: Option<&SettlementState>,
    next: &SettlementState,
) -> bool {
    use SettlementState::*;
    match rail {
        SettlementKind::Ach => matches!(
            (previous, next),
            (None, Issued)
                | (Some(Issued), Presented | Cancelled)
                | (
                    Some(Presented),
                    Pending | Settled | Returned | Rejected | Cancelled
                )
                | (Some(Pending), Settled | Returned | Rejected | Cancelled)
                | (Some(Settled), Returned | Reversed)
        ),
        SettlementKind::Card => matches!(
            (previous, next),
            (None, Issued)
                | (Some(Issued), Authorized | Presented | Rejected | Cancelled)
                | (
                    Some(Authorized),
                    Presented | Rejected | Cancelled | Reversed
                )
                | (Some(Presented), Pending | Settled | Rejected | Cancelled)
                | (Some(Pending), Settled | Rejected | Cancelled)
                | (Some(Settled), Reversed | Refunded | Disputed | ChargedBack)
                | (Some(Disputed), Resolved | ChargedBack)
                | (Some(ChargedBack), Represented)
                | (Some(Represented), Pending | Settled | Rejected)
        ),
        SettlementKind::Check => matches!(
            (previous, next),
            (None, Issued)
                | (Some(Issued), Presented | Cancelled)
                | (
                    Some(Presented),
                    Pending | Settled | Returned | Rejected | Cancelled
                )
                | (Some(Pending), Settled | Returned | Rejected | Cancelled)
                | (Some(Settled), Returned)
                | (Some(Returned), Presented | Cancelled)
        ),
    }
}

fn entry_from_form(form: &CapableSettlementForm) -> SettlementStateV1Entry {
    let record = form.record();
    SettlementStateV1Entry {
        occurrence: form.occurrence().clone(),
        occurrence_hash: occurrence_hash(form.occurrence()),
        value_hash: form.value_hash(),
        package_root: form.package_root(),
        qualified_schema: form.qualified_schema().to_owned(),
        schema_id: form.schema_id(),
        settlement: record.settlement.as_str().to_owned(),
        rail: record
            .kind()
            .expect("projection always carries a settlement rail"),
        state: record.state.clone(),
        date: record.at.expect("SettlementStateV1 requires a date"),
        from: record.from.entity.clone(),
        to: record.to.entity.clone(),
        instrument: record.instrument.clone(),
        amount: record.amount.clone(),
    }
}

fn occurrence_hash(occurrence: &OccurrenceId) -> ContentHash {
    ContentHash::domain_separated(OCCURRENCE_HASH_DOMAIN, occurrence.as_str().as_bytes())
}

fn invalid(reason: impl Into<String>) -> SettlementProofError {
    SettlementProofError::Invalid(reason.into())
}

fn canonical_identifier(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn put_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_string(out, value);
        }
        None => out.push(0),
    }
}

fn put_hash(out: &mut Vec<u8>, value: ContentHash) {
    out.extend_from_slice(value.as_bytes());
}

fn put_date(out: &mut Vec<u8>, value: Date) {
    out.extend_from_slice(&value.year.to_be_bytes());
    out.push(value.month);
    out.push(value.day);
}

fn kind_name(kind: SettlementKind) -> &'static str {
    match kind {
        SettlementKind::Ach => "ach",
        SettlementKind::Card => "card",
        SettlementKind::Check => "check",
    }
}

fn state_name(state: &SettlementState) -> &'static str {
    match state {
        SettlementState::Issued => "issued",
        SettlementState::Authorized => "authorized",
        SettlementState::Presented => "presented",
        SettlementState::Pending => "pending",
        SettlementState::Settled => "settled",
        SettlementState::Returned => "returned",
        SettlementState::Reversed => "reversed",
        SettlementState::Rejected => "rejected",
        SettlementState::Cancelled => "cancelled",
        SettlementState::Refunded => "refunded",
        SettlementState::Disputed => "disputed",
        SettlementState::ChargedBack => "charged-back",
        SettlementState::Represented => "represented",
        SettlementState::Resolved => "resolved",
    }
}

fn encode_entry(out: &mut Vec<u8>, entry: &SettlementStateV1Entry) {
    put_string(out, entry.occurrence.as_str());
    put_hash(out, entry.occurrence_hash);
    put_hash(out, entry.value_hash);
    put_hash(out, entry.package_root);
    put_string(out, &entry.qualified_schema);
    put_hash(out, entry.schema_id);
    put_string(out, &entry.settlement);
    put_string(out, kind_name(entry.rail));
    put_string(out, state_name(&entry.state));
    put_date(out, entry.date);
    put_string(out, entry.from.as_str());
    put_string(out, entry.to.as_str());
    put_string(out, entry.instrument.as_str());
    put_string(out, &entry.amount.number.canonical_string());
    put_optional_string(out, entry.amount.unit().map(ToString::to_string).as_deref());
}
