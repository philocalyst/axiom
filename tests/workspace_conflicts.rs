//! Integration coverage for conflict lifecycles at the public store/workspace boundary.
//!
//! These fixtures deliberately build the immutable objects through the public
//! APIs.  In particular, a package conflict is useful here because a resolved
//! source snapshot may retain exactly one selected package while preserving
//! the conflict record and its decision as audit roots.

use axiom_ledger::model::Date;
use axiom_ledger::store::{
    Close, Commit, ConflictRecord, Decision, Evidence, MergeConflict, ObjectStore, PolicyPackage,
    ProofObject, StoreError,
};
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const SOURCE: &[u8] = b"book tax\n";
const SOURCE_ID: &str = "book";
const PACKAGE_NAME: &str = "lots/choice";
const PACKAGE_BODY_LEFT: &[u8] = b"selector=earliest_acquisition\ntie=ambiguous";
const PACKAGE_BODY_RIGHT: &[u8] = b"selector=latest_acquisition\ntie=ambiguous";

struct PackageConflictFixture {
    store: ObjectStore,
    merged: axiom_ledger::store::CommitId,
    conflict: axiom_ledger::store::ConflictId,
    proof: axiom_ledger::store::ProofObjectId,
    left_package: axiom_ledger::store::PackageId,
    right_package: axiom_ledger::store::PackageId,
}

fn package_conflict_fixture() -> PackageConflictFixture {
    let mut store = ObjectStore::new();
    let source_evidence = store
        .put_evidence(Evidence::new("source/ledger", SOURCE_ID, SOURCE.to_vec()))
        .expect("source evidence is valid");
    let proof = store
        .put_proof(ProofObject::recognized([], b"recognized"))
        .expect("recognized proof is valid");
    let root = store
        .put_commit(Commit::new(
            [],
            [source_evidence],
            [],
            [],
            [],
            [],
            [],
            "root",
        ))
        .expect("root commit is valid");

    let left_package = store
        .put_package(PolicyPackage::new(
            PACKAGE_NAME,
            "1",
            PACKAGE_BODY_LEFT.to_vec(),
        ))
        .expect("left package is valid");
    let right_package = store
        .put_package(PolicyPackage::new(
            PACKAGE_NAME,
            "2",
            PACKAGE_BODY_RIGHT.to_vec(),
        ))
        .expect("right package is valid");
    let left = store
        .put_commit(Commit::new(
            [root],
            [source_evidence],
            [],
            [],
            [],
            [left_package],
            [],
            "left",
        ))
        .expect("left branch is valid");
    let right = store
        .put_commit(Commit::new(
            [root],
            [source_evidence],
            [],
            [],
            [],
            [right_package],
            [],
            "right",
        ))
        .expect("right branch is valid");
    let merged = store
        .merge(root, left, right, "merge")
        .expect("three-way merge is valid");
    assert_eq!(merged.conflict_objects.len(), 1);
    assert_eq!(merged.unresolved_conflicts, merged.conflict_objects);
    assert!(matches!(
        merged.conflicts.as_slice(),
        [MergeConflict::Policies { name, .. }] if name == PACKAGE_NAME
    ));

    PackageConflictFixture {
        store,
        merged: merged.commit,
        conflict: merged.conflict_objects[0],
        proof,
        left_package,
        right_package,
    }
}

fn inherited_descendant(
    store: &mut ObjectStore,
    parent: axiom_ledger::store::CommitId,
    author: &str,
) -> axiom_ledger::store::CommitId {
    let snapshot = store.commit(parent).expect("parent exists").clone();
    store
        .put_commit(
            Commit::new(
                [parent],
                snapshot.evidence,
                snapshot.statements,
                snapshot.decisions,
                snapshot.completeness,
                snapshot.packages,
                snapshot.proofs,
                author,
            )
            .with_conflicts(snapshot.conflicts),
        )
        .expect("descendant preserves its parent conflict roots")
}

