//! Moving one flow's value: relief at the source, realization, arrival at the
//! target, with the laws that watch each step (PLAN §10's firing order):
//!
//! window totals, `on out` at `from`, relief, `on gain` per relieved parcel,
//! arrival, `on in` at `to`, `on spend` for every tie that left its owner's
//! places, then `always` at both ends.
//!
//! The value in flight is a list of [`Slice`]s: the parcels relieved (or, for a
//! source that holds none, one fresh slice), each remembering what it was at the
//! source and, once the flow's worth is known, the basis it will carry into the
//! target.

use axiom_core::{Diagnostic, Id, Qty};
use axiom_model::{Amount, Book, Class, Entity, Fault};

use crate::eval::Realized;
use crate::explain;
use crate::fire::Firing;
use crate::ledger::Ledger;
use crate::lots::{Origin, Request, Shares, Slice};
use crate::motion::Motion;
use crate::scope::stays_with_owner;
use crate::{Gain, Parcel, show};

/// What becomes of the parcels' identity when they arrive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Parcels {
    /// The same parcels, moved: basis, acquisition day, transaction and tie
    /// travel with them.
    Travel,
    /// New parcels, acquired today by this flow's transaction.
    Start,
}

/// Whether the parcels' basis starts over at what they fetched. It does unless
/// the value only changes place: a same-commodity transfer within one owner's
/// asset places (whose lots travel with their basis, acquisition day and ties),
/// or any move between two `deferred` places (which realize nothing, though an
/// exchange there still starts a new parcel). Value from a non-asset place has
/// no earlier basis to keep.
fn restarts_basis(book: &Book, m: &Motion) -> bool {
    let (from, to) = (&book.places[m.from], &book.places[m.to]);
    if from.class != Class::Asset {
        return true;
    }
    if from.deferred && to.deferred {
        return false;
    }
    m.is_exchange() || from.deferred || !stays_with_owner(book, m.from, m.to)
}

impl<'b, 's> Ledger<'b, 's> {
    /// Applies a flow: moves its value and fires every law that watches it.
    pub(crate) fn post(&mut self, m: &Motion) {
        let book = self.book;
        let flow = Firing::flow(m);
        self.count(m);
        self.fire(&book.rules.on_out[m.from], flow.moving(m.out).skipping_internal());
        self.relieve(m);
        let parcels = self.carry_basis(m);
        self.arrive(m, parcels);
        self.fire(&book.rules.on_in[m.to], flow.moving(m.arrive).skipping_internal());
        self.fire_spend(m);
        self.fire(&book.rules.always[m.from], flow);
        if m.to != m.from {
            self.fire(&book.rules.always[m.to], flow);
        }
    }

    /// Adds the flow to the totals some law reads, valuing only the sides
    /// that matter.
    fn count(&mut self, m: &Motion) {
        let book = self.book;
        let (leaves, enters) = self.world.totals.watched_sides(book, m.from, m.to);
        let out = if leaves { self.base_value(m, m.out) } else { None };
        let arrive = if enters { self.base_value(m, m.arrive) } else { None };
        self.world.totals.record(book, (m.from, m.to), m.day, out, arrive);
    }

    /// Takes `m.out` from the source, leaving the value in flight in
    /// `scratch.relief.slices`.
    fn relieve(&mut self, m: &Motion) {
        let book = self.book;
        let (unit, source, now) = (m.out.unit, &book.places[m.from], (m.day, m.txn));
        let is_base = unit == book.base;
        self.scratch.relief.slices.clear();
        if source.class != Class::Asset {
            self.world.holdings.credit(m.from, unit, -m.out.qty);
            self.scratch.relief.slices.push(Slice::fresh(m.out.qty, is_base, now));
            return;
        }
        self.ask_ties(m);
        let explain = !self.record.ambiguous.contains(&m.from);
        let request = Request {
            need: m.out.qty,
            money: is_base,
            selectors: m.select,
            policy: source.select,
            txns: &book.txns,
            permits: &self.scratch.permits,
            now,
            explain,
        };
        self.world.holdings.relieve(m.from, unit, &request, &mut self.scratch.relief);
        if self.scratch.relief.ambiguous && self.record.ambiguous.insert(m.from) {
            self.report_ambiguity(m);
        }
        let shortfall = self.scratch.relief.shortfall;
        if shortfall > Qty::ZERO && !is_base {
            self.report_shortfall(m, shortfall);
        }
        // What arrives must land even when nothing left (`all` of an empty
        // holding): value never vanishes from a balanced flow.
        if shortfall > Qty::ZERO || self.scratch.relief.slices.is_empty() {
            self.scratch.relief.slices.push(Slice::fresh(shortfall, is_base, now));
        }
    }

