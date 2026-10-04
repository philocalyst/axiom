//! An asset's parts: its acquisition and each improvement, what each cost and what is left of its basis.
//!
//! The asset's unit is one parcel, keyed by its acquisition's [`PartId`], and that parcel holds the asset's whole basis: an
//! improvement has no quantity, so it cannot be a parcel of its own (K3c's map, section 4). The part table says how that
//! basis is made up, which a parcel cannot: what each part cost, what of it is left (a law consumes each part by its own
//! basis), and when each was recorded and goes into service. So the parts' basis adds up to the parcel's, and the fold keeps
//! it so by writing both in one step ([`Ledger::consume_asset_part`], [`Ledger::add_asset_part`],
//! [`Ledger::carry_basis_to_parts`]): the parcels are checked and written first, and a part is written only once they have
//! been. The table is part of the ledger's clonable world, an arena by asset with an index by part.

use axiom_core::{Day, Id, Qty, Span};
use axiom_model::{Asset, Book, Commodity, Entity, Flow, FlowCodes, Law, RuntimeTxn};

use crate::lots::CarryLotAddition;
use crate::{Cause, Ledger};

/// Stable identity of one asset part within its originating runtime flow.
///
/// A transaction can add more than one capital item. `ordinal` distinguishes
/// those items without borrowing a vector position that can change as state is
/// replayed or forked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PartId {
    pub origin: RuntimeTxn,
    pub ordinal: u32,
}

/// The position of a flow in the ledger's total event order.
///
/// The sequence is assigned by the ledger for ties on a day. Asset state does
/// not infer event order from its part vector.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EventKey {
    pub day: Day,
    pub sequence: u64,
}

/// Whether the unit remains owned at a specified ledger boundary.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DisposalBoundary {
    /// Sale or other disposal takes effect just after this flow has run.
    After(EventKey),
    /// Disposed after all events and closing laws on this day.
    Close(Day),
}

impl DisposalBoundary {
    fn includes(self, event: EventKey) -> bool {
        match self {
            DisposalBoundary::After(at) => event <= at,
            DisposalBoundary::Close(day) => event.day <= day,
        }
    }
}

/// The source's relationship to the original unit.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PartKind {
    /// The asset unit itself: purchase, opening amount, or gifted basis.
    Acquisition,
    /// A later capital addition to the same unit.
    Improvement,
}

/// Cost and remaining basis attributable to one acquisition or improvement.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Part {
    pub id: PartId,
    /// Original source flow when one exists. Runtime-only occurrences are
    /// identified by `id.origin` and need no synthetic Book flow id.
    pub flow: Option<Id<Flow>>,
    pub kind: PartKind,
    /// When the part entered the ledger's asset world.
    pub recorded: EventKey,
    /// The acquisition or in-service day. An opening `since` may predate the
    /// event that introduced its initial basis.
    pub day: Day,
    /// Original cost in base-currency quanta; not the current market value.
    pub cost: Qty,
    /// Cost less law consumption, plus any carried basis.
    pub basis: Qty,
}

/// The source and boundary at which an asset left the owner's state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Disposal {
    pub txn: RuntimeTxn,
    pub flow: Option<Id<Flow>>,
    pub boundary: DisposalBoundary,
}

/// A loss awaiting a replacement lot inside its statutory acquisition window.
/// This is matching metadata, not a second holdings balance; its remaining
/// quantity and amount are resolved against canonical `Holdings` parcels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PendingCarry {
    pub law: Id<Law>,
    pub from: PartId,
    pub cause: Cause,
    pub owner: Id<Entity>,
    pub unit: Id<Commodity>,
    pub sold: Day,
    /// The disposed slice's tacked holding-period start, separate from its
    /// actual acquisition day used to assess the replacement window.
    pub held_since: Day,
    pub within: Span,
    pub quantity: Qty,
    pub amount: Qty,
    pub codes: FlowCodes,
}

/// One asset's parts and explicit disposal state.
#[derive(Clone, Debug, Hash)]
pub struct AssetState {
    pub asset: Id<Asset>,
    pub parts: Vec<Part>,
    pub disposed: Option<Disposal>,
}

impl AssetState {
    fn new(asset: Id<Asset>) -> AssetState {
        AssetState { asset, parts: Vec::new(), disposed: None }
    }

