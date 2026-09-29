use axiom_v2::{Expr, Model, Value};

fn model(source: &str) -> Result<Model, Vec<axiom_v2::Diagnostic>> {
    Model::compile(&[source.to_owned()])
}

const LEDGER_PACKAGE: &str = r#"package core

person ?person
  name ?name
  nickname "friend"

item ?item
  owner @?person
  peers [?peer]
  state "open"
  details {score: 0, label: ""}

?date note ?note
  content ?content

?date buy ?buy
  account ?account
  units ?units ?asset
  cost ?cost ?currency
  fees 0 ?currency

rule score_person
  for p person
  emit score
  set person (ref p)
  set value 1
  book report

book report
  include score
"#;

#[test]
fn patterns_infer_output_shapes_and_order_rules_topologically() {
    let source = r#"package core

left ?left
  value 0

right ?right
  value 0

rule z_result
  for t total
  emit result
  set value t.value

rule b_total
  for r right
  emit total
  set value r.value

rule a_total
  for l left
  emit total
  set value l.value
"#;
    let compiled = model(source).expect("model should compile");
    let names: Vec<_> = compiled
        .rules()
        .iter()
        .map(|rule| rule.name.as_str())
        .collect();
    assert_eq!(names, ["a_total", "b_total", "z_result"]);

    // The output schemas are compiler results: no package declarations for
    // `total` or `result` are authored.
    for relation in ["total", "result"] {
        let inferred = compiled.schema(relation).expect("output shape is inferred");
        assert!(!inferred.authored);
        assert_eq!(inferred.fields.len(), 1);
        assert_eq!(inferred.fields["value"].to_string(), "number");
    }

    // Make the consumer lexically earliest while reversing the source order.
    // It must still execute after both producers, and the ordered source
    // relations remain identical.
    let reversed_rules = source
        .replace("rule z_result", "rule __z_result")
        .replace("rule b_total", "rule z_result")
        .replace("rule a_total", "rule b_total")
        .replace("rule __z_result", "rule a_total");
    let same_order =
        model(&reversed_rules).expect("declaration names do not determine execution order");
    assert_eq!(
        same_order
            .rules()
            .iter()
            .map(|rule| &rule.source)
            .collect::<Vec<_>>(),
        compiled
            .rules()
            .iter()
            .map(|rule| &rule.source)
            .collect::<Vec<_>>()
    );
}

#[test]
fn multiple_rules_can_produce_one_inferred_relation_and_all_precede_its_readers() {
    let source = r#"package core

left ?left
  value 0

right ?right
  value 0

rule result_rule
  for t total
  emit result
  set value t.value

rule right_rule
  for r right
  emit total
  set value r.value

rule left_rule
  for l left
  emit total
  set value l.value
"#;
    let compiled = model(source).expect("union producers should compile");
    let names: Vec<_> = compiled
        .rules()
        .iter()
        .map(|rule| rule.name.as_str())
        .collect();
    assert_eq!(names, ["left_rule", "right_rule", "result_rule"]);
    assert_eq!(
        compiled.schema("total").unwrap().fields["value"]
            .ty
            .to_string(),
        "number"
    );

    let incompatible = r#"package core
left ?left
  value 0
right ?right
  value ""
rule left_rule
  for l left
  emit shared
  set value l.value
rule right_rule
  for r right
  emit shared
  set value r.value
"#;
    assert!(
        model(incompatible)
            .unwrap_err()
            .iter()
            .any(|error| { error.message.contains("incompatible producer types") })
    );

    let mixed_scope = format!(
        "{}\nbook tax\n  include total\n",
        source.replace("set value r.value", "set value r.value\n  book tax")
    );
    assert!(
        model(&mixed_scope)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("same world or book"))
    );
}

