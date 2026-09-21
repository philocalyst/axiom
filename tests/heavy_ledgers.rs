//! Deterministic integration workloads for the economic core.
//!
//! These are deliberately larger than the constitution smoke tests.  They
//! exercise the public semantic APIs over hundreds of rows/events while
//! preserving exact arithmetic and explicit failure diagnostics.  The source
//! grammar is used where it has a representation; richer evidence, ownership,
//! allocation, and settlement cases use the public semantic APIs because the
//! V0 journal surface does not claim to encode those relations.

use std::collections::BTreeMap;

use axiom_ledger::evidence::{
    AdapterProvenance, Authority, Confidence, CorrectionScope, EvidenceLookup, EvidenceRelation,
    EvidenceStore, ImportBatch, RawEvidence, SourceSpan,
};
use axiom_ledger::exact::ExactNumber;
use axiom_ledger::ir::{Atom, Nominal, NominalKind, Term};
use axiom_ledger::liquidity::{
    ActionEdge, ActionKind, InstrumentNode, LiquidityGraph, NodeId, PositionNode, SearchCompletion,
    SearchLimits,
};
use axiom_ledger::logic::{
    Goal, Literal, Polarity as LogicPolarity, Program, ResourceProfile, SemanticContext, Solver,
};
use axiom_ledger::model::{ContentHash, Date, Quantity, Unit};
use axiom_ledger::ontology::{
    Encumbrance, EncumbranceKind, Endpoint, ExchangeLeg, ExchangeRecord, Instrument,
    InstrumentKind, Obligation, OntologyError, Position, Role, RoleAssignments,
    SatisfactionAllocation, Settlement, SettlementState, TransferRecord, validate_exchange_legs,
    validate_obligation_allocation, validate_remaining_lot_quantities, validate_settlement_states,
    validate_transfer_conservation,
};
use axiom_ledger::parser::{parse_ledger, parse_source};
use axiom_ledger::proof::{Node, Operation, Proof, ProofId};
use axiom_ledger::recognize::{
    AcceptedWorld, BookPolicy, CloseError, CompletenessClaims, FactScope, RecognitionAcceptedFact,
    RecognitionError, ReportingPeriod, close, recognize,
};
use axiom_ledger::scenario::{
    Assumption, BoundedRecurrence, ExpectedEvent, Horizon, RealizedEvent, Scenario,
};
use axiom_ledger::semantics::{
    Completion as SemanticCompletion, Conditional, Conflict, GoalId, Multiplicity, Requirement,
    Resolution, Truth,
};
use axiom_ledger::time::{Frequency, LocalDate, Recurrence};
use axiom_ledger::units::{
    InstrumentUnit, Quantity as UnitQuantity, Quote, QuoteKind, Ratio, ValuationPolicy,
    ValuationStatus, value,
};
use axiom_ledger::workspace::Workspace;

fn date(value: &str) -> Date {
    value.parse().expect("valid fixture date")
}

fn quantity(number: &str, unit: &str) -> Quantity {
    Quantity::with_unit(
        ExactNumber::parse(number).expect("valid exact number"),
        unit,
    )
    .expect("nonzero quantities have explicit units")
}

fn hash(seed: u8) -> ContentHash {
    ContentHash::domain_separated("axiom-heavy-ledger-test", &[seed])
}

fn proof(seed: u8) -> ProofId {
    proof_node(seed).id
}

fn proof_node(seed: u8) -> Node {
    Node::new(
        format!("proof/{seed}"),
        Operation::Observation {
            source: format!("source/{seed}"),
        },
        Vec::new(),
        BTreeMap::new(),
    )
}

fn proof_bundle(ids: impl IntoIterator<Item = ProofId>) -> Proof {
    let mut bundle = Proof::new();
    for id in ids {
        let node = (0..=u8::MAX)
            .map(proof_node)
            .find(|node| node.id == id)
            .expect("test proof id has a corresponding node");
        bundle.insert(node);
        bundle.root(id);
    }
    bundle
}

