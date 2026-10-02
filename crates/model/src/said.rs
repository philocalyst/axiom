//! What a book says of a thing: the values of slots, read back.
//!
//! The facts store holds datums, numbers and tags, and the schema knows the type of each slot, so a reader that has a
//! name and a thing asks here and gets a [`Value`]. A kind's facts are the defaults of its things, so a thing that
//! says nothing of a slot on a day has the nearest kind above it that does. The kind chain is the model's tree, which
//! the store does not know, so the walk is here and the store is asked once per step of it.

use axiom_core::{Day, Id, Sym};

use crate::book::{Book, Kind};
use crate::holders::Holder;
use crate::law::Value;

impl Holder {
    /// The kind of a thing, and of a kind itself.
    pub fn kind(self, book: &Book) -> Id<Kind> {
        match self {
            Holder::Kind(kind) => kind,
            Holder::Place(id) => book.places[id].kind,
            Holder::Entity(id) => book.entities[id].kind,
            Holder::Commodity(id) => book.commodities[id].kind,
            Holder::Asset(id) => book.assets[id].kind,
        }
    }
}

impl Book<'_> {
    /// What `thing` itself says of its slot `name` on `day`. `None` where it says nothing, or where the slot holds
    /// several values, which a law does not read.
    pub fn own(&self, thing: impl Into<Holder>, name: Sym, day: Day) -> Option<Value> {
        self.said_of(self.holders.number(thing), name, day)
    }

    /// What the nearest of `kind` and the kinds above it that says anything says of `name` on `day`: the default of
    /// the things of `kind`.
    pub fn by_kind(&self, kind: Id<Kind>, name: Sym, day: Day) -> Option<Value> {
        self.kinds.lineage(kind).find_map(|above| self.said_of(self.holders.number(Holder::Kind(above)), name, day))
    }

    /// What holds of `thing`'s slot `name` on `day`: its own, else its kind's default.
    pub fn said(&self, thing: impl Into<Holder>, name: Sym, day: Day) -> Option<Value> {
        let thing = thing.into();
        self.own(thing, name, day).or_else(|| self.by_kind(thing.kind(self), name, day))
    }

    /// Whether `thing` itself says anything of its slot `name`, on some day.
    pub fn says(&self, thing: impl Into<Holder>, name: Sym) -> bool {
        self.schema.number(name).is_some_and(|slot| self.facts.says(slot, self.holders.number(thing)))
    }

    fn said_of(&self, holder: u32, name: Sym, day: Day) -> Option<Value> {
        let slot = self.schema.number(name)?;
        let datum = self.facts.datum_at(slot, holder, day)?;
        Value::of(datum, self.schema.ty(slot))
    }
}
