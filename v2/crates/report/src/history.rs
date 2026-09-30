//! What happened, as posted: the journal's flows with their solved quantities
//! and settlement, and the balances they add up to on any days.
//!
//! Historical views read the run's `posted` list instead of replaying the
//! engine. Any number of days cost one pass over the flows.

use std::ops::{AddAssign, Range};

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Holding, Pad, Posted, Run, State};
use axiom_model::{Amount, Book, Commodity, End, Flow, Place};

use crate::lens::{Basket, Lens, Priced, on_balance_sheet};

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

    /// The days on which the flow stands, as the first and the first day past
    /// it: from its own day, or its settlement, until it is returned.
    fn standing(&self) -> Option<(Day, Day)> {
        let forever = Day::MAX;
        match self.posted.state {
            State::Actual => Some((self.flow.day, forever)),
            State::Settled(on) => Some((self.flow.day.max(on), forever)),
            State::Returned(on) => Some((self.flow.day, on)),
            State::Pending | State::Void | State::Planned => None,
        }
    }

    /// The other end of the flow, seen from `place`.
    pub fn counterparty(&self, place: Id<Place>) -> Id<Place> {
        if self.flow.from == place { self.flow.to } else { self.flow.from }
    }

    /// The place at one end of the flow.
    pub fn place(&self, end: End) -> Id<Place> {
        match end {
            End::From => self.flow.from,
            End::To => self.flow.to,
        }
    }

    /// What the flow did at `end`, as solved: what the place lost (negative)
    /// or gained (positive), or, at a `PLACE.basis` end, how far the basis of
    /// its parcels fell or rose.
    pub fn change(&self, end: End) -> Change {
        let amount = match end {
            End::From => Amount::new(-self.posted.out, self.flow.out.unit),
            End::To => self.arrive(),
        };
        if self.flow.moves_quantity(end) { Change::Moved(amount) } else { Change::Rebased(amount) }
    }

    /// What happened at `place`: once for each end of the flow that is there.
    pub fn changes_at(self, place: Id<Place>) -> impl Iterator<Item = Change> {
        [End::From, End::To].into_iter().filter(move |&end| self.place(end) == place).map(move |end| self.change(end))
    }

    /// The out side priced on the day the flow happened.
    pub fn out_in_base(&self, lens: Lens) -> Option<Qty> {
        lens.on(self.flow.day).value(self.out())
    }

    /// The arrival priced on the day the flow happened.
    pub fn arrive_in_base(&self, lens: Lens) -> Option<Qty> {
        lens.on(self.flow.day).value(self.arrive())
    }
}

/// What a flow did to the place at one of its ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// The place gained (positive) or lost (negative) this much.
    Moved(Amount),
    /// The end is `PLACE.basis`: the place holds what it held, and the basis of
    /// its parcels rose (positive) or fell (negative) by this much.
    Rebased(Amount),
}

/// A pad, seen as the flow it stands for: from its counter place into the
/// place the assertion is about (or out of it, for a negative gap).
pub fn pad_ends(pad: &Pad) -> [(Id<Place>, Amount); 2] {
    let (gain, loss) = (pad.amount, Amount::new(-pad.amount.qty, pad.amount.unit));
    [(pad.place, gain), (pad.counter, loss)]
}

/// Every journal flow with its posting, in journal order.
pub fn postings<'a>(book: &'a Book, run: &'a Run) -> impl Iterator<Item = Posting<'a>> {
    book.flows.iter().zip(run.posted.iter()).map(|((id, flow), posted)| Posting { id, flow, posted })
}

/// Whether the journal holds nothing after `day`, so the run's final state is
/// the state on `day`.
pub fn journal_ends_by(book: &Book, day: Day) -> bool {
    let by = |last: Option<Day>| last.is_none_or(|last| last <= day);
    by(book.flows.as_slice().last().map(|flow| flow.day))
        && by(book.events.last().map(|event| event.day))
        && by(book.splits.last().map(|split| split.day))
        && by(book.asserts.last().map(|assert| assert.day))
}

