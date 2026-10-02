//! What the places, entities and commodities say that the fold asks, resolved once.
//!
//! The fold asks of a flow's two places whether they defer gains, what basis they take, whether they hold claims, and
//! how they relieve parcels, and of its owner whom it belongs to and whether its money is tied; the model keeps these
//! as facts, found through the kinds of a thing. A fold that went to the facts for each would make a random read of a
//! large store for every flow, so the plan resolves them once into a dense array by place (four bytes each), one by
//! entity and one by commodity, which the fold then reads as it reads a place's class.

use axiom_core::Id;
use axiom_model::{Basis, Book, Commodity, Entity, Place, Policy};

/// What the fold asks of a place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PlaceTraits {
    /// How the place relieves parcels, where it says.
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
}

/// The traits of every place and entity, and how each commodity relieves parcels.
pub(crate) struct Traits {
    places: Box<[PlaceTraits]>,
    entities: Box<[EntityTraits]>,
    units: Box<[Option<Policy>]>,
}

impl Traits {
    pub fn of(book: &Book) -> Traits {
        let places = book.places.ids().map(|place| PlaceTraits {
            select: book.select(place),
            basis: book.basis(place),
            deferred: book.is_deferred(place),
            claim: book.is_claim(place),
        });
        let entities = book.entities.ids().map(|entity| EntityTraits {
            member: book.member(entity),
            restricted: book.is_restricted(entity),
            currency: book.currency(entity),
        });
        let units = book.commodities.ids().map(|unit| book.select(unit));
        Traits { places: places.collect(), entities: entities.collect(), units: units.collect() }
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
