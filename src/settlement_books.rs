//! Dedicated typed settlement acceptance and recognition.
//!
//! `SettlementStateV1Proof` is the only authority in this module.  The
//! accepted world retains that proof and a checked, typed grouping of its
//! rows; recognition, journals, and closes are immutable projections over
//! that value.  This module intentionally has no generic lifecycle or
//! accrual policy.

use std::collections::BTreeMap;
use std::fmt;

use crate::evidence::RawEvidence;
use crate::model::{
    AccountId, ContentHash, Date, EntityId, InstrumentId, OccurrenceId, Quantity, SettlementKind,
};
use crate::ontology::SettlementState;
use crate::settlement_projection::SettlementProjection;
use crate::settlement_proof::{
    SettlementProofError, SettlementStateV1Entry, SettlementStateV1Proof,
};
use crate::store::{CommitId, EvidenceId, ObjectStore};

const AUTHORITY_DOMAIN: &str = "axiom/settlement-state-v1-authority";
const FACT_DOMAIN: &str = "axiom/settlement-recognized-fact/v1";
const RECOGNIZED_DOMAIN: &str = "axiom/settlement-recognized/v1";
const JOURNAL_DOMAIN: &str = "axiom/settlement-journal/v1";
const CLOSE_DOMAIN: &str = "axiom/settlement-close/v1";

/// A state transition retained in source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementTransitionView {
    pub state: SettlementState,
    pub at: Date,
    pub occurrence: OccurrenceId,
}

/// One grouped settlement history.  Ineffective histories are retained as
/// observations; `effective` is true only when the final state is `Settled`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementHistory {
    settlement: String,
    kind: SettlementKind,
    from: EntityId,
    to: EntityId,
    instrument: InstrumentId,
    amount: Quantity,
    transitions: Vec<SettlementTransitionView>,
    effective: bool,
    current_state: SettlementState,
    occurrence_date: Date,
    settlement_date: Option<Date>,
    package_root: ContentHash,
    qualified_schema: String,
    schema_id: ContentHash,
}

impl SettlementHistory {
    pub fn settlement(&self) -> &str {
        &self.settlement
    }

    pub fn kind(&self) -> SettlementKind {
        self.kind
    }

    pub fn from(&self) -> &EntityId {
        &self.from
    }

    pub fn to(&self) -> &EntityId {
        &self.to
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn amount(&self) -> &Quantity {
        &self.amount
    }

    pub fn transitions(&self) -> &[SettlementTransitionView] {
        &self.transitions
    }

    pub fn effective(&self) -> bool {
        self.effective
    }

    pub fn current_state(&self) -> &SettlementState {
        &self.current_state
    }

    pub fn occurrence_date(&self) -> Date {
        self.occurrence_date
    }

    pub fn settlement_date(&self) -> Option<Date> {
        self.settlement_date
    }

    pub fn package_root(&self) -> ContentHash {
        self.package_root
    }

    pub fn qualified_schema(&self) -> &str {
        &self.qualified_schema
    }

    pub fn schema_id(&self) -> ContentHash {
        self.schema_id
    }
}

/// The accepted source world.  Its `SettlementStateV1Proof` is the only
/// authority; histories are checked projections, not a second authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementWorld {
    settlement_proof: SettlementStateV1Proof,
    histories: Vec<SettlementHistory>,
    authority_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementBookError {
    SettlementProof(SettlementProofError),
    Invalid(String),
    ConflictingSettlementIdentity {
        settlement: String,
        first: String,
        conflicting: String,
    },
    MissingAccount {
        settlement: String,
        endpoint: EntityId,
    },
    SameAccount {
        settlement: String,
        account: AccountId,
    },
    OutOfPeriod {
        settlement: String,
        date: Date,
    },
}

impl fmt::Display for SettlementBookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SettlementProof(error) => write!(formatter, "settlement proof error: {error}"),
            Self::Invalid(reason) => write!(formatter, "invalid settlement book: {reason}"),
            Self::ConflictingSettlementIdentity {
                settlement,
                first,
                conflicting,
            } => write!(
                formatter,
                "settlement `{settlement}` spans package/schema identities `{first}` and `{conflicting}`"
            ),
            Self::MissingAccount {
                settlement,
                endpoint,
            } => write!(
                formatter,
                "cash settlement `{settlement}` has no account mapping for endpoint `{endpoint}`"
            ),
            Self::SameAccount {
                settlement,
                account,
            } => write!(
                formatter,
                "cash settlement `{settlement}` maps both endpoints to `{account}`"
            ),
            Self::OutOfPeriod { settlement, date } => write!(
                formatter,
                "settlement `{settlement}` date {date} is outside the reporting period"
            ),
        }
    }
}

