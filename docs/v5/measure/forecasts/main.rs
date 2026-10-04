//! A scratch dump of what the engine says about a forecast and about the fold that has the forecast's occurrences written, to
//! hold the one to the other (`docs/v5/measure/forecast.py dump`). It is not a member of the workspace: give it a Cargo.toml with
//! path dependencies on the `core`, `syntax`, `model`, `engine` and `systems` crates of a tree, and build it once for each tree.
//!
//!     forecasts forecast PATH TODAY UNTIL   the fold to TODAY through its view checkpoint, resumed, promising to UNTIL
//!     forecasts history  PATH TODAY UNTIL   the fold run to UNTIL (what is written is history)
//!
//! `derived` and `flow` rows are for `docs/v5/measure/derived.py`: `flow` is every flow the fold posted, a journal line's, an
//! occurrence's or a law's (a derived flow has the state of the flow it began with), said as what it is and is for; `derived`
//! is a law's, after TODAY, with the law that made it. A forecast and a fold that has what it forecast written must print
//! the same `derived` rows.
//!
//! Both print the same kinds of line, so that they can be compared: `holding DAY PLACE UNIT QTY LOTS` at every month end after
//! TODAY and at UNTIL; `effect`, `violation`, `gain` after TODAY; `missed` (a due day nothing kept) after TODAY; and, for each
//! occurrence, `planned` (the forecast posted it) or `kept` (a line did) with the flows it made.
use axiom_core::calendar::Window;
use axiom_core::{Day, FileId, Period};
use axiom_engine::{Options, Plan, Planned, Promise, Run, State};
use axiom_model::{Book, Cause, Object, Offspring, RuntimeFlow, Source};
use axiom_syntax::Folder;

fn day(text: &str) -> Day {
    let mut parts = text.split('-').map(|part| part.parse::<u32>().expect("a number"));
    let (y, m, d) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
    Day::from_ymd(y as i32, m, d).expect("a day")
}

fn build(path: &str) -> Result<(Vec<(String, String, bool)>, ()), String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let mut files = vec![(path.to_string(), text, false)];
    for (p, s) in axiom_systems::SYSTEMS {
        files.push((p.to_string(), s.to_string(), true));
    }
    Ok((files, ()))
}

/// The month ends after `today`, and `until`.
fn checkpoints(today: Day, until: Day) -> Vec<Day> {
    let mut days = Vec::new();
    let mut at = today;
    while at < until {
        let end = Window::containing(Period::Month, at.add_days(1)).days().last();
        at = end.min(until);
        days.push(at);
    }
    days
}

fn place(book: &Book, id: axiom_core::Id<axiom_model::Place>) -> String {
    format!("p{}:{}", id.index(), book.name(book.places[id].path))
}

fn flow(book: &Book, runtime: &RuntimeFlow, details: &axiom_core::Arena<axiom_model::RuntimeDetail>) -> String {
    let f = &runtime.flow;
    let detail = runtime.detail.map(|id| format!("{:?}", details[id]));
    format!(
        "{}>{} out={}{} arrive={}{} day={} rec={}..{} purpose={:?} payee={:?} owner={} ord={} infer={:?} detail={:?}",
        place(book, f.from),
        place(book, f.to),
        f.out.qty.0,
        book.name(book.commodities[f.out.unit].symbol),
        f.arrive.qty.0,
        book.name(book.commodities[f.arrive.unit].symbol),
        f.day,
        f.recognized.first(),
        f.recognized.last(),
        f.purpose,
        f.payee,
        f.owner.index(),
        runtime.ordinal,
        f.infer,
        detail
    )
}

/// A flow said as what it is and what it is for, and nothing of which line or law made it.
fn plain(book: &Book, flow: &axiom_model::Flow) -> String {
    let said = |place| book.name(book.places[place].path);
    let purpose = flow.purpose.map_or("-".to_owned(), |purposed| {
        let name = book.name(book.purposes[purposed.purpose].name);
        match purposed.of {
            Some(Object::Asset(asset)) => format!("{name}+of+{}", book.name(book.assets[asset].name)),
            Some(_) => format!("{name}+of+?"),
            None => name.to_owned(),
        }
    });
    let payee = flow.payee.map_or("-", |entity| book.name(book.entities[entity].path));
    format!(
        "{} {}>{} out={}{} arrive={} purpose={purpose} owner={} payee={payee}",
        flow.day,
        said(flow.from),
        said(flow.to),
        flow.out.qty.0,
        book.name(book.commodities[flow.out.unit].symbol),
        flow.arrive.qty.0,
        book.name(book.entities[flow.owner].path),
    )
}

/// What a derived flow is after: the flow it began with, returned or not, since it is returned with it.
fn after(run: &Run, offspring: &Offspring) -> String {
    match offspring.root {
        Cause::Flow(id) => match run.posted[id.index()].state {
            State::Returned(day) => format!("Returned({day:?})"),
            _ => "Actual".to_owned(),
        },
        _ => "Actual".to_owned(),
    }
}

fn derived_rows(book: &Book, today: Day, offspring: &[Offspring]) {
    for derived in offspring.iter().filter(|derived| derived.flow.day > today) {
        println!("derived law={} {}", derived.law.index(), plain(book, &derived.flow));
    }
}

/// Every flow the fold posted, once: the journal's, the occurrences' it kept and the laws' derived.
fn flow_rows(book: &Book, run: &Run) {
    for (id, flow) in book.flows.iter() {
        println!("flow {} {:?}", plain(book, flow), run.posted[id.index()].state);
    }
    for promise in run.promises.iter().filter(|promise| promise.kept.is_some()) {
        for runtime in run.promise_flows(promise) {
            println!("flow {} Actual", plain(book, &runtime.flow));
        }
    }
    for derived in run.offspring.iter() {
        println!("flow {} {}", plain(book, &derived.flow), after(run, derived));
    }
}

