//! The model-native, no-write planning boundary used by `axiom sync`.

use std::borrow::Cow;
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
use crate::world::{Feed, FeedDelta, World};
use crate::write::{Change, changes_borrowed, preview};
use crate::{Insert, Unit};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// A captured command's stdout, registered in the same source catalog used by
/// diagnostic rendering.
#[derive(Debug)]
pub struct GeneratedSource {
    pub file: FileId,
    pub path: String,
}

/// The append-only source catalog used during planning. Imported and
/// generated text is stored once, then borrowed while its source is parsed.
pub trait SourceRegistry {
    /// Read and register a path once. `None` means the path does not exist.
    fn read(&mut self, path: &str) -> Result<Option<FileId>, Diagnostic>;

    /// Borrow text previously registered by [`read`](Self::read) or
    /// [`generated`](Self::generated).
    fn text(&self, file: FileId) -> Option<&str>;

    /// Register command output and return its real diagnostic `FileId`.
    fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic>;
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
    Generated(Diagnostic),
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
    Local { file: FileId },
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

/// Select, read/run and plan declared sources. Reads are cached by path. Inputs
/// and every potential sink target are registered before any source text is
/// borrowed. Files are only planned: callers own diff and write behavior.
pub fn plan<'b, 's>(
    book: &'b Book<'s>,
    run: &'b EngineRun,
    root: &Path,
    today: Day,
    wanted: &[&str],
    project_paths: &[&str],
    file_paths: &[&str],
    registry: &mut dyn SourceRegistry,
) -> Result<PlanOutcome, Diagnostic> {
    let selected = selected_sources(book, wanted)?;
    let mut world = binding::world(book, run, project_paths)?;
    let mut base_files: Map<String, Result<Option<FileId>, Diagnostic>> = Map::default();
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

    for (work_index, current) in work.iter_mut().enumerate() {
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
                            match cached_read(&path, &mut base_files, registry) {
                                Ok(Some(file)) => current.items.push(WorkItem {
                                    path,
                                    input: InputText::Local { file },
                                }),
                                Ok(None) => current.items.push(WorkItem {
                                    path: path.clone(),
                                    input: InputText::Failed(SourceFailure::Read(
                                        Diagnostic::error(
                                            "sync-read-file",
                                            format!("could not read `{path}`"),
                                        ),
                                    )),
                                }),
                                Err(problem) => current.items.push(WorkItem {
                                    path,
                                    input: InputText::Failed(SourceFailure::Read(problem)),
                                }),
                            }
                        }
                    }
                }
            }
            Fetch::Run(command) => {
                let since = source_since(book, &world, source, first);
                queue_command(
                    &mut commands,
                    &mut command_for_work,
                    work_index,
                    substitute(book.text(command), since, today, &units),
                );
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
            Ok(text) => match registry.generated(&path, text) {
                Ok(file) => {
                    generated.push(GeneratedSource {
                        file,
                        path: path.clone(),
                    });
                    InputText::Local { file }
                }
                Err(problem) => InputText::Failed(SourceFailure::Generated(problem)),
            },
            Err(failed) => InputText::Failed(SourceFailure::Command(failed)),
        };
        work[work_index].items.push(WorkItem { path, input });
    }

    // Load every target the merger may inspect before borrowing any input
    // text. This keeps the source catalog append-only during the plan phase.
    for current in &work {
        let source = &book.sources[current.index];
        for item in &current.items {
            let InputText::Local { file } = &item.input else {
                continue;
            };
            let paths = match registry.text(*file) {
                Some(output) => sink_target_paths(book, source, output, file_paths, &world.layout),
                None => Vec::new(),
            };
            for path in paths {
                let _ = cached_read(&path, &mut base_files, registry);
            }
        }
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
                InputText::Local { file } => match registry.text(*file) {
                    Some(text) => (text, *file),
                    None => {
                        results[source_index].push(SourceResult {
                            source: source_name.clone(),
                            path: item.path.clone(),
                            added: 0,
                            failure: Some(SourceFailure::Read(Diagnostic::error(
                                "sync-source-text",
                                "registered source text is unavailable",
                            ))),
                        });
                        continue;
                    }
                },
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

            if let Some(problem) = sink_target_paths(book, source, text, file_paths, &world.layout)
                .into_iter()
                .find_map(|path| {
                    if overlay.contains_key(&path) {
                        return None;
                    }
                    match base_files.get(&path) {
                        Some(Err(problem)) => Some(problem.clone()),
                        _ => None,
                    }
                })
            {
                results[source_index].push(SourceResult {
                    source: source_name.clone(),
                    path: item.path.clone(),
                    added: 0,
                    failure: Some(SourceFailure::Read(problem)),
                });
                continue;
            }

            let is_feed = matches!(&source.sink, ModelSink::Feed { .. });
            let planned = source_inserts(
                book,
                &mut world,
                source,
                text,
                file,
                file_paths,
                &overlay,
                &base_files,
                registry,
            );
            match planned {
                Err(problems) => results[source_index].push(SourceResult {
                    source: source_name.clone(),
                    path: item.path.clone(),
                    added: 0,
                    failure: Some(SourceFailure::Output(problems)),
                }),
                Ok((inserts, feed_delta)) => {
                    if is_feed {
                        let mut targets: Map<&str, ()> = Map::default();
                        for insert in &inserts {
                            if !overlay.contains_key(&insert.path)
                                && targets.insert(insert.path.as_str(), ()).is_none()
                            {
                                let _ = cached_read(&insert.path, &mut base_files, registry);
                            }
                        }
                    }
                    if let Some(problem) = inserts
                        .iter()
                        .filter(|insert| !overlay.contains_key(&insert.path))
                        .find_map(|insert| match base_files.get(&insert.path) {
                            Some(Err(problem)) => Some(problem.clone()),
                            _ => None,
                        })
                    {
                        results[source_index].push(SourceResult {
                            source: source_name.clone(),
                            path: item.path.clone(),
                            added: 0,
                            failure: Some(SourceFailure::Read(problem)),
                        });
                        continue;
                    }
                    let mut read_virtual = |path: &str| {
                        overlay
                            .get(path)
                            .map(|text| Cow::Borrowed(text.as_str()))
                            .or_else(|| cached_text(path, &base_files, registry).map(Cow::Borrowed))
                    };
                    match preview(&inserts, &mut read_virtual) {
                        Err(problems) => {
                            results[source_index].push(SourceResult {
                                source: source_name.clone(),
                                path: item.path.clone(),
                                added: 0,
                                failure: Some(SourceFailure::Output(problems)),
                            });
                        }
                        Ok(changes) => {
                            if let Some(delta) = feed_delta {
                                world.commit_feed(delta);
                            }
                            for update in changes {
                                overlay.insert(update.path, update.after);
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

    let mut read_base = |path: &str| cached_text(path, &base_files, registry).map(Cow::Borrowed);
    let (changes, problems) = match changes_borrowed(&all_inserts, &mut read_base) {
        Ok(changes) => (changes, Vec::new()),
        Err(problems) => (Vec::new(), problems),
    };
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

fn queue_command(
    commands: &mut Vec<String>,
    command_for_work: &mut Vec<usize>,
    work_index: usize,
    command: String,
) {
    commands.push(command);
    command_for_work.push(work_index);
}

fn cached_read(
    path: &str,
    cache: &mut Map<String, Result<Option<FileId>, Diagnostic>>,
    registry: &mut dyn SourceRegistry,
) -> Result<Option<FileId>, Diagnostic> {
    if let Some(found) = cache.get(path) {
        return found.clone();
    }
    let found = registry.read(path);
    cache.insert(path.to_string(), found.clone());
    found
}

fn cached_text<'a>(
    path: &str,
    cache: &Map<String, Result<Option<FileId>, Diagnostic>>,
    registry: &'a dyn SourceRegistry,
) -> Option<&'a str> {
    let file = cache.get(path)?.as_ref().ok().copied().flatten()?;
    registry.text(file)
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

fn sink_target_paths(
    book: &Book<'_>,
    source: &ModelSource,
    output: &str,
    file_paths: &[&str],
    layout: &crate::Layout,
) -> Vec<String> {
    match &source.sink {
        ModelSink::Feed { .. } => Vec::new(),
        ModelSink::Journal => sink::target_paths(Sink::Journal, output, layout),
        ModelSink::File(path) => sink::target_paths(Sink::File(book.text(*path)), output, layout),
        ModelSink::Param(param) => {
            let value = &book.params[*param];
            let Some(path) = file_paths.get(value.loc.file.0 as usize).copied() else {
                return Vec::new();
            };
            if crate::paths::is_project_path(path) {
                sink::target_paths(
                    Sink::Param {
                        name: book.name(value.name),
                        path,
                    },
                    output,
                    layout,
                )
            } else {
                Vec::new()
            }
        }
    }
}

fn source_inserts<'b, 's>(
    book: &'b Book<'s>,
    world: &mut World<'b, 's>,
    source: &ModelSource,
    text: &str,
    file: FileId,
    file_paths: &[&str],
    overlay: &Map<String, String>,
    cache: &Map<String, Result<Option<FileId>, Diagnostic>>,
    registry: &dyn SourceRegistry,
) -> Result<(Vec<Insert>, Option<FeedDelta<'s>>), Vec<Diagnostic>> {
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
            world
                .plan_feed_at(&feed, text, file)
                .map(|(inserts, delta)| (inserts, Some(delta)))
        }
        ModelSink::Journal => merge(Sink::Journal, text, file, world, overlay, cache, registry)
            .map(|inserts| (inserts, None)),
        ModelSink::File(path) => {
            let path = book.text(*path);
            merge(
                Sink::File(path),
                text,
                file,
                world,
                overlay,
                cache,
                registry,
            )
            .map(|inserts| (inserts, None))
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
                file,
                world,
                overlay,
                cache,
                registry,
            )
            .map(|inserts| (inserts, None))
        }
    }
}

fn merge<'b, 's>(
    sink: Sink<'_>,
    text: &str,
    file: FileId,
    world: &mut World<'b, 's>,
    overlay: &Map<String, String>,
    cache: &Map<String, Result<Option<FileId>, Diagnostic>>,
    registry: &dyn SourceRegistry,
) -> Result<Vec<Insert>, Vec<Diagnostic>> {
    let mut read_virtual = |path: &str| {
        overlay
            .get(path)
            .map(|text| Cow::Borrowed(text.as_str()))
            .or_else(|| cached_text(path, cache, registry).map(Cow::Borrowed))
    };
    sink::merge_at(sink, text, &world.layout, file, &mut read_virtual)
}

fn failure_for_result(failure: &SourceFailure) -> SourceFailure {
    match failure {
        SourceFailure::Read(problem) => SourceFailure::Read(problem.clone()),
        SourceFailure::Command(failed) => SourceFailure::Command(failed.clone()),
        SourceFailure::Generated(problem) => SourceFailure::Generated(problem.clone()),
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axiom_core::{Day, Diagnostic, FileId, Folder};
    use axiom_engine::{Options, Plan};
    use axiom_model::Source;

    use super::{SourceRegistry, plan};

    #[derive(Default)]
    struct Registry {
        next: u32,
        files: Vec<(FileId, String, String)>,
    }

    impl SourceRegistry for Registry {
        fn read(&mut self, _path: &str) -> Result<Option<FileId>, Diagnostic> {
            Ok(None)
        }

        fn text(&self, file: FileId) -> Option<&str> {
            self.files
                .iter()
                .find(|(id, _, _)| *id == file)
                .map(|(_, _, text)| text.as_str())
        }

        fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
            let file = FileId(
                u16::try_from(self.next)
                    .map_err(|_| Diagnostic::error("sync-file-limit", "too many source files"))?,
            );
            self.next += 1;
            self.files.push((file, path.to_string(), text));
            Ok(file)
        }
    }

    #[test]
    fn each_run_source_gets_its_own_output_and_registered_diagnostic_file() {
        let std = include_str!("../../../systems/src/std.ax");
        let axiom = concat!(
            "base USD\n",
            "sync first\n",
            "  run printf '2026-01-02 checking -> food 10 USD\\n'\n",
            "  into first.ax\n",
            "sync second\n",
            "  run printf '2026-01-03 checking -> food 20 USD\\n'\n",
            "  into second.ax\n",
            "sync bad\n",
            "  run printf 'not a dated line\\n'\n",
            "  into bad.ax\n",
        );
        let (std_file, diagnostics) = axiom_syntax::parse(FileId(0), std, Folder::default());
        assert!(diagnostics.is_empty(), "std.ax: {diagnostics:?}");
        let (axiom_file, diagnostics) = axiom_syntax::parse(FileId(1), axiom, Folder::default());
        assert!(diagnostics.is_empty(), "axiom.ax: {diagnostics:?}");
        let sources = [
            Source {
                path: "std.ax",
                file: std_file,
                embedded: true,
            },
            Source {
                path: "axiom.ax",
                file: axiom_file,
                embedded: false,
            },
        ];
        let (book, diagnostics) = axiom_model::build(&sources);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let today = Day::parse(b"2026-01-10").unwrap();
        let run = Plan::new(&book).run(Options {
            today,
            relaxed: false,
        });
        let mut registry = Registry {
            next: u32::try_from(sources.len()).unwrap(),
            ..Registry::default()
        };
        let outcome = plan(
            &book,
            &run,
            Path::new("."),
            today,
            &["first", "second", "bad"],
            &["axiom.ax"],
            &["std.ax", "axiom.ax"],
            &mut registry,
        )
        .unwrap();

        assert_eq!(
            outcome.sources.len(),
            3,
            "{:?}",
            outcome
                .sources
                .iter()
                .map(|s| (&s.source, &s.path, s.added))
                .collect::<Vec<_>>()
        );
        assert_eq!(outcome.sources[0].source, "first");
        assert_eq!(outcome.sources[0].added, 1);
        assert_eq!(outcome.sources[1].source, "second");
        assert_eq!(outcome.sources[1].added, 1);
        assert_eq!(outcome.sources[2].source, "bad");
        assert!(matches!(
            outcome.sources[2].failure.as_ref(),
            Some(super::SourceFailure::Output(_))
        ));
        assert_eq!(outcome.generated.len(), 3);
        assert_ne!(outcome.generated[0].file, outcome.generated[1].file);
        assert_eq!(
            registry.text(outcome.generated[0].file),
            Some("2026-01-02 checking -> food 10 USD\n")
        );
        assert_eq!(
            registry.text(outcome.generated[1].file),
            Some("2026-01-03 checking -> food 20 USD\n")
        );
        assert_eq!(
            registry.text(outcome.generated[2].file),
            Some("not a dated line\n")
        );
        let Some(super::SourceFailure::Output(problems)) = outcome.sources[2].failure.as_ref()
        else {
            panic!("the malformed output should retain its parser diagnostic")
        };
        assert!(
            problems
                .iter()
                .flat_map(|problem| &problem.labels)
                .any(|label| label.loc.file == outcome.generated[2].file)
        );
        assert_eq!(
            outcome
                .changes
                .iter()
                .map(|change| change.path.as_str())
                .collect::<Vec<_>>(),
            ["first.ax", "second.ax"]
        );
    }
}