impl std::error::Error for SettlementBookError {}

impl From<SettlementProofError> for SettlementBookError {
    fn from(error: SettlementProofError) -> Self {
        Self::SettlementProof(error)
    }
}

impl SettlementWorld {
    pub(crate) fn build(
        projection: &SettlementProjection,
        source: &RawEvidence,
        evidence: EvidenceId,
        store: &ObjectStore,
    ) -> Result<Self, SettlementBookError> {
        let proof = SettlementStateV1Proof::from_projection(projection, source, evidence)?;
        proof.check(store)?;
        let histories = derive_histories(&proof)?;
        let authority_hash = authority_hash(&proof);
        Ok(Self {
            settlement_proof: proof,
            histories,
            authority_hash,
        })
    }

    pub fn settlement_proof(&self) -> &SettlementStateV1Proof {
        &self.settlement_proof
    }

    pub fn source_commit(&self) -> CommitId {
        self.settlement_proof.source_commit
    }

    pub fn authority_hash(&self) -> ContentHash {
        self.authority_hash
    }

    pub fn histories(&self) -> &[SettlementHistory] {
        &self.histories
    }

    pub fn history(&self, settlement: &str) -> Option<&SettlementHistory> {
        self.histories
            .binary_search_by(|history| history.settlement.as_str().cmp(settlement))
            .ok()
            .map(|index| &self.histories[index])
    }

    /// Recheck the exact source/artifact/schema/value coverage and rebuild
    /// all grouped histories.  No cached projection is trusted.
    pub fn check(&self, store: &ObjectStore) -> Result<(), SettlementBookError> {
        self.settlement_proof.check(store)?;
        self.check_without_store()
    }

    fn check_without_store(&self) -> Result<(), SettlementBookError> {
        if self.authority_hash != authority_hash(&self.settlement_proof) {
            return Err(invalid("cached settlement authority does not match proof"));
        }
        let expected = derive_histories(&self.settlement_proof)?;
        if self.histories != expected {
            return Err(invalid(
                "grouped settlement histories do not match proof coverage",
            ));
        }
        Ok(())
    }

    pub fn recognize(
        &self,
        policy: SettlementRecognitionPolicy,
    ) -> Result<SettlementRecognition, SettlementBookError> {
        self.recognize_internal(policy, None)
    }

    fn recognize_internal(
        &self,
        policy: SettlementRecognitionPolicy,
        period: Option<SettlementReportingPeriod>,
    ) -> Result<SettlementRecognition, SettlementBookError> {
        let authority = self.authority_hash;
        let facts = self
            .histories
            .iter()
            .filter(|history| match &policy {
                SettlementRecognitionPolicy::Observation => true,
                SettlementRecognitionPolicy::Cash { .. } => history.effective,
            })
            .filter(|history| {
                period.is_none_or(|period| {
                    let date = if matches!(&policy, SettlementRecognitionPolicy::Cash { .. }) {
                        history
                            .settlement_date
                            .expect("cash recognition only contains effective histories")
                    } else {
                        history.occurrence_date
                    };
                    period.contains(date)
                })
            })
            .map(|history| {
                let cash = matches!(&policy, SettlementRecognitionPolicy::Cash { .. });
                RecognizedSettlement::from_history(authority, history, cash)
            })
            .collect::<Vec<_>>();

        match policy {
            SettlementRecognitionPolicy::Observation => Ok(SettlementRecognition::Observation(
                ObservationRecognition::new(authority, facts),
            )),
            SettlementRecognitionPolicy::Cash { accounts } => {
                let journal = SettlementJournal::from_cash(authority, &facts, &accounts)?;
                Ok(SettlementRecognition::Cash(CashRecognition::new(
                    authority, facts, journal, accounts,
                )))
            }
        }
    }

