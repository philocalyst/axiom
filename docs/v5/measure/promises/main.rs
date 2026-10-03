//! What the contract machinery says of every contract of a project, as text, so that two builds can be compared, and,
//! with `--check` and the `new` feature, whether the compiled promise says what the old machinery does.
//!
//! The driver is `docs/v5/measure/contracts.py`, which builds this for a tree of the workspace (it is not a member: the
//! driver writes a Cargo.toml with path dependencies on `core`, `syntax`, `model`, `engine` and `systems` of the tree)
//! and runs it over generated projects. Each contract is asked, by a `Reading`, the questions the fold, the lowering and
//! the reports ask of it:
//!
//! | line      | the question                                                                                       |
//! |-----------|----------------------------------------------------------------------------------------------------|
//! | `due`     | the days due in a window, both schedules merged, each with its schedule                            |
//! | `ordinal` | which due day of its own schedule an occurrence is, counted from the contract's first day          |
//! | `keep`    | which due day a line dated on a day keeps (`nearest_occurrence`), or none, or that it is ambiguous  |
//! | `factor`  | `amount_on_schedule`: the escalation and the proration of a day, or why there is none              |
//! | `recog`   | `recognition_on_schedule`: the days an occurrence is recognized over                               |
//! | `payment` | what a loan's payment is, as the fold makes it (`instantiate_occurrence`)                          |
//!
//! and the engine's own `promise` lines for every occurrence the journal kept. A window or a probe is chosen from the
//! contract's own days, never from an answer, so that two readings are asked the same. A result of more than forty
//! days is printed as a count, the first and the last day and a checksum.
//!
//! `--slow` also asks the ordinal of a contract with no `from`, which counts its due days from `Day::MIN`: about four
//! seconds for each.

mod ask;
#[cfg(feature = "new")]
mod check;
mod reading;

use axiom_core::{Day, Days, FileId};
use axiom_engine::{Options, Plan};
use axiom_model::{Book, ScheduleKind, Source};
use axiom_syntax::Folder;

use ask::Facts;
use reading::Old;

const TODAY: (i32, u32, u32) = (2026, 6, 30);

fn main() {
    let (mut slow, mut checking, mut path) = (false, false, None);
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--slow" => slow = true,
            "--check" => checking = true,
            _ => path = Some(arg),
        }
    }
    let path = path.expect("a project folder or a file");
    let files = read_sources(std::path::Path::new(&path));
    let parsed: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(i, (p, t, _))| axiom_syntax::parse(FileId(i as u16), t, Folder::of(p)))
        .collect();
    let sources: Vec<Source> = files
        .iter()
        .zip(parsed)
        .map(|((path, _, embedded), (file, _))| Source { path, file, embedded: *embedded })
        .collect();
    let (book, diagnostics) = axiom_model::build(&sources);
    let mut codes: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.code.to_string()).collect();
    codes.sort();
    println!("diagnostics {}", codes.join(" "));
    let plan = Plan::new(&book);
    let failed = if checking { verdicts(&book, &plan, slow) } else { dump(&book, &plan, slow) };
    std::process::exit(i32::from(failed));
}

/// What the old machinery says of every contract, and of every occurrence the journal kept.
fn dump(book: &Book<'_>, plan: &Plan<'_, '_>, slow: bool) -> bool {
    let old = Old { book, plan };
    for (id, contract) in book.contracts.iter() {
        println!("== contract {} {}", id.index(), book.name(contract.name));
        let facts = Facts { id, contract, slow };
        for line in facts.shape() {
            println!("{line}");
        }
        let ordinals = facts.ordinal_days(&old);
        let skipped: Vec<ScheduleKind> =
            if slow || contract.days.first() != Day::MIN { Vec::new() } else { facts.schedules() };
        for line in facts.ask(&old, &ordinals).lines(&skipped) {
            println!("{line}");
        }
        for line in old.first_occurrence(id, facts.life().0) {
            println!("{line}");
        }
    }
    let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
    let run = plan.run(Options { today, relaxed: false });
    println!("== promises {}", run.promises.len());
    for promise in &run.promises {
        let kept = promise.kept.map(|(day, _)| show(day));
        println!(
            "promise {} {} ordinal {} due {} kept {:?} waived {}",
            book.name(book.contracts[promise.contract].name),
            kind(promise.schedule),
            promise.ordinal,
            show(promise.due),
            kept,
            promise.waived
        );
    }
    false
}

/// Holds the compiled promise to the reference and the old machinery to both: whether any answer fails.
#[cfg(feature = "new")]
fn verdicts(book: &Book<'_>, plan: &Plan<'_, '_>, slow: bool) -> bool {
    let (old, new) = (Old { book, plan }, reading::New { book });
    let mut failed = false;
    for (id, contract) in book.contracts.iter() {
        let facts = Facts { id, contract, slow };
        let ordinals = facts.ordinal_days(&old);
        let tally = check::check(book, &facts, &old, &facts.ask(&old, &ordinals), &facts.ask(&new, &ordinals));
        failed |= !tally.fails.is_empty();
        for line in tally.lines() {
            println!("{line}");
        }
    }
    failed
}

#[cfg(not(feature = "new"))]
fn verdicts(_: &Book<'_>, _: &Plan<'_, '_>, _: bool) -> bool {
    panic!("--check needs the `new` feature: build with `contracts.py build TREE OUT --new`")
}

/// The project's `.ax` files in path order, then the embedded systems: what the CLI reads, without the CLI.
fn read_sources(path: &std::path::Path) -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    if path.is_file() {
        let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
        found.push((name, std::fs::read_to_string(path).expect("readable"), false));
    } else {
        collect(path, std::path::Path::new(""), &mut found);
        found.sort();
    }
    for (p, s) in axiom_systems::SYSTEMS {
        found.push((p.to_string(), s.to_string(), true));
    }
    found
}

fn collect(root: &std::path::Path, relative: &std::path::Path, found: &mut Vec<(String, String, bool)>) {
    for entry in std::fs::read_dir(root.join(relative)).expect("a folder") {
        let entry = entry.expect("an entry");
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || name == "target" {
            continue;
        }
        let path = relative.join(&name);
        if entry.file_type().expect("a type").is_dir() {
            collect(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "ax") {
            let shown: Vec<_> = path.components().map(|part| part.as_os_str().to_string_lossy()).collect();
            found.push((shown.join("/"), std::fs::read_to_string(root.join(&path)).expect("readable"), false));
        }
    }
}

pub fn show(day: Day) -> String {
    match day {
        Day::MIN => "MIN".into(),
        Day::MAX => "MAX".into(),
        day => day.to_string(),
    }
}

pub fn kind(kind: ScheduleKind) -> &'static str {
    match kind {
        ScheduleKind::Regular => "regular",
        ScheduleKind::Standing => "standing",
    }
}

pub fn span(days: Days) -> String {
    format!("{}..{}", show(days.first()), show(days.last()))
}
