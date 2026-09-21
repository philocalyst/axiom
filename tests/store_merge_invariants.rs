//! Adversarial coverage for the public three-way store merge boundary.

use axiom_ledger::model::Date;
use axiom_ledger::store::{Commit, Completeness, Decision, MergeConflict, ObjectStore, StoreError};

fn date(value: &str) -> Date {
    value.parse().expect("valid test date")
}

fn root(store: &mut ObjectStore) -> axiom_ledger::store::CommitId {
    store
        .put_commit(Commit::new([], [], [], [], [], [], [], "root"))
        .expect("root commit")
}

#[test]
fn merge_rejects_a_base_that_is_not_common_to_both_heads() {
    let mut store = ObjectStore::new();
    let base = root(&mut store);
    let left = store
        .put_commit(Commit::new([base], [], [], [], [], [], [], "left"))
        .expect("left descendant");
    let unrelated = store
        .put_commit(Commit::new([], [], [], [], [], [], [], "unrelated"))
        .expect("unrelated root");

    assert!(matches!(
        store.merge_three_way(base, left, unrelated, "merge"),
        Err(StoreError::BaseNotAncestor { base: found, branch })
            if found == base && branch == unrelated
    ));
}

#[test]
fn decision_conflicts_are_scoped_to_different_selected_values() {
    let mut store = ObjectStore::new();
    let base = root(&mut store);

    // Distinct immutable decisions with the same subject, scope, and
    // selection are concurrent metadata revisions, not a choice conflict.
    let left_same_choice = store
        .put_decision(
            Decision::new("sale/1", "lot/a")
                .with_scope("tax/2026")
                .with_rationale("left review"),
        )
        .expect("left decision");
    let right_same_choice = store
        .put_decision(
            Decision::new("sale/1", "lot/a")
                .with_scope("tax/2026")
                .with_rationale("right review"),
        )
        .expect("right decision");
    let left = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [left_same_choice],
            [],
            [],
            [],
            "left",
        ))
        .expect("left head");
    let right = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [right_same_choice],
            [],
            [],
            [],
            "right",
        ))
        .expect("right head");
    let merged = store.merge_three_way(base, left, right, "merge").unwrap();
    assert!(merged.is_clean());
    assert_eq!(
        store.commit(merged.commit).unwrap().decisions,
        vec![
            left_same_choice.min(right_same_choice),
            left_same_choice.max(right_same_choice)
        ]
    );

    // The same selected value in a disjoint scope is also independent.
    let left_tax = store
        .put_decision(Decision::new("sale/1", "lot/b").with_scope("tax/2026"))
        .expect("tax decision");
    let right_legal = store
        .put_decision(Decision::new("sale/1", "lot/c").with_scope("legal/2026"))
        .expect("legal decision");
    let left_scoped = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [left_tax],
            [],
            [],
            [],
            "left-scoped",
        ))
        .expect("left scoped head");
    let right_scoped = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [right_legal],
            [],
            [],
            [],
            "right-scoped",
        ))
        .expect("right scoped head");
    assert!(
        store
            .merge_three_way(base, left_scoped, right_scoped, "merge-scoped")
            .unwrap()
            .is_clean()
    );

    // Divergent selected values in one scope remain an explicit conflict.
    let right_other_choice = store
        .put_decision(Decision::new("sale/1", "lot/z").with_scope("tax/2026"))
        .expect("other decision");
    let right_conflict = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [right_other_choice],
            [],
            [],
            [],
            "right-conflict",
        ))
        .expect("right conflict head");
    let conflicting = store
        .merge_three_way(base, left_scoped, right_conflict, "merge-conflict")
        .unwrap();
    assert!(conflicting.conflicts.iter().any(|conflict| {
        matches!(
            conflict,
            MergeConflict::Decisions { subject, scope, .. }
                if subject == "sale/1" && scope == "tax/2026"
        )
    }));

    // Removing the base choice while the other branch edits it is also a
    // conflict; choosing the edit must not erase the deletion alternative.
    let base_with_choice = store
        .put_commit(Commit::new(
            [],
            [],
            [],
            [left_same_choice],
            [],
            [],
            [],
            "choice-base",
        ))
        .expect("choice base");
    let deleted = store
        .put_commit(Commit::new(
            [base_with_choice],
            [],
            [],
            [],
            [],
            [],
            [],
            "deleted",
        ))
        .expect("deleted choice head");
    let edited = store
        .put_commit(Commit::new(
            [base_with_choice],
            [],
            [],
            [right_other_choice],
            [],
            [],
            [],
            "edited",
        ))
        .expect("edited choice head");
    let delete_edit = store
        .merge_three_way(base_with_choice, deleted, edited, "merge-delete-edit")
        .unwrap();
    assert!(delete_edit.conflicts.iter().any(|conflict| {
        matches!(
            conflict,
            MergeConflict::Decisions { subject, scope, left, right }
                if subject == "sale/1"
                    && scope == "tax/2026"
                    && (left.is_empty() || right.is_empty())
        )
    }));
}

#[test]
fn completeness_conflicts_compare_unchanged_base_against_changed_branch() {
    let mut store = ObjectStore::new();
    let base_claim = store
        .put_completeness(Completeness::new(
            "bank_activity",
            "statement/september",
            "checking",
            Some(date("2026-09-01")),
            Some(date("2026-09-30")),
        ))
        .expect("base completeness claim");
    let base = store
        .put_commit(Commit::new([], [], [], [], [base_claim], [], [], "base"))
        .expect("base commit");
    let left = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [],
            [base_claim],
            [],
            [],
            "left-unchanged",
        ))
        .expect("left head");
    let right_claim = store
        .put_completeness(
            Completeness::new(
                "bank_activity",
                "statement/september",
                "checking",
                Some(date("2026-09-15")),
                Some(date("2026-10-15")),
            )
            .incomplete(),
        )
        .expect("right completeness claim");
    let right = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [],
            [right_claim],
            [],
            [],
            "right-changed",
        ))
        .expect("right head");

    let merged = store.merge_three_way(base, left, right, "merge").unwrap();
    assert!(merged.conflicts.iter().any(|conflict| {
        matches!(
            conflict,
            MergeConflict::Completeness {
                relation,
                source,
                scope,
                left,
                right,
                left_bounds,
                right_bounds,
            } if relation == "bank_activity"
                && source == "statement/september"
                && scope == "checking"
                && left.contains(&base_claim)
                && right.contains(&right_claim)
                && left_bounds.iter().any(|bounds| bounds.complete)
                && right_bounds.iter().any(|bounds| !bounds.complete)
        )
    }));
    assert!(
        store
            .commit(merged.commit)
            .unwrap()
            .completeness
            .contains(&right_claim)
    );

    let deleted = store
        .put_commit(Commit::new(
            [base],
            [],
            [],
            [],
            [],
            [],
            [],
            "delete-completeness",
        ))
        .expect("deleted completeness head");
    let delete_edit = store
        .merge_three_way(base, deleted, right, "merge-delete-edit")
        .unwrap();
    assert!(delete_edit.conflicts.iter().any(|conflict| {
        matches!(
            conflict,
            MergeConflict::Completeness { left, right, .. }
                if left.is_empty() || right.is_empty()
        )
    }));
}
