//! Dense state with an undo log: going back in time as cheap as going forward.
//!
//! The fold's mutable state lives in dense arrays (parcels, tallies, residuals) and every write to them goes through a
//! [`Trailed`], which can log the cell's old value as it writes. A [`Mark`] is the log's length and the array's, so
//! taking one costs nothing; [`Trail::undo_to`] puts back the old values in reverse, newest first, and cuts off
//! what was pushed since. The one mechanism is a staged write, a what-if, a forecast past today, a checkpoint to
//! return to and a live edit's rewind. A [`Fork`] is a mark held by a guard: it undoes on drop unless
//! [`Fork::keep`] commits it, and because it borrows the arrays mutably, nothing can read a hypothetical after it is
//! gone.
//!
//! # The invariant
//!
//! After `undo_to(mark)` every cell holds what it held when the mark was taken, and there are as many cells. Each
//! `set` since pushed the cell's old value, so replaying the log newest first leaves every cell as it was; each `push`
//! only lengthened the array, so cutting it back to the marked length undoes those. A push needs no entry of its own.
//!
//! # Zero cost when unused
//!
//! The log is a type parameter. `()` records nothing, and `Trailed<T, ()>` is a `Vec<T>` that writes like one: the
//! one-shot `check` pays nothing for the sessions' ability to go back. Marks and undo exist only where there is a log,
//! and ask for it in the types:
//!
//! ```compile_fail,E0599
//! use axiom_core::trail::{Trail, Trailed};
//! let check = Trailed::<u32, ()>::new(vec![1, 2, 3]);
//! check.mark(); // no log, so nothing to go back to
//! ```
//!
//! # Marks
//!
//! A mark is good for the trail that made it, until the trail is undone to an earlier mark. Both are checked when it
//! is used, as far as is cheap: every log is born with an identity, which its marks carry, and a mark the trail has
//! been undone past is refused while the trail is still shorter than it was. What is not caught is a mark kept across
//! an undo to an earlier one, once the trail has grown past it again. A [`Fork`] cannot make that mistake, for each is
//! inside the one before. A holder of marks, such as a list of checkpoints, must drop the ones it undoes past.
//!
//! The identity is a process-wide counter, the one global in the crate, and it only names trails. The type system
//! could do it instead, with a brand lifetime on every trail, mark and struct that holds one: a lifetime in every
//! signature of the engine, to save one comparison for each undo.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};

/// Where a trailed array records the value a write replaced. `()` does not.
pub trait Log<T> {
    fn record(&mut self, at: u32, old: T);
}

impl<T> Log<T> for () {
    #[inline(always)]
    fn record(&mut self, _: u32, _: T) {}
}

/// The undo log: the index and old value of every write.
pub struct Undo<T> {
    entries: Vec<(u32, T)>,
    /// Which trail this is, so that a mark made on another is known.
    id: u32,
}

impl<T> Default for Undo<T> {
    fn default() -> Undo<T> {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        Undo { entries: Vec::new(), id: NEXT_ID.fetch_add(1, Ordering::Relaxed) }
    }
}

impl<T> Log<T> for Undo<T> {
    #[inline]
    fn record(&mut self, at: u32, old: T) {
        self.entries.push((at, old));
    }
}

/// Dense state: an array of cells, and whatever log `L` keeps of writes to them.
pub struct Trailed<T: Copy, L: Log<T> = Undo<T>> {
    cells: Vec<T>,
    log: L,
}

impl<T: Copy, L: Log<T> + Default> Trailed<T, L> {
    pub fn new(cells: Vec<T>) -> Trailed<T, L> {
        Trailed { cells, log: L::default() }
    }
}

impl<T: Copy, L: Log<T>> Trailed<T, L> {
    pub fn get(&self, at: u32) -> T {
        self.cells[at as usize]
    }

