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

/// Which side of the income statement a place is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Income,
    Spending,
}

/// What a place's side of the books says about it.
struct Sided {
    /// What shows a balance the way people read it.
    sign: i64,
    side: Option<Side>,
    /// A path root (`assets`, `expenses`, …) only groups places; nobody declares it.
    root: bool,
}

// v3 bridge: v4 places say what they are by `class`, and purposes tell income
// from spending. `Sides` and `Side` go with the v3 model.
/// The side of the books each place is on and the sign of its balance, read
/// once from the path roots of a v3 book, so no view parses a path while it
/// works. A place under no path root is a v4 one: its class gives its sign.
pub struct Sides(Box<[Sided]>);

impl Sides {
    pub fn new(book: &Book) -> Sides {
        let sided = |place: &Place| {
            let path = book.name(place.path);
            let Some(root) = PathRoot::of(path) else {
                return Sided { sign: place.class.display_sign(), side: None, root: false };
            };
            let side = match root {
                PathRoot::Income => Some(Side::Income),
                PathRoot::Expenses => Some(Side::Spending),
                PathRoot::Assets | PathRoot::Liabilities | PathRoot::Equity => None,
            };
            Sided { sign: root.display_sign(), side, root: path == root.path() }
        };
        Sides(book.places.values().map(sided).collect())
    }

    pub fn sign(&self, place: Id<Place>) -> i64 {
        self.0[place.index()].sign
    }

    /// The side of the income statement the place is on, if it is on one.
    pub fn side(&self, place: Id<Place>) -> Option<Side> {
        self.0[place.index()].side
    }

    pub fn is_root(&self, place: Id<Place>) -> bool {
        self.0[place.index()].root
    }

    /// The top-level category a place belongs to: the child of its root
    /// (`expenses/food` for `expenses/food/groceries`).
    pub fn category(&self, book: &Book, place: Id<Place>) -> Id<Place> {
        let (mut top, mut below) = (place, place);
        for ancestor in book.places.lineage(place) {
            (below, top) = (top, ancestor);
        }
        if self.is_root(top) { below } else { top }
    }
}
