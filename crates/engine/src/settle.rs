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
//!
//! What a flow settled is told to [`recognition`](crate::recognition), which decides what it counts as.

use axiom_core::{Day, Id, Qty};
use axiom_model::{Class, Dir, Entity, Flow, Place, Role};

use crate::Cause;
use crate::ledger::{Ledger, solved};
use crate::lots::{Origin, Request, Slice};
use crate::motion::{Motion, Moves};
use crate::recognition::{Dealing, Reaches, Settlement};

/// What a flow did to claims, before it is counted: it settled them, or, run backwards, opened them again.
pub(crate) struct Claiming {
    settlement: Settlement,
    dir: Dir,
}

impl Claiming {
    /// What recognition makes of it, for a flow that moved `moved`.
    pub fn dealing(&self, moved: Qty) -> Dealing<'_> {
        Dealing::Settling { settlement: &self.settlement, dir: self.dir, moved }
    }

    /// What of the flow the claims paid, which the party's place is not debited: they were counted in the tab.
    pub fn paid(&self) -> Qty {
        match self.dir {
            Dir::In => self.settlement.parcels.iter().map(|parcel| parcel.qty).sum(),
            Dir::Out => Qty::ZERO,
        }
    }
}

/// Whether the source of a flow has been relieved by the time its claims are dealt with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relief {
    /// Not yet: the flow relieves it as it always did, after what it counts is counted.
    Pending,
    /// A claim place is relieved first, because the claims it gave up are what the flow settled.
    Done,
}

/// What a flow out of a party's place settles against, once it is known to be part of a payment.
#[derive(Clone, Copy)]
struct Payment {
    /// The tab that holds what the party owes the owner.
    tab: Id<Place>,
    /// What the party pays from this flow on, in all: the exact amount of a claim that this payment is the size of.
    rest: Qty,
    reaches: Reaches,
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
    fn owner_paid(&self, m: &Motion) -> Option<(Id<Entity>, Reaches)> {
        if m.target.class == Class::Asset && !self.plan.traits.place(m.to).claim {
            return Some((m.target.owner, Reaches::Owner));
        }
        let (book, Cause::Flow(this)) = (self.plan.book, m.cause) else { return None };
        let flows = book.txns[book.flows[this].txn].flows.ids().map(|id| &book.flows[id]);
        let paid = flows.filter(|flow| flow.from == m.from).find_map(|flow| match self.leg(flow)? {
            Leg::Owner(owner) => Some(owner),
            Leg::Elsewhere => None,
        });
        paid.filter(|_| m.target.class == Class::Outside).map(|owner| (owner, Reaches::Elsewhere))
    }

    /// The payment a flow is part of, if it is one: out of a party's place, in one commodity, not an opening, to an owner
    /// that the party owes something, or to a third party beside such a flow.
    fn payment_of(&self, m: &Motion) -> Option<Payment> {
        let from_party = matches!(m.source.role, Role::Outside(Some(_)));
        if !from_party || m.opening || m.is_exchange() || m.moves != Moves::Value || !self.plan.traits.owes(m.from) {
            return None;
        }
        let (owner, reaches) = self.owner_paid(m)?;
        let tab = self.plan.traits.tab_of(m.from, owner)?;
        let rest = match m.cause {
            Cause::Flow(this) => self.legs_from(this, m.day).map(|(id, flow)| self.pays(m, id, flow, owner)).sum(),
            _ => m.out.qty,
        };
        Some(Payment { tab, rest, reaches })
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

    /// Relieves the claims a payment from a party settles, or puts back the ones a payment that is returned had, and says
    /// what it did. Nothing else of the flow is moved here: the party's place is the caller's.
    pub(crate) fn settle_claims(&mut self, m: &Motion) -> Option<Claiming> {
        let Some(payment) = self.payment_of(m) else { return self.returned_claims(m) };
        let claiming = self.relieve_tab(m, &payment)?;
        if let Cause::Flow(flow) = m.cause {
            self.record.settled.insert(flow, claiming.settlement.clone());
            self.record.settlements.push((flow, claiming.settlement.clone()));
        }
        Some(claiming)
    }

    /// Takes out of the tab what the payment settles of the claims the flow's codes and selectors reach.
    fn relieve_tab(&mut self, m: &Motion, payment: &Payment) -> Option<Claiming> {
        let Payment { tab, rest, reaches } = *payment;
        let (book, unit) = (self.plan.book, m.out.unit);
        let named = self.name_claims(m, tab);
        let selectors = if named { &self.scratch.selectors[..] } else { m.select() };
        let open =
            self.world.holdings.get(tab, unit).map_or(Qty::ZERO, |slot| slot.admitted(false, selectors, &book.codes));
        let need = open.min(m.out.qty);
        if need <= Qty::ZERO {
            return None;
        }
        let policy = self.plan.traits.place(tab).select;
        let request =
            Request { selectors, exact: open.min(rest), ..Request::of(need, policy, &book.codes, (m.day, m.txn)) };
        self.world.holdings.relieve(tab, unit, &request, &mut self.scratch.relief);
        let parcels = self.scratch.relief.slices.iter().map(Slice::parcel).collect();
        Some(Claiming { settlement: Settlement { tab, unit, parcels, reaches }, dir: Dir::In })
    }

    /// A payment that is returned runs backwards: the claims it settled are open again, and the party gives back only what
    /// was not one. Says what it opened, which is what counts for it.
    fn returned_claims(&mut self, m: &Motion) -> Option<Claiming> {
        let Cause::Flow(flow) = m.cause else { return None };
        let settlement = self.record.settled.remove(&flow)?;
        let slot = self.world.holdings.entry(settlement.tab, settlement.unit);
        let codes = &self.plan.book.codes;
        settlement.parcels.iter().for_each(|&parcel| slot.land_with_codes(parcel, false, codes));
        let reopened: Qty = settlement.parcels.iter().map(|parcel| parcel.qty).sum();
        self.world.holdings.credit(m.to, settlement.unit, -reopened);
        Some(Claiming { settlement, dir: Dir::Out })
    }

    /// What a flow out of a claim place settled, once it has been relieved: the claims it took. They are the owner's own,
    /// so no one is owed a debit for them, and they are not what the flow moved.
    pub(crate) fn relieved_claims(&mut self, m: &Motion) -> Option<Claiming> {
        let lots = self.scratch.relief.slices.iter().filter(|slice| slice.origin == Origin::Lot);
        let parcels: Box<[_]> = lots.map(Slice::parcel).collect();
        if parcels.is_empty() {
            return None;
        }
        let settlement = Settlement { tab: m.from, unit: m.out.unit, parcels, reaches: Reaches::Elsewhere };
        if let Cause::Flow(flow) = m.cause {
            self.record.settlements.push((flow, settlement.clone()));
        }
        Some(Claiming { settlement, dir: Dir::In })
    }
}
