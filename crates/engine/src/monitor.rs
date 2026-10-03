//! What the fold has yet to settle of every promise, and what a run says of them.
//!
//! A contract's occurrences fall due on days its schedule says (`axiom_model::promise`), and a line in the journal
//! keeps one of them. A **missed** due day is one that no line kept and that no line can keep any more: its day plus its
//! schedule's reach has passed (LANGUAGE §7: "past its grace with no occurrence"), or a later due day has been kept, and
//! the fold takes lines in day order, so nothing dated later can keep an earlier day than the nearest to it.
//!
//! # Why the state is the one it is
//!
//! One [`Residual`] per stream (a contract's regular schedule, its standing `buy`), in a dense vector in contract order,
//! so that a stream is found by binary search and the whole is cloned with the world, a few words a stream. A residual
//! is the due day the stream waits for and nothing else it needs to be told about: the ordinal, and what a loan still
//! owes. It moves when that day is kept (`settle`) or missed (`miss_through`), and the day it will be missed is
//! the only thing the fold has to ask of it before every fact, so the days are kept in a min-heap beside the streams.
//! A heap, not a scan of the streams or a sorted vector: the next miss changes only when a stream moves, the question is
//! asked before every fact of the journal, and a stale entry (its stream has moved since) is dropped when it surfaces,
//! which makes a move one push.
//!
//! The monitor reads the promises and never touches a holding: a miss is a record. (A later lane posts the claim a missed
//! `due ... else` makes; it will do it where [`Monitor::miss_through`] hands the miss over.)
//!
//! A stream starts on the day the book does, not on its own first day: a due day before the book's first fact is not
//! the book's to miss, and a contract with no `from` begins at the beginning of time.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};

use axiom_core::{Day, Diagnostic, Id, Map, Qty};
use axiom_model::promise::{Promises, Residual};
use axiom_model::{Book, Contract, Flow, ScheduleKind};

use crate::lots::Holdings;
use crate::plan::Plan;
use crate::{OmittedInputs, OpenClaim, Parcel, Promise, PromisedFlows};

/// One stream of one contract, and the occurrence it waits for.
#[derive(Clone, Copy, Hash)]
struct Waiting {
    contract: Id<Contract>,
    schedule: ScheduleKind,
    residual: Residual,
    /// How far from its due day a line may still keep it.
    reach: i32,
    /// The day the occurrence it waits for is missed, and the heap says so: the first day a line cannot keep it.
    miss: Option<Day>,
}

impl Waiting {
    /// What a miss of the occurrence it waits for is, as the run records it; none if it waits for nothing.
    fn missed(&self) -> Option<Promise> {
        Some(Promise {
            contract: self.contract,
            schedule: self.schedule,
            ordinal: self.residual.ordinal(),
            due: self.residual.next()?,
            kept: None,
            waived: false,
            flows: PromisedFlows::of(0..0),
            missing_inputs: OmittedInputs::of(0..0),
        })
    }

    /// Where the stream sorts: by contract, the regular schedule before the standing one.
    fn key(&self) -> (usize, bool) {
        (self.contract.index(), self.schedule == ScheduleKind::Standing)
    }
}

/// Every stream of the book's promises, and when each next occurrence is missed.
#[derive(Clone, Default)]
pub(crate) struct Monitor {
    waiting: Vec<Waiting>,
    /// When each stream's next occurrence is missed, soonest first. An entry is stale once its stream has moved.
    misses: BinaryHeap<Reverse<(Day, u32)>>,
}

impl Hash for Monitor {
    /// What the rest of the fold depends on is where each stream stands; the heap is that, sorted.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.waiting.hash(state);
    }
}

impl Monitor {
    /// Every stream of `promises`, each waiting for its first due day on or after `from`.
    pub fn start(promises: &Promises, from: Day) -> Monitor {
        let mut monitor = Monitor::default();
        for (contract, promise) in promises.by_contract() {
            for schedule in [ScheduleKind::Regular, ScheduleKind::Standing] {
                let Some(stream) = promise.stream(schedule) else { continue };
                let residual = Residual::starting_at(promises, stream.every, from);
                let reach = promises.schedule_of(stream.schedule).reach();
                if !residual.is_done() {
                    monitor.waiting.push(Waiting { contract, schedule, residual, reach, miss: None });
                    monitor.expect(monitor.waiting.len() - 1);
                }
            }
        }
        monitor
    }

