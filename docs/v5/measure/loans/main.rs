//! What the engine says of a project's loan, as lines `loans.py` holds to what its reference says (`expect.txt`).
//!
//! It is not a member of the workspace: `loans.py build` writes a Cargo.toml with path dependencies on the `core`, `syntax`,
//! `model`, `engine` and `systems` crates of a tree and builds it. It is run on one project (`main.ax`), on the day the reference
//! is written for:
//!
//! | line       | what it says                                                                                              |
//! |------------|-----------------------------------------------------------------------------------------------------------|
//! | `pay`      | each payment of the loan's schedule: its day, the interest, the principal and what is owed after it       |
//! | `prepay`   | each prepayment of the schedule, the same                                                                 |
//! | `posted`   | what the fold posted for a line that keeps a due day, by purpose: the interest and the principal          |
//! | `planned`  | what the forecast promised after the day of the run, the same                                             |
//! | `missed`   | a due day nothing kept, that the monitor found                                                            |
//! | `open`     | what the schedule says is owed at each month end                                                          |
//! | `tab`      | what the debt tab holds on the day of the run, as owed                                                    |
//! | `diag`     | an `assertion` the fold reported, and each `loan-balance` with the cause its note names                   |
//!
//! Nothing here judges; two of these files, one from the engine and one from the reference, are compared line by line.

use axiom_core::calendar::Window;
use axiom_core::{Day, FileId, Period};
use axiom_engine::{Options, Plan};
use axiom_model::promise::Kind;
use axiom_model::{Book, RuntimeFlow, Source};
use axiom_syntax::Folder;

/// The day of the run and how far the forecast looks: the reference's `TODAY` and `UNTIL`.
const TODAY: (i32, u32, u32) = (2026, 6, 30);
const UNTIL: (i32, u32, u32) = (2027, 8, 31);

/// What a note says, and the cause the reference gives it. The order is the order the notes are tried in.
const CAUSES: [(&str, &str); 5] = [
    ("a payment was missed", "missed"),
    ("a payment was short", "short"),
    ("an extra was counted as principal", "extra"),
    ("a prepayment nobody wrote", "prepayment"),
    ("no cause", "none"),
];

fn day((year, month, date): (i32, u32, u32)) -> Day {
    Day::from_ymd(year, month, date).expect("a day")
}

/// What a flow says of itself that the reference compares: how much of it is `purpose`.
fn of_purpose(book: &Book, flows: &[RuntimeFlow], purpose: &str) -> i64 {
    let purpose = book.purpose(purpose).ok();
    let counts = |flow: &&RuntimeFlow| flow.flow.purpose.map(|purposed| purposed.purpose) == purpose;
    flows.iter().filter(counts).map(|flow| flow.flow.out.qty.0).sum()
}

fn split(book: &Book, flows: &[RuntimeFlow]) -> String {
    format!("{} {}", of_purpose(book, flows, "interest"), of_purpose(book, flows, "principal"))
}

/// The last day of the month after the one `day` is in.
fn next_month_end(day: Day) -> Day {
    let next_month = Window::containing(Period::Month, day).days().last().add_days(1);
    Window::containing(Period::Month, next_month).days().last()
}

fn parse(path: &str) -> Vec<(String, String, bool)> {
    let text = std::fs::read_to_string(path).expect("readable");
    let mut files = vec![("main.ax".to_string(), text, false)];
    files.extend(axiom_systems::SYSTEMS.iter().map(|(path, text)| (path.to_string(), text.to_string(), true)));
    files
}

fn main() {
    let path = std::env::args().nth(1).expect("a project file");
    let files = parse(&path);
    let parsed: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(index, (path, text, _))| axiom_syntax::parse(FileId(index as u16), text, Folder::of(path)).0)
        .collect();
    let sources: Vec<Source> = files
        .iter()
        .zip(parsed)
        .map(|((path, _, embedded), file)| Source { path, file, embedded: *embedded })
        .collect();
    let (book, built) = axiom_model::build(&sources);
    for diagnostic in built.iter().filter(|diagnostic| diagnostic.is_error()) {
        println!("builderror {}", diagnostic.code);
    }
    let (today, until) = (day(TODAY), day(UNTIL));
    let plan = Plan::new(&book);
    let options = Options { today, relaxed: false };
    let (run, view) = plan.run_with_view(options);
    let mut ledger = plan.resume(&view.checkpoint(), options);
    ledger.advance(today);
    ledger.reach(until);
    ledger.promise(|_| true);
    ledger.advance(until);
    let recorded = ledger.recorded();

    for (id, contract) in book.contracts.iter() {
        let (Some(terms), Some(loan)) = (contract.loan, book.promises.loan(id)) else { continue };
        let name = book.name(contract.name);
        for entry in loan.entries() {
            let word = if entry.kind == Kind::Pay { "pay" } else { "prepay" };
            let paid = entry.paid;
            println!("{word} {name} {} {} {} {}", entry.day, paid.interest.0, paid.principal.0, paid.open.0);
        }
        for promise in run.promises.iter().filter(|promise| promise.contract == id) {
            match promise.kept {
                Some(_) => println!("posted {name} {} {}", promise.due, split(&book, run.promise_flows(promise))),
                None => println!("missed {name} {}", promise.due),
            }
        }
        for planned in recorded.planned.iter().filter(|planned| planned.contract == id) {
            let Ok(made) = planned.made else { continue };
            let flows = made.flows(recorded.promised_flows).unwrap_or_default();
            println!("planned {name} {} {}", planned.due, split(&book, flows));
        }
        let mut probe = terms.on;
        while probe <= until {
            println!("open {name} {probe} {}", loan.open_on(probe).map_or(0, |owed| owed.0));
            probe = next_month_end(probe);
        }
        let owed: i64 = run.holdings.iter().filter(|holding| holding.place == terms.debt).map(|holding| -holding.qty().0).sum();
        println!("tab {name} {today} {owed}");
    }
    for diagnostic in run.diagnostics.iter() {
        match &*diagnostic.code {
            "assertion" => println!("diag assertion"),
            "loan-balance" => {
                let notes = diagnostic.notes.join(" ");
                let cause = CAUSES.iter().find(|(phrase, _)| notes.contains(phrase)).map_or("unnamed", |(_, cause)| cause);
                println!("diag loan-balance {cause}");
            }
            _ => {}
        }
    }
}
