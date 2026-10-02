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
//! holder, then each (holder, slot) group painted in arrival order into a small reused vector, and written out. The
//! holders do not depend on each other, so they are painted and written in chunks on every core, and the chunks laid
//! end to end.
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
use crate::hash::Map;
use crate::id::{Id, Run};
use crate::par;
use crate::tagless::{Column, Datum, Field, Payload, Tag, sealed};
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

/// A set of `V`s as the value of a step: a slot that holds many things at once, such as the systems a person lives
/// in or the owners of a position.
///
/// It is a handle: where the members are in the store's second column. They are sorted and without repeats, and each
/// set is stored once, so within a store two equal sets are the same handle, are equal as values and merge as steps
/// do. Open it with [`Facts::members`] of the store it came from; a handle of another store is a wrong claim like a
/// wrong key, and gives other members, or panics.
pub struct Many<V> {
    run: Run<V>,
}

const _: () = assert!(size_of::<Many<u32>>() == 8);

impl<V> Many<V> {
    fn at(start: u32, len: u32) -> Many<V> {
        Many { run: Run::new(Id::new(start), len) }
    }
}

impl<V> Clone for Many<V> {
    fn clone(&self) -> Many<V> {
        *self
    }
}

impl<V> Copy for Many<V> {}

impl<V> PartialEq for Many<V> {
    fn eq(&self, other: &Many<V>) -> bool {
        self.run == other.run
    }
}

impl<V> Eq for Many<V> {}

impl<V> fmt::Debug for Many<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Many<{}>{:?}", std::any::type_name::<V>(), self.run)
    }
}

impl<V> sealed::Sealed for Many<V> {}

/// A run in the payload, as it is for [`Run`]: one more way to say it, in the type of the key.
impl<V> Field for Many<V> {
    const TAG: Tag = Tag::Run;

    fn put(self) -> Payload {
        self.run.put()
    }

    fn pull(payload: Payload) -> Many<V> {
        Many { run: Run::pull(payload) }
    }
}

/// The members of a set, in order, borrowed from the store's second column: what [`Facts::members`] gives.
pub struct Members<'a, V> {
    column: &'a Column,
    range: Range<u32>,
    of: PhantomData<fn() -> V>,
}

const _: () = assert!(size_of::<Members<'_, u32>>() == 16);

impl<V: Field> Members<'_, V> {
    /// Whether `member` is in the set, by the equality of values that merging uses. A scan: a set is a handful.
    pub fn contains(&self, member: V) -> bool {
        let member = Datum::of(member);
        self.range.clone().any(|at| self.column.datum(at) == member)
    }
}

impl<V: Field> Iterator for Members<'_, V> {
    type Item = V;

    fn next(&mut self) -> Option<V> {
        self.range.next().map(|at| self.column.get(at))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.range.size_hint()
    }
}

impl<V: Field> DoubleEndedIterator for Members<'_, V> {
    fn next_back(&mut self) -> Option<V> {
        self.range.next_back().map(|at| self.column.get(at))
    }
}

impl<V: Field> ExactSizeIterator for Members<'_, V> {}

impl<V> Clone for Members<'_, V> {
    fn clone(&self) -> Self {
        Members { column: self.column, range: self.range.clone(), of: PhantomData }
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
    /// The members of every set that a step holds, each set stored once: what a [`Many`] points into.
    members: Column,
}

impl Facts {
    /// A builder for a store of `holders` rows, numbered from zero.
    pub fn builder(holders: usize) -> Builder {
        Builder {
            holders: u32::try_from(holders).expect("fewer than 2^32 holders"),
            statements: Vec::new(),
            sets: Sets::default(),
        }
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

    /// What holds of `key` on `day` for the first of `chain` that says anything then: a thing, then the kinds it
    /// descends from, nearest first. A kind's facts are the defaults of its things, and the tree of kinds is the
    /// model's, which hands its ancestors over as holder numbers; the store needs no tree.
    pub fn at_first<V: Field>(&self, key: Key<V>, chain: impl IntoIterator<Item = u32>, day: Day) -> Option<V> {
        chain.into_iter().find_map(|holder| self.at(key, holder, day))
    }

    /// The members of `set`, a value of this store.
    pub fn members<V: Field>(&self, set: Many<V>) -> Members<'_, V> {
        let start = set.run.start().index() as u32;
        let range = start..start + set.run.len();
        debug_assert!(range.end as usize <= self.members.len(), "a set of this store");
        Members { column: &self.members, range, of: PhantomData }
    }

