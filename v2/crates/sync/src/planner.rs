//! The model-native, no-write planning boundary used by `axiom sync`.

use std::path::Path;
use std::time::Duration;

use axiom_core::{Day, Diagnostic, FileId, Map};
use axiom_engine::Run as EngineRun;
use axiom_model::sync::{Fetch, Sink as ModelSink, Source as ModelSource};
use axiom_model::{Book, Role};

use crate::binding;
use crate::command::{Failed, run_all, substitute};
use crate::paths::matching_paths;
use crate::sink::{self, Sink};
use crate::world::{Feed, World};
use crate::write::{Change, changes};
use crate::{Insert, Unit};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// A captured command's stdout, registered after project source files so any
/// reader diagnostics can be attached to the exact text that was parsed.
#[derive(Debug)]
pub struct GeneratedSource {
    pub file: FileId,
    pub path: String,
    pub text: String,
}

/// One selected source file or command result.
#[derive(Debug)]
pub struct SourceResult {
    pub source: String,
    pub path: String,
    pub added: usize,
    pub failure: Option<SourceFailure>,
}

#[derive(Debug)]
pub enum SourceFailure {
    Read(Diagnostic),
    Command(Failed),
    Output(Vec<Diagnostic>),
}

/// Everything the CLI needs to render a dry-run or apply the changes later.
/// This function never writes a project file.
pub struct PlanOutcome {
    pub sources: Vec<SourceResult>,
    pub changes: Vec<Change>,
    /// Errors while validating the combined target files.
    pub problems: Vec<Diagnostic>,
    /// The current engine fold still has a declared-but-unpopulated promise
    /// and open-claim result seam. These diagnostics make that limitation
    /// visible until the native monitor fills it.
    pub incomplete: Vec<Diagnostic>,
    pub generated: Vec<GeneratedSource>,
}

enum InputText {
    Local { file: FileId, text: String },
    Generated(usize),
    Missing,
    Failed(SourceFailure),
}

struct WorkItem {
    path: String,
    input: InputText,
}

struct WorkSource {
    index: usize,
    items: Vec<WorkItem>,
}

