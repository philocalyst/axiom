//! What the fold says of every contract of a project, as text, so that two builds can be compared, and, with `--check`,
//! whether what it says is what the reference says.
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
//! | `keep`    | which due day a line dated on a day keeps, or none, or that it is ambiguous                        |
//! | `factor`  | the escalation and the proration of a day, or why there is none                                    |
//! | `recog`   | the days an occurrence is recognized over                                                          |
//! | `payment` | what a loan's payment is, as the fold makes it (`instantiate_occurrence`)                          |
//!
//! and the fold's own `promise` lines for every occurrence the journal kept, and `missed` lines for every one nothing
//! kept and nothing can. A window or a probe is chosen from the contract's own days, never from an answer, so that two
//! readings are asked the same. A result of more than forty days is printed as a count, the first and the last day and a
//! checksum.
//!
//! `--slow` also asks the ordinal of a contract with no `from`, which counts its due days from `Day::MIN` for a schedule
//! that can only be walked.

mod ask;
mod check;
mod monitor;
mod reading;
mod walk;

use axiom_core::{Day, Days, FileId};
use axiom_engine::{Options, Plan};
use axiom_model::{Book, ScheduleKind, Source};
use axiom_syntax::Folder;

use ask::Facts;
use reading::Fold;

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

/// What the fold says of every contract, and of every occurrence the journal kept or missed.
fn dump(book: &Book<'_>, plan: &Plan<'_, '_>, slow: bool) -> bool {
    let fold = Fold { book, plan };
    for (id, contract) in book.contracts.iter() {
        println!("== contract {} {}", id.index(), book.name(contract.name));
        let facts = Facts { id, contract, slow };
        for line in facts.shape() {
            println!("{line}");
        }
        let ordinals = facts.ordinal_days(&fold);
        let skipped: Vec<ScheduleKind> =
            if slow || contract.days.first() != Day::MIN { Vec::new() } else { facts.schedules() };
        for line in facts.ask(&fold, &ordinals).lines(&skipped) {
            println!("{line}");
        }
        for line in fold.first_occurrence(id, facts.life().0) {
            println!("{line}");
        }
        // What the old walkers said, rebuilt from `calendar::due` as they asked it, to hold the frozen dump to.
        let reference = check::Reference { id, contract };
        for line in facts.old_rule(&reference).lines(&skipped) {
            let (word, rest) = line.split_once(' ').expect("a line has a word");
            println!("{word}old {rest}");
        }
    }
    let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
    let run = plan.run(Options { today, relaxed: false });
    let (kept, missed): (Vec<&axiom_engine::Promise>, Vec<_>) = run.promises.iter().partition(|promise| promise.kept.is_some());
    println!("== promises {}", kept.len());
    for promise in kept {
        let day = promise.kept.map(|(day, _)| show(day));
        println!(
            "promise {} {} ordinal {} due {} kept {:?} waived {}",
            book.name(book.contracts[promise.contract].name),
            kind(promise.schedule),
            promise.ordinal,
            show(promise.due),
            day,
            promise.waived
        );
    }
    println!("== missed {}", missed.len());
    for promise in missed {
        let name = book.name(book.contracts[promise.contract].name);
        println!("missed {name} {} ordinal {} due {}", kind(promise.schedule), promise.ordinal, show(promise.due));
    }
    false
}

/// Holds the fold to the reference: whether any answer fails.
fn verdicts(book: &Book<'_>, plan: &Plan<'_, '_>, slow: bool) -> bool {
    let fold = Fold { book, plan };
    let mut failed = false;
    for (id, contract) in book.contracts.iter() {
        let facts = Facts { id, contract, slow };
        let ordinals = facts.ordinal_days(&fold);
        let tally = check::check(book, &facts, &facts.ask(&fold, &ordinals));
        failed |= !tally.fails.is_empty();
        for line in tally.lines() {
            println!("{line}");
        }
    }
    let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
    let run = plan.run(Options { today, relaxed: false });
    let mut tally = check::Tally::default();
    monitor::check(book, &run, slow, &mut tally);
    failed |= !tally.fails.is_empty();
    for line in tally.lines() {
        println!("{line}");
    }
    failed
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
