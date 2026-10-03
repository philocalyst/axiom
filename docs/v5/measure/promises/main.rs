//! What the contract machinery says of every contract of a project, as text, so that two builds (or two ways of
//! answering) can be compared. The driver is `docs/v5/measure/contracts.py`, which builds this for a tree of the
//! workspace (it is not a member: the driver writes a Cargo.toml with path dependencies on `core`, `syntax`, `model`,
//! `engine` and `systems` of the tree) and runs it over generated projects.
//!
//! Each contract is asked, by a `Reading`, the questions the fold, the lowering and the reports ask of it:
//!
//! | line      | the question                                                                                    |
//! |-----------|-------------------------------------------------------------------------------------------------|
//! | `due`     | the days due in a window, both schedules merged, each with its schedule                         |
//! | `ordinal` | which due day of its own schedule an occurrence is, counted from the contract's first day         |
//! | `keep`    | which due day a line dated on a day keeps (`nearest_occurrence`), or none, or that it is ambiguous |
//! | `factor`  | `amount_on_schedule`: the escalation and the proration of a day, or why there is none            |
//! | `recog`   | `recognition_on_schedule`: the days an occurrence is recognized over                              |
//! | `payment` | what a loan's payment is, as the fold makes it (`instantiate_occurrence`)                        |
//!
//! and the engine's own answer to the last two for every occurrence the journal kept (`promise`). A window or a
//! probe is chosen from the contract's own days, never from an answer, so that two readings are asked the same.
//! A result of more than forty days is printed as a count, the first and the last day and a checksum.
//!
//! `--slow` also asks the ordinal of a contract with no `from`, which counts its due days from `Day::MIN`: about four
//! seconds for each.

use axiom_core::{Day, Days, FileId, Id, Ratio, Span};
use axiom_engine::{Options, Plan};
use axiom_model::{Book, Contract, ForecastError, ScheduleKind, Source, TermsState, nearest_occurrence};
use axiom_syntax::Folder;

const TODAY: (i32, u32, u32) = (2026, 6, 30);
/// The day a contract with no `from` is read around: its days come from `Day::MIN`.
const REFERENCE: (i32, u32, u32) = (2026, 1, 1);

fn main() {
    let mut slow = false;
    let mut path = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--slow" => slow = true,
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
    let reading = Old { book: &book, plan: &plan };
    for (id, contract) in book.contracts.iter() {
        println!("== contract {} {}", id.index(), book.name(contract.name));
        let facts = Facts { id, contract, slow };
        for line in facts.lines(&reading) {
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

fn show(day: Day) -> String {
    match day {
        Day::MIN => "MIN".into(),
        Day::MAX => "MAX".into(),
        day => day.to_string(),
    }
}

fn kind(kind: ScheduleKind) -> &'static str {
    match kind {
        ScheduleKind::Regular => "regular",
        ScheduleKind::Standing => "standing",
    }
}

fn span(days: Days) -> String {
    format!("{}..{}", show(days.first()), show(days.last()))
}

/// What one build of the contract machinery says. The old code is one reading; the new structure is another.
trait Reading {
    /// The days due in `window`, both schedules merged, in the order `Contract::occurrences` gives them.
    fn due(&self, contract: Id<Contract>, window: Days) -> Vec<(Day, ScheduleKind)>;
    /// The number of due days of the schedule from the contract's first day to `due`, less one.
    fn ordinal(&self, contract: Id<Contract>, schedule: ScheduleKind, due: Day) -> Option<u32>;
    /// The line `day` keeps.
    fn keep(&self, contract: Id<Contract>, day: Day) -> Keep;
    /// The factor of `day` for a schedule, or the error that says why there is none.
    fn factor(&self, contract: Id<Contract>, schedule: ScheduleKind, day: Day) -> Result<Ratio, ForecastError>;
    /// The window `day`'s occurrence is recognized over, or the error; none if the schedule has no template.
    fn recognized(
        &self,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Option<Result<Days, ForecastError>>;
    /// What the fold makes of the first due day of a loan: the payment, or the error.
    fn payment(&self, contract: Id<Contract>) -> String;
}

/// Which due day a line keeps.
enum Keep {
    Out,
    Kept(ScheduleKind, Day),
    Ambiguous(Day, Day),
}

impl std::fmt::Display for Keep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Keep::Out => f.write_str("none"),
            Keep::Kept(schedule, due) => write!(f, "{} {}", kind(*schedule), show(*due)),
            Keep::Ambiguous(regular, standing) => write!(f, "ambiguous {} {}", show(*regular), show(*standing)),
        }
    }
}

