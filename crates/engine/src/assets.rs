//! Canonical, part-aware state for assets.
//!
//! The asset table is part of the ledger's clonable world. It is not a second
//! holdings ledger: each part is keyed by the same [`RuntimeTxn`] identity as
//! the parcel that represents the asset unit, and the owning ledger updates
//! both atomically. The parcel remains the aggregate the holdings need; this
//! table preserves the composition of its cost basis for part-aware laws.

use axiom_core::{Day, Id, Qty, Span};
use axiom_model::{Asset, Book, Commodity, Entity, Flow, FlowCodes, Law, RuntimeTxn};

use crate::Cause;

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
        AssetState {
            asset,
            parts: Vec::new(),
            disposed: None,
        }
    }

    /// Parts in source/ledger order, borrowed without allocating.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Original cost for one explicitly selected law subject part.
    pub fn cost(&self, part: PartId) -> Result<Qty, AssetError> {
        self.measure(part, |part| part.cost)
    }

    /// Remaining basis for one explicitly selected law subject part.
    pub fn basis(&self, part: PartId) -> Result<Qty, AssetError> {
        self.measure(part, |part| part.basis)
    }

    /// Aggregate cost for reports that ask about the whole asset.
    pub fn total_cost(&self) -> Result<Qty, AssetError> {
        self.total(|part| part.cost)
    }

    /// Aggregate basis for the asset's holdings parcel.
    pub fn total_basis(&self) -> Result<Qty, AssetError> {
        self.total(|part| part.basis)
    }

    fn measure(&self, id: PartId, value: impl Fn(&Part) -> Qty) -> Result<Qty, AssetError> {
        self.parts
            .iter()
            .find(|part| part.id == id)
            .map(value)
            .ok_or(AssetError::UnknownPart)
    }

    fn total(&self, value: impl Fn(&Part) -> Qty) -> Result<Qty, AssetError> {
        self.parts.iter().try_fold(Qty::ZERO, |sum, part| {
            sum.0
                .checked_add(value(part).0)
                .map(Qty)
                .ok_or(AssetError::Overflow)
        })
    }

    /// The service date for a part. The original unit uses its effective
    /// asset-level `in-service` property when present; each improvement starts
    /// service on its own acquisition day.
    pub fn in_service(&self, id: PartId, asset_property: Option<Day>) -> Result<Day, AssetError> {
        let part = self
            .parts
            .iter()
            .find(|part| part.id == id)
            .ok_or(AssetError::UnknownPart)?;
        Ok(match part.kind {
            PartKind::Acquisition => asset_property.unwrap_or(part.day),
            PartKind::Improvement => part.day,
        })
    }

    /// Declared asset properties apply to the original part by default. A
    /// compiler/runtime-supplied explicit attribution to an improvement takes
    /// precedence over that default.
    pub fn property_applies(
        &self,
        id: PartId,
        explicitly_attributed: bool,
    ) -> Result<bool, AssetError> {
        let part = self
            .parts
            .iter()
            .find(|part| part.id == id)
            .ok_or(AssetError::UnknownPart)?;
        Ok(explicitly_attributed || part.kind == PartKind::Acquisition)
    }

    /// Whether the asset still belongs to its owner at this exact event.
    pub fn held_at(&self, event: EventKey) -> bool {
        self.parts
            .first()
            .is_some_and(|acquisition| acquisition.recorded <= event)
            && self
                .disposed
                .is_none_or(|disposal| disposal.boundary.includes(event))
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
            states: (0..count)
                .map(|index| AssetState::new(Id::new(index as u32)))
                .collect(),
            part_index: axiom_core::Map::default(),
            pending_carries: Vec::new(),
        }
    }

    pub fn from_book(book: &Book<'_>) -> Assets {
        Assets::new(book.assets.len())
    }

    pub fn asset(&self, id: Id<Asset>) -> Option<&AssetState> {
        self.states
            .get(id.index())
            .filter(|state| state.asset == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AssetState> {
        self.states.iter()
    }

    pub(crate) fn into_states(self) -> Vec<AssetState> {
        self.states
    }

    pub(crate) fn into_run_parts(self) -> (Vec<AssetState>, Vec<PendingCarry>) {
        (self.states, self.pending_carries)
    }

    pub fn pending_carries(&self) -> &[PendingCarry] {
        &self.pending_carries
    }

    pub(crate) fn expire_carries_through(&mut self, day: Day) {
        self.pending_carries.retain(|carry| {
            shift(carry.sold, carry.within).is_none_or(|expires| expires > day)
        });
    }

    pub(crate) fn within_carry_window(left: Day, right: Day, within: Span) -> bool {
        shift(left, within).is_none_or(|last| right <= last)
            && shift(right, within).is_none_or(|last| left <= last)
    }

    pub(crate) fn enqueue_carry(&mut self, carry: PendingCarry) -> Result<(), AssetError> {
        if carry.quantity <= Qty::ZERO || carry.amount <= Qty::ZERO {
            return Err(AssetError::NegativeAmount);
        }
        if carry.within.months < 0 || carry.within.days < 0 {
            return Err(AssetError::NegativeSpan);
        }
        if let Some(existing) = self.pending_carries.iter_mut().find(|existing| {
            existing.law == carry.law
                && existing.from == carry.from
                && existing.cause == carry.cause
                && existing.owner == carry.owner
                && existing.unit == carry.unit
                && existing.sold == carry.sold
                && existing.held_since == carry.held_since
                && existing.within == carry.within
        }) {
            // Preflight both arithmetic operations before changing either
            // field. An overflow must not leave a half-merged request behind.
            let quantity = existing
                .quantity
                .0
                .checked_add(carry.quantity.0)
                .map(Qty)
                .ok_or(AssetError::Overflow)?;
            let amount = existing
                .amount
                .0
                .checked_add(carry.amount.0)
                .map(Qty)
                .ok_or(AssetError::Overflow)?;
            existing.quantity = quantity;
            existing.amount = amount;
        } else {
            self.pending_carries.push(carry);
        }
        Ok(())
    }

    pub(crate) fn pending_carry(&self, index: usize) -> Option<PendingCarry> {
        self.pending_carries.get(index).copied()
    }

    pub(crate) fn update_pending_carry(
        &mut self,
        index: usize,
        quantity: Qty,
        amount: Qty,
    ) -> Result<(), AssetError> {
        if index >= self.pending_carries.len() {
            return Err(AssetError::UnknownPart);
        }
        if quantity.is_negative() || amount.is_negative() {
            return Err(AssetError::NegativeAmount);
        }
        if quantity.is_zero() || amount.is_zero() {
            self.pending_carries.remove(index);
        } else {
            let request = &mut self.pending_carries[index];
            request.quantity = quantity;
            request.amount = amount;
        }
        Ok(())
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
        let state = self
            .states
            .get(asset.index())
            .filter(|state| state.asset == asset)
            .ok_or(AssetError::UnknownAsset)?;
        if state.disposed.is_some() {
            return Err(AssetError::Disposed);
        }
        match (state.parts.first(), part.kind) {
            (None, PartKind::Acquisition) => {}
            (Some(_), PartKind::Improvement) => {}
            (None, PartKind::Improvement) => return Err(AssetError::MissingAcquisition),
            (Some(_), PartKind::Acquisition) => return Err(AssetError::DuplicateAcquisition),
        }
        if state
            .parts
            .last()
            .is_some_and(|last| last.recorded > part.recorded)
        {
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
        let state = self
            .states
            .get_mut(asset.index())
            .filter(|state| state.asset == asset)
            .ok_or(AssetError::UnknownAsset)?;
        if state.parts.is_empty() {
            return Err(AssetError::MissingAcquisition);
        }
        if state.disposed.is_some() {
            return Err(AssetError::AlreadyDisposed);
        }
        if state
            .parts
            .iter()
            .any(|part| !boundary.includes(part.recorded))
        {
            return Err(AssetError::NotHeldAtBoundary);
        }
        state.disposed = Some(Disposal {
            txn,
            flow,
            boundary,
        });
        Ok(())
    }

    /// Finds a part by its stable origin key, returning its asset and borrowed
    /// record. The key is globally unique by construction.
    pub fn part(&self, id: PartId) -> Option<(Id<Asset>, &Part)> {
        let &(asset, index) = self.part_index.get(&id)?;
        let part = self.states.get(asset.index())?.parts.get(index)?;
        (part.id == id).then_some((asset, part))
    }

    /// Lowers one part's basis. Over-consumption is represented as `excess`
    /// for the caller's typed diagnostic; the stored basis never goes below
    /// zero and no other part changes.
    pub fn consume(
        &mut self,
        asset: Id<Asset>,
        part_id: PartId,
        requested: Qty,
    ) -> Result<Consumption, AssetError> {
        Ok(self.prepare_consumption(asset, part_id, requested)?.apply())
    }

    /// Validates a consumption without changing canonical asset state. Ledger
    /// callers pair this with a prepared parcel adjustment before committing
    /// either store.
    pub(crate) fn prepare_consumption(
        &mut self,
        asset: Id<Asset>,
        part_id: PartId,
        requested: Qty,
    ) -> Result<ConsumptionGuard<'_>, AssetError> {
        if requested.is_negative() {
            return Err(AssetError::NegativeAmount);
        }
        let state = self
            .states
            .get_mut(asset.index())
            .filter(|state| state.asset == asset)
            .ok_or(AssetError::UnknownAsset)?;
        if state.disposed.is_some() {
            return Err(AssetError::Disposed);
        }
        let (owner, index) = self.part_index.get(&part_id).copied().ok_or(AssetError::UnknownPart)?;
        if owner != asset {
            return Err(AssetError::UnknownPart);
        }
        let part = state.parts.get_mut(index).ok_or(AssetError::UnknownPart)?;
        let before = part.basis;
        let applied = Qty(requested.0.min(part.basis.0));
        let basis = Qty(part
            .basis
            .0
            .checked_sub(applied.0)
            .ok_or(AssetError::Overflow)?);
        Ok(ConsumptionGuard {
            part,
            before,
            result: Consumption {
                asset,
                part: part_id,
                requested,
                applied,
                excess: Qty(requested.0 - applied.0),
            },
            basis,
        })
    }

    /// Adds a carried loss to a selected acquisition part. `None` records an
    /// unreceived carry without changing basis. The engine owns the pending
    /// carry queue and decides when the search window has closed.
    pub fn carry(
        &mut self,
        from: PartId,
        to: Option<(Id<Asset>, PartId)>,
        amount: Qty,
    ) -> Result<CarryUpdate, AssetError> {
        Ok(self.prepare_carry(from, to, amount)?.apply())
    }

    /// Validates both sides of a basis carry without mutation.
    pub(crate) fn prepare_carry(
        &mut self,
        from: PartId,
        to: Option<(Id<Asset>, PartId)>,
        amount: Qty,
    ) -> Result<CarryGuard<'_>, AssetError> {
        if amount.is_negative() {
            return Err(AssetError::NegativeAmount);
        }
        if self.part(from).is_none() {
            return Err(AssetError::UnknownPart);
        }
        let target = if let Some((asset, part_id)) = to {
            let state = self
                .states
                .get_mut(asset.index())
                .filter(|state| state.asset == asset)
                .ok_or(AssetError::UnknownAsset)?;
            if state.disposed.is_some() {
                return Err(AssetError::Disposed);
            }
            let (owner, index) = self.part_index.get(&part_id).copied().ok_or(AssetError::UnknownPart)?;
            if owner != asset {
                return Err(AssetError::UnknownPart);
            }
            let part = state.parts.get_mut(index).ok_or(AssetError::UnknownPart)?;
            let before = part.basis;
            let basis = Qty(part
                .basis
                .0
                .checked_add(amount.0)
                .ok_or(AssetError::Overflow)?);
            Some((part, before, basis))
        } else {
            None
        };
        Ok(CarryGuard {
            result: CarryUpdate {
                from,
                to: to.map(|(_, part)| part),
                amount,
            },
            target,
        })
    }

    /// Prepares a positive basis addition to one declared asset part. The
    /// corresponding physical parcel is adjusted by `assets_runtime` under a
    /// second disjoint mutable guard before either store is committed.
    pub(crate) fn prepare_basis_additions(
        &mut self,
        additions: &[(Id<Asset>, PartId, Qty)],
    ) -> Result<AssetBasisBatchGuard<'_>, AssetError> {
        let mut changes = Vec::with_capacity(additions.len());
        for &(asset, part_id, amount) in additions {
            if amount.is_negative() {
                return Err(AssetError::NegativeAmount);
            }
            if amount.is_zero() {
                continue;
            }
            if changes.iter().any(|change: &AssetBasisChange| change.part_id == part_id) {
                return Err(AssetError::DuplicatePart);
            }
            let state = self
                .states
                .get(asset.index())
                .filter(|state| state.asset == asset)
                .ok_or(AssetError::UnknownAsset)?;
            if state.disposed.is_some() {
                return Err(AssetError::Disposed);
            }
            let (owner, index) = self.part_index.get(&part_id).copied().ok_or(AssetError::UnknownPart)?;
            if owner != asset {
                return Err(AssetError::UnknownPart);
            }
            let part = state.parts.get(index).ok_or(AssetError::UnknownPart)?;
            let basis = Qty(part.basis.0.checked_add(amount.0).ok_or(AssetError::Overflow)?);
            changes.push(AssetBasisChange { asset: asset.index(), part_id, part: index, basis });
        }
        Ok(AssetBasisBatchGuard { assets: self, changes })
    }

    /// The nearest acquisition of `unit` owned by `owner` within `within`,
    /// considering only parts the canonical holdings still report live. Day
    /// distance is measured in civil days; equal distances prefer the earlier
    /// acquisition, then stable asset/part order.
    pub fn nearest_acquisition(
        &self,
        book: &Book<'_>,
        owner: Id<Entity>,
        unit: Id<Commodity>,
        sale_day: Day,
        within: Span,
        exclude: Option<PartId>,
        mut is_live: impl FnMut(Id<Asset>, PartId) -> bool,
    ) -> Result<Option<(Id<Asset>, PartId)>, AssetError> {
        if within.months < 0 || within.days < 0 {
            return Err(AssetError::NegativeSpan);
        }
        let candidates = book.assets.iter().filter_map(|(asset_id, asset)| {
            self.asset(asset_id)
                .map(|state| (asset_id, asset.owner, asset.unit, state))
        });
        nearest_from(
            candidates,
            owner,
            unit,
            sale_day,
            within,
            exclude,
            &mut is_live,
        )
    }
}