    /// Writes `value` at `at`, and logs what it replaced.
    pub fn set(&mut self, at: u32, value: T) {
        let old = std::mem::replace(&mut self.cells[at as usize], value);
        self.log.record(at, old);
    }

    /// Appends a cell and says where it went.
    pub fn push(&mut self, value: T) -> u32 {
        let at = len32(self.cells.len());
        self.cells.push(value);
        at
    }

    /// All the cells, for a pass that reads them in order.
    pub fn as_slice(&self) -> &[T] {
        &self.cells
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 cells and log entries")
}

/// A point in a trail's past to go back to: how long its log and its array were.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    trail: u32,
    entries: u32,
    cells: u32,
}

/// State that can go back to a mark: a trailed array that has a log, or several of them together.
pub trait Trail {
    type Mark: Copy;

    fn mark(&self) -> Self::Mark;

    /// Puts everything back as it was at `mark`. The mark stays good, to go back to again; any made after it do not.
    fn undo_to(&mut self, mark: Self::Mark);

    /// A mark held by a guard that goes back to it when dropped.
    fn fork(&mut self) -> Fork<'_, Self>
    where
        Self: Sized,
    {
        Fork { mark: self.mark(), trail: self }
    }
}

impl<T: Copy> Trail for Trailed<T, Undo<T>> {
    type Mark = Mark;

    fn mark(&self) -> Mark {
        Mark { trail: self.log.id, entries: len32(self.log.entries.len()), cells: len32(self.cells.len()) }
    }

    fn undo_to(&mut self, mark: Mark) {
        assert_eq!(mark.trail, self.log.id, "a mark undoes only the trail that made it");
        let (entries, cells) = (mark.entries as usize, mark.cells as usize);
        assert!(entries <= self.log.entries.len() && cells <= self.cells.len(), "the trail was undone past this mark");
        for (at, old) in self.log.entries.drain(entries..).rev() {
            self.cells[at as usize] = old;
        }
        self.cells.truncate(cells);
    }
}

/// Arrays that move together (parcels, tallies, residuals) are marked and undone as one: a tuple of trails is a trail.
macro_rules! trails_in_tuples {
    ($(($($trail:ident $at:tt),+))+) => {$(
        impl<$($trail: Trail),+> Trail for ($($trail,)+) {
            type Mark = ($($trail::Mark,)+);

            fn mark(&self) -> Self::Mark {
                ($(self.$at.mark(),)+)
            }

            fn undo_to(&mut self, mark: Self::Mark) {
                $(self.$at.undo_to(mark.$at);)+
            }
        }
    )+};
}

trails_in_tuples! {
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
}

/// A trail, until dropped, and then as it was: every write through it is undone unless it is [kept](Fork::keep).
///
/// ```
/// use axiom_core::trail::{Trail, Trailed};
///
/// let mut balances = Trailed::<i64>::new(vec![10, 20]);
/// {
///     let mut what_if = balances.fork();
///     what_if.set(0, 99);
///     what_if.push(7);
///     assert_eq!(what_if.as_slice(), [99, 20, 7]);
/// }
/// assert_eq!(balances.as_slice(), [10, 20]);
///
/// let mut real = balances.fork();
/// real.set(1, 25);
/// real.keep();
/// assert_eq!(balances.as_slice(), [10, 25]);
/// ```
#[must_use = "a fork undoes what is written through it as soon as it is dropped"]
pub struct Fork<'t, X: Trail> {
    trail: &'t mut X,
    mark: X::Mark,
}

impl<X: Trail> Fork<'_, X> {
    /// Commits: what was written through the fork stays. An outer mark can still undo it.
    pub fn keep(self) {
        std::mem::forget(self);
    }
}

impl<X: Trail> Deref for Fork<'_, X> {
    type Target = X;

    fn deref(&self) -> &X {
        self.trail
    }
}

impl<X: Trail> DerefMut for Fork<'_, X> {
    fn deref_mut(&mut self) -> &mut X {
        self.trail
    }
}

