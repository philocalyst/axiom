use axiom_v2::{Model, Number, Outcome, Phase, Value, World, check, verify, view};
use std::str::FromStr;

fn personal_model() -> Model {
    Model::compile(&[include_str!("../models/personal.axm").to_owned()])
        .unwrap_or_else(|errors| panic!("personal package did not compile: {errors:#?}"))
}

fn full_model() -> Model {
    Model::compile(&[
        include_str!("../models/personal.axm").to_owned(),
        include_str!("../models/community_rewards.axm").to_owned(),
    ])
    .unwrap_or_else(|errors| panic!("package closure did not compile: {errors:#?}"))
}

fn checked(source: &str, model: &Model) -> (axiom_v2::TypedDocument, World, axiom_v2::Certificate) {
    check(source, model, 100_000)
        .unwrap_or_else(|errors| panic!("ledger did not check: {errors:#?}"))
}

fn field<'a>(claim: &'a axiom_v2::Claim, name: &str) -> &'a Value {
    claim
        .row
        .fields
        .get(name)
        .unwrap_or_else(|| panic!("{} claim has no `{name}` field", claim.row.schema))
}

fn fact_for<'a>(world: &'a World, schema: &str, key: &str, value: &Value) -> &'a axiom_v2::Claim {
    world
        .evaluation()
        .claims
        .iter()
        .find(|claim| claim.row.schema == schema && field(claim, key) == value)
        .unwrap_or_else(|| panic!("missing {schema} fact where {key} = {value}"))
}

fn assert_quantity(claim: &axiom_v2::Claim, name: &str, expected: &str) {
    assert_eq!(field(claim, name).to_string(), expected);
}

fn sum_quantities<'a>(
    claims: impl IntoIterator<Item = &'a axiom_v2::Claim>,
    field_name: &str,
) -> (String, String) {
    let mut total = Number::from_str("0").expect("zero is an exact number");
    let mut unit = None;
    for claim in claims {
        let Value::Quantity(amount, current_unit) = field(claim, field_name) else {
            panic!("{}.{field_name} is not a quantity", claim.row.schema);
        };
        if let Some(expected) = &unit {
            assert_eq!(expected, current_unit, "cannot sum different units");
        } else {
            unit = Some(current_unit.clone());
        }
        total = total.add(amount).expect("same-unit quantities should sum");
    }
    (
        total.to_string(),
        unit.expect("sum needs at least one quantity"),
    )
}

#[test]
fn mixed_ledger_keeps_ambiguity_and_oversell_visible() {
    let model = personal_model();
    let (_, world, _) = checked(include_str!("../examples/mixed_ledger.axm"), &model);

    let ambiguity = world
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == "sale_beta_ambiguous")
        .expect("ambiguous sale should have a rule finding");
    assert_eq!(ambiguity.rule, "investment_gain");
    assert_eq!(
        ambiguity.outcome,
        Outcome::Alternatives(vec![
            Value::Ref("lot_beta_one".into()),
            Value::Ref("lot_beta_two".into()),
        ])
    );

    for sale in ["sale_alpha_part", "sale_alpha_over"] {
        let finding = world
            .evaluation()
            .findings
            .iter()
            .find(|finding| finding.subject == sale && finding.rule == "investment_gain")
            .expect("each linked sale should have a gain finding");
        assert!(
            matches!(finding.outcome, Outcome::Conflict(_)),
            "{sale}: {:?}",
            finding.outcome
        );
    }

    let position = fact_for(
        &world,
        "payment_position",
        "payment",
        &Value::Ref("payment_1".into()),
    );
    assert_eq!(field(position, "state"), &Value::Text("settled".into()));
    assert_quantity(position, "amount", "600 USD");

    let cash = view(&world, &model, "cash", 100_000).expect("partial cash view should replay");
    let investment_sales: Vec<_> = cash
        .evaluation
        .claims
        .iter()
        .filter(|claim| {
            claim.row.schema == "cash_entry"
                && field(claim, "kind") == &Value::Text("investment sale".into())
        })
        .collect();
    assert_eq!(investment_sales.len(), 3);
    assert_eq!(
        sum_quantities(investment_sales.iter().copied(), "amount"),
        ("1641".into(), "USD".into())
    );

    let satisfactions: Vec<_> = world
        .evaluation()
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "satisfaction")
        .collect();
    assert_eq!(satisfactions.len(), 2);
    for satisfaction in satisfactions {
        assert_quantity(satisfaction, "invoice_remaining", "400 USD");
        assert_quantity(satisfaction, "payment_remaining", "0 USD");
    }

    let tax = view(&world, &model, "tax", 100_000).expect("partial tax view should replay");
    assert!(
        tax.evaluation
            .claims
            .iter()
            .all(|claim| claim.row.schema != "tax_entry")
    );
    assert!(tax.evaluation.findings.iter().any(|finding| {
        finding.subject == "sale_beta_ambiguous"
            && matches!(finding.outcome, Outcome::Alternatives(_))
    }));
    assert!(tax.evaluation.findings.iter().any(|finding| {
        finding.subject == "sale_alpha_over" && matches!(finding.outcome, Outcome::Conflict(_))
    }));
}