    pub fn close(
        &self,
        policy: SettlementRecognitionPolicy,
        period: SettlementReportingPeriod,
    ) -> Result<SettlementClose, SettlementBookError> {
        if period.start > period.end {
            return Err(invalid("reporting period start is after its end"));
        }
        let recognition = self.recognize_internal(policy.clone(), Some(period))?;
        let journal_root = recognition.journal().map(SettlementJournal::root);
        let recognized_root = recognition.root();
        let close_root = close_root(
            self.authority_hash,
            &policy,
            period,
            recognized_root,
            journal_root,
        );
        let close = SettlementClose {
            world: self.clone(),
            policy,
            period,
            recognition,
            recognized_root,
            journal_root,
            close_root,
        };
        close.check_without_store()?;
        Ok(close)
    }
}

/// The only two settlement recognition interpretations.  There is
/// intentionally no accrual variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementRecognitionPolicy {
    Observation,
    Cash {
        accounts: BTreeMap<EntityId, AccountId>,
    },
}

impl SettlementRecognitionPolicy {
    pub const fn observation() -> Self {
        Self::Observation
    }

    pub fn cash<I>(accounts: I) -> Result<Self, SettlementBookError>
    where
        I: IntoIterator<Item = (EntityId, AccountId)>,
    {
        let mut result = BTreeMap::new();
        for (endpoint, account) in accounts {
            if result.insert(endpoint.clone(), account).is_some() {
                return Err(invalid(format!("duplicate endpoint mapping `{endpoint}`")));
            }
        }
        Ok(Self::Cash { accounts: result })
    }

    pub fn content_hash(&self) -> ContentHash {
        let mut bytes = Vec::new();
        match self {
            Self::Observation => bytes.push(0),
            Self::Cash { accounts } => {
                bytes.push(1);
                put_u64(&mut bytes, accounts.len() as u64);
                for (endpoint, account) in accounts {
                    put_string(&mut bytes, endpoint.as_str());
                    put_string(&mut bytes, account.as_str());
                }
            }
        }
        ContentHash::domain_separated("axiom/settlement-policy/v1", &bytes)
    }
}

/// One recognized settlement, carrying a typed reference to the exact proof
/// coverage that established it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizedSettlement {
    history: SettlementHistory,
    date: Date,
    proof: SettlementFactProof,
}

impl RecognizedSettlement {
    fn from_history(authority: ContentHash, history: &SettlementHistory, cash: bool) -> Self {
        let date = if cash {
            history
                .settlement_date
                .expect("cash recognition only contains effective histories")
        } else {
            history.occurrence_date
        };
        Self {
            history: history.clone(),
            date,
            proof: SettlementFactProof {
                authority,
                settlement: history.settlement.clone(),
                source_occurrences: history
                    .transitions
                    .iter()
                    .map(|transition| transition.occurrence.clone())
                    .collect(),
                effective: history.effective,
            },
        }
    }

    pub fn settlement(&self) -> &str {
        self.history.settlement()
    }

    pub fn history(&self) -> &SettlementHistory {
        &self.history
    }

    pub fn date(&self) -> Date {
        self.date
    }

    pub fn settlement_date(&self) -> Option<Date> {
        self.history.settlement_date()
    }

    pub fn proof(&self) -> &SettlementFactProof {
        &self.proof
    }
}

/// Typed proof reference for one grouped settlement fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementFactProof {
    authority: ContentHash,
    settlement: String,
    source_occurrences: Vec<OccurrenceId>,
    effective: bool,
}

impl SettlementFactProof {
    pub fn authority(&self) -> ContentHash {
        self.authority
    }

    pub fn settlement(&self) -> &str {
        &self.settlement
    }

    pub fn source_occurrences(&self) -> &[OccurrenceId] {
        &self.source_occurrences
    }

    pub fn effective(&self) -> bool {
        self.effective
    }

