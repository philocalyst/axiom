//! Large, exact end-to-end contract workflows.
//!
//! The scenarios in this file intentionally stay at the public contract
//! boundary.  They model a broad holder base, a sequence of corporate
//! actions, two amortization styles, and a collateral reservation lifecycle.
//! Every stage checks a conservation or state invariant before moving on.

use std::collections::BTreeSet;

use axiom_ledger::contracts::{
    AmortizationMethod, CollateralPosition, CollateralState, ContractError, CorporateAction,
    DebtSchedule, Dividend, DividendLeg, Merge, Spinoff, SpinoffLeg, Split, TransformationLeg,
};
use axiom_ledger::exact::ExactNumber;
use axiom_ledger::model::{AccountId, OccurrenceId, Quantity};

fn quantity(number: &str, unit: &str) -> Quantity {
    Quantity::with_unit(
        ExactNumber::parse(number).expect("valid exact number"),
        unit,
    )
    .expect("non-empty unit")
}

fn ratio(number: i128, denominator: i128) -> ExactNumber {
    ExactNumber::rational(number, denominator).expect("non-zero ratio denominator")
}

fn total<I>(numbers: I) -> ExactNumber
where
    I: IntoIterator<Item = ExactNumber>,
{
    numbers
        .into_iter()
        .fold(ExactNumber::integer(0), |sum, number| {
            sum.checked_add(&number)
        })
}

fn holders(count: u32) -> Vec<AccountId> {
    (1..=count)
        .map(|number| AccountId::from(format!("holder-{number:04}")))
        .collect()
}

