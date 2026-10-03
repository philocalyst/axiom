//! What every position held, on every day: balances as steps, recorded by the fold as it goes.
//!
//! A *position* is one place's holding of one commodity, and its history is the days its balance changed and what it
//! was from each. The fold records a step at the one moment a position's quantity can change, when a fact has been
//! done (`Ledger::record_balances`), so a history is a by-product of the pass that posts the flows and costs no pass of
//! its own. A view of a past day then reads a balance in a binary search, where it used to replay every flow before
//! the day.
//!
//! # Why columns
//!
//! A query finds a position (they are few: the accounts a book has, times the commodities they hold), then the step
//! it stood on, which is the last day not after the one asked for. The days of one position lie end to end in one
//! column of 4-byte days, so the search touches four bytes a probe and nothing else, and the balance it finds is read
//! once from a column of its own. The rows of all positions are one flat pair of columns and an offset a position, as
//! [`Groups`](axiom_core::Groups) lays out keys, so that nothing is chased and a snapshot of every position on a day is a
//! sweep of binary searches through memory that stays put.
//!
//! Positions are sorted by place then commodity, and places are numbered in pre-order, so the positions beneath a
//! place are one contiguous run ([`Histories::beneath`]): a subtree's balance is a sum over a slice.
//!
//! # Extremes
//!
//! The most a position held in a window (FBAR's peak) and the least are range queries over its balances, answered in
//! O(1) by a sparse table ([`core::sparse`](axiom_core::sparse)) that costs O(n log n) to build, so it is built by the
//! one who asks, for the one position they ask about, and held for as many windows as they like: [`Steps::extremes`].
//! Nothing is sampled, and nothing is built that nobody asks for.

use axiom_core::sparse::{self, Max, Min, Sparse};
use axiom_core::{Day, Days, Id, Qty, Run};
use axiom_model::{Commodity, Place};

/// One place's holding of one commodity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Position {
    pub place: Id<Place>,
    pub unit: Id<Commodity>,
}

/// What every position held, as steps on days. Built by the fold; read by every view of a day.
#[derive(Clone, Default, Debug)]
pub struct Histories {
    /// The positions that ever held anything, by place then commodity.
    positions: Vec<Position>,
    /// The steps of position `p` are `starts[p] .. starts[p + 1]` of `days` and `balances`.
    starts: Vec<u32>,
    /// The day each step begins, ascending within a position.
    days: Vec<Day>,
    /// What the position holds from that day, until its next step.
    balances: Vec<Qty>,
    /// The positions of place `i` are `places[i] .. places[i + 1]`: one more entry than there are places.
    places: Vec<u32>,
}

/// A position's steps, borrowed: what is held from each day on.
#[derive(Clone, Copy, Debug)]
pub struct Steps<'a> {
    days: &'a [Day],
    balances: &'a [Qty],
}

impl Histories {
    /// How many positions held anything.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Every position, in order, with its id.
    pub fn positions(&self) -> impl ExactSizeIterator<Item = (Id<Position>, Position)> + Clone {
        self.positions.iter().enumerate().map(|(at, &position)| (Id::new(at as u32), position))
    }

    pub fn position(&self, id: Id<Position>) -> Position {
        self.positions[id.index()]
    }

    /// The positions of the places `first` up to, not including, `past`: those of a subtree when `past` is the end of
    /// the subtree's root. Places are numbered in pre-order, so they are one run.
    pub fn beneath(&self, first: Id<Place>, past: Id<Place>) -> Run<Position> {
        let at = |place: Id<Place>| self.places.get(place.index()).map_or(self.positions.len(), |&at| at as usize);
        Run::of(at(first)..at(past))
    }

