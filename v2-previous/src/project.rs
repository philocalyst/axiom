//! Project-level history, replay, book views, and immutable closes.

use crate::{
    Certificate, Date, Diagnostic, Evaluation, Id, Model, Outcome, Source, View, canonical, check,
    render_diagnostics, view_period,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const REVISION_KIND: &str = "revision";
const CLOSE_KIND: &str = "close";
const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODEL_BYTES: usize = 8 * 1024 * 1024;
const MAX_PACKAGES: usize = 256;
const MAX_HISTORY_DEPTH: usize = 4096;
const CHECK_BUDGET: usize = 1_000_000;

/// An inclusive date range used to filter rows in a book projection.
///
/// This is a reporting filter over the current revision's computed view, not
/// an as-of replay of changing histories. A close still pins the exact revision
/// used to produce the filtered projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Period {
    from: Date,
    to: Date,
}

impl Period {
    pub fn new(from: Date, to: Date) -> Result<Self, String> {
        if from > to {
            return Err("period start must not be after its end".into());
        }
        Ok(Self { from, to })
    }

    pub fn from(&self) -> &Date {
        &self.from
    }
    pub fn to(&self) -> &Date {
        &self.to
    }
}

/// A verified handle to one immutable source revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Revision {
    id: Id,
    ledger: String,
    model: Id,
    world: Id,
    parent: Option<Id>,
}

impl Revision {
    pub fn id(&self) -> &Id {
        &self.id
    }
    pub fn ledger(&self) -> &str {
        &self.ledger
    }
    pub fn model_id(&self) -> &Id {
        &self.model
    }
    pub fn world_id(&self) -> &Id {
        &self.world
    }
    pub fn parent(&self) -> Option<&Id> {
        self.parent.as_ref()
    }
}

/// A verified handle to an immutable close snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Close {
    id: Id,
    revision: Id,
    world: Id,
    book: String,
    period: Option<Period>,
    complete: bool,
    prior: Option<Id>,
    view_root: Id,
    restatement_depth: usize,
}

impl Close {
    pub fn id(&self) -> &Id {
        &self.id
    }
    pub fn revision_id(&self) -> &Id {
        &self.revision
    }
    pub fn world_id(&self) -> &Id {
        &self.world
    }
    pub fn book(&self) -> &str {
        &self.book
    }
    pub fn period(&self) -> Option<&Period> {
        self.period.as_ref()
    }
    /// Completeness is scoped to this exact source, model, book, and period.
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn prior_close(&self) -> Option<&Id> {
        self.prior.as_ref()
    }

    pub fn view_root(&self) -> &Id {
        &self.view_root
    }
}

/// Durable source and model history for one project directory.
pub struct Project {
    repository: super::repository::Repository,
}

