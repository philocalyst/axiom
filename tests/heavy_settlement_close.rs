//! A deliberately dense end-to-end exercise of the typed settlement package.
//!
//! The fixture stays source-first: package capability, artifact, source commit,
//! proof, world, recognition, journals, closes, and corrections all cross the
//! same public boundaries a caller would use.

use std::collections::BTreeMap;

use axiom_ledger::hir::{
    AstDeclaration, AstDeclarationKind, AstModule, AstType, ModulePath, Name, QualifiedName, Span,
};
use axiom_ledger::model::{AccountId, ContentHash, Date, EntityId, SettlementKind};
use axiom_ledger::ontology::{OntologyError, SettlementState};
use axiom_ledger::package_compiler::{PackageInput, SchemaCapability};
use axiom_ledger::package_lock::{
    Dependency, LockedPackage, Lockfile, PackageManifest, Version, VersionReq,
};
use axiom_ledger::settlement_books::{
    SettlementBookError, SettlementRecognition, SettlementRecognitionPolicy,
    SettlementReportingPeriod,
};
use axiom_ledger::settlement_projection::SettlementProjectionError;
use axiom_ledger::store::CommitId;
use axiom_ledger::workspace::{Workspace, WorkspaceError};

const FIXTURE: &str = include_str!("../fixtures/heavy/settlement_world_close.axm");

fn settlement_schema() -> QualifiedName {
    QualifiedName {
        module: ModulePath::new(vec![Name::new("types").unwrap()]).unwrap(),
        name: Name::new("SettlementState").unwrap(),
    }
}

fn settlement_package() -> (PackageInput, Lockfile) {
    let manifest = PackageManifest::new(
        "settlements",
        Version::new(1, 0, 0),
        "heavy settlement state package",
    );
    let module = AstModule {
        path: ModulePath::root(Name::new("types").unwrap()),
        declarations: vec![AstDeclaration {
            name: "SettlementState".to_owned(),
            kind: AstDeclarationKind::Type(AstType::Record {
                fields: vec![
                    ("settlement".to_owned(), AstType::Text),
                    ("kind".to_owned(), AstType::Text),
                    ("state".to_owned(), AstType::Text),
                    ("at".to_owned(), AstType::Text),
                    ("from".to_owned(), AstType::Text),
                    ("to".to_owned(), AstType::Text),
                    ("instrument".to_owned(), AstType::Text),
                    ("amount".to_owned(), AstType::Decimal),
                ],
                open_tail: None,
            }),
            span: Span::default(),
        }],
    };
    let package = PackageInput::new(manifest.clone(), [axiom_ledger::hir::lower(module)])
        .with_schema_capability(settlement_schema(), SchemaCapability::SettlementStateV1);
    let manifest_hash = manifest.hash();
    let lockfile = Lockfile {
        roots: vec![Dependency::new(
            manifest.name.clone(),
            VersionReq::Exact(manifest.version),
        )],
        packages: vec![LockedPackage {
            name: manifest.name,
            version: manifest.version,
            hash: manifest_hash,
            dependencies: Vec::new(),
        }],
    };
    (package, lockfile)
}

fn workspace_for(source: &str) -> (Workspace, CommitId) {
    let (package, lockfile) = settlement_package();
    let mut workspace = Workspace::new();
    let source_commit = workspace.load_source("heavy-settlement", source).unwrap();
    let (artifact_id, artifact) = workspace
        .compile_packages_persisted([package], &lockfile)
        .unwrap();
    assert_eq!(
        artifact.artifact_hash(),
        workspace
            .compiled_artifact(artifact_id)
            .unwrap()
            .artifact_hash()
    );
    assert_eq!(artifact.artifact_hash(), artifact.recomputed_hash());
    let pinned = workspace
        .commit_with_compiled_artifact(source_commit.commit, artifact_id)
        .unwrap();
    let commit = workspace.store().commit(pinned.commit).unwrap();
    assert_eq!(commit.parents, vec![source_commit.commit]);
    assert_eq!(commit.compiled_artifact, Some(artifact_id));
    (workspace, pinned.commit)
}

