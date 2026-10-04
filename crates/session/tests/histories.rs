//! The histories the fold records, held to two oracles that share nothing with the recorder, on every project this
//! repository has and on any folder of books it is pointed at.
//!
//! **The fold.** A ledger advanced a day at a time knows what every position holds on that day (`Ledger::balance` is the
//! slot's own quantity). The run's history of a position must say the same on every day, and every position the ledger
//! holds something in must have a history. This is exact: a change the recorder missed, a step on the wrong day, a step
//! dropped or merged with another day's, is a day on which the two differ.
//!
//! **The replay.** The way `balance --at` used to answer: add up the flows that stand on a day, the pads, the span of each
//! asset and the splits, with no fold at all, written here as plainly as it can be (every event of every day, in order).
//! It sees the flows the book lists, and what a contract's template makes for a line that keeps an occurrence (the line's
//! own flows are placeholders), so it cannot see a claim place relieved by a payment from a party or by a write-off, nor the claim the monitor makes of a missed
//! occurrence, it counts an asset's unit for the span of its parts even where an opening line brought it in too, and it scales
//! a split over a whole balance where the fold scales each parcel. Those are the places the old answer was wrong, and each
//! difference must be at one of them: a position that is a claim place or a party's place in a book with settlements,
//! write-offs or claims, the place and unit of an asset, or a commodity that has a split. Any other difference fails.
//!
//! `AXIOM_FUZZ_BOOKS=DIR` adds every project under `DIR` (`docs/v5/measure/session/fuzzbooks.py` writes them) to the second
//! test, which is `#[ignore]`d for its time.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use axiom_core::{Day, Id, Qty};
use axiom_engine::{DisposalBoundary, Options, PartKind, Plan, Position, Run, State};
use axiom_model::{Book, Class, Commodity, Place};
use axiom_session::{Session, Sources, Texts};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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

/// The projects under `root`: its folders, and its lone `.ax` files.
fn projects(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> =
        fs::read_dir(root).map(|entries| entries.map(|entry| entry.unwrap().path()).collect()).unwrap_or_default();
    found.retain(|path| path.is_dir() || path.extension().is_some_and(|extension| extension == "ax"));
    found.sort();
    found
}

/// Every project the repository has: the examples (the folder of exploratory ones holds a project each), and the probe books.
fn repository_projects() -> Vec<PathBuf> {
    let repository = repository();
    let explored = repository.join("examples/explore-v5");
    let examples = projects(&repository.join("examples")).into_iter().filter(|root| *root != explored);
    let probes = projects(&repository.join("docs/v5/measure/diff/cases2"));
    examples
        .chain(projects(&explored))
        .chain(probes)
        .filter(|root| root.extension().is_none_or(|e| e == "ax"))
        .collect()
}

/// A project's files; a lone file is a project of one.
fn files(project: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    match project.is_dir() {
        true => read(project, Path::new(""), &mut found),
        false => found.push(("axiom.ax".to_string(), fs::read_to_string(project).unwrap())),
    }
    found
}

/// The first and the last day anything of the book is dated on.
fn span(book: &Book) -> Option<(Day, Day)> {
    let facts = book
        .txns
        .values()
        .map(|txn| txn.day)
        .chain(book.asserts.iter().map(|assert| assert.day))
        .chain(book.claim_changes.iter().map(|change| change.day))
        .chain(book.events.iter().map(|event| event.day))
        .chain(book.splits.iter().map(|split| split.day));
    let (first, last) = facts.fold((Day::MAX, Day::MIN), |(first, last), day| (first.min(day), last.max(day)));
    (first <= last).then_some((first, last))
}

/// The days the fold must be held on: every day from the one before the book's first fact to the one after its last,
/// which is all the days anything can change on and one either side.
fn days(book: &Book) -> Vec<Day> {
    span(book).map_or_else(Vec::new, |(first, last)| (first.0 - 1..=last.0 + 1).map(Day).collect())
}

/// Folds a project through each of the days `today` is asked to be, and gives each fold to `then`: the day the examples are
/// read on, and the day of the book's last fact (what a command line given that day would fold). The day matters to a fold:
/// the monitor claims an occurrence that is missed by `today`.
fn folded(project: &Path, mut then: impl FnMut(&Book, &Run, Options)) {
    let texts = Texts::default();
    let open = |today| {
        let sources =
            Sources::assemble(&texts, files(project), axiom_systems::SYSTEMS).expect("a project is few files");
        Session::open(sources, Options { today, relaxed: false })
    };
    let examples = Day::from_ymd(2026, 4, 16).unwrap();
    let session = open(examples);
    let last = span(session.book()).map(|(_, last)| last).filter(|&last| last != examples);
    for session in [Some(session), last.map(open)].into_iter().flatten() {
        then(session.book(), session.run(), session.options());
    }
}

