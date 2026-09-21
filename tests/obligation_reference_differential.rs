//! Differential checks for the independent authored-obligation oracle.

use axiom_ledger::engine::{Analysis, IssueCode};
use axiom_ledger::parser::parse_ledger;
use axiom_ledger::reference;
use axiom_ledger::workspace::Workspace;

fn analyze(source: &str) -> (reference::ReferenceResult, Analysis) {
    let ledger = parse_ledger(source).expect("source parses");
    let expected = reference::evaluate(&ledger);
    let mut workspace = Workspace::new();
    let loaded = workspace
        .load_source("obligation-reference.axm", source.as_bytes())
        .expect("source loads");
    let actual = workspace
        .analyze_commit(loaded.commit)
        .expect("source analyzes");
    (expected, actual.analysis)
}

fn assert_differential(source: &str) {
    let (expected, actual) = analyze(source);

    let mut expected_obligations = expected
        .obligations
        .iter()
        .map(|obligation| {
            (
                obligation.id.clone(),
                obligation.promised.canonical(),
                obligation.remaining.as_ref().map(|value| value.canonical()),
                format!("{:?}", obligation.status),
            )
        })
        .collect::<Vec<_>>();
    let mut actual_obligations = actual
        .obligations
        .iter()
        .map(|obligation| {
            (
                obligation.id.clone(),
                obligation.promised.canonical(),
                obligation.remaining.as_ref().map(|value| value.canonical()),
                format!("{:?}", obligation.status),
            )
        })
        .collect::<Vec<_>>();
    expected_obligations.sort();
    actual_obligations.sort();
    assert_eq!(
        expected_obligations, actual_obligations,
        "obligations:\n{source}"
    );

    let mut expected_settlements = expected
        .settlement_histories
        .iter()
        .map(|settlement| {
            (
                settlement.id.clone(),
                settlement.amount.canonical(),
                format!("{:?}", settlement.current),
                settlement.effective,
                settlement.unused.as_ref().map(|value| value.canonical()),
            )
        })
        .collect::<Vec<_>>();
    let mut actual_settlements = actual
        .settlement_histories
        .iter()
        .map(|settlement| {
            (
                settlement.id.clone(),
                settlement.amount.canonical(),
                format!("{:?}", settlement.current),
                settlement.effective,
                settlement.unused.as_ref().map(|value| value.canonical()),
            )
        })
        .collect::<Vec<_>>();
    expected_settlements.sort();
    actual_settlements.sort();
    assert_eq!(
        expected_settlements, actual_settlements,
        "settlements:\n{source}"
    );

    let mut expected_satisfactions = expected
        .satisfactions
        .iter()
        .map(|satisfaction| {
            (
                satisfaction.id.clone(),
                satisfaction.obligation.clone(),
                satisfaction.settlement.clone(),
                satisfaction.amount.canonical(),
                format!("{:?}", satisfaction.state),
                satisfaction.effective,
            )
        })
        .collect::<Vec<_>>();
    let mut actual_satisfactions = actual
        .satisfactions
        .iter()
        .map(|satisfaction| {
            (
                satisfaction.id.clone(),
                satisfaction.obligation.clone(),
                satisfaction.settlement.clone(),
                satisfaction.amount.canonical(),
                format!("{:?}", satisfaction.state),
                satisfaction.effective,
            )
        })
        .collect::<Vec<_>>();
    expected_satisfactions.sort();
    actual_satisfactions.sort();
    assert_eq!(
        expected_satisfactions, actual_satisfactions,
        "satisfactions:\n{source}"
    );

    let expected_conflict = expected
        .issues
        .iter()
        .any(|issue| issue.code == reference::ReferenceIssueCode::ObligationConflict);
    let actual_conflict = actual
        .issues
        .iter()
        .any(|issue| issue.code == IssueCode::ObligationConflict);
    assert_eq!(expected_conflict, actual_conflict, "conflict:\n{source}");
}

#[test]
fn partial_and_returned_settlements_match_independent_oracle() {
    let partial = r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
settlement payment/a
  kind ach
  from customer
  to vendor
  instrument USD
  amount 60 USD
  state issued at 2026-01-01
  state presented at 2026-01-02
  state settled at 2026-01-03
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 60 USD
  state applied
"#;
    let (reference, _) = analyze(partial);
    assert_eq!(
        reference
            .obligation("invoice/a")
            .unwrap()
            .remaining
            .as_ref()
            .unwrap()
            .canonical(),
        "40 USD"
    );
    assert!(reference.satisfactions[0].effective);
    assert_differential(partial);

    let returned = partial
        .replace("amount 60 USD", "amount 100 USD")
        .replace(
            "state settled at 2026-01-03",
            "state settled at 2026-01-03\n  state returned at 2026-01-04",
        )
        .replace(
            "amount 60 USD\n  state applied",
            "amount 100 USD\n  state applied",
        );
    let (reference, _) = analyze(&returned);
    assert_eq!(
        reference
            .obligation("invoice/a")
            .unwrap()
            .remaining
            .as_ref()
            .unwrap()
            .canonical(),
        "100 USD"
    );
    assert!(!reference.satisfactions[0].effective);
    assert_differential(&returned);
}

