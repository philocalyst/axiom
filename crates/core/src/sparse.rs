//! Range minimum and maximum in constant time: a sparse table.
//!
//! The peak balance of an account over a year (FBAR) is the maximum of a stretch of its history, and a report asks for
//! it for every account and every year. A history is only built once and never changes, so it pays to answer every
//! such question in O(1): row `k` of the table holds the extreme of every block of `2^k` values, and a range is covered
//! by two blocks of the largest power of two that fits in it, overlapping in the middle. Overlap is harmless because
//! the extreme of a union does not care how often it sees a value.
//!
//! The rows are laid end to end in one `Vec`, row `k` holding a cell for each of the `n + 1 - 2^k` blocks it covers. So
//! the table is about `n log n` values in one allocation, and a query reads two cells of one row. Whether the extreme
//! is the largest or the smallest is a type parameter, [`Max`] or [`Min`], so the choice costs nothing at a query.
//!
//! [`peak_within`] and [`low_within`] are the questions a history is asked: the extreme of what its steps held over a
//! window of days.

use std::marker::PhantomData;
use std::ops::Range;

use crate::calendar::Days;
use crate::day::Day;

/// Which of two values a table keeps. Must be idempotent and associative: `pick(a, a) == a`.
pub trait Order {
    fn pick<T: Ord>(a: T, b: T) -> T;
}

/// The largest.
pub enum Max {}

/// The smallest.
pub enum Min {}

impl Order for Max {
    fn pick<T: Ord>(a: T, b: T) -> T {
        a.max(b)
    }
}

impl Order for Min {
    fn pick<T: Ord>(a: T, b: T) -> T {
        a.min(b)
    }
}

/// The extreme, by `O`, of any range of the values it was built from.
pub struct Sparse<T, O> {
    /// Row `k` is the extreme of each run of `2^k` values: `len + 1 - 2^k` of them. Row 0 is the values.
    cells: Vec<T>,
    len: usize,
    order: PhantomData<fn() -> O>,
}

impl<T: Copy + Ord, O: Order> Sparse<T, O> {
    pub fn new(values: &[T]) -> Sparse<T, O> {
        let len = values.len();
        let rows = len.checked_ilog2().map_or(0, |log| log as usize + 1);
        let mut cells = Vec::with_capacity(row_start(len, rows));
        cells.extend_from_slice(values);
        for row in 1..rows {
            // Each cell is the extreme of two cells of the row below, side by side.
            let (below, half) = (row_start(len, row - 1), 1 << (row - 1));
            for at in below..below + len + 1 - 2 * half {
                cells.push(O::pick(cells[at], cells[at + half]));
            }
        }
        Sparse { cells, len, order: PhantomData }
    }

    /// The extreme of `values[range]`, or `None` for an empty range. Panics if the range reaches past the values.
    pub fn query(&self, range: Range<usize>) -> Option<T> {
        assert!(range.end <= self.len, "{range:?} reaches past {} values", self.len);
        if range.is_empty() {
            return None;
        }
        let row = range.len().ilog2();
        let blocks = self.row(row);
        Some(O::pick(blocks[range.start], blocks[range.end - (1 << row)]))
    }

    fn row(&self, row: u32) -> &[T] {
        let start = row_start(self.len, row as usize);
        &self.cells[start..start + self.len + 1 - (1 << row)]
    }
}

/// Where row `row` begins: the rows before it hold `len + 1 - 2^j` cells each.
fn row_start(len: usize, row: usize) -> usize {
    row * (len + 1) - ((1 << row) - 1)
}

/// The steps of a history that are in effect on some day of `window`, as a range of step indices: the one in effect
/// on its first day, if there is one, and every step that begins after that and by its last.
///
/// A history is a step for each change, `days[i]` the day from which the `i`th holds until the next begins. The days
/// must increase. A step that begins on the window's first day is the one in effect then, and one that begins on its
/// last day counts, for it holds on that day.
pub fn steps_in_effect(days: &[Day], window: Days) -> Range<usize> {
    let begun_by_first = days.partition_point(|&from| from <= window.first());
    let begun_by_last = days.partition_point(|&from| from <= window.last());
    // Before the first step nothing is held, so a window that opens before it starts at it.
    begun_by_first.saturating_sub(1)..begun_by_last
}