#[test]
fn explicit_decision_yields_conserved_lot_basis_and_pure_books() {
    let model = personal_model();
    let source = include_str!("../examples/corrected_ledger.axm");
    let (document, world, certificate) = checked(source, &model);

    assert_eq!(document.decisions.len(), 1);
    assert!(world.evaluation().findings.is_empty());
    verify(source, &model, &certificate, 100_000).expect("certificate should replay exactly");

    let first = fact_for(
        &world,
        "gain",
        "sell",
        &Value::Ref("sale_alpha_part".into()),
    );
    assert_quantity(first, "basis", "404 USD");
    assert_quantity(first, "amount", "192 USD");
    let second = fact_for(
        &world,
        "gain",
        "sell",
        &Value::Ref("sale_alpha_over".into()),
    );
    assert_quantity(second, "basis", "606 USD");
    assert_quantity(second, "amount", "290 USD");
    let beta = fact_for(
        &world,
        "gain",
        "sell",
        &Value::Ref("sale_beta_ambiguous".into()),
    );
    assert_quantity(beta, "basis", "90 USD");
    assert_quantity(beta, "amount", "59 USD");

    let cash = view(&world, &model, "cash", 100_000).expect("cash view should replay");
    let cash_entries: Vec<_> = cash
        .evaluation
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "cash_entry")
        .collect();
    assert_eq!(cash_entries.len(), 7);
    assert!(cash_entries.iter().any(|entry| field(entry, "kind")
        == &Value::Text("settled payment".into())
        && field(entry, "amount").to_string() == "600 USD"));
    assert_eq!(
        sum_quantities(cash_entries.iter().copied(), "amount"),
        ("671".into(), "USD".into())
    );

    let tax = view(&world, &model, "tax", 100_000).expect("tax view should replay");
    let tax_amounts: Vec<_> = tax
        .evaluation
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "tax_entry")
        .map(|claim| field(claim, "amount").to_string())
        .collect();
    assert_eq!(tax_amounts.len(), 3);
    assert!(tax_amounts.contains(&"192 USD".to_owned()));
    assert!(tax_amounts.contains(&"290 USD".to_owned()));
    assert!(tax_amounts.contains(&"59 USD".to_owned()));
    assert_eq!(
        sum_quantities(
            tax.evaluation
                .claims
                .iter()
                .filter(|claim| claim.row.schema == "tax_entry"),
            "amount"
        ),
        ("541".into(), "USD".into())
    );

    let accrual = view(&world, &model, "accrual", 100_000).expect("accrual view should replay");
    let accruals: Vec<_> = accrual
        .evaluation
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "accrual_entry")
        .collect();
    assert_eq!(accruals.len(), 1);
    assert_quantity(accruals[0], "amount", "1000 USD");
}