    /// Notes when the stream at `at` will miss the occurrence it now waits for.
    fn expect(&mut self, at: usize) {
        let waiting = &mut self.waiting[at];
        let miss = waiting.residual.next().and_then(|due| due.0.checked_add(waiting.reach)?.checked_add(1)).map(Day);
        waiting.miss = miss;
        if let Some(day) = miss {
            self.misses.push(Reverse((day, at as u32)));
        }
    }

    /// Hands over every occurrence missed on or before `day`, in the order they were missed.
    pub fn miss_through(&mut self, promises: &Promises, day: Day, mut missed: impl FnMut(Promise)) {
        while let Some(&Reverse((miss, at))) = self.misses.peek().filter(|&&Reverse((miss, _))| miss <= day) {
            self.misses.pop();
            let at = at as usize;
            if self.waiting[at].miss != Some(miss) {
                continue;
            }
            missed(self.waiting[at].missed().expect("a stream that waits has a day"));
            self.waiting[at].residual.advance(promises);
            self.expect(at);
        }
    }

    /// A line has kept the occurrence of `ordinal` of a stream: the stream moves past it, and past every earlier one that
    /// was not kept, which no later line can keep now.
    pub fn settle(
        &mut self,
        promises: &Promises,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        ordinal: u32,
        mut missed: impl FnMut(Promise),
    ) {
        let key = (contract.index(), schedule == ScheduleKind::Standing);
        let at = self.waiting.partition_point(|waiting| waiting.key() < key);
        let Some(waiting) = self.waiting.get_mut(at).filter(|waiting| waiting.key() == key) else { return };
        let before = waiting.residual;
        while !waiting.residual.is_done() && waiting.residual.ordinal() < ordinal {
            missed(waiting.missed().expect("a stream that is not done has a day"));
            waiting.residual.advance(promises);
        }
        if !waiting.residual.is_done() && waiting.residual.ordinal() == ordinal {
            waiting.residual.advance(promises);
        }
        if waiting.residual != before {
            self.expect(at);
        }
    }
}

/// The days up to which a warning lists every due day it is about.
const LISTED: usize = 5;

/// One warning for each contract that has due days nothing kept, with the day of the last, and how to write it down.
pub(crate) fn missed(book: &Book, promises: &[Promise], horizon: Day) -> Vec<Diagnostic> {
    let mut by_contract: Map<Id<Contract>, Vec<Day>> = Map::default();
    for promise in promises.iter().filter(|promise| promise.kept.is_none()) {
        by_contract.entry(promise.contract).or_default().push(promise.due);
    }
    let mut contracts: Vec<_> = by_contract.into_iter().collect();
    contracts.sort_unstable_by_key(|(contract, _)| *contract);
    contracts.into_iter().map(|(contract, days)| missed_by(book, &book.contracts[contract], days, horizon)).collect()
}

fn missed_by(book: &Book, contract: &Contract, mut days: Vec<Day>, horizon: Day) -> Diagnostic {
    days.sort_unstable();
    days.dedup();
    let (name, last) = (book.name(contract.name), days[days.len() - 1]);
    let headline = match days.as_slice() {
        [only] => format!("`{name}` was due on {only} and no occurrence was written"),
        many => format!("`{name}` was due {} times and no occurrence was written (the last on {last})", many.len()),
    };
    let listed = if days.len() <= LISTED {
        days.iter().map(Day::to_string).collect::<Vec<_>>().join(", ")
    } else {
        let (first, recent) = (days[0], &days[days.len() - 2..]);
        format!("{first}, … {}, {} (all {})", recent[0], recent[1], days.len())
    };
    Diagnostic::warning("missed-occurrence", headline)
        .label(contract.loc, format!("last due {} days ago, on {last}", horizon.0 - last.0))
        .note(format!("not kept: {listed}"))
        .help(format!("write what happened: `{last} {name}`, with its own amount if it was another"))
        .help(format!("or say it was not owed: `{last} {name} waived`"))
}

/// What each claim place holds of what others owe, as one open claim a parcel.
pub(crate) fn open_claims(plan: &Plan, holdings: &Holdings) -> Box<[OpenClaim]> {
    let book = plan.book;
    let places = holdings.iter().filter(|slot| plan.traits.place(slot.place).claim);
    let parcels = places.flat_map(|slot| slot.holding.lots.iter().map(move |lot| (slot.place, slot.unit, lot)));
    parcels
        .filter(|(.., lot)| lot.qty > Qty::ZERO)
        .filter_map(|(place, unit, lot)| claim(book, place, unit, lot))
        .collect()
}

