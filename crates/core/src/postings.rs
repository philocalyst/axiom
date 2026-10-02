//! Intersecting sorted lists of ids: how the words of an address find the things they name.
//!
//! The inverted index of a book keeps, for each word, the ids of the things whose address contains it, sorted: a
//! posting list. A path of words denotes the things in every one of its lists, so resolving it is intersecting them.
//! Sorted lists intersect by merging, in time linear in both, or, when one is far shorter, by galloping through the
//! longer one: for each id of the short list, double a step until it overshoots, then bisect the last stretch. That is
//! `k log(n/k)` for lists of `k` and `n`, which beats a merge once `n` is several times `k`.
//!
//! Lists of similar length merge a block of eight ids at a time. Each id of one list's block is compared with every id
//! of the other's, which is eight vector comparisons for 64 pairs and leaves a mask of the ids that were in both. The
//! block that ends on the smaller id is spent, since everything after the other block's last id is larger than all of
//! it. The ends of the lists, too short to fill a block, merge an id at a time.
//!
//! Lists are `&[u32]`, not a set type: the index stores them contiguously (a `Groups`), and a list that was just
//! intersected is another slice to intersect. Ids in a list strictly increase.

use fearless_simd::{Level, Simd, SimdBase, dispatch, prelude::*, u32x8};

/// How many times longer one list must be than the other for galloping to beat the merge: where the two cross in the
/// benchmark in the tests, which has the galloping 1.2 times faster at 8, 3 times at 32 and 65 times at 1,024.
const SKEW: usize = 8;

/// The ids in both `a` and `b`, in order, in `out`, whose old contents are dropped.
pub fn intersect(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if short.len() * SKEW <= long.len() {
        gallop(short, long, out);
    } else {
        merge(short, long, out);
    }
}

/// The ids in every list, in `out`, whose old contents are dropped; no lists have none. Starts from the shortest,
/// which no result can outgrow, and stops as soon as nothing is left. Reorders `lists`.
pub fn intersect_all(lists: &mut [&[u32]], out: &mut Vec<u32>) {
    lists.sort_unstable_by_key(|list| list.len());
    out.clear();
    let Some((shortest, rest)) = lists.split_first() else { return };
    out.extend_from_slice(shortest);
    let mut scratch = Vec::new();
    for list in rest {
        intersect(out, list, &mut scratch);
        std::mem::swap(out, &mut scratch);
        if out.is_empty() {
            return;
        }
    }
}

/// The ids in both lists, which are of similar length: a block at a time if each is at least [`BLOCKS_FROM`] long, else
/// an id at a time. The machine is asked what it can do here, once for the call and not for each block.
fn merge(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    if a.len().min(b.len()) < BLOCKS_FROM {
        merge_scalar(a, b, out);
    } else {
        merge_blocks(Level::new(), a, b, out);
    }
}

/// How long both lists must be for blocks to pay. The blocks leave the scalar loop the ends of the lists, up to a block
/// and a half of ids, and a short list is nearly all end: at 9 to 15 ids each the block merge is no faster than the
/// loop (0.95 to 1.05 times, in the benchmark in the tests), from 16 it is 1.4 to 1.9 times faster, and at 256, 2.4.
const BLOCKS_FROM: usize = 2 * BLOCK;

/// Ids compared at once: the lanes of a `u32x8`, a 256-bit register, which `fearless_simd` makes of two on a machine
/// with only 128-bit ones. A sweep of 4, 8 and 16 lanes at every level of two machines found 8 the fastest or tied at
/// all of them: 4 lanes was up to 1.35 times slower, and a tie on 128-bit registers; 16 lanes from 1.05 to 1.5 times.
const BLOCK: usize = 8;

/// One bit for each lane of a block.
type Mask = u8;
const _: () = assert!(Mask::BITS as usize == BLOCK);

