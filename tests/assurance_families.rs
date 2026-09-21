//! Deterministic generated assurance for the Gate 10 semantic families.
//!
//! The cases below deliberately exercise public boundaries rather than private
//! helpers.  They are small enough to run on every test invocation, but broad
//! enough to catch regressions in rollback, canonicalization, exact arithmetic,
//! graph closure, and immutable import/merge behavior.

use axiom_ledger::evidence::{
    AdapterProvenance, Availability, EvidenceStore, ImportBatch, ImportDisposition, ImportError,
    ObservationAdapter, RawEvidence,
};
use axiom_ledger::exact::{ExactNumber, RoundingMode};
use axiom_ledger::ir::{Atom, Nominal, NominalKind, Record, Sort, Symbol, Term, Var};
use axiom_ledger::logic::{Clause, Goal, Literal, Program, Solver, TraceEvent};
use axiom_ledger::model::{ContentHash, Date, OccurrenceId, SourceId, Unit};
use axiom_ledger::package_lock::{
    Dependency, PackageLockError, PackageManifest, PackageRegistry, Version, VersionReq, resolve,
};
use axiom_ledger::parser::{parse_ledger, parse_source};
use axiom_ledger::semantics::{Completion, Truth};
use axiom_ledger::store::{Commit, Decision, MergeConflict, ObjectStore};
use axiom_ledger::surface::{Severity, SurfaceFile, canonical_format, canonical_round_trip};
use axiom_ledger::time::{
    BeforeAfter, Bound, BusinessCalendar, BusinessDayPolicy, Frequency, Instant, Interval,
    LocalDateTime, LocalTime, LocalTimeStatus, MissingDayPolicy, Period, TimeError, TimeZone,
    UncertainInterval,
};
use axiom_ledger::units::{
    ConversionLeg, ConversionPath, InstrumentDefinition, InstrumentUnit, Quantum, Ratio,
    RoundingCertificate, UnitError,
};

fn date(year: i32, month: u8, day: u8) -> Date {
    Date::new(year, month, day).expect("generated date is valid")
}

fn predicate(name: impl Into<String>) -> Nominal {
    Nominal::new(NominalKind::Predicate, name.into())
}

fn entity(name: impl Into<String>) -> Term {
    Term::nominal(Nominal::new(NominalKind::Entity, name.into()))
}

#[test]
fn parser_generated_validity_and_diagnostics_are_stable() {
    // 384 valid ledgers cover month/day boundaries, exact decimal spellings,
    // and the parser-to-model lowering boundary.
    for case in 0..384u32 {
        let month = (case % 12 + 1) as u8;
        let day = ((case / 12) % 28 + 1) as u8;
        let year = 2020 + (case % 7);
        let amount = format!("{}.{}", case % 97 + 1, case % 10);
        let source = format!(
            "book book/{case}\nbuy buy/{case} on {year:04}-{month:02}-{day:02}\n  {amount} ABC into checking/{case}\n  for {} USD\n",
            case + 2
        );
        let parsed = parse_source(&source).expect("generated buy should parse");
        assert_eq!(parsed.statements.len(), 1);
        assert!(
            parse_ledger(&source).is_ok(),
            "lowering failed for case {case}"
        );

        // Typed holes are retained by the parser-facing form but rejected by
        // the resolved model boundary.
        let hole_source = format!(
            "book book/{case}\nbuy buy/{case} on {year:04}-{month:02}-{day:02}\n  ?amount ?unit into checking/{case}\n  for 1 USD\n"
        );
        assert!(parse_source(&hole_source).is_ok());
        assert!(parse_ledger(&hole_source).is_err());
    }

    for case in 0..192u32 {
        let month = (case % 12 + 1) as u8;
        let day = ((case % 28) + 1) as u8;
        let year = 2021 + (case % 5);
        let source = format!(
            "book b\nbuy duplicate on {year:04}-{month:02}-{day:02}\n  1 ABC into cash\n  for 1 USD\nbuy duplicate on {year:04}-{month:02}-{day:02}\n  1 ABC into cash\n  for 1 USD\n"
        );
        let error = parse_source(&source).expect_err("duplicate IDs must be rejected");
        assert_eq!(error.location.line, 5);
        assert!(error.message.contains("duplicate"));

        let bad_date =
            format!("book b\nbuy x on {year:04}-02-30\n  1 ABC into cash\n  for 1 USD\n");
        assert!(parse_source(&bad_date).is_err());
        let bad_unit =
            format!("book b\nbuy x on {year:04}-{month:02}-{day:02}\n  1 into cash\n  for 1 USD\n");
        assert!(parse_source(&bad_unit).is_err());
        let unknown = format!("book b\nfuture-directive {case}\n");
        let unknown_error = parse_source(&unknown).expect_err("unknown directives are errors");
        assert_eq!(unknown_error.location.line, 2);
    }
}

