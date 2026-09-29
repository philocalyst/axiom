//! Whose value a flow moves.
//!
//! A law governs a *subject*: a place (and its subtree) or an entity (and the
//! asset places it owns). A flow whose two ends both lie inside a subject moves
//! value around within it, so it neither enters nor leaves the subject: it is
//! not in the subject's totals and does not fire its `on in` / `on out` laws.
//!
//! An entity's boundary is its *asset* places. Income, expense, equity and
//! liability places are the world outside: a paycheck from `income/wages` into
//! checking enters `me` even though `me` owns both places. A household is the
//! entity of everyone who is a member of it, so it owns what they own.

use axiom_core::{Id, Qty};
use axiom_model::{Book, Class, Entity, Place, Subject};

use crate::eval::V3;
use crate::motion::{Motion, Moves};

/// The entities that own what `owner` owns: itself, what it belongs to in the
/// entity tree, and the household it is a member of.
fn owners<'a>(book: &'a Book, owner: Id<Entity>) -> impl Iterator<Item = Id<Entity>> + 'a {
    let household = book.entities[owner].member.filter(move |&house| !book.entities.covers(house, owner));
    book.entities.lineage(owner).chain(household)
}

/// Whether `place` lies within `subject`.
pub(crate) fn inside(book: &Book, subject: Subject, place: Id<Place>) -> bool {
    match subject {
        Subject::Place(root) => book.places.covers(root, place),
        Subject::Entity(root) => {
            let place = &book.places[place];
            place.class == Class::Asset && owners(book, place.owner).any(|owner| owner == root)
        }
        Subject::Asset(_) => unreachable!("{V3}"),
    }
}

/// Every subject `place` lies within: it and its ancestors, and, for an asset
/// place, the entities that own what its owner owns.
pub(crate) fn containing<'a>(book: &'a Book, place: Id<Place>) -> impl Iterator<Item = Subject> + 'a {
    let this = &book.places[place];
    let places = book.places.lineage(place).map(Subject::Place);
    let owners = (this.class == Class::Asset).then(|| owners(book, this.owner)).into_iter().flatten();
    places.chain(owners.map(Subject::Entity))
}

/// Who a subject belongs to: a place's owner; an entity is its own.
pub(crate) fn owner_of(book: &Book, subject: Subject) -> Id<Entity> {
    match subject {
        Subject::Place(place) => book.places[place].owner,
        Subject::Entity(entity) => entity,
        Subject::Asset(asset) => book.assets[asset].owner,
    }
}

/// A place's balance (inflow minus outflow) in the sign people write and read
/// it: `visa = 1_234.56 USD` means 1,234.56 owed. Income, liabilities and
/// equity are flipped, and flipping twice is the identity, so this converts
/// both ways.
pub(crate) fn display(book: &Book, place: Id<Place>, qty: Qty) -> Qty {
    Qty(qty.0 * book.v3_root(place).display_sign())
}

/// Whether `unit` in `place` is money: base currency that is not a claim.
/// Money is told apart by its basis per unit; everything else by the purchase
/// that made it.
pub(crate) fn is_money(book: &Book, place: Id<Place>, unit: Id<axiom_model::Commodity>) -> bool {
    unit == book.base && !book.places[place].claim
}

/// Whether the flow leaves value with one owner's asset places: a transfer
/// between them, or a market moving what an asset is worth.
pub(crate) fn stays_with_owner(m: &Motion) -> bool {
    m.moves == Moves::Loss || (m.target.class == Class::Asset && m.target.owner == m.source.owner)
}
