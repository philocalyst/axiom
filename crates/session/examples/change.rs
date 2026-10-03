//! What an MCP server does with a session, with the transport left out: open a project, answer a query, say what an
//! edit *would* change, apply it, and answer again. Each step ends in JSON, the form a client is sent.
//!
//! ```text
//! cargo run --release -p axiom-session --example change [PROJECT-FOLDER]
//! ```
//!
//! The folder is `examples/05-family` unless one is given. A server is this and a loop that reads a request, makes
//! one of these calls and writes what it returned.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_core::{Day, Qty};
use axiom_report::{Money, Query, json};
use axiom_session::{Applied, NewTransaction, Options, Session, Sources, Texts};

/// Every `.ax` file under `root`, by path, as the project's files are.
fn read(root: &Path, folder: &Path, found: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = fs::read_dir(root.join(folder)).expect("a folder").map(|entry| entry.unwrap()).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = folder.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            read(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "ax") {
            found.push((path.to_string_lossy().into_owned(), fs::read_to_string(root.join(&path)).unwrap()));
        }
    }
}

fn day(text: &str) -> Day {
    Day::parse(text.as_bytes()).expect("a date")
}

/// The balance of everyone, as JSON.
fn balance(session: &Session<'_>) -> String {
    let query = Query::Balance { globs: vec!["joint-checking"], at: None, value: false, monthly: false };
    json::render(&session.query(&query, None).expect("the balance resolves"), session.sources())
}

/// The project in `root`, as the sources of a session over `texts`.
fn open<'t>(texts: &'t Texts, root: &Path) -> Sources<'t> {
    let mut files = Vec::new();
    read(root, Path::new(""), &mut files);
    Sources::assemble(texts, files, axiom_systems::SYSTEMS).expect("a project is few files")
}

/// A typed transaction, not a line of text: a gift in the last month of the journal.
fn gift() -> NewTransaction<'static> {
    NewTransaction {
        day: day("2025-12-31"),
        from: "joint-checking",
        to: "st-annes-parish",
        amount: Money { qty: Qty(15_000), scale: 2, unit: "USD" },
        purpose: Some("charity"),
        description: Some("year-end gift"),
        codes: &[],
    }
}

fn main() {
    let default = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/05-family");
    let root = std::env::args_os().nth(1).map_or_else(|| PathBuf::from(default), PathBuf::from);

    // The texts are kept in `texts`, the session borrows them, and an applied edit adds to them.
    let texts = Texts::default();
    let mut session = Session::open(open(&texts, &root), Options { today: day("2026-04-16"), relaxed: false });
    println!("opened: {} diagnostics (the example is still in the v3 syntax)", session.diagnostics().count());
    let before = balance(&session);
    println!("before: {before}");

    let december = session.sources().find("journal/2025/12.ax").expect("the last month").id;
    let edit = gift().append_to(december).expect("every part is a word");

    // What would change, read from a session that does not exist when the call returns.
    let (after, change) = session
        .what_if(&edit, |hypothetical| (balance(hypothetical), Applied::between(december, &session, hypothetical)))
        .expect("the edit applies");
    println!("what if: {after}");
    println!("what if, the change in diagnostics: {}", change.json(session.sources()));
    assert_eq!(balance(&session), before, "asking changed nothing");

    // And now for real.
    let applied = session.apply(edit).expect("the edit applies");
    println!("applied: {}", applied.json(session.sources()));
    println!("after: {}", balance(&session));
    let text = &session.sources().get(december).expect("the file").text;
    println!("the file now ends: {}", text.lines().last().unwrap_or_default());
}
