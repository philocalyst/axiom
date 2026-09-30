//! `axiom sync`: the sources a project declares, run by the `sync` crate.
//! What they would write is shown as a diff (`--dry`), or written file by file,
//! each only if it still parses.

use std::fs;
use std::path::{Component, Path};
use std::time::Duration;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, FileId};
use axiom_model::Book;
use axiom_sync::{Change, Env, Failure, Input, Kind, Layout, Patterns, Recognizer, Sink, Source, World, sync};

use crate::Outcome;
use crate::project::Sources;
use crate::render::Renderer;
use crate::style::{Ink, Line, Terminal};
use crate::text::plural;

/// How long a command may run before it is killed.
const TIMEOUT: Duration = Duration::from_secs(60);

/// A failed command's stderr is shown up to this many lines.
const STDERR_LINES: usize = 10;

/// One declared `sync`: what it is called, and the command that feeds it.
struct Declared<'a> {
    name: &'a str,
    command: &'a str,
}

/// Runs the declared syncs (only those for `wanted` names, if any are given) and
/// reports each.
pub fn execute(
    book: &Book,
    wanted: &[&str],
    dry: bool,
    files: &Sources,
    root: &Path,
    today: Day,
    terminal: Terminal,
) -> Result<Outcome, Diagnostic> {
    let declared =
        book.syncs.iter().map(|sync| Declared { name: book.name(sync.file), command: book.name(sync.run) }).collect();
    let chosen = choose(declared, wanted)?;
    let first = book.flows.values().map(|flow| flow.day).min().unwrap_or(today);
    let sources: Vec<Source> = chosen
        .iter()
        .map(|sync| Source {
            name: sync.name,
            input: Input::Run(sync.command),
            since: first,
            kind: Kind::Sink(Sink::File(sync.name)),
        })
        .collect();
    let held: Vec<&str> = book
        .commodities
        .iter()
        .filter(|(id, _)| *id != book.base && !book.assets.values().any(|asset| asset.unit == *id))
        .map(|(_, commodity)| book.name(commodity.symbol))
        .collect();
    let paths: Vec<&str> =
        (0..).map_while(|id| files.get(FileId(id))).filter(|file| !file.embedded).map(|file| &*file.path).collect();
    let mut world = World {
        recognizer: Recognizer::new(Vec::new(), &[], &Patterns::default())
            .unwrap_or_else(|_| unreachable!("no patterns, none can be wrong")),
        layout: Layout::new(paths),
        accounts: Default::default(),
        units: Vec::new(),
        dues: Vec::new(),
        claims: Default::default(),
    };
    let env = Env { root, today, units: &held, timeout: TIMEOUT };
    let read = |path: &str| {
        files.find(path).map(|file| file.text.to_string()).or_else(|| fs::read_to_string(root.join(path)).ok())
    };
    let outcome = sync(&mut world, &sources, &env, &read);
    Ok(report(&outcome.sources, &outcome.changes, dry, root, terminal))
}

/// The declared syncs for the `wanted` names, or all of them if none are named.
fn choose<'a>(declared: Vec<Declared<'a>>, wanted: &[&str]) -> Result<Vec<Declared<'a>>, Diagnostic> {
    let same = |declared: &str, name: &str| declared.trim_start_matches("./") == name.trim_start_matches("./");
    if let Some(unknown) = wanted.iter().find(|&&name| !declared.iter().any(|sync| same(sync.name, name))) {
        let error = Diagnostic::error("", format!("no sync is declared for `{unknown}`"));
        return Err(match closest(unknown, declared.iter().map(|sync| sync.name)) {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error,
        });
    }
    Ok(declared
        .into_iter()
        .filter(|sync| wanted.is_empty() || wanted.iter().any(|&name| same(sync.name, name)))
        .collect())
}

