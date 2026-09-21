//! Human-facing views over the semantic analysis.
//!
//! Rendering is intentionally a projection: it never resolves an open lot
//! selector, chooses a lot, or changes the source ledger.  The output is plain
//! text so it is calm in a terminal, useful in a review, and stable in a
//! redirected file.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::engine::{
    Analysis, Issue, IssueCode, JournalEntry, JournalLine, ObligationStatus, ObservationStatus,
    QuoteStatus, RecognitionStatus, Side,
};
use crate::package::PolicyRegistry;
use crate::proof::Operation;

/// A rendering failure is a user-facing query error, not a semantic result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    UnknownGoal(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownGoal(goal) => write!(f, "no explanation is available for `{goal}`"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render the accounting-first validation view.
pub fn render_check(analysis: &Analysis) -> String {
    let mut out = String::new();
    let status = if has_blocking_state(analysis) {
        "blocked"
    } else {
        "ok"
    };
    let _ = writeln!(out, "check: {status}");
    let _ = writeln!(out, "book: {}", analysis.book);

    if !analysis.positions.is_empty() {
        out.push_str("\npositions\n");
        for position in &analysis.positions {
            let _ = writeln!(
                out,
                "  {}  {}  {}",
                position.account,
                quantity(&position.quantity),
                observation_label(&position.status)
            );
        }
    }

    if !analysis.settlements.is_empty() {
        out.push_str("\nsettlements\n");
        for settlement in &analysis.settlements {
            let _ = writeln!(
                out,
                "  {}  {}  {}{}",
                settlement.reference,
                quantity(&settlement.quantity),
                observation_label(&settlement.status),
                settlement
                    .into
                    .as_deref()
                    .map(|account| format!(" into {account}"))
                    .unwrap_or_else(|| " (account missing)".to_owned())
            );
        }
    }

    if !analysis.sales.is_empty() {
        out.push_str("\nsales\n");
        for sale in &analysis.sales {
            let _ = writeln!(out, "  {}", sale.id);
            let eligible = if sale.eligible_lots.is_empty() {
                "none".to_owned()
            } else {
                sale.eligible_lots.join(", ")
            };
            let _ = writeln!(out, "    eligible lots: {eligible}");
            match &sale.status {
                RecognitionStatus::Recognized => {
                    let selected = sale
                        .selected_lot
                        .as_deref()
                        .map(str::to_owned)
                        .or_else(|| {
                            (!sale.selected_lots.is_empty()).then(|| sale.selected_lots.join(", "))
                        })
                        .unwrap_or_else(|| "unknown".to_owned());
                    let _ = writeln!(out, "    selected lot: {selected}");
                    if let Some(gain) = analysis.recognized_gain(&sale.id) {
                        let _ = writeln!(out, "    gain: {}", quantity(&gain.gain));
                    }
                    for allocation in &sale.allocations {
                        let _ = writeln!(
                            out,
                            "      {} -> {} basis {}, gain {}",
                            allocation.lot_id,
                            quantity(&allocation.quantity),
                            quantity(&allocation.basis),
                            quantity(&allocation.gain)
                        );
                    }
                    out.push_str("    recognition: recognized\n");
                }
                RecognitionStatus::Ambiguous { .. } => {
                    out.push_str("    selected lot: ambiguous\n");
                    out.push_str("    gain: conditional\n");
                    for gain in &sale.conditional_gains {
                        let _ = writeln!(out, "      {} -> {}", gain.lot_id, quantity(&gain.gain));
                    }
                    out.push_str("    recognition: blocked\n");
                }
                RecognitionStatus::Conflict {
                    policy_lot,
                    decision_lot,
                } => {
                    let _ = writeln!(
                        out,
                        "    selected lot: conflict ({policy_lot} by policy, {decision_lot} by decision)"
                    );
                    out.push_str("    gain: conditional\n");
                    for gain in &sale.conditional_gains {
                        let _ = writeln!(out, "      {} -> {}", gain.lot_id, quantity(&gain.gain));
                    }
                    out.push_str("    recognition: blocked\n");
                }
                RecognitionStatus::MissingLot => {
                    out.push_str("    recognition: blocked (no eligible lot)\n")
                }
                RecognitionStatus::InvalidAmount => {
                    out.push_str("    recognition: blocked (invalid amount)\n")
                }
            }
        }
    }

    if !analysis.quote_status.is_empty() {
        out.push_str("\nquotes\n");
        for (group, status) in &analysis.quote_status {
            let label = quote_label(group);
            match status {
                QuoteStatus::Unique => {
                    let _ = writeln!(out, "  {label}  unique");
                }
                QuoteStatus::Ambiguous { quote_ids } => {
                    let _ = writeln!(out, "  {label}  conflicting ({})", quote_ids.join(", "));
                }
            }
        }
    }

    if !analysis.obligations.is_empty() {
        out.push_str("\nobligations\n");
        for obligation in &analysis.obligations {
            let status = match obligation.status {
                ObligationStatus::Outstanding => "outstanding",
                ObligationStatus::PartiallySatisfied => "partially satisfied",
                ObligationStatus::Satisfied => "satisfied",
                ObligationStatus::Invalid => "invalid",
            };
            let remaining = obligation
                .remaining
                .as_ref()
                .map(quantity)
                .unwrap_or_else(|| "unknown".into());
            let due = obligation
                .due
                .map(|date| format!("  due {date}"))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  {}  {}  remaining {}{}",
                obligation.id, status, remaining, due
            );
        }
    }
    if !analysis.settlement_histories.is_empty() {
        out.push_str("\nsettlements\n");
        for settlement in &analysis.settlement_histories {
            let unused = settlement
                .unused
                .as_ref()
                .map(quantity)
                .unwrap_or_else(|| "unknown".into());
            let _ = writeln!(
                out,
                "  {}  {}  {}  {}  unused {}",
                settlement.id,
                settlement_kind(settlement.kind),
                settlement_state(settlement.current),
                if settlement.effective {
                    "effective"
                } else {
                    "ineffective"
                },
                unused
            );
        }
    }
    if !analysis.satisfactions.is_empty() {
        out.push_str("\nsatisfactions\n");
        for satisfaction in &analysis.satisfactions {
            let _ = writeln!(
                out,
                "  {}  {} -> {}  {}  {}",
                satisfaction.id,
                satisfaction.settlement,
                satisfaction.obligation,
                quantity(&satisfaction.amount),
                satisfaction_state(satisfaction.state)
            );
        }
    }

    let blocking_issues = analysis
        .issues
        .iter()
        .filter(|issue| issue.code != IssueCode::AmbiguousQuote)
        .collect::<Vec<_>>();
    if !blocking_issues.is_empty() {
        out.push_str("\nissues\n");
        for issue in blocking_issues {
            let _ = writeln!(out, "  {}: {}", issue_code(issue), issue.message);
        }
        render_next_actions(analysis, &mut out);
    }

    render_attention(analysis, &mut out);

    out
}

