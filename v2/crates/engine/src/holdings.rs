//! Where value rests: one [`Holding`] per `(place, commodity)`.
//!
//! A place holds a handful of commodities, usually one or two. Each place
//! therefore threads a short chain of slots, sorted by commodity, through one
//! flat vector. A lookup is one index and a hop or two, with no hashing;
//! iteration by place then commodity is the chain order; a new pair costs one
//! push; and cloning is two flat copies (a holding without lots owns no heap).

use std::iter::successors;
use std::ops::Range;

use axiom_core::{Day, Id, Qty};
use axiom_model::{Commodity, Entity, Place, Txn};

use crate::{Holding, Parcel};

const NONE: u32 = u32::MAX;

#[derive(Clone)]
pub(crate) struct Holdings {
    /// The first slot of each place's chain.
    heads: Vec<u32>,
    slots: Vec<Slot>,
}

#[derive(Clone)]
struct Slot {
    holding: Holding,
    next: u32,
}

impl Holdings {
    pub fn new(places: usize) -> Holdings {
        Holdings { heads: vec![NONE; places], slots: Vec::new() }
    }

    fn chain(&self, head: u32) -> impl Iterator<Item = &Holding> {
        let next = |&at: &u32| Some(self.slots[at as usize].next).filter(|&n| n != NONE);
        successors(Some(head).filter(|&h| h != NONE), next).map(|at| &self.slots[at as usize].holding)
    }

    pub fn get(&self, place: Id<Place>, unit: Id<Commodity>) -> Option<&Holding> {
        self.chain(self.heads[place.index()]).find(|h| h.unit >= unit).filter(|h| h.unit == unit)
    }

    /// The holding, created empty if the place has never held `unit`.
    pub fn entry(&mut self, place: Id<Place>, unit: Id<Commodity>) -> &mut Holding {
        let (mut before, mut at) = (NONE, self.heads[place.index()]);
        while at != NONE && self.slots[at as usize].holding.unit < unit {
            (before, at) = (at, self.slots[at as usize].next);
        }
        if at == NONE || self.slots[at as usize].holding.unit != unit {
            let new = self.slots.len() as u32;
            let holding = Holding { place, unit, plain: Qty::ZERO, lots: Vec::new() };
            self.slots.push(Slot { holding, next: at });
            match before {
                NONE => self.heads[place.index()] = new,
                _ => self.slots[before as usize].next = new,
            }
            at = new;
        }
        &mut self.slots[at as usize].holding
    }

    pub fn qty(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        self.get(place, unit).map_or(Qty::ZERO, Holding::qty)
    }

    /// Adds (or, negative, removes) plain value.
    pub fn credit(&mut self, place: Id<Place>, unit: Id<Commodity>, qty: Qty) {
        self.entry(place, unit).plain += qty;
    }

    /// Every holding by place, then commodity, empty ones included.
    pub fn iter(&self) -> impl Iterator<Item = &Holding> {
        self.within(0..self.heads.len())
    }

    /// The holdings of the places whose ids lie in `places`: a subtree.
    pub fn within(&self, places: Range<usize>) -> impl Iterator<Item = &Holding> {
        self.heads[places].iter().flat_map(|&head| self.chain(head))
    }

    /// The non-empty holdings, by place then commodity.
    pub fn into_sorted(self) -> Vec<Holding> {
        let mut all: Vec<Holding> = self.slots.into_iter().map(|slot| slot.holding).filter(|h| !h.is_empty()).collect();
        all.sort_unstable_by_key(|h| (h.place, h.unit));
        all
    }
}

/// What makes two parcels interchangeable. Parcels merge exactly when their
/// identities are equal, and relief between candidates with equal identities is
/// never ambiguous: taking any of them is the same as taking any other.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Identity {
    /// Base currency: what matters is who it is tied to and how much of each
    /// unit is already accounted for. Where and when it arrived does not
    /// matter, so a 401k's hundreds of zero-basis deferrals are one lot.
    Money { tied: Option<Id<Entity>>, basis: Qty, qty: Qty },
    /// Anything else: each purchase is its own lot, for selectors and for how
    /// long it has been held.
    Lot { acquired: Day, txn: Id<Txn>, tied: Option<Id<Entity>> },
}

impl PartialEq for Identity {
    fn eq(&self, other: &Identity) -> bool {
        match (*self, *other) {
            (Identity::Money { tied: a, basis: ab, qty: aq }, Identity::Money { tied: b, basis: bb, qty: bq }) => {
                // Basis per unit, compared exactly: ab/aq == bb/bq.
                a == b && ab.0 as i128 * bq.0 as i128 == bb.0 as i128 * aq.0 as i128
            }
            (Identity::Lot { acquired: a, txn: at, tied: ap }, Identity::Lot { acquired: b, txn: bt, tied: bp }) => {
                (a, at, ap) == (b, bt, bp)
            }
            _ => false,
        }
    }
}

