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
use crate::paths::is_project_path;
use crate::paths::matching_paths;
use crate::sink::{self, Sink};
use crate::world::{Feed, FeedDelta, World};
use crate::write::{Change, Update, preview, validate_text_at};
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

#[derive(Clone, Debug)]
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

/// What a source reads from: a file the project has, a command's output, or why there is none.
enum InputText {
    Local { file: FileId },
    Missing,
    Failed(SourceFailure),
}

struct WorkItem {
    path: String,
    input: InputText,
}

/// One selected source, by its index in the book, and what it reads from.
struct WorkSource {
    index: usize,
    items: Vec<WorkItem>,
}

/// Select, read/run and plan declared sources. Reads are cached by path. Inputs
/// and every potential sink target are registered before any source text is
/// borrowed. Files are only planned: callers own diff and write behavior.
///
/// The eight inputs are independent: the book and its run, where and when to run
/// (`root`, `today`), which sources (`wanted`), the two path tables that
/// `FileId`s index (`project_paths` for the book's own files, `file_paths` for
/// all of them), and the registry that gives text its ids.
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
    let world = binding::world(book, run, project_paths)?;
    let mut planner = Planner { book, world, files: Files::new(registry, file_paths), inserts: Vec::new() };
    let mut work: Vec<WorkSource> = selected.iter().map(|&index| WorkSource { index, items: Vec::new() }).collect();

    planner.read_inputs(root, &mut work);
    planner.run_inputs(root, today, &command_units(book, run), &mut work);
    // Every target the merger may inspect is loaded before any input text is
    // borrowed, so the source catalog only grows while this happens.
    planner.load_targets(&work);
    let sources = planner.plan_sources(&work);
    let (changes, problems) = planner.settle();
    let generated = planner.files.generated;
    Ok(PlanOutcome { sources, changes, problems, incomplete: monitor_gaps(book, run), generated })
}

/// The project's files as planning sees them: each read once through the
/// registry, and as the sources planned so far would leave them.
struct Files<'a> {
    registry: &'a mut dyn SourceRegistry,
    /// Where each of the book's source files is, by `FileId`: where a `param` was declared.
    paths: &'a [&'a str],
    /// What reading each project path found, kept whether or not there was a file.
    read: Map<String, Result<Option<FileId>, Diagnostic>>,
    /// What each file would hold once the sources planned so far have written.
    planned: Map<String, String>,
    /// Command output and planned text, registered so that diagnostics can point into them.
    generated: Vec<GeneratedSource>,
}

impl<'a> Files<'a> {
    fn new(registry: &'a mut dyn SourceRegistry, paths: &'a [&'a str]) -> Files<'a> {
        Files { registry, paths, read: Map::default(), planned: Map::default(), generated: Vec::new() }
    }

    /// Reads `path`, once.
    fn read(&mut self, path: &str) -> &Result<Option<FileId>, Diagnostic> {
        if !self.read.contains_key(path) {
            self.read.insert(path.to_string(), self.registry.read(path));
        }
        &self.read[path]
    }

    /// The text registered for `file`.
    fn source(&self, file: FileId) -> Option<&str> {
        self.registry.text(file)
    }

    /// What `path` held when it was read.
    fn as_read(&self, path: &str) -> Option<&str> {
        let file = self.read.get(path)?.as_ref().ok().copied().flatten()?;
        self.registry.text(file)
    }

    /// What `path` holds now: what the sources planned so far leave in it, else what was read.
    fn current(&self, path: &str) -> Option<Cow<'_, str>> {
        match self.planned.get(path) {
            Some(text) => Some(Cow::Borrowed(text.as_str())),
            None => self.as_read(path).map(Cow::Borrowed),
        }
    }

    /// Why a file a source would write to could not be read, unless a source planned before it already made its text.
    fn unreadable(&self, path: &str) -> Option<Diagnostic> {
        match self.read.get(path) {
            Some(Err(problem)) if !self.planned.contains_key(path) => Some(problem.clone()),
            _ => None,
        }
    }

    /// Registers `text` under `path`, so that a diagnostic can point into it.
    fn generate(&mut self, path: String, text: String) -> Result<FileId, Diagnostic> {
        let file = self.registry.generated(&path, text)?;
        self.generated.push(GeneratedSource { file, path });
        Ok(file)
    }

    /// The updates if every planned file still parses; else what it says, located in a registered copy of the file.
    fn validated(&mut self, updates: Vec<Update>) -> Result<Vec<Update>, Vec<Diagnostic>> {
        let mut problems = Vec::new();
        for update in &updates {
            let Err(unlocated) = validate_text_at(&update.path, &update.after, FileId(0)) else {
                continue;
            };
            let file = self
                .generate(format!("<sync planned {}>", update.path), update.after.clone())
                .map_err(|problem| vec![problem])?;
            let Err(located) = validate_text_at(&update.path, &update.after, file) else {
                unreachable!("the same planned text was just validated");
            };
            debug_assert_eq!(unlocated.len(), located.len());
            problems.extend(located);
        }
        if problems.is_empty() { Ok(updates) } else { Err(problems) }
    }
}

/// One planning run: the book's world as the sources planned so far leave it,
/// the files they would write, and what they insert.
struct Planner<'a, 'b, 's> {
    book: &'b Book<'s>,
    world: World<'b, 's>,
    files: Files<'a>,
    /// Everything planned so far, to be written file by file at the end.
    inserts: Vec<Insert>,
}

