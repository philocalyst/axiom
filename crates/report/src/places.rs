//! Questions views ask about places.

use axiom_core::Id;
use axiom_model::{Book, Flow, Place};

/// A place's full path.
pub fn path<'s>(book: &'s Book<'_>, place: Id<Place>) -> &'s str {
    book.name(book.places[place].path)
}

/// Every name someone might type for a place: full paths and last segments.
pub fn names<'a>(book: &'a Book<'_>) -> impl Iterator<Item = &'a str> + 'a {
    book.places.ids().flat_map(|place| [path(book, place), leaf(book, place)])
}

/// A route such as `checking → landlord`.
pub fn route(book: &Book, flow: &Flow) -> String {
    format!("{} → {}", path(book, flow.from), path(book, flow.to))
}

/// The last segment of a path: a tree's indentation supplies the rest. A spelled account has no places above it, so
/// nothing supplies the entities before its name, and it is labelled by all of them.
pub fn leaf<'s>(book: &'s Book<'_>, place: Id<Place>) -> &'s str {
    let full = path(book, place);
    if book.is_spelled(place) {
        return full;
    }
    full.rsplit('/').next().unwrap_or(full)
}

/// A place's indentation in a tree table.
pub fn depth(book: &Book, place: Id<Place>) -> usize {
    book.places.depth(place) as usize
}
