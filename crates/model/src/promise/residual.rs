//! What is still owed of one stream of a promise.
//!
//! A residual is a cursor into the [`Promises`](super::Promises) terms and never a copy of them: the `Every` it is
//! on, the day the next occurrence falls due, that occurrence's ordinal, and how much of a loan is still owed. It is 24
//! bytes, so a fold keeps one for every schedule of every promise in a dense array and moves each with a store. It
//! advances when the occurrence it waits for has been kept or missed; when nothing more is owed its term is
//! [`Promises::DONE`].

use axiom_core::{Day, Qty};

use super::{Promises, Term, TermId};

/// The part of a promise that is still owed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Residual {
    term: TermId,
    next: Day,
    ordinal: u32,
    /// The ordinal the first payment of a loan has: payment `n` of the loan is the occurrence of ordinal `began + n`.
    began: u32,
    /// What a loan still owes: its principal before the first payment, zero after the last. Zero for what is no loan.
    open: Qty,
}

const _: () = assert!(size_of::<Residual>() <= 24);

impl Residual {
    /// What is owed of the stream that `every` is, before anything has been kept: its first occurrence is the first due
    /// day, or, for a loan, the first after the day the loan was made. Done if nothing is ever due.
    pub fn start(promises: &Promises, every: TermId) -> Residual {
        let Term::Every { schedule, body } = promises.term(every) else { return Residual::done() };
        let schedule = promises.schedule_of(schedule);
        let annuity = promises.annuity_of(body).map(|annuity| promises.annuity(annuity));
        let began = annuity.map_or(0, |annuity| schedule.before(Day(annuity.begins().0.saturating_add(1))));
        let open = annuity.map_or(Qty::ZERO, |annuity| annuity.principal().qty);
        let first = schedule.nth(began);
        first.map_or(Residual::done(), |next| Residual { term: every, next, ordinal: began, began, open })
    }

    /// Nothing owed.
    pub fn done() -> Residual {
        Residual { term: Promises::DONE, next: Day::MIN, ordinal: 0, began: 0, open: Qty::ZERO }
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

    /// What a loan still owes before the next payment.
    pub fn open(&self) -> Qty {
        self.open
    }

    /// The last day the next occurrence may be kept by, if its promise has a deadline.
    pub fn deadline(&self, promises: &Promises) -> Option<Day> {
        let Term::Every { body, .. } = promises.term(self.term) else { return None };
        let deadline = promises.deadline_of(body)?;
        self.next.checked_add(promises.deadline(deadline).after)
    }

    /// The occurrence this waits for has been kept or missed: wait for the one after it. A loan that the payment has
    /// paid off is done, whatever the schedule would go on to say. The last payment of a loan pays off what is left
    /// (see [`Annuity::pay`](super::Annuity::pay)), so a loan is done when nothing is owed, and one whose payment cannot be worked out is
    /// not waited for again.
    pub fn advance(&mut self, promises: &Promises) {
        let Term::Every { schedule, body } = promises.term(self.term) else { return };
        let paid_off = promises.annuity_of(body).is_some_and(|annuity| {
            let paid = promises.annuity(annuity).pay(self.open, self.ordinal - self.began);
            self.open = paid.map_or(Qty::ZERO, |paid| paid.open);
            self.open == Qty::ZERO
        });
        self.ordinal = self.ordinal.saturating_add(1);
        match promises.schedule_of(schedule).nth(self.ordinal).filter(|_| !paid_off) {
            Some(next) => self.next = next,
            None => *self = Residual::done(),
        }
    }
}