    fn report_ambiguity(&mut self, m: &Motion) {
        let proceeds = self.proceeds(m);
        let diagnostic = explain::ambiguous(self.book, m, &self.scratch.relief.candidates, proceeds);
        self.record.report(diagnostic);
    }

    /// A sale of more than the holding has: what it held before, and what the
    /// selectors let it reach, follow from what is left and what was missing.
    fn report_shortfall(&mut self, m: &Motion, short: Qty) {
        let held = self.world.holdings.qty(m.from, m.out.unit) + m.out.qty;
        let diagnostic = explain::shortfall(self.book, m, held, m.out.qty - short, short);
        self.record.report(diagnostic);
    }

    /// Learns, for each entity a parcel at the source is tied to, whether its
    /// `on spend` laws permit this flow. Only a flow that leaves the owner's
    /// places spends anything; on an internal transfer tied parcels go last.
    fn ask_ties(&mut self, m: &Motion) {
        let book = self.book;
        self.scratch.permits.clear();
        let Some(slot) = self.world.holdings.get(m.from, m.out.unit).filter(|slot| slot.is_tied()) else { return };
        for entity in slot.holding.lots.iter().filter_map(|lot| lot.tied) {
            if !self.scratch.permits.iter().any(|&(known, _)| known == entity) {
                self.scratch.permits.push((entity, false));
            }
        }
        if stays_with_owner(book, m.from, m.to) {
            return;
        }
        for at in 0..self.scratch.permits.len() {
            let entity = self.scratch.permits[at].0;
            self.scratch.permits[at].1 = self.permits_spend(entity, m);
        }
    }

    /// Decides what the flow was worth and what each slice carries into the
    /// target. Realized parcels emit a [`Gain`] and fire `on gain`.
    fn carry_basis(&mut self, m: &Motion) -> Parcels {
        let book = self.book;
        if !restarts_basis(book, m) {
            for slice in &mut self.scratch.relief.slices {
                slice.carried = slice.basis;
            }
            return if m.is_exchange() { Parcels::Start } else { Parcels::Travel };
        }
        let proceeds = self.proceeds(m);
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = proceeds.map(|total| Shares::new(total, whole));
        for slice in &mut self.scratch.relief.slices {
            slice.carried = match &mut shares {
                Some(shares) => shares.take(slice.qty),
                None if slice.origin == Origin::Fresh => Qty::ZERO,
                None => slice.basis,
            };
        }
        if proceeds.is_some() {
            self.realize(m);
        }
        Parcels::Start
    }

    /// Records a gain, and fires `on gain`, for every relieved lot. Plain money
    /// never realizes: its gain is zero by definition. So in a pro-rata place
    /// holding plain contributions and a zero-basis growth lot, a withdrawal
    /// relieves both and only the lot's share is a gain.
    fn realize(&mut self, m: &Motion) {
        let book = self.book;
        let ambiguous = self.scratch.relief.ambiguous;
        for at in 0..self.scratch.relief.slices.len() {
            let slice = self.scratch.relief.slices[at];
            if slice.origin != Origin::Lot {
                continue;
            }
            let gain = slice.carried - slice.basis;
            let row = Gain {
                cause: m.cause,
                day: m.day,
                from: m.from,
                to: m.to,
                unit: m.out.unit,
                qty: slice.qty,
                basis: slice.basis,
                proceeds: slice.carried,
                acquired: slice.acquired,
                ambiguous,
            };
            self.record.gains.push(row);
            let realized =
                Realized { gain, proceeds: slice.carried, basis: slice.basis, held: m.day.since(slice.acquired) };
            let amount = Amount::new(slice.qty, m.out.unit);
            self.fire(&book.rules.on_gain[m.from], Firing::flow(m).moving(amount).realizing(realized));
        }
    }

