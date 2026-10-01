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
        let consumption = self.world.assets.prepare_consumption(asset, part, requested)?;
        if self.world.holdings.part_basis(part)? != consumption.before() {
            return Err(AssetError::ParcelBasisMismatch);
        }
        let parcel_change = self
            .world
            .holdings
            .prepare_part_basis_adjustment(part, -consumption.result().applied)?;

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
        let carry = self.world.assets.prepare_carry(from, to, amount)?;
        let parcel_change = if let Some((_, part)) = to {
            if self.world.holdings.part_basis(part)? != carry.before().ok_or(AssetError::UnknownPart)? {
                return Err(AssetError::ParcelBasisMismatch);
            }
            Some(self.world.holdings.prepare_part_basis_adjustment(part, amount)?)
        } else {
            None
        };

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
}
