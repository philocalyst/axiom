//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::{Options, Run};
use axiom_model::{Book, sync::Fetch};
use axiom_report::json::{JsonRenderer, string as json_string};
use axiom_report::{Context, Query, ReportRenderer, Summary};
use axiom_sync::{Change, PlanOutcome, SourceFailure, SourceResult};

use crate::args::{Command, Invocation};
use crate::project::{Project, SourceFile, Sources};
use crate::render::{Limit, Renderer, Tally};
use crate::style::{Ink, Line};
use crate::text::plural;
use crate::{Outcome, Terminals, help, sync, table};

/// Runs the command. An `Err` means there was nothing to run it on: the
/// project could not be found or read.
pub fn run(invocation: &Invocation, terminals: Terminals) -> Result<Outcome, Diagnostic> {
    let command = &invocation.command;
    match command {
        Command::Help => return Ok(Outcome::ok(help::screen(terminals.out))),
        Command::Version => return Ok(Outcome::ok(help::version())),
        _ => {}
    }
    let project = Project::find(invocation.project.unwrap_or(Path::new(".")))?;
    let mut sources = project.load()?;
    if let Command::Fmt { files, check } = command {
        return Ok(crate::fmt::execute(&sources, &project.root, files, *check, terminals.out));
    }
    let (parsed, mut diagnostics) = Sources::parse_files(&sources.files);
    let (book, built) = axiom_model::build(&parsed);
    // The syntax trees are done with; the book borrows only the source text.
    drop(parsed);
    diagnostics.extend(built);

    let options = Options { today: invocation.today.unwrap_or_else(system_today), relaxed: invocation.relaxed };
    let shown = Shown {
        terminals,
        limit: if invocation.all { Limit::Every } else { Limit::Capped },
        form: if invocation.json { Form::Json } else { Form::Text },
    };
    match command {
        Command::Sync { names, dry } => {
            let run = axiom_engine::run(&book, options);
            let (files, auxiliary) = (&sources.files, &mut sources.auxiliary);
            let planned = sync::plan(&book, &run, &project, options.today, names, files, auxiliary)?;
            diagnostics.extend(run.diagnostics);
            let fate = if *dry { Fate::Shown } else { Fate::apply(&diagnostics, &project.root, &planned.changes) };
            Ok(render_sync(planned, fate, &sources, terminals, &diagnostics))
        }
        Command::Report(query, whose) => match Context::new(&book, options, *whose) {
            Ok(context) => {
                Ok(Session::new(&book, context.run(), &sources, &diagnostics, shown).report(&context, query))
            }
            Err(problem) => {
                // Owner resolution failed before the context could make its run, whose diagnostics are still shown.
                let run = axiom_engine::run(&book, options);
                Ok(Session::new(&book, &run, &sources, &diagnostics, shown).refuse(&problem))
            }
        },
        _ => {
            // `check` needs only the final run summary; constructing a report context
            // would also retain the pre-close checkpoint that no check view uses.
            let run = axiom_engine::run(&book, options);
            let (reader_diagnostics, suggestions) = if matches!(command, Command::Check) {
                check_memos(&book, &project, &sources.files, &mut sources.auxiliary)
            } else {
                (Vec::new(), Vec::new())
            };
            diagnostics.extend(reader_diagnostics);
            Ok(Session::new(&book, &run, &sources, &diagnostics, shown).check(&suggestions))
        }
    }
}