impl<'a, 'b, 's> Planner<'a, 'b, 's> {
    /// The inputs of the sources that `read` files: each file the pattern matches, or why none.
    fn read_inputs(&mut self, root: &Path, work: &mut [WorkSource]) {
        for current in work {
            if let Fetch::Read(pattern) = self.book.sources[current.index].fetch {
                current.items = self.read_pattern(root, self.book.text(pattern));
            }
        }
    }

    fn read_pattern(&mut self, root: &Path, pattern: &str) -> Vec<WorkItem> {
        let item = |input| vec![WorkItem { path: pattern.to_string(), input }];
        match matching_paths(root, pattern) {
            Err(problem) => item(InputText::Failed(SourceFailure::Read(problem))),
            Ok(paths) if paths.is_empty() => item(InputText::Missing),
            Ok(paths) => paths.into_iter().map(|path| WorkItem { input: self.local_input(&path), path }).collect(),
        }
    }

    fn local_input(&mut self, path: &str) -> InputText {
        match self.files.read(path) {
            Ok(Some(file)) => InputText::Local { file: *file },
            Ok(None) => InputText::Failed(SourceFailure::Read(Diagnostic::error(
                "sync-read-file",
                format!("could not read `{path}`"),
            ))),
            Err(problem) => InputText::Failed(SourceFailure::Read(problem.clone())),
        }
    }

    /// The inputs of the sources that `run` a command, all commands at once.
    fn run_inputs(&mut self, root: &Path, today: Day, units: &[&str], work: &mut [WorkSource]) {
        let first = self.book.flows.iter().map(|(_, flow)| flow.day).min().unwrap_or(today);
        let (at, commands): (Vec<usize>, Vec<String>) = work
            .iter()
            .enumerate()
            .filter_map(|(at, current)| {
                let source = &self.book.sources[current.index];
                let Fetch::Run(command) = source.fetch else { return None };
                Some((at, substitute(self.book.text(command), self.since(source, first), today, units)))
            })
            .unzip();
        for (at, result) in at.into_iter().zip(run_all(&commands, root, COMMAND_TIMEOUT)) {
            let name = self.book.name(self.book.sources[work[at].index].name);
            work[at].items.push(self.output(name, result));
        }
    }

    /// The day a source's command should start from: after what its account already has.
    fn since(&self, source: &ModelSource, first: Day) -> Day {
        let ModelSink::Feed { account } = &source.sink else {
            return first;
        };
        let name = self.book.name(self.book.places[*account].path);
        self.world.accounts.get(name).map_or(first, |account| account.since(first))
    }

    /// What a source's command printed, registered as text its diagnostics can point into.
    fn output(&mut self, source: &str, result: Result<String, Failed>) -> WorkItem {
        let path = format!("<sync {source} output>");
        let input = match result {
            Ok(text) => match self.files.generate(path.clone(), text) {
                Ok(file) => InputText::Local { file },
                Err(problem) => InputText::Failed(SourceFailure::Generated(problem)),
            },
            Err(failed) => InputText::Failed(SourceFailure::Command(failed)),
        };
        WorkItem { path, input }
    }

