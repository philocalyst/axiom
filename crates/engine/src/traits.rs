//! What the places, entities and commodities say that the fold asks, resolved once.
//!
//! The fold asks of a flow's two places whether they defer gains, what basis they take, whether they hold claims, and
//! how they relieve parcels, and of its owner whom it belongs to and whether its money is tied; the model keeps these
//! as facts, found through the kinds of a thing. A fold that went to the facts for each would make a random read of a
//! large store for every flow, so the plan resolves them once into a dense array by place (four bytes each), one by
//! entity and one by commodity, which the fold then reads as it reads a place's class.

use axiom_core::Id;
use axiom_model::{Basis, Book, Books, Class, Commodity, Entity, Place, Policy, Role};

/// What the fold asks of a place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PlaceTraits {
    /// How the place relieves parcels: what it says, and for a claim place `Exact` where it says nothing.
    pub select: Option<Policy>,
    /// What basis value arriving takes.
    pub basis: Basis,
    /// Gains are not realized inside.
    pub deferred: bool,
    /// What others owe is held here, and its parcels stay apart.
    pub claim: bool,
}

const _: () = assert!(size_of::<PlaceTraits>() == 4);

/// What the fold asks of an entity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EntityTraits {
    /// The household it belongs to.
    pub member: Option<Id<Entity>>,
    /// Its money stays tied to it.
    pub restricted: bool,
    /// What it counts in.
    pub currency: Id<Commodity>,
    /// When its claims count as income or spending.
    pub books: Books,
}

/// The traits of every place and entity, and how each commodity relieves parcels.
pub(crate) struct Traits {
    places: Box<[PlaceTraits]>,
    entities: Box<[EntityTraits]>,
    units: Box<[Option<Policy>]>,
    /// The tab that holds what a party owes an owner, by the party's place and the owner, sorted: a flow out of a party's
    /// place finds the claims it may settle by a search over the few owners the party owes.
    claims: Box<[(Id<Place>, Id<Entity>, Id<Place>)]>,
}

impl Traits {
    pub fn of(book: &Book) -> Traits {
        let places = book.places.ids().map(|place| {
            // A claim is settled by the exact amount, then the oldest, unless its place says another way.
            let claim = book.is_claim(place);
            let select = book.select(place).or(claim.then_some(Policy::Exact));
            PlaceTraits { select, basis: book.basis(place), deferred: book.is_deferred(place), claim }
        });
        let entities = book.entities.ids().map(|entity| EntityTraits {
            member: book.member(entity),
            restricted: book.is_restricted(entity),
            currency: book.currency(entity),
            books: book.books(entity),
        });
        let units = book.commodities.ids().map(|unit| book.select(unit));
        let owed = book.places.iter().filter(|(_, tab)| tab.class == Class::Asset);
        let mut claims: Vec<_> = owed
            .filter_map(|(id, tab)| match tab.role {
                Role::Tab(party) => Some((book.entities[party].place?, tab.owner, id)),
                _ => None,
            })
            .collect();
        claims.sort_unstable();
        Traits {
            places: places.collect(),
            entities: entities.collect(),
            units: units.collect(),
            claims: claims.into_boxed_slice(),
        }
    }

    /// Whether the party whose place this is owes any owner anything on record: a flow out of a place that does not has
    /// no claim to settle, and nothing about its statement is looked up.
    pub fn owes(&self, party: Id<Place>) -> bool {
        let at = self.claims.partition_point(|&(found, ..)| found < party);
        self.claims.get(at).is_some_and(|&(found, ..)| found == party)
    }

    /// The tab that holds what the party whose place this is owes `owner`, if the party owes it anything on record.
    pub fn tab_of(&self, party: Id<Place>, owner: Id<Entity>) -> Option<Id<Place>> {
        let at = self.claims.partition_point(|&(found, by, _)| (found, by) < (party, owner));
        self.claims.get(at).filter(|&&(found, by, _)| (found, by) == (party, owner)).map(|&(_, _, tab)| tab)
    }

    pub fn entity(&self, entity: Id<Entity>) -> EntityTraits {
        self.entities[entity.index()]
    }

    pub fn place(&self, place: Id<Place>) -> PlaceTraits {
        self.places[place.index()]
    }

    /// How a commodity relieves parcels where neither the flow nor the place says.
    pub fn unit_select(&self, unit: Id<Commodity>) -> Option<Policy> {
        self.units[unit.index()]
    }
}