fn all_accounts() -> BTreeMap<EntityId, AccountId> {
    BTreeMap::from([
        (
            EntityId::from("merchant"),
            AccountId::from("receivable/merchant"),
        ),
        (
            EntityId::from("customer"),
            AccountId::from("receivable/customer"),
        ),
        (
            EntityId::from("clearing-usd"),
            AccountId::from("cash/clearing-usd"),
        ),
        (
            EntityId::from("clearing-eur"),
            AccountId::from("cash/clearing-eur"),
        ),
        (
            EntityId::from("card-network"),
            AccountId::from("cash/card-network"),
        ),
    ])
}

fn cash_policy() -> SettlementRecognitionPolicy {
    SettlementRecognitionPolicy::cash(all_accounts()).unwrap()
}

fn period(year: i32, month: u8) -> SettlementReportingPeriod {
    SettlementReportingPeriod::new(
        Date::new(year, month, 1).unwrap(),
        Date::new(year, month, if month == 2 { 28 } else { 31 }).unwrap(),
    )
}

fn replace_form_field(source: &str, occurrence: &str, from: &str, to: &str) -> String {
    let marker = format!("form {occurrence} : ");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("fixture form {occurrence} is missing"));
    let end = source[start..]
        .find("\n\n")
        .map_or(source.len(), |offset| start + offset);
    let form = &source[start..end];
    assert_eq!(
        form.matches(from).count(),
        1,
        "expected one occurrence of {from:?} in form {occurrence}"
    );
    let replacement = form.replacen(from, to, 1);
    format!("{}{}{}", &source[..start], replacement, &source[end..])
}

fn assert_history(
    world: &axiom_ledger::settlement_books::SettlementWorld,
    settlement: &str,
    rail: SettlementKind,
    expected: &[(SettlementState, Date)],
) {
    let history = world.history(settlement).unwrap();
    assert_eq!(history.kind(), rail, "rail for {settlement}");
    assert_eq!(
        history
            .transitions()
            .iter()
            .map(|transition| (transition.state.clone(), transition.at))
            .collect::<Vec<_>>(),
        expected,
        "transitions for {settlement}"
    );
}

fn assert_fact_authorities(recognition: &SettlementRecognition, authority: ContentHash) {
    for fact in recognition.facts() {
        assert_eq!(
            fact.proof().authority(),
            authority,
            "fact {}",
            fact.settlement()
        );
    }
    if let Some(journal) = recognition.journal() {
        for entry in journal.entries() {
            assert_eq!(
                entry.proof().authority(),
                authority,
                "journal entry {}",
                entry.settlement()
            );
        }
    }
}

