//! Bounded V1/V2 differential coverage for the overlapping personal-trade
//! semantics. V1's independent reference evaluator is the oracle; V2 is
//! exercised through its public package compiler and source-to-world checker.

use std::collections::BTreeSet;

use axiom_ledger::parser::parse_ledger;
use axiom_ledger::reference::{self, ReferenceResult, SelectionStatus};
use axiom_v2::{Model, Outcome, Value, World, check};

const PERSONAL_PACKAGE: &str = include_str!("../models/personal.axm");
const CHECK_LIMIT: usize = 100_000;

fn model() -> Model {
    Model::compile(&[PERSONAL_PACKAGE.to_owned()]).expect("personal package compiles")
}

fn v1(source: &str) -> ReferenceResult {
    let ledger = parse_ledger(source).expect("generated V1 source parses");
    reference::evaluate(&ledger)
}

fn v2(source: &str, model: &Model) -> World {
    let (_, world, _) = check(source, model, CHECK_LIMIT)
        .unwrap_or_else(|errors| panic!("generated V2 source checks: {errors:#?}\n{source}"));
    world
}

fn gain_claim(world: &World) -> &axiom_v2::Claim {
    let gains: Vec<_> = world
        .evaluation()
        .claims
        .iter()
        .filter(|claim| claim.row.schema == "gain")
        .collect();
    assert_eq!(gains.len(), 1, "expected one derived gain claim");
    gains[0]
}

fn field<'a>(claim: &'a axiom_v2::Claim, name: &str) -> &'a Value {
    claim
        .row
        .fields
        .get(name)
        .unwrap_or_else(|| panic!("gain claim has no `{name}` field: {:?}", claim.row.fields))
}

fn quantity<'a>(claim: &'a axiom_v2::Claim, name: &str) -> (&'a axiom_v2::Number, &'a str) {
    match field(claim, name) {
        Value::Quantity(number, unit) => (number, unit),
        value => panic!("gain field `{name}` is not a quantity: {value}"),
    }
}

/// V1 has one `for` amount and no separate sale-fee field. The generated V1
/// source therefore records net sale proceeds; V2 records gross proceeds and
/// its fee separately. Compare their normalized net proceeds, basis, and gain.
fn canonical_quantity(claim: &axiom_v2::Claim, name: &str) -> String {
    let (number, unit) = quantity(claim, name);
    format!("{number} {unit}")
}

fn assert_gain_parity(v1: &ReferenceResult, world: &World, sale_id: &str) {
    let expected = v1
        .recognized_gain(sale_id)
        .unwrap_or_else(|| panic!("V1 did not recognize `{sale_id}`"));
    let expected_sale = v1.sale(sale_id).expect("V1 sale exists");
    let actual = gain_claim(world);

    assert_eq!(field(actual, "sell"), &Value::Ref(sale_id.to_owned()));
    assert_eq!(field(actual, "lot"), &Value::Ref(expected.lot_id.clone()));
    assert_eq!(
        field(actual, "account"),
        &Value::Text(expected_sale.account.clone())
    );
    assert_eq!(
        field(actual, "date").to_string(),
        expected_sale.date.to_string()
    );
    assert_eq!(
        canonical_quantity(actual, "units"),
        expected_sale.quantity.canonical()
    );
    assert_eq!(
        canonical_quantity(actual, "basis"),
        expected.basis.canonical()
    );
    assert_eq!(
        canonical_quantity(actual, "amount"),
        expected.gain.canonical()
    );

    let (gross, gross_unit) = quantity(actual, "proceeds");
    let (fee, fee_unit) = quantity(actual, "fees");
    assert_eq!(gross_unit, fee_unit);
    let net = gross.sub(fee).expect("same-currency sale fee subtraction");
    assert_eq!(format!("{net} {gross_unit}"), expected.proceeds.canonical());
}

fn cents(value: u32) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}

fn shares(hundredths: u32) -> String {
    if hundredths % 100 == 0 {
        (hundredths / 100).to_string()
    } else {
        format!("{}.{:02}", hundredths / 100, hundredths % 100)
    }
}

