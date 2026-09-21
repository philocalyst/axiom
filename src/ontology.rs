//! The economic core beneath the journal surface.
//!
//! This module deliberately contains *records*, not a parser and not a book
//! recognizer.  An economic fact can be accepted into an event graph and can
//! later be projected into one or more books, but recognition is not allowed
//! to change the accepted graph.  The records are intentionally small and
//! extensible: a new event family implements [`EventRecord`] instead of
//! enlarging a central event enum.
//!
//! The types here are useful before a complete solver exists.  Constructors
//! preserve incomplete information; the validators are the places where
//! conservation, allocation, and lot invariants become explicit.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::exact::{ExactError, ExactNumber};
pub use crate::model::SettlementKind;
use crate::model::{
    AccountId, BookId, ContentHash, Date, EntityId, InstrumentId, LotId, ModelError, OccurrenceId,
    Quantity, Unit,
};
use num_bigint::BigInt;
use num_traits::One;

macro_rules! ontology_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

ontology_id!(RightId);
ontology_id!(EncumbranceId);
ontology_id!(PositionId);
ontology_id!(ObligationId);
ontology_id!(AllocationId);
ontology_id!(SettlementId);
ontology_id!(CorrectionId);

/// Errors raised by domain constructors and invariant checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OntologyError {
    Model(ModelError),
    Numeric(ExactError),
    EmptyIdentifier(&'static str),
    InvalidQuantity {
        context: &'static str,
    },
    InvalidInterval,
    RoleSharesExceedOne,
    DuplicateRoleHolder,
    UnitMismatch {
        left: String,
        right: String,
    },
    RestrictedQuantityExceeded,
    UnknownEncumbrance(EncumbranceId),
    TransferNotConserved {
        instrument: InstrumentId,
        sources: Box<Quantity>,
        destinations: Box<Quantity>,
    },
    ExchangeNotConserved {
        instrument: InstrumentId,
        outgoing: Box<Quantity>,
        incoming: Box<Quantity>,
    },
    ObligationOverallocated {
        obligation: ObligationId,
        promised: Box<Quantity>,
        allocated: Box<Quantity>,
    },
    SettlementOverallocated {
        settlement: SettlementId,
        amount: Box<Quantity>,
        allocated: Box<Quantity>,
    },
    DuplicateObligation(ObligationId),
    DuplicateSettlement(SettlementId),
    UnknownObligation(ObligationId),
    UnknownSettlement(SettlementId),
    DuplicateAllocation(AllocationId),
    InvalidSettlementTransition {
        from: Option<SettlementState>,
        to: SettlementState,
    },
    NonChronologicalSettlement {
        settlement: SettlementId,
        previous: Date,
        current: Date,
    },
    AllocationMismatch,
    LotInstrumentMismatch {
        lot: LotId,
        expected: InstrumentId,
        actual: InstrumentId,
    },
    LotOverconsumed {
        lot: LotId,
        remaining: Box<Quantity>,
        requested: Box<Quantity>,
    },
    MissingLot,
    DuplicateEvent(OccurrenceId),
    MissingEvent(OccurrenceId),
    InvalidEdge,
    InvalidEvent {
        kind: &'static str,
        reason: String,
    },
}

impl fmt::Display for OntologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => error.fmt(formatter),
            Self::Numeric(error) => error.fmt(formatter),
            Self::EmptyIdentifier(kind) => write!(formatter, "empty {kind} identifier"),
            Self::InvalidQuantity { context } => write!(formatter, "invalid quantity: {context}"),
            Self::InvalidInterval => formatter.write_str("invalid time interval"),
            Self::RoleSharesExceedOne => formatter.write_str("joint role shares exceed one"),
            Self::DuplicateRoleHolder => formatter.write_str("duplicate holder in a joint role"),
            Self::UnitMismatch { left, right } => {
                write!(formatter, "unit mismatch: {left} versus {right}")
            }
            Self::RestrictedQuantityExceeded => {
                formatter.write_str("encumbrances exceed the position quantity")
            }
            Self::UnknownEncumbrance(id) => write!(formatter, "unknown encumbrance {id}"),
            Self::TransferNotConserved {
                instrument,
                sources,
                destinations,
            } => write!(
                formatter,
                "transfer of {instrument} is not conserved: {sources} out versus {destinations} in"
            ),
            Self::ExchangeNotConserved {
                instrument,
                outgoing,
                incoming,
            } => write!(
                formatter,
                "exchange of {instrument} is not conserved: {outgoing} out versus {incoming} in"
            ),
            Self::ObligationOverallocated {
                obligation,
                promised,
                allocated,
            } => write!(
                formatter,
                "obligation {obligation} is overallocated: {allocated} of {promised}"
            ),
            Self::SettlementOverallocated {
                settlement,
                amount,
                allocated,
            } => write!(
                formatter,
                "settlement {settlement} is allocated {allocated} against {amount}"
            ),
            Self::DuplicateObligation(id) => write!(formatter, "duplicate obligation {id}"),
            Self::DuplicateSettlement(id) => write!(formatter, "duplicate settlement {id}"),
            Self::UnknownObligation(id) => write!(formatter, "unknown obligation {id}"),
            Self::UnknownSettlement(id) => write!(formatter, "unknown settlement {id}"),
            Self::DuplicateAllocation(id) => write!(formatter, "duplicate allocation {id}"),
            Self::InvalidSettlementTransition { from, to } => {
                write!(
                    formatter,
                    "illegal settlement transition {from:?} -> {to:?}"
                )
            }
            Self::NonChronologicalSettlement {
                settlement,
                previous,
                current,
            } => write!(
                formatter,
                "settlement {settlement} moves backward from {previous} to {current}"
            ),
            Self::AllocationMismatch => {
                formatter.write_str("allocation does not match its obligation")
            }
            Self::LotInstrumentMismatch {
                lot,
                expected,
                actual,
            } => write!(formatter, "lot {lot} contains {expected}, not {actual}"),
            Self::LotOverconsumed {
                lot,
                remaining,
                requested,
            } => write!(
                formatter,
                "lot {lot} has {remaining} remaining, requested {requested}"
            ),
            Self::MissingLot => formatter.write_str("disposal requires an explicit lot"),
            Self::DuplicateEvent(id) => write!(formatter, "duplicate event {id}"),
            Self::MissingEvent(id) => write!(formatter, "event edge refers to missing event {id}"),
            Self::InvalidEdge => formatter.write_str("invalid event edge"),
            Self::InvalidEvent { kind, reason } => write!(formatter, "invalid {kind}: {reason}"),
        }
    }
}

impl std::error::Error for OntologyError {}

impl From<ModelError> for OntologyError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<ExactError> for OntologyError {
    fn from(error: ExactError) -> Self {
        Self::Numeric(error)
    }
}

fn require_nonnegative(quantity: &Quantity, context: &'static str) -> Result<(), OntologyError> {
    if quantity.number.is_negative() {
        Err(OntologyError::InvalidQuantity { context })
    } else {
        Ok(())
    }
}

fn require_positive(quantity: &Quantity, context: &'static str) -> Result<(), OntologyError> {
    require_nonnegative(quantity, context)?;
    if quantity.is_zero() {
        Err(OntologyError::InvalidQuantity { context })
    } else {
        Ok(())
    }
}

fn sum_quantities<I>(quantities: I) -> Result<Quantity, OntologyError>
where
    I: IntoIterator<Item = Quantity>,
{
    let mut total: Option<Quantity> = None;
    for quantity in quantities {
        total = Some(match total {
            Some(total) => total.checked_add(&quantity)?,
            None => quantity,
        });
    }
    total.ok_or(OntologyError::InvalidQuantity {
        context: "an event needs at least one quantity",
    })
}

fn quantities_equal(left: &Quantity, right: &Quantity) -> Result<bool, OntologyError> {
    Ok(left.checked_sub(right)?.is_zero())
}

fn validate_named_instrument(
    named: &InstrumentId,
    instrument: &Instrument,
    kind: &'static str,
) -> Result<(), OntologyError> {
    if named != &instrument.id {
        return Err(OntologyError::InvalidEvent {
            kind,
            reason: format!(
                "record names {named}, supplied instrument is {}",
                instrument.id
            ),
        });
    }
    Ok(())
}

fn require_identifier(value: &str, kind: &'static str) -> Result<(), OntologyError> {
    if value.trim().is_empty() {
        Err(OntologyError::EmptyIdentifier(kind))
    } else {
        Ok(())
    }
}

fn validate_endpoint(endpoint: &Endpoint, kind: &'static str) -> Result<(), OntologyError> {
    require_identifier(endpoint.entity.as_str(), kind)?;
    if let Some(account) = &endpoint.account {
        require_identifier(account.as_str(), "account")?;
    }
    if let Some(position) = &endpoint.position {
        require_identifier(position.as_str(), "position")?;
    }
    Ok(())
}

/// Broad identity categories are intentionally descriptive.  They do not
/// decide ownership, which is represented by [`RoleAssignment`].
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EntityKind {
    Person,
    Organization,
    Trust,
    Household,
    Institution,
    Government,
    Fund,
    Estate,
    Software,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub name: String,
}