#[test]
fn heavy_fixture_runs_compile_proof_world_recognition_journals_and_closes() {
    let (mut workspace, source_commit) = workspace_for(FIXTURE);
    let projection = workspace.project_settlement_states(source_commit).unwrap();
    assert_eq!(projection.records().len(), 26);
    assert_eq!(
        projection
            .records()
            .map(|record| record.occurrence.as_str())
            .next(),
        Some("ach-cash/1")
    );
    assert_eq!(
        projection
            .records()
            .map(|record| record.occurrence.as_str())
            .last(),
        Some("card-cash/5")
    );
    assert_eq!(
        projection.records().next().unwrap().amount.canonical(),
        "401/4 USD"
    );
    assert_eq!(
        projection
            .records()
            .find(|record| record.occurrence.as_str() == "card-cash/5")
            .unwrap()
            .amount
            .canonical(),
        "3999/200 EUR"
    );
    assert_eq!(
        projection
            .records()
            .filter_map(|record| record.kind())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            SettlementKind::Ach,
            SettlementKind::Card,
            SettlementKind::Check,
        ])
    );

    let anchor = workspace
        .persist_settlement_state_proof(source_commit)
        .unwrap();
    assert_eq!(anchor.source_commit, source_commit);
    let child = workspace.store().commit(anchor.projection_commit).unwrap();
    assert_eq!(child.parents, vec![source_commit]);
    assert_eq!(child.settlement_proofs, vec![anchor.proof_id]);
    assert_eq!(child.author, "workspace/settlement-proof");
    let persisted_proof = workspace
        .store()
        .settlement_state_proof(anchor.proof_id)
        .unwrap();
    assert_eq!(persisted_proof.source_commit, source_commit);
    assert_eq!(persisted_proof.coverage().len(), 26);
    persisted_proof.check(workspace.store()).unwrap();

    let world = workspace.settlement_world(source_commit).unwrap();
    assert_eq!(world.source_commit(), source_commit);
    assert_eq!(world.settlement_proof(), persisted_proof);
    assert_eq!(world.histories().len(), 6);
    assert_history(
        &world,
        "ach-cash",
        SettlementKind::Ach,
        &[
            (SettlementState::Issued, Date::new(2026, 1, 3).unwrap()),
            (SettlementState::Presented, Date::new(2026, 1, 4).unwrap()),
            (SettlementState::Pending, Date::new(2026, 1, 5).unwrap()),
            (SettlementState::Settled, Date::new(2026, 1, 6).unwrap()),
        ],
    );
    assert_history(
        &world,
        "ach-returned",
        SettlementKind::Ach,
        &[
            (SettlementState::Issued, Date::new(2026, 1, 7).unwrap()),
            (SettlementState::Presented, Date::new(2026, 1, 8).unwrap()),
            (SettlementState::Pending, Date::new(2026, 1, 9).unwrap()),
            (SettlementState::Returned, Date::new(2026, 1, 10).unwrap()),
        ],
    );
    assert_history(
        &world,
        "card-chargeback",
        SettlementKind::Card,
        &[
            (SettlementState::Issued, Date::new(2026, 1, 12).unwrap()),
            (SettlementState::Authorized, Date::new(2026, 1, 12).unwrap()),
            (SettlementState::Presented, Date::new(2026, 1, 13).unwrap()),
            (SettlementState::Settled, Date::new(2026, 1, 15).unwrap()),
            (SettlementState::Disputed, Date::new(2026, 2, 10).unwrap()),
            (
                SettlementState::ChargedBack,
                Date::new(2026, 2, 11).unwrap(),
            ),
        ],
    );
    assert_history(
        &world,
        "check-represented",
        SettlementKind::Check,
        &[
            (SettlementState::Issued, Date::new(2026, 1, 29).unwrap()),
            (SettlementState::Presented, Date::new(2026, 1, 30).unwrap()),
            (SettlementState::Returned, Date::new(2026, 2, 4).unwrap()),
            (SettlementState::Presented, Date::new(2026, 2, 6).unwrap()),
            (SettlementState::Settled, Date::new(2026, 2, 7).unwrap()),
        ],
    );
    assert_history(
        &world,
        "ach-cancelled",
        SettlementKind::Ach,
        &[
            (SettlementState::Issued, Date::new(2026, 2, 10).unwrap()),
            (SettlementState::Cancelled, Date::new(2026, 2, 11).unwrap()),
        ],
    );
    assert_history(
        &world,
        "card-cash",
        SettlementKind::Card,
        &[
            (SettlementState::Issued, Date::new(2026, 3, 1).unwrap()),
            (SettlementState::Authorized, Date::new(2026, 3, 2).unwrap()),
            (SettlementState::Presented, Date::new(2026, 3, 3).unwrap()),
            (SettlementState::Pending, Date::new(2026, 3, 4).unwrap()),
            (SettlementState::Settled, Date::new(2026, 3, 5).unwrap()),
        ],
    );
    assert!(world.history("ach-cash").unwrap().effective());
    assert_eq!(
        world.history("ach-returned").unwrap().current_state(),
        &SettlementState::Returned
    );
    assert_eq!(
        world.history("ach-cancelled").unwrap().current_state(),
        &SettlementState::Cancelled
    );
    assert_eq!(
        world.history("card-chargeback").unwrap().current_state(),
        &SettlementState::ChargedBack
    );
    assert_eq!(
        world.history("check-represented").unwrap().kind(),
        SettlementKind::Check
    );

    let observation = world
        .recognize(SettlementRecognitionPolicy::observation())
        .unwrap();
    assert!(matches!(observation, SettlementRecognition::Observation(_)));
    assert_eq!(observation.facts().len(), 6);
    assert_eq!(
        observation
            .facts()
            .iter()
            .map(|fact| fact.settlement())
            .collect::<Vec<_>>(),
        vec![
            "ach-cancelled",
            "ach-cash",
            "ach-returned",
            "card-cash",
            "card-chargeback",
            "check-represented",
        ]
    );
    observation.check(&world).unwrap();
    assert_fact_authorities(&observation, world.authority_hash());

    let policy = cash_policy();
    let cash = world.recognize(policy.clone()).unwrap();
    assert!(matches!(cash, SettlementRecognition::Cash(_)));
    assert_eq!(
        cash.facts().len(),
        3,
        "returned/cancelled/chargeback are excluded from cash"
    );
    assert_eq!(cash.facts()[0].settlement(), "ach-cash");
    assert_eq!(cash.facts()[1].settlement(), "card-cash");
    assert_eq!(cash.facts()[2].settlement(), "check-represented");
    let journal = cash.journal().unwrap();
    assert_eq!(journal.entries().len(), 3);
    assert!(journal.is_structurally_balanced());
    assert_eq!(journal.entries()[0].debit().as_str(), "cash/clearing-usd");
    assert_eq!(
        journal.entries()[0].credit().as_str(),
        "receivable/merchant"
    );
    assert_eq!(journal.entries()[0].amount().canonical(), "401/4 USD");
    assert_eq!(journal.entries()[1].debit().as_str(), "cash/card-network");
    assert_eq!(
        journal.entries()[1].credit().as_str(),
        "receivable/customer"
    );
    assert_eq!(journal.entries()[1].amount().canonical(), "3999/200 EUR");
    assert_eq!(journal.entries()[2].debit().as_str(), "cash/clearing-eur");
    assert_eq!(
        journal.entries()[2].credit().as_str(),
        "receivable/customer"
    );
    assert_eq!(journal.entries()[2].amount().canonical(), "607/8 EUR");
    cash.check(&world).unwrap();
    assert_fact_authorities(&cash, world.authority_hash());

    let january = world.close(policy.clone(), period(2026, 1)).unwrap();
    let february = world.close(policy.clone(), period(2026, 2)).unwrap();
    let march = world.close(policy.clone(), period(2026, 3)).unwrap();
    january.check(workspace.store()).unwrap();
    february.check(workspace.store()).unwrap();
    march.check(workspace.store()).unwrap();
    assert_fact_authorities(january.recognition(), world.authority_hash());
    assert_fact_authorities(february.recognition(), world.authority_hash());
    assert_fact_authorities(march.recognition(), world.authority_hash());
    assert_eq!(january.recognition().facts().len(), 1);
    assert_eq!(february.recognition().facts().len(), 1);
    assert_eq!(march.recognition().facts().len(), 1);
    assert_eq!(january.recognition().facts()[0].settlement(), "ach-cash");
    assert!(
        !january
            .recognition()
            .facts()
            .iter()
            .any(|fact| fact.settlement() == "check-represented")
    );
    assert_eq!(
        february.recognition().facts()[0].settlement(),
        "check-represented"
    );
    assert_eq!(march.recognition().facts()[0].settlement(), "card-cash");
    assert_ne!(january.root(), february.root());
    assert_ne!(february.root(), march.root());

    let observation_january = world
        .close(SettlementRecognitionPolicy::observation(), period(2026, 1))
        .unwrap();
    assert_eq!(observation_january.recognition().facts().len(), 4);
    assert_eq!(
        observation_january
            .recognition()
            .facts()
            .iter()
            .map(|fact| fact.settlement())
            .collect::<Vec<_>>(),
        vec![
            "ach-cash",
            "ach-returned",
            "card-chargeback",
            "check-represented"
        ]
    );
    observation_january.check(workspace.store()).unwrap();
    assert_fact_authorities(observation_january.recognition(), world.authority_hash());
    workspace.store().verify().unwrap();
}

