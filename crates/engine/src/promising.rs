//! An occurrence falls due: a line of the journal keeps it, or, past the day a ledger stands on, the promise is posted.
//!
//! A contract's occurrences are kept by lines (`2026-03-01 rent`), and the fold posts what a line says. A forecast is that
//! same fold carried past the last day anything is written: where no line keeps an occurrence that falls due, the ledger
//! posts the occurrence the promise says, on its due day, through the function that posts a kept one (`post_occurrence`),
//! and tells the monitor it is kept. What a forecast posts is what the fold would have posted had the book said so; the
//! report that asks for it has no loop of its own to decide which occurrence is next, to number it, or to apply it.
//!
//! # Why the state is the one it is
//!
//! A stream's [`Residual`] in the monitor says what is still *owed*: the oldest due day nothing kept and nothing has
//! missed. A forecast needs another cursor over the same schedule, what is still to be *promised* after today, and the
//! two differ whenever a stream waits on an occurrence that is behind today (a `grace` of two months keeps March open while
//! the forecast is at April). So [`Promising`] holds a residual of its own for every stream it was asked to promise, and one
//! min-heap of `(due day, stream)` with one entry for each stream that has a day to come: the question the fold asks before
//! every fact, "does a promise fall due before this?", is a peek, and moving a stream is a pop and a push. The entry at the
//! head is always an occurrence to post, never one the journal wrote: a written day is skipped when its entry is made, so
//! that nothing past the limit the fold was given can be taken by accident. A line written *ahead* of today (a
//! future-dated `2026-09-03 rent`) is in the journal's timeline and will be posted as the fold reaches it, so the table
//! of written due days is kept beside the streams, sorted, and searched when an entry is made.
//!
//! The state is not in the monitor's `World`, which a checkpoint hashes and a resumed ledger starts from: a ledger that does
//! not promise carries two empty vectors, and a fork of a ledger that does carries the promise with it.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use axiom_core::{Day, Diagnostic, Id, Loc};
use axiom_model::promise::{Promises, Residual};
use axiom_model::{Contract, ScheduleKind, Txn};

use crate::ledger::{Ledger, Upcoming};
use crate::monitor::stream_key;
use crate::motion::{Amounts, Motion};
use crate::plan::Plan;
use crate::{Cause, OccurrenceOutput, OmittedInputs, Planned, Promise, PromisedFlows, TemplateError};

/// Which occurrence of which stream: how a line that keeps it and a forecast that promises it name the same one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Occurrence {
    pub contract: Id<Contract>,
    pub schedule: ScheduleKind,
    /// Its index in its schedule.
    pub ordinal: u32,
    pub due: Day,
}

/// A stream a forecast promises, and what of it is still to be promised.
#[derive(Clone, Copy)]
struct Ahead {
    contract: Id<Contract>,
    schedule: ScheduleKind,
    residual: Residual,
}

/// What a forecast still has to promise, and when each stream's next occurrence falls due.
#[derive(Clone, Default)]
pub(crate) struct Promising {
    ahead: Vec<Ahead>,
    /// The due day of every occurrence a line of the journal keeps, by stream: not to be promised again.
    written: Vec<((usize, bool), Day)>,
    /// Soonest first, one entry for each stream that has an occurrence to come.
    falling: BinaryHeap<Reverse<(Day, u32)>>,
}

impl Promising {
    /// Every stream `wanted` picks of the plan's book, each promising from the day after `after`.
    pub fn start(plan: &Plan, after: Day, wanted: impl Fn(Id<Contract>) -> bool) -> Promising {
        let (book, promises) = (plan.book, &plan.book.promises);
        let kept = plan.occurrence_txns.iter().filter_map(|&id| {
            let txn = book.txns.get(id)?;
            let written = book.written_occurrences.get(txn.occurrence?)?;
            Some((stream_key(txn.contract?, written.schedule), written.due))
        });
        let mut written: Vec<_> = kept.collect();
        written.sort_unstable();
        let mut promising = Promising { written, ..Promising::default() };
        for (contract, promise) in promises.by_contract().filter(|&(contract, _)| wanted(contract)) {
            for schedule in [ScheduleKind::Regular, ScheduleKind::Standing] {
                let Some(stream) = promise.stream(schedule) else { continue };
                let residual = Residual::starting_at(promises, stream.every, after.add_days(1));
                promising.ahead.push(Ahead { contract, schedule, residual });
                promising.arm(promises, promising.ahead.len() - 1);
            }
        }
        promising
    }

