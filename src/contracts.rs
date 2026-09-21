//! Typed contract primitives which sit above the accepted ontology.
//!
//! This module intentionally does not introduce another numeric type or a
//! second unit system.  Quantities are the model's exact, nominal quantities;
//! all ratios and balances are calculated with [`ExactNumber`].  The types
//! here describe the contract rules which are useful to an importer or a
//! book projection: corporate actions, debt cash-flow schedules, and
//! collateral state transitions.

use std::collections::BTreeMap;
use std::fmt;

use num_bigint::BigInt;
use num_traits::One;

use crate::exact::{ExactError, ExactNumber};
use crate::model::{AccountId, EntityId, InstrumentId, ModelError, OccurrenceId, Quantity, Unit};

/// A content-addressed policy identity.  `PolicyPackage` already owns the
/// canonical serialization and domain-separated hash used by the ledger, so
/// this alias keeps contract code from creating a subtly different identity.
pub type PolicyIdentity = crate::package::PolicyPackage;

/// Construct a policy identity using the same canonical package machinery as
/// the rest of the ledger.
pub fn policy_identity(
    name: impl Into<String>,
    version: impl Into<String>,
    body: impl crate::package::BodySource,
) -> PolicyIdentity {
    crate::package::PolicyPackage::new(name, version, body)
}

/// Errors at the contract boundary.  They retain enough context for callers
/// to distinguish malformed units from economic rule violations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    Model(ModelError),
    Numeric(ExactError),
    EmptyLegs(&'static str),
    NonPositive {
        context: &'static str,
    },
    Negative {
        context: &'static str,
    },
    UnitMismatch {
        expected: Unit,
        found: Option<Unit>,
    },
    InconsistentUnits {
        context: &'static str,
    },
    OffQuantum {
        quantity: Box<Quantity>,
        quantum: Box<Quantity>,
    },
    InvalidRatio {
        ratio: ExactNumber,
    },
    InconsistentActionLeg {
        holder: AccountId,
        expected: Box<Quantity>,
        actual: Box<Quantity>,
    },
    ActionNotConserved {
        context: &'static str,
        expected: Box<Quantity>,
        actual: Box<Quantity>,
    },
    DuplicateHolder(AccountId),
    InvalidSchedule(&'static str),
    UnknownPayment(u32),
    Overpayment {
        obligation: u32,
        remaining: Box<Quantity>,
        attempted: Box<Quantity>,
    },
    UnknownEncumbrance(OccurrenceId),
    DuplicateEncumbrance(OccurrenceId),
    EncumbranceExceedsFree {
        free: Box<Quantity>,
        requested: Box<Quantity>,
    },
    AlreadyReleased(OccurrenceId),
    ReleaseAfterDefault(OccurrenceId),
    InvalidDefault(OccurrenceId),
    InvalidRealization(OccurrenceId),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => error.fmt(f),
            Self::Numeric(error) => error.fmt(f),
            Self::EmptyLegs(kind) => write!(f, "{kind} requires at least one leg"),
            Self::NonPositive { context } => write!(f, "{context} must be positive"),
            Self::Negative { context } => write!(f, "{context} cannot be negative"),
            Self::UnitMismatch { expected, found } => {
                write!(f, "unit mismatch: expected {expected}, found {found:?}")
            }
            Self::InconsistentUnits { context } => write!(f, "inconsistent units: {context}"),
            Self::OffQuantum { quantity, quantum } => {
                write!(
                    f,
                    "{quantity} is not an exact multiple of quantum {quantum}"
                )
            }
            Self::InvalidRatio { ratio } => write!(f, "invalid ratio {ratio}"),
            Self::InconsistentActionLeg {
                holder,
                expected,
                actual,
            } => write!(
                f,
                "action leg for {holder} expects {expected}, got {actual}"
            ),
            Self::ActionNotConserved {
                context,
                expected,
                actual,
            } => write!(
                f,
                "{context} is not conserved: expected {expected}, got {actual}"
            ),
            Self::DuplicateHolder(holder) => write!(f, "duplicate action holder {holder}"),
            Self::InvalidSchedule(reason) => write!(f, "invalid debt schedule: {reason}"),
            Self::UnknownPayment(period) => write!(f, "unknown payment period {period}"),
            Self::Overpayment {
                obligation,
                remaining,
                attempted,
            } => write!(
                f,
                "payment {attempted} exceeds obligation {obligation} remaining {remaining}"
            ),
            Self::UnknownEncumbrance(id) => write!(f, "unknown encumbrance {id}"),
            Self::DuplicateEncumbrance(id) => write!(f, "duplicate encumbrance {id}"),
            Self::EncumbranceExceedsFree { free, requested } => {
                write!(f, "encumbrance {requested} exceeds free collateral {free}")
            }
            Self::AlreadyReleased(id) => write!(f, "encumbrance {id} was already released"),
            Self::ReleaseAfterDefault(id) => {
                write!(f, "encumbrance {id} cannot be released after default")
            }
            Self::InvalidDefault(id) => write!(f, "encumbrance {id} cannot default in this state"),
            Self::InvalidRealization(id) => {
                write!(f, "encumbrance {id} cannot be realized in this state")
            }
        }
    }
}