    /// Parts in source/ledger order, borrowed without allocating.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Aggregate cost for reports that ask about the whole asset.
    pub fn total_cost(&self) -> Result<Qty, AssetError> {
        self.total(|part| part.cost)
    }

    /// Aggregate basis for the asset's holdings parcel.
    pub fn total_basis(&self) -> Result<Qty, AssetError> {
        self.total(|part| part.basis)
    }

    /// What `value` reads of every part, added up.
    pub(crate) fn total(&self, value: impl Fn(&Part) -> Qty) -> Result<Qty, AssetError> {
        self.parts
            .iter()
            .try_fold(Qty::ZERO, |sum, part| sum.0.checked_add(value(part).0).map(Qty).ok_or(AssetError::Overflow))
    }

    /// The service date for a part. The original unit uses its effective
    /// asset-level `in-service` property when present; each improvement starts
    /// service on its own acquisition day.
    pub fn in_service(&self, id: PartId, asset_property: Option<Day>) -> Result<Day, AssetError> {
        let part = self.parts.iter().find(|part| part.id == id).ok_or(AssetError::UnknownPart)?;
        Ok(match part.kind {
            PartKind::Acquisition => asset_property.unwrap_or(part.day),
            PartKind::Improvement => part.day,
        })
    }

    /// Declared asset properties apply to the original part by default. A
    /// compiler/runtime-supplied explicit attribution to an improvement takes
    /// precedence over that default.
    pub fn property_applies(&self, id: PartId, explicitly_attributed: bool) -> Result<bool, AssetError> {
        let part = self.parts.iter().find(|part| part.id == id).ok_or(AssetError::UnknownPart)?;
        Ok(explicitly_attributed || part.kind == PartKind::Acquisition)
    }

    /// Whether the asset still belongs to its owner at this exact event.
    pub fn held_at(&self, event: EventKey) -> bool {
        self.parts.first().is_some_and(|acquisition| acquisition.recorded <= event)
            && self.disposed.is_none_or(|disposal| disposal.boundary.includes(event))
    }
}

/// The canonical asset state table owned by a ledger `World`.
#[derive(Clone, Debug)]
pub struct Assets {
    states: Vec<AssetState>,
    /// Append-only part table keyed by stable identity. The value is an asset
    /// arena index and the part's position within that asset's append-only list.
    part_index: axiom_core::Map<PartId, (Id<Asset>, usize)>,
    /// Unmatched statutory carry requests, in source order. They affect future
    /// basis and therefore participate in checkpoint identity.
    pending_carries: Vec<PendingCarry>,
}

impl std::hash::Hash for Assets {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `part_index` is derived from the append-only state vectors. Hashing
        // those vectors in asset/ledger order is deterministic and includes
        // each part's cost, remaining basis, event boundary, and disposal.
        self.states.hash(state);
        self.pending_carries.hash(state);
    }
}

impl Assets {
    /// One empty state slot per model asset id.
    pub fn new(count: usize) -> Assets {
        Assets {
            states: (0..count).map(|index| AssetState::new(Id::new(index as u32))).collect(),
            part_index: axiom_core::Map::default(),
            pending_carries: Vec::new(),
        }
    }

    pub fn from_book(book: &Book<'_>) -> Assets {
        Assets::new(book.assets.len())
    }