    /// The steps of a position.
    pub fn steps(&self, id: Id<Position>) -> Steps<'_> {
        let (from, to) = (self.starts[id.index()] as usize, self.starts[id.index() + 1] as usize);
        Steps { days: &self.days[from..to], balances: &self.balances[from..to] }
    }

    /// What a position held at the end of `day`.
    pub fn at(&self, id: Id<Position>, day: Day) -> Qty {
        self.steps(id).at(day)
    }

    /// The steps recorded in all, which is what a history costs.
    pub fn steps_in_all(&self) -> usize {
        self.days.len()
    }

    /// The histories of the given steps, for a run that was not folded here: a test, or a client that keeps its own. Each
    /// position is listed once, with its steps in ascending order of day.
    pub fn from_steps(places: usize, steps: impl IntoIterator<Item = (Position, Vec<(Day, Qty)>)>) -> Histories {
        let (keys, steps): (Vec<Position>, Vec<Vec<(Day, Qty)>>) = steps.into_iter().unzip();
        let rows = steps.iter().enumerate().flat_map(|(at, rows)| rows.iter().map(move |&(day, held)| (at, day, held)));
        Histories::assemble(&keys, places, rows)
    }

    /// Lays out `rows`: each is a step of the position `keys[at]`, and the rows of one position come in the order of
    /// their days. Two passes, as a counting sort does: the first counts the steps each position will have, the second
    /// puts each where it goes. A step on a day the position already has one on replaces it (what it held when that day
    /// ended is what stands), and a step that changes nothing is left out.
    pub(crate) fn assemble(
        keys: &[Position],
        places: usize,
        rows: impl Iterator<Item = (usize, Day, Qty)> + Clone,
    ) -> Histories {
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.sort_unstable_by_key(|&at| keys[at]);
        let mut rank = vec![0; keys.len()];
        for (sorted, &at) in order.iter().enumerate() {
            rank[at] = sorted;
        }
        let positions: Vec<Position> = order.iter().map(|&at| keys[at]).collect();

        let mut starts = vec![0u32; positions.len() + 1];
        let mut last: Vec<Option<(Day, Qty)>> = vec![None; positions.len()];
        for (at, day, held) in rows.clone() {
            starts[rank[at] + 1] += u32::from(Step::of(&mut last[rank[at]], day, held) == Step::New);
        }
        for at in 1..starts.len() {
            starts[at] += starts[at - 1];
        }
        let mut histories = Histories {
            days: vec![Day::MIN; starts[positions.len()] as usize],
            balances: vec![Qty::ZERO; starts[positions.len()] as usize],
            places: place_starts(&positions, places),
            positions,
            starts,
        };
        histories.fill(&rank, rows);
        histories
    }

    /// The second pass of [`assemble`](Histories::assemble): writes each step where the counting put its position.
    fn fill(&mut self, rank: &[usize], rows: impl Iterator<Item = (usize, Day, Qty)>) {
        let mut next: Vec<u32> = self.starts[..self.positions.len()].to_vec();
        let mut last: Vec<Option<(Day, Qty)>> = vec![None; self.positions.len()];
        for (at, day, held) in rows {
            let position = rank[at];
            let to = next[position] as usize;
            match Step::of(&mut last[position], day, held) {
                Step::New => {
                    (self.days[to], self.balances[to]) = (day, held);
                    next[position] += 1;
                }
                Step::Replace => self.balances[to - 1] = held,
                Step::Skip => {}
            }
        }
    }
}

/// What a row recorded for a position is, given the last step the position has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// A step of its own.
    New,
    /// On the day of the last step: what the position held when that day ended is what stands, so it takes the place of
    /// that step.
    Replace,
    /// It holds what the position held already: nothing to say.
    Skip,
}

impl Step {
    /// Decides a row of `day` holding `held` and, unless it is to be skipped, makes it the position's last step.
    fn of(last: &mut Option<(Day, Qty)>, day: Day, held: Qty) -> Step {
        let step = match *last {
            Some((was, _)) if was == day => Step::Replace,
            Some((_, before)) if before == held => Step::Skip,
            None if held.is_zero() => Step::Skip,
            _ => Step::New,
        };
        if step != Step::Skip {
            *last = Some((day, held));
        }
        step
    }
}

/// Where the positions of each place begin: `places + 1` offsets into `positions`, which are sorted by place.
fn place_starts(positions: &[Position], places: usize) -> Vec<u32> {
    let mut starts = vec![0u32; places + 1];
    for position in positions {
        starts[position.place.index() + 1] += 1;
    }
    for at in 1..starts.len() {
        starts[at] += starts[at - 1];
    }
    starts
}

impl<'a> Steps<'a> {
    /// What the position held at the end of `day`: what its last step on or before `day` says, and nothing before its
    /// first.
    pub fn at(self, day: Day) -> Qty {
        match self.days.partition_point(|&from| from <= day) {
            0 => Qty::ZERO,
            after => self.balances[after - 1],
        }
    }

    pub fn len(self) -> usize {
        self.days.len()
    }

    pub fn is_empty(self) -> bool {
        self.days.is_empty()
    }

    /// Each step: the day it begins and what the position holds from it.
    pub fn iter(self) -> impl ExactSizeIterator<Item = (Day, Qty)> + 'a {
        self.days.iter().copied().zip(self.balances.iter().copied())
    }

    /// The tables that answer the most and the least held over any window: O(n log n) to make, O(1) to ask.
    pub fn extremes(self) -> Extremes<'a> {
        Extremes { days: self.days, peaks: Sparse::new(self.balances), lows: Sparse::new(self.balances) }
    }
}

/// The most and the least a position held over windows of days, for as many windows as are asked.
pub struct Extremes<'a> {
    days: &'a [Day],
    peaks: Sparse<Qty, Max>,
    lows: Sparse<Qty, Min>,
}