#[test]
fn canonicalization_ignores_trivia_but_retains_unknown_content() {
    let base = "book b\nbuy tx 2024-01-02\n  1 ABC into cash\n  for 2 USD\n";
    let expected = canonical_format(base);
    let baseline = SurfaceFile::parse(base);
    assert!(canonical_round_trip(base));
    assert_eq!(baseline.lossless(), base);

    for case in 0..512usize {
        let newline = if case % 2 == 0 { "\n" } else { "\r\n" };
        let between = match case % 5 {
            0 => " ",
            1 => "  ",
            2 => "\t",
            3 => " \t ",
            _ => "\t\t",
        };
        let source = format!(
            "book{between}b{newline}buy{between}tx{between}2024-01-02{newline}  {case_mod}ABC{between}into{between}cash{newline}\
             \tfor{between}2{between}USD{newline}",
            case_mod = if case % 3 == 0 { "1 " } else { "1\t" }
        );
        let file = SurfaceFile::parse(&source);
        assert_eq!(file.lossless(), source);
        assert_eq!(file.format_lossless(), source);
        let expected_for_newline = expected.replace('\n', newline);
        assert_eq!(
            file.canonical(),
            expected_for_newline,
            "trivia changed canonical case {case}"
        );
        assert!(canonical_round_trip(&source));
        assert_eq!(file.nodes().len(), 2);
        assert!(
            file.tokens()
                .iter()
                .all(|token| token.span.text(&source).is_some())
        );
    }

    for case in 0..128usize {
        let comment = if case % 2 == 0 {
            "# future"
        } else {
            "; future"
        };
        let source = format!("book b\n{comment} syntax {case}\n??future {case}\n");
        let file = SurfaceFile::parse(&source);
        assert_eq!(file.lossless(), source);
        assert!(
            file.tokens()
                .iter()
                .any(|token| token.lexeme.starts_with(comment))
        );
        assert!(file.canonical().contains(comment));
        assert!(file.tokens().iter().any(|token| token.lexeme == "??"));
        assert!(
            file.diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.span.text(&source).is_some())
        );
        assert!(
            file.diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.severity == Severity::Error
                    || diagnostic.severity == Severity::Warning)
        );
    }
}

#[test]
fn unification_is_transactional_across_generated_shapes() {
    for case in 0..384u32 {
        let variable = Var::inference(case);
        let mut unifier = axiom_ledger::unify::Unifier::new();
        unifier
            .unify(&Term::var(variable.clone()), &Term::Bool(true))
            .expect("initial binding");
        let before = unifier.substitutions();
        let left = Term::Tuple(vec![Term::var(variable.clone()), Term::Bool(false)]);
        let right = Term::Tuple(vec![Term::Bool(true), Term::Text(format!("bad-{case}"))]);
        let error = unifier
            .unify(&left, &right)
            .expect_err("second tuple member mismatches");
        assert!(matches!(
            error.kind,
            axiom_ledger::unify::UnifyErrorKind::ConstructorMismatch
        ));
        assert_eq!(
            unifier.substitutions(),
            before,
            "partial binding leaked at {case}"
        );

        let occurs_variable = Var::inference(case + 10_000);
        let recursive = Term::App {
            constructor: Symbol::new(format!("list-{case}")),
            arguments: vec![Term::var(occurs_variable.clone())],
        };
        let occurs = unifier
            .unify(&Term::var(occurs_variable.clone()), &recursive)
            .expect_err("recursive terms must fail occurs-check");
        assert!(matches!(
            occurs.kind,
            axiom_ledger::unify::UnifyErrorKind::OccursCheck(found) if *found == occurs_variable
        ));
        assert_eq!(unifier.substitutions(), before);

        let left_record = Term::record(Record::closed([
            ("z", Term::Text(format!("z-{case}"))),
            ("a", Term::Bool(case % 2 == 0)),
        ]));
        let right_record = Term::record(Record::closed([
            ("a", Term::Bool(case % 2 == 0)),
            ("z", Term::Text(format!("z-{case}"))),
        ]));
        unifier
            .unify(&left_record, &right_record)
            .expect("record field order is canonical");
        assert_eq!(unifier.resolve(&left_record), left_record);
    }

    for case in 0..256u32 {
        let hole = Var::hole(case, format!("hole-{case}"), Sort::Any);
        let first = Term::nominal(Nominal::new(NominalKind::Instrument, format!("I-{case}")));
        let second = Term::nominal(Nominal::new(NominalKind::Instrument, format!("J-{case}")));
        let mut unifier = axiom_ledger::unify::Unifier::new();
        let narrowing = unifier
            .narrow_hole(&hole, [first.clone(), second.clone(), first.clone()])
            .expect("candidate probe");
        assert_eq!(narrowing.candidates(), &[first.clone(), second.clone()]);
        assert_eq!(
            unifier.hole_domain(&hole),
            Some(vec![first.clone(), second.clone()])
        );
        assert!(unifier.substitution(&hole).is_none());

        let none = unifier
            .narrow_hole(&hole, [Term::Bool(case % 2 == 0)])
            .expect("empty narrowing is explicit");
        assert!(matches!(none, axiom_ledger::unify::Narrowing::None));
        assert!(unifier.substitution(&hole).is_none());

        let unique_hole = Var::hole(case + 10_000, "unique", Sort::Any);
        let unique = unifier
            .narrow_hole(&unique_hole, [Term::Text(format!("value-{case}"))])
            .expect("unique candidate");
        assert!(unique.is_unique());
        assert!(unifier.substitution(&unique_hole).is_some());
    }
}