/// The most a history held on any day of `window`, or `None` if it held nothing yet: `values[i]` is what step `i`
/// held, from `days[i]`.
pub fn peak_within<T: Copy + Ord>(days: &[Day], values: &Sparse<T, Max>, window: Days) -> Option<T> {
    values.query(steps_in_effect(days, window))
}

/// The least a history held on any day of `window`, or `None` if it held nothing yet.
pub fn low_within<T: Copy + Ord>(days: &[Day], values: &Sparse<T, Min>, window: Days) -> Option<T> {
    values.query(steps_in_effect(days, window))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Rng, best_of};

    fn days(first: i32, last: i32) -> Days {
        Days::new(Day(first), Day(last)).unwrap()
    }

    fn random_values(rng: &mut Rng, len: usize, spread: i64) -> Vec<i64> {
        (0..len).map(|_| rng.below(spread as usize) as i64 - spread / 2).collect()
    }

    /// The extreme of a range by looking at all of it.
    fn scan<O: Order>(values: &[i64], range: Range<usize>) -> Option<i64> {
        values[range].iter().copied().reduce(O::pick)
    }

    fn check_ranges<O: Order>(values: &[i64], ranges: impl Iterator<Item = Range<usize>>) {
        let table = Sparse::<i64, O>::new(values);
        for range in ranges {
            assert_eq!(table.query(range.clone()), scan::<O>(values, range.clone()), "{range:?} of {values:?}");
        }
    }

    #[test]
    fn every_range_of_small_arrays() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        for len in 0..40 {
            let values = random_values(&mut rng, len, 9);
            let ranges = || (0..=len).flat_map(move |start| (start..=len).map(move |end| start..end));
            check_ranges::<Max>(&values, ranges());
            check_ranges::<Min>(&values, ranges());
        }
    }

    #[test]
    fn random_ranges_of_large_arrays() {
        let mut rng = Rng::new(0x2545_F491_4F6C_DD1D);
        for _ in 0..20 {
            let len = 1 + rng.below(3000);
            let spread = if rng.below(2) == 0 { 5 } else { 1 << 40 };
            let values = random_values(&mut rng, len, spread);
            let ranges: Vec<Range<usize>> = (0..300)
                .map(|_| {
                    let start = rng.below(len);
                    start..start + rng.below(len - start + 1)
                })
                .collect();
            check_ranges::<Max>(&values, ranges.iter().cloned());
            check_ranges::<Min>(&values, ranges.iter().cloned());
        }
    }

    #[test]
    fn a_table_is_n_log_n_cells_in_one_vector() {
        let table = Sparse::<u8, Max>::new(&[0; 1000]);
        assert_eq!(table.cells.len(), (0..10).map(|row| 1001 - (1 << row)).sum::<usize>());
        assert!(Sparse::<u8, Min>::new(&[]).cells.is_empty());
        assert_eq!(Sparse::<u8, Min>::new(&[7]).query(0..1), Some(7));
    }

    #[test]
    fn an_empty_range_has_no_extreme() {
        let table = Sparse::<i64, Max>::new(&[3, 1, 2]);
        let (end, start) = (1, 2);
        assert_eq!((table.query(1..1), table.query(3..3), table.query(start..end)), (None, None, None));
    }

    #[test]
    #[should_panic(expected = "reaches past 3 values")]
    fn a_range_past_the_end_is_refused() {
        Sparse::<i64, Max>::new(&[3, 1, 2]).query(1..4);
    }

    #[test]
    fn the_steps_in_effect_over_a_window() {
        let history = [Day(10), Day(20), Day(30)];
        let steps = |first, last| steps_in_effect(&history, days(first, last));
        assert_eq!(steps(10, 10), 0..1, "a step is in effect on its own day");
        assert_eq!(steps(15, 15), 0..1, "and until the next one begins");
        assert_eq!(steps(15, 19), 0..1);
        assert_eq!(steps(15, 20), 0..2, "the next counts from its first day, which is the window's last");
        assert_eq!(steps(20, 29), 1..2, "a window that opens on a step leaves the one before it out");
        assert_eq!(steps(19, 29), 0..2);
        assert_eq!(steps(5, 9), 0..0, "nothing is held before the first step");
        assert_eq!(steps(5, 10), 0..1);
        assert_eq!(steps(31, 100), 2..3, "after the last step it still holds");
        assert_eq!(steps_in_effect(&history, Days::ALWAYS), 0..3);
        assert_eq!(steps_in_effect(&[], Days::ALWAYS), 0..0);
    }

    #[test]
    fn peak_and_low_over_a_window() {
        let (days_of, balances) = ([Day(10), Day(20), Day(30)], [5, 9, 2]);
        let (peaks, lows) = (Sparse::<i64, Max>::new(&balances), Sparse::<i64, Min>::new(&balances));
        let window = days(15, 25);
        assert_eq!((peak_within(&days_of, &peaks, window), low_within(&days_of, &lows, window)), (Some(9), Some(5)));
        let after = days(40, 50);
        assert_eq!((peak_within(&days_of, &peaks, after), low_within(&days_of, &lows, after)), (Some(2), Some(2)));
        assert_eq!(peak_within(&days_of, &peaks, days(0, 9)), None);
        assert_eq!(peak_within(&days_of, &peaks, Days::ALWAYS), Some(9));
    }

    #[test]
    fn peak_and_low_agree_with_a_look_at_every_day() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        for case in 0..300 {
            let steps = rng.below(12);
            let mut from = rng.below(60) as i32 - 20;
            let history: Vec<(Day, i64)> = (0..steps)
                .map(|_| {
                    from += 1 + rng.below(25) as i32;
                    (Day(from), rng.below(50) as i64)
                })
                .collect();
            let (starts, balances): (Vec<Day>, Vec<i64>) = history.iter().copied().unzip();
            let (peaks, lows) = (Sparse::<i64, Max>::new(&balances), Sparse::<i64, Min>::new(&balances));

            let first = rng.below(300) as i32 - 40;
            let window = days(first, first + rng.below(120) as i32);
            // The value on each day of the window, where there is one: the last step at or before the day.
            let held: Vec<i64> = (window.first().0..=window.last().0)
                .filter_map(|day| history.iter().rev().find(|(from, _)| from.0 <= day).map(|&(_, balance)| balance))
                .collect();
            assert_eq!(peak_within(&starts, &peaks, window), held.iter().copied().max(), "case {case}");
            assert_eq!(low_within(&starts, &lows, window), held.iter().copied().min(), "case {case}");
        }
    }

    /// `cargo test -p axiom-core --release sparse::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_a_query_against_a_scan() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        let values = random_values(&mut rng, 1_000_000, 1 << 40);
        let build = best_of(5, || Sparse::<i64, Max>::new(&values));
        let table = Sparse::<i64, Max>::new(&values);
        eprintln!("build over 1,000,000 values: {build:?} ({} cells)", table.cells.len());
        for width in [16, 1_024, 65_536] {
            let ranges: Vec<Range<usize>> = (0..20_000)
                .map(|_| {
                    let start = rng.below(values.len() - width);
                    start..start + width
                })
                .collect();
            let per_query = |total: std::time::Duration, queries: usize| total.as_nanos() as f64 / queries as f64;
            let sparse = best_of(5, || ranges.iter().map(|r| table.query(r.clone()).unwrap()).fold(0, |a, b| a ^ b));
            let few = &ranges[..1_000];
            let scanned =
                best_of(3, || few.iter().map(|r| scan::<Max>(&values, r.clone()).unwrap()).fold(0, |a, b| a ^ b));
            let (sparse, scanned) = (per_query(sparse, ranges.len()), per_query(scanned, few.len()));
            eprintln!(
                "width {width:>6}: sparse {sparse:>8.1} ns/query, scan {scanned:>10.1} ns/query ({:.0}x)",
                scanned / sparse
            );
        }
    }
}