/// One line per source, `✓` or `✗`, and under a failure what it has to say; then
/// each file's diff, or that it was written.
fn report(
    sources: &[(String, Result<usize, Failure>)],
    changes: &[Change],
    dry: bool,
    root: &Path,
    terminal: Terminal,
) -> Outcome {
    if sources.is_empty() {
        return Outcome::ok(terminal.painter.paint(&[Line::text("no sync is declared in this project", Ink::DIM)]));
    }
    let paint = |lines: &[Line]| terminal.painter.paint(lines);
    let width = sources.iter().map(|(name, _)| name.chars().count()).max().unwrap_or(0);
    let mut text = String::new();
    let mut failed = false;
    for (name, result) in sources {
        let (mark, ink) = if result.is_ok() { ("✓ ", Ink::GREEN) } else { ("✗ ", Ink::RED) };
        let mut line = Line::text(mark, ink.bold());
        line.push(name, Ink::BOLD);
        line.pad_to(2 + width + 2);
        match result {
            Ok(0) => line.push("nothing new", Ink::DIM),
            Ok(added) => line.push(&format!("{} added", plural(*added, "line")), Ink::PLAIN),
            Err(failure) => line.push(&failure_summary(failure), Ink::RED),
        }
        text += &paint(&[line]);
        if let Err(failure) = result {
            failed = true;
            text += &failure_details(name, failure, terminal);
        }
    }
    for change in changes.iter().filter(|change| !change.diff().is_empty()) {
        if dry {
            text += &paint(&[Line::new()]);
            text += &diff_lines(&change.diff(), terminal);
        } else {
            match write(change, root, terminal) {
                Ok(()) => text += &paint(&[Line::text(&format!("wrote {}", change.path), Ink::DIM)]),
                Err(refusal) => {
                    failed = true;
                    text += &refusal;
                }
            }
        }
    }
    Outcome { answer: text, diagnostics: String::new(), failed }
}

fn failure_summary(failure: &Failure) -> String {
    match failure {
        Failure::Command(failed) => failed.summary.clone(),
        Failure::Output { problems, .. } => {
            let errors = problems.iter().filter(|problem| problem.is_error()).count();
            format!("what it gave is not usable ({})", plural(errors, "problem"))
        }
    }
}

/// What backs up a failure: what the command said, or the problems in what it
/// gave, drawn against it.
fn failure_details(name: &str, failure: &Failure, terminal: Terminal) -> String {
    match failure {
        Failure::Command(failed) => {
            let said: Vec<&str> = failed.stderr.lines().collect();
            let mut lines: Vec<Line> =
                said.iter().take(STDERR_LINES).map(|line| Line::text(&format!("    {line}"), Ink::DIM)).collect();
            if said.len() > STDERR_LINES {
                lines.push(Line::text(
                    &format!("    … and {} more", plural(said.len() - STDERR_LINES, "line")),
                    Ink::DIM,
                ));
            }
            terminal.painter.paint(&lines)
        }
        Failure::Output { text, problems } => {
            let sources = Sources::single(name.to_string(), text.clone());
            let shown = Renderer::new(&sources, terminal).present(&problems.iter().collect::<Vec<_>>(), false).0;
            format!("\n{shown}")
        }
    }
}

/// A unified diff, additions in green.
fn diff_lines(diff: &str, terminal: Terminal) -> String {
    let ink = |line: &str| match line.chars().next() {
        Some('+') if !line.starts_with("+++") => Ink::GREEN,
        Some('-') if !line.starts_with("---") => Ink::RED,
        Some('@') => Ink::CYAN,
        _ => Ink::PLAIN,
    };
    terminal.painter.paint(&diff.lines().map(|line| Line::text(line, ink(line))).collect::<Vec<_>>())
}