#[test]
fn logic_cycles_need_a_base_and_negative_cycles_are_unstratified() {
    for case in 1..96usize {
        let width = 2 + case % 7;
        let argument = entity(format!("e-{case}"));
        let mut program = Program::new();
        for index in 0..width {
            let variable = Var::named((index + 1) as u32, format!("x-{case}-{index}"));
            let head = Literal::positive(Atom::new(
                predicate(format!("p-{case}-{index}")),
                vec![Term::var(variable.clone())],
            ));
            let body = Literal::positive(Atom::new(
                predicate(format!("p-{case}-{}", (index + 1) % width)),
                vec![Term::var(variable)],
            ));
            program.add_clause(Clause::new(head, Goal::atom(body)));
        }
        let goal = Goal::atom(Literal::positive(Atom::new(
            predicate(format!("p-{case}-0")),
            vec![argument.clone()],
        )));
        let mut solver = Solver::new();
        let result = solver.solve(&program, &goal, &Default::default());
        assert_eq!(result.truth(), Truth::Neither);
        assert_eq!(result.completion(), Completion::Complete);
        assert!(
            result
                .trace()
                .iter()
                .any(|event| matches!(event, TraceEvent::CycleWithoutBase { .. }))
        );
        assert!(result.check_proofs().is_ok());

        let mut based = program.clone();
        based
            .add_fact(Literal::positive(Atom::new(
                predicate(format!("p-{case}-0")),
                vec![argument.clone()],
            )))
            .expect("ground base fact");
        let based_result = Solver::new().solve(&based, &goal, &Default::default());
        assert!(based_result.truth().has_positive());
        assert!(
            !based_result
                .trace()
                .iter()
                .any(|event| matches!(event, TraceEvent::CycleWithoutBase { .. }))
        );
        assert!(based_result.check_proofs().is_ok());
    }

    for case in 0..96usize {
        let atom = Literal::positive(Atom::new(
            predicate(format!("neg-{case}")),
            vec![entity(format!("e-{case}"))],
        ));
        let mut program = Program::new();
        program.add_clause(Clause::new(
            atom.clone(),
            Goal::default_not(Goal::atom(atom.clone())),
        ));
        assert!(matches!(
            program.validate(),
            Err(axiom_ledger::logic::LogicError::UnstratifiedNegation { .. })
        ));
        let result = Solver::new().solve(&program, &Goal::atom(atom), &Default::default());
        assert!(
            result.is_incomplete()
                || result
                    .trace()
                    .iter()
                    .any(|event| matches!(event, TraceEvent::UnstratifiedNegation { .. }))
        );
    }
}

