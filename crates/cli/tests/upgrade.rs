//! `axiom fmt --upgrade` changes how a book is spelled and never what it says.
//!
//! `tests/v4-syntax/examples` holds the examples as v4 wrote them. Each is upgraded in a copy, and two things are
//! shown: the upgrade of it is the example as it is written now, and the one through every view the v4 text and its
//! upgrade give the same answer, down to each diagnostic and each figure. Positions are the one thing allowed to differ,
//! because the upgrade lays a file out again, and so is the warning that a file is written the v4 way, which the upgrade
//! is for.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the examples are asked: the checks, the balance sheet at cost, the flows, the claims and a year's tax.
const VIEWS: [&[&str]; 6] = [
    &["check", "--json", "--all"],
    &["balance", "--json"],
    &["balance", "--value", "--monthly", "--json"],
    &["flow", "--json"],
    &["claims", "--json"],
    &["tax", "2025", "--json"],
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the workspace root")
}

/// The files under `dir` (or `dir` itself, when it is one), relative to it.
fn files(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, at: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(at).expect("a folder").flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => walk(base, &path, found),
                false => found.push(path.strip_prefix(base).expect("under the base").to_path_buf()),
            }
        }
    }
    let mut found = Vec::new();
    match dir.is_dir() {
        true => walk(dir, dir, &mut found),
        false => found.push(PathBuf::new()),
    }
    found.sort();
    found
}

fn copy(from: &Path, to: &Path) {
    for file in files(from) {
        let (source, target) = if file.as_os_str().is_empty() {
            (from.to_path_buf(), to.to_path_buf())
        } else {
            (from.join(&file), to.join(&file))
        };
        fs::create_dir_all(target.parent().expect("a parent")).expect("a folder");
        fs::copy(source, target).expect("a copy");
    }
}

/// What axiom writes for `args` on the project at `path`: its answer and what it says besides, without the warning that a
/// file is written the v4 way and with every position zeroed.
fn asked(path: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_axiom"))
        .args(args)
        .args(["--today", "2026-03-31", "--color", "never", "-C"])
        .arg(path)
        .output()
        .expect("axiom runs");
    let said = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let kept: Vec<&str> = said.lines().filter(|line| !line.contains("\"code\":\"v4-syntax\"")).collect();
    // A clean book has nothing to check, and the other views always have a table to write.
    assert!(!kept.is_empty() || args[0] == "check", "`axiom {}` said nothing, so it shows nothing", args.join(" "));
    without_positions(&kept.join("\n"))
}

/// `json` with every number that says where in a file something is written as 0.
fn without_positions(json: &str) -> String {
    const WHERE: [&str; 7] = [
        "\"start_byte\":",
        "\"end_byte\":",
        "\"line\":",
        "\"column\":",
        "\"end_line\":",
        "\"end_column\":",
        "\"file_id\":",
    ];
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some((at, key)) = WHERE.iter().filter_map(|key| rest.find(key).map(|at| (at, key))).min() {
        let (head, tail) = rest.split_at(at + key.len());
        out.push_str(head);
        out.push('0');
        rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

fn upgraded_examples_say_what_the_v4_ones_do(names: &[&str]) {
    let (v4, now) = (root().join("tests/v4-syntax/examples"), root().join("examples"));
    let scratch = std::env::temp_dir().join(format!("axiom-upgrade-it-{}-{}", std::process::id(), names[0]));
    let _ = fs::remove_dir_all(&scratch);
    for name in names {
        let (before, after) = (scratch.join("v4").join(name), scratch.join("upgraded").join(name));
        copy(&v4.join(name), &before);
        copy(&v4.join(name), &after);
        let upgrade = Command::new(env!("CARGO_BIN_EXE_axiom"))
            .args(["fmt", "--upgrade", "--color", "never", "-C"])
            .arg(&after)
            .output()
            .expect("axiom runs");
        assert!(upgrade.status.success(), "{name}: {}", String::from_utf8_lossy(&upgrade.stderr));
        for file in files(&v4.join(name)) {
            let (written, upgraded) = (now.join(name).join(&file), after.join(&file));
            let (written, upgraded) =
                if file.as_os_str().is_empty() { (now.join(name), after.clone()) } else { (written, upgraded) };
            assert_eq!(
                fs::read_to_string(&upgraded).expect("an upgraded file"),
                fs::read_to_string(&written).expect("an example"),
                "{name}/{}: the upgrade of the v4 text is not the example as it is written now",
                file.display()
            );
        }
        for view in VIEWS {
            assert_eq!(
                asked(&before, view),
                asked(&after, view),
                "{name}: `axiom {}` says another thing",
                view.join(" ")
            );
        }
    }
    let _ = fs::remove_dir_all(scratch);
}

#[test]
fn the_small_examples_say_the_same_upgraded() {
    upgraded_examples_say_what_the_v4_ones_do(&[
        "01-first-steps.ax",
        "03-violations.ax",
        "02-household",
        "10-budgeter",
    ]);
}

#[test]
fn the_freelancer_and_the_family_say_the_same_upgraded() {
    upgraded_examples_say_what_the_v4_ones_do(&["04-freelancer", "05-family"]);
}

#[test]
fn the_investor_and_the_landlord_say_the_same_upgraded() {
    upgraded_examples_say_what_the_v4_ones_do(&["06-investor", "07-landlord"]);
}

#[test]
fn the_expat_the_shared_house_and_sam_say_the_same_upgraded() {
    upgraded_examples_say_what_the_v4_ones_do(&["08-expat", "09-shared", "11-sam"]);
}

#[test]
fn an_upgrade_of_an_upgrade_changes_nothing() {
    let now = root().join("examples");
    let scratch = std::env::temp_dir().join(format!("axiom-upgrade-twice-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    for name in ["04-freelancer", "05-family", "06-investor"] {
        let copied = scratch.join(name);
        copy(&now.join(name), &copied);
        let output = Command::new(env!("CARGO_BIN_EXE_axiom"))
            .args(["fmt", "--upgrade", "--check", "--color", "never", "-C"])
            .arg(&copied)
            .output()
            .expect("axiom runs");
        assert!(
            output.status.success(),
            "{name} is not as an upgrade leaves it:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    let _ = fs::remove_dir_all(scratch);
}
