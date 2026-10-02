//! A scratch dump of what the engine materializes for each promise, kept and forecast, to compare two builds.
use axiom_core::{Arena, Day, Days, FileId};
use axiom_engine::{Options, Plan};
use axiom_model::{RuntimeDetail, ScheduleKind, Source};
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
    // the forecast's way: each occurrence the contracts expect, with nothing written
    let plan = Plan::new(&book);
    let mut ledger = plan.start(options);
    for (id, contract) in book.contracts.iter() {
        let first = contract.days.first().max(Day::from_ymd(2026, 1, 1).unwrap());
        let Some(window) = Days::new(first, Day::from_ymd(2027, 6, 30).unwrap()) else { continue };
        let mut ordinals = [0u32; 2];
        for occurrence in contract.occurrences(window) {
            let slot = match occurrence.schedule {
                ScheduleKind::Regular => 0,
                ScheduleKind::Standing => 1,
            };
            let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
            let made = ledger.instantiate_occurrence(
                id,
                occurrence.schedule,
                occurrence.day,
                ordinals[slot],
                None,
                &mut flows,
                &mut details,
                &mut missing,
            );
            ordinals[slot] += 1;
            println!("forecast {:?} {:?} {:?} -> {:?}", id, occurrence.schedule, occurrence.day, made);
            for flow in &flows {
                println!("  flow {flow:?}");
                if let Some(detail) = flow.detail {
                    println!("  detail {:?}", details[detail]);
                }
            }
            println!("  missing {missing:?}");
        }
    }
}