impl Extremes<'_> {
    /// The most the position held on any day of `window`, or `None` if it held nothing yet: the days before its first
    /// step are not counted, for it did not exist.
    pub fn peak(&self, window: Days) -> Option<Qty> {
        sparse::peak_within(self.days, &self.peaks, window)
    }

    /// The least, on the same terms.
    pub fn low(&self, window: Days) -> Option<Qty> {
        sparse::low_within(self.days, &self.lows, window)
    }
}

/// What the fold writes as it goes: every time a position's balance moved, in the order it did. Slots are the holdings'
/// own numbering, in the order the fold made them, and [`Changes::freeze`] sorts them out into [`Histories`].
#[derive(Clone, Default)]
pub(crate) struct Changes {
    rows: Vec<Change>,
}

#[derive(Clone, Copy)]
struct Change {
    slot: u32,
    day: Day,
    balance: Qty,
}

impl Changes {
    /// Slot `slot` held `balance` once `day` was done with it.
    pub fn push(&mut self, slot: u32, day: Day, balance: Qty) {
        self.rows.push(Change { slot, day, balance });
    }

    /// The histories: `slots` says which position each slot is, in the order the slots were made.
    pub fn freeze(&self, slots: &[Position], places: usize) -> Histories {
        let rows = self.rows.iter().map(|row| (row.slot as usize, row.day, row.balance));
        Histories::assemble(slots, places, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(place: u32, unit: u32) -> Position {
        Position { place: Id::new(place), unit: Id::new(unit) }
    }

    fn days(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn histories(steps: &[(Position, &[(i32, i64)])]) -> Histories {
        let steps =
            steps.iter().map(|&(at, rows)| (at, rows.iter().map(|&(day, held)| (Day(day), Qty(held))).collect()));
        Histories::from_steps(4, steps)
    }

    #[test]
    fn a_balance_is_the_last_step_not_after_the_day_and_nothing_before_the_first() {
        let all = histories(&[(position(1, 0), &[(10, 5), (20, 8), (30, 0)])]);
        let at = |day| all.at(Id::new(0), Day(day)).0;
        assert_eq!([at(9), at(10), at(19), at(20), at(29), at(30), at(31)], [0, 5, 5, 8, 8, 0, 0]);
    }

    #[test]
    fn positions_are_sorted_and_a_subtree_is_one_run() {
        let all = histories(&[(position(3, 0), &[(1, 1)]), (position(1, 1), &[(1, 2)]), (position(1, 0), &[(1, 3)])]);
        let placed: Vec<_> = all.positions().map(|(_, at)| (at.place.index(), at.unit.index())).collect();
        assert_eq!(placed, [(1, 0), (1, 1), (3, 0)]);
        let beneath = |first, past| all.beneath(Id::new(first), Id::new(past)).ids().count();
        assert_eq!([beneath(0, 1), beneath(1, 2), beneath(1, 4), beneath(2, 3), beneath(3, 4)], [0, 2, 3, 0, 1]);
    }

    #[test]
    fn two_steps_on_one_day_are_the_later_and_a_step_that_changes_nothing_is_left_out() {
        let all = histories(&[(position(0, 0), &[(5, 10), (5, 0), (5, 7), (6, 7), (9, 7), (9, 2)])]);
        let steps: Vec<_> = all.steps(Id::new(0)).iter().map(|(day, held)| (day.0, held.0)).collect();
        assert_eq!(steps, [(5, 7), (9, 2)]);
    }

    #[test]
    fn a_position_that_only_ever_held_nothing_has_no_steps() {
        let all = histories(&[(position(0, 0), &[(5, 0), (6, 0)])]);
        assert!(all.steps(Id::new(0)).is_empty() && all.at(Id::new(0), Day(7)) == Qty::ZERO);
    }

    #[test]
    fn the_extremes_of_a_window_include_the_step_in_effect_on_its_first_day() {
        let all = histories(&[(position(0, 0), &[(10, 5), (20, 9), (30, 2), (40, 6)])]);
        let extremes = all.steps(Id::new(0)).extremes();
        let (peak, low) =
            (|first, last| extremes.peak(days(first, last)), |first, last| extremes.low(days(first, last)));
        assert_eq!(
            [peak(10, 19), peak(15, 25), peak(25, 35), peak(31, 39)],
            [Some(Qty(5)), Some(Qty(9)), Some(Qty(9)), Some(Qty(2))]
        );
        assert_eq!([low(10, 40), low(15, 25), low(20, 29)], [Some(Qty(2)), Some(Qty(5)), Some(Qty(9))]);
        assert_eq!(peak(1, 9), None, "it held nothing yet");
    }
}
