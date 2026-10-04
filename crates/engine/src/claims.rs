//! Making a claim of what a party owed and nothing kept, and forgiving a claim: `^code waived`.
//!
//! A due day that is missed (`monitor`) and that the party owed is a claim on it: the occurrence's header, paid into the
//! tab the owner keeps with the party, on the day it was found missing, with the purpose the contract gives it. Its purpose
//! is its recognition (`recognition`): accrual books count it when it is made, cash books when what pays it settles it. A
//! later payment from the party settles it as any payment does (`settle`: by code, then the exact amount, then the oldest).
//!
//! A claim is a parcel in a claim place, so forgiving it is relief: every parcel the named transaction made is taken out
//! of the place that holds it, and the value goes back to the place it came from, as if the party had never been paid
//! into the claim. The same is true of a bill, which the party forgives: the parcel is the owner's debt, and the value goes
//! back from the party's place, which was credited it. No money moves and no law watches it (a [`ClaimChange`] is "a source event that changes an
//! already-open claim without inventing a monetary transaction"); what the owner holds falls by what was forgiven,
//! and `claims`, `balance` and `overdue` see it because they read the parcels. In accrual books the claim recognized its
//! purpose when it was made, and forgiving it takes that back (`take_back`); in cash books it recognized nothing yet.

use axiom_core::{Day, Id, Qty};
use axiom_model::{Amount, ClaimChange, Flow, Place, Policy, RuntimeTxn, Select};

use crate::explain;
use crate::histories::Position;
use crate::ledger::Ledger;
use crate::lots::Request;
use crate::motion::{Amounts, Motion};
use crate::recognition::{Counting, Counts, Piece, Share};
use crate::{Cause, Parcel, Promise, WriteOff};

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
        let made = book.txns[target].flows.ids().filter_map(|id| Some((id, book.claim_made_in(&book.flows[id])?)));
        let forgiven: usize = made.map(|(id, place)| self.forgive(at, id, place)).sum();
        if forgiven == 0 {
            self.record.report(explain::empty_write_off(book, &book.claim_changes[at as usize]));
        }
    }

    /// Takes the parcels the write-off `at` makes of one line of the claim out of the place that line paid into (or, for a bill,
    /// out of), gives their value back to the party's place, and says how many parcels that was. Each parcel is forgiven under
    /// the line that made it, which is what says what it was for.
    fn forgive(&mut self, at: u32, claim: Id<Flow>, place: Id<Place>) -> usize {
        let book = self.plan.book;
        let (change, flow) = (book.claim_changes[at as usize], &book.flows[claim]);
        let unit = if place == flow.to { flow.arrive.unit } else { flow.out.unit };
        let made = [Select::Txn(change.target)];
        let open = self.world.holdings.get(place, unit).map_or(Qty::ZERO, |slot| slot.admitted(&made, &book.codes));
        if open.is_zero() {
            return 0;
        }
        let now = (change.day, RuntimeTxn::journal(change.target).expect("a written transaction"));
        let request = Request { selectors: &made, ..Request::of(open, Some(Policy::Fifo), &book.codes, now) };
        self.world.holdings.relieve(place, unit, &request, &mut self.scratch.relief);
        for (line, qty) in self.record_write_off(at, claim, Position { place, unit }) {
            let line = &book.flows[line];
            let party = if place == line.to { line.from } else { line.to };
            self.world.holdings.credit(party, unit, self.plan.sides.display(place, qty));
            self.take_back(change, line, place, qty);
        }
        self.scratch.relief.slices.len()
    }

    /// Records the parcels just relieved as forgiven by the write-off `at`, each under the line of the claim that made it
    /// (`claim`, for one that came from nowhere written), and says how much of each line that was.
    fn record_write_off(
        &mut self,
        at: u32,
        claim: Id<Flow>,
        Position { place, unit }: Position,
    ) -> Vec<(Id<Flow>, Qty)> {
        let book = self.plan.book;
        let mut lines: Vec<(Id<Flow>, Qty)> = Vec::new();
        for slice in &self.scratch.relief.slices {
            let line = slice.part.and_then(|part| book.txn_flow(part.origin, part.ordinal)).unwrap_or(claim);
            let Parcel { qty, basis, acquired, .. } = slice.lot;
            self.record.written_off.push(WriteOff { change: at, claim: line, place, unit, qty, basis, acquired });
            match lines.iter_mut().find(|(seen, _)| *seen == line) {
                Some((_, total)) => *total += qty,
                None => lines.push((line, qty)),
            }
        }
        lines
    }

    /// What the claim recognized when it was made, in accrual books, is taken back by forgiving it: its purpose, the amount
    /// forgiven, the other way round, on the day of the write-off. In cash books it recognized nothing yet. A law cannot
    /// subtract what it counted (`count amount as receipts` adds), so no law fires on it and only the totals follow.
    fn take_back(&mut self, change: ClaimChange, claim: &Flow, tab: Id<Place>, forgiven: Qty) {
        let day = change.day;
        Counting::forgiving(self.plan, claim, tab, day, forgiven).pieces(self.plan.book, &mut self.scratch.pieces);
        self.scratch.worth.clear();
        for at in 0..self.scratch.pieces.len() {
            let Piece { purpose, share, counts, recognized } = self.scratch.pieces[at];
            let (Some(purposed), Share::Part(qty), Counts::Claim { dir, .. }) = (purpose, share, counts) else {
                continue;
            };
            let watch = &self.plan.watch;
            if !watch.reads_purpose(purposed.purpose) {
                continue;
            }
            if let Some(value) = self.base_value_on((day, change.loc), Amount::new(qty, claim.arrive.unit)) {
                self.world.totals.record_purpose(watch, claim.owner, purposed.purpose, (day, recognized), dir, value);
            }
        }
    }
}