/// Select, read/run and plan declared sources. The `read` callback gives each
/// imported file its diagnostic `FileId`; it may append the text to a CLI's
/// auxiliary source catalog. Reads are cached by path, so the callback is
/// called at most once per path. Files are only planned: callers own diff and
/// write behavior.
pub fn plan<'b, 's>(
    book: &'b Book<'s>,
    run: &'b EngineRun,
    root: &Path,
    today: Day,
    wanted: &[&str],
    project_paths: &[&str],
    file_paths: &[&str],
    read: &mut dyn FnMut(&str) -> Option<(FileId, String)>,
) -> Result<PlanOutcome, Diagnostic> {
    let selected = selected_sources(book, wanted)?;
    let mut world = binding::world(book, run, project_paths)?;
    let mut base_files: Map<String, Option<(FileId, String)>> = Map::default();
    let mut work: Vec<WorkSource> = selected
        .iter()
        .map(|&index| WorkSource {
            index,
            items: Vec::new(),
        })
        .collect();

    let mut commands = Vec::new();
    let mut command_for_work = Vec::new();
    let units = command_units(book, run);
    let first = book
        .flows
        .iter()
        .map(|(_, flow)| flow.day)
        .min()
        .unwrap_or(today);

    for current in &mut work {
        let source = &book.sources[current.index];
        match source.fetch {
            Fetch::Read(pattern) => {
                let pattern = book.text(pattern);
                match matching_paths(root, pattern) {
                    Err(problem) => current.items.push(WorkItem {
                        path: pattern.to_string(),
                        input: InputText::Failed(SourceFailure::Read(problem)),
                    }),
                    Ok(paths) if paths.is_empty() => current.items.push(WorkItem {
                        path: pattern.to_string(),
                        input: InputText::Missing,
                    }),
                    Ok(paths) => {
                        for path in paths {
                            match cached_read(&path, &mut base_files, read) {
                                Some((file, text)) => current.items.push(WorkItem {
                                    path,
                                    input: InputText::Local { file, text },
                                }),
                                None => current.items.push(WorkItem {
                                    path: path.clone(),
                                    input: InputText::Failed(SourceFailure::Read(
                                        Diagnostic::error(
                                            "sync-read-file",
                                            format!("could not read `{path}`"),
                                        ),
                                    )),
                                }),
                            }
                        }
                    }
                }
            }
            Fetch::Run(command) => {
                let since = source_since(book, &world, source, first);
                commands.push(substitute(book.text(command), since, today, &units));
                command_for_work.push(work.len() - 1);
            }
        }
    }

    let command_results = run_all(&commands, root, COMMAND_TIMEOUT);
    let mut generated = Vec::new();
    for (work_index, result) in command_for_work.into_iter().zip(command_results) {
        let source = &book.sources[work[work_index].index];
        let name = book.name(source.name);
        let path = format!("<sync {name} output>");
        let input = match result {
            Ok(text) => {
                let index = generated.len();
                let placeholder = u16::MAX
                    .checked_sub(u16::try_from(index).map_err(|_| {
                        Diagnostic::error("sync-file-limit", "too many command outputs")
                    })?)
                    .ok_or_else(|| {
                        Diagnostic::error("sync-file-limit", "too many command outputs")
                    })?;
                let file = FileId(placeholder);
                generated.push(GeneratedSource {
                    file,
                    path: path.clone(),
                    text,
                });
                InputText::Generated(index)
            }
            Err(failed) => InputText::Failed(SourceFailure::Command(failed)),
        };
        work[work_index].items.push(WorkItem { path, input });
    }

    // Documents and Axiom output are planned before statement feeds, allowing
    // their flows to participate in the bank's same-run reconciliation.
    let mut order: Vec<usize> = (0..work.len()).collect();
    order.sort_by_key(|&at| matches!(&book.sources[work[at].index].sink, ModelSink::Feed { .. }));
    let mut results: Vec<Vec<SourceResult>> = (0..book.sources.len()).map(|_| Vec::new()).collect();
    let mut all_inserts = Vec::new();
    let mut overlay: Map<String, String> = Map::default();

    for work_index in order {
        let source_index = work[work_index].index;
        let source = &book.sources[source_index];
        let source_name = book.name(source.name).to_string();
        if work[work_index].items.is_empty() {
            results[source_index].push(SourceResult {
                source: source_name.clone(),
                path: source_name,
                added: 0,
                failure: None,
            });
            continue;
        }

        for item in &work[work_index].items {
            let (text, file) = match &item.input {
                InputText::Local { file, text } => (text.as_str(), *file),
                InputText::Generated(index) => {
                    let generated = &generated[*index];
                    (generated.text.as_str(), generated.file)
                }
                InputText::Missing => {
                    results[source_index].push(SourceResult {
                        source: source_name.clone(),
                        path: item.path.clone(),
                        added: 0,
                        failure: None,
                    });
                    continue;
                }
                InputText::Failed(failure) => {
                    results[source_index].push(SourceResult {
                        source: source_name.clone(),
                        path: item.path.clone(),
                        added: 0,
                        failure: Some(failure_for_result(failure)),
                    });
                    continue;
                }
            };

            let is_feed = matches!(&source.sink, ModelSink::Feed { .. });
            let old_accounts = is_feed.then(|| world.accounts.clone());
            let planned = source_inserts(
                book,
                &mut world,
                source,
                text,
                file,
                file_paths,
                &overlay,
                &mut base_files,
                read,
            );
            match planned {
                Err(problems) => results[source_index].push(SourceResult {
                    source: source_name.clone(),
                    path: item.path.clone(),
                    added: 0,
                    failure: Some(SourceFailure::Output(problems)),
                }),
                Ok(inserts) => {
                    let mut read_virtual = |path: &str| {
                        overlay.get(path).cloned().or_else(|| {
                            cached_read(path, &mut base_files, read).map(|(_, text)| text)
                        })
                    };
                    match changes(&inserts, &mut read_virtual) {
                        Err(problems) => {
                            if let Some(accounts) = old_accounts {
                                world.accounts = accounts;
                            }
                            results[source_index].push(SourceResult {
                                source: source_name.clone(),
                                path: item.path.clone(),
                                added: 0,
                                failure: Some(SourceFailure::Output(problems)),
                            });
                        }
                        Ok(changes) => {
                            for change in changes {
                                overlay.insert(change.path, change.after);
                            }
                            let added = inserts.len();
                            if !is_feed {
                                world.learn(&inserts);
                            }
                            all_inserts.extend(inserts);
                            results[source_index].push(SourceResult {
                                source: source_name.clone(),
                                path: item.path.clone(),
                                added,
                                failure: None,
                            });
                        }
                    }
                }
            }
        }
    }

    let mut read_base = |path: &str| cached_read(path, &mut base_files, read).map(|(_, text)| text);
    let (changes, problems) = match changes(&all_inserts, &mut read_base) {
        Ok(changes) => (changes, Vec::new()),
        Err(problems) => (Vec::new(), problems),
    };
    remap_generated_files(&mut generated, &mut results, file_paths.len(), &base_files)?;
    let incomplete = monitor_gaps(book, run);

    Ok(PlanOutcome {
        sources: results.into_iter().flatten().collect(),
        changes,
        problems,
        incomplete,
        generated,
    })
}