fn v1_buy(id: &str, date: &str, units: &str, cost: &str, fees: &str) -> String {
    format!("buy {id} on {date}\n  {units} ABC into brokerage\n  for {cost} USD\n  fee {fees} USD")
}

fn v1_sell(id: &str, date: &str, units: &str, net_proceeds: &str, lot: &str) -> String {
    format!(
        "sell {id} on {date}\n  {units} ABC from brokerage\n  for {net_proceeds} USD\n  lot {lot}"
    )
}

fn v2_buy(id: &str, date: &str, units: &str, cost: &str, fees: &str) -> String {
    format!(
        "buy {id}\n  account brokerage\n  date {date}\n  units {units} ABC\n  cost {cost} USD\n  fees {fees} USD"
    )
}

fn v2_sell(
    id: &str,
    date: &str,
    units: &str,
    gross_proceeds: &str,
    fees: &str,
    lot: &str,
) -> String {
    format!(
        "sell {id}\n  account brokerage\n  date {date}\n  units {units} ABC\n  proceeds {gross_proceeds} USD\n  fees {fees} USD\n  lot {lot}"
    )
}

fn v1_ledger(forms: &[String]) -> String {
    format!("book tax-us\n{}\n", forms.join("\n"))
}

fn v2_ledger(forms: &[String], decision: Option<&str>) -> String {
    let mut source = format!(
        "ledger differential\nuse personal\n\n{}\n",
        forms.join("\n")
    );
    if let Some(decision) = decision {
        source.push_str(decision);
        source.push('\n');
    }
    source
}

#[test]
fn generated_explicit_lot_gains_match_for_partial_quantities_and_fees() {
    let model = model();

    // The fixed seed and arithmetic below give a repeatable 64-case corpus.
    // It varies lot size, partial/full sale quantity, purchase cost, both fee
    // amounts, and gross proceeds. Decimal share quantities and money retain
    // exact rational arithmetic on both sides.
    for index in 0..64u32 {
        let bought_hundredths = (2 + index % 9) * 100;
        let sold_hundredths = if index % 4 == 0 {
            bought_hundredths
        } else {
            100 + (index * 53 % (bought_hundredths - 100))
        };
        let cost_cents = 101 + (index * 173 % 8_000);
        let buy_fee_cents = index * 61 % 301;
        let gross_cents = 200 + (index * 263 + 71) % 10_000;
        let sell_fee_cents = (index * 79 + 2) % 151;
        let sell_fee_cents = sell_fee_cents.min(gross_cents - 1);
        let net_cents = gross_cents - sell_fee_cents;
        let bought = shares(bought_hundredths);
        let sold = shares(sold_hundredths);
        let cost = cents(cost_cents);
        let buy_fee = cents(buy_fee_cents);
        let gross = cents(gross_cents);
        let sell_fee = cents(sell_fee_cents);
        let net = cents(net_cents);

        let mut v1_source = v1_ledger(&[
            v1_buy("lot", "2026-01-04", &bought, &cost, &buy_fee),
            v1_sell("sale", "2026-09-20", &sold, &net, "?lot"),
        ]);
        // V1 lot selectors are typed holes; the separate decision makes the
        // selected lot explicit in the ledger authority.
        v1_source.push_str("decide sale lot lot\n");
        let v2_source = v2_ledger(
            &[
                v2_buy("lot", "2026-01-04", &bought, &cost, &buy_fee),
                v2_sell("sale", "2026-09-20", &sold, &gross, &sell_fee, "@lot"),
            ],
            None,
        );

        let reference = v1(&v1_source);
        let world = v2(&v2_source, &model);
        assert_gain_parity(&reference, &world, "sale");
    }
}

