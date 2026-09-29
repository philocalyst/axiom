use axiom_v2::{check, Model, Outcome, Value};

fn model(source: &str) -> Model {
    Model::compile(&[source.to_owned()]).expect("package should compile")
}

fn finding<'a>(world: &'a axiom_v2::World, subject: &str, rule: &str) -> &'a axiom_v2::Finding {
    world
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == subject && finding.rule == rule)
        .expect("expected rule finding")
}

#[test]
fn a_typed_choice_hole_rejects_rows_from_the_wrong_pattern() {
    let package = r#"package authority

buy ?buy
  units ?units ?asset

other ?other
  units ?units ?asset

sell ?sell
  units ?units ?asset
  lot @?buy

rule choose_lot
  for s sell
  choose b s.lot (concat (rows buy) (rows other))
  require (le s.units b.units)
  emit selected
  set sell (ref s)
  set lot (ref b)
"#;
    let model = model(package);
    let source = "ledger audit\nuse authority\n\nbuy b1\n  units 10 ABC\n\nother o1\n  units 10 ABC\n\nsell sale\n  units 5 ABC\n  lot ?lot\n";

    let (_, world, _) = check(source, &model, 100_000).expect("ledger should elaborate");

    assert_eq!(
        finding(&world, "sale", "choose_lot").outcome,
        Outcome::Alternatives(vec![Value::Ref("b1".into())]),
        "the selector is ref:buy, so a valid row from pattern `other` is not an eligible choice"
    );
}

#[test]
fn incomplete_relation_diagnostics_bound_the_number_of_rendered_causes() {
    let package = r#"package completeness

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
  let count (count (rows positive))
  emit counted
  set probe (ref p)
  set count count
"#;
    let model = model(package);
    let mut source = String::from("ledger audit\nuse completeness\n\n");
    for index in 0..512 {
        source.push_str(&format!("item failed_{index:04}\n  amount -1\n\n"));
    }
    source.push_str("probe p\n  name \"aggregate\"\n");

    let (_, world, _) = check(&source, &model, 1_000_000).expect("ledger should elaborate");
    let Outcome::Missing(messages) = &finding(&world, "p", "count_positive").outcome else {
        panic!("an incomplete relation should produce a missing outcome");
    };
    let diagnostic_bytes: usize = messages.iter().map(String::len).sum();

    assert!(
        diagnostic_bytes <= 4_096,
        "incomplete-cause summary must stay bounded, got {diagnostic_bytes} bytes"
    );
}

#[test]
fn choice_candidate_enumeration_exhausts_the_declared_work_budget() {
    let package = r#"package chooser

buy ?buy
  label ?label

sell ?sell
  lot @?buy

rule choose_lot
  for s sell
  choose b s.lot (rows buy)
  emit selected
  set sell (ref s)
  set lot (ref b)
"#;
    let model = model(package);
    let mut source = String::from("ledger audit\nuse chooser\n\n");
    for index in 0..512 {
        source.push_str(&format!("buy b{index:04}\n  label \"candidate\"\n\n"));
    }
    source.push_str("sell sale\n  lot @b0511\n");

    // Relation loading and list evaluation fit under this budget; enumerating
    // every candidate for the explicit-membership check does not.
    let (_, world, _) = check(&source, &model, 5_300).expect("ledger should elaborate");

    assert!(
        matches!(
            finding(&world, "sale", "choose_lot").outcome,
            Outcome::Incomplete(_)
        ),
        "enumerating a large choice relation must not bypass the work budget"
    );
    assert!(!world
        .evaluation()
        .claims
        .iter()
        .any(|claim| claim.row.schema == "selected"));
}
