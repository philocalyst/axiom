//! Adversarial checks for the small cross-record ontology invariants.

use axiom_ledger::exact::ExactNumber;
use axiom_ledger::model::Quantity;
use axiom_ledger::ontology::{
    Endpoint, ExchangeLeg, ExchangeRecord, Obligation, OntologyError, SatisfactionAllocation,
    Settlement, SettlementState, validate_exchange_legs, validate_satisfaction_network,
};

fn quantity(number: &str, unit: &str) -> Quantity {
    Quantity::with_unit(ExactNumber::parse(number).unwrap(), unit).unwrap()
}

fn endpoint(entity: &str, account: &str) -> Endpoint {
    Endpoint::entity(entity).at_account(account)
}

fn effective_settlement(id: &str, amount: Quantity) -> Settlement {
    let mut settlement = Settlement::new(
        id,
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        amount,
    )
    .unwrap();
    settlement
        .transition(SettlementState::Presented, None, None)
        .unwrap();
    settlement
        .transition(SettlementState::Settled, None, None)
        .unwrap();
    settlement
}

#[test]
fn exchange_checks_explicit_same_instrument_totals_at_party_boundary() {
    // The payer uses two different accounts. Closure is a party-level fact,
    // while per-instrument give/receive totals remain exact.
    let balanced = ExchangeRecord::new(
        "exchange/balanced",
        vec![
            ExchangeLeg::give(
                endpoint("trader", "checking"),
                Endpoint::entity("venue"),
                "currency/USD",
                quantity("10", "USD"),
            ),
            ExchangeLeg::receive(
                Endpoint::entity("venue"),
                endpoint("trader", "brokerage"),
                "currency/USD",
                quantity("10", "USD"),
            ),
        ],
    );
    validate_exchange_legs(&balanced).unwrap();

    let unbalanced = ExchangeRecord::new(
        "exchange/unbalanced",
        vec![
            ExchangeLeg::give(
                endpoint("trader", "checking"),
                Endpoint::entity("venue"),
                "currency/USD",
                quantity("10", "USD"),
            ),
            ExchangeLeg::receive(
                Endpoint::entity("venue"),
                endpoint("trader", "brokerage"),
                "currency/USD",
                quantity("9", "USD"),
            ),
        ],
    );
    assert!(matches!(
        validate_exchange_legs(&unbalanced),
        Err(OntologyError::ExchangeNotConserved { instrument, .. })
            if instrument.as_str() == "currency/USD"
    ));
}

#[test]
fn returned_settlement_capacity_is_still_global_across_obligations() {
    let first = Obligation::transfer(
        "invoice/one",
        "payer",
        "merchant",
        "USD",
        quantity("60", "USD"),
    )
    .unwrap();
    let second = Obligation::transfer(
        "invoice/two",
        "payer",
        "merchant",
        "USD",
        quantity("60", "USD"),
    )
    .unwrap();
    let mut payment = effective_settlement("payment/returned", quantity("100", "USD"));
    payment.returned(None, "bounced").unwrap();
    let allocations = vec![
        SatisfactionAllocation::new(
            "allocation/one",
            first.id.clone(),
            payment.id.clone(),
            quantity("60", "USD"),
        )
        .unwrap()
        .applied(),
        SatisfactionAllocation::new(
            "allocation/two",
            second.id.clone(),
            payment.id.clone(),
            quantity("60", "USD"),
        )
        .unwrap()
        .applied(),
    ];

    assert!(matches!(
        validate_satisfaction_network(&[first, second], &[payment], &allocations),
        Err(OntologyError::SettlementOverallocated { settlement, .. })
            if settlement.as_str() == "payment/returned"
    ));
}

#[test]
fn satisfaction_rejects_unit_and_party_mismatch_even_before_effective_state() {
    let obligation = Obligation::transfer(
        "invoice/unit",
        "payer",
        "merchant",
        "USD",
        quantity("10", "USD"),
    )
    .unwrap();
    let payment = Settlement::new(
        "payment/unit",
        Endpoint::entity("payer"),
        Endpoint::entity("merchant"),
        "USD",
        quantity("10", "USD"),
    )
    .unwrap();
    let wrong_unit = SatisfactionAllocation::new(
        "allocation/wrong-unit",
        obligation.id.clone(),
        payment.id.clone(),
        quantity("10", "EUR"),
    )
    .unwrap();
    assert!(matches!(
        validate_satisfaction_network(
            std::slice::from_ref(&obligation),
            std::slice::from_ref(&payment),
            &[wrong_unit],
        ),
        Err(OntologyError::UnitMismatch { .. })
    ));

    let wrong_party_payment = Settlement::new(
        "payment/party",
        Endpoint::entity("other-payer"),
        Endpoint::entity("merchant"),
        "USD",
        quantity("10", "USD"),
    )
    .unwrap();
    let wrong_party = SatisfactionAllocation::new(
        "allocation/wrong-party",
        obligation.id.clone(),
        wrong_party_payment.id.clone(),
        quantity("10", "USD"),
    )
    .unwrap();
    assert_eq!(
        validate_satisfaction_network(
            std::slice::from_ref(&obligation),
            std::slice::from_ref(&wrong_party_payment),
            &[wrong_party],
        ),
        Err(OntologyError::AllocationMismatch)
    );
}