/// Render the journal projection.  A blocked recognition never gets a guessed
/// balancing line; the user sees the exact reason and next action.
pub fn render_journal(analysis: &Analysis) -> String {
    let mut out = String::new();
    let mut missing_settlements = analysis
        .journal
        .iter()
        .filter(|entry| !has_settlement(analysis, &entry.sale))
        .map(|entry| entry.sale.as_str())
        .collect::<Vec<_>>();
    // The engine deliberately does not invent a journal line when the
    // settlement has no receiving account.  Keep that accepted recognition
    // visible as an incomplete projection instead of calling the journal
    // empty (or, worse, balanced).
    for sale in &analysis.sales {
        if sale.status.is_complete()
            && !has_settlement(analysis, &sale.id)
            && !missing_settlements.contains(&sale.id.as_str())
        {
            missing_settlements.push(sale.id.as_str());
        }
    }
    let journal_balanced = !analysis.journal.is_empty()
        && missing_settlements.is_empty()
        && analysis.journal.iter().all(JournalEntry::balanced);
    let status = if !missing_settlements.is_empty() {
        "partial (blocked)"
    } else if has_blocking_state(analysis) {
        "blocked"
    } else if journal_balanced {
        "balanced"
    } else if analysis.journal.is_empty() {
        "empty"
    } else {
        "partial"
    };
    let _ = writeln!(out, "journal: {status}");
    let _ = writeln!(out, "book: {}", analysis.book);

    if analysis.journal.is_empty() && missing_settlements.is_empty() {
        out.push_str("\n  no recognized entries\n");
    } else {
        out.push('\n');
        for entry in &analysis.journal {
            render_entry(
                entry,
                !missing_settlements.contains(&entry.sale.as_str()),
                &mut out,
            );
        }
        let rendered = analysis
            .journal
            .iter()
            .map(|entry| entry.sale.as_str())
            .collect::<BTreeSet<_>>();
        for sale in &missing_settlements {
            if !rendered.contains(sale) {
                let _ = writeln!(out, "entry {sale}");
                out.push_str("  partial (missing settlement account)\n");
            }
        }
    }

    if has_blocking_state(analysis) || !missing_settlements.is_empty() {
        out.push_str("\nblocked\n");
        for sale in missing_settlements {
            let _ = writeln!(out, "  missing settlement account for `{sale}`");
        }
        for issue in analysis
            .issues
            .iter()
            .filter(|issue| issue.code != IssueCode::AmbiguousQuote)
        {
            let _ = writeln!(out, "  {}: {}", issue_code(issue), issue.message);
        }
        render_next_actions(analysis, &mut out);
    }

    render_attention(analysis, &mut out);

    out
}