/// What a place held of one commodity: how many, and, for places that are not
/// on the balance sheet, what the flows into it were worth on their own days.
#[derive(Clone, Copy, Default, Debug)]
pub struct Held {
    pub qty: Qty,
    pub booked: Qty,
}

impl AddAssign for Held {
    fn add_assign(&mut self, other: Held) {
        self.qty += other.qty;
        self.booked += other.booked;
    }
}

/// What every place held on each of several days, for the lens's owners.
///
/// Only the `(place, commodity)` pairs that ever held anything have cells: a
/// place holds one or two commodities, not all of them. Pairs are sorted, and
/// places are numbered in pre-order, so a subtree is a run of pairs.
pub struct Snapshots {
    days: Vec<Day>,
    pairs: Vec<(Id<Place>, Id<Commodity>)>,
    /// A cell for every pair, a day at a time.
    cells: Vec<Held>,
    /// Flows with no price on their day, left out of `booked`.
    pub unpriced: Priced,
}

/// A flow's, or a pad's, effect on one pair: `held` from column `columns.start`
/// until `columns.end`.
struct Delta {
    pair: (Id<Place>, Id<Commodity>),
    columns: Range<usize>,
    held: Held,
}

impl Snapshots {
    /// The books on `days` (ascending, at least one). One day, when the journal
    /// has nothing after it, is the run's final state and needs no replay;
    /// otherwise one pass over the flows fills every day at once.
    pub fn of(lens: Lens, days: &[Day], valued: bool) -> Snapshots {
        let (book, run) = (lens.book, lens.run);
        // Holdings do not remember what flows were worth, so a valued balance
        // of a foreign commodity in an expense place needs the flows.
        let foreign =
            |holding: &Holding| holding.unit != book.base && !on_balance_sheet(book.places[holding.place].class);
        match days {
            [day] if journal_ends_by(book, *day) && !(valued && run.holdings.iter().any(foreign)) => {
                Snapshots::final_state(lens, *day)
            }
            _ => Snapshots::replay(lens, days, valued),
        }
    }

    fn final_state(lens: Lens, day: Day) -> Snapshots {
        let held = lens.run.holdings.iter().filter(|holding| lens.owns(holding.place));
        let mut cells: Vec<_> = held.map(|holding| ((holding.place, holding.unit), holding.qty())).collect();
        cells.sort_unstable_by_key(|&(pair, _)| pair);
        let base = lens.book.base;
        Snapshots {
            days: vec![day],
            pairs: cells.iter().map(|&(pair, _)| pair).collect(),
            cells: cells
                .iter()
                .map(|&((_, unit), qty)| Held { qty, booked: if unit == base { qty } else { Qty::ZERO } })
                .collect(),
            unpriced: Priced::default(),
        }
    }

    fn replay(lens: Lens, days: &[Day], valued: bool) -> Snapshots {
        let (book, run) = (lens.book, lens.run);
        // A split multiplies what stood on its day, so its day is a column too.
        let mut columns = days.to_vec();
        columns.extend(book.splits.iter().map(|split| split.day));
        columns.sort_unstable();
        columns.dedup();
        let mut snapshots =
            Snapshots { days: columns, pairs: Vec::new(), cells: Vec::new(), unpriced: Priced::default() };

        // Each flow changes every column from the one it stands on until it is
        // returned: a difference at each edge, summed across columns after.
        let mut deltas = Vec::new();
        for posting in postings(book, run) {
            let Some((start, past)) = posting.standing() else { continue };
            let (lo, hi) = (snapshots.column_from(start), snapshots.column_from(past));
            for end in [End::From, End::To] {
                let place = posting.place(end);
                // A basis end moves no quantity, and so no money.
                if lo < hi
                    && lens.owns(place)
                    && let Change::Moved(amount) = posting.change(end)
                {
                    let held = snapshots.held(lens.on(posting.flow.day), place, amount, valued);
                    deltas.push(Delta { pair: (place, amount.unit), columns: lo..hi, held });
                }
            }
        }
        // A pad is a flow from its counter place (`unknown`, or the `via`
        // place): the gap it accepted, and its mirror. One after the last day
        // asked for changes none of them.
        for pad in &run.pads {
            let lo = snapshots.column_from(pad.day);
            for (place, amount) in pad_ends(pad) {
                if lo < snapshots.days.len() && lens.owns(place) {
                    let held = snapshots.held(lens.on(pad.day), place, amount, valued);
                    deltas.push(Delta { pair: (place, amount.unit), columns: lo..snapshots.days.len(), held });
                }
            }
        }
        snapshots.fill(&deltas);
        for split in book.splits.iter() {
            snapshots.split(split.day, split.unit, split.ratio);
        }
        snapshots.keep(days)
    }

