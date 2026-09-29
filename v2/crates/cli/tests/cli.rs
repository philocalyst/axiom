//! The `axiom` binary as a user meets it: what it prints and how it exits.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn axiom(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_axiom"));
    command.args(args).env_remove("NO_COLOR").env_remove("COLUMNS");
    command
}

fn run(args: &[&str]) -> Output {
    axiom(args).output().expect("axiom runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("axiom prints UTF-8")
}

/// An empty folder that is not inside any project.
fn empty_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("axiom-cli-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).expect("create folder");
    folder
}

#[test]
fn help_lists_the_commands_and_exits_cleanly() {
    let output = run(&["help"]);
    assert_eq!(output.status.code(), Some(0));
    let screen = text(&output.stdout);
    for command in [
        "check",
        "balance",
        "register",
        "flow",
        "available",
        "budget",
        "limits",
        "claims",
        "tax",
        "gains",
        "lots",
        "forecast",
        "why",
        "sync",
    ] {
        assert!(screen.contains(command), "{command} is missing from\n{screen}");
    }
    assert!(!screen.contains('\x1b'), "a pipe gets no colour");
}

#[test]
fn no_arguments_show_the_help_too() {
    assert_eq!(text(&run(&[]).stdout), text(&run(&["--help"]).stdout));
}

#[test]
fn a_typo_is_a_usage_error_with_a_suggestion() {
    let output = run(&["balnce"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(text(&output.stderr), "error: unknown command `balnce`\n  = help: did you mean `balance`?\n");
}

#[test]
fn colour_is_only_added_when_asked_for() {
    let coloured = text(&run(&["--color", "always", "help"]).stdout);
    assert!(coloured.contains("\x1b[1m"), "{coloured:?}");
    let plain = text(&run(&["--color", "never", "help"]).stdout);
    assert!(!plain.contains('\x1b'));
    let outside = axiom(&["--color=always", "check"]).current_dir(empty_folder("colour")).output().expect("axiom runs");
    assert!(text(&outside.stderr).starts_with("\x1b[1;31merror[no-project]\x1b[0m"), "{:?}", text(&outside.stderr));
}

#[test]
fn outside_a_project_there_is_nothing_to_run_on() {
    let folder = empty_folder("outside");
    let output = axiom(&["check"]).current_dir(&folder).output().expect("axiom runs");
    assert_eq!(output.status.code(), Some(2));
    let message = text(&output.stderr);
    assert!(message.starts_with("error[no-project]: no axiom.ax in "), "{message}");
    assert!(message.contains("create an `axiom.ax`"), "{message}");
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn a_missing_path_is_reported_not_panicked() {
    let output = run(&["check", "/no/such/place"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).starts_with("error[unreadable]: cannot open /no/such/place"));
}

/// A project with a journal that has one typo in it.
fn project_with_a_typo(name: &str) -> PathBuf {
    let folder = empty_folder(name);
    let setup = "base USD\nuse std\naccount assets/checking : bank\naccount expenses/food\n";
    fs::write(folder.join("axiom.ax"), setup).unwrap();
    let journal = "2026-01-02 income/salary -> checking 1_000 USD\n2026-01-08 checking -> food 84.20 USD\n2026-01-09 checking -> fod 5 USD\n";
    fs::write(folder.join("journal.ax"), journal).unwrap();
    folder
}

#[test]
fn a_report_runs_on_a_book_with_errors_and_says_what_it_rests_on() {
    let folder = project_with_a_typo("report-anyway");
    let output = axiom(&["balance", "--today", "2026-02-01"]).current_dir(&folder).output().expect("axiom runs");
    assert_eq!(output.status.code(), Some(1));
    let (answer, problems) = (text(&output.stdout), text(&output.stderr));
    assert!(problems.starts_with("error[unknown-place]: there is no place `fod`"), "{problems}");
    assert!(problems.ends_with("✗ 1 error\n"), "{problems}");
    assert!(answer.starts_with("✗ rests on a book with 1 error"), "{answer}");
    assert!(answer.contains("Balances at"), "{answer}");
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn a_file_that_is_not_utf8_is_named_with_its_line() {
    let folder = project_with_a_typo("not-utf8");
    fs::write(folder.join("prices.ax"), b"2026-01-01 VTI 285.70 USD\n2026-01-02 \xff\n").unwrap();
    let output = axiom(&["check"]).current_dir(&folder).output().expect("axiom runs");
    assert_eq!(output.status.code(), Some(2));
    let problem = text(&output.stderr);
    assert!(problem.starts_with("error[not-utf8]: cannot read prices.ax: line 2 is not valid UTF-8"), "{problem}");
    let _ = fs::remove_dir_all(folder);
}