fn selected_sources(book: &Book<'_>, wanted: &[&str]) -> Result<Vec<usize>, Diagnostic> {
    if wanted.is_empty() {
        return Ok((0..book.sources.len()).collect());
    }
    let mut selected = Vec::with_capacity(wanted.len());
    for &name in wanted {
        let Some(index) = book
            .sources
            .iter()
            .position(|source| book.name(source.name).eq_ignore_ascii_case(name))
        else {
            return Err(Diagnostic::error(
                "sync-no-such-source",
                format!("the book has no sync source named `{name}`"),
            ));
        };
        if !selected.contains(&index) {
            selected.push(index);
        }
    }
    selected.sort_unstable();
    Ok(selected)
}

fn cached_read(
    path: &str,
    cache: &mut Map<String, Option<(FileId, String)>>,
    read: &mut dyn FnMut(&str) -> Option<(FileId, String)>,
) -> Option<(FileId, String)> {
    if let Some(found) = cache.get(path) {
        return found.clone();
    }
    let found = read(path);
    cache.insert(path.to_string(), found.clone());
    found
}

fn command_units<'s>(book: &Book<'s>, run: &EngineRun) -> Vec<&'s str> {
    let mut units: Vec<_> = run
        .holdings
        .iter()
        .map(|holding| book.name(book.commodities[holding.unit].symbol))
        .collect();
    units.sort_unstable();
    units.dedup();
    units
}

fn source_since(book: &Book<'_>, world: &World<'_, '_>, source: &ModelSource, first: Day) -> Day {
    let ModelSink::Feed { account } = &source.sink else {
        return first;
    };
    let name = book.name(book.places[*account].path);
    world
        .accounts
        .get(name)
        .map_or(first, |account| account.since(first))
}

