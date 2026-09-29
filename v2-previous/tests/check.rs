use axiom_v2::{Model, Outcome, Phase, Value, check, decision_claim_id, verify, view_period};

const CHOICE_PACKAGE: &str = r#"package chooser

buy ?buy
  units ?units ?asset

sell ?sell
  units ?units ?asset
  lot @?buy

rule choose_lot
  for s sell
  choose b s.lot (rows buy)
  require (le s.units b.units)
  emit selected
  set sell (ref s)
  set lot (ref b)
"#;

const SIMPLE_PACKAGE: &str = r#"package simple

item ?item
  label ?label
  date ?date

rule report_item
  for i item
  emit report
  set item (ref i)
  set label i.label
  set date i.date
  book records

book records
  include report
"#;

const COMPLETENESS_PACKAGE: &str = r#"package completeness

item ?item
  amount ?amount

probe ?probe
  name ?name

rule positive_item
  for i item
  require (gt i.amount 0)
  emit positive
  set item (ref i)
  set amount i.amount

rule count_positive
  for p probe
  let amount (count (rows positive))
  emit counted
  set probe (ref p)
  set count amount

rule copy_probe
  for p probe
  emit copied
  set probe (ref p)
  set name p.name

rule missing_nested_reference
  for p probe
  emit linked
  set probe (ref p)
  set item (if (eq p.name "missing") [@absent] [@valid])
"#;

fn model(source: &str) -> Model {
    Model::compile(&[source.to_owned()]).expect("package should compile")
}

fn choice_source(buys: &[(&str, &str)], sell_units: &str, lot: &str) -> String {
    let mut source = String::from("ledger choice\nuse chooser\n\n");
    for (id, units) in buys {
        source.push_str(&format!("buy {id}\n  units {units} ABC\n\n"));
    }
    source.push_str(&format!(
        "sell sale\n  units {sell_units} ABC\n  lot {lot}\n"
    ));
    source
}

fn rule_finding<'a>(
    world: &'a axiom_v2::World,
    subject: &str,
    rule: &str,
) -> &'a axiom_v2::Finding {
    world
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == subject && finding.rule == rule)
        .expect("expected rule finding")
}

#[test]
fn unresolved_choice_checks_every_candidate_and_never_picks_the_first() {
    let model = model(CHOICE_PACKAGE);
    let source = choice_source(&[("too_small", "2"), ("enough", "10")], "5", "?lot");
    let (_, world, _) = check(&source, &model, 100_000).expect("ledger should elaborate");

    assert_eq!(
        rule_finding(&world, "sale", "choose_lot").outcome,
        Outcome::Alternatives(vec![Value::Ref("enough".into())])
    );
    assert!(
        world
            .evaluation()
            .claims
            .iter()
            .all(|claim| claim.row.schema != "selected")
    );

    let single = choice_source(&[("only", "10")], "5", "?lot");
    let (_, world, _) = check(&single, &model, 100_000).expect("single candidate should check");
    assert_eq!(
        rule_finding(&world, "sale", "choose_lot").outcome,
        Outcome::Alternatives(vec![Value::Ref("only".into())]),
        "a unique candidate still requires an explicit choice"
    );
}

#[test]
fn impossible_choice_is_a_conflict_and_explicit_choice_derives_a_claim() {
    let model = model(CHOICE_PACKAGE);
    let impossible = choice_source(&[("small", "2"), ("smaller", "3")], "5", "?lot");
    let (_, world, _) = check(&impossible, &model, 100_000).expect("ledger should elaborate");
    assert!(matches!(
        rule_finding(&world, "sale", "choose_lot").outcome,
        Outcome::Conflict(_)
    ));

    let selected = choice_source(&[("small", "2"), ("enough", "10")], "5", "@enough");
    let (_, world, _) = check(&selected, &model, 100_000).expect("explicit choice should check");
    let claims: Vec<_> = world
        .evaluation()
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "selected")
        .collect();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].phase, Phase::Candidate);
    assert_eq!(claims[0].row.fields["lot"], Value::Ref("enough".into()));
    assert!(
        !world
            .evaluation()
            .findings
            .iter()
            .any(|finding| finding.subject == "sale" && finding.rule == "choose_lot")
    );
}

#[test]
fn world_identity_ignores_source_trivia_but_certificate_binds_exact_bytes_and_coverage() {
    let model = model(SIMPLE_PACKAGE);
    let first = "ledger house\nuse simple\n\nitem a\n  label \"A\"\n  date 2026-01-02\n\nitem b\n  label \"B\"\n  date 2026-02-03\n";
    let second = "# comment\nledger house\nuse simple\n\nitem b\n  date 2026-02-03\n  label \"B\"\n\nitem a\n  date 2026-01-02\n  label \"A\"\n";
    let (_, first_world, first_certificate) = check(first, &model, 100_000).unwrap();
    let (_, second_world, second_certificate) = check(second, &model, 100_000).unwrap();

    assert_eq!(first_world.id(), second_world.id());
    assert_ne!(first_certificate.revision, second_certificate.revision);
    verify(first, &model, &first_certificate, 100_000).unwrap();

    let mut omitted = first_certificate.clone();
    omitted.evaluation.claims.pop();
    assert!(verify(first, &model, &omitted, 100_000).is_err());

    let mut extra = first_certificate;
    extra
        .evaluation
        .claims
        .push(extra.evaluation.claims[0].clone());
    assert!(verify(first, &model, &extra, 100_000).is_err());
}