fn accepted_world(
    source_commit: ContentHash,
    facts: impl IntoIterator<Item = RecognitionAcceptedFact>,
) -> AcceptedWorld {
    let mut bundle = Proof::new();
    let mut checked = Vec::new();
    for fact in facts {
        let node = Node::new(
            format!("accepted fact {}", fact.id()),
            Operation::Observation {
                source: fact.id().to_string(),
            },
            Vec::new(),
            BTreeMap::from([("accepted-fact".to_string(), fact.id().to_string())]),
        );
        let proof = node.id;
        bundle.insert(node);
        bundle.root(proof);
        let rebuilt = match fact.scope() {
            FactScope::Actual => RecognitionAcceptedFact::actual(
                fact.id().clone(),
                fact.kind(),
                fact.occurrence_date(),
                proof,
                fact.authority(),
            ),
            FactScope::Scenario(scenario) => RecognitionAcceptedFact::scenario(
                fact.id().clone(),
                fact.kind(),
                fact.occurrence_date(),
                scenario.into_string(),
                proof,
                fact.authority(),
            ),
        }
        .expect("rebuilt accepted fact is valid")
        .with_attributes(fact.attributes().clone());
        checked.push(match fact.settlement_date() {
            Some(date) => rebuilt.with_settlement_date(date),
            None => rebuilt,
        });
    }
    AcceptedWorld::from_facts_checked(source_commit, checked, bundle)
        .expect("accepted world has a rooted fact proof")
}

fn endpoint(entity: &str) -> Endpoint {
    Endpoint::entity(entity)
}

fn instrument(id: &str, unit: &str, kind: InstrumentKind) -> Instrument {
    Instrument::new(id, kind).denominated(Unit::new(unit).expect("unit"))
}

fn unit(id: &str, instrument: &str) -> InstrumentUnit {
    InstrumentUnit::new(id, instrument)
}

fn uq(amount: &str, unit: InstrumentUnit) -> UnitQuantity {
    UnitQuantity::typed(ExactNumber::parse(amount).expect("exact amount"), unit)
}

fn text_fact(predicate: &str, value: &str) -> Literal {
    Literal::positive(Atom::new(
        Nominal::new(NominalKind::Predicate, predicate),
        vec![Term::Text(value.to_string())],
    ))
}

/// A multi-year source import stays deterministic and keeps every row's
/// occurrence, even when the normalized payload repeats every month.
#[test]
fn multi_year_personal_small_business_evidence_scale() {
    let source = "business-bank";
    let adapter = AdapterProvenance::new("csv-bank", "3.2.1");
    let mut batch = ImportBatch::new(source).with_adapter(adapter.clone());
    for row in 0..1_200_u32 {
        let year = 2020 + row / 200;
        let month = 1 + ((row / 17) % 12) as u8;
        let day = 1 + (row % 27) as u8;
        // Repeated payloads are intentional: content identity is not
        // occurrence identity.
        let payload = format!(
            "date={year:04}-{month:02}-{day:02};amount={};unit=USD",
            100 + row % 11
        );
        let occurrence = format!("bank-row-{row:04}");
        let observation = RawEvidence::from_bytes(
            source,
            occurrence,
            Some(format!("statement-{row:04}").into()),
            payload.as_bytes(),
        )
        .with_provenance(
            axiom_ledger::evidence::Provenance::new(source)
                .with_span(SourceSpan::csv_row(source, row as u64)),
        );
        batch.push(observation);
    }

    let mut store = EvidenceStore::new();
    let report = store
        .import_batch(batch)
        .expect("deterministic batch import");
    assert_eq!(report.items.len(), 1_200);
    assert_eq!(store.len(), 1_200);
    assert_eq!(store.iter().filter(|row| row.is_available()).count(), 1_200);

    // Exact re-import is idempotent, not a replacement and not a duplicate.
    let duplicate = RawEvidence::from_bytes(
        source,
        "bank-row-0042",
        Some("statement-0042".into()),
        b"date=2020-03-16;amount=109;unit=USD",
    )
    .with_provenance(axiom_ledger::evidence::Provenance::new(source).with_adapter(adapter));
    assert!(store.insert(duplicate).is_ok());
    assert_eq!(store.len(), 1_200);

    // A separate source row can be retained as a candidate identity without
    // collapsing the two leaves.
    let left = match store.get(&"bank-row-0042".into()) {
        EvidenceLookup::Unique(evidence) => evidence.identity().clone(),
        EvidenceLookup::Missing => panic!("bank-row-0042 was not imported"),
        EvidenceLookup::Multiple(_) => panic!("bank-row-0042 has ambiguous adapter derivations"),
    };
    let receipt = RawEvidence::from_bytes(
        "receipts",
        "receipt-0042",
        Some("receipt/0042".into()),
        b"date=2020-03-16;amount=109;unit=USD",
    );
    store.insert(receipt.clone()).unwrap();
    let right = receipt.identity().clone();
    let candidate = store
        .propose_identity_link(
            left.clone(),
            right.clone(),
            Confidence::from_percent(97),
            axiom_ledger::evidence::Provenance::new("matcher-v2"),
            Authority::user("reviewer"),
        )
        .unwrap();
    assert!(candidate);
    assert_eq!(store.candidate_links().count(), 1);
    assert!(store.contains(&"bank-row-0042".into()));
}