#[test]
fn temporal_boundaries_and_recurrences_preserve_precision() {
    for case in 0..512i64 {
        let instant = Instant::from_unix_nanos((case * 1_000_003 - 200_000_000).into());
        let delta: i128 = (case * 97 - 10_000).into();
        let shifted = instant
            .checked_add_nanos(delta)
            .expect("small generated offset");
        assert_eq!(shifted.duration_since(instant), Some(delta));
        assert_eq!(instant.checked_add_nanos(delta).unwrap(), shifted);

        let hour = (case as u8) % 24;
        let minute = ((case / 24) as u8) % 60;
        let second = ((case / (24 * 60)) as u8) % 60;
        let local = LocalTime::new(hour, minute, second, (case as u32) % 1_000_000_000).unwrap();
        let local_date = date(
            2024 + (case as i32 % 3),
            (case as u8 % 12) + 1,
            1 + (case as u8 % 27),
        );
        let fixed = LocalDateTime::new(
            local_date,
            local,
            TimeZone::FixedOffsetSeconds((case as i32 % 25 - 12) * 3_600),
        );
        assert_eq!(fixed.status(), LocalTimeStatus::Exact);
        assert!(fixed.to_instant().is_ok());
        let named = LocalDateTime::new(local_date, local, TimeZone::Named(format!("Zone/{case}")));
        assert_eq!(named.status(), LocalTimeStatus::Unresolved);
        assert!(matches!(
            named.to_instant(),
            Err(TimeError::TimezoneRulesRequired)
        ));
    }

    for hour in [0u8, 1, 23, 24, 25] {
        for minute in [0u8, 1, 59, 60] {
            for second in [0u8, 59, 60] {
                let result = LocalTime::new(hour, minute, second, 0);
                if hour < 24 && minute < 60 && second < 60 {
                    assert!(result.is_ok());
                } else {
                    assert!(matches!(result, Err(TimeError::InvalidLocalTime { .. })));
                }
            }
        }
    }

    let points = [
        Instant::from_unix_seconds(-2),
        Instant::from_unix_seconds(0),
        Instant::from_unix_seconds(2),
        Instant::from_unix_seconds(4),
    ];
    let bounds = [(true, true), (false, true), (true, false), (false, false)];
    for start_index in 0..points.len() - 1 {
        for end_index in start_index + 1..points.len() {
            for (start_closed, end_closed) in bounds {
                let start = if start_closed {
                    Bound::Closed(points[start_index])
                } else {
                    Bound::Open(points[start_index])
                };
                let end = if end_closed {
                    Bound::Closed(points[end_index])
                } else {
                    Bound::Open(points[end_index])
                };
                let interval = Interval::new(start, end).unwrap();
                assert!(interval.contains(&points[start_index]) == interval.start().is_closed());
                assert!(interval.contains(&points[end_index]) == interval.end().is_closed());
                assert!(interval.intersects(&interval));
                let other =
                    Interval::new(Bound::Unbounded, Bound::Closed(points[end_index])).unwrap();
                assert_eq!(interval.intersection(&other), other.intersection(&interval));
            }
        }
    }
    for value in 0..64i64 {
        let earlier = Instant::from_unix_seconds(value);
        let later = Instant::from_unix_seconds(value + 1);
        assert!(BeforeAfter::new(earlier, later).unwrap().holds());
        assert!(BeforeAfter::new(later, earlier).is_err());
        assert!(
            UncertainInterval::new(earlier, later)
                .unwrap()
                .contains(&earlier)
        );
        assert!(UncertainInterval::new(later, earlier).is_err());
    }

    for case in 0..128i32 {
        let start = date(2024, 1, 1 + (case as u8 % 20));
        let recurrence = axiom_ledger::time::Recurrence::new(
            start,
            Frequency::Daily {
                every: (case as u32 % 5) + 1,
            },
        )
        .unwrap()
        .with_count(20);
        let output = recurrence.between(start, date(2024, 3, 31)).unwrap();
        assert!(output.windows(2).all(|window| window[0] < window[1]));
        assert!(output.len() <= 20);

        let monthly = axiom_ledger::time::Recurrence::new(
            date(2024, 1, 31),
            Frequency::Monthly { every: 1, day: 31 },
        )
        .unwrap()
        .with_count(4)
        .with_missing_day_policy(if case % 2 == 0 {
            MissingDayPolicy::ClampToLastDay
        } else {
            MissingDayPolicy::Skip
        });
        let monthly_output = monthly
            .between(date(2024, 1, 1), date(2024, 6, 30))
            .unwrap();
        assert!(
            monthly_output
                .iter()
                .all(|value| *value >= date(2024, 1, 1))
        );
    }

    let mut calendar = BusinessCalendar::default();
    calendar.holidays.insert(date(2024, 1, 1));
    assert_eq!(
        calendar
            .adjust(date(2024, 1, 1), BusinessDayPolicy::Following)
            .unwrap(),
        date(2024, 1, 2)
    );
    assert!(Period::month(2024, 1).is_ok());
    assert!(Period::month(2024, 13).is_err());
    assert!(Period::quarter(2024, 4).is_ok());
    assert!(Period::quarter(2024, 5).is_err());
}

