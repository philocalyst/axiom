//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::{Options, Run};
use axiom_model::{Book, sync::Fetch};
use axiom_report::{Context, Query, ReportRenderer, Summary, json::JsonRenderer};
use axiom_sync::{Change, PlanOutcome, SourceFailure};

use crate::args::{Command, Invocation};
use crate::project::{Project, SourceFile, Sources};
use crate::render::{Renderer, Tally};
use crate::style::{Ink, Line};
use crate::text::plural;
use crate::{Outcome, Terminals, help, sync, table};

/// Runs the command. An `Err` means there was nothing to run it on: the
/// project could not be found or read.
pub fn run(invocation: &Invocation, terminals: Terminals) -> Result<Outcome, Diagnostic> {
    let Invocation { command, .. } = invocation;
    match command {
        Command::Help => return Ok(Outcome::ok(help::screen(terminals.out))),
        Command::Version => return Ok(Outcome::ok(help::version())),
        _ => {}
    }
    let project = Project::find(invocation.project.unwrap_or(Path::new(".")))?;
    let mut sources = project.load()?;
    if let Command::Fmt { files, check } = command {
        return Ok(crate::fmt::execute(
            &sources,
            &project.root,
            files,
            *check,
            terminals.out,
        ));
    }
    let (parsed, mut diagnostics) = Sources::parse_files(&sources.files);
    let (book, built) = axiom_model::build(&parsed);
    // The syntax trees are done with; the book borrows only the source text.
    drop(parsed);
    diagnostics.extend(built);

    let options = Options {
        today: invocation.today.unwrap_or_else(system_today),
        relaxed: invocation.relaxed,
    };
    if let Command::Sync { names, dry } = command {
        let run = axiom_engine::run(&book, options);
        let planned = {
            let (files, auxiliary) = (&sources.files, &mut sources.auxiliary);
            sync::plan(
                &book,
                &run,
                &project,
                options.today,
                names,
                files,
                auxiliary,
            )?
        };
        let mut sync_diagnostics = diagnostics;
        sync_diagnostics.extend(run.diagnostics);
        return Ok(render_sync(
            planned,
            &mut sources,
            &project.root,
            *dry,
            terminals,
            &sync_diagnostics,
        ));
    }
    if let Command::Report(query, whose) = command {
        let context = match Context::new(&book, options, *whose) {
            Ok(context) => context,
            Err(problem) => {
                // Preserve the report command's diagnostics and exit status if
                // owner resolution fails before Context can create its run.
                let run = axiom_engine::run(&book, options);
                let mut shown: Vec<&Diagnostic> = diagnostics
                    .iter()
                    .chain(&run.diagnostics)
                    .filter(|diagnostic| diagnostic.is_error())
                    .collect();
                shown.push(&problem);
                return Ok(report_error(
                    &shown,
                    &sources,
                    terminals,
                    invocation.all,
                    invocation.json,
                ));
            }
        };
        let run = context.run();
        let session = Session {
            book: &book,
            run,
            sources: &sources,
            diagnostics: diagnostics.iter().chain(&run.diagnostics).collect(),
            terminals,
            all: invocation.all,
            json: invocation.json,
        };
        return Ok(session.report(&context, query));
    }

    // `check` needs only the final run summary; constructing a report context
    // would also retain the pre-close checkpoint that no check view uses.
    let run = axiom_engine::run(&book, options);
    let (reader_diagnostics, suggestions) = if matches!(command, Command::Check) {
        check_memos(&book, &project, &sources.files, &mut sources.auxiliary)
    } else {
        (Vec::new(), Vec::new())
    };
    diagnostics.extend(reader_diagnostics);
    let session = Session {
        book: &book,
        run: &run,
        sources: &sources,
        diagnostics: diagnostics.iter().chain(&run.diagnostics).collect(),
        terminals,
        all: invocation.all,
        json: invocation.json,
    };
    Ok(session.check(&suggestions))
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
    let mut diagnostics = Vec::new();
    let mut inputs = Vec::new();
    let mut registered: HashMap<String, FileId> = auxiliary
        .iter()
        .map(|file| (file.path.to_string(), file.id))
        .collect();
    for (source_index, source) in book.sources.iter().enumerate() {
        let Fetch::Read(pattern) = source.fetch else {
            continue;
        };
        let pattern = book.text(pattern);
        let paths = match axiom_sync::matching_paths(&project.root, pattern) {
            Ok(paths) => paths,
            Err(problem) => {
                diagnostics.push(problem);
                continue;
            }
        };
        for path in paths {
            if let Some(&file) = registered.get(&path) {
                inputs.push((source_index, file));
                continue;
            }
            let text = match project.read_local(&path) {
                Ok(text) => text,
                Err(problem) => {
                    diagnostics.push(problem);
                    continue;
                }
            };
            match Sources::append_auxiliary_to(auxiliary, parsed_files.len(), path.clone(), text) {
                Ok(file) => {
                    registered.insert(path, file);
                    inputs.push((source_index, file));
                }
                Err(problem) => diagnostics.push(problem),
            }
        }
    }

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

    let groups = axiom_sync::unrecognized(memos.iter().map(|memo| memo.as_ref()));
    let suggestions = groups
        .into_iter()
        .map(|group| MemoSuggestion {
            stem: group.stem.to_owned(),
            count: group.count,
            example: group.example.to_owned(),
            pattern: group.pattern(),
            known_as: group.known_as(),
        })
        .collect();
    (diagnostics, suggestions)
}

