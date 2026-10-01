//! Typed indices into typed arenas.
//!
//! An `Id<Place>` cannot index the entity arena. Ids are `u32`, `Copy`, and
//! carry their type only at compile time. A [`Run`] is a contiguous stretch of
//! them: what one transaction made, or one node's subtree.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

pub struct Id<T> {
    raw: u32,
    of: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn new(raw: u32) -> Id<T> {
        Id { raw, of: PhantomData }
    }

    pub const fn index(self) -> usize {
        self.raw as usize
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Id<T> {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Id<T>) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Id<T>) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Id<T>) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.raw);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("?");
        write!(f, "{name}#{}", self.raw)
    }
}

/// A vector indexed by `Id<T>`.
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Arena<T> {
    pub const fn new() -> Arena<T> {
        Arena { items: Vec::new() }
    }

    pub fn push(&mut self, item: T) -> Id<T> {
        let id = Id::new(u32::try_from(self.items.len()).expect("fewer than 2^32 items"));
        self.items.push(item);
        id
    }

    /// Reserves room for `additional` items without changing any existing id.
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.items.get(id.index())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn ids(&self) -> Ids<T> {
        Ids { next: 0, end: self.items.len() as u32, of: PhantomData }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Id<T>, &T)> {
        self.items.iter().enumerate().map(|(i, item)| (Id::new(i as u32), item))
    }

    pub fn values(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

impl<T> From<Vec<T>> for Arena<T> {
    /// Items keep their positions: item `i` gets `Id::new(i)`.
    fn from(items: Vec<T>) -> Arena<T> {
        assert!(u32::try_from(items.len()).is_ok(), "fewer than 2^32 items");
        Arena { items }
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Arena<T> {
        Arena::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Arena<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.items).finish()
    }
}

impl<T> Index<Id<T>> for Arena<T> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &T {
        &self.items[id.index()]
    }
}

impl<T> IndexMut<Id<T>> for Arena<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut T {
        &mut self.items[id.index()]
    }
}

/// A contiguous run of ids in one arena: the flows a transaction made are
/// `first .. first + len` of the book's flows. A run is one value where a
/// pair of numbers was, and indexing the arena by it gives the slice.
pub struct Run<T> {
    start: u32,
    len: u32,
    of: PhantomData<fn() -> T>,
}

impl<T> Run<T> {
    /// The `len` ids from `start` on.
    pub const fn new(start: Id<T>, len: u32) -> Run<T> {
        Run { start: start.raw, len, of: PhantomData }
    }

    /// The first id of the run; where an empty run would be.
    pub const fn start(self) -> Id<T> {
        Id::new(self.start)
    }

    pub const fn len(self) -> u32 {
        self.len
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// The ids in the run, in order.
    pub fn ids(self) -> Ids<T> {
        Ids { next: self.start, end: self.start + self.len, of: PhantomData }
    }

    pub fn contains(self, id: Id<T>) -> bool {
        id.raw.wrapping_sub(self.start) < self.len
    }
}

impl<T> Clone for Run<T> {
    fn clone(&self) -> Run<T> {
        *self
    }
}

impl<T> Copy for Run<T> {}

impl<T> PartialEq for Run<T> {
    fn eq(&self, other: &Run<T>) -> bool {
        (self.start, self.len) == (other.start, other.len)
    }
}

impl<T> Eq for Run<T> {}

impl<T> fmt::Debug for Run<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("?");
        write!(f, "{name}#{}..{}", self.start, self.start + self.len)
    }
}

impl<T> Index<Run<T>> for Arena<T> {
    type Output = [T];
    fn index(&self, run: Run<T>) -> &[T] {
        &self.items[run.start as usize..(run.start + run.len) as usize]
    }
}

/// The ids of a [`Run`], of a whole arena, or of a subtree, in order.
pub struct Ids<T> {
    next: u32,
    end: u32,
    of: PhantomData<fn() -> T>,
}

impl<T> Iterator for Ids<T> {
    type Item = Id<T>;
    fn next(&mut self) -> Option<Id<T>> {
        (self.next < self.end).then(|| {
            self.next += 1;
            Id::new(self.next - 1)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.end - self.next) as usize;
        (n, Some(n))
    }
}

impl<T> ExactSizeIterator for Ids<T> {}
impl<T> FusedIterator for Ids<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_a_slice_of_its_arena() {
        let arena = Arena::from(vec!['a', 'b', 'c', 'd', 'e']);
        let run = Run::new(Id::new(1), 3);
        assert_eq!(&arena[run], &['b', 'c', 'd']);
        assert_eq!(run.ids().map(|id| arena[id]).collect::<String>(), "bcd");
        assert_eq!((run.len(), run.ids().len()), (3, 3));
        let inside = |at: u32| run.contains(Id::new(at));
        assert_eq!([0, 1, 2, 3, 4].map(inside), [false, true, true, true, false]);
    }

    #[test]
    fn an_empty_run_holds_nothing() {
        let arena = Arena::from(vec![1, 2, 3]);
        let none = Run::new(Id::new(3), 0);
        assert!(none.is_empty() && arena[none].is_empty() && none.ids().next().is_none());
        assert!(!none.contains(Id::new(3)) && !none.contains(Id::new(0)));
        assert_eq!(arena.ids().len(), 3);
    }
}
