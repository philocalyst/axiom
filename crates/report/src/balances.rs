//! What every place held at the end of each of several days, for one owner scope: a read of the run's histories.
//!
//! The fold records a position's balance at every step it changes (`axiom_engine::Histories`), so a day's balance is a
//! binary search and there is nothing to replay: a view of forty days is forty sweeps of the positions, however long
//! the journal is. The balances are read once, scaled to the view's owners (a share of a joint account is rounded once,
//! on the whole balance, not on each movement that made it), and laid out one column a day beside one another, in the
//! order of the positions, so that the places beneath one place, which are one run of positions, are one slice.

use axiom_core::{Day, Id, Qty};
use axiom_engine::{Histories, Position};
use axiom_model::{Book, Place};

use crate::view::{Basket, View};

/// What each position held on each of some days, as the view's owners hold it.
pub struct Balances<'r> {
    histories: &'r Histories,
    days: Vec<Day>,
    /// One column for each day, a cell for each position of `histories`.
    cells: Vec<Qty>,
}

impl<'r> Balances<'r> {
    /// The books on `days`, which are in ascending order.
    pub fn of(view: View<'_, '_, 'r>, days: &[Day]) -> Balances<'r> {
        let histories = &view.run.histories;
        let held =
            |day: Day| histories.positions().map(move |(id, at)| view.place_qty(at.place, histories.at(id, day)));
        Balances { histories, days: days.to_vec(), cells: days.iter().flat_map(|&day| held(day)).collect() }
    }

    pub fn days(&self) -> &[Day] {
        &self.days
    }

    /// What `place` and everything beneath it held at the end of `days()[column]`, a quantity of each commodity.
    pub fn subtree(&self, book: &Book, column: usize, place: Id<Place>) -> Basket {
        let positions = self.histories.beneath(place, book.places.end(place));
        let column = &self.cells[column * self.histories.len()..][..self.histories.len()];
        let mut basket = Basket::default();
        for id in positions.ids() {
            let Position { unit, .. } = self.histories.position(id);
            basket.add(unit, column[id.index()]);
        }
        basket
    }
}
