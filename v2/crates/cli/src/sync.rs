//! `axiom sync`: run the scripts a project declares, and keep what they print
//! if it is valid Axiom.
//!
//! ```text
//! sync prices/2026.ax
//!   run ./scripts/quotes.py VTI BND
//! ```
//!
//! Each script runs as `sh -c COMMAND` in the project root, all at once. Its
//! output goes straight to a scratch file beside the destination, so a script
//! that prints a great deal cannot stall on a full pipe, one that hangs can be
//! killed, and a valid result is put in place with one atomic rename. The
//! destination is never touched unless the output parses.

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, FileId};
use axiom_model::Book;

use crate::Outcome;
use crate::project::Sources;
use crate::render::Renderer;
use crate::style::{Ink, Line, Terminal};
use crate::text::plural;

/// How long a script may run before it is killed.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How often a running script is checked on.
const POLL: Duration = Duration::from_millis(20);

/// A script's stderr is shown up to this many lines when it fails.
const STDERR_LINES: usize = 10;

/// One `sync FILE` with its `run COMMAND`.
struct Job<'a> {
    file: &'a str,
    command: &'a str,
}

/// Why a file was not written.
enum Failure {
    /// The declared path leaves the project.
    Outside,
    /// Something went wrong with the file system or the process, while `doing`
    /// what the sentence "could not …" completes.
    Io {
        doing: &'static str,
        error: io::Error,
    },
    Exited {
        status: ExitStatus,
        stderr: String,
    },
    TimedOut(Duration),
    /// What the script printed is not valid Axiom.
    Invalid {
        sources: Sources,
        diagnostics: Vec<Diagnostic>,
    },
}

/// Runs the declared syncs (only those for `wanted` files, if any are named)
/// and reports each.
pub fn execute(book: &Book, wanted: &[&str], root: &Path, terminal: Terminal) -> Result<Outcome, Diagnostic> {
    let declared =
        book.syncs.iter().map(|sync| Job { file: book.name(sync.file), command: book.name(sync.run) }).collect();
    let jobs = choose(declared, wanted)?;
    let results = run_all(&jobs, root, TIMEOUT);
    Ok(report(&jobs, &results, terminal))
}

/// The declared jobs for the `wanted` files, or all of them if none are named.
fn choose<'a>(jobs: Vec<Job<'a>>, wanted: &[&str]) -> Result<Vec<Job<'a>>, Diagnostic> {
    let same = |declared: &str, name: &str| declared.trim_start_matches("./") == name.trim_start_matches("./");
    if let Some(unknown) = wanted.iter().find(|&&name| !jobs.iter().any(|job| same(job.file, name))) {
        let error = Diagnostic::error("", format!("no sync is declared for `{unknown}`"));
        return Err(match closest(unknown, jobs.iter().map(|job| job.file)) {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error,
        });
    }
    Ok(jobs.into_iter().filter(|job| wanted.is_empty() || wanted.iter().any(|&name| same(job.file, name))).collect())
}

/// Every job at once, each result in the order of `jobs`. `Ok` is how many
/// items were written.
fn run_all(jobs: &[Job], root: &Path, timeout: Duration) -> Vec<Result<usize, Failure>> {
    thread::scope(|scope| {
        let workers: Vec<_> = jobs.iter().map(|job| scope.spawn(move || sync_file(job, root, timeout))).collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
            .collect()
    })
}

fn sync_file(job: &Job, root: &Path, timeout: Duration) -> Result<usize, Failure> {
    let target = destination(root, job.file)?;
    let scratch = Scratch::beside(&target).map_err(io_error("prepare the folder"))?;
    let status = execute_script(job.command, root, &scratch, timeout)?;
    if !status.success() {
        return Err(Failure::Exited { status, stderr: scratch.stderr() });
    }
    let text = fs::read_to_string(&scratch.stdout).map_err(io_error("read the output"))?;
    let (file, diagnostics) = axiom_syntax::parse(FileId(0), &text);
    let items = file.items.len();
    drop(file);
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(Failure::Invalid { sources: Sources::single(job.file.to_string(), text), diagnostics });
    }
    fs::rename(&scratch.stdout, &target).map_err(io_error("write the file"))?;
    Ok(items)
}

fn io_error(doing: &'static str) -> impl FnOnce(io::Error) -> Failure {
    move |error| Failure::Io { doing, error }
}

/// Where `file` goes: inside the project, and no way out of it.
fn destination(root: &Path, file: &str) -> Result<PathBuf, Failure> {
    let relative = Path::new(file);
    let stays_inside = relative.components().all(|part| matches!(part, Component::Normal(_)));
    if file.is_empty() || !stays_inside {
        return Err(Failure::Outside);
    }
    Ok(root.join(relative))
}