#[test]
fn roots_are_deterministic_across_fresh_workspaces_and_corrections_are_immutable() {
    let (mut first, first_source) = workspace_for(FIXTURE);
    let first_anchor = first.persist_settlement_state_proof(first_source).unwrap();
    let first_world = first.settlement_world(first_source).unwrap();
    let first_close = first_world.close(cash_policy(), period(2026, 1)).unwrap();

    let (mut second, second_source) = workspace_for(FIXTURE);
    let second_anchor = second
        .persist_settlement_state_proof(second_source)
        .unwrap();
    let second_world = second.settlement_world(second_source).unwrap();
    let second_close = second_world.close(cash_policy(), period(2026, 1)).unwrap();

    assert_eq!(first_source, second_source);
    assert_eq!(first_anchor, second_anchor);
    assert_eq!(first_world.authority_hash(), second_world.authority_hash());
    assert_eq!(first_close.root(), second_close.root());

    let old_store_len = first.store().len();
    let old_proof = first
        .store()
        .settlement_state_proof(first_anchor.proof_id)
        .unwrap()
        .clone();
    let corrected_source = ["ach-cash/1", "ach-cash/2", "ach-cash/3", "ach-cash/4"]
        .into_iter()
        .fold(FIXTURE.to_owned(), |source, occurrence| {
            replace_form_field(&source, occurrence, "amount 100.25", "amount 101.25")
        });
    let corrected = first
        .correct_source(first_source, corrected_source)
        .unwrap();
    let corrected_anchor = first
        .persist_settlement_state_proof(corrected.commit)
        .unwrap();
    let corrected_world = first.settlement_world(corrected.commit).unwrap();
    let corrected_close = corrected_world
        .close(cash_policy(), period(2026, 1))
        .unwrap();

    assert_ne!(corrected.commit, first_source);
    assert_ne!(corrected_anchor.proof_id, first_anchor.proof_id);
    assert_ne!(
        corrected_world.authority_hash(),
        first_world.authority_hash()
    );
    assert_ne!(corrected_close.root(), first_close.root());
    assert_eq!(old_proof.coverage()[0].amount.canonical(), "401/4 USD");
    assert_eq!(
        corrected_world
            .history("ach-cash")
            .unwrap()
            .amount()
            .canonical(),
        "405/4 USD"
    );
    first_close.check(first.store()).unwrap();
    first_world.check(first.store()).unwrap();
    old_proof.check(first.store()).unwrap();
    first
        .store()
        .settlement_state_proof(corrected_anchor.proof_id)
        .unwrap()
        .check(first.store())
        .unwrap();
    assert!(first.store().len() > old_store_len);
    first.store().verify().unwrap();
}

