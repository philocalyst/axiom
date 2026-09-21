//! Deterministic adversarial/property checks for the public semantic boundaries.
//!
//! These tests intentionally use small exhaustive permutation sets rather than
//! random generation.  The point is to assert that order is irrelevant only
//! where the data model says it is, and that forged content cannot cross a
//! checked boundary.

use std::collections::BTreeMap;

use axiom_ledger::exact::ExactNumber;
use axiom_ledger::incremental::{IncrementalDb, MemoOutcome, QueryKey, SourceKey, TraceEvent};
use axiom_ledger::ir::{Nominal, NominalKind, Sort, Symbol, Term, Var};
use axiom_ledger::model::{ContentHash, Date};
use axiom_ledger::package_lock::{
    Dependency, PackageManifest, PackageRegistry, Version, VersionReq, resolve,
};
use axiom_ledger::parser::{parse_source, parse_surface_file};
use axiom_ledger::proof::{CheckError, Node, Operation, Proof, Statement};
use axiom_ledger::store::{Commit, Decision, MergeConflict, ObjectStore};
use axiom_ledger::surface::{SurfaceFile, canonical_round_trip};
use axiom_ledger::time::{
    BeforeAfter, Bound, Instant, Interval, LocalDateTime, LocalTime, TimeError, TimeZone,
    UncertainInterval,
};
use axiom_ledger::units::{InstrumentUnit, Quantum, Ratio, UnitError};

fn permutations<T: Clone>(values: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(prefix: &mut Vec<T>, rest: &mut Vec<T>, output: &mut Vec<Vec<T>>) {
        if rest.is_empty() {
            output.push(prefix.clone());
            return;
        }
        for index in 0..rest.len() {
            let value = rest.remove(index);
            prefix.push(value.clone());
            visit(prefix, rest, output);
            prefix.pop();
            rest.insert(index, value);
        }
    }

    let mut output = Vec::new();
    visit(&mut Vec::new(), &mut values.to_vec(), &mut output);
    output
}

#[test]
fn canonical_addresses_and_sets_are_permutation_invariant() {
    let leaf_a = Node::new(
        "a",
        Operation::Observation {
            source: "source/a".into(),
        },
        Vec::new(),
        BTreeMap::new(),
    );
    let leaf_b = Node::new(
        "b",
        Operation::Observation {
            source: "source/b".into(),
        },
        Vec::new(),
        BTreeMap::new(),
    );
    let leaf_c = Node::new(
        "c",
        Operation::Observation {
            source: "source/c".into(),
        },
        Vec::new(),
        BTreeMap::new(),
    );
    let ids = [leaf_a.id, leaf_b.id, leaf_c.id];
    let reference = Node::new(
        "aggregate",
        Operation::Derive {
            rule: "all-three".into(),
        },
        ids.to_vec(),
        BTreeMap::from([("kind".into(), "aggregate".into())]),
    );
    for order in permutations(&ids) {
        let candidate = Node::new(
            "aggregate",
            Operation::Derive {
                rule: "all-three".into(),
            },
            order,
            BTreeMap::from([("kind".into(), "aggregate".into())]),
        );
        assert_eq!(candidate.id, reference.id);
        assert_eq!(candidate.inputs, reference.inputs);
    }

    let mut first = Proof::new();
    first.insert(leaf_a.clone());
    first.insert(leaf_b.clone());
    first.insert(leaf_c.clone());
    first.insert(reference.clone());
    first.root_all([reference.id, leaf_a.id, leaf_b.id, leaf_c.id]);

    let mut second = Proof::new();
    let second_roots = [leaf_b.id, reference.id, leaf_c.id, leaf_a.id];
    second.insert(leaf_c);
    second.insert(reference);
    second.insert(leaf_a);
    second.insert(leaf_b);
    second.root_all(second_roots);
    assert_eq!(first, second);
    assert!(first.check().is_ok());
}

#[test]
fn surface_and_strict_parser_round_trip_without_losing_source_trivia() {
    let source = "# heading\r\nbook tax-us\r\n\r\nbuy buy/one on 2024-01-02\r\n  1 ABC into checking\r\n  for 2 USD\r\n\r\nobserve position checking 1 ABC\r\n";
    let surface = SurfaceFile::parse(source);
    assert_eq!(surface.lossless(), source);
    assert_eq!(surface.format_lossless(), source);
    assert!(
        surface
            .tokens()
            .iter()
            .any(|token| token.lexeme == "# heading")
    );
    assert!(parse_surface_file(&surface).is_ok());
    assert!(parse_source(source).is_ok());

    let canonical = surface.canonical();
    assert!(!canonical.is_empty());
    assert_eq!(canonical, SurfaceFile::parse(&canonical).canonical());
    assert!(canonical_round_trip(source));
    assert_eq!(surface.reparsed_canonical().canonical(), canonical);
}

