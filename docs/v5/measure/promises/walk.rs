//! The residual walked: from the first day a stream owes, one occurrence after another, as a fold will take them.
//!
//! Nothing but this reads a `Residual`, so there is no old answer to compare it with and it is held to the reference
//! alone (check.rs): the occurrence it waits for is the reference's next owed day, with the reference's ordinal, until
//! the stream owes nothing more. A loan's payments begin at the first owed day after the loan was made, there are as many
//! as the term holds periods of the cadence (a part of a period is one), and the loan is done after the last, whatever the
//! schedule would go on to say. What a loan owes after each is the loan's schedule's to say (K5d, `loans.py` holds it to
//! its own reference): here it is asked that each leaves no more owed than the last and that the last leaves nothing.

use axiom_core::{Cadence, Day, Days, Dues, Span};
use axiom_model::promise::{Residual, Stream, Term};
use axiom_model::{Book, Loan, ScheduleKind};

use crate::ask::Facts;
use crate::check::{Reference, Tally};

/// How many occurrences of a stream are walked, and how many days from its start they are looked for in: ten years of
/// daily ones is more than a hundred times what the walk takes, and a yearly stream's are all in a century.
const STEPS: usize = 48;
const HORIZON: i32 = 40_000;

/// Walks every stream of the contract.
pub fn walk(book: &Book<'_>, facts: &Facts<'_>, reference: &Reference<'_>, tally: &mut Tally) {
    let promise = book.promises.of(reference.id);
    for (kind, stream) in [(ScheduleKind::Regular, promise.regular), (ScheduleKind::Standing, promise.standing)] {
        if let Some(stream) = stream {
            walk_stream(book, facts, reference, kind, stream, tally);
        }
    }
}

fn walk_stream(
    book: &Book<'_>,
    facts: &Facts<'_>,
    reference: &Reference<'_>,
    kind: ScheduleKind,
    stream: Stream,
    tally: &mut Tally,
) {
    let contract = reference.contract;
    let Some(declared) = contract.terms_of(kind) else { return };
    // A cadence that can only be walked is walked from its first day, which for a contract with no `from` is the
    // beginning of time: the walk is not asked of it (the old ordinal is the slow question that does).
    if !Dues::new(declared.every, &declared.on, contract.days.first()).is_counted() && contract.days.first() == Day::MIN {
        return;
    }
    let loan = contract.loan.as_ref().filter(|_| kind == ScheduleKind::Regular);
    // The first payment of a loan is counted from the contract's first day, as the old ordinal is: 71 million owed days
    // for a loan with no `from`, which the reference has to list. Only `--slow` asks it.
    if loan.is_some() && contract.days.first() == Day::MIN && !facts.slow {
        return;
    }
    let payments = loan.and_then(|loan| periods(loan, declared.every)).filter(|periods| *periods <= 100_000);
    let name = || book.name(contract.name).to_string();
    let Term::Every { body, .. } = book.promises.term(stream.every) else { return };
    let has_annuity = book.promises.annuity_of(body).is_some();
    tally.new("residual loan", has_annuity == payments.is_some(), || format!("{}: annuity {has_annuity}", name()));

    let start = loan.map_or(contract.days.first(), |loan| loan.on.max(contract.days.first()));
    let Some(window) = Days::new(contract.days.first(), Day(start.0.saturating_add(HORIZON))) else { return };
    let owed = reference.owed(kind, window);
    let made = loan.filter(|_| payments.is_some()).map(|loan| loan.on);
    let began = made.map_or(0, |made| owed.partition_point(|day| *day <= made));
    let mut expected: Vec<(Day, u32)> =
        (began..).zip(&owed[began..]).map(|(ordinal, day)| (*day, ordinal as u32)).collect();
    expected.truncate(payments.map_or(STEPS, |periods| STEPS.min(periods as usize)));

    let schedule = book.promises.loan(reference.id).filter(|_| payments.is_some());
    let mut residual = Residual::start(&book.promises, stream.every);
    let mut open = None;
    for (day, ordinal) in &expected {
        let (next, at) = (residual.next(), residual.ordinal());
        tally.new("residual", next == Some(*day) && at == *ordinal, || {
            format!("{} {kind:?}: waits for {next:?} (ordinal {at}), the reference for {day} ({ordinal})", name())
        });
        if let Some(schedule) = schedule {
            let owes = schedule.paid_on(*day).map(|paid| paid.open);
            tally.new("residual open", owes.is_ok_and(|owes| owes.0 >= 0 && open.is_none_or(|before| owes <= before)), || {
                format!("{} {kind:?} {day}: {owes:?} owed after {open:?}", name())
            });
            open = owes.ok();
        }
        residual.advance(&book.promises);
    }
    // After the last occurrence it is done if the reference has no more of them to wait for: the loan has been paid, or
    // the days it looked in reached the end of the contract.
    let paid = payments.is_some_and(|periods| expected.len() == periods as usize);
    let ended = expected.len() < STEPS && window.last() >= contract.days.last();
    if paid || ended {
        tally.new("residual end", residual.is_done(), || {
            format!("{} {kind:?}: not done after {} days", name(), expected.len())
        });
    }
    if let Some(schedule) = schedule.filter(|_| paid) {
        tally.new("residual repaid", open.is_some_and(|owes| owes.0 == 0), || {
            format!("{} {kind:?}: owes {:?} after {:?}", name(), open, schedule.entries().last().map(|entry| entry.day))
        });
    }
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
    u32::try_from(count).ok().filter(|count| *count > 0)
}
