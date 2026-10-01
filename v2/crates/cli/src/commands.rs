//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::{Options, Run};
use axiom_model::{Book, sync::Fetch};
use axiom_report::{Context, Query, ReportRenderer, Summary, json::JsonRenderer};

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

    if let Command::Sync { names, dry } = command {
        return sync::execute(&book, names, &project.root, terminals.out, *dry);
    }
    let options = Options {
        today: invocation.today.unwrap_or_else(system_today),
        relaxed: invocation.relaxed,
    };
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

fn source_by_id(files: &[SourceFile], auxiliary: &[SourceFile], id: FileId) -> Option<&SourceFile> {
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
}