#[test]
fn unification_is_transactional_and_rejects_recursive_terms() {
    let left = Var::inference(1);
    let mut unifier = axiom_ledger::unify::Unifier::new();
    let partial_left = Term::Tuple(vec![Term::var(left.clone()), Term::Bool(false)]);
    let partial_right = Term::Tuple(vec![Term::Bool(true), Term::Text("not-a-bool".into())]);
    let error = unifier
        .unify(&partial_left, &partial_right)
        .expect_err("the second tuple element must fail");
    assert!(matches!(
        error.kind,
        axiom_ledger::unify::UnifyErrorKind::ConstructorMismatch
    ));
    assert!(
        unifier.substitutions().is_empty(),
        "failed probes must roll back the first binding"
    );

    let recursive = Term::App {
        constructor: Symbol::new("list"),
        arguments: vec![Term::var(left.clone())],
    };
    let error = unifier
        .unify(&Term::var(left.clone()), &recursive)
        .expect_err("occurs check");
    assert!(matches!(
        error.kind,
        axiom_ledger::unify::UnifyErrorKind::OccursCheck(variable) if *variable == left
    ));
    assert!(unifier.substitution(&left).is_none());

    let hole = Var::hole(7, "asset", Sort::Any);
    let candidates = vec![
        Term::nominal(Nominal::new(NominalKind::Instrument, "ABC")),
        Term::nominal(Nominal::new(NominalKind::Instrument, "USD")),
    ];
    let narrowing = unifier
        .narrow_hole(&hole, candidates.clone())
        .expect("candidate probing");
    assert_eq!(narrowing.candidates(), candidates.as_slice());
    assert!(unifier.substitution(&hole).is_none());
    let none = unifier
        .narrow_hole(&hole, [Term::Bool(true)])
        .expect("incompatible candidate is an explicit empty result");
    assert!(matches!(none, axiom_ledger::unify::Narrowing::None));
    assert!(unifier.substitution(&hole).is_none());
}

#[test]
fn proof_checker_rejects_node_and_root_tampering() {
    let node = Node::new(
        "observed",
        Operation::Observation {
            source: "bank/statement".into(),
        },
        Vec::new(),
        BTreeMap::new(),
    );
    let id = node.id;
    let mut proof = Proof::new();
    proof.insert(node);
    proof.root(id);
    assert!(proof.check().is_ok());

    let mut forged = proof.clone();
    forged.nodes.get_mut(&id).expect("node exists").statement = Statement::new("forged");
    assert!(matches!(forged.check(), Err(CheckError::TamperedNode { id: found }) if found == id));

    let mut roots_forged = proof;
    roots_forged.roots.push(id);
    assert!(matches!(
        roots_forged.check(),
        Err(CheckError::NonCanonicalRoots)
    ));
}

#[test]
fn package_lock_is_order_independent_but_detects_hash_and_dependency_tampering() {
    let leaf = PackageManifest::new("leaf", Version::new(1, 2, 0), "kind=leaf");
    let root =
        PackageManifest::new("root", Version::new(1, 0, 0), "kind=root").with_dependencies([
            Dependency::new("leaf", VersionReq::Caret(Version::new(1, 0, 0))),
        ]);
    let mut registry = PackageRegistry::default();
    registry.insert(leaf.clone()).expect("leaf registry entry");
    registry.insert(root.clone()).expect("root registry entry");

    let lock = resolve(&registry, [Dependency::new("root", VersionReq::Any)])
        .expect("deterministic resolution");
    lock.verify(&registry).expect("fresh lock verifies");

    let mut reordered = lock.clone();
    reordered.roots.reverse();
    reordered.packages.reverse();
    assert_eq!(reordered.canonical_bytes(), lock.canonical_bytes());
    assert_eq!(reordered.hash(), lock.hash());
    reordered.verify(&registry).expect("order is not semantic");

    let mut bad_hash = lock.clone();
    bad_hash.packages[0].hash = ContentHash::ZERO;
    assert!(matches!(
        bad_hash.verify(&registry),
        Err(axiom_ledger::package_lock::PackageLockError::LockHashMismatch { .. })
    ));

    let mut bad_dependency = lock;
    bad_dependency
        .packages
        .iter_mut()
        .find(|package| package.name == "root")
        .unwrap()
        .dependencies
        .clear();
    assert!(matches!(
        bad_dependency.verify(&registry),
        Err(axiom_ledger::package_lock::PackageLockError::LockDependencyMismatch { .. })
    ));
}

