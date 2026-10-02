//! Everything said about a thing, as steps on days: one store, rows of timelines.
//!
//! A thing is a holder, and a slot is something that can be said of it: its owner, when it opened, the systems a
//! person lives in. What is said of a slot is a timeline: a value from some day, until a statement from a later day
//! says another. Holders and slots are dense numbers (the model numbers them), and a value is whatever a
//! [`Field`](crate::tagless::Field) can hold.
//!
//! # Why compressed rows
//!
//! A book of a million things says a few slots each, and the fold asks for one slot of one thing on one day. So the
//! store is a table of three levels, laid end to end in flat arrays and found by offsets, with nothing to chase:
//!
//! ```text
//! rows[holder] ‥ rows[holder + 1]      the entries of a holder, sorted by slot
//! entries[e].first ‥ entries[e+1].first    the steps of an entry: a slot's timeline
//! days[step], values[step]             a step: the day it begins, and what holds from then
//! ```
//!
//! A read is a short scan of the holder's entries and a binary search of one entry's days, and touches four bytes of
//! day and seventeen of value. Reading a whole slot is a slice. Building is one counting sort of the statements by
//! holder, then each (holder, slot) group painted in arrival order into a small reused vector, and written out.
//!
//! # Steps and gaps
//!
//! Every entry is a timeline over all time: its first step begins at [`Day::MIN`], so a read always finds the step
//! it is in, with no case for "before the first". Where nothing is said (before the first statement, and after a
//! window that ends with nothing to resume) the step holds an empty value, a gap. A read of a gap is `None`, and
//! [`Steps`] leaves gaps out.
//!
//! # Keys are claims
//!
//! A [`Key<V>`] says that a slot holds `V`s. The store cannot check it: a slot's range lives in the model, which
//! checks every key against it when it builds the book (lane K12). A read as the wrong type gives a wrong value, and
//! never undefined behaviour, for the reason [`tagless`](crate::tagless) gives: every payload is initialized and every
//! field takes every bit pattern. Debug builds assert the tag as [`Column::get`] does, so a test that reads the wrong
//! way panics.
//!
//! # Equal values merge
//!
//! Painting merges adjacent steps that hold the same value, and "the same" is [`Datum`] equality: the same tag and the
//! same sixteen bytes. That is `==` for every type a column holds; [`Datum`] says what would break it.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Range;

use crate::calendar::Days;
use crate::day::Day;
use crate::dayset::DaySet;
use crate::groups::bucket;
use crate::tagless::{Column, Datum, Field, Tag};
use crate::timeline::paint_steps;

/// A slot, numbered densely from zero by whoever declares the slots.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SlotId(pub u32);

/// A slot and the type its values have: a claim the store cannot check (see the module docs).
///
/// A constant when the engine knows the slot, and made at run time from the slot's number when the book declared it.
pub struct Key<V> {
    slot: SlotId,
    of: PhantomData<fn() -> V>,
}

const _: () = assert!(size_of::<SlotId>() == 4 && size_of::<Key<(u32, u32)>>() == 4);

impl<V> Key<V> {
    pub const fn new(slot: SlotId) -> Key<V> {
        Key { slot, of: PhantomData }
    }

    pub const fn slot(self) -> SlotId {
        self.slot
    }
}

// By hand: a derive would ask `V` for what a key, which holds no `V`, does not need.
impl<V> Clone for Key<V> {
    fn clone(&self) -> Key<V> {
        *self
    }
}

impl<V> Copy for Key<V> {}

impl<V> PartialEq for Key<V> {
    fn eq(&self, other: &Key<V>) -> bool {
        self.slot == other.slot
    }
}

impl<V> Eq for Key<V> {}

impl<V> Hash for Key<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.slot.hash(state);
    }
}

impl<V> fmt::Debug for Key<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key<{}>#{}", std::any::type_name::<V>(), self.slot.0)
    }
}

/// One slot of one holder: which slot, and where its steps begin. They end where the next entry's begin, and the
/// last entry of the store is followed by one that says only where the steps end.
#[derive(Clone, Copy)]
struct Entry {
    slot: SlotId,
    first: u32,
}