    /// The day the next occurrence falls due.
    pub fn next_due(&self) -> Option<Day> {
        self.falling.peek().map(|&Reverse((due, _))| due)
    }

    /// The occurrence that falls due next, and the stream moves on to the one after it.
    pub fn take(&mut self, promises: &Promises) -> Option<Occurrence> {
        let Reverse((due, at)) = self.falling.pop()?;
        let ahead = &mut self.ahead[at as usize];
        let occurrence =
            Occurrence { contract: ahead.contract, schedule: ahead.schedule, ordinal: ahead.residual.ordinal(), due };
        ahead.residual.advance(promises);
        self.arm(promises, at as usize);
        Some(occurrence)
    }

    /// Puts the stream's next occurrence that no line wrote on the heap, passing the ones that were written.
    fn arm(&mut self, promises: &Promises, at: usize) {
        let Promising { ahead, written, falling } = self;
        let Ahead { contract, schedule, residual } = &mut ahead[at];
        while let Some(due) = residual.next() {
            if written.binary_search(&(stream_key(*contract, *schedule), due)).is_err() {
                falling.push(Reverse((due, at as u32)));
                return;
            }
            residual.advance(promises);
        }
    }
}

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
    /// Makes the ledger promise: from the day after the one it stands on, every occurrence of a contract `wanted` picks
    /// that falls due and that no line of the journal keeps is posted on its due day, as a kept one is, and recorded in
    /// [`Recorded::planned`](crate::Recorded::planned). The ledger takes them as it takes the journal's facts: whatever
    /// advances it past a due day posts the occurrence on the way.
    pub fn promise(&mut self, wanted: impl Fn(Id<Contract>) -> bool) {
        self.promising = Promising::start(self.plan, self.clock.day, wanted);
    }

    /// Posts the next occurrence promised on or before `day` and says what it was, after the journal's facts that come
    /// before it; none, with nothing done, if no occurrence falls due by then or before the horizon. It is the step
    /// [`advance`](Ledger::advance) takes without saying, so that a reader who must see each occurrence as it posts (the
    /// balance after it, the flows it made) sees what `advance` does.
    pub fn promise_through(&mut self, day: Day) -> Option<Planned> {
        self.promising.next_due().filter(|&due| due <= day.min(self.horizon))?;
        loop {
            let next = self.upcoming()?;
            self.take(next);
            if matches!(next, Upcoming::Promised(_)) {
                return self.record.planned.last().copied();
            }
        }
    }

    /// The occurrence that falls due next is posted and recorded.
    pub(crate) fn fall_due(&mut self) {
        let Some(occurrence) = self.promising.take(&self.plan.book.promises) else { return };
        let made = self.post_occurrence(occurrence, None, self.clock.day);
        if made.is_ok() {
            self.settle(occurrence);
        }
        let Occurrence { contract, schedule, ordinal, due } = occurrence;
        self.record.planned.push(Planned { contract, schedule, ordinal, due, made });
    }

    /// A line of the journal keeps an occurrence: its flows are posted on the day it was written, and it is a kept one.
    pub(crate) fn post_written_occurrence(&mut self, txn_id: Id<Txn>, day: Day) {
        let Some(occurrence) = self.kept_by(txn_id) else { return };
        match self.post_occurrence(occurrence, Some(txn_id), day) {
            Ok(made) => {
                self.settle(occurrence);
                let Occurrence { contract, schedule, ordinal, due } = occurrence;
                let (flows, missing_inputs) = (made.flows, made.missing_inputs);
                let kept = Some((day, txn_id));
                self.record.promises.push(Promise {
                    contract,
                    schedule,
                    ordinal,
                    due,
                    kept,
                    waived: false,
                    flows,
                    missing_inputs,
                });
            }
            Err(error) => {
                let loc = self.plan.book.txns[txn_id].loc;
                let message = format!("could not materialize this occurrence: {error:?}");
                let diagnostic = Diagnostic::error("contract-occurrence-materialization", message);
                self.record.report(diagnostic.label(loc, "this written occurrence could not be applied"));
            }
        }
    }

    /// The occurrence a line keeps, if the model made the line consistently; else what is wrong is said.
    fn kept_by(&mut self, txn_id: Id<Txn>) -> Option<Occurrence> {
        let book = self.plan.book;
        let txn = book.txns.get(txn_id)?;
        let (Some(contract), Some(schedule), Some(written)) = (txn.contract, txn.contract_schedule, txn.occurrence)
        else {
            self.source_error(txn.loc, "this transaction does not identify a complete contract occurrence");
            return None;
        };
        let Some(due) = book.written_occurrences.get(written).map(|written| written.due) else {
            self.source_error(txn.loc, "this transaction points at a missing occurrence record");
            return None;
        };
        if book.contracts.get(contract).is_none() {
            self.source_error(txn.loc, "this occurrence points at a missing contract");
            return None;
        }
        let ordinal = book.promises.schedule(contract, schedule).and_then(|schedule| schedule.ordinal(due));
        let Some(ordinal) = ordinal else {
            self.source_error(txn.loc, "this occurrence is not part of its contract schedule");
            return None;
        };
        Some(Occurrence { contract, schedule, ordinal, due })
    }

    fn source_error(&mut self, loc: Loc, message: &str) {
        let diagnostic = Diagnostic::error("contract-occurrence-source", message);
        self.record.report(diagnostic.label(loc, "the occurrence cannot be materialized"));
    }

    /// The cause of the next flow a ledger takes that the journal does not hold.
    fn applied(&mut self) -> Cause {
        self.clock.applied += 1;
        Cause::Applied(self.clock.applied - 1)
    }

    /// Tells the monitor the occurrence is kept: it moves past it, and past every earlier one that nothing kept.
    fn settle(&mut self, occurrence: Occurrence) {
        let (promises, record) = (&self.plan.book.promises, &mut self.record);
        let Occurrence { contract, schedule, ordinal, .. } = occurrence;
        self.world.monitor.settle(promises, contract, schedule, ordinal, |missed| record.promises.push(missed));
    }

    /// Makes the occurrence into flows (`instantiate_occurrence`) and posts them on `day`, the day a line kept it or
    /// the day it falls due, keeping them in the record for whoever reads what was made. Said where it came from by
    /// `source`: the line that wrote it, or none.
    fn post_occurrence(
        &mut self,
        occurrence: Occurrence,
        source: Option<Id<Txn>>,
        day: Day,
    ) -> Result<OccurrenceOutput, TemplateError> {
        // Reuse pools between occurrences. Taking them from Scratch keeps the materializer borrow disjoint from the
        // mutable posting path.
        let mut flows = std::mem::take(&mut self.scratch.runtime_flows);
        let mut details = std::mem::take(&mut self.scratch.runtime_details);
        let mut missing = std::mem::take(&mut self.scratch.missing_inputs);
        flows.clear();
        details.truncate(0);
        missing.clear();
        let Occurrence { contract, schedule, ordinal, due } = occurrence;
        let made = self.instantiate_occurrence(
            contract,
            schedule,
            due,
            ordinal,
            source,
            &mut flows,
            &mut details,
            &mut missing,
        );
        let book = self.plan.book;
        let output = made.map(|made| {
            let flow_start = self.record.promised_flows.len();
            let missing_start = self.record.promise_missing_inputs.len();
            for runtime in made.flows(&flows).unwrap_or_default() {
                let mut retained = runtime.clone();
                if let Some(detail) = runtime.detail.and_then(|id| details.get(id).copied()) {
                    retained.detail = Some(self.record.promise_runtime_details.push(detail));
                }
                self.record.promised_flows.push(retained);
                let cause = source.map_or_else(|| self.applied(), Cause::Transaction);
                let view = book.runtime_flow_view(runtime, &details);
                let amounts = Amounts::written(&runtime.flow);
                self.post(&Motion::from_view_at(book, view, runtime.txn, cause, day, amounts, runtime.ordinal));
            }
            self.record.promise_missing_inputs.extend_from_slice(made.missing(&missing).unwrap_or_default());
            OccurrenceOutput {
                flows: PromisedFlows::of(flow_start..self.record.promised_flows.len()),
                missing_inputs: OmittedInputs::of(missing_start..self.record.promise_missing_inputs.len()),
            }
        });
        self.scratch.runtime_flows = flows;
        self.scratch.runtime_details = details;
        self.scratch.missing_inputs = missing;
        output
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::Qty;
    use axiom_model::Book;

    use crate::source_tests::{day, with_book, with_run};
    use crate::{Ledger, Options, Plan};

    const PRELUDE: &str = "\
base USD
commodity USD
  precision 2
entity landlord
entity shop
account checking : asset
opening 2026-01-01
  checking 1_000.00 USD
";

    fn rent(clauses: &str, journal: &str) -> String {
        format!(
            "{PRELUDE}contract rent with landlord\n  100.00 USD monthly on 15 from checking\n  from 2026-01-15\n  until 2026-06-30\n{clauses}{journal}"
        )
    }

    /// A ledger that stands on `today` and promises everything to `until`.
    fn promising<'p, 'b, 's>(plan: &'p Plan<'b, 's>, today: &str, until: &str) -> Ledger<'p, 'b, 's> {
        let (today, until) = (parse(today), parse(until));
        let mut ledger = plan.start(Options { today, relaxed: false });
        ledger.advance(today);
        ledger.reach(until);
        ledger.promise(|_| true);
        ledger
    }

    fn parse(text: &str) -> axiom_core::Day {
        let mut parts = text.split('-').map(|part| part.parse::<u32>().unwrap());
        let (year, month, dom) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        day(year as i32, month, dom)
    }

    fn checking(book: &Book, ledger: &Ledger) -> Qty {
        ledger.balance(book.place("checking").unwrap(), book.base)
    }

    /// What a forecast promised: the due days and ordinals.
    fn planned(ledger: &Ledger) -> Vec<(String, u32)> {
        ledger.recorded().planned.iter().map(|planned| (planned.due.to_string(), planned.ordinal)).collect()
    }

    #[test]
    fn a_ledger_that_promises_posts_what_the_lines_would_have_kept() {
        with_book(&rent("", ""), |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-06-30");
            ledger.advance(parse("2026-06-30"));
            // 01-15, 02-15 and 03-15 are not after today: nothing kept them, and they are no promise of the forecast.
            assert_eq!(
                planned(&ledger),
                [("2026-04-15".into(), 3), ("2026-05-15".into(), 4), ("2026-06-15".into(), 5)]
            );
            assert_eq!(checking(book, &ledger), Qty(70_000));
        });
        // The same book with those three written is the fold that has them in its past: the same holdings.
        let written = rent("", "2026-04-15 rent\n2026-05-15 rent\n2026-06-15 rent\n");
        with_run(&written, day(2026, 6, 30), |book, run| {
            let held = run.holdings.iter().find(|holding| holding.place == book.place("checking").unwrap());
            assert_eq!(held.map(|holding| holding.qty()), Some(Qty(70_000)));
        });
    }

    #[test]
    fn a_line_written_ahead_of_today_is_not_promised_again() {
        let ahead = rent("", "2026-04-15 rent\n");
        with_book(&ahead, |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-06-30");
            ledger.advance(parse("2026-06-30"));
            assert_eq!(planned(&ledger), [("2026-05-15".into(), 4), ("2026-06-15".into(), 5)]);
            // The line posts as the fold reaches it, and the two promises after it: three payments in all.
            assert_eq!(checking(book, &ledger), Qty(70_000));
        });
    }

    #[test]
    fn a_line_written_early_for_a_day_after_today_is_not_promised_either() {
        // 2026-04-12 keeps the due day 04-15, and is itself before the day the ledger stands on.
        let early = rent("", "2026-04-12 rent\n");
        with_book(&early, |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-04-13", "2026-06-30");
            ledger.advance(parse("2026-06-30"));
            assert_eq!(planned(&ledger), [("2026-05-15".into(), 4), ("2026-06-15".into(), 5)]);
        });
    }

    #[test]
    fn nothing_falls_due_in_a_ledger_that_was_not_asked_to_promise() {
        with_book(&rent("", ""), |book| {
            let plan = Plan::new(book);
            let mut ledger = plan.start(Options { today: parse("2026-03-15"), relaxed: false });
            ledger.advance(parse("2026-03-15"));
            ledger.reach(parse("2026-06-30"));
            ledger.advance(parse("2026-06-30"));
            assert!(ledger.recorded().planned.is_empty());
            assert_eq!(checking(book, &ledger), Qty(100_000));
            assert!(ledger.promise_through(parse("2026-06-30")).is_none());
        });
    }

    #[test]
    fn what_is_due_on_the_day_a_ledger_stands_on_is_not_promised() {
        with_book(&rent("", ""), |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-03-31");
            ledger.advance(parse("2026-03-31"));
            assert!(planned(&ledger).is_empty(), "03-15 is today; 04-15 is past the horizon");
        });
    }

    #[test]
    fn the_horizon_is_the_last_day_an_occurrence_is_promised_on() {
        with_book(&rent("", ""), |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-05-15");
            ledger.advance(parse("2026-12-31"));
            assert_eq!(planned(&ledger), [("2026-04-15".into(), 3), ("2026-05-15".into(), 4)]);
        });
    }

    #[test]
    fn one_step_posts_the_next_occurrence_after_the_journal_facts_before_it() {
        let journal = "2026-05-10 checking -> shop 50.00 USD\n";
        with_book(&rent("", journal), |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-06-30");
            let first = ledger.promise_through(parse("2026-06-30")).unwrap();
            assert_eq!((first.due, first.ordinal), (parse("2026-04-15"), 3));
            assert_eq!(checking(book, &ledger), Qty(90_000), "the flow of 05-10 is not folded yet");
            assert!(ledger.promise_through(parse("2026-05-01")).is_none(), "nothing falls due by then");
            let second = ledger.promise_through(parse("2026-05-15")).unwrap();
            assert_eq!(second.due, parse("2026-05-15"));
            assert_eq!(checking(book, &ledger), Qty(75_000), "the flow of 05-10, then the rent of 05-15");
            assert_eq!(ledger.recorded().planned.len(), 2);
        });
    }

    #[test]
    fn what_a_step_made_is_in_the_pools_the_record_holds() {
        with_book(&rent("", ""), |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-03-15", "2026-04-30");
            let planned = ledger.promise_through(parse("2026-04-30")).unwrap();
            let recorded = ledger.recorded();
            let flows = planned.made.unwrap().flows(recorded.promised_flows).unwrap();
            assert_eq!((flows.len(), flows[0].flow.day, flows[0].flow.out.qty), (1, parse("2026-04-15"), Qty(10_000)));
        });
    }

    #[test]
    fn a_loan_is_promised_to_its_last_payment_and_no_further() {
        let text = format!(
            "{PRELUDE}contract car with shop\n  loan 3_000.00 USD on 2026-01-01 at 0% over 3m\n  monthly on 1 from checking\n  from 2026-01-01\n"
        );
        with_book(&text, |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-01-02", "2026-12-31");
            ledger.advance(parse("2026-12-31"));
            assert_eq!(
                planned(&ledger),
                [("2026-02-01".into(), 1), ("2026-03-01".into(), 2), ("2026-04-01".into(), 3)]
            );
        });
    }

    #[test]
    fn only_the_contracts_asked_for_are_promised() {
        let text = format!(
            "{PRELUDE}contract rent with landlord\n  100.00 USD monthly on 15 from checking\n  from 2026-01-15\ncontract gym with shop\n  30.00 USD monthly on 10 from checking\n  from 2026-01-10\n"
        );
        with_book(&text, |book| {
            let plan = Plan::new(book);
            let today = parse("2026-03-01");
            let mut ledger = plan.start(Options { today, relaxed: false });
            ledger.advance(today);
            ledger.reach(parse("2026-04-30"));
            let gym = book.contract("gym").unwrap();
            ledger.promise(|contract| contract == gym);
            ledger.advance(parse("2026-04-30"));
            assert_eq!(planned(&ledger), [("2026-03-10".into(), 2), ("2026-04-10".into(), 3)]);
        });
    }

    #[test]
    fn a_stream_promises_beyond_a_day_it_is_still_waiting_on() {
        // A grace of two months keeps 03-15 open on 04-01: the monitor waits on it, and the forecast promises 04-15.
        let text = rent("  grace 60d\n", "");
        with_book(&text, |book| {
            let plan = Plan::new(book);
            let mut ledger = promising(&plan, "2026-04-01", "2026-05-31");
            ledger.advance(parse("2026-05-31"));
            assert_eq!(planned(&ledger), [("2026-04-15".into(), 3), ("2026-05-15".into(), 4)]);
        });
    }
}
