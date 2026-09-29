//! Questions views ask about places.

use axiom_core::Id;
use axiom_model::{Book, End, Flow, PathRoot, Place};


/// A place's full path.
pub fn path<'s>(book: &Book<'s>, place: Id<Place>) -> &'s str {
    book.name(book.places[place].path)
}

/// Every name someone might type for a place: full paths and last segments.
pub fn names<'a, 's>(book: &'a Book<'s>) -> impl Iterator<Item = &'s str> + 'a {
    book.places.ids().flat_map(|place| [path(book, place), leaf(book, place)])
}

/// `assets/bank/checking → expenses/rent`. An end that only changes basis is
/// written the way the journal writes it: `assets/house.basis`.
pub fn route(book: &Book, flow: &Flow) -> String {
    let end = |end: End, place: Id<Place>| {
        let path = path(book, place);
        if flow.moves_quantity(end) { path.to_string() } else { format!("{path}.basis") }
    };
    format!("{} → {}", end(End::From, flow.from), end(End::To, flow.to))
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

/// Whether the place is a path root: `assets`, `expenses`, … Roots group
/// their places; nobody declares them.
pub fn is_class_root(book: &Book, place: Id<Place>) -> bool {
    book.name(book.places[place].path) == book.v3_root(place).path()
}

// v3 bridge: the report lane replaces `Side` and `v3_side` with purposes.
/// Which side of the income statement a place is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Income,
    Spending,
}

/// The side the path root a place sits under puts it on, if it is on one.
pub fn v3_side(book: &Book, place: Id<Place>) -> Option<Side> {
    match book.v3_root(place) {
        PathRoot::Income => Some(Side::Income),
        PathRoot::Expenses => Some(Side::Spending),
        PathRoot::Assets | PathRoot::Liabilities | PathRoot::Equity => None,
    }
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