/// Folds the book a day at a time and holds every position's history to the ledger's balance.
fn fold_oracle(book: &Book, run: &Run, options: Options, days: &[Day]) {
    let plan = Plan::new(book);
    let mut ledger = plan.start(options);
    let known: Vec<Position> = run.histories.positions().map(|(_, at)| at).collect();
    for &day in days {
        ledger.advance(day);
        for (id, at) in run.histories.positions() {
            assert_eq!(run.histories.at(id, day), ledger.balance(at.place, at.unit), "{at:?} on {day}");
        }
        for holding in ledger.holdings().filter(|holding| !holding.qty().is_zero()) {
            let at = Position { place: holding.place, unit: holding.unit };
            assert!(known.binary_search(&at).is_ok(), "{at:?} holds {:?} on {day} and has no history", holding.qty());
        }
    }
    for holding in &run.holdings {
        let at = Position { place: holding.place, unit: holding.unit };
        let (id, _) = run.histories.positions().find(|&(_, found)| found == at).expect("a holding has a history");
        assert_eq!(run.histories.at(id, Day::MAX), holding.qty(), "{at:?} at the end");
    }
}

/// What happens to a position's balance on a day.
enum Event {
    Add(Position, Qty),
    /// A split: everything of the commodity, in every place, is multiplied.
    Scale(Id<Commodity>, axiom_core::Ratio),
}

/// What a replay does about the lines that keep a contract's occurrence.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Occurrences {
    /// Adds up the flows the lines list, which for an occurrence are placeholders: what `balance --at` used to do.
    AsBooked,
    /// Adds up what the contract's template made for each, which is what the fold posts.
    FromTemplates,
}

/// The raw balance of every position at the end of each of `days`, by adding up what stands: the old replay, plainly.
fn replay(book: &Book, run: &Run, days: &[Day], occurrences: Occurrences) -> BTreeMap<Position, Vec<Qty>> {
    let mut events: BTreeMap<Day, Vec<Event>> = BTreeMap::new();
    let mut add =
        |day: Day, place, unit, qty| events.entry(day).or_default().push(Event::Add(Position { place, unit }, qty));
    for ((_, flow), posted) in book.flows.iter().zip(run.posted.iter()) {
        // The lines that keep a contract's occurrence are not posted as flows: the contract's template makes the flows.
        if occurrences == Occurrences::FromTemplates && book.txns[flow.txn].occurrence.is_some() {
            continue;
        }
        let standing = match posted.state {
            State::Actual => Some((flow.day, Day::MAX)),
            State::Settled(on) => Some((flow.day.max(on), Day::MAX)),
            State::Returned(on) => Some((flow.day, on)),
            State::Pending | State::Void | State::Planned => None,
        };
        if let Some((from, past)) = standing {
            for (place, unit, qty) in
                [(flow.from, flow.out.unit, -posted.out), (flow.to, flow.arrive.unit, posted.arrive)]
            {
                add(from, place, unit, qty);
                add(past, place, unit, -qty);
            }
        }
    }
    // What a kept occurrence made, on the day its line was written.
    let kept = run.promises.iter().filter(|_| occurrences == Occurrences::FromTemplates);
    for (day, promise) in kept.filter_map(|promise| Some((promise.kept?.0, promise))) {
        for runtime in run.promise_flows(promise) {
            let flow = &runtime.flow;
            add(day, flow.from, flow.out.unit, -flow.out.qty);
            add(day, flow.to, flow.arrive.unit, flow.arrive.qty);
        }
    }
    for pad in &run.pads {
        add(pad.day, pad.place, pad.amount.unit, pad.amount.qty);
        add(pad.day, pad.counter, pad.amount.unit, -pad.amount.qty);
    }
    for state in &run.assets {
        let asset = &book.assets[state.asset];
        let bought =
            state.parts().iter().filter(|part| part.kind == PartKind::Acquisition).map(|part| part.recorded.day);
        let Some(start) = bought.min() else { continue };
        let end = state.disposed.map_or(Day::MAX, |disposal| match disposal.boundary {
            DisposalBoundary::After(at) => at.day,
            DisposalBoundary::Close(day) => day,
        });
        add(start, asset.place, asset.unit, Qty(1));
        add(end, asset.place, asset.unit, Qty(-1));
    }
    for split in &book.splits {
        events.entry(split.day).or_default().push(Event::Scale(split.unit, split.ratio));
    }

    let mut held: BTreeMap<Position, Qty> = BTreeMap::new();
    let mut snapshots: BTreeMap<Position, Vec<Qty>> = BTreeMap::new();
    let mut pending = events.into_iter().peekable();
    for (at, &day) in days.iter().enumerate() {
        while let Some((_, today)) = pending.next_if(|(when, _)| *when <= day) {
            // The day's movements first, then its splits: a split multiplies what stood on its day.
            for event in today.iter().filter(|event| matches!(event, Event::Add(..))) {
                if let Event::Add(position, qty) = event {
                    *held.entry(*position).or_default() += *qty;
                }
            }
            for event in &today {
                if let Event::Scale(unit, ratio) = event {
                    for (position, qty) in held.iter_mut().filter(|(position, _)| position.unit == *unit) {
                        let _ = position;
                        *qty += qty.scale(*ratio).map_or(Qty::ZERO, |scaled| scaled - *qty);
                    }
                }
            }
        }
        for (&position, &qty) in &held {
            snapshots.entry(position).or_insert_with(|| vec![Qty::ZERO; days.len()])[at] = qty;
        }
        // A position that fell out of `held` cannot: it stays, at zero.
    }
    snapshots
}