    pub fn asset(&self, id: Id<Asset>) -> Option<&AssetState> {
        self.states.get(id.index()).filter(|state| state.asset == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AssetState> {
        self.states.iter()
    }

    pub(crate) fn into_run_parts(self) -> (Vec<AssetState>, Vec<PendingCarry>) {
        (self.states, self.pending_carries)
    }

    pub fn pending_carries(&self) -> &[PendingCarry] {
        &self.pending_carries
    }

    pub(crate) fn expire_carries_through(&mut self, day: Day) {
        self.pending_carries.retain(|carry| shift(carry.sold, carry.within).is_none_or(|expires| expires > day));
    }

    pub(crate) fn within_carry_window(left: Day, right: Day, within: Span) -> bool {
        shift(left, within).is_none_or(|last| right <= last) && shift(right, within).is_none_or(|last| left <= last)
    }

    pub(crate) fn enqueue_carry(&mut self, carry: PendingCarry) -> Result<(), AssetError> {
        if carry.quantity <= Qty::ZERO || carry.amount <= Qty::ZERO {
            return Err(AssetError::NegativeAmount);
        }
        if carry.within.months < 0 || carry.within.days < 0 {
            return Err(AssetError::NegativeSpan);
        }
        // The same loss of the same sale, carried by the same law, waits as one carry.
        let key = |c: &PendingCarry| (c.law, c.from, c.cause, c.owner, c.unit, c.sold, c.held_since, c.within);
        let Some(existing) = self.pending_carries.iter_mut().find(|existing| key(existing) == key(&carry)) else {
            self.pending_carries.push(carry);
            return Ok(());
        };
        // Both sums are checked before either is written: an overflow leaves no half-merged carry behind.
        let quantity = existing.quantity.0.checked_add(carry.quantity.0).map(Qty).ok_or(AssetError::Overflow)?;
        let amount = existing.amount.0.checked_add(carry.amount.0).map(Qty).ok_or(AssetError::Overflow)?;
        (existing.quantity, existing.amount) = (quantity, amount);
        Ok(())
    }

    /// Leaves the carry at `index` waiting for `quantity` and `amount`, or, when either is spent, takes it off the queue.
    pub(crate) fn update_pending_carry(&mut self, index: usize, quantity: Qty, amount: Qty) {
        match quantity.is_zero() || amount.is_zero() {
            true => drop(self.pending_carries.remove(index)),
            false => (self.pending_carries[index].quantity, self.pending_carries[index].amount) = (quantity, amount),
        }
    }

    /// Adds a part in event order. An asset's first part must be its original
    /// acquisition; all later parts are improvements. Basis/cost inputs must
    /// already be converted to base-currency quanta.
    pub fn add_part(&mut self, asset: Id<Asset>, part: Part) -> Result<(), AssetError> {
        self.validate_part(asset, &part)?;
        let state = &mut self.states[asset.index()];
        let index = state.parts.len();
        let id = part.id;
        state.parts.push(part);
        self.part_index.insert(id, (asset, index));
        Ok(())
    }

    /// Checks the stable ordering and uniqueness constraints without changing
    /// the table. A ledger can call this before it lands the matching parcel,
    /// then commit the part after that parcel is in place.
    pub fn validate_part(&self, asset: Id<Asset>, part: &Part) -> Result<(), AssetError> {
        if part.cost.is_negative() {
            return Err(AssetError::NegativeCost);
        }
        if part.basis.is_negative() {
            return Err(AssetError::NegativeBasis);
        }
        if self.part_index.contains_key(&part.id) {
            return Err(AssetError::DuplicatePart);
        }
        let state =
            self.states.get(asset.index()).filter(|state| state.asset == asset).ok_or(AssetError::UnknownAsset)?;
        if state.disposed.is_some() {
            return Err(AssetError::Disposed);
        }
        match (state.parts.first(), part.kind) {
            (None, PartKind::Acquisition) => {}
            (Some(_), PartKind::Improvement) => {}
            (None, PartKind::Improvement) => return Err(AssetError::MissingAcquisition),
            (Some(_), PartKind::Acquisition) => return Err(AssetError::DuplicateAcquisition),
        }
        if state.parts.last().is_some_and(|last| last.recorded > part.recorded) {
            return Err(AssetError::OutOfOrder);
        }
        Ok(())
    }

    /// Records an explicit end-of-ownership boundary. Callers choose whether
    /// the asset leaves after a flow or after the day's closings.
    pub fn dispose(
        &mut self,
        asset: Id<Asset>,
        txn: RuntimeTxn,
        flow: Option<Id<Flow>>,
        boundary: DisposalBoundary,
    ) -> Result<(), AssetError> {
        let state =
            self.states.get_mut(asset.index()).filter(|state| state.asset == asset).ok_or(AssetError::UnknownAsset)?;
        if state.parts.is_empty() {
            return Err(AssetError::MissingAcquisition);
        }
        if state.disposed.is_some() {
            return Err(AssetError::AlreadyDisposed);
        }
        if state.parts.iter().any(|part| !boundary.includes(part.recorded)) {
            return Err(AssetError::NotHeldAtBoundary);
        }
        state.disposed = Some(Disposal { txn, flow, boundary });
        Ok(())
    }

    /// Finds a part by its stable origin key, returning its asset and borrowed
    /// record. The key is globally unique by construction.
    pub fn part(&self, id: PartId) -> Option<(Id<Asset>, &Part)> {
        let &(asset, index) = self.part_index.get(&id)?;
        let part = self.states.get(asset.index())?.parts.get(index)?;
        (part.id == id).then_some((asset, part))
    }

    /// A part of `asset` that is still held, and the asset's acquisition, whose parcels hold its basis: what a law may
    /// consume of.
    fn held(&self, asset: Id<Asset>, id: PartId) -> Result<(PartId, Part), AssetError> {
        let state = self.asset(asset).ok_or(AssetError::UnknownAsset)?;
        if state.disposed.is_some() {
            return Err(AssetError::Disposed);
        }
        let part = self.part(id).filter(|&(owner, _)| owner == asset).ok_or(AssetError::UnknownPart)?.1;
        Ok((state.parts[0].id, *part))
    }

    fn part_mut(&mut self, id: PartId) -> &mut Part {
        let (asset, index) = self.part_index[&id];
        &mut self.states[asset.index()].parts[index]
    }

    /// Adds a loss carried into a part, if it is an asset's: what is carried into is what was bought, its acquisition.
    fn add_basis(&mut self, id: PartId, amount: Qty) -> Result<(), AssetError> {
        let Some(&(asset, index)) = self.part_index.get(&id) else { return Ok(()) };
        match (index, self.states[asset.index()].disposed) {
            (0, None) => {}
            (0, Some(_)) => return Err(AssetError::Disposed),
            _ => return Err(AssetError::UnknownPart),
        }
        let part = self.part_mut(id);
        part.basis = part.basis.0.checked_add(amount.0).map(Qty).ok_or(AssetError::Overflow)?;
        Ok(())
    }
}

impl Ledger<'_, '_, '_> {
    /// Adds a part to an asset's table: its acquisition, whose parcel has just landed with its basis, or an improvement,
    /// whose basis the acquisition's parcels take on.
    pub(crate) fn add_asset_part(&mut self, asset: Id<Asset>, part: Part) -> Result<(), AssetError> {
        self.world.assets.validate_part(asset, &part)?;
        if part.kind == PartKind::Improvement {
            let anchor = self.world.assets.states[asset.index()].parts[0].id;
            self.world.holdings.adjust(self.plan.book.assets[asset].unit, anchor, part.basis)?;
        }
        self.world.assets.add_part(asset, part)
    }

    /// Consumes up to `requested` of one part's basis, in the part and in the asset's parcels: the part's own basis caps
    /// it. Says how much it consumed; what it could not is the caller's to say.
    pub(crate) fn consume_asset_part(
        &mut self,
        asset: Id<Asset>,
        part: PartId,
        requested: Qty,
    ) -> Result<Qty, AssetError> {
        if requested.is_negative() {
            return Err(AssetError::NegativeAmount);
        }
        let (anchor, held) = self.world.assets.held(asset, part)?;
        let applied = requested.min(held.basis);
        self.world.holdings.adjust(self.plan.book.assets[asset].unit, anchor, -applied)?;
        self.world.assets.part_mut(part).basis -= applied;
        Ok(applied)
    }

    /// Carries a sale's loss into the shares of `unit` that replaced the sold ones ([`Holdings::carry`]), and into the part
    /// of an asset's acquisition among them.
    ///
    /// [`Holdings::carry`]: crate::lots::Holdings::carry
    pub(crate) fn carry_basis_to_parts(
        &mut self,
        unit: Id<Commodity>,
        additions: &[CarryLotAddition],
    ) -> Result<(), AssetError> {
        self.world.holdings.carry(unit, additions)?;
        additions.iter().try_for_each(|addition| self.world.assets.add_basis(addition.part, addition.amount))
    }
}

/// Calendar addition with a clamped month day and checked arithmetic. An
/// unrepresentable edge is treated by the caller as extending beyond the
/// civil-date domain, so valid candidate dates remain inside the window.
fn shift(day: Day, span: Span) -> Option<Day> {
    let (year, month, of_month) = day.ymd();
    let months = (year as i64).checked_mul(12)?.checked_add(month as i64 - 1)?.checked_add(span.months as i64)?;
    let target_year = i32::try_from(months.div_euclid(12)).ok()?;
    let target_month = u32::try_from(months.rem_euclid(12) + 1).ok()?;
    let target_day =
        (1..=of_month).rev().find(|&candidate| Day::from_ymd(target_year, target_month, candidate).is_some())?;
    let month_day = Day::from_ymd(target_year, target_month, target_day)?;
    Some(Day(month_day.0.checked_add(span.days)?))
}

/// Typed failures that the ledger can turn into law/source diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetError {
    UnknownAsset,
    UnknownPart,
    MissingAcquisition,
    DuplicatePart,
    DuplicateAcquisition,
    NegativeCost,
    NegativeBasis,
    NegativeAmount,
    NegativeSpan,
    OutOfOrder,
    Disposed,
    AlreadyDisposed,
    NotHeldAtBoundary,
    ParcelBasisMismatch,
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::Span;

