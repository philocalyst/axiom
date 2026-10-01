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
//! target. What arrives, with what basis, and tied to whom, is one decision
//! with a few inputs, so a revaluation, a basis flow and an opening are the same
//! path with different inputs:
//!
//! - a market moving an asset's worth is a flow whose growth arrives with no
//!   basis, and whose loss relieves parcels and hands their basis back to what
//!   remains;
//! - `PLACE.basis` at one end changes the basis of that place's parcels, and
//!   the other end sees plain value;
//! - an opening line is a flow nothing watches, whose parcels carry the basis
//!   and acquisition day it gives.

use axiom_core::{Diagnostic, Id, Qty};
use axiom_model::{Amount, Basis, Class, Dir, End, Entity, Fault, Object, Subject};

use crate::eval::{Occasion, Realized};
use crate::explain;
use crate::ledger::Ledger;
use crate::lots::{Origin, Request, Selection, Shares, Slice};
use crate::motion::{Motion, Moves};
use crate::scope::{is_money, stays_with_owner};
use crate::state::Missing;
use crate::{Cause, Gain, Parcel, show};

fn fresh_slice(
    m: &Motion,
    qty: Qty,
    is_base: bool,
    now: (axiom_core::Day, axiom_core::Id<axiom_model::Txn>),
) -> Slice {
    let mut slice = Slice::fresh(qty, is_base, now);
    slice.codes = m.code_runs;
    slice
}

/// Whether the parcels' basis starts over at what they fetched. It does unless
/// the value only changes place: a same-commodity transfer within one owner's
/// asset places (whose lots travel with their basis, acquisition day and ties),
/// or any move between two `deferred` places (which realize nothing, though an
/// exchange there still starts a new parcel). Value from a non-asset place has
/// no earlier basis to keep. A market moving an asset's worth, and a change of
/// basis, fetch nothing.
fn restarts_basis(m: &Motion) -> bool {
    let (from, to) = (m.source, m.target);
    // v3 bridge: a change of basis fetches nothing.
    if m.moves == Moves::Loss {
        return false;
    }
    if from.class != Class::Asset {
        return true;
    }
    if from.deferred && to.deferred {
        return false;
    }
    m.is_exchange() || from.deferred || !stays_with_owner(m)
}

