//! What happened, as posted: the journal's flows with their solved quantities
//! and settlement, and the balances they add up to on any days.
//!
//! Historical views read the run's `posted` list instead of replaying the
//! engine. Any number of days cost one pass over the flows.

use std::ops::{AddAssign, Range};

use axiom_core::{Day, Id, Qty, Ratio};
use axiom_engine::{Holding, Pad, Posted, Run, State};
use axiom_model::{Amount, Book, End, Flow, Place};

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
/// A dense grid over days, places and commodities. Places are numbered in
/// pre-order, so a subtree is one contiguous range of it.
pub struct Snapshots {
    days: Vec<Day>,
    places: usize,
    units: usize,
    cells: Vec<Held>,
    /// Flows with no price on their day, left out of `booked`.
    pub unpriced: Priced,
}

impl Snapshots {
    /// The books on `days` (ascending, at least one). One day, when the journal
    /// has nothing after it, is the run's final state and needs no replay;
    /// otherwise one pass over the flows fills every day at once.
    pub fn of(lens: Lens, run: &Run, days: &[Day], valued: bool) -> Snapshots {
        let book = lens.book;
        // Holdings do not remember what flows were worth, so a valued balance
        // of a foreign commodity in an expense place needs the flows.
        let foreign =
            |holding: &Holding| holding.unit != book.base && !on_balance_sheet(book.places[holding.place].class);
        match days {
            [day] if journal_ends_by(book, *day) && !(valued && run.holdings.iter().any(foreign)) => {
                Snapshots::final_state(lens, run, *day)
            }
            _ => Snapshots::replay(lens, run, days, valued),
        }
    }

    fn empty(book: &Book, days: Vec<Day>) -> Snapshots {
        let (places, units) = (book.places.len(), book.commodities.len());
        Snapshots { cells: vec![Held::default(); days.len() * places * units], days, places, units, unpriced: Priced::default() }
    }

    fn final_state(lens: Lens, run: &Run, day: Day) -> Snapshots {
        let mut snapshots = Snapshots::empty(lens.book, vec![day]);
        for holding in run.holdings.iter().filter(|holding| lens.owns(holding.place)) {
            let qty = holding.qty();
            let booked = if holding.unit == lens.book.base { qty } else { Qty::ZERO };
            *snapshots.cell(0, holding.place, holding.unit.index()) = Held { qty, booked };
        }
        snapshots
    }

    fn replay(lens: Lens, run: &Run, days: &[Day], valued: bool) -> Snapshots {
        let book = lens.book;
        // A split multiplies what stood on its day, so its day is a column too.
        let mut columns = days.to_vec();
        columns.extend(book.splits.iter().map(|split| split.day));
        columns.sort_unstable();
        columns.dedup();
        let mut snapshots = Snapshots::empty(book, columns);

        // Each flow changes every column from the one it stands on until it is
        // returned: a difference at each edge, summed across columns after.
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
                    snapshots.change(lo..hi, place, amount.unit, held);
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
                    snapshots.change(lo..snapshots.days.len(), place, amount.unit, held);
                }
            }
        }
        snapshots.accumulate();
        for split in book.splits.iter() {
            snapshots.split(split.day, split.unit, split.ratio);
        }
        snapshots.keep(days)
    }

    /// The first column on or after `day`.
    fn column_from(&self, day: Day) -> usize {
        self.days.partition_point(|&column| column < day)
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

    fn cell(&mut self, column: usize, place: Id<Place>, unit: usize) -> &mut Held {
        &mut self.cells[(column * self.places + place.index()) * self.units + unit]
    }

    /// Adds `held` to `columns`, as a difference at each edge.
    fn change(&mut self, columns: Range<usize>, place: Id<Place>, unit: Id<axiom_model::Commodity>, held: Held) {
        self.cell(columns.start, place, unit.index()).add_assign(held);
        if columns.end < self.days.len() {
            self.cell(columns.end, place, unit.index()).add_assign(Held { qty: -held.qty, booked: -held.booked });
        }
    }

    /// Turns differences into what stood on each column.
    fn accumulate(&mut self) {
        let stride = self.places * self.units;
        for at in stride..self.cells.len() {
            let before = self.cells[at - stride];
            self.cells[at] += before;
        }
    }

    /// Multiplies what stood of `unit` in every place on `day`, and on the columns after.
    fn split(&mut self, day: Day, unit: Id<axiom_model::Commodity>, ratio: Ratio) {
        let column = self.column_from(day);
        for place in (0..self.places).map(|place| Id::new(place as u32)) {
            let standing = self.cell(column, place, unit.index()).qty;
            let more = standing.scale(ratio).map_or(Qty::ZERO, |scaled| scaled - standing);
            for later in column..self.days.len() {
                self.cell(later, place, unit.index()).qty += more;
            }
        }
    }

    /// Only the days that were asked for.
    fn keep(self, days: &[Day]) -> Snapshots {
        if self.days == days {
            return self;
        }
        let stride = self.places * self.units;
        let cells = days
            .iter()
            .flat_map(|day| {
                let column = self.column_from(*day);
                self.cells[column * stride..(column + 1) * stride].iter().copied()
            })
            .collect();
        Snapshots { days: days.to_vec(), cells, ..self }
    }

    pub fn days(&self) -> &[Day] {
        &self.days
    }

    /// What `place` and everything beneath it held on `days()[column]`.
    pub fn subtree(&self, book: &Book, column: usize, place: Id<Place>) -> Basket {
        let first = (column * self.places + place.index()) * self.units;
        let past = (column * self.places + book.places.end(place).index()) * self.units;
        let mut basket = Basket::default();
        for (at, held) in self.cells[first..past].iter().enumerate() {
            if held.qty != Qty::ZERO || held.booked != Qty::ZERO {
                basket.add(Id::new((at % self.units) as u32), *held);
            }
        }
        basket
    }
}