fn nearest_from<'a>(
    candidates: impl IntoIterator<Item = (Id<Asset>, Id<Entity>, Id<Commodity>, &'a AssetState)>,
    owner: Id<Entity>,
    unit: Id<Commodity>,
    sale_day: Day,
    within: Span,
    exclude: Option<PartId>,
    is_live: &mut impl FnMut(Id<Asset>, PartId) -> bool,
) -> Result<Option<(Id<Asset>, PartId)>, AssetError> {
    if within.months < 0 || within.days < 0 {
        return Err(AssetError::NegativeSpan);
    }
    let mut best: Option<(u64, Day, Id<Asset>, PartId)> = None;
    for (asset_id, candidate_owner, candidate_unit, state) in candidates {
        if candidate_owner != owner || candidate_unit != unit {
            continue;
        }
        for part in &state.parts {
            if part.kind != PartKind::Acquisition
                || Some(part.id) == exclude
                || !is_live(asset_id, part.id)
            {
                continue;
            }
            let in_window = if part.day <= sale_day {
                shift(part.day, within).is_none_or(|end| end >= sale_day)
            } else {
                shift(sale_day, within).is_none_or(|end| end >= part.day)
            };
            if !in_window {
                continue;
            }
            let days = (part.day.0 as i64 - sale_day.0 as i64).unsigned_abs();
            let candidate = (days, part.day, asset_id, part.id);
            if best.is_none_or(|current| {
                candidate.0 < current.0
                    || (candidate.0 == current.0
                        && (candidate.1, candidate.2.index()) < (current.1, current.2.index()))
            }) {
                best = Some(candidate);
            }
        }
    }
    Ok(best.map(|(_, _, asset, part)| (asset, part)))
}