/// Puts the change in place: inside the project, and only if what results still
/// parses. What is refused is drawn.
fn write(change: &Change, root: &Path, terminal: Terminal) -> Result<(), String> {
    let refuse = |why: String, details: String| {
        let mut line = Line::text("✗ ", Ink::RED.bold());
        line.push(&change.path, Ink::BOLD);
        line.push(&format!("  {why}"), Ink::RED);
        Err(terminal.painter.paint(&[line]) + &details)
    };
    let inside = Path::new(&change.path).components().all(|part| matches!(part, Component::Normal(_)));
    if change.path.is_empty() || !inside {
        return refuse("the file must be inside the project".into(), String::new());
    }
    let (_, problems) = axiom_syntax::parse(FileId(0), &change.after);
    if problems.iter().any(Diagnostic::is_error) {
        let sources = Sources::single(change.path.clone(), change.after.clone());
        let shown = Renderer::new(&sources, terminal).present(&problems.iter().collect::<Vec<_>>(), false).0;
        let errors = problems.iter().filter(|problem| problem.is_error()).count();
        return refuse(format!("it would not parse ({})", plural(errors, "error")), format!("\n{shown}"));
    }
    let target = root.join(&change.path);
    let put = || -> std::io::Result<()> {
        fs::create_dir_all(target.parent().unwrap_or(root))?;
        // Written beside the file and renamed, so that it is there whole or not at all.
        let scratch = target.with_extension("ax.sync-tmp");
        fs::write(&scratch, &change.after)?;
        fs::rename(&scratch, &target)
    };
    put().or_else(|error| refuse(format!("could not be written: {error}"), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    fn declared<'a>(name: &'a str, command: &'a str) -> Declared<'a> {
        Declared { name, command }
    }

    /// The source, run in `dir`: what `axiom sync` would report and write.
    fn run(dir: &TempDir, sources: &[Source], dry: bool) -> Outcome {
        let mut world = World {
            recognizer: Recognizer::new(Vec::new(), &[], &Patterns::default()).unwrap(),
            layout: Layout::new(["prices/2026.ax"]),
            accounts: Default::default(),
            units: Vec::new(),
            dues: Vec::new(),
            claims: Default::default(),
        };
        let env = Env { root: dir.path(), today: Day(20_000), units: &[], timeout: TIMEOUT };
        let read = |path: &str| fs::read_to_string(dir.path().join(path)).ok();
        let outcome = sync(&mut world, sources, &env, &read);
        report(&outcome.sources, &outcome.changes, dry, dir.path(), Terminal::plain(100))
    }

    fn feeding<'a>(file: &'a str, command: &'a str) -> Source<'a> {
        Source { name: file, input: Input::Run(command), since: Day(0), kind: Kind::Sink(Sink::File(file)) }
    }

    const PRINT: &str = "printf '2026-01-01 checking -> food 4 USD\\n2026-01-02 checking -> food 5 USD\\n'";

    #[test]
    fn the_output_is_merged_into_the_file_and_a_second_sync_writes_nothing() {
        let dir = TempDir::new("sync-merges");
        dir.write("prices.ax", "2026-01-01 checking -> food 4 USD\n");
        let outcome = run(&dir, &[feeding("prices.ax", PRINT)], false);
        assert!(!outcome.failed, "{}", outcome.answer);
        assert_eq!(outcome.answer, "✓ prices.ax  1 line added\nwrote prices.ax\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("prices.ax")).unwrap(),
            "2026-01-01 checking -> food 4 USD\n2026-01-02 checking -> food 5 USD\n"
        );
        let again = run(&dir, &[feeding("prices.ax", PRINT)], false);
        assert_eq!(again.answer, "✓ prices.ax  nothing new\n");
    }

    #[test]
    fn a_new_file_in_a_new_folder_is_created() {
        let dir = TempDir::new("sync-creates");
        let outcome = run(&dir, &[feeding("a/b/new.ax", PRINT)], false);
        assert!(!outcome.failed, "{}", outcome.answer);
        assert_eq!(
            fs::read_to_string(dir.path().join("a/b/new.ax")).unwrap(),
            "2026-01-01 checking -> food 4 USD\n2026-01-02 checking -> food 5 USD\n"
        );
    }

    #[test]
    fn a_dry_run_shows_the_diff_and_writes_nothing() {
        let dir = TempDir::new("sync-dry");
        let outcome = run(&dir, &[feeding("prices.ax", PRINT)], true);
        assert_eq!(
            outcome.answer,
            "✓ prices.ax  2 lines added\n\n--- /dev/null\n+++ b/prices.ax\n@@ -0,0 +1,2 @@\n+2026-01-01 checking -> food 4 USD\n+2026-01-02 checking -> food 5 USD\n"
        );
        assert!(!dir.path().join("prices.ax").exists());
    }

    #[test]
    fn output_that_does_not_read_is_shown_against_what_was_printed_and_nothing_is_written() {
        let dir = TempDir::new("sync-invalid");
        dir.write("prices.ax", "old\n");
        let outcome = run(&dir, &[feeding("prices.ax", "printf 'this is not axiom\\n'")], false);
        assert!(outcome.failed);
        assert!(
            outcome.answer.starts_with("✗ prices.ax  what it gave is not usable (1 problem)\n"),
            "{}",
            outcome.answer
        );
        assert!(outcome.answer.contains("the output has a line that does not start with a date"), "{}", outcome.answer);
        assert!(
            outcome.answer.contains("[prices.ax:1:") && outcome.answer.contains("this is not axiom"),
            "{}",
            outcome.answer
        );
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
    }

    #[test]
    fn a_result_that_would_not_parse_is_refused_and_a_file_outside_the_project_too() {
        let dir = TempDir::new("sync-refused");
        dir.write("prices.ax", "2026-01-01 checking -> food 4 USD\n");
        // The printed line reads on its own, but not under the file's own text.
        let outcome = run(
            &dir,
            &[feeding("prices.ax", "printf '2026-01-02 this is nonsense\\n'"), feeding("../x.ax", PRINT)],
            false,
        );
        assert!(outcome.failed);
        assert!(outcome.answer.contains("✗ prices.ax  it would not parse (1 error)"), "{}", outcome.answer);
        assert!(outcome.answer.contains("✗ ../x.ax  the file must be inside the project"), "{}", outcome.answer);
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "2026-01-01 checking -> food 4 USD\n");
        assert!(!dir.path().parent().unwrap().join("x.ax").exists());
    }

    #[test]
    fn a_failing_command_shows_what_it_said_and_leaves_the_folder_as_it_was() {
        let dir = TempDir::new("sync-fails");
        dir.write("prices.ax", "old\n");
        let outcome = run(&dir, &[feeding("prices.ax", "echo partial; echo 'no network' >&2; exit 3")], false);
        assert!(outcome.failed);
        assert_eq!(outcome.answer, "✗ prices.ax  the command failed (exit status: 3)\n    no network\n");
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
        let mut names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["prices.ax"]);
    }

    #[test]
    fn a_named_source_selects_its_job_and_a_near_miss_is_suggested() {
        let all = || vec![declared("prices/2026.ax", "a"), declared("statements.ax", "b")];
        let names = |chosen: Vec<Declared>| chosen.iter().map(|sync| sync.name.to_string()).collect::<Vec<_>>();
        assert_eq!(names(choose(all(), &[]).unwrap()), ["prices/2026.ax", "statements.ax"]);
        assert_eq!(names(choose(all(), &["./statements.ax"]).unwrap()), ["statements.ax"]);
        let error = choose(all(), &["statments.ax"]).err().unwrap();
        assert_eq!(error.message, "no sync is declared for `statments.ax`");
        assert_eq!(error.help[0].text, "did you mean `statements.ax`?");
    }

    #[test]
    fn the_report_says_what_became_of_each_source() {
        use axiom_sync::Failed;
        let failed = |summary: &str, stderr: &str| {
            Err(Failure::Command(Failed { summary: summary.into(), stderr: stderr.into() }))
        };
        let sources = vec![
            ("prices/2026.ax".to_string(), Ok(312)),
            (
                "statements.ax".to_string(),
                failed("the command failed (exit status: 3)", "login expired\nsee `axiom help`"),
            ),
            ("checking".to_string(), Ok(0)),
        ];
        let outcome = report(&sources, &[], false, Path::new("."), Terminal::plain(100));
        assert!(outcome.failed);
        assert_eq!(
            outcome.answer,
            "\
✓ prices/2026.ax  312 lines added
✗ statements.ax   the command failed (exit status: 3)
    login expired
    see `axiom help`
✓ checking        nothing new
"
        );
    }
}
