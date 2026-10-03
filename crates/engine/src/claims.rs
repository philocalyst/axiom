//! Making a claim of what a party owed and nothing kept, and forgiving a claim: `^code waived`.
//!
//! A due day that is missed (`monitor`) and that the party owed is a claim on it: the occurrence's header, paid into the
//! tab the owner keeps with the party, on the day it was found missing, with no purpose, so that nothing is recognized by
//! it that `books cash|accrual` has not said (nothing reads it yet). A later payment from the party settles it as any
//! payment does (`settle`: by code, then the exact amount, then the oldest). What the owner owes is not made a claim here:
//! a debt is a plain balance and a payment to the party does not settle it.
//!
//! A claim is a parcel in a claim place, so forgiving it is relief: every parcel the named transaction made is taken out
//! of the place that holds it, and the value goes back to the place it came from, as if the party had never been paid
//! into the claim. No money moves and no law watches it (a [`ClaimChange`] is "a source event that changes an
//! already-open claim without inventing a monetary transaction"); what the owner holds falls by what was forgiven,
//! and `claims`, `balance` and `overdue` see it because they read the parcels. What the claim recognized when it was
//! made is not reversed: that depends on whether the books are cash or accrual, which nothing reads yet.

use axiom_core::{Day, Qty};
use axiom_model::{Flow, Policy, RuntimeTxn, Select};

use crate::explain;
use crate::ledger::Ledger;
use crate::lots::Request;
use crate::motion::{Amounts, Motion};
use crate::{Cause, Promise, WriteOff};

impl Ledger<'_, '_, '_> {
    /// An occurrence nothing kept, found missing on `found`: if the party owed it, what it owed is claimed, and whether it was
    /// is said. An occurrence that cannot be made, or whose header is no amount, claims nothing.
    pub(crate) fn claim_missed(&mut self, missed: Promise, found: Day) -> bool {
        let Some(tab) = self.plan.claim_tab(missed.contract, missed.schedule) else { return false };
        let (mut flows, mut details, mut missing) = self.scratch.take_pools();
        let Promise { contract, schedule, ordinal, due, .. } = missed;
        let made =
            self.instantiate_occurrence(contract, schedule, due, ordinal, None, &mut flows, &mut details, &mut missing);
        let header = made.ok().and_then(|made| made.flows(&flows)?.first().cloned());
        let header = header.filter(|header| !header.flow.is_exchange() && header.flow.out.qty > Qty::ZERO);
        let claimed = header.is_some();
        if let Some(mut header) = header {
            header.flow.to = tab;
            header.flow.purpose = None;
            let book = self.plan.book;
            let (view, amounts) = (book.runtime_flow_view(&header, &details), Amounts::written(&header.flow));
            let day = found.max(self.clock.day);
            self.post(&Motion::from_view_at(book, view, header.txn, Cause::Time, day, amounts, header.ordinal));
        }
        self.scratch.give_pools(flows, details, missing);
        claimed
    }

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
