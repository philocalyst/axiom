//! Typed indices into typed arenas.
//!
//! An `Id<Place>` cannot index the entity arena. Ids are `u32`, `Copy`, and
//! carry their type only at compile time.

use std::fmt;
use std::hash::{Hash, Hasher};
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

    pub fn with_capacity(capacity: usize) -> Arena<T> {
        Arena { items: Vec::with_capacity(capacity) }
    }

    /// Room for `more` items beyond those held.
    pub fn reserve(&mut self, more: usize) {
        self.items.reserve(more);
    }

    pub fn push(&mut self, item: T) -> Id<T> {
        let id = Id::new(u32::try_from(self.items.len()).expect("fewer than 2^32 items"));
        self.items.push(item);
        id
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

    pub fn ids(&self) -> impl ExactSizeIterator<Item = Id<T>> + use<T> {
        (0..self.items.len() as u32).map(Id::new)
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