    fn check(&self, authority: ContentHash, history: &SettlementHistory) -> bool {
        self.authority == authority
            && self.settlement == history.settlement
            && self.source_occurrences
                == history
                    .transitions
                    .iter()
                    .map(|transition| transition.occurrence.clone())
                    .collect::<Vec<_>>()
            && self.effective == history.effective
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationRecognition {
    policy: SettlementRecognitionPolicy,
    facts: Vec<RecognizedSettlement>,
    root: ContentHash,
}

impl ObservationRecognition {
    fn new(authority: ContentHash, facts: Vec<RecognizedSettlement>) -> Self {
        let root = recognized_root(authority, &SettlementRecognitionPolicy::Observation, &facts);
        Self {
            policy: SettlementRecognitionPolicy::Observation,
            facts,
            root,
        }
    }

    pub fn policy(&self) -> &SettlementRecognitionPolicy {
        &self.policy
    }

    pub fn facts(&self) -> &[RecognizedSettlement] {
        &self.facts
    }

    pub fn root(&self) -> ContentHash {
        self.root
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CashRecognition {
    policy: SettlementRecognitionPolicy,
    facts: Vec<RecognizedSettlement>,
    journal: SettlementJournal,
    root: ContentHash,
}

impl CashRecognition {
    fn new(
        authority: ContentHash,
        facts: Vec<RecognizedSettlement>,
        journal: SettlementJournal,
        accounts: BTreeMap<EntityId, AccountId>,
    ) -> Self {
        let policy = SettlementRecognitionPolicy::Cash { accounts };
        let root = recognized_root(authority, &policy, &facts);
        Self {
            policy,
            facts,
            journal,
            root,
        }
    }

    pub fn policy(&self) -> &SettlementRecognitionPolicy {
        &self.policy
    }

    pub fn facts(&self) -> &[RecognizedSettlement] {
        &self.facts
    }

    pub fn journal(&self) -> &SettlementJournal {
        &self.journal
    }

    pub fn root(&self) -> ContentHash {
        self.root
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementRecognition {
    Observation(ObservationRecognition),
    Cash(CashRecognition),
}

impl SettlementRecognition {
    pub fn policy(&self) -> &SettlementRecognitionPolicy {
        match self {
            Self::Observation(value) => value.policy(),
            Self::Cash(value) => value.policy(),
        }
    }

    pub fn facts(&self) -> &[RecognizedSettlement] {
        match self {
            Self::Observation(value) => value.facts(),
            Self::Cash(value) => value.facts(),
        }
    }

    pub fn root(&self) -> ContentHash {
        match self {
            Self::Observation(value) => value.root(),
            Self::Cash(value) => value.root(),
        }
    }

    pub fn journal(&self) -> Option<&SettlementJournal> {
        match self {
            Self::Observation(_) => None,
            Self::Cash(value) => Some(value.journal()),
        }
    }

    pub fn check(&self, world: &SettlementWorld) -> Result<(), SettlementBookError> {
        self.check_for_period(world, None)
    }

    fn check_for_period(
        &self,
        world: &SettlementWorld,
        period: Option<SettlementReportingPeriod>,
    ) -> Result<(), SettlementBookError> {
        world.check_without_store()?;
        let expected = world.recognize_internal(self.policy().clone(), period)?;
        if self != &expected {
            return Err(invalid("settlement recognition was tampered"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementJournalEntry {
    settlement: String,
    debit: AccountId,
    credit: AccountId,
    amount: Quantity,
    proof: SettlementFactProof,
}

impl SettlementJournalEntry {
    pub fn settlement(&self) -> &str {
        &self.settlement
    }

    pub fn debit(&self) -> &AccountId {
        &self.debit
    }

    pub fn credit(&self) -> &AccountId {
        &self.credit
    }

    pub fn amount(&self) -> &Quantity {
        &self.amount
    }

    pub fn proof(&self) -> &SettlementFactProof {
        &self.proof
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementJournal {
    entries: Vec<SettlementJournalEntry>,
    root: ContentHash,
}

impl SettlementJournal {
    fn from_cash(
        authority: ContentHash,
        facts: &[RecognizedSettlement],
        accounts: &BTreeMap<EntityId, AccountId>,
    ) -> Result<Self, SettlementBookError> {
        let mut entries = Vec::with_capacity(facts.len());
        for fact in facts {
            let history = fact.history();
            let debit =
                accounts
                    .get(history.to())
                    .ok_or_else(|| SettlementBookError::MissingAccount {
                        settlement: history.settlement.clone(),
                        endpoint: history.to.clone(),
                    })?;
            let credit = accounts.get(history.from()).ok_or_else(|| {
                SettlementBookError::MissingAccount {
                    settlement: history.settlement.clone(),
                    endpoint: history.from.clone(),
                }
            })?;
            if debit == credit {
                return Err(SettlementBookError::SameAccount {
                    settlement: history.settlement.clone(),
                    account: debit.clone(),
                });
            }
            if !fact.proof.check(authority, history) {
                return Err(invalid(
                    "cash fact proof is not bound to settlement authority",
                ));
            }
            entries.push(SettlementJournalEntry {
                settlement: history.settlement.clone(),
                debit: debit.clone(),
                credit: credit.clone(),
                amount: history.amount.clone(),
                proof: fact.proof.clone(),
            });
        }
        let root = journal_root(authority, &entries);
        Ok(Self { entries, root })
    }

    pub fn entries(&self) -> &[SettlementJournalEntry] {
        &self.entries
    }

    pub fn root(&self) -> ContentHash {
        self.root
    }

    pub fn is_structurally_balanced(&self) -> bool {
        self.entries.iter().all(|entry| {
            entry.debit != entry.credit
                && !entry.amount.number.is_negative()
                && !entry.amount.number.is_zero()
                && entry.amount.unit().is_some()
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SettlementReportingPeriod {
    pub start: Date,
    pub end: Date,
}

impl SettlementReportingPeriod {
    pub const fn new(start: Date, end: Date) -> Self {
        Self { start, end }
    }

    pub fn contains(&self, date: Date) -> bool {
        date >= self.start && date <= self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementClose {
    world: SettlementWorld,
    policy: SettlementRecognitionPolicy,
    period: SettlementReportingPeriod,
    recognition: SettlementRecognition,
    recognized_root: ContentHash,
    journal_root: Option<ContentHash>,
    close_root: ContentHash,
}

impl SettlementClose {
    pub fn source_commit(&self) -> CommitId {
        self.world.source_commit()
    }

    pub fn settlement_proof(&self) -> &SettlementStateV1Proof {
        self.world.settlement_proof()
    }

    pub fn policy(&self) -> &SettlementRecognitionPolicy {
        &self.policy
    }

    pub fn period(&self) -> SettlementReportingPeriod {
        self.period
    }

    pub fn recognition(&self) -> &SettlementRecognition {
        &self.recognition
    }

    pub fn recognized_root(&self) -> ContentHash {
        self.recognized_root
    }

    pub fn journal_root(&self) -> Option<ContentHash> {
        self.journal_root
    }

    pub fn root(&self) -> ContentHash {
        self.close_root
    }

    pub fn check(&self, store: &ObjectStore) -> Result<(), SettlementBookError> {
        self.world.check(store)?;
        self.check_without_store()
    }

    fn check_without_store(&self) -> Result<(), SettlementBookError> {
        if self.period.start > self.period.end {
            return Err(invalid("reporting period start is after its end"));
        }
        for fact in self.recognition.facts() {
            if !self.period.contains(fact.date()) {
                return Err(SettlementBookError::OutOfPeriod {
                    settlement: fact.settlement().to_owned(),
                    date: fact.date(),
                });
            }
        }
        self.recognition
            .check_for_period(&self.world, Some(self.period))?;
        if self.policy != *self.recognition.policy()
            || self.recognized_root != self.recognition.root()
            || self.journal_root != self.recognition.journal().map(SettlementJournal::root)
            || self.close_root
                != close_root(
                    self.world.authority_hash,
                    &self.policy,
                    self.period,
                    self.recognized_root,
                    self.journal_root,
                )
        {
            return Err(invalid(
                "settlement close roots or policy do not match recognition",
            ));
        }
        if let Some(journal) = self.recognition.journal()
            && !journal.is_structurally_balanced()
        {
            return Err(invalid("settlement journal is not balanced"));
        }
        Ok(())
    }
}

fn derive_histories(
    proof: &SettlementStateV1Proof,
) -> Result<Vec<SettlementHistory>, SettlementBookError> {
    let mut grouped: BTreeMap<String, Vec<&SettlementStateV1Entry>> = BTreeMap::new();
    for entry in proof.coverage() {
        grouped
            .entry(entry.settlement.clone())
            .or_default()
            .push(entry);
    }
    let mut histories = Vec::with_capacity(grouped.len());
    for (settlement, entries) in grouped {
        let first = entries
            .first()
            .ok_or_else(|| invalid("settlement history is empty"))?;
        if let Some(conflicting) = entries.iter().find(|entry| !same_identity(first, entry)) {
            return Err(SettlementBookError::ConflictingSettlementIdentity {
                first: identity_description(first),
                conflicting: identity_description(conflicting),
                settlement,
            });
        }
        let last = entries.last().expect("nonempty grouped history");
        let effective = matches!(last.state, SettlementState::Settled);
        histories.push(SettlementHistory {
            settlement: first.settlement.clone(),
            kind: first.rail,
            from: first.from.clone(),
            to: first.to.clone(),
            instrument: first.instrument.clone(),
            amount: first.amount.clone(),
            transitions: entries
                .iter()
                .map(|entry| SettlementTransitionView {
                    state: entry.state.clone(),
                    at: entry.date,
                    occurrence: entry.occurrence.clone(),
                })
                .collect(),
            effective,
            current_state: last.state.clone(),
            occurrence_date: first.date,
            settlement_date: effective.then_some(last.date),
            package_root: first.package_root,
            qualified_schema: first.qualified_schema.clone(),
            schema_id: first.schema_id,
        });
    }
    Ok(histories)
}

fn same_identity(left: &SettlementStateV1Entry, right: &SettlementStateV1Entry) -> bool {
    left.package_root == right.package_root
        && left.qualified_schema == right.qualified_schema
        && left.schema_id == right.schema_id
}

fn identity_description(entry: &SettlementStateV1Entry) -> String {
    format!(
        "package={}, schema={}, schema-id={}",
        entry.package_root, entry.qualified_schema, entry.schema_id
    )
}

fn authority_hash(proof: &SettlementStateV1Proof) -> ContentHash {
    ContentHash::domain_separated(AUTHORITY_DOMAIN, &proof.canonical_bytes())
}

fn recognized_root(
    authority: ContentHash,
    policy: &SettlementRecognitionPolicy,
    facts: &[RecognizedSettlement],
) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, authority);
    put_hash(&mut bytes, policy.content_hash());
    put_u64(&mut bytes, facts.len() as u64);
    for fact in facts {
        put_string(&mut bytes, fact.settlement());
        put_date(&mut bytes, fact.date());
        put_optional_date(&mut bytes, fact.settlement_date());
        put_hash(&mut bytes, fact_hash(fact));
    }
    ContentHash::domain_separated(RECOGNIZED_DOMAIN, &bytes)
}

fn fact_hash(fact: &RecognizedSettlement) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, fact.proof.authority);
    put_string(&mut bytes, &fact.proof.settlement);
    put_u64(&mut bytes, fact.proof.source_occurrences.len() as u64);
    for occurrence in &fact.proof.source_occurrences {
        put_string(&mut bytes, occurrence.as_str());
    }
    bytes.push(u8::from(fact.proof.effective));
    ContentHash::domain_separated(FACT_DOMAIN, &bytes)
}

fn journal_root(authority: ContentHash, entries: &[SettlementJournalEntry]) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, authority);
    put_u64(&mut bytes, entries.len() as u64);
    for entry in entries {
        put_string(&mut bytes, &entry.settlement);
        put_string(&mut bytes, entry.debit.as_str());
        put_string(&mut bytes, entry.credit.as_str());
        put_string(&mut bytes, &entry.amount.canonical());
        put_hash(&mut bytes, entry.proof.authority);
    }
    ContentHash::domain_separated(JOURNAL_DOMAIN, &bytes)
}

fn close_root(
    authority: ContentHash,
    policy: &SettlementRecognitionPolicy,
    period: SettlementReportingPeriod,
    recognized_root: ContentHash,
    journal_root: Option<ContentHash>,
) -> ContentHash {
    let mut bytes = Vec::new();
    put_hash(&mut bytes, authority);
    put_hash(&mut bytes, policy.content_hash());
    put_hash(&mut bytes, recognized_root);
    match journal_root {
        Some(root) => {
            bytes.push(1);
            put_hash(&mut bytes, root);
        }
        None => bytes.push(0),
    }
    put_date(&mut bytes, period.start);
    put_date(&mut bytes, period.end);
    ContentHash::domain_separated(CLOSE_DOMAIN, &bytes)
}

fn invalid(reason: impl Into<String>) -> SettlementBookError {
    SettlementBookError::Invalid(reason.into())
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn put_hash(bytes: &mut Vec<u8>, value: ContentHash) {
    bytes.extend_from_slice(value.as_bytes());
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    put_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

fn put_date(bytes: &mut Vec<u8>, value: Date) {
    bytes.extend_from_slice(&value.year.to_be_bytes());
    bytes.push(value.month);
    bytes.push(value.day);
}

fn put_optional_date(bytes: &mut Vec<u8>, value: Option<Date>) {
    match value {
        Some(value) => {
            bytes.push(1);
            put_date(bytes, value);
        }
        None => bytes.push(0),
    }
}
