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
    for command in
        ["check", "balance", "register", "flow", "available", "budget", "tax", "lots", "forecast", "why", "sync"]
    {
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
    assert!(text(&outside.stderr).starts_with("\x1b[1;31merror\x1b[0m"), "{:?}", text(&outside.stderr));
}

#[test]
fn outside_a_project_there_is_nothing_to_run_on() {
    let folder = empty_folder("outside");
    let output = axiom(&["check"]).current_dir(&folder).output().expect("axiom runs");
    assert_eq!(output.status.code(), Some(2));
    let message = text(&output.stderr);
    assert!(message.starts_with("error: no axiom.ax in "), "{message}");
    assert!(message.contains("run axiom inside a project"), "{message}");
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn a_missing_path_is_reported_not_panicked() {
    let output = run(&["check", "/no/such/place"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).starts_with("error: cannot open /no/such/place"));
}