const _: () = assert!(size_of::<Entry>() == 8);

/// Every slot of every holder, frozen: steps on days in compressed rows. Built by a [`Builder`], then only read.
pub struct Facts {
    /// Holder to its first entry; one more at the end, so that a holder's entries are `rows[h]..rows[h + 1]`.
    rows: Vec<u32>,
    /// Entry to its slot and first step; sorted by slot within a row; one more at the end.
    entries: Vec<Entry>,
    /// Step to the day it begins. Strictly increasing within an entry, which begins at [`Day::MIN`].
    days: Vec<Day>,
    /// Step to what holds from its day on: empty where nothing is said.
    values: Column,
}

impl Facts {
    /// A builder for a store of `holders` rows, numbered from zero.
    pub fn builder(holders: usize) -> Builder {
        Builder { holders: u32::try_from(holders).expect("fewer than 2^32 holders"), statements: Statements::default() }
    }

    /// How many holders: rows, most of them with few entries or none.
    pub fn holders(&self) -> usize {
        self.rows.len() - 1
    }

    /// What holds of `holder`'s `key` on `day`, or `None` if nothing is said then. Two searches and no allocation.
    pub fn at<V: Field>(&self, key: Key<V>, holder: u32, day: Day) -> Option<V> {
        let steps = self.steps_of(key.slot, holder)?;
        let begun = self.days[steps.clone()].partition_point(|&from| from <= day);
        read(&self.values, (steps.start + begun - 1) as u32)
    }

    /// What was painted of `holder`'s `key`, in order: each stretch of days, and the value that holds through it.
    pub fn steps<V: Field>(&self, key: Key<V>, holder: u32) -> Steps<'_, V> {
        let steps = self.steps_of(key.slot, holder).unwrap_or(0..0);
        Steps::new(&self.days[steps.clone()], &self.values, steps.start as u32)
    }

    /// The days of `within` on which `holder`'s `key` holds a value that satisfies `holds`.
    ///
    /// An integral, not a sample: it walks the steps that meet the window once, and what it returns is exact. Each
    /// satisfying step is clipped to the window, and steps that touch make one interval.
    pub fn days_where<V: Field>(
        &self,
        key: Key<V>,
        holder: u32,
        within: Days,
        mut holds: impl FnMut(V) -> bool,
    ) -> DaySet {
        let satisfying = self.steps(key, holder).within(within).filter(|&(_, value)| holds(value));
        satisfying.filter_map(|(days, _)| days.intersect(within)).collect()
    }

    /// Where the steps of `slot` are among the store's, if `holder` said anything of it.
    fn steps_of(&self, slot: SlotId, holder: u32) -> Option<Range<usize>> {
        let row = self.rows[holder as usize] as usize..self.rows[holder as usize + 1] as usize;
        // One entry past the row: it is the next row's first, or the last, and says where this row's steps end.
        let entries = &self.entries[row.start..=row.end];
        let found = entries[..entries.len() - 1].iter().position(|entry| entry.slot == slot)?;
        Some(entries[found].first as usize..entries[found + 1].first as usize)
    }
}

/// The value of step `at`, or `None` if it is a gap.
fn read<V: Field>(values: &Column, at: u32) -> Option<V> {
    (values.tags()[at as usize] != Tag::Empty).then(|| values.get(at))
}

/// The day before `day`, which stays put at the beginning of time, where nothing is before it.
fn day_before(day: Day) -> Day {
    Day(day.0.saturating_sub(1))
}

/// The steps of one timeline, borrowed: what [`Facts::steps`] gives, and what the params, prices, terms and budgets
/// that are not in a [`Facts`] are read through too.
///
/// It borrows a slice of the days the steps begin on and the column of their values, and nothing else, which is
/// why it needs no [`Facts`]: any structure that keeps a timeline as one day column and one value column can lend
/// it. It runs forwards and backwards and counts, and yields each stretch of days with its value, leaving out the
/// gaps, where nothing is said.
pub struct Steps<'a, V> {
    stretches: Stretches<'a, V>,
}