#[test]
fn ambiguous_lots_remain_alternatives_and_explicit_decisions_match_v1() {
    let model = model();

    // FIFO/LIFO are intentionally outside this package's overlap: V2's
    // personal package requires an explicit lot choice and defines no lot
    // policy. These cases compare the shared ambiguity boundary and then the
    // shared semantics after an explicit decision resolves the hole.
    for index in 0..12u32 {
        let units_a = 4 + index % 3;
        let units_b = 5 + (index * 3 % 4);
        let sold = 1 + index % 3;
        let cost_a = cents(203 + index * 137);
        let fee_a = cents(index * 29 % 97);
        let cost_b = cents(507 + index * 83);
        let fee_b = cents(index * 47 % 131);
        let gross = cents(1_000 + index * 211);
        let sell_fee = cents(index * 17 % 91);
        let net = cents(1_000 + index * 211 - index * 17 % 91);
        let units_a = units_a.to_string();
        let units_b = units_b.to_string();
        let sold = sold.to_string();

        let buys_v1 = [
            v1_buy("lot_a", "2026-01-04", &units_a, &cost_a, &fee_a),
            v1_buy("lot_b", "2026-02-04", &units_b, &cost_b, &fee_b),
        ];
        let buys_v2 = [
            v2_buy("lot_a", "2026-01-04", &units_a, &cost_a, &fee_a),
            v2_buy("lot_b", "2026-02-04", &units_b, &cost_b, &fee_b),
        ];
        let v1_sale = v1_sell("sale", "2026-09-20", &sold, &net, "?lot");
        let v2_sale = v2_sell("sale", "2026-09-20", &sold, &gross, &sell_fee, "?lot");
        let v1_ambiguous_source =
            v1_ledger(&[buys_v1[0].clone(), buys_v1[1].clone(), v1_sale.clone()]);
        let v2_ambiguous_source = v2_ledger(
            &[buys_v2[0].clone(), buys_v2[1].clone(), v2_sale.clone()],
            None,
        );

        let reference = v1(&v1_ambiguous_source);
        let reference_sale = reference.sale("sale").expect("V1 sale exists");
        assert!(matches!(
            reference_sale.status,
            SelectionStatus::Ambiguous { .. }
        ));
        assert_eq!(reference_sale.conditional_gains.len(), 2);

        let world = v2(&v2_ambiguous_source, &model);
        let finding = world
            .evaluation()
            .findings
            .iter()
            .find(|finding| finding.subject == "sale" && finding.rule == "investment_gain")
            .expect("V2 retains the blocked sale finding");
        let Outcome::Alternatives(values) = &finding.outcome else {
            panic!(
                "V2 should expose eligible lot alternatives, found {:?}",
                finding.outcome
            );
        };
        let v2_candidates: BTreeSet<_> = values
            .iter()
            .filter_map(|value| match value {
                Value::Ref(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let v1_candidates: BTreeSet<_> = reference_sale
            .eligible_lots
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(v2_candidates, v1_candidates);
        assert!(
            !world
                .evaluation()
                .claims
                .iter()
                .any(|claim| claim.row.schema == "gain")
        );

        // V1 computes conditional gain values for every candidate while V2
        // emits candidate IDs and waits for a choice before deriving a gain.
        // This intentional result-shape difference is covered here; selected
        // arithmetic is compared after explicit resolution.
        let selected = if index % 2 == 0 { "lot_a" } else { "lot_b" };
        let v1_decision_source = format!("decide sale lot {selected}\n");
        let v2_decision_source =
            format!("decide resolve_lot\n  target sale.lot\n  value @{selected}\n");
        let v1_resolved_source = format!("{v1_ambiguous_source}{v1_decision_source}");
        let v2_resolved_source = v2_ledger(
            &[buys_v2[0].clone(), buys_v2[1].clone(), v2_sale],
            Some(&v2_decision_source),
        );
        let resolved_reference = v1(&v1_resolved_source);
        let resolved_world = v2(&v2_resolved_source, &model);
        assert_gain_parity(&resolved_reference, &resolved_world, "sale");
        assert_eq!(
            field(gain_claim(&resolved_world), "lot"),
            &Value::Ref(selected.to_owned())
        );
    }
}