/// Runs `command` to completion, or kills it once `timeout` has passed.
fn execute_script(command: &str, root: &Path, scratch: &Scratch, timeout: Duration) -> Result<ExitStatus, Failure> {
    let stdout = File::create(&scratch.stdout).map_err(io_error("keep the output"))?;
    let stderr = File::create(&scratch.stderr).map_err(io_error("keep the output"))?;
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(io_error("start the command"))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(io_error("wait for the command"))? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            // Killing an already-finished child is not an error worth reporting.
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::TimedOut(timeout));
        }
        thread::sleep(POLL);
    }
}

/// Where a script's output lands while it runs, next to the file it is for so
/// that moving it into place is a rename within one folder. Removed when
/// dropped, whatever became of the script.
struct Scratch {
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Scratch {
    fn beside(target: &Path) -> io::Result<Scratch> {
        let folder = target.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(folder)?;
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        Ok(Scratch {
            stdout: folder.join(format!(".{name}.sync-out")),
            stderr: folder.join(format!(".{name}.sync-err")),
        })
    }

    fn stderr(&self) -> String {
        fs::read(&self.stderr).map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string()).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stdout);
        let _ = fs::remove_file(&self.stderr);
    }
}

/// One line per file, `✓` or `✗`, and under a failure what it has to say.
fn report(jobs: &[Job], results: &[Result<usize, Failure>], terminal: Terminal) -> Outcome {
    if jobs.is_empty() {
        return Outcome::ok(
            Line::text("no sync is declared in this project", Ink::DIM).render(terminal.painter) + "\n",
        );
    }
    let width = jobs.iter().map(|job| job.file.chars().count()).max().unwrap_or(0);
    let mut text = String::new();
    for (job, result) in jobs.iter().zip(results) {
        let (mark, ink) = if result.is_ok() { ("✓ ", Ink::GREEN) } else { ("✗ ", Ink::RED) };
        let mut line = Line::text(mark, ink.bold());
        line.push(job.file, Ink::BOLD);
        line.pad_to(2 + width + 2);
        match result {
            Ok(items) => line.push(&format!("{} written", plural(*items, "item")), Ink::PLAIN),
            Err(failure) => line.push(&failure.summary(), Ink::RED),
        }
        text.push_str(&line.render(terminal.painter));
        text.push('\n');
        if let Err(failure) = result {
            text.push_str(&failure.details(terminal));
        }
    }
    Outcome { text, failed: results.iter().any(Result::is_err) }
}

impl Failure {
    fn summary(&self) -> String {
        match self {
            Failure::Outside => "the file must be inside the project".to_string(),
            Failure::Io { doing, error } => format!("could not {doing}: {error}"),
            Failure::Exited { status, .. } => format!("the command failed ({status})"),
            Failure::TimedOut(limit) => {
                format!("the command took more than {} to finish", plural(limit.as_secs() as usize, "second"))
            }
            Failure::Invalid { diagnostics, .. } => format!(
                "the output is not valid Axiom ({})",
                plural(diagnostics.iter().filter(|found| found.is_error()).count(), "error")
            ),
        }
    }