impl<'a, V: Field> Steps<'a, V> {
    /// The steps that begin on `days`, valued by `values` from its value number `first`. The days strictly
    /// increase, and the last step lasts for ever.
    pub fn new(days: &'a [Day], values: &'a Column, first: u32) -> Steps<'a, V> {
        debug_assert!(days.windows(2).all(|pair| pair[0] < pair[1]), "steps begin in order");
        debug_assert!(first as usize + days.len() <= values.len(), "a value for every step");
        Steps { stretches: Stretches { days, end: Day::MAX, values, first, of: PhantomData } }
    }

    /// Only the steps that meet `window`. Each is whole: the first may begin before the window and the last end
    /// after it.
    pub fn within(self, window: Days) -> Steps<'a, V> {
        Steps { stretches: self.stretches.within(window) }
    }
}

impl<V: Field> Iterator for Steps<'_, V> {
    type Item = (Days, V);

    fn next(&mut self) -> Option<(Days, V)> {
        self.stretches.find_map(|(days, value)| Some((days, value?)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let present = self.stretches.present();
        (present, Some(present))
    }
}

impl<V: Field> DoubleEndedIterator for Steps<'_, V> {
    fn next_back(&mut self) -> Option<(Days, V)> {
        (&mut self.stretches).rev().find_map(|(days, value)| Some((days, value?)))
    }
}

impl<V: Field> ExactSizeIterator for Steps<'_, V> {}

impl<V> Clone for Steps<'_, V> {
    fn clone(&self) -> Self {
        Steps { stretches: self.stretches.clone() }
    }
}

/// Every step, gaps too: the timeline as consecutive stretches of days, each with its value if it has one.
struct Stretches<'a, V> {
    /// The days the remaining steps begin on.
    days: &'a [Day],
    /// The last day of the last of them: the next step's day is not in `days` if it was taken from the back.
    end: Day,
    values: &'a Column,
    /// The value number of `days[0]`.
    first: u32,
    of: PhantomData<fn() -> V>,
}

impl<'a, V: Field> Stretches<'a, V> {
    fn within(self, window: Days) -> Stretches<'a, V> {
        let begun = |day: Day| self.days.partition_point(|&from| from <= day);
        let (from, to) = (begun(window.first()).saturating_sub(1), begun(window.last()));
        let end = self.days.get(to).map_or(self.end, |&next| day_before(next));
        Stretches { days: &self.days[from..to], end, first: self.first + from as u32, ..self }
    }

    /// How many steps are not gaps: a scan of one byte each.
    fn present(&self) -> usize {
        let tags = &self.values.tags()[self.first as usize..][..self.days.len()];
        tags.iter().filter(|&&tag| tag != Tag::Empty).count()
    }
}

impl<V: Field> Iterator for Stretches<'_, V> {
    type Item = (Days, Option<V>);

    fn next(&mut self) -> Option<Self::Item> {
        let (&from, rest) = self.days.split_first()?;
        let until = rest.first().map_or(self.end, |&next| day_before(next));
        let value = read(self.values, self.first);
        (self.days, self.first) = (rest, self.first + 1);
        Some((Days::new(from, until).expect("a step lasts a day or more"), value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.days.len(), Some(self.days.len()))
    }
}

impl<V: Field> DoubleEndedIterator for Stretches<'_, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (&from, rest) = self.days.split_last()?;
        let until = std::mem::replace(&mut self.end, day_before(from));
        let value = read(self.values, self.first + rest.len() as u32);
        self.days = rest;
        Some((Days::new(from, until).expect("a step lasts a day or more"), value))
    }
}

impl<V> Clone for Stretches<'_, V> {
    fn clone(&self) -> Self {
        Stretches { days: self.days, end: self.end, values: self.values, first: self.first, of: PhantomData }
    }
}

/// Statements in the order they were made, and nothing else, one column for each part: the counting sort reads the
/// holders alone, and painting reads the rest of one statement at a time.
#[derive(Default)]
struct Statements {
    holder: Vec<u32>,
    slot: Vec<SlotId>,
    days: Vec<Days>,
    values: Column,
}

