//! Whose value a flow moves.
//!
//! A law governs a *subject*: a place (and its subtree) or an entity (and the
//! asset places it owns). A flow whose two ends both lie inside a subject moves
//! value around within it, so it neither enters nor leaves the subject: it is
//! not in the subject's totals and does not fire its `on in` / `on out` laws.
//!
//! An entity's boundary is its *asset* places. Income, expense, equity and
//! liability places are the world outside: a paycheck from `income/wages` into
//! checking enters `me` even though `me` owns both places.

use axiom_core::{Id, Qty};
use axiom_model::{Book, Class, Entity, Place, Subject};

/// Whether `place` lies within `subject`.
pub(crate) fn inside(book: &Book, subject: Subject, place: Id<Place>) -> bool {
    match subject {
        Subject::Place(root) => book.places.covers(root, place),
        Subject::Entity(root) => {
            let place = &book.places[place];
            place.class == Class::Asset && book.entities.covers(root, place.owner)
        }
    }
}

/// The subjects a flow from `from` to `to` leaves: every subject containing
/// `from` but not `to`. Entering is leaving read backwards:
/// `departures(book, to, from)`.
pub(crate) fn departures<'a>(book: &'a Book, from: Id<Place>, to: Id<Place>) -> impl Iterator<Item = Subject> + 'a {
    let place = &book.places[from];
    let owners = (place.class == Class::Asset).then(|| book.entities.lineage(place.owner));
    let places = book.places.lineage(from).map(Subject::Place);
    places.chain(owners.into_iter().flatten().map(Subject::Entity)).filter(move |&s| !inside(book, s, to))
}

/// Who a subject belongs to: a place's owner; an entity is its own.
pub(crate) fn owner_of(book: &Book, subject: Subject) -> Id<Entity> {
    match subject {
        Subject::Place(place) => book.places[place].owner,
        Subject::Entity(entity) => entity,
    }
}

/// A place's balance (inflow minus outflow) in the sign people write and read
/// it: `visa = 1_234.56 USD` means 1,234.56 owed. Income, liabilities and
/// equity are flipped, and flipping twice is the identity, so this converts
/// both ways.
pub(crate) fn display(book: &Book, place: Id<Place>, qty: Qty) -> Qty {
    Qty(qty.0 * book.places[place].class.display_sign())
}

/// Whether value moving `from` → `to` stays with one owner's asset places.
pub(crate) fn stays_with_owner(book: &Book, from: Id<Place>, to: Id<Place>) -> bool {
    let (from, to) = (&book.places[from], &book.places[to]);
    to.class == Class::Asset && to.owner == from.owner
}