/// Calendar addition with a clamped month day and checked arithmetic. An
/// unrepresentable edge is treated by the caller as extending beyond the
/// civil-date domain, so valid candidate dates remain inside the window.
fn shift(day: Day, span: Span) -> Option<Day> {
    let (year, month, of_month) = day.ymd();
    let months = (year as i64)
        .checked_mul(12)?
        .checked_add(month as i64 - 1)?
        .checked_add(span.months as i64)?;
    let target_year = i32::try_from(months.div_euclid(12)).ok()?;
    let target_month = u32::try_from(months.rem_euclid(12) + 1).ok()?;
    let target_day = (1..=of_month)
        .rev()
        .find(|&candidate| Day::from_ymd(target_year, target_month, candidate).is_some())?;
    let month_day = Day::from_ymd(target_year, target_month, target_day)?;
    Some(Day(month_day.0.checked_add(span.days)?))
}

/// The result of a `consume`, including the amount that could not be applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumption {
    pub asset: Id<Asset>,
    pub part: PartId,
    pub requested: Qty,
    pub applied: Qty,
    pub excess: Qty,
}

pub(crate) struct ConsumptionGuard<'a> {
    part: &'a mut Part,
    before: Qty,
    result: Consumption,
    basis: Qty,
}

impl ConsumptionGuard<'_> {
    pub fn before(&self) -> Qty {
        self.before
    }

    pub fn result(&self) -> Consumption {
        self.result
    }

    /// Commits the prepared change while the exclusive part borrow is held.
    pub fn apply(self) -> Consumption {
        self.part.basis = self.basis;
        self.result
    }
}