pub(crate) fn identity(parcel: &Parcel, is_base: bool) -> Identity {
    if is_base {
        Identity::Money { tied: parcel.tied, basis: parcel.basis, qty: parcel.qty }
    } else {
        Identity::Lot { acquired: parcel.acquired, txn: parcel.txn, tied: parcel.tied }
    }
}

impl Holding {
    /// Receives a parcel. Base money at its face, tied to nothing, is plain;
    /// anything else joins the lots, merging into the interchangeable lot if
    /// there is one (its basis adds; it keeps its own acquisition day) and
    /// otherwise taking its place among the lots, oldest first.
    pub fn land(&mut self, parcel: Parcel, is_base: bool) {
        if is_base && parcel.tied.is_none() && parcel.basis == parcel.qty {
            self.plain += parcel.qty;
            return;
        }
        let kind = identity(&parcel, is_base);
        // Outside the base only a lot acquired on the same day can match.
        let (start, end) = match is_base {
            true => (0, self.lots.len()),
            false => (
                self.lots.partition_point(|lot| lot.acquired < parcel.acquired),
                self.lots.partition_point(|lot| lot.acquired <= parcel.acquired),
            ),
        };
        match self.lots[start..end].iter_mut().find(|lot| identity(lot, is_base) == kind) {
            Some(lot) => {
                lot.qty += parcel.qty;
                lot.basis += parcel.basis;
            }
            None => {
                let at = self.lots.partition_point(|lot| lot.acquired <= parcel.acquired);
                self.lots.insert(at, parcel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;

    use super::*;

    fn parcel(qty: i64, basis: i64, acquired: i32, txn: u32) -> Parcel {
        Parcel { qty: Qty(qty), basis: Qty(basis), acquired: Day(acquired), txn: Id::new(txn), tied: None }
    }

    #[test]
    fn lots_merge_by_identity_and_stay_oldest_first() {
        let mut held = Holdings::new(2);
        let (place, unit) = (Id::new(1), Id::new(3));
        let holding = held.entry(place, unit);
        holding.land(parcel(5, 50, 20, 1), false);
        holding.land(parcel(2, 10, 10, 2), false);
        holding.land(parcel(3, 30, 20, 1), false);
        holding.land(parcel(1, 10, 20, 9), false);
        let lots: Vec<_> = holding.lots.iter().map(|l| (l.acquired.0, l.qty.0, l.basis.0)).collect();
        assert_eq!(lots, [(10, 2, 10), (20, 8, 80), (20, 1, 10)]);
        assert_eq!(held.qty(place, unit), Qty(11));
        assert_eq!(held.qty(Id::new(0), unit), Qty::ZERO);
    }

    #[test]
    fn base_lots_merge_when_tie_and_basis_per_unit_agree() {
        let mut holding = Holding { place: Id::new(0), unit: Id::new(0), plain: Qty::ZERO, lots: Vec::new() };
        let entity = Id::new(9);
        for (qty, basis, acquired, tied) in [
            (100, 0, 5, None),
            (250, 0, 900, None),
            (10, 5, 6, None),
            (20, 10, 7, None),
            (30, 30, 8, Some(entity)),
            (5, 5, 9, Some(entity)),
        ] {
            holding.land(Parcel { tied, ..parcel(qty, basis, acquired, acquired as u32) }, true);
        }
        let lots: Vec<_> = holding.lots.iter().map(|l| (l.qty.0, l.basis.0, l.acquired.0, l.tied.is_some())).collect();
        // Zero-basis money is one lot, half-basis money another, tied money a third; each keeps its first day.
        assert_eq!(lots, [(350, 0, 5, false), (30, 15, 6, false), (35, 35, 8, true)]);
    }

    #[test]
    fn plain_money_never_allocates_and_iteration_is_ordered() {
        let mut held = Holdings::new(3);
        held.credit(Id::new(2), Id::new(1), Qty(7));
        held.credit(Id::new(0), Id::new(5), Qty(1));
        held.credit(Id::new(0), Id::new(2), Qty(1));
        held.entry(Id::new(0), Id::new(2)).land(parcel(4, 4, 0, 0), true);
        let order: Vec<_> =
            held.iter().map(|h| (h.place.index(), h.unit.index(), h.plain.0, h.lots.capacity())).collect();
        assert_eq!(order, [(0, 2, 5, 0), (0, 5, 1, 0), (2, 1, 7, 0)]);
    }
}
