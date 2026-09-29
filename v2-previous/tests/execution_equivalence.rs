use axiom_v2::{Model, Number, Outcome, Value, check, verify};

const PACKAGE: &str = r#"package execution.composition

line ?line
  account ?account
  amount ?amount ?currency
  active true

request ?request
  account ?account

rule summarize
  for r request
  emit summary
  set total (sum (map (filter (rows line) x (and (eq x.account r.account) x.active)) x x.amount))
  set count (count (filter (rows line) x (and (eq x.account r.account) x.active)))
  set ids (map (filter (rows line) x (and (eq x.account r.account) x.active)) x (ref x))
  set sorted (map (sort (filter (rows line) x (and (eq x.account r.account) x.active)) x x.amount) x x.amount)
  set any (any (filter (rows line) x (eq x.account r.account)) x x.active)
"#;

#[test]
fn composed_queries_keep_exact_values_identity_order_and_replay() {
    let model = Model::compile(&[PACKAGE.to_owned()]).unwrap();
    let mut source =
        "ledger composition\nuse execution.composition\nrequest r\n  account \"cash\"\n".to_owned();
    let mut expected_ids = Vec::new();
    let mut expected_amounts = Vec::new();
    let mut total = Number::zero();
    for i in 0..40 {
        let amount: Number = format!("{}/3", i - 20).parse().unwrap();
        let account = if i % 3 == 0 { "cash" } else { "other" };
        let active = i % 4 != 0;
        source.push_str(&format!(
            "line l{i:02}\n  account \"{account}\"\n  amount {amount} USD\n  active {active}\n"
        ));
        if account == "cash" && active {
            total = total.add(&amount).unwrap();
            expected_ids.push(Value::Ref(format!("l{i:02}")));
            expected_amounts.push(Value::Quantity(amount, "USD".into()));
        }
    }
    expected_amounts.sort();
    let (_, world, certificate) = check(&source, &model, 100_000).unwrap();
    let summary = world
        .evaluation()
        .claims
        .iter()
        .find(|c| c.row.schema == "summary")
        .unwrap();
    assert_eq!(
        summary.row.fields["total"],
        Value::Quantity(total, "USD".into())
    );
    assert_eq!(
        summary.row.fields["count"],
        Value::Number(expected_ids.len().to_string().parse().unwrap())
    );
    assert_eq!(summary.row.fields["ids"], Value::List(expected_ids));
    assert_eq!(summary.row.fields["sorted"], Value::List(expected_amounts));
    assert_eq!(summary.row.fields["any"], Value::Bool(true));
    verify(&source, &model, &certificate, 100_000).unwrap();
    assert_eq!(check(&source, &model, 100_000).unwrap().2, certificate);
}

#[test]
fn a_later_filter_hole_is_not_skipped_by_first() {
    let model = Model::compile(&[r#"package execution.eager
line ?line
  active true
request ?request
  marker true
rule select_first
  for r request
  emit result
  set selected (ref (first (filter (rows line) x x.active)))
"#
    .to_owned()])
    .unwrap();
    let source = "ledger eager\nuse execution.eager\nrequest r\n  marker true\nline a\n  active true\nline z\n  active ?pending\n";
    let (_, world, certificate) = check(source, &model, 10_000).unwrap();
    assert!(
        !world
            .evaluation()
            .claims
            .iter()
            .any(|c| c.row.schema == "result")
    );
    assert!(
        world
            .evaluation()
            .findings
            .iter()
            .any(|f| matches!(f.outcome, Outcome::Missing(_)))
    );
    verify(source, &model, &certificate, 10_000).unwrap();
}

#[test]
fn mapper_fault_precedes_aggregate_dimension_conflict() {
    let model = Model::compile(&[r#"package execution.errors
line ?line
  active true
  amount ?amount ?currency
request ?request
  marker true
rule total
  for r request
  emit result
  set total (sum (map (rows line) x (if x.active x.amount (quantity (div 1 0) "USD"))))
"#
    .to_owned()])
    .unwrap();
    let source = "ledger errors\nuse execution.errors\nrequest r\n  marker true\nline a\n  active true\n  amount 1 USD\nline b\n  active true\n  amount 1 EUR\nline c\n  active false\n  amount 1 USD\n";
    let (_, world, _) = check(source, &model, 10_000).unwrap();
    assert!(
        world
            .evaluation()
            .findings
            .iter()
            .any(|f| matches!(&f.outcome,
        Outcome::Conflict(messages) if messages.iter().any(|m| m.contains("division by zero"))))
    );
}