/// The claim a parcel of a claim place is: the flow of the transaction that paid into the place says who owes it,
/// and when.
fn claim(
    book: &Book,
    place: Id<axiom_model::Place>,
    unit: Id<axiom_model::Commodity>,
    lot: &Parcel,
) -> Option<OpenClaim> {
    let txn = lot.txn.source_txn()?;
    let flows = book.txns.get(txn)?.flows;
    let (source, made) = flows.ids().map(|id| (id, &book.flows[id])).find(|(_, flow)| flow.to == place)?;
    Some(OpenClaim {
        origin: lot.txn,
        source: Some(source),
        ordinal: (source.index() - flows.start().index()) as u32,
        due: book.flow_view(made).detail().due,
        claimant: place,
        counterpart: made.from,
        debtor: debtor(book, place, made),
        creditor: book.places[place].owner,
        owner: book.places[place].owner,
        unit,
        amount: lot.qty,
        codes: lot.codes,
    })
}

/// Who owes a claim: the party of the tab it sits in, else the payee of the flow that made it.
fn debtor(book: &Book, place: Id<axiom_model::Place>, made: &Flow) -> Option<Id<axiom_model::Entity>> {
    match book.places[place].role {
        axiom_model::Role::Tab(party) => Some(party),
        _ => made.payee,
    }
}

#[cfg(test)]
mod tests {
    use crate::Run;
    use crate::source_tests::{day, with_run};

    const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
commodity VTI
entity landlord
account checking
opening 2026-01-01
  checking 10_000 USD
";

    /// Every promise the run recorded of the contract `name`, in the order recorded: its due day, and whether it was kept.
    fn promised(book: &axiom_model::Book, run: &Run, name: &str) -> Vec<(String, bool)> {
        let contract = book.contract(name).unwrap();
        let of = run.promises.iter().filter(|promise| promise.contract == contract);
        of.map(|promise| (promise.due.to_string(), promise.kept.is_some())).collect()
    }

    fn rent(clauses: &str, journal: &str) -> String {
        format!(
            "{PRELUDE}contract rent with landlord\n  1_000 USD monthly on 1 from checking\n  from 2026-01-01\n{clauses}{journal}"
        )
    }

    #[test]
    fn a_due_day_nothing_kept_is_missed_once_it_is_past_its_reach_and_not_before() {
        let text = rent("", "2026-01-02 rent\n");
        // The reach of a monthly schedule is 15 days: 2026-02-01 is missed on 02-17, 2026-03-01 on 03-17.
        with_run(&text, day(2026, 3, 10), |book, run| {
            assert_eq!(promised(book, run, "rent"), [("2026-01-01".into(), true), ("2026-02-01".into(), false)]);
        });
        with_run(&text, day(2026, 3, 17), |book, run| {
            let due: Vec<_> = promised(book, run, "rent").into_iter().map(|(due, kept)| (due, kept)).collect();
            assert_eq!(due.len(), 3, "{due:?}");
            assert_eq!(due[2], ("2026-03-01".to_string(), false));
        });
    }

    #[test]
    fn a_grace_moves_the_day_a_due_day_is_missed() {
        let text = rent("  grace 3d\n", "");
        with_run(&text, day(2026, 1, 4), |book, run| assert_eq!(promised(book, run, "rent"), []));
        with_run(&text, day(2026, 1, 5), |book, run| {
            assert_eq!(promised(book, run, "rent"), [("2026-01-01".into(), false)]);
        });
    }

    #[test]
    fn a_kept_due_day_settles_the_ones_before_it_that_nothing_kept() {
        // A grace of 60 days keeps 2026-01-01 and 02-01 open on 03-02; the line that keeps 03-01 closes them.
        let text = rent("  grace 60d\n", "2026-03-01 rent\n");
        with_run(&text, day(2026, 3, 2), |book, run| {
            let promised = promised(book, run, "rent");
            assert_eq!(
                promised,
                [("2026-01-01".into(), false), ("2026-02-01".into(), false), ("2026-03-01".into(), true)]
            );
        });
    }