#[test]
fn explicit_decision_changes_world_and_is_bound_into_accepted_claim_inputs() {
    let model = model(SIMPLE_PACKAGE);
    let unresolved = "ledger house\nuse simple\n\nitem a\n  label ?label\n  date 2026-01-02\n";
    let resolved = "ledger house\nuse simple\n\nitem a\n  label ?label\n  date 2026-01-02\n\ndecide label_a\n  target a.label\n  value \"A\"\n";
    let (_, unresolved_world, _) = check(unresolved, &model, 100_000).unwrap();
    let (_, resolved_world, _) = check(resolved, &model, 100_000).unwrap();
    assert_ne!(unresolved_world.id(), resolved_world.id());

    let decision = resolved_world
        .evaluation()
        .claims
        .iter()
        .find(|claim| claim.row.id == "a" && claim.phase == Phase::Accepted)
        .unwrap();
    let parsed = model.elaborate(resolved).unwrap();
    let decision_id = decision_claim_id(&parsed.decisions[0]);
    assert!(decision.inputs.contains(&decision_id));
}

#[test]
fn period_views_are_inclusive_and_have_distinct_roots() {
    let model = model(SIMPLE_PACKAGE);
    let source = "ledger house\nuse simple\n\nitem a\n  label \"A\"\n  date 2026-01-02\n\nitem b\n  label \"B\"\n  date 2026-02-03\n";
    let (_, world, _) = check(source, &model, 100_000).unwrap();
    let from: axiom_v2::Date = "2026-01-02".parse().unwrap();
    let to: axiom_v2::Date = "2026-01-02".parse().unwrap();
    let one_day = view_period(&world, &model, "records", Some((&from, &to)), 100_000).unwrap();
    let full = axiom_v2::view(&world, &model, "records", 100_000).unwrap();
    assert_eq!(one_day.evaluation.claims.len(), 1);
    assert_eq!(one_day.period(), Some((&from, &to)));
    assert_ne!(one_day.id(), full.id());
}

#[test]
fn failed_rows_taint_relation_aggregates_but_leave_unrelated_facts_usable() {
    let model = model(COMPLETENESS_PACKAGE);
    let source = "ledger check\nuse completeness\n\nitem valid\n  amount 2\n\nitem negative\n  amount -2\n\nitem unknown\n  amount ?amount\n\nprobe p\n  name \"still usable\"\n\nprobe q\n  name \"missing\"\n";
    let (_, world, certificate) = check(source, &model, 100_000).unwrap();
    let evaluation = world.evaluation();

    assert!(
        evaluation
            .claims
            .iter()
            .any(|claim| claim.row.schema == "positive")
    );
    assert!(
        evaluation
            .claims
            .iter()
            .any(|claim| claim.row.schema == "copied")
    );
    assert!(
        !evaluation
            .claims
            .iter()
            .any(|claim| claim.row.schema == "counted")
    );
    assert!(matches!(
        rule_finding(&world, "p", "count_positive").outcome,
        Outcome::Missing(_)
    ));
    let failed_requirement = rule_finding(&world, "negative", "positive_item");
    assert!(matches!(failed_requirement.outcome, Outcome::Conflict(_)));
    let context = failed_requirement
        .context
        .as_ref()
        .expect("failed requirement should carry bounded explanation context");
    assert_eq!(context.instruction, 0);
    assert_eq!(context.expression, "(gt i.amount 0)");
    assert_eq!(context.bindings["i.amount"], "-2");
    assert!(matches!(
        rule_finding(&world, "q", "missing_nested_reference").outcome,
        Outcome::Missing(_)
    ));
    let linked = evaluation
        .claims
        .iter()
        .find(|claim| {
            claim.row.schema == "linked"
                && claim.row.fields.get("probe") == Some(&Value::Ref("p".into()))
        })
        .expect("nested reference to an existing row should be checked");
    assert_eq!(
        linked.row.fields["item"],
        Value::List(vec![Value::Ref("valid".into())])
    );

    let read_ids: std::collections::BTreeSet<_> =
        evaluation.read_sets.values().flatten().cloned().collect();
    assert!(
        certificate
            .evaluation
            .claims
            .iter()
            .filter(|claim| claim.rule.is_some())
            .all(|claim| claim.inputs.iter().all(|input| {
                certificate
                    .evaluation
                    .claims
                    .iter()
                    .any(|candidate| &candidate.id == input)
                    || evaluation.read_sets.contains_key(input)
            }))
    );
    assert!(read_ids.iter().any(|id| {
        certificate
            .evaluation
            .claims
            .iter()
            .any(|claim| &claim.id == id)
    }));
}
