//! Interned names.
//!
//! Symbols point into the source text that was read once at startup. Interning
//! copies nothing: the interner maps borrowed `&'s str` to a `u32`.

use crate::hash::Map;

/// An interned name. Compare and hash as a `u32`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Sym(u32);

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
}