fn resolved_descendant(
    store: &mut ObjectStore,
    parent: axiom_ledger::store::CommitId,
    conflict: axiom_ledger::store::ConflictId,
    selected_package: axiom_ledger::store::PackageId,
    author: &str,
) -> (
    axiom_ledger::store::CommitId,
    axiom_ledger::store::ConflictId,
    axiom_ledger::store::DecisionId,
) {
    let conflict_value = store
        .conflict(conflict)
        .expect("unresolved conflict exists")
        .conflict
        .clone();
    let resolution = store
        .put_decision(
            Decision::new(
                ConflictRecord::resolution_subject(conflict),
                selected_package.to_string(),
            )
            .with_scope("policy")
            .with_rationale("the package was selected by review"),
        )
        .expect("conflict-scoped decision is valid");
    let resolved_conflict = store
        .put_conflict(ConflictRecord::resolved(
            conflict_value,
            conflict,
            resolution,
            "the package was selected by review",
        ))
        .expect("resolution record is valid");
    let snapshot = store.commit(parent).expect("parent exists").clone();
    let child = store
        .put_commit(
            Commit::new(
                [parent],
                snapshot.evidence,
                snapshot.statements,
                snapshot.decisions.into_iter().chain([resolution]),
                snapshot.completeness,
                [selected_package],
                snapshot.proofs,
                author,
            )
            .with_conflicts([resolved_conflict]),
        )
        .expect("resolved descendant is valid");
    (child, resolved_conflict, resolution)
}

fn period() -> axiom_ledger::store::Period {
    axiom_ledger::store::Period::new(
        Date::new(2026, 1, 1).expect("valid date"),
        Date::new(2026, 12, 31).expect("valid date"),
    )
    .expect("valid period")
}

#[test]
fn unresolved_conflicts_survive_descendants_and_block_analysis_and_close() {
    let mut fixture = package_conflict_fixture();
    let descendant = inherited_descendant(&mut fixture.store, fixture.merged, "descendant");
    let descendant_snapshot = fixture.store.commit(descendant).expect("descendant exists");
    assert_eq!(descendant_snapshot.conflicts, vec![fixture.conflict]);

    let inherited_merge = fixture
        .store
        .merge(fixture.merged, descendant, descendant, "inherit")
        .expect("identical descendants merge");
    assert_eq!(inherited_merge.unresolved_conflicts, vec![fixture.conflict]);
    assert_eq!(
        fixture
            .store
            .commit(inherited_merge.commit)
            .expect("inherited merge exists")
            .conflicts,
        vec![fixture.conflict]
    );

    let mut workspace = Workspace::from_store(fixture.store.clone());
    assert!(matches!(
        workspace.analyze_commit(descendant),
        Err(WorkspaceError::NotSourceCommit { reason, .. })
            if reason.contains("unresolved semantic conflicts")
    ));
    assert!(matches!(
        fixture.store.put_close(Close::new(
            period(),
            "tax",
            [],
            descendant,
            fixture.proof.hash(),
        )),
        Err(StoreError::InvalidObject(reason))
            if reason.contains("unresolved semantic conflict")
    ));
}

#[test]
fn arbitrary_decisions_fail_but_explicit_conflict_scoped_resolution_unblocks() {
    let mut fixture = package_conflict_fixture();
    let conflict_value = fixture
        .store
        .conflict(fixture.conflict)
        .expect("unresolved conflict exists")
        .conflict
        .clone();
    let arbitrary = fixture
        .store
        .put_decision(Decision::new(
            "package/lots/choice",
            fixture.left_package.to_string(),
        ))
        .expect("arbitrary decision is still a valid decision object");
    assert!(matches!(
        fixture.store.put_conflict(ConflictRecord::resolved(
            conflict_value,
            fixture.conflict,
            arbitrary,
            "arbitrary decision",
        )),
        Err(StoreError::InvalidObject(reason))
            if reason.contains("explicitly name the conflict")
    ));

    let (resolved, resolved_conflict, resolution) = resolved_descendant(
        &mut fixture.store,
        fixture.merged,
        fixture.conflict,
        fixture.left_package,
        "resolver",
    );
    assert_eq!(
        fixture
            .store
            .conflict(resolved_conflict)
            .expect("resolved conflict exists")
            .supersedes,
        Some(fixture.conflict)
    );
    assert_eq!(
        fixture
            .store
            .commit(resolved)
            .expect("resolved commit exists")
            .decisions,
        vec![resolution]
    );

    let mut workspace = Workspace::from_store(fixture.store.clone());
    let analysis = workspace
        .analyze_commit(resolved)
        .expect("explicit conflict-scoped resolution unblocks analysis");
    analysis
        .check_proof()
        .expect("bound analysis proof is valid");
    let mut analyzed_store = workspace.store().clone();
    let resolved_snapshot = analyzed_store
        .commit(resolved)
        .expect("resolved commit exists")
        .clone();
    let close_source = analyzed_store
        .put_commit(
            Commit::new(
                [resolved],
                resolved_snapshot.evidence,
                resolved_snapshot.statements,
                resolved_snapshot.decisions,
                resolved_snapshot.completeness,
                resolved_snapshot.packages,
                [fixture.proof],
                "recognizer",
            )
            .with_conflicts(resolved_snapshot.conflicts),
        )
        .expect("recognized commit pins the typed proof");
    analyzed_store
        .put_close(Close::new(
            period(),
            "tax",
            [fixture.left_package],
            close_source,
            fixture.proof.hash(),
        ))
        .expect("explicit conflict-scoped resolution unblocks close");
}

