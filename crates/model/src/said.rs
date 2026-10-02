//! What a book says of a thing: the values of slots, read back.
//!
//! The facts store holds datums, numbers and tags, and the schema knows the type of each slot, so a reader that has a
//! name and a thing asks here and gets a [`Value`]. A kind's facts are the defaults of its things, so a thing that
//! says nothing of a slot on a day has the nearest kind above it that does. The kind chain is the model's tree, which
//! the store does not know, so the walk is here and the store is asked once per step of it.

use axiom_core::tagless::Field;
use axiom_core::{Day, Days, Id, Key, Loc, Many, Ratio, SlotId, Span, Sym};
use axiom_syntax::Policy;

use crate::book::{Basis, Book, Commodity, Entity, Kind, Place, Purpose, Role, System};
use crate::builtin::{self, Coded};
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

    /// What `thing` says of one of the language's own slots, else the nearest kind above it that does. The language
    /// says its slots by declarations alone, which hold from the beginning of time.
    pub fn fact<V: Field>(&self, key: Key<V>, thing: impl Into<Holder>) -> Option<V> {
        let thing = thing.into();
        let kinds = self.kinds.lineage(thing.kind(self)).map(|kind| self.holders.number(Holder::Kind(kind)));
        let own = (!matches!(thing, Holder::Kind(_))).then(|| self.holders.number(thing));
        self.facts.at_first(key, own.into_iter().chain(kinds), Day::MIN)
    }

    /// What the nearest of `thing` and its kinds that says anything of one of the language's own slots says, and
    /// which of them says it.
    pub fn saying<V: Field>(&self, key: Key<V>, thing: impl Into<Holder>) -> Option<(V, Holder)> {
        let thing = thing.into();
        let own = (!matches!(thing, Holder::Kind(_))).then_some(thing);
        let kinds = self.kinds.lineage(thing.kind(self)).map(Holder::Kind);
        let says = |holder: Holder| Some((self.facts.at(key, self.holders.number(holder), Day::MIN)?, holder));
        own.into_iter().chain(kinds).find_map(says)
    }

    /// Where a line of the language was written, as it was said of `thing`'s slot (and of `member` of it, for a slot of
    /// several).
    pub fn site(&self, thing: impl Into<Holder>, slot: SlotId, member: u32) -> Option<Loc> {
        self.sites.get(&(self.holders.number(thing), slot.0, member)).copied()
    }

    /// What an account kind takes a flow of purpose `from` as, and the kind that says so: the nearest.
    pub fn take(&self, kind: Id<Kind>, from: Id<Purpose>) -> Option<(Id<Purpose>, Holder)> {
        let from = from.index() as u32;
        self.kinds.lineage(kind).find_map(|above| {
            let set = self.facts.at(builtin::TAKES, self.holders.number(Holder::Kind(above)), Day::MIN)?;
            let (_, to) = self.facts.members(set).find(|&(taken, _)| taken == from)?;
            Some((Id::new(to), Holder::Kind(above)))
        })
    }

    /// How `thing`, a place or a commodity, has its parcels relieved, where it says.
    pub fn select(&self, thing: impl Into<Holder>) -> Option<Policy> {
        self.fact(builtin::SELECT, thing).and_then(Policy::decode)
    }

    /// How long `thing`, a place or a commodity, takes to turn into money, where it says.
    pub fn liquidity(&self, thing: impl Into<Holder>) -> Option<Span> {
        self.fact(builtin::LIQUIDITY, thing)
    }

    /// The commodities a place may hold, or `None` for any.
    pub fn holds(&self, place: Id<Place>) -> Option<impl ExactSizeIterator<Item = Id<Commodity>> + Clone + '_> {
        let set: Many<Id<Commodity>> = self.fact(builtin::HOLDS, place)?;
        let members = self.facts.members(set);
        (members.len() > 0).then_some(members)
    }

    /// The one commodity a place may hold, if it names exactly one.
    pub fn holds_only(&self, place: Id<Place>) -> Option<Id<Commodity>> {
        let mut holds = self.holds(place)?;
        let only = holds.next()?;
        holds.next().is_none().then_some(only)
    }

    /// The household an entity belongs to.
    pub fn member(&self, entity: Id<Entity>) -> Option<Id<Entity>> {
        self.fact(builtin::MEMBER, entity)
    }

    /// Whether an entity's money stays tied to it.
    pub fn is_restricted(&self, entity: Id<Entity>) -> bool {
        self.fact(builtin::RESTRICTED, entity).unwrap_or(false)
    }

    /// The systems an entity lives under on a day.
    pub fn residing(&self, entity: Id<Entity>, day: Day) -> impl Iterator<Item = Id<System>> + '_ {
        let set: Option<Many<Id<System>>> = self.facts.at(builtin::LIVES, self.holders.number(entity), day);
        set.into_iter().flat_map(|set| self.facts.members(set))
    }

    /// Every system an entity lives under, and the days it does: the stretches of its residences.
    pub fn residences(&self, entity: Id<Entity>) -> impl Iterator<Item = (Days, Id<System>)> + '_ {
        let steps = self.facts.steps(builtin::LIVES, self.holders.number(entity));
        steps.flat_map(|(days, set)| self.facts.members(set).map(move |system| (days, system)))
    }

    /// The currency an entity counts in: its own, else its kinds', else that of the first system it lives under, else
    /// the book's.
    pub fn currency(&self, entity: Id<Entity>) -> Id<Commodity> {
        let own = self.fact(builtin::CURRENCY, entity);
        let residence = || self.residences(entity).find_map(|(_, system)| self.systems[system].currency);
        own.or_else(residence).unwrap_or(self.base)
    }

    /// How a commodity grows, a year at a time, where it says.
    pub fn growth(&self, unit: Id<Commodity>) -> Option<Ratio> {
        self.fact(builtin::GROWS, unit)
    }

    /// Whether the gains of a place are not realized inside it.
    pub fn is_deferred(&self, place: Id<Place>) -> bool {
        self.fact(builtin::DEFERRED, place).unwrap_or(false)
    }

    /// What basis value arriving in a place takes: what its kinds say, else nothing if it is deferred, else its cost.
    pub fn basis(&self, place: Id<Place>) -> Basis {
        let said = self.fact(builtin::BASIS, place).and_then(Basis::decode);
        said.unwrap_or(if self.is_deferred(place) { Basis::Zero } else { Basis::Cost })
    }

    /// Whether a place holds what others owe, so that its parcels stay apart by the transaction that made them: what
    /// its kinds say, and every tab.
    pub fn is_claim(&self, place: Id<Place>) -> bool {
        self.fact(builtin::CLAIM, place).unwrap_or(false) || matches!(self.places[place].role, Role::Tab(_))
    }

    fn said_of(&self, holder: u32, name: Sym, day: Day) -> Option<Value> {
        let slot = self.schema.number(name)?;
        let datum = self.facts.datum_at(slot, holder, day)?;
        Value::of(datum, self.schema.ty(slot))
    }
}