/// For each mask, the lanes it has set, in order and packed to the front, then zeros that [`pack`] writes and the
/// caller does not keep. A table, because both alternatives lose: a loop over the set bits runs as many times as the
/// data says, a branch mispredicted half the time on lists that share half their ids, and storing every lane and
/// stepping on by its bit is 1.4 times slower with 256-bit registers and 1.1 times with 128-bit ones.
static LANES_SET: [[u8; BLOCK]; 1 << BLOCK] = lanes_set();

const fn lanes_set() -> [[u8; BLOCK]; 1 << BLOCK] {
    let mut table = [[0; BLOCK]; 1 << BLOCK];
    let mut mask = 0;
    while mask < table.len() {
        let (mut lane, mut packed) = (0, 0);
        while lane < BLOCK {
            if mask >> lane & 1 == 1 {
                table[mask][packed] = lane as u8;
                packed += 1;
            }
            lane += 1;
        }
        mask += 1;
    }
    table
}

/// The merge a block at a time, finished an id at a time. `level` is what the machine can do, found by the caller.
fn merge_blocks(level: Level, a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    // Room for the shorter list, and for the whole block that is written after the last id kept.
    out.resize(a.len().min(b.len()) + BLOCK, 0);
    let Unmerged { a, b, found } = dispatch!(level, simd => compare_blocks(simd, a, b, out));
    let found = found + merge_into(a, b, &mut out[found..]);
    out.truncate(found);
}

/// What the block kernel leaves to the scalar loop: the ids of each list it did not reach, and how many ids it kept.
struct Unmerged<'a> {
    a: &'a [u32],
    b: &'a [u32],
    found: usize,
}

/// Compares block after block while each list has one to give, writing the ids in both to `out` from `out[0]`, which
/// must have room for as many as the shorter list and `BLOCK` more. Inlined into the caller, which `dispatch!` has
/// given the target features of its level: `simd` is the proof.
#[inline(always)]
fn compare_blocks<'a, S: Simd>(simd: S, mut a: &'a [u32], mut b: &'a [u32], out: &mut [u32]) -> Unmerged<'a> {
    let mut found = 0;
    while let (Some(block_a), Some(block_b)) = (a.first_chunk::<BLOCK>(), b.first_chunk::<BLOCK>()) {
        found += pack(block_a, shared_lanes(simd, block_a, block_b), slots(out, found));
        let (last_a, last_b) = (block_a[BLOCK - 1], block_b[BLOCK - 1]);
        a = &a[BLOCK * usize::from(last_a <= last_b)..];
        b = &b[BLOCK * usize::from(last_b <= last_a)..];
    }
    Unmerged { a, b, found }
}

/// The lanes of `a` that hold an id of `b`: each id of `b` is broadcast and compared with all of `a` at once, and the
/// eight answers are or-ed together. No shuffle, so nothing here is specific to a level.
#[inline(always)]
fn shared_lanes<S: Simd>(simd: S, a: &[u32; BLOCK], b: &[u32; BLOCK]) -> Mask {
    let lanes = u32x8::from_slice(simd, a);
    let mut same = lanes.simd_eq(u32x8::splat(simd, b[0]));
    for &id in &b[1..] {
        same |= lanes.simd_eq(u32x8::splat(simd, id));
    }
    same.to_bitmask() as Mask
}

/// Writes the ids of `block` in the lanes `shared` has set to the front of `window`, and says how many there were.
/// Every slot is written, set or not, so that no branch depends on the data. The `% BLOCK` changes nothing the table
/// does not already promise: it is there for the compiler, which then drops the bounds check.
#[inline(always)]
fn pack(block: &[u32; BLOCK], shared: Mask, window: &mut [u32; BLOCK]) -> usize {
    for (slot, &lane) in window.iter_mut().zip(&LANES_SET[usize::from(shared)]) {
        *slot = block[usize::from(lane) % BLOCK];
    }
    shared.count_ones() as usize
}

