//! What a client that scrubs a date slider pays: one project, opened and folded once, then asked the same question of forty
//! days. Each view is timed over the whole sweep, the fastest of several sweeps, and per question.
//!
//! ```text
//! cargo run --release -p axiom-session --example scrub PROJECT-FOLDER TODAY [SWEEPS]
//! ```
//!
//! The question is the session's, so the same file builds against a checkout from before the fold recorded histories and
//! against one after, and the two print what the change did for a client.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axiom_core::Day;
use axiom_report::Query;
use axiom_session::{Options, Session, Sources, Texts};

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

/// `count` days spread from the book's first flow to `today`.
fn days(session: &Session<'_>, count: i32) -> Vec<Day> {
    let first = session.book().flows.as_slice().first().map_or(session.options().today, |flow| flow.day);
    let span = session.options().today.0 - first.0;
    (0..count).map(|at| Day(first.0 + span * (at + 1) / count)).collect()
}

/// The fastest of `sweeps` runs of `sweep`, as a time per question asked.
fn per_question(sweeps: usize, questions: usize, mut sweep: impl FnMut()) -> Duration {
    let fastest = (0..sweeps)
        .map(|_| {
            let started = Instant::now();
            sweep();
            started.elapsed()
        })
        .min()
        .expect("a sweep");
    fastest / questions as u32
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("a project folder"));
    let today = Day::parse(args.next().expect("a day").as_bytes()).expect("a date");
    let sweeps: usize = args.next().map_or(5, |text| text.parse().expect("a count"));
    let mut files = Vec::new();
    read(&root, Path::new(""), &mut files);

    let texts = Texts::default();
    let started = Instant::now();
    let sources = Sources::assemble(&texts, files, axiom_systems::SYSTEMS).expect("a project is few files");
    let session = Session::open(sources, Options { today, relaxed: false });
    let opened = started.elapsed();
    let started = Instant::now();
    let held = session.run().posted.len();
    let folded = started.elapsed();
    println!("open {opened:.2?}, first fold {folded:.2?}, {held} flows");
    // What every answer pays before it reads anything: the plan the session builds for each.
    let planned = per_question(sweeps, 1, || drop(axiom_engine::Plan::new(session.book())));
    println!("{:<28} {planned:>10.2?} a question", "a plan, built for each answer");

    let sweep = days(&session, 40);
    let book = session.book();
    let place = book.listed_places().into_iter().next().map(|id| book.name(book.places[id].path));
    asked(&session, sweeps, "balance --at", &sweep, &|at| Query::Balance {
        globs: vec![],
        at: Some(at),
        value: false,
        monthly: false,
    });
    asked(&session, sweeps, "balance --at --value", &sweep, &|at| Query::Balance {
        globs: vec![],
        at: Some(at),
        value: true,
        monthly: false,
    });
    asked(&session, sweeps, "balance --monthly --at", &sweep, &|at| Query::Balance {
        globs: vec![],
        at: Some(at),
        value: false,
        monthly: true,
    });
    if let Some(place) = place {
        asked(&session, sweeps, "register --to", &sweep, &|to| Query::Register { place, from: None, to: Some(to) });
    }
    asked(&session, sweeps, "claims --at", &sweep[..8], &|at| Query::Claims { at: Some(at) });
}

/// Each of `over` asked of the session as `query` says, `sweeps` times; the fastest sweep, as a time a question.
fn asked<'q>(session: &Session<'_>, sweeps: usize, name: &str, over: &[Day], query: &dyn Fn(Day) -> Query<'q>) {
    let each = per_question(sweeps, over.len(), || {
        for &day in over {
            session.query(&query(day), None).expect("a view");
        }
    });
    println!("{name:<28} {each:>10.2?} a question, over {} days", over.len());
}
