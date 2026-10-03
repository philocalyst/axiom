//! A payment from a party settles the claims on it: LANGUAGE §7.
//!
//! "A later flow between them settles open claims: those its codes name, in order; else the one whose open amount is
//! exactly the flow's; else the oldest first. What remains is an ordinary flow." A claim is a parcel in a tab and
//! settling it is relief, so a flow out of a party's place relieves the tab of the owner it pays, as far as the tab holds
//! and by the order the tab's policy says. The flow itself is what it always was: the claims were already counted in the
//! tab, so the party's place is debited only by what did not settle one, and a return puts the claims back.
//!
//! What a party pays is what it pays *in all*, in one statement: a client who pays 3,100 as 3,009.80 into the bank and
//! 90.20 to the processor that took its fee has paid the invoice of 3,100, and the fee leg is as much of the payment as
//! the leg that reached the bank. The statement's flows out of that party's place that pay the owner, or pay someone
//! else beside a flow that does, are one payment. Each leg settles in its turn, and "exactly the flow's" is judged on
//! what the party still pays from that leg on, so the legs of one payment find the claim the first of them chose.

use axiom_core::{Day, Id, Qty};
use axiom_model::{Class, Entity, Flow, Place, Role};

use crate::Cause;
use crate::ledger::{Ledger, solved};
use crate::lots::{Request, Slice};
use crate::motion::{Motion, Moves};
use crate::state::Settled;

/// What a flow out of a party's place settles against, once it is known to be part of a payment.
struct Payment {
    /// The tab that holds what the party owes the owner.
    tab: Id<Place>,
    /// What the party pays from this flow on, in all: the exact amount of a claim that this payment is the size of.
    rest: Qty,
}

/// Where one flow out of a party's place goes, as far as a payment is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leg {
    /// Into money an owner holds.
    Owner(Id<Entity>),
    /// Into a third party's place: paid on the owner's behalf if the same statement pays the owner.
    Elsewhere,
}

impl Ledger<'_, '_, '_> {
    /// Where a flow goes: into an owner's money, or to a third party. A claim place or a debt is no payment.
    fn leg(&self, flow: &Flow) -> Option<Leg> {
        let place = &self.plan.book.places[flow.to];
        match place.class {
            Class::Asset if !self.plan.traits.place(flow.to).claim => Some(Leg::Owner(place.owner)),
            Class::Outside => Some(Leg::Elsewhere),
            Class::Asset | Class::Debt => None,
        }
    }

    /// The flows of the statement `this` is one of, from it on, that leave the same place in the same commodity and are
    /// real on `day`: what the party pays at this moment, whoever it pays.
    fn legs_from(&self, this: Id<Flow>, day: Day) -> impl Iterator<Item = (Id<Flow>, &Flow)> {
        let book = self.plan.book;
        let source = &book.flows[this];
        let legs = book.txns[source.txn].flows.ids().filter(move |&id| id >= this);
        let legs = legs.map(|id| (id, &book.flows[id]));
        legs.filter(move |(id, flow)| {
            let same = flow.from == source.from && flow.out.unit == source.out.unit && !flow.is_exchange();
            same && self.plan.events.state(*id, flow).is_real_on(day)
        })
    }

    /// The owner a flow pays: the one whose money it reaches, or, for a leg to a third party, the one the statement pays
    /// first, on whose behalf it is paid.
    fn owner_paid(&self, m: &Motion) -> Option<Id<Entity>> {
        if m.target.class == Class::Asset && !self.plan.traits.place(m.to).claim {
            return Some(m.target.owner);
        }
        let (book, Cause::Flow(this)) = (self.plan.book, m.cause) else { return None };
        let flows = book.txns[book.flows[this].txn].flows.ids().map(|id| &book.flows[id]);
        let paid = flows.filter(|flow| flow.from == m.from).find_map(|flow| match self.leg(flow)? {
            Leg::Owner(owner) => Some(owner),
            Leg::Elsewhere => None,
        });
        paid.filter(|_| m.target.class == Class::Outside)
    }

    /// The payment a flow is part of, if it is one: out of a party's place, in one commodity, not an opening, to an owner
    /// that the party owes something, or to a third party beside such a flow.
    fn payment_of(&self, m: &Motion) -> Option<Payment> {
        let from_party = matches!(m.source.role, Role::Outside(Some(_)));
        if !from_party || m.opening || m.is_exchange() || m.moves != Moves::Value || !self.plan.traits.owes(m.from) {
            return None;
        }
        let owner = self.owner_paid(m)?;
        let tab = self.plan.traits.tab_of(m.from, owner)?;
        let rest = match m.cause {
            Cause::Flow(this) => self.legs_from(this, m.day).map(|(id, flow)| self.pays(m, id, flow, owner)).sum(),
            _ => m.out.qty,
        };
        Some(Payment { tab, rest })
    }

    /// What the flow `id` pays toward the owner's claims: its own amount if it reaches the owner or goes to a third party,
    /// and nothing if it pays another owner or was not solved yet.
    fn pays(&self, this: &Motion, id: Id<Flow>, flow: &Flow, owner: Id<Entity>) -> Qty {
        let toward = self.leg(flow).is_some_and(|leg| leg == Leg::Owner(owner) || leg == Leg::Elsewhere);
        let out = match this.cause {
            Cause::Flow(first) if first == id => Some(this.out.qty),
            _ => solved(self.plan, &self.record, id, flow).map(|amounts| amounts.out),
        };
        out.filter(|_| toward).unwrap_or(Qty::ZERO)
    }

    /// Relieves the claims a flow out of a party's place settles, and says how much they came to; the flow's own value is
    /// the caller's. What it takes out of the tab is `scratch.relief`.
    pub(crate) fn settle_claims(&mut self, m: &Motion) -> Qty {
        let Some(Payment { tab, rest }) = self.payment_of(m) else { return Qty::ZERO };
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
        let request =
            Request { selectors, exact: open.min(rest), ..Request::of(need, policy, &book.codes, (m.day, m.txn)) };
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