impl Project {
    /// Open a project and verify every existing object before returning it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let repository = super::repository::Repository::open(path)?;
        let project = Self { repository };
        project.verify_all()?;
        Ok(project)
    }

    /// Check and persist a source revision, its exact model package sources, and
    /// the replay certificate. A parent, when supplied, must be this ledger's
    /// verified history.
    pub fn commit(
        &self,
        source: &str,
        model: &Model,
        parent: Option<&Revision>,
    ) -> Result<Revision, String> {
        validate_source_size(source)?;
        let package_sources = package_sources_from_model(model)?;
        let (document, world, certificate) = check(source, model, CHECK_BUDGET)
            .map_err(|errors| source_diagnostics(source, errors))?;
        validate_ledger_name(&document.name)?;

        let parent_id = if let Some(parent) = parent {
            let verified = self.load_revision(parent.id())?;
            if verified.revision.ledger != document.name {
                return Err("a revision parent must belong to the same ledger".into());
            }
            Some(verified.revision.id)
        } else {
            None
        };

        // All semantic checks happen before any durable project object is added.
        let record = RevisionRecord {
            source: source.to_owned(),
            packages: package_sources,
            parent: parent_id,
            certificate,
        };
        let id = self.repository.put(REVISION_KIND, &record)?;
        Ok(Revision {
            id,
            ledger: document.name,
            model: model.id(),
            world: world.id().clone(),
            parent: record.parent.clone(),
        })
    }

    /// Reopen and fully replay a revision and its complete parent history.
    pub fn revision(&self, id: &Id) -> Result<Revision, String> {
        Ok(self.load_revision(id)?.revision)
    }

    /// Load the exact package sources pinned to a verified revision.
    pub fn model(&self, revision: &Revision) -> Result<Model, String> {
        Ok(self.load_revision(revision.id())?.model)
    }

    /// Load the exact source bytes pinned to a verified revision.
    pub fn source(&self, revision: &Revision) -> Result<String, String> {
        Ok(self.load_revision(revision.id())?.record.source)
    }

    /// Recompute the book view pinned by a revision. A period filters the
    /// resulting projection by typed dates; it does not rewind histories to an
    /// as-of state. Missing projection dates make a period view invalid.
    pub fn view(
        &self,
        revision: &Revision,
        book: &str,
        period: Option<&Period>,
    ) -> Result<View, String> {
        let loaded = self.load_revision(revision.id())?;
        self.compute_view(&loaded, book, period)
    }

    /// Create an immutable close. By default every relevant finding must be
    /// proven; `allow_partial` explicitly permits a snapshot with open findings.
    pub fn close(
        &self,
        revision: &Revision,
        book: &str,
        period: Option<&Period>,
        allow_partial: bool,
    ) -> Result<Close, String> {
        let loaded = self.load_revision(revision.id())?;
        self.create_close(&loaded, book, period, allow_partial, None)
    }

    /// Restate a prior close from its direct same-ledger, same-model correction.
    pub fn restate(
        &self,
        prior: &Close,
        correction: &Revision,
        allow_partial: bool,
    ) -> Result<Close, String> {
        let mut verified_closes = BTreeMap::new();
        let mut verified_revisions = BTreeMap::new();
        let prior = self.load_close(
            prior.id(),
            &mut verified_closes,
            &mut verified_revisions,
        )?;
        let corrected =
            self.load_revision_with_cache(correction.id(), &mut verified_revisions)?;
        if corrected.revision.parent.as_ref() != Some(prior.revision_id()) {
            return Err(
                "restatement requires a direct correction of the prior close's revision".into(),
            );
        }
        let prior_revision = verified_revisions
            .get(prior.revision_id())
            .ok_or_else(|| "verified prior close is missing its revision".to_owned())?;
        if corrected.revision.ledger != prior_revision.ledger {
            return Err("restatement must remain within the same ledger".into());
        }
        if corrected.revision.model != prior_revision.model {
            return Err("restatement cannot change the pinned model".into());
        }
        let period = prior.period.clone();
        self.create_close(
            &corrected,
            prior.book(),
            period.as_ref(),
            allow_partial,
            Some(&prior),
        )
    }

    /// Reopen a close and recompute its report roots from pinned source and model.
    pub fn close_by_id(&self, id: &Id) -> Result<Close, String> {
        self.load_close(id, &mut BTreeMap::new(), &mut BTreeMap::new())
    }

    /// Validate every stored envelope, source, package closure, revision chain,
    /// certificate replay, and close projection in this project.
    pub fn verify_all(&self) -> Result<(), String> {
        self.validated_objects().map(|_| ())
    }

    /// List verified revisions in content-address order. Project IDs are
    /// explicit; the repository does not maintain a mutable "current" head.
    pub fn revisions(&self) -> Result<Vec<Revision>, String> {
        self.validated_objects().map(|(revisions, _)| revisions)
    }

    /// List verified closes in content-address order.
    pub fn closes(&self) -> Result<Vec<Close>, String> {
        self.validated_objects().map(|(_, closes)| closes)
    }

    fn validated_objects(&self) -> Result<(Vec<Revision>, Vec<Close>), String> {
        let objects = self.repository.verify_envelopes()?;
        let mut revision_ids = Vec::new();
        let mut close_ids = Vec::new();
        for object in objects {
            match object.kind.as_str() {
                REVISION_KIND => {
                    revision_ids.push(object.id);
                }
                CLOSE_KIND => {
                    close_ids.push(object.id);
                }
                other => return Err(format!("unknown repository object kind {other}")),
            }
        }

        let mut verified_revisions = BTreeMap::new();
        let mut verified_closes = BTreeMap::new();
        let mut closes = Vec::with_capacity(close_ids.len());
        for id in close_ids {
            closes.push(self.load_close(
                &id,
                &mut verified_closes,
                &mut verified_revisions,
            )?);
        }
        let mut revisions = Vec::with_capacity(revision_ids.len());
        for id in revision_ids {
            if let Some(verified) = verified_revisions.get(&id) {
                revisions.push(verified.as_revision(id));
            } else {
                revisions.push(
                    self.load_revision_with_cache(&id, &mut verified_revisions)?
                        .revision,
                );
            }
        }
        Ok((revisions, closes))
    }

    fn load_revision(&self, id: &Id) -> Result<LoadedRevision, String> {
        self.load_revision_with_cache(id, &mut BTreeMap::new())
    }

    fn load_revision_with_cache(
        &self,
        id: &Id,
        verified: &mut BTreeMap<Id, VerifiedRevision>,
    ) -> Result<LoadedRevision, String> {
        let target = self.read_revision(id)?;
        let mut current_id = id.clone();
        let mut current = VerifiedRevision::from(&target.revision);
        let mut seen = BTreeSet::new();
        let mut chain = vec![(current_id.clone(), current.clone())];
        let mut known_depth = 0;

        loop {
            if !seen.insert(current_id.clone()) {
                return Err("revision history contains a cycle".into());
            }
            if seen.len() > MAX_HISTORY_DEPTH {
                return Err(format!(
                    "revision history exceeds {MAX_HISTORY_DEPTH} links"
                ));
            }
            let Some(parent_id) = current.parent.clone() else {
                break;
            };
            if seen.contains(&parent_id) {
                return Err("revision history contains a cycle".into());
            }
            if let Some(parent) = verified.get(&parent_id) {
                if parent.ledger != current.ledger {
                    return Err("revision parent belongs to a different ledger".into());
                }
                known_depth = parent.depth;
                break;
            }
            let parent = self.read_revision(&parent_id)?;
            if parent.revision.ledger != current.ledger {
                return Err("revision parent belongs to a different ledger".into());
            }
            current_id = parent_id;
            current = VerifiedRevision::from(&parent.revision);
            chain.push((current_id.clone(), current.clone()));
        }
        for (revision_id, mut revision) in chain.into_iter().rev() {
            known_depth += 1;
            if known_depth > MAX_HISTORY_DEPTH {
                return Err(format!(
                    "revision history exceeds {MAX_HISTORY_DEPTH} links"
                ));
            }
            revision.depth = known_depth;
            verified.insert(revision_id, revision);
        }
        Ok(target)
    }

    fn read_revision(&self, id: &Id) -> Result<LoadedRevision, String> {
        let record: RevisionRecord = self.repository.get(id, REVISION_KIND)?;
        validate_source_size(&record.source)?;
        let model = compile_model_sources(&record.packages, &record.certificate.model)?;

        let (document, world, replayed_certificate) =
            check(&record.source, &model, CHECK_BUDGET)
                .map_err(|errors| source_diagnostics(&record.source, errors))?;
        if canonical(&replayed_certificate) != canonical(&record.certificate) {
            return Err("stored revision certificate differs from a fresh replay".into());
        }
        validate_ledger_name(&document.name)?;
        if world.id() != &record.certificate.world {
            return Err("revision world root does not match its source replay".into());
        }
        let revision = Revision {
            id: id.clone(),
            ledger: document.name.clone(),
            model: record.certificate.model.clone(),
            world: record.certificate.world.clone(),
            parent: record.parent.clone(),
        };
        Ok(LoadedRevision {
            revision,
            record,
            model,
            world,
        })
    }

    fn compute_view(
        &self,
        loaded: &LoadedRevision,
        book: &str,
        period: Option<&Period>,
    ) -> Result<View, String> {
        view_period(
            &loaded.world,
            &loaded.model,
            book,
            period.map(|p| (p.from(), p.to())),
            CHECK_BUDGET,
        )
        .map_err(diagnostics)
    }

    fn create_close(
        &self,
        loaded: &LoadedRevision,
        book: &str,
        period: Option<&Period>,
        allow_partial: bool,
        prior: Option<&Close>,
    ) -> Result<Close, String> {
        let project_view = self.compute_view(loaded, book, period)?;
        let complete = is_complete(&project_view.evaluation);
        if !complete && !allow_partial {
            return Err(
                "close has unresolved findings; pass allow_partial explicitly to snapshot them"
                    .into(),
            );
        }
        let record = CloseRecord {
            revision: loaded.revision.id.clone(),
            book: project_view.book.clone(),
            period: period.map(StoredPeriod::from_period),
            view_root: project_view.id.clone(),
            prior: prior.map(|close| close.id().clone()),
        };
        let restatement_depth = prior.map_or(1, |close| close.restatement_depth + 1);
        if restatement_depth > MAX_HISTORY_DEPTH {
            return Err(format!(
                "close restatement chain exceeds {MAX_HISTORY_DEPTH} links"
            ));
        }
        let id = self.repository.put(CLOSE_KIND, &record)?;
        Ok(close_handle(
            id,
            &record,
            project_view.world.clone(),
            complete,
            restatement_depth,
        ))
    }

    fn load_close(
        &self,
        id: &Id,
        verified: &mut BTreeMap<Id, Close>,
        verified_revisions: &mut BTreeMap<Id, VerifiedRevision>,
    ) -> Result<Close, String> {
        if let Some(close) = verified.get(id) {
            return Ok(close.clone());
        }
        let mut chain = Vec::<(Id, CloseRecord)>::new();
        let mut seen = BTreeSet::new();
        let mut current_id = id.clone();
        loop {
            if verified.contains_key(&current_id) {
                break;
            }
            if !seen.insert(current_id.clone()) {
                return Err("close restatement chain contains a cycle".into());
            }
            if seen.len() > MAX_HISTORY_DEPTH {
                return Err(format!(
                    "close restatement chain exceeds {MAX_HISTORY_DEPTH} links"
                ));
            }
            let record: CloseRecord = self.repository.get(&current_id, CLOSE_KIND)?;
            validate_ledger_name(&record.book)?;
            record
                .period
                .as_ref()
                .map(StoredPeriod::to_period)
                .transpose()?;
            let prior = record.prior.clone();
            chain.push((current_id, record));
            match prior {
                Some(prior_id) if verified.contains_key(&prior_id) => break,
                Some(prior_id) => current_id = prior_id,
                None => break,
            }
        }

        for (close_id, record) in chain.into_iter().rev() {
            let period = record
                .period
                .as_ref()
                .map(StoredPeriod::to_period)
                .transpose()?;
            let revision = self.load_revision_with_cache(&record.revision, verified_revisions)?;
            let project_view = self.compute_view(&revision, &record.book, period.as_ref())?;
            if project_view.world != revision.revision.world || project_view.id != record.view_root {
                return Err("close report or world root does not match a fresh replay".into());
            }
            let complete = is_complete(&project_view.evaluation);

            let restatement_depth = if let Some(prior_id) = &record.prior {
                let prior = verified.get(prior_id).ok_or_else(|| {
                    "close restatement parent was not verified before its correction".to_owned()
                })?;
                let prior_revision = verified_revisions
                    .get(prior.revision_id())
                    .ok_or_else(|| "verified prior close is missing its revision".to_owned())?;
                if revision.revision.parent.as_ref() != Some(prior.revision_id()) {
                    return Err(
                        "restated close is not a direct correction of its prior revision".into(),
                    );
                }
                if revision.revision.ledger != prior_revision.ledger
                    || revision.revision.model != prior_revision.model
                    || record.book != prior.book
                    || period != prior.period
                {
                    return Err("restated close changed its ledger, model, book, or period".into());
                }
                prior.restatement_depth + 1
            } else {
                1
            };
            if restatement_depth > MAX_HISTORY_DEPTH {
                return Err(format!(
                    "close restatement chain exceeds {MAX_HISTORY_DEPTH} links"
                ));
            }
            let close = close_handle(
                close_id.clone(),
                &record,
                project_view.world.clone(),
                complete,
                restatement_depth,
            );
            verified.insert(close_id, close);
        }
        verified
            .get(id)
            .cloned()
            .ok_or_else(|| "close history did not include the requested close".to_owned())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRecord {
    source: String,
    packages: BTreeMap<String, String>,
    parent: Option<Id>,
    certificate: Certificate,
}

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct StoredPeriod {
    from: Date,
    to: Date,
}

impl StoredPeriod {
    fn from_period(period: &Period) -> Self {
        Self {
            from: period.from.clone(),
            to: period.to.clone(),
        }
    }

    fn to_period(&self) -> Result<Period, String> {
        Period::new(self.from.clone(), self.to.clone())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseRecord {
    revision: Id,
    book: String,
    period: Option<StoredPeriod>,
    view_root: Id,
    prior: Option<Id>,
}

struct LoadedRevision {
    revision: Revision,
    record: RevisionRecord,
    model: Model,
    world: crate::World,
}

fn close_handle(id: Id, record: &CloseRecord, world: Id, complete: bool) -> Close {
    Close {
        id,
        revision: record.revision.clone(),
        world,
        book: record.book.clone(),
        period: record
            .period
            .as_ref()
            .map(|period| period.to_period().expect("validated close period")),
        complete,
        prior: record.prior.clone(),
        view_root: record.view_root.clone(),
    }
}

fn package_sources_from_model(model: &Model) -> Result<BTreeMap<String, String>, String> {
    let packages = model.packages().clone();
    let compiled = compile_model_sources(&packages, &model.id())?;
    if canonical(&compiled) != canonical(model) {
        return Err("model artifact differs from compiling its exact package sources".into());
    }
    Ok(packages)
}

fn compile_model_sources(
    packages: &BTreeMap<String, String>,
    model_root: &Id,
) -> Result<Model, String> {
    if packages.len() > MAX_PACKAGES {
        return Err(format!("model has more than {MAX_PACKAGES} packages"));
    }
    let total: usize = packages.values().map(String::len).sum();
    if total > MAX_MODEL_BYTES {
        return Err(format!(
            "model package sources exceed {MAX_MODEL_BYTES} bytes"
        ));
    }
    let sources: Vec<String> = packages.values().cloned().collect();
    let model = Model::compile(&sources).map_err(diagnostics)?;
    if model.packages() != packages || model.id() != *model_root {
        return Err("stored model does not recompile to its pinned package closure".into());
    }
    Ok(model)
}

fn validate_source_size(source: &str) -> Result<(), String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!("source exceeds {MAX_SOURCE_BYTES} bytes"));
    }
    Ok(())
}

fn validate_ledger_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err("invalid ledger or book name in project object".into());
    }
    Ok(())
}

fn diagnostics(errors: Vec<Diagnostic>) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn is_complete(evaluation: &Evaluation) -> bool {
    evaluation
        .findings
        .iter()
        .all(|finding| matches!(&finding.outcome, Outcome::Proven(_)))
}