/// Render a calm source-level explanation for one named goal.
pub fn render_why(analysis: &Analysis, goal: &str) -> Result<String, RenderError> {
    if analysis.dependency_roots(goal).is_empty() {
        return Err(RenderError::UnknownGoal(goal.to_owned()));
    }

    let mut out = String::new();
    let _ = writeln!(out, "why {goal}");
    if let Some((kind, subject)) = goal.split_once(':') {
        match kind {
            "gain" => {
                if let Some(sale) = analysis.sale(subject) {
                    let _ = writeln!(out, "status: {}", sale.status);
                    let _ = writeln!(out, "because:");
                    let _ = writeln!(
                        out,
                        "  sale `{}` was observed on {} for {} from `{}`",
                        sale.id,
                        sale.date,
                        quantity(&sale.quantity),
                        sale.account
                    );
                    let _ = writeln!(out, "source: sell `{}`", sale.id);
                    if sale.eligible_lots.is_empty() {
                        out.push_str("  no eligible acquisition lot was found\n");
                    } else {
                        let _ = writeln!(out, "  eligible lots: {}", sale.eligible_lots.join(", "));
                    }
                    if let Some(policy) = &analysis.policy {
                        let _ = writeln!(out, "policy: {policy}");
                    }
                    let decisions = decision_answers(analysis, subject);
                    for decision in decisions {
                        let _ = writeln!(out, "decision: {decision}");
                    }
                    if let Some(gain) = analysis.recognized_gain(subject) {
                        let _ =
                            writeln!(out, "result: {} from {}", quantity(&gain.gain), gain.lot_id);
                    } else if !sale.conditional_gains.is_empty() {
                        out.push_str("possible results:\n");
                        for gain in &sale.conditional_gains {
                            let _ = writeln!(out, "  {} -> {}", gain.lot_id, quantity(&gain.gain));
                        }
                    }
                }
            }
            "position" => {
                if let Some(position) = analysis
                    .positions
                    .iter()
                    .find(|position| position.account == subject)
                {
                    let _ = writeln!(out, "status: {}", observation_label(&position.status));
                    out.push_str("because:\n  source: observe position\n");
                    let _ = writeln!(out, "result: {}", quantity(&position.quantity));
                }
            }
            "settlement" => {
                if let Some(settlement) = analysis
                    .settlement_histories
                    .iter()
                    .find(|settlement| settlement.id == subject)
                {
                    let status = if settlement.unused.is_none() {
                        "invalid"
                    } else if settlement.effective {
                        "effective"
                    } else {
                        "ineffective"
                    };
                    let _ = writeln!(out, "status: {status}");
                    out.push_str("because:\n  source: settlement history\n");
                    let _ = writeln!(
                        out,
                        "  kind: {}\n  current state: {}",
                        settlement_kind(settlement.kind),
                        settlement_state(settlement.current)
                    );
                    let unused = settlement
                        .unused
                        .as_ref()
                        .map(quantity)
                        .unwrap_or_else(|| "unknown".into());
                    let _ = writeln!(out, "result: unused {unused}");
                } else if let Some(settlement) = analysis
                    .settlements
                    .iter()
                    .find(|settlement| settlement.reference == subject)
                {
                    let _ = writeln!(out, "status: {}", observation_label(&settlement.status));
                    out.push_str("because:\n  source: observe settlement\n");
                    let _ = writeln!(out, "result: {}", quantity(&settlement.quantity));
                }
            }
            "obligation" => {
                if let Some(obligation) = analysis
                    .obligations
                    .iter()
                    .find(|obligation| obligation.id == subject)
                {
                    let status = match obligation.status {
                        ObligationStatus::Outstanding => "outstanding",
                        ObligationStatus::PartiallySatisfied => "partially satisfied",
                        ObligationStatus::Satisfied => "satisfied",
                        ObligationStatus::Invalid => "invalid",
                    };
                    let _ = writeln!(out, "status: {status}");
                    let _ = writeln!(
                        out,
                        "because:\n  {} owes {} to {}",
                        obligation.debtor,
                        quantity(&obligation.promised),
                        obligation.creditor
                    );
                    if let Some(due) = obligation.due {
                        let _ = writeln!(out, "  due: {due}");
                    }
                    let remaining = obligation
                        .remaining
                        .as_ref()
                        .map(quantity)
                        .unwrap_or_else(|| "unknown".into());
                    let _ = writeln!(out, "result: remaining {remaining}");
                }
            }
            "satisfaction" => {
                if let Some(satisfaction) = analysis
                    .satisfactions
                    .iter()
                    .find(|satisfaction| satisfaction.id == subject)
                {
                    let status = if satisfaction.effective {
                        "effective"
                    } else {
                        "ineffective"
                    };
                    let _ = writeln!(out, "status: {status}");
                    let _ = writeln!(
                        out,
                        "because:\n  settlement `{}` allocates {} to obligation `{}`\n  allocation state: {}",
                        satisfaction.settlement,
                        quantity(&satisfaction.amount),
                        satisfaction.obligation,
                        satisfaction_state(satisfaction.state)
                    );
                }
            }
            _ => {}
        }
    }

    Ok(out)
}

