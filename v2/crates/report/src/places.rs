//! Questions views ask about places.

use axiom_core::Id;
use axiom_model::{Book, Flow, Place};

/// A place's full path.
pub fn path<'s>(book: &Book<'s>, place: Id<Place>) -> &'s str {
    book.name(book.places[place].path)
}

/// Every name someone might type for a place: full paths and last segments.
pub fn names<'a, 's>(book: &'a Book<'s>) -> impl Iterator<Item = &'s str> + 'a {
    book.places.ids().flat_map(|place| [path(book, place), leaf(book, place)])
}

/// `assets/bank/checking → expenses/rent`
pub fn route(book: &Book, flow: &Flow) -> String {
    format!("{} → {}", path(book, flow.from), path(book, flow.to))
}

/// The last segment of a path: a tree's indentation supplies the rest.
pub fn leaf<'s>(book: &Book<'s>, place: Id<Place>) -> &'s str {
    let full = path(book, place);
    full.rsplit('/').next().unwrap_or(full)
}

/// A place's indentation in a tree table.
pub fn depth(book: &Book, place: Id<Place>) -> usize {
    book.places.depth(place) as usize
}

/// Whether the place is a class root: `assets`, `expenses`, … Roots group
/// their class; nobody declares them.
pub fn is_class_root(book: &Book, place: Id<Place>) -> bool {
    let place = &book.places[place];
    book.name(place.path) == place.class.root()
}

/// The top-level category a place belongs to: the child of its class root
/// (`expenses/food` for `expenses/food/groceries`).
pub fn category(book: &Book, place: Id<Place>) -> Id<Place> {
    let (mut top, mut below) = (place, place);
    for ancestor in book.places.lineage(place) {
        (below, top) = (top, ancestor);
    }
    if is_class_root(book, top) { below } else { top }
}
