//! Projection of an explicitly-capable package form into settlement facts.
//!
//! This module is intentionally a small authority boundary.  A form is
//! eligible only when its pinned [`RecordSchema`] carries the
//! [`SchemaCapability::SettlementStateV1`] capability.  The capability is
//! supplied by the compiled package artifact; this module does not infer it
//! from a schema name, field shape, or value contents.
//!
//! The v1 capability is an event contract, not a journal or a settlement
//! aggregate.  Every accepted form becomes one [`SettlementStateRecord`],
//! retained in elaboration/source order.  Validation is performed against a
//! temporary graph before the result is returned, so a malformed form or
//! history cannot produce a partially projected value.

use std::fmt;

use crate::elaboration::ElaboratedForm;
use crate::exact::ExactNumber;
use crate::ir::{Record, Term, Var};
use crate::model::{
    ContentHash, Date, EntityId, InstrumentId, OccurrenceId, Quantity, SettlementKind,
};
use crate::ontology::{
    Endpoint, EventGraph, OntologyError, SettlementState, SettlementStateRecord,
    validate_settlement_state_records,
};
use crate::package_compiler::SchemaCapability;
use crate::store::{CommitId, CompiledArtifactId};
use crate::workspace::BoundPackageForms;

/// The atomic result of projecting one bound package-form snapshot.
#[derive(Clone, Debug)]
pub struct SettlementProjection {
    source_commit: CommitId,
    compiled_artifact: CompiledArtifactId,
    artifact_hash: ContentHash,
    capable_forms: Vec<CapableSettlementForm>,
    graph: EventGraph,
}

/// The checked identity and typed payload of one explicitly capable form.
/// This is retained separately from the ontology record so proof producers
/// can bind the source value hash and package schema without re-decoding an
/// untrusted record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapableSettlementForm {
    occurrence: OccurrenceId,
    value_hash: ContentHash,
    package_root: ContentHash,
    qualified_schema: String,
    schema_id: ContentHash,
    record: SettlementStateRecord,
}

impl CapableSettlementForm {
    pub(crate) fn occurrence(&self) -> &OccurrenceId {
        &self.occurrence
    }

    pub(crate) fn value_hash(&self) -> ContentHash {
        self.value_hash
    }

    pub(crate) fn package_root(&self) -> ContentHash {
        self.package_root
    }

    pub(crate) fn qualified_schema(&self) -> &str {
        &self.qualified_schema
    }

    pub(crate) fn schema_id(&self) -> ContentHash {
        self.schema_id
    }

    pub(crate) fn record(&self) -> &SettlementStateRecord {
        &self.record
    }
}

impl SettlementProjection {
    pub fn source_commit(&self) -> CommitId {
        self.source_commit
    }

    pub fn compiled_artifact(&self) -> CompiledArtifactId {
        self.compiled_artifact
    }

    pub fn artifact_hash(&self) -> ContentHash {
        self.artifact_hash
    }

    /// Return settlement-state records in exact elaboration/source order.
    pub fn records(&self) -> impl ExactSizeIterator<Item = &SettlementStateRecord> {
        self.capable_forms.iter().map(CapableSettlementForm::record)
    }

    /// Return capable forms in exact elaboration/source order, including the
    /// schema and checked value identity needed by a settlement proof.
    pub(crate) fn capable_forms(&self) -> &[CapableSettlementForm] {
        &self.capable_forms
    }

    /// Return the accepted typed event graph.  The graph preserves insertion
    /// order through `records_of::<SettlementStateRecord>()` and has already
    /// passed its full validation boundary.
    pub fn graph(&self) -> &EventGraph {
        &self.graph
    }
}

/// Why one source form could not cross the settlement projection boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementProjectionError {
    /// No source form explicitly carried the v1 settlement capability.
    /// Projection never treats an ordinary record as a settlement by shape.
    NoCapableForms,
    /// A value term did not have the primitive type attested by the
    /// capability contract.  In particular, variables are never treated as
    /// defaults or holes.
    WrongType {
        occurrence: String,
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    MissingField {
        occurrence: String,
        field: &'static str,
    },
    InvalidField {
        occurrence: String,
        field: &'static str,
        value: String,
        reason: String,
    },
    /// The ontology rejected a decoded record or its ordered history.
    Ontology(OntologyError),
}

impl fmt::Display for SettlementProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCapableForms => formatter.write_str(
                "source snapshot contains no form carrying SettlementStateV1 capability",
            ),
            Self::WrongType {
                occurrence,
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "settlement form `{occurrence}` field `{field}` has {actual}, expected {expected}"
            ),
            Self::MissingField { occurrence, field } => {
                write!(
                    formatter,
                    "settlement form `{occurrence}` is missing `{field}`"
                )
            }
            Self::InvalidField {
                occurrence,
                field,
                value,
                reason,
            } => write!(
                formatter,
                "settlement form `{occurrence}` field `{field}` value `{value}` is invalid: {reason}"
            ),
            Self::Ontology(error) => write!(formatter, "invalid settlement history: {error}"),
        }
    }
}

