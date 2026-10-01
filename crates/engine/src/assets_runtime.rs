//! Ledger hooks that keep canonical asset basis and parcel basis in lockstep.
//!
//! The asset table describes the cost history; parcels describe where that
//! basis is held. Every consume or carry updates both in one ledger operation.

use axiom_core::{Day, Days, Id, Qty, Span};
use axiom_model::Window;
use axiom_model::{Asset, Flow, RuntimeTxn};

use crate::Ledger;
use crate::assets::{
    AssetError, Assets, CarryUpdate, Consumption, DisposalBoundary, Part, PartKind,
};
use crate::lots::PartBasisAdjustment;

/// Returns one part's depreciation over a requested calendar window.
///
/// Acquisition parts use the asset's effective `in-service` date and may
/// exclude land. Improvements start on their own day and never inherit the
/// acquisition's land allocation. `None` indicates an invalid schedule or a
/// land amount greater than the acquisition's cost.
pub(crate) fn part_straight_line(
    part: &Part,
    acquisition_in_service: Day,
    land: Qty,
    life: Span,
    over: Days,
    period: Window,
    mid_month: bool,
) -> Option<Qty> {
    let from = match part.kind {
        PartKind::Acquisition => acquisition_in_service,
        PartKind::Improvement => part.day,
    };
    let land = if part.kind == PartKind::Acquisition {
        land
    } else {
        Qty::ZERO
    };
    let cost = Qty(part.cost.0.checked_sub(land.0)?);
    if cost.is_negative() {
        return None;
    }
    crate::calc::straight_line(cost, life, from, over, period, mid_month)
}

struct AssetPartAddition<'a> {
    assets: &'a mut Assets,
    parcels: Option<PartBasisAdjustment<'a>>,
    asset: Id<Asset>,
    part: Part,
}

impl AssetPartAddition<'_> {
    fn apply(self) -> Result<(), AssetError> {
        let Self {
            assets,
            parcels,
            asset,
            part,
        } = self;
        // `validate_part` ran while this exclusive borrow was acquired, so a
        // second validation cannot fail before the parcel guard is applied.
        assets.add_part(asset, part)?;
        if let Some(parcels) = parcels {
            parcels.apply();
        }
        Ok(())
    }
}

fn anchor_and_total(assets: &Assets, asset: Id<Asset>) -> Result<(crate::PartId, Qty), AssetError> {
    let state = assets.asset(asset).ok_or(AssetError::UnknownAsset)?;
    let anchor = state
        .parts()
        .first()
        .ok_or(AssetError::MissingAcquisition)?
        .id;
    Ok((anchor, state.total_basis()?))
}