/// Whether the replay may be wrong about `position` in this book: it cannot see a claim place relieved by a settlement or a
/// write-off (nor the place of the party that paid), it adds the unit of an asset for the span of its parts whether or not
/// a line already brought it in (an opening does), and it scales a split over the whole balance.
fn replay_may_differ(book: &Book, run: &Run, position: Position) -> bool {
    let claims = !run.settlements.is_empty() || !run.written_off.is_empty() || run.promises.iter().any(|p| p.claimed);
    let party = |place: Id<Place>| book.places[place].class == Class::Outside;
    let claim_side = claims && (book.is_claim(position.place) || party(position.place));
    let asset = book.assets.values().any(|asset| (asset.place, asset.unit) == (position.place, position.unit));
    claim_side || asset || book.splits.iter().any(|split| split.unit == position.unit)
}

/// What the book and the run say touched `position` on `day`: the flows it lists, and the flows a kept occurrence made.
fn what_touches(book: &Book, run: &Run, position: Position, day: Day) -> String {
    let mut said = Vec::new();
    for (id, flow) in book.flows.iter().filter(|(_, flow)| flow.day == day) {
        if [flow.from, flow.to].contains(&position.place) {
            let posted = run.posted[id.index()];
            said.push(format!(
                "  flow {} -> {} out {:?} arrive {:?} {:?}",
                flow.from.index(),
                flow.to.index(),
                posted.out,
                posted.arrive,
                posted.state
            ));
        }
    }
    for promise in run.promises.iter().filter(|promise| promise.kept.is_some_and(|(kept, _)| kept == day)) {
        for runtime in run.promise_flows(promise) {
            let flow = &runtime.flow;
            if [flow.from, flow.to].contains(&position.place) {
                said.push(format!(
                    "  occurrence flow {} -> {} out {:?} arrive {:?}",
                    flow.from.index(),
                    flow.to.index(),
                    flow.out,
                    flow.arrive
                ));
            }
        }
    }
    said.join("\n")
}

/// Holds the replay to the history. Returns the days compared and the positions on which the two differ where they may.
fn replay_oracle(book: &Book, run: &Run, days: &[Day], name: &str) -> (usize, usize) {
    let replayed = replay(book, run, days, Occurrences::FromTemplates);
    let mut all: Vec<Position> = replayed.keys().copied().chain(run.histories.positions().map(|(_, at)| at)).collect();
    all.sort();
    all.dedup();
    let by_position: BTreeMap<Position, Id<Position>> = run.histories.positions().map(|(id, at)| (at, id)).collect();
    let mut explained = 0;
    for position in all {
        let history = |day| by_position.get(&position).map_or(Qty::ZERO, |&id| run.histories.at(id, day));
        let naive = |at: usize| replayed.get(&position).map_or(Qty::ZERO, |column| column[at]);
        let differing = days.iter().enumerate().find(|&(at, &day)| naive(at) != history(day));
        if let Some((at, &day)) = differing {
            if !replay_may_differ(book, run, position) {
                eprintln!("{}", what_touches(book, run, position, day));
            }
            assert!(
                replay_may_differ(book, run, position),
                "{name}: {} ({:?}) on {day}, the first day they differ: the replay says {:?} and the history {:?}, and nothing explains it",
                book.name(book.places[position.place].path),
                position,
                naive(at),
                history(day)
            );
            explained += 1;
        }
    }
    (days.len(), explained)
}