impl std::error::Error for SettlementProjectionError {}

impl From<OntologyError> for SettlementProjectionError {
    fn from(error: OntologyError) -> Self {
        Self::Ontology(error)
    }
}

/// Project all explicitly-capable settlement forms in one bound snapshot.
///
/// Ordinary package forms are ignored.  Once a form advertises
/// `SettlementStateV1`, however, every field is decoded and validated; no
/// invalid capable form is silently skipped.  The graph and records are
/// built locally and returned only after the entire ordered history set has
/// passed validation.
pub(crate) fn project_settlement_states(
    forms: &BoundPackageForms,
) -> Result<SettlementProjection, SettlementProjectionError> {
    let mut records = Vec::new();
    let mut capable_forms = Vec::new();

    for form in forms.forms() {
        if form.schema().capability() != Some(SchemaCapability::SettlementStateV1) {
            continue;
        }
        let record = decode_settlement_state(form)?;
        records.push(record.clone());
        capable_forms.push(CapableSettlementForm {
            occurrence: form.occurrence().clone(),
            value_hash: form.value().content_hash(),
            package_root: form.package_root(),
            qualified_schema: form.schema().qualified_name().canonical(),
            schema_id: form.schema().schema_id(),
            record,
        });
    }

    if records.is_empty() {
        return Err(SettlementProjectionError::NoCapableForms);
    }

    // Validate the grouped histories before inserting anything into the
    // graph.  The caller's vector order is authoritative; the validator only
    // groups by settlement to check each history and does not reorder it.
    validate_settlement_state_records(&records)?;

    let mut graph = EventGraph::new();
    for record in records.iter().cloned() {
        graph.insert(record)?;
    }
    graph.validate()?;

    Ok(SettlementProjection {
        source_commit: forms.source_commit(),
        compiled_artifact: forms.compiled_artifact(),
        artifact_hash: forms.artifact_hash(),
        capable_forms,
        graph,
    })
}

fn decode_settlement_state(
    form: &ElaboratedForm,
) -> Result<SettlementStateRecord, SettlementProjectionError> {
    let occurrence = form.occurrence().as_str().to_owned();
    let record = form.value().record();
    let settlement = text(record, &occurrence, "settlement")?;
    if !canonical_identifier(settlement) {
        return Err(invalid(
            &occurrence,
            "settlement",
            settlement,
            "settlement identifier must be nonempty canonical text",
        ));
    }
    let kind = parse_kind(&occurrence, text(record, &occurrence, "kind")?)?;
    let state = parse_state(&occurrence, text(record, &occurrence, "state")?)?;
    let at = parse_date(&occurrence, text(record, &occurrence, "at")?)?;
    let from_text = text(record, &occurrence, "from")?;
    if !canonical_identifier(from_text) {
        return Err(invalid(
            &occurrence,
            "from",
            from_text,
            "endpoint identifier must be nonempty canonical text",
        ));
    }
    let from = EntityId::try_new(from_text).map_err(|error| {
        invalid(
            &occurrence,
            "from",
            record_text(record, "from"),
            error.to_string(),
        )
    })?;
    let to_text = text(record, &occurrence, "to")?;
    if !canonical_identifier(to_text) {
        return Err(invalid(
            &occurrence,
            "to",
            to_text,
            "endpoint identifier must be nonempty canonical text",
        ));
    }
    let to = EntityId::try_new(to_text).map_err(|error| {
        invalid(
            &occurrence,
            "to",
            record_text(record, "to"),
            error.to_string(),
        )
    })?;
    let instrument_text = text(record, &occurrence, "instrument")?;
    if !canonical_identifier(instrument_text) {
        return Err(invalid(
            &occurrence,
            "instrument",
            instrument_text,
            "instrument identifier must be nonempty canonical text",
        ));
    }
    let instrument = InstrumentId::try_new(instrument_text).map_err(|error| {
        invalid(
            &occurrence,
            "instrument",
            record_text(record, "instrument"),
            error.to_string(),
        )
    })?;
    let amount = decimal(record, &occurrence, "amount")?;
    // SettlementStateV1 defines the Decimal amount as denominated by its
    // explicit instrument field.  This is contract decoding, not a unit
    // guess from a name or from an unrelated record.
    let amount = Quantity::with_unit(amount, instrument.as_str()).map_err(|error| {
        invalid(
            &occurrence,
            "amount",
            record_text(record, "amount"),
            error.to_string(),
        )
    })?;

    let mut state_record = SettlementStateRecord::new(
        form.occurrence().clone(),
        settlement,
        state,
        amount,
        instrument,
        Endpoint::entity(from),
        Endpoint::entity(to),
    )
    .with_kind(kind);
    state_record.at = Some(at);
    // `EventGraph::insert` repeats this check, but checking at the decode
    // boundary makes the atomic contract explicit and keeps this function
    // safe if the graph's accepted-record policy evolves independently.
    state_record.validate()?;
    Ok(state_record)
}