/// The `BLOCK` slots of `out` from `at`, as an array, so that writing them needs one check and not eight.
#[inline(always)]
fn slots(out: &mut [u32], at: usize) -> &mut [u32; BLOCK] {
    (&mut out[at..at + BLOCK]).try_into().expect("a slice of BLOCK slots is an array of BLOCK")
}

/// The merge, an id at a time, over the whole of both lists.
fn merge_scalar(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.resize(a.len().min(b.len()), 0);
    let found = merge_into(a, b, out);
    out.truncate(found);
}

/// Two cursors that advance by comparisons turned into arithmetic: whichever list has the smaller id moves on, both
/// if they are equal, and the id is written down either way and kept only when they were. No branch depends on the
/// data, so none is mispredicted: 2 to 3 times faster than the match on the comparison, in the benchmark. The ids
/// kept start at `out[0]`, which must have room for as many as the shorter list; returns how many.
fn merge_into(a: &[u32], b: &[u32], out: &mut [u32]) -> usize {
    let (mut at_a, mut at_b, mut found) = (0, 0, 0);
    while at_a < a.len() && at_b < b.len() {
        let (x, y) = (a[at_a], b[at_b]);
        out[found] = x;
        found += usize::from(x == y);
        at_a += usize::from(x <= y);
        at_b += usize::from(x >= y);
    }
    found
}

/// For each id of `short`, finds it in what is left of `long`.
fn gallop(short: &[u32], long: &[u32], out: &mut Vec<u32>) {
    let mut rest = long;
    for &id in short {
        rest = &rest[first_at_least(rest, id)..];
        match rest.first() {
            None => return,
            Some(&found) if found == id => out.push(id),
            Some(_) => {}
        }
    }
}