#[test]
fn institutional_workflow_preserves_value_through_actions_debt_and_collateral() {
    // A 256-holder cap table is large enough to exercise aggregate behavior
    // without hiding the per-holder equations in helper code.
    let holders = holders(256);
    let holder_count = ExactNumber::integer(holders.len() as i128);

    let split_legs: Vec<_> = holders
        .iter()
        .map(|holder| {
            TransformationLeg::new(
                holder.clone(),
                quantity("100", "legacy-share"),
                quantity("150", "new-share"),
            )
        })
        .collect();
    let split = Split::new(
        "corp/split-2026",
        "legacy-equity",
        "new-equity",
        ratio(3, 2),
        split_legs,
    )
    .with_quantum(quantity("1", "legacy-share"), quantity("1", "new-share"));
    CorporateAction::Split(split.clone())
        .validate()
        .expect("every holder receives the exact three-for-two split");
    assert_eq!(
        total(split.legs.iter().map(|leg| leg.before.number.clone())),
        ExactNumber::integer(100).checked_mul(&holder_count)
    );
    assert_eq!(
        total(split.legs.iter().map(|leg| leg.after.number.clone())),
        ExactNumber::integer(150).checked_mul(&holder_count)
    );

    let spinoff_legs: Vec<_> = holders
        .iter()
        .map(|holder| {
            SpinoffLeg::new(
                holder.clone(),
                quantity("150", "new-share"),
                quantity("150", "new-share"),
                quantity("15", "child-share"),
            )
        })
        .collect();
    let spinoff = Spinoff::new(
        "corp/spinoff-2026",
        "new-equity",
        "child-equity",
        ratio(1, 10),
        spinoff_legs,
    )
    .with_quantum(quantity("1", "new-share"), quantity("1", "child-share"));
    CorporateAction::Spinoff(spinoff.clone())
        .validate()
        .expect("spinoff parent position stays unchanged and child issue is exact");
    assert_eq!(
        total(
            spinoff
                .legs
                .iter()
                .map(|leg| leg.parent_before.number.clone())
        ),
        total(
            spinoff
                .legs
                .iter()
                .map(|leg| leg.parent_after.number.clone())
        )
    );
    assert_eq!(
        total(spinoff.legs.iter().map(|leg| leg.spin_off.number.clone())),
        ExactNumber::integer(15).checked_mul(&holder_count)
    );

    // A reverse denomination after the spin-off must recover the original
    // post-split count, independently for every holder.
    let merge_legs: Vec<_> = holders
        .iter()
        .map(|holder| {
            TransformationLeg::new(
                holder.clone(),
                quantity("150", "new-share"),
                quantity("100", "restated-share"),
            )
        })
        .collect();
    let merge = Merge::new(
        "corp/merge-2027",
        "new-equity",
        "restated-equity",
        ratio(2, 3),
        merge_legs,
    )
    .with_quantum(quantity("1", "new-share"), quantity("1", "restated-share"));
    CorporateAction::Merge(merge.clone())
        .validate()
        .expect("reverse denomination is exact");
    assert_eq!(
        total(merge.legs.iter().map(|leg| leg.before.number.clone())),
        ExactNumber::integer(150).checked_mul(&holder_count)
    );
    assert_eq!(
        total(merge.legs.iter().map(|leg| leg.after.number.clone())),
        ExactNumber::integer(100).checked_mul(&holder_count)
    );

    let dividend = Dividend::new(
        "corp/dividend-2027",
        "issuer",
        quantity("640", "USD"),
        holders
            .iter()
            .map(|holder| DividendLeg::new(holder.clone(), quantity("2.5", "USD")))
            .collect(),
    );
    CorporateAction::Dividend(dividend.clone())
        .validate()
        .expect("cash funding equals every holder's dividend leg");
    assert_eq!(
        total(dividend.legs.iter().map(|leg| leg.amount.number.clone())),
        dividend.funding.number
    );

    // Equal-principal amortization gives a deliberately quantum-safe debt:
    // every principal and interest installment is payable to the cent.
    let mut equal_principal = DebtSchedule::new(
        "debt/warehouse-loan",
        quantity("960000", "USD"),
        ratio(1, 100),
        24,
        AmortizationMethod::EqualPrincipal,
    )
    .expect("valid equal-principal schedule")
    .with_payment_quantum(quantity("0.01", "USD"))
    .expect("schedule is cent exact");
    let principal_due = total(
        equal_principal
            .obligations
            .iter()
            .map(|obligation| obligation.principal_due.number.clone()),
    );
    assert_eq!(principal_due, equal_principal.principal.number);
    let expected_debt_cash = total(
        equal_principal
            .obligations
            .iter()
            .map(|obligation| obligation.total_due().expect("same-unit debt due").number),
    );
    for period in 1..=equal_principal.periods {
        let principal = equal_principal
            .obligation(period)
            .expect("contiguous debt period")
            .principal_due
            .clone();
        let interest = equal_principal
            .obligation(period)
            .expect("contiguous debt period")
            .interest_due
            .clone();
        equal_principal
            .apply_payment(period, principal)
            .expect("principal installment applies");
        equal_principal
            .apply_payment(period, interest)
            .expect("interest installment applies");
    }
    assert!(
        equal_principal
            .outstanding()
            .expect("outstanding balance")
            .is_zero()
    );
    let paid = total(
        equal_principal
            .obligations
            .iter()
            .map(|obligation| obligation.paid.number.clone()),
    );
    assert_eq!(paid, expected_debt_cash);

    // Annuity schedules keep the contractual payment constant while exact
    // rational arithmetic makes the principal/interest split change each
    // period without rounding drift.
    let mut annuity = DebtSchedule::new(
        "debt/equipment-loan",
        quantity("480000", "USD"),
        ratio(1, 100),
        12,
        AmortizationMethod::Annuity,
    )
    .expect("valid annuity schedule");
    let first_payment = annuity
        .obligation(1)
        .expect("first annuity period")
        .total_due()
        .expect("annuity due")
        .number;
    let last_payment = annuity
        .obligation(12)
        .expect("last annuity period")
        .total_due()
        .expect("annuity due")
        .number;
    assert_eq!(first_payment, last_payment);
    for period in 1..=annuity.periods {
        let payment = annuity
            .obligation(period)
            .expect("contiguous annuity period")
            .total_due()
            .expect("same-unit annuity due");
        annuity
            .apply_payment(period, payment)
            .expect("annuity installment applies");
    }
    assert!(
        annuity
            .outstanding()
            .expect("annuity outstanding balance")
            .is_zero()
    );

    // Collateral reservations are independent of the debt arithmetic, but
    // both ledgers must still agree on the available quantity at each state.
    let mut collateral = CollateralPosition::new(
        "collateral/warehouse",
        "borrower",
        "warehouse-equity",
        quantity("10000", "share"),
    )
    .expect("positive collateral position")
    .with_quantum(quantity("1", "share"))
    .expect("whole-share collateral");
    collateral
        .encumber_quantity("encumbrance/a", "senior-lender", quantity("6000", "share"))
        .expect("senior facility reservation");
    collateral
        .encumber_quantity(
            "encumbrance/b",
            "mezzanine-lender",
            quantity("3000", "share"),
        )
        .expect("mezzanine facility reservation");
    assert_eq!(
        collateral
            .free_quantity()
            .expect("free collateral after reservations")
            .number,
        ExactNumber::integer(1000)
    );
    collateral
        .release(&OccurrenceId::from("encumbrance/a"))
        .expect("releasing a performing senior facility");
    assert_eq!(
        collateral
            .free_quantity()
            .expect("free collateral after release")
            .number,
        ExactNumber::integer(7000)
    );
    collateral
        .declare_default(&OccurrenceId::from("encumbrance/b"))
        .expect("default transitions the mezzanine facility");
    collateral
        .realize(&OccurrenceId::from("encumbrance/b"))
        .expect("defaulted collateral can be realized");
    assert_eq!(
        collateral
            .encumbrance(&OccurrenceId::from("encumbrance/b"))
            .expect("mezzanine encumbrance")
            .state,
        CollateralState::Realized
    );
    // Realized collateral remains reserved until a separate release/settlement
    // record exists, so the reservation ledger still reports 7,000 free.
    assert_eq!(
        collateral
            .free_quantity()
            .expect("free collateral after realization")
            .number,
        ExactNumber::integer(7000)
    );
}