/// The recorded relationship between a sale part and a receiving part.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CarryUpdate {
    pub from: PartId,
    pub to: Option<PartId>,
    pub amount: Qty,
}

pub(crate) struct CarryGuard<'a> {
    result: CarryUpdate,
    target: Option<(&'a mut Part, Qty, Qty)>,
}

struct AssetBasisChange {
    asset: usize,
    part_id: PartId,
    part: usize,
    basis: Qty,
}

pub(crate) struct AssetBasisBatchGuard<'a> {
    assets: &'a mut Assets,
    changes: Vec<AssetBasisChange>,
}

impl AssetBasisBatchGuard<'_> {
    pub fn apply(self) {
        for change in self.changes {
            self.assets.states[change.asset].parts[change.part].basis = change.basis;
        }
    }
}

impl CarryGuard<'_> {
    pub fn before(&self) -> Option<Qty> {
        self.target.as_ref().map(|(_, before, _)| *before)
    }

    /// Commits the prepared change while the exclusive part borrow is held.
    pub fn apply(self) -> CarryUpdate {
        if let Some((part, _, basis)) = self.target {
            part.basis = basis;
        }
        self.result
    }
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
    use axiom_model::{Commodity, Entity};

    fn part(
        origin: u32,
        ordinal: u32,
        kind: PartKind,
        day: i32,
        sequence: u64,
        cost: i64,
        basis: i64,
    ) -> Part {
        Part {
            id: PartId {
                origin: RuntimeTxn::Adjustment {
                    place: Id::new(origin),
                    day: Day(day),
                },
                ordinal,
            },
            flow: None,
            kind,
            recorded: EventKey {
                day: Day(day),
                sequence,
            },
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
        assets
            .add_part(
                asset,
                part(2, 1, PartKind::Improvement, 20, 2, 5_000, 5_000),
            )
            .unwrap();
        let state = assets.asset(asset).unwrap();
        assert_eq!(state.total_cost(), Ok(Qty(45_000)));
        assert_eq!(state.total_basis(), Ok(Qty(43_000)));
        assert_eq!(state.cost(original_id), Ok(Qty(40_000)));
        assert_eq!(state.basis(original_id), Ok(Qty(38_000)));
        assert_eq!(state.part_count(), 2);
        assert_eq!(state.in_service(original_id, Some(Day(5))), Ok(Day(5)));
        assert_eq!(
            state.in_service(state.parts()[1].id, Some(Day(5))),
            Ok(Day(20))
        );
        assert!(state.property_applies(original_id, false).unwrap());
        assert!(!state.property_applies(state.parts()[1].id, false).unwrap());
        assert!(state.property_applies(state.parts()[1].id, true).unwrap());
    }

    #[test]
    fn consumption_is_part_specific_and_reports_excess_without_negative_basis() {
        let mut assets = Assets::new(1);
        let original = part(1, 0, PartKind::Acquisition, 10, 1, 40_000, 3_000);
        let original_id = original.id;
        assets.add_part(Id::new(0), original).unwrap();
        assets
            .add_part(
                Id::new(0),
                part(2, 1, PartKind::Improvement, 20, 2, 5_000, 5_000),
            )
            .unwrap();
        let result = assets.consume(Id::new(0), original_id, Qty(4_000)).unwrap();
        assert_eq!((result.applied, result.excess), (Qty(3_000), Qty(1_000)));
        assert_eq!(
            assets.asset(Id::new(0)).unwrap().basis(original_id),
            Ok(Qty::ZERO)
        );
        assert_eq!(
            assets.asset(Id::new(0)).unwrap().total_basis(),
            Ok(Qty(5_000))
        );
    }

    #[test]
    fn carry_updates_only_the_selected_part_and_reports_unreceived_loss() {
        let mut assets = Assets::new(2);
        let source = part(1, 0, PartKind::Acquisition, 10, 1, 10_000, 10_000);
        let source_id = source.id;
        assets.add_part(Id::new(0), source).unwrap();
        let target = part(2, 0, PartKind::Acquisition, 20, 2, 11_000, 11_000);
        let target_id = target.id;
        assets.add_part(Id::new(1), target).unwrap();
        assert_eq!(
            assets.carry(source_id, Some((Id::new(1), target_id)), Qty(2_500)),
            Ok(CarryUpdate {
                from: source_id,
                to: Some(target_id),
                amount: Qty(2_500)
            })
        );
        assert_eq!(
            assets.asset(Id::new(1)).unwrap().basis(target_id),
            Ok(Qty(13_500))
        );
        assert_eq!(assets.carry(source_id, None, Qty(1_000)).unwrap().to, None);
        assert_eq!(
            assets.asset(Id::new(0)).unwrap().basis(source_id),
            Ok(Qty(10_000))
        );
    }

    #[test]
    fn acquisition_search_uses_owner_unit_window_and_deterministic_nearest_tie() {
        let (owner, other_owner) = (Id::<Entity>::new(0), Id::new(1));
        let (unit, other_unit) = (Id::<Commodity>::new(0), Id::new(1));
        let mut before_state = AssetState::new(Id::new(0));
        let mut after_state = AssetState::new(Id::new(1));
        let mut wrong_owner_state = AssetState::new(Id::new(2));
        let mut wrong_unit_state = AssetState::new(Id::new(3));
        let before = part(1, 0, PartKind::Acquisition, 90, 1, 100, 100);
        let before_id = before.id;
        before_state.parts.push(before);
        let after = part(2, 0, PartKind::Acquisition, 110, 2, 100, 100);
        let after_id = after.id;
        after_state.parts.push(after);
        wrong_owner_state
            .parts
            .push(part(3, 0, PartKind::Acquisition, 101, 3, 100, 100));
        wrong_unit_state
            .parts
            .push(part(4, 0, PartKind::Acquisition, 99, 4, 100, 100));
        let candidates = [
            (Id::new(0), owner, unit, &before_state),
            (Id::new(1), owner, unit, &after_state),
            (Id::new(2), other_owner, unit, &wrong_owner_state),
            (Id::new(3), owner, other_unit, &wrong_unit_state),
        ];
        let nearest = nearest_from(
            candidates,
            owner,
            unit,
            Day(100),
            Span::days(20),
            None,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(
            nearest,
            Some((Id::new(0), before_id)),
            "equal distances prefer the earlier acquisition after unit/owner filters"
        );
        let tied = nearest_from(
            candidates,
            owner,
            unit,
            Day(100),
            Span::days(10),
            Some(before_id),
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(tied, Some((Id::new(1), after_id)));
        let none = nearest_from(
            candidates,
            owner,
            unit,
            Day(100),
            Span::days(5),
            None,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn acquisition_window_uses_calendar_shifts_without_span_ordering_shortcuts() {
        let owner = Id::<Entity>::new(0);
        let unit = Id::<Commodity>::new(0);
        let sale = Day::parse(b"2024-03-31").unwrap();
        let acquired = Day::parse(b"2024-02-29").unwrap();
        let mut state = AssetState::new(Id::new(0));
        let mut acquisition = part(8, 0, PartKind::Acquisition, acquired.0, 1, 100, 100);
        acquisition.day = acquired;
        state.parts.push(acquisition);
        let candidates = [(Id::new(0), owner, unit, &state)];
        let within_days = nearest_from(
            candidates,
            owner,
            unit,
            sale,
            Span::days(31),
            None,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(within_days, Some((Id::new(0), acquisition.id)));

        let january = Day::parse(b"2024-01-31").unwrap();
        let february = Day::parse(b"2024-02-29").unwrap();
        let month_clamped = shift(january, Span::months(1));
        assert_eq!(month_clamped, Some(february));
    }

    #[test]
    fn invalid_parts_and_overflow_leave_state_unchanged() {
        let asset = Id::new(0);
        let mut assets = Assets::new(1);
        let improvement = part(1, 0, PartKind::Improvement, 10, 1, 100, 100);
        assert_eq!(
            assets.add_part(asset, improvement),
            Err(AssetError::MissingAcquisition)
        );

        let original = part(2, 0, PartKind::Acquisition, 10, 1, i64::MAX, i64::MAX);
        let original_id = original.id;
        assets.add_part(asset, original).unwrap();
        assert_eq!(
            assets.add_part(asset, original),
            Err(AssetError::DuplicatePart)
        );
        assert_eq!(
            assets.asset(asset).unwrap().cost(original_id),
            Ok(Qty(i64::MAX))
        );

        let improvement = part(3, 1, PartKind::Improvement, 12, 2, 1, 1);
        assets.add_part(asset, improvement).unwrap();
        assert_eq!(
            assets.asset(asset).unwrap().total_cost(),
            Err(AssetError::Overflow)
        );
        assert_eq!(
            assets.carry(original_id, Some((asset, original_id)), Qty(1)),
            Err(AssetError::Overflow)
        );
        assert_eq!(
            assets.asset(asset).unwrap().basis(original_id),
            Ok(Qty(i64::MAX))
        );
        assert_eq!(
            assets.asset(asset).unwrap().cost(original_id),
            Ok(Qty(i64::MAX))
        );
    }

    #[test]
    fn disposal_boundary_is_explicit_and_blocks_later_improvements() {
        let mut assets = Assets::new(1);
        let original = part(1, 0, PartKind::Acquisition, 10, 1, 100, 100);
        let original_id = original.id;
        assets.add_part(Id::new(0), original).unwrap();
        assert!(!assets.asset(Id::new(0)).unwrap().held_at(EventKey {
            day: Day(9),
            sequence: u64::MAX,
        }));
        assets
            .dispose(
                Id::new(0),
                original_id.origin,
                None,
                DisposalBoundary::After(EventKey {
                    day: Day(10),
                    sequence: 1,
                }),
            )
            .unwrap();
        assert!(assets.asset(Id::new(0)).unwrap().held_at(EventKey {
            day: Day(10),
            sequence: 1
        }));
        assert!(!assets.asset(Id::new(0)).unwrap().held_at(EventKey {
            day: Day(10),
            sequence: 2
        }));
        assert_eq!(
            assets.add_part(Id::new(0), part(2, 0, PartKind::Improvement, 11, 2, 10, 10)),
            Err(AssetError::Disposed)
        );
        assert_eq!(
            assets.dispose(
                Id::new(0),
                original_id.origin,
                None,
                DisposalBoundary::Close(Day(12))
            ),
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

        let before_improvement = DisposalBoundary::After(EventKey {
            day: Day(10),
            sequence: 1,
        });
        assert_eq!(
            assets.dispose(asset, acquisition.id.origin, None, before_improvement),
            Err(AssetError::NotHeldAtBoundary),
            "a sale boundary must include every part already present in the ledger"
        );
        assert!(assets.asset(asset).unwrap().held_at(EventKey {
            day: Day(11),
            sequence: 2,
        }));
        assets
            .dispose(
                asset,
                acquisition.id.origin,
                None,
                DisposalBoundary::Close(Day(11)),
            )
            .unwrap();
        assert!(assets.asset(asset).unwrap().held_at(EventKey {
            day: Day(11),
            sequence: 2,
        }));
        assert!(!assets.asset(asset).unwrap().held_at(EventKey {
            day: Day(12),
            sequence: 0,
        }));
    }
}
