use axiom_v2::{Date, Model, Period, Project};
use std::{fs, str::FromStr, sync::Arc, thread};
use tempfile::tempdir;

const PACKAGE: &str = r#"package personal
event ?event
  date ?date
book general
  include dated
rule copy-date
  for e event
  emit dated
  set event (ref e)
  set date e.date
  book general
"#;

fn model() -> Model {
    Model::compile(&[PACKAGE.to_owned()]).expect("valid fixture package")
}

fn source(date: &str) -> String {
    format!("ledger household\nuse personal\nevent purchase\n  date {date}\n")
}

#[test]
fn revision_close_and_restatement_replay_after_reopen() {
    let directory = tempdir().unwrap();
    let model = model();
    let first_source = source("2026-01-05");
    let project = Project::open(directory.path()).unwrap();
    let first = project.commit(&first_source, &model, None).unwrap();
    let period = Period::new(
        Date::from_str("2026-01-01").unwrap(),
        Date::from_str("2026-01-31").unwrap(),
    )
    .unwrap();
    let first_view = project.view(&first, "general", Some(&period)).unwrap();
    assert_eq!(first_view.evaluation().claims.len(), 1);
    let prior_close = project
        .close(&first, "general", Some(&period), false)
        .unwrap();
    assert!(prior_close.complete());

    let corrected = project
        .commit(&source("2026-02-02"), &model, Some(&first))
        .unwrap();
    let restated = project.restate(&prior_close, &corrected, false).unwrap();
    assert_eq!(restated.prior_close(), Some(prior_close.id()));
    assert_eq!(project.revisions().unwrap().len(), 2);
    assert_eq!(project.closes().unwrap().len(), 2);
    assert_eq!(
        project.close_by_id(prior_close.id()).unwrap(),
        prior_close,
        "the original close remains replayable"
    );
    assert!(
        project
            .view(&corrected, "general", Some(&period))
            .unwrap()
            .evaluation()
            .claims
            .is_empty()
    );

    drop(project);
    let reopened = Project::open(directory.path()).unwrap();
    let reopened_first = reopened.revision(first.id()).unwrap();
    assert_eq!(reopened.source(&reopened_first).unwrap(), first_source);
    assert_eq!(
        reopened.model(&reopened_first).unwrap().id(),
        *first.model_id()
    );
    assert_eq!(
        reopened.close_by_id(restated.id()).unwrap().prior_close(),
        Some(prior_close.id())
    );
    reopened.verify_all().unwrap();
}

#[test]
fn restatement_rejects_contract_changes_and_non_direct_correction() {
    let directory = tempdir().unwrap();
    let project = Project::open(directory.path()).unwrap();
    let model = model();
    let original = project.commit(&source("2026-01-05"), &model, None).unwrap();
    let close = project.close(&original, "general", None, false).unwrap();
    let unrelated_ledger = project.commit(
        &source("2026-01-06").replace("household", "other"),
        &model,
        Some(&original),
    );
    assert!(unrelated_ledger.is_err());

    let changed_model = Model::compile(&[PACKAGE.replace("copy-date", "copy-date-v2")]).unwrap();
    let changed_model_revision = project
        .commit(&source("2026-01-06"), &changed_model, Some(&original))
        .unwrap();
    assert!(
        project
            .restate(&close, &changed_model_revision, false)
            .is_err()
    );

    let correction = project
        .commit(&source("2026-01-06"), &model, Some(&original))
        .unwrap();
    let second_correction = project
        .commit(&source("2026-01-07"), &model, Some(&correction))
        .unwrap();
    assert!(project.restate(&close, &second_correction, false).is_err());
}

#[test]
fn project_open_rejects_noncanonical_corruption_and_writes_never_repair_it() {
    let directory = tempdir().unwrap();
    let model = model();
    let project = Project::open(directory.path()).unwrap();
    let revision = project.commit(&source("2026-01-05"), &model, None).unwrap();
    let path = directory
        .path()
        .join("objects")
        .join(format!("{}.json", revision.id()));
    let original = fs::read(&path).unwrap();
    let mut corrupted = original.clone();
    corrupted.push(b' ');
    fs::write(&path, &corrupted).unwrap();

    assert!(Project::open(directory.path()).is_err());
    assert!(project.commit(&source("2026-01-05"), &model, None).is_err());
    assert_eq!(fs::read(path).unwrap(), corrupted);
}

#[test]
fn concurrent_identical_commits_share_one_atomic_revision() {
    let directory = tempdir().unwrap();
    let project = Arc::new(Project::open(directory.path()).unwrap());
    let model = Arc::new(model());
    let source = Arc::new(source("2026-01-05"));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let project = Arc::clone(&project);
            let model = Arc::clone(&model);
            let source = Arc::clone(&source);
            thread::spawn(move || project.commit(&source, &model, None).unwrap().id().clone())
        })
        .collect();
    let ids: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();

    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(project.revisions().unwrap().len(), 1);
}

#[test]
fn envelope_rejects_unknown_fields() {
    let directory = tempdir().unwrap();
    let project = Project::open(directory.path()).unwrap();
    let revision = project
        .commit(&source("2026-01-05"), &model(), None)
        .unwrap();
    let path = directory
        .path()
        .join("objects")
        .join(format!("{}.json", revision.id()));
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    envelope["unrecognized"] = serde_json::Value::Bool(true);
    fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();

    assert!(Project::open(directory.path()).is_err());
}

#[test]
fn period_must_be_ordered() {
    assert!(
        Period::new(
            Date::from_str("2026-02-01").unwrap(),
            Date::from_str("2026-01-31").unwrap(),
        )
        .is_err()
    );
}

#[test]
fn period_view_rejects_projected_claims_without_dates() {
    let directory = tempdir().unwrap();
    let package = r#"package nodates
event ?event
  label ?label
book general
  include label
rule copylabel
  for e event
  emit label
  set label e.label
  book general
"#;
    let model = Model::compile(&[package.to_owned()]).unwrap();
    let source = "ledger household\nuse nodates\nevent purchase\n  label \"book\"\n";
    let project = Project::open(directory.path()).unwrap();
    let revision = project.commit(source, &model, None).unwrap();
    let period = Period::new(
        Date::from_str("2026-01-01").unwrap(),
        Date::from_str("2026-01-31").unwrap(),
    )
    .unwrap();

    assert!(project.view(&revision, "general", Some(&period)).is_err());
}

#[test]
fn unresolved_book_findings_require_an_explicit_partial_close() {
    let directory = tempdir().unwrap();
    let package = PACKAGE.replace("  emit dated", "  require false\n  emit dated");
    let model = Model::compile(&[package]).unwrap();
    let project = Project::open(directory.path()).unwrap();
    let revision = project.commit(&source("2026-01-05"), &model, None).unwrap();

    assert!(project.close(&revision, "general", None, false).is_err());
    let partial = project.close(&revision, "general", None, true).unwrap();
    assert!(!partial.complete());
    assert!(!project.close_by_id(partial.id()).unwrap().complete());
    project.verify_all().unwrap();
}