    fn part(origin: u32, ordinal: u32, kind: PartKind, day: i32, sequence: u64, cost: i64, basis: i64) -> Part {
        Part {
            id: PartId { origin: RuntimeTxn::Adjustment { place: Id::new(origin), day: Day(day) }, ordinal },
            flow: None,
            kind,
            recorded: EventKey { day: Day(day), sequence },
            day: Day(day),
            cost: Qty(cost),
            basis: Qty(basis),
        }
    }

    #[test]
    fn parts_keep_stable_identity_cost_basis_and_original_service_date() {
        let asset = Id::new(0);
        let mut assets = Assets::new(1);
        let original = part(1, 0, PartKind::Acquisition, 10, 1, 40_000, 38_000);
        let original_id = original.id;
        assets.add_part(asset, original).unwrap();
        assets.add_part(asset, part(2, 1, PartKind::Improvement, 20, 2, 5_000, 5_000)).unwrap();
        let state = assets.asset(asset).unwrap();
        assert_eq!(state.total_cost(), Ok(Qty(45_000)));
        assert_eq!(state.total_basis(), Ok(Qty(43_000)));
        assert_eq!(assets.part(original_id).map(|(_, part)| (part.cost, part.basis)), Some((Qty(40_000), Qty(38_000))));
        assert_eq!(state.part_count(), 2);
        assert_eq!(state.in_service(original_id, Some(Day(5))), Ok(Day(5)));
        assert_eq!(state.in_service(state.parts()[1].id, Some(Day(5))), Ok(Day(20)));
        assert!(state.property_applies(original_id, false).unwrap());
        assert!(!state.property_applies(state.parts()[1].id, false).unwrap());
        assert!(state.property_applies(state.parts()[1].id, true).unwrap());
    }

