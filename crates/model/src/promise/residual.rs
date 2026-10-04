//! What is still owed of one stream of a promise.
//!
//! A residual is a cursor into the [`Promises`](super::Promises) terms and never a copy of them: the `Every` it is
//! on, the day the next occurrence falls due, and that occurrence's ordinal. It is 12 bytes, so a fold keeps one for every
//! schedule of every promise in a dense array and moves each with a store. It advances when the occurrence it waits for has
//! been kept or missed; when nothing more is owed its term is [`Promises::DONE`].
//!
//! A loan's payments are a schedule of their own ([`Amortization`](super::Amortization)), walked once, and a stream that
//! pays one is owed exactly the payments it has: the cursor is past the last when no payment is at its ordinal, whether the
//! term ran out or a prepayment paid the loan off. What a loan owes after any of them is that schedule's to say, not the
//! cursor's, so that however a fold meets a loan's facts it asks the same walk.

use axiom_core::Day;

use super::{Promises, Term, TermId};

/// The part of a promise that is still owed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Residual {
    term: TermId,
    next: Day,
    ordinal: u32,
}

const _: () = assert!(size_of::<Residual>() <= 12);

impl Residual {
    /// What is owed of the stream that `every` is, before anything has been kept: its first occurrence is the first due
    /// day, or, for a loan, the first after the day the loan was made. Done if nothing is ever due.
    pub fn start(promises: &Promises, every: TermId) -> Residual {
        Residual::starting_at(promises, every, Day::MIN)
    }

    /// What is owed of the stream from `day` on: its first occurrence is the first owed day on or after `day` (and, for a
    /// loan, after the day the loan was made), with the ordinal it has in the schedule. A contract with no `from` does not
    /// walk from the beginning of time to get here.
    pub fn starting_at(promises: &Promises, every: TermId, day: Day) -> Residual {
        let Term::Every { schedule, body } = promises.term(every) else { return Residual::done() };
        let first = promises.annuity_of(body).map_or(0, |annuity| promises.annuity(annuity).first());
        Residual::at(promises, every, promises.schedule_of(schedule).before(day).max(first))
    }

    /// The residual of the stream that `every` is, waiting for the occurrence of `ordinal`; done if there is none.
    fn at(promises: &Promises, every: TermId, ordinal: u32) -> Residual {
        let Term::Every { schedule, body } = promises.term(every) else { return Residual::done() };
        let owed = promises.annuity_of(body).is_none_or(|annuity| promises.annuity(annuity).pays(ordinal));
        let next = promises.schedule_of(schedule).nth(ordinal).filter(|_| owed);
        next.map_or(Residual::done(), |next| Residual { term: every, next, ordinal })
    }

    /// Nothing owed.
    pub fn done() -> Residual {
        Residual { term: Promises::DONE, next: Day::MIN, ordinal: 0 }
    }

    pub fn is_done(&self) -> bool {
        self.term == Promises::DONE
    }

    /// The day the next occurrence falls due.
    pub fn next(&self) -> Option<Day> {
        (!self.is_done()).then_some(self.next)
    }

    /// The next occurrence's index in its schedule: the same in the journal and in a forecast.
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// The day the deadline of the next occurrence passes, if its promise has one.
    pub fn deadline(&self, promises: &Promises) -> Option<Day> {
        let Term::Every { body, .. } = promises.term(self.term) else { return None };
        self.next.checked_add(promises.deadline_of(body)?)
    }

    /// The occurrence this waits for has been kept or missed: wait for the one after it. A loan is done after its last
    /// payment, which is the one that pays it off.
    pub fn advance(&mut self, promises: &Promises) {
        if !self.is_done() {
            *self = Residual::at(promises, self.term, self.ordinal.saturating_add(1));
        }
    }
}