/// The index of the first id in `list` that is `target` or more, or its length if there is none: found by doubling a
/// step until the id there passes the target, then bisecting the stretch the last doubling crossed. If all of that
/// stretch is below the target, the answer is the id after it.
fn first_at_least(list: &[u32], target: u32) -> usize {
    let mut reach = 1;
    while reach < list.len() && list[reach] < target {
        reach *= 2;
    }
    let crossed = &list[reach / 2..list.len().min(reach)];
    reach / 2 + crossed.partition_point(|&id| id < target)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::hint::black_box;

    use super::*;
    use crate::testing::{Rng, best_of};

    /// A sorted list of about `len` ids drawn from `0..domain`.
    fn random_list(rng: &mut Rng, len: usize, domain: usize) -> Vec<u32> {
        let ids: BTreeSet<u32> = (0..len).map(|_| rng.below(domain) as u32).collect();
        ids.into_iter().collect()
    }

    fn by_sets(a: &[u32], b: &[u32]) -> Vec<u32> {
        let (a, b): (BTreeSet<u32>, BTreeSet<u32>) = (a.iter().copied().collect(), b.iter().copied().collect());
        a.intersection(&b).copied().collect()
    }

    fn intersection(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut out = vec![7, 7, 7];
        intersect(a, b, &mut out);
        out
    }

    #[test]
    fn the_shapes_of_two_lists() {
        assert_eq!(intersection(&[], &[]), []);
        assert_eq!(intersection(&[1, 2, 3], &[]), []);
        assert_eq!(intersection(&[], &[1, 2, 3]), []);
        assert_eq!(intersection(&[1, 3, 5], &[2, 4, 6]), [], "disjoint and interleaved");
        assert_eq!(intersection(&[1, 2, 3], &[10, 11]), [], "disjoint and apart");
        assert_eq!(intersection(&[4, 5, 6], &[4, 5, 6]), [4, 5, 6], "identical");
        assert_eq!(intersection(&[5], &(0..1000).collect::<Vec<_>>()), [5], "one in a long list: a gallop");
        assert_eq!(intersection(&[0, 999], &(0..1000).collect::<Vec<_>>()), [0, 999], "at both ends");
        assert_eq!(intersection(&[u32::MAX], &[0, u32::MAX]), [u32::MAX]);
    }

    #[test]
    fn the_shapes_of_two_lists_that_fill_blocks() {
        let ids = |range: std::ops::Range<u32>| range.collect::<Vec<_>>();
        // Every way, not `intersect`: that sends lists under two blocks long to the scalar loop.
        let all_ways_give = |a: &[u32], b: &[u32], expected: &[u32], shape: &str| {
            for (way, got) in every_way(a, b) {
                assert_eq!(got, expected, "{way}: {shape}");
            }
        };
        all_ways_give(&ids(0..8), &ids(0..8), &ids(0..8), "one block, identical");
        all_ways_give(&ids(0..16), &ids(8..24), &ids(8..16), "blocks that overlap by one");
        all_ways_give(&ids(0..24), &ids(24..48), &[], "blocks that touch and share nothing");
        all_ways_give(&ids(0..8), &ids(7..15), &[7], "the last lane of one block and the first of the next");
        all_ways_give(&ids(0..40), &ids(20..60), &ids(20..40), "more blocks than one, half shared");
        let evens: Vec<u32> = (0..40).map(|n| 2 * n).collect();
        all_ways_give(&evens, &ids(0..40), &ids(0..20).iter().map(|n| 2 * n).collect::<Vec<_>>(), "every other id");
        let top: Vec<u32> = (u32::MAX - 15..=u32::MAX).collect();
        all_ways_give(&top, &top, &top, "the last ids there are");
    }

    #[test]
    fn the_first_id_at_least_a_target() {
        let list = [2, 4, 6, 8, 10, 12, 14, 16, 18];
        for target in 0..21 {
            assert_eq!(first_at_least(&list, target), list.partition_point(|&id| id < target), "target {target}");
        }
        assert_eq!(first_at_least(&[], 3), 0);
    }

    /// Every level of SIMD this machine can run, from the best to the baseline, so that each code path of the kernel is
    /// tested and not only the one that `Level::new` picks.
    fn levels() -> Vec<Level> {
        let best = Level::new();
        let mut levels = vec![best, Level::baseline()];
        #[cfg(target_arch = "x86_64")]
        {
            levels.extend(best.as_avx2().map(Level::Avx2));
            levels.extend(best.as_sse4_2().map(Level::Sse4_2));
        }
        levels
    }

    /// `Avx2`, not `Avx2(Avx2 { _private: () })`.
    fn name(level: Level) -> String {
        format!("{level:?}").split('(').next().unwrap_or_default().to_string()
    }

    /// What the scalar merge, the block merge at each level, and the whole of `intersect` make of two lists, each
    /// starting from an `out` that holds something else.
    fn every_way(a: &[u32], b: &[u32]) -> Vec<(String, Vec<u32>)> {
        let mut ways = Vec::new();
        let mut run = |name: String, way: &dyn Fn(&mut Vec<u32>)| {
            let mut out = vec![7, 7, 7];
            way(&mut out);
            ways.push((name, out));
        };
        run("scalar merge".into(), &|out| merge_scalar(a, b, out));
        for level in levels() {
            run(format!("block merge at {}", name(level)), &|out| merge_blocks(level, a, b, out));
        }
        let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
        run("gallop".into(), &|out| {
            out.clear();
            gallop(short, long, out);
        });
        run("intersect".into(), &|out| intersect(a, b, out));
        ways
    }

    #[test]
    fn every_way_of_intersecting_agrees_with_sets_whatever_the_lengths() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        for case in 0..1500 {
            let (len_a, len_b) = (rng.below(60), if rng.below(3) == 0 { rng.below(5000) } else { rng.below(60) });
            let domain = [30, 200, 100_000][rng.below(3)];
            let (a, b) = (random_list(&mut rng, len_a, domain), random_list(&mut rng, len_b, domain));
            let expected = by_sets(&a, &b);
            for (way, got) in every_way(&a, &b) {
                assert_eq!(got, expected, "{way}, case {case}");
            }
        }
    }

    #[test]
    fn the_kernel_leaves_only_the_ends_that_do_not_fill_a_block() {
        let ids: Vec<u32> = (0..20).collect();
        for level in levels() {
            let mut out = vec![0; ids.len() + BLOCK];
            let Unmerged { a, b, found } = dispatch!(level, simd => compare_blocks(simd, &ids, &ids, &mut out));
            assert_eq!((a, b, found), (&ids[16..], &ids[16..], 16), "at {}", name(level));
            assert_eq!(out[..found], ids[..found]);
        }
    }

    /// How the second list of a pair is made from the first, or alongside it.
    #[derive(Clone, Copy)]
    enum Relation {
        /// Both drawn from one range of ids: some shared, most not.
        Drawn,
        /// Nothing shared, the ids alternating between the lists.
        Interleaved,
        /// Nothing shared, one list wholly below the other.
        Apart,
        /// Everything shared: the same list twice.
        Identical,
        /// The second is every other id of the first.
        EveryOther,
        /// Both start at 0, and the shorter is the front of the longer.
        Prefix,
    }

    const RELATIONS: [Relation; 6] = [
        Relation::Drawn,
        Relation::Interleaved,
        Relation::Apart,
        Relation::Identical,
        Relation::EveryOther,
        Relation::Prefix,
    ];

    /// A length from 0 to 200, of a kind that `kind` picks: any, or one that ends exactly where a block does, or one id
    /// after a block's end, or one before.
    fn length(rng: &mut Rng, kind: usize) -> usize {
        match kind % 4 {
            0 => rng.below(201),
            1 => BLOCK * rng.below(26),
            2 => BLOCK * rng.below(25) + 1,
            _ => BLOCK * (rng.below(25) + 1) - 1,
        }
    }

    /// Exactly `len` distinct ids from `0..domain`, which must be larger than `len`, in order.
    fn exactly(rng: &mut Rng, len: usize, domain: usize) -> Vec<u32> {
        let mut ids = BTreeSet::new();
        while ids.len() < len {
            ids.insert(rng.below(domain) as u32);
        }
        ids.into_iter().collect()
    }

    /// A pair of lists related as `relation` says, with lengths of the kind `kind` picks.
    fn pair_of(rng: &mut Rng, relation: Relation, kind: usize) -> (Vec<u32>, Vec<u32>) {
        let (len_a, len_b) = (length(rng, kind), length(rng, kind));
        let domain = (len_a + len_b) * [1, 2, 8][rng.below(3)] + 1;
        let (below, above) = (0..len_a as u32, len_a as u32 + 3..(len_a + len_b + 3) as u32);
        match relation {
            Relation::Drawn => (exactly(rng, len_a, domain), exactly(rng, len_b, domain)),
            Relation::Interleaved => {
                ((0..len_a as u32).map(|n| 2 * n).collect(), (0..len_b as u32).map(|n| 2 * n + 1).collect())
            }
            Relation::Apart if rng.chance(50) => (below.collect(), above.collect()),
            Relation::Apart => (above.collect(), below.collect()),
            Relation::Identical => {
                let ids = exactly(rng, len_a, domain);
                (ids.clone(), ids)
            }
            Relation::EveryOther => {
                let ids = exactly(rng, len_a, domain);
                let every_other = ids.iter().copied().skip(rng.below(2)).step_by(2).collect();
                (ids, every_other)
            }
            Relation::Prefix => ((0..len_a as u32).collect(), (0..len_b as u32).collect()),
        }
    }

    /// Where in the range of ids a pair sits. Ids are moved and never reordered, so what the lists share is kept.
    #[derive(Clone, Copy)]
    enum Placement {
        /// From 0 up.
        Bottom,
        /// Up to `u32::MAX`.
        Top,
        /// Across 2^31, where a comparison that took ids for signed would turn over.
        Middle,
        /// The lower half of the ids at the bottom and the upper half at the top, so that one block can hold ids near 0
        /// and ids near `u32::MAX` together.
        BothEnds,
    }

    const PLACEMENTS: [Placement; 4] = [Placement::Bottom, Placement::Top, Placement::Middle, Placement::BothEnds];

    fn place(placement: Placement, a: &mut [u32], b: &mut [u32]) {
        let largest = a.iter().chain(&*b).copied().max().unwrap_or(0);
        let (from, lift) = match placement {
            Placement::Bottom => (0, 0),
            Placement::Top => (0, u32::MAX - largest),
            Placement::Middle => (0, (1 << 31) - largest / 2),
            Placement::BothEnds => (largest / 2 + 1, u32::MAX - largest),
        };
        for id in a.iter_mut().chain(b).filter(|id| **id >= from) {
            *id += lift;
        }
    }

    fn by_search(a: &[u32], b: &[u32]) -> Vec<u32> {
        a.iter().copied().filter(|id| b.binary_search(id).is_ok()).collect()
    }

    /// 6 relations, in 4 places among the ids, in 4 kinds of length: each of the 96 shapes 2,500 times. A debug build runs
    /// a twentieth of them, which is still 125 of each shape, because it takes a minute to run them all.
    #[test]
    fn the_block_kernel_equals_the_scalar_loop_on_240_000_pairs_of_every_shape() {
        const CASES: usize = if cfg!(debug_assertions) { 12_000 } else { 240_000 };
        let mut rng = Rng::new(0xD1B5_4A32_D192_ED03);
        let levels = levels();
        for case in 0..CASES {
            let (mut a, mut b) = pair_of(&mut rng, RELATIONS[case % 6], case / 24);
            place(PLACEMENTS[case / 6 % 4], &mut a, &mut b);
            let mut scalar = vec![7, 7, 7];
            merge_scalar(&a, &b, &mut scalar);
            assert_eq!(scalar, by_search(&a, &b), "the scalar loop, case {case}");
            for &level in &levels {
                let mut blocks = vec![7, 7, 7];
                merge_blocks(level, &a, &b, &mut blocks);
                assert_eq!(
                    blocks,
                    scalar,
                    "the blocks at {}, case {case}: {} and {} ids",
                    name(level),
                    a.len(),
                    b.len()
                );
            }
        }
    }

    #[test]
    fn many_lists_intersect_shortest_first() {
        let mut rng = Rng::new(0x2545_F491_4F6C_DD1D);
        for case in 0..400 {
            let count = rng.below(6);
            let lists: Vec<Vec<u32>> = (0..count)
                .map(|_| {
                    let len = rng.below(300);
                    random_list(&mut rng, len, 400)
                })
                .collect();
            let sets = lists.iter().map(|list| list.iter().copied().collect::<BTreeSet<u32>>());
            let expected: Vec<u32> = sets.reduce(|all, next| &all & &next).unwrap_or_default().into_iter().collect();
            let mut views: Vec<&[u32]> = lists.iter().map(Vec::as_slice).collect();
            let mut out = vec![1, 2, 3];
            intersect_all(&mut views, &mut out);
            assert_eq!(out, expected, "case {case}");
        }
    }

    /// A list of `len` ids from `0..20 * len`, and one as long that shares `overlap` percent of it.
    fn pair_sharing(rng: &mut Rng, len: usize, overlap: usize) -> (Vec<u32>, Vec<u32>) {
        let a = random_list(rng, len, 20 * len);
        let mut b: BTreeSet<u32> = a.iter().copied().filter(|_| rng.chance(overlap)).collect();
        while b.len() < len {
            b.insert(rng.below(20 * len) as u32);
        }
        (a, b.into_iter().collect())
    }

    fn classic_merge(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
        let (mut at_a, mut at_b) = (0, 0);
        while at_a < a.len() && at_b < b.len() {
            match a[at_a].cmp(&b[at_b]) {
                std::cmp::Ordering::Less => at_a += 1,
                std::cmp::Ordering::Greater => at_b += 1,
                std::cmp::Ordering::Equal => {
                    out.push(a[at_a]);
                    (at_a, at_b) = (at_a + 1, at_b + 1);
                }
            }
        }
    }

    fn time(runs: usize, mut intersect: impl FnMut(&mut Vec<u32>)) -> f64 {
        let mut out = Vec::new();
        best_of(runs, || {
            out.clear();
            intersect(&mut out);
            black_box(out.len())
        })
        .as_nanos() as f64
    }

    /// `cargo test -p axiom-core --release postings::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_merge_against_the_classic_merge_on_lists_of_similar_length() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        for len in [10_000, 100_000, 1_000_000] {
            for overlap in [1, 50] {
                let (a, b) = pair_sharing(&mut rng, len, overlap);
                let classic = time(9, |out| classic_merge(&a, &b, out));
                let branchless = time(9, |out| merge_scalar(&a, &b, out));
                let n = (a.len() + b.len()) as f64;
                eprintln!(
                    "{len:>9} ids, {overlap:>2}% shared: classic {:.2} ns/id, branchless {:.2} ns/id ({:.2}x)",
                    classic / n,
                    branchless / n,
                    classic / branchless
                );
            }
        }
    }

    /// `cargo test -p axiom-core --release postings::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_the_block_merge_against_the_scalar_merge_at_every_level() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        for len in [10_000, 100_000, 1_000_000] {
            for overlap in [1, 50] {
                let (a, b) = pair_sharing(&mut rng, len, overlap);
                let (scalar, n) = (time(15, |out| merge_scalar(&a, &b, out)), (a.len() + b.len()) as f64);
                let mut row = format!("{len:>9} ids, {overlap:>2}% shared: scalar {:.2} ns/id", scalar / n);
                for level in levels() {
                    let blocks = time(15, |out| merge_blocks(level, &a, &b, out));
                    row += &format!(" | {} {:.2} ({:.2}x)", name(level), blocks / n, scalar / blocks);
                }
                eprintln!("{row}");
            }
        }
    }

    /// `cargo test -p axiom-core --release postings::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_merge_on_short_lists_where_it_decides_between_blocks_and_ids() {
        type Merge = fn(&[u32], &[u32], &mut Vec<u32>);
        // Called through pointers, so that neither is inlined into the loop: a call is most of what a short list costs.
        // `merge` is one call more here than inside `intersect`, which inlines it: about 2 ns, the shortfall under 16 ids.
        let (by_ids, by_choice): (Merge, Merge) = (black_box(merge_scalar), black_box(merge));
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        for len in [8, 9, 12, 15, 16, 18, 24, 32, 64, 256] {
            let pairs: Vec<_> = (0..1024).map(|which| pair_sharing(&mut rng, len, [1, 50][which % 2])).collect();
            let scalar = time(15, |out| pairs.iter().for_each(|(a, b)| by_ids(a, b, out)));
            let chosen = time(15, |out| pairs.iter().for_each(|(a, b)| by_choice(a, b, out)));
            let per_pair = pairs.len() as f64;
            eprintln!(
                "{len:>4} ids each: scalar {:>7.1} ns, merge {:>7.1} ns ({:.2}x)",
                scalar / per_pair,
                chosen / per_pair,
                scalar / chosen
            );
        }
    }

    /// `cargo test -p axiom-core --release postings::tests::bench -- --ignored --nocapture`
    #[test]
    #[ignore = "a benchmark"]
    fn bench_gallop_against_merge_by_how_much_longer_one_list_is() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        let long = random_list(&mut rng, 1_000_000, 4_000_000);
        for skew in [2, 4, 8, 16, 32, 64, 128, 1024] {
            let short: Vec<u32> = random_list(&mut rng, long.len() / skew, 4_000_000);
            let merged = time(9, |out| merge_scalar(&short, &long, out));
            let galloped = time(9, |out| gallop(&short, &long, out));
            eprintln!(
                "one list {skew:>4}x the other: merge {:>10.0} ns, gallop {:>10.0} ns ({:.2}x)",
                merged,
                galloped,
                merged / galloped
            );
        }
    }
}
