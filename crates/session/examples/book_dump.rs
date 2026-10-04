//! The Book a project builds, as text: every arena, list and table in order (names as the symbols they were interned
//! as, so the order of interning is part of it), every map sorted, and the diagnostics the build said in the order it
//! said them. Lane U's lowering unifications (C1 to C3)
//! are held to it: the same sources must build the same Book.
//!
//!     cargo run --release -p axiom-session --example book_dump -- PROJECT [PROJECT ...]
//!
//! A PROJECT is a directory (every `.ax` file beneath it, in path order, as the CLI reads a project) or one `.ax` file.
//! The dump of each follows a `=== PROJECT` line on standard output.

use std::fmt::{Debug, Write};
use std::path::{Path, PathBuf};

use axiom_core::Day;
use axiom_model::Book;
use axiom_session::{Sources, Texts};

fn main() {
    for project in std::env::args().skip(1) {
        println!("=== {project}");
        print!("{}", dump_project(Path::new(&project)));
    }
}

fn dump_project(root: &Path) -> String {
    let files = if root.is_dir() { project_files(root) } else { vec![root.to_path_buf()] };
    let base = if root.is_dir() { root } else { root.parent().unwrap_or(Path::new(".")) };
    let project: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let name = path.strip_prefix(base).unwrap_or(path).to_string_lossy().into_owned();
            (name, std::fs::read_to_string(path).expect("a readable source"))
        })
        .collect();
    let texts = Texts::default();
    let sources = match Sources::assemble(&texts, project, axiom_systems::SYSTEMS) {
        Ok(sources) => sources,
        Err(problem) => return format!("not assembled: {}\n", problem.message),
    };
    let (parsed, syntax) = sources.parse();
    let (book, built) = axiom_model::build(&parsed);
    let mut out = String::new();
    for diagnostic in syntax.iter().chain(&built) {
        let _ = writeln!(
            out,
            "said {:?} {} {} at {:?}",
            diagnostic.severity,
            diagnostic.code,
            diagnostic.message,
            diagnostic.labels.first().map(|label| label.loc)
        );
    }
    dump_book(&book, &mut out);
    out
}

/// Every `.ax` file beneath `root`, in path order; links to folders are not followed.
fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_link = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            if path.is_dir() && !is_link {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "ax") && path.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn list<T: Debug>(out: &mut String, name: &str, items: impl IntoIterator<Item = T>) {
    for (at, item) in items.into_iter().enumerate() {
        let _ = writeln!(out, "{name}[{at}] {item:?}");
    }
}

fn one<T: Debug>(out: &mut String, name: &str, value: T) {
    let _ = writeln!(out, "{name} {value:?}");
}

fn sorted<T: Debug>(out: &mut String, name: &str, items: impl IntoIterator<Item = T>) {
    let mut lines: Vec<String> = items.into_iter().map(|item| format!("{name} {item:?}")).collect();
    lines.sort();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
}

fn dump_book(book: &Book<'_>, out: &mut String) {
    one(out, "base", book.base);
    one(out, "relaxed", book.relaxed);
    one(out, "roots", &book.roots);
    list(out, "text_values", book.text_values.iter());
    list(out, "places", book.places.iter());
    sorted(out, "issuer_places", book.issuer_places.iter());
    list(out, "entities", book.entities.iter());
    list(out, "kinds", book.kinds.iter());
    one(out, "schema", &book.schema);
    one(out, "holders", &book.holders);
    facts(book, out);
    sorted(out, "sites", book.sites.iter());
    list(out, "purposes", book.purposes.iter());
    list(out, "systems", book.systems.iter());
    list(out, "commodities", book.commodities.iter());
    list(out, "assets", book.assets.iter());
    list(out, "contracts", book.contracts.iter());
    one(out, "promises", &book.promises);
    list(out, "derived", book.derived.iter());
    list(out, "laws", book.laws.iter());
    one(out, "rules", &book.rules);
    list(out, "budgets", book.budgets.iter());
    list(out, "params", book.params.iter());
    list(out, "schedules", book.schedules.iter());
    list(out, "code_rules", book.code_rules.iter());
    list(out, "codes", book.codes.iter());
    list(out, "selectors", book.selectors.iter());
    list(out, "details", book.details.iter());
    list(out, "patterns", book.patterns.iter());
    list(out, "formats", book.formats.iter());
    list(out, "txns", book.txns.iter());
    list(out, "journal_programs", book.journal_programs.iter());
    list(out, "written_occurrences", book.written_occurrences.iter());
    list(out, "input_values", book.input_values.iter());
    list(out, "flows", book.flows.iter());
    one(out, "touching", &book.touching);
    list(out, "asserts", book.asserts.iter());
    list(out, "assertion_programs", book.assertion_programs.iter());
    list(out, "events", book.events.iter());
    list(out, "endings", book.endings.iter());
    list(out, "claim_changes", book.claim_changes.iter());
    one(out, "prices", &book.prices);
    list(out, "splits", book.splits.iter());
    list(out, "measures", book.measures.iter());
    list(out, "readings", book.readings.iter());
    list(out, "filed", book.filed.iter());
    list(out, "sources", book.sources.iter());
}

/// What the facts say of each thing, slot by slot: the value from the start of time and each day it changes.
fn facts(book: &Book<'_>, out: &mut String) {
    let facts = &book.facts;
    let mut days: Vec<Day> = facts.step_days().collect();
    days.sort();
    days.dedup();
    days.insert(0, Day::MIN);
    for holder in 0..facts.holders() as u32 {
        for slot in facts.slots(holder) {
            let mut said = Vec::new();
            for &day in &days {
                let value = facts.datum_at(slot, holder, day);
                if said.last().is_none_or(|(_, last)| *last != value) {
                    said.push((day, value));
                }
            }
            let _ = writeln!(out, "facts {holder} {slot:?} {said:?}");
        }
    }
}