fn source_inserts<'b, 's>(
    book: &'b Book<'s>,
    world: &mut World<'b, 's>,
    source: &ModelSource,
    text: &str,
    file: FileId,
    file_paths: &[&str],
    overlay: &Map<String, String>,
    cache: &mut Map<String, Option<(FileId, String)>>,
    read: &mut dyn FnMut(&str) -> Option<(FileId, String)>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    match &source.sink {
        ModelSink::Feed { account } => {
            let Some(format_id) = source.format else {
                return Err(vec![Diagnostic::error(
                    "sync-no-format",
                    format!("`{}` has no format for its feed", book.name(source.name)),
                )]);
            };
            let Some(format) = book.formats.get(format_id) else {
                return Err(vec![Diagnostic::error(
                    "sync-no-format",
                    format!(
                        "`{}` refers to a format that is not in the book",
                        book.name(source.name)
                    ),
                )]);
            };
            let place = &book.places[*account];
            if !matches!(place.role, Role::Account { .. }) {
                return Err(vec![Diagnostic::error(
                    "sync-feed-account",
                    format!("`{}` does not target an account", book.name(source.name)),
                )]);
            }
            let owner = &book.entities[place.owner];
            let commodity = &book.commodities[owner.currency];
            let feed = Feed {
                account: book.name(place.path),
                unit: Unit {
                    name: book.name(commodity.symbol),
                    scale: commodity.scale,
                },
                format,
            };
            world.feed_at(&feed, text, file)
        }
        ModelSink::Journal => merge(Sink::Journal, text, world, overlay, cache, read),
        ModelSink::File(path) => {
            let path = book.text(*path);
            merge(Sink::File(path), text, world, overlay, cache, read)
        }
        ModelSink::Param(param) => {
            let value = &book.params[*param];
            let Some(path) = file_paths.get(value.loc.file.0 as usize).copied() else {
                return Err(vec![Diagnostic::error(
                    "sync-param-file",
                    format!(
                        "the declaration for `param {}` has no project source path",
                        book.name(value.name)
                    ),
                )]);
            };
            if !crate::paths::is_project_path(path) {
                return Err(vec![Diagnostic::error(
                    "sync-param-file",
                    format!(
                        "`param {}` is declared in a non-project file",
                        book.name(value.name)
                    ),
                )]);
            }
            merge(
                Sink::Param {
                    name: book.name(value.name),
                    path,
                },
                text,
                world,
                overlay,
                cache,
                read,
            )
        }
    }
}

fn merge<'b, 's>(
    sink: Sink<'_>,
    text: &str,
    world: &mut World<'b, 's>,
    overlay: &Map<String, String>,
    cache: &mut Map<String, Option<(FileId, String)>>,
    read: &mut dyn FnMut(&str) -> Option<(FileId, String)>,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    let mut read_virtual = |path: &str| {
        overlay
            .get(path)
            .cloned()
            .or_else(|| cached_read(path, cache, read).map(|(_, text)| text))
    };
    sink::merge(sink, text, &world.layout, &mut read_virtual)
}

fn remap_generated_files(
    generated: &mut [GeneratedSource],
    results: &mut [Vec<SourceResult>],
    file_count: usize,
    base_files: &Map<String, Option<(FileId, String)>>,
) -> Result<(), Diagnostic> {
    let mut next = file_count;
    for file in base_files.values().flatten().map(|(file, _)| file.index()) {
        next = next.max(file + 1);
    }
    for generated in generated {
        let old = generated.file;
        let new = FileId(u16::try_from(next).map_err(|_| {
            Diagnostic::error("sync-file-limit", "too many source files for diagnostics")
        })?);
        next += 1;
        generated.file = new;
        for result in results
            .iter_mut()
            .flatten()
            .filter(|result| result.path == generated.path)
        {
            if let Some(SourceFailure::Output(problems)) = result.failure.as_mut() {
                for problem in problems {
                    remap_diagnostic(problem, old, new);
                }
            }
        }
    }
    Ok(())
}

fn remap_diagnostic(problem: &mut Diagnostic, old: FileId, new: FileId) {
    for label in &mut problem.labels {
        if label.loc.file == old {
            label.loc.file = new;
        }
    }
    for help in &mut problem.help {
        if let Some((loc, _)) = &mut help.edit
            && loc.file == old
        {
            loc.file = new;
        }
    }
}

fn failure_for_result(failure: &SourceFailure) -> SourceFailure {
    match failure {
        SourceFailure::Read(problem) => SourceFailure::Read(problem.clone()),
        SourceFailure::Command(failed) => SourceFailure::Command(failed.clone()),
        SourceFailure::Output(problems) => SourceFailure::Output(problems.clone()),
    }
}

fn monitor_gaps(book: &Book<'_>, run: &EngineRun) -> Vec<Diagnostic> {
    let has_contracts = !book.contracts.is_empty();
    let has_claim_places = book.places.iter().any(|(_, place)| place.claim);
    if (has_contracts && run.promises.is_empty())
        || (has_claim_places && run.open_claims.is_empty())
    {
        vec![Diagnostic::warning(
            "sync-monitor-incomplete",
            "this outlook does not yet use the engine's contract occurrence and open-claim results; matching may omit contract and claim records",
        )]
    } else {
        Vec::new()
    }
}
