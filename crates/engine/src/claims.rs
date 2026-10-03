//! Forgiving a claim: `^code waived`.
//!
//! A claim is a parcel in a claim place, so forgiving it is relief: every parcel the named transaction made is taken out
//! of the place that holds it, and the value goes back to the place it came from, as if the party had never been paid
//! into the claim. No money moves and no law watches it (a [`ClaimChange`] is "a source event that changes an
//! already-open claim without inventing a monetary transaction"); what the owner holds falls by what was forgiven,
//! and `claims`, `balance` and `overdue` see it because they read the parcels. What the claim recognized when it was
//! made is not reversed: that depends on whether the books are cash or accrual, which nothing reads yet.

use axiom_core::Qty;
use axiom_model::{Flow, Policy, RuntimeTxn, Select};

use crate::WriteOff;
use crate::explain;
use crate::ledger::Ledger;
use crate::lots::Request;

impl Ledger<'_, '_, '_> {
    /// Forgives what is still open of the claim `Book::claim_changes[at]` names, wherever the transaction made it: the
    /// first line of an itemized claim takes all its parcels, and the others find nothing left.
    pub(crate) fn write_off(&mut self, at: u32) {
        let book = self.plan.book;
        let target = book.claim_changes[at as usize].target;
        let made = book.flows[book.txns[target].flows].iter().filter(|flow| book.makes_claim(flow));
        let forgiven: usize = made.map(|flow| self.forgive(at, flow)).sum();
        if forgiven == 0 {
            self.record.report(explain::empty_write_off(book, &book.claim_changes[at as usize]));
        }
    }

    /// Takes the parcels the write-off `at` makes of one flow of the claim out of the place that flow paid into, gives
    /// their value back to the place it came from, and says how many parcels that was.
    fn forgive(&mut self, at: u32, flow: &Flow) -> usize {
        let book = self.plan.book;
        let change = book.claim_changes[at as usize];
        let (place, unit) = (flow.to, flow.arrive.unit);
        let made = [Select::Txn(change.target)];
        let open =
            self.world.holdings.get(place, unit).map_or(Qty::ZERO, |slot| slot.admitted(false, &made, &book.codes));
        if open.is_zero() {
            return 0;
        }
        let request = Request {
            need: open,
            money: false,
            selectors: &made,
            policy: Some(Policy::Fifo),
            codes: &book.codes,
            permits: &[],
            spender: None,
            now: (change.day, RuntimeTxn::journal(change.target).expect("a written transaction")),
            explain: &|| false,
        };
        self.world.holdings.relieve(place, unit, &request, &mut self.scratch.relief);
        self.world.holdings.credit(flow.from, unit, open);
        let slices = &self.scratch.relief.slices;
        let rows = slices.iter().map(|s| WriteOff {
            change: at,
            place,
            unit,
            qty: s.qty,
            basis: s.basis,
            acquired: s.acquired,
        });
        self.record.written_off.extend(rows);
        slices.len()
    }
}