impl std::error::Error for ContractError {}

impl From<ModelError> for ContractError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<ExactError> for ContractError {
    fn from(error: ExactError) -> Self {
        Self::Numeric(error)
    }
}

fn positive(value: &ExactNumber, context: &'static str) -> Result<(), ContractError> {
    if value.is_negative() {
        Err(ContractError::Negative { context })
    } else if value.is_zero() {
        Err(ContractError::NonPositive { context })
    } else {
        Ok(())
    }
}

fn nonnegative(value: &ExactNumber, context: &'static str) -> Result<(), ContractError> {
    if value.is_negative() {
        Err(ContractError::Negative { context })
    } else {
        Ok(())
    }
}

fn same_unit(left: &Quantity, right: &Quantity) -> Result<(), ContractError> {
    if left.unit != right.unit {
        return Err(ContractError::InconsistentUnits {
            context: "quantities in one contract equation must share a unit",
        });
    }
    Ok(())
}

fn require_unit(quantity: &Quantity, context: &'static str) -> Result<Unit, ContractError> {
    quantity
        .unit
        .clone()
        .ok_or(ContractError::InvalidSchedule(context))
}

fn add(left: &Quantity, right: &Quantity) -> Result<Quantity, ContractError> {
    left.checked_add(right).map_err(ContractError::Model)
}

fn sub(left: &Quantity, right: &Quantity) -> Result<Quantity, ContractError> {
    left.checked_sub(right).map_err(ContractError::Model)
}

fn multiply(quantity: &Quantity, ratio: &ExactNumber) -> Quantity {
    Quantity {
        number: quantity.number.checked_mul(ratio),
        unit: quantity.unit.clone(),
    }
}

fn ensure_multiple(quantity: &Quantity, quantum: &Quantity) -> Result<(), ContractError> {
    same_unit(quantity, quantum)?;
    positive(&quantum.number, "quantum")?;
    nonnegative(&quantity.number, "quantity")?;
    if quantity.is_zero() {
        return Ok(());
    }
    let quotient = quantity.number.checked_div(&quantum.number)?;
    if quotient.as_rational().denom() != &BigInt::one() {
        return Err(ContractError::OffQuantum {
            quantity: Box::new(quantity.clone()),
            quantum: Box::new(quantum.clone()),
        });
    }
    Ok(())
}

fn equal(left: &Quantity, right: &Quantity) -> Result<bool, ContractError> {
    same_unit(left, right)?;
    Ok(left.number == right.number)
}

fn equal_amount(left: &Quantity, right: &Quantity) -> bool {
    left.number == right.number
}

// -------------------------------------------------------------------------
// Corporate actions

/// One holder's before/after position in a split, merge, or spin-off.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TransformationLeg {
    pub holder: AccountId,
    pub before: Quantity,
    pub after: Quantity,
}

impl TransformationLeg {
    pub fn new(holder: impl Into<AccountId>, before: Quantity, after: Quantity) -> Self {
        Self {
            holder: holder.into(),
            before,
            after,
        }
    }
}

/// A split or reverse split. `ratio` is destination units per source unit.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Split {
    pub id: OccurrenceId,
    pub source: InstrumentId,
    pub destination: InstrumentId,
    pub ratio: ExactNumber,
    pub legs: Vec<TransformationLeg>,
    pub source_quantum: Option<Quantity>,
    pub destination_quantum: Option<Quantity>,
}