    fn basis(assets: &Assets, id: PartId) -> Option<Qty> {
        assets.part(id).map(|(_, part)| part.basis)
    }

    #[test]
    fn carry_updates_only_the_acquisition_carried_into() {
        let mut assets = Assets::new(2);
        let source = part(1, 0, PartKind::Acquisition, 10, 1, 10_000, 10_000);
        let source_id = source.id;
        assets.add_part(Id::new(0), source).unwrap();
        let target = part(2, 0, PartKind::Acquisition, 20, 2, 11_000, 11_000);
        let target_id = target.id;
        assets.add_part(Id::new(1), target).unwrap();
        let improvement = part(3, 1, PartKind::Improvement, 21, 3, 500, 500);
        assets.add_part(Id::new(1), improvement).unwrap();
        assert_eq!(assets.add_basis(target_id, Qty(2_500)), Ok(()));
        assert_eq!((basis(&assets, target_id), basis(&assets, source_id)), (Some(Qty(13_500)), Some(Qty(10_000))));
        let security = PartId { origin: RuntimeTxn::Adjustment { place: Id::new(9), day: Day(1) }, ordinal: 0 };
        assert_eq!(assets.add_basis(security, Qty(1_000)), Ok(()), "a security's shares have no part table");
        assert_eq!(assets.add_basis(improvement.id, Qty(1)), Err(AssetError::UnknownPart), "only what was bought");
        assert_eq!(assets.asset(Id::new(1)).unwrap().total_basis(), Ok(Qty(14_000)));
    }

