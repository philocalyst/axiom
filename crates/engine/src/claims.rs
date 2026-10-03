//! Forgiving a claim: `^code waived`.
//!
//! A claim is a parcel in a claim place, so forgiving it is relief: every parcel the named transaction made is taken out
//! of the place that holds it, and the value goes back to the place it came from, as if the party had never been paid
//! into the claim. No money moves and no law watches it (a [`ClaimChange`] is "a source event that changes an
//! already-open claim without inventing a monetary transaction"); what the owner holds falls by what was forgiven,
//! and `claims`, `balance` and `overdue` see it because they read the parcels. What the claim recognized when it was
//! made is not reversed: that depends on whether the books are cash or accrual, which nothing reads yet.

use axiom_core::{Id, Qty};
use axiom_model::{ClaimChange, Commodity, Place, Policy, RuntimeTxn, Select};

use crate::WriteOff;
use crate::explain;
use crate::ledger::Ledger;
use crate::lots::Request;

impl Ledger<'_, '_, '_> {
    /// Forgives what is still open of the claim `Book::claim_changes[at]` names, wherever the transaction made it.
    pub(crate) fn write_off(&mut self, at: u32) {
        let book = self.plan.book;
        let change = book.claim_changes[at as usize];
        let made = book.flows[book.txns[change.target].flows].iter().filter(|flow| book.makes_claim(flow));
        let mut claims: Vec<(Id<Place>, Id<Commodity>, Id<Place>)> = Vec::new();
        for flow in made {
            let claim = (flow.to, flow.arrive.unit, flow.from);
            if !claims.contains(&claim) {
                claims.push(claim);
            }
        }
        let forgiven =
            claims.iter().map(|&(place, unit, back)| self.forgive(at, &change, place, unit, back)).sum::<usize>();
        if forgiven == 0 {
            self.record.report(explain::empty_write_off(book, &change));
        }
    }

    /// Takes the parcels the change's transaction made out of `place`, gives their value to `back`, and says how many.
    fn forgive(
        &mut self,
        at: u32,
        change: &ClaimChange,
        place: Id<Place>,
        unit: Id<Commodity>,
        back: Id<Place>,
    ) -> usize {
        let book = self.plan.book;
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
        self.world.holdings.credit(back, unit, open);
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