fn promise_line(
    book: &Book,
    kind: &str,
    contract: axiom_core::Id<axiom_model::Contract>,
    due: Day,
    schedule: axiom_model::ScheduleKind,
    ordinal: u32,
    flows: &[RuntimeFlow],
    details: &axiom_core::Arena<axiom_model::RuntimeDetail>,
) {
    let flows: Vec<String> = flows.iter().map(|runtime| flow(book, runtime, details)).collect();
    println!("{kind} {} {due} {schedule:?} {ordinal} [{}]", book.name(book.contracts[contract].name), flows.join(" | "));
}

fn holdings(book: &Book, at: Day, ledger: &axiom_engine::Ledger) {
    for holding in ledger.holdings() {
        let lots: Vec<String> =
            holding.lots.iter().map(|lot| format!("{}/{}/{}/{}", lot.qty.0, lot.basis.0, lot.acquired, lot.held_since)).collect();
        println!(
            "holding {at} {} {} {} plain={} [{}]",
            place(book, holding.place),
            book.name(book.commodities[holding.unit].symbol),
            holding.qty().0,
            holding.plain.0,
            lots.join(",")
        );
    }
}

fn recorded(book: &Book, today: Day, recorded: axiom_engine::Recorded<'_>) {
    for effect in recorded.effects.iter().filter(|effect| effect.day > today) {
        println!(
            "effect {} law={} subject={:?} owner={} {} {} {:?}",
            effect.day,
            effect.law.index(),
            effect.subject,
            effect.owner.index(),
            book.name(effect.name),
            effect.amount.qty.0,
            effect.consequence
        );
    }
    for violation in recorded.violations.iter().filter(|violation| violation.day > today) {
        println!("violation {} law={} subject={:?} {:?}", violation.day, violation.law.index(), violation.subject, violation.verdict);
    }
    for gain in recorded.gains.iter().filter(|gain| gain.day > today) {
        println!("gain {} {}>{} {} {} {} {}", gain.day, place(book, gain.from), place(book, gain.to), gain.qty.0, gain.basis.0, gain.proceeds.0, gain.acquired);
    }
}

/// The occurrences nothing kept that became past their reach after `today`: the ones a fold found when it went on from there.
fn missed(book: &Book, today: Day, promises: &[Promise]) {
    let reach = |promise: &Promise| book.promises.schedule(promise.contract, promise.schedule).map_or(0, |schedule| schedule.reach());
    let found = |promise: &&Promise| promise.due.add_days(reach(promise) + 1) > today;
    for promise in promises.iter().filter(|promise| promise.kept.is_none()).filter(found) {
        println!("missed {} {} {:?} {}", book.name(book.contracts[promise.contract].name), promise.due, promise.schedule, promise.ordinal);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, mode, path, today, until] = &args[..] else { panic!("forecasts forecast|history PATH TODAY UNTIL") };
    let (files, ()) = match build(path) {
        Ok(found) => found,
        Err(message) => return println!("error {message}"),
    };
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
    let (today, until) = (day(today), day(until));
    let plan = Plan::new(&book);
    let ahead = checkpoints(today, until);
    match mode.as_str() {
        "forecast" => {
            let options = Options { today, relaxed: false };
            let (run, view) = plan.run_with_view(options);
            for diagnostic in &run.diagnostics {
                println!("diagnostic {}", diagnostic.code);
            }
            let checkpoint = view.checkpoint();
            let mut ledger = plan.resume(&checkpoint, options);
            ledger.advance(today);
            ledger.reach(until);
            ledger.promise(|_| true);
            for &at in &ahead {
                ledger.advance(at);
                holdings(&book, at, &ledger);
            }
            let recorded_ = ledger.recorded();
            for planned in recorded_.planned {
                let Planned { contract, schedule, ordinal, due, made } = *planned;
                match made {
                    Ok(made) => {
                        let flows = made.flows(recorded_.promised_flows).unwrap();
                        promise_line(&book, "planned", contract, due, schedule, ordinal, flows, recorded_.promised_details);
                    }
                    Err(error) => println!("planned {} {due} {schedule:?} {ordinal} error {error:?}", book.name(book.contracts[contract].name)),
                }
            }
            recorded(&book, today, recorded_);
            derived_rows(&book, today, recorded_.offspring);
            missed(&book, today, recorded_.promises);
        }
        _ => {
            let options = Options { today: until, relaxed: false };
            let mut ledger = plan.start(options);
            for &at in &ahead {
                ledger.advance(at);
                holdings(&book, at, &ledger);
            }
            let run = plan.run(options);
            for promise in run.promises.iter().filter(|promise| promise.kept.is_some() && promise.due > today) {
                let flows = run.promise_flows(promise);
                promise_line(&book, "kept", promise.contract, promise.due, promise.schedule, promise.ordinal, flows, &run.runtime_details);
            }
            for diagnostic in &run.diagnostics {
                println!("diagnostic {}", diagnostic.code);
            }
            recorded(&book, today, axiom_engine::Recorded {
                gains: &run.gains,
                effects: &run.effects,
                adjustments: &run.adjustments,
                violations: &run.violations,
                diagnostics: &run.diagnostics,
                promises: &run.promises,
                planned: &[],
                promised_flows: &run.promised_flows,
                offspring: &run.offspring,
                first_offspring: 0,
                promised_inputs: &run.missing_inputs,
                promised_details: &run.runtime_details,
            });
            derived_rows(&book, today, &run.offspring);
            flow_rows(&book, &run);
            missed(&book, today, &run.promises);
        }
    }
}
