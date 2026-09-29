//! Monte Carlo bands: how far variable spending could pull the forecast down.
//!
//! Each path adds, month by month, one bootstrapped past month of spending per
//! category to the committed path. Paths are computed eight at a time in
//! fixed-size lanes, so the generator and the accumulation are straight-line
//! loops over `[_; 8]` that the compiler turns into vector instructions. It is
//! integer arithmetic throughout, and the generator is seeded, so the same
//! books always give the same bands.

use axiom_core::num::mul_div;

const LANES: usize = 8;

/// The fraction of a month a projected period covers: its first and last
/// months are usually partial.
#[derive(Clone, Copy, Debug)]
pub struct Share {
    pub days: i64,
    pub of: i64,
}

impl Share {
    fn apply(self, quanta: i64) -> i64 {
        let scaled = mul_div(quanta.into(), self.days.into(), self.of.into()).expect("a month has days");
        i64::try_from(scaled).expect("a share of an i64 fits")
    }
}

/// Sampled net worth, per month and path.
pub struct Bands {
    paths: usize,
    /// Month-major: the paths of one month are contiguous.
    values: Vec<i64>,
}

impl Bands {
    /// The 10th, 50th and 90th percentile of month `month`.
    pub fn percentiles(&self, month: usize) -> [i64; 3] {
        let mut sorted = self.values[month * self.paths..(month + 1) * self.paths].to_vec();
        sorted.sort_unstable();
        [10, 50, 90].map(|percentile| sorted[(sorted.len() - 1) * percentile / 100])
    }

    pub fn paths(&self) -> usize {
        self.paths
    }
}

/// Runs `paths` paths (rounded up to a whole number of lanes). `committed[m]`
/// is where the forecast stands at the end of month `m`, `shares[m]` how much
/// of a month it is, and `history` holds each category's spending in past
/// months.
pub fn simulate(committed: &[i64], shares: &[Share], history: &[Vec<i64>], paths: usize, seed: u64) -> Bands {
    let groups = paths.div_ceil(LANES);
    let mut bands = Bands { paths: groups * LANES, values: vec![0; committed.len() * groups * LANES] };
    for group in 0..groups {
        let mut streams = Streams::seeded(seed, group as u64);
        let mut spent = [0i64; LANES];
        for (month, (&standing, &share)) in committed.iter().zip(shares).enumerate() {
            let drawn = draw_month(&mut streams, history);
            for lane in 0..LANES {
                spent[lane] += share.apply(drawn[lane]);
                bands.values[month * bands.paths + group * LANES + lane] = standing - spent[lane];
            }
        }
    }
    bands
}

/// One month of spending per lane: a random past month for every category.
fn draw_month(streams: &mut Streams, history: &[Vec<i64>]) -> [i64; LANES] {
    let mut drawn = [0i64; LANES];
    for category in history.iter().filter(|category| !category.is_empty()) {
        let picks = streams.below(category.len());
        for lane in 0..LANES {
            drawn[lane] += category[picks[lane]];
        }
    }
    drawn
}

/// Eight xorshift64* generators, advanced together.
struct Streams {
    state: [u64; LANES],
}

const MULTIPLIER: u64 = 0x2545_F491_4F6C_DD1D;

impl Streams {
    /// One stream per lane, each started from a different SplitMix64 output of
    /// the seed, so no two lanes or groups share a sequence.
    fn seeded(seed: u64, group: u64) -> Streams {
        let mut state = [0u64; LANES];
        for (lane, slot) in state.iter_mut().enumerate() {
            *slot = splitmix(seed ^ splitmix(group * LANES as u64 + lane as u64)).max(1);
        }
        Streams { state }
    }

    fn next(&mut self) -> [u64; LANES] {
        let mut out = [0u64; LANES];
        for lane in 0..LANES {
            let mut x = self.state[lane];
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.state[lane] = x;
            out[lane] = x.wrapping_mul(MULTIPLIER);
        }
        out
    }

    /// Uniform in `0..n` for `n` below 2^32: multiply-shift, which needs no
    /// division and whose bias is below 2^-32.
    fn below(&mut self, n: usize) -> [usize; LANES] {
        let bound = n as u64;
        self.next().map(|word| (((word >> 32) * bound) >> 32) as usize)
    }
}

fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: Share = Share { days: 1, of: 1 };

    #[test]
    fn without_variation_every_path_agrees() {
        // One category that always cost 100: after k months every path is 100·k short.
        let bands = simulate(&[1_000, 1_000, 1_000], &[WHOLE; 3], &[vec![100]], 16, 7);
        assert_eq!(bands.paths(), 16);
        assert_eq!(bands.percentiles(0), [900; 3]);
        assert_eq!(bands.percentiles(2), [700; 3]);
    }

    #[test]
    fn the_same_seed_gives_the_same_bands_and_they_are_ordered() {
        let history = vec![vec![0, 100, 200, 300, 400], vec![50, 50, 900]];
        let run = |seed| simulate(&[10_000; 6], &[WHOLE; 6], &history, 1_000, seed);
        let (a, b) = (run(42), run(42));
        for month in 0..6 {
            let [low, middle, high] = a.percentiles(month);
            assert_eq!(a.percentiles(month), b.percentiles(month));
            assert!(low <= middle && middle <= high && low < high);
        }
        // Spending only accumulates: later months sit lower.
        assert!(a.percentiles(5)[1] < a.percentiles(0)[1]);
        assert_eq!(a.paths(), 1_000);
    }

    #[test]
    fn a_partial_month_draws_a_partial_share() {
        let half = Share { days: 15, of: 30 };
        let bands = simulate(&[0], &[half], &[vec![1_001]], 8, 1);
        // Half of 1,001 is 500.5, which rounds half to even: 500.
        assert_eq!(bands.percentiles(0), [-500; 3]);
    }
}