impl Statements {
    fn len(&self) -> usize {
        self.holder.len()
    }

    fn push(&mut self, holder: u32, slot: SlotId, days: Days, value: Datum) {
        self.holder.push(holder);
        self.slot.push(slot);
        self.days.push(days);
        self.values.push_datum(value);
    }

    /// The timeline the statements `group` of one slot of one holder make, painted in the order they are given:
    /// nothing, and then each statement over its days.
    fn paint(&self, group: &[u32], steps: &mut Vec<(Day, Datum)>) {
        debug_assert!(
            group.windows(2).all(|pair| self.values.datum(pair[0]).tag() == self.values.datum(pair[1]).tag()),
            "a slot holds one type"
        );
        steps.clear();
        steps.push((Day::MIN, Datum::EMPTY));
        for &at in group {
            paint_steps(steps, self.days[at as usize], self.values.datum(at));
        }
    }
}

/// Statements that have been made, to be frozen into a [`Facts`].
///
/// Statements are painted in the order they were made, whatever holders and slots they were about: a later one
/// overrides an earlier one over the days it covers, and what held before resumes after it.
pub struct Builder {
    holders: u32,
    statements: Statements,
}

impl Builder {
    /// `value` holds of `holder`'s `key` over `days`: `from … until …`, or `Days::new(from, Day::MAX)` from a day on.
    pub fn paint<V: Field>(&mut self, holder: u32, key: Key<V>, days: Days, value: V) {
        assert!(holder < self.holders, "holder {holder} of a store of {}", self.holders);
        self.statements.push(holder, key.slot, days, Datum::of(value));
    }

    /// `value` holds of `holder`'s `key` from the beginning of time: a declaration.
    pub fn paint_always<V: Field>(&mut self, holder: u32, key: Key<V>, value: V) {
        self.paint(holder, key, Days::ALWAYS, value);
    }

    /// The store of every statement made so far. A function of the statements alone, which the builder keeps: a
    /// live edit makes more of them and freezes again, and gets what a build of all of them would give.
    pub fn freeze(&self) -> Facts {
        let statements = &self.statements;
        assert!(u32::try_from(statements.len()).is_ok(), "fewer than 2^32 statements");
        let holder_of = |at: usize| statements.holder[at] as usize;
        let (starts, mut order) = bucket(self.holders as usize, statements.len(), holder_of);
        let mut facts = Facts::with_room_for(self.holders as usize, statements.len());
        let mut painted = Vec::new();
        for row in starts.windows(2) {
            let mine = &mut order[row[0] as usize..row[1] as usize];
            // Stable, so a slot's statements stay in the order they were made.
            mine.sort_by_key(|&at| statements.slot[at as usize]);
            for group in mine.chunk_by(|&a, &b| statements.slot[a as usize] == statements.slot[b as usize]) {
                statements.paint(group, &mut painted);
                facts.push_entry(statements.slot[group[0] as usize], &painted);
            }
            facts.end_row();
        }
        facts.finish()
    }
}

/// The writer side of [`Facts`], for `freeze`: entries and steps appended in order, row by row.
impl Facts {
    fn with_room_for(holders: usize, statements: usize) -> Facts {
        let mut rows = Vec::with_capacity(holders + 1);
        rows.push(0);
        Facts {
            rows,
            entries: Vec::with_capacity(statements),
            days: Vec::with_capacity(statements),
            values: Column::with_capacity(statements),
        }
    }

    fn push_entry(&mut self, slot: SlotId, steps: &[(Day, Datum)]) {
        self.entries.push(Entry { slot, first: len32(self.days.len()) });
        for &(day, value) in steps {
            self.days.push(day);
            self.values.push_datum(value);
        }
    }

    fn end_row(&mut self) {
        self.rows.push(len32(self.entries.len()));
    }

