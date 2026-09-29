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

use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_model::{Amount, Book, Class, Entity, Fault, Txn};

use crate::eval::Realized;
use crate::explain;
use crate::fire::Firing;
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::relief::{self, Piece, Request, Shares, Source};
use crate::scope::stays_with_owner;
use crate::{Gain, Holding, Parcel, show};

/// Part of the value in flight.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slice {
    pub qty: Qty,
    /// Basis relieved with it at the source.
    pub basis: Qty,
    pub acquired: Day,
    pub txn: Id<Txn>,
    pub tied: Option<Id<Entity>>,
    pub origin: Origin,
    /// The basis it carries into the target, set once the flow's worth is known.
    pub carried: Qty,
}

/// Where a slice came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Origin {
    /// Plain money: base currency at its face.
    Plain,
    /// A parcel with an identity of its own.
    Lot,
    /// Nothing gave it: value from an income, equity or liability place, or
    /// what a sale asked for beyond what was held.
    Fresh,
}

impl Slice {
    fn plain(m: &Motion, piece: &Piece) -> Slice {
        Slice {
            qty: piece.qty,
            basis: piece.basis,
            acquired: m.day,
            txn: m.txn,
            tied: None,
            origin: Origin::Plain,
            carried: Qty::ZERO,
        }
    }

    fn lot(lot: &Parcel, piece: &Piece) -> Slice {
        let (acquired, txn, tied) = (lot.acquired, lot.txn, lot.tied);
        Slice { qty: piece.qty, basis: piece.basis, acquired, txn, tied, origin: Origin::Lot, carried: Qty::ZERO }
    }

    /// Base currency conjured from nowhere is at its face; anything else has no
    /// basis of its own.
    fn fresh(m: &Motion, qty: Qty, base: Id<axiom_model::Commodity>) -> Slice {
        let basis = if m.out.unit == base { qty } else { Qty::ZERO };
        Slice { qty, basis, acquired: m.day, txn: m.txn, tied: None, origin: Origin::Fresh, carried: Qty::ZERO }
    }
}

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
    /// `scratch.slices`.
    fn relieve(&mut self, m: &Motion) {
        let book = self.book;
        self.scratch.slices.clear();
        let source = &book.places[m.from];
        if source.class != Class::Asset {
            self.world.holdings.credit(m.from, m.out.unit, -m.out.qty);
            self.scratch.slices.push(Slice::fresh(m, m.out.qty, book.base));
            return;
        }
        self.ask_ties(m);
        let is_base = m.out.unit == book.base;
        let request = Request {
            need: m.out.qty,
            is_base,
            selectors: m.select,
            policy: source.select,
            txns: &book.txns,
            permits: &self.scratch.permits,
        };
        relief::plan(self.world.holdings.get(m.from, m.out.unit), &request, &mut self.scratch.relief);
        if !self.scratch.relief.ambiguous.is_empty() {
            self.report_ambiguity(m);
        }
        if self.scratch.relief.shortfall > Qty::ZERO && !is_base {
            self.report_shortfall(m);
        }
        self.take(m);
    }

    fn report_ambiguity(&mut self, m: &Motion) {
        let proceeds = self.proceeds(m);
        let Some(holding) = self.world.holdings.get(m.from, m.out.unit) else { return };
        let diagnostic = explain::ambiguous(self.book, m, holding, &self.scratch.relief.ambiguous, proceeds);
        self.record.report(diagnostic);
    }

    fn report_shortfall(&mut self, m: &Motion) {
        let book = self.book;
        let holding = self.world.holdings.get(m.from, m.out.unit);
        let held = holding.map_or(Qty::ZERO, Holding::qty);
        let is_base = m.out.unit == book.base;
        let admitted = holding.map_or(Qty::ZERO, |h| relief::admitted(h, is_base, m.select, &book.txns));
        let diagnostic = explain::shortfall(book, m, held, admitted, self.scratch.relief.shortfall);
        self.record.report(diagnostic);
    }

    /// Removes the planned pieces from the source holding.
    fn take(&mut self, m: &Motion) {
        let holding = self.world.holdings.entry(m.from, m.out.unit);
        let (relief, slices) = (&self.scratch.relief, &mut self.scratch.slices);
        for piece in &relief.pieces {
            slices.push(match piece.source {
                Source::Plain => {
                    holding.plain -= piece.qty;
                    Slice::plain(m, piece)
                }
                Source::Lot(at) => {
                    let lot = &mut holding.lots[at];
                    let slice = Slice::lot(lot, piece);
                    lot.qty -= piece.qty;
                    lot.basis -= piece.basis;
                    slice
                }
            });
        }
        holding.plain -= relief.shortfall;
        // What arrives must land even when nothing left (`all` of an empty
        // holding): value never vanishes from a balanced flow.
        if relief.shortfall > Qty::ZERO || slices.is_empty() {
            slices.push(Slice::fresh(m, relief.shortfall, self.book.base));
        }
        holding.lots.retain(|lot| !lot.qty.is_zero());
    }

    /// Learns, for each entity a parcel at the source is tied to, whether its
    /// `on spend` laws permit this flow. Only a flow that leaves the owner's
    /// places spends anything; on an internal transfer tied parcels go last.
    fn ask_ties(&mut self, m: &Motion) {
        let book = self.book;
        self.scratch.permits.clear();
        let Some(holding) = self.world.holdings.get(m.from, m.out.unit) else { return };
        for entity in holding.lots.iter().filter_map(|lot| lot.tied) {
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
            for slice in &mut self.scratch.slices {
                slice.carried = slice.basis;
            }
            return if m.is_exchange() { Parcels::Start } else { Parcels::Travel };
        }
        let proceeds = self.proceeds(m);
        let whole: Qty = self.scratch.slices.iter().map(|s| s.qty).sum();
        let mut shares = proceeds.map(|total| Shares::new(total, whole));
        for slice in &mut self.scratch.slices {
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
        let ambiguous = !self.scratch.relief.ambiguous.is_empty();
        for at in 0..self.scratch.slices.len() {
            let slice = self.scratch.slices[at];
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
        let whole: Qty = self.scratch.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(m.arrive.qty, whole);
        let holding = self.world.holdings.entry(m.to, m.arrive.unit);
        for slice in &self.scratch.slices {
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
        for at in 0..self.scratch.slices.len() {
            let Some(entity) = self.scratch.slices[at].tied else { continue };
            let slices = &self.scratch.slices;
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