/// Reads only declared local `read` sources for check's unknown-memo hints.
/// Run sources are intentionally inert here: checking a book never executes
/// an external command.
fn check_memos(
    book: &Book<'_>,
    project: &Project,
    parsed_files: &[SourceFile],
    auxiliary: &mut Vec<SourceFile>,
) -> (Vec<Diagnostic>, Vec<MemoSuggestion>) {
    let mut files = LocalFiles::new(project, parsed_files.len(), auxiliary);
    let mut inputs = Vec::new();
    for (source_index, source) in book.sources.iter().enumerate() {
        let Fetch::Read(pattern) = source.fetch else {
            continue;
        };
        match axiom_sync::matching_paths(&project.root, book.text(pattern)) {
            Ok(paths) => {
                inputs.extend(paths.into_iter().filter_map(|path| files.register(path)).map(|id| (source_index, id)))
            }
            Err(problem) => files.diagnostics.push(problem),
        }
    }
    let LocalFiles { auxiliary, mut diagnostics, .. } = files;

    // Appending is finished before any memo borrows begin. Auxiliary strings
    // live in a disjoint vector, so the parsed Book keeps borrowing `files`.
    let mut memos = Vec::new();
    for (source_index, file_id) in inputs {
        let Some(file) = source_by_id(parsed_files, auxiliary, file_id) else {
            continue;
        };
        match axiom_sync::read_memos(book, &book.sources[source_index], &file.text, file_id) {
            Ok(found) => memos.extend(found),
            Err(problems) => diagnostics.extend(problems),
        }
    }
    (diagnostics, MemoSuggestion::of_unrecognized(&memos))
}

/// The local files that `read` sources name, each registered once so that its text can be borrowed.
struct LocalFiles<'p, 'a> {
    project: &'p Project,
    /// How many files the book was parsed from: the id of the first one registered here.
    parsed: usize,
    auxiliary: &'a mut Vec<SourceFile>,
    registered: HashMap<String, FileId>,
    /// Why a file could not be registered.
    diagnostics: Vec<Diagnostic>,
}

impl<'p, 'a> LocalFiles<'p, 'a> {
    fn new(project: &'p Project, parsed: usize, auxiliary: &'a mut Vec<SourceFile>) -> LocalFiles<'p, 'a> {
        let registered = auxiliary.iter().map(|file| (file.path.to_string(), file.id)).collect();
        LocalFiles { project, parsed, auxiliary, registered, diagnostics: Vec::new() }
    }

    /// The id of the file at `path`, which is read and registered the first time. A file that cannot be is a
    /// diagnostic, and no input.
    fn register(&mut self, path: String) -> Option<FileId> {
        if let Some(&id) = self.registered.get(&path) {
            return Some(id);
        }
        let text = match self.project.read_local(&path) {
            Ok(text) => text,
            Err(problem) => {
                self.diagnostics.push(problem);
                return None;
            }
        };
        match Sources::append_auxiliary_to(self.auxiliary, self.parsed, path.clone(), text) {
            Ok(id) => {
                self.registered.insert(path, id);
                Some(id)
            }
            Err(problem) => {
                self.diagnostics.push(problem);
                None
            }
        }
    }
}

fn source_by_id<'a>(files: &'a [SourceFile], auxiliary: &'a [SourceFile], id: FileId) -> Option<&'a SourceFile> {
    let index = usize::from(id.0);
    files.get(index).or_else(|| auxiliary.get(index.checked_sub(files.len())?))
}

#[derive(Clone, Debug)]
struct MemoSuggestion {
    stem: String,
    count: usize,
    example: String,
    pattern: String,
    known_as: String,
}

impl MemoSuggestion {
    /// A suggestion for each group of memos that nothing recognized.
    fn of_unrecognized(memos: &[Cow<'_, str>]) -> Vec<MemoSuggestion> {
        let groups = axiom_sync::unrecognized(memos.iter().map(|memo| memo.as_ref()));
        groups
            .into_iter()
            .map(|group| MemoSuggestion {
                stem: group.stem.to_owned(),
                count: group.count,
                example: group.example.to_owned(),
                pattern: group.pattern(),
                known_as: group.known_as(),
            })
            .collect()
    }
}

fn suggestion_lines(suggestions: &[MemoSuggestion]) -> Vec<String> {
    if suggestions.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["Memos nothing recognized:".to_string()];
    for group in suggestions {
        lines.push(format!("  {} ({} records)", group.example.replace('\n', " ").replace('\r', " "), group.count));
        lines.push(format!("    {}", group.known_as));
    }
    lines
}

/// A separate NDJSON record keeps diagnostic objects unchanged while
/// returning the memo groups as machine-readable data.
fn json_suggestions(suggestions: &[MemoSuggestion]) -> String {
    if suggestions.is_empty() {
        return String::new();
    }
    let mut out = String::from("{\"type\":\"unrecognized_memos\",\"groups\":[");
    for (index, group) in suggestions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"stem\":");
        json_string(&mut out, &group.stem);
        let _ = write!(out, ",\"count\":{},\"example\":", group.count);
        json_string(&mut out, &group.example);
        out.push_str(",\"pattern\":");
        json_string(&mut out, &group.pattern);
        out.push_str(",\"known_as\":");
        json_string(&mut out, &group.known_as);
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

/// What became of the changes a sync planned.
enum Fate {
    /// `--dry`: they are shown as diffs and nothing is written.
    Shown,
    /// The project has errors, so nothing is written.
    Blocked,
    /// They were written, and these are what stopped any of them.
    Applied(Vec<Diagnostic>),
}

impl Fate {
    /// `changes` are written under the project at `root`, unless the book's `diagnostics` hold an error.
    fn apply(diagnostics: &[Diagnostic], root: &Path, changes: &[Change]) -> Fate {
        if diagnostics.iter().any(Diagnostic::is_error) {
            Fate::Blocked
        } else {
            Fate::Applied(axiom_sync::apply(root, changes))
        }
    }