fn source_by_id<'a>(
    files: &'a [SourceFile],
    auxiliary: &'a [SourceFile],
    id: FileId,
) -> Option<&'a SourceFile> {
    let index = usize::from(id.0);
    files
        .get(index)
        .or_else(|| auxiliary.get(index.checked_sub(files.len())?))
}

#[derive(Clone, Debug)]
struct MemoSuggestion {
    stem: String,
    count: usize,
    example: String,
    pattern: String,
    known_as: String,
}

fn suggestion_lines(suggestions: &[MemoSuggestion]) -> Vec<String> {
    if suggestions.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["Memos nothing recognized:".to_string()];
    for group in suggestions {
        lines.push(format!(
            "  {} ({} records)",
            group.example.replace('\n', " ").replace('\r', " "),
            group.count
        ));
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

fn json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch <= '\u{1f}' => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Shows a query-construction error in the same channels as a report error.
fn report_error(
    diagnostics: &[&Diagnostic],
    sources: &Sources,
    terminals: Terminals,
    all: bool,
    json: bool,
) -> Outcome {
    if json {
        return Outcome {
            answer: axiom_report::json::diagnostics(diagnostics, sources),
            diagnostics: String::new(),
            failed: true,
        };
    }
    let (mut text, tally) = Renderer::new(sources, terminals.err).present(diagnostics, all);
    if let Some(line) = tally.line() {
        text += &terminals.err.painter.paint(&[line]);
    }
    Outcome {
        answer: String::new(),
        diagnostics: text,
        failed: true,
    }
}

/// Presents a no-write sync plan, and applies its changes only when the user
/// did not ask for `--dry`.
fn render_sync(
    planned: PlanOutcome,
    sources: &mut Sources,
    root: &Path,
    dry: bool,
    terminals: Terminals,
    prior: &[Diagnostic],
) -> Outcome {
    let blocked_by_book_errors = prior.iter().any(Diagnostic::is_error);
    let write_problems = if dry || blocked_by_book_errors {
        Vec::new()
    } else {
        apply_changes(root, &planned.changes)
    };
    let mut diagnostics: Vec<&Diagnostic> = prior
        .iter()
        .chain(&planned.problems)
        .chain(&planned.incomplete)
        .collect();
    for source in &planned.sources {
        match source.failure.as_ref() {
            Some(SourceFailure::Read(problem) | SourceFailure::Generated(problem)) => {
                diagnostics.push(problem);
            }
            Some(SourceFailure::Output(problems)) => diagnostics.extend(problems),
            Some(SourceFailure::Command(_)) | None => {}
        }
    }
    diagnostics.extend(&write_problems);

    let mut answer = String::new();
    for source in &planned.sources {
        let mut line = match &source.failure {
            None => Line::text("✓ ", Ink::GREEN.bold()),
            Some(_) => Line::text("✗ ", Ink::RED.bold()),
        };
        line.push(&source.path, Ink::BOLD);
        line.push("  ", Ink::PLAIN);
        match &source.failure {
            None if source.added == 0 => line.push("no new items", Ink::DIM),
            None => line.push(
                &format!("{} added", plural(source.added, "item")),
                Ink::PLAIN,
            ),
            Some(SourceFailure::Command(failure)) => {
                line.push(&failure.summary, Ink::RED);
                if !failure.stderr.trim().is_empty() {
                    answer.push_str(&failure.stderr);
                    if !failure.stderr.ends_with('\n') {
                        answer.push('\n');
                    }
                }
            }
            Some(
                SourceFailure::Read(_) | SourceFailure::Generated(_) | SourceFailure::Output(_),
            ) => {
                line.push("see diagnostics", Ink::RED);
            }
        }
        answer.push_str(&terminals.out.painter.paint(&[line]));
    }

    if dry {
        for change in &planned.changes {
            answer.push_str(&change_diff(change));
        }
    } else if blocked_by_book_errors && !planned.changes.is_empty() {
        answer.push_str("sync changes were not applied because the project has errors\n");
    } else {
        if write_problems.is_empty() {
            for change in &planned.changes {
                answer.push_str(&format!("updated {}\n", change.path));
            }
        }
    }
    if planned.sources.is_empty() && planned.changes.is_empty() {
        answer.push_str("no sync sources selected\n");
    } else if planned.changes.is_empty() {
        answer.push_str("no changes\n");
    }

    let (diagnostic_text, tally) =
        Renderer::new(sources, terminals.err).present(&diagnostics, true);
    let failed = tally.errors > 0
        || !write_problems.is_empty()
        || planned
            .sources
            .iter()
            .any(|source| source.failure.is_some());
    Outcome {
        answer,
        diagnostics: diagnostic_text,
        failed,
    }
}

/// Writes planned targets through sibling temporary files. The canonical
/// parent check catches symlinked folders that would otherwise leave the
/// project, and rename replaces a final symlink without following it.
fn apply_changes(root: &Path, changes: &[Change]) -> Vec<Diagnostic> {
    let canonical_root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) => {
            return vec![Diagnostic::error(
                "sync-project-root",
                format!("cannot resolve the project root: {error}"),
            )];
        }
    };
    let mut problems = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        let relative = Path::new(&change.path);
        if change.path.is_empty()
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            problems.push(Diagnostic::error(
                "sync-path-outside-project",
                format!("`{}` is not a project-relative path", change.path),
            ));
            continue;
        }
        let target = canonical_root.join(relative);
        let Some(parent) = target.parent() else {
            problems.push(Diagnostic::error(
                "sync-path-outside-project",
                format!("`{}` has no project directory", change.path),
            ));
            continue;
        };
        // Check the nearest existing ancestor before creating missing folders.
        // Otherwise `link/new-folder/file.ax` could create `new-folder` outside
        // the project before the final-parent containment check noticed `link`.
        let existing = match canonical_existing_ancestor(parent) {
            Ok(existing) if existing.starts_with(&canonical_root) => existing,
            Ok(_) => {
                problems.push(Diagnostic::error(
                    "sync-path-outside-project",
                    format!("`{}` leaves the project through a symlink", change.path),
                ));
                continue;
            }
            Err(error) => {
                problems.push(Diagnostic::error(
                    "sync-write",
                    format!("cannot resolve the folder for `{}`: {error}", change.path),
                ));
                continue;
            }
        };
        let _ = existing;
        if let Err(error) = fs::create_dir_all(parent) {
            problems.push(Diagnostic::error(
                "sync-write",
                format!("cannot create the folder for `{}`: {error}", change.path),
            ));
            continue;
        }
        let canonical_parent = match fs::canonicalize(parent) {
            Ok(parent) if parent.starts_with(&canonical_root) => parent,
            Ok(_) => {
                problems.push(Diagnostic::error(
                    "sync-path-outside-project",
                    format!("`{}` leaves the project through a symlink", change.path),
                ));
                continue;
            }
            Err(error) => {
                problems.push(Diagnostic::error(
                    "sync-write",
                    format!("cannot resolve the folder for `{}`: {error}", change.path),
                ));
                continue;
            }
        };
        let Some(name) = target.file_name() else {
            continue;
        };
        let target = canonical_parent.join(name);
        let mut temporary = None;
        for suffix in 0..100 {
            let candidate = canonical_parent.join(format!(
                ".{}.axiom-sync-{}-{index}-{suffix}",
                name.to_string_lossy(),
                std::process::id(),
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(change.after.as_bytes()) {
                        let _ = fs::remove_file(&candidate);
                        problems.push(Diagnostic::error(
                            "sync-write",
                            format!("cannot write `{}`: {error}", change.path),
                        ));
                    } else {
                        temporary = Some(candidate);
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    problems.push(Diagnostic::error(
                        "sync-write",
                        format!("cannot prepare `{}`: {error}", change.path),
                    ));
                    break;
                }
            }
        }
        let Some(temporary) = temporary else {
            continue;
        };
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            problems.push(Diagnostic::error(
                "sync-write",
                format!("cannot replace `{}`: {error}", change.path),
            ));
        }
    }
    problems
}