fn settlement_kind(kind: crate::model::SettlementKind) -> &'static str {
    match kind {
        crate::model::SettlementKind::Ach => "ach",
        crate::model::SettlementKind::Card => "card",
        crate::model::SettlementKind::Check => "check",
    }
}

fn settlement_state(state: crate::model::SettlementStateKind) -> &'static str {
    use crate::model::SettlementStateKind as State;
    match state {
        State::Issued => "issued",
        State::Authorized => "authorized",
        State::Presented => "presented",
        State::Pending => "pending",
        State::Settled => "settled",
        State::Returned => "returned",
        State::Reversed => "reversed",
        State::Rejected => "rejected",
        State::Cancelled => "cancelled",
        State::Refunded => "refunded",
        State::Disputed => "disputed",
        State::ChargedBack => "charged-back",
        State::Represented => "represented",
        State::Resolved => "resolved",
    }
}

fn satisfaction_state(state: crate::model::SatisfactionState) -> &'static str {
    match state {
        crate::model::SatisfactionState::Proposed => "proposed",
        crate::model::SatisfactionState::Applied => "applied",
        crate::model::SatisfactionState::Reversed => "reversed",
    }
}

/// Render the fixed V0 source vocabulary and the policy selected by this book.
/// Package hashes make a review reproducible without exposing internal
/// implementation details in ordinary diagnostics.
pub fn render_packages(book: &str, policy: Option<&str>) -> String {
    render_packages_with_registry(book, policy, &PolicyRegistry::builtins())
}

/// Render the package set actually used for an analysis.  The legacy
/// [`render_packages`] entry point remains the builtin-only view for callers
/// that do not have a committed package context; workspace-bound callers
/// should pass the registry returned with [`crate::workspace::CommitAnalysis`].
pub fn render_packages_with_registry(
    book: &str,
    policy: Option<&str>,
    registry: &PolicyRegistry,
) -> String {
    let mut out = String::new();
    out.push_str("packages\n");
    let _ = writeln!(out, "book: {book}");
    out.push_str("\nsource vocabulary\n");
    out.push_str("  buy, sell, quote, obligation, settlement, satisfy, observe, use, decide\n");
    out.push_str("  V0 keeps this vocabulary fixed.\n");
    out.push_str("\npolicies\n");
    let packages = registry.packages().collect::<Vec<_>>();
    for package in &packages {
        let active = policy == Some(package.name.as_str());
        let marker = if active { "  [active]" } else { "" };
        let _ = writeln!(
            out,
            "  {}@{}  {}{}",
            package.name,
            package.version,
            package.hash(),
            marker
        );
    }
    if let Some(selected) = policy
        && !packages.iter().any(|package| package.name == selected)
    {
        let _ = writeln!(out, "  {selected}  [selected, unavailable]");
    }
    out
}