    /// Where the steps of `slot` are among the store's, if `holder` said anything of it.
    fn steps_of(&self, slot: SlotId, holder: u32) -> Option<Range<usize>> {
        let (start, end) = (self.rows[holder as usize] as usize, self.rows[holder as usize + 1] as usize);
        let found = start + find_slot(&self.entries[start..end], slot)?;
        // The entry after a row's last is the next row's first, or the last of all: it says where the steps end.
        Some(self.entries[found].first as usize..self.entries[found + 1].first as usize)
    }
}

/// The longest row that is scanned for a slot; a longer one is searched. Where the two cross in the benchmark in the
/// tests, which has the scan faster by half up to 6 entries, even at 12, and the search faster by a third at 40 and by
/// 2.5 times at 200: a thing says a handful of its slots, but a kind of thing may say many.
const SCAN_UP_TO: usize = 16;

/// Where `slot` is in a row, whose entries are sorted by slot.
fn find_slot(row: &[Entry], slot: SlotId) -> Option<usize> {
    if row.len() <= SCAN_UP_TO {
        return row.iter().position(|entry| entry.slot == slot);
    }
    let at = row.partition_point(|entry| entry.slot < slot);
    row.get(at).is_some_and(|entry| entry.slot == slot).then_some(at)
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

/// One statement: whose slot it is about, and what it paints there. One record, so that reading a statement is one
/// miss and not one for each of its parts; and `holder` fills the four bytes the rest would leave empty.
#[derive(Clone, Copy)]
struct Statement {
    holder: u32,
    slot: SlotId,
    days: Days,
    value: Datum,
}

const _: () = assert!(size_of::<Statement>() == 40);

/// Paints the statements `group`, which are about one slot of one holder, in the order they are given: onto nothing,
/// each over its days.
fn paint(group: &[Statement], steps: &mut Vec<(Day, Datum)>) {
    debug_assert!(group.windows(2).all(|pair| pair[0].value.tag() == pair[1].value.tag()), "a slot holds one type");
    steps.clear();
    steps.push((Day::MIN, Datum::EMPTY));
    for statement in group {
        paint_steps(steps, statement.days, statement.value);
    }
}

/// Statements that have been made, to be frozen into a [`Facts`].
///
/// Statements are painted in the order they were made, whatever holders and slots they were about: a later one
/// overrides an earlier one over the days it covers, and what held before resumes after it.
pub struct Builder {
    holders: u32,
    statements: Vec<Statement>,
    sets: Sets,
}

/// The sets that statements hold, each once: the second column of the store, and where each set is in it.
#[derive(Default)]
struct Sets {
    members: Column,
    /// A set's members, sorted, to where they begin in `members`.
    interned: Map<Box<[Datum]>, u32>,
}

impl Sets {
    /// The set of `members`, which are put in order and without repeats, and stored if it was not yet.
    fn intern<V: Field>(&mut self, members: impl IntoIterator<Item = V>) -> Many<V> {
        let mut sorted: Vec<Datum> = members.into_iter().map(Datum::of).collect();
        sorted.sort_unstable();
        sorted.dedup();
        let start = self.interned.get(sorted.as_slice()).copied().unwrap_or_else(|| self.store(&sorted));
        Many::at(start, len32(sorted.len()))
    }

    /// Appends a set that is not yet stored, and says where it went.
    fn store(&mut self, sorted: &[Datum]) -> u32 {
        let start = len32(self.members.len());
        for &member in sorted {
            self.members.push_datum(member);
        }
        self.interned.insert(sorted.into(), start);
        start
    }
}

/// How many holders are frozen as one piece of work.
const CHUNK: usize = 1 << 10;

impl Builder {
    /// `value` holds of `holder`'s `key` over `days`: `from … until …`, or `Days::new(from, Day::MAX)` from a day on.
    pub fn paint<V: Field>(&mut self, holder: u32, key: Key<V>, days: Days, value: V) {
        assert!(holder < self.holders, "holder {holder} of a store of {}", self.holders);
        self.statements.push(Statement { holder, slot: key.slot, days, value: Datum::of(value) });
    }

    /// The set of `members` holds of `holder`'s `key` over `days`. Order and repeats do not matter: a set is the same
    /// set, and merges with the same set next to it.
    pub fn paint_many<V: Field>(
        &mut self,
        holder: u32,
        key: Key<Many<V>>,
        days: Days,
        members: impl IntoIterator<Item = V>,
    ) {
        let set = self.sets.intern(members);
        self.paint(holder, key, days, set);
    }

    /// `value` holds of `holder`'s `key` from the beginning of time: a declaration.
    pub fn paint_always<V: Field>(&mut self, holder: u32, key: Key<V>, value: V) {
        self.paint(holder, key, Days::ALWAYS, value);
    }

    /// The store of every statement made so far. A function of the statements alone, which the builder keeps: a
    /// live edit makes more of them and freezes again, and gets what a build of all of them would give.
    pub fn freeze(&self) -> Facts {
        self.freeze_in_chunks(CHUNK)
    }

    /// One counting sort of the statements by holder, and then the holders in chunks of `per_chunk`, each painted
    /// and written as a store of its own, and the stores laid end to end. Holders do not depend on each other, so
    /// the chunks are frozen on every core.
    fn freeze_in_chunks(&self, per_chunk: usize) -> Facts {
        assert!(u32::try_from(self.statements.len()).is_ok(), "fewer than 2^32 statements");
        let holders = self.holders as usize;
        let by_holder = ByHolder::new(&self.statements, holders);
        let chunks: Vec<Range<usize>> =
            (0..holders).step_by(per_chunk).map(|first| first..holders.min(first + per_chunk)).collect();
        let mut facts = Facts::with_room_for(holders, self.statements.len()).finish();
        par::map_each_ordered(&chunks, |chunk| by_holder.freeze(chunk.clone()), |part| facts.append(part));
        facts.members = self.sets.members.clone();
        facts
    }
}

/// The statements sorted by holder, as the positions of each holder's statements in the order they were made.
struct ByHolder<'a> {
    statements: &'a [Statement],
    /// Holder to where its statements begin in `order`; one more at the end.
    starts: Vec<u32>,
    order: Vec<u32>,
}