    fn problems(&self) -> &[Diagnostic] {
        match self {
            Fate::Applied(problems) => problems,
            Fate::Shown | Fate::Blocked => &[],
        }
    }
}

/// Presents a sync plan, and what became of its changes.
fn render_sync(
    planned: PlanOutcome,
    fate: Fate,
    sources: &Sources,
    terminals: Terminals,
    prior: &[Diagnostic],
) -> Outcome {
    let mut diagnostics: Vec<&Diagnostic> = prior.iter().chain(&planned.problems).collect();
    diagnostics.extend(
        planned.sources.iter().filter_map(|source| source.failure.as_ref()).flat_map(SourceFailure::diagnostics),
    );
    diagnostics.extend(fate.problems());

    let mut answer: String = planned.sources.iter().map(|source| source_report(source, terminals)).collect();
    answer.push_str(&changes_report(&planned.changes, &fate));
    if planned.sources.is_empty() && planned.changes.is_empty() {
        answer.push_str("no sync sources selected\n");
    } else if planned.changes.is_empty() {
        answer.push_str("no changes\n");
    }

    let (diagnostic_text, tally) = Renderer::new(sources, terminals.err).present(&diagnostics, Limit::Every);
    let failed = tally.errors > 0
        || !fate.problems().is_empty()
        || planned.sources.iter().any(|source| source.failure.is_some());
    Outcome { answer, diagnostics: diagnostic_text, failed }
}

/// What one source of a sync came to: its line, preceded by what a failed command printed.
fn source_report(source: &SourceResult, terminals: Terminals) -> String {
    let mut line = match &source.failure {
        None => Line::text("✓ ", Ink::GREEN.bold()),
        Some(_) => Line::text("✗ ", Ink::RED.bold()),
    };
    line.push(&source.path, Ink::BOLD);
    line.push("  ", Ink::PLAIN);
    let mut report = String::new();
    match &source.failure {
        None if source.added == 0 => line.push("no new items", Ink::DIM),
        None => line.push(&format!("{} added", plural(source.added, "item")), Ink::PLAIN),
        Some(SourceFailure::Command(failure)) => {
            line.push(&failure.summary, Ink::RED);
            if !failure.stderr.trim().is_empty() {
                report.push_str(&failure.stderr);
                if !failure.stderr.ends_with('\n') {
                    report.push('\n');
                }
            }
        }
        Some(SourceFailure::Read(_) | SourceFailure::Generated(_) | SourceFailure::Output(_)) => {
            line.push("see diagnostics", Ink::RED);
        }
    }
    report + &terminals.out.painter.paint(&[line])
}

/// The changes as they were shown, refused, or written.
fn changes_report(changes: &[Change], fate: &Fate) -> String {
    match fate {
        Fate::Shown => changes.iter().map(change_diff).collect(),
        Fate::Blocked if !changes.is_empty() => {
            "sync changes were not applied because the project has errors\n".to_string()
        }
        Fate::Applied(problems) if problems.is_empty() => {
            changes.iter().map(|change| format!("updated {}\n", change.path)).collect()
        }
        Fate::Blocked | Fate::Applied(_) => String::new(),
    }
}

/// A compact unified hunk: the common prefix and suffix stay context, and the
/// changed middle is shown once, regardless of how much text the target has.
fn change_diff(change: &Change) -> String {
    let before = change.before.as_deref().unwrap_or("");
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = change.after.split_inclusive('\n').collect();
    let (prefix, suffix) = shared_lines(&old, &new);
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let start = prefix.saturating_sub(1);
    let old_count = old_end.saturating_sub(start) + usize::from(suffix > 0);
    let new_count = new_end.saturating_sub(start) + usize::from(suffix > 0);
    let mut out = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{first},{old_count} +{first},{new_count} @@\n",
        path = change.path,
        first = start + 1,
    );
    if start < prefix {
        push_line(&mut out, ' ', old[start]);
    }
    for line in &old[prefix..old_end] {
        push_line(&mut out, '-', line);
    }
    for line in &new[prefix..new_end] {
        push_line(&mut out, '+', line);
    }
    if suffix > 0 {
        push_line(&mut out, ' ', old[old.len() - suffix]);
    }
    out
}