/// Source corrections retain the old row and are scoped to a field; a
/// correction relation is not an in-place update.
#[test]
fn multi_year_corrections_conflicts_and_tombstones_remain_audit_visible() {
    let mut store = EvidenceStore::new();
    let old = RawEvidence::from_bytes(
        "erp",
        "invoice-0007",
        Some("INV-0007".into()),
        b"total=100 USD",
    );
    let corrected = RawEvidence::from_bytes(
        "erp",
        "invoice-0007-correction",
        Some("INV-0007".into()),
        b"total=130 USD",
    );
    let deleted =
        RawEvidence::deleted_tombstone("erp", "invoice-0008", Some("INV-0008".into()), hash(8))
            .unwrap();
    store.insert(old.clone()).unwrap();
    store.insert(corrected.clone()).unwrap();
    store.insert(deleted).unwrap();
    assert_eq!(store.len(), 3);
    store
        .add_relation(EvidenceRelation::corrects(
            corrected.identity().clone(),
            old.identity().clone(),
            CorrectionScope::field("total")
                .with_source("erp")
                .with_rationale("tax-inclusive total corrected by ERP export"),
        ))
        .unwrap();
    assert_eq!(store.relations().count(), 1);
    match store.get(&"invoice-0008".into()) {
        EvidenceLookup::Unique(evidence) => assert!(evidence.availability().is_tombstone()),
        EvidenceLookup::Missing => panic!("invoice-0008 tombstone was not imported"),
        EvidenceLookup::Multiple(_) => panic!("invoice-0008 has ambiguous adapter derivations"),
    }

    // Two institutional rows disagree, while their conflict and candidate
    // identity hypotheses both remain queryable.
    let bank = RawEvidence::from_bytes(
        "bank-a",
        "payment-a",
        Some("PAY-1".into()),
        b"amount=100 USD",
    );
    let card = RawEvidence::from_bytes(
        "bank-b",
        "payment-b",
        Some("PAY-1".into()),
        b"amount=102 USD",
    );
    store.insert(bank.clone()).unwrap();
    store.insert(card.clone()).unwrap();
    store
        .propose_identity_link(
            bank.identity().clone(),
            card.identity().clone(),
            Confidence::from_percent(81),
            axiom_ledger::evidence::Provenance::new("matcher"),
            Authority::institution("reconciliation"),
        )
        .unwrap();
    store
        .add_relation(EvidenceRelation::possibly_same_as(
            bank.identity().clone(),
            card.identity().clone(),
        ))
        .unwrap();
    assert_eq!(store.candidate_links().count(), 1);
    assert_eq!(store.relations().count(), 2);
}

/// A parser fixture covers the journal subset; invoices, ownership and
/// evidence relations intentionally use semantic APIs because V0 has no
/// surface syntax for them.
#[test]
fn multi_year_fixture_parses_journal_subset() {
    let source = include_str!("../fixtures/heavy/multi_year_small_business.axm");
    let surface = parse_source(source).expect("checked-in heavy fixture preserves its surface");
    assert_eq!(surface.book.name.as_known(), Some("management-2020-2025"));
    let ledger = parse_ledger(source).expect("checked-in heavy fixture strictly elaborates");
    assert_eq!(ledger.book.as_str(), "management-2020-2025");
    assert!(ledger.forms.len() >= 10);

    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("fixtures/heavy/multi-year", source)
        .expect("fixture enters the immutable source store");
    let result = workspace
        .analyze_commit(loaded.commit_id())
        .expect("fixture reaches proof-producing production analysis");
    result
        .analysis
        .check_proof()
        .expect("fixture produces a valid proof graph");
}