/// Resolves the closest existing ancestor, following any symlink in its path.
fn canonical_existing_ancestor(path: &Path) -> std::io::Result<std::path::PathBuf> {
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => return fs::canonicalize(ancestor),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

/// A compact unified hunk: the common prefix and suffix stay context, and the
/// changed middle is shown once, regardless of how much text the target has.
fn change_diff(change: &Change) -> String {
    let before = change.before.as_deref().unwrap_or("");
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = change.after.split_inclusive('\n').collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len().saturating_sub(prefix)
        && suffix < new.len().saturating_sub(prefix)
        && old[old.len() - suffix - 1] == new[new.len() - suffix - 1]
    {
        suffix += 1;
    }
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    let start = prefix.saturating_sub(1);
    let old_count = old_end.saturating_sub(start) + usize::from(suffix > 0);
    let new_count = new_end.saturating_sub(start) + usize::from(suffix > 0);
    let mut out = format!(
        "--- a/{}\n+++ b/{}\n@@ -{},{} +{},{} @@\n",
        change.path,
        change.path,
        start + 1,
        old_count,
        start + 1,
        new_count,
    );
    if start < prefix {
        out.push(' ');
        out.push_str(old[start]);
        if !old[start].ends_with('\n') {
            out.push('\n');
        }
    }
    for line in &old[prefix..old_end] {
        out.push('-');
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
    }
    for line in &new[prefix..new_end] {
        out.push('+');
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
    }
    if suffix > 0 {
        out.push(' ');
        out.push_str(old[old.len() - suffix]);
        if !old[old.len() - suffix].ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// The current day, by the system clock, in UTC.
fn system_today() -> Day {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    Day((seconds / 86_400) as i32)
}

/// A book that has been run, and everything found on the way.
struct Session<'a, 's> {
    book: &'a Book<'s>,
    run: &'a Run,
    sources: &'a Sources,
    /// From parsing, building, and running.
    diagnostics: Vec<&'a Diagnostic>,
    terminals: Terminals,
    /// Show every diagnostic, however many.
    all: bool,
    /// Machine-readable rendering is independent of terminal width and colour.
    json: bool,
}

impl Session<'_, '_> {
    /// Every diagnostic; and if none is an error, the book in one line.
    fn check(&self, suggestions: &[MemoSuggestion]) -> Outcome {
        let (diagnostics, tally) = self.show(&self.diagnostics);
        if self.json {
            let mut answer = diagnostics;
            answer.push_str(&json_suggestions(suggestions));
            return Outcome {
                answer,
                diagnostics: String::new(),
                failed: tally.errors > 0,
            };
        }
        if tally.errors > 0 {
            let mut diagnostics = diagnostics;
            diagnostics.push_str(&self.suggestions(suggestions, self.terminals.err));
            return Outcome {
                answer: String::new(),
                diagnostics,
                failed: true,
            };
        }
        let summary = axiom_report::summary(self.book, self.run);
        let mut answer = self
            .terminals
            .out
            .painter
            .paint(&[summary_line(self.book, &summary)]);
        answer.push_str(&self.suggestions(suggestions, self.terminals.out));
        Outcome {
            answer,
            diagnostics,
            failed: false,
        }
    }

    fn suggestions(
        &self,
        suggestions: &[MemoSuggestion],
        terminal: crate::style::Terminal,
    ) -> String {
        let lines = suggestion_lines(suggestions);
        if lines.is_empty() {
            return String::new();
        }
        let lines = lines
            .iter()
            .map(|suggestion| Line::text(suggestion, Ink::DIM))
            .collect::<Vec<_>>();
        terminal.painter.paint(&lines)
    }

    /// The errors, and the report. A report runs whatever the book's errors, so
    /// that a reader can investigate them; it says at its head what it rests on.
    fn report(&self, context: &Context<'_, '_>, query: &Query<'_>) -> Outcome {
        let result = context.report_with_sources(query, self.sources);
        let mut shown: Vec<&Diagnostic> = self
            .diagnostics
            .iter()
            .copied()
            .filter(|found| found.is_error())
            .collect();
        shown.extend(result.as_ref().err());
        let (diagnostics, tally) = self.show(&shown);
        let Ok(report) = result else {
            return if self.json {
                Outcome {
                    answer: diagnostics,
                    diagnostics: String::new(),
                    failed: true,
                }
            } else {
                Outcome {
                    answer: String::new(),
                    diagnostics,
                    failed: true,
                }
            };
        };
        if self.json {
            return Outcome {
                answer: JsonRenderer.render(&report, self.sources),
                diagnostics,
                failed: tally.errors > 0,
            };
        }
        let mut answer = table::TableRenderer {
            terminal: self.terminals.out,
        }
        .render(&report, self.sources);
        if tally.errors > 0 {
            let caveat = format!(
                "rests on a book with {} (`axiom check` lists them): what they touch may be wrong",
                plural(tally.errors, "error")
            );
            let mut line = Line::text("✗ ", Ink::RED.bold());
            line.push(&caveat, Ink::DIM);
            answer.insert_str(0, &self.terminals.out.painter.paint(&[line, Line::new()]));
        }
        Outcome {
            answer,
            diagnostics,
            failed: tally.errors > 0,
        }
    }

    /// The diagnostics, and after them how many of each there were.
    fn show(&self, diagnostics: &[&Diagnostic]) -> (String, Tally) {
        if self.json {
            return (
                crate::render::json::diagnostics(diagnostics, self.sources),
                Tally::of(diagnostics.iter().copied()),
            );
        }
        let (mut text, tally) =
            Renderer::new(self.sources, self.terminals.err).present(diagnostics, self.all);
        if let Some(line) = tally.line() {
            text += &self.terminals.err.painter.paint(&[line]);
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
        line.push(
            &format!(" · {} unpriced", plural(summary.unpriced, "holding")),
            Ink::YELLOW,
        );
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use crate::testing::TempDir;

    #[test]
    fn check_reads_local_memos_and_never_runs_declared_commands() {
        let dir = TempDir::new("check-memos");
        let marker = dir.path().join("command-ran");
        dir.write("axiom.ax", "base USD\ncommodity USD\n  precision 2\n");
        dir.write(
            "sync.ax",
            &format!(
                "sync checking\n  read \"imports/*.csv\"\n  format csv\n    memo \"Description\"\nsync prices\n  run touch {}\n  into prices.ax\n",
                marker.display()
            ),
        );
        dir.write(
            "imports/card.csv",
            "Description\nTRADER JOE'S #12\nTrader Joe's #13\n",
        );

        let project = Project::find(dir.path()).unwrap();
        let mut sources = project.load().unwrap();
        let (parsed, mut diagnostics) = Sources::parse_files(&sources.files);
        let (book, built) = axiom_model::build(&parsed);
        drop(parsed);
        diagnostics.extend(built);
        assert!(
            diagnostics.iter().all(|problem| !problem.is_error()),
            "fixture has no model errors: {diagnostics:?}"
        );

        let (read_problems, suggestions) =
            check_memos(&book, &project, &sources.files, &mut sources.auxiliary);
        assert!(read_problems.is_empty(), "{read_problems:?}");
        assert!(suggestions.iter().any(|group| group.count == 2));
        assert!(
            suggestions
                .iter()
                .any(|group| group.known_as == "known-as \"TRADER JOE'S\"")
        );
        assert!(
            !marker.exists(),
            "check must never execute a declared run command"
        );
    }

    #[test]
    fn sync_dry_shows_a_change_without_writing_it() {
        let dir = TempDir::new("sync-dry");
        dir.write("axiom.ax", "base USD\n");
        dir.write("prices.ax", "old\n");
        let project = Project::find(dir.path()).unwrap();
        let mut sources = project.load().unwrap();
        let outcome = render_sync(
            PlanOutcome {
                sources: Vec::new(),
                changes: vec![Change {
                    path: "prices.ax".to_owned(),
                    before: Some("old\n".to_owned()),
                    after: "new\n".to_owned(),
                }],
                problems: Vec::new(),
                incomplete: Vec::new(),
                generated: Vec::new(),
            },
            &mut sources,
            &project.root,
            true,
            Terminals {
                out: crate::style::Terminal::plain(80),
                err: crate::style::Terminal::plain(80),
            },
            &[],
        );

        assert!(!outcome.failed);
        assert!(outcome.answer.contains("-old\n"));
        assert!(outcome.answer.contains("+new\n"));
        assert_eq!(
            fs::read_to_string(dir.path().join("prices.ax")).unwrap(),
            "old\n"
        );
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
        let mut sources = project.load().unwrap();
        let outcome = render_sync(
            PlanOutcome {
                sources: Vec::new(),
                changes: vec![Change {
                    path: "link/new-folder/prices.ax".to_owned(),
                    before: None,
                    after: "new\n".to_owned(),
                }],
                problems: Vec::new(),
                incomplete: Vec::new(),
                generated: Vec::new(),
            },
            &mut sources,
            &project.root,
            false,
            Terminals {
                out: crate::style::Terminal::plain(80),
                err: crate::style::Terminal::plain(80),
            },
            &[],
        );

        assert!(outcome.failed);
        assert!(!outside.path().join("new-folder").exists());
    }

    #[test]
    fn sync_does_not_apply_changes_when_the_loaded_book_has_errors() {
        let dir = TempDir::new("sync-invalid-book");
        dir.write("axiom.ax", "base USD\n");
        dir.write("prices.ax", "old\n");
        let project = Project::find(dir.path()).unwrap();
        let mut sources = project.load().unwrap();
        let invalid_book = Diagnostic::error("invalid-book", "the project has a model error");
        let outcome = render_sync(
            PlanOutcome {
                sources: Vec::new(),
                changes: vec![Change {
                    path: "prices.ax".to_owned(),
                    before: Some("old\n".to_owned()),
                    after: "new\n".to_owned(),
                }],
                problems: Vec::new(),
                incomplete: Vec::new(),
                generated: Vec::new(),
            },
            &mut sources,
            &project.root,
            false,
            Terminals {
                out: crate::style::Terminal::plain(80),
                err: crate::style::Terminal::plain(80),
            },
            &[invalid_book],
        );

        assert!(outcome.failed);
        assert!(
            outcome
                .answer
                .contains("not applied because the project has errors")
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("prices.ax")).unwrap(),
            "old\n"
        );
    }
}