fn render_entry(entry: &JournalEntry, complete: bool, out: &mut String) {
    let _ = writeln!(out, "entry {}", entry.sale);
    for line in &entry.lines {
        let _ = writeln!(
            out,
            "  {:<6} {:<24} {}",
            side(line),
            line.account,
            quantity(&line.quantity)
        );
    }
    let _ = writeln!(
        out,
        "  {}",
        if complete && entry.balanced() {
            "balanced"
        } else if complete {
            "unbalanced"
        } else {
            "partial (missing settlement account)"
        }
    );
}

/// Quotes are useful review attention, but a conflicting valuation quote does
/// not block a directly stated sale or a cash journal in V0.
fn render_attention(analysis: &Analysis, out: &mut String) {
    let quote_issues = analysis
        .issues
        .iter()
        .filter(|issue| issue.code == IssueCode::AmbiguousQuote)
        .collect::<Vec<_>>();
    if quote_issues.is_empty() {
        return;
    }
    out.push_str("\nattention\n");
    for issue in quote_issues {
        let _ = writeln!(out, "  {}: {}", issue_code(issue), issue.message);
    }
    out.push_str("next\n");
    out.push_str("  inspect the conflicting quote evidence (selection is deferred in V0)\n");
}

/// Whether the CLI should return a non-zero status for this view.
///
/// Quote disagreement is deliberately excluded: it is useful attention, but
/// it does not prevent a directly evidenced sale from being recognized.
pub fn has_blocking_state(analysis: &Analysis) -> bool {
    analysis
        .issues
        .iter()
        .any(|issue| issue.code != IssueCode::AmbiguousQuote)
        || analysis.sales.iter().any(|sale| sale.status.is_blocked())
}

/// Whether the journal command has an unresolved recognition or settlement
/// account.  A check can still report an observed sale while this projection
/// remains incomplete.
pub fn journal_is_blocked(analysis: &Analysis) -> bool {
    has_blocking_state(analysis)
        || analysis
            .sales
            .iter()
            .any(|sale| sale.status.is_complete() && !has_settlement(analysis, &sale.id))
}

fn has_settlement(analysis: &Analysis, sale: &str) -> bool {
    analysis.settlements.iter().any(|settlement| {
        settlement.reference == sale
            && settlement.into.is_some()
            && matches!(
                settlement.status,
                ObservationStatus::Observed | ObservationStatus::Reconciled
            )
    })
}

fn observation_label(status: &ObservationStatus) -> &'static str {
    match status {
        ObservationStatus::Observed => "observed",
        ObservationStatus::Reconciled => "reconciled",
        ObservationStatus::Conflict => "observed (conflict)",
    }
}

