//! v3 bridge: what only the v3 model needs.
//!
//! The v3 model puts every place under one of five path roots, which decides
//! the sign its balance is shown in, and lets a flow end at `PLACE.basis`,
//! moving a place's basis rather than its quantity. The v4 model has neither,
//! and compiles no law of an asset. Everything in the engine that depends on
//! them is here, or is a hook that calls here and says so, so that deleting v3
//! is deleting this module and those hooks.

use axiom_core::{Id, Qty};
use axiom_model::{Book, Detail, End, Flow, Place};

use crate::explain;
use crate::ledger::Ledger;
use crate::lots::Selection;
use crate::motion::Motion;
use crate::scope::is_money;

/// Why an arm the v3 model never reaches says so.
pub(crate) const V3: &str = "the v3 model compiles no asset laws";

/// The sign each place's balance is shown in, by place.
///
/// A balance is inflow minus outflow. Liabilities, income and equity are
/// naturally negative, and people write them the other way round:
/// `visa = 1_234.56 USD` means 1,234.56 owed. Flipping twice is the identity,
/// so one sign converts both ways. v3 reads the sign off the path root, which
/// costs a string comparison; it is read once here.
pub struct Sides(Box<[i64]>);

impl Sides {
    pub(crate) fn of(book: &Book) -> Sides {
        Sides(book.places.ids().map(|place| book.v3_root(place).display_sign()).collect())
    }

    /// The sign used to show the balance of `place`.
    pub fn sign(&self, place: Id<Place>) -> i64 {
        self.0[place.index()]
    }

    /// A quantity in the sign people write and read it, or back.
    pub fn display(&self, place: Id<Place>, qty: Qty) -> Qty {
        Qty(qty.0 * self.sign(place))
    }
}

/// `PLACE.basis` at one end of the flow: that end moves basis, not quantity.
pub(crate) fn basis_end(detail: &Detail) -> Option<End> {
    detail.basis_end
}

/// The end the same value moving back would have it at.
pub(crate) fn opposite(end: End) -> End {
    match end {
        End::From => End::To,
        End::To => End::From,
    }
}

/// Whether the flow's `end` moves any quantity; a basis end does not.
pub(crate) fn moves_quantity(flow: &Flow, end: End) -> bool {
    flow.moves_quantity(end)
}

impl Ledger<'_, '_, '_> {
    /// `PLACE.basis`: what the flow moved changes the basis of the parcels the
    /// place holds (`sign` +1 raises it, -1 lowers it), spread by quantity, and
    /// nothing else about them. A place that holds nothing cannot carry it.
    pub(crate) fn change_basis(&mut self, m: &Motion, place: Id<Place>, sign: i64) {
        let book = self.plan.book;
        let amount = self.base_value(m, if sign > 0 { m.arrive } else { m.out }).unwrap_or(Qty::ZERO);
        let selection = Selection { selectors: m.select, txns: &book.txns };
        let held: Qty = self.world.holdings.of(place).map(|slot| slot.basis(is_money(book, place, slot.unit))).sum();
        let moved = if sign > 0 { amount } else { amount.min(held) };
        let money = |unit| is_money(book, place, unit);
        let carried = moved.is_zero()
            || self.world.holdings.rebase(place, Qty(moved.0 * sign), &selection, money, (m.day, m.txn));
        if !carried || moved < amount {
            let diagnostic = explain::basis_shortfall(book, m, place, held, amount, carried);
            self.record.report(diagnostic);
        }
    }
}
