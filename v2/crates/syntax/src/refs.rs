//! Typed indices into a file's tables.
//!
//! A large file is parsed in pieces, each with tables of its own, so an index
//! must say which piece it is in as well as where. This module is the only
//! code that knows how: the top bits name the piece and the rest is a position
//! in that piece's table of `T`s.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Range;

/// The bits of a reference that give the position in its piece's table.
const LOCAL_BITS: u32 = 24;
const LOCAL_MASK: u32 = (1 << LOCAL_BITS) - 1;

/// How many pieces a file can be cut into.
pub(crate) const MAX_PIECES: usize = 1 << (32 - LOCAL_BITS);

/// A position in a file's tables: the piece it is in, and where in that
/// piece's table of `T`s. Opaque: index the [`File`](crate::File) it came from.
pub struct Ref<T> {
    raw: u32,
    of: PhantomData<fn() -> T>,
}

impl<T> Ref<T> {
    /// Position `local` of piece `piece`'s table. A piece has at most 2^24 of
    /// any one node, which its size (at most 16 MiB) guarantees: a node takes a
    /// byte of source.
    pub(crate) fn new(piece: usize, local: usize) -> Ref<T> {
        debug_assert!(piece < MAX_PIECES && local <= LOCAL_MASK as usize);
        Ref { raw: (piece as u32) << LOCAL_BITS | local as u32, of: PhantomData }
    }

    /// The piece of the file this is in.
    pub(crate) fn piece(self) -> usize {
        (self.raw >> LOCAL_BITS) as usize
    }

    /// The position in that piece's table.
    pub(crate) fn local(self) -> usize {
        (self.raw & LOCAL_MASK) as usize
    }

    /// How far into its table this is past `first`, which is in the same piece:
    /// what a subtree's node is, counted from the subtree's start.
    pub fn offset_from(self, first: Ref<T>) -> usize {
        debug_assert_eq!(self.piece(), first.piece(), "positions in different pieces");
        self.local() - first.local()
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Ref<T> {
        *self
    }
}

impl<T> Copy for Ref<T> {}

impl<T> PartialEq for Ref<T> {
    fn eq(&self, other: &Ref<T>) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Ref<T> {}

impl<T> PartialOrd for Ref<T> {
    fn partial_cmp(&self, other: &Ref<T>) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Ref<T> {
    fn cmp(&self, other: &Ref<T>) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for Ref<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for Ref<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.piece(), self.local())
    }
}

/// A run of `T`s in their table, addressed by position: `&file[many]`.
pub struct Many<T> {
    first: Ref<T>,
    len: u32,
}

impl<T> Many<T> {
    /// Nothing. Every empty run is this one.
    pub const EMPTY: Many<T> = Many { first: Ref { raw: 0, of: PhantomData }, len: 0 };

    /// The `len` nodes from `first`.
    pub(crate) fn new(first: Ref<T>, len: usize) -> Many<T> {
        match len {
            0 => Many::EMPTY,
            _ => Many { first, len: len as u32 },
        }
    }

    /// How many nodes are in the run.
    pub fn len(self) -> usize {
        self.len as usize
    }

    /// Whether the run has no nodes.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    /// The piece the run is in.
    pub(crate) fn piece(self) -> usize {
        self.first.piece()
    }

    /// Where the run is in its piece's table.
    pub(crate) fn range(self) -> Range<usize> {
        self.first.local()..self.first.local() + self.len()
    }
}

impl<T> Clone for Many<T> {
    fn clone(&self) -> Many<T> {
        *self
    }
}

impl<T> Copy for Many<T> {}

impl<T> Default for Many<T> {
    fn default() -> Many<T> {
        Many::EMPTY
    }
}

impl<T> fmt::Debug for Many<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}+{}", self.first, self.len)
    }
}