    /// Adds the last entry, which says where the steps end.
    fn finish(mut self) -> Facts {
        self.entries.push(Entry { slot: SlotId(u32::MAX), first: len32(self.days.len()) });
        self
    }
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 entries and steps")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: Key<u32> = Key::new(SlotId(0));
    const OTHER: Key<u32> = Key::new(SlotId(1));
    const OPENED: Key<Day> = Key::new(SlotId(2));

    fn days(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn from(first: i32) -> Days {
        Days::new(Day(first), Day::MAX).unwrap()
    }

    /// A store with one holder, whose `LETTER` was painted by `paints` in order.
    fn painted(paints: &[(Days, u32)]) -> Facts {
        let mut builder = Facts::builder(1);
        for &(days, letter) in paints {
            builder.paint(0, LETTER, days, letter);
        }
        builder.freeze()
    }

    /// What was painted of the first holder's `LETTER`, as `(first day, last day, letter)`, the unbounded ends as the
    /// ends of `i32`.
    fn seen(facts: &Facts) -> Vec<(i32, i32, u32)> {
        facts.steps(LETTER, 0).map(|(days, letter)| (days.first().0, days.last().0, letter)).collect()
    }

    const MIN: i32 = i32::MIN;
    const MAX: i32 = i32::MAX;

    fn letters(facts: &Facts, at: &[i32]) -> Vec<Option<u32>> {
        at.iter().map(|&day| facts.at(LETTER, 0, Day(day))).collect()
    }

    #[test]
    fn a_declaration_holds_from_the_beginning_of_time() {
        let facts = painted(&[(Days::ALWAYS, 7)]);
        assert_eq!(seen(&facts), [(MIN, MAX, 7)]);
        assert_eq!(letters(&facts, &[MIN, 0, MAX]), [Some(7); 3]);
    }

    #[test]
    fn a_statement_holds_over_its_days_and_what_was_before_resumes() {
        let facts = painted(&[(Days::ALWAYS, 1), (days(20, 29), 2)]);
        assert_eq!(seen(&facts), [(MIN, 19, 1), (20, 29, 2), (30, MAX, 1)]);
        assert_eq!(letters(&facts, &[19, 20, 29, 30]), [Some(1), Some(2), Some(2), Some(1)]);
    }

    #[test]
    fn a_statement_that_covers_an_earlier_one_wholly_replaces_it() {
        let facts = painted(&[(Days::ALWAYS, 1), (days(20, 29), 2), (days(10, 40), 3)]);
        assert_eq!(seen(&facts), [(MIN, 9, 1), (10, 40, 3), (41, MAX, 1)]);
    }

    #[test]
    fn a_statement_inside_an_earlier_one_resumes_it_after() {
        let facts = painted(&[(days(10, 40), 2), (days(20, 29), 3)]);
        assert_eq!(seen(&facts), [(10, 19, 2), (20, 29, 3), (30, 40, 2)]);
    }

    #[test]
    fn statements_that_touch_are_adjacent_steps_and_merge_if_they_agree() {
        assert_eq!(seen(&painted(&[(days(10, 19), 1), (days(20, 29), 2)])), [(10, 19, 1), (20, 29, 2)]);
        assert_eq!(seen(&painted(&[(days(10, 19), 1), (days(20, 29), 1)])), [(10, 29, 1)]);
        assert_eq!(seen(&painted(&[(days(10, 19), 1), (days(30, 39), 1), (days(20, 29), 1)])), [(10, 39, 1)]);
    }

    #[test]
    fn painting_what_already_holds_changes_nothing() {
        let once = painted(&[(days(10, 29), 1)]);
        let twice = painted(&[(days(10, 29), 1), (days(10, 29), 1), (days(15, 20), 1)]);
        assert_eq!(seen(&once), seen(&twice));
        let undone = painted(&[(Days::ALWAYS, 1), (days(10, 29), 2), (days(10, 29), 1)]);
        assert_eq!(seen(&undone), [(MIN, MAX, 1)], "painting the old value back leaves no step");
    }

    #[test]
    fn an_unbounded_statement_holds_from_its_day_on() {
        let facts = painted(&[(from(100), 1), (days(150, 159), 2)]);
        assert_eq!(seen(&facts), [(100, 149, 1), (150, 159, 2), (160, MAX, 1)]);
        assert_eq!(letters(&facts, &[MAX, 160]), [Some(1), Some(1)]);
    }

    #[test]
    fn before_a_first_statement_and_after_a_window_with_nothing_to_resume_nothing_is_said() {
        let facts = painted(&[(days(10, 19), 5)]);
        assert_eq!(letters(&facts, &[MIN, 9, 10, 19, 20, MAX]), [None, None, Some(5), Some(5), None, None]);
        assert_eq!(seen(&facts), [(10, 19, 5)], "the gaps are not steps");
        let steps = facts.steps(LETTER, 0);
        assert_eq!(steps.len(), 1, "and are not counted");
    }

    #[test]
    fn a_later_declaration_under_an_earlier_window_fills_the_gaps() {
        let facts = painted(&[(days(10, 19), 5), (Days::ALWAYS, 1)]);
        assert_eq!(seen(&facts), [(MIN, MAX, 1)], "a later statement overrides the days it covers, all of them");
    }

    #[test]
    fn a_slot_said_of_no_holder_and_a_holder_that_said_nothing_hold_nothing() {
        let mut builder = Facts::builder(5);
        builder.paint_always(1, LETTER, 3);
        builder.paint_always(3, LETTER, 4);
        let facts = builder.freeze();
        assert_eq!(facts.holders(), 5);
        for holder in [0, 2, 4] {
            assert_eq!(facts.at(LETTER, holder, Day(0)), None, "holder {holder} said nothing");
            assert_eq!(facts.steps(LETTER, holder).len(), 0);
        }
        assert_eq!((facts.at(LETTER, 1, Day(0)), facts.at(LETTER, 3, Day(0))), (Some(3), Some(4)));
        assert_eq!(facts.at(OTHER, 1, Day(0)), None, "a slot nobody set");
        assert_eq!(Facts::builder(0).freeze().holders(), 0);
    }

    #[test]
    fn statements_come_in_any_order_of_holders_and_slots_and_each_slot_is_painted_in_its_own() {
        let mut builder = Facts::builder(3);
        builder.paint(2, OTHER, days(0, 9), 20);
        builder.paint(0, LETTER, Days::ALWAYS, 1);
        builder.paint(2, LETTER, from(5), 2);
        builder.paint(0, OPENED, Days::ALWAYS, Day(100));
        builder.paint(0, LETTER, days(5, 9), 3);
        builder.paint(2, LETTER, days(7, 8), 4);
        builder.paint(2, OTHER, days(5, 20), 21);
        let facts = builder.freeze();
        let letters = |holder, key: Key<u32>| -> Vec<(i32, i32, u32)> {
            facts.steps(key, holder).map(|(days, v)| (days.first().0, days.last().0, v)).collect()
        };
        assert_eq!(letters(0, LETTER), [(MIN, 4, 1), (5, 9, 3), (10, MAX, 1)]);
        assert_eq!(letters(2, LETTER), [(5, 6, 2), (7, 8, 4), (9, MAX, 2)]);
        assert_eq!(letters(2, OTHER), [(0, 4, 20), (5, 20, 21)]);
        assert_eq!(facts.at(OPENED, 0, Day(-5)), Some(Day(100)));
    }

    #[test]
    fn a_holder_with_every_slot_finds_each() {
        let mut builder = Facts::builder(2);
        let keys: Vec<Key<u32>> = (0..50).map(|slot| Key::new(SlotId(slot))).collect();
        for (i, &key) in keys.iter().enumerate().rev() {
            builder.paint_always(1, key, i as u32 * 10);
        }
        let facts = builder.freeze();
        assert!(keys.iter().enumerate().all(|(i, &key)| facts.at(key, 1, Day(0)) == Some(i as u32 * 10)));
        assert!(keys.iter().all(|&key| facts.at(key, 0, Day(0)).is_none()));
        assert_eq!(facts.at(Key::<u32>::new(SlotId(50)), 1, Day(0)), None);
    }

    #[test]
    fn steps_run_forwards_and_backwards_and_count() {
        let facts = painted(&[(Days::ALWAYS, 1), (days(10, 19), 2), (days(30, 39), 3)]);
        let forwards: Vec<_> = facts.steps(LETTER, 0).collect();
        let mut backwards: Vec<_> = facts.steps(LETTER, 0).rev().collect();
        backwards.reverse();
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.len(), 5);
        let mut steps = facts.steps(LETTER, 0);
        assert_eq!(steps.len(), 5);
        assert_eq!(steps.next().map(|(days, v)| (days.last().0, v)), Some((9, 1)));
        assert_eq!(steps.next_back().map(|(days, v)| (days.first().0, v)), Some((40, 1)));
        assert_eq!(steps.len(), 3);
        assert_eq!(steps.size_hint(), (3, Some(3)));
        let rest: Vec<u32> = steps.map(|(_, v)| v).collect();
        assert_eq!(rest, [2, 1, 3]);
    }