#[test]
fn multi_currency_business_exchange_conserves_each_instrument() {
    let usd = instrument("USD", "USD", InstrumentKind::Currency);
    let eur = instrument("EUR", "EUR", InstrumentKind::Currency);
    let good = ExchangeRecord::new(
        "fx-2025-06-01",
        vec![
            ExchangeLeg::give(
                endpoint("company"),
                endpoint("dealer"),
                "USD",
                quantity("1080", "USD"),
            ),
            ExchangeLeg::receive(
                endpoint("dealer"),
                endpoint("company"),
                "EUR",
                quantity("1000", "EUR"),
            ),
        ],
    );
    assert!(
        good.validate_with_instruments(&[usd.clone(), eur.clone()])
            .is_ok()
    );

    // A same-instrument give/receive mismatch is rejected exactly; there is
    // no implicit balancing currency or float conversion.
    let bad = ExchangeRecord::new(
        "fx-bad",
        vec![
            ExchangeLeg::give(
                endpoint("company"),
                endpoint("dealer"),
                "USD",
                quantity("100", "USD"),
            ),
            ExchangeLeg::receive(
                endpoint("dealer"),
                endpoint("company"),
                "USD",
                quantity("99", "USD"),
            ),
        ],
    );
    assert!(matches!(
        validate_exchange_legs(&bad),
        Err(OntologyError::ExchangeNotConserved { .. })
    ));
}

#[test]
fn brokerage_lots_and_time_indexed_quotes_keep_ambiguity_exact() {
    let share = instrument("ABC", "share", InstrumentKind::Equity);
    let lot_a = axiom_ledger::ontology::Lot::new("lot-a", "ABC", quantity("100", "share"), "buy-a")
        .unwrap()
        .with_basis(quantity("1000", "USD"));
    let lot_b = axiom_ledger::ontology::Lot::new("lot-b", "ABC", quantity("100", "share"), "buy-b")
        .unwrap()
        .with_basis(quantity("1200", "USD"));
    let sale = axiom_ledger::ontology::DisposeRecord::new(
        "sell-2025",
        endpoint("brokerage"),
        "ABC",
        quantity("100", "share"),
    )
    .from_lot("lot-a")
    .for_proceeds(quantity("1500", "USD"));
    validate_remaining_lot_quantities(&[lot_a.clone(), lot_b], &[sale]).unwrap();
    assert!(lot_a.consumed(&quantity("101", "share")).is_err());
    assert!(matches!(
        lot_a.consumed(&quantity("1", "USD")),
        Err(OntologyError::UnitMismatch { .. })
    ));

    let share_unit = unit("share", "ABC");
    let usd_unit = unit("USD", "USD");
    let quotes = vec![
        Quote::new(
            "quote-a",
            Ratio::new(
                share_unit.clone(),
                usd_unit.clone(),
                ExactNumber::parse("15").unwrap(),
            ),
            QuoteKind::Close,
            axiom_ledger::time::Instant::EPOCH,
            axiom_ledger::time::Instant::EPOCH,
            "primary",
            "statement",
            axiom_ledger::time::InstantInterval::closed(
                axiom_ledger::time::Instant::EPOCH,
                axiom_ledger::time::Instant::from_unix_seconds(86_400),
            )
            .unwrap(),
        ),
        Quote::new(
            "quote-b",
            Ratio::new(
                share_unit.clone(),
                usd_unit.clone(),
                ExactNumber::parse("16").unwrap(),
            ),
            QuoteKind::Close,
            axiom_ledger::time::Instant::EPOCH,
            axiom_ledger::time::Instant::EPOCH,
            "secondary",
            "statement",
            axiom_ledger::time::InstantInterval::closed(
                axiom_ledger::time::Instant::EPOCH,
                axiom_ledger::time::Instant::from_unix_seconds(86_400),
            )
            .unwrap(),
        ),
    ];
    let valuation = value(
        &uq("100", share_unit),
        &usd_unit,
        axiom_ledger::time::Instant::EPOCH,
        &quotes,
        ValuationPolicy::default(),
    )
    .unwrap();
    assert_eq!(valuation.status, ValuationStatus::Ambiguous);
    assert!(!valuation.is_unique());
    assert_eq!(share.id.as_str(), "ABC");
}

