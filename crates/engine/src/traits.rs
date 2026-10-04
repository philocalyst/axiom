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

/// A tab the owner keeps with a party, found by the party's place, the owner and the class of the tab: an `Asset` tab holds
/// what the party owes the owner, a `Debt` tab what the owner owes the party.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Tab {
    key: (Id<Place>, Id<Entity>, Class),
    place: Id<Place>,
}

/// The traits of every place and entity, and how each commodity relieves parcels.
pub(crate) struct Traits {
    places: Box<[PlaceTraits]>,
    entities: Box<[EntityTraits]>,
    units: Box<[Option<Policy>]>,
    /// Every tab that says `claim`, sorted: a flow out of or into a party's place finds the claims it may settle by a search over
    /// the few owners the party has a tab with.
    claims: Box<[Tab]>,
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
        let mut claims: Vec<_> = book
            .places
            .iter()
            .filter_map(|(id, tab)| match tab.role {
                Role::Tab(party) if book.is_claim(id) => {
                    Some(Tab { key: (book.entities[party].place?, tab.owner, tab.class), place: id })
                }
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

    /// Whether the party whose place this is has a tab with any owner: a flow out of or into a place that has none has no claim
    /// to settle, and nothing about its statement is looked up.
    pub fn has_tab(&self, party: Id<Place>) -> bool {
        let at = self.claims.partition_point(|tab| tab.key.0 < party);
        self.claims.get(at).is_some_and(|tab| tab.key.0 == party)
    }

    /// The tab of the given class that holds what the party whose place this is owes `owner` (`Asset`) or `owner` owes it
    /// (`Debt`), if the two have one on record.
    pub fn tab_of(&self, party: Id<Place>, owner: Id<Entity>, class: Class) -> Option<Id<Place>> {
        let key = (party, owner, class);
        let at = self.claims.partition_point(|tab| tab.key < key);
        self.claims.get(at).filter(|tab| tab.key == key).map(|tab| tab.place)
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