#[test]
fn clean_ledger_is_complete_and_over_selling_is_a_conflict() {
    let model = personal_model();
    let (_, clean, _) = checked(include_str!("../examples/clean_ledger.axm"), &model);
    assert!(clean.evaluation().findings.is_empty());
    let purchase = fact_for(&clean, "buy_check", "buy", &Value::Ref("lot_1".into()));
    assert_quantity(purchase, "cash_basis", "50 EUR");
    let satisfaction = clean
        .evaluation()
        .claims
        .iter()
        .find(|claim| claim.row.schema == "satisfaction")
        .expect("one allocation should satisfy part of the invoice");
    assert_quantity(satisfaction, "invoice_remaining", "80 EUR");
    assert_quantity(satisfaction, "payment_remaining", "0 EUR");
    let cash = view(&clean, &model, "cash", 100_000).expect("clean EUR cash view should replay");
    let cash_entries = cash
        .evaluation
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "cash_entry");
    assert_eq!(
        sum_quantities(cash_entries, "amount"),
        ("100".into(), "EUR".into())
    );

    let (_, same_day, _) = checked(include_str!("../examples/same_day_sales.axm"), &model);
    assert!(same_day.evaluation().findings.is_empty());
    assert_eq!(
        same_day
            .evaluation()
            .claims
            .iter()
            .filter(|claim| claim.row.schema == "gain")
            .count(),
        2
    );

    let (_, over_sold, _) = checked(include_str!("../examples/over_sold.axm"), &model);
    let finding = over_sold
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == "sale_1" && finding.rule == "investment_gain")
        .expect("oversold transaction should be checked");
    assert!(matches!(finding.outcome, Outcome::Conflict(_)));
    assert!(
        !over_sold
            .evaluation()
            .claims
            .iter()
            .any(|claim| claim.row.schema == "gain")
    );

    let (_, cross_account, _) = checked(include_str!("../examples/cross_account_lot.axm"), &model);
    let finding = cross_account
        .evaluation()
        .findings
        .iter()
        .find(|finding| {
            finding.subject == "sale_wrong_account" && finding.rule == "investment_gain"
        })
        .expect("cross-account lot should be checked");
    assert!(matches!(finding.outcome, Outcome::Conflict(_)));

    let (_, currency_mismatch, _) =
        checked(include_str!("../examples/currency_mismatch.axm"), &model);
    let finding = currency_mismatch
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == "sale_1" && finding.rule == "investment_gain")
        .expect("currency mismatch should be checked");
    assert!(matches!(finding.outcome, Outcome::Conflict(_)));

    let (_, unresolved, _) = checked(include_str!("../examples/unresolved_lot.axm"), &model);
    let finding = unresolved
        .evaluation()
        .findings
        .iter()
        .find(|finding| finding.subject == "sale_1" && finding.rule == "investment_gain")
        .expect("unresolved lot should be checked");
    assert_eq!(
        finding.outcome,
        Outcome::Alternatives(vec![Value::Ref("lot_1".into()), Value::Ref("lot_2".into())])
    );

    let (_, overallocated, _) = checked(
        include_str!("../examples/overallocated_satisfaction.axm"),
        &model,
    );
    let allocation_findings: Vec<_> = overallocated
        .evaluation()
        .findings
        .iter()
        .filter(|finding| finding.rule == "allocate_satisfaction")
        .collect();
    assert_eq!(allocation_findings.len(), 2);
    assert!(
        allocation_findings
            .iter()
            .all(|finding| matches!(finding.outcome, Outcome::Conflict(_)))
    );
    assert!(
        !overallocated
            .evaluation()
            .claims
            .iter()
            .any(|claim| claim.row.schema == "satisfaction")
    );

    for (source, ledger, target) in [
        (
            include_str!("../examples/party_mismatch.axm"),
            "party_mismatch",
            "allocation_1",
        ),
        (
            include_str!("../examples/unit_mismatch_allocation.axm"),
            "unit_mismatch_allocation",
            "allocation_1",
        ),
    ] {
        let (_, mismatch, _) = checked(source, &model);
        let finding = mismatch
            .evaluation()
            .findings
            .iter()
            .find(|finding| finding.subject == target && finding.rule == "allocate_satisfaction")
            .unwrap_or_else(|| panic!("{ledger} should have an allocation finding"));
        assert!(
            matches!(finding.outcome, Outcome::Conflict(_)),
            "{ledger}: {:?}",
            finding.outcome
        );
    }
}

#[test]
fn community_payroll_adds_a_book_with_the_same_rule_language() {
    let model = full_model();
    let (_, world, _) = checked(include_str!("../examples/payroll.axm"), &model);
    assert!(
        !world
            .evaluation()
            .claims
            .iter()
            .any(|claim| claim.row.schema == "reward")
    );

    let payroll =
        view(&world, &model, "payroll_book", 100_000).expect("community book should replay");
    let reward = payroll
        .evaluation
        .claims
        .iter()
        .find(|claim| claim.row.schema == "reward")
        .expect("payroll book should recognize the reward");
    assert_eq!(reward.phase, Phase::Recognized);
    assert_eq!(field(reward, "worker"), &Value::Ref("worker_ada".into()));
    assert_quantity(reward, "amount", "2800 USD");
}

#[test]
fn everyday_spending_needs_only_a_dated_entry_and_produces_cash() {
    let model = personal_model();
    let (_, world, _) = checked(include_str!("../examples/everyday.axm"), &model);
    assert!(world.evaluation().findings.is_empty());

    let cash = view(&world, &model, "cash", 100_000).expect("everyday cash book should replay");
    let spending = cash
        .evaluation
        .claims
        .iter()
        .find(|claim| {
            claim.row.schema == "cash_entry"
                && field(claim, "kind") == &Value::Text("everyday spending".into())
        })
        .expect("spending rule should recognize the dated ordinary entry");
    assert_quantity(spending, "amount", "-42.75 USD");
}