#[test]
fn divergent_resolutions_remain_blocking() {
    let mut fixture = package_conflict_fixture();
    let (left, left_resolution_conflict, _) = resolved_descendant(
        &mut fixture.store,
        fixture.merged,
        fixture.conflict,
        fixture.left_package,
        "left resolver",
    );
    let (right, right_resolution_conflict, _) = resolved_descendant(
        &mut fixture.store,
        fixture.merged,
        fixture.conflict,
        fixture.right_package,
        "right resolver",
    );
    assert_ne!(left_resolution_conflict, right_resolution_conflict);

    let merged = fixture
        .store
        .merge(fixture.merged, left, right, "divergent resolvers")
        .expect("divergent resolution branches merge as data");
    assert!(!merged.unresolved_conflicts.is_empty());
    assert!(merged.conflicts.iter().any(|conflict| {
        matches!(
            conflict,
            MergeConflict::Decisions { subject, .. }
                if subject == &ConflictRecord::resolution_subject(fixture.conflict)
        )
    }));
    assert!(matches!(
        fixture.store.put_close(Close::new(
            period(),
            "tax",
            [],
            merged.commit,
            fixture.proof.hash(),
        )),
        Err(StoreError::InvalidObject(reason))
            if reason.contains("conflicting semantic conflict resolutions")
                || reason.contains("unresolved semantic conflict")
    ));
}

#[test]
fn as_known_at_never_silently_chooses_a_merge_side() {
    let mut store = ObjectStore::new();
    let original = store
        .put_evidence(Evidence::new("source/ledger", SOURCE_ID, SOURCE.to_vec()))
        .expect("source evidence is valid");
    let root = store
        .put_commit(Commit::new([], [original], [], [], [], [], [], "root"))
        .expect("root commit is valid");
    let left_evidence = store
        .put_evidence(Evidence::correction(
            "source/ledger",
            SOURCE_ID,
            b"book tax\n# left\n".to_vec(),
            original,
            "whole-source",
            "left correction",
            "left",
        ))
        .expect("left correction is valid");
    let right_evidence = store
        .put_evidence(Evidence::correction(
            "source/ledger",
            SOURCE_ID,
            b"book tax\n# right\n".to_vec(),
            original,
            "whole-source",
            "right correction",
            "right",
        ))
        .expect("right correction is valid");
    let left = store
        .put_commit(Commit::new(
            [root],
            [left_evidence],
            [],
            [],
            [],
            [],
            [],
            "left",
        ))
        .expect("left branch is valid");
    let right = store
        .put_commit(Commit::new(
            [root],
            [right_evidence],
            [],
            [],
            [],
            [],
            [],
            "right",
        ))
        .expect("right branch is valid");
    let merged = store
        .merge(root, left, right, "merge")
        .expect("correction branches merge as data");

    let visible = store
        .evidence_as_known_at(SOURCE_ID, merged.commit)
        .expect("known-at query is valid");
    assert_eq!(visible.len(), 2);
    assert!(visible.contains(&left_evidence));
    assert!(visible.contains(&right_evidence));

    let workspace = Workspace::from_store(store);
    assert!(matches!(
        workspace.as_known_at(SOURCE_ID, merged.commit),
        Err(WorkspaceError::NotSourceCommit { reason, .. })
            if reason.contains("unresolved evidence alternatives")
    ));
}

#[test]
fn package_and_correction_workflows_preserve_conflict_roots() {
    let mut fixture = package_conflict_fixture();
    let (resolved, resolved_conflict, _) = resolved_descendant(
        &mut fixture.store,
        fixture.merged,
        fixture.conflict,
        fixture.left_package,
        "resolver",
    );
    let mut workspace = Workspace::from_store(fixture.store);

    let packaged = workspace
        .commit_with_packages(resolved, [fixture.left_package])
        .expect("package workflow accepts the resolved source snapshot");
    assert_eq!(
        workspace
            .store()
            .commit(packaged.commit)
            .expect("packaged commit exists")
            .conflicts,
        vec![resolved_conflict]
    );

    let corrected = workspace
        .correct_source(packaged.commit, b"book tax\n# corrected\n")
        .expect("correction workflow accepts the resolved source snapshot");
    assert_eq!(
        workspace
            .store()
            .commit(corrected.commit)
            .expect("corrected commit exists")
            .conflicts,
        vec![resolved_conflict]
    );
    assert_eq!(corrected.bytes(), b"book tax\n# corrected\n");
}
