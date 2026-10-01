//! The sign in which each place's balance is shown.

use axiom_core::{Id, Qty};
use axiom_model::{Book, Place};

/// A per-place balance sign, computed once from its semantic class.
pub struct Sides(Box<[i64]>);

impl Sides {
    pub(crate) fn of(book: &Book) -> Sides {
        Sides(book.places.ids().map(|place| book.places[place].class.display_sign()).collect())
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