impl<'a> ByHolder<'a> {
    fn new(statements: &'a [Statement], holders: usize) -> ByHolder<'a> {
        let (starts, order) = bucket(holders, statements.len(), |at| statements[at].holder as usize);
        ByHolder { statements, starts, order }
    }

    /// The store of the holders `holders`, whose rows are numbered from zero.
    fn freeze(&self, holders: Range<usize>) -> Facts {
        let said = |holder: usize| &self.order[self.starts[holder] as usize..self.starts[holder + 1] as usize];
        let statements_in = (self.starts[holders.end] - self.starts[holders.start]) as usize;
        let mut facts = Facts::with_room_for(holders.len(), statements_in);
        let (mut row, mut painted) = (Vec::new(), Vec::new());
        for holder in holders {
            row.clear();
            row.extend(said(holder).iter().map(|&at| self.statements[at as usize]));
            // Stable, so that a slot's statements stay in the order they were made.
            row.sort_by_key(|statement| statement.slot);
            for group in row.chunk_by(|a, b| a.slot == b.slot) {
                paint(group, &mut painted);
                facts.push_entry(group[0].slot, &painted);
            }
            facts.end_row();
        }
        facts.finish()
    }
}

/// The writer side of [`Facts`], for `freeze`: entries and steps appended in order, row by row, to a store that is
/// finished when the last row is.
impl Facts {
    fn with_room_for(holders: usize, statements: usize) -> Facts {
        let mut rows = Vec::with_capacity(holders + 1);
        rows.push(0);
        Facts {
            rows,
            entries: Vec::with_capacity(statements + 1),
            days: Vec::with_capacity(statements),
            values: Column::with_capacity(statements),
            members: Column::new(),
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

    /// Lays the holders of `part` after those of this store.
    fn append(&mut self, part: Facts) {
        let (entries, steps) = (len32(self.entries.len() - 1), len32(self.days.len()));
        // This store's last entry said where its steps end, which the first of `part` now does.
        self.entries.pop();
        self.rows.extend(part.rows[1..].iter().map(|row| row + entries));
        self.entries.extend(part.entries.iter().map(|entry| Entry { first: entry.first + steps, ..*entry }));
        self.days.extend_from_slice(&part.days);
        self.values.extend(&part.values);
    }
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 entries and steps")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Rng, best_of};

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

    const LIVES: Key<Many<u32>> = Key::new(SlotId(3));
    const OWNERS: Key<Many<u32>> = Key::new(SlotId(4));

    /// The members of the set that holds of the first holder's `LIVES` over each stretch of days it holds.
    fn lived(facts: &Facts) -> Vec<(i32, i32, Vec<u32>)> {
        let seen = facts.steps(LIVES, 0);
        seen.map(|(days, set)| (days.first().0, days.last().0, facts.members(set).collect())).collect()
    }

    #[test]
    fn a_set_is_a_value_whose_members_are_in_order_and_without_repeats() {
        let mut builder = Facts::builder(1);
        builder.paint_many(0, LIVES, Days::ALWAYS, [30, 10, 20, 10, 30]);
        let facts = builder.freeze();
        assert_eq!(lived(&facts), [(MIN, MAX, vec![10, 20, 30])]);
        let set = facts.at(LIVES, 0, Day(0)).unwrap();
        let members = facts.members(set);
        assert_eq!(members.len(), 3);
        assert_eq!(members.clone().rev().collect::<Vec<_>>(), [30, 20, 10]);
        assert!(members.contains(20) && !members.contains(25));
        assert_eq!(facts.at(LIVES, 0, Day(MAX)), Some(set));
    }

    #[test]
    fn equal_sets_are_the_same_value_whatever_order_they_were_said_in_and_merge() {
        let mut builder = Facts::builder(2);
        builder.paint_many(0, LIVES, days(0, 9), [2, 1]);
        builder.paint_many(0, LIVES, days(10, 19), [1, 2, 2]);
        builder.paint_many(0, LIVES, days(20, 29), [1]);
        builder.paint_many(1, OWNERS, Days::ALWAYS, [1, 2]);
        let facts = builder.freeze();
        assert_eq!(lived(&facts), [(0, 19, vec![1, 2]), (20, 29, vec![1])], "the first two touch, and agree");
        let (here, there) = (facts.at(LIVES, 0, Day(5)), facts.at(OWNERS, 1, Day(5)));
        assert_eq!(here, there, "one set of the store, kept once, whatever slot and holder says it");
        assert_ne!(here, facts.at(LIVES, 0, Day(25)));
        assert_eq!(facts.members(here.unwrap()).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn the_empty_set_is_a_value_and_nothing_said_is_not() {
        let mut builder = Facts::builder(1);
        builder.paint_many(0, LIVES, days(10, 19), []);
        builder.paint_many(0, LIVES, days(30, 39), [4]);
        let facts = builder.freeze();
        assert_eq!(lived(&facts), [(10, 19, vec![]), (30, 39, vec![4])]);
        let nobody = facts.at(LIVES, 0, Day(15)).expect("the empty set holds");
        assert_eq!((facts.members(nobody).len(), facts.at(LIVES, 0, Day(25))), (0, None));
        assert_eq!(facts.steps(LIVES, 0).len(), 2);
    }

    #[test]
    fn the_days_a_person_lived_in_a_system_are_the_days_a_set_holds_it() {
        let mut builder = Facts::builder(1);
        builder.paint_many(0, LIVES, Days::ALWAYS, [1]);
        builder.paint_many(0, LIVES, days(100, 199), [1, 2]);
        builder.paint_many(0, LIVES, days(200, 299), [2]);
        builder.paint_many(0, LIVES, from(300), [1, 3]);
        let facts = builder.freeze();
        let lived_in =
            |system: u32, within: Days| facts.days_where(LIVES, 0, within, |set| facts.members(set).contains(system));
        let intervals = |set: DaySet| set.intervals().iter().map(|d| (d.first().0, d.last().0)).collect::<Vec<_>>();
        assert_eq!(intervals(lived_in(1, Days::ALWAYS)), [(MIN, 199), (300, MAX)]);
        assert_eq!(intervals(lived_in(2, Days::ALWAYS)), [(100, 299)]);
        assert_eq!(intervals(lived_in(2, days(250, 350))), [(250, 299)]);
        assert_eq!(intervals(lived_in(3, days(0, 365))), [(300, 365)]);
        assert!(lived_in(4, Days::ALWAYS).is_empty());
    }

    #[test]
    fn sets_of_different_types_that_have_the_same_bytes_stay_different_sets() {
        const DAYS: Key<Many<Day>> = Key::new(SlotId(5));
        let mut builder = Facts::builder(1);
        builder.paint_many(0, LIVES, Days::ALWAYS, [1, 2]);
        builder.paint_many(0, DAYS, Days::ALWAYS, [Day(1), Day(2)]);
        let facts = builder.freeze();
        let (letters, dates) = (facts.at(LIVES, 0, Day(0)).unwrap(), facts.at(DAYS, 0, Day(0)).unwrap());
        assert_eq!(facts.members(letters).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(facts.members(dates).collect::<Vec<_>>(), [Day(1), Day(2)]);
    }

    #[test]
    fn a_set_slot_and_a_plain_slot_of_one_holder_do_not_disturb_each_other() {
        let mut builder = Facts::builder(1);
        builder.paint_always(0, LETTER, 7);
        builder.paint_many(0, LIVES, Days::ALWAYS, [7]);
        builder.paint(0, LETTER, days(5, 6), 8);
        let facts = builder.freeze();
        assert_eq!(seen(&facts), [(MIN, 4, 7), (5, 6, 8), (7, MAX, 7)]);
        assert_eq!(lived(&facts), [(MIN, MAX, vec![7])]);
    }

    #[test]
    fn a_chain_of_holders_falls_back_to_the_first_that_says_something_that_day() {
        // A thing, the kind it is, and the kind that kind is: holders 0, 1, 2.
        let mut builder = Facts::builder(3);
        builder.paint(0, LETTER, from(10), 9);
        builder.paint_always(1, LETTER, 5);
        builder.paint_always(2, OTHER, 7);
        builder.paint_always(2, LETTER, 1);
        let facts = builder.freeze();
        let letter = |day: i32| facts.at_first(LETTER, [0, 1, 2], Day(day));
        assert_eq!((letter(0), letter(10)), (Some(5), Some(9)), "the thing says nothing until day 10");
        assert_eq!(facts.at_first(OTHER, [0, 1, 2], Day(0)), Some(7), "the nearest kind to say it");
        assert_eq!(facts.at_first(OTHER, [0, 1], Day(0)), None);
        assert_eq!(facts.at_first(LETTER, [], Day(0)), None, "an empty chain says nothing");
        assert_eq!(facts.at_first(LETTER, 1..3, Day(0)), Some(5), "any iterator of holders is a chain");
        assert_eq!(facts.at_first(Key::<u32>::new(SlotId(99)), [0, 1, 2], Day(0)), None);
    }

    /// One statement of a generated book: `(holder, slot, days, letter)`.
    type Statement = (u32, u32, Days, u32);

    /// A book of `holders` things that each say one to five of `SLOTS` slots: a declaration, and for the slots that
    /// the dice say so, one more statement from a later day. About three slots and two steps a slot, in random order.
    fn generated_book(holders: u32, rng: &mut Rng) -> Vec<Statement> {
        const SLOTS: u32 = 8;
        let (mut declarations, mut later) = (Vec::new(), Vec::new());
        for holder in 0..holders {
            let said = 1 + rng.below(5) as u32;
            for slot in (0..SLOTS).cycle().skip(rng.below(SLOTS as usize)).step_by(3).take(said as usize) {
                declarations.push((holder, slot, Days::ALWAYS, rng.below(1000) as u32));
                if rng.chance(70) {
                    later.push((holder, slot, from(rng.below(3000) as i32), rng.below(1000) as u32));
                }
            }
        }
        for statements in [&mut declarations, &mut later] {
            for at in (1..statements.len()).rev() {
                statements.swap(at, rng.below(at + 1));
            }
        }
        declarations.extend(later);
        declarations
    }

    /// The obvious alternative: for each holder, a vector for each slot, of `(day, letter)`, painted by the same
    /// function. `NOTHING` is a gap.
    struct Nested(Vec<Vec<Vec<(Day, u32)>>>);

    const NOTHING: u32 = u32::MAX;

    impl Nested {
        fn build(holders: u32, book: &[Statement]) -> Nested {
            let mut rows = vec![vec![Vec::new(); 8]; holders as usize];
            for &(holder, slot, days, letter) in book {
                let steps = &mut rows[holder as usize][slot as usize];
                if steps.is_empty() {
                    steps.push((Day::MIN, NOTHING));
                }
                paint_steps(steps, days, letter);
            }
            Nested(rows)
        }

        fn at(&self, holder: u32, slot: u32, day: Day) -> Option<u32> {
            let steps = &self.0[holder as usize][slot as usize];
            let begun = steps.partition_point(|&(from, _)| from <= day);
            steps.get(begun.checked_sub(1)?).map(|&(_, letter)| letter).filter(|&letter| letter != NOTHING)
        }

        /// Bytes of vector headers and of what they hold, not counting the allocator's own, and how many allocations.
        fn bytes(&self) -> (usize, usize) {
            let (mut bytes, mut allocations) = (size_of::<Vec<Vec<Vec<(Day, u32)>>>>(), 0);
            for row in &self.0 {
                bytes += size_of::<Vec<Vec<(Day, u32)>>>() + row.capacity() * size_of::<Vec<(Day, u32)>>();
                allocations += 1;
                for steps in row.iter().filter(|steps| steps.capacity() > 0) {
                    bytes += steps.capacity() * size_of::<(Day, u32)>();
                    allocations += 1;
                }
            }
            (bytes, allocations)
        }
    }

    /// `cargo test -p axiom-core --release facts::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_a_million_holders_built_and_read() {
        const HOLDERS: u32 = 1_000_000;
        const READS: usize = 4_000_000;
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        let book = generated_book(HOLDERS, &mut rng);
        let keys: Vec<Key<u32>> = (0..8).map(|slot| Key::new(SlotId(slot))).collect();
        let per = |time: std::time::Duration, count: usize| time.as_nanos() as f64 / count as f64;

        let painting = best_of(3, || {
            let mut builder = Facts::builder(HOLDERS as usize);
            for &(holder, slot, days, letter) in &book {
                builder.paint(holder, keys[slot as usize], days, letter);
            }
            builder
        });
        let mut builder = Facts::builder(HOLDERS as usize);
        for &(holder, slot, days, letter) in &book {
            builder.paint(holder, keys[slot as usize], days, letter);
        }
        let freezing = best_of(3, || builder.freeze());
        let nested_build = best_of(3, || Nested::build(HOLDERS, &book));
        let facts = builder.freeze();
        let nested = Nested::build(HOLDERS, &book);

        let (entries, steps) = (facts.entries.len() - 1, facts.days.len());
        let facts_bytes = facts.rows.len() * 4 + facts.entries.len() * 8 + steps * 21;
        let (nested_bytes, nested_allocations) = nested.bytes();
        eprintln!(
            "{} statements for {HOLDERS} holders: {entries} entries, {steps} steps ({:.2} slots a holder, {:.2} steps a slot)",
            book.len(),
            entries as f64 / f64::from(HOLDERS),
            steps as f64 / entries as f64
        );
        eprintln!(
            "  build: paint {:.0} ms, freeze {:.0} ms ({:.0} ns a statement); nested vectors {:.0} ms",
            painting.as_secs_f64() * 1e3,
            freezing.as_secs_f64() * 1e3,
            per(freezing, book.len()),
            nested_build.as_secs_f64() * 1e3
        );
        eprintln!(
            "  size: facts {:.1} MB = {:.1} bytes a step ({:.1} with the statements' slack), nested {:.1} MB = {:.1} bytes a step in {nested_allocations} allocations",
            facts_bytes as f64 / 1e6,
            facts_bytes as f64 / steps as f64,
            (facts.rows.capacity() * 4 + facts.entries.capacity() * 8 + facts.days.capacity() * 21) as f64
                / steps as f64,
            nested_bytes as f64 / 1e6,
            nested_bytes as f64 / steps as f64
        );

        let queries: Vec<(u32, u32, Day)> = (0..READS)
            .map(|_| (rng.below(HOLDERS as usize) as u32, rng.below(8) as u32, Day(rng.below(4000) as i32)))
            .collect();
        let mut sorted = queries.clone();
        sorted.sort_unstable_by_key(|&(holder, slot, _)| (holder, slot));
        let sum_facts = |queries: &[(u32, u32, Day)]| {
            queries.iter().map(|&(h, s, day)| facts.at(keys[s as usize], h, day).map_or(0, u64::from)).sum::<u64>()
        };
        let sum_nested = |queries: &[(u32, u32, Day)]| {
            queries.iter().map(|&(h, s, day)| nested.at(h, s, day).map_or(0, u64::from)).sum::<u64>()
        };
        assert_eq!(sum_facts(&queries), sum_nested(&queries), "the two agree on every read");
        eprintln!(
            "  at, cold random holders: facts {:.1} ns, nested {:.1} ns; warm sweep in holder order: facts {:.1} ns, nested {:.1} ns",
            per(best_of(5, || sum_facts(&queries)), READS),
            per(best_of(5, || sum_nested(&queries)), READS),
            per(best_of(5, || sum_facts(&sorted)), READS),
            per(best_of(5, || sum_nested(&sorted)), READS)
        );
    }
}