#[test]
fn invalid_mappings_and_failed_proof_persistence_are_deeply_rejected() {
    let (workspace, source_commit) = workspace_for(FIXTURE);

    let missing = SettlementRecognitionPolicy::cash([
        (
            EntityId::from("merchant"),
            AccountId::from("receivable/merchant"),
        ),
        (
            EntityId::from("customer"),
            AccountId::from("receivable/customer"),
        ),
    ])
    .unwrap();
    assert_eq!(
        workspace
            .settlement_world(source_commit)
            .unwrap()
            .recognize(missing)
            .unwrap_err(),
        SettlementBookError::MissingAccount {
            settlement: "ach-cash".to_owned(),
            endpoint: EntityId::from("clearing-usd"),
        }
    );

    let same_account = SettlementRecognitionPolicy::cash([
        (EntityId::from("merchant"), AccountId::from("cash/shared")),
        (EntityId::from("customer"), AccountId::from("cash/shared")),
        (
            EntityId::from("clearing-usd"),
            AccountId::from("cash/shared"),
        ),
        (EntityId::from("clearing-eur"), AccountId::from("cash/east")),
        (EntityId::from("card-network"), AccountId::from("cash/west")),
    ])
    .unwrap();
    assert_eq!(
        workspace
            .settlement_world(source_commit)
            .unwrap()
            .recognize(same_account)
            .unwrap_err(),
        SettlementBookError::SameAccount {
            settlement: "ach-cash".to_owned(),
            account: AccountId::from("cash/shared"),
        }
    );

    let changed_static =
        replace_form_field(FIXTURE, "ach-cash/1", "amount 100.25", "amount 100.26");
    let (changed_workspace, changed_commit) = workspace_for(&changed_static);
    assert_eq!(
        changed_workspace
            .settlement_world(changed_commit)
            .unwrap_err(),
        WorkspaceError::SettlementProjection(SettlementProjectionError::Ontology(
            OntologyError::InvalidEvent {
                kind: "settlement state",
                reason: "inconsistent facts for settlement ach-cash".to_owned(),
            },
        ))
    );

    let illegal = replace_form_field(FIXTURE, "ach-cash/2", "state presented", "state authorized");
    let (illegal_workspace, illegal_commit) = workspace_for(&illegal);
    assert_eq!(
        illegal_workspace
            .settlement_world(illegal_commit)
            .unwrap_err(),
        WorkspaceError::SettlementProjection(SettlementProjectionError::Ontology(
            OntologyError::InvalidSettlementTransition {
                from: Some(SettlementState::Issued),
                to: SettlementState::Authorized,
            },
        ))
    );

    let rail_specific = replace_form_field(
        FIXTURE,
        "check-represented/5",
        "state settled",
        "state charged-back",
    );
    let (rail_workspace, rail_commit) = workspace_for(&rail_specific);
    assert_eq!(
        rail_workspace.settlement_world(rail_commit).unwrap_err(),
        WorkspaceError::SettlementProjection(SettlementProjectionError::Ontology(
            OntologyError::InvalidSettlementTransition {
                from: Some(SettlementState::Presented),
                to: SettlementState::ChargedBack,
            },
        ))
    );

    let malformed = replace_form_field(FIXTURE, "ach-cash/4", "state settled", "state authorized");
    let (mut malformed_workspace, malformed_commit) = workspace_for(&malformed);
    let before_failed_persist = malformed_workspace.store().len();
    assert_eq!(
        malformed_workspace
            .persist_settlement_state_proof(malformed_commit)
            .unwrap_err(),
        WorkspaceError::SettlementProjection(SettlementProjectionError::Ontology(
            OntologyError::InvalidSettlementTransition {
                from: Some(SettlementState::Pending),
                to: SettlementState::Authorized,
            },
        ))
    );
    assert_eq!(malformed_workspace.store().len(), before_failed_persist);
    malformed_workspace.store().verify().unwrap();
}