#[test]
fn ordinary_entries_apply_defaults_holes_references_decisions_and_date_headers() {
    let compiled = model(LEDGER_PACKAGE).expect("patterns and rules should compile");
    let source = r#"ledger home
use core

person ada
  name "Ada"

person bo
  name "Bo"

item first
  owner ?owner
  peers [@bo, @ada]
  details {score: 3, label: "first"}

item second
  owner @bo
  peers []
  state "closed"
  details {score: 4, label: "second"}

2026-04-01 note memo
  content "date supplied by header"

2026-01-04 buy purchased
  account brokerage
  units 2 ABC
  cost 10 USD

decide owner_choice
  target first.owner
  value @ada
"#;
    let document = compiled.elaborate(source).expect("source should elaborate");
    let item = document.rows.iter().find(|row| row.id == "first").unwrap();
    assert_eq!(item.fields["owner"], Value::Ref("ada".into()));
    assert_eq!(
        item.fields["peers"],
        Value::List(vec![Value::Ref("bo".into()), Value::Ref("ada".into())])
    );
    assert_eq!(
        item.fields["details"],
        Value::parse("{score: 3, label: \"first\"}").unwrap()
    );
    assert_eq!(item.fields["state"], Value::Text("open".into()));

    let second = document.rows.iter().find(|row| row.id == "second").unwrap();
    assert_eq!(second.fields["state"], Value::Text("closed".into()));
    let ada = document.rows.iter().find(|row| row.id == "ada").unwrap();
    assert_eq!(ada.fields["nickname"], Value::Text("friend".into()));
    assert_eq!(
        document
            .rows
            .iter()
            .find(|row| row.id == "memo")
            .unwrap()
            .fields["date"],
        Value::parse("2026-04-01").unwrap()
    );
    assert_eq!(document.decisions.len(), 1);
    assert_eq!(document.decisions[0].target, "first.owner");
    assert!(document.locations.contains_key("first"));

    // Omitted fees share the currency captured from cost; inferred defaults
    // are visible to callers and remain exact typed values.
    assert_eq!(
        document.inferred["purchased.fees"],
        Value::parse("0 USD").unwrap()
    );
    assert_eq!(
        document.inferred["ada.nickname"],
        Value::Text("friend".into())
    );
    assert_eq!(document.inferred["first.state"], Value::Text("open".into()));

    let score = compiled.schema("score").expect("rule output is inferred");
    assert!(!score.authored);
    assert_eq!(score.fields.len(), 2);
    assert_eq!(score.fields["person"].to_string(), "ref:person");
    assert_eq!(score.fields["value"].to_string(), "number");
}

#[test]
fn decisions_resolve_only_direct_typed_holes() {
    let compiled = model(LEDGER_PACKAGE).unwrap();
    let source = r#"ledger home
use core
person ada
  name "Ada"
item thing
  owner ?owner
  peers []
  details {score: 1, label: "x"}
decide override
  target thing.state
  value "closed"
"#;
    assert!(
        compiled.elaborate(source).unwrap_err()[0]
            .message
            .contains("not a hole")
    );

    let duplicate_targets = source.replace(
        "target thing.state\n  value \"closed\"",
        "target thing.owner\n  value @ada\n\ndecide again\n  target thing.owner\n  value @ada",
    );
    assert!(compiled.elaborate(&duplicate_targets).is_err());

    let wrong_type = source.replace(
        "target thing.state\n  value \"closed\"",
        "target thing.owner\n  value \"not a reference\"",
    );
    assert!(
        compiled.elaborate(&wrong_type).unwrap_err()[0]
            .message
            .contains("expected ref:person")
    );
}

#[test]
fn missing_fields_remain_typed_holes_and_dangling_or_derived_entries_fail() {
    let compiled = model(LEDGER_PACKAGE).unwrap();
    let incomplete = r#"ledger home
use core
person ada
  name "Ada"
2026-01-04 buy partial
  account brokerage
  units 2 ABC
"#;
    let document = compiled
        .elaborate(incomplete)
        .expect("underdetermined entries remain usable");
    let partial = document
        .rows
        .iter()
        .find(|row| row.id == "partial")
        .unwrap();
    assert_eq!(partial.fields["cost"], Value::Hole("cost".into()));
    assert_eq!(partial.fields["fees"], Value::Hole("currency".into()));
    assert_eq!(partial.fields["units"], Value::parse("2 ABC").unwrap());
    assert!(document.rows.iter().any(|row| row.id == "ada"));

    let dangling_ref = r#"ledger home
use core
person ada
  name "Ada"
item thing
  owner @unknown
  peers []
  details {score: 1, label: "x"}
"#;
    assert!(
        compiled.elaborate(dangling_ref).unwrap_err()[0]
            .message
            .contains("does not resolve")
    );

    let derived_source = r#"ledger home
use core
score result
  value 1
"#;
    assert!(
        compiled.elaborate(derived_source).unwrap_err()[0]
            .message
            .contains("cannot be authored")
    );
}

