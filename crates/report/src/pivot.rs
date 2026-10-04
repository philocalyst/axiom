//! What the postings count, as a table with a row for whatever a view groups by and a cell for each period.
//!
//! Income, spending and capital are one table built three ways: group what the postings count by purpose (and by what the
//! purpose is of, and by what had none), or by the party the money came from or went to, and spread each amount over the
//! periods it is recognized in. What each posting counts is `recognition`'s rule, asked once by `flow::for_each_counted`;
//! this module keeps the rows. So a view says what its rows are (a key type of its own) and reads them back, and the
//! walk, the pricing, the spreading and the counts of what could not be priced are written once.
//!
//! A row is a slice of one flat grid, found by its key in a map, and rows are numbered in the order they were first
//! counted: a table of a thousand parties is two allocations, not a thousand, and a view that wants its rows in the order
//! they appeared reads [`Pivot::keys`].

use std::hash::Hash;

use axiom_core::{Day, Days, Map, Qty, spread};

use crate::calendar::Periods;
use crate::flow::Counted;

/// A table of quantities with a row of one value per period for each of whatever is being tallied: one flat vector, so
/// that a row costs no allocation of its own.
pub(crate) struct Grid {
    width: usize,
    cells: Vec<Qty>,
}

impl Grid {
    pub fn new(width: usize) -> Grid {
        Grid { width, cells: Vec::new() }
    }

    /// Adds a row of zeros, and says which it is.
    pub fn push(&mut self) -> usize {
        self.cells.resize(self.cells.len() + self.width, Qty::ZERO);
        self.cells.len() / self.width.max(1) - 1
    }

    pub fn row(&self, at: usize) -> &[Qty] {
        &self.cells[at * self.width..][..self.width]
    }

    pub fn row_mut(&mut self, at: usize) -> &mut [Qty] {
        &mut self.cells[at * self.width..][..self.width]
    }

    /// Adds the row `from` into the row `into`.
    pub fn add_row(&mut self, from: usize, into: usize) {
        for period in 0..self.width {
            let value = self.cells[from * self.width + period];
            self.cells[into * self.width + period] += value;
        }
    }
}

/// What the postings counted, by key and by period.
pub(crate) struct Pivot<K> {
    periods: Periods,
    rows: Map<K, usize>,
    keys: Vec<K>,
    grid: Grid,
    /// Whether anything counted in the row moved something in the periods.
    moved: Vec<bool>,
    /// How many counted amounts had no price on their day.
    pub unpriced: usize,
    /// Whether some posting is recognized over a range of days, not on one.
    pub spread: bool,
}

impl<K: Copy + Eq + Hash> Pivot<K> {
    pub fn new(periods: Periods) -> Pivot<K> {
        Pivot {
            periods,
            rows: Map::default(),
            keys: Vec::new(),
            grid: Grid::new(periods.len()),
            moved: Vec::new(),
            unpriced: 0,
            spread: false,
        }
    }

    /// What `counted` is worth, if it can be priced: a posting that cannot is counted as unpriced, and none of its rows move.
    pub fn price(&mut self, counted: &Counted<'_>) -> Option<Qty> {
        self.spread |= counted.recognized.last() > counted.day;
        self.unpriced += usize::from(counted.amount.is_none());
        counted.amount
    }

    /// Adds `amount`, which `counted` is worth, to the row of `key`, spread over the periods it is recognized in as far as
    /// `cutoff`. The row is made when the first part of an amount reaches it, and moves if the amount does.
    pub fn add(&mut self, key: K, counted: &Counted<'_>, amount: Qty, cutoff: Day) {
        let (periods, recognized) = (self.periods, counted.recognized);
        let moved = !amount.is_zero() && periods.overlapping(recognized.first(), recognized.last()).next().is_some();
        let mut row = None;
        spread_over(periods, recognized, cutoff, amount, |period, part| {
            let row = *row.get_or_insert_with(|| self.row_of(key));
            self.moved[row] |= moved;
            self.grid.row_mut(row)[period] += part;
        });
    }

    /// The row of `key`, made when it is first asked for.
    pub fn row_of(&mut self, key: K) -> usize {
        *self.rows.entry(key).or_insert_with(|| {
            self.keys.push(key);
            self.moved.push(false);
            self.grid.push()
        })
    }

    /// The amounts of `key`, a cell for each period, or `None` if nothing reached it.
    pub fn amounts(&self, key: K) -> Option<&[Qty]> {
        self.rows.get(&key).map(|&row| self.grid.row(row))
    }

    /// Whether anything that reached `key` moved something in the periods.
    pub fn moved(&self, key: K) -> bool {
        self.rows.get(&key).is_some_and(|&row| self.moved[row])
    }

    /// Every key, in the order its row was made.
    pub fn keys(&self) -> &[K] {
        &self.keys
    }

    /// Adds the row of `from` into the row of `into`, making the row of `into` if there is none, and says it has moved
    /// if `from` did.
    pub fn roll_up(&mut self, from: K, into: K) {
        let Some(&source) = self.rows.get(&from) else { return };
        let target = self.row_of(into);
        self.grid.add_row(source, target);
        self.moved[target] |= self.moved[source];
    }
}

/// Spreads `amount` over the periods `recognized` touches, as far as `cutoff`, and says what each period gets.
pub(crate) fn spread_over(
    periods: Periods,
    recognized: Days,
    cutoff: Day,
    amount: Qty,
    mut each: impl FnMut(usize, Qty),
) {
    for index in periods.overlapping(recognized.first(), recognized.last()) {
        let window = periods.window(index).days();
        if let Some(happened) = Days::new(window.first(), window.last().min(cutoff)) {
            each(index, spread(amount, recognized, happened));
        }
    }
}