#[test]
fn allocation_conflicts_are_order_independent_and_blocked() {
    let source = r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 100 USD
obligation invoice/b
  debtor customer
  creditor vendor
  performance transfer 100 USD
settlement payment/a
  kind card
  from customer
  to vendor
  instrument USD
  amount 100 USD
  state issued
  state presented
  state settled
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 60 USD
  state applied
satisfy allocation/b
  obligation invoice/b
  settlement payment/a
  amount 60 USD
  state applied
"#;
    let (reference, actual) = analyze(source);
    assert!(reference.blocked());
    assert!(actual.blocked());
    assert_differential(source);

    let first = "satisfy allocation/a\n  obligation invoice/a\n  settlement payment/a\n  amount 60 USD\n  state applied\n";
    let second = "satisfy allocation/b\n  obligation invoice/b\n  settlement payment/a\n  amount 60 USD\n  state applied\n";
    let reordered = source.replace(&format!("{first}{second}"), &format!("{second}{first}"));
    assert_differential(&reordered);
}

#[test]
fn invalid_history_and_endpoint_mismatch_are_conflicts() {
    let history = r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 10 USD
settlement payment/a
  kind check
  from customer
  to vendor
  instrument USD
  amount 10 USD
  state issued at 2026-01-03
  state settled at 2026-01-02
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 10 USD
  state applied
"#;
    assert_differential(history);

    let mismatch = history
        .replace(
            "state settled at 2026-01-02",
            "state presented at 2026-01-04\n  state settled at 2026-01-05",
        )
        .replace("  to vendor", "  to other-vendor");
    assert_differential(&mismatch);
}

#[test]
fn invalid_component_does_not_poison_unrelated_component() {
    let source = r#"book receivables
obligation invoice/bad
  debtor customer
  creditor vendor
  performance transfer 100 USD
settlement payment/bad
  kind ach
  from customer
  to vendor
  instrument USD
  amount 100 USD
  state issued
  state presented
  state settled
satisfy allocation/bad
  obligation invoice/bad
  settlement payment/bad
  amount 120 USD
  state applied
obligation invoice/good
  debtor customer
  creditor vendor
  performance transfer 40 USD
settlement payment/good
  kind ach
  from customer
  to vendor
  instrument USD
  amount 40 USD
  state issued
  state presented
  state settled
satisfy allocation/good
  obligation invoice/good
  settlement payment/good
  amount 40 USD
  state applied
"#;
    let (reference, actual) = analyze(source);
    assert!(reference.blocked());
    assert!(actual.blocked());
    assert_eq!(reference.obligation("invoice/bad").unwrap().remaining, None);
    assert_eq!(
        reference
            .obligation("invoice/good")
            .unwrap()
            .remaining
            .as_ref()
            .unwrap()
            .canonical(),
        "0 USD"
    );
    assert_eq!(
        reference.settlement_history("payment/bad").unwrap().unused,
        None
    );
    assert_eq!(
        reference
            .settlement_history("payment/good")
            .unwrap()
            .unused
            .as_ref()
            .unwrap()
            .canonical(),
        "0 USD"
    );
    assert!(
        !reference
            .satisfactions
            .iter()
            .find(|satisfaction| satisfaction.id == "allocation/bad")
            .unwrap()
            .effective
    );
    assert!(
        reference
            .satisfactions
            .iter()
            .find(|satisfaction| satisfaction.id == "allocation/good")
            .unwrap()
            .effective
    );
    assert_differential(source);
}

#[test]
fn resolved_settlement_is_not_effective() {
    let source = r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 10 USD
settlement payment/a
  kind check
  from customer
  to vendor
  instrument USD
  amount 10 USD
  state issued
  state presented
  state settled
  state disputed
  state resolved
"#;
    // Keep this oracle-only assertion independent of proof serialization: the
    // production proof checker has a separate certificate for this terminal
    // dispute state, while the semantic rule is simply that `resolved` does
    // not itself make a payment effective.
    let reference = reference::evaluate(&parse_ledger(source).expect("source parses"));
    assert!(!reference.settlement_history("payment/a").unwrap().effective);
    assert_eq!(
        reference
            .obligation("invoice/a")
            .unwrap()
            .remaining
            .as_ref()
            .unwrap()
            .canonical(),
        "10 USD"
    );
}