impl Split {
    pub fn new(
        id: impl Into<OccurrenceId>,
        source: impl Into<InstrumentId>,
        destination: impl Into<InstrumentId>,
        ratio: ExactNumber,
        legs: Vec<TransformationLeg>,
    ) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            destination: destination.into(),
            ratio,
            legs,
            source_quantum: None,
            destination_quantum: None,
        }
    }

    pub fn with_quantum(mut self, source: Quantity, destination: Quantity) -> Self {
        self.source_quantum = Some(source);
        self.destination_quantum = Some(destination);
        self
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        validate_ratio(&self.ratio)?;
        validate_transformation_legs(
            "split",
            &self.legs,
            &self.ratio,
            self.source_quantum.as_ref(),
            self.destination_quantum.as_ref(),
        )
    }
}

/// A merge is represented by the same exact transformation equation as a
/// split; a ratio below one is normally used for a reverse denomination.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Merge {
    pub id: OccurrenceId,
    pub source: InstrumentId,
    pub destination: InstrumentId,
    pub ratio: ExactNumber,
    pub legs: Vec<TransformationLeg>,
    pub source_quantum: Option<Quantity>,
    pub destination_quantum: Option<Quantity>,
}

impl Merge {
    pub fn new(
        id: impl Into<OccurrenceId>,
        source: impl Into<InstrumentId>,
        destination: impl Into<InstrumentId>,
        ratio: ExactNumber,
        legs: Vec<TransformationLeg>,
    ) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            destination: destination.into(),
            ratio,
            legs,
            source_quantum: None,
            destination_quantum: None,
        }
    }

    pub fn with_quantum(mut self, source: Quantity, destination: Quantity) -> Self {
        self.source_quantum = Some(source);
        self.destination_quantum = Some(destination);
        self
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        validate_ratio(&self.ratio)?;
        validate_transformation_legs(
            "merge",
            &self.legs,
            &self.ratio,
            self.source_quantum.as_ref(),
            self.destination_quantum.as_ref(),
        )
    }
}

/// A spin-off can leave the parent instrument in place and issue a second
/// instrument.  Both mappings are explicit; there is no hidden balancing
/// leg and the parent after-position must be stated for every holder.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SpinoffLeg {
    pub holder: AccountId,
    pub parent_before: Quantity,
    pub parent_after: Quantity,
    pub spin_off: Quantity,
}

