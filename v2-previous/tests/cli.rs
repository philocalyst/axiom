use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_axiom-v2")
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("axiom-v2 process starts")
}

fn successful_output(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("CLI output is UTF-8")
}

fn id_line(output: &str, prefix: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing {prefix:?} line in output: {output}"))
        .trim()
        .to_owned()
}

#[test]
fn check_reports_ambiguous_lot_from_source_with_semantic_exit_status() {
    let path = example("unresolved_lot.axm");
    let output = Command::new(binary())
        .arg("check")
        .arg(path)
        .output()
        .expect("axiom-v2 process starts");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("CLI output is UTF-8");
    assert!(stdout.contains("sale_1"), "{stdout}");
    assert!(stdout.contains("needs a decision"), "{stdout}");
    assert!(stdout.contains("lot_1"), "{stdout}");
    assert!(stdout.contains("decide"), "{stdout}");
    assert!(output.stderr.is_empty());
}

#[test]
fn why_traces_source_span_and_explicit_decision() {
    let path = example("corrected_ledger.axm");
    let output = successful_output(run(&[
        "why",
        path.to_str().expect("example path is UTF-8"),
        "sale_beta_ambiguous",
    ]));

    assert!(
        output.contains("source occurrence sale_beta_ambiguous at line"),
        "{output}"
    );
    assert!(output.contains("decision choose_beta_lot"), "{output}");
}

#[test]
fn why_marks_omitted_dated_entry_fields_as_pattern_inference() {
    let directory = tempfile::tempdir().expect("temporary source directory");
    let source_path = directory.path().join("dated_buy.axm");
    let source = "ledger ordinary\nuse personal\n\n2026-05-01 buy lot_1\n  account brokerage_primary\n  units 5 FUND\n  cost 50 EUR\n";
    std::fs::write(&source_path, source).expect("write dated entry");
    assert!(!source.contains("fees"));

    let output = successful_output(run(&[
        "why",
        source_path.to_str().expect("source path is UTF-8"),
        "lot_1",
    ]));
    assert!(
        output.contains("source occurrence lot_1 at line"),
        "{output}"
    );
    assert!(
        output.contains("inferred field fees = 0 EUR (entry pattern)"),
        "{output}"
    );
}

#[test]
fn why_ambiguous_subject_shows_candidate_source_occurrences() {
    let path = example("unresolved_lot.axm");
    let output = run(&[
        "why",
        path.to_str().expect("example path is UTF-8"),
        "sale_1",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("CLI output is UTF-8");
    assert!(stdout.contains("candidate lot_1"), "{stdout}");
    assert!(stdout.contains("candidate lot_2"), "{stdout}");
    assert!(
        stdout.contains("source occurrence lot_1 at line"),
        "{stdout}"
    );
    assert!(
        stdout.contains("source occurrence lot_2 at line"),
        "{stdout}"
    );
}

#[test]
fn commit_close_show_and_verify_survive_process_restarts() {
    let directory = tempfile::tempdir().expect("temporary repository directory");
    let repo = directory.path().to_str().expect("temp path is UTF-8");
    let source = example("clean_ledger.axm");
    let source = source.to_str().expect("example path is UTF-8");

    let committed = successful_output(run(&["commit", source, "--repo", repo]));
    let revision = id_line(&committed, "revision ");

    let closed = successful_output(run(&[
        "close",
        &revision,
        "tax",
        "--repo",
        repo,
        "--from",
        "2026-01-01",
        "--to",
        "2026-12-31",
    ]));
    let close = id_line(&closed, "close ");

    let shown = successful_output(run(&["show", &close, "--repo", repo]));
    assert!(shown.contains(&format!("revision {revision}")), "{shown}");
    assert!(shown.contains("book tax"), "{shown}");

    let history = successful_output(run(&["history", "--repo", repo]));
    assert!(
        history.contains(&format!("revision {revision}")),
        "{history}"
    );
    assert!(history.contains(&format!("close {close}")), "{history}");
    assert!(history.contains("book tax"), "{history}");

    let verified = successful_output(run(&["verify", "--repo", repo]));
    assert!(verified.contains("repository verified"), "{verified}");
}

#[test]
fn command_rejects_unknown_options_as_usage_errors() {
    let output = run(&[
        "check",
        example("clean_ledger.axm")
            .to_str()
            .expect("example path is UTF-8"),
        "--model-extra",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn source_errors_show_the_named_line_without_guessing_a_token_span() {
    let directory = tempfile::tempdir().expect("temporary source directory");
    let source_path = directory.path().join("bad_source.axm");
    std::fs::write(
        &source_path,
        "ledger bad_source\nuse personal\n\n2026-01-01 buy lot_1\n  account brokerage\n  units 1 FUND\n  cost 2 USD\n  mystery value\n",
    )
    .expect("write invalid source");
    let output = run(&["check", source_path.to_str().expect("source path is UTF-8")]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("CLI error is UTF-8");
    assert!(stderr.contains("unknown field"), "{stderr}");
    assert!(stderr.contains("bad_source.axm"), "{stderr}");
    assert!(stderr.contains("mystery value"), "{stderr}");
}

#[test]
fn no_color_keeps_success_output_plain_and_deterministic() {
    let path = example("clean_ledger.axm");
    let path = path.to_str().expect("example path is UTF-8");
    let plain = successful_output(run(&["check", path]));
    let explicit_plain = successful_output(run(&["check", path, "--no-color"]));
    assert_eq!(plain, explicit_plain);
    assert!(!plain.contains('\u{1b}'));
}

#[test]
fn help_is_self_contained_and_does_not_require_a_file() {
    let output = successful_output(run(&["--help"]));
    for command in [
        "check FILE",
        "view FILE BOOK",
        "why FILE SUBJECT",
        "packages",
        "commit FILE",
        "close REV_ID BOOK",
        "show CLOSE_ID",
        "restate CLOSE_ID REV_ID",
        "history --repo DIR",
        "verify --repo DIR",
    ] {
        assert!(output.contains(command), "help omits {command}: {output}");
    }
    assert!(output.contains("--model FILE"));
    assert!(output.contains("--no-color"));
    assert!(output.contains("2026-01-04 buy first_purchase"));
    assert!(output.contains("A dated header supplies the `date` field"));
    assert!(output.contains("`KIND ID` plus"));
}

#[test]
fn model_option_can_add_multiple_explicit_packages() {
    let directory = tempfile::tempdir().expect("temporary package directory");
    let first = directory.path().join("first.axm");
    let second = directory.path().join("second.axm");
    std::fs::write(&first, "package extra.first\nuse personal\n").expect("write first package");
    std::fs::write(&second, "package extra.second\nuse personal\n").expect("write second package");

    let output = successful_output(run(&[
        "packages",
        "--model",
        first.to_str().expect("package path is UTF-8"),
        "--model",
        second.to_str().expect("package path is UTF-8"),
    ]));
    assert!(output.contains("package extra.first"), "{output}");
    assert!(output.contains("package extra.second"), "{output}");
    assert!(output.contains("entry patterns"), "{output}");
    assert!(!output.contains("schemas"), "{output}");
}
