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
        "fmt",
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

#[test]
fn a_balance_before_an_accepted_gap_does_not_see_it() {
    let folder = empty_folder("gap-after-day");
    let book = "base USD\nuse std\naccount assets/cash : cash\nopening 2025-01-01\n  cash 100 USD\n2025-06-30 cash = 90 USD !\n";
    fs::write(folder.join("axiom.ax"), book).expect("write project");
    let project = folder.to_str().expect("a UTF-8 path");
    let before = run(&["-C", project, "--today", "2025-12-31", "balance", "--at", "2025-03-01"]);
    assert_eq!(before.status.code(), Some(0), "{}", text(&before.stderr));
    assert!(text(&before.stdout).contains("100.00 USD"), "{}", text(&before.stdout));
    let after = run(&["-C", project, "--today", "2025-12-31", "balance", "--at", "2025-07-01"]);
    assert!(text(&after.stdout).contains("90.00 USD"), "{}", text(&after.stdout));
}

/// Two years under `us`, the first with more of a capital loss than one year may deduct.
const LOSSES: &str = "\
base USD
use std
use us

entity me : person
  born 1988-04-12
  filing single
  lives us

commodity STK : stock
  precision 0

account assets/checking : bank
account assets/broker : broker

opening 2024-01-01
  checking 50_000 USD

// 2025: 4,000 USD lost on shares held over a year, 5,000 USD on shares held less.
2024-02-01 checking -> broker 100 STK @ 100 USD
2025-03-01 checking -> broker 100 STK @ 100 USD
2025-06-01 broker 100 STK -> checking 6_000 USD
2025-09-01 broker 100 STK -> checking 5_000 USD
// 2026: 1,000 USD gained.
2025-12-01 checking -> broker 10 STK @ 100 USD
2026-03-01 broker 10 STK -> checking 2_000 USD
";

#[test]
fn a_net_capital_loss_beyond_the_limit_is_carried_into_the_next_years_return() {
    let folder = empty_folder("loss-carryforward");
    fs::write(folder.join("axiom.ax"), LOSSES).expect("write project");
    let project = folder.to_str().expect("a UTF-8 path");
    // The lines of the return about losses and income, as `name amount USD`.
    let lines = |year: &str| -> Vec<String> {
        let output = run(&["-C", project, "--today", "2027-04-16", "--color", "never", "tax", year]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        let screen = text(&output.stdout);
        let names = ["short-loss-carried", "long-loss-carried", "agi"];
        let named = |line: &&str| names.iter().any(|name| line.trim().starts_with(name));
        let cells = |line: &str| line.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        screen.lines().filter(named).map(cells).collect()
    };
    // 9,000 USD lost: 3,000 is deducted, from the short-term loss first, and the rest waits.
    let first = ["short-loss-carried 2,000.00 USD", "long-loss-carried 4,000.00 USD", "agi -3,000.00 USD"];
    assert_eq!(lines("2025"), first);
    // The next year's 1,000 USD of gain meets the 2,000 USD short-term loss, then the long-term one:
    // 5,000 USD is lost again, 3,000 deducted, and only long-term loss is left to carry.
    assert_eq!(lines("2026"), ["long-loss-carried 2,000.00 USD", "agi -3,000.00 USD"]);
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn check_writes_a_line_of_json_for_each_diagnostic_with_its_fix_as_an_edit() {
    let folder = project_with_a_typo("check-json");
    let output = axiom(&["check", "--json"]).current_dir(&folder).output().expect("axiom runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
    let answer = text(&output.stdout);
    let [line] = answer.lines().collect::<Vec<_>>()[..] else { panic!("one diagnostic, one line: {answer}") };
    let expected = concat!(
        r#"{"code":"unknown-place","severity":"error","disposition":"open","#,
        r#""headline":"there is no place `fod`","message":"there is no place `fod`","#,
        r#""labels":[{"file":"journal.ax","line":3,"column":24,"end_line":3,"end_column":27,"#,
        r#""text":"not a known place","primary":true}],"notes":[],"helps":["did you mean `food`?"],"#,
        r#""fixes":[{"help":"did you mean `food`?","edits":[{"file":"journal.ax","line":3,"column":24,"#,
        r#""end_line":3,"end_column":27,"replacement":"food"}]}]}"#
    );
    assert_eq!(line, expected);
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn a_view_written_as_json_has_the_sections_a_reader_sees_and_its_figures_as_facts() {
    let folder = empty_folder("report-json");
    let book = "base USD\nuse std\naccount assets/cash : cash\nopening 2025-01-01\n  cash 100 USD\n";
    fs::write(folder.join("axiom.ax"), book).expect("write project");
    let output = axiom(&["balance", "cash", "--json", "--today", "2025-03-01"]).current_dir(&folder).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let answer = text(&output.stdout);
    assert!(answer.contains(r#""title":["Balances at",{"day":"2025-03-01"}]"#), "{answer}");
    let row = r#"{"style":"normal","depth":1,"cells":["cash",{"amount":"100.00","unit":"USD"}]}"#;
    assert!(answer.contains(row), "{answer}");
    let fact = r#"{"concept":"balance","of":"assets/cash","entity":"everyone","period":{"instant":"2025-03-01"},"unit":"USD","value":"100.00"}"#;
    assert!(answer.contains(fact), "{answer}");
    assert_eq!(answer.lines().count(), 1, "one document");
    let _ = fs::remove_dir_all(folder);
}

#[test]
fn fmt_names_a_file_that_is_not_there_and_leaves_a_formatted_project_alone() {
    let folder = project_with_a_typo("fmt");
    let project = folder.to_str().expect("a UTF-8 path");
    let asked = run(&["-C", project, "fmt", "--check", "journal.ax"]);
    assert_eq!((asked.status.code(), asked.stdout.len()), (Some(0), 0));
    let stray = run(&["-C", project, "fmt", "jurnal.ax"]);
    assert_eq!(stray.status.code(), Some(2));
    assert_eq!(
        text(&stray.stderr),
        "error[unknown-file]: no source file named `jurnal.ax`\n  = help: did you mean `journal.ax`?\n"
    );
    let _ = fs::remove_dir_all(folder);
}