    #[test]
    fn steps_that_meet_a_window_are_whole_and_the_ends_are_right_from_either_side() {
        let facts = painted(&[(Days::ALWAYS, 1), (days(10, 19), 2), (days(30, 39), 3)]);
        let meeting = |window: Days| -> Vec<(i32, i32, u32)> {
            facts.steps(LETTER, 0).within(window).map(|(d, v)| (d.first().0, d.last().0, v)).collect()
        };
        assert_eq!(meeting(days(12, 15)), [(10, 19, 2)]);
        assert_eq!(meeting(days(19, 20)), [(10, 19, 2), (20, 29, 1)]);
        assert_eq!(meeting(Days::on(Day(35))), [(30, 39, 3)]);
        assert_eq!(meeting(Days::on(Day(MIN))), [(MIN, 9, 1)]);
        assert_eq!(meeting(Days::on(Day(MAX))), [(40, MAX, 1)]);
        assert_eq!(meeting(Days::ALWAYS).len(), 5);
        let narrowed = facts.steps(LETTER, 0).within(days(15, 35));
        assert_eq!(narrowed.clone().rev().map(|(d, _)| d.last().0).collect::<Vec<_>>(), [39, 29, 19]);
        assert_eq!(narrowed.len(), 3);
        let after_every_step = facts.steps(LETTER, 0).within(days(0, 0)).within(days(5, 6));
        assert_eq!(after_every_step.map(|(d, v)| (d.last().0, v)).collect::<Vec<_>>(), [(9, 1)]);
    }