/// The contract machinery as it is: `Contract::occurrences`, `nearest_occurrence`, `amount_on_schedule`, the engine.
struct Old<'a, 'b, 's> {
    book: &'b Book<'s>,
    plan: &'a Plan<'b, 's>,
}

impl Reading for Old<'_, '_, '_> {
    fn due(&self, contract: Id<Contract>, window: Days) -> Vec<(Day, ScheduleKind)> {
        self.book.contracts[contract]
            .occurrences(window)
            .map(|occurrence| (occurrence.day, occurrence.schedule))
            .collect()
    }

    /// The engine's own count (`post_written_occurrence`), made of the same `Contract::occurrences`.
    fn ordinal(&self, contract: Id<Contract>, schedule: ScheduleKind, due: Day) -> Option<u32> {
        let contract = &self.book.contracts[contract];
        let through = Days::new(contract.days.first(), due).unwrap_or(Days::on(due));
        let count = contract
            .occurrences(through)
            .filter(|occurrence| occurrence.schedule == schedule && occurrence.day <= due)
            .count();
        count.checked_sub(1).and_then(|index| u32::try_from(index).ok())
    }

    fn keep(&self, contract: Id<Contract>, day: Day) -> Keep {
        match nearest_occurrence(&self.book.contracts[contract], day) {
            Ok(Some((schedule, due, _))) => Keep::Kept(schedule, due),
            Ok(None) => Keep::Out,
            Err((regular, standing)) => Keep::Ambiguous(regular, standing),
        }
    }

    fn factor(&self, contract: Id<Contract>, schedule: ScheduleKind, day: Day) -> Result<Ratio, ForecastError> {
        self.book.contracts[contract].amount_on_schedule(self.book, schedule, day)
    }

    fn recognized(
        &self,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Option<Result<Days, ForecastError>> {
        let contract = &self.book.contracts[contract];
        let terms = contract.terms_on_schedule(schedule, day)?;
        let template = terms.template.first()?;
        Some(contract.recognition_on_schedule(&template.header.flow, schedule, day))
    }

    fn payment(&self, id: Id<Contract>) -> String {
        let contract = &self.book.contracts[id];
        let Some(first) = contract.occurrences(Days::new(contract.days.first(), Day::MAX).expect("days")).next() else {
            return "no due day".into();
        };
        let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
        let mut ledger = self.plan.start(Options { today, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), axiom_core::Arena::new(), Vec::new());
        let made = ledger.instantiate_occurrence(
            id,
            first.schedule,
            first.day,
            0,
            None,
            &mut flows,
            &mut details,
            &mut missing,
        );
        match made {
            Ok(_) => format!("{} {:?}", show(first.day), flows.first().map(|flow| flow.flow.out)),
            Err(error) => format!("{} {error:?}", show(first.day)),
        }
    }
}

/// The questions put to a contract, in an order that does not depend on any answer.
struct Facts<'a> {
    id: Id<Contract>,
    contract: &'a Contract,
    slow: bool,
}