/// How many lines two versions of a file share at their start, and then at their end.
fn shared_lines(old: &[&str], new: &[&str]) -> (usize, usize) {
    let prefix = old.iter().zip(new).take_while(|(before, after)| before == after).count();
    let (old, new) = (&old[prefix..], &new[prefix..]);
    let suffix = old.iter().rev().zip(new.iter().rev()).take_while(|(before, after)| before == after).count();
    (prefix, suffix)
}

/// Adds `line` to a diff, marked, and ended if it was the last of its file and had no line ending.
fn push_line(diff: &mut String, mark: char, line: &str) {
    diff.push(mark);
    diff.push_str(line);
    if !line.ends_with('\n') {
        diff.push('\n');
    }
}

/// The current day, by the system clock, in UTC.
fn system_today() -> Day {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Day((seconds / 86_400) as i32)
}

/// How an answer is shown: where it goes, and in what form.
#[derive(Clone, Copy)]
struct Shown {
    terminals: Terminals,
    limit: Limit,
    form: Form,
}

/// What an answer is written as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// For a reader at a terminal.
    Text,
    /// For a program: independent of terminal width and colour.
    Json,
}

/// A book that has been run, and everything found on the way.
struct Session<'a, 's> {
    book: &'a Book<'s>,
    run: &'a Run,
    sources: &'a Sources,
    /// From parsing, building, and running.
    diagnostics: Vec<&'a Diagnostic>,
    shown: Shown,
}

impl<'a, 's> Session<'a, 's> {
    /// The book as run, with what was found before the run and by it.
    fn new(
        book: &'a Book<'s>,
        run: &'a Run,
        sources: &'a Sources,
        before: &'a [Diagnostic],
        shown: Shown,
    ) -> Session<'a, 's> {
        Session { book, run, sources, diagnostics: before.iter().chain(&run.diagnostics).collect(), shown }
    }

    /// Every diagnostic; and if none is an error, the book in one line.
    fn check(&self, suggestions: &[MemoSuggestion]) -> Outcome {
        let (diagnostics, tally) = self.show(&self.diagnostics);
        if self.shown.form == Form::Json {
            let mut answer = diagnostics;
            answer.push_str(&json_suggestions(suggestions));
            return Outcome { answer, diagnostics: String::new(), failed: tally.errors > 0 };
        }
        if tally.errors > 0 {
            let mut diagnostics = diagnostics;
            diagnostics.push_str(&self.suggestions(suggestions, self.shown.terminals.err));
            return Outcome { answer: String::new(), diagnostics, failed: true };
        }
        let summary = axiom_report::summary(self.book, self.run);
        let mut answer = self.shown.terminals.out.painter.paint(&[summary_line(self.book, &summary)]);
        answer.push_str(&self.suggestions(suggestions, self.shown.terminals.out));
        Outcome { answer, diagnostics, failed: false }
    }

