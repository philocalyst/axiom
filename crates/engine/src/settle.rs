//! A payment from a party settles the claims on it: LANGUAGE §7.
//!
//! "A later flow between them settles open claims: those its codes name, in order; else the one whose open amount is
//! exactly the flow's; else the oldest first. What remains is an ordinary flow." A claim is a parcel in a tab and
//! settling it is relief, so a flow out of a party's place relieves the tab of the owner it pays, as far as the tab holds
//! and by the order the tab's policy says. The flow itself is what it always was: the claims were already counted in the
//! tab, so the party's place is debited only by what did not settle one, and a return puts the claims back.

use axiom_core::{Id, Qty};
use axiom_model::{Class, Place, Role};

use crate::Cause;
use crate::ledger::Ledger;
use crate::lots::{Request, Slice};
use crate::motion::{Motion, Moves};
use crate::state::Settled;

impl Ledger<'_, '_, '_> {
    /// The tab a flow settles: it comes out of a party's place and pays an owner's place in one commodity, and that owner
    /// has a tab with the party. An opening is a state, and a flow into a claim place makes a claim: neither is a payment.
    fn tab_to_settle(&self, m: &Motion) -> Option<Id<Place>> {
        let from_party = matches!(m.source.role, Role::Outside(Some(_)));
        let money = m.target.class == Class::Asset && !self.plan.traits.place(m.to).claim;
        let pays = money && !m.opening && !m.is_exchange() && m.moves == Moves::Value;
        (from_party && pays).then(|| self.plan.traits.tab_of(m.from, m.target.owner)).flatten()
    }

    /// Relieves the claims a flow out of a party's place settles, and says how much they came to; the flow's own value is
    /// the caller's. What it takes out of the tab is `scratch.relief`.
    pub(crate) fn settle_claims(&mut self, m: &Motion) -> Qty {
        let Some(tab) = self.tab_to_settle(m) else { return Qty::ZERO };
        let (book, unit) = (self.plan.book, m.out.unit);
        let named = self.name_claims(m, tab);
        let selectors = if named { &self.scratch.selectors[..] } else { m.select() };
        let open =
            self.world.holdings.get(tab, unit).map_or(Qty::ZERO, |slot| slot.admitted(false, selectors, &book.codes));
        let need = open.min(m.out.qty);
        if need <= Qty::ZERO {
            return Qty::ZERO;
        }
        let policy = self.plan.traits.place(tab).select;
        let request = Request {
            need,
            money: false,
            selectors,
            policy,
            codes: &book.codes,
            permits: &[],
            spender: None,
            now: (m.day, m.txn),
            explain: &|| false,
        };
        self.world.holdings.relieve(tab, unit, &request, &mut self.scratch.relief);
        if let Cause::Flow(flow) = m.cause {
            let parcels = self.scratch.relief.slices.iter().map(Slice::parcel).collect();
            self.record.settled.insert(flow, Settled { tab, unit, parcels });
        }
        need
    }

    /// A payment that is returned runs backwards: the claims it settled are open again, and the party gives back only
    /// what was not one.
    pub(crate) fn reopen_claims(&mut self, m: &Motion) {
        let Cause::Flow(flow) = m.cause else { return };
        let Some(Settled { tab, unit, parcels }) = self.record.settled.remove(&flow) else { return };
        let codes = &self.plan.book.codes;
        let slot = self.world.holdings.entry(tab, unit);
        parcels.iter().for_each(|&parcel| slot.land_with_codes(parcel, false, codes));
        let reopened: Qty = parcels.iter().map(|parcel| parcel.qty).sum();
        self.world.holdings.credit(m.to, unit, -reopened);
    }
}