#[test]
fn units_are_nominal_exact_and_never_implicitly_rounded() {
    for case in 1..384i64 {
        let a = InstrumentUnit::new(format!("share-{case}"), format!("asset-{case}"));
        let b = InstrumentUnit::new(format!("cent-{case}"), format!("cash-{case}"));
        let c = InstrumentUnit::new(format!("micro-{case}"), format!("cash2-{case}"));
        let first_rate = ExactNumber::rational(case + 1, case + 2).unwrap();
        let second_rate = ExactNumber::rational(case + 2, case + 3).unwrap();
        let first = Ratio::checked_new(a.clone(), b.clone(), first_rate.clone()).unwrap();
        let second = Ratio::checked_new(b.clone(), c.clone(), second_rate.clone()).unwrap();
        let path = ConversionPath::new(vec![
            ConversionLeg::direct(first.clone(), None),
            ConversionLeg::direct(second.clone(), None),
        ])
        .unwrap();
        assert_eq!(path.from(), &a);
        assert_eq!(path.to(), &c);
        assert_eq!(path.legs().len(), 2);
        assert_eq!(path.rate(), &first_rate.checked_mul(&second_rate));
        let input = axiom_ledger::units::Quantity::typed(ExactNumber::integer(case), a.clone());
        let converted = path.convert(&input).unwrap();
        assert_eq!(converted.unit(), Some(&c.as_model_unit()));

        let inverse = first.inverse().unwrap();
        assert_eq!(inverse.from(), &b);
        assert_eq!(inverse.to(), &a);
        assert_eq!(
            first.apply(&input).unwrap().unit(),
            Some(&b.as_model_unit())
        );
        assert!(matches!(
            second.apply(&input),
            Err(UnitError::UnitMismatch { .. })
        ));
        assert!(matches!(
            Ratio::checked_new(a.clone(), b.clone(), ExactNumber::integer(0)),
            Err(UnitError::NonPositiveRatio)
        ));
        assert!(matches!(
            Ratio::checked_new(a.clone(), b.clone(), ExactNumber::integer(-1)),
            Err(UnitError::NonPositiveRatio)
        ));

        let quantum = Quantum::new(a.clone(), ExactNumber::integer(2)).unwrap();
        let accepted =
            axiom_ledger::units::Quantity::typed(ExactNumber::integer(case * 2), a.clone());
        let rejected =
            axiom_ledger::units::Quantity::typed(ExactNumber::integer(case * 2 + 1), a.clone());
        assert!(quantum.accepts(&accepted).is_ok());
        assert!(matches!(
            quantum.accepts(&rejected),
            Err(UnitError::OffQuantum { .. })
        ));

        let definition =
            InstrumentDefinition::new(format!("asset-{case}"), a.clone(), ExactNumber::integer(2))
                .unwrap();
        assert!(definition.accepts(&accepted).is_ok());
        assert!(matches!(
            InstrumentDefinition::new(format!("other-{case}"), a.clone(), ExactNumber::integer(2)),
            Err(UnitError::InstrumentMismatch { .. })
        ));

        let certificate = RoundingCertificate::apply(
            ExactNumber::rational(case * 10 + 1, 3).unwrap(),
            (case as u32) % 4,
            RoundingMode::HalfEven,
        )
        .unwrap();
        assert!(certificate.verify().is_ok());
    }

    let invalid = Unit::new("plain").unwrap();
    assert!(matches!(
        InstrumentUnit::from_model_unit(&invalid),
        Err(UnitError::InvalidCanonicalUnit(_))
    ));
}