    /// Allocates a cell for every pair the deltas touch, and sums them up.
    fn fill(&mut self, deltas: &[Delta]) {
        self.pairs = deltas.iter().map(|delta| delta.pair).collect();
        self.pairs.sort_unstable();
        self.pairs.dedup();
        let width = self.pairs.len();
        self.cells = vec![Held::default(); self.days.len() * width];
        for delta in deltas {
            let at = self.pair(delta.pair);
            self.cells[delta.columns.start * width + at] += delta.held;
            if delta.columns.end < self.days.len() {
                self.cells[delta.columns.end * width + at] += Held { qty: -delta.held.qty, booked: -delta.held.booked };
            }
        }
        for at in width..self.cells.len() {
            let before = self.cells[at - width];
            self.cells[at] += before;
        }
    }

    /// The first column on or after `day`.
    fn column_from(&self, day: Day) -> usize {
        self.days.partition_point(|&column| column < day)
    }

    /// Where a pair's cells are in a day.
    fn pair(&self, pair: (Id<Place>, Id<Commodity>)) -> usize {
        self.pairs.binary_search(&pair).expect("every pair a delta touches has a cell")
    }

    /// What `moved` adds to a place: its quantity, and, off the balance sheet,
    /// its worth at the prices of `lens`'s day.
    fn held(&mut self, lens: Lens, place: Id<Place>, moved: Amount, valued: bool) -> Held {
        let mut held = Held { qty: moved.qty, booked: Qty::ZERO };
        if valued && !on_balance_sheet(lens.book.places[place].class) {
            held.booked = self.unpriced.add(lens.value(moved)).unwrap_or_default();
        }
        held
    }

    /// Multiplies what stood of `unit` in every place on `day`, and on the columns after.
    fn split(&mut self, day: Day, unit: Id<Commodity>, ratio: Ratio) {
        let (column, width) = (self.column_from(day), self.pairs.len());
        for at in (0..width).filter(|&at| self.pairs[at].1 == unit) {
            let standing = self.cells[column * width + at].qty;
            let more = standing.scale(ratio).map_or(Qty::ZERO, |scaled| scaled - standing);
            for later in column..self.days.len() {
                self.cells[later * width + at].qty += more;
            }
        }
    }

    /// Only the days that were asked for.
    fn keep(self, days: &[Day]) -> Snapshots {
        if self.days == days {
            return self;
        }
        let width = self.pairs.len();
        let cells = days
            .iter()
            .flat_map(|day| {
                let column = self.column_from(*day);
                self.cells[column * width..(column + 1) * width].iter().copied()
            })
            .collect();
        Snapshots { days: days.to_vec(), cells, ..self }
    }

    pub fn days(&self) -> &[Day] {
        &self.days
    }

    /// What `place` and everything beneath it held on `days()[column]`.
    pub fn subtree(&self, book: &Book, column: usize, place: Id<Place>) -> Basket {
        let first = self.pairs.partition_point(|&(other, _)| other < place);
        let past = self.pairs.partition_point(|&(other, _)| other < book.places.end(place));
        let mut basket = Basket::default();
        let cells = &self.cells[column * self.pairs.len()..][first..past];
        for (&(_, unit), held) in self.pairs[first..past].iter().zip(cells) {
            if held.qty != Qty::ZERO || held.booked != Qty::ZERO {
                basket.add(unit, *held);
            }
        }
        basket
    }
}
