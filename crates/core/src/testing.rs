//! What the property tests share: a small deterministic generator, so that a failure is one a rerun repeats.

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
}