#[test]
fn package_resolution_is_deterministic_and_closure_checked() {
    for case in 0..128u64 {
        let leaf_name = format!("leaf-{case}");
        let root_name = format!("root-{case}");
        let mut registry = PackageRegistry::default();
        let leaf_old =
            PackageManifest::new(&leaf_name, Version::new(1, 0, 0), format!("old-{case}"));
        let leaf_new = PackageManifest::new(
            &leaf_name,
            Version::new(1, 4, case % 10),
            format!("new-{case}"),
        );
        let leaf_major =
            PackageManifest::new(&leaf_name, Version::new(2, 0, 0), format!("major-{case}"));
        registry.insert(leaf_old.clone()).unwrap();
        registry.insert(leaf_new.clone()).unwrap();
        registry.insert(leaf_major).unwrap();
        let root = PackageManifest::new(&root_name, Version::new(1, 0, 0), format!("root-{case}"))
            .with_dependencies([Dependency::new(
                &leaf_name,
                VersionReq::Caret(Version::new(1, 0, 0)),
            )]);
        registry.insert(root.clone()).unwrap();
        assert_eq!(registry.insert(root.clone()).unwrap(), root.hash());

        let root_dependency = Dependency::new(&root_name, VersionReq::Any);
        let lock = resolve(
            &registry,
            [root_dependency.clone(), root_dependency.clone()],
        )
        .unwrap();
        lock.verify(&registry).unwrap();
        let mut reordered = lock.clone();
        reordered.roots.reverse();
        reordered.packages.reverse();
        assert_eq!(reordered.canonical_bytes(), lock.canonical_bytes());
        assert_eq!(reordered.hash(), lock.hash());
        assert_eq!(
            lock.packages
                .iter()
                .find(|package| package.name == leaf_name)
                .unwrap()
                .version,
            leaf_new.version
        );

        let mut bad_hash = lock.clone();
        bad_hash.packages[0].hash = ContentHash::ZERO;
        assert!(matches!(
            bad_hash.verify(&registry),
            Err(PackageLockError::LockHashMismatch { .. })
        ));
        let mut bad_dependency = lock.clone();
        bad_dependency
            .packages
            .iter_mut()
            .find(|package| package.name == root_name)
            .unwrap()
            .dependencies
            .clear();
        assert!(matches!(
            bad_dependency.verify(&registry),
            Err(PackageLockError::LockDependencyMismatch { .. })
        ));
    }

    for case in 0..64u64 {
        let a_name = format!("cycle-a-{case}");
        let b_name = format!("cycle-b-{case}");
        let a = PackageManifest::new(&a_name, Version::new(1, 0, 0), "a")
            .with_dependencies([Dependency::new(&b_name, VersionReq::Any)]);
        let b = PackageManifest::new(&b_name, Version::new(1, 0, 0), "b")
            .with_dependencies([Dependency::new(&a_name, VersionReq::Any)]);
        let mut registry = PackageRegistry::default();
        registry.insert(a).unwrap();
        registry.insert(b).unwrap();
        assert!(matches!(
            resolve(&registry, [Dependency::new(&a_name, VersionReq::Any)]),
            Err(PackageLockError::DependencyCycle(_))
        ));
        assert!(matches!(
            Version::parse("01.0.0"),
            Err(PackageLockError::InvalidVersion(_))
        ));
        assert!(matches!(
            VersionReq::parse("^1.2"),
            Err(PackageLockError::InvalidVersion(_))
        ));
    }
    let mut registry = PackageRegistry::default();
    assert!(matches!(
        registry.insert(PackageManifest::new(
            "bad name",
            Version::new(1, 0, 0),
            "body"
        )),
        Err(PackageLockError::InvalidName(_))
    ));
    assert!(matches!(
        registry.insert(PackageManifest::new("empty", Version::new(1, 0, 0), "  ")),
        Err(PackageLockError::EmptyBody(_))
    ));
}