    fn suggestions(&self, suggestions: &[MemoSuggestion], terminal: crate::style::Terminal) -> String {
        let lines = suggestion_lines(suggestions);
        if lines.is_empty() {
            return String::new();
        }
        let lines = lines.iter().map(|suggestion| Line::text(suggestion, Ink::DIM)).collect::<Vec<_>>();
        terminal.painter.paint(&lines)
    }

    /// The errors, and the report. A report runs whatever the book's errors, so
    /// that a reader can investigate them; it says at its head what it rests on.
    fn report(&self, context: &Context<'_, '_>, query: &Query<'_>) -> Outcome {
        match context.report_with_sources(query, self.sources) {
            Ok(report) => self.answer(&report),
            Err(problem) => self.refuse(&problem),
        }
    }

    /// The errors, and why there is no report.
    fn refuse(&self, problem: &Diagnostic) -> Outcome {
        let mut errors = self.errors();
        errors.push(problem);
        let (diagnostics, _) = self.show(&errors);
        if self.shown.form == Form::Json {
            Outcome { answer: diagnostics, diagnostics: String::new(), failed: true }
        } else {
            Outcome { answer: String::new(), diagnostics, failed: true }
        }
    }

    /// The report, and the errors it rests on.
    fn answer(&self, report: &axiom_report::Report<'_>) -> Outcome {
        let (diagnostics, tally) = self.show(&self.errors());
        if self.shown.form == Form::Json {
            return Outcome {
                answer: JsonRenderer.render(report, self.sources),
                diagnostics,
                failed: tally.errors > 0,
            };
        }
        let mut answer = table::TableRenderer { terminal: self.shown.terminals.out }.render(report, self.sources);
        if tally.errors > 0 {
            let caveat = format!(
                "rests on a book with {} (`axiom check` lists them): what they touch may be wrong",
                plural(tally.errors, "error")
            );
            let mut line = Line::text("✗ ", Ink::RED.bold());
            line.push(&caveat, Ink::DIM);
            answer.insert_str(0, &self.shown.terminals.out.painter.paint(&[line, Line::new()]));
        }
        Outcome { answer, diagnostics, failed: tally.errors > 0 }
    }

    fn errors(&self) -> Vec<&'a Diagnostic> {
        self.diagnostics.iter().copied().filter(|found| found.is_error()).collect()
    }

    /// The diagnostics, and after them how many of each there were.
    fn show(&self, diagnostics: &[&Diagnostic]) -> (String, Tally) {
        if self.shown.form == Form::Json {
            return (
                crate::render::json::diagnostics(diagnostics, self.sources),
                Tally::of(diagnostics.iter().copied()),
            );
        }
        let (mut text, tally) =
            Renderer::new(self.sources, self.shown.terminals.err).present(diagnostics, self.shown.limit);
        if let Some(line) = tally.line() {
            text += &self.shown.terminals.err.painter.paint(&[line]);
        }
        (text, tally)
    }
}