impl<X: Trail> Drop for Fork<'_, X> {
    fn drop(&mut self) {
        self.trail.undo_to(self.mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use crate::testing::{Rng, best_of};

    type Cells = Trailed<i64>;

    #[test]
    fn undo_puts_back_old_values_and_cuts_off_pushes() {
        let mut cells = Cells::new(vec![1, 2, 3]);
        let start = cells.mark();
        cells.set(0, 10);
        cells.set(0, 11);
        let pushed = cells.push(4);
        cells.set(pushed, 40);
        let middle = cells.mark();
        cells.set(2, 30);
        assert_eq!(cells.as_slice(), [11, 2, 30, 40]);
        cells.undo_to(middle);
        assert_eq!(cells.as_slice(), [11, 2, 3, 40]);
        cells.undo_to(start);
        assert_eq!((cells.as_slice(), cells.len()), ([1, 2, 3].as_slice(), 3));
        cells.undo_to(start);
        assert_eq!(cells.get(0), 1, "a mark stays good after it is undone to");
    }

    #[test]
    fn a_fork_undoes_on_drop_and_keeps_on_keep() {
        let mut cells = Cells::new(vec![5]);
        let mut outer = cells.fork();
        outer.set(0, 6);
        let mut inner = outer.fork();
        inner.push(7);
        inner.keep();
        assert_eq!(outer.as_slice(), [6, 7]);
        drop(outer);
        assert_eq!(cells.as_slice(), [5], "a kept fork is still undone by the one around it");
    }

    #[test]
    fn a_log_of_nothing_writes_like_a_vec() {
        let mut cells = Trailed::<u32, ()>::new(vec![1, 2]);
        cells.set(0, 9);
        assert_eq!(cells.push(3), 2);
        assert_eq!((cells.as_slice(), cells.get(0)), ([9, 2, 3].as_slice(), 9));
        assert_eq!(size_of::<Trailed<u32, ()>>(), size_of::<Vec<u32>>());
    }

    #[test]
    #[should_panic(expected = "only the trail that made it")]
    fn a_mark_is_refused_by_any_other_trail() {
        let (one, mut other) = (Cells::new(vec![1]), Cells::new(vec![1]));
        other.undo_to(one.mark());
    }

    #[test]
    #[should_panic(expected = "undone past this mark")]
    fn a_mark_the_trail_was_undone_past_is_refused() {
        let mut cells = Cells::new(vec![1]);
        let early = cells.mark();
        cells.set(0, 2);
        let late = cells.mark();
        cells.undo_to(early);
        cells.undo_to(late);
    }

    type Run = (Cells, Trailed<u8>, Trailed<bool>);

    fn state(run: &Run) -> (Vec<i64>, Vec<u8>, Vec<bool>) {
        (run.0.as_slice().to_vec(), run.1.as_slice().to_vec(), run.2.as_slice().to_vec())
    }

    #[test]
    fn arrays_that_move_together_are_marked_and_undone_together() {
        let mut run: Run = (Cells::new(vec![1]), Trailed::new(vec![]), Trailed::new(vec![true]));
        let mark = run.mark();
        run.0.set(0, 2);
        run.1.push(9);
        run.2.set(0, false);
        {
            let mut fork = run.fork();
            fork.0.push(3);
            fork.1.set(0, 8);
        }
        assert_eq!(state(&run), (vec![2], vec![9], vec![false]));
        run.undo_to(mark);
        assert_eq!(state(&run), (vec![1], vec![], vec![true]));
    }

    /// One random step on the cells and on the model of them, which keeps a whole copy of the cells at each mark.
    fn step(cells: &mut Cells, model: &mut Vec<i64>, marks: &mut Vec<(Mark, Vec<i64>)>, rng: &mut Rng) {
        match rng.below(100) {
            0..35 if !model.is_empty() => {
                let (at, value) = (rng.below(model.len()), rng.next() as i64);
                cells.set(at as u32, value);
                model[at] = value;
            }
            35..55 => {
                let value = rng.next() as i64;
                assert_eq!(cells.push(value) as usize, model.len());
                model.push(value);
            }
            55..70 => marks.push((cells.mark(), model.clone())),
            70..100 if !marks.is_empty() => {
                let at = rng.below(marks.len());
                cells.undo_to(marks[at].0);
                *model = marks[at].1.clone();
                marks.truncate(at + 1);
            }
            _ => {}
        }
    }

    /// Steps, and between them a fork that takes steps of its own, down to `depth`, and then is kept or dropped.
    fn walk(cells: &mut Cells, model: &mut Vec<i64>, rng: &mut Rng, steps: usize, depth: usize) {
        let mut marks = Vec::new();
        for _ in 0..steps {
            if depth > 0 && rng.chance(15) {
                let before = model.clone();
                let mut fork = cells.fork();
                let steps = 1 + rng.below(12);
                walk(&mut fork, model, rng, steps, depth - 1);
                if rng.chance(50) {
                    fork.keep();
                } else {
                    drop(fork);
                    *model = before;
                }
            } else {
                step(cells, model, &mut marks, rng);
            }
            assert_eq!(cells.as_slice(), model.as_slice());
        }
    }

    #[test]
    fn random_writes_marks_undos_and_forks_agree_with_copying_everything() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        for _ in 0..300 {
            let start: Vec<i64> = (0..rng.below(6)).map(|_| rng.next() as i64).collect();
            let (mut cells, mut model) = (Cells::new(start.clone()), start);
            walk(&mut cells, &mut model, &mut rng, 60, 3);
        }
    }

    const WRITES: u32 = 1 << 24;

    /// `WRITES` writes to cells chosen by `cell`, which are the same for every kind of trail timed with it, and
    /// computed rather than read, so that the timing is of the write and not of fetching where to put it.
    fn drive(cell: impl Fn(u32) -> u32, mut write: impl FnMut(u32, u64)) {
        for step in 0..WRITES {
            write(cell(step), u64::from(step));
        }
    }

    /// Times every kind of trail on the same writes and prints what each costs a write.
    fn time_writes(name: &str, cells: usize, cell: impl Fn(u32) -> u32 + Copy) {
        let per_write = |time: Duration| time.as_nanos() as f64 / f64::from(WRITES);
        let mut vec = vec![0u64; cells];
        let plain = best_of(7, || drive(cell, |at, value| vec[at as usize] = value));
        let mut unlogged = Trailed::<u64, ()>::new(vec![0; cells]);
        let bare = best_of(7, || drive(cell, |at, value| unlogged.set(at, value)));
        let mut logged = Trailed::<u64>::new(vec![0; cells]);
        let (mut set, mut undo) = (Duration::MAX, Duration::MAX);
        for _ in 0..7 {
            let mark = logged.mark();
            let started = Instant::now();
            drive(cell, |at, value| logged.set(at, value));
            set = set.min(started.elapsed());
            let started = Instant::now();
            logged.undo_to(mark);
            undo = undo.min(started.elapsed());
            black_box(logged.get(0));
        }
        eprintln!(
            "{:>6} KB, {name:>7}: Vec {:.3} ns/write, Trailed<_, ()> {:.3}, Undo {:.3} to set and {:.3} to undo",
            cells * 8 / 1024,
            per_write(plain),
            per_write(bare),
            per_write(set),
            per_write(undo)
        );
    }

    /// `cargo test -p axiom-core --release trail::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_a_write_with_no_log_against_a_vec() {
        for log_cells in [12, 20] {
            let (cells, mask) = (1usize << log_cells, (1u32 << log_cells) - 1);
            time_writes("scatter", cells, move |step| step.wrapping_mul(0x9E37_79B1) >> 5 & mask);
            time_writes("sweep", cells, move |step| step & mask);
        }
    }
}