    #[test]
    fn the_days_a_condition_holds_are_exact_and_adjacent_steps_that_satisfy_it_make_one_interval() {
        let facts = painted(&[(Days::ALWAYS, 1), (days(10, 19), 2), (days(20, 29), 3), (days(40, 49), 2)]);
        let odd = |letter: u32| letter % 2 == 1;
        let even = |letter: u32| letter.is_multiple_of(2);
        let intervals = |set: DaySet| set.intervals().iter().map(|d| (d.first().0, d.last().0)).collect::<Vec<_>>();
        assert_eq!(intervals(facts.days_where(LETTER, 0, Days::ALWAYS, odd)), [(MIN, 9), (20, 39), (50, MAX)]);
        assert_eq!(intervals(facts.days_where(LETTER, 0, Days::ALWAYS, even)), [(10, 19), (40, 49)]);
        let within = days(5, 45);
        assert_eq!(intervals(facts.days_where(LETTER, 0, within, odd)), [(5, 9), (20, 39)], "clipped to the window");
        assert_eq!(intervals(facts.days_where(LETTER, 0, days(12, 15), even)), [(12, 15)]);
        assert_eq!(facts.days_where(LETTER, 0, days(12, 15), odd).len(), 0);
        let nothing = facts.days_where(LETTER, 0, Days::ALWAYS, |_| false);
        assert!(nothing.is_empty());
        assert!(facts.days_where(OTHER, 0, Days::ALWAYS, |_: u32| true).is_empty(), "a slot nobody set");
    }

