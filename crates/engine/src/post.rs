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
use axiom_model::{
    Amount, Asset, Basis, Class, Dir, Entity, Fault, Object, PurposeRoot, RuntimeTxn, Subject,
};

use crate::eval::{Occasion, Realized};
use crate::explain;
use crate::ledger::Ledger;
use crate::lots::{Origin, Request, Selection, Shares, Slice};
use crate::motion::{Motion, Moves};
use crate::scope::{is_money, stays_with_owner};
use crate::state::Missing;
use crate::{Cause, DisposalBoundary, EventKey, Gain, Parcel, Part, PartId, PartKind, show};

fn fresh_slice(m: &Motion, qty: Qty, is_base: bool, now: (axiom_core::Day, RuntimeTxn)) -> Slice {
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
        if let (Cause::Flow(_) | Cause::Applied(_), true, Some(waive)) = (m.cause, watched, m.waive)
        {
            self.record.waivers.entry(waive.loc).or_insert(false);
        }
        if watched {
            self.count(m);
            self.sample_temporal(m.day);
            self.fire(
                &book.rules.on_out[m.from],
                &Occasion {
                    amount: Some(m.out),
                    skip_internal: true,
                    ..on
                },
            );
        }
        if m.source.class.holds_parcels()
            || m.target.class.holds_parcels()
            || m.moves != Moves::Value
        {
            self.relieve(m);
            let keeps = self.price(m);
            self.arrive(m, keeps);
        } else {
            // Places that hold only a plain balance have no parcels to move, and nothing was relieved.
            self.scratch.relief.slices.clear();
            self.world.holdings.credit(m.from, m.out.unit, -m.out.qty);
            self.world
                .holdings
                .credit(m.to, m.arrive.unit, m.arrive.qty);
            self.sample_temporal(m.day);
        }
        self.record_capital_outflow(m);
        self.sample_temporal(m.day);
        if watched {
            self.fire(
                &book.rules.on_in[m.to],
                &Occasion {
                    amount: Some(m.arrive),
                    skip_internal: true,
                    ..on
                },
            );
            self.fire_purpose(m, &on);
            self.fire_spend(m);
            self.fire(&book.rules.always[m.from], &on);
            if m.to != m.from {
                self.fire(&book.rules.always[m.to], &on);
            }
        }
        if self.is_asset_sale(m) {
            self.dispose_sold_asset(m);
        }
    }

    /// Purpose laws see the event after its value has moved and its window
    /// total has been counted. Their `self` is the flow's owner. When the
    /// purpose names an asset, laws about that asset also see the flow.
    fn fire_purpose(&mut self, m: &Motion, on: &Occasion) {
        let Some(purpose) = m.purpose else { return };
        let book = self.plan.book;
        let purpose_on = Occasion {
            amount: Some(m.out),
            ..*on
        };
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
        let out = if leaves {
            self.base_value(m, m.out)
        } else {
            None
        };
        let arrive = if enters {
            self.base_value(m, m.arrive)
        } else {
            None
        };
        self.world
            .totals
            .record(watch, (m.from, m.to), (m.day, m.recognized), out, arrive);

        // Contract-scoped totals describe that contract's occurrences, not
        // all activity of its owner. Runtime future flows already carry their
        // contract identity; journal occurrences resolve through their Txn.
        let contract = match m.txn {
            RuntimeTxn::Journal(txn) => self.plan.book.txns.get(txn.id()).and_then(|txn| txn.contract),
            RuntimeTxn::ContractOccurrence { contract, .. } => Some(contract),
            RuntimeTxn::Adjustment { .. } => None,
        }
        .filter(|&contract| self.plan.book.contracts.get(contract).is_some());
        if let Some(contract) = contract {
            let owner = self.plan.book.contracts[contract].owner;
            let within_owner = Subject::Entity(owner);
            let (source_owned, target_owned) = (
                self.plan.inside(within_owner, m.from),
                self.plan.inside(within_owner, m.to),
            );
            let movement = match (source_owned, target_owned) {
                (true, false) => self.base_value(m, m.out).map(|amount| (Dir::Out, amount)),
                (false, true) => self.base_value(m, m.arrive).map(|amount| (Dir::In, amount)),
                _ => None,
            };
            if let Some((dir, amount)) = movement {
                self.world.totals.record_contract(
                    watch,
                    contract,
                    m.day,
                    m.recognized,
                    dir,
                    amount,
                );
            }
        }

        if let Some(purpose) = m
            .purpose
            .map(|purpose| purpose.purpose)
            .filter(|&purpose| watch.reads_purpose(purpose))
        {
            if let Some((dir, amount)) = self.purpose_flow(m, purpose) {
                self.world.totals.record_purpose(
                    watch,
                    m.owner,
                    purpose,
                    (m.day, m.recognized),
                    dir,
                    amount,
                );
            }
        }
    }

    /// The sign of a purpose follows value crossing the owner's boundary.
    /// The written flow direction is what matters here: paying an expense from
    /// a card is an outflow, and a refund from that expense into the card is an
    /// inflow. The debt balance's display sign must not reverse that meaning.
    fn purpose_flow(
        &mut self,
        m: &Motion,
        purpose: axiom_core::Id<axiom_model::Purpose>,
    ) -> Option<(Dir, Qty)> {
        let (source_owned, target_owned) = (
            m.source.owner == m.owner && m.source.class != Class::Outside,
            m.target.owner == m.owner && m.target.class != Class::Outside,
        );
        let root = self.plan.book.purposes[purpose].root;
        let direction = crate::purpose_direction(source_owned, target_owned, root)?;
        let amount = match direction {
            Dir::Out => m.out,
            Dir::In => m.arrive,
        };
        self.base_value(m, amount).map(|amount| (direction, amount))
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
            self.scratch
                .relief
                .slices
                .push(fresh_slice(m, m.out.qty, is_base, now));
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
        self.world
            .holdings
            .relieve(m.from, unit, &request, &mut self.scratch.relief);
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
            self.scratch
                .relief
                .slices
                .push(fresh_slice(m, shortfall, is_base, now));
        } else if self.scratch.relief.slices.is_empty() {
            self.scratch
                .relief
                .slices
                .push(fresh_slice(m, m.out.qty, is_base, now));
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
        let Some(slot) = self
            .world
            .holdings
            .get(m.from, m.out.unit)
            .filter(|slot| slot.is_tied())
        else {
            return;
        };
        for entity in slot.holding.lots.iter().filter_map(|lot| lot.tied) {
            if !self
                .scratch
                .permits
                .iter()
                .any(|&(known, _)| known == entity)
            {
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
        let priced = restarts
            && (m.source.class == Class::Asset || (m.detail().basis.is_none() && !unbased));
        let proceeds = if priced { self.proceeds(m) } else { None };
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let (mut worth, mut fixed) = (
            proceeds.map(|p| Shares::new(p, whole)),
            m.detail().basis.map(|b| Shares::new(b, whole)),
        );
        for slice in &mut self.scratch.relief.slices {
            slice.worth = worth
                .as_mut()
                .map_or(Qty::ZERO, |shares| shares.take(slice.qty));
            let stated = fixed.as_mut().map(|shares| shares.take(slice.qty));
            slice.carried = match (stated, restarts, proceeds.is_some(), slice.origin) {
                (Some(basis), ..) => basis,
                // A taxed account funding a basis-zero destination is a
                // contribution, even when relief selected an existing lot
                // and the transfer otherwise keeps parcel identity. A move
                // between two tax-deferred accounts still carries its basis.
                (None, _, _, _) if unbased && !m.source.deferred => Qty::ZERO,
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
        m.detail()
            .cost
            .and_then(|cost| self.base_value(m, cost))
            .unwrap_or(Qty::ZERO)
    }

    /// What the parcels that leave realize against their basis: what they
    /// fetched, less the selling cost that comes off it.
    fn realizes(&mut self, m: &Motion) -> Option<Qty> {
        let fetched = self.proceeds(m)?;
        Some(if m.out.unit == self.plan.book.base {
            fetched
        } else {
            fetched - self.exchange_cost(m)
        })
    }

    /// Records a gain, and fires `on gain`, for every relieved lot. Plain money
    /// never realizes: its gain is zero by definition. So in a pro-rata place
    /// holding plain contributions and a zero-basis growth lot, a withdrawal
    /// relieves both and only the lot's share is a gain.
    fn realize(&mut self, m: &Motion) {
        let book = self.plan.book;
        let ambiguous = self.scratch.relief.ambiguous;
        let purpose = self.reimbursed_purpose(m).or(m.purpose);
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
            let realized = Realized {
                gain: slice.worth - slice.basis,
                proceeds: slice.worth,
                basis: slice.basis,
                held: m.day.since(slice.held_since),
                held_since: slice.held_since,
                acquired: slice.acquired,
                quantity: slice.qty,
                part: slice.part,
                codes: m.code_runs,
            };
            let on = Occasion {
                amount: Some(Amount::new(slice.qty, m.out.unit)),
                realized: Some(realized),
                purpose,
                ..Occasion::flow(m)
            };
            self.fire(&book.rules.on_gain[m.from], &on);
        }
    }

    /// A reimbursement's purpose is the purpose of the transaction it names.
    /// A linked transaction with conflicting purposes is ambiguous, so leave
    /// the current flow's purpose in force instead of choosing one line.
    fn reimbursed_purpose(&self, m: &Motion) -> Option<axiom_model::Purposed> {
        let book = self.plan.book;
        let txn = book.txns.get(m.detail().against?)?;
        let mut purpose = None;
        for flow in txn.flows.ids().filter_map(|id| book.flows.get(id)) {
            let Some(found) = flow.purpose else { continue };
            if purpose.is_some_and(|prior| prior != found) {
                return None;
            }
            purpose = Some(found);
        }
        purpose
    }

    /// Lands the slices at the target.
    fn arrive(&mut self, m: &Motion, keeps: bool) {
        let book = self.plan.book;
        if m.target.class != Class::Asset {
            self.world
                .holdings
                .credit(m.to, m.arrive.unit, m.arrive.qty);
            if m.moves == Moves::Loss {
                self.keep_basis(m);
            }
            self.sample_temporal(m.day);
            return;
        }
        let (stays, restricted) = (stays_with_owner(m), self.restricted_source(m));
        // `for` an entity ties what arrives to it; `for` the owner (or its household) unties it.
        let owner = m.target.owner;
        let hold = m.detail().hold.map(|entity| {
            Some(entity).filter(|&e| e != owner && book.entities[owner].member != Some(e))
        });
        let (money, since) = (
            is_money(book, m.to, m.arrive.unit),
            m.detail().since.unwrap_or(m.day),
        );
        let acquisition = self.new_acquisition_part(m);
        let declared_asset = book
            .commodities
            .get(m.arrive.unit)
            .is_some_and(|commodity| book.asset(book.name(commodity.symbol)).is_some());
        let fresh_part = if keeps || money || m.moves != Moves::Value {
            None
        } else {
            acquisition.map(|(_, part)| part.id).or_else(|| {
                (!declared_asset).then_some(PartId {
                    origin: m.txn,
                    ordinal: m.flow_ordinal,
                })
            })
        };
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(m.arrive.qty, whole);
        {
            let slot = self.world.holdings.entry(m.to, m.arrive.unit);
            for slice in &self.scratch.relief.slices {
                let qty = shares.take(slice.qty);
                let kept = if keeps || (stays && slice.origin != Origin::Fresh) {
                    slice.tied
                } else {
                    restricted
                };
                // Parcels that keep their identity keep their day and purchase; the rest start over.
                let (acquired, txn) = if keeps {
                    (slice.acquired, slice.txn)
                } else {
                    (since, m.txn)
                };
                let codes = if keeps { slice.codes } else { m.code_runs };
                slot.land_with_codes(
                    Parcel {
                        qty,
                        basis: slice.carried,
                        acquired,
                    held_since: if keeps { slice.held_since } else { since },
                    wash_matched: keeps && slice.wash_matched,
                        txn,
                        // An ordinary asset-place transfer carries the same
                        // acquisition anchor through every split slice.
                        part: if keeps { slice.part } else { fresh_part },
                        codes,
                        tied: hold.unwrap_or(kept),
                    },
                    money,
                    &book.codes,
                );
            }
        }
        for slice in &self.scratch.relief.slices {
            let part = if keeps { slice.part } else { fresh_part };
            if let Some(part) = part {
                self.world
                    .holdings
                    .index_part_slot(m.to, m.arrive.unit, part);
            }
        }
        let part_ready = if let Some((asset, part)) = acquisition {
            match self.add_asset_part(asset, part) {
                Ok(()) => true,
                Err(error) => {
                    self.report_asset_state_error(m, error);
                    false
                }
            }
        } else {
            !declared_asset
        };
        self.sample_temporal(m.day);
        if !keeps && part_ready {
            if let Some(part) = fresh_part {
                self.match_pending_carries(part, owner, m.arrive.unit, since, m.arrive.qty, m);
                self.sample_temporal(m.day);
            }
        }
    }

    /// An asset's acquisition arrival anchors all its physical parcels to one
    /// part id. Improvements are separate basis parts, but never another unit
    /// of the asset commodity.
    fn new_acquisition_part(&mut self, m: &Motion) -> Option<(Id<Asset>, Part)> {
        let book = self.plan.book;
        if m.target.class != Class::Asset {
            return None;
        }
        let name = book.name(book.commodities.get(m.arrive.unit)?.symbol);
        let asset = book.asset(name)?;
        let declaration = &book.assets[asset];
        if declaration.place != m.to || declaration.unit != m.arrive.unit {
            return None;
        }
        let purchase = self.capital_asset(m) == Some(asset);
        if !m.opening && !purchase && m.detail().basis.is_none() {
            return None;
        }
        if self.world.assets.asset(asset)?.part_count() != 0 {
            return None;
        }
        let ordinal = self.flow_ordinal(m);
        let id = PartId {
            origin: m.txn,
            ordinal,
        };
        let basis = self
            .scratch
            .relief
            .slices
            .iter()
            .try_fold(Qty::ZERO, |sum, slice| {
                sum.0.checked_add(slice.carried.0).map(Qty)
            });
        let Some(basis) = basis else {
            self.report_asset_state_error(m, crate::AssetError::Overflow);
            return None;
        };
        let Some(cost) = self.capital_cost(m) else {
            self.record.report(
                Diagnostic::error(
                    "asset-cost",
                    "the acquisition cost could not be valued in the book's base currency",
                )
                .label(m.loc, "asset part was not recorded"),
            );
            return None;
        };
        let part = Part {
            id,
            flow: self.source_flow(m),
            kind: PartKind::Acquisition,
            recorded: self.event_key(m),
            day: m.detail().since.unwrap_or(m.day),
            cost,
            basis,
        };
        if let Err(error) = self.validate_asset_part(asset, &part) {
            self.report_asset_state_error(m, error);
            return None;
        }
        Some((asset, part))
    }

    /// Capital-purpose outflows name the asset they acquire or improve.
    /// The asset unit is not an endpoint of the payment flow, so its physical
    /// parcel is materialized here from the typed purpose object.
    fn record_capital_outflow(&mut self, m: &Motion) {
        let Some((asset, Dir::Out)) = self.capital_asset_direction(m) else {
            return;
        };
        let book = self.plan.book;
        if m.source.class != Class::Asset || m.target.class == Class::Asset {
            return;
        }
        if m.source.owner != book.assets[asset].owner {
            return;
        }
        if self
            .world
            .assets
            .asset(asset)
            .is_some_and(|state| state.part_count() > 0)
        {
            self.add_improvement_part(m, asset);
        } else {
            self.add_acquisition_part(m, asset);
        }
    }

    fn add_acquisition_part(&mut self, m: &Motion, asset: Id<Asset>) {
        let Some(cost) = self.capital_cost(m) else {
            self.record.report(
                Diagnostic::error(
                    "asset-cost",
                    "the acquisition cost could not be valued in the book's base currency",
                )
                .label(m.loc, "asset acquisition was not recorded"),
            );
            return;
        };
        let part = Part {
            id: PartId {
                origin: m.txn,
                ordinal: self.flow_ordinal(m),
            },
            flow: self.source_flow(m),
            kind: PartKind::Acquisition,
            recorded: self.event_key(m),
            day: m.day,
            cost,
            basis: cost,
        };
        if let Err(error) = self.validate_asset_part(asset, &part) {
            self.report_asset_state_error(m, error);
            return;
        }
        let declaration = &self.plan.book.assets[asset];
        self.world
            .holdings
            .entry(declaration.place, declaration.unit)
            .land_with_codes(
                Parcel {
                    qty: Qty(1),
                    basis: cost,
                    acquired: m.day,
                    held_since: m.day,
                    wash_matched: false,
                    txn: m.txn,
                    part: Some(part.id),
                    codes: m.code_runs,
                    tied: None,
                },
                false,
                &self.plan.book.codes,
            );
        self.world
            .holdings
            .index_part_slot(declaration.place, declaration.unit, part.id);
        if let Err(error) = self.add_asset_part(asset, part) {
            self.report_asset_state_error(m, error);
        }
    }

    /// A capital-purpose outflow on an already held asset is an improvement;
    /// it adds basis to the acquisition parcel without another unit.
    fn add_improvement_part(&mut self, m: &Motion, asset: Id<Asset>) {
        let book = self.plan.book;
        let declaration = &book.assets[asset];
        if m.to == declaration.place && m.arrive.unit == declaration.unit {
            return;
        }
        if m.from == declaration.place && m.out.unit == declaration.unit {
            return;
        }
        let Some(cost) = self.capital_cost(m) else {
            self.record.report(
                Diagnostic::error(
                    "asset-cost",
                    "the improvement cost could not be valued in the book's base currency",
                )
                .label(m.loc, "improvement part was not recorded"),
            );
            return;
        };
        let part = Part {
            id: PartId {
                origin: m.txn,
                ordinal: self.flow_ordinal(m),
            },
            flow: self.source_flow(m),
            kind: PartKind::Improvement,
            recorded: self.event_key(m),
            day: m.day,
            cost,
            basis: cost,
        };
        if let Err(error) = self.add_asset_part(asset, part) {
            self.report_asset_state_error(m, error);
        }
    }

    fn capital_asset(&self, m: &Motion) -> Option<Id<Asset>> {
        let purpose = m.purpose?;
        let book = self.plan.book;
        (book.purposes[purpose.purpose].root == PurposeRoot::Capital)
            .then_some(purpose.of)
            .flatten()
            .and_then(|object| match object {
                Object::Asset(asset) => Some(asset),
                _ => None,
            })
    }

    fn capital_asset_direction(&self, m: &Motion) -> Option<(Id<Asset>, Dir)> {
        let asset = self.capital_asset(m)?;
        let root = self.plan.book.purposes[m.purpose?.purpose].root;
        let direction = crate::purpose_direction(
            m.source.class == Class::Asset,
            m.target.class == Class::Asset,
            root,
        )?;
        Some((asset, direction))
    }

    fn capital_cost(&mut self, m: &Motion) -> Option<Qty> {
        if let Some(stated) = m.detail().basis
            && (m.opening || m.out.unit == m.arrive.unit)
        {
            return Some(stated);
        }
        let cost = self.proceeds(m)?;
        let fees = match m.detail().cost {
            Some(fee) => self.base_value(m, fee)?,
            None => Qty::ZERO,
        };
        cost.0.checked_add(fees.0).map(Qty)
    }

    fn source_flow(&self, m: &Motion) -> Option<Id<axiom_model::Flow>> {
        match m.cause {
            Cause::Flow(flow) => Some(flow),
            Cause::Applied(_) | Cause::Time => None,
        }
    }

    fn flow_ordinal(&self, m: &Motion) -> u32 {
        m.flow_ordinal
    }

    fn event_key(&self, m: &Motion) -> EventKey {
        let sequence = match m.cause {
            Cause::Flow(flow) => u64::try_from(flow.index()).unwrap_or(u64::MAX),
            Cause::Applied(ordinal) => u64::from(ordinal),
            Cause::Time => 0,
        };
        EventKey {
            day: m.day,
            sequence,
        }
    }

    fn is_asset_sale(&self, m: &Motion) -> bool {
        let Some((asset, Dir::In)) = self.capital_asset_direction(m) else {
            return false;
        };
        let declaration = &self.plan.book.assets[asset];
        m.target.owner == declaration.owner
            && self
                .world
                .assets
                .asset(asset)
                .is_some_and(|state| state.part_count() > 0 && state.disposed.is_none())
            && is_money(self.plan.book, m.to, m.arrive.unit)
    }

    fn dispose_sold_asset(&mut self, m: &Motion) {
        let Some((asset, Dir::In)) = self.capital_asset_direction(m) else {
            return;
        };
        self.pre_disposal(asset, m.day);
        let book = self.plan.book;
        let declaration = &book.assets[asset];
        let boundary = DisposalBoundary::After(self.event_key(m));
        let Some(state) = self.world.assets.asset(asset) else {
            self.report_asset_state_error(m, crate::AssetError::UnknownAsset);
            return;
        };
        let Some(anchor) = state.parts().first().map(|part| part.id) else {
            self.report_asset_state_error(m, crate::AssetError::MissingAcquisition);
            return;
        };
        if state.disposed.is_some() {
            self.report_asset_state_error(m, crate::AssetError::AlreadyDisposed);
            return;
        }
        if !state
            .parts()
            .iter()
            .all(|part| part.recorded <= self.event_key(m))
        {
            self.report_asset_state_error(m, crate::AssetError::NotHeldAtBoundary);
            return;
        }
        let basis = match (state.total_basis(), self.world.holdings.part_basis(anchor)) {
            (Ok(total), Ok(held)) if total == held => total,
            (Err(error), _) | (_, Err(error)) => {
                self.report_asset_state_error(m, error);
                return;
            }
            _ => {
                self.report_asset_state_error(m, crate::AssetError::ParcelBasisMismatch);
                return;
            }
        };
        let quantity = self.world.holdings.qty(declaration.place, declaration.unit);
        if quantity.is_negative() || quantity.is_zero() {
            self.report_asset_state_error(m, crate::AssetError::UnknownPart);
            return;
        }
        let Some(gross) = self.proceeds(m) else {
            return;
        };
        let Some(stated_less) = self.asset_sale_less_items(m) else {
            return;
        };
        let proceeds = gross - self.exchange_cost(m) - stated_less;
        if proceeds.is_negative() {
            self.report_asset_state_error(m, crate::AssetError::NegativeAmount);
            return;
        }
        let request = Request {
            need: quantity,
            money: false,
            selectors: &[],
            policy: book.places[declaration.place]
                .select
                .or(book.commodities[declaration.unit].select),
            codes: &book.codes,
            permits: &[],
            spender: None,
            now: (m.day, m.txn),
            explain: &|| false,
        };
        self.world.holdings.relieve(
            declaration.place,
            declaration.unit,
            &request,
            &mut self.scratch.relief,
        );
        self.sample_temporal(m.day);
        if self.scratch.relief.shortfall > Qty::ZERO {
            self.report_asset_state_error(m, crate::AssetError::ParcelBasisMismatch);
            return;
        }
        let relieved: Qty = self
            .scratch
            .relief
            .slices
            .iter()
            .map(|slice| slice.basis)
            .sum();
        if relieved != basis {
            self.report_asset_state_error(m, crate::AssetError::ParcelBasisMismatch);
            return;
        }

        let on_out = Occasion {
            amount: Some(Amount::new(quantity, declaration.unit)),
            purpose: m.purpose,
            ..Occasion::flow(m)
        };
        self.fire(&book.rules.on_out[declaration.place], &on_out);
        let mut shares = Shares::new(proceeds, quantity);
        for at in 0..self.scratch.relief.slices.len() {
            let slice = self.scratch.relief.slices[at];
            let fetched = shares.take(slice.qty);
            let gain = fetched - slice.basis;
            self.record.gains.push(Gain {
                cause: m.cause,
                day: m.day,
                from: declaration.place,
                to: m.to,
                unit: declaration.unit,
                qty: slice.qty,
                basis: slice.basis,
                proceeds: fetched,
                acquired: slice.acquired,
                ambiguous: self.scratch.relief.ambiguous,
            });
            let on_gain = Occasion {
                amount: Some(Amount::new(slice.qty, declaration.unit)),
                realized: Some(Realized {
                    gain,
                    proceeds: fetched,
                    basis: slice.basis,
                    held: m.day.since(slice.held_since),
                    held_since: slice.held_since,
                    acquired: slice.acquired,
                    quantity: slice.qty,
                    part: slice.part,
                    codes: m.code_runs,
                }),
                purpose: m.purpose,
                ..Occasion::flow(m)
            };
            self.fire(&book.rules.on_gain[declaration.place], &on_gain);
        }
        if let Err(error) = self.dispose_asset(asset, m.txn, self.source_flow(m), boundary) {
            self.report_asset_state_error(m, error);
        }
        self.sample_temporal(m.day);
    }

    /// Matches future replacement acquisitions against losses already waiting
    /// in the canonical carry queue. The parcel has been landed and indexed,
    /// and a declared asset part (if any) has been added before this runs.
    fn match_pending_carries(
        &mut self,
        part: PartId,
        owner: Id<Entity>,
        unit: Id<axiom_model::Commodity>,
        acquired: axiom_core::Day,
        quantity: Qty,
        motion: &Motion,
    ) {
        let mut left = quantity;
        let mut matched = Vec::new();
        for index in 0..self.world.assets.pending_carries().len() {
            let Some(request) = self.world.assets.pending_carry(index) else { continue };
            if left.is_zero()
                || request.owner != owner
                || request.unit != unit
                || request.from == part
                || acquired < request.sold
                || !crate::Assets::within_carry_window(request.sold, acquired, request.within)
            {
                continue;
            }
            let take = request.quantity.min(left);
            if take.is_zero() {
                continue;
            }
            let mut shares = Shares::new(request.amount, request.quantity);
            let amount = shares.take(take);
            matched.push((index, request, take, amount));
            left -= take;
        }
        if matched.is_empty() {
            return;
        }
        let additions: Vec<_> = matched
            .iter()
            .map(|(_, request, taken, amount)| crate::lots::CarryLotAddition {
                part,
                acquired,
                held_since: request.held_since,
                quantity: *taken,
                amount: *amount,
            })
            .collect();
        if let Err(error) = self.carry_basis_to_parts(&additions) {
            self.report_asset_state_error(motion, error);
            return;
        }
        self.sample_temporal(motion.day);
        // `index` values refer to the pre-update queue. Removing in reverse
        // order preserves the remaining indices; all updated amounts were
        // precomputed while the basis guard was still untouched.
        for (index, request, taken, amount) in matched.into_iter().rev() {
            let quantity = request.quantity - taken;
            let remaining = request.amount - amount;
            if let Err(error) = self.world.assets.update_pending_carry(index, quantity, remaining) {
                self.report_asset_state_error(motion, error);
                return;
            }
            self.record.adjustments.push(crate::Adjustment {
                day: motion.day,
                law: request.law,
                kind: crate::AdjustmentKind::Carried {
                    from: request.from,
                    to: Some(part),
                },
                amount,
            });
        }
    }

    fn report_asset_state_error(&mut self, m: &Motion, error: crate::AssetError) {
        self.record.report(
            Diagnostic::error(
                "asset-state",
                format!("asset part state could not be updated: {error:?}"),
            )
            .label(m.loc, "asset event"),
        );
    }

    /// A loss in what an asset is worth shrank its parcels and left their
    /// basis unspent: it goes to the parcels that remain, in proportion to
    /// their quantity.
    fn keep_basis(&mut self, m: &Motion) {
        let book = self.plan.book;
        let left: Qty = self
            .scratch
            .relief
            .slices
            .iter()
            .filter(|s| s.origin != Origin::Fresh)
            .map(|s| s.basis)
            .sum();
        let selection = Selection {
            selectors: &[],
            codes: &book.codes,
        };
        let slot = self.world.holdings.entry(m.from, m.out.unit);
        slot.rebase(
            left,
            &selection,
            is_money(book, m.from, m.out.unit),
            (m.day, m.txn),
        );
    }

    /// Fires the `on spend` laws of every entity whose tied money just left
    /// its owner's places.
    fn fire_spend(&mut self, m: &Motion) {
        let book = self.plan.book;
        if stays_with_owner(m) {
            return;
        }
        for at in 0..self.scratch.relief.slices.len() {
            let Some(entity) = self.scratch.relief.slices[at].tied else {
                continue;
            };
            let slices = &self.scratch.relief.slices;
            if slices[..at].iter().any(|s| s.tied == Some(entity)) {
                continue;
            }
            let spent: Qty = slices
                .iter()
                .filter(|s| s.tied == Some(entity))
                .map(|s| s.qty)
                .sum();
            let on = Occasion {
                amount: Some(Amount::new(spent, m.out.unit)),
                ..Occasion::flow(m)
            };
            self.fire(&book.rules.on_spend[entity], &on);
        }
    }

    /// The restricted entity that money crossing to another owner is tied to:
    /// the payee if it is restricted, else the source place's owner.
    fn restricted_source(&self, m: &Motion) -> Option<Id<Entity>> {
        let book = self.plan.book;
        let restricted = |entity: &Id<Entity>| book.entities[*entity].restricted;
        m.payee
            .filter(restricted)
            .or(Some(m.source.owner).filter(restricted))
    }

    /// What the flow's parcels fetched, in the base currency. The exchange
    /// itself says so when one side is the base; otherwise prices at the flow's
    /// day do.
    fn proceeds(&mut self, m: &Motion) -> Option<Qty> {
        let base = self.plan.book.base;
        match (m.arrive.unit == base, m.out.unit == base) {
            (true, _) => Some(m.arrive.qty),
            (_, true) => Some(m.out.qty),
            _ => self
                .base_value(m, m.out)
                .or_else(|| self.base_value(m, m.arrive)),
        }
    }

    /// A sale's header proceeds include explicit Less items before basis is
    /// realized. These item values are carried by the transaction's typed
    /// journal group, not by the header's Detail.cost field.
    fn asset_sale_less_items(&mut self, m: &Motion) -> Option<Qty> {
        let book = self.plan.book;
        let Some(txn_id) = m.txn.source_txn() else { return Some(Qty::ZERO) };
        let Some(txn) = book.txns.get(txn_id) else { return Some(Qty::ZERO) };
        let Some(program_id) = txn.program else { return Some(Qty::ZERO) };
        let program = &book.journal_programs[program_id];
        let Some(group) = program
            .groups
            .iter()
            .find(|group| group.header == Some(m.flow_ordinal)) else { return Some(Qty::ZERO) };
        let mut total = Qty::ZERO;
        for item in group.items.iter().filter(|item| {
            item.parent == axiom_model::TemplateItemParent::Header
                && item.sign == axiom_model::Sign::Less
        }) {
            let amount = match item.amount {
                axiom_model::TemplateAmount::Literal(amount) => amount,
                axiom_model::TemplateAmount::Computed(_) => {
                    self.record.report(
                        Diagnostic::error(
                            "asset-sale-cost",
                            "a computed Less item cannot be valued before asset gain realization",
                        )
                        .label(item.loc, "asset sale proceeds are incomplete"),
                    );
                    return None;
                }
            };
            let value = if amount.unit == book.base {
                amount.qty
            } else {
                let Some(value) = book.convert(amount, book.base, m.day) else {
                    self.record.report(
                        Diagnostic::error(
                            "asset-sale-cost",
                            "a sale cost has no price in the book's base currency",
                        )
                        .label(item.loc, "cost could not be subtracted from proceeds"),
                    );
                    return None;
                };
                value.qty
            };
            let Some(sum) = total.0.checked_add(value.0) else {
                self.report_asset_state_error(m, crate::AssetError::Overflow);
                return None;
            };
            total = Qty(sum);
        }
        Some(total)
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
        let value = book
            .convert(amount, book.base, m.day)
            .map(|priced| priced.qty);
        self.scratch.worth.push((amount, value));
        if value.is_none()
            && self
                .record
                .missing
                .insert(Missing::Price(amount.unit, book.base))
        {
            let fault = Fault::NoPrice {
                unit: amount.unit,
                quote: book.base,
            };
            let (what, help) = show::fault(book, fault, m.day);
            let mut d = Diagnostic::error("no-price", what)
                .label(m.loc, format!("needed to value {}", book.show(amount)));
            if let Some(help) = help {
                d = d.help(help);
            }
            self.record.report(d);
        }
        value
    }
}

#[cfg(test)]
mod asset_flow_tests {
    use axiom_core::{Day, FileId};
    use axiom_model::Source;
    use axiom_syntax::Folder;

    use crate::{DisposalBoundary, EventKey, Options, PartKind, Plan};

    fn day(year: i32, month: u32, day: u32) -> Day {
        Day::from_ymd(year, month, day).unwrap()
    }

    fn book<'s>(text: &'s str) -> axiom_model::Book<'s> {
        let (file, parsed) = axiom_syntax::parse(FileId(0), text, Folder::default());
        assert!(parsed.is_empty(), "source parses: {parsed:?}");
        let (book, built) = axiom_model::build(&[Source {
            path: "asset-test.ax",
            file,
            embedded: false,
        }]);
        assert!(
            built.iter().all(|diagnostic| !diagnostic.is_error()),
            "source builds: {built:?}"
        );
        book
    }

    #[test]
    fn capital_outflows_acquire_or_improve_and_inflow_sells_the_asset() {
        let text = "\
base USD
commodity USD
  precision 2
kind property : thing
purpose purchase : capital
  of asset
purpose improvement : capital
  of asset
purpose sale : capital
  of asset
purpose fees : spending
account assets/checking
entity contractor
entity buyer
asset condo : property

opening 2025-01-01
  checking 5_000 USD

2025-01-05 checking -> contractor 1_000 USD #purchase of condo
2025-02-15 checking -> contractor 100 USD #improvement of condo
2025-03-01 buyer -> checking 1_500 USD #sale of condo
  - 60 USD #fees
";
        let book = book(text);
        let run = crate::run(
            &book,
            Options {
                today: day(2025, 3, 31),
                relaxed: false,
            },
        );
        let errors: Vec<_> = run
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .collect();
        assert!(
            errors.is_empty(),
            "native purchase/improvement/sale: {errors:?}"
        );

        let asset = book.asset("condo").unwrap();
        let state = &run.assets[asset.index()];
        assert_eq!(
            state.parts().len(),
            2,
            "the capital payment acquires one part and the later payment adds one improvement"
        );
        assert_eq!(state.parts()[0].kind, PartKind::Acquisition);
        assert_eq!(state.parts()[1].kind, PartKind::Improvement);
        assert_eq!(
            (
                state.total_cost().unwrap().0,
                state.total_basis().unwrap().0
            ),
            (110_000, 110_000)
        );
        assert_eq!(
            state.disposed.map(|disposal| disposal.boundary),
            Some(DisposalBoundary::After(EventKey {
                day: day(2025, 3, 1),
                sequence: u64::from(book.flows.len() as u32 - 2),
            }))
        );

        let sale = run
            .gains
            .iter()
            .find(|gain| gain.day == day(2025, 3, 1))
            .expect("sale records realized gain");
        assert_eq!(
            (sale.proceeds.0, sale.basis.0, sale.gain().0),
            (144_000, 110_000, 34_000)
        );
        assert!(
            !run.holdings
                .iter()
                .any(|holding| holding.place == book.assets[asset].place),
            "the sold asset unit is relieved"
        );
        let checking = book.place("assets/checking").unwrap();
        let usd = book.commodity("USD").unwrap();
        let cash = run
            .holdings
            .iter()
            .find(|holding| holding.place == checking && holding.unit == usd)
            .unwrap();
        assert_eq!(cash.qty().0, 5_340_00);
    }

    #[test]
    fn source_acquisition_improvement_consume_carry_and_disposal_share_one_asset_parcel() {
        let text = "\
base USD
commodity USD
  precision 2
kind property : thing
purpose improvement : capital
  of asset
account assets/checking
entity contractor
asset condo : property
asset cabin : property

opening 2025-01-01
  checking 5_000 USD
  condo basis 1_000 USD since 2024-01-01
  cabin basis 500 USD since 2024-01-01
2025-02-15 checking -> contractor 100 USD #improvement of condo
";
        let book = book(text);
        let options = Options {
            today: day(2025, 3, 31),
            relaxed: false,
        };
        let plan = Plan::new(&book);
        let (initial, mut ledger) = plan.run_with_view(options);
        let asset = book.asset("condo").unwrap();
        let cabin = book.asset("cabin").unwrap();
        let condo_parts = initial.assets[asset.index()].parts();
        let cabin_part = initial.assets[cabin.index()].parts()[0].id;
        assert_eq!(
            condo_parts.len(),
            2,
            "opening basis and improvement are separate clocks"
        );
        assert_eq!(condo_parts[0].kind, PartKind::Acquisition);
        assert_eq!(condo_parts[1].kind, PartKind::Improvement);
        assert_eq!(
            initial.assets[asset.index()].total_cost().unwrap().0,
            110_000
        );

        let improvement = condo_parts[1].id;
        let consumed = ledger
            .consume_asset_part(asset, improvement, axiom_core::Qty(2_000))
            .unwrap();
        assert_eq!((consumed.applied.0, consumed.excess.0), (2_000, 0));
        let carry = ledger
            .carry_asset_basis(cabin_part, Some((asset, improvement)), axiom_core::Qty(500))
            .unwrap();
        assert_eq!(carry.to, Some(improvement));
        ledger
            .dispose_asset(
                asset,
                cabin_part.origin,
                None,
                DisposalBoundary::After(EventKey {
                    day: day(2025, 3, 1),
                    sequence: 0,
                }),
            )
            .unwrap();
        let run = ledger.finish();

        let state = &run.assets[asset.index()];
        assert_eq!(state.total_cost().unwrap().0, 110_000);
        assert_eq!(state.total_basis().unwrap().0, 108_500);
        assert!(state.held_at(EventKey {
            day: day(2025, 3, 1),
            sequence: 0
        }));
        assert!(!state.held_at(EventKey {
            day: day(2025, 3, 1),
            sequence: 1
        }));
        let checking = book.place("assets/checking").unwrap();
        let usd = book.commodity("USD").unwrap();
        let cash = run
            .holdings
            .iter()
            .find(|holding| holding.place == checking && holding.unit == usd)
            .unwrap();
        assert_eq!(
            cash.qty().0,
            4_900_00,
            "only the improvement is a cash flow in this fixture"
        );
        let condo = run
            .holdings
            .iter()
            .find(|holding| {
                holding.place == book.assets[asset].place && holding.unit == book.assets[asset].unit
            })
            .unwrap();
        assert_eq!(
            condo.qty().0,
            1,
            "basis parts never duplicate the physical asset unit"
        );
        assert_eq!(
            condo.lots.iter().map(|lot| lot.basis.0).sum::<i64>(),
            108_500
        );
    }
}