fn decision_answers(analysis: &Analysis, sale: &str) -> Vec<String> {
    let mut answers = analysis
        .proof
        .nodes
        .values()
        .filter_map(|node| match &node.operation {
            Operation::Decision { subject, answer } if subject == sale => Some(answer.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    answers.sort();
    answers.dedup();
    answers
}

fn render_next_actions(analysis: &Analysis, out: &mut String) {
    let mut actions = BTreeSet::new();
    for sale in &analysis.sales {
        match &sale.status {
            RecognitionStatus::Ambiguous { .. } => {
                actions.insert(format!("use lots/fifo for {}", analysis.book));
                for lot in &sale.eligible_lots {
                    actions.insert(format!("decide {} lot {lot}", sale.id));
                }
            }
            RecognitionStatus::Conflict { .. } => {
                actions.insert(format!(
                    "remove the conflicting lot decision or policy for {} (or decide {} lot LOT_ID)",
                    sale.id, sale.id
                ));
            }
            RecognitionStatus::MissingLot => {
                actions.insert(format!("add an eligible acquisition lot for {}", sale.id));
            }
            RecognitionStatus::InvalidAmount => {
                actions.insert(format!("provide concrete amount and unit for {}", sale.id));
            }
            RecognitionStatus::Recognized => {}
        };
    }
    for issue in &analysis.issues {
        match &issue.code {
            IssueCode::AmbiguousQuote => {}
            IssueCode::UnknownPolicy => {
                actions.insert("replace the policy with a known policy package".into());
            }
            IssueCode::IncompatibleUnit => {
                actions.insert("make the affected quantities use compatible units".into());
            }
            IssueCode::InvalidAmount => {
                actions.insert("provide an exact amount with its unit".into());
            }
            IssueCode::InsufficientInventory => {
                actions.insert("add or identify enough eligible inventory for the sale".into());
            }
            IssueCode::PositionConflict | IssueCode::SettlementConflict => {}
            IssueCode::ObligationConflict => {
                actions.insert(
                    "correct the obligation, settlement history, or satisfaction allocation".into(),
                );
            }
            IssueCode::AmbiguousLot
            | IssueCode::DecisionConflict
            | IssueCode::PolicyDecisionConflict
            | IssueCode::MissingLot => {}
        };
    }
    if !actions.is_empty() {
        out.push_str("\nnext\n");
        for action in actions {
            let _ = writeln!(out, "  {action}");
        }
    }
}

fn issue_code(issue: &Issue) -> &'static str {
    match issue.code {
        IssueCode::AmbiguousLot => "ambiguous lot",
        IssueCode::DecisionConflict => "decision conflict",
        IssueCode::PolicyDecisionConflict => "policy/decision conflict",
        IssueCode::MissingLot => "missing lot",
        IssueCode::InsufficientInventory => "insufficient inventory",
        IssueCode::PositionConflict => "position conflict",
        IssueCode::SettlementConflict => "settlement conflict",
        IssueCode::AmbiguousQuote => "conflicting quote",
        IssueCode::UnknownPolicy => "unknown policy",
        IssueCode::IncompatibleUnit => "incompatible unit",
        IssueCode::InvalidAmount => "invalid amount",
        IssueCode::ObligationConflict => "obligation conflict",
    }
}

fn side(line: &JournalLine) -> &'static str {
    match line.side {
        Side::Debit => "debit",
        Side::Credit => "credit",
    }
}

fn quantity(quantity: &crate::engine::Quantity) -> String {
    quantity.canonical()
}

fn quote_label(group: &str) -> String {
    let mut parts = group.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(date), Some(base), Some(counter)) => format!("{base}/{counter} on {date}"),
        _ => group.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::analyze;
    use crate::parser::parse_ledger;

    const AMBIGUOUS: &str = r#"book tax-us
buy buy/one on 2026-01-04
  10 ABC into brokerage
  for 200 USD
  fee 1 USD
buy buy/two on 2026-02-01
  10 ABC into brokerage
  for 300 USD
sell sell on 2026-09-20
  10 ABC from brokerage
  for 500 USD
  lot ?lot
quote quote/one on 2026-09-20
  1 ABC = 52 USD
quote quote/two on 2026-09-20
  1 ABC = 53 USD
observe position brokerage 10 ABC
observe settlement sell 500 USD
"#;

    #[test]
    fn check_names_accounting_state_before_repair() {
        let analysis = analyze(&parse_ledger(AMBIGUOUS).unwrap());
        let output = render_check(&analysis);
        assert!(output.starts_with("check: blocked\nbook: tax-us"));
        assert!(output.contains("position"));
        assert!(output.contains("gain: conditional"));
        assert!(output.contains("buy/one -> 299 USD"));
        assert!(output.contains("next\n"));
    }

    #[test]
    fn why_is_a_stable_source_explanation() {
        let analysis = analyze(&parse_ledger(AMBIGUOUS).unwrap());
        let output = render_why(&analysis, "gain:sell").unwrap();
        assert!(output.starts_with("why gain:sell\nstatus: ambiguous lot"));
        assert!(output.contains("because:\n"));
        assert!(output.contains("source: sell `sell`"));
        assert!(!output.contains("proof\n"));
        assert!(!output.contains("already shown"));
        assert!(!output.contains('['));
        assert_eq!(output, render_why(&analysis, "gain:sell").unwrap());
    }

    #[test]
    fn why_explains_authored_obligations_settlements_and_satisfactions() {
        let analysis = analyze(
            &parse_ledger(
                r#"book receivables
obligation invoice/a
  debtor customer
  creditor vendor
  performance transfer 10 USD
  due 2026-10-01
settlement payment/a
  kind ach
  from customer
  to vendor
  amount 10 USD
  state issued
  state presented
  state settled
satisfy allocation/a
  obligation invoice/a
  settlement payment/a
  amount 10 USD
  state applied
"#,
            )
            .unwrap(),
        );
        let obligation = render_why(&analysis, "obligation:invoice/a").unwrap();
        assert!(obligation.contains("status: satisfied"));
        assert!(obligation.contains("customer owes 10 USD to vendor"));
        assert!(obligation.contains("due: 2026-10-01"));
        let settlement = render_why(&analysis, "settlement:payment/a").unwrap();
        assert!(settlement.contains("status: effective"));
        assert!(settlement.contains("current state: settled"));
        let satisfaction = render_why(&analysis, "satisfaction:allocation/a").unwrap();
        assert!(satisfaction.contains("status: effective"));
        assert!(satisfaction.contains("allocation state: applied"));
    }

    #[test]
    fn packages_are_content_addressed_and_mark_active_policy() {
        let output = render_packages("tax-us", Some("lots/fifo"));
        assert!(output.contains("source vocabulary\n"));
        assert!(
            output.contains(
                "buy, sell, quote, obligation, settlement, satisfy, observe, use, decide"
            )
        );
        assert!(output.contains("lots/fifo@0"));
        assert!(output.contains("[active]"));
    }

    #[test]
    fn blocked_sale_does_not_overstate_position_reconciliation() {
        let analysis = analyze(&parse_ledger(AMBIGUOUS).unwrap());
        let output = render_check(&analysis);
        assert!(output.contains("brokerage  10 ABC  observed (conflict)"));
        assert!(output.contains("sell  500 USD  reconciled"));
        assert!(!output.contains("proven"));
        assert!(
            render_why(&analysis, "position:brokerage")
                .unwrap()
                .contains("status: observed (conflict)")
        );
    }

    #[test]
    fn packages_show_an_unavailable_selected_policy() {
        let output = render_packages("tax-us", Some("lots/community"));
        assert!(output.contains("lots/community  [selected, unavailable]"));
    }

    #[test]
    fn quote_attention_does_not_block_fifo_check() {
        let analysis = analyze(
            &parse_ledger(&AMBIGUOUS.replace("lot ?lot", "lot ?lot\n\nuse lots/fifo for tax-us"))
                .unwrap(),
        );
        let output = render_check(&analysis);
        assert!(output.starts_with("check: ok\n"));
        assert!(output.contains("attention\n"));
        assert!(output.contains("conflicting quote"));
    }

    #[test]
    fn decision_suggestions_use_the_actual_command_shape() {
        let analysis = analyze(&parse_ledger(AMBIGUOUS).unwrap());
        let output = render_check(&analysis);
        assert!(output.contains("decide sell lot buy/one"));
        assert!(output.contains("decide sell lot buy/two"));
    }

    #[test]
    fn journal_without_an_account_is_partial_and_blocked() {
        let source = AMBIGUOUS.replace("lot ?lot", "lot ?lot\n\nuse lots/fifo for tax-us");
        let analysis = analyze(&parse_ledger(&source).unwrap());
        let output = render_journal(&analysis);
        assert!(journal_is_blocked(&analysis));
        assert!(output.contains("journal: partial (blocked)"));
        assert!(output.contains("missing settlement account"));
        assert!(!output.contains("\n  balanced\n"));
    }

    #[test]
    fn complete_settlement_keeps_quote_attention_nonblocking() {
        let source = AMBIGUOUS
            .replace("lot ?lot", "lot ?lot\n\nuse lots/fifo for tax-us")
            .replace(
                "observe settlement sell 500 USD",
                "observe settlement sell 500 USD into checking",
            );
        let analysis = analyze(&parse_ledger(&source).unwrap());
        let output = render_journal(&analysis);
        assert!(output.starts_with("journal: balanced\n"));
        assert!(output.contains("attention\n"));
        assert!(!output.contains("journal: partial"));
    }
}