    #[test]
    fn the_days_where_nothing_is_said_are_never_days_where_it_holds() {
        let facts = painted(&[(days(10, 19), 1), (days(30, 39), 1)]);
        let set = facts.days_where(LETTER, 0, Days::ALWAYS, |_| true);
        assert_eq!(set.intervals(), [days(10, 19), days(30, 39)]);
    }

    #[test]
    fn a_condition_over_all_time_counts_every_day_there_is() {
        let facts = painted(&[(Days::ALWAYS, 1)]);
        assert_eq!(facts.days_where(LETTER, 0, Days::ALWAYS, |_| true).len(), 1 << 32);
    }

    #[test]
    fn a_store_is_the_same_whatever_the_order_statements_of_different_slots_come_in() {
        let statements =
            [(0, LETTER, days(0, 9), 1), (0, LETTER, days(5, 20), 2), (1, LETTER, from(3), 3), (0, OTHER, from(0), 9)];
        let build = |order: &[usize]| {
            let mut builder = Facts::builder(2);
            for &at in order {
                let (holder, key, days, value) = statements[at];
                builder.paint(holder, key, days, value);
            }
            builder.freeze()
        };
        let (one, other) = (build(&[0, 1, 2, 3]), build(&[2, 3, 0, 1]));
        for (holder, key) in [(0, LETTER), (1, LETTER), (0, OTHER), (1, OTHER)] {
            assert_eq!(one.steps(key, holder).collect::<Vec<_>>(), other.steps(key, holder).collect::<Vec<_>>());
        }
        let again = build(&[0, 1, 2, 3]);
        assert_eq!(again.steps(LETTER, 0).collect::<Vec<_>>(), one.steps(LETTER, 0).collect::<Vec<_>>());
    }

    #[test]
    fn freezing_leaves_the_builder_to_take_more_statements() {
        let mut builder = Facts::builder(1);
        builder.paint_always(0, LETTER, 1);
        let before = builder.freeze();
        builder.paint(0, LETTER, from(10), 2);
        let after = builder.freeze();
        assert_eq!((before.at(LETTER, 0, Day(20)), after.at(LETTER, 0, Day(20))), (Some(1), Some(2)));
    }

    #[test]
    fn a_key_is_a_slot_and_a_type_and_costs_what_the_slot_does() {
        const CONSTANT: Key<Day> = Key::new(SlotId(9));
        assert_eq!(CONSTANT.slot(), SlotId(9));
        assert_eq!(Key::<Day>::new(SlotId(9)), CONSTANT);
        assert_ne!(Key::<Day>::new(SlotId(8)), CONSTANT);
        assert_eq!(format!("{CONSTANT:?}"), "Key<axiom_core::day::Day>#9");
        struct NotCopy;
        let key: Key<NotCopy> = Key::new(SlotId(1));
        let copy = key;
        assert_eq!(key, copy, "a key is Copy and Eq whatever its type is");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "is not a")]
    fn a_wrong_claim_is_caught_in_debug_builds() {
        let mut builder = Facts::builder(1);
        builder.paint_always(0, OPENED, Day(5));
        let _: Option<u32> = builder.freeze().at(Key::new(OPENED.slot()), 0, Day(0));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a slot holds one type")]
    fn one_slot_painted_with_two_types_is_caught_in_debug_builds() {
        let mut builder = Facts::builder(1);
        builder.paint_always(0, OPENED, Day(5));
        builder.paint_always(0, Key::<u32>::new(OPENED.slot()), 5);
        builder.freeze();
    }

    #[test]
    #[should_panic(expected = "holder 3 of a store of 3")]
    fn a_holder_the_store_does_not_have_is_refused_when_painted() {
        Facts::builder(3).paint_always(3, LETTER, 1);
    }

    #[test]
    fn the_layout_is_the_one_the_design_commits_to() {
        assert_eq!(size_of::<Entry>(), 8);
        assert_eq!(size_of::<Key<Day>>(), size_of::<SlotId>());
        assert_eq!(size_of::<Steps<'_, Day>>(), 32);
        assert_eq!(size_of::<(Day, Datum)>(), 32);
    }
}
