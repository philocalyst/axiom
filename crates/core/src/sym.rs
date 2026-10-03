//! Interned names.
//!
//! Symbols point into the source text that was read once at startup. Interning
//! copies nothing: the interner maps borrowed `&'s str` to a `u32`.

use crate::hash::Map;

/// An interned name. Compare and hash as a `u32`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Sym(u32);

impl Sym {
    /// The symbol's place among the names interned, from zero: what a table indexed by name is indexed by.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Default)]
pub struct Interner<'s> {
    ids: Map<&'s str, Sym>,
    names: Vec<&'s str>,
}

impl<'s> Interner<'s> {
    pub fn intern(&mut self, name: &'s str) -> Sym {
        *self.ids.entry(name).or_insert_with(|| {
            self.names.push(name);
            Sym(self.names.len() as u32 - 1)
        })
    }

    /// The symbol for `name`, if it was ever interned.
    pub fn get(&self, name: &str) -> Option<Sym> {
        self.ids.get(name).copied()
    }

    pub fn name(&self, sym: Sym) -> &'s str {
        self.names[sym.0 as usize]
    }

    /// How many names are interned: one more than the greatest [`Sym::index`].
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_numbered_in_the_order_names_are_first_interned() {
        let mut names = Interner::default();
        assert!(names.is_empty());
        let (a, b, again) = (names.intern("jordan"), names.intern("401k"), names.intern("jordan"));
        assert_eq!((a.index(), b.index()), (0, 1));
        assert_eq!(again, a, "a name is interned once");
        assert_eq!(names.len(), 2, "one more than the greatest index");
        assert_eq!((names.get("401k"), names.get("riley")), (Some(b), None));
        assert_eq!(names.name(b), "401k");
    }
}