    /// The window of a wash sale is a calendar span either side of the sale: a month from January 31 is February 29.
    #[test]
    fn acquisition_window_uses_calendar_shifts_without_span_ordering_shortcuts() {
        let sale = Day::parse(b"2024-03-31").unwrap();
        let acquired = Day::parse(b"2024-02-29").unwrap();
        assert!(Assets::within_carry_window(acquired, sale, Span::days(31)));
        assert!(!Assets::within_carry_window(acquired, sale, Span::days(30)));
        let (january, february) = (Day::parse(b"2024-01-31").unwrap(), Day::parse(b"2024-02-29").unwrap());
        assert_eq!(shift(january, Span::months(1)), Some(february));
        assert!(Assets::within_carry_window(january, february, Span::months(1)));
    }

    #[test]
    fn invalid_parts_and_overflow_leave_state_unchanged() {
        let asset = Id::new(0);
        let mut assets = Assets::new(1);
        let improvement = part(1, 0, PartKind::Improvement, 10, 1, 100, 100);
        assert_eq!(assets.add_part(asset, improvement), Err(AssetError::MissingAcquisition));

        let original = part(2, 0, PartKind::Acquisition, 10, 1, i64::MAX, i64::MAX);
        let original_id = original.id;
        assets.add_part(asset, original).unwrap();
        assert_eq!(assets.add_part(asset, original), Err(AssetError::DuplicatePart));
        let cost_and_basis = |assets: &Assets| assets.part(original_id).map(|(_, part)| (part.cost, part.basis));
        assert_eq!(cost_and_basis(&assets), Some((Qty(i64::MAX), Qty(i64::MAX))));

        let improvement = part(3, 1, PartKind::Improvement, 12, 2, 1, 1);
        assets.add_part(asset, improvement).unwrap();
        assert_eq!(assets.asset(asset).unwrap().total_cost(), Err(AssetError::Overflow));
        assert_eq!(assets.add_basis(original_id, Qty(1)), Err(AssetError::Overflow));
        assert_eq!(cost_and_basis(&assets), Some((Qty(i64::MAX), Qty(i64::MAX))));
    }

    #[test]
    fn disposal_boundary_is_explicit_and_blocks_later_improvements() {
        let mut assets = Assets::new(1);
        let original = part(1, 0, PartKind::Acquisition, 10, 1, 100, 100);
        let original_id = original.id;
        assets.add_part(Id::new(0), original).unwrap();
        assert!(!assets.asset(Id::new(0)).unwrap().held_at(EventKey { day: Day(9), sequence: u64::MAX }));
        assets
            .dispose(
                Id::new(0),
                original_id.origin,
                None,
                DisposalBoundary::After(EventKey { day: Day(10), sequence: 1 }),
            )
            .unwrap();
        assert!(assets.asset(Id::new(0)).unwrap().held_at(EventKey { day: Day(10), sequence: 1 }));
        assert!(!assets.asset(Id::new(0)).unwrap().held_at(EventKey { day: Day(10), sequence: 2 }));
        assert_eq!(
            assets.add_part(Id::new(0), part(2, 0, PartKind::Improvement, 11, 2, 10, 10)),
            Err(AssetError::Disposed)
        );
        assert_eq!(
            assets.dispose(Id::new(0), original_id.origin, None, DisposalBoundary::Close(Day(12))),
            Err(AssetError::AlreadyDisposed)
        );
    }

    #[test]
    fn disposal_cannot_precede_an_already_recorded_improvement() {
        let asset = Id::new(0);
        let mut assets = Assets::new(1);
        let acquisition = part(1, 0, PartKind::Acquisition, 10, 1, 100, 100);
        assets.add_part(asset, acquisition).unwrap();
        let improvement = part(2, 0, PartKind::Improvement, 11, 2, 25, 25);
        assets.add_part(asset, improvement).unwrap();

        let before_improvement = DisposalBoundary::After(EventKey { day: Day(10), sequence: 1 });
        assert_eq!(
            assets.dispose(asset, acquisition.id.origin, None, before_improvement),
            Err(AssetError::NotHeldAtBoundary),
            "a sale boundary must include every part already present in the ledger"
        );
        assert!(assets.asset(asset).unwrap().held_at(EventKey { day: Day(11), sequence: 2 }));
        assets.dispose(asset, acquisition.id.origin, None, DisposalBoundary::Close(Day(11))).unwrap();
        assert!(assets.asset(asset).unwrap().held_at(EventKey { day: Day(11), sequence: 2 }));
        assert!(!assets.asset(asset).unwrap().held_at(EventKey { day: Day(12), sequence: 0 }));
    }
}