impl Entity {
    pub fn new(id: impl Into<EntityId>, kind: EntityKind) -> Self {
        let id = id.into();
        Self {
            name: id.to_string(),
            id,
            kind,
        }
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

/// Roles are relations rather than attributes on an entity or position.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Role {
    LegalOwner,
    BeneficialOwner,
    Custodian,
    Controller,
    AuthorizedUser,
    Issuer,
    Debtor,
    Creditor,
    Payer,
    Payee,
    Employer,
    Employee,
    Agent,
    Principal,
    Trustee,
    Beneficiary,
    TaxOwner,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleAssignment {
    pub subject: EntityId,
    pub role: Role,
    pub holder: EntityId,
    /// A fractional share is optional.  If present for joint holders, the
    /// shares are checked to be no greater than one.
    pub share: Option<ExactNumber>,
    pub effective_from: Option<Date>,
    pub effective_until: Option<Date>,
    pub disputed: bool,
}

impl RoleAssignment {
    pub fn new(subject: impl Into<EntityId>, role: Role, holder: impl Into<EntityId>) -> Self {
        Self {
            subject: subject.into(),
            role,
            holder: holder.into(),
            share: None,
            effective_from: None,
            effective_until: None,
            disputed: false,
        }
    }

    pub fn with_share(mut self, share: ExactNumber) -> Self {
        self.share = Some(share);
        self
    }

    pub fn during(mut self, from: Option<Date>, until: Option<Date>) -> Self {
        self.effective_from = from;
        self.effective_until = until;
        self
    }

    pub fn disputed(mut self) -> Self {
        self.disputed = true;
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoleAssignments {
    pub assignments: Vec<RoleAssignment>,
}

impl RoleAssignments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, assignment: RoleAssignment) {
        self.assignments.push(assignment);
    }

    pub fn with(mut self, assignment: RoleAssignment) -> Self {
        self.push(assignment);
        self
    }

    /// Add a joint role with equal shares.  The holders remain separate
    /// relations, so a later policy can dispute or replace one holder without
    /// rewriting the others.
    pub fn joint(
        subject: impl Into<EntityId>,
        role: Role,
        holders: impl IntoIterator<Item = EntityId>,
    ) -> Result<Self, OntologyError> {
        let subject = subject.into();
        let holders: Vec<_> = holders.into_iter().collect();
        if holders.is_empty() {
            return Err(OntologyError::InvalidEvent {
                kind: "joint role",
                reason: "a joint role needs a holder".to_string(),
            });
        }
        let share = ExactNumber::rational(1i64, holders.len() as i64)?;
        let mut assignments = Self::new();
        for holder in holders {
            assignments.push(
                RoleAssignment::new(subject.clone(), role.clone(), holder)
                    .with_share(share.clone()),
            );
        }
        assignments.validate()?;
        Ok(assignments)
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        for assignment in &self.assignments {
            if assignment.subject.is_empty() || assignment.holder.is_empty() {
                return Err(OntologyError::EmptyIdentifier("role"));
            }
            if let (Some(from), Some(until)) =
                (assignment.effective_from, assignment.effective_until)
                && from > until
            {
                return Err(OntologyError::InvalidInterval);
            }
            if let Some(share) = &assignment.share
                && share.is_negative()
            {
                return Err(OntologyError::InvalidQuantity {
                    context: "a role share cannot be negative",
                });
            }
        }

        // The same holder may receive a historical assignment more than once,
        // provided those assignments do not overlap. This is distinct from a
        // duplicate active relation.
        for (index, left) in self.assignments.iter().enumerate() {
            for right in self.assignments.iter().skip(index + 1) {
                if left.subject == right.subject
                    && left.role == right.role
                    && left.holder == right.holder
                    && role_intervals_overlap(left, right)
                {
                    return Err(OntologyError::DuplicateRoleHolder);
                }
            }
        }

        let mut groups: BTreeMap<(EntityId, Role), Vec<&RoleAssignment>> = BTreeMap::new();
        for assignment in &self.assignments {
            if assignment.share.is_some() {
                groups
                    .entry((assignment.subject.clone(), assignment.role.clone()))
                    .or_default()
                    .push(assignment);
            }
        }
        for assignments in groups.values() {
            let boundary_dates: BTreeSet<Date> = assignments
                .iter()
                .flat_map(|assignment| {
                    assignment
                        .effective_from
                        .into_iter()
                        .chain(assignment.effective_until)
                })
                .collect();
            if boundary_dates.is_empty() {
                validate_role_share_slice(assignments, None)?;
            } else {
                for date in boundary_dates {
                    validate_role_share_slice(assignments, Some(date))?;
                }
            }
        }
        Ok(())
    }

    pub fn holders(&self, subject: &EntityId, role: &Role) -> impl Iterator<Item = &EntityId> {
        self.assignments
            .iter()
            .filter(move |assignment| &assignment.subject == subject && &assignment.role == role)
            .map(|assignment| &assignment.holder)
    }
}

fn role_intervals_overlap(left: &RoleAssignment, right: &RoleAssignment) -> bool {
    left.effective_until
        .is_none_or(|until| right.effective_from.is_none_or(|from| from <= until))
        && right
            .effective_until
            .is_none_or(|until| left.effective_from.is_none_or(|from| from <= until))
}

fn role_active_at(assignment: &RoleAssignment, date: Option<Date>) -> bool {
    let Some(date) = date else {
        return true;
    };
    assignment.effective_from.is_none_or(|from| from <= date)
        && assignment.effective_until.is_none_or(|until| date <= until)
}

fn validate_role_share_slice(
    assignments: &[&RoleAssignment],
    date: Option<Date>,
) -> Result<(), OntologyError> {
    let mut total = ExactNumber::integer(0);
    for assignment in assignments
        .iter()
        .filter(|assignment| role_active_at(assignment, date))
    {
        if let Some(share) = &assignment.share {
            total = total.checked_add(share);
        }
    }
    if total > ExactNumber::integer(1) {
        Err(OntologyError::RoleSharesExceedOne)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InstrumentKind {
    Currency,
    Equity,
    DebtSecurity,
    Derivative,
    PhysicalGood,
    ServiceUnit,
    Claim,
    LoyaltyPoint,
    EnergyUnit,
    CarbonCredit,
    Basket,
    UniqueAsset,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instrument {
    pub id: InstrumentId,
    pub kind: InstrumentKind,
    pub issuer: Option<EntityId>,
    pub unit: Option<Unit>,
    pub quantum: Option<Quantity>,
    pub transferable: bool,
    pub expires: Option<Date>,
    pub rights: BTreeSet<RightId>,
}

impl Instrument {
    pub fn new(id: impl Into<InstrumentId>, kind: InstrumentKind) -> Self {
        Self {
            id: id.into(),
            kind,
            issuer: None,
            unit: None,
            quantum: None,
            transferable: true,
            expires: None,
            rights: BTreeSet::new(),
        }
    }

    pub fn issued_by(mut self, issuer: impl Into<EntityId>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    pub fn denominated(mut self, unit: Unit) -> Self {
        self.unit = Some(unit);
        self
    }

    pub fn with_quantum(mut self, quantum: Quantity) -> Self {
        self.quantum = Some(quantum);
        self
    }

    pub fn nontransferable(mut self) -> Self {
        self.transferable = false;
        self
    }

    pub fn has_right(&self, right: &RightId) -> bool {
        self.rights.contains(right)
    }

    /// Validate a quantity against this instrument's declared unit. Zero is
    /// intentionally polymorphic, matching the model's quantity rule.
    pub fn validate_quantity(&self, quantity: &Quantity) -> Result<(), OntologyError> {
        require_nonnegative(quantity, "an instrument quantity")?;
        let expected_unit = self.unit.as_ref().or_else(|| {
            self.quantum
                .as_ref()
                .and_then(|quantum| quantum.unit.as_ref())
        });
        if let Some(expected) = expected_unit
            && !quantity.is_zero()
            && quantity.unit.as_ref() != Some(expected)
        {
            return Err(OntologyError::UnitMismatch {
                left: self
                    .unit
                    .as_ref()
                    .or_else(|| self.quantum.as_ref().and_then(|q| q.unit.as_ref()))
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<unitless instrument>".to_string()),
                right: quantity
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
            });
        }
        if let Some(quantum) = &self.quantum
            && !quantity.is_zero()
        {
            let ratio = quantity.number.checked_div(&quantum.number)?;
            if ratio.as_rational().denom() != &BigInt::one() {
                return Err(OntologyError::InvalidQuantity {
                    context: "quantity is not an exact multiple of the instrument quantum",
                });
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        if let Some(quantum) = &self.quantum {
            require_positive(quantum, "an instrument quantum")?;
            self.validate_quantity(quantum)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RightKind {
    Ownership,
    BeneficialUse,
    Custody,
    Control,
    Transfer,
    Redemption,
    Income,
    Voting,
    Collateral,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Right {
    pub id: RightId,
    pub kind: RightKind,
    pub holder: EntityId,
    pub instrument: Option<InstrumentId>,
    pub effective_from: Option<Date>,
    pub effective_until: Option<Date>,
}

impl Right {
    pub fn new(id: impl Into<RightId>, kind: RightKind, holder: impl Into<EntityId>) -> Self {
        Self {
            id: id.into(),
            kind,
            holder: holder.into(),
            instrument: None,
            effective_from: None,
            effective_until: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EncumbranceKind {
    Hold,
    Pledge,
    Earmark,
    Collateral,
    PendingTransfer,
    Restricted,
    Other(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Encumbrance {
    pub id: EncumbranceId,
    pub kind: EncumbranceKind,
    pub quantity: Option<Quantity>,
    pub beneficiary: Option<EntityId>,
    pub reason: Option<String>,
    released: bool,
}

impl Encumbrance {
    pub fn whole(id: impl Into<EncumbranceId>, kind: EncumbranceKind) -> Self {
        Self {
            id: id.into(),
            kind,
            quantity: None,
            beneficiary: None,
            reason: None,
            released: false,
        }
    }

    pub fn for_quantity(
        id: impl Into<EncumbranceId>,
        kind: EncumbranceKind,
        quantity: Quantity,
    ) -> Result<Self, OntologyError> {
        require_nonnegative(&quantity, "an encumbrance quantity")?;
        Ok(Self {
            id: id.into(),
            kind,
            quantity: Some(quantity),
            beneficiary: None,
            reason: None,
            released: false,
        })
    }

    pub fn for_beneficiary(mut self, beneficiary: impl Into<EntityId>) -> Self {
        self.beneficiary = Some(beneficiary.into());
        self
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn release(mut self) -> Self {
        self.released = true;
        self
    }

    pub fn is_released(&self) -> bool {
        self.released
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        if let Some(quantity) = &self.quantity {
            require_nonnegative(quantity, "an encumbrance quantity")?;
        }
        Ok(())
    }
}

/// A material account is a real venue or contract boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialAccount {
    pub id: AccountId,
    pub contract: String,
    pub legal_holder: Option<EntityId>,
    pub beneficiary: Option<EntityId>,
    pub controller: Option<EntityId>,
    pub accepts: BTreeSet<InstrumentId>,
    pub operational_quantum: BTreeMap<InstrumentId, Quantity>,
}

impl MaterialAccount {
    pub fn new(id: impl Into<AccountId>, contract: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            contract: contract.into(),
            legal_holder: None,
            beneficiary: None,
            controller: None,
            accepts: BTreeSet::new(),
            operational_quantum: BTreeMap::new(),
        }
    }

    pub fn legal_holder(mut self, holder: impl Into<EntityId>) -> Self {
        self.legal_holder = Some(holder.into());
        self
    }

    pub fn beneficiary(mut self, beneficiary: impl Into<EntityId>) -> Self {
        self.beneficiary = Some(beneficiary.into());
        self
    }

    pub fn controller(mut self, controller: impl Into<EntityId>) -> Self {
        self.controller = Some(controller.into());
        self
    }

    pub fn accepts(mut self, instrument: impl Into<InstrumentId>) -> Self {
        self.accepts.insert(instrument.into());
        self
    }

    pub fn operational_quantum(
        mut self,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        self.operational_quantum.insert(instrument.into(), quantity);
        self
    }
}

/// A virtual account is a named view; it cannot pretend to be an external
/// custody boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualAccount {
    pub id: AccountId,
    pub name: String,
    pub query: String,
}

impl VirtualAccount {
    pub fn new(id: impl Into<AccountId>, query: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.to_string(),
            id,
            query: query.into(),
        }
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

/// A book account exists only inside a recognition policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookAccount {
    pub book: BookId,
    pub id: AccountId,
    pub name: String,
}

impl BookAccount {
    pub fn new(book: impl Into<BookId>, id: impl Into<AccountId>, name: impl Into<String>) -> Self {
        Self {
            book: book.into(),
            id: id.into(),
            name: name.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub id: PositionId,
    pub beneficiary: EntityId,
    pub custodian: Option<EntityId>,
    pub account: Option<AccountId>,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    pub rights: BTreeSet<RightId>,
    pub encumbrances: BTreeSet<EncumbranceId>,
    pub lot: Option<LotId>,
    pub valid_from: Option<Date>,
    pub valid_until: Option<Date>,
}

impl Position {
    pub fn new(
        id: impl Into<PositionId>,
        beneficiary: impl Into<EntityId>,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Result<Self, OntologyError> {
        require_nonnegative(&quantity, "a position cannot be negative")?;
        Ok(Self {
            id: id.into(),
            beneficiary: beneficiary.into(),
            custodian: None,
            account: None,
            instrument: instrument.into(),
            quantity,
            rights: BTreeSet::new(),
            encumbrances: BTreeSet::new(),
            lot: None,
            valid_from: None,
            valid_until: None,
        })
    }

    pub fn at_account(mut self, account: impl Into<AccountId>) -> Self {
        self.account = Some(account.into());
        self
    }

    pub fn held_by(mut self, custodian: impl Into<EntityId>) -> Self {
        self.custodian = Some(custodian.into());
        self
    }

    pub fn from_lot(mut self, lot: impl Into<LotId>) -> Self {
        self.lot = Some(lot.into());
        self
    }

    pub fn with_right(mut self, right: impl Into<RightId>) -> Self {
        self.rights.insert(right.into());
        self
    }

    pub fn with_encumbrance(mut self, encumbrance: impl Into<EncumbranceId>) -> Self {
        self.encumbrances.insert(encumbrance.into());
        self
    }

    pub fn restricted(&self) -> bool {
        !self.encumbrances.is_empty()
    }

    /// Determine restriction using the current encumbrance records. Released
    /// records remain provenance, but no longer reduce availability.
    pub fn is_restricted(
        &self,
        encumbrances: &BTreeMap<EncumbranceId, Encumbrance>,
    ) -> Result<bool, OntologyError> {
        Ok(self.available_quantity(encumbrances)?.number < self.quantity.number)
    }

    /// Calculate what is available after explicit, unreleased encumbrances.
    /// An unquantified encumbrance reserves the whole position.
    pub fn available_quantity(
        &self,
        encumbrances: &BTreeMap<EncumbranceId, Encumbrance>,
    ) -> Result<Quantity, OntologyError> {
        let mut reserved = Quantity::zero();
        for id in &self.encumbrances {
            let encumbrance = encumbrances
                .get(id)
                .ok_or_else(|| OntologyError::UnknownEncumbrance(id.clone()))?;
            encumbrance.validate()?;
            if encumbrance.is_released() {
                continue;
            }
            let Some(quantity) = &encumbrance.quantity else {
                return Ok(Quantity::new(
                    ExactNumber::integer(0),
                    self.quantity.unit.clone(),
                )?);
            };
            reserved = reserved.checked_add(quantity)?;
        }
        if reserved.number > self.quantity.number {
            return Err(OntologyError::RestrictedQuantityExceeded);
        }
        Ok(self.quantity.checked_sub(&reserved)?)
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        validate_named_instrument(&self.instrument, instrument, "position")?;
        instrument.validate_quantity(&self.quantity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lot {
    pub id: LotId,
    pub instrument: InstrumentId,
    pub acquired: Quantity,
    pub remaining: Quantity,
    pub acquisition: OccurrenceId,
    pub basis: Option<Quantity>,
}

impl Lot {
    pub fn new(
        id: impl Into<LotId>,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
        acquisition: impl Into<OccurrenceId>,
    ) -> Result<Self, OntologyError> {
        require_positive(&quantity, "a lot must contain a positive quantity")?;
        Ok(Self {
            id: id.into(),
            instrument: instrument.into(),
            acquired: quantity.clone(),
            remaining: quantity,
            acquisition: acquisition.into(),
            basis: None,
        })
    }

    pub fn with_basis(mut self, basis: Quantity) -> Self {
        self.basis = Some(basis);
        self
    }

    /// Return the post-consumption lot without mutating accepted state.  The
    /// returned value can be attached to an explicit disposal/correction
    /// transition by a caller that wants to commit it.
    pub fn consumed(&self, quantity: &Quantity) -> Result<Self, OntologyError> {
        require_positive(quantity, "a lot consumption must be positive")?;
        if quantity.unit != self.remaining.unit {
            return Err(OntologyError::UnitMismatch {
                left: self
                    .remaining
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                right: quantity
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
            });
        }
        if quantity.number > self.remaining.number {
            return Err(OntologyError::LotOverconsumed {
                lot: self.id.clone(),
                remaining: Box::new(self.remaining.clone()),
                requested: Box::new(quantity.clone()),
            });
        }
        let mut next = self.clone();
        next.remaining = self.remaining.checked_sub(quantity)?;
        Ok(next)
    }

    pub fn exhausted(&self) -> bool {
        self.remaining.is_zero()
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.acquired, "a lot must contain a positive quantity")?;
        require_nonnegative(
            &self.remaining,
            "a lot cannot have negative remaining quantity",
        )?;
        if self.remaining.unit != self.acquired.unit || self.remaining.number > self.acquired.number
        {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} has invalid remaining quantity", self.id),
            });
        }
        Ok(())
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        validate_named_instrument(&self.instrument, instrument, "lot")?;
        instrument.validate_quantity(&self.acquired)?;
        instrument.validate_quantity(&self.remaining)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Performance {
    Transfer {
        instrument: InstrumentId,
        quantity: Quantity,
        from: Option<EntityId>,
        to: EntityId,
    },
    Deliver {
        instrument: InstrumentId,
        quantity: Quantity,
        to: EntityId,
    },
    Service {
        description: String,
    },
}

impl Performance {
    pub fn quantity(&self) -> Option<&Quantity> {
        match self {
            Self::Transfer { quantity, .. } | Self::Deliver { quantity, .. } => Some(quantity),
            Self::Service { .. } => None,
        }
    }

    fn instrument(&self) -> Option<&InstrumentId> {
        match self {
            Self::Transfer { instrument, .. } | Self::Deliver { instrument, .. } => {
                Some(instrument)
            }
            Self::Service { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Obligation {
    pub id: ObligationId,
    pub debtor: EntityId,
    pub creditor: EntityId,
    pub performance: Performance,
    pub due: Option<Date>,
    pub contract: Option<OccurrenceId>,
}

impl Obligation {
    pub fn transfer(
        id: impl Into<ObligationId>,
        debtor: impl Into<EntityId>,
        creditor: impl Into<EntityId>,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Result<Self, OntologyError> {
        require_positive(&quantity, "an obligation must promise a positive quantity")?;
        let creditor = creditor.into();
        Ok(Self {
            id: id.into(),
            debtor: debtor.into(),
            creditor: creditor.clone(),
            performance: Performance::Transfer {
                instrument: instrument.into(),
                quantity,
                from: None,
                to: creditor,
            },
            due: None,
            contract: None,
        })
    }

    pub fn due_on(mut self, due: Date) -> Self {
        self.due = Some(due);
        self
    }

    pub fn under(mut self, contract: impl Into<OccurrenceId>) -> Self {
        self.contract = Some(contract.into());
        self
    }

    /// Validate the structural parts of an obligation before it participates
    /// in a satisfaction network.  Builders reject invalid quantities, but
    /// the public fields intentionally remain inspectable and can be assembled
    /// directly by importers.
    pub fn validate(&self) -> Result<(), OntologyError> {
        require_identifier(self.id.as_str(), "obligation")?;
        require_identifier(self.debtor.as_str(), "debtor")?;
        require_identifier(self.creditor.as_str(), "creditor")?;
        if let Some(contract) = &self.contract {
            require_identifier(contract.as_str(), "contract")?;
        }
        let quantity = self.promised_quantity()?;
        require_positive(quantity, "an obligation must promise a positive quantity")?;
        match &self.performance {
            Performance::Transfer {
                instrument,
                from,
                to,
                ..
            } => {
                require_identifier(instrument.as_str(), "instrument")?;
                require_identifier(to.as_str(), "creditor")?;
                if quantity.unit.as_ref().map(Unit::as_str) != Some(instrument.as_str()) {
                    return Err(OntologyError::UnitMismatch {
                        left: instrument.to_string(),
                        right: quantity
                            .unit
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "<missing>".to_string()),
                    });
                }
                if to != &self.creditor {
                    return Err(OntologyError::AllocationMismatch);
                }
                if let Some(from) = from {
                    require_identifier(from.as_str(), "debtor")?;
                    if from != &self.debtor {
                        return Err(OntologyError::AllocationMismatch);
                    }
                }
            }
            Performance::Deliver { instrument, to, .. } => {
                require_identifier(instrument.as_str(), "instrument")?;
                require_identifier(to.as_str(), "creditor")?;
                if quantity.unit.as_ref().map(Unit::as_str) != Some(instrument.as_str()) {
                    return Err(OntologyError::UnitMismatch {
                        left: instrument.to_string(),
                        right: quantity
                            .unit
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "<missing>".to_string()),
                    });
                }
                if to != &self.creditor {
                    return Err(OntologyError::AllocationMismatch);
                }
            }
            Performance::Service { description } if description.trim().is_empty() => {
                return Err(OntologyError::InvalidEvent {
                    kind: "obligation",
                    reason: "service performance needs a description".to_string(),
                });
            }
            Performance::Service { .. } => {}
        }
        Ok(())
    }

    pub fn promised_quantity(&self) -> Result<&Quantity, OntologyError> {
        self.performance
            .quantity()
            .ok_or(OntologyError::InvalidEvent {
                kind: "obligation",
                reason: "service performance has no scalar quantity".to_string(),
            })
    }

    pub fn remaining(
        &self,
        allocations: &[SatisfactionAllocation],
        settlements: &[Settlement],
    ) -> Result<Quantity, OntologyError> {
        validate_obligation_allocation(self, allocations, settlements)?;
        let promised = self.promised_quantity()?.clone();
        let mut allocated = Quantity::zero();
        for allocation in allocations.iter().filter(|allocation| {
            allocation.obligation == self.id && allocation.state == AllocationState::Applied
        }) {
            let Some(settlement) = settlements
                .iter()
                .find(|settlement| settlement.id == allocation.settlement)
            else {
                return Err(OntologyError::AllocationMismatch);
            };
            if settlement.is_effective() {
                allocated = allocated.checked_add(&allocation.quantity)?;
            }
        }
        Ok(promised.checked_sub(&allocated)?)
    }

    pub fn is_satisfied(
        &self,
        allocations: &[SatisfactionAllocation],
        settlements: &[Settlement],
    ) -> Result<bool, OntologyError> {
        Ok(self.remaining(allocations, settlements)?.is_zero())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementState {
    Issued,
    Authorized,
    Presented,
    Pending,
    Settled,
    Returned,
    Reversed,
    Rejected,
    Cancelled,
    Refunded,
    Disputed,
    ChargedBack,
    Represented,
    Resolved,
}

fn settlement_transition_is_legal(from: Option<&SettlementState>, to: &SettlementState) -> bool {
    matches!(
        (from, to),
        (None, SettlementState::Issued)
            | (Some(SettlementState::Issued), SettlementState::Authorized)
            | (Some(SettlementState::Issued), SettlementState::Presented)
            | (Some(SettlementState::Issued), SettlementState::Cancelled)
            | (
                Some(SettlementState::Authorized),
                SettlementState::Presented
            )
            | (
                Some(SettlementState::Authorized),
                SettlementState::Cancelled
            )
            | (Some(SettlementState::Authorized), SettlementState::Rejected)
            | (Some(SettlementState::Presented), SettlementState::Pending)
            | (Some(SettlementState::Presented), SettlementState::Settled)
            | (Some(SettlementState::Presented), SettlementState::Returned)
            | (Some(SettlementState::Presented), SettlementState::Rejected)
            | (Some(SettlementState::Presented), SettlementState::Cancelled)
            | (Some(SettlementState::Pending), SettlementState::Settled)
            | (Some(SettlementState::Pending), SettlementState::Returned)
            | (Some(SettlementState::Pending), SettlementState::Rejected)
            | (Some(SettlementState::Pending), SettlementState::Cancelled)
            | (Some(SettlementState::Settled), SettlementState::Returned)
            | (Some(SettlementState::Settled), SettlementState::Reversed)
            | (Some(SettlementState::Settled), SettlementState::Refunded)
            | (Some(SettlementState::Settled), SettlementState::Disputed)
            | (Some(SettlementState::Settled), SettlementState::ChargedBack)
            | (Some(SettlementState::Disputed), SettlementState::Resolved)
            | (
                Some(SettlementState::Disputed),
                SettlementState::ChargedBack
            )
            | (
                Some(SettlementState::ChargedBack),
                SettlementState::Represented
            )
            | (Some(SettlementState::Represented), SettlementState::Pending)
            | (Some(SettlementState::Represented), SettlementState::Settled)
            | (
                Some(SettlementState::Represented),
                SettlementState::Rejected
            )
            | (Some(SettlementState::Returned), SettlementState::Presented)
            | (Some(SettlementState::Returned), SettlementState::Cancelled)
            | (Some(SettlementState::Reversed), SettlementState::Presented)
            | (Some(SettlementState::Reversed), SettlementState::Cancelled)
            | (Some(SettlementState::Rejected), SettlementState::Presented)
            | (Some(SettlementState::Rejected), SettlementState::Cancelled)
    )
}

/// The broad state machine above is retained for callers that only know that
/// a value is a settlement.  Once a payment rail is known, callers should use
/// this stricter machine.  In particular, a check return may be presented
/// again, while a returned ACH is a failed attempt that needs a new
/// settlement; a card chargeback can be represented, but a check cannot be
/// charged back.
fn settlement_transition_is_legal_for(
    kind: SettlementKind,
    from: Option<&SettlementState>,
    to: &SettlementState,
) -> bool {
    use SettlementState::*;

    match kind {
        SettlementKind::Ach => matches!(
            (from, to),
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
            (from, to),
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
            (from, to),
            (None, Issued)
                | (Some(Issued), Presented | Cancelled)
                | (Some(Presented), Pending | Settled | Returned | Rejected | Cancelled)
                | (Some(Pending), Settled | Returned | Rejected | Cancelled)
                | (Some(Settled), Returned)
                // Re-presentation is a new attempt in the same check
                // history. It is deliberately the only legal transition
                // out of Returned other than cancellation.
                | (Some(Returned), Presented | Cancelled)
        ),
    }
}

/// Validate a settlement history for a known payment rail.  The history is
/// consumed in source order and is never sorted or repaired.
pub fn validate_settlement_states_for(
    kind: SettlementKind,
    states: &[SettlementState],
) -> Result<(), OntologyError> {
    let mut previous: Option<&SettlementState> = None;
    for state in states {
        if !settlement_transition_is_legal_for(kind, previous, state) {
            return Err(OntologyError::InvalidSettlementTransition {
                from: previous.cloned(),
                to: state.clone(),
            });
        }
        previous = Some(state);
    }
    Ok(())
}

/// Descriptive alias for callers that prefer the explicit "kind" wording.
pub fn validate_settlement_states_for_kind(
    kind: SettlementKind,
    states: &[SettlementState],
) -> Result<(), OntologyError> {
    validate_settlement_states_for(kind, states)
}

pub fn validate_settlement_states(states: &[SettlementState]) -> Result<(), OntologyError> {
    let mut previous: Option<&SettlementState> = None;
    for state in states {
        if !settlement_transition_is_legal(previous, state) {
            return Err(OntologyError::InvalidSettlementTransition {
                from: previous.cloned(),
                to: state.clone(),
            });
        }
        previous = Some(state);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementTransition {
    pub state: SettlementState,
    pub at: Option<Date>,
    pub reason: Option<String>,
}

/// Validate an ordered settlement history, including the date order carried
/// by transitions.  Missing dates do not reset the ordering cursor: once two
/// dated observations establish an order, a later dated observation may not
/// move backwards merely because an intervening event had no date.
pub fn validate_settlement_history(
    settlement: &SettlementId,
    history: &[SettlementTransition],
) -> Result<(), OntologyError> {
    if history.is_empty() {
        return Err(OntologyError::InvalidEvent {
            kind: "settlement",
            reason: "settlement history cannot be empty".to_string(),
        });
    }
    let mut previous_at = None;
    for transition in history {
        if let (Some(previous), Some(current)) = (previous_at, transition.at)
            && current < previous
        {
            return Err(OntologyError::NonChronologicalSettlement {
                settlement: settlement.clone(),
                previous,
                current,
            });
        }
        previous_at = transition.at.or(previous_at);
    }
    validate_settlement_states(
        &history
            .iter()
            .map(|transition| transition.state.clone())
            .collect::<Vec<_>>(),
    )
}

/// Validate an ordered history for an explicitly identified payment rail.
pub fn validate_settlement_history_for(
    settlement: &SettlementId,
    kind: SettlementKind,
    history: &[SettlementTransition],
) -> Result<(), OntologyError> {
    if history.is_empty() {
        return Err(OntologyError::InvalidEvent {
            kind: "settlement",
            reason: "settlement history cannot be empty".to_string(),
        });
    }
    let mut previous_at = None;
    for transition in history {
        if let (Some(previous), Some(current)) = (previous_at, transition.at)
            && current < previous
        {
            return Err(OntologyError::NonChronologicalSettlement {
                settlement: settlement.clone(),
                previous,
                current,
            });
        }
        previous_at = transition.at.or(previous_at);
    }
    validate_settlement_states_for(
        kind,
        &history
            .iter()
            .map(|transition| transition.state.clone())
            .collect::<Vec<_>>(),
    )
}

pub fn validate_settlement_history_for_kind(
    settlement: &SettlementId,
    kind: SettlementKind,
    history: &[SettlementTransition],
) -> Result<(), OntologyError> {
    validate_settlement_history_for(settlement, kind, history)
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Endpoint {
    pub entity: EntityId,
    pub account: Option<AccountId>,
    pub position: Option<PositionId>,
}

impl Endpoint {
    pub fn entity(entity: impl Into<EntityId>) -> Self {
        Self {
            entity: entity.into(),
            account: None,
            position: None,
        }
    }

    pub fn at_account(mut self, account: impl Into<AccountId>) -> Self {
        self.account = Some(account.into());
        self
    }

    pub fn at_position(mut self, position: impl Into<PositionId>) -> Self {
        self.position = Some(position.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    pub id: SettlementId,
    pub from: Endpoint,
    pub to: Endpoint,
    pub instrument: InstrumentId,
    pub amount: Quantity,
    /// `None` keeps the original open-world settlement API.  A known rail is
    /// opt-in through [`Settlement::new_with_kind`] or [`Settlement::with_kind`]
    /// and enables the stricter instrument-specific state machine.
    kind: Option<SettlementKind>,
    history: Vec<SettlementTransition>,
}

impl Settlement {
    pub fn new(
        id: impl Into<SettlementId>,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        amount: Quantity,
    ) -> Result<Self, OntologyError> {
        require_positive(&amount, "a settlement must have a positive amount")?;
        Ok(Self {
            id: id.into(),
            from,
            to,
            instrument: instrument.into(),
            amount,
            kind: None,
            history: vec![SettlementTransition {
                state: SettlementState::Issued,
                at: None,
                reason: None,
            }],
        })
    }

    pub fn from_history(
        id: impl Into<SettlementId>,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        amount: Quantity,
        history: Vec<SettlementTransition>,
    ) -> Result<Self, OntologyError> {
        let settlement = Self {
            id: id.into(),
            from,
            to,
            instrument: instrument.into(),
            amount,
            kind: None,
            history,
        };
        settlement.validate()?;
        Ok(settlement)
    }

    /// Construct a settlement whose rail is explicit.  No rail is inferred
    /// from an instrument id, endpoint, or state history.
    pub fn new_with_kind(
        id: impl Into<SettlementId>,
        kind: SettlementKind,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        amount: Quantity,
    ) -> Result<Self, OntologyError> {
        Self::new(id, from, to, instrument, amount)?.with_kind(kind)
    }

    /// Descriptive constructor alias for callers that put the rail first.
    pub fn for_kind(
        kind: SettlementKind,
        id: impl Into<SettlementId>,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        amount: Quantity,
    ) -> Result<Self, OntologyError> {
        Self::new_with_kind(id, kind, from, to, instrument, amount)
    }

    /// Construct a typed settlement from an already observed append-only
    /// history.
    pub fn from_history_with_kind(
        id: impl Into<SettlementId>,
        kind: SettlementKind,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        amount: Quantity,
        history: Vec<SettlementTransition>,
    ) -> Result<Self, OntologyError> {
        let settlement = Self {
            id: id.into(),
            from,
            to,
            instrument: instrument.into(),
            amount,
            kind: Some(kind),
            history,
        };
        settlement.validate()?;
        Ok(settlement)
    }

    /// Add rail information to an existing open-world settlement.  This is a
    /// consuming builder: the original value cannot be changed behind the
    /// caller's back, and the observed history must already satisfy the rail.
    pub fn with_kind(mut self, kind: SettlementKind) -> Result<Self, OntologyError> {
        validate_settlement_history_for(&self.id, kind, &self.history)?;
        self.kind = Some(kind);
        Ok(self)
    }

    pub fn kind(&self) -> Option<SettlementKind> {
        self.kind
    }

    pub fn settlement_kind(&self) -> Option<SettlementKind> {
        self.kind()
    }

    pub fn transition(
        &mut self,
        state: SettlementState,
        at: Option<Date>,
        reason: Option<String>,
    ) -> Result<(), OntologyError> {
        if let (Some(previous), Some(current)) = (
            self.history
                .iter()
                .rev()
                .find_map(|transition| transition.at),
            at,
        ) && current < previous
        {
            return Err(OntologyError::NonChronologicalSettlement {
                settlement: self.id.clone(),
                previous,
                current,
            });
        }
        let legal = match self.kind {
            Some(kind) => settlement_transition_is_legal_for(kind, self.latest_state(), &state),
            None => settlement_transition_is_legal(self.latest_state(), &state),
        };
        if !legal {
            return Err(OntologyError::InvalidSettlementTransition {
                from: self.latest_state().cloned(),
                to: state,
            });
        }
        self.history
            .push(SettlementTransition { state, at, reason });
        Ok(())
    }

    pub fn latest_state(&self) -> Option<&SettlementState> {
        self.history.last().map(|transition| &transition.state)
    }

    pub fn is_effective(&self) -> bool {
        // `Resolved` only says that a dispute reached an outcome; this
        // record does not carry that outcome.  Treating it as effective would
        // silently choose "the payment stood" over "the payment was
        // reversed".  A resolved dispute therefore needs a separate explicit
        // settled observation before it can satisfy an obligation.
        matches!(self.latest_state(), Some(SettlementState::Settled))
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_identifier(self.id.as_str(), "settlement")?;
        validate_endpoint(&self.from, "settlement source entity")?;
        validate_endpoint(&self.to, "settlement destination entity")?;
        require_identifier(self.instrument.as_str(), "instrument")?;
        require_positive(&self.amount, "a settlement must have a positive amount")?;
        if self.amount.unit.as_ref().map(Unit::as_str) != Some(self.instrument.as_str()) {
            return Err(OntologyError::UnitMismatch {
                left: self.instrument.to_string(),
                right: self
                    .amount
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<missing>".to_string()),
            });
        }
        match self.kind {
            Some(kind) => validate_settlement_history_for(&self.id, kind, &self.history),
            None => validate_settlement_history(&self.id, &self.history),
        }
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        validate_named_instrument(&self.instrument, instrument, "settlement")?;
        instrument.validate_quantity(&self.amount)
    }

    pub fn returned(
        &mut self,
        at: Option<Date>,
        reason: impl Into<String>,
    ) -> Result<(), OntologyError> {
        self.transition(SettlementState::Returned, at, Some(reason.into()))
    }

    /// Return a new settlement with one appended transition, leaving the
    /// source settlement untouched.  This is the preferred form when a
    /// correction, reversal, refund, or chargeback arrives as a later fact.
    pub fn appended(
        &self,
        state: SettlementState,
        at: Option<Date>,
        reason: Option<String>,
    ) -> Result<Self, OntologyError> {
        let mut next = self.clone();
        next.transition(state, at, reason)?;
        Ok(next)
    }

    /// Alias emphasizing that a transition is an append-only observation.
    pub fn append_transition(
        &self,
        state: SettlementState,
        at: Option<Date>,
        reason: Option<String>,
    ) -> Result<Self, OntologyError> {
        self.appended(state, at, reason)
    }

    pub fn history(&self) -> impl Iterator<Item = &SettlementTransition> {
        self.history.iter()
    }
}

/// A later economic consequence of a settlement is a separate fact.  It does
/// not rewrite the settlement amount or remove an earlier state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SettlementEffectKind {
    ProvisionalCredit,
    Fee,
    Correction,
    Reversal,
    Refund,
    Chargeback,
}

/// A typed alias for code that calls these records adjustments.
pub type SettlementAdjustmentKind = SettlementEffectKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementEffectRecord {
    pub occurrence: OccurrenceId,
    pub settlement: SettlementId,
    pub kind: SettlementEffectKind,
    pub amount: Quantity,
    pub instrument: InstrumentId,
    pub at: Option<Date>,
    pub reason: Option<String>,
    /// A correction may explicitly point at an earlier effect.  It never
    /// replaces that record in place.
    pub corrects: Option<OccurrenceId>,
}

impl SettlementEffectRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        kind: SettlementEffectKind,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            settlement: settlement.into(),
            kind,
            amount,
            instrument: instrument.into(),
            at: None,
            reason: None,
            corrects: None,
        }
    }

    pub fn provisional_credit(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::ProvisionalCredit,
            amount,
            instrument,
        )
    }

    pub fn fee(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::Fee,
            amount,
            instrument,
        )
    }

    pub fn correction(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::Correction,
            amount,
            instrument,
        )
    }

    pub fn reversal(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::Reversal,
            amount,
            instrument,
        )
    }

    pub fn refund(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::Refund,
            amount,
            instrument,
        )
    }

    pub fn chargeback(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
    ) -> Self {
        Self::new(
            occurrence,
            settlement,
            SettlementEffectKind::Chargeback,
            amount,
            instrument,
        )
    }

    pub fn at(mut self, at: Date) -> Self {
        self.at = Some(at);
        self
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn corrects(mut self, occurrence: impl Into<OccurrenceId>) -> Self {
        self.corrects = Some(occurrence.into());
        self
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_identifier(self.occurrence.as_str(), "settlement effect")?;
        require_identifier(self.settlement.as_str(), "settlement")?;
        require_identifier(self.instrument.as_str(), "instrument")?;
        require_positive(&self.amount, "a settlement effect amount")?;
        if self.amount.unit.as_ref().map(Unit::as_str) != Some(self.instrument.as_str()) {
            return Err(OntologyError::UnitMismatch {
                left: self.instrument.to_string(),
                right: self
                    .amount
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<missing>".to_string()),
            });
        }
        if self.corrects.as_ref() == Some(&self.occurrence) {
            return Err(OntologyError::InvalidEvent {
                kind: "settlement effect",
                reason: "an effect cannot correct itself".to_string(),
            });
        }
        if self.kind == SettlementEffectKind::Correction {
            if self
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
            {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement effect",
                    reason: "a correction effect needs a reason".to_string(),
                });
            }
            if self.corrects.is_none() {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement effect",
                    reason: "a correction effect needs an earlier effect".to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Short alias for callers that do not need the event-record suffix.
pub type SettlementEffect = SettlementEffectRecord;

/// Validate append-only effects against their referenced settlements.  Effects
/// are additive facts: a provisional credit is not settlement, a fee is not a
/// reduction of the gross amount, and a reversal/refund/chargeback is not an
/// in-place amount edit.  The only aggregate bound is the exact original
/// settlement amount for effects that undo value.
pub fn validate_settlement_effects(
    settlements: &[Settlement],
    effects: &[SettlementEffectRecord],
) -> Result<(), OntologyError> {
    let mut settlements_by_id = BTreeMap::new();
    for settlement in settlements {
        settlement.validate()?;
        if settlements_by_id
            .insert(settlement.id.clone(), settlement)
            .is_some()
        {
            return Err(OntologyError::DuplicateSettlement(settlement.id.clone()));
        }
    }

    let mut occurrences = BTreeSet::new();
    let mut effects_by_occurrence = BTreeMap::new();
    for effect in effects {
        effect.validate()?;
        if !occurrences.insert(effect.occurrence.clone()) {
            return Err(OntologyError::DuplicateEvent(effect.occurrence.clone()));
        }
        effects_by_occurrence.insert(effect.occurrence.clone(), effect);
    }
    let mut undo_totals: BTreeMap<SettlementId, Quantity> = BTreeMap::new();
    for effect in effects {
        let settlement = settlements_by_id
            .get(&effect.settlement)
            .ok_or_else(|| OntologyError::UnknownSettlement(effect.settlement.clone()))?;

        if let Some(corrects) = &effect.corrects {
            let prior = effects_by_occurrence
                .get(corrects)
                .ok_or_else(|| OntologyError::MissingEvent(corrects.clone()))?;
            if prior.settlement != effect.settlement {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement effect",
                    reason: "a correction must target an effect on the same settlement".to_string(),
                });
            }
        }

        if let Some(kind) = settlement.kind {
            let allowed = match kind {
                SettlementKind::Ach => matches!(
                    effect.kind,
                    SettlementEffectKind::ProvisionalCredit
                        | SettlementEffectKind::Fee
                        | SettlementEffectKind::Correction
                        | SettlementEffectKind::Reversal
                ),
                SettlementKind::Card => true,
                SettlementKind::Check => matches!(
                    effect.kind,
                    SettlementEffectKind::ProvisionalCredit
                        | SettlementEffectKind::Fee
                        | SettlementEffectKind::Correction
                ),
            };
            if !allowed {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement effect",
                    reason: format!(
                        "{:?} is not a legal {:?} settlement effect",
                        effect.kind, kind
                    ),
                });
            }
        }

        let undoing = matches!(
            effect.kind,
            SettlementEffectKind::Reversal
                | SettlementEffectKind::Refund
                | SettlementEffectKind::Chargeback
        );
        if undoing {
            let required_state = match effect.kind {
                SettlementEffectKind::Reversal => SettlementState::Reversed,
                SettlementEffectKind::Refund => SettlementState::Refunded,
                SettlementEffectKind::Chargeback => SettlementState::ChargedBack,
                _ => unreachable!("undoing effect kinds are matched above"),
            };
            if settlement.latest_state() != Some(&required_state)
                || !settlement
                    .history
                    .iter()
                    .any(|transition| transition.state == SettlementState::Settled)
            {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement effect",
                    reason: format!(
                        "{:?} requires a previously settled instrument whose current state is {:?}",
                        effect.kind, required_state
                    ),
                });
            }
            if effect.instrument != settlement.instrument
                || effect.amount.unit != settlement.amount.unit
            {
                return Err(OntologyError::UnitMismatch {
                    left: settlement.instrument.to_string(),
                    right: effect.instrument.to_string(),
                });
            }
            let total = undo_totals
                .entry(settlement.id.clone())
                .or_insert_with(Quantity::zero);
            *total = total.checked_add(&effect.amount)?;
            if total.number > settlement.amount.number {
                return Err(OntologyError::SettlementOverallocated {
                    settlement: settlement.id.clone(),
                    amount: Box::new(settlement.amount.clone()),
                    allocated: Box::new(total.clone()),
                });
            }
        }
    }
    Ok(())
}

impl Settlement {
    pub fn validate_effects(
        &self,
        effects: &[SettlementEffectRecord],
    ) -> Result<(), OntologyError> {
        validate_settlement_effects(std::slice::from_ref(self), effects)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllocationState {
    Proposed,
    Applied,
    Reversed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SatisfactionAllocation {
    pub id: AllocationId,
    pub obligation: ObligationId,
    pub settlement: SettlementId,
    pub quantity: Quantity,
    pub state: AllocationState,
}

impl SatisfactionAllocation {
    pub fn new(
        id: impl Into<AllocationId>,
        obligation: impl Into<ObligationId>,
        settlement: impl Into<SettlementId>,
        quantity: Quantity,
    ) -> Result<Self, OntologyError> {
        require_positive(&quantity, "an allocation must have a positive quantity")?;
        Ok(Self {
            id: id.into(),
            obligation: obligation.into(),
            settlement: settlement.into(),
            quantity,
            state: AllocationState::Proposed,
        })
    }

    pub fn applied(mut self) -> Self {
        self.state = AllocationState::Applied;
        self
    }

    pub fn reversed(mut self) -> Self {
        self.state = AllocationState::Reversed;
        self
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_identifier(self.id.as_str(), "allocation")?;
        require_identifier(self.obligation.as_str(), "obligation")?;
        require_identifier(self.settlement.as_str(), "settlement")?;
        require_positive(
            &self.quantity,
            "an allocation must have a positive quantity",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SatisfactionSummary {
    pub obligation_remaining: BTreeMap<ObligationId, Quantity>,
    pub settlement_unused: BTreeMap<SettlementId, Quantity>,
}

/// Validate the complete many-to-many satisfaction network in one indexed
/// pass. No obligation can over-consume performance and no settlement can be
/// spent twice across different obligations.
pub fn validate_satisfaction_network(
    obligations: &[Obligation],
    settlements: &[Settlement],
    allocations: &[SatisfactionAllocation],
) -> Result<SatisfactionSummary, OntologyError> {
    let mut obligation_by_id = BTreeMap::new();
    for obligation in obligations {
        obligation.validate()?;
        if obligation_by_id
            .insert(obligation.id.clone(), obligation)
            .is_some()
        {
            return Err(OntologyError::DuplicateObligation(obligation.id.clone()));
        }
    }
    let mut settlement_by_id = BTreeMap::new();
    for settlement in settlements {
        settlement.validate()?;
        if settlement_by_id
            .insert(settlement.id.clone(), settlement)
            .is_some()
        {
            return Err(OntologyError::DuplicateSettlement(settlement.id.clone()));
        }
    }

    let mut allocation_ids = BTreeSet::new();
    let mut by_obligation = BTreeMap::<ObligationId, Quantity>::new();
    let mut by_settlement = BTreeMap::<SettlementId, Quantity>::new();
    for allocation in allocations {
        allocation.validate()?;
        if !allocation_ids.insert(allocation.id.clone()) {
            return Err(OntologyError::DuplicateAllocation(allocation.id.clone()));
        }
        let obligation = obligation_by_id
            .get(&allocation.obligation)
            .ok_or_else(|| OntologyError::UnknownObligation(allocation.obligation.clone()))?;
        let settlement = settlement_by_id
            .get(&allocation.settlement)
            .ok_or_else(|| OntologyError::UnknownSettlement(allocation.settlement.clone()))?;
        let promised = obligation.promised_quantity()?;
        let instrument = obligation
            .performance
            .instrument()
            .ok_or(OntologyError::AllocationMismatch)?;
        if settlement.instrument != *instrument
            || settlement.from.entity != obligation.debtor
            || settlement.to.entity != obligation.creditor
        {
            return Err(OntologyError::AllocationMismatch);
        }
        if allocation.quantity.unit != promised.unit
            || allocation.quantity.unit != settlement.amount.unit
        {
            return Err(OntologyError::UnitMismatch {
                left: allocation
                    .quantity
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                right: promised
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
            });
        }
        if allocation.state == AllocationState::Applied && settlement.is_effective() {
            let obligation_total = by_obligation
                .entry(obligation.id.clone())
                .or_insert_with(Quantity::zero);
            *obligation_total = obligation_total.checked_add(&allocation.quantity)?;
            let settlement_total = by_settlement
                .entry(settlement.id.clone())
                .or_insert_with(Quantity::zero);
            *settlement_total = settlement_total.checked_add(&allocation.quantity)?;
        }
    }

    let mut obligation_remaining = BTreeMap::new();
    for obligation in obligations {
        let promised = obligation.promised_quantity()?.clone();
        let allocated = by_obligation
            .remove(&obligation.id)
            .unwrap_or_else(Quantity::zero);
        if allocated.number > promised.number {
            return Err(OntologyError::ObligationOverallocated {
                obligation: obligation.id.clone(),
                promised: Box::new(promised),
                allocated: Box::new(allocated),
            });
        }
        obligation_remaining.insert(obligation.id.clone(), promised.checked_sub(&allocated)?);
    }
    let mut settlement_unused = BTreeMap::new();
    for settlement in settlements {
        let allocated = by_settlement
            .remove(&settlement.id)
            .unwrap_or_else(Quantity::zero);
        if allocated.number > settlement.amount.number {
            return Err(OntologyError::SettlementOverallocated {
                settlement: settlement.id.clone(),
                amount: Box::new(settlement.amount.clone()),
                allocated: Box::new(allocated),
            });
        }
        settlement_unused.insert(
            settlement.id.clone(),
            settlement.amount.checked_sub(&allocated)?,
        );
    }
    Ok(SatisfactionSummary {
        obligation_remaining,
        settlement_unused,
    })
}

/// Check that all effective allocations for one obligation fit inside its
/// promised performance.  A returned settlement is deliberately not
/// effective, so a bounced payment restores the outstanding amount without
/// deleting the earlier allocation or settlement event.
pub fn validate_obligation_allocation(
    obligation: &Obligation,
    allocations: &[SatisfactionAllocation],
    settlements: &[Settlement],
) -> Result<(), OntologyError> {
    let promised = obligation.promised_quantity()?.clone();
    let Some(instrument) = obligation.performance.instrument() else {
        return Err(OntologyError::AllocationMismatch);
    };
    let mut allocation_ids = BTreeSet::new();
    let mut settlement_ids = BTreeSet::new();
    for settlement in settlements {
        if !settlement_ids.insert(settlement.id.clone()) {
            return Err(OntologyError::AllocationMismatch);
        }
    }

    // A settlement is one finite transfer of custody.  Aggregate all applied
    // allocations for settlements used by this obligation, not only the
    // current obligation, before checking capacity; otherwise two obligations
    // can each appear valid while silently spending the same settled payment.
    // Allocations attached to unrelated settlements are deliberately outside
    // this scoped helper.  The complete-network validator below is the place
    // that validates every obligation and settlement atomically.
    let relevant_settlements: BTreeSet<SettlementId> = allocations
        .iter()
        .filter(|allocation| allocation.obligation == obligation.id)
        .map(|allocation| allocation.settlement.clone())
        .collect();
    let mut allocated_by_settlement: BTreeMap<SettlementId, Quantity> = BTreeMap::new();
    for allocation in allocations {
        if !relevant_settlements.contains(&allocation.settlement) {
            continue;
        }
        if !allocation_ids.insert(allocation.id.clone()) {
            return Err(OntologyError::DuplicateAllocation(allocation.id.clone()));
        }
        require_positive(&allocation.quantity, "an allocation quantity")?;
        let settlement = settlements
            .iter()
            .find(|settlement| settlement.id == allocation.settlement)
            .ok_or(OntologyError::AllocationMismatch)?;
        if allocation.state == AllocationState::Applied && settlement.is_effective() {
            if allocation.quantity.unit != settlement.amount.unit {
                return Err(OntologyError::UnitMismatch {
                    left: allocation
                        .quantity
                        .unit
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                    right: settlement
                        .amount
                        .unit
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                });
            }
            let total = allocated_by_settlement
                .entry(settlement.id.clone())
                .or_insert_with(Quantity::zero);
            *total = total.checked_add(&allocation.quantity)?;
        }
    }
    for (settlement_id, allocated) in allocated_by_settlement {
        let settlement = settlements
            .iter()
            .find(|settlement| settlement.id == settlement_id)
            .ok_or(OntologyError::AllocationMismatch)?;
        if allocated.number > settlement.amount.number {
            return Err(OntologyError::SettlementOverallocated {
                settlement: settlement.id.clone(),
                amount: Box::new(settlement.amount.clone()),
                allocated: Box::new(allocated),
            });
        }
    }

    let mut allocated = Quantity::zero();
    for allocation in allocations.iter().filter(|allocation| {
        allocation.obligation == obligation.id && allocation.state == AllocationState::Applied
    }) {
        let Some(settlement) = settlements
            .iter()
            .find(|settlement| settlement.id == allocation.settlement)
        else {
            return Err(OntologyError::AllocationMismatch);
        };
        if settlement.instrument != *instrument {
            return Err(OntologyError::AllocationMismatch);
        }
        if settlement.from.entity != obligation.debtor
            || settlement.to.entity != obligation.creditor
        {
            return Err(OntologyError::AllocationMismatch);
        }
        if let Performance::Transfer {
            from: Some(expected_from),
            to: expected_to,
            ..
        } = &obligation.performance
            && (settlement.from.entity != *expected_from || settlement.to.entity != *expected_to)
        {
            return Err(OntologyError::AllocationMismatch);
        }
        if allocation.quantity.unit != promised.unit || settlement.amount.unit != promised.unit {
            return Err(OntologyError::UnitMismatch {
                left: allocation
                    .quantity
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                right: promised
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
            });
        }
        if allocation.quantity.number > settlement.amount.number {
            return Err(OntologyError::AllocationMismatch);
        }
        if settlement.is_effective() {
            allocated = allocated.checked_add(&allocation.quantity)?;
        }
    }
    if allocated.number > promised.number {
        return Err(OntologyError::ObligationOverallocated {
            obligation: obligation.id.clone(),
            promised: Box::new(promised),
            allocated: Box::new(allocated),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferLeg {
    pub endpoint: Endpoint,
    pub quantity: Quantity,
}

impl TransferLeg {
    pub fn new(endpoint: Endpoint, quantity: Quantity) -> Self {
        Self { endpoint, quantity }
    }
}

/// A transfer may split or merge positions.  It is represented as balanced
/// source and destination legs rather than a special balancing account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferRecord {
    pub occurrence: OccurrenceId,
    pub instrument: InstrumentId,
    pub sources: Vec<TransferLeg>,
    pub destinations: Vec<TransferLeg>,
}

impl TransferRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        instrument: impl Into<InstrumentId>,
        sources: Vec<TransferLeg>,
        destinations: Vec<TransferLeg>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            instrument: instrument.into(),
            sources,
            destinations,
        }
    }

    pub fn between(
        occurrence: impl Into<OccurrenceId>,
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self::new(
            occurrence,
            instrument,
            vec![TransferLeg::new(from, quantity.clone())],
            vec![TransferLeg::new(to, quantity)],
        )
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        let sources = sum_quantities(self.sources.iter().map(|leg| leg.quantity.clone()))?;
        let destinations =
            sum_quantities(self.destinations.iter().map(|leg| leg.quantity.clone()))?;
        for leg in self.sources.iter().chain(self.destinations.iter()) {
            require_positive(&leg.quantity, "a transfer leg")?;
        }
        if !quantities_equal(&sources, &destinations)? {
            return Err(OntologyError::TransferNotConserved {
                instrument: self.instrument.clone(),
                sources: Box::new(sources),
                destinations: Box::new(destinations),
            });
        }
        Ok(())
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        self.validate()?;
        validate_named_instrument(&self.instrument, instrument, "transfer")?;
        for leg in self.sources.iter().chain(self.destinations.iter()) {
            instrument.validate_quantity(&leg.quantity)?;
        }
        Ok(())
    }
}

/// Validate every transfer in a collection.  Issue and retire are not
/// accepted as hidden balancing legs: callers represent them with their own
/// explicit records and validate those records independently.
pub fn validate_transfer_conservation(transfers: &[TransferRecord]) -> Result<(), OntologyError> {
    for transfer in transfers {
        transfer.validate()?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueRecord {
    pub occurrence: OccurrenceId,
    pub to: Endpoint,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
}

impl IssueRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            to,
            instrument: instrument.into(),
            quantity,
        }
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.quantity, "an issue quantity")
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        self.validate()?;
        validate_named_instrument(&self.instrument, instrument, "issue")?;
        instrument.validate_quantity(&self.quantity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetireRecord {
    pub occurrence: OccurrenceId,
    pub from: Endpoint,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
}

impl RetireRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        from: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            from,
            instrument: instrument.into(),
            quantity,
        }
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.quantity, "a retire quantity")
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        self.validate()?;
        validate_named_instrument(&self.instrument, instrument, "retire")?;
        instrument.validate_quantity(&self.quantity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeLeg {
    pub from: Endpoint,
    pub to: Endpoint,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    /// A side is optional for backwards-compatible graph construction.  When
    /// present, it makes the exchange's give/receive boundary explicit and
    /// enables per-instrument net-conservation checks.
    pub side: ExchangeSide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExchangeSide {
    Unspecified,
    Give,
    Receive,
}

impl ExchangeLeg {
    pub fn new(
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            from,
            to,
            instrument: instrument.into(),
            quantity,
            side: ExchangeSide::Unspecified,
        }
    }

    pub fn give(
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            side: ExchangeSide::Give,
            ..Self::new(from, to, instrument, quantity)
        }
    }

    pub fn receive(
        from: Endpoint,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            side: ExchangeSide::Receive,
            ..Self::new(from, to, instrument, quantity)
        }
    }

    pub fn as_give(mut self) -> Self {
        self.side = ExchangeSide::Give;
        self
    }

    pub fn as_receive(mut self) -> Self {
        self.side = ExchangeSide::Receive;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeRecord {
    pub occurrence: OccurrenceId,
    pub legs: Vec<ExchangeLeg>,
}

impl ExchangeRecord {
    pub fn new(occurrence: impl Into<OccurrenceId>, legs: Vec<ExchangeLeg>) -> Self {
        Self {
            occurrence: occurrence.into(),
            legs,
        }
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        validate_exchange_legs(self)
    }

    pub fn validate_with_instruments(
        &self,
        instruments: &[Instrument],
    ) -> Result<(), OntologyError> {
        self.validate()?;
        for leg in &self.legs {
            let instrument = instruments
                .iter()
                .find(|candidate| candidate.id == leg.instrument)
                .ok_or_else(|| OntologyError::InvalidEvent {
                    kind: "exchange",
                    reason: format!("unknown instrument {}", leg.instrument),
                })?;
            instrument.validate_quantity(&leg.quantity)?;
        }
        Ok(())
    }
}

/// Exchange legs conserve each instrument across the complete exchange, even
/// when the two sides barter different instruments.
pub fn validate_exchange_legs(exchange: &ExchangeRecord) -> Result<(), OntologyError> {
    if exchange.legs.len() < 2 {
        return Err(OntologyError::InvalidEvent {
            kind: "exchange",
            reason: "an exchange needs at least two legs".to_string(),
        });
    }
    let mut endpoint_activity: BTreeMap<Endpoint, (usize, usize)> = BTreeMap::new();
    let mut give: BTreeMap<InstrumentId, Quantity> = BTreeMap::new();
    let mut receive: BTreeMap<InstrumentId, Quantity> = BTreeMap::new();
    for leg in &exchange.legs {
        require_positive(&leg.quantity, "an exchange leg")?;
        if leg.from == leg.to {
            return Err(OntologyError::InvalidEvent {
                kind: "exchange",
                reason: "an exchange leg needs distinct endpoints".to_string(),
            });
        }
        endpoint_activity.entry(leg.from.clone()).or_default().0 += 1;
        endpoint_activity.entry(leg.to.clone()).or_default().1 += 1;
        match leg.side {
            ExchangeSide::Give => {
                add_side_quantity(&mut give, &leg.instrument, &leg.quantity)?;
            }
            ExchangeSide::Receive => {
                add_side_quantity(&mut receive, &leg.instrument, &leg.quantity)?;
            }
            ExchangeSide::Unspecified => {}
        }
    }
    // A closed exchange requires each participating endpoint to give and
    // receive something. A one-way leg is a transfer, not an exchange. This
    // admits ordinary barter and multi-party exchange cycles.
    for (endpoint, (outgoing, incoming)) in endpoint_activity {
        if outgoing == 0 || incoming == 0 {
            return Err(OntologyError::InvalidEvent {
                kind: "exchange",
                reason: format!("endpoint {} is not closed", endpoint.entity),
            });
        }
    }

    // An instrument appearing on both explicit sides is a round-trip, not a
    // conversion.  Its give and receive totals must agree exactly.  Different
    // instruments (for example USD given for ABC) are intentionally allowed
    // to occur on only one side: barter conserves each instrument within its
    // own transfer leg, while the exchange couples the legs economically.
    for (instrument, outgoing) in give {
        if let Some(incoming) = receive.get(&instrument)
            && !quantities_equal(&outgoing, incoming)?
        {
            return Err(OntologyError::ExchangeNotConserved {
                instrument,
                outgoing: Box::new(outgoing),
                incoming: Box::new(incoming.clone()),
            });
        }
    }
    Ok(())
}

fn add_side_quantity(
    totals: &mut BTreeMap<InstrumentId, Quantity>,
    instrument: &InstrumentId,
    quantity: &Quantity,
) -> Result<(), OntologyError> {
    if let Some(total) = totals.get_mut(instrument) {
        *total = total.checked_add(quantity)?;
    } else {
        totals.insert(instrument.clone(), quantity.clone());
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcquireRecord {
    pub occurrence: OccurrenceId,
    pub to: Endpoint,
    pub source: Option<Endpoint>,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    pub lot: Option<LotId>,
    pub consideration: Option<Quantity>,
}

impl AcquireRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        to: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            to,
            source: None,
            instrument: instrument.into(),
            quantity,
            lot: None,
            consideration: None,
        }
    }

    pub fn from(mut self, source: Endpoint) -> Self {
        self.source = Some(source);
        self
    }

    pub fn into_lot(mut self, lot: impl Into<LotId>) -> Self {
        self.lot = Some(lot.into());
        self
    }

    pub fn for_consideration(mut self, consideration: Quantity) -> Self {
        self.consideration = Some(consideration);
        self
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.quantity, "an acquisition quantity")
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        self.validate()?;
        validate_named_instrument(&self.instrument, instrument, "acquire")?;
        instrument.validate_quantity(&self.quantity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisposeRecord {
    pub occurrence: OccurrenceId,
    pub from: Endpoint,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    pub lot: Option<LotId>,
    pub proceeds: Option<Quantity>,
}

impl DisposeRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        from: Endpoint,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            from,
            instrument: instrument.into(),
            quantity,
            lot: None,
            proceeds: None,
        }
    }

    pub fn from_lot(mut self, lot: impl Into<LotId>) -> Self {
        self.lot = Some(lot.into());
        self
    }

    pub fn for_proceeds(mut self, proceeds: Quantity) -> Self {
        self.proceeds = Some(proceeds);
        self
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.quantity, "a disposal quantity")
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        self.validate()?;
        validate_named_instrument(&self.instrument, instrument, "dispose")?;
        instrument.validate_quantity(&self.quantity)
    }
}

/// Apply all explicitly lot-bound disposals to copies of the supplied lots.
/// The input lots are not mutated, which makes this suitable for checking an
/// accepted graph before recognition chooses a book-specific result.
pub fn validate_remaining_lot_quantities(
    lots: &[Lot],
    disposals: &[DisposeRecord],
) -> Result<(), OntologyError> {
    for lot in lots {
        require_nonnegative(
            &lot.remaining,
            "a lot cannot have negative remaining quantity",
        )?;
        if lot.remaining.unit != lot.acquired.unit || lot.remaining.number > lot.acquired.number {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} has invalid remaining quantity", lot.id),
            });
        }
    }
    let mut by_id: BTreeMap<LotId, Lot> = lots
        .iter()
        .cloned()
        .map(|lot| (lot.id.clone(), lot))
        .collect();
    for disposal in disposals {
        disposal.validate()?;
        let lot_id = disposal.lot.clone().ok_or(OntologyError::MissingLot)?;
        let lot = by_id
            .get_mut(&lot_id)
            .ok_or_else(|| OntologyError::InvalidEvent {
                kind: "disposal",
                reason: format!("unknown lot {lot_id}"),
            })?;
        if lot.instrument != disposal.instrument {
            return Err(OntologyError::LotInstrumentMismatch {
                lot: lot.id.clone(),
                expected: lot.instrument.clone(),
                actual: disposal.instrument.clone(),
            });
        }
        *lot = lot.consumed(&disposal.quantity)?;
    }
    Ok(())
}

/// Validate the inventory represented by a graph's lot, acquire, and dispose
/// records. A lot's `remaining` value is a declared current state, so it must
/// equal acquisition less every linked disposal; it is not treated as a
/// pre-disposal balance. Missing lot links are explicit blockers.
pub fn validate_lot_inventory(
    lots: &[Lot],
    acquisitions: &[AcquireRecord],
    disposals: &[DisposeRecord],
) -> Result<(), OntologyError> {
    let mut by_id = BTreeMap::new();
    for lot in lots {
        lot.validate()?;
        if by_id.insert(lot.id.clone(), lot).is_some() {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("duplicate lot {}", lot.id),
            });
        }
    }

    let mut acquired_by_lot: BTreeMap<LotId, Quantity> = BTreeMap::new();
    let mut acquisition_count: BTreeMap<LotId, usize> = BTreeMap::new();
    let mut disposed_by_lot: BTreeMap<LotId, Quantity> = BTreeMap::new();
    for acquisition in acquisitions {
        acquisition.validate()?;
        let lot_id = acquisition.lot.clone().ok_or(OntologyError::MissingLot)?;
        let lot = by_id
            .get(&lot_id)
            .ok_or_else(|| OntologyError::InvalidEvent {
                kind: "acquire",
                reason: format!(
                    "acquire {} has no declared lot {lot_id}",
                    acquisition.occurrence
                ),
            })?;
        if lot.instrument != acquisition.instrument {
            return Err(OntologyError::LotInstrumentMismatch {
                lot: lot.id.clone(),
                expected: lot.instrument.clone(),
                actual: acquisition.instrument.clone(),
            });
        }
        if lot.acquisition != acquisition.occurrence {
            return Err(OntologyError::InvalidEvent {
                kind: "acquire",
                reason: format!(
                    "lot {} is acquired by {}, not {}",
                    lot.id, lot.acquisition, acquisition.occurrence
                ),
            });
        }
        *acquisition_count.entry(lot_id.clone()).or_default() += 1;
        add_lot_quantity(&mut acquired_by_lot, &lot_id, &acquisition.quantity)?;
    }
    for disposal in disposals {
        disposal.validate()?;
        let lot_id = disposal.lot.clone().ok_or(OntologyError::MissingLot)?;
        let lot = by_id
            .get(&lot_id)
            .ok_or_else(|| OntologyError::InvalidEvent {
                kind: "dispose",
                reason: format!(
                    "dispose {} has no declared lot {lot_id}",
                    disposal.occurrence
                ),
            })?;
        if lot.instrument != disposal.instrument {
            return Err(OntologyError::LotInstrumentMismatch {
                lot: lot.id.clone(),
                expected: lot.instrument.clone(),
                actual: disposal.instrument.clone(),
            });
        }
        add_lot_quantity(&mut disposed_by_lot, &lot_id, &disposal.quantity)?;
    }

    for lot in lots {
        if acquisition_count.get(&lot.id).copied().unwrap_or_default() != 1 {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} lacks exactly one acquire linkage", lot.id),
            });
        }
        let acquired = acquired_by_lot
            .get(&lot.id)
            .ok_or_else(|| OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} has no acquire quantity", lot.id),
            })?;
        if acquired != &lot.acquired {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} acquired quantity disagrees with its record", lot.id),
            });
        }
        let disposed = match disposed_by_lot.get(&lot.id) {
            Some(quantity) => quantity.clone(),
            None => Quantity::new(ExactNumber::integer(0), lot.acquired.unit.clone())?,
        };
        if disposed.unit != lot.acquired.unit {
            return Err(OntologyError::UnitMismatch {
                left: lot
                    .acquired
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
                right: disposed
                    .unit
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<polymorphic zero>".to_string()),
            });
        }
        if disposed.number > lot.acquired.number {
            return Err(OntologyError::LotOverconsumed {
                lot: lot.id.clone(),
                remaining: Box::new(lot.acquired.clone()),
                requested: Box::new(disposed),
            });
        }
        let expected_remaining = lot.acquired.checked_sub(&disposed)?;
        if expected_remaining != lot.remaining {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("lot {} remaining quantity is not inventory-derived", lot.id),
            });
        }
    }
    Ok(())
}

fn add_lot_quantity(
    totals: &mut BTreeMap<LotId, Quantity>,
    lot: &LotId,
    quantity: &Quantity,
) -> Result<(), OntologyError> {
    if let Some(total) = totals.get_mut(lot) {
        *total = total.checked_add(quantity)?;
    } else {
        totals.insert(lot.clone(), quantity.clone());
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementStateRecord {
    pub occurrence: OccurrenceId,
    pub settlement: SettlementId,
    pub state: SettlementState,
    pub at: Option<Date>,
    pub amount: Quantity,
    pub instrument: InstrumentId,
    pub from: Endpoint,
    pub to: Endpoint,
    pub obligation: Option<ObligationId>,
    pub reason: Option<String>,
    /// Optional payment-rail declaration. `None` preserves the original
    /// generic state-record API; `Some` applies the explicit rail machine.
    kind: Option<SettlementKind>,
}

impl SettlementStateRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        settlement: impl Into<SettlementId>,
        state: SettlementState,
        amount: Quantity,
        instrument: impl Into<InstrumentId>,
        from: Endpoint,
        to: Endpoint,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            settlement: settlement.into(),
            state,
            at: None,
            amount,
            instrument: instrument.into(),
            from,
            to,
            obligation: None,
            reason: None,
            kind: None,
        }
    }

    pub fn with_kind(mut self, kind: SettlementKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn kind(&self) -> Option<SettlementKind> {
        self.kind
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        require_positive(&self.amount, "a settlement state amount")
    }

    pub fn validate_with_instrument(&self, instrument: &Instrument) -> Result<(), OntologyError> {
        validate_named_instrument(&self.instrument, instrument, "settlement state")?;
        instrument.validate_quantity(&self.amount)
    }
}

/// Validate the ordered state history for one or more settlement instruments.
/// The caller's record order is authoritative; this function never sorts
/// records into a seemingly valid history. Dates, when present, must move
/// forward in that same sequence.
pub fn validate_settlement_state_records(
    records: &[SettlementStateRecord],
) -> Result<(), OntologyError> {
    let mut history: BTreeMap<SettlementId, Vec<&SettlementStateRecord>> = BTreeMap::new();
    for record in records {
        history
            .entry(record.settlement.clone())
            .or_default()
            .push(record);
    }
    for (settlement, records) in history {
        let first = records[0];
        let declared_kind = first.kind;
        let mut previous_at = None;
        for record in &records {
            if let (Some(previous), Some(current)) = (previous_at, record.at)
                && current < previous
            {
                return Err(OntologyError::NonChronologicalSettlement {
                    settlement: settlement.clone(),
                    previous,
                    current,
                });
            }
            previous_at = record.at.or(previous_at);
            if record.amount != first.amount
                || record.instrument != first.instrument
                || record.from != first.from
                || record.to != first.to
                || record.kind != declared_kind
            {
                return Err(OntologyError::InvalidEvent {
                    kind: "settlement state",
                    reason: format!("inconsistent facts for settlement {settlement}"),
                });
            }
        }
        let states = records
            .iter()
            .map(|record| record.state.clone())
            .collect::<Vec<_>>();
        match declared_kind {
            Some(kind) => validate_settlement_states_for(kind, &states)?,
            None => validate_settlement_states(&states)?,
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrectionRecord {
    pub occurrence: OccurrenceId,
    pub corrects: OccurrenceId,
    pub replacement: Option<OccurrenceId>,
    pub reason: String,
}

impl CorrectionRecord {
    pub fn new(
        occurrence: impl Into<OccurrenceId>,
        corrects: impl Into<OccurrenceId>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            occurrence: occurrence.into(),
            corrects: corrects.into(),
            replacement: None,
            reason: reason.into(),
        }
    }

    pub fn replaces(mut self, replacement: impl Into<OccurrenceId>) -> Self {
        self.replacement = Some(replacement.into());
        self
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        if self.occurrence == self.corrects {
            return Err(OntologyError::InvalidEvent {
                kind: "correction",
                reason: "a correction cannot correct itself".to_string(),
            });
        }
        if self.reason.trim().is_empty() {
            return Err(OntologyError::InvalidEvent {
                kind: "correction",
                reason: "a correction needs a reason".to_string(),
            });
        }
        if self.replacement.as_ref() == Some(&self.occurrence)
            || self.replacement.as_ref() == Some(&self.corrects)
        {
            return Err(OntologyError::InvalidEvent {
                kind: "correction",
                reason: "a correction replacement must be a distinct event".to_string(),
            });
        }
        Ok(())
    }
}

/// Extensible event record interface.  New economic event families can be
/// added without changing [`EventGraph`] or making recognition match on a
/// closed mega-enum.
pub trait EventRecord: fmt::Debug + Send + Sync {
    fn occurrence(&self) -> &OccurrenceId;
    fn kind(&self) -> &'static str;
    fn validate(&self) -> Result<(), OntologyError>;
    fn as_any(&self) -> &dyn Any;
}

macro_rules! event_record_impl {
    ($type:ty, $kind:literal) => {
        impl EventRecord for $type {
            fn occurrence(&self) -> &OccurrenceId {
                &self.occurrence
            }

            fn kind(&self) -> &'static str {
                $kind
            }

            fn validate(&self) -> Result<(), OntologyError> {
                <$type>::validate(self)
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

event_record_impl!(TransferRecord, "transfer");
event_record_impl!(IssueRecord, "issue");
event_record_impl!(RetireRecord, "retire");
event_record_impl!(ExchangeRecord, "exchange");
event_record_impl!(AcquireRecord, "acquire");
event_record_impl!(DisposeRecord, "dispose");
event_record_impl!(SettlementStateRecord, "settlement-state");
event_record_impl!(SettlementEffectRecord, "settlement-effect");
event_record_impl!(CorrectionRecord, "correction");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventRelation {
    Follows,
    Satisfies,
    Settles,
    Corrects,
    Derives,
    Reverses,
    Related,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventEdge {
    pub from: OccurrenceId,
    pub to: OccurrenceId,
    pub relation: EventRelation,
}

/// The graph stores accepted, typed economic records.  It has no book account
/// and therefore cannot silently become a recognition journal.
#[derive(Clone, Default)]
pub struct EventGraph {
    records: BTreeMap<OccurrenceId, Arc<dyn EventRecord>>,
    edges: Vec<EventEdge>,
    record_order: Vec<OccurrenceId>,
    lots: BTreeMap<LotId, Lot>,
}

impl fmt::Debug for EventGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventGraph")
            .field("records", &self.records.keys().collect::<Vec<_>>())
            .field("edges", &self.edges)
            .finish()
    }
}

impl EventGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<R: EventRecord + 'static>(&mut self, record: R) -> Result<(), OntologyError> {
        record.validate()?;
        let occurrence = record.occurrence().clone();
        if self.records.contains_key(&occurrence) {
            return Err(OntologyError::DuplicateEvent(occurrence));
        }
        self.records.insert(occurrence.clone(), Arc::new(record));
        self.record_order.push(occurrence);
        Ok(())
    }

    pub fn insert_lot(&mut self, lot: Lot) -> Result<(), OntologyError> {
        lot.validate()?;
        if self.lots.contains_key(&lot.id) {
            return Err(OntologyError::InvalidEvent {
                kind: "lot",
                reason: format!("duplicate lot {}", lot.id),
            });
        }
        self.lots.insert(lot.id.clone(), lot);
        Ok(())
    }

    pub fn lot(&self, id: &LotId) -> Option<&Lot> {
        self.lots.get(id)
    }

    pub fn lots(&self) -> impl Iterator<Item = &Lot> {
        self.lots.values()
    }

    pub fn add_edge(
        &mut self,
        from: impl Into<OccurrenceId>,
        to: impl Into<OccurrenceId>,
        relation: EventRelation,
    ) -> Result<(), OntologyError> {
        let from = from.into();
        let to = to.into();
        if from == to || !self.records.contains_key(&from) || !self.records.contains_key(&to) {
            return Err(if from == to {
                OntologyError::InvalidEdge
            } else if !self.records.contains_key(&from) {
                OntologyError::MissingEvent(from)
            } else {
                OntologyError::MissingEvent(to)
            });
        }
        self.edges.push(EventEdge { from, to, relation });
        Ok(())
    }

    pub fn get(&self, occurrence: &OccurrenceId) -> Option<&dyn EventRecord> {
        self.records.get(occurrence).map(AsRef::as_ref)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn EventRecord> {
        self.records.values().map(AsRef::as_ref)
    }

    pub fn edges(&self) -> impl Iterator<Item = &EventEdge> {
        self.edges.iter()
    }

    pub fn records_of<T: EventRecord + 'static>(&self) -> impl Iterator<Item = &T> {
        self.record_order.iter().filter_map(|occurrence| {
            self.records
                .get(occurrence)
                .and_then(|record| record.as_any().downcast_ref::<T>())
        })
    }

    pub fn validate(&self) -> Result<(), OntologyError> {
        for record in self.records.values() {
            record.validate()?;
        }
        for edge in &self.edges {
            if !self.records.contains_key(&edge.from) {
                return Err(OntologyError::MissingEvent(edge.from.clone()));
            }
            if !self.records.contains_key(&edge.to) {
                return Err(OntologyError::MissingEvent(edge.to.clone()));
            }
        }
        for correction in self.records_of::<CorrectionRecord>() {
            if !self.records.contains_key(&correction.corrects) {
                return Err(OntologyError::MissingEvent(correction.corrects.clone()));
            }
            if let Some(replacement) = &correction.replacement
                && !self.records.contains_key(replacement)
            {
                return Err(OntologyError::MissingEvent(replacement.clone()));
            }
        }
        let transfers: Vec<_> = self.records_of::<TransferRecord>().cloned().collect();
        validate_transfer_conservation(&transfers)?;
        for exchange in self.records_of::<ExchangeRecord>() {
            exchange.validate()?;
        }
        let acquisitions: Vec<_> = self.records_of::<AcquireRecord>().cloned().collect();
        let disposals: Vec<_> = self.records_of::<DisposeRecord>().cloned().collect();
        let lots: Vec<_> = self.lots.values().cloned().collect();
        validate_lot_inventory(&lots, &acquisitions, &disposals)?;
        let settlement_states: Vec<_> = self
            .records_of::<SettlementStateRecord>()
            .cloned()
            .collect();
        validate_settlement_state_records(&settlement_states)?;
        // Effect records are validated independently here.  Their referenced
        // settlement may be supplied by another source graph, so resolving or
        // synthesizing a settlement in order to validate an effect would be a
        // hidden guess.  Callers with concrete Settlement values can use
        // `validate_settlement_effects` for cross-record amount bounds.
        for effect in self.records_of::<SettlementEffectRecord>() {
            effect.validate()?;
        }
        Ok(())
    }

    /// Validate the graph's instrument-bearing records against declarations.
    /// The plain [`EventGraph::validate`] remains useful for open-world input
    /// where declarations have not arrived yet.
    pub fn validate_with_instruments(
        &self,
        instruments: &[Instrument],
    ) -> Result<(), OntologyError> {
        self.validate()?;
        for instrument in instruments {
            instrument.validate()?;
        }
        let lookup = |id: &InstrumentId| {
            instruments
                .iter()
                .find(|instrument| instrument.id == *id)
                .ok_or_else(|| OntologyError::InvalidEvent {
                    kind: "event graph",
                    reason: format!("unknown instrument {id}"),
                })
        };
        for lot in self.lots.values() {
            lot.validate_with_instrument(lookup(&lot.instrument)?)?;
        }
        for transfer in self.records_of::<TransferRecord>() {
            transfer.validate_with_instrument(lookup(&transfer.instrument)?)?;
        }
        for issue in self.records_of::<IssueRecord>() {
            issue.validate_with_instrument(lookup(&issue.instrument)?)?;
        }
        for retire in self.records_of::<RetireRecord>() {
            retire.validate_with_instrument(lookup(&retire.instrument)?)?;
        }
        for exchange in self.records_of::<ExchangeRecord>() {
            exchange.validate_with_instruments(instruments)?;
        }
        for acquire in self.records_of::<AcquireRecord>() {
            acquire.validate_with_instrument(lookup(&acquire.instrument)?)?;
        }
        for dispose in self.records_of::<DisposeRecord>() {
            dispose.validate_with_instrument(lookup(&dispose.instrument)?)?;
        }
        for state in self.records_of::<SettlementStateRecord>() {
            state.validate_with_instrument(lookup(&state.instrument)?)?;
        }
        for effect in self.records_of::<SettlementEffectRecord>() {
            let instrument = lookup(&effect.instrument)?;
            instrument.validate_quantity(&effect.amount)?;
        }
        Ok(())
    }
}

/// Accepted facts and recognized facts are different wrappers on purpose.  A
/// book projection can only be made from an accepted value and cannot feed a
/// recognized value back into the accepted graph through an implicit
/// conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedFact<T> {
    pub value: T,
    pub provenance: Option<ContentHash>,
}

impl<T> AcceptedFact<T> {
    pub fn new(value: T, provenance: Option<ContentHash>) -> Self {
        Self { value, provenance }
    }

    pub fn recognize(self, book: impl Into<BookId>) -> RecognizedFact<T> {
        RecognizedFact {
            book: book.into(),
            value: self.value,
            provenance: self.provenance,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizedFact<T> {
    pub book: BookId,
    pub value: T,
    pub provenance: Option<ContentHash>,
}

impl<T> RecognizedFact<T> {
    pub fn book(&self) -> &BookId {
        &self.book
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantity(number: &str, unit: &str) -> Quantity {
        Quantity::with_unit(ExactNumber::parse(number).unwrap(), unit).unwrap()
    }

    fn endpoint(entity: &str) -> Endpoint {
        Endpoint::entity(entity)
    }

    #[test]
    fn joint_roles_keep_each_holder_explicit() {
        let roles = RoleAssignments::joint(
            "property",
            Role::BeneficialOwner,
            vec![EntityId::new("alice"), EntityId::new("bob")],
        )
        .unwrap();
        roles.validate().unwrap();
        assert_eq!(
            roles
                .holders(&EntityId::new("property"), &Role::BeneficialOwner)
                .count(),
            2
        );
        assert_eq!(roles.assignments[0].share, ExactNumber::rational(1, 2).ok());
    }

    #[test]
    fn historical_role_shares_are_checked_per_active_interval() {
        let first_from = Date::new(2020, 1, 1).unwrap();
        let first_until = Date::new(2020, 12, 31).unwrap();
        let second_from = Date::new(2021, 1, 1).unwrap();
        let second_until = Date::new(2021, 12, 31).unwrap();
        let historical = RoleAssignments::new()
            .with(
                RoleAssignment::new("property", Role::BeneficialOwner, "alice")
                    .with_share(ExactNumber::integer(1))
                    .during(Some(first_from), Some(first_until)),
            )
            .with(
                RoleAssignment::new("property", Role::BeneficialOwner, "bob")
                    .with_share(ExactNumber::integer(1))
                    .during(Some(second_from), Some(second_until)),
            );
        historical.validate().unwrap();

        let overlapping = RoleAssignments::new()
            .with(
                RoleAssignment::new("property", Role::BeneficialOwner, "alice")
                    .with_share(ExactNumber::integer(1))
                    .during(Some(first_from), Some(first_until)),
            )
            .with(
                RoleAssignment::new("property", Role::BeneficialOwner, "bob")
                    .with_share(ExactNumber::integer(1))
                    .during(Some(first_until), Some(second_until)),
            );
        assert_eq!(
            overlapping.validate(),
            Err(OntologyError::RoleSharesExceedOne)
        );
    }

    #[test]
    fn restricted_position_is_not_freely_available() {
        let hold = Encumbrance::for_quantity("hold", EncumbranceKind::Hold, quantity("100", "USD"))
            .unwrap();
        let position = Position::new("p", "alice", "USD", quantity("100", "USD"))
            .unwrap()
            .with_encumbrance("hold");
        let mut encumbrances = BTreeMap::new();
        encumbrances.insert(hold.id.clone(), hold);
        assert!(position.is_restricted(&encumbrances).unwrap());
        assert!(
            position
                .available_quantity(&encumbrances)
                .unwrap()
                .is_zero()
        );
        let released =
            Encumbrance::for_quantity("released", EncumbranceKind::Hold, quantity("100", "USD"))
                .unwrap()
                .release();
        let released = BTreeMap::from([(released.id.clone(), released)]);
        let released_position =
            Position::new("released-position", "alice", "USD", quantity("100", "USD"))
                .unwrap()
                .with_encumbrance("released");
        assert!(!released_position.is_restricted(&released).unwrap());
        assert_eq!(
            released_position.available_quantity(&released).unwrap(),
            quantity("100", "USD")
        );
        assert!(
            Encumbrance::for_quantity("negative", EncumbranceKind::Hold, quantity("-1", "USD"),)
                .is_err()
        );
    }

    #[test]
    fn declared_instrument_units_are_checked_at_domain_boundaries() {
        let usd =
            Instrument::new("USD", InstrumentKind::Currency).denominated(Unit::new("USD").unwrap());
        let position = Position::new("position", "alice", "USD", quantity("5", "USD")).unwrap();
        position.validate_with_instrument(&usd).unwrap();
        let wrong_position = Position::new("wrong", "alice", "USD", quantity("5", "EUR")).unwrap();
        assert!(wrong_position.validate_with_instrument(&usd).is_err());
        let transfer = TransferRecord::between(
            "transfer",
            endpoint("alice"),
            endpoint("bob"),
            "USD",
            quantity("1", "USD"),
        );
        transfer.validate_with_instrument(&usd).unwrap();
    }

    #[test]
    fn event_graph_enforces_declared_instrument_quantum() {
        let usd = Instrument::new("USD", InstrumentKind::Currency)
            .denominated(Unit::new("USD").unwrap())
            .with_quantum(quantity("0.05", "USD"));
        let mut invalid = EventGraph::new();
        invalid
            .insert(TransferRecord::between(
                "off-quantum",
                endpoint("alice"),
                endpoint("bob"),
                "USD",
                quantity("1.01", "USD"),
            ))
            .unwrap();
        assert!(matches!(
            invalid.validate_with_instruments(std::slice::from_ref(&usd)),
            Err(OntologyError::InvalidQuantity { .. })
        ));

        let mut valid = EventGraph::new();
        valid
            .insert(TransferRecord::between(
                "on-quantum",
                endpoint("alice"),
                endpoint("bob"),
                "USD",
                quantity("1.00", "USD"),
            ))
            .unwrap();
        valid
            .validate_with_instruments(std::slice::from_ref(&usd))
            .unwrap();

        let abc = Instrument::new("ABC", InstrumentKind::Equity)
            .denominated(Unit::new("ABC").unwrap())
            .with_quantum(quantity("0.5", "ABC"));
        let mut off_quantum_lot = EventGraph::new();
        off_quantum_lot
            .insert_lot(Lot::new("lot-1", "ABC", quantity("1.01", "ABC"), "acquire-1").unwrap())
            .unwrap();
        off_quantum_lot
            .insert(
                AcquireRecord::new(
                    "acquire-1",
                    endpoint("alice"),
                    "ABC",
                    quantity("1.01", "ABC"),
                )
                .into_lot("lot-1"),
            )
            .unwrap();
        assert!(matches!(
            off_quantum_lot.validate_with_instruments(std::slice::from_ref(&abc)),
            Err(OntologyError::InvalidQuantity { .. })
        ));
    }

    #[test]
    fn settlement_history_requires_representation_after_return() {
        let mut settlement = Settlement::new(
            "check",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("10", "USD"),
        )
        .unwrap();
        assert!(matches!(
            settlement.transition(SettlementState::Settled, None, None),
            Err(OntologyError::InvalidSettlementTransition { .. })
        ));
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        settlement.returned(None, "bounced").unwrap();
        assert!(
            settlement
                .transition(SettlementState::Settled, None, None)
                .is_err()
        );
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        assert!(settlement.is_effective());
    }

    #[test]
    fn typed_payment_rails_keep_their_distinct_lifecycles() {
        let amount = quantity("10", "USD");
        let mut check = Settlement::new_with_kind(
            "typed-check",
            SettlementKind::Check,
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            amount.clone(),
        )
        .unwrap();
        check
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        check
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        check.returned(None, "insufficient funds").unwrap();
        // A returned check can be presented again, but the old attempt is
        // never erased.
        check
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        assert_eq!(check.history().count(), 5);
        assert!(
            check
                .transition(SettlementState::ChargedBack, None, None)
                .is_err()
        );

        let mut ach = Settlement::new_with_kind(
            "typed-ach",
            SettlementKind::Ach,
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            amount.clone(),
        )
        .unwrap();
        ach.transition(SettlementState::Presented, None, None)
            .unwrap();
        ach.transition(SettlementState::Pending, None, None)
            .unwrap();
        ach.transition(SettlementState::Returned, None, None)
            .unwrap();
        assert!(
            ach.transition(SettlementState::Presented, None, None)
                .is_err()
        );

        let mut card = Settlement::new_with_kind(
            "typed-card",
            SettlementKind::Card,
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            amount,
        )
        .unwrap();
        card.transition(SettlementState::Authorized, None, None)
            .unwrap();
        card.transition(SettlementState::Presented, None, None)
            .unwrap();
        card.transition(SettlementState::Settled, None, None)
            .unwrap();
        card.transition(SettlementState::Disputed, None, None)
            .unwrap();
        card.transition(SettlementState::ChargedBack, None, None)
            .unwrap();
        card.transition(SettlementState::Represented, None, None)
            .unwrap();
        card.transition(SettlementState::Settled, None, None)
            .unwrap();
    }

    #[test]
    fn settlement_effects_are_explicit_append_only_and_bounded() {
        let mut card = Settlement::new_with_kind(
            "effect-card",
            SettlementKind::Card,
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        card.transition(SettlementState::Presented, None, None)
            .unwrap();
        card.transition(SettlementState::Settled, None, None)
            .unwrap();

        let original = card.clone();
        let charged_back = card
            .appended(
                SettlementState::ChargedBack,
                None,
                Some("issuer dispute".into()),
            )
            .unwrap();
        assert_eq!(original.latest_state(), Some(&SettlementState::Settled));
        assert_eq!(
            charged_back.latest_state(),
            Some(&SettlementState::ChargedBack)
        );

        let effects = vec![
            SettlementEffectRecord::provisional_credit(
                "provisional",
                "effect-card",
                quantity("100", "USD"),
                "USD",
            ),
            SettlementEffectRecord::fee("fee", "effect-card", quantity("2", "USD"), "USD"),
            SettlementEffectRecord::chargeback(
                "chargeback-partial",
                "effect-card",
                quantity("40", "USD"),
                "USD",
            ),
            SettlementEffectRecord::chargeback(
                "chargeback-rest",
                "effect-card",
                quantity("60", "USD"),
                "USD",
            ),
        ];
        validate_settlement_effects(std::slice::from_ref(&charged_back), &effects).unwrap();

        let too_much = SettlementEffectRecord::chargeback(
            "refund-too-much",
            "effect-card",
            quantity("101", "USD"),
            "USD",
        );
        assert!(matches!(
            validate_settlement_effects(std::slice::from_ref(&charged_back), &[too_much]),
            Err(OntologyError::SettlementOverallocated { .. })
        ));

        let invalid_check = Settlement::new_with_kind(
            "effect-check",
            SettlementKind::Check,
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("10", "USD"),
        )
        .unwrap();
        let chargeback = SettlementEffectRecord::chargeback(
            "check-chargeback",
            "effect-check",
            quantity("1", "USD"),
            "USD",
        );
        assert!(matches!(
            validate_settlement_effects(&[invalid_check], &[chargeback]),
            Err(OntologyError::InvalidEvent {
                kind: "settlement effect",
                ..
            })
        ));
    }

    #[test]
    fn correction_effects_need_an_explicit_prior_effect() {
        let correction = SettlementEffectRecord::correction(
            "correction",
            "payment",
            quantity("1", "USD"),
            "USD",
        );
        assert!(matches!(
            correction.validate(),
            Err(OntologyError::InvalidEvent {
                kind: "settlement effect",
                ..
            })
        ));
        let correction = correction.corrects("fee").reason("bank fee correction");
        correction.validate().unwrap();
    }

    #[test]
    fn event_graph_requires_correction_targets_and_legal_settlement_history() {
        let mut graph = EventGraph::new();
        graph
            .insert(CorrectionRecord::new("correction", "missing", "fix source"))
            .unwrap();
        assert!(matches!(
            graph.validate(),
            Err(OntologyError::MissingEvent(id)) if id == OccurrenceId::new("missing")
        ));

        let mut graph = EventGraph::new();
        graph
            .insert(TransferRecord::between(
                "payment",
                endpoint("alice"),
                endpoint("vendor"),
                "USD",
                quantity("10", "USD"),
            ))
            .unwrap();
        graph
            .insert(
                CorrectionRecord::new("correction", "payment", "new bank evidence")
                    .replaces("replacement"),
            )
            .unwrap();
        assert!(matches!(
            graph.validate(),
            Err(OntologyError::MissingEvent(id)) if id == OccurrenceId::new("replacement")
        ));
    }

    #[test]
    fn event_graph_preserves_settlement_record_order_and_dates() {
        let amount = quantity("10", "USD");
        let mut graph = EventGraph::new();
        let mut issued = SettlementStateRecord::new(
            "issued",
            "settlement",
            SettlementState::Issued,
            amount.clone(),
            "USD",
            endpoint("alice"),
            endpoint("vendor"),
        );
        issued.at = Some(Date::new(2026, 1, 1).unwrap());
        let mut presented = SettlementStateRecord::new(
            "presented",
            "settlement",
            SettlementState::Presented,
            amount.clone(),
            "USD",
            endpoint("alice"),
            endpoint("vendor"),
        );
        presented.at = Some(Date::new(2026, 1, 2).unwrap());
        let mut settled = SettlementStateRecord::new(
            "settled",
            "settlement",
            SettlementState::Settled,
            amount,
            "USD",
            endpoint("alice"),
            endpoint("vendor"),
        );
        settled.at = Some(Date::new(2026, 1, 3).unwrap());
        graph.insert(issued).unwrap();
        graph.insert(presented).unwrap();
        graph.insert(settled).unwrap();
        graph.validate().unwrap();

        let mut out_of_order = EventGraph::new();
        let mut issued = SettlementStateRecord::new(
            "issued",
            "settlement",
            SettlementState::Issued,
            quantity("10", "USD"),
            "USD",
            endpoint("alice"),
            endpoint("vendor"),
        );
        issued.at = Some(Date::new(2026, 1, 2).unwrap());
        let mut presented = SettlementStateRecord::new(
            "presented",
            "settlement",
            SettlementState::Presented,
            quantity("10", "USD"),
            "USD",
            endpoint("alice"),
            endpoint("vendor"),
        );
        presented.at = Some(Date::new(2026, 1, 1).unwrap());
        out_of_order.insert(issued).unwrap();
        out_of_order.insert(presented).unwrap();
        assert!(matches!(
            out_of_order.validate(),
            Err(OntologyError::NonChronologicalSettlement { .. })
        ));
    }

    #[test]
    fn event_graph_derives_lot_remaining_and_blocks_unlinked_inventory() {
        let mut graph = EventGraph::new();
        let mut lot = Lot::new("lot-1", "ABC", quantity("10", "ABC"), "acquire-1").unwrap();
        lot.remaining = quantity("6", "ABC");
        graph.insert_lot(lot).unwrap();
        graph
            .insert(
                AcquireRecord::new("acquire-1", endpoint("alice"), "ABC", quantity("10", "ABC"))
                    .into_lot("lot-1"),
            )
            .unwrap();
        graph
            .insert(
                DisposeRecord::new("dispose-1", endpoint("alice"), "ABC", quantity("4", "ABC"))
                    .from_lot("lot-1"),
            )
            .unwrap();
        graph.validate().unwrap();

        let mut intact = EventGraph::new();
        intact
            .insert_lot(Lot::new("intact", "ABC", quantity("10", "ABC"), "acquire-2").unwrap())
            .unwrap();
        intact
            .insert(
                AcquireRecord::new("acquire-2", endpoint("alice"), "ABC", quantity("10", "ABC"))
                    .into_lot("intact"),
            )
            .unwrap();
        intact.validate().unwrap();

        let mut wrong_remaining = EventGraph::new();
        wrong_remaining
            .insert_lot(Lot::new("lot-1", "ABC", quantity("10", "ABC"), "acquire-1").unwrap())
            .unwrap();
        wrong_remaining
            .insert(
                AcquireRecord::new("acquire-1", endpoint("alice"), "ABC", quantity("10", "ABC"))
                    .into_lot("lot-1"),
            )
            .unwrap();
        wrong_remaining
            .insert(
                DisposeRecord::new("dispose-1", endpoint("alice"), "ABC", quantity("4", "ABC"))
                    .from_lot("lot-1"),
            )
            .unwrap();
        assert!(matches!(
            wrong_remaining.validate(),
            Err(OntologyError::InvalidEvent { kind: "lot", .. })
        ));

        let mut missing_link = EventGraph::new();
        missing_link
            .insert(AcquireRecord::new(
                "acquire-1",
                endpoint("alice"),
                "ABC",
                quantity("10", "ABC"),
            ))
            .unwrap();
        assert_eq!(missing_link.validate(), Err(OntologyError::MissingLot));
    }

    #[test]
    fn partial_payment_leaves_obligation_open() {
        let obligation =
            Obligation::transfer("rent", "alice", "landlord", "USD", quantity("100", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment-1",
            endpoint("alice"),
            endpoint("landlord"),
            "USD",
            quantity("40", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let allocation =
            SatisfactionAllocation::new("alloc-1", "rent", "payment-1", quantity("40", "USD"))
                .unwrap()
                .applied();
        let settlements = vec![settlement];
        validate_obligation_allocation(
            &obligation,
            std::slice::from_ref(&allocation),
            &settlements,
        )
        .unwrap();
        assert_eq!(
            obligation
                .remaining(std::slice::from_ref(&allocation), &settlements)
                .unwrap(),
            quantity("60", "USD")
        );
    }

    #[test]
    fn allocations_cannot_double_spend_one_settlement() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("200", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let first = SatisfactionAllocation::new(
            "allocation-1",
            "invoice",
            "payment",
            quantity("60", "USD"),
        )
        .unwrap()
        .applied();
        let second = SatisfactionAllocation::new(
            "allocation-2",
            "other-invoice",
            "payment",
            quantity("60", "USD"),
        )
        .unwrap()
        .applied();
        assert!(matches!(
            validate_obligation_allocation(&obligation, &[first, second], &[settlement]),
            Err(OntologyError::SettlementOverallocated { .. })
        ));
    }

    #[test]
    fn allocation_ids_are_unique_and_units_are_checked() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let first =
            SatisfactionAllocation::new("same-id", "invoice", "payment", quantity("25", "USD"))
                .unwrap()
                .applied();
        let duplicate =
            SatisfactionAllocation::new("same-id", "invoice", "payment", quantity("25", "USD"))
                .unwrap()
                .applied();
        assert!(matches!(
            validate_obligation_allocation(
                &obligation,
                &[first, duplicate],
                std::slice::from_ref(&settlement)
            ),
            Err(OntologyError::DuplicateAllocation(_))
        ));

        let wrong_unit = SatisfactionAllocation::new(
            "different-id",
            "invoice",
            "payment",
            quantity("25", "EUR"),
        )
        .unwrap()
        .applied();
        assert!(matches!(
            validate_obligation_allocation(
                &obligation,
                &[wrong_unit],
                std::slice::from_ref(&settlement)
            ),
            Err(OntologyError::UnitMismatch { .. })
        ));
    }

    #[test]
    fn allocation_endpoints_must_match_the_obligation() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment",
            endpoint("mallory"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let allocation =
            SatisfactionAllocation::new("allocation", "invoice", "payment", quantity("100", "USD"))
                .unwrap()
                .applied();
        assert_eq!(
            validate_obligation_allocation(&obligation, &[allocation], &[settlement]),
            Err(OntologyError::AllocationMismatch)
        );
    }

    #[test]
    fn bounced_settlement_preserves_the_underlying_obligation() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "check-42",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        settlement.returned(None, "insufficient funds").unwrap();
        let allocation = SatisfactionAllocation::new(
            "allocation",
            "invoice",
            "check-42",
            quantity("100", "USD"),
        )
        .unwrap()
        .applied();
        let settlements = vec![settlement];
        validate_obligation_allocation(
            &obligation,
            std::slice::from_ref(&allocation),
            &settlements,
        )
        .unwrap();
        assert_eq!(
            obligation
                .remaining(std::slice::from_ref(&allocation), &settlements)
                .unwrap(),
            quantity("100", "USD")
        );
    }

    #[test]
    fn satisfaction_network_rejects_duplicate_ids_and_unknown_references() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();

        assert!(matches!(
            validate_satisfaction_network(
                &[obligation.clone(), obligation.clone()],
                std::slice::from_ref(&settlement),
                &[],
            ),
            Err(OntologyError::DuplicateObligation(id)) if id == obligation.id
        ));
        assert!(matches!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                &[settlement.clone(), settlement.clone()],
                &[],
            ),
            Err(OntologyError::DuplicateSettlement(id)) if id == settlement.id
        ));
        let duplicate_allocation = SatisfactionAllocation::new(
            "same-allocation",
            "invoice",
            "payment",
            quantity("10", "USD"),
        )
        .unwrap();
        assert!(matches!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                std::slice::from_ref(&settlement),
                &[duplicate_allocation.clone(), duplicate_allocation],
            ),
            Err(OntologyError::DuplicateAllocation(id)) if id == AllocationId::new("same-allocation")
        ));

        let unknown_obligation = SatisfactionAllocation::new(
            "allocation-unknown-obligation",
            "missing-invoice",
            "payment",
            quantity("10", "USD"),
        )
        .unwrap()
        .applied();
        assert!(matches!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                std::slice::from_ref(&settlement),
                &[unknown_obligation],
            ),
            Err(OntologyError::UnknownObligation(id)) if id == ObligationId::new("missing-invoice")
        ));

        let unknown_settlement = SatisfactionAllocation::new(
            "allocation-unknown-settlement",
            "invoice",
            "missing-payment",
            quantity("10", "USD"),
        )
        .unwrap()
        .applied();
        assert!(matches!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                std::slice::from_ref(&settlement),
                &[unknown_settlement],
            ),
            Err(OntologyError::UnknownSettlement(id)) if id == SettlementId::new("missing-payment")
        ));
    }

    #[test]
    fn satisfaction_network_caps_one_settlement_across_obligations() {
        let first =
            Obligation::transfer("invoice-1", "alice", "vendor", "USD", quantity("60", "USD"))
                .unwrap();
        let second =
            Obligation::transfer("invoice-2", "alice", "vendor", "USD", quantity("60", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let allocations = vec![
            SatisfactionAllocation::new(
                "allocation-1",
                "invoice-1",
                "payment",
                quantity("60", "USD"),
            )
            .unwrap()
            .applied(),
            SatisfactionAllocation::new(
                "allocation-2",
                "invoice-2",
                "payment",
                quantity("60", "USD"),
            )
            .unwrap()
            .applied(),
        ];
        assert!(matches!(
            validate_satisfaction_network(&[first, second], &[settlement], &allocations),
            Err(OntologyError::SettlementOverallocated { settlement, .. })
                if settlement == SettlementId::new("payment")
        ));
    }

    #[test]
    fn satisfaction_network_checks_allocation_units_and_endpoints() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let mut settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        settlement
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        settlement
            .transition(SettlementState::Settled, None, None)
            .unwrap();

        let wrong_unit = SatisfactionAllocation::new(
            "allocation-unit",
            "invoice",
            "payment",
            quantity("100", "EUR"),
        )
        .unwrap()
        .applied();
        assert!(matches!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                std::slice::from_ref(&settlement),
                &[wrong_unit],
            ),
            Err(OntologyError::UnitMismatch { .. })
        ));

        let wrong_endpoint = Settlement::new(
            "wrong-payment",
            endpoint("mallory"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        let endpoint_allocation = SatisfactionAllocation::new(
            "allocation-endpoint",
            "invoice",
            "wrong-payment",
            quantity("100", "USD"),
        )
        .unwrap()
        .applied();
        assert_eq!(
            validate_satisfaction_network(
                std::slice::from_ref(&obligation),
                &[wrong_endpoint],
                &[endpoint_allocation],
            ),
            Err(OntologyError::AllocationMismatch)
        );
    }

    #[test]
    fn direct_domain_values_cannot_disagree_with_their_units_or_parties() {
        let mut obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("10", "USD"))
                .unwrap();
        let Performance::Transfer { instrument, .. } = &mut obligation.performance else {
            unreachable!()
        };
        *instrument = InstrumentId::new("EUR");
        assert!(matches!(
            obligation.validate(),
            Err(OntologyError::UnitMismatch { .. })
        ));

        let mut obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("10", "USD"))
                .unwrap();
        let Performance::Transfer { to, .. } = &mut obligation.performance else {
            unreachable!()
        };
        *to = EntityId::new("mallory");
        assert_eq!(
            obligation.validate(),
            Err(OntologyError::AllocationMismatch)
        );

        let mut settlement = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("10", "USD"),
        )
        .unwrap();
        settlement.instrument = InstrumentId::new("EUR");
        assert!(matches!(
            settlement.validate(),
            Err(OntologyError::UnitMismatch { .. })
        ));
        settlement.instrument = InstrumentId::new("USD");
        settlement.amount.unit = None;
        assert!(matches!(
            settlement.validate(),
            Err(OntologyError::UnitMismatch { .. })
        ));
    }

    #[test]
    fn per_obligation_validation_scopes_unrelated_settlements() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let mut payment = Settlement::new(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("100", "USD"),
        )
        .unwrap();
        payment
            .transition(SettlementState::Presented, None, None)
            .unwrap();
        payment
            .transition(SettlementState::Settled, None, None)
            .unwrap();
        let unrelated = Settlement::new(
            "unrelated-payment",
            endpoint("mallory"),
            endpoint("other-vendor"),
            "EUR",
            quantity("1", "EUR"),
        )
        .unwrap();
        let allocations = vec![
            SatisfactionAllocation::new("allocation", "invoice", "payment", quantity("100", "USD"))
                .unwrap()
                .applied(),
            // This allocation belongs to another settlement component.  It
            // must not prevent checking the invoice above.
            SatisfactionAllocation {
                id: AllocationId::new("unrelated-allocation"),
                obligation: ObligationId::new("other-invoice"),
                settlement: SettlementId::new("unrelated-payment"),
                quantity: quantity("1", "EUR"),
                state: AllocationState::Applied,
            },
        ];
        validate_obligation_allocation(&obligation, &allocations, &[payment, unrelated]).unwrap();
    }

