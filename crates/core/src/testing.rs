//! What the tests share: a small deterministic generator, so that a failure is one a rerun repeats, and a stopwatch
//! for the benchmarks.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// xorshift64. Not for anything but tests.
pub(crate) struct Rng(u64);

impl Rng {
    /// A generator for `seed`, which must not be zero.
    pub(crate) fn new(seed: u64) -> Rng {
        assert_ne!(seed, 0, "xorshift never leaves zero");
        Rng(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..n`.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// Whether to do something that should happen `percent` times in a hundred.
    pub(crate) fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// The fastest of `runs` runs of `work`, after one run to warm the caches. The fastest, because on a busy machine
/// everything else is noise added to it.
pub(crate) fn best_of<R>(runs: usize, mut work: impl FnMut() -> R) -> Duration {
    black_box(work());
    let timed = |_| {
        let start = Instant::now();
        black_box(work());
        start.elapsed()
    };
    (0..runs).map(timed).min().expect("at least one run")
}