#[test]
fn invoice_payment_allocation_rejects_double_spend_and_preserves_bounce() {
    let invoice_a = Obligation::transfer(
        "invoice-a",
        "customer",
        "vendor",
        "USD",
        quantity("100", "USD"),
    )
    .unwrap();
    let _invoice_b = Obligation::transfer(
        "invoice-b",
        "customer",
        "vendor",
        "USD",
        quantity("100", "USD"),
    )
    .unwrap();
    let mut payment = Settlement::new(
        "payment-1",
        endpoint("customer"),
        endpoint("vendor"),
        "USD",
        quantity("100", "USD"),
    )
    .unwrap();
    payment
        .transition(SettlementState::Presented, Some(date("2025-02-02")), None)
        .unwrap();
    payment
        .transition(SettlementState::Settled, Some(date("2025-02-03")), None)
        .unwrap();
    let first = SatisfactionAllocation::new(
        "allocation-a",
        "invoice-a",
        "payment-1",
        quantity("100", "USD"),
    )
    .unwrap()
    .applied();
    let second = SatisfactionAllocation::new(
        "allocation-b",
        "invoice-b",
        "payment-1",
        quantity("100", "USD"),
    )
    .unwrap()
    .applied();
    assert!(matches!(
        validate_obligation_allocation(&invoice_a, &[first.clone(), second], &[payment.clone()]),
        Err(OntologyError::SettlementOverallocated { .. })
    ));

    payment
        .returned(Some(date("2025-02-05")), "insufficient funds")
        .unwrap();
    assert!(!payment.is_effective());
    assert_eq!(
        invoice_a.remaining(&[first], &[payment]).unwrap(),
        quantity("100", "USD")
    );
}

#[test]
fn ownership_and_restrictions_scale_without_collapsing_holders() {
    let roles = RoleAssignments::joint(
        "operating-company",
        Role::BeneficialOwner,
        vec!["alice".into(), "bob".into(), "carol".into()],
    )
    .unwrap();
    roles.validate().unwrap();
    assert_eq!(
        roles
            .holders(&"operating-company".into(), &Role::BeneficialOwner)
            .count(),
        3
    );

    let hold = Encumbrance::for_quantity(
        "payroll-hold",
        EncumbranceKind::Hold,
        quantity("400", "USD"),
    )
    .unwrap()
    .for_beneficiary("employees")
    .reason("next payroll");
    let position = Position::new(
        "checking-position",
        "operating-company",
        "USD",
        quantity("1000", "USD"),
    )
    .unwrap()
    .with_encumbrance("payroll-hold");
    let mut encumbrances = BTreeMap::new();
    encumbrances.insert(hold.id.clone(), hold.clone());
    assert_eq!(
        position.available_quantity(&encumbrances).unwrap(),
        quantity("600", "USD")
    );
    assert!(position.is_restricted(&encumbrances).unwrap());
    encumbrances.insert(hold.id.clone(), hold.release());
    assert_eq!(
        position.available_quantity(&encumbrances).unwrap(),
        quantity("1000", "USD")
    );
}

#[test]
fn settlement_state_machine_reports_illegal_transition_precisely() {
    let mut settlement = Settlement::new(
        "wire-1",
        endpoint("a"),
        endpoint("b"),
        "USD",
        quantity("50", "USD"),
    )
    .unwrap();
    let direct = settlement.transition(SettlementState::Settled, Some(date("2025-01-01")), None);
    assert!(matches!(
        direct,
        Err(OntologyError::InvalidSettlementTransition {
            from: Some(SettlementState::Issued),
            to: SettlementState::Settled
        })
    ));
    assert!(matches!(
        validate_settlement_states(&[SettlementState::Issued, SettlementState::Settled]),
        Err(OntologyError::InvalidSettlementTransition { .. })
    ));
}