    /// Reads every file a source's input would be merged into.
    fn load_targets(&mut self, work: &[WorkSource]) {
        for current in work {
            let source = &self.book.sources[current.index];
            for item in &current.items {
                let InputText::Local { file } = item.input else { continue };
                let targets = self.files.source(file).map(|output| self.targets(source, output)).unwrap_or_default();
                for path in targets {
                    let _ = self.files.read(&path);
                }
            }
        }
    }

    /// The files `output` would be merged into.
    fn targets(&self, source: &ModelSource, output: &str) -> Vec<String> {
        match self.merging(source) {
            Some(Ok(sink)) => sink::target_paths(sink, output, &self.world.layout),
            _ => Vec::new(),
        }
    }

    /// Where a source merges its output, or `None` for a feed, which is reconciled instead.
    fn merging(&self, source: &ModelSource) -> Option<Result<Sink<'_>, Diagnostic>> {
        let book = self.book;
        Some(match &source.sink {
            ModelSink::Feed { .. } => return None,
            ModelSink::Journal => Ok(Sink::Journal),
            ModelSink::File(path) => Ok(Sink::File(book.text(*path))),
            ModelSink::Param(param) => {
                let value = &book.params[*param];
                let name = book.name(value.name);
                match self.files.paths.get(value.loc.file.0 as usize).copied() {
                    None => Err(Diagnostic::error(
                        "sync-param-file",
                        format!("the declaration for `param {name}` has no project source path"),
                    )),
                    Some(path) if !is_project_path(path) => Err(Diagnostic::error(
                        "sync-param-file",
                        format!("`param {name}` is declared in a non-project file"),
                    )),
                    Some(path) => Ok(Sink::Param { name, path }),
                }
            }
        })
    }

    /// Plans every selected source, documents and Axiom output before statement
    /// feeds, so that their flows take part in the bank's same-run reconciliation.
    fn plan_sources(&mut self, work: &[WorkSource]) -> Vec<SourceResult> {
        let mut order: Vec<usize> = (0..work.len()).collect();
        order.sort_by_key(|&at| matches!(&self.book.sources[work[at].index].sink, ModelSink::Feed { .. }));
        let mut planned: Vec<(usize, SourceResult)> = Vec::new();
        for at in order {
            planned.extend(self.plan_source(&work[at]).into_iter().map(|result| (at, result)));
        }
        planned.sort_by_key(|&(at, _)| at);
        planned.into_iter().map(|(_, result)| result).collect()
    }

    /// One result for each input of the source, or for the source alone if it has none.
    fn plan_source(&mut self, current: &WorkSource) -> Vec<SourceResult> {
        let source = &self.book.sources[current.index];
        let name = self.book.name(source.name);
        let result = |path: &str, planned: Result<usize, Box<SourceFailure>>| match planned {
            Ok(added) => SourceResult { source: name.to_string(), path: path.to_string(), added, failure: None },
            Err(failure) => {
                SourceResult { source: name.to_string(), path: path.to_string(), added: 0, failure: Some(*failure) }
            }
        };
        if current.items.is_empty() {
            return vec![result(name, Ok(0))];
        }
        current.items.iter().map(|item| result(&item.path, self.plan_item(source, item))).collect()
    }

    /// Plans one input of a source: how many items it adds, or why it cannot. The failure is boxed, so that the
    /// result stays small.
    fn plan_item(&mut self, source: &ModelSource, item: &WorkItem) -> Result<usize, Box<SourceFailure>> {
        let file = match &item.input {
            InputText::Local { file } => *file,
            InputText::Missing => return Ok(0),
            InputText::Failed(failure) => return Err(failure.clone().into()),
        };
        let text = self.files.source(file).ok_or_else(|| {
            SourceFailure::Read(Diagnostic::error("sync-source-text", "registered source text is unavailable"))
        })?;
        if let Some(problem) = self.targets(source, text).iter().find_map(|path| self.files.unreadable(path)) {
            return Err(SourceFailure::Read(problem).into());
        }
        let (inserts, delta) = self.inserts_of(source, text, file).map_err(SourceFailure::Output)?;
        let feed = delta.is_some();
        if feed {
            self.load_inserted(&inserts);
        }
        if let Some(problem) = inserts.iter().find_map(|insert| self.files.unreadable(&insert.path)) {
            return Err(SourceFailure::Read(problem).into());
        }
        let previewed = preview(&inserts, &mut |path| self.files.current(path));
        let updates = previewed.and_then(|updates| self.files.validated(updates)).map_err(SourceFailure::Output)?;
        if let Some(delta) = delta {
            self.world.commit_feed(delta);
        }
        for update in updates {
            self.files.planned.insert(update.path, update.after);
        }
        let added = inserts.len();
        if !feed {
            self.world.learn(&inserts);
        }
        self.inserts.extend(inserts);
        Ok(added)
    }

    /// Reads the files a feed's inserts go into, which only the feed knows.
    fn load_inserted(&mut self, inserts: &[Insert]) {
        for insert in inserts {
            if !self.files.planned.contains_key(&insert.path) {
                let _ = self.files.read(&insert.path);
            }
        }
    }

    /// What `source` would insert, and for a feed what it adds to the world.
    fn inserts_of(
        &self,
        source: &ModelSource,
        text: &str,
        file: FileId,
    ) -> Result<(Vec<Insert>, Option<FeedDelta<'s>>), Vec<Diagnostic>> {
        let Some(sink) = self.merging(source) else {
            return self.feed(source, text, file).map(|(inserts, delta)| (inserts, Some(delta)));
        };
        let sink = sink.map_err(|problem| vec![problem])?;
        sink::merge_at(sink, text, &self.world.layout, file, &mut |path| self.files.current(path))
            .map(|inserts| (inserts, None))
    }

    /// The records of a statement, reconciled against the account's journal.
    fn feed(
        &self,
        source: &ModelSource,
        text: &str,
        file: FileId,
    ) -> Result<(Vec<Insert>, FeedDelta<'s>), Vec<Diagnostic>> {
        let book = self.book;
        let (name, ModelSink::Feed { account }) = (book.name(source.name), &source.sink) else {
            unreachable!("only feeds are not merged");
        };
        let problem = |code, message| Err(vec![Diagnostic::error(code, message)]);
        let Some(format_id) = source.format else {
            return problem("sync-no-format", format!("`{name}` has no format for its feed"));
        };
        let Some(format) = book.formats.get(format_id) else {
            return problem("sync-no-format", format!("`{name}` refers to a format that is not in the book"));
        };
        let place = &book.places[*account];
        if !matches!(place.role, Role::Account { .. }) {
            return problem("sync-feed-account", format!("`{name}` does not target an account"));
        }
        let commodity = &book.commodities[book.entities[place.owner].currency];
        let unit = Unit { name: book.name(commodity.symbol), scale: commodity.scale };
        self.world.plan_feed_at(&Feed { account: book.name(place.path), unit, format }, text, file)
    }

    /// Previews every insert against the files as they were read: the changes to write, or what is wrong with them.
    fn settle(&mut self) -> (Vec<Change>, Vec<Diagnostic>) {
        let previewed = preview(&self.inserts, &mut |path| self.files.as_read(path).map(Cow::Borrowed));
        match previewed.and_then(|updates| self.files.validated(updates)) {
            Ok(updates) => {
                let change = |update: Update| Change {
                    before: self.files.as_read(&update.path).map(str::to_owned),
                    path: update.path,
                    after: update.after,
                };
                (updates.into_iter().map(change).collect(), Vec::new())
            }
            Err(problems) => (Vec::new(), problems),
        }
    }
}