    #[test]
    fn a_waived_stretch_and_the_days_after_an_end_are_not_owed() {
        let text = rent("  until 2026-04-30\n", "2026-02-01 rent waived until 2026-02-28\n");
        with_run(&text, day(2026, 8, 1), |book, run| {
            let due: Vec<_> = promised(book, run, "rent").into_iter().map(|(due, _)| due).collect();
            assert_eq!(due, ["2026-01-01", "2026-03-01", "2026-04-01"]);
        });
    }

    #[test]
    fn what_was_due_before_the_book_began_is_not_the_books_to_miss() {
        let text = format!("{PRELUDE}contract rent with landlord\n  1_000 USD monthly on 1 from checking\n");
        // No `from`: the contract has been owed since the beginning of time. The book begins on 2026-01-01.
        let started = std::time::Instant::now();
        with_run(&text, day(2026, 3, 20), |book, run| {
            let due: Vec<_> = promised(book, run, "rent").into_iter().map(|(due, _)| due).collect();
            assert_eq!(due, ["2026-01-01", "2026-02-01", "2026-03-01"]);
        });
        assert!(started.elapsed().as_secs() < 2, "a contract with no `from` is not walked from the beginning of time");
    }

    #[test]
    fn a_loan_is_watched_through_its_last_payment_only() {
        let text = format!(
            "{PRELUDE}contract car-loan with landlord\n  loan 3_000 USD on 2026-01-01 at 0% over 3m\n  monthly on 1 from checking\n"
        );
        with_run(&text, day(2026, 12, 1), |book, run| {
            let due: Vec<_> = promised(book, run, "car-loan").into_iter().map(|(due, _)| due).collect();
            assert_eq!(due, ["2026-02-01", "2026-03-01", "2026-04-01"]);
        });
    }

    #[test]
    fn a_standing_order_and_a_payment_are_watched_apart() {
        let text = format!(
            "{PRELUDE}contract invest with landlord\n  50 USD monthly on 1 from checking\n  buy VTI for 500 USD monthly on 15 from checking\n  from 2026-01-01\n2026-01-01 invest\n"
        );
        with_run(&text, day(2026, 2, 20), |book, run| {
            let contract = book.contract("invest").unwrap();
            let mut seen: Vec<_> = run
                .promises
                .iter()
                .filter(|promise| promise.contract == contract)
                .map(|promise| (promise.due.to_string(), format!("{:?}", promise.schedule), promise.kept.is_some()))
                .collect();
            seen.sort();
            // 02-15 of the standing order is still within its reach on 02-20.
            assert_eq!(
                seen,
                [
                    ("2026-01-01".to_string(), "Regular".to_string(), true),
                    ("2026-01-15".to_string(), "Standing".to_string(), false),
                    ("2026-02-01".to_string(), "Regular".to_string(), false),
                ]
            );
        });
    }

    #[test]
    fn what_was_missed_is_one_warning_a_contract_with_the_days_listed() {
        let text = rent("", "");
        with_run(&text, day(2026, 5, 1), |book, run| {
            let warnings: Vec<_> = run.diagnostics.iter().filter(|found| found.code == "missed-occurrence").collect();
            assert_eq!(warnings.len(), 1);
            assert_eq!(
                warnings[0].message,
                "`rent` was due 4 times and no occurrence was written (the last on 2026-04-01)"
            );
            assert_eq!(warnings[0].notes, ["not kept: 2026-01-01, 2026-02-01, 2026-03-01, 2026-04-01"]);
            assert_eq!(warnings[0].anchor(), Some(book.contracts[book.contract("rent").unwrap()].loc));
        });
    }

    #[test]
    fn the_open_claims_are_the_open_parcels_of_the_claim_places() {
        let text = "\
base USD
commodity USD
  precision 2
purpose rent : spending
account assets/checking
entity ben
entity landlord
opening 2025-02-01
  checking 5_000 USD

2025-03-01 ben owes landlord 1_050 USD due 2025-03-08 #rent
";
        with_run(text, day(2025, 4, 1), |book, run| {
            assert!(run.monitor_complete);
            let [claim] = run.open_claims[..] else { panic!("{:?}", run.open_claims) };
            assert_eq!((claim.due, claim.amount.0), (Some(day(2025, 3, 8)), 105_000));
            assert_eq!(claim.debtor, book.entity("ben").ok());
            assert_eq!(claim.owner, book.entity("landlord").unwrap());
        });
    }
}
