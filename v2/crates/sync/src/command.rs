//! Running a source's command: from the project root, with `{since}`,
//! `{today}` and `{units}` filled in, every source at once. Axiom opens no
//! connection of its own; whatever a command reaches, it reaches itself.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use axiom_core::{Day, par};

/// How often a running command is checked on.
const POLL: Duration = Duration::from_millis(20);

/// Why a command's output is not to be used, and what it said on the way.
#[derive(Debug)]
pub struct Failed {
    pub summary: String,
    pub stderr: String,
}

impl Failed {
    fn new(summary: impl Into<String>) -> Failed {
        Failed { summary: summary.into(), stderr: String::new() }
    }
}

/// `template` with `{since}`, `{today}` and `{units}` (the commodities held,
/// separated by spaces) replaced.
pub fn substitute(template: &str, since: Day, today: Day, units: &[&str]) -> String {
    template
        .replace("{since}", &since.to_string())
        .replace("{today}", &today.to_string())
        .replace("{units}", &units.join(" "))
}

/// Every command at once, each result in the order of `commands`.
pub fn run_all(commands: &[String], root: &Path, timeout: Duration) -> Vec<Result<String, Failed>> {
    par::map_each(commands, |command| run(command, root, timeout))
}

/// What the command printed, once it has succeeded. Its output goes to files,
/// so that one that prints a great deal cannot stall on a full pipe and one
/// that hangs can be killed without waiting on what it left behind.
fn run(command: &str, root: &Path, timeout: Duration) -> Result<String, Failed> {
    let scratch = Scratch::new();
    let files = (File::create(&scratch.stdout), File::create(&scratch.stderr));
    let (Ok(stdout), Ok(stderr)) = files else { return Err(Failed::new("could not keep the command's output")) };
    let mut child = Command::new("sh")
        .args(["-c", command])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|error| Failed::new(format!("could not start the command: {error}")))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(Failed::new(format!("could not wait for the command: {error}"))),
        }
        if started.elapsed() >= timeout {
            // Killing a child that has just finished is not worth reporting.
            let _ = child.kill();
            let _ = child.wait();
            let limit = match timeout.as_secs() {
                0 => format!("{} milliseconds", timeout.as_millis()),
                seconds => format!("{seconds} seconds"),
            };
            return Err(Failed::new(format!("the command took more than {limit} to finish")));
        }
        thread::sleep(POLL);
    };
    if !status.success() {
        let stderr = String::from_utf8_lossy(&fs::read(&scratch.stderr).unwrap_or_default()).trim().to_string();
        return Err(Failed { summary: format!("the command failed ({status})"), stderr });
    }
    fs::read_to_string(&scratch.stdout)
        .map_err(|_| Failed::new("the command printed something that is not text (UTF-8)"))
}

/// Where a command's output lands while it runs; removed when dropped, whatever
/// became of the command.
struct Scratch {
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!("axiom-sync-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let folder = std::env::temp_dir();
        Scratch { stdout: folder.join(format!("{name}.out")), stderr: folder.join(format!("{name}.err")) }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stdout);
        let _ = fs::remove_file(&self.stderr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(60);

    fn run_one(command: &str, root: &Path, timeout: Duration) -> Result<String, Failed> {
        run_all(&[command.to_string()], root, timeout).pop().expect("one result")
    }

    #[test]
    fn placeholders_are_filled_in() {
        let (since, today) = (Day::parse(b"2026-03-01").unwrap(), Day::parse(b"2026-03-31").unwrap());
        let command =
            substitute("quotes {units} --since {since} --until {today} {since}", since, today, &["VTI", "VXUS"]);
        assert_eq!(command, "quotes VTI VXUS --since 2026-03-01 --until 2026-03-31 2026-03-01");
        assert_eq!(substitute("echo {a,b} {year}", since, today, &[]), "echo {a,b} {year}");
    }

    #[test]
    fn a_command_runs_in_the_project_root_and_its_output_is_the_answer() {
        let root = std::env::current_dir().unwrap();
        let output = run_one("pwd; printf 'two\\nlines\\n'", &root, LONG).unwrap();
        let mut lines = output.lines();
        let printed = lines.next().unwrap();
        assert_eq!(fs::canonicalize(printed).unwrap(), fs::canonicalize(&root).unwrap());
        assert_eq!(lines.collect::<Vec<_>>(), ["two", "lines"]);
    }

    #[test]
    fn a_failing_command_shows_what_it_said_and_a_lot_of_output_does_not_stall_it() {
        let root = std::env::temp_dir();
        let failed = run_one("echo partial; echo 'no network' >&2; exit 3", &root, LONG).unwrap_err();
        assert_eq!(
            (failed.summary.as_str(), failed.stderr.as_str()),
            ("the command failed (exit status: 3)", "no network")
        );
        let big = run_one("head -c 5000000 /dev/zero | tr '\\0' x", &root, LONG).unwrap();
        assert_eq!(big.len(), 5_000_000);
        let unreadable = run_one("printf '\\377\\376'", &root, LONG).unwrap_err();
        assert!(unreadable.summary.contains("not text"), "{}", unreadable.summary);
    }

    #[test]
    fn a_command_that_hangs_is_killed_and_the_others_still_run_together() {
        let root = std::env::temp_dir();
        let started = Instant::now();
        let hung = run_one("sleep 30", &root, Duration::from_millis(200)).unwrap_err();
        assert_eq!(hung.summary, "the command took more than 200 milliseconds to finish");
        assert!(started.elapsed() < Duration::from_secs(10));
        let commands: Vec<String> = (0..4).map(|n| format!("sleep 0.4; echo {n}")).collect();
        let started = Instant::now();
        let outputs = run_all(&commands, &root, LONG);
        let printed: Vec<_> = outputs.into_iter().map(|output| output.unwrap()).collect();
        assert_eq!(printed, ["0\n", "1\n", "2\n", "3\n"]);
        let cores = thread::available_parallelism().map_or(1, |n| n.get());
        let waves = 4usize.div_ceil(cores.min(4)) as f32;
        assert!(started.elapsed().as_secs_f32() < 0.4 * waves + 1.0, "{:?}", started.elapsed());
    }
}