/// `✓ 1,284 flows · 23 places · 4 laws enforced · net worth 184,220.13 USD`
fn summary_line(book: &Book, summary: &Summary) -> Line {
    let facts = [
        plural(summary.flows, "flow"),
        plural(summary.places, "place"),
        format!("{} enforced", plural(summary.laws, "law")),
        format!("net worth {}", book.show(summary.net_worth)),
    ];
    let mut line = Line::text("✓ ", Ink::GREEN.bold());
    line.push(&facts.join(" · "), Ink::PLAIN);
    if summary.unpriced > 0 {
        line.push(&format!(" · {} unpriced", plural(summary.unpriced, "holding")), Ink::YELLOW);
    }
    line
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::project::Project;
    use crate::testing::TempDir;

    #[test]
    fn check_reads_local_memos_and_never_runs_declared_commands() {
        let dir = TempDir::new("check-memos");
        let marker = dir.path().join("command-ran");
        dir.write("axiom.ax", "base USD\n");
        dir.write(
            "sync.ax",
            &format!(
                "sync checking\n  read \"imports/*.csv\"\n  format csv\n    date \"Date\" \"YYYY-MM-DD\"\n    amount \"Amount\"\n    memo \"Description\"\nsync prices\n  run touch {}\n  into prices.ax\n",
                marker.display()
            ),
        );
        dir.write(
            "imports/card.csv",
            "Date,Amount,Description\n2026-01-01,12.00,TRADER JOE'S #12\n2026-01-02,13.00,Trader Joe's #13\n",
        );

        let project = Project::find(dir.path()).unwrap();
        let mut sources = project.load().unwrap();
        let (parsed, mut diagnostics) = Sources::parse_files(&sources.files);
        let (book, built) = axiom_model::build(&parsed);
        drop(parsed);
        diagnostics.extend(built);
        assert!(diagnostics.iter().all(|problem| !problem.is_error()), "fixture has no model errors: {diagnostics:?}");

        let (read_problems, suggestions) = check_memos(&book, &project, &sources.files, &mut sources.auxiliary);
        assert!(read_problems.is_empty(), "{read_problems:?}");
        assert!(suggestions.iter().any(|group| group.count == 2));
        assert!(suggestions.iter().any(|group| group.known_as == "known-as \"TRADER JOE'S\""));
        assert!(!marker.exists(), "check must never execute a declared run command");
    }

    #[test]
    fn sync_dry_shows_a_change_without_writing_it() {
        let dir = TempDir::new("sync-dry");
        dir.write("axiom.ax", "base USD\n");
        dir.write("prices.ax", "old\n");
        let project = Project::find(dir.path()).unwrap();
        let sources = project.load().unwrap();
        let changes =
            vec![Change { path: "prices.ax".to_owned(), before: Some("old\n".to_owned()), after: "new\n".to_owned() }];
        let fate = Fate::Shown;
        let planned = PlanOutcome { sources: Vec::new(), changes, problems: Vec::new(), generated: Vec::new() };
        let terminals = Terminals { out: crate::style::Terminal::plain(80), err: crate::style::Terminal::plain(80) };
        let outcome = render_sync(planned, fate, &sources, terminals, &[]);

        assert!(!outcome.failed);
        assert!(outcome.answer.contains("-old\n"));
        assert!(outcome.answer.contains("+new\n"));
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
    }

    #[cfg(unix)]
    #[test]
    fn sync_refuses_to_write_through_a_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new("sync-symlink");
        let outside = TempDir::new("sync-symlink-outside");
        dir.write("axiom.ax", "base USD\n");
        symlink(outside.path(), dir.path().join("link")).unwrap();
        let project = Project::find(dir.path()).unwrap();
        let sources = project.load().unwrap();
        let changes =
            vec![Change { path: "link/new-folder/prices.ax".to_owned(), before: None, after: "new\n".to_owned() }];
        let fate = Fate::apply(&[], &project.root, &changes);
        let planned = PlanOutcome { sources: Vec::new(), changes, problems: Vec::new(), generated: Vec::new() };
        let terminals = Terminals { out: crate::style::Terminal::plain(80), err: crate::style::Terminal::plain(80) };
        let outcome = render_sync(planned, fate, &sources, terminals, &[]);

        assert!(outcome.failed);
        assert!(!outside.path().join("new-folder").exists());
    }

    #[test]
    fn sync_does_not_apply_changes_when_the_loaded_book_has_errors() {
        let dir = TempDir::new("sync-invalid-book");
        dir.write("axiom.ax", "base USD\n");
        dir.write("prices.ax", "old\n");
        let project = Project::find(dir.path()).unwrap();
        let sources = project.load().unwrap();
        let invalid_book = [Diagnostic::error("invalid-book", "the project has a model error")];
        let changes =
            vec![Change { path: "prices.ax".to_owned(), before: Some("old\n".to_owned()), after: "new\n".to_owned() }];
        let fate = Fate::apply(&invalid_book, &project.root, &changes);
        let planned = PlanOutcome { sources: Vec::new(), changes, problems: Vec::new(), generated: Vec::new() };
        let terminals = Terminals { out: crate::style::Terminal::plain(80), err: crate::style::Terminal::plain(80) };
        let outcome = render_sync(planned, fate, &sources, terminals, &invalid_book);

        assert!(outcome.failed);
        assert!(outcome.answer.contains("not applied because the project has errors"));
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
    }
}
