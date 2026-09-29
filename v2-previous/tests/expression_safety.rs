use axiom_v2::{Model, Outcome, Value, check};

fn compile(source: &str) -> Model {
    Model::compile(&[source.to_owned()])
        .unwrap_or_else(|errors| panic!("package should compile: {errors:#?}"))
}

fn check_world(model: &Model, source: &str, limit: usize) -> axiom_v2::World {
    let (_, world, _) = check(source, model, limit)
        .unwrap_or_else(|errors| panic!("ledger should check: {errors:#?}"));
    world
}

fn finding<'a>(world: &'a axiom_v2::World, rule: &str) -> &'a axiom_v2::Finding {
    world
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.rule == rule)
        .unwrap_or_else(|| panic!("missing finding for rule `{rule}`"))
}

#[test]
fn quantifiers_keep_searching_after_an_unknown_body() {
    let model = compile(
        r#"package expression.quantifiers

?date quantifier_input ?id
  every [?every_item, false]
  some [?some_item, true]

rule check_quantifiers
  for q quantifier_input
  emit quantifier_result
  set every_ok (all q.every x x)
  set some_ok (any q.some x x)
"#,
    );
    let world = check_world(
        &model,
        "ledger quantifier_test\nuse expression.quantifiers\n2026-01-01 quantifier_input q\n  every [?pending, false]\n  some [?pending, true]\n",
        10_000,
    );

    assert!(
        !world
            .evaluation()
            .findings
            .iter()
            .any(|finding| finding.rule == "check_quantifiers")
    );
    let result = world
        .evaluation()
        .claims
        .iter()
        .find(|claim| claim.row.schema == "quantifier_result")
        .expect("known false/true witnesses should resolve both quantifiers");
    assert_eq!(result.row.fields.get("every_ok"), Some(&Value::Bool(false)));
    assert_eq!(result.row.fields.get("some_ok"), Some(&Value::Bool(true)));
}

#[test]
fn equality_and_contains_report_nested_holes_as_missing() {
    let model = compile(
        r#"package expression.nested_holes

?date equality_input ?id
  nested {inner: ?inner}

?date contains_input ?id
  candidates [{inner: ?candidate_inner}]
  target {inner: "wanted"}

rule compare_nested
  for e equality_input
  require (eq e.nested e.nested)
  emit equality_result
  set ok true

rule find_nested
  for c contains_input
  require (contains c.candidates c.target)
  emit contains_result
  set ok true
"#,
    );
    let world = check_world(
        &model,
        "ledger nested_test\nuse expression.nested_holes\n2026-01-01 equality_input eq_row\n  nested {inner: ?pending}\n2026-01-02 contains_input contains_row\n  candidates [{inner: ?pending}]\n",
        10_000,
    );

    assert!(matches!(
        &finding(&world, "compare_nested").outcome,
        Outcome::Missing(_)
    ));
    assert!(matches!(
        &finding(&world, "find_nested").outcome,
        Outcome::Missing(_)
    ));
}

#[test]
fn mixed_dimension_sort_returns_conflict_without_panicking() {
    let model = compile(
        r#"package expression.sort_dimensions

?date sort_input ?id
  marker false
  quote ?amount ?unit

rule reject_incomparable_keys
  for s sort_input
  require (eq (first (sort [0, 1 USD, 1 EUR] x x)) 0)
  emit sort_result
  set ok true
"#,
    );
    let world = check_world(
        &model,
        "ledger sort_test\nuse expression.sort_dimensions\n2026-01-01 sort_input s\n  marker true\n  quote 5 USD\n",
        10_000,
    );

    assert!(matches!(
        &finding(&world, "reject_incomparable_keys").outcome,
        Outcome::Conflict(_)
    ));
}

#[test]
fn expression_budget_exhaustion_stays_incomplete() {
    let model = compile(
        r#"package expression.budget

?date budget_input ?id
  marker false
  count 0

rule metered_expression
  for s budget_input
  emit budget_result
  set ok (eq s.marker true)
"#,
    );
    let world = check_world(
        &model,
        "ledger budget_test\nuse expression.budget\n2026-01-01 budget_input s\n  marker true\n  count 7\n",
        1,
    );

    assert!(matches!(
        &finding(&world, "metered_expression").outcome,
        Outcome::Incomplete(_)
    ));
}

#[test]
fn ref_operator_requires_an_explicit_reference_id() {
    let model = compile(
        r#"package expression.references

?date target ?target
  marker false

?date reference_input ?id
  marker false
  target @?target

rule reject_text_id
  for s reference_input
  require (eq (ref {id: "not-a-reference"}) s.target)
  emit reference_result
  set ok true
"#,
    );
    let world = check_world(
        &model,
        "ledger reference_test\nuse expression.references\n2026-01-01 target known\n  marker false\n2026-01-02 reference_input s\n  marker true\n  target @known\n",
        10_000,
    );

    assert!(matches!(
        &finding(&world, "reject_text_id").outcome,
        Outcome::Conflict(_)
    ));
}

#[test]
fn unconstrained_pattern_captures_still_validate_reference_existence() {
    let model = compile(
        r#"package expression.any_references

?date reference_input ?id
  opaque ?value
"#,
    );
    let errors = check(
        "ledger dangling_reference\nuse expression.any_references\n2026-01-01 reference_input row\n  opaque @not_there\n",
        &model,
        10_000,
    )
    .expect_err("an unconstrained capture must not admit a dangling reference");

    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("does not resolve"))
    );
}

#[test]
fn inferred_output_relations_require_producer_types_to_agree() {
    let source = r#"package expression.output_types

?date input ?id
  marker false

rule produce_bool
  for i input
  emit shared
  set value true

rule produce_text
  for i input
  emit shared
  set value "not a bool"
"#;
    let errors = Model::compile(&[source.to_owned()])
        .expect_err("producers of one relation must agree on inferred field types");

    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("incompatible producer types"))
    );
}

#[test]
fn literal_reference_outputs_resolve_against_existing_occurrences() {
    let model = compile(
        r#"package expression.literal_references

?date target ?target
  marker false

?date source ?id
  marker false

rule link_known_target
  for s source
  emit literal_link
  set target @known
"#,
    );
    let world = check_world(
        &model,
        "ledger literal_reference_test\nuse expression.literal_references\n2026-01-01 target known\n2026-01-02 source source_row\n",
        10_000,
    );

    assert!(
        world
            .evaluation()
            .claims
            .iter()
            .any(|claim| claim.row.schema == "literal_link"
                && claim.row.fields.get("target") == Some(&Value::Ref("known".into())))
    );
    assert!(
        !world
            .evaluation()
            .findings
            .iter()
            .any(|finding| finding.rule == "link_known_target")
    );
}
