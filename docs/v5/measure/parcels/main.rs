//! A scratch dump of every parcel the fold holds and of the history around it, to compare two builds.
//!
//! The driver is `docs/v5/measure/claims.py`, which builds this against a tree of the workspace (it is not a member: the
//! driver writes a Cargo.toml with path dependencies on the crates of the tree) and runs it over generated projects. It
//! says, for a project folded to 2026-06-30: every holding with every field of every parcel (the claim parcels among
//! them, each with the codes of the transaction that made it), the gains, the adjustments the laws made, the parts of every
//! asset and what disposed it, the wash-sale carries still waiting, every diagnostic, and with the `new` feature the claims
//! a `waived` statement forgave. The reports are compared by the driver through the CLI, so this is the engine's own state.

use axiom_core::{Day, FileId};
use axiom_engine::{Options, Parcel, Run};
use axiom_model::{Book, Class, Place, Role, Source};
use axiom_syntax::Folder;

const TODAY: (i32, u32, u32) = (2026, 6, 30);

fn main() {
    let path = std::env::args().nth(1).expect("a project folder or a file");
    let files = read_sources(std::path::Path::new(&path));
    let parsed: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(i, (p, t, _))| axiom_syntax::parse(FileId(i as u16), t, Folder::of(p)))
        .collect();
    let sources: Vec<Source> = files
        .iter()
        .zip(parsed)
        .map(|((p, _, embedded), (file, _))| Source { path: p, file, embedded: *embedded })
        .collect();
    let (book, built) = axiom_model::build(&sources);
    let mut codes: Vec<_> = built.iter().map(|d| d.code.to_string()).collect();
    codes.sort();
    println!("model {}", codes.join(" "));
    let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
    let run = axiom_engine::run(&book, Options { today, relaxed: false });
    holdings(&book, &run);
    println!("== gains {}", run.gains.len());
    for gain in &run.gains {
        println!("gain {gain:?}");
    }
    println!("== adjustments {}", run.adjustments.len());
    for adjustment in &run.adjustments {
        println!("adjustment {adjustment:?}");
    }
    println!("== assets");
    for state in &run.assets {
        println!("asset {} disposed {:?}", state.asset.index(), state.disposed);
        for part in state.parts() {
            println!("  part {part:?}");
        }
    }
    println!("== carries {}", run.pending_carries.len());
    for carry in &run.pending_carries {
        println!("carry {carry:?}");
    }
    written_off(&book, &run);
    println!("== posted {}", run.posted.len());
    for posted in run.posted.iter() {
        println!("posted {posted:?}");
    }
    println!("== diagnostics {}", run.diagnostics.len());
    for d in &run.diagnostics {
        println!("diagnostic {:?} {} {}", d.severity, d.code, d.message);
    }
}

#[cfg(feature = "new")]
fn written_off(book: &Book<'_>, run: &Run) {
    println!("== written off {}", run.written_off.len());
    for off in &run.written_off {
        let unit = book.name(book.commodities[off.unit].symbol);
        println!(
            "written-off change {} {} {unit} qty {} basis {} acquired {}",
            off.change,
            label(book, off.place),
            off.qty.0,
            off.basis.0,
            off.acquired
        );
    }
}

/// Before the lane nothing was forgiven: the section is there, empty, so that two builds' dumps line up.
#[cfg(not(feature = "new"))]
fn written_off(_: &Book<'_>, _: &Run) {
    println!("== written off 0");
}

/// What a place is called: a tab has no name, so it is the party it is with and which way the debt runs.
fn label(book: &Book<'_>, place: axiom_core::Id<Place>) -> String {
    let at = &book.places[place];
    match at.role {
        Role::Tab(party) => {
            let way = if at.class == Class::Debt { "owed-by" } else { "owed-by-party" };
            format!("tab({},{way},{})", book.name(book.entities[party].path), book.name(book.entities[at.owner].path))
        }
        _ => book.name(at.path).to_string(),
    }
}

fn holdings(book: &Book<'_>, run: &Run) {
    println!("== holdings {}", run.holdings.len());
    let mut shown: Vec<_> = run.holdings.iter().collect();
    shown.sort_by_key(|holding| (label(book, holding.place), holding.unit));
    for holding in shown {
        let unit = book.name(book.commodities[holding.unit].symbol);
        println!("holding {} {unit} plain {}", label(book, holding.place), holding.plain.0);
        for lot in &holding.lots {
            println!("  lot {}", parcel(book, lot));
        }
    }
}

fn parcel(book: &Book<'_>, lot: &Parcel) -> String {
    let codes = [lot.codes.header, lot.codes.local].into_iter().flat_map(|run| book.codes[run].iter());
    let codes: Vec<_> = codes.map(|&code| book.name(code)).collect();
    format!(
        "qty {} basis {} acquired {} held-since {} wash {} txn {:?} part {:?} tied {:?} codes [{}]",
        lot.qty.0,
        lot.basis.0,
        lot.acquired,
        lot.held_since,
        lot.wash_matched,
        lot.txn,
        lot.part,
        lot.tied.map(|entity| entity.index()),
        codes.join(" ")
    )
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