#[test]
fn contract_error_paths_are_specific_and_leave_state_unchanged() {
    let duplicate_holder = AccountId::from("holder-duplicate");
    let duplicate_legs = vec![
        TransformationLeg::new(
            duplicate_holder.clone(),
            quantity("10", "share"),
            quantity("20", "share"),
        ),
        TransformationLeg::new(
            duplicate_holder,
            quantity("5", "share"),
            quantity("10", "share"),
        ),
    ];
    let duplicate_split = Split::new(
        "error/split-duplicate",
        "old",
        "new",
        ExactNumber::integer(2),
        duplicate_legs,
    );
    assert!(matches!(
        duplicate_split.validate(),
        Err(ContractError::DuplicateHolder(_))
    ));

    let inconsistent_split = Split::new(
        "error/split-equation",
        "old",
        "new",
        ExactNumber::integer(2),
        vec![TransformationLeg::new(
            "holder-1",
            quantity("10", "share"),
            quantity("19", "share"),
        )],
    );
    assert!(matches!(
        inconsistent_split.validate(),
        Err(ContractError::InconsistentActionLeg { .. })
    ));

    let off_quantum = Split::new(
        "error/split-quantum",
        "old",
        "new",
        ExactNumber::integer(2),
        vec![TransformationLeg::new(
            "holder-1",
            quantity("1.5", "share"),
            quantity("3", "share"),
        )],
    )
    .with_quantum(quantity("1", "share"), quantity("1", "share"));
    assert!(matches!(
        off_quantum.validate(),
        Err(ContractError::OffQuantum { .. })
    ));

    let short_dividend = Dividend::new(
        "error/dividend-short",
        "issuer",
        quantity("100", "USD"),
        vec![
            DividendLeg::new("holder-1", quantity("60", "USD")),
            DividendLeg::new("holder-2", quantity("30", "USD")),
        ],
    );
    assert!(matches!(
        short_dividend.validate(),
        Err(ContractError::ActionNotConserved { .. })
    ));

    let mut debt = DebtSchedule::new(
        "error/debt",
        quantity("1000", "USD"),
        ExactNumber::integer(0),
        2,
        AmortizationMethod::EqualPrincipal,
    )
    .expect("valid test debt")
    .with_payment_quantum(quantity("0.01", "USD"))
    .expect("whole-cent test debt");
    assert!(matches!(
        debt.apply_payment(0, quantity("1", "USD")),
        Err(ContractError::UnknownPayment(0))
    ));
    assert!(matches!(
        debt.apply_payment(1, quantity("501", "USD")),
        Err(ContractError::Overpayment { obligation: 1, .. })
    ));
    assert!(
        debt.obligation(1)
            .expect("period one still exists")
            .paid
            .is_zero()
    );
    assert!(matches!(
        debt.apply_payment(1, quantity("1", "EUR")),
        Err(ContractError::InconsistentUnits { .. })
    ));
    assert!(matches!(
        debt.apply_payment(1, quantity("0.005", "USD")),
        Err(ContractError::OffQuantum { .. })
    ));
    assert!(
        debt.obligation(1)
            .expect("failed payments do not mutate debt")
            .paid
            .is_zero()
    );

    let mut collateral = CollateralPosition::new(
        "error/collateral",
        "borrower",
        "bond",
        quantity("100", "bond"),
    )
    .expect("valid collateral")
    .with_quantum(quantity("1", "bond"))
    .expect("whole-bond collateral");
    assert!(matches!(
        collateral.release(&OccurrenceId::from("missing")),
        Err(ContractError::UnknownEncumbrance(_))
    ));
    collateral
        .encumber_quantity("encumbrance/one", "lender", quantity("60", "bond"))
        .expect("first reservation");
    assert!(matches!(
        collateral.encumber_quantity("encumbrance/one", "lender", quantity("1", "bond")),
        Err(ContractError::DuplicateEncumbrance(_))
    ));
    assert!(matches!(
        collateral.encumber_quantity("encumbrance/too-large", "lender", quantity("41", "bond")),
        Err(ContractError::EncumbranceExceedsFree { .. })
    ));
    assert_eq!(
        collateral
            .free_quantity()
            .expect("failed reservation is atomic")
            .number,
        ExactNumber::integer(40)
    );
    collateral
        .release(&OccurrenceId::from("encumbrance/one"))
        .expect("active reservation can be released");
    assert!(matches!(
        collateral.release(&OccurrenceId::from("encumbrance/one")),
        Err(ContractError::AlreadyReleased(_))
    ));

    collateral
        .encumber_quantity("encumbrance/two", "lender", quantity("20", "bond"))
        .expect("second reservation");
    assert!(matches!(
        collateral.realize(&OccurrenceId::from("encumbrance/two")),
        Err(ContractError::InvalidRealization(_))
    ));
    collateral
        .declare_default(&OccurrenceId::from("encumbrance/two"))
        .expect("active reservation defaults");
    assert!(matches!(
        collateral.declare_default(&OccurrenceId::from("encumbrance/two")),
        Err(ContractError::InvalidDefault(_))
    ));
    assert!(matches!(
        collateral.release(&OccurrenceId::from("encumbrance/two")),
        Err(ContractError::ReleaseAfterDefault(_))
    ));
    collateral
        .realize(&OccurrenceId::from("encumbrance/two"))
        .expect("defaulted reservation realizes");
    assert!(matches!(
        collateral.release(&OccurrenceId::from("encumbrance/two")),
        Err(ContractError::ReleaseAfterDefault(_))
    ));

    let active_ids: BTreeSet<_> = collateral
        .encumbrances
        .values()
        .filter(|encumbrance| encumbrance.is_active())
        .map(|encumbrance| encumbrance.id.clone())
        .collect();
    assert!(
        active_ids.is_empty(),
        "terminal workflow leaves no active liens"
    );
}
