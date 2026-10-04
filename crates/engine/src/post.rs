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

use axiom_core::{Day, Diagnostic, Id, Loc, Qty};
use axiom_model::{
    Amount, Asset, Basis, Class, Contract, Dir, Entity, Fault, Object, Place, Purpose, PurposeRoot, RuntimeTxn, Select,
    Subject, Table, Watch,
};

use crate::eval::{Occasion, Realized};
use crate::explain;
use crate::ledger::Ledger;
use crate::lots::{Origin, Request, Selection, Shares, Slice};
use crate::motion::{Course, Motion, Moves};
use crate::plan::Plan;
use crate::recognition::{Counting, Counts, Dealing, Piece, Share};
use crate::scope::{is_money, stays_with_owner};
use crate::settle::{Claiming, Relief};
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
fn restarts_basis(plan: &Plan, m: &Motion) -> bool {
    let from = m.source;
    let (from_deferred, to_deferred) = (plan.traits.place(m.from).deferred, plan.traits.place(m.to).deferred);
    // v3 bridge: a change of basis fetches nothing.
    if m.moves == Moves::Loss {
        return false;
    }
    if from.class != Class::Asset {
        return true;
    }
    if from_deferred && to_deferred {
        return false;
    }
    m.is_exchange() || from_deferred || !stays_with_owner(m)
}