fn text<'a>(
    record: &'a Record,
    occurrence: &str,
    field: &'static str,
) -> Result<&'a str, SettlementProjectionError> {
    match record.field(field) {
        Some(Term::Text(value)) => Ok(value),
        Some(Term::Var(_)) => Err(SettlementProjectionError::WrongType {
            occurrence: occurrence.to_owned(),
            field,
            expected: "text",
            actual: "hole",
        }),
        Some(value) => Err(SettlementProjectionError::WrongType {
            occurrence: occurrence.to_owned(),
            field,
            expected: "text",
            actual: term_kind(value),
        }),
        None => Err(SettlementProjectionError::MissingField {
            occurrence: occurrence.to_owned(),
            field,
        }),
    }
}

fn decimal(
    record: &Record,
    occurrence: &str,
    field: &'static str,
) -> Result<ExactNumber, SettlementProjectionError> {
    match record.field(field) {
        Some(Term::Decimal(value)) => {
            ExactNumber::rational(value.numer().clone(), value.denom().clone())
                .map_err(|error| invalid(occurrence, field, value.to_string(), error.to_string()))
        }
        Some(Term::Var(Var { .. })) => Err(SettlementProjectionError::WrongType {
            occurrence: occurrence.to_owned(),
            field,
            expected: "decimal",
            actual: "hole",
        }),
        Some(value) => Err(SettlementProjectionError::WrongType {
            occurrence: occurrence.to_owned(),
            field,
            expected: "decimal",
            actual: term_kind(value),
        }),
        None => Err(SettlementProjectionError::MissingField {
            occurrence: occurrence.to_owned(),
            field,
        }),
    }
}

fn parse_kind(occurrence: &str, value: &str) -> Result<SettlementKind, SettlementProjectionError> {
    match value {
        "ach" => Ok(SettlementKind::Ach),
        "card" => Ok(SettlementKind::Card),
        "check" => Ok(SettlementKind::Check),
        _ => Err(invalid(
            occurrence,
            "kind",
            value,
            "expected one of `ach`, `card`, or `check`",
        )),
    }
}

fn parse_state(
    occurrence: &str,
    value: &str,
) -> Result<SettlementState, SettlementProjectionError> {
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
            return Err(invalid(
                occurrence,
                "state",
                value,
                "unknown SettlementStateV1 state",
            ));
        }
    };
    Ok(state)
}

fn parse_date(occurrence: &str, value: &str) -> Result<Date, SettlementProjectionError> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| (index != 4 && index != 7) && !byte.is_ascii_digit())
    {
        return Err(invalid(
            occurrence,
            "at",
            value,
            "date must use exactly 10-byte canonical YYYY-MM-DD spelling",
        ));
    }
    let date = value.parse::<Date>().map_err(|error| {
        invalid(
            occurrence,
            "at",
            value,
            format!("expected an ISO civil date: {error}"),
        )
    })?;
    if date.to_string() != value {
        return Err(invalid(
            occurrence,
            "at",
            value,
            "date must use canonical YYYY-MM-DD spelling",
        ));
    }
    Ok(date)
}

fn canonical_identifier(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn invalid(
    occurrence: &str,
    field: &'static str,
    value: impl Into<String>,
    reason: impl Into<String>,
) -> SettlementProjectionError {
    SettlementProjectionError::InvalidField {
        occurrence: occurrence.to_owned(),
        field,
        value: value.into(),
        reason: reason.into(),
    }
}

fn record_text(record: &Record, field: &'static str) -> String {
    match record.field(field) {
        Some(Term::Text(value)) => value.clone(),
        Some(value) => term_kind(value).to_owned(),
        None => "<missing>".to_owned(),
    }
}

fn term_kind(term: &Term) -> &'static str {
    match term {
        Term::Var(_) => "variable",
        Term::Nominal(_) => "nominal",
        Term::Unit(_) => "unit",
        Term::Quantity(_) => "quantity",
        Term::Integer(_) => "integer",
        Term::Decimal(_) => "decimal",
        Term::Record(_) => "record",
        Term::Tuple(_) => "tuple",
        Term::App { .. } => "application",
        Term::Bool(_) => "boolean",
        Term::Text(_) => "text",
    }
}