impl Ledger<'_, '_, '_> {
    /// Checks a part before the caller lands the corresponding asset parcel.
    pub(crate) fn validate_asset_part(
        &self,
        asset: Id<Asset>,
        part: &Part,
    ) -> Result<(), AssetError> {
        self.world.assets.validate_part(asset, part)
    }

    /// Commits a part after its parcel is in the ledger. The basis check keeps
    /// the part table and holdings from starting out inconsistent.
    pub(crate) fn add_asset_part(
        &mut self,
        asset: Id<Asset>,
        part: Part,
    ) -> Result<(), AssetError> {
        let world = &mut self.world;
        let (assets, holdings) = (&mut world.assets, &mut world.holdings);
        assets.validate_part(asset, &part)?;

        let parcels = if part.kind == PartKind::Acquisition {
            if holdings.part_basis(part.id)? != part.basis {
                return Err(AssetError::ParcelBasisMismatch);
            }
            None
        } else {
            let (anchor, total) = anchor_and_total(assets, asset)?;
            if holdings.part_basis(anchor)? != total {
                return Err(AssetError::ParcelBasisMismatch);
            }
            Some(holdings.prepare_part_basis_adjustment(anchor, part.basis)?)
        };

        AssetPartAddition {
            assets,
            parcels,
            asset,
            part,
        }
        .apply()
    }

    /// Consumes basis on one part and the corresponding held parcels together.
    /// The returned `excess` remains explicit for the caller's diagnostic.
    pub(crate) fn consume_asset_part(
        &mut self,
        asset: Id<Asset>,
        part: crate::PartId,
        requested: Qty,
    ) -> Result<Consumption, AssetError> {
        if requested.is_negative() {
            return Err(AssetError::NegativeAmount);
        }
        let world = &mut self.world;
        let (assets, holdings) = (&mut world.assets, &mut world.holdings);
        let (anchor, total) = anchor_and_total(assets, asset)?;
        if holdings.part_basis(anchor)? != total {
            return Err(AssetError::ParcelBasisMismatch);
        }
        let consumption = assets.prepare_consumption(asset, part, requested)?;
        let applied = consumption.result().applied;
        let parcel_change = holdings.prepare_part_basis_adjustment(anchor, Qty(-applied.0))?;

        // Both stores have been checked. These commits are infallible, so an
        // error cannot leave asset history and live parcel basis out of sync.
        // The guards keep exclusive borrows of both stores through the commit.
        let result = consumption.apply();
        parcel_change.apply();
        Ok(result)
    }

    /// Adds a deferred loss to the selected receiving part, if one was found.
    /// An unreceived carry is still returned to the monitor but changes neither
    /// table.
    pub(crate) fn carry_asset_basis(
        &mut self,
        from: crate::PartId,
        to: Option<(Id<Asset>, crate::PartId)>,
        amount: Qty,
    ) -> Result<CarryUpdate, AssetError> {
        let world = &mut self.world;
        let (assets, holdings) = (&mut world.assets, &mut world.holdings);
        let parcel_change = if let Some((asset, _)) = to {
            let (anchor, total) = anchor_and_total(assets, asset)?;
            if holdings.part_basis(anchor)? != total {
                return Err(AssetError::ParcelBasisMismatch);
            }
            Some(holdings.prepare_part_basis_adjustment(anchor, amount)?)
        } else {
            None
        };
        let carry = assets.prepare_carry(from, to, amount)?;

        // Asset overflow/identity and every parcel share were preflighted.
        // Both exclusive guards remain alive, preventing stale prepared state.
        let result = carry.apply();
        if let Some(parcel_change) = parcel_change {
            parcel_change.apply();
        }
        Ok(result)
    }

    /// Records an ownership boundary after a sale or closing event.
    pub(crate) fn dispose_asset(
        &mut self,
        asset: Id<Asset>,
        txn: RuntimeTxn,
        flow: Option<Id<Flow>>,
        boundary: DisposalBoundary,
    ) -> Result<(), AssetError> {
        self.world.assets.dispose(asset, txn, flow, boundary)
    }

    /// Adds a replacement-basis amount to the canonical lot. For a declared
    /// asset the lot is the acquisition anchor, while the asset table owns the
    /// selected part detail; both checked guards are held before either is
    /// applied. Ordinary security lots have only the holdings side.
    pub(crate) fn carry_basis_to_part(
        &mut self,
        part: crate::PartId,
        amount: Qty,
    ) -> Result<(), AssetError> {
        self.carry_basis_to_parts(&[(part, amount)])
    }

    pub(crate) fn carry_basis_to_parts(
        &mut self,
        additions: &[(crate::PartId, Qty)],
    ) -> Result<(), AssetError> {
        let world = &mut self.world;
        let (assets, holdings) = (&mut world.assets, &mut world.holdings);
        let mut asset_additions = Vec::new();
        let mut checked_assets = Vec::new();
        for &(part, amount) in additions {
            if amount.is_negative() {
                return Err(AssetError::NegativeAmount);
            }
            let Some((asset, record)) = assets.part(part) else { continue };
            let state = assets.asset(asset).ok_or(AssetError::UnknownAsset)?;
            let anchor = state.parts().first().ok_or(AssetError::MissingAcquisition)?.id;
            if part != anchor {
                return Err(AssetError::UnknownPart);
            }
            if !checked_assets.iter().any(|(seen, _)| *seen == asset) {
                if holdings.part_basis(anchor)? != state.total_basis()? {
                    return Err(AssetError::ParcelBasisMismatch);
                }
                checked_assets.push((asset, anchor));
            }
            asset_additions.push((asset, record.id, amount));
        }
        let parcel_additions: Vec<_> = additions
            .iter()
            .map(|&(part, amount)| (part, amount))
            .collect();
        let asset_parts = assets.prepare_basis_additions(&asset_additions)?;
        let parcels = holdings.prepare_part_basis_additions(&parcel_additions)?;
        // Both mutations have been completely preflighted and the guards
        // borrow disjoint canonical stores through these infallible commits.
        asset_parts.apply();
        parcels.apply();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::PartId;
    use axiom_model::ScheduleKind;

    fn day(year: i32, month: u32, day: u32) -> Day {
        Day::from_ymd(year, month, day).unwrap()
    }

    fn month(year: i32, month: u32) -> Days {
        let first = day(year, month, 1);
        let last = day(year, month, axiom_core::day::days_in_month(year, month));
        Days::new(first, last).unwrap()
    }

    fn part(kind: PartKind, day: Day, cost: i64, ordinal: u32) -> Part {
        Part {
            id: PartId {
                origin: RuntimeTxn::contract_occurrence(
                    Id::new(0),
                    ScheduleKind::Regular,
                    day,
                    0,
                    None,
                ),
                ordinal,
            },
            flow: None,
            kind,
            recorded: crate::EventKey {
                day,
                sequence: u64::from(ordinal),
            },
            day,
            cost: Qty(cost),
            basis: Qty(cost),
        }
    }

    #[test]
    fn acquisition_land_and_improvement_service_are_independent() {
        let acquisition = part(PartKind::Acquisition, day(2024, 12, 18), 376_850_00, 0);
        let improvement = part(PartKind::Improvement, day(2025, 9, 15), 14_200_00, 1);
        let life = Span::months(330);

        let dec_2024_acquisition = part_straight_line(
            &acquisition,
            day(2024, 12, 18),
            Qty(93_000_00),
            life,
            month(2024, 12),
            Window::Month,
            true,
        )
        .unwrap();
        let aug_2025_improvement = part_straight_line(
            &improvement,
            day(2024, 12, 18),
            Qty(93_000_00),
            life,
            month(2025, 8),
            Window::Month,
            true,
        )
        .unwrap();
        let sep_2025_improvement = part_straight_line(
            &improvement,
            day(2024, 12, 18),
            Qty(93_000_00),
            life,
            month(2025, 9),
            Window::Month,
            true,
        )
        .unwrap();

        assert!(dec_2024_acquisition > Qty::ZERO);
        assert_eq!(aug_2025_improvement, Qty::ZERO);
        assert!(sep_2025_improvement > Qty::ZERO);
        assert_eq!(
            dec_2024_acquisition,
            crate::calc::straight_line(
                Qty(283_850_00),
                life,
                day(2024, 12, 18),
                month(2024, 12),
                Window::Month,
                true,
            )
            .unwrap(),
            "land is excluded from the acquisition cost only"
        );
    }

    #[test]
    fn part_schedule_rejects_land_above_acquisition_cost() {
        let acquisition = part(PartKind::Acquisition, day(2024, 1, 1), 10_000, 0);
        assert_eq!(
            part_straight_line(
                &acquisition,
                day(2024, 1, 1),
                Qty(10_001),
                Span::months(12),
                month(2024, 1),
                Window::Month,
                true,
            ),
            None
        );
    }
}
