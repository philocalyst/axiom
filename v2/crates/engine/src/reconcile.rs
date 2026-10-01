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

use axiom_core::{Id, Qty};
use axiom_model::{Amount, Gap, Place, Waive};

use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::state::LastCheck;
use crate::{Pad, explain};

impl Ledger<'_, '_, '_> {
    pub(crate) fn reconcile(&mut self, index: usize) {
        let book = self.plan.book;
        let assert = &book.asserts[index];
        let (place, unit) = (assert.place, assert.amount.unit);
        let shown = self
            .plan
            .sides
            .display(place, self.world.holdings.qty(place, unit));
        let gap = assert.amount.qty - shown;
        let last = self
            .record
            .checkpoints
            .get(&(place, unit))
            .copied()
            .unwrap_or_default();
        let now = LastCheck {
            day: Some(assert.day),
            gap,
            unsolved_said: last.unsolved_said,
        };
        let blame = self
            .plan
            .unsolved
            .get(&(place, unit))
            .filter(|&&(day, _)| day <= assert.day)
            .map(|&(_, flow)| flow);
        let now = match (assert.gap, gap.is_zero()) {
            (_, true) => now,
            (Gap::Refused, false) => match blame.filter(|_| !last.unsolved_said) {
                Some(unknown) => {
                    self.record
                        .report(explain::unchecked(book, assert, book.flows[unknown].loc));
                    LastCheck {
                        unsolved_said: true,
                        ..now
                    }
                }
                None if gap == last.gap => now,
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
                        (shown, gap - last.gap),
                        last.day,
                        &others,
                    );
                    self.record.report(report);
                    now
                }
            },
            (Gap::Unexplained(waive), false) => {
                self.pad(
                    index,
                    gap,
                    book.entities[book.roots.unknown]
                        .place
                        .expect("the unknown entity owns its balancing place"),
                    Some(waive),
                );
                LastCheck {
                    gap: Qty::ZERO,
                    ..now
                }
            }
            (Gap::Via { place: counter, .. }, false) => {
                self.pad(index, gap, counter, None);
                LastCheck {
                    gap: Qty::ZERO,
                    ..now
                }
            }
        };
        self.record.checkpoints.insert((place, unit), now);
    }

    /// Posts the gap as a flow between the asserted place and `counter`.
    fn pad(&mut self, index: usize, gap: Qty, counter: Id<Place>, waive: Option<Waive>) {
        let (book, assert) = (self.plan.book, &self.plan.book.asserts[index]);
        // What moves into the place, in balance terms.
        let moved = self.plan.sides.display(assert.place, gap);
        self.post(&Motion::pad(book, assert, counter, moved, waive));
        let amount = Amount::new(moved, assert.amount.unit);
        self.record.pads.push(Pad {
            assert: index as u32,
            place: assert.place,
            counter,
            amount,
            day: assert.day,
        });
        if let Some(waive) = waive {
            let note = explain::padded(book, assert, waive, amount);
            self.record.report(note);
        }
    }
}
