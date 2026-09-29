use axiom_v2::{Model, Value};

fn compile(source: &str) -> Result<Model, Vec<axiom_v2::Diagnostic>> {
    Model::compile(&[source.to_owned()])
}

fn messages(errors: &[axiom_v2::Diagnostic]) -> String {
    errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn inferred_output_names_cannot_shadow_other_definitions() {
    let cases = [
        // A relation may not reuse its producer's rule name.
        r#"package core
item ?item
  amount ?amount

rule collision
  for i item
  emit collision
  set value i.amount
"#,
        // Nor may it take a book's name (which would alias definition identity).
        r#"package core
item ?item
  amount ?amount

book report
  include report

rule producer
  for i item
  emit report
  set value i.amount
"#,
        // Nor the package name.
        r#"package core
item ?item
  amount ?amount

rule producer
  for i item
  emit core
  set value i.amount
"#,
    ];

    for source in cases {
        let result = compile(source);
        assert!(
            result.is_err(),
            "an inferred relation must not shadow a rule, book, or package"
        );
    }
}

#[test]
fn a_date_prefix_cannot_be_silently_discarded_by_an_undated_pattern() {
    let model = compile(
        r#"package core
item ?item
  amount ?amount
"#,
    )
    .unwrap();

    let errors = model
        .elaborate(
            r#"ledger test
use core
2026-01-04 item item_1
  amount 12
"#,
        )
        .expect_err("date-prefixed syntax must either supply or reject a date");
    assert!(!messages(&errors).is_empty());
}

#[test]
fn structural_source_heads_are_not_usable_as_entry_kinds() {
    for kind in ["ledger", "decide"] {
        let source = format!(
            "package core\n{kind} ?row\n  value ?value\n"
        );
        assert!(
            compile(&source).is_err(),
            "`{kind}` is parsed as source structure, not an ordinary entry"
        );
    }
}

#[test]
fn repeated_captures_unify_and_scalar_amounts_keep_their_magnitude() {
    let model = compile(
        r#"package core
pair ?pair
  left ?shared
  right ?shared
  echoed ?shared

trade ?trade
  cost ?amount ?currency
  fee 0 ?currency
"#,
    )
    .unwrap();

    let document = model
        .elaborate(
            r#"ledger test
use core
pair matching
  left Ada
  right Ada

trade scalar_cost
  cost 7
  fee 3 USD
"#,
        )
        .expect("equal captures and a unit inferred from another field should elaborate");
    let trade = document
        .rows
        .iter()
        .find(|row| row.id == "scalar_cost")
        .unwrap();
    let pair = document
        .rows
        .iter()
        .find(|row| row.id == "matching")
        .unwrap();
    assert_eq!(pair.fields["echoed"], Value::Text("Ada".into()));
    assert_eq!(trade.fields["cost"], Value::parse("7 USD").unwrap());
    assert_eq!(trade.fields["fee"], Value::parse("3 USD").unwrap());

    let errors = model
        .elaborate(
            r#"ledger test
use core
pair conflicting
  left Ada
  right Grace
"#,
        )
        .expect_err("one named capture cannot unify to two different values");
    assert!(messages(&errors).contains("capture `?shared` conflicts"));
}

#[test]
fn an_explicit_open_hole_does_not_conflict_with_a_concrete_repeated_capture() {
    let model = compile(
        r#"package core
pair ?pair
  left ?shared
  right ?shared
  echoed ?shared
"#,
    )
    .unwrap();
    let document = model
        .elaborate(
            r#"ledger test
use core
pair unresolved
  left ?undecided
  right Ada
"#,
        )
        .expect("an open source hole should not contradict a concrete capture");
    let pair = document
        .rows
        .iter()
        .find(|row| row.id == "unresolved")
        .unwrap();
    assert_eq!(pair.fields["left"], Value::Hole("undecided".into()));
    assert_eq!(pair.fields["right"], Value::Text("Ada".into()));
    assert_eq!(pair.fields["echoed"], Value::Text("Ada".into()));
}

#[test]
fn pattern_references_constrain_occurrence_kind_not_identity() {
    let model = compile(
        r#"package core
party ?party
  name ?name

thing ?thing
  owner @?party
"#,
    )
    .unwrap();

    model
        .elaborate(
            r#"ledger test
use core
party ada
  name Ada
thing owned
  owner @ada
"#,
        )
        .expect("any concrete party occurrence may fill the pattern reference");

    let errors = model
        .elaborate(
            r#"ledger test
use core
party ada
  name Ada
thing other
  owner @ada
thing wrong
  owner @other
"#,
        )
        .expect_err("the reference must target a party occurrence");
    assert!(messages(&errors).contains("expected `party`"));
}

#[test]
fn heterogeneous_list_positions_keep_their_individual_reference_types() {
    let model = compile(
        r#"package core
party ?party
  name ?name

account ?account
  name ?name

links ?links
  targets [@?party, @?account]
"#,
    )
    .unwrap();

    model
        .elaborate(
            r#"ledger test
use core
party p
  name Person
account a
  name Checking
links valid
  targets [@p, @a]
"#,
        )
        .expect("both pattern-typed references resolve in their positions");

    let errors = model
        .elaborate(
            r#"ledger test
use core
party p
  name Person
account a
  name Checking
links swapped
  targets [@a, @p]
"#,
        )
        .expect_err("heterogeneous list elements retain their reference constraints");
    assert!(messages(&errors).contains("expected `party`"));
}

#[test]
fn unconstrained_any_values_still_validate_nested_reference_existence() {
    let model = compile(
        r#"package core
thing ?thing
  payload ?payload
"#,
    )
    .unwrap();

    model
        .elaborate(
            r#"ledger test
use core
thing target
  payload "present"
thing source
  payload {links: [@target]}
"#,
        )
        .expect("Any accepts arbitrary values while still resolving their references");

    let errors = model
        .elaborate(
            r#"ledger test
use core
thing source
  payload {links: [@missing]}
"#,
        )
        .expect_err("a reference inside an unconstrained record must not dangle");
    assert!(messages(&errors).contains("reference `@missing`"));
    assert!(messages(&errors).contains("does not resolve"));
}

#[test]
fn generated_occurrences_are_stable_under_trivia_edits() {
    let model = compile(
        r#"package core
?date item ?item
  value ?value
"#,
    )
    .unwrap();

    let plain = model
        .elaborate(
            r#"ledger test
use core
item
  value A
item
  value B
"#,
        )
        .unwrap();
    let commented = model
        .elaborate(
            r#"# before ledger
ledger test # inline comment
use core

# before first entry
item
  value A # inline field comment

# between entries
item
  value B
"#,
        )
        .unwrap();

    let semantic_rows = |document: &axiom_v2::TypedDocument| {
        document
            .rows
            .iter()
            .map(|row| (row.id.clone(), row.schema.clone(), row.fields.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(semantic_rows(&plain), semantic_rows(&commented));
    assert_eq!(plain.rows[0].id, "entry/1");
    assert_eq!(plain.rows[1].id, "entry/2");
}

#[test]
fn field_typos_do_not_become_new_open_fields() {
    let model = compile(
        r#"package core
item ?item
  value ?value
"#,
    )
    .unwrap();
    let errors = model
        .elaborate(
            r#"ledger test
use core
item one
  valu 1
"#,
        )
        .expect_err("pattern fields define the closed row shape");
    assert!(messages(&errors).contains("unknown field `valu`"));
}

#[test]
fn expression_inference_rejects_unbound_names_bad_arity_and_shadowing() {
    let cases = [
        (
            "unbound name",
            r#"package core
item ?item
  amount ?amount
rule bad_name
  for i item
  emit output
  set value missing.amount
"#,
        ),
        (
            "wrong arity",
            r#"package core
item ?item
  amount ?amount
rule bad_arity
  for i item
  emit output
  set value (add i.amount)
"#,
        ),
        (
            "shadowed binder",
            r#"package core
item ?item
  amount ?amount
rule shadow
  for i item
  let i 1
  emit output
  set value i.amount
"#,
        ),
    ];

    for (label, source) in cases {
        assert!(compile(source).is_err(), "static inference must reject {label}");
    }
}

#[test]
fn expression_inference_rejects_known_non_numeric_and_non_list_operands() {
    let cases = [
        (
            "sum of text",
            r#"package core
item ?item
  labels ["not numeric"]
rule bad_sum
  for i item
  emit output
  set value (sum i.labels)
"#,
        ),
        (
            "contains on text",
            r#"package core
item ?item
  label "not a list"
rule bad_contains
  for i item
  require (contains i.label "x")
  emit output
  set value true
"#,
        ),
        (
            "multiplication of text values",
            r#"package core
item ?item
  left "x"
  right "y"
rule bad_multiply
  for i item
  emit output
  set value (mul i.left i.right)
"#,
        ),
    ];
    for (label, source) in cases {
        assert!(compile(source).is_err(), "static inference must reject {label}");
    }
}

#[test]
fn get_treats_a_bare_second_argument_as_a_field_even_if_bound() {
    let model = compile(
        r#"package core
item ?item
  i ?value
rule copy_field
  for i item
  emit copied
  set value (get i i)
"#,
    )
    .expect("the second `i` is the static field name, not a lexical lookup");

    let (_, world, _) = axiom_v2::check(
        r#"ledger test
use core
item one
  i present
"#,
        &model,
        10_000,
    )
    .expect("static field access should also evaluate consistently");
    let copied = world
        .evaluation()
        .claims
        .iter()
        .find(|claim| claim.row.schema == "copied")
        .expect("rule should produce a copied row");
    assert_eq!(copied.row.fields["value"], Value::Text("present".into()));
}

#[test]
fn inferred_nested_output_shapes_reject_missing_fields_in_consumers() {
    let source = r#"package core
item ?item
  details {score: 1}

rule copy_details
  for i item
  emit copied
  set details i.details

rule inspect_missing
  for c copied
  emit inspected
  set value c.details.missing
"#;
    let errors = compile(source)
        .expect_err("output record fields must be inferred and checked by downstream rules");
    assert!(
        messages(&errors).contains("missing"),
        "diagnostic should identify the missing nested field: {}",
        messages(&errors)
    );
}

#[test]
fn derived_relations_cannot_shadow_the_implicit_occurrence_id_field() {
    let source = r#"package core
item ?item
  value ?value

rule bad_identity
  for i item
  emit output
  set id "not-the-occurrence"
  set value i.value
"#;
    assert!(
        compile(source).is_err(),
        "`id` is the relation's implicit occurrence identity, not a user field"
    );
}

#[test]
fn pattern_defaults_change_semantic_definition_ids_but_comments_do_not() {
    let zero = compile(
        r#"package core
item ?item
  amount 0
"#,
    )
    .unwrap();
    let one = compile(
        r#"package core
item ?item
  amount 1
"#,
    )
    .unwrap();
    let commented = compile(
        r#"# harmless source trivia
package core
item ?item
  amount 0
"#,
    )
    .unwrap();

    assert_ne!(zero.definition_id("item"), one.definition_id("item"));
    assert_ne!(zero.definition_id("core"), one.definition_id("core"));
    assert_eq!(zero.definition_id("item"), commented.definition_id("item"));
    assert_eq!(zero.definition_id("core"), commented.definition_id("core"));
}