impl Facts<'_> {
    fn lines(&self, reading: &impl Reading) -> Vec<String> {
        let mut lines = self.shape();
        let (first, last) = self.life();
        let stretches = self.stretches();
        lines.extend(self.windows(first, last, &stretches).into_iter().map(|window| self.due_line(reading, window)));
        let probes = self.probes(first, last, &stretches);
        lines.extend(
            probes.into_iter().map(|day| (day, reading.keep(self.id, day).to_string())).collect::<Runs>().lines("keep"),
        );
        lines.extend(self.ordinals(reading, first));
        for schedule in self.schedules() {
            let days: Vec<Day> = self.days_to_read(first);
            let factor = |day| said(day, format!("{:?}", reading.factor(self.id, schedule, day)));
            lines.extend(
                days.iter()
                    .map(|&day| (day, factor(day)))
                    .collect::<Runs>()
                    .lines(&format!("factor {}", kind(schedule))),
            );
            let window = |day| match reading.recognized(self.id, schedule, day) {
                // A window that starts on the day it is asked of is the same answer on every day, said that way.
                Some(Ok(days)) if days.first() == day => format!("Ok(from the day, {} days)", days.len()),
                Some(found) => said(day, format!("{:?}", found.map(span))),
                None => "no template".to_string(),
            };
            lines.extend(
                days.iter()
                    .map(|&day| (day, window(day)))
                    .collect::<Runs>()
                    .lines(&format!("recog {}", kind(schedule))),
            );
        }
        if self.contract.loan.is_some() {
            lines.push(format!("payment {}", reading.payment(self.id)));
        }
        lines
    }

    /// Which schedules the contract has.
    fn schedules(&self) -> Vec<ScheduleKind> {
        let mut schedules = Vec::new();
        if self.contract.terms.is_some() {
            schedules.push(ScheduleKind::Regular);
        }
        if self.contract.standing.is_some() {
            schedules.push(ScheduleKind::Standing);
        }
        schedules
    }

    /// What the contract is, as the book holds it.
    fn shape(&self) -> Vec<String> {
        let contract = self.contract;
        let mut lines = vec![format!(
            "days {} ended {} loan {} buys {} deposit {}",
            span(contract.days),
            contract.ended.is_some(),
            contract.loan.is_some(),
            contract.buys.is_some(),
            contract.deposit.is_some()
        )];
        for (schedule, timeline) in
            [(ScheduleKind::Regular, &contract.terms), (ScheduleKind::Standing, &contract.standing)]
        {
            let Some(timeline) = timeline else { continue };
            let mut equal = true;
            let first = timeline.at(Day::MIN);
            for (days, terms) in timeline.within(Days::ALWAYS) {
                let state = match terms.state {
                    TermsState::Active => "active",
                    TermsState::Waived => "waived",
                };
                lines.push(format!("stretch {} {} {state}", kind(schedule), span(days)));
                let mut same = terms.clone();
                same.state = first.state;
                same.change = first.change;
                equal &= same == *first;
            }
            lines.push(format!(
                "terms {} every {:?} on {:?} anchor {} estimate {} grace {:?} escalation {:?} prorated {} period {:?} covers {:?} equal-stretches {equal}",
                kind(schedule),
                first.every,
                first.on,
                show(first.anchor),
                first.estimate,
                first.grace,
                first.escalation,
                first.prorated,
                first.period,
                first.covers,
            ));
        }
        lines
    }

    /// The contract's first and last day, or, for an unbounded end, a day to read around.
    fn life(&self) -> (Day, Day) {
        let reference = Day::from_ymd(REFERENCE.0, REFERENCE.1, REFERENCE.2).expect("a date");
        let first = if self.contract.days.first() == Day::MIN { reference } else { self.contract.days.first() };
        let last =
            if self.contract.days.last() == Day::MAX { first.add(Span::months(60)) } else { self.contract.days.last() };
        (first, last)
    }

    /// The stretches of both timelines, as days.
    fn stretches(&self) -> Vec<Days> {
        let timelines = [&self.contract.terms, &self.contract.standing];
        timelines
            .into_iter()
            .flatten()
            .flat_map(|timeline| timeline.within(Days::ALWAYS).map(|(days, _)| days))
            .collect()
    }

    /// Windows to ask the due days of: around the first day, the last, each change, and far from all of them.
    fn windows(&self, first: Day, last: Day, stretches: &[Days]) -> Vec<Days> {
        let near = |center: Day, before: i32, after: i32| {
            Days::new(Day(center.0.saturating_sub(before)), Day(center.0.saturating_add(after)))
        };
        let mut windows = vec![
            near(first, 40, 40),
            near(first, 0, 400),
            near(first, -100, 101),
            near(first, -1000, 1200),
            near(first, 3650, -1),
            near(last, 70, 70),
            near(last, -1, 400),
            near(first, 0, 0),
            near(last, 0, 0),
            near(first, 0, 12_000),
        ];
        for days in stretches {
            for edge in [days.first(), days.last()] {
                if edge != Day::MIN && edge != Day::MAX {
                    windows.push(near(edge, 3, 3));
                }
            }
        }
        if self.contract.days.first() == Day::MIN {
            windows.push(Days::new(Day::MIN, Day(i32::MIN + 400)));
            windows.push(Days::new(Day(-400_000), Day(-399_000)));
        }
        if self.contract.days.last() == Day::MAX {
            windows.push(Days::new(Day(i32::MAX - 400), Day::MAX));
        }
        let mut windows: Vec<Days> = windows.into_iter().flatten().collect();
        windows.dedup();
        windows
    }

    fn due_line(&self, reading: &impl Reading, window: Days) -> String {
        let found = reading.due(self.id, window);
        let shown = |(day, schedule): &(Day, ScheduleKind)| format!("{}/{}", show(*day), &kind(*schedule)[..1]);
        if found.len() <= 40 {
            return format!("due {} : {}", span(window), found.iter().map(shown).collect::<Vec<_>>().join(" "));
        }
        let sum = found.iter().fold(0xcbf29ce484222325u64, |sum, (day, schedule)| {
            (sum ^ (day.0 as u32 as u64) ^ ((*schedule == ScheduleKind::Standing) as u64) << 33)
                .wrapping_mul(0x100000001b3)
        });
        format!(
            "due {} : n {} first {} last {} sum {sum:016x}",
            span(window),
            found.len(),
            shown(&found[0]),
            shown(&found[found.len() - 1])
        )
    }

    /// The days a line may be dated on: every day around the first, around each change and the last, and a spread.
    fn probes(&self, first: Day, last: Day, stretches: &[Days]) -> Vec<Day> {
        let mut days: Vec<i64> = (-3..=60).map(|offset| i64::from(first.0) + offset).collect();
        let edges = stretches.iter().flat_map(|days| [days.first(), days.last()]).chain([last]);
        for edge in edges.filter(|&edge| edge != Day::MIN && edge != Day::MAX) {
            days.extend((-6..=6).map(|offset| i64::from(edge.0) + offset));
        }
        let (low, high) = (i64::from(first.0), i64::from(last.0).max(i64::from(first.0) + 1));
        let spread = (high - low).clamp(1, 4000);
        let mut state = 0x9e3779b97f4a7c15u64 ^ (low as u64);
        for _ in 0..60 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            days.push(low + (state >> 33) as i64 % spread);
        }
        days.sort_unstable();
        days.dedup();
        days.into_iter().filter_map(|day| i32::try_from(day).ok().map(Day)).collect()
    }

    /// The ordinal of the first dozen due days of each schedule, a few later ones and the last before the end.
    fn ordinals(&self, reading: &impl Reading, first: Day) -> Vec<String> {
        let mut lines = Vec::new();
        let unbounded = self.contract.days.first() == Day::MIN;
        for schedule in self.schedules() {
            let window = Days::new(first, Day(first.0.saturating_add(3000))).expect("days");
            let due: Vec<Day> =
                reading.due(self.id, window).into_iter().filter(|(_, s)| *s == schedule).map(|(day, _)| day).collect();
            let picks: Vec<Day> =
                due.iter().copied().take(12).chain(due.iter().copied().skip(12).step_by(37)).collect();
            for day in picks.into_iter().take(if unbounded { usize::from(self.slow) } else { 40 }) {
                lines.push(format!(
                    "ordinal {} {} : {:?}",
                    kind(schedule),
                    show(day),
                    reading.ordinal(self.id, schedule, day)
                ));
            }
            if unbounded && !self.slow {
                lines.push(format!("ordinal {} skipped: no first day", kind(schedule)));
            }
        }
        lines
    }

    /// The days the factor and the recognition window are asked on: about six years from just before the first day.
    fn days_to_read(&self, first: Day) -> Vec<Day> {
        (-3..2200).map(|offset| Day(first.0.saturating_add(offset))).collect()
    }
}

/// An answer that names the day it was asked on names it as `the day`, so that the same answer on two days is one.
fn said(day: Day, answer: String) -> String {
    answer.replace(&format!("Day({})", day.0), "the day")
}

/// Consecutive days with the same answer, as ranges: `2026-01-01..2026-01-31 : Ok(..)`.
#[derive(Default)]
struct Runs(Vec<(Day, Day, String)>);

impl FromIterator<(Day, String)> for Runs {
    fn from_iter<I: IntoIterator<Item = (Day, String)>>(items: I) -> Runs {
        let mut runs: Vec<(Day, Day, String)> = Vec::new();
        for (day, answer) in items {
            match runs.last_mut() {
                Some((_, last, said)) if *said == answer && last.0.checked_add(1) == Some(day.0) => *last = day,
                _ => runs.push((day, day, answer)),
            }
        }
        Runs(runs)
    }
}

impl Runs {
    fn lines(&self, name: &str) -> Vec<String> {
        let range = |from: Day, to: Day| span(Days::new(from, to).expect("ordered"));
        self.0.iter().map(|(from, to, answer)| format!("{name} {} : {answer}", range(*from, *to))).collect()
    }
}