fn selected_sources(book: &Book<'_>, wanted: &[&str]) -> Result<Vec<usize>, Diagnostic> {
    if wanted.is_empty() {
        return Ok((0..book.sources.len()).collect());
    }
    let mut selected = Vec::with_capacity(wanted.len());
    for &name in wanted {
        let Some(index) = book.sources.iter().position(|source| book.name(source.name).eq_ignore_ascii_case(name))
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

fn command_units<'s>(book: &Book<'s>, run: &EngineRun) -> Vec<&'s str> {
    let mut units: Vec<_> =
        run.holdings.iter().map(|holding| book.name(book.commodities[holding.unit].symbol)).collect();
    units.sort_unstable();
    units.dedup();
    units
}

fn monitor_gaps(book: &Book<'_>, run: &EngineRun) -> Vec<Diagnostic> {
    let has_contracts = !book.contracts.is_empty();
    let has_claim_places = book.places.iter().any(|(_, place)| place.claim);
    if (has_contracts || has_claim_places) && !run.monitor_complete {
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

    use axiom_core::{Day, Diagnostic, FileId};
    use axiom_engine::{Options, Plan};
    use axiom_model::Source;
    use axiom_syntax::Folder;

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
            self.files.iter().find(|(id, _, _)| *id == file).map(|(_, _, text)| text.as_str())
        }

        fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
            let file = FileId(
                u16::try_from(self.next).map_err(|_| Diagnostic::error("sync-file-limit", "too many source files"))?,
            );
            self.next += 1;
            self.files.push((file, path.to_string(), text));
            Ok(file)
        }
    }

    #[test]
    fn each_run_source_gets_its_own_output_and_registered_diagnostic_file() {
        let std = include_str!("../../systems/src/std.ax");
        let axiom = concat!(
            "base USD\n",
            "sync first\n",
            "  run printf '2026-01-02 checking -> food 10 USD\\n'\n",
            "  into first.ax\n",
            "sync second\n",
            "  run printf '2026-01-03 checking -> food 20 USD\\n'\n",
            "  into second.ax\n",
            "sync bad\n",
            "  run printf '2026-01-03 checking -> food 2 USD\\n2026-01-04 checking -> food 1 USD )\\n'\n",
            "  into bad.ax\n",
        );
        let (std_file, diagnostics) = axiom_syntax::parse(FileId(0), std, Folder::default());
        assert!(diagnostics.is_empty(), "std.ax: {diagnostics:?}");
        let (axiom_file, diagnostics) = axiom_syntax::parse(FileId(1), axiom, Folder::default());
        assert!(diagnostics.is_empty(), "axiom.ax: {diagnostics:?}");
        let sources = [
            Source { path: "std.ax", file: std_file, embedded: true },
            Source { path: "axiom.ax", file: axiom_file, embedded: false },
        ];
        let (book, diagnostics) = axiom_model::build(&sources);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let today = Day::parse(b"2026-01-10").unwrap();
        let run = Plan::new(&book).run(Options { today, relaxed: false });
        let mut registry = Registry { next: u32::try_from(sources.len()).unwrap(), ..Registry::default() };
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
            outcome.sources.iter().map(|s| (&s.source, &s.path, s.added)).collect::<Vec<_>>()
        );
        assert_eq!(outcome.sources[0].source, "first");
        assert_eq!(outcome.sources[0].added, 1);
        assert_eq!(outcome.sources[1].source, "second");
        assert_eq!(outcome.sources[1].added, 1);
        assert_eq!(outcome.sources[2].source, "bad");
        assert!(matches!(outcome.sources[2].failure.as_ref(), Some(super::SourceFailure::Output(_))));
        assert_eq!(outcome.generated.len(), 3);
        assert_ne!(outcome.generated[0].file, outcome.generated[1].file);
        assert_eq!(registry.text(outcome.generated[0].file), Some("2026-01-02 checking -> food 10 USD\n"));
        assert_eq!(registry.text(outcome.generated[1].file), Some("2026-01-03 checking -> food 20 USD\n"));
        assert_eq!(
            registry.text(outcome.generated[2].file),
            Some("2026-01-03 checking -> food 2 USD\n2026-01-04 checking -> food 1 USD )\n")
        );
        let Some(super::SourceFailure::Output(problems)) = outcome.sources[2].failure.as_ref() else {
            panic!("the malformed output should retain its parser diagnostic")
        };
        let problem = problems.first().expect("a malformed item is diagnosed");
        let label = problem.labels.first().expect("syntax diagnostics identify the offending text");
        assert_eq!(label.loc.file, outcome.generated[2].file);
        let output = registry.text(outcome.generated[2].file).unwrap();
        let slice = &output[label.loc.range()];
        assert!(
            label.loc.start as usize > output.find('\n').unwrap(),
            "the label should start on the malformed second line: {label:?}"
        );
        assert!(slice.contains(')'), "label {label:?} points to {slice:?}");
        assert_eq!(
            outcome.changes.iter().map(|change| change.path.as_str()).collect::<Vec<_>>(),
            ["first.ax", "second.ax"]
        );
    }
}