    /// What the script said, or what is wrong with what it printed.
    fn details(&self, terminal: Terminal) -> String {
        match self {
            Failure::Exited { stderr, .. } => {
                let shown = stderr.lines().take(STDERR_LINES);
                let more = stderr.lines().count().saturating_sub(STDERR_LINES);
                let mut lines: Vec<String> = shown.map(|line| format!("    {line}")).collect();
                if more > 0 {
                    lines.push(format!("    … and {} more", plural(more, "line")));
                }
                lines.iter().map(|line| Line::text(line, Ink::DIM).render(terminal.painter) + "\n").collect()
            }
            Failure::Invalid { sources, diagnostics } => {
                let found: Vec<&Diagnostic> = diagnostics.iter().collect();
                format!("\n{}", Renderer::new(sources, terminal).diagnostics(&found))
            }
            Failure::Outside | Failure::Io { .. } | Failure::TimedOut(_) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;
    use crate::testing::TempDir;

    fn job<'a>(file: &'a str, command: &'a str) -> Job<'a> {
        Job { file, command }
    }

    fn files_in(dir: &TempDir) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_destination_stays_inside_the_project() {
        let root = Path::new("/project");
        assert_eq!(destination(root, "prices/2026.ax").ok(), Some(PathBuf::from("/project/prices/2026.ax")));
        for outside in ["../x.ax", "/etc/x.ax", "a/../../x.ax", ""] {
            assert!(matches!(destination(root, outside), Err(Failure::Outside)), "{outside:?}");
        }
    }

    #[test]
    fn valid_output_replaces_the_file() {
        let dir = TempDir::new("sync-writes");
        dir.write("prices/2026.ax", "old\n");
        let script = "printf '2026-01-01 checking -> food 4 USD\\n2026-01-02 checking -> food 5 USD\\n'";
        let results = run_all(&[job("prices/2026.ax", script)], dir.path(), TIMEOUT);
        assert!(matches!(results[0], Ok(2)));
        let written = fs::read_to_string(dir.path().join("prices/2026.ax")).unwrap();
        assert_eq!(written, "2026-01-01 checking -> food 4 USD\n2026-01-02 checking -> food 5 USD\n");
        assert_eq!(files_in(&dir), ["prices"]);
    }

    #[test]
    fn a_new_file_in_a_new_folder_is_created() {
        let dir = TempDir::new("sync-creates");
        let results =
            run_all(&[job("a/b/new.ax", "printf '2026-01-01 checking -> food 4 USD\\n'")], dir.path(), TIMEOUT);
        assert!(matches!(results[0], Ok(1)));
        assert!(dir.path().join("a/b/new.ax").is_file());
    }

    #[test]
    fn output_that_does_not_parse_is_shown_against_what_was_printed_and_not_written() {
        let dir = TempDir::new("sync-invalid");
        dir.write("prices.ax", "old\n");
        let jobs = [job("prices.ax", "printf 'this is not axiom\\n'")];
        let results = run_all(&jobs, dir.path(), TIMEOUT);
        assert!(matches!(results[0], Err(Failure::Invalid { .. })));
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
        assert_eq!(files_in(&dir), ["prices.ax"]);

        let outcome = report(&jobs, &results, Terminal::plain(100));
        assert!(outcome.failed);
        assert!(outcome.text.starts_with("✗ prices.ax  the output is not valid Axiom ("), "{}", outcome.text);
        assert!(outcome.text.contains("[prices.ax:1:"), "{}", outcome.text);
        assert!(outcome.text.contains("this is not axiom"), "{}", outcome.text);
    }

    #[test]
    fn a_failing_script_leaves_the_file_and_the_folder_as_they_were() {
        let dir = TempDir::new("sync-fails");
        dir.write("prices.ax", "old\n");
        let results = run_all(&[job("prices.ax", "echo partial; echo 'no network' >&2; exit 3")], dir.path(), TIMEOUT);
        let Err(Failure::Exited { status, stderr }) = &results[0] else { panic!("the script fails") };
        assert_eq!((status.code(), stderr.as_str()), (Some(3), "no network"));
        assert_eq!(fs::read_to_string(dir.path().join("prices.ax")).unwrap(), "old\n");
        assert_eq!(files_in(&dir), ["prices.ax"]);
    }

    #[test]
    fn a_script_that_hangs_is_killed() {
        let dir = TempDir::new("sync-hangs");
        let started = Instant::now();
        let results = run_all(&[job("out.ax", "sleep 30")], dir.path(), Duration::from_millis(200));
        assert!(matches!(results[0], Err(Failure::TimedOut(_))));
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(files_in(&dir).is_empty());
    }

    #[test]
    fn scripts_run_together_in_the_project_root() {
        let dir = TempDir::new("sync-parallel");
        let mut jobs: Vec<Job> = (0..4).map(|_| job("out.ax", "sleep 0.5; exit 1")).collect();
        jobs.push(job("where.ax", "pwd > cwd.txt; exit 1"));
        let started = Instant::now();
        let results = run_all(&jobs, dir.path(), TIMEOUT);
        assert_eq!(results.len(), 5);
        assert!(
            started.elapsed() < Duration::from_millis(1800),
            "four half-second scripts took {:?}",
            started.elapsed()
        );
        let cwd = fs::read_to_string(dir.path().join("cwd.txt")).unwrap();
        assert_eq!(fs::canonicalize(cwd.trim()).unwrap(), fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn a_named_file_selects_its_job_and_a_near_miss_is_suggested() {
        let declared = || vec![job("prices/2026.ax", "a"), job("statements.ax", "b")];
        let names = |jobs: Vec<Job>| jobs.iter().map(|job| job.file.to_string()).collect::<Vec<_>>();
        assert_eq!(names(choose(declared(), &[]).unwrap()), ["prices/2026.ax", "statements.ax"]);
        assert_eq!(names(choose(declared(), &["./statements.ax"]).unwrap()), ["statements.ax"]);
        let error = choose(declared(), &["statments.ax"]).err().unwrap();
        assert_eq!(error.message, "no sync is declared for `statments.ax`");
        assert_eq!(error.help[0].text, "did you mean `statements.ax`?");
    }

    #[test]
    fn the_report_says_what_became_of_each_file() {
        let jobs = [job("prices/2026.ax", ""), job("statements.ax", ""), job("elsewhere.ax", ""), job("slow.ax", "")];
        let results = vec![
            Ok(312),
            Err(Failure::Exited {
                status: ExitStatus::from_raw(3 << 8),
                stderr: "login expired\nsee `axiom help`".to_string(),
            }),
            Err(Failure::Outside),
            Err(Failure::TimedOut(TIMEOUT)),
        ];
        let outcome = report(&jobs, &results, Terminal::plain(100));
        assert!(outcome.failed);
        assert_eq!(
            outcome.text,
            "\
✓ prices/2026.ax  312 items written
✗ statements.ax   the command failed (exit status: 3)
    login expired
    see `axiom help`
✗ elsewhere.ax    the file must be inside the project
✗ slow.ax         the command took more than 60 seconds to finish
"
        );
    }
}