impl Ledger<'_, '_, '_> {
    /// Applies a flow: moves its value and fires every law that watches it.
    pub(crate) fn post(&mut self, m: &Motion) {
        let book = self.plan.book;
        let on = Occasion::flow(m);
        let watched = !m.opening;
        self.scratch.worth.clear();
        // A `!` on an assertion accepts its gap: it is never unused.
        if let (Cause::Flow(_) | Cause::Applied(_), true, Some(waive)) = (m.cause, watched, m.waive) {
            self.record.waivers.entry(waive.loc).or_insert(false);
        }
        if watched {
            self.count(m);
            self.fire(&book.rules.on_out[m.from], &Occasion { amount: Some(m.out), skip_internal: true, ..on });
        }
        if m.source.class.holds_parcels() || m.target.class.holds_parcels() || m.moves != Moves::Value {
            self.relieve(m);
            let keeps = self.price(m);
            self.arrive(m, keeps);
        } else {
            // Places that hold only a plain balance have no parcels to move, and nothing was relieved.
            self.scratch.relief.slices.clear();
            self.world.holdings.credit(m.from, m.out.unit, -m.out.qty);
            self.world.holdings.credit(m.to, m.arrive.unit, m.arrive.qty);
        }
        if watched {
            self.fire(&book.rules.on_in[m.to], &Occasion { amount: Some(m.arrive), skip_internal: true, ..on });
            self.fire_purpose(m, &on);
            self.fire_spend(m);
            self.fire(&book.rules.always[m.from], &on);
            if m.to != m.from {
                self.fire(&book.rules.always[m.to], &on);
            }
        }
    }

    /// Purpose laws see the event after its value has moved and its window
    /// total has been counted. Their `self` is the flow's owner. When the
    /// purpose names an asset, laws about that asset also see the flow.
    fn fire_purpose(&mut self, m: &Motion, on: &Occasion) {
        let Some(purpose) = m.purpose else { return };
        let book = self.plan.book;
        let purpose_on = Occasion { amount: Some(m.out), ..*on };
        self.fire_as(
            &book.rules.purposes[purpose.purpose],
            &purpose_on,
            Some(Subject::Entity(m.owner)),
        );
        if let Some(Object::Asset(asset)) = purpose.of {
            let place = book.assets[asset].place;
            self.fire(&book.rules.about[place], &purpose_on);
        }
    }

    /// Adds the flow to the totals some law reads, valuing only the sides
    /// that matter.
    fn count(&mut self, m: &Motion) {
        let watch = &self.plan.watch;
        let (leaves, enters) = watch.sides(m.from, m.to);
        let out = if leaves { self.base_value(m, m.out) } else { None };
        let arrive = if enters { self.base_value(m, m.arrive) } else { None };
        self.world.totals.record(watch, (m.from, m.to), (m.day, m.recognized), out, arrive);

        if let Some(purpose) = m.purpose.map(|purpose| purpose.purpose).filter(|&purpose| watch.reads_purpose(purpose)) {
            if let Some((dir, amount)) = self.purpose_flow(m, purpose) {
                self.world.totals.record_purpose(watch, m.owner, purpose, (m.day, m.recognized), dir, amount);
            }
        }
    }

    /// The sign of a purpose follows value crossing the owner's boundary.
    /// The written flow direction is what matters here: paying an expense from
    /// a card is an outflow, and a refund from that expense into the card is an
    /// inflow. The debt balance's display sign must not reverse that meaning.
    fn purpose_flow(&mut self, m: &Motion, purpose: axiom_core::Id<axiom_model::Purpose>) -> Option<(Dir, Qty)> {
        let (source_owned, target_owned) = (
            m.source.owner == m.owner && m.source.class != Class::Outside,
            m.target.owner == m.owner && m.target.class != Class::Outside,
        );
        let root = self.plan.book.purposes[purpose].root;
        // A capital purchase between two asset places changes the form of the
        // owner's property but still belongs in the capital total.
        if source_owned && target_owned && root == axiom_model::PurposeRoot::Capital {
            return self.base_value(m, m.out).map(|amount| (Dir::Out, amount));
        }
        match (source_owned, target_owned) {
            (true, false) => self.base_value(m, m.out).map(|amount| (Dir::Out, amount)),
            (false, true) => self.base_value(m, m.arrive).map(|amount| (Dir::In, amount)),
            // A movement wholly inside one owner's books is not an income or
            // spending event. Capital acquisitions are the exception above.
            (true, true) | (false, false) => None,
        }
    }

    /// Takes `m.out` from the source, leaving the value in flight in
    /// `scratch.relief.slices`.
    fn relieve(&mut self, m: &Motion) {
        let book = self.plan.book;
        let (unit, source, now) = (m.out.unit, m.source, (m.day, m.txn));
        let is_base = unit == book.base;
        self.scratch.relief.slices.clear();
        if source.class != Class::Asset {
            self.world.holdings.credit(m.from, unit, -m.out.qty);
            self.scratch.relief.slices.push(fresh_slice(m, m.out.qty, is_base, now));
            return;
        }
        self.ask_ties(m);
        let request = Request {
            need: m.out.qty,
            money: is_money(book, m.from, unit),
            selectors: m.select(),
            // A flow's selector, then the place's policy, then what the commodity says (currencies are FIFO).
            policy: source.select.or(book.commodities[unit].select),
            codes: &book.codes,
            permits: &self.scratch.permits,
            spender: m.detail().spender,
            now,
            explain: &|| !self.record.ambiguous.contains(&m.from),
        };
        self.world.holdings.relieve(m.from, unit, &request, &mut self.scratch.relief);
        if self.scratch.relief.ambiguous && self.record.ambiguous.insert(m.from) {
            let proceeds = self.realizes(m);
            let diagnostic = explain::ambiguous(book, m, &self.scratch.relief.candidates, proceeds);
            self.record.report(diagnostic);
        }
        let shortfall = self.scratch.relief.shortfall;
        if shortfall > Qty::ZERO && !is_base {
            // What it held before, and what the selectors let it reach, follow
            // from what is left and what was missing.
            let held = self.world.holdings.qty(m.from, unit) + m.out.qty;
            let diagnostic = explain::shortfall(book, m, held, m.out.qty - shortfall, shortfall);
            self.record.report(diagnostic);
        }
        // What arrives must land even when nothing left (`all` of an empty
        // holding): value never vanishes from a balanced flow.
        if shortfall > Qty::ZERO {
            self.scratch.relief.slices.push(fresh_slice(m, shortfall, is_base, now));
        } else if self.scratch.relief.slices.is_empty() {
            self.scratch.relief.slices.push(fresh_slice(m, m.out.qty, is_base, now));
        }
    }

    /// Learns, for each entity a parcel at the source is tied to, whether its
    /// `on spend` laws permit this flow. Only a flow that leaves the owner's
    /// places spends anything; on an internal transfer tied parcels go last.
    /// A flow written out of an entity says whose money it is, so it needs no
    /// law to say so: that entity's parcels go first, and nobody else's are
    /// asked.
    fn ask_ties(&mut self, m: &Motion) {
        self.scratch.permits.clear();
        let Some(slot) = self.world.holdings.get(m.from, m.out.unit).filter(|slot| slot.is_tied()) else { return };
        for entity in slot.holding.lots.iter().filter_map(|lot| lot.tied) {
            if !self.scratch.permits.iter().any(|&(known, _)| known == entity) {
                self.scratch.permits.push((entity, false));
            }
        }
        if stays_with_owner(m) || m.detail().spender.is_some() {
            return;
        }
        for at in 0..self.scratch.permits.len() {
            let entity = self.scratch.permits[at].0;
            self.scratch.permits[at].1 = self.permits_spend(entity, m);
        }
    }

    /// Decides what the flow was worth and what each slice carries into the
    /// target, and realizes what leaves. Returns whether the parcels keep
    /// their identity on arrival.
    fn price(&mut self, m: &Motion) -> bool {
        let restarts = restarts_basis(m);
        // Value from outside takes the target's arrival rule; a market's growth has no basis.
        let unbased = m.target.basis == Basis::Zero || m.moves == Moves::Growth;
        // What was fetched matters to what a sale realizes, and to a basis nobody stated.
        let priced = restarts && (m.source.class == Class::Asset || (m.detail().basis.is_none() && !unbased));
        let proceeds = if priced { self.proceeds(m) } else { None };
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let (mut worth, mut fixed) =
            (proceeds.map(|p| Shares::new(p, whole)), m.detail().basis.map(|b| Shares::new(b, whole)));
        for slice in &mut self.scratch.relief.slices {
            slice.worth = worth.as_mut().map_or(Qty::ZERO, |shares| shares.take(slice.qty));
            let stated = fixed.as_mut().map(|shares| shares.take(slice.qty));
            slice.carried = match (stated, restarts, proceeds.is_some(), slice.origin) {
                (Some(basis), ..) => basis,
                (None, false, ..) => slice.basis,
                (None, true, true, Origin::Fresh) if unbased => Qty::ZERO,
                (None, true, true, _) => slice.worth,
                (None, true, false, Origin::Fresh) => Qty::ZERO,
                (None, true, false, _) => slice.basis,
            };
        }
        if restarts && proceeds.is_some() {
            self.charge_costs(m);
            if !m.opening {
                self.realize(m);
            }
        }
        !restarts && !m.is_exchange()
    }

    /// What the exchange's fee legs cost it (LANGUAGE §2), shared over its
    /// slices by quantity: a sale fetched that much less, since a selling cost
    /// comes off the proceeds, and what a purchase bought cost that much more,
    /// unless the flow states the basis itself. What arrives from a sale keeps
    /// the price, because the fee leaves the place it landed in as an expense.
    fn charge_costs(&mut self, m: &Motion) {
        let cost = self.exchange_cost(m);
        if cost.is_zero() {
            return;
        }
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(cost, whole);
        let sold = m.out.unit != self.plan.book.base;
        for slice in &mut self.scratch.relief.slices {
            let part = shares.take(slice.qty);
            match sold {
                true => slice.worth -= part,
                false if m.detail().basis.is_none() => slice.carried += part,
                false => {}
            }
        }
    }

    /// What the exchange's fee legs cost it, in the base currency.
    fn exchange_cost(&mut self, m: &Motion) -> Qty {
        m.detail().cost.and_then(|cost| self.base_value(m, cost)).unwrap_or(Qty::ZERO)
    }

    /// What the parcels that leave realize against their basis: what they
    /// fetched, less the selling cost that comes off it.
    fn realizes(&mut self, m: &Motion) -> Option<Qty> {
        let fetched = self.proceeds(m)?;
        Some(if m.out.unit == self.plan.book.base { fetched } else { fetched - self.exchange_cost(m) })
    }

    /// Records a gain, and fires `on gain`, for every relieved lot. Plain money
    /// never realizes: its gain is zero by definition. So in a pro-rata place
    /// holding plain contributions and a zero-basis growth lot, a withdrawal
    /// relieves both and only the lot's share is a gain.
    fn realize(&mut self, m: &Motion) {
        let book = self.plan.book;
        let ambiguous = self.scratch.relief.ambiguous;
        for at in 0..self.scratch.relief.slices.len() {
            let slice = self.scratch.relief.slices[at];
            if slice.origin != Origin::Lot {
                continue;
            }
            let row = Gain {
                cause: m.cause,
                day: m.day,
                from: m.from,
                to: m.to,
                unit: m.out.unit,
                qty: slice.qty,
                basis: slice.basis,
                proceeds: slice.worth,
                acquired: slice.acquired,
                ambiguous,
            };
            self.record.gains.push(row);
            let held = m.day.since(slice.acquired);
            let realized =
                Realized { gain: slice.worth - slice.basis, proceeds: slice.worth, basis: slice.basis, held };
            let on = Occasion {
                amount: Some(Amount::new(slice.qty, m.out.unit)),
                realized: Some(realized),
                ..Occasion::flow(m)
            };
            self.fire(&book.rules.on_gain[m.from], &on);
        }
    }

    /// Lands the slices at the target.
    fn arrive(&mut self, m: &Motion, keeps: bool) {
        let book = self.plan.book;
        if m.target.class != Class::Asset {
            self.world.holdings.credit(m.to, m.arrive.unit, m.arrive.qty);
            if m.moves == Moves::Loss {
                self.keep_basis(m);
            }
            return;
        }
        let (stays, restricted) = (stays_with_owner(m), self.restricted_source(m));
        // `for` an entity ties what arrives to it; `for` the owner (or its household) unties it.
        let owner = m.target.owner;
        let hold =
            m.detail().hold.map(|entity| Some(entity).filter(|&e| e != owner && book.entities[owner].member != Some(e)));
        let (money, since) = (is_money(book, m.to, m.arrive.unit), m.detail().since.unwrap_or(m.day));
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(m.arrive.qty, whole);
        let slot = self.world.holdings.entry(m.to, m.arrive.unit);
        for slice in &self.scratch.relief.slices {
            let qty = shares.take(slice.qty);
            let kept = if keeps || (stays && slice.origin != Origin::Fresh) { slice.tied } else { restricted };
            // Parcels that keep their identity keep their day and purchase; the rest start over.
            let (acquired, txn) = if keeps { (slice.acquired, slice.txn) } else { (since, m.txn) };
            let codes = if keeps { slice.codes } else { m.code_runs };
            slot.land_with_codes(
                Parcel { qty, basis: slice.carried, acquired, txn, codes, tied: hold.unwrap_or(kept) },
                money,
                &book.codes,
            );
        }
    }

    /// A loss in what an asset is worth shrank its parcels and left their
    /// basis unspent: it goes to the parcels that remain, in proportion to
    /// their quantity.
    fn keep_basis(&mut self, m: &Motion) {
        let book = self.plan.book;
        let left: Qty = self.scratch.relief.slices.iter().filter(|s| s.origin != Origin::Fresh).map(|s| s.basis).sum();
        let selection = Selection { selectors: &[], codes: &book.codes };
        let slot = self.world.holdings.entry(m.from, m.out.unit);
        slot.rebase(left, &selection, is_money(book, m.from, m.out.unit), (m.day, m.txn));
    }

    /// Fires the `on spend` laws of every entity whose tied money just left
    /// its owner's places.
    fn fire_spend(&mut self, m: &Motion) {
        let book = self.plan.book;
        if stays_with_owner(m) {
            return;
        }
        for at in 0..self.scratch.relief.slices.len() {
            let Some(entity) = self.scratch.relief.slices[at].tied else { continue };
            let slices = &self.scratch.relief.slices;
            if slices[..at].iter().any(|s| s.tied == Some(entity)) {
                continue;
            }
            let spent: Qty = slices.iter().filter(|s| s.tied == Some(entity)).map(|s| s.qty).sum();
            let on = Occasion { amount: Some(Amount::new(spent, m.out.unit)), ..Occasion::flow(m) };
            self.fire(&book.rules.on_spend[entity], &on);
        }
    }

    /// The restricted entity that money crossing to another owner is tied to:
    /// the payee if it is restricted, else the source place's owner.
    fn restricted_source(&self, m: &Motion) -> Option<Id<Entity>> {
        let book = self.plan.book;
        let restricted = |entity: &Id<Entity>| book.entities[*entity].restricted;
        m.payee.filter(restricted).or(Some(m.source.owner).filter(restricted))
    }

    /// What the flow's parcels fetched, in the base currency. The exchange
    /// itself says so when one side is the base; otherwise prices at the flow's
    /// day do.
    fn proceeds(&mut self, m: &Motion) -> Option<Qty> {
        let base = self.plan.book.base;
        match (m.arrive.unit == base, m.out.unit == base) {
            (true, _) => Some(m.arrive.qty),
            (_, true) => Some(m.out.qty),
            _ => self.base_value(m, m.out).or_else(|| self.base_value(m, m.arrive)),
        }
    }

    /// `amount` in the base currency at the flow's day. A missing price is
    /// reported once per commodity: the first day it is missing, which is
    /// before the first price, since a price stands until the next.
    pub(crate) fn base_value(&mut self, m: &Motion, amount: Amount) -> Option<Qty> {
        let book = self.plan.book;
        if amount.unit == book.base {
            return Some(amount.qty);
        }
        if let Some(&(_, worth)) = self.scratch.worth.iter().find(|&&(seen, _)| seen == amount) {
            return worth;
        }
        let value = book.convert(amount, book.base, m.day).map(|priced| priced.qty);
        self.scratch.worth.push((amount, value));
        if value.is_none() && self.record.missing.insert(Missing::Price(amount.unit, book.base)) {
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