#[test]
fn authored_schema_declarations_are_rejected() {
    let obsolete = "package old\nform purchase\n  value 0\n";
    assert!(
        model(obsolete).unwrap_err()[0]
            .message
            .contains("authored schema declarations are removed")
    );
}

#[test]
fn model_pin_keeps_exact_package_bytes_but_definition_ids_ignore_trivia() {
    let original = model(LEDGER_PACKAGE).unwrap();
    let edited_source = format!("# harmless source comment\n{LEDGER_PACKAGE}");
    let edited = model(&edited_source).unwrap();
    assert_ne!(
        original.id(),
        edited.id(),
        "the model pin includes exact package bytes"
    );
    assert_eq!(
        original.definition_id("person"),
        edited.definition_id("person")
    );
    assert_eq!(original.definition_id("core"), edited.definition_id("core"));
}

#[test]
fn package_visibility_multiple_roots_and_book_scopes_are_enforced() {
    let a = r#"package a
alpha ?alpha
  value 0
rule alpha_rule
  for x beta
  emit alpha_fact
  set value x.value
"#;
    let b = r#"package b
beta ?beta
  value 0
"#;
    let visibility_errors = Model::compile(&[a.to_owned(), b.to_owned()]).unwrap_err();
    assert!(
        visibility_errors
            .iter()
            .any(|error| error.message.contains("must import `b`"))
    );

    let a_with_dependency = a.replacen("package a\n", "package a\nuse b\n", 1);
    Model::compile(&[a_with_dependency, b.to_owned()])
        .expect("a rule may read a pattern from an imported package");

    let independent = "package independent\nunused ?unused\n  value 0\n";
    Model::compile(&[LEDGER_PACKAGE.to_owned(), independent.to_owned()])
        .expect("a model may pin multiple independent package roots");
    assert!(
        model("package a\nuse missing\nx ?x\n  value 0\n").unwrap_err()[0]
            .message
            .contains("missing package")
    );

    let leakage = r#"package books

source ?source
  value 0

rule tax_rule
  for s source
  emit taxable
  set value s.value
  book tax

rule cash_rule
  for t taxable
  emit cash_record
  set value t.value
  book cash

book tax
  include taxable

book cash
  include cash_record
"#;
    assert!(
        model(leakage)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("another book"))
    );
}

#[test]
fn expression_parser_handles_nested_prefix_forms_and_rejects_malformed_input() {
    assert!(Expr::parse("(mul -1 b.cash_basis)").is_ok());
    assert!(Expr::parse("10 USD").is_ok());
    assert!(Expr::parse("(eq (get (last p.history) state) \"settled\")").is_ok());
    assert!(Expr::parse("(add 1").is_err());
    assert!(Expr::parse("s.amount extra").is_err());
}

#[test]
fn compiler_rejects_unknown_operators_fields_and_recursive_relations() {
    let unknown_operator = r#"package core
source ?source
  value 0
rule derive
  for s source
  emit out
  set value (nonsense s.value)
"#;
    assert!(
        model(unknown_operator).unwrap_err()[0]
            .message
            .contains("unknown expression operator")
    );

    let unknown_field = unknown_operator.replace("(nonsense s.value)", "s.missing");
    assert!(
        model(&unknown_field)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("unknown field `missing`"))
    );

    let recursive = r#"package core
source ?source
  value 0
rule one
  for s source
  let x (rows second)
  emit first
  set value s.value
rule two
  for s source
  let x (rows first)
  emit second
  set value s.value
"#;
    assert!(
        model(recursive)
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("recursive rule dependencies"))
    );
}
