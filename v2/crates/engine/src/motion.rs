//! A flow as the fold sees it: oriented, with its quantities solved, on the
//! day it takes effect.
//!
//! Journal flows, applied flows, reversals (a returned deposit runs its flow
//! backwards) and the flows an assertion posts to close a gap all become a
//! `Motion`, so exactly one code path moves value.

use axiom_core::{Day, Id, Loc, Qty, Sym};
use axiom_model::{
    Amount, Assert, Book, Class, End, Entity, Flow, Mode, Place, Recognition, Select, Terms, Txn, Waive,
};

use crate::Cause;

/// What leaves and what arrives, once every `?`, `=` and `all` is solved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Amounts {
    pub out: Qty,
    pub arrive: Qty,
}

impl Amounts {
    /// The quantities as written (zero where the source said `?`).
    pub fn written(flow: &Flow) -> Amounts {
        Amounts { out: flow.out.qty, arrive: flow.arrive.qty }
    }
}

/// What a flow does to the parcels it touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moves {
    /// Value crosses from one place to another: relief, realization, arrival.
    Value,
    /// A market raises what an asset is worth: the growth arrives with no basis.
    Growth,
    /// A market lowers it: parcels shrink and leave their basis behind, an
    /// unrealized loss. Nothing is realized, and nothing is spent.
    Loss,
    /// `PLACE.basis` at this end: the parcels there change basis, not quantity.
    Basis(End),
}

impl Moves {
    fn of(book: &Book, terms: &Terms, source: &Place, target: &Place) -> Moves {
        let market = |place: &Place| book.is_a(place.kind, book.roots.market);
        match terms.basis_end {
            Some(end) => Moves::Basis(end),
            None if target.class == Class::Asset && source.class == Class::Income && market(source) => Moves::Growth,
            None if source.class == Class::Asset && target.class == Class::Income && market(target) => Moves::Loss,
            None => Moves::Value,
        }
    }

    fn reversed(self) -> Moves {
        match self {
            Moves::Growth => Moves::Loss,
            Moves::Loss => Moves::Growth,
            Moves::Basis(End::From) => Moves::Basis(End::To),
            Moves::Basis(End::To) => Moves::Basis(End::From),
            Moves::Value => Moves::Value,
        }
    }
}

pub(crate) struct Motion<'f> {
    pub cause: Cause,
    pub day: Day,
    pub recognized: Recognition,
    pub from: Id<Place>,
    pub to: Id<Place>,
    /// The places at the two ends, so nobody looks them up again.
    pub source: &'f Place,
    pub target: &'f Place,
    pub out: Amount,
    pub arrive: Amount,
    pub txn: Id<Txn>,
    pub payee: Option<Id<Entity>>,
    pub select: &'f [Select],
    pub codes: &'f [Sym],
    pub terms: &'f Terms,
    pub moves: Moves,
    /// An `opening` line: value moves, but no law sees it and no total counts it.
    pub opening: bool,
    pub waive: Option<Waive>,
    pub loc: Loc,
}

impl<'f> Motion<'f> {
    pub fn new(book: &'f Book, flow: &'f Flow, cause: Cause, day: Day, amounts: Amounts) -> Motion<'f> {
        let (source, target) = (&book.places[flow.from], &book.places[flow.to]);
        Motion {
            cause,
            day,
            recognized: flow.recognized,
            from: flow.from,
            to: flow.to,
            source,
            target,
            out: Amount::new(amounts.out, flow.out.unit),
            arrive: Amount::new(amounts.arrive, flow.arrive.unit),
            txn: flow.txn,
            payee: flow.payee,
            select: &flow.select,
            codes: &flow.codes,
            terms: flow.terms(),
            moves: Moves::of(book, flow.terms(), source, target),
            opening: flow.mode == Mode::Opening,
            waive: flow.waive,
            loc: flow.loc,
        }
    }

    /// What an assertion posts to close a gap: `moved` arrives at the asserted
    /// place from `counter`, or, negative, leaves it for `counter`. The parcels
    /// belong to the transaction that last touched the place.
    pub fn pad(book: &'f Book, assert: &Assert, counter: Id<Place>, moved: Qty, waive: Option<Waive>) -> Motion<'f> {
        let (from, to) = if moved.is_negative() { (assert.place, counter) } else { (counter, assert.place) };
        let (source, target) = (&book.places[from], &book.places[to]);
        let amount = Amount::new(moved.abs(), assert.amount.unit);
        let touching = &book.touching[assert.place];
        let last = touching.partition_point(|&id| book.flows[id].day <= assert.day);
        let txn = last.checked_sub(1).map_or(Id::new(0), |at| book.flows[touching[at]].txn);
        Motion {
            cause: Cause::Time,
            day: assert.day,
            recognized: Recognition::on(assert.day),
            from,
            to,
            source,
            target,
            out: amount,
            arrive: amount,
            txn,
            payee: None,
            select: &[],
            codes: &[],
            terms: &Terms::NONE,
            moves: Moves::of(book, &Terms::NONE, source, target),
            opening: false,
            waive,
            loc: assert.loc,
        }
    }

    /// The same value moving back: what arrived leaves, and comes home. The
    /// original's lot selectors chose parcels at the other end and mean
    /// nothing here.
    pub fn reversed(&self) -> Motion<'f> {
        let (from, to) = (self.to, self.from);
        let (source, target) = (self.target, self.source);
        Motion { from, to, source, target, out: self.arrive, arrive: self.out, select: &[], moves: self.moves.reversed(), ..*self }
    }

    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }
}
