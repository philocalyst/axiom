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
//! What the owner owes is the mirror of it. A bill is a parcel of a debt place (a tab, or a declared place that says `claim`),
//! negative as a liability's balance is, and paying it is relief of it: by a flow into the place, or by one from the owner's
//! money to the party's place, in the same order. What the party's place is credited is what did not settle a bill.
//!
//! What a flow settled is told to [`recognition`](crate::recognition), which decides what it counts as.

use axiom_core::{Day, Id, Qty};
use axiom_model::{Class, Dir, Entity, Flow, Place, Role};

use crate::ledger::{Ledger, solved};
use crate::lots::{Origin, Request, Slice};
use crate::motion::{Course, Motion, Moves};
use crate::recognition::{Dealing, Reaches, Settlement, claim_dir};
use crate::{Cause, Parcel, PartId};

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

    /// What of the flow the claims paid, which the party's place is not debited as the source: they were counted in the tab. A
    /// claim that counts Out (a bill) is paid to the party, whose place is the target, and is credited less by it
    /// ([`Ledger::credit_party`]).
    pub fn paid(&self) -> Qty {
        match self.dir {
            Dir::In => self.settled(),
            Dir::Out => Qty::ZERO,
        }
    }

    fn settled(&self) -> Qty {
        self.settlement.parcels.iter().map(|parcel| parcel.qty).sum()
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

    /// The payment a flow is part of, if it is one, in one commodity and not an opening: out of a party's place to an owner
    /// that the party owes something, or to a third party beside such a flow; out of an owner's money into the place of a party
    /// the owner owes; or into a place that holds what the owner owes. A flow run backwards is a payment only into a debt (a
    /// bill that is returned), for the other two would take a returned payment for one the party made.
    fn payment_of(&self, m: &Motion) -> Option<Payment> {
        if m.opening || m.is_exchange() || m.moves != Moves::Value {
            return None;
        }
        let forward = (m.course == Course::Forward).then(|| self.paid_by_party(m).or_else(|| self.paid_to_party(m)));
        forward.flatten().or_else(|| self.paid_into_debt(m))
    }

    /// What a flow out of a party's place pays of what the party owes.
    fn paid_by_party(&self, m: &Motion) -> Option<Payment> {
        if !matches!(m.source.role, Role::Outside(Some(_))) || !self.plan.traits.has_tab(m.from) {
            return None;
        }
        let (owner, reaches) = self.owner_paid(m)?;
        let tab = self.plan.traits.tab_of(m.from, owner, Class::Asset)?;
        let rest = match m.cause {
            Cause::Flow(this) => self.legs_from(this, m.day).map(|(id, flow)| self.pays(m, id, flow, owner)).sum(),
            _ => m.out.qty,
        };
        Some(Payment { tab, rest, reaches })
    }

    /// What a flow out of an owner's money pays of the bills the owner has from the party whose place it goes to.
    fn paid_to_party(&self, m: &Motion) -> Option<Payment> {
        let traits = &self.plan.traits;
        if m.source.class != Class::Asset || !traits.has_tab(m.to) || traits.place(m.from).claim {
            return None;
        }
        let tab = traits.tab_of(m.to, m.source.owner, Class::Debt)?;
        Some(Payment { tab, rest: m.out.qty, reaches: Reaches::Owner })
    }

    /// What a flow into a place that holds what the owner owes pays of it: a place of the owner's, so it is no boundary crossing
    /// and counts nothing of its own.
    fn paid_into_debt(&self, m: &Motion) -> Option<Payment> {
        let debt = m.target.class == Class::Debt && self.plan.traits.place(m.to).claim;
        debt.then_some(Payment { tab: m.to, rest: m.out.qty, reaches: Reaches::Elsewhere })
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
        self.credit_party(m, &claiming);
        if let (Cause::Flow(flow), Course::Forward) = (m.cause, m.course) {
            self.record.settled.insert(flow, claiming.settlement.clone());
            self.record.settlements.push((flow, claiming.settlement.clone()));
        }
        Some(claiming)
    }

    /// The claims a flow settled were counted in their tab, so the party is not credited them again: a flow that pays a party
    /// (a claim that counts Out) credits its place `arrive` less what settled, and one that opens the claims again, run
    /// backwards, gives it back what it was not credited. (A claim that counts In is the other end: [`Claiming::paid`].)
    fn credit_party(&mut self, m: &Motion, claiming: &Claiming) {
        if claiming.dir == Dir::Out {
            self.world.holdings.credit(m.to, claiming.settlement.unit, -claiming.settled());
        }
    }

    /// Takes out of the tab what the payment settles of the claims the flow's codes and selectors reach.
    fn relieve_tab(&mut self, m: &Motion, payment: &Payment) -> Option<Claiming> {
        let Payment { tab, rest, reaches } = *payment;
        let (book, unit) = (self.plan.book, m.out.unit);
        let named = self.name_claims(m, tab);
        let selectors = if named { &self.scratch.selectors[..] } else { m.select() };
        let open = self.world.holdings.get(tab, unit).map_or(Qty::ZERO, |slot| slot.admitted(selectors, &book.codes));
        let need = open.min(m.out.qty);
        if need <= Qty::ZERO {
            return None;
        }
        let policy = self.plan.traits.place(tab).select;
        let request =
            Request { selectors, exact: open.min(rest), ..Request::of(need, policy, &book.codes, (m.day, m.txn)) };
        self.world.holdings.relieve(tab, unit, &request, &mut self.scratch.relief);
        let parcels = self.scratch.relief.slices.iter().map(Slice::parcel).collect();
        let dir = claim_dir(book.places[tab].class);
        Some(Claiming { settlement: Settlement { tab, unit, parcels, reaches }, dir })
    }

    /// A payment that is returned runs backwards: the claims it settled are open again, and the party gives back only what
    /// was not one. Says what it opened, which is what counts for it.
    fn returned_claims(&mut self, m: &Motion) -> Option<Claiming> {
        let Cause::Flow(flow) = m.cause else { return None };
        let settlement = self.record.settled.remove(&flow)?;
        let slot = self.world.holdings.entry(settlement.tab, settlement.unit);
        let codes = &self.plan.book.codes;
        settlement.parcels.iter().for_each(|&parcel| slot.restore(parcel, codes));
        let dir = claim_dir(self.plan.book.places[settlement.tab].class).reversed();
        let claiming = Claiming { settlement, dir };
        self.credit_party(m, &claiming);
        Some(claiming)
    }

    /// A bill: what leaves a place that says `claim` and holds what the owner owes is owed, as a parcel of the transaction that
    /// made it, the line that did and the codes it carries.
    pub(crate) fn owe(&mut self, m: &Motion) {
        let part = Some(PartId { origin: m.txn, ordinal: m.flow_ordinal });
        let bill = Parcel {
            qty: m.out.qty,
            basis: Qty::ZERO,
            acquired: m.day,
            held_since: m.day,
            wash_matched: false,
            txn: m.txn,
            part,
            codes: m.code_runs,
            tied: None,
        };
        self.world.holdings.entry(m.from, m.out.unit).owe(bill, &self.plan.book.codes);
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