#[test]
fn one_invalid_row_invalidates_only_the_batch_validation_and_not_prior_graph() {
    let mut transfers = Vec::with_capacity(501);
    for row in 0..500 {
        transfers.push(TransferRecord::between(
            format!("transfer-{row:04}"),
            endpoint("cash"),
            endpoint("merchant"),
            "USD",
            quantity("1", "USD"),
        ));
    }
    transfers.push(TransferRecord::new(
        "transfer-invalid",
        "USD",
        vec![axiom_ledger::ontology::TransferLeg::new(
            endpoint("cash"),
            quantity("1", "USD"),
        )],
        vec![axiom_ledger::ontology::TransferLeg::new(
            endpoint("merchant"),
            quantity("2", "USD"),
        )],
    ));
    assert!(matches!(
        validate_transfer_conservation(&transfers),
        Err(OntologyError::TransferNotConserved { .. })
    ));
    let mut graph = axiom_ledger::ontology::EventGraph::new();
    for transfer in transfers.iter().take(500) {
        graph.insert(transfer.clone()).unwrap();
    }
    assert_eq!(graph.len(), 500);
    assert!(matches!(
        graph.insert(transfers[500].clone()),
        Err(OntologyError::TransferNotConserved { .. })
    ));
    assert_eq!(graph.len(), 500);
}

#[test]
fn ambiguity_and_conflict_coexist_as_first_class_resolution_state() {
    let subject = GoalId::new(hash(61)).unwrap();
    let conflict = Conflict::new(subject, vec![proof(62)], vec![proof(63)]).unwrap();
    let proof_bundle = proof_bundle([proof(62), proof(63)]);
    let resolution = Resolution::new_checked(
        &proof_bundle,
        vec![proof(62)],
        vec![proof(63)],
        Multiplicity::multiple(vec![
            Conditional::new("lot-a", vec![Requirement::Evidence(hash(64))]),
            Conditional::new("lot-b", vec![Requirement::Evidence(hash(65))]),
        ])
        .unwrap(),
        SemanticCompletion::Complete,
        vec![
            Requirement::Evidence(hash(64)),
            Requirement::Evidence(hash(65)),
        ],
        vec![conflict],
        vec![],
    )
    .unwrap();
    assert_eq!(resolution.truth(), Truth::Both);
    assert!(resolution.is_ambiguous());
    assert!(resolution.is_blocked());
    assert_eq!(resolution.conflicts().len(), 1);
}

#[test]
fn recognition_close_reports_out_of_period_missing_completeness_and_scenario_leakage() {
    let old = RecognitionAcceptedFact::actual("old", "sale", date("2019-12-31"), proof(70), "bank")
        .unwrap();
    let actual =
        RecognitionAcceptedFact::actual("actual", "sale", date("2025-06-30"), proof(71), "bank")
            .unwrap();
    let hypothetical = RecognitionAcceptedFact::scenario(
        "forecast",
        "sale",
        date("2025-07-01"),
        "base-case",
        proof(72),
        "planner",
    )
    .unwrap();
    let dated_world = accepted_world(hash(73), vec![old.clone(), actual.clone()]);
    let policy = BookPolicy::new("tax", date("2020-01-01"), Some(date("2025-12-31")));
    assert!(matches!(
        recognize(&dated_world, &policy),
        Err(RecognitionError::PolicyNotEffective { fact, .. }) if fact == *old.id()
    ));

    let actual_world = accepted_world(hash(74), vec![actual.clone()]);
    let period = ReportingPeriod::new(date("2025-01-01"), date("2025-12-31"));
    let missing = close(
        &actual_world,
        axiom_ledger::recognize::CloseRequest::new(actual_world.commit(), [policy.clone()], period),
    );
    assert_eq!(missing, Err(CloseError::CompletenessNotSatisfied));
    let closed = close(
        &actual_world,
        axiom_ledger::recognize::CloseRequest::new(actual_world.commit(), [policy], period)
            .with_scope("bank-feed-2025")
            .with_completeness(
                CompletenessClaims::new()
                    .claim_scoped(
                        "bank-feed-2025",
                        true,
                        actual_world.commit(),
                        "bank-feed-2025",
                        period,
                        proof(73),
                        hash(76),
                    )
                    .with_evidence("bank-feed-2025", "bank import complete"),
            ),
    )
    .unwrap();
    assert_eq!(closed.recognized.iter().count(), 1);
    assert!(matches!(
        recognize(
            &accepted_world(hash(75), vec![actual, hypothetical]),
            &BookPolicy::new("tax", date("2020-01-01"), Some(date("2030-12-31"))),
        ),
        Err(RecognitionError::ScenarioFact { .. })
    ));
}