#[test]
fn generated_three_way_merges_retain_conflicts_and_are_commutative() {
    let mut store = ObjectStore::new();
    for case in 0..96u32 {
        let base = store
            .put_commit(Commit::new(
                [],
                [],
                [],
                [],
                [],
                [],
                [],
                format!("base-{case}"),
            ))
            .unwrap();
        let left_decision = store
            .put_decision(
                Decision::new(format!("subject-{case}"), format!("left-{case}"))
                    .with_scope("scope"),
            )
            .unwrap();
        let right_decision = store
            .put_decision(
                Decision::new(format!("subject-{case}"), format!("right-{case}"))
                    .with_scope("scope"),
            )
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
                format!("left-{case}"),
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
                format!("right-{case}"),
            ))
            .unwrap();
        let forward = store
            .merge_three_way(base, left, right, format!("merge-{case}"))
            .unwrap();
        let reverse = store
            .merge_three_way(base, right, left, format!("merge-{case}"))
            .unwrap();
        assert_eq!(forward.conflicts, reverse.conflicts);
        assert_eq!(forward.unresolved_conflicts, reverse.unresolved_conflicts);
        assert_eq!(forward.commit, reverse.commit);
        assert!(matches!(
            forward.conflicts.as_slice(),
            [MergeConflict::Decisions { subject, .. }] if subject == &format!("subject-{case}")
        ));
        assert!(!forward.is_clean());

        let clean = store
            .merge_three_way(base, left, base, format!("clean-{case}"))
            .unwrap();
        assert!(clean.is_clean());
    }
    store.verify().unwrap();
}

#[derive(Clone, Copy)]
struct GeneratedAdapter;

impl ObservationAdapter for GeneratedAdapter {
    type Error = &'static str;

    fn observe(&self, source: &SourceId, bytes: &[u8]) -> Result<ImportBatch, Self::Error> {
        let adapter = AdapterProvenance::new("generated-adapter", "1.0");
        let mut batch = ImportBatch::new(source.clone()).with_adapter(adapter);
        for (index, byte) in bytes.iter().enumerate() {
            batch.push(RawEvidence::from_bytes(
                source.clone(),
                format!("row-{index}"),
                None,
                [*byte, index as u8],
            ));
        }
        Ok(batch)
    }
}

#[test]
fn adapters_are_observation_only_and_imports_are_idempotent() {
    for case in 0..128usize {
        let source = SourceId::new(format!("source-{case}"));
        let bytes = vec![case as u8, (case >> 1) as u8, (case >> 2) as u8];
        let batch = GeneratedAdapter.observe(&source, &bytes).unwrap();
        assert!(!batch.is_empty());
        let mut reordered = batch.clone();
        reordered.observations.reverse();
        assert_eq!(batch.content_hash(), reordered.content_hash());

        let mut store = EvidenceStore::new();
        let first = store.import_batch(batch.clone()).unwrap();
        assert_eq!(first.inserted_count(), bytes.len());
        assert_eq!(first.existing_count(), 0);
        let second = store.import_batch(reordered).unwrap();
        assert_eq!(second.inserted_count(), 0);
        assert_eq!(second.existing_count(), bytes.len());
        assert_eq!(store.len(), bytes.len());
        assert!(store.iter().all(|evidence| evidence.is_available()));

        let wrong_source = ImportBatch::from_observations(
            source.clone(),
            [RawEvidence::from_bytes("other-source", "wrong", None, b"x")],
        );
        assert!(matches!(
            store.import_batch(wrong_source),
            Err(ImportError::SourceMismatch { .. })
        ));
        assert_eq!(store.len(), bytes.len(), "failed batch was not atomic");

        let conflicting = ImportBatch::from_observations(
            source.clone(),
            [RawEvidence::from_bytes(
                source.clone(),
                "row-0",
                None,
                b"changed",
            )],
        )
        .with_adapter(AdapterProvenance::new("generated-adapter", "1.0"));
        assert!(matches!(
            store.import_batch(conflicting),
            Err(ImportError::OccurrenceConflict { .. })
        ));
        assert_eq!(store.len(), bytes.len());

        let changed_adapter = ImportBatch::from_observations(
            source.clone(),
            [RawEvidence::from_bytes(
                source.clone(),
                "row-0",
                None,
                b"changed",
            )],
        )
        .with_adapter(AdapterProvenance::new("generated-adapter", "2.0"));
        let changed_report = store.import_batch(changed_adapter).unwrap();
        assert_eq!(changed_report.inserted_count(), 1);
        assert_eq!(store.len(), bytes.len() + 1);
    }

    let source = SourceId::new("tombstone-source");
    let tombstone = RawEvidence::deleted_tombstone(
        source.clone(),
        OccurrenceId::new("deleted"),
        None,
        ContentHash::domain_separated("test", b"deleted"),
    )
    .unwrap();
    assert_eq!(tombstone.availability(), Availability::Deleted);
    assert!(!tombstone.is_available());
    let mut store = EvidenceStore::new();
    assert_eq!(
        store.insert(tombstone).unwrap(),
        ImportDisposition::Inserted
    );
    assert_eq!(store.len(), 1);
}