/// Folds a project and holds its histories to both oracles. Returns the days held and the positions the replay is wrong about.
fn hold(project: &Path) -> (usize, usize) {
    let (mut held, mut wrong) = (0, 0);
    folded(project, |book, run, options| {
        let days = days(book);
        fold_oracle(book, run, options, &days);
        let (checked, differing) = replay_oracle(book, run, &days, &project.display().to_string());
        (held, wrong) = (held + checked, wrong + differing);
    });
    (held, wrong)
}

fn hold_all(roots: &[PathBuf]) {
    let (mut checked, mut books, mut explained) = (0, 0, 0);
    for project in roots {
        let (days, differing) = hold(project);
        checked += days;
        explained += differing;
        books += 1;
    }
    eprintln!(
        "{books} projects, {checked} days held to the fold and to the replay; {explained} positions the replay gets wrong where it may"
    );
}

/// Why the old `balance --at` was wrong about `position`, in the words of the report: what the replay could not see.
fn cause(book: &Book, run: &Run, position: Position) -> &'static str {
    let ends = |place: Id<Place>| {
        run.promises.iter().filter(|promise| promise.kept.is_some()).any(|promise| {
            run.promise_flows(promise).iter().any(|runtime| [runtime.flow.from, runtime.flow.to].contains(&place))
        })
    };
    let claims = !run.settlements.is_empty() || !run.written_off.is_empty() || run.promises.iter().any(|p| p.claimed);
    if ends(position.place) {
        "a kept occurrence's flows"
    } else if claims && (book.is_claim(position.place) || book.places[position.place].class == Class::Outside) {
        "a claim settled, forgiven or made by the monitor"
    } else if book.assets.values().any(|asset| (asset.place, asset.unit) == (position.place, position.unit)) {
        "an asset counted twice"
    } else if book.splits.iter().any(|split| split.unit == position.unit) {
        "a split scaled over the balance"
    } else {
        "unexplained"
    }
}

/// Every day on which the old replay (the flows as booked) and the fold disagree about a position, one line each, for the
/// report of what `balance --at` got wrong: `project`, `day`, place, what it said, what the fold held, why.
fn baseline_wrong(project: &Path) -> Vec<String> {
    let mut found = Vec::new();
    folded(project, |book, run, options| found.extend(baseline_wrong_in(project, book, run, options)));
    found.sort();
    found.dedup();
    found
}

fn baseline_wrong_in(project: &Path, book: &Book, run: &Run, _: Options) -> Vec<String> {
    let days = days(book);
    let old = replay(book, run, &days, Occurrences::AsBooked);
    let positions: Vec<_> = old.keys().copied().chain(run.histories.positions().map(|(_, at)| at)).collect();
    let mut found = Vec::new();
    for position in positions.into_iter().collect::<std::collections::BTreeSet<_>>() {
        let by_day = |at: usize, day: Day| {
            let said = old.get(&position).map_or(Qty::ZERO, |column| column[at]);
            let held = run
                .histories
                .positions()
                .find(|&(_, found)| found == position)
                .map_or(Qty::ZERO, |(id, _)| run.histories.at(id, day));
            (said, held)
        };
        let wrong: Vec<_> =
            days.iter().enumerate().filter(|&(at, &day)| by_day(at, day).0 != by_day(at, day).1).collect();
        if let Some(&(at, &first)) = wrong.first() {
            let (said, held) = by_day(at, first);
            let name = book.name(book.places[position.place].path);
            let (said, held) = (
                book.show(axiom_model::Amount::new(said, position.unit)),
                book.show(axiom_model::Amount::new(held, position.unit)),
            );
            let why = cause(book, run, position);
            found.push(format!("{}\t{first}\t{name}\t{said}\t{held}\t{} days\t{why}", project.display(), wrong.len()));
        }
    }
    found
}

#[test]
#[ignore = "prints what the old balance --at got wrong, for the report"]
fn what_the_old_balance_at_got_wrong() {
    for line in repository_projects().iter().flat_map(|root| baseline_wrong(root)) {
        println!("WRONG\t{line}");
    }
}

#[test]
fn the_examples_and_the_probe_books_have_the_histories_the_fold_and_the_replay_say() {
    hold_all(&repository_projects());
}

#[test]
#[ignore = "reads the books AXIOM_FUZZ_BOOKS names"]
fn fuzz_books() {
    let dir = std::env::var("AXIOM_FUZZ_BOOKS").expect("AXIOM_FUZZ_BOOKS names a folder of projects");
    hold_all(&projects(Path::new(&dir)));
}