#[test]
fn scenarios_and_liquidity_do_not_leak_into_actual_and_resource_boundary_is_incomplete() {
    let root = hash(80);
    let mut scenario = Scenario::new("base-case", root).unwrap();
    scenario
        .assume(Assumption::text("customer-pays", "net-30").unwrap())
        .unwrap();
    let recurrence = Recurrence::new(
        LocalDate::new(2025, 1, 1).unwrap(),
        Frequency::Monthly { every: 1, day: 1 },
    )
    .unwrap()
    .with_count(24);
    let bounded = BoundedRecurrence::new(
        recurrence,
        Horizon::new(date("2025-01-01"), date("2026-12-31")).unwrap(),
    )
    .unwrap();
    scenario
        .expect(
            ExpectedEvent::new("rent")
                .with_quantity(quantity("2000", "USD"))
                .recurring(bounded),
        )
        .unwrap();
    let materialized = scenario
        .materialize(Horizon::new(date("2025-01-01"), date("2026-12-31")).unwrap())
        .unwrap();
    assert_eq!(materialized.len(), 24);
    let realized = RealizedEvent::new("rent-actual-1", date("2025-01-02"))
        .with_quantity(quantity("2000", "USD"));
    scenario.register_actual_event(realized.clone()).unwrap();
    let linked = scenario.link_realized_occurrence(materialized[0].identity(), realized);
    assert!(linked.is_ok(), "scenario link failed: {linked:?}");
    assert_eq!(scenario.actual_root(), root);
    assert_eq!(scenario.links().count(), 1);

    let mut graph = LiquidityGraph::new();
    let cash = NodeId::position("cash");
    let usd = NodeId::instrument("USD");
    graph
        .add_position(PositionNode::new("cash", "USD", quantity("10000", "USD")).grant("withdraw"))
        .unwrap();
    graph.add_instrument(InstrumentNode::new("USD")).unwrap();
    graph
        .add_edge(
            ActionEdge::new("withdraw", cash.clone(), usd.clone(), ActionKind::Withdraw)
                .requires_permission("withdraw"),
        )
        .unwrap();
    let routes = graph
        .find_routes(
            cash.clone(),
            usd.clone(),
            &quantity("1000", "USD"),
            SearchLimits {
                max_expansions: 0,
                max_depth: 8,
            },
        )
        .unwrap();
    assert_eq!(
        routes.completion,
        SearchCompletion::Incomplete {
            expanded: 0,
            limit: 0
        }
    );
    assert!(routes.is_incomplete());

    let absent = Goal::default_not(Goal::atom(text_fact("bank-row", "missing")));
    let mut solver = Solver::new();
    let open = solver.solve(&Program::new(), &absent, &SemanticContext::default());
    assert_eq!(
        open.completion(),
        axiom_ledger::logic::Completion::OpenWorld
    );
    let complete = solver.solve(
        &Program::new(),
        &absent,
        &SemanticContext::default().complete_relation("bank-row", 1, LogicPolarity::Positive),
    );
    assert_eq!(complete.truth(), axiom_ledger::logic::Truth::TrueOnly);
    let limited = solver.solve(
        &Program::new(),
        &Goal::atom(text_fact("bank-row", "missing")),
        &SemanticContext::default().with_resources(ResourceProfile::bounded(0)),
    );
    assert_eq!(
        limited.completion(),
        axiom_ledger::logic::Completion::ResourceLimited
    );
    assert!(limited.is_incomplete());
}

/// Larger than the default workload, but intentionally excluded from normal
/// CI.  It is useful for profiling import ordering and hash/index pressure.
#[test]
#[ignore = "deterministic stress profile; run explicitly"]
fn stress_ten_thousand_evidence_rows() {
    let mut store = EvidenceStore::new();
    for row in 0..10_000_u32 {
        store
            .insert(RawEvidence::from_bytes(
                "stress-bank",
                format!("row-{row:05}"),
                Some(format!("external-{row:05}").into()),
                format!("amount={};unit=USD", row % 997 + 1).as_bytes(),
            ))
            .unwrap();
    }
    assert_eq!(store.len(), 10_000);
}