    /// Lands the slices at the target.
    fn arrive(&mut self, m: &Motion, parcels: Parcels) {
        let book = self.book;
        if book.places[m.to].class != Class::Asset {
            self.world.holdings.credit(m.to, m.arrive.unit, m.arrive.qty);
            return;
        }
        let stays = stays_with_owner(book, m.from, m.to);
        let restricted = self.restricted_source(m);
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(m.arrive.qty, whole);
        let holding = self.world.holdings.entry(m.to, m.arrive.unit);
        for slice in &self.scratch.relief.slices {
            let qty = shares.take(slice.qty);
            let parcel = match parcels {
                Parcels::Travel => {
                    Parcel { qty, basis: slice.carried, acquired: slice.acquired, txn: slice.txn, tied: slice.tied }
                }
                Parcels::Start => {
                    let tied = if stays && slice.origin != Origin::Fresh { slice.tied } else { restricted };
                    Parcel { qty, basis: slice.carried, acquired: m.day, txn: m.txn, tied }
                }
            };
            holding.land(parcel, m.arrive.unit == book.base);
        }
    }

    /// Fires the `on spend` laws of every entity whose tied money just left
    /// its owner's places.
    fn fire_spend(&mut self, m: &Motion) {
        let book = self.book;
        if stays_with_owner(book, m.from, m.to) {
            return;
        }
        for at in 0..self.scratch.relief.slices.len() {
            let Some(entity) = self.scratch.relief.slices[at].tied else { continue };
            let slices = &self.scratch.relief.slices;
            if slices[..at].iter().any(|s| s.tied == Some(entity)) {
                continue;
            }
            let spent: Qty = slices.iter().filter(|s| s.tied == Some(entity)).map(|s| s.qty).sum();
            self.fire(&book.rules.on_spend[entity], Firing::flow(m).moving(Amount::new(spent, m.out.unit)));
        }
    }

    /// The restricted entity that money crossing to another owner is tied to:
    /// the payee if it is restricted, else the source place's owner.
    fn restricted_source(&self, m: &Motion) -> Option<Id<Entity>> {
        let book = self.book;
        let restricted = |entity: &Id<Entity>| book.entities[*entity].restricted;
        m.payee.filter(restricted).or(Some(book.places[m.from].owner).filter(restricted))
    }

    /// What the flow's parcels fetched, in the base currency. The exchange
    /// itself says so when one side is the base; otherwise prices at the flow's
    /// day do. Pre-tax income into a `deferred` place is worth nothing: that
    /// money has not been taxed yet, so it has no basis.
    fn proceeds(&mut self, m: &Motion) -> Option<Qty> {
        let book = self.book;
        let pre_tax = matches!(book.places[m.from].class, Class::Income | Class::Equity) && book.places[m.to].deferred;
        match (pre_tax, m.arrive.unit == book.base, m.out.unit == book.base) {
            (true, ..) => Some(Qty::ZERO),
            (_, true, _) => Some(m.arrive.qty),
            (_, _, true) => Some(m.out.qty),
            _ => self.base_value(m, m.out).or_else(|| self.base_value(m, m.arrive)),
        }
    }

    /// `amount` in the base currency at the flow's day. A missing price is
    /// reported once per commodity and day.
    fn base_value(&mut self, m: &Motion, amount: Amount) -> Option<Qty> {
        let book = self.book;
        if amount.unit == book.base {
            return Some(amount.qty);
        }
        let value = book.convert(amount, book.base, m.day).map(|priced| priced.qty);
        if value.is_none() && self.record.unpriced.insert((amount.unit, m.day)) {
            let fault = Fault::NoPrice { unit: amount.unit, quote: book.base };
            let (what, help) = show::fault(book, fault, m.day);
            let mut d =
                Diagnostic::error("no-price", what).label(m.loc, format!("needed to value {}", book.show(amount)));
            if let Some(help) = help {
                d = d.help(help);
            }
            self.record.report(d);
        }
        value
    }
}