#[test]
fn temporal_and_unit_boundaries_are_explicit() {
    assert!(matches!(
        LocalTime::new(24, 0, 0, 0),
        Err(TimeError::InvalidLocalTime { .. })
    ));
    assert!(matches!(
        LocalTime::new(23, 59, 60, 0),
        Err(TimeError::InvalidLocalTime { .. })
    ));

    let epoch_date = Date::new(1970, 1, 1).unwrap();
    let noon = LocalTime::new(0, 0, 0, 0).unwrap();
    let fixed = LocalDateTime::new(epoch_date, noon, TimeZone::FixedOffsetSeconds(0));
    assert_eq!(fixed.to_instant().unwrap(), Instant::EPOCH);
    let named = LocalDateTime::new(epoch_date, noon, TimeZone::Named("UTC".into()));
    assert!(matches!(
        named.to_instant(),
        Err(TimeError::TimezoneRulesRequired)
    ));

    let zero = Instant::EPOCH;
    let ten = Instant::from_unix_seconds(10);
    let open = Interval::open(zero, ten).unwrap();
    assert!(!open.contains(&zero));
    assert!(open.contains(&Instant::from_unix_seconds(5)));
    assert!(!open.contains(&ten));
    assert!(Interval::new(Bound::Open(zero), Bound::Closed(zero)).is_err());
    assert!(UncertainInterval::new(ten, zero).is_err());
    assert!(BeforeAfter::new(ten, ten).is_err());

    let abc = InstrumentUnit::new("share", "ABC");
    let usd = InstrumentUnit::new("cent", "USD");
    assert!(matches!(
        Ratio::checked_new(abc.clone(), usd.clone(), ExactNumber::integer(0)),
        Err(UnitError::NonPositiveRatio)
    ));
    let ratio = Ratio::new(abc.clone(), usd.clone(), ExactNumber::integer(2));
    let converted = ratio
        .apply(&axiom_ledger::units::Quantity::typed(
            ExactNumber::integer(3),
            abc.clone(),
        ))
        .unwrap();
    assert_eq!(converted.amount().canonical_string(), "6");
    assert_eq!(converted.unit(), Some(&usd.as_model_unit()));

    let quantum = Quantum::new(abc.clone(), ExactNumber::integer(2)).unwrap();
    assert!(
        quantum
            .accepts(&axiom_ledger::units::Quantity::typed(
                ExactNumber::integer(4),
                abc.clone(),
            ))
            .is_ok()
    );
    assert!(matches!(
        quantum.accepts(&axiom_ledger::units::Quantity::typed(
            ExactNumber::integer(3),
            abc,
        )),
        Err(UnitError::OffQuantum { .. })
    ));
}

#[test]
fn merge_is_permutation_invariant_and_retains_conflicts() {
    let mut store = ObjectStore::new();
    let base = store
        .put_commit(Commit::new([], [], [], [], [], [], [], "base"))
        .unwrap();
    let left_decision = store
        .put_decision(Decision::new("sale/1", "lot/a").with_scope("book/tax"))
        .unwrap();
    let right_decision = store
        .put_decision(Decision::new("sale/1", "lot/b").with_scope("book/tax"))
        .unwrap();
    let left = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [left_decision],
            [],
            [],
            [],
            "left",
        ))
        .unwrap();
    let right = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [right_decision],
            [],
            [],
            [],
            "right",
        ))
        .unwrap();

    let first = store.merge_three_way(base, left, right, "merge").unwrap();
    let second = store.merge_three_way(base, right, left, "merge").unwrap();
    assert_eq!(first.conflicts, second.conflicts);
    assert_eq!(first.unresolved_conflicts, second.unresolved_conflicts);
    assert_eq!(first.commit, second.commit);
    assert!(matches!(
        first.conflicts.as_slice(),
        [MergeConflict::Decisions { subject, .. }] if subject == "sale/1"
    ));
    assert!(!first.is_clean());
    assert!(store.verify().is_ok());
}

fn evaluate_input(
    ctx: &mut axiom_ledger::incremental::QueryContext<'_>,
    source: &SourceKey,
) -> MemoOutcome {
    let input = ctx.input(source).expect("input dependency");
    MemoOutcome::value(input.content().to_vec())
}

#[test]
fn incremental_clean_and_warm_evaluations_have_identical_values() {
    let source = SourceKey::new("source/ledger").unwrap();
    let key = QueryKey::new("query/report").unwrap();
    let mut clean = IncrementalDb::new();
    clean
        .upsert_input(source.clone(), "ledger", b"book tax-us\n".to_vec())
        .unwrap();
    let clean_value = clean.evaluate(key.clone(), |ctx| evaluate_input(ctx, &source));

    let mut warm = clean.clone();
    warm.clear_trace();
    let warm_value = warm.evaluate(key.clone(), |_ctx| {
        panic!("a valid warm memo must not execute its producer")
    });
    assert_eq!(clean_value, warm_value);
    assert!(matches!(warm.trace(), [TraceEvent::CacheHit { query, .. }] if query == &key));
    assert!(warm.is_valid(&key));
}
