//! A scratch dump of what the engine materializes for each promise, kept and forecast, to compare two builds.
use axiom_core::{Day, FileId};
use axiom_engine::{Options, Plan};
use axiom_model::Source;
use axiom_syntax::Folder;

fn main() {
    let path = std::env::args().nth(1).expect("a path");
    let text = std::fs::read_to_string(&path).expect("readable");
    let mut files: Vec<(String, String, bool)> = vec![(path.clone(), text, false)];
    for (p, s) in axiom_systems::SYSTEMS {
        files.push((p.to_string(), s.to_string(), true));
    }
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
    let (book, _diagnostics) = axiom_model::build(&sources);
    let today = Day::from_ymd(2026, 6, 30).unwrap();
    let options = Options { today, relaxed: false };
    let run = axiom_engine::run(&book, options);
    println!("PROMISES {}", run.promises.len());
    for promise in &run.promises {
        println!("promise {promise:?}");
        for flow in run.promise_flows(promise) {
            println!("  flow {flow:?}");
            if let Some(detail) = flow.detail {
                println!("  detail {:?}", run.runtime_details[detail]);
            }
        }
        println!("  missing {:?}", run.promise_missing_inputs(promise));
    }
    println!("posted {:?}", run.posted);
    println!("gains {:?}", run.gains);
    println!("holdings {:?}", run.holdings);
    println!("violations {:?}", run.violations.len());
    println!("adjustments {:?}", run.adjustments);
    for d in &run.diagnostics {
        println!("diagnostic {} {}", d.code, d.message);
    }
    // the forecast's way: the fold to today, promising every contract to the end of 2027
    let plan = Plan::new(&book);
    let until = Day::from_ymd(2027, 6, 30).unwrap();
    let mut ledger = plan.start(options);
    ledger.advance(today);
    ledger.reach(until);
    ledger.promise(|_| true);
    ledger.advance(until);
    let recorded = ledger.recorded();
    for planned in recorded.planned {
        println!("forecast {:?} {:?} {:?} {} -> {:?}", planned.contract, planned.schedule, planned.due, planned.ordinal, planned.made);
        let Ok(made) = planned.made else { continue };
        for flow in made.flows(recorded.promised_flows).unwrap_or_default() {
            println!("  flow {flow:?}");
            if let Some(detail) = flow.detail {
                println!("  detail {:?}", recorded.promised_details[detail]);
            }
        }
        println!("  missing {:?}", made.missing(recorded.promised_inputs).unwrap_or_default());
    }
}
