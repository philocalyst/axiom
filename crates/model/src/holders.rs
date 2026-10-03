//! The things a book says things about, numbered.
//!
//! The model keeps a thing of each sort in an arena of its own: kinds, entities, commodities, assets and places. The
//! facts store wants one dense number for every holder of a fact, so the arenas are laid end to end. A thing's number
//! is its arena's offset plus its id, and a number goes back to the sort and the id by a search among six offsets.
//! Nothing is stored per thing. (K3 merges the arenas, and this becomes the identity.)
//!
//! The places are last because they are the one arena that grows after the numbering is made: a claim tab is made
//! the first time a claim asks for it, while the journal is lowered. Growing the last arena renumbers nothing.
//!
//! Kinds are holders too: what a kind says is the default of its things.

use axiom_core::Id;

use crate::book::{Asset, Commodity, Entity, Kind, Place};

/// Something a book says things about, as the arena it is in and its id there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Holder {
    Kind(Id<Kind>),
    Place(Id<Place>),
    Entity(Id<Entity>),
    Commodity(Id<Commodity>),
    Asset(Id<Asset>),
}

const _: () = assert!(size_of::<Holder>() == 8);

macro_rules! holders_from {
    ($($ty:ident),+) => {$(
        impl From<Id<$ty>> for Holder {
            fn from(id: Id<$ty>) -> Holder {
                Holder::$ty(id)
            }
        }
    )+};
}
holders_from!(Kind, Place, Entity, Commodity, Asset);

/// How many arenas the numbering lays end to end.
const SORTS: usize = 5;

/// Where each arena begins in the numbering, and where the last ends: kinds, entities, commodities, assets, places.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HolderIndex {
    starts: [u32; SORTS + 1],
}

impl HolderIndex {
    /// The numbering of arenas of these sizes.
    pub fn new(kinds: usize, places: usize, entities: usize, commodities: usize, assets: usize) -> HolderIndex {
        let mut starts = [0; SORTS + 1];
        for (at, size) in [kinds, entities, commodities, assets, places].into_iter().enumerate() {
            starts[at + 1] = starts[at] + u32::try_from(size).expect("fewer than 2^32 things");
        }
        HolderIndex { starts }
    }

    /// One more place, numbered after every thing there is.
    pub fn add_place(&mut self) {
        self.starts[SORTS] = self.starts[SORTS].checked_add(1).expect("fewer than 2^32 things");
    }

    /// How many holders there are.
    pub fn len(&self) -> usize {
        self.starts[SORTS] as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of a thing.
    pub fn number(&self, thing: impl Into<Holder>) -> u32 {
        let (sort, id) = match thing.into() {
            Holder::Kind(id) => (0, id.index()),
            Holder::Entity(id) => (1, id.index()),
            Holder::Commodity(id) => (2, id.index()),
            Holder::Asset(id) => (3, id.index()),
            Holder::Place(id) => (4, id.index()),
        };
        debug_assert!(self.starts[sort] as usize + id < self.starts[sort + 1] as usize, "a thing of this numbering");
        self.starts[sort] + id as u32
    }

    /// The thing a number is of.
    pub fn holder(&self, number: u32) -> Holder {
        assert!((number as usize) < self.len(), "holder {number} of {}", self.len());
        let sort = self.starts[1..].partition_point(|&end| end <= number);
        let id = number - self.starts[sort];
        match sort {
            0 => Holder::Kind(Id::new(id)),
            1 => Holder::Entity(Id::new(id)),
            2 => Holder::Commodity(Id::new(id)),
            3 => Holder::Asset(Id::new(id)),
            _ => Holder::Place(Id::new(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_thing_has_a_number_and_a_number_gives_the_thing_back() {
        let index = HolderIndex::new(3, 2, 4, 2, 1);
        assert_eq!(index.len(), 12);
        let things: Vec<Holder> = (0..3)
            .map(|id| Holder::Kind(Id::new(id)))
            .chain((0..4).map(|id| Holder::Entity(Id::new(id))))
            .chain((0..2).map(|id| Holder::Commodity(Id::new(id))))
            .chain([Holder::Asset(Id::new(0))])
            .chain((0..2).map(|id| Holder::Place(Id::new(id))))
            .collect();
        let numbers: Vec<u32> = things.iter().map(|&thing| index.number(thing)).collect();
        assert_eq!(numbers, (0..12).collect::<Vec<_>>(), "arenas end to end, places last");
        assert!(numbers.iter().all(|&number| things[number as usize] == index.holder(number)));
    }

    #[test]
    fn an_id_converts_to_the_holder_of_its_sort() {
        let index = HolderIndex::new(2, 5, 3, 0, 0);
        let place: Id<Place> = Id::new(4);
        assert_eq!(index.number(place), 2 + 3 + 4);
        assert_eq!(index.holder(index.number(place)), Holder::Place(place));
        assert_eq!(index.number(Id::<Entity>::new(0)), 2);
        assert!(HolderIndex::new(0, 0, 0, 0, 0).is_empty());
    }

    #[test]
    fn a_place_made_later_renumbers_nothing() {
        let mut index = HolderIndex::new(2, 3, 4, 1, 1);
        let numbers = |index: &HolderIndex| {
            let things = [
                Holder::Kind(Id::new(1)),
                Holder::Entity(Id::new(3)),
                Holder::Commodity(Id::new(0)),
                Holder::Asset(Id::new(0)),
                Holder::Place(Id::new(2)),
            ];
            things.map(|thing| index.number(thing))
        };
        let before = numbers(&index);

        index.add_place();

        assert_eq!(numbers(&index), before);
        assert_eq!(index.len(), 11 + 1);
        let tab = Holder::Place(Id::new(3));
        assert_eq!(index.number(tab), 11);
        assert_eq!(index.holder(11), tab);
    }

    #[test]
    #[should_panic(expected = "holder 5 of 5")]
    fn a_number_past_the_last_thing_is_no_holder() {
        HolderIndex::new(2, 2, 1, 0, 0).holder(5);
    }
}
