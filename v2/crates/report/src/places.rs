//! Questions views ask about places.

use axiom_core::Id;
use axiom_model::{Book, End, Flow, Place};


/// A place's full path.
pub fn path<'s>(book: &'s Book<'_>, place: Id<Place>) -> &'s str {
    book.name(book.places[place].path)
}

/// Every name someone might type for a place: full paths and last segments.
pub fn names<'a>(book: &'a Book<'_>) -> impl Iterator<Item = &'a str> + 'a {
    book.places.ids().flat_map(|place| [path(book, place), leaf(book, place)])
}

/// A route such as `checking → landlord`. An end that only changes basis is
/// written the way the journal writes it: `house.basis`.
pub fn route(book: &Book, flow: &Flow) -> String {
    let end = |end: End, place: Id<Place>| {
        let path = path(book, place);
        if flow.moves_quantity(end) { path.to_string() } else { format!("{path}.basis") }
    };
    format!("{} → {}", end(End::From, flow.from), end(End::To, flow.to))
}

/// The last segment of a path: a tree's indentation supplies the rest.
pub fn leaf<'s>(book: &'s Book<'_>, place: Id<Place>) -> &'s str {
    let full = path(book, place);
    full.rsplit('/').next().unwrap_or(full)
}

/// A place's indentation in a tree table.
pub fn depth(book: &Book, place: Id<Place>) -> usize {
    book.places.depth(place) as usize
}
