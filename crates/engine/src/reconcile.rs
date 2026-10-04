//! Balance assertions, checked at the end of their day.
//!
//! An assertion is about the place itself and one commodity, not the place's
//! subtree: `assets/bank` asserts what sits directly in `assets/bank`, and an
//! account that holds its money in sub-accounts is asserted account by account.
//! It is written in the place's display sign (`visa = 1_234.56 USD` is 1,234.56
//! owed), so the balance is converted before comparing.
//!
//! A mismatch is an error naming the gap and the flows since the last checkpoint.
//! The gap is carried: a later assertion that fails by the same amount says
//! nothing, and one that fails by another reports only what is new. An
//! assertion that depends on an amount that could not be solved is not judged.
//! With `!` the gap is accepted as an explicit externality, and with `via` it
//! is booked to that place: either way it is posted as a flow, so parcels,
//! basis and laws see it like any other, and recorded as a [`Pad`].

use axiom_core::{Days, Id, Qty};
use axiom_model::{Amount, Assert, Fault, Gap, Place, Value, Waive};

use crate::eval::{self, Context, Env, Occasion};
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::scope::owner_of;
use crate::state::LastCheck;
use crate::{Pad, explain, loan_balance};

impl Ledger<'_, '_, '_> {
    pub(crate) fn reconcile(&mut self, index: usize) {
        let book = self.plan.book;
        let Some(amount) = self.assertion_amount(index) else {
            return;
        };
        let assert = with_amount(&book.asserts[index], amount);
        let (place, unit) = (assert.place, assert.amount.unit);
        let shown = self.plan.sides.display(place, self.world.holdings.qty(place, unit));
        let gap = assert.amount.qty - shown;
        let last = self.record.checkpoints.get(&(place, unit)).copied().unwrap_or_default();
        let now = LastCheck { day: Some(assert.day), gap, unsolved_said: last.unsolved_said };
        let schedule = self.schedule_disagrees(&assert);
        let now = match (assert.gap, gap.is_zero()) {
            (_, true) => now,
            // The loan's diagnostic says it better where the book and the schedule agree with each other.
            (Gap::Refused, false) if schedule.as_ref().is_some_and(|(_, found)| found.gap() == gap) => now,
            (Gap::Refused, false) => self.refuse(&assert, shown, last, now),
            (Gap::Unexplained(waive), false) => {
                let unknown = book.entities[book.roots.unknown].place;
                self.pad(
                    index,
                    &assert,
                    gap,
                    unknown.expect("the unknown entity owns its balancing place"),
                    Some(waive),
                );
                LastCheck { gap: Qty::ZERO, ..now }
            }
            (Gap::Via { place: counter, .. }, false) => {
                self.pad(index, &assert, gap, counter, None);
                LastCheck { gap: Qty::ZERO, ..now }
            }
        };
        self.record.checkpoints.insert((place, unit), now);
        if let Some((contract, found)) = schedule {
            self.record.report(loan_balance::disagreement(book, &assert, contract, &found));
        }
    }

    /// A gap nobody accepted. It is reported once: not when an amount that could not be solved may be to blame
    /// and has been said to be, and not when it is the gap that was reported before. Returns what is now known
    /// of the place's checks.
    fn refuse(&mut self, assert: &Assert, shown: Qty, last: LastCheck, now: LastCheck) -> LastCheck {
        let book = self.plan.book;
        let (place, unit) = (assert.place, assert.amount.unit);
        let blame =
            self.plan.unsolved.get(&(place, unit)).filter(|&&(day, _)| day <= assert.day).map(|&(_, flow)| flow);
        match blame.filter(|_| !last.unsolved_said) {
            Some(unknown) => {
                self.record.report(explain::unchecked(book, assert, book.flows[unknown].loc));
                LastCheck { unsolved_said: true, ..now }
            }
            None if now.gap == last.gap => now,
            None => {
                let others: Vec<_> = self
                    .world
                    .holdings
                    .of(place)
                    .map(|slot| (slot.unit, self.plan.sides.display(place, slot.qty)))
                    .collect();
                let report = explain::mismatch(
                    book,
                    &self.plan.events,
                    (assert, self.plan.sides.sign(place)),
                    (shown, now.gap - last.gap),
                    last.day,
                    &others,
                );
                self.record.report(report);
                now
            }
        }
    }

    /// Computes a statement amount at the statement's day. Literal assertions
    /// take the compact direct path; computed roots share the evaluator and its
    /// scratch buffer with laws and contract templates.
    fn assertion_amount(&mut self, index: usize) -> Option<Amount> {
        let (book, assertion) = (self.plan.book, &self.plan.book.asserts[index]);
        let Some((program, root)) = assertion.computed else {
            return Some(assertion.amount);
        };
        let subject = assertion.subject;
        let owner = owner_of(book, subject);
        let on = Occasion::time(assertion.day, Days::on(assertion.day));
        let ctx = Context::new(subject, owner, &on);
        let value = eval::program_expression(
            Env { plan: self.plan, world: &self.world },
            &book.assertion_programs[program],
            root,
            &ctx,
            &mut self.scratch.values,
        );
        match value {
            Value::Amount(amount) => Some(amount),
            Value::Fault(fault) => {
                self.record.report(explain::assertion_fault(book, assertion, fault));
                None
            }
            _ => {
                self.record.report(explain::assertion_fault(book, assertion, Fault::InvalidProgram));
                None
            }
        }
    }

    /// Posts the gap as a flow between the asserted place and `counter`.
    fn pad(&mut self, index: usize, assert: &Assert, gap: Qty, counter: Id<Place>, waive: Option<Waive>) {
        let book = self.plan.book;
        // What moves into the place, in balance terms.
        let moved = self.plan.sides.display(assert.place, gap);
        self.post(&Motion::pad(book, assert, counter, moved, waive));
        let amount = Amount::new(moved, assert.amount.unit);
        self.record.pads.push(Pad { assert: index as u32, place: assert.place, counter, amount, day: assert.day });
        if let Some(waive) = waive {
            let note = explain::padded(book, assert, waive, amount);
            self.record.report(note);
        }
    }
}

/// The assertion as written, with the amount it says: a computed one is evaluated first.
fn with_amount(source: &Assert, amount: Amount) -> Assert {
    Assert {
        day: source.day,
        place: source.place,
        subject: source.subject,
        amount,
        computed: source.computed,
        gap: source.gap,
        loc: source.loc,
    }
}