impl SpinoffLeg {
    pub fn new(
        holder: impl Into<AccountId>,
        parent_before: Quantity,
        parent_after: Quantity,
        spin_off: Quantity,
    ) -> Self {
        Self {
            holder: holder.into(),
            parent_before,
            parent_after,
            spin_off,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Spinoff {
    pub id: OccurrenceId,
    pub parent: InstrumentId,
    pub child: InstrumentId,
    pub child_ratio: ExactNumber,
    pub legs: Vec<SpinoffLeg>,
    pub parent_quantum: Option<Quantity>,
    pub child_quantum: Option<Quantity>,
}

impl Spinoff {
    pub fn new(
        id: impl Into<OccurrenceId>,
        parent: impl Into<InstrumentId>,
        child: impl Into<InstrumentId>,
        child_ratio: ExactNumber,
        legs: Vec<SpinoffLeg>,
    ) -> Self {
        Self {
            id: id.into(),
            parent: parent.into(),
            child: child.into(),
            child_ratio,
            legs,
            parent_quantum: None,
            child_quantum: None,
        }
    }

    pub fn with_quantum(mut self, parent: Quantity, child: Quantity) -> Self {
        self.parent_quantum = Some(parent);
        self.child_quantum = Some(child);
        self
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        validate_ratio(&self.child_ratio)?;
        if self.legs.is_empty() {
            return Err(ContractError::EmptyLegs("spinoff"));
        }
        let mut holders = std::collections::BTreeSet::new();
        for leg in &self.legs {
            if !holders.insert(leg.holder.clone()) {
                return Err(ContractError::DuplicateHolder(leg.holder.clone()));
            }
            positive(&leg.parent_before.number, "spinoff parent before")?;
            nonnegative(&leg.parent_after.number, "spinoff parent after")?;
            positive(&leg.spin_off.number, "spinoff child")?;
            same_unit(&leg.parent_before, &leg.parent_after)?;
            if leg.parent_after.number != leg.parent_before.number {
                return Err(ContractError::InconsistentActionLeg {
                    holder: leg.holder.clone(),
                    expected: Box::new(leg.parent_before.clone()),
                    actual: Box::new(leg.parent_after.clone()),
                });
            }
            let expected = multiply(&leg.parent_before, &self.child_ratio);
            if !equal_amount(&expected, &leg.spin_off) {
                return Err(ContractError::InconsistentActionLeg {
                    holder: leg.holder.clone(),
                    expected: Box::new(expected),
                    actual: Box::new(leg.spin_off.clone()),
                });
            }
            if let Some(quantum) = &self.parent_quantum {
                ensure_multiple(&leg.parent_before, quantum)?;
                ensure_multiple(&leg.parent_after, quantum)?;
            }
            if let Some(quantum) = &self.child_quantum {
                ensure_multiple(&leg.spin_off, quantum)?;
            }
        }
        Ok(())
    }
}

/// One dividend recipient. The sum of all payments must equal the declared
/// funding quantity, so a dividend cannot create or destroy cash silently.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DividendLeg {
    pub recipient: AccountId,
    pub amount: Quantity,
}

impl DividendLeg {
    pub fn new(recipient: impl Into<AccountId>, amount: Quantity) -> Self {
        Self {
            recipient: recipient.into(),
            amount,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Dividend {
    pub id: OccurrenceId,
    pub payer: AccountId,
    pub funding: Quantity,
    pub legs: Vec<DividendLeg>,
}

impl Dividend {
    pub fn new(
        id: impl Into<OccurrenceId>,
        payer: impl Into<AccountId>,
        funding: Quantity,
        legs: Vec<DividendLeg>,
    ) -> Self {
        Self {
            id: id.into(),
            payer: payer.into(),
            funding,
            legs,
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        positive(&self.funding.number, "dividend funding")?;
        if self.legs.is_empty() {
            return Err(ContractError::EmptyLegs("dividend"));
        }
        let mut recipients = std::collections::BTreeSet::new();
        let mut total: Option<Quantity> = None;
        for leg in &self.legs {
            if !recipients.insert(leg.recipient.clone()) {
                return Err(ContractError::DuplicateHolder(leg.recipient.clone()));
            }
            positive(&leg.amount.number, "dividend leg")?;
            same_unit(&self.funding, &leg.amount)?;
            total = Some(match total {
                Some(total) => add(&total, &leg.amount)?,
                None => leg.amount.clone(),
            });
        }
        let total = total.expect("nonempty dividend legs");
        if !equal(&self.funding, &total)? {
            return Err(ContractError::ActionNotConserved {
                context: "dividend funding",
                expected: Box::new(self.funding.clone()),
                actual: Box::new(total),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum CorporateAction {
    Split(Split),
    Merge(Merge),
    Spinoff(Spinoff),
    Dividend(Dividend),
}

impl CorporateAction {
    pub fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::Split(action) => action.validate(),
            Self::Merge(action) => action.validate(),
            Self::Spinoff(action) => action.validate(),
            Self::Dividend(action) => action.validate(),
        }
    }
}

fn validate_ratio(ratio: &ExactNumber) -> Result<(), ContractError> {
    positive(ratio, "corporate-action ratio")
}

fn validate_transformation_legs(
    context: &'static str,
    legs: &[TransformationLeg],
    ratio: &ExactNumber,
    source_quantum: Option<&Quantity>,
    destination_quantum: Option<&Quantity>,
) -> Result<(), ContractError> {
    if legs.is_empty() {
        return Err(ContractError::EmptyLegs(context));
    }
    let mut holders = std::collections::BTreeSet::new();
    for leg in legs {
        if !holders.insert(leg.holder.clone()) {
            return Err(ContractError::DuplicateHolder(leg.holder.clone()));
        }
        positive(&leg.before.number, "transformation source")?;
        positive(&leg.after.number, "transformation destination")?;
        let expected = Quantity {
            number: leg.before.number.checked_mul(ratio),
            // A ratio maps nominal source units to nominal destination
            // units. The destination unit is carried by the leg rather than
            // silently copied from the source.
            unit: leg.after.unit.clone(),
        };
        if !equal_amount(&expected, &leg.after) {
            return Err(ContractError::InconsistentActionLeg {
                holder: leg.holder.clone(),
                expected: Box::new(expected),
                actual: Box::new(leg.after.clone()),
            });
        }
        if let Some(quantum) = source_quantum {
            ensure_multiple(&leg.before, quantum)?;
        }
        if let Some(quantum) = destination_quantum {
            ensure_multiple(&leg.after, quantum)?;
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------
// Debt schedules and payment obligations

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AmortizationMethod {
    EqualPrincipal,
    Annuity,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PaymentObligation {
    pub period: u32,
    pub principal_due: Quantity,
    pub interest_due: Quantity,
    pub paid: Quantity,
}

impl PaymentObligation {
    fn new(
        period: u32,
        principal_due: Quantity,
        interest_due: Quantity,
    ) -> Result<Self, ContractError> {
        positive(&principal_due.number, "principal due")?;
        nonnegative(&interest_due.number, "interest due")?;
        same_unit(&principal_due, &interest_due)?;
        Ok(Self {
            period,
            principal_due: principal_due.clone(),
            interest_due,
            paid: Quantity::typed(ExactNumber::integer(0), principal_due.unit.clone().unwrap()),
        })
    }

    pub fn total_due(&self) -> Result<Quantity, ContractError> {
        add(&self.principal_due, &self.interest_due)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.period == 0 {
            return Err(ContractError::InvalidSchedule(
                "payment period must be positive",
            ));
        }
        positive(&self.principal_due.number, "principal due")?;
        nonnegative(&self.interest_due.number, "interest due")?;
        nonnegative(&self.paid.number, "paid amount")?;
        same_unit(&self.principal_due, &self.interest_due)?;
        same_unit(&self.principal_due, &self.paid)?;
        let total = self.total_due()?;
        if self.paid.number > total.number {
            return Err(ContractError::Overpayment {
                obligation: self.period,
                remaining: Box::new(Quantity::typed(
                    ExactNumber::integer(0),
                    require_unit(&total, "payment obligation must carry a unit")?,
                )),
                attempted: Box::new(self.paid.clone()),
            });
        }
        Ok(())
    }

    pub fn remaining(&self) -> Result<Quantity, ContractError> {
        sub(&self.total_due()?, &self.paid)
    }

    pub fn is_paid(&self) -> Result<bool, ContractError> {
        Ok(self.remaining()?.is_zero())
    }

    pub fn apply(&mut self, payment: Quantity) -> Result<(), ContractError> {
        positive(&payment.number, "payment")?;
        let remaining = self.remaining()?;
        same_unit(&payment, &remaining)?;
        if payment.number > remaining.number {
            return Err(ContractError::Overpayment {
                obligation: self.period,
                remaining: Box::new(remaining),
                attempted: Box::new(payment),
            });
        }
        self.paid = add(&self.paid, &payment)?;
        Ok(())
    }
}

/// A deterministic schedule of exact principal and interest obligations.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DebtSchedule {
    pub id: OccurrenceId,
    pub principal: Quantity,
    pub rate_per_period: ExactNumber,
    pub periods: u32,
    pub method: AmortizationMethod,
    pub obligations: Vec<PaymentObligation>,
    pub payment_quantum: Option<Quantity>,
}

impl DebtSchedule {
    pub fn new(
        id: impl Into<OccurrenceId>,
        principal: Quantity,
        rate_per_period: ExactNumber,
        periods: u32,
        method: AmortizationMethod,
    ) -> Result<Self, ContractError> {
        positive(&principal.number, "debt principal")?;
        require_unit(&principal, "debt principal must carry a unit")?;
        nonnegative(&rate_per_period, "interest rate")?;
        if periods == 0 {
            return Err(ContractError::InvalidSchedule(
                "period count must be positive",
            ));
        }
        let obligations = build_obligations(&principal, &rate_per_period, periods, method)?;
        let schedule = Self {
            id: id.into(),
            principal,
            rate_per_period,
            periods,
            method,
            obligations,
            payment_quantum: None,
        };
        schedule.validate()?;
        Ok(schedule)
    }

    pub fn with_payment_quantum(mut self, quantum: Quantity) -> Result<Self, ContractError> {
        positive(&quantum.number, "payment quantum")?;
        same_unit(&self.principal, &quantum)?;
        for obligation in &self.obligations {
            ensure_multiple(&obligation.principal_due, &quantum)?;
            ensure_multiple(&obligation.interest_due, &quantum)?;
            ensure_multiple(&obligation.total_due()?, &quantum)?;
        }
        self.payment_quantum = Some(quantum);
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        positive(&self.principal.number, "debt principal")?;
        nonnegative(&self.rate_per_period, "interest rate")?;
        if self.obligations.len() != self.periods as usize {
            return Err(ContractError::InvalidSchedule(
                "obligation count does not match periods",
            ));
        }
        let principal_unit = require_unit(&self.principal, "debt principal must carry a unit")?;
        let mut principal_total = Quantity::typed(ExactNumber::integer(0), principal_unit);
        for (index, obligation) in self.obligations.iter().enumerate() {
            if obligation.period != index as u32 + 1 {
                return Err(ContractError::InvalidSchedule("periods must be contiguous"));
            }
            obligation.validate()?;
            same_unit(&obligation.principal_due, &self.principal)?;
            same_unit(&obligation.interest_due, &self.principal)?;
            principal_total = add(&principal_total, &obligation.principal_due)?;
            if let Some(quantum) = &self.payment_quantum {
                ensure_multiple(&obligation.principal_due, quantum)?;
                ensure_multiple(&obligation.interest_due, quantum)?;
                ensure_multiple(&obligation.total_due()?, quantum)?;
            }
        }
        if !equal(&principal_total, &self.principal)? {
            return Err(ContractError::ActionNotConserved {
                context: "debt principal amortization",
                expected: Box::new(self.principal.clone()),
                actual: Box::new(principal_total),
            });
        }
        Ok(())
    }

    pub fn obligation(&self, period: u32) -> Option<&PaymentObligation> {
        self.obligations.get(period.checked_sub(1)? as usize)
    }

    pub fn obligation_mut(&mut self, period: u32) -> Result<&mut PaymentObligation, ContractError> {
        self.obligations
            .get_mut(
                period
                    .checked_sub(1)
                    .ok_or(ContractError::UnknownPayment(period))? as usize,
            )
            .ok_or(ContractError::UnknownPayment(period))
    }

    pub fn apply_payment(&mut self, period: u32, payment: Quantity) -> Result<(), ContractError> {
        if let Some(quantum) = &self.payment_quantum {
            ensure_multiple(&payment, quantum)?;
        }
        self.obligation_mut(period)?.apply(payment)
    }

    pub fn outstanding(&self) -> Result<Quantity, ContractError> {
        let principal_unit = require_unit(&self.principal, "debt principal must carry a unit")?;
        let mut total = Quantity::typed(ExactNumber::integer(0), principal_unit);
        for obligation in &self.obligations {
            total = add(&total, &obligation.remaining()?)?;
        }
        Ok(total)
    }
}

/// Alias emphasizing that the generated obligations are a payment schedule.
pub type PaymentSchedule = DebtSchedule;

fn build_obligations(
    principal: &Quantity,
    rate: &ExactNumber,
    periods: u32,
    method: AmortizationMethod,
) -> Result<Vec<PaymentObligation>, ContractError> {
    let periods_exact = ExactNumber::integer(i128::from(periods));
    let equal_principal = principal.number.checked_div(&periods_exact)?;
    let mut obligations = Vec::with_capacity(periods as usize);
    let mut opening = principal.number.clone();
    let payment = match method {
        AmortizationMethod::EqualPrincipal => None,
        AmortizationMethod::Annuity => Some(annuity_payment(&principal.number, rate, periods)?),
    };
    for period in 1..=periods {
        let interest = opening.checked_mul(rate);
        let principal_due = match payment.as_ref() {
            None => equal_principal.clone(),
            Some(payment) => payment.checked_sub(&interest),
        };
        positive(&principal_due, "principal due")?;
        let obligation = PaymentObligation::new(
            period,
            Quantity {
                number: principal_due.clone(),
                unit: principal.unit.clone(),
            },
            Quantity {
                number: interest,
                unit: principal.unit.clone(),
            },
        )?;
        obligations.push(obligation);
        opening = opening.checked_sub(&principal_due);
    }
    if !opening.is_zero() {
        return Err(ContractError::InvalidSchedule(
            "amortization leaves a nonzero principal balance",
        ));
    }
    Ok(obligations)
}

fn annuity_payment(
    principal: &ExactNumber,
    rate: &ExactNumber,
    periods: u32,
) -> Result<ExactNumber, ContractError> {
    if rate.is_zero() {
        return Ok(principal.checked_div(&ExactNumber::integer(i128::from(periods)))?);
    }
    let mut factor = ExactNumber::integer(1);
    for _ in 0..periods {
        factor = factor.checked_mul(&rate.checked_add(&ExactNumber::integer(1)));
    }
    let numerator = principal.checked_mul(rate).checked_mul(&factor);
    let denominator = factor.checked_sub(&ExactNumber::integer(1));
    numerator.checked_div(&denominator).map_err(Into::into)
}

// -------------------------------------------------------------------------
// Collateral

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CollateralState {
    Active,
    Released,
    Defaulted,
    Realized,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CollateralEncumbrance {
    pub id: OccurrenceId,
    pub beneficiary: EntityId,
    pub quantity: Quantity,
    pub state: CollateralState,
}

impl CollateralEncumbrance {
    pub fn new(
        id: impl Into<OccurrenceId>,
        beneficiary: impl Into<EntityId>,
        quantity: Quantity,
    ) -> Result<Self, ContractError> {
        positive(&quantity.number, "encumbrance quantity")?;
        Ok(Self {
            id: id.into(),
            beneficiary: beneficiary.into(),
            quantity,
            state: CollateralState::Active,
        })
    }

    pub fn is_active(&self) -> bool {
        self.state == CollateralState::Active
    }

    pub fn release(&mut self) -> Result<(), ContractError> {
        match self.state {
            CollateralState::Active => {
                self.state = CollateralState::Released;
                Ok(())
            }
            CollateralState::Released => Err(ContractError::AlreadyReleased(self.id.clone())),
            CollateralState::Defaulted | CollateralState::Realized => {
                Err(ContractError::ReleaseAfterDefault(self.id.clone()))
            }
        }
    }

    pub fn declare_default(&mut self) -> Result<(), ContractError> {
        if self.state != CollateralState::Active {
            return Err(ContractError::InvalidDefault(self.id.clone()));
        }
        self.state = CollateralState::Defaulted;
        Ok(())
    }

    pub fn realize(&mut self) -> Result<(), ContractError> {
        if self.state != CollateralState::Defaulted {
            return Err(ContractError::InvalidRealization(self.id.clone()));
        }
        self.state = CollateralState::Realized;
        Ok(())
    }
}

/// A collateral position owns the reservation ledger.  Encumbrances are
/// accepted only when the free quantity is sufficient, and all reservations
/// remain unit-checked against the position.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CollateralPosition {
    pub id: OccurrenceId,
    pub owner: EntityId,
    pub instrument: InstrumentId,
    pub quantity: Quantity,
    pub quantum: Option<Quantity>,
    pub encumbrances: BTreeMap<OccurrenceId, CollateralEncumbrance>,
}

impl CollateralPosition {
    pub fn new(
        id: impl Into<OccurrenceId>,
        owner: impl Into<EntityId>,
        instrument: impl Into<InstrumentId>,
        quantity: Quantity,
    ) -> Result<Self, ContractError> {
        positive(&quantity.number, "collateral position")?;
        Ok(Self {
            id: id.into(),
            owner: owner.into(),
            instrument: instrument.into(),
            quantity,
            quantum: None,
            encumbrances: BTreeMap::new(),
        })
    }

    pub fn with_quantum(mut self, quantum: Quantity) -> Result<Self, ContractError> {
        ensure_multiple(&self.quantity, &quantum)?;
        self.quantum = Some(quantum);
        Ok(self)
    }

    pub fn encumber(&mut self, encumbrance: CollateralEncumbrance) -> Result<(), ContractError> {
        if self.encumbrances.contains_key(&encumbrance.id) {
            return Err(ContractError::DuplicateEncumbrance(encumbrance.id));
        }
        same_unit(&self.quantity, &encumbrance.quantity)?;
        if let Some(quantum) = &self.quantum {
            ensure_multiple(&encumbrance.quantity, quantum)?;
        }
        let free = self.free_quantity()?;
        if encumbrance.quantity.number > free.number {
            return Err(ContractError::EncumbranceExceedsFree {
                free: Box::new(free),
                requested: Box::new(encumbrance.quantity),
            });
        }
        self.encumbrances
            .insert(encumbrance.id.clone(), encumbrance);
        Ok(())
    }

    pub fn encumber_quantity(
        &mut self,
        id: impl Into<OccurrenceId>,
        beneficiary: impl Into<EntityId>,
        quantity: Quantity,
    ) -> Result<(), ContractError> {
        self.encumber(CollateralEncumbrance::new(id, beneficiary, quantity)?)
    }

    pub fn release(&mut self, id: &OccurrenceId) -> Result<(), ContractError> {
        self.encumbrance_mut(id)?.release()
    }

    pub fn declare_default(&mut self, id: &OccurrenceId) -> Result<(), ContractError> {
        self.encumbrance_mut(id)?.declare_default()
    }

    pub fn realize(&mut self, id: &OccurrenceId) -> Result<(), ContractError> {
        self.encumbrance_mut(id)?.realize()
    }

    pub fn encumbrance(&self, id: &OccurrenceId) -> Option<&CollateralEncumbrance> {
        self.encumbrances.get(id)
    }

    pub fn free_quantity(&self) -> Result<Quantity, ContractError> {
        let mut reserved = Quantity::typed(
            ExactNumber::integer(0),
            self.quantity
                .unit
                .clone()
                .ok_or(ContractError::InvalidSchedule(
                    "collateral must carry a unit",
                ))?,
        );
        for encumbrance in self.encumbrances.values() {
            if encumbrance.state != CollateralState::Released {
                reserved = add(&reserved, &encumbrance.quantity)?;
            }
        }
        if reserved.number > self.quantity.number {
            return Err(ContractError::EncumbranceExceedsFree {
                free: Box::new(Quantity {
                    number: ExactNumber::integer(0),
                    unit: self.quantity.unit.clone(),
                }),
                requested: Box::new(reserved),
            });
        }
        sub(&self.quantity, &reserved)
    }

    fn encumbrance_mut(
        &mut self,
        id: &OccurrenceId,
    ) -> Result<&mut CollateralEncumbrance, ContractError> {
        self.encumbrances
            .get_mut(id)
            .ok_or_else(|| ContractError::UnknownEncumbrance(id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentHash;

    fn quantity(text: &str, unit: &str) -> Quantity {
        Quantity::with_unit(text.parse().unwrap(), unit).unwrap()
    }

    #[test]
    fn split_rejects_inconsistent_leg_and_off_quantum() {
        let split = Split::new(
            "split/1",
            "old",
            "new",
            "2".parse().unwrap(),
            vec![TransformationLeg::new(
                "alice",
                quantity("1", "share"),
                quantity("3", "share"),
            )],
        )
        .with_quantum(quantity("1", "share"), quantity("2", "share"));
        assert!(matches!(
            split.validate(),
            Err(ContractError::InconsistentActionLeg { .. })
        ));

        let split = Split::new(
            "split/2",
            "old",
            "new",
            "2".parse().unwrap(),
            vec![TransformationLeg::new(
                "alice",
                quantity("0.5", "share"),
                quantity("1", "share"),
            )],
        )
        .with_quantum(quantity("1", "share"), quantity("1", "share"));
        assert!(matches!(
            split.validate(),
            Err(ContractError::OffQuantum { .. })
        ));
    }

    #[test]
    fn dividend_and_schedule_cannot_create_or_overpay_value() {
        let dividend = Dividend::new(
            "div/1",
            "issuer",
            quantity("10", "USD"),
            vec![
                DividendLeg::new("alice", quantity("6", "USD")),
                DividendLeg::new("bob", quantity("3", "USD")),
            ],
        );
        assert!(matches!(
            dividend.validate(),
            Err(ContractError::ActionNotConserved { .. })
        ));

        let mut schedule = DebtSchedule::new(
            "loan/1",
            quantity("100", "USD"),
            "0".parse().unwrap(),
            2,
            AmortizationMethod::EqualPrincipal,
        )
        .unwrap();
        assert!(schedule.apply_payment(1, quantity("51", "USD")).is_err());
        schedule.apply_payment(1, quantity("50", "USD")).unwrap();
        assert!(schedule.apply_payment(1, quantity("1", "USD")).is_err());
    }

    #[test]
    fn collateral_release_is_single_use_and_default_is_terminal_for_release() {
        let mut position =
            CollateralPosition::new("position/1", "alice", "bond", quantity("10", "bond")).unwrap();
        position
            .encumber_quantity("enc/1", "lender", quantity("4", "bond"))
            .unwrap();
        position.release(&OccurrenceId::from("enc/1")).unwrap();
        assert!(matches!(
            position.release(&OccurrenceId::from("enc/1")),
            Err(ContractError::AlreadyReleased(_))
        ));
        position
            .encumber_quantity("enc/2", "lender", quantity("4", "bond"))
            .unwrap();
        position
            .declare_default(&OccurrenceId::from("enc/2"))
            .unwrap();
        assert!(matches!(
            position.release(&OccurrenceId::from("enc/2")),
            Err(ContractError::ReleaseAfterDefault(_))
        ));
    }

    #[test]
    fn policy_identity_is_content_stable() {
        let first = policy_identity("contract/priority", "1", "z=last\na=first");
        let second = policy_identity("contract/priority", "1", "a=first\nz=last");
        assert_eq!(first.hash(), second.hash());
        let _hash: ContentHash = first.hash();
    }
}
