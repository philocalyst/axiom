//! Balance assertions, checked at the end of their day.
//!
//! An assertion is about the place itself and one commodity, not the place's
//! subtree: `assets/bank` asserts what sits directly in `assets/bank`, and an
//! account that holds its money in sub-accounts is asserted account by account.
//! It is written in the place's display sign (`visa = 1_234.56 USD` is 1,234.56
//! owed), so the balance is converted before comparing.
//!
//! A mismatch is an error naming the gap and the flows since the last time the
//! assertion held. With `!` the gap is accepted as an explicit externality: it
//! moves from `unknown` into the place, so later assertions hold, and is
//! recorded as a [`Pad`].

use axiom_model::Amount;

use crate::ledger::Ledger;
use crate::scope::display;
use crate::{Pad, explain};

impl<'b, 's> Ledger<'b, 's> {
    pub(crate) fn reconcile(&mut self, index: usize) {
        let book = self.book;
        let assert = &book.asserts[index];
        let (place, unit) = (assert.place, assert.amount.unit);
        let shown = display(book, place, self.world.holdings.qty(place, unit));
        let gap = assert.amount.qty - shown;
        if gap.is_zero() {
            self.record.reconciled.insert((place, unit), assert.day);
            return;
        }
        let Some(waive) = assert.pad else {
            let since = self.record.reconciled.get(&(place, unit)).copied();
            let diagnostic = explain::mismatch(book, &self.solved.events, assert, shown, since);
            self.record.report(diagnostic);
            return;
        };
        // The pad is what moves into the place, in balance terms.
        let moved = display(book, place, gap);
        self.world.holdings.credit(place, unit, moved);
        self.world.holdings.credit(book.roots.unknown, unit, -moved);
        let amount = Amount::new(moved, unit);
        self.record.pads.push(Pad { assert: index as u32, place, amount, day: assert.day });
        self.record.reconciled.insert((place, unit), assert.day);
        self.record.report(explain::padded(book, assert, waive, amount));
    }
}
