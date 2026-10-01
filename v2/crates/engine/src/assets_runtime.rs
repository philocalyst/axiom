//! Ledger hooks that keep canonical asset basis and parcel basis in lockstep.
//!
//! The asset table describes the cost history; parcels describe where that
//! basis is held. Every consume or carry updates both in one ledger operation.

use axiom_core::{Id, Qty};
use axiom_model::{Asset, Flow, RuntimeTxn};

use crate::Ledger;
use crate::assets::{AssetError, CarryUpdate, Consumption, DisposalBoundary, Part};

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
        if self.world.holdings.part_basis(part.id)? != part.basis {
            return Err(AssetError::ParcelBasisMismatch);
        }
        self.world.assets.add_part(asset, part)
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
        let part_basis = self
            .world
            .assets
            .asset(asset)
            .ok_or(AssetError::UnknownAsset)?
            .basis(part)?;
        if self.world.holdings.part_basis(part)? != part_basis {
            return Err(AssetError::ParcelBasisMismatch);
        }
        let consumed = self.world.assets.consume(asset, part, requested)?;
        self.world
            .holdings
            .adjust_part_basis(part, -consumed.applied)?;
        Ok(consumed)
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
        if let Some((asset, part)) = to {
            let basis = self
                .world
                .assets
                .asset(asset)
                .ok_or(AssetError::UnknownAsset)?
                .basis(part)?;
            if self.world.holdings.part_basis(part)? != basis {
                return Err(AssetError::ParcelBasisMismatch);
            }
        }
        let update = self.world.assets.carry(from, to, amount)?;
        if let Some((_, part)) = to {
            self.world.holdings.adjust_part_basis(part, amount)?;
        }
        Ok(update)
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
}
