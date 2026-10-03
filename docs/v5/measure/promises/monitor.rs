//! What the fold recorded of every promise, held to the reference.
//!
//! Every occurrence a journal line kept must be the occurrence the reference says that line keeps, with the ordinal the
//! reference counts. Every occurrence the fold says was missed must be one the reference says is: owed, after the day
//! the book begins, kept by no line, and either past its reach at the run's horizon or before a due day some line kept
//! (no line dated later can keep it). And no other is: the two sets are equal. A loan's stream owes its payments and no
//! more.

use std::collections::BTreeSet;

use axiom_core::{Cadence, Day, Days, Span};
use axiom_engine::Run;
use axiom_model::{Book, Contract, Loan, ScheduleKind};

use crate::check::{Reference, Tally};
use crate::reading::Keep;

/// The days the book's own facts are on: its first (the day the monitor starts) and its last.
fn book_days(book: &Book<'_>) -> (Option<Day>, Option<Day>) {
    let flows = book.flows.iter().map(|(_, flow)| flow.day);
    let occurrences = book.txns.iter().filter_map(|(_, txn)| txn.occurrence.map(|_| txn.day));
    let asserts = book.asserts.iter().map(|assert| assert.day);
    let splits = book.splits.iter().map(|split| split.day);
    let claims = book.claim_changes.iter().map(|change| change.day);
    let days: Vec<Day> = flows.chain(occurrences).chain(asserts).chain(splits).chain(claims).collect();
    (days.iter().copied().min(), days.iter().copied().max())
}

/// Holds what `run` recorded of each contract's promises to the reference.
pub fn check(book: &Book<'_>, run: &Run, slow: bool, tally: &mut Tally) {
    let (first, last) = book_days(book);
    let horizon = last.map_or(run.today, |last| last.max(run.today));
    for (id, contract) in book.contracts.iter() {
        let reference = Reference { id, contract };
        let name = || book.name(contract.name).to_string();
        let entries: Vec<_> = run.promises.iter().filter(|promise| promise.contract == id).collect();
        for promise in entries.iter().filter(|promise| promise.kept.is_some()) {
            let (day, _) = promise.kept.expect("kept");
            let said = Keep::Kept(promise.schedule, promise.due);
            let expected = reference.keep(day);
            tally.new("kept line", expected == said, || format!("{} {day}: {said} against {expected}", name()));
            let ordinal = reference.ordinal(promise.schedule, promise.due);
            tally.new("kept ordinal", ordinal == Some(promise.ordinal), || {
                format!("{} {:?} {}: {} against {ordinal:?}", name(), promise.schedule, promise.due, promise.ordinal)
            });
        }
        for kind in [ScheduleKind::Regular, ScheduleKind::Standing] {
            if !watched(&reference, kind, slow) {
                continue;
            }
            let kept: BTreeSet<Day> = entries
                .iter()
                .filter(|promise| promise.schedule == kind && promise.kept.is_some())
                .map(|promise| promise.due)
                .collect();
            let missed: Vec<Day> = {
                let mut said: Vec<Day> = entries
                    .iter()
                    .filter(|promise| promise.schedule == kind && promise.kept.is_none())
                    .map(|promise| promise.due)
                    .collect();
                said.sort_unstable();
                said
            };
            let expected = expected_missed(&reference, kind, first.unwrap_or(run.today), horizon, &kept);
            tally.new("missed", expected == missed, || {
                format!("{} {kind:?}: the fold says {missed:?}, the reference {expected:?}", name())
            });
        }
    }
}

/// Whether the stream is one the reference can judge: not a loan with no first day, whose payments it would have to count
/// from the beginning of time (`--slow` asks them).
fn watched(reference: &Reference<'_>, kind: ScheduleKind, slow: bool) -> bool {
    let contract = reference.contract;
    let unbounded = contract.days.first() == Day::MIN;
    contract.terms_of(kind).is_some() && !(contract.loan.is_some() && kind == ScheduleKind::Regular && unbounded && !slow)
}

/// The days of the stream the reference says were missed: its payments (all of its owed days, or a loan's) from the day
/// the book began that no line kept, and that are past their reach, and their deadline if the terms say one (`due 5d`),
/// at `horizon` or before a day that was kept.
fn expected_missed(
    reference: &Reference<'_>,
    kind: ScheduleKind,
    start: Day,
    horizon: Day,
    kept: &BTreeSet<Day>,
) -> Vec<Day> {
    let contract = reference.contract;
    let reach = reference.reach(kind);
    // A loan's payments are counted from the contract's first day; any other stream is looked at from the book's.
    let is_loan = contract.loan.is_some() && kind == ScheduleKind::Regular;
    let from = if is_loan { contract.days.first() } else { contract.days.first().max(start) };
    let Some(all) = Days::new(from, horizon) else { return Vec::new() };
    let owed = reference.owed(kind, all);
    let payments = payments(contract, kind, &owed);
    let latest_kept = kept.iter().next_back().copied();
    let deadline = contract.terms_of(kind).and_then(|terms| terms.due.as_ref()).map(|due| due.after);
    // The last day a line may keep it, or the party may pay it in time, whichever is later.
    let last_day = |due: Day| {
        let in_time = deadline.and_then(|after| due.checked_add(after)).map_or(i64::MIN, |day| i64::from(day.0));
        (i64::from(due.0) + reach).max(in_time)
    };
    payments
        .iter()
        .copied()
        .filter(|due| *due >= start && !kept.contains(due))
        .filter(|due| last_day(*due) < i64::from(horizon.0) || latest_kept.is_some_and(|kept| kept > *due))
        .collect()
}

/// The owed days a stream pays: a loan's payments, from the first after the loan was made and as many as its term holds,
/// else every one.
fn payments<'a>(contract: &Contract, kind: ScheduleKind, owed: &'a [Day]) -> &'a [Day] {
    let loan = contract.loan.as_ref().filter(|_| kind == ScheduleKind::Regular);
    let every = contract.terms_of(kind).map(|terms| terms.every);
    let Some((loan, count)) = loan.zip(every).and_then(|(loan, every)| Some((loan, periods(loan, every)?))) else {
        return owed;
    };
    let began = owed.partition_point(|day| *day <= loan.on);
    &owed[began..owed.len().min(began + count as usize)]
}

/// How many payments pay off `loan` when they are made every `every`: as many periods as its term holds, a part of one
/// being one. None for a cadence a payment has no meaning for.
fn periods(loan: &Loan, every: Cadence) -> Option<u32> {
    let periods = |term: i32, step: i32| (term + step - 1) / step;
    let count = match every {
        Cadence::Every(Span { months, days: 0 }) if months > 0 => periods(loan.term.months, months),
        Cadence::Every(Span { months: 0, days }) if days > 0 => periods(loan.term.days, days),
        Cadence::TwiceMonthly => loan.term.months * 2,
        Cadence::Every(_) => return None,
    };
    u32::try_from(count).ok().filter(|count| *count > 0 && *count <= 100_000)
}
