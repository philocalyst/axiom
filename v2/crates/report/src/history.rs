//! What happened, as posted: the journal's flows with their solved quantities
//! and settlement, and the balances they add up to on any day.
//!
//! Historical views read the run's `posted` list instead of replaying the
//! engine, so any past day costs one pass over the flows.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty};
use axiom_engine::{Posted, Run};
use axiom_model::{Amount, Book, Commodity, Flow, Place};

use crate::value::{Basket, Valuer};

/// A journal flow together with what the run made of it.
#[derive(Clone, Copy)]
pub struct Posting<'a> {
    pub id: Id<Flow>,
    pub flow: &'a Flow,
    pub posted: &'a Posted,
}

impl<'a> Posting<'a> {
    pub fn at(book: &'a Book, run: &'a Run, id: Id<Flow>) -> Posting<'a> {
        Posting { id, flow: &book.flows[id], posted: &run.posted[id.index()] }
    }

    /// What left `flow.from`, as solved.
    pub fn out(&self) -> Amount {
        Amount::new(self.posted.out, self.flow.out.unit)
    }

    /// What reached `flow.to`, as solved.
    pub fn arrive(&self) -> Amount {
        Amount::new(self.posted.arrive, self.flow.arrive.unit)
    }

    /// Whether the flow has happened, and stands, at the end of `day`.
    pub fn is_real_on(&self, day: Day) -> bool {
        self.flow.day <= day && self.posted.state.is_real_on(day)
    }

    /// Whether the flow is written but not yet real at the end of `day`.
    pub fn is_pending_on(&self, day: Day) -> bool {
        self.flow.day <= day && self.posted.state.is_pending_on(day)
    }

    /// The other end of the flow, seen from `place`.
    pub fn counterparty(&self, place: Id<Place>) -> Id<Place> {
        if self.flow.from == place { self.flow.to } else { self.flow.from }
    }

    /// What `place` gained (positive) or lost (negative) in this flow.
    pub fn change_at(&self, place: Id<Place>) -> Option<Amount> {
        let (gain, loss) = (self.flow.to == place, self.flow.from == place);
        match (gain, loss) {
            (true, false) => Some(self.arrive()),
            (false, true) => Some(negated(self.out())),
            _ => None,
        }
    }

    /// The out side priced on the day the flow happened.
    pub fn out_in_base(&self, book: &Book) -> Option<Qty> {
        Valuer::new(book, self.flow.day).qty(self.out())
    }

    /// The arrival priced on the day the flow happened.
    pub fn arrive_in_base(&self, book: &Book) -> Option<Qty> {
        Valuer::new(book, self.flow.day).qty(self.arrive())
    }
}

fn negated(amount: Amount) -> Amount {
    Amount::new(-amount.qty, amount.unit)
}

/// Every journal flow with its posting, in journal order.
pub fn postings<'a>(book: &'a Book, run: &'a Run) -> impl Iterator<Item = Posting<'a>> {
    book.flows.iter().zip(run.posted.iter()).map(|((id, flow), posted)| Posting { id, flow, posted })
}

/// What every place held on one day.
///
/// Keyed by place then commodity, and places are numbered in pre-order, so a
/// subtree's holdings are one contiguous key range.
pub struct Balances {
    held: BTreeMap<(Id<Place>, Id<Commodity>), Qty>,
}

impl Balances {
    /// Sums the flows that are real at the end of `day`, and the pads that
    /// accepted unexplained gaps by then.
    pub fn at(book: &Book, run: &Run, day: Day) -> Balances {
        let mut held: BTreeMap<(Id<Place>, Id<Commodity>), Qty> = BTreeMap::new();
        for posting in postings(book, run).filter(|posting| posting.is_real_on(day)) {
            *held.entry((posting.flow.from, posting.flow.out.unit)).or_default() -= posting.posted.out;
            *held.entry((posting.flow.to, posting.flow.arrive.unit)).or_default() += posting.posted.arrive;
        }
        // A pad is a flow from `unknown`: the gap it accepted, and its mirror.
        for pad in run.pads.iter().filter(|pad| pad.day <= day) {
            *held.entry((pad.place, pad.amount.unit)).or_default() += pad.amount.qty;
            *held.entry((book.roots.unknown, pad.amount.unit)).or_default() -= pad.amount.qty;
        }
        held.retain(|_, qty| !qty.is_zero());
        Balances { held }
    }

    /// Every non-zero holding, by place then commodity.
    pub fn holdings(&self) -> impl Iterator<Item = (Id<Place>, Amount)> + '_ {
        self.held.iter().map(|(&(place, unit), &qty)| (place, Amount::new(qty, unit)))
    }

    /// What `place` and everything beneath it hold, by commodity.
    pub fn subtree(&self, book: &Book, place: Id<Place>) -> Basket {
        let first = (place, Id::new(0));
        let past = (book.places.end(place), Id::new(0));
        self.held.range(first..past).map(|(&(_, unit), &qty)| Amount::new(qty, unit)).collect()
    }
}