impl Ledger<'_, '_, '_> {
    /// Moves one flow's value and fires every law that watches it. What the laws derive is queued, and posted by
    /// [`post`](Ledger::post) once this has finished with them all.
    pub(crate) fn post_flow(&mut self, m: &Motion) {
        let on = Occasion::flow(m);
        let watched = !m.opening;
        self.scratch.worth.clear();
        self.accept_waiver(m);
        let (claiming, relief) = self.deal_with_claims(m);
        let paid = claiming.as_ref().map_or(Qty::ZERO, Claiming::paid);
        if watched {
            self.count_leaving(m, claiming.as_ref(), &on);
        }
        if self.holds_parcels(m) || m.moves != Moves::Value {
            if relief == Relief::Pending {
                self.relieve(m, paid);
            }
            let keeps = self.price(m);
            self.arrive(m, keeps);
        } else {
            // Places that hold only a plain balance have no parcels to move, and nothing was relieved.
            self.scratch.relief.slices.clear();
            self.world.holdings.credit(m.from, m.out.unit, paid - m.out.qty);
            self.world.holdings.credit(m.to, m.arrive.unit, m.arrive.qty);
            self.sample_temporal(m.day);
        }
        self.record_capital_outflow(m);
        self.sample_temporal(m.day);
        if watched {
            self.fire_arrival(m, &on);
        }
        if self.is_asset_sale(m) {
            self.dispose_sold_asset(m);
        }
        self.record_balances(m.day);
    }

    /// Whether either end of a flow holds parcels: an asset, or a debt place that says `claim` (what is owed is a parcel there
    /// as well). Between places that hold none a flow is two credits.
    fn holds_parcels(&self, m: &Motion) -> bool {
        let holds = |end: &Place, at: Id<Place>| match end.class {
            Class::Asset => true,
            Class::Debt => self.plan.traits.place(at).claim,
            Class::Outside => false,
        };
        holds(m.source, m.from) || holds(m.target, m.to)
    }

    /// A `!` on an assertion accepts its gap: it is never unused.
    fn accept_waiver(&mut self, m: &Motion) {
        if let (Cause::Flow(_) | Cause::Transaction(_) | Cause::Applied(_) | Cause::Derived(_), false, Some(waive)) =
            (m.cause, m.opening, m.waive)
        {
            self.record.waivers.entry(waive.loc).or_insert(false);
        }
    }

    /// What a flow counts as depends on the claims it settled (or, run backwards, opened), which relief of the tab or of
    /// the claim place it came out of decides. A claim place is relieved here, first: the claims it gave up are what the
    /// flow settled. Says whether the source has been relieved.
    fn deal_with_claims(&mut self, m: &Motion) -> (Option<Claiming>, Relief) {
        let claiming = self.settle_claims(m);
        if !(self.plan.traits.place(m.from).claim && m.source.class == Class::Asset) {
            return (claiming, Relief::Pending);
        }
        self.relieve(m, Qty::ZERO);
        (self.relieved_claims(m), Relief::Done)
    }

    /// Counts a flow toward what laws read, and fires the laws that watch what leaves its source.
    fn count_leaving(&mut self, m: &Motion, claiming: Option<&Claiming>, on: &Occasion) {
        self.count(m);
        self.count_purposes(m, claiming);
        self.sample_temporal(m.day);
        let leaving = Occasion { amount: Some(m.out), skip_internal: true, ..*on };
        self.fire(self.plan.book.rules.at(Watch::Out(m.from)), &leaving);
    }

    /// Fires the laws that watch what arrives, the purposes and the spending a flow is for, and the laws that watch
    /// every flow of either place.
    fn fire_arrival(&mut self, m: &Motion, on: &Occasion) {
        let rules = &self.plan.book.rules;
        self.fire(rules.at(Watch::In(m.to)), &Occasion { amount: Some(m.arrive), skip_internal: true, ..*on });
        self.fire_touching(m, on);
        self.fire_purpose(m, on);
        self.fire_contract(m, on);
        self.fire_spend(m);
        self.fire(rules.at(Watch::Always(m.from)), on);
        if m.to != m.from {
            self.fire(rules.at(Watch::Always(m.to)), on);
        }
    }

    /// The laws that say what happens to a flow at a place, either end: an account's, its kind's, and those of the
    /// entity that stands there and of its kind. Value that moved around inside what a law governs entered and left
    /// nothing, so such a flow does not fire it.
    fn fire_touching(&mut self, m: &Motion, on: &Occasion) {
        let rules = &self.plan.book.rules;
        // A book that writes no `on flow` law under a place, a kind or an entity looks nothing up, for any flow.
        if !rules.watches(Table::Touching) {
            return;
        }
        let on = Occasion { amount: Some(m.out), skip_internal: true, ..*on };
        self.fire(rules.at(Watch::Touching(m.from)), &on);
        if m.to != m.from {
            self.fire(rules.at(Watch::Touching(m.to)), &on);
        }
    }

    /// Purpose laws see the event after its value has moved and its window
    /// total has been counted. Their `self` is the flow's owner. When the
    /// purpose names an asset, laws about that asset also see the flow. A law
    /// sees each piece the flow counts in (`recognition`) as the flow, with
    /// that purpose and that much of it.
    fn fire_purpose(&mut self, m: &Motion, on: &Occasion) {
        let book = self.plan.book;
        for at in 0..self.scratch.pieces.len() {
            let Piece { purpose, share, recognized, .. } = self.scratch.pieces[at];
            let Some(purpose) = purpose else { continue };
            let amount = match share {
                Share::Whole => m.out,
                Share::Part(qty) => Amount::new(qty, m.out.unit),
            };
            let purpose_on = Occasion { purpose: Some(purpose), amount: Some(amount), over: recognized, ..*on };
            self.fire_as(book.rules.at(Watch::Purpose(purpose.purpose)), &purpose_on, Some(Subject::Entity(m.owner)));
            if let Some(Object::Asset(asset)) = purpose.of {
                let place = book.assets[asset].place;
                self.fire(book.rules.at(Watch::About(place)), &purpose_on);
            }
        }
    }

    /// The contract whose promise a flow keeps: an occurrence a line wrote, or one the fold made.
    fn contract_of(&self, m: &Motion) -> Option<Id<Contract>> {
        let book = self.plan.book;
        match m.txn {
            RuntimeTxn::Journal(txn) => book.txns.get(txn.id()).and_then(|txn| txn.contract),
            RuntimeTxn::ContractOccurrence { contract, .. } => Some(contract),
            RuntimeTxn::Adjustment { .. } | RuntimeTxn::Derived(_) => None,
        }
        .filter(|&contract| book.contracts.get(contract).is_some())
    }

    /// A flow of a contract's occurrence is judged by the laws written in the contract, whose `self` is the contract.
    fn fire_contract(&mut self, m: &Motion, on: &Occasion) {
        let Some(contract) = self.contract_of(m) else { return };
        let rules = self.plan.book.rules.at(Watch::Contract(contract));
        self.fire(rules, &Occasion { amount: Some(m.out), ..*on });
    }

    /// Adds the flow to the totals some law reads, valuing only the sides
    /// that matter.
    fn count(&mut self, m: &Motion) {
        let watch = &self.plan.watch;
        let (leaves, enters) = watch.sides(m.from, m.to);
        let out = if leaves { self.base_value(m, m.out) } else { None };
        let arrive = if enters { self.base_value(m, m.arrive) } else { None };
        self.world.totals.record(watch, (m.from, m.to), (m.day, m.recognized), out, arrive);

        // Contract-scoped totals describe that contract's occurrences, not
        // all activity of its owner. Runtime future flows already carry their
        // contract identity; journal occurrences resolve through their Txn.
        if let Some(contract) = self.contract_of(m) {
            let owner = self.plan.book.contracts[contract].owner;
            let within_owner = Subject::Entity(owner);
            let (source_owned, target_owned) =
                (self.plan.inside(within_owner, m.from), self.plan.inside(within_owner, m.to));
            let movement = match (source_owned, target_owned) {
                (true, false) => self.base_value(m, m.out).map(|amount| (Dir::Out, amount)),
                (false, true) => self.base_value(m, m.arrive).map(|amount| (Dir::In, amount)),
                _ => None,
            };
            if let Some((dir, amount)) = movement {
                self.world.totals.record_contract(watch, contract, m.day, m.recognized, dir, amount);
            }
        }
    }

    /// Counts what the flow is worth to the purposes some law reads, as `recognition` says it counts: in full when it
    /// moved, nothing when it only made a claim, and the claims' purposes for what it settled. The pieces stay in
    /// `scratch.pieces` for the laws that fire on them.
    fn count_purposes(&mut self, m: &Motion, claiming: Option<&Claiming>) {
        if m.purpose.is_none() && claiming.is_none() {
            self.scratch.pieces.clear();
            return;
        }
        let dealing = match claiming {
            Some(claiming) => claiming.dealing(m.out.qty),
            None if self.plan.makes_claim(m.from, m.to) => Dealing::Making,
            None => Dealing::Ordinary,
        };
        Counting::moving(self.plan, m, dealing).pieces(self.plan.book, &mut self.scratch.pieces);
        let watch = &self.plan.watch;
        for at in 0..self.scratch.pieces.len() {
            let piece = self.scratch.pieces[at];
            let Some(purpose) = piece.purpose.map(|purpose| purpose.purpose).filter(|&p| watch.reads_purpose(p)) else {
                continue;
            };
            if let Some((dir, amount)) = self.purpose_flow(m, piece, purpose) {
                self.world.totals.record_purpose(watch, m.owner, purpose, (m.day, piece.recognized), dir, amount);
            }
        }
    }

    /// The sign of a purpose follows value crossing the owner's boundary.
    /// The written flow direction is what matters here: paying an expense from
    /// a card is an outflow, and a refund from that expense into the card is an
    /// inflow. The debt balance's display sign must not reverse that meaning. A piece that is a claim settled counts
    /// the way the claim did, whatever the payment's own ends say.
    fn purpose_flow(&mut self, m: &Motion, piece: Piece, purpose: Id<Purpose>) -> Option<(Dir, Qty)> {
        let direction = match piece.counts {
            Counts::Claim { dir, .. } => dir,
            Counts::Flow => {
                let (source_owned, target_owned) = (
                    m.source.owner == m.owner && m.source.class != Class::Outside,
                    m.target.owner == m.owner && m.target.class != Class::Outside,
                );
                let root = self.plan.book.purposes[purpose].root;
                crate::purpose_direction(source_owned, target_owned, root)?
            }
        };
        let side = match direction {
            Dir::Out => m.out,
            Dir::In => m.arrive,
        };
        let amount = match piece.share {
            Share::Whole => side,
            Share::Part(qty) => Amount::new(qty, side.unit),
        };
        self.base_value(m, amount).map(|amount| (direction, amount))
    }

    /// Takes `m.out` from the source, leaving the value in flight in
    /// `scratch.relief.slices`. `settled` of it paid claims, which the tab it came out of already counted.
    fn relieve(&mut self, m: &Motion, settled: Qty) {
        let book = self.plan.book;
        let (unit, now) = (m.out.unit, (m.day, m.txn));
        self.scratch.relief.slices.clear();
        if m.source.class != Class::Asset {
            return self.relieve_balance(m, settled);
        }
        self.ask_ties(m);
        let named = self.name_claims(m, m.from);
        // A flow's selector, then the place's policy, then what the commodity says (currencies are FIFO).
        let policy = self.plan.traits.place(m.from).select.or(self.plan.traits.unit_select(unit));
        let request = Request {
            money: is_money(self.plan, m.from, unit),
            selectors: if named { &self.scratch.selectors } else { m.select() },
            permits: &self.scratch.permits,
            spender: m.detail().spender,
            explain: &|| !self.record.ambiguous.contains(&m.from),
            ..Request::of(m.out.qty, policy, &book.codes, now)
        };
        self.world.holdings.relieve(m.from, unit, &request, &mut self.scratch.relief);
        self.account_for_relief(m);
    }

    /// A source that holds no parcels to give up, a debt or the outside: the balance falls by what leaves, less what settled a
    /// claim, and the value in flight is one fresh slice. A debt place that says `claim` owes what leaves as a bill, a parcel.
    fn relieve_balance(&mut self, m: &Motion, settled: Qty) {
        self.scratch.relief.slices.clear();
        match m.source.class == Class::Debt && self.plan.traits.place(m.from).claim && m.course == Course::Forward {
            true => self.owe(m),
            false => self.world.holdings.credit(m.from, m.out.unit, settled - m.out.qty),
        }
        let fresh = fresh_slice(m, m.out.qty, m.out.unit == self.plan.book.base, (m.day, m.txn));
        self.scratch.relief.slices.push(fresh);
    }

    /// What a relief that has been made says: an ambiguous choice, what was missing, and the fresh slice for it.
    fn account_for_relief(&mut self, m: &Motion) {
        let book = self.plan.book;
        let (unit, now) = (m.out.unit, (m.day, m.txn));
        let is_base = unit == book.base;
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

    /// Lets the codes a flow carries name the claims it settles at `at` (LANGUAGE §7: "those its codes name"): each code
    /// that a claim there carries joins the selectors in `scratch.selectors`, unless the flow chose by a code or a day
    /// itself. A code that names no claim there is a label, as it was. Whether there is anything to select by.
    pub(crate) fn name_claims(&mut self, m: &Motion, at: Id<Place>) -> bool {
        let book = self.plan.book;
        let chosen = m.select().iter().any(|select| matches!(select, Select::Code(_) | Select::Range(_)));
        let slot = self.world.holdings.get(at, m.out.unit);
        let Some(slot) = slot.filter(|_| self.plan.traits.place(at).claim && !chosen) else { return false };
        let codes = [m.code_runs.header, m.code_runs.local].into_iter().flat_map(|run| book.codes[run].iter().copied());
        let named = codes.filter(|&code| slot.carries(code, &book.codes));
        self.scratch.selectors.clear();
        self.scratch.selectors.extend(m.select());
        self.scratch.selectors.extend(named.map(Select::Code));
        self.scratch.selectors.len() > m.select().len()
    }

    /// Learns, for each entity a parcel at the source is tied to, whether its
    /// `on spend` laws permit this flow. Only a flow that leaves the owner's
    /// places spends anything; on an internal transfer tied parcels go last.
    /// A flow written out of an entity says whose money it is, so it needs no
    /// law to say so: that entity's parcels go first, and nobody else's are
    /// asked.
    fn ask_ties(&mut self, m: &Motion) {
        self.scratch.permits.clear();
        let Some(slot) = self.world.holdings.get(m.from, m.out.unit).filter(|slot| slot.is_tied()) else {
            return;
        };
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
        let restarts = restarts_basis(self.plan, m);
        // Value from outside takes the target's arrival rule; a market's growth has no basis.
        let unbased = self.plan.traits.place(m.to).basis == Basis::Zero || m.moves == Moves::Growth;
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
                // A taxed account funding a basis-zero destination is a
                // contribution, even when relief selected an existing lot
                // and the transfer otherwise keeps parcel identity. A move
                // between two tax-deferred accounts still carries its basis.
                (None, _, _, _) if unbased && !self.plan.traits.place(m.from).deferred => Qty::ZERO,
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
            self.fire(book.rules.at(Watch::Gain(m.from)), &on);
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

    /// A target that holds no parcels, a debt or the outside, only a balance: it rises by what arrives.
    fn arrive_balance(&mut self, m: &Motion) {
        self.world.holdings.credit(m.to, m.arrive.unit, m.arrive.qty);
        if m.moves == Moves::Loss {
            self.keep_basis(m);
        }
        self.sample_temporal(m.day);
    }

    /// Lands the slices at the target.
    fn arrive(&mut self, m: &Motion, keeps: bool) {
        let book = self.plan.book;
        if m.target.class != Class::Asset {
            return self.arrive_balance(m);
        }
        let (stays, restricted) = (stays_with_owner(m), self.restricted_source(m));
        // `for` an entity ties what arrives to it; `for` the owner (or its household) unties it.
        let owner = m.target.owner;
        let hold = m
            .detail()
            .hold
            .map(|entity| Some(entity).filter(|&e| e != owner && self.plan.traits.entity(owner).member != Some(e)));
        let (money, since) = (is_money(self.plan, m.to, m.arrive.unit), m.detail().since.unwrap_or(m.day));
        let acquisition = self.new_acquisition_part(m);
        let declared_asset = book
            .commodities
            .get(m.arrive.unit)
            .is_some_and(|commodity| book.asset(book.name(commodity.symbol)).is_some());
        let fresh_part = if keeps || money || m.moves != Moves::Value {
            None
        } else {
            acquisition
                .map(|(_, part)| part.id)
                .or_else(|| (!declared_asset).then_some(PartId { origin: m.txn, ordinal: m.flow_ordinal }))
        };
        let whole: Qty = self.scratch.relief.slices.iter().map(|s| s.qty).sum();
        let mut shares = Shares::new(m.arrive.qty, whole);
        {
            let slot = self.world.holdings.entry(m.to, m.arrive.unit);
            for slice in &self.scratch.relief.slices {
                let qty = shares.take(slice.qty);
                let kept = if keeps || (stays && slice.origin != Origin::Fresh) { slice.tied } else { restricted };
                // Parcels that keep their identity keep their day and purchase; the rest start over.
                let (acquired, txn) = if keeps { (slice.acquired, slice.txn) } else { (since, m.txn) };
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
                self.world.holdings.index_part_slot(m.to, m.arrive.unit, part);
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
        let id = PartId { origin: m.txn, ordinal };
        let basis = self
            .scratch
            .relief
            .slices
            .iter()
            .try_fold(Qty::ZERO, |sum, slice| sum.0.checked_add(slice.carried.0).map(Qty));
        let Some(basis) = basis else {
            self.report_asset_state_error(m, crate::AssetError::Overflow);
            return None;
        };
        let Some(cost) = self.capital_cost(m) else {
            self.record.report(
                Diagnostic::error("asset-cost", "the acquisition cost could not be valued in the book's base currency")
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
        if self.world.assets.asset(asset).is_some_and(|state| state.part_count() > 0) {
            self.add_improvement_part(m, asset);
        } else {
            self.add_acquisition_part(m, asset);
        }
    }

    fn add_acquisition_part(&mut self, m: &Motion, asset: Id<Asset>) {
        let Some(cost) = self.capital_cost(m) else {
            self.record.report(
                Diagnostic::error("asset-cost", "the acquisition cost could not be valued in the book's base currency")
                    .label(m.loc, "asset acquisition was not recorded"),
            );
            return;
        };
        let part = Part {
            id: PartId { origin: m.txn, ordinal: self.flow_ordinal(m) },
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
        self.world.holdings.entry(declaration.place, declaration.unit).land_with_codes(
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
        self.world.holdings.index_part_slot(declaration.place, declaration.unit, part.id);
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
                Diagnostic::error("asset-cost", "the improvement cost could not be valued in the book's base currency")
                    .label(m.loc, "improvement part was not recorded"),
            );
            return;
        };
        let part = Part {
            id: PartId { origin: m.txn, ordinal: self.flow_ordinal(m) },
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
        (book.purposes[purpose.purpose].root == PurposeRoot::Capital).then_some(purpose.of).flatten().and_then(
            |object| match object {
                Object::Asset(asset) => Some(asset),
                _ => None,
            },
        )
    }

    fn capital_asset_direction(&self, m: &Motion) -> Option<(Id<Asset>, Dir)> {
        let asset = self.capital_asset(m)?;
        let root = self.plan.book.purposes[m.purpose?.purpose].root;
        let direction = crate::purpose_direction(m.source.class == Class::Asset, m.target.class == Class::Asset, root)?;
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
            Cause::Transaction(_) | Cause::Applied(_) | Cause::Time | Cause::Derived(_) => None,
        }
    }

    fn flow_ordinal(&self, m: &Motion) -> u32 {
        m.flow_ordinal
    }

    fn event_key(&self, m: &Motion) -> EventKey {
        let sequence = match m.cause {
            Cause::Flow(flow) => u64::try_from(flow.index()).unwrap_or(u64::MAX),
            Cause::Transaction(txn) => u64::try_from(txn.index())
                .unwrap_or(u64::MAX >> 32)
                .checked_shl(32)
                .and_then(|prefix| prefix.checked_add(u64::from(m.flow_ordinal)))
                .unwrap_or(u64::MAX),
            Cause::Applied(ordinal) => u64::from(ordinal),
            // After every flow the journal and the fold number, in the order they derived.
            Cause::Derived(offspring) => (1 << 62) + offspring.index() as u64,
            Cause::Time => 0,
        };
        EventKey { day: m.day, sequence }
    }

    fn is_asset_sale(&self, m: &Motion) -> bool {
        let Some((asset, Dir::In)) = self.capital_asset_direction(m) else {
            return false;
        };
        let declaration = &self.plan.book.assets[asset];
        m.target.owner == declaration.owner
            && self.world.assets.asset(asset).is_some_and(|state| state.part_count() > 0 && state.disposed.is_none())
            && is_money(self.plan, m.to, m.arrive.unit)
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
        if !state.parts().iter().all(|part| part.recorded <= self.event_key(m)) {
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
        let policy =
            self.plan.traits.place(declaration.place).select.or(self.plan.traits.unit_select(declaration.unit));
        let request = Request::of(quantity, policy, &book.codes, (m.day, m.txn));
        self.world.holdings.relieve(declaration.place, declaration.unit, &request, &mut self.scratch.relief);
        self.sample_temporal(m.day);
        if self.scratch.relief.shortfall > Qty::ZERO {
            self.report_asset_state_error(m, crate::AssetError::ParcelBasisMismatch);
            return;
        }
        let relieved: Qty = self.scratch.relief.slices.iter().map(|slice| slice.basis).sum();
        if relieved != basis {
            self.report_asset_state_error(m, crate::AssetError::ParcelBasisMismatch);
            return;
        }

        let on_out =
            Occasion { amount: Some(Amount::new(quantity, declaration.unit)), purpose: m.purpose, ..Occasion::flow(m) };
        self.fire(book.rules.at(Watch::Out(declaration.place)), &on_out);
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
            self.fire(book.rules.at(Watch::Gain(declaration.place)), &on_gain);
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
                kind: crate::AdjustmentKind::Carried { from: request.from, to: Some(part) },
                amount,
            });
        }
    }

    fn report_asset_state_error(&mut self, m: &Motion, error: crate::AssetError) {
        self.record.report(
            Diagnostic::error("asset-state", format!("asset part state could not be updated: {error:?}"))
                .label(m.loc, "asset event"),
        );
    }

    /// A loss in what an asset is worth shrank its parcels and left their
    /// basis unspent: it goes to the parcels that remain, in proportion to
    /// their quantity.
    fn keep_basis(&mut self, m: &Motion) {
        let book = self.plan.book;
        let left: Qty = self.scratch.relief.slices.iter().filter(|s| s.origin != Origin::Fresh).map(|s| s.basis).sum();
        let selection = Selection { selectors: &[], codes: &book.codes };
        let slot = self.world.holdings.entry(m.from, m.out.unit);
        slot.rebase(left, &selection, is_money(self.plan, m.from, m.out.unit), (m.day, m.txn));
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
            let spent: Qty = slices.iter().filter(|s| s.tied == Some(entity)).map(|s| s.qty).sum();
            let on = Occasion { amount: Some(Amount::new(spent, m.out.unit)), ..Occasion::flow(m) };
            self.fire(book.rules.at(Watch::Spend(entity)), &on);
        }
    }

    /// The restricted entity that money crossing to another owner is tied to:
    /// the payee if it is restricted, else the source place's owner.
    fn restricted_source(&self, m: &Motion) -> Option<Id<Entity>> {
        let restricted = |entity: &Id<Entity>| self.plan.traits.entity(*entity).restricted;
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

    /// A sale's header proceeds include explicit Less items before basis is
    /// realized. These item values are carried by the transaction's typed
    /// journal group, not by the header's Detail.cost field. A `Less` that makes
    /// no flow is already off the header, which the model carved it from.
    fn asset_sale_less_items(&mut self, m: &Motion) -> Option<Qty> {
        let book = self.plan.book;
        let Some(txn_id) = m.txn.source_txn() else { return Some(Qty::ZERO) };
        let Some(txn) = book.txns.get(txn_id) else { return Some(Qty::ZERO) };
        let Some(program_id) = txn.program else { return Some(Qty::ZERO) };
        let program = &book.journal_programs[program_id];
        let header = axiom_model::Heading::Flow(m.flow_ordinal);
        let Some(group) = program.group.as_deref().filter(|group| group.header == header) else {
            return Some(Qty::ZERO);
        };
        let mut total = Qty::ZERO;
        for item in group.items.iter().filter(|item| item.sign == axiom_model::Sign::Less && item.flow.is_some()) {
            let amount = match item.amount {
                // What a share came to is in the flow it made: the model solved it with the header.
                axiom_model::Cut::Share(_) => {
                    let flow =
                        &book.flows[axiom_core::Id::new(txn.flows.start().index() as u32 + item.flow.unwrap_or(0))];
                    flow.out
                }
                axiom_model::Cut::Of(axiom_model::Expr::Literal(amount)) => amount,
                axiom_model::Cut::Of(axiom_model::Expr::Computed(_)) => {
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
                        Diagnostic::error("asset-sale-cost", "a sale cost has no price in the book's base currency")
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
        self.base_value_on((m.day, m.loc), amount)
    }

    /// `amount` in the base currency on `day`, a price missing then being said at `loc`.
    pub(crate) fn base_value_on(&mut self, (day, loc): (Day, Loc), amount: Amount) -> Option<Qty> {
        let book = self.plan.book;
        if amount.unit == book.base {
            return Some(amount.qty);
        }
        if let Some(&(_, worth)) = self.scratch.worth.iter().find(|&&(seen, _)| seen == amount) {
            return worth;
        }
        let value = book.convert(amount, book.base, day).map(|priced| priced.qty);
        self.scratch.worth.push((amount, value));
        if value.is_none() && self.record.missing.insert(Missing::Price(amount.unit, book.base)) {
            let fault = Fault::NoPrice { unit: amount.unit, quote: book.base };
            let (what, help) = show::fault(book, fault, day);
            let mut d =
                Diagnostic::error("no-price", what).label(loc, format!("needed to value {}", book.show(amount)));
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
        let (book, built) = axiom_model::build(&[Source { path: "asset-test.ax", file, embedded: false }]);
        assert!(built.iter().all(|diagnostic| !diagnostic.is_error()), "source builds: {built:?}");
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
        let run = crate::run(&book, Options { today: day(2025, 3, 31), relaxed: false });
        let errors: Vec<_> = run.diagnostics.iter().filter(|diagnostic| diagnostic.is_error()).collect();
        assert!(errors.is_empty(), "native purchase/improvement/sale: {errors:?}");

        let asset = book.asset("condo").unwrap();
        let state = &run.assets[asset.index()];
        assert_eq!(
            state.parts().len(),
            2,
            "the capital payment acquires one part and the later payment adds one improvement"
        );
        assert_eq!(state.parts()[0].kind, PartKind::Acquisition);
        assert_eq!(state.parts()[1].kind, PartKind::Improvement);
        assert_eq!((state.total_cost().unwrap().0, state.total_basis().unwrap().0), (110_000, 110_000));
        assert_eq!(
            state.disposed.map(|disposal| disposal.boundary),
            Some(DisposalBoundary::After(EventKey {
                day: day(2025, 3, 1),
                sequence: u64::from(book.flows.len() as u32 - 2),
            }))
        );

        let sale = run.gains.iter().find(|gain| gain.day == day(2025, 3, 1)).expect("sale records realized gain");
        assert_eq!((sale.proceeds.0, sale.basis.0, sale.gain().0), (144_000, 110_000, 34_000));
        assert!(
            !run.holdings.iter().any(|holding| holding.place == book.assets[asset].place),
            "the sold asset unit is relieved"
        );
        let checking = book.place("assets/checking").unwrap();
        let usd = book.commodity("USD").unwrap();
        let cash = run.holdings.iter().find(|holding| holding.place == checking && holding.unit == usd).unwrap();
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
        let options = Options { today: day(2025, 3, 31), relaxed: false };
        let plan = Plan::new(&book);
        let (initial, mut ledger) = plan.run_with_view(options);
        let asset = book.asset("condo").unwrap();
        let cabin = book.asset("cabin").unwrap();
        let condo_parts = initial.assets[asset.index()].parts();
        let cabin_part = initial.assets[cabin.index()].parts()[0].id;
        assert_eq!(condo_parts.len(), 2, "opening basis and improvement are separate clocks");
        assert_eq!(condo_parts[0].kind, PartKind::Acquisition);
        assert_eq!(condo_parts[1].kind, PartKind::Improvement);
        assert_eq!(initial.assets[asset.index()].total_cost().unwrap().0, 110_000);

        let improvement = condo_parts[1].id;
        let consumed = ledger.consume_asset_part(asset, improvement, axiom_core::Qty(2_000)).unwrap();
        assert_eq!((consumed.applied.0, consumed.excess.0), (2_000, 0));
        let carry = ledger.carry_asset_basis(cabin_part, Some((asset, improvement)), axiom_core::Qty(500)).unwrap();
        assert_eq!(carry.to, Some(improvement));
        ledger
            .dispose_asset(
                asset,
                cabin_part.origin,
                None,
                DisposalBoundary::After(EventKey { day: day(2025, 3, 1), sequence: 0 }),
            )
            .unwrap();
        let run = ledger.finish();

        let state = &run.assets[asset.index()];
        assert_eq!(state.total_cost().unwrap().0, 110_000);
        assert_eq!(state.total_basis().unwrap().0, 108_500);
        assert!(state.held_at(EventKey { day: day(2025, 3, 1), sequence: 0 }));
        assert!(!state.held_at(EventKey { day: day(2025, 3, 1), sequence: 1 }));
        let checking = book.place("assets/checking").unwrap();
        let usd = book.commodity("USD").unwrap();
        let cash = run.holdings.iter().find(|holding| holding.place == checking && holding.unit == usd).unwrap();
        assert_eq!(cash.qty().0, 4_900_00, "only the improvement is a cash flow in this fixture");
        let condo = run
            .holdings
            .iter()
            .find(|holding| holding.place == book.assets[asset].place && holding.unit == book.assets[asset].unit)
            .unwrap();
        assert_eq!(condo.qty().0, 1, "basis parts never duplicate the physical asset unit");
        assert_eq!(condo.lots.iter().map(|lot| lot.basis.0).sum::<i64>(), 108_500);
    }
}