#[test]
fn same_settlement_id_across_capable_package_roots_cannot_fuse_authority() {
    let (first, _) = settlement_package();
    let mut second = first.clone();
    second.manifest = PackageManifest::new(
        "settlements_alt",
        Version::new(1, 0, 0),
        "a distinct settlement state package",
    );
    let lockfile = Lockfile {
        roots: vec![
            Dependency::new("settlements", VersionReq::Exact(Version::new(1, 0, 0))),
            Dependency::new("settlements_alt", VersionReq::Exact(Version::new(1, 0, 0))),
        ],
        packages: vec![
            LockedPackage {
                name: first.manifest.name.clone(),
                version: first.manifest.version,
                hash: first.manifest.hash(),
                dependencies: first.manifest.dependencies.clone(),
            },
            LockedPackage {
                name: second.manifest.name.clone(),
                version: second.manifest.version,
                hash: second.manifest.hash(),
                dependencies: second.manifest.dependencies.clone(),
            },
        ],
    };
    let source =
        "form first/1 : settlements::types::SettlementState\n  settlement same-id\n  kind ach\n  state issued\n  at 2026-01-01\n  from merchant\n  to clearing-usd\n  instrument USD\n  amount 1\n\nform second/1 : settlements_alt::types::SettlementState\n  settlement same-id\n  kind ach\n  state presented\n  at 2026-01-02\n  from merchant\n  to clearing-usd\n  instrument USD\n  amount 1\n"
            .to_owned();
    let mut workspace = Workspace::new();
    let source_commit = workspace.load_source("cross-package", source).unwrap();
    let (artifact_id, _) = workspace
        .compile_packages_persisted([first, second], &lockfile)
        .unwrap();
    let pinned = workspace
        .commit_with_compiled_artifact(source_commit.commit, artifact_id)
        .unwrap();
    let error = workspace.settlement_world(pinned.commit).unwrap_err();
    assert!(
        matches!(
            error,
            WorkspaceError::SettlementBook(
                SettlementBookError::ConflictingSettlementIdentity { .. }
            )
        ),
        "unexpected cross-package identity result: {error:?}"
    );
    workspace.store().verify().unwrap();
}

#[test]
fn every_fixture_row_resolves_through_one_explicit_capability_identity() {
    let (workspace, source_commit) = workspace_for(FIXTURE);
    let world = workspace.settlement_world(source_commit).unwrap();
    let coverage = world.settlement_proof().coverage();
    assert_eq!(world.settlement_proof().source_commit, source_commit);
    assert_eq!(coverage.len(), 26);
    assert!(
        coverage
            .iter()
            .all(|entry| entry.qualified_schema == "types::SettlementState")
    );
    assert_eq!(
        coverage
            .iter()
            .map(|entry| (entry.package_root, entry.schema_id))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        1
    );
    world.check(workspace.store()).unwrap();
}
