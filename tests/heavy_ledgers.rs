//! Deterministic integration workloads for the economic core.
//!
//! These are deliberately larger than the constitution smoke tests.  They
//! exercise the public semantic APIs over hundreds of rows/events while
//! preserving exact arithmetic and explicit failure diagnostics.  The source
//! grammar is used where it has a representation; richer evidence, ownership,
//! allocation, and settlement cases use the public semantic APIs because the
//! V0 journal surface does not claim to encode those relations.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Instant;

use axiom_ledger::engine::{IssueCode, ObligationStatus, ObservationStatus, RecognitionStatus};
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
use axiom_ledger::model::{
    ContentHash, Date, Quantity, SatisfactionState, SettlementStateKind, Unit,
};
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

/// Build a source-ledger fixture with one obligation, one settlement history,
/// and one applied satisfaction allocation per row.  The IDs deliberately
/// carry stable zero-padded ordinals: this makes source order, first/last key
/// checks, and proof identities reproducible while keeping the rows realistic
/// enough to exercise the same parser path as imported receivables.
fn obligation_settlement_fixture(rows: usize) -> String {
    let mut source = String::with_capacity(rows.saturating_mul(420) + 32);
    source.push_str("book receivables\n");
    for row in 0..rows {
        writeln!(
            source,
            "obligation invoice/{row:05}\n  debtor customer/{row:05}\n  creditor vendor/{row:05}\n  performance transfer 100 USD\n  due 2026-12-31"
        )
        .expect("writing obligation fixture");
    }
    for row in 0..rows {
        writeln!(
            source,
            "settlement payment/{row:05}\n  kind ach\n  from customer/{row:05}\n  to vendor/{row:05}\n  amount 100 USD\n  state issued at 2026-01-01\n  state presented at 2026-01-02\n  state pending at 2026-01-03\n  state settled at 2026-01-04"
        )
        .expect("writing settlement fixture");
    }
    for row in 0..rows {
        writeln!(
            source,
            "satisfy allocation/{row:05}\n  obligation invoice/{row:05}\n  settlement payment/{row:05}\n  amount 100 USD\n  state applied"
        )
        .expect("writing satisfaction fixture");
    }
    source
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

/// The authored source form is also exercised end-to-end on a small checked-in
/// fixture.  Keeping this case out of the stress test makes ordinary CI cover
/// the complete obligation/settlement/satisfaction path and its proof binding.
#[test]
fn obligation_settlement_satisfaction_fixture_is_deterministic() {
    let source = include_str!("../fixtures/heavy/obligation_settlement_satisfaction.axm");
    let surface = parse_source(source).expect("ontology fixture preserves its source surface");
    assert_eq!(surface.statements.len(), 9);
    let ledger = parse_ledger(source).expect("ontology fixture strictly elaborates");
    assert_eq!(ledger.forms.len(), 9);

    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("fixtures/heavy/obligation-settlement-satisfaction", source)
        .expect("ontology fixture enters the immutable source store");
    let first = workspace
        .analyze_commit(loaded.commit_id())
        .expect("ontology fixture reaches production analysis")
        .analysis;
    first
        .check_proof()
        .expect("ontology fixture proof graph checks independently");

    assert_eq!(
        first
            .obligations
            .iter()
            .map(|obligation| obligation.id.as_str())
            .collect::<Vec<_>>(),
        ["invoice/00001", "invoice/00002", "invoice/00003"]
    );
    assert!(first.obligations.iter().all(|obligation| {
        obligation.status == ObligationStatus::Satisfied
            && obligation
                .remaining
                .as_ref()
                .is_some_and(|remaining| remaining.is_zero())
    }));
    assert!(first.settlement_histories.iter().all(|settlement| {
        settlement.current == SettlementStateKind::Settled
            && settlement.effective
            && settlement
                .unused
                .as_ref()
                .is_some_and(|unused| unused.is_zero())
    }));
    assert!(first.satisfactions.iter().all(|satisfaction| {
        satisfaction.state == SatisfactionState::Applied && satisfaction.effective
    }));
    assert!(
        first.issues.is_empty(),
        "fixture issues: {:?}",
        first.issues
    );

    // Re-evaluating the same immutable source must retain stable key results;
    // this guards against traversal-order-dependent proof or view assembly.
    let second = workspace
        .analyze_commit(loaded.commit_id())
        .expect("re-evaluation remains deterministic")
        .analysis;
    assert_eq!(
        first
            .obligations
            .iter()
            .map(|obligation| (&obligation.id, obligation.proof))
            .collect::<Vec<_>>(),
        second
            .obligations
            .iter()
            .map(|obligation| (&obligation.id, obligation.proof))
            .collect::<Vec<_>>()
    );
    assert_eq!(first.proof.roots, second.proof.roots);
    assert_eq!(first.proof.nodes.len(), second.proof.nodes.len());
}

/// A 10,000-row source-ledger profile for the authored ontology forms.  It is
/// ignored because the proof DAG and exact arithmetic make this a profiling
/// workload rather than a normal unit test, but the assertions intentionally
/// cover parsing, analysis, proof checking, stable keys, and a generous wall
/// bound that catches accidental quadratic scans.
#[test]
#[ignore = "deterministic 10,000-row source-ledger profile; run explicitly"]
fn stress_ten_thousand_obligations_settlements_and_satisfactions() {
    const ROWS: usize = 10_000;
    let started = Instant::now();
    let source = obligation_settlement_fixture(ROWS);
    let parsed = parse_ledger(&source).expect("10,000-row ontology source parses");
    assert_eq!(parsed.forms.len(), ROWS * 3);
    assert!(
        parsed
            .forms
            .first()
            .is_some_and(|form| { matches!(form, axiom_ledger::model::LedgerForm::Obligation(_)) })
    );
    assert!(
        parsed
            .forms
            .get(ROWS)
            .is_some_and(|form| { matches!(form, axiom_ledger::model::LedgerForm::Settlement(_)) })
    );
    assert!(
        parsed.forms.last().is_some_and(|form| {
            matches!(form, axiom_ledger::model::LedgerForm::Satisfaction(_))
        })
    );

    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source(
            "fixtures/heavy/obligation-settlement-satisfaction-10k",
            &source,
        )
        .expect("10,000-row ontology source enters immutable storage");
    let analyzed = workspace
        .analyze_commit(loaded.commit_id())
        .expect("10,000-row ontology source reaches production analysis");
    let analysis = analyzed.analysis;
    assert_eq!(analysis.obligations.len(), ROWS);
    assert_eq!(analysis.settlement_histories.len(), ROWS);
    assert_eq!(analysis.satisfactions.len(), ROWS);
    assert_eq!(analysis.obligations.first().unwrap().id, "invoice/00000");
    assert_eq!(analysis.obligations.last().unwrap().id, "invoice/09999");
    assert_eq!(
        analysis.settlement_histories.first().unwrap().id,
        "payment/00000"
    );
    assert_eq!(
        analysis.settlement_histories.last().unwrap().id,
        "payment/09999"
    );
    assert_eq!(
        analysis.satisfactions.first().unwrap().id,
        "allocation/00000"
    );
    assert_eq!(
        analysis.satisfactions.last().unwrap().id,
        "allocation/09999"
    );
    assert!(analysis.obligations.iter().all(|obligation| {
        obligation.status == ObligationStatus::Satisfied
            && obligation
                .remaining
                .as_ref()
                .is_some_and(|remaining| remaining.is_zero())
    }));
    assert!(
        analysis
            .settlement_histories
            .iter()
            .all(|settlement| settlement.effective)
    );
    assert!(
        analysis
            .satisfactions
            .iter()
            .all(
                |satisfaction| satisfaction.state == SatisfactionState::Applied
                    && satisfaction.effective
            )
    );
    assert!(
        analysis.issues.is_empty(),
        "stress fixture issues: {:?}",
        analysis.issues
    );
    analysis
        .check_proof()
        .expect("10,000-row ontology proof graph checks independently");

    // This is deliberately a very generous bound for slower CI hosts.  It is
    // not a benchmark target; it only rejects an accidental all-pairs pass
    // that would turn this 30,001-form fixture into an unbounded test.
    let limit = if cfg!(debug_assertions) { 180 } else { 60 };
    assert!(
        started.elapsed().as_secs() < limit,
        "10,000-row ontology fixture took {:?} (limit {limit}s); likely quadratic",
        started.elapsed()
    );
}

/// A single source-ledger workload exercises the production path from strict
/// parsing, through immutable source commits, to exact lot recognition.  The
/// smaller differential corpus checks these rules independently; keeping this
/// fixture here makes their interaction (and proof binding) auditable in one
/// realistic ledger.
#[test]
fn heavy_ledger_allocates_lots_and_preserves_resolution_conflicts() {
    const BASE: &str = r#"book tax-us

buy lot/one on 2026-01-01
  10 ABC into brokerage
  for 200 USD

buy lot/two on 2026-01-02
  10 ABC into brokerage
  for 300 USD

buy lot/three on 2026-01-03
  5 ABC into brokerage
  for 125 USD

sell sale/multi on 2026-02-01
  15 ABC from brokerage
  for 600 USD
  lot ?lot

sell sale/next on 2026-02-02
  3 ABC from brokerage
  for 150 USD
  lot ?lot

sell sale/over on 2026-02-03
  10 ABC from brokerage
  for 500 USD
  lot ?lot

use lots/fifo for tax-us
observe position brokerage 7 ABC
observe settlement sale/multi 600 USD into checking
observe settlement sale/next 150 USD into checking
"#;

    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("fixtures/heavy/complex-ledger", BASE)
        .expect("heavy source enters immutable workspace");
    let result = workspace
        .analyze_commit(loaded.commit_id())
        .expect("heavy source reaches proof-producing analysis");
    let analysis = &result.analysis;

    assert_eq!(analysis.lots.len(), 3);
    assert_eq!(analysis.sales.len(), 3);
    assert_eq!(
        analysis.sale("sale/multi").unwrap().selected_lots,
        ["lot/one", "lot/two"]
    );
    assert_eq!(analysis.sale("sale/multi").unwrap().allocations.len(), 2);
    assert_eq!(
        analysis
            .sale("sale/multi")
            .unwrap()
            .allocations
            .iter()
            .map(|allocation| (
                allocation.lot_id.as_str(),
                allocation.quantity.canonical(),
                allocation.basis.canonical(),
                allocation.gain.canonical()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "lot/one",
                "10 ABC".to_owned(),
                "200 USD".to_owned(),
                "200 USD".to_owned(),
            ),
            (
                "lot/two",
                "5 ABC".to_owned(),
                "150 USD".to_owned(),
                "50 USD".to_owned(),
            ),
        ]
    );
    assert_eq!(
        analysis
            .recognized_gain("sale/multi")
            .unwrap()
            .gain
            .canonical(),
        "250 USD"
    );
    assert_eq!(
        analysis.sale("sale/next").unwrap().selected_lot.as_deref(),
        Some("lot/two")
    );
    assert_eq!(
        analysis.sale("sale/next").unwrap().allocations[0]
            .quantity
            .canonical(),
        "3 ABC"
    );
    assert!(matches!(
        analysis.sale("sale/over").unwrap().status,
        RecognitionStatus::MissingLot
    ));
    assert!(analysis.issues.iter().any(|issue| {
        issue.code == IssueCode::InsufficientInventory && issue.sale.as_deref() == Some("sale/over")
    }));

    // The observed position and both settlements are independently
    // reproduced by the recognized events.  Only recognized sales reduce the
    // position, so the over-consumption attempt does not forge a balance.
    assert_eq!(analysis.positions.len(), 1);
    assert_eq!(analysis.positions[0].status, ObservationStatus::Reconciled);
    assert!(
        analysis
            .settlements
            .iter()
            .all(|settlement| settlement.status == ObservationStatus::Reconciled)
    );
    assert_eq!(analysis.journal.len(), 2);
    assert!(analysis.journal.iter().all(|entry| entry.balanced()));
    analysis
        .check_proof()
        .expect("untampered heavy proof checks");

    // Two same-day sales compete for the same inventory.  The engine keeps
    // both unresolved instead of allowing source ordering to choose a winner.
    const SAME_DAY: &str = r#"book tax-us
buy lot/alpha on 2026-01-01
  5 ABC into brokerage
  for 100 USD
buy lot/beta on 2026-01-01
  5 ABC into brokerage
  for 120 USD
sell same/a on 2026-02-01
  5 ABC from brokerage
  for 300 USD
  lot ?lot
sell same/b on 2026-02-01
  5 ABC from brokerage
  for 330 USD
  lot ?lot
"#;
    let mut ambiguous_workspace = Workspace::new();
    let ambiguous_source = ambiguous_workspace
        .load_source("fixtures/heavy/same-day-ambiguous", SAME_DAY)
        .expect("same-day source enters immutable workspace");
    let ambiguous = ambiguous_workspace
        .analyze_commit(ambiguous_source.commit_id())
        .expect("same-day source reaches analysis");
    assert_eq!(ambiguous.analysis.sales.len(), 2);
    assert!(
        ambiguous
            .analysis
            .sales
            .iter()
            .all(|sale| matches!(sale.status, RecognitionStatus::Ambiguous { .. }))
    );
    assert_eq!(
        ambiguous
            .analysis
            .issues
            .iter()
            .filter(|issue| issue.code == IssueCode::AmbiguousLot)
            .count(),
        2
    );

    // Distinct user decisions make the same-day allocations independent.
    let decided_source_text =
        format!("{SAME_DAY}decide same/a lot lot/alpha\ndecide same/b lot lot/beta\n");
    let mut decided_workspace = Workspace::new();
    let decided_source = decided_workspace
        .load_source("fixtures/heavy/same-day-decided", decided_source_text)
        .expect("decided source enters immutable workspace");
    let decided = decided_workspace
        .analyze_commit(decided_source.commit_id())
        .expect("decided source reaches analysis");
    assert_eq!(decided.analysis.sales.len(), 2);
    assert!(decided.analysis.sales.iter().all(|sale| {
        matches!(sale.status, RecognitionStatus::Recognized) && sale.allocations.len() == 1
    }));
    assert_eq!(
        decided.analysis.decisions,
        vec!["lot/alpha".to_owned(), "lot/beta".to_owned()]
    );
    assert!(!decided.analysis.blocked());
    decided
        .analysis
        .check_proof()
        .expect("decisions are included in the proof graph");

    // A contradictory position and a contradictory settlement remain visible
    // as evidence conflicts; neither observation silently wins by row order.
    let conflict_source_text = format!(
        "{BASE}observe position brokerage 8 ABC\nobserve settlement sale/multi 599 USD into checking\n"
    );
    let mut conflict_workspace = Workspace::new();
    let conflict_source = conflict_workspace
        .load_source(
            "fixtures/heavy/conflicting-observations",
            conflict_source_text,
        )
        .expect("conflicting source enters immutable workspace");
    let conflict = conflict_workspace
        .analyze_commit(conflict_source.commit_id())
        .expect("conflicting source reaches analysis");
    assert_eq!(conflict.analysis.positions.len(), 2);
    assert_eq!(
        conflict
            .analysis
            .settlements
            .iter()
            .filter(|settlement| settlement.reference == "sale/multi")
            .count(),
        2
    );
    assert!(
        conflict
            .analysis
            .positions
            .iter()
            .all(|position| { position.status == ObservationStatus::Conflict })
    );
    assert!(
        conflict
            .analysis
            .settlements
            .iter()
            .filter(|settlement| settlement.reference == "sale/multi")
            .all(|settlement| { settlement.status == ObservationStatus::Conflict })
    );
    assert!(
        conflict
            .analysis
            .issues
            .iter()
            .any(|issue| { issue.code == IssueCode::PositionConflict })
    );
    assert!(
        conflict
            .analysis
            .issues
            .iter()
            .any(|issue| { issue.code == IssueCode::SettlementConflict })
    );
    conflict
        .analysis
        .check_proof()
        .expect("conflict explanations remain valid proof nodes");

    // Proof nodes are content addressed.  Mutating a cloned result is
    // rejected by the independent checker, demonstrating tamper detection
    // without modifying the immutable workspace or source commit.
    let mut tampered = decided.analysis.clone();
    let root = *tampered
        .proof
        .roots
        .first()
        .expect("decided proof has a root");
    tampered
        .proof
        .nodes
        .get_mut(&root)
        .expect("proof root has a node")
        .metadata
        .insert("tampered".into(), "yes".into());
    assert!(tampered.check_proof().is_err());
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
