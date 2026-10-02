//! Intersecting sorted lists of ids: how the words of an address find the things they name.
//!
//! The inverted index of a book keeps, for each word, the ids of the things whose address contains it, sorted: a
//! posting list. A path of words denotes the things in every one of its lists, so resolving it is intersecting them.
//! Sorted lists intersect by merging, in time linear in both, or, when one is far shorter, by galloping through the
//! longer one: for each id of the short list, double a step until it overshoots, then bisect the last stretch. That is
//! `k log(n/k)` for lists of `k` and `n`, which beats a merge once `n` is several times `k`.
//!
//! Lists are `&[u32]`, not a set type: the index stores them contiguously (a `Groups`), and a list that was just
//! intersected is another slice to intersect. Ids in a list strictly increase.

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

/// Two cursors that advance by comparisons turned into arithmetic: whichever list has the smaller id moves on, both
/// if they are equal, and the id is written down either way and kept only when they were. No branch depends on the
/// data, so none is mispredicted: 2 to 3 times faster than the match on the comparison, in the benchmark.
fn merge(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.resize(a.len().min(b.len()), 0);
    let (mut at_a, mut at_b, mut found) = (0, 0, 0);
    while at_a < a.len() && at_b < b.len() {
        let (x, y) = (a[at_a], b[at_b]);
        out[found] = x;
        found += usize::from(x == y);
        at_a += usize::from(x <= y);
        at_b += usize::from(x >= y);
    }
    out.truncate(found);
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
    fn the_first_id_at_least_a_target() {
        let list = [2, 4, 6, 8, 10, 12, 14, 16, 18];
        for target in 0..21 {
            assert_eq!(first_at_least(&list, target), list.partition_point(|&id| id < target), "target {target}");
        }
        assert_eq!(first_at_least(&[], 3), 0);
    }

    #[test]
    fn both_ways_of_intersecting_agree_with_sets_whatever_the_lengths() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        for case in 0..1500 {
            let (len_a, len_b) = (rng.below(60), if rng.below(3) == 0 { rng.below(5000) } else { rng.below(60) });
            let domain = [30, 200, 100_000][rng.below(3)];
            let (a, b) = (random_list(&mut rng, len_a, domain), random_list(&mut rng, len_b, domain));
            let expected = by_sets(&a, &b);
            let (mut by_merge, mut by_gallop) = (Vec::new(), Vec::new());
            merge(&a, &b, &mut by_merge);
            let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
            gallop(short, long, &mut by_gallop);
            assert_eq!(
                (&by_merge, &by_gallop, intersection(&a, &b)),
                (&expected, &expected, expected.clone()),
                "case {case}"
            );
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
                let branchless = time(9, |out| merge(&a, &b, out));
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
    fn bench_gallop_against_merge_by_how_much_longer_one_list_is() {
        let mut rng = Rng::new(0x1234_5678_9ABC_DEF1);
        let long = random_list(&mut rng, 1_000_000, 4_000_000);
        for skew in [2, 4, 8, 16, 32, 64, 128, 1024] {
            let short: Vec<u32> = random_list(&mut rng, long.len() / skew, 4_000_000);
            let merged = time(9, |out| merge(&short, &long, out));
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