    #[test]
    fn returned_charged_back_and_unresolved_disputes_restore_remaining() {
        let obligation =
            Obligation::transfer("invoice", "alice", "vendor", "USD", quantity("100", "USD"))
                .unwrap();
        let allocation =
            SatisfactionAllocation::new("allocation", "invoice", "payment", quantity("100", "USD"))
                .unwrap()
                .applied();

        for terminal in [
            SettlementState::Returned,
            SettlementState::Refunded,
            SettlementState::ChargedBack,
            SettlementState::Resolved,
        ] {
            let mut settlement = Settlement::new(
                "payment",
                endpoint("alice"),
                endpoint("vendor"),
                "USD",
                quantity("100", "USD"),
            )
            .unwrap();
            settlement
                .transition(SettlementState::Presented, None, None)
                .unwrap();
            settlement
                .transition(SettlementState::Settled, None, None)
                .unwrap();
            match terminal {
                SettlementState::Returned => settlement.returned(None, "bounced").unwrap(),
                SettlementState::Refunded => settlement
                    .transition(SettlementState::Refunded, None, Some("refunded".into()))
                    .unwrap(),
                SettlementState::ChargedBack => settlement
                    .transition(
                        SettlementState::ChargedBack,
                        None,
                        Some("chargeback".into()),
                    )
                    .unwrap(),
                SettlementState::Resolved => {
                    settlement
                        .transition(SettlementState::Disputed, None, Some("disputed".into()))
                        .unwrap();
                    settlement
                        .transition(SettlementState::Resolved, None, Some("resolved".into()))
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(!settlement.is_effective());
            assert_eq!(
                obligation
                    .remaining(
                        std::slice::from_ref(&allocation),
                        std::slice::from_ref(&settlement)
                    )
                    .unwrap(),
                quantity("100", "USD")
            );
        }
    }

    #[test]
    fn settlement_history_uses_source_order_for_chronology() {
        let id = SettlementId::new("payment");
        let history = vec![
            SettlementTransition {
                state: SettlementState::Issued,
                at: Some(Date::new(2026, 1, 2).unwrap()),
                reason: None,
            },
            SettlementTransition {
                state: SettlementState::Presented,
                at: Some(Date::new(2026, 1, 1).unwrap()),
                reason: None,
            },
        ];
        assert!(matches!(
            validate_settlement_history(&id, &history),
            Err(OntologyError::NonChronologicalSettlement { settlement, .. })
                if settlement == id
        ));

        let mut settlement = Settlement::from_history(
            "payment",
            endpoint("alice"),
            endpoint("vendor"),
            "USD",
            quantity("10", "USD"),
            vec![
                SettlementTransition {
                    state: SettlementState::Issued,
                    at: Some(Date::new(2026, 1, 2).unwrap()),
                    reason: None,
                },
                SettlementTransition {
                    state: SettlementState::Presented,
                    at: None,
                    reason: None,
                },
            ],
        )
        .unwrap();
        assert!(matches!(
            settlement.transition(
                SettlementState::Settled,
                Some(Date::new(2026, 1, 1).unwrap()),
                None,
            ),
            Err(OntologyError::NonChronologicalSettlement { .. })
        ));
    }

    #[test]
    fn lot_consumption_cannot_exceed_remaining_quantity() {
        let lot = Lot::new("lot-1", "ABC", quantity("10", "ABC"), "buy-1").unwrap();
        let consumed = lot.consumed(&quantity("4", "ABC")).unwrap();
        assert_eq!(lot.remaining, quantity("10", "ABC"));
        assert_eq!(consumed.remaining, quantity("6", "ABC"));
        assert!(consumed.consumed(&quantity("7", "ABC")).is_err());
        let disposal = DisposeRecord::new("sell-1", endpoint("alice"), "ABC", quantity("4", "ABC"))
            .from_lot("lot-1");
        let lot = Lot::new("lot-1", "ABC", quantity("10", "ABC"), "buy-1").unwrap();
        validate_remaining_lot_quantities(&[lot], &[disposal]).unwrap();
    }

    #[test]
    fn barter_exchange_conserves_each_leg_instrument() {
        let exchange = ExchangeRecord::new(
            "barter",
            vec![
                ExchangeLeg::new(
                    endpoint("alice"),
                    endpoint("bob"),
                    "apple",
                    quantity("1", "apple"),
                ),
                ExchangeLeg::new(
                    endpoint("bob"),
                    endpoint("alice"),
                    "bread",
                    quantity("2", "bread"),
                ),
            ],
        );
        validate_exchange_legs(&exchange).unwrap();
    }

    #[test]
    fn unequal_same_instrument_give_and_receive_is_rejected() {
        let exchange = ExchangeRecord::new(
            "unbalanced-cash",
            vec![
                ExchangeLeg::give(
                    endpoint("alice"),
                    endpoint("broker"),
                    "USD",
                    quantity("10", "USD"),
                ),
                ExchangeLeg::receive(
                    endpoint("broker"),
                    endpoint("alice"),
                    "USD",
                    quantity("1", "USD"),
                ),
            ],
        );
        assert!(matches!(
            validate_exchange_legs(&exchange),
            Err(OntologyError::ExchangeNotConserved { .. })
        ));
    }

    #[test]
    fn accepted_facts_only_become_recognized_by_an_explicit_book_projection() {
        let accepted = AcceptedFact::new("fact", None);
        let recognized = accepted.recognize(BookId::new("cash"));
        assert_eq!(recognized.book(), &BookId::new("cash"));
        assert_eq!(recognized.value(), &"fact");
    }
}
