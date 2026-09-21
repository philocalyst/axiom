//! Small generated differential corpus for the independent semantic oracle.
//!
//! The reference evaluator deliberately does not call the proof-producing
//! engine.  These tests therefore compare only canonical semantic fields and
//! check the production proof/algebra independently.  Source form order is
//! varied exhaustively for each small case so an accidental first-row winner
//! cannot hide behind a hand-written fixture.

use std::collections::BTreeMap;

use axiom_ledger::engine::{Analysis, ObservationStatus, QuoteStatus, RecognitionStatus};
use axiom_ledger::model::Quantity;
use axiom_ledger::parser::parse_ledger;
use axiom_ledger::reference::{
    self, PositionStatus, SatisfactionStatus, SelectionStatus, ValuationStatus,
};
use axiom_ledger::workspace::Workspace;

#[derive(Clone, Debug, Eq, PartialEq)]
enum SelectionProjection {
    Recognized,
    Ambiguous(Vec<String>),
    Conflict { policy: String, decision: String },
    Blocked,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GainProjection {
    lot: String,
    proceeds: String,
    basis: String,
    gain: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AllocationProjection {
    lot: String,
    quantity: String,
    proceeds: String,
    basis: String,
    gain: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SaleProjection {
    id: String,
    date: String,
    account: String,
    asset: String,
    quantity: String,
    proceeds: String,
    candidates: Vec<String>,
    gains: Vec<GainProjection>,
    allocations: Vec<AllocationProjection>,
    selected_lots: Vec<String>,
    selected: Option<String>,
    status: SelectionProjection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QuoteProjection {
    group: String,
    quotes: Vec<(String, String, String)>,
    status: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PositionProjection {
    account: String,
    asset: String,
    quantity: String,
    status: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SettlementProjection {
    sale: String,
    amount: Option<String>,
    into: Option<String>,
    status: SettlementProjectionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SettlementProjectionStatus {
    Satisfied,
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SemanticProjection {
    lots: Vec<(String, String, String, String, String, String, String)>,
    sales: Vec<SaleProjection>,
    quotes: Vec<QuoteProjection>,
    positions: Vec<PositionProjection>,
    settlements: Vec<SettlementProjection>,
    blocked: bool,
}

fn canonical(quantity: &Quantity) -> String {
    quantity.canonical()
}

fn reference_selection(status: &SelectionStatus) -> SelectionProjection {
    match status {
        SelectionStatus::Recognized => SelectionProjection::Recognized,
        SelectionStatus::Ambiguous { candidates } => {
            SelectionProjection::Ambiguous(candidates.clone())
        }
        SelectionStatus::Conflict {
            policy_lot,
            decision_lot,
        } => SelectionProjection::Conflict {
            policy: policy_lot.clone(),
            decision: decision_lot.clone(),
        },
        SelectionStatus::PolicyConflict { .. } | SelectionStatus::MissingLot => {
            SelectionProjection::Blocked
        }
        SelectionStatus::InvalidAmount => SelectionProjection::Invalid,
    }
}

fn production_selection(status: &RecognitionStatus) -> SelectionProjection {
    match status {
        RecognitionStatus::Recognized => SelectionProjection::Recognized,
        RecognitionStatus::Ambiguous { candidates } => {
            SelectionProjection::Ambiguous(candidates.clone())
        }
        RecognitionStatus::Conflict {
            policy_lot,
            decision_lot,
        } => SelectionProjection::Conflict {
            policy: policy_lot.clone(),
            decision: decision_lot.clone(),
        },
        RecognitionStatus::MissingLot => SelectionProjection::Blocked,
        RecognitionStatus::InvalidAmount => SelectionProjection::Invalid,
    }
}

fn reference_projection(result: &reference::ReferenceResult) -> SemanticProjection {
    let mut lots = result
        .lots
        .iter()
        .map(|lot| {
            (
                lot.id.clone(),
                lot.date.to_string(),
                lot.account.clone(),
                lot.asset.clone(),
                canonical(&lot.quantity),
                canonical(&lot.consideration),
                canonical(&lot.basis),
            )
        })
        .collect::<Vec<_>>();
    lots.sort();

    let mut sales = result
        .sales
        .iter()
        .map(|sale| {
            let mut gains = sale
                .conditional_gains
                .iter()
                .map(|gain| GainProjection {
                    lot: gain.lot_id.clone(),
                    proceeds: canonical(&gain.proceeds),
                    basis: canonical(&gain.basis),
                    gain: canonical(&gain.gain),
                })
                .collect::<Vec<_>>();
            gains.sort_by(|left, right| left.lot.cmp(&right.lot));
            let allocations = sale
                .allocations
                .iter()
                .map(|allocation| AllocationProjection {
                    lot: allocation.lot_id.clone(),
                    quantity: canonical(&allocation.quantity),
                    proceeds: canonical(&allocation.proceeds),
                    basis: canonical(&allocation.basis),
                    gain: canonical(&allocation.gain),
                })
                .collect();
            SaleProjection {
                id: sale.id.clone(),
                date: sale.date.to_string(),
                account: sale.account.clone(),
                asset: sale.asset.clone(),
                quantity: canonical(&sale.quantity),
                proceeds: canonical(&sale.proceeds),
                candidates: sale.eligible_lots.clone(),
                gains,
                allocations,
                selected_lots: sale.selected_lots.clone(),
                selected: sale.selected_lot.clone(),
                status: reference_selection(&sale.status),
            }
        })
        .collect::<Vec<_>>();
    sales.sort_by(|left, right| left.id.cmp(&right.id));

    let mut quotes = result
        .valuation
        .iter()
        .map(|valuation| QuoteProjection {
            group: valuation.id.clone(),
            quotes: valuation
                .quotes
                .iter()
                .map(|quote| {
                    (
                        quote.id.clone(),
                        canonical(&quote.base),
                        canonical(&quote.quote),
                    )
                })
                .collect(),
            status: matches!(valuation.status, ValuationStatus::Conflict { .. }),
        })
        .collect::<Vec<_>>();
    quotes.sort_by(|left, right| left.group.cmp(&right.group));

    let mut positions = result
        .positions
        .iter()
        .map(|position| PositionProjection {
            account: position.account.clone(),
            asset: position.asset.clone(),
            quantity: canonical(&position.quantity),
            status: matches!(position.status, PositionStatus::Conflict),
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| {
        left.account
            .cmp(&right.account)
            .then_with(|| left.asset.cmp(&right.asset))
            .then_with(|| left.quantity.cmp(&right.quantity))
    });

    let mut settlements = result
        .satisfies
        .iter()
        .map(|satisfies| SettlementProjection {
            sale: satisfies.sale.clone(),
            amount: satisfies.amount.as_ref().map(canonical),
            into: satisfies.into.clone(),
            status: match satisfies.status {
                SatisfactionStatus::Satisfied => SettlementProjectionStatus::Satisfied,
                SatisfactionStatus::Conflict => SettlementProjectionStatus::Conflict,
                SatisfactionStatus::Unavailable => SettlementProjectionStatus::Unavailable,
            },
        })
        .collect::<Vec<_>>();
    settlements.sort_by(|left, right| left.sale.cmp(&right.sale));

    SemanticProjection {
        lots,
        sales,
        quotes,
        positions,
        settlements,
        blocked: result.blocked(),
    }
}

fn production_projection(result: &Analysis) -> SemanticProjection {
    let mut lots = result
        .lots
        .iter()
        .map(|lot| {
            (
                lot.id.clone(),
                lot.date.to_string(),
                lot.account.clone(),
                lot.asset.clone(),
                canonical(&lot.quantity),
                canonical(&lot.consideration),
                canonical(&lot.basis),
            )
        })
        .collect::<Vec<_>>();
    lots.sort();

    let mut sales = result
        .sales
        .iter()
        .map(|sale| {
            let mut gains = sale
                .conditional_gains
                .iter()
                .map(|gain| GainProjection {
                    lot: gain.lot_id.clone(),
                    proceeds: canonical(&gain.proceeds),
                    basis: canonical(&gain.basis),
                    gain: canonical(&gain.gain),
                })
                .collect::<Vec<_>>();
            gains.sort_by(|left, right| left.lot.cmp(&right.lot));
            let allocations = sale
                .allocations
                .iter()
                .map(|allocation| AllocationProjection {
                    lot: allocation.lot_id.clone(),
                    quantity: canonical(&allocation.quantity),
                    proceeds: canonical(&allocation.proceeds),
                    basis: canonical(&allocation.basis),
                    gain: canonical(&allocation.gain),
                })
                .collect();
            SaleProjection {
                id: sale.id.clone(),
                date: sale.date.to_string(),
                account: sale.account.clone(),
                asset: sale.asset.clone(),
                quantity: canonical(&sale.quantity),
                proceeds: canonical(&sale.proceeds),
                candidates: sale.eligible_lots.clone(),
                gains,
                allocations,
                selected_lots: sale.selected_lots.clone(),
                selected: sale.selected_lot.clone(),
                status: production_selection(&sale.status),
            }
        })
        .collect::<Vec<_>>();
    sales.sort_by(|left, right| left.id.cmp(&right.id));

    let mut quote_groups = BTreeMap::<String, Vec<(String, String, String)>>::new();
    for quote in &result.quotes {
        let group = format!(
            "{}:{}:{}",
            quote.date,
            quote
                .base
                .unit
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "?".into()),
            quote
                .quote
                .unit
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "?".into())
        );
        quote_groups.entry(group).or_default().push((
            quote.id.clone(),
            canonical(&quote.base),
            canonical(&quote.quote),
        ));
    }
    let mut quotes = quote_groups
        .into_iter()
        .map(|(group, mut quote_values)| {
            quote_values.sort();
            let status = matches!(
                result.quote_status.get(&group),
                Some(QuoteStatus::Ambiguous { .. })
            );
            QuoteProjection {
                group,
                quotes: quote_values,
                status,
            }
        })
        .collect::<Vec<_>>();
    quotes.sort_by(|left, right| left.group.cmp(&right.group));

    let mut positions = result
        .positions
        .iter()
        .map(|position| PositionProjection {
            account: position.account.clone(),
            asset: position
                .quantity
                .unit
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "?".into()),
            quantity: canonical(&position.quantity),
            status: matches!(position.status, ObservationStatus::Conflict),
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| {
        left.account
            .cmp(&right.account)
            .then_with(|| left.asset.cmp(&right.asset))
            .then_with(|| left.quantity.cmp(&right.quantity))
    });

    let mut settlements = Vec::new();
    let mut grouped = BTreeMap::<String, Vec<_>>::new();
    for settlement in &result.settlements {
        grouped
            .entry(settlement.reference.clone())
            .or_default()
            .push(settlement);
    }
    for sale in &result.sales {
        let rows = grouped.remove(&sale.id).unwrap_or_default();
        let (amount, into, status) = if rows.is_empty() {
            (None, None, SettlementProjectionStatus::Unavailable)
        } else if rows
            .iter()
            .all(|row| row.status == ObservationStatus::Conflict)
        {
            (None, None, SettlementProjectionStatus::Conflict)
        } else {
            let row = rows[0];
            (
                Some(canonical(&row.quantity)),
                row.into.clone(),
                if row.status == ObservationStatus::Reconciled {
                    SettlementProjectionStatus::Satisfied
                } else {
                    SettlementProjectionStatus::Conflict
                },
            )
        };
        settlements.push(SettlementProjection {
            sale: sale.id.clone(),
            amount,
            into,
            status,
        });
    }

    SemanticProjection {
        lots,
        sales,
        quotes,
        positions,
        settlements,
        blocked: result.blocked(),
    }
}

fn analyze(
    source: &str,
) -> (
    reference::ReferenceResult,
    axiom_ledger::workspace::CommitAnalysis,
) {
    let ledger = parse_ledger(source).expect("generated source parses");
    let reference = reference::evaluate(&ledger);
    let mut workspace = Workspace::new();
    let source_ledger = workspace
        .load_source("generated.axm", source.as_bytes())
        .expect("generated source loads");
    let production = workspace
        .analyze_commit(source_ledger.commit)
        .expect("generated source analyzes");
    (reference, production)
}

fn assert_differential(source: &str) {
    let (reference, production) = analyze(source);
    let expected = reference_projection(&reference);
    let actual = production_projection(&production.analysis);
    assert_eq!(expected, actual, "reference/production mismatch:\n{source}");
    assert_eq!(
        reference.blocked(),
        production.analysis.blocked(),
        "blocked mismatch:\n{source}"
    );
    assert!(
        production.analysis.check_proof().is_ok(),
        "production proof does not check:\n{source}"
    );

    for sale in &reference.sales {
        for gain in &sale.conditional_gains {
            let expected_gain = &gain.proceeds.number - &gain.basis.number;
            assert_eq!(
                expected_gain, gain.gain.number,
                "reference arithmetic:\n{source}"
            );
        }
    }
    for sale in &production.analysis.sales {
        for gain in &sale.conditional_gains {
            let expected_gain = &gain.proceeds.number - &gain.basis.number;
            assert_eq!(
                expected_gain, gain.gain.number,
                "production arithmetic:\n{source}"
            );
        }
    }
    for journal in &production.analysis.journal {
        assert!(
            journal.balanced(),
            "unbalanced journal for `{}`:\n{source}",
            journal.sale
        );
    }
}

fn buy(label: &str, date: &str, quantity: u32, cost: u32) -> String {
    format!("buy {label} on {date}\n  {quantity} ABC into brokerage\n  for {cost} USD\n  fee 1 USD")
}

fn sell(quantity: u32, proceeds: u32, lot: &str) -> String {
    format!(
        "sell sell on 2026-09-20\n  {quantity} ABC from brokerage\n  for {proceeds} USD\n  lot {lot}"
    )
}

fn compose(forms: &[String], quotes: &str) -> String {
    let mut source = String::from("book tax-us\n");
    for form in forms {
        source.push_str(form);
        source.push('\n');
    }
    source.push_str(quotes);
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source
}

fn permutations(items: &[String]) -> Vec<Vec<String>> {
    if items.len() < 2 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(index);
        for mut tail in permutations(&rest) {
            let mut current = vec![head.clone()];
            current.append(&mut tail);
            result.push(current);
        }
    }
    result
}

/// Exhaust all source orders for the three/four-form ledgers and keep a
/// deterministic spread of rotations/reversals once optional directives make
/// the form count five.  The semantic dimensions themselves remain exhaustive
/// below; this cap keeps the integration corpus quick enough for every commit.
fn bounded_permutations(items: &[String]) -> Vec<Vec<String>> {
    let all = permutations(items);
    if all.len() <= 24 {
        return all;
    }
    let mut selected = Vec::new();
    for index in 0..items.len() {
        let mut rotated = items[index..].to_vec();
        rotated.extend_from_slice(&items[..index]);
        selected.push(rotated.clone());
        rotated.reverse();
        selected.push(rotated);
    }
    selected.push(items.to_vec());
    let mut reversed = items.to_vec();
    reversed.reverse();
    selected.push(reversed);
    selected.sort();
    selected.dedup();
    selected
}

fn quote_block(mode: &str) -> String {
    match mode {
        "none" => String::new(),
        "equal" => String::from(
            "quote quote/one on 2026-09-20\n  1 ABC = 52 USD\nquote quote/two on 2026-09-20\n  2 ABC = 104 USD\n",
        ),
        "conflict" => String::from(
            "quote quote/one on 2026-09-20\n  1 ABC = 52 USD\nquote quote/two on 2026-09-20\n  1 ABC = 53 USD\n",
        ),
        other => panic!("unknown quote mode {other}"),
    }
}

#[test]
fn generated_permutations_match_for_fifo_lifo_decisions_partial_lots_and_quotes() {
    let quantities = [1, 5, 10, 15];
    let policies = [None, Some("lots/fifo"), Some("lots/lifo")];
    let directives = [
        "",
        "decide sell lot buy/one",
        "decide sell lot buy/two",
        "decide sell lot buy/one\ndecide sell lot buy/two",
    ];
    let quote_modes = ["none", "equal", "conflict"];

    for quantity in quantities {
        for policy in policies {
            for decisions in directives {
                for quote_mode in quote_modes {
                    let mut controls = vec![
                        buy("buy/one", "2026-01-04", 10, 200),
                        buy("buy/two", "2026-02-04", 10, 300),
                        sell(quantity, 500, "?lot"),
                    ];
                    if let Some(policy) = policy {
                        controls.push(format!("use {policy} for tax-us"));
                    }
                    if !decisions.is_empty() {
                        controls.push(decisions.to_owned());
                    }
                    for order in bounded_permutations(&controls) {
                        let source = compose(&order, &quote_block(quote_mode));
                        assert_differential(&source);
                    }
                }
            }
        }
    }
}

#[test]
fn same_day_policy_ties_and_unknown_policies_remain_blocked() {
    for policy in ["lots/fifo", "lots/lifo"] {
        let source = compose(
            &[
                buy("buy/one", "2026-01-04", 10, 200),
                buy("buy/two", "2026-01-04", 10, 300),
                sell(10, 500, "?lot"),
                format!("use {policy} for tax-us"),
            ],
            "",
        );
        let (reference, production) = analyze(&source);
        assert!(matches!(
            reference.sale("sell").unwrap().status,
            SelectionStatus::Ambiguous { .. }
        ));
        assert!(matches!(
            production.analysis.sale("sell").unwrap().status,
            RecognitionStatus::Ambiguous { .. }
        ));
        assert_differential(&source);
    }

    let unknown = compose(
        &[
            buy("buy/one", "2026-01-04", 10, 200),
            sell(10, 500, "?lot"),
            String::from("use lots/unknown for tax-us"),
        ],
        "",
    );
    let (reference, production) = analyze(&unknown);
    assert!(matches!(
        reference.sale("sell").unwrap().status,
        SelectionStatus::MissingLot
    ));
    assert!(matches!(
        production.analysis.sale("sell").unwrap().status,
        RecognitionStatus::MissingLot
    ));
    assert_differential(&unknown);

    let explicit_decision = unknown.replace(
        "use lots/unknown for tax-us",
        "use lots/unknown for tax-us\ndecide sell lot buy/one",
    );
    let (reference, production) = analyze(&explicit_decision);
    assert!(matches!(
        reference.sale("sell").unwrap().status,
        SelectionStatus::Recognized
    ));
    assert!(matches!(
        production.analysis.sale("sell").unwrap().status,
        RecognitionStatus::Recognized
    ));
    assert_differential(&explicit_decision);

    let policy_conflict = compose(
        &[
            buy("buy/one", "2026-01-04", 10, 200),
            buy("buy/two", "2026-02-04", 10, 300),
            sell(10, 500, "?lot"),
            String::from("use lots/fifo for tax-us"),
            String::from("use lots/lifo for tax-us"),
        ],
        "",
    );
    assert_differential(&policy_conflict);
}

#[test]
fn settlements_positions_and_multi_sale_allocation_are_differentially_checked() {
    let exact = compose(
        &[
            buy("buy/one", "2026-01-04", 10, 200),
            sell(5, 500, "?lot"),
            String::from("use lots/fifo for tax-us"),
            String::from("observe position brokerage 15 ABC"),
            String::from("observe settlement sell 500 USD into checking"),
        ],
        "",
    );
    let (reference, production) = analyze(&exact);
    assert!(matches!(
        reference.satisfies[0].status,
        SatisfactionStatus::Satisfied
    ));
    assert!(matches!(
        production.analysis.settlements[0].status,
        ObservationStatus::Reconciled
    ));
    assert_eq!(production.analysis.journal.len(), 1);
    assert_differential(&exact);

    for policy in ["lots/fifo", "lots/lifo"] {
        let cross_lot = compose(
            &[
                buy("buy/one", "2026-01-04", 10, 200),
                buy("buy/two", "2026-02-04", 10, 300),
                sell(15, 500, "?lot"),
                format!("use {policy} for tax-us"),
                String::from("observe position brokerage 5 ABC"),
                String::from("observe settlement sell 500 USD into checking"),
            ],
            "",
        );
        let (reference, production) = analyze(&cross_lot);
        let reference_sale = reference.sale("sell").unwrap();
        let production_sale = production.analysis.sale("sell").unwrap();
        assert_eq!(reference_sale.allocations.len(), 2);
        assert_eq!(production_sale.allocations.len(), 2);
        assert_eq!(
            reference
                .recognized_gain("sell")
                .unwrap()
                .proceeds
                .canonical(),
            "500 USD"
        );
        assert_eq!(
            production
                .analysis
                .recognized_gain("sell")
                .unwrap()
                .proceeds
                .canonical(),
            "500 USD"
        );
        assert_differential(&cross_lot);
    }

    for settlement_rows in [
        "observe settlement sell 400 USD into checking\n",
        "observe settlement sell 500 USD into checking\nobserve settlement sell 500 USD into checking\n",
    ] {
        let source = compose(
            &[
                buy("buy/one", "2026-01-04", 10, 200),
                sell(5, 500, "?lot"),
                String::from("use lots/fifo for tax-us"),
                String::from("observe position brokerage 15 ABC"),
            ],
            settlement_rows,
        );
        let (reference, production) = analyze(&source);
        assert!(matches!(
            reference.satisfies[0].status,
            SatisfactionStatus::Conflict
        ));
        assert!(production.analysis.journal.is_empty());
        assert_differential(&source);
    }

    let multi_sale = compose(
        &[
            buy("buy/one", "2026-01-04", 20, 400),
            String::from(
                "sell first on 2026-09-20\n  10 ABC from brokerage\n  for 250 USD\n  lot ?lot",
            ),
            String::from(
                "sell second on 2026-09-21\n  10 ABC from brokerage\n  for 250 USD\n  lot ?lot",
            ),
            String::from("observe position brokerage 20 ABC"),
        ],
        "",
    );
    let (reference, production) = analyze(&multi_sale);
    assert!(reference.sales.iter().all(|sale| {
        sale.selected_lot.as_deref() == Some("buy/one")
            && matches!(sale.status, SelectionStatus::Recognized)
            && sale.allocations.len() == 1
    }));
    assert!(production.analysis.sales.iter().all(|sale| {
        sale.selected_lot.as_deref() == Some("buy/one")
            && matches!(sale.status, RecognitionStatus::Recognized)
            && sale.allocations.len() == 1
    }));
    assert!(production.analysis.journal.is_empty());
    assert_differential(&multi_sale);
}
