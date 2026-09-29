//! Reproducible layout experiment. No production checker or million-claim
//! verification is benchmarked here. Run optimized, without concurrent tests.
mod alloc;
mod data;
mod number;

use data::{Baseline, BigPool, CompactPool, Dense, Ledger, Soa, Value};
use num_rational::BigRational;
use num_traits::Zero;
use number::{NumberRef, Sum};
use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: alloc::Counting = alloc::Counting;

#[derive(Debug, Eq, PartialEq)]
struct QueryResult {
    count: usize,
    digest: u64,
    sums: [BigRational; 3],
}
fn mix(digest: &mut u64, n: u64) {
    *digest = digest.wrapping_mul(0x100000001b3).wrapping_add(n);
}
fn text_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// A tiny lowered physical plan: bindings are row handles, fields are resolved
/// slots/columns and predicates/projection/aggregate execute in one pass. The
/// exact BigRational results are the same as an owning relational interpreter.
fn query<L: Ledger>(ledger: &L) -> QueryResult {
    let mut totals = std::array::from_fn::<_, 3, _>(|_| Sum::zero());
    let mut count = 0;
    let mut digest = 0;
    for i in 0..ledger.len() {
        let fact = ledger.fact(i);
        if !fact.sale || !fact.active || fact.date < 225 {
            continue;
        }
        let target = fact.reference.expect("sale reference invariant");
        let purchase = ledger.fact(target);
        if !purchase.active || purchase.account >= "account16" {
            continue;
        }
        count += 1;
        mix(&mut digest, i as u64);
        mix(&mut digest, target as u64);
        mix(&mut digest, text_hash(purchase.account));
        mix(&mut digest, fact.unit as u64);
        totals[fact.unit].add(fact.amount);
    }
    QueryResult {
        count,
        digest,
        sums: totals.map(Sum::into_big),
    }
}

/// Pseudorandom point reads use already resolved occurrence handles. The
/// baseline still resolves each explicit purchase reference through its ID
/// BTreeMap. No number arithmetic masks row/column locality in this query.
fn lookup<L: Ledger>(ledger: &L, reads: usize) -> u64 {
    let mut rng = 0x123456789abcdefu64;
    let mut digest = 0;
    for _ in 0..reads {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let i = rng as usize % ledger.len();
        let fact = ledger.fact(i);
        mix(&mut digest, fact.date as u64);
        mix(&mut digest, text_hash(fact.account));
        mix(&mut digest, text_hash(fact.memo));
        mix(&mut digest, text_hash(fact.party));
        mix(&mut digest, text_hash(ledger.identity(i)));
        if let Some(reference) = fact.reference {
            mix(&mut digest, ledger.fact(reference).date as u64);
        }
    }
    digest
}

fn measure<T>(live: usize, operation: impl FnOnce() -> T) -> (T, f64, alloc::Stats) {
    alloc::start(live);
    let start = Instant::now();
    let result = black_box(operation());
    let millis = start.elapsed().as_secs_f64() * 1000.0;
    let stats = alloc::stop();
    (result, millis, stats)
}

fn timed<T>(operation: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let result = black_box(operation());
    (result, start.elapsed().as_secs_f64() * 1000.0)
}

fn run<L: Ledger>(
    name: &str,
    n: usize,
    repeats: usize,
    build: impl Fn() -> L,
    expected: &mut Option<QueryResult>,
    expected_lookup: &mut Option<u64>,
) {
    // Separate timing from instrumentation: atomic counters on every heap
    // operation would otherwise penalize allocation-heavy representations.
    let (timing_fixture, build_ms) = timed(&build);
    drop(timing_fixture);
    let (ledger, _, constructed) = measure(0, build);
    let (warm, _, qs) = measure(constructed.live, || query(&ledger));
    if let Some(expected) = expected {
        assert_eq!(&warm, expected, "semantic mismatch in {name}");
    } else {
        *expected = Some(warm);
    }
    let reads = n.min(100_000);
    let (warm_lookup, _, ls) = measure(constructed.live, || lookup(&ledger, reads));
    if let Some(expected) = expected_lookup {
        assert_eq!(warm_lookup, *expected, "lookup mismatch in {name}");
    } else {
        *expected_lookup = Some(warm_lookup);
    }
    let mut best_query = f64::INFINITY;
    let mut best_lookup = f64::INFINITY;
    for _ in 0..repeats {
        let (result, ms) = timed(|| query(black_box(&ledger)));
        assert_eq!(&result, expected.as_ref().unwrap());
        if ms < best_query {
            best_query = ms;
        }
        drop(result);
        let (result, ms) = timed(|| lookup(black_box(&ledger), reads));
        assert_eq!(result, warm_lookup);
        if ms < best_lookup {
            best_lookup = ms;
        }
    }
    println!(
        "layout,{name},{n},{build_ms:.3},{},{},{},{},{best_query:.3},{},{},{best_lookup:.3},{},{},{}",
        constructed.calls,
        constructed.bytes,
        constructed.live,
        constructed.peak,
        qs.calls,
        qs.bytes,
        ls.calls,
        ls.bytes,
        expected.as_ref().unwrap().count
    );
    println!(
        "checksum,{name},{n},{:016x},{warm_lookup:016x},{:?}",
        expected.as_ref().unwrap().digest,
        expected.as_ref().unwrap().sums
    );
}

fn owned_name(row: &Value, key: &str) -> Value {
    // Mirrors the current expression Name implementation: clone the complete
    // root before destructively removing one field.
    match row.clone() {
        Value::Record(mut fields) => fields.remove(key).unwrap(),
        _ => unreachable!(),
    }
}
fn owned_eval(relation: &[Value]) -> BigRational {
    let rows = relation.to_vec(); // `rows` returns an owning List.
    let mut filtered = Vec::new();
    for row in rows {
        let binding = row.clone(); // iterator inserts an owning binding.
        if matches!(owned_name(&binding, "active"), Value::Bool(true)) {
            filtered.push(row);
        }
    }
    let mut projected = Vec::with_capacity(filtered.len());
    for row in filtered {
        let binding = row.clone();
        projected.push(owned_name(&binding, "amount"));
    }
    let mut total = BigRational::zero();
    for value in projected {
        if let Value::Quantity(value, _) = value {
            total += value;
        }
    }
    total
}
fn borrowed_eval(relation: &[Value]) -> BigRational {
    let mut total = BigRational::zero();
    for row in relation {
        let Value::Record(fields) = row else {
            unreachable!()
        };
        if matches!(fields["active"], Value::Bool(true)) {
            let Value::Quantity(value, _) = &fields["amount"] else {
                unreachable!()
            };
            total += value;
        }
    }
    total
}

fn interpreter_probe(n: usize, repeats: usize) {
    let baseline = Baseline::new(n);
    let relation = baseline.relation();
    let expected = borrowed_eval(&relation);
    for (name, operation) in [
        ("owned-cloning", owned_eval as fn(&[Value]) -> BigRational),
        ("borrowed-fused", borrowed_eval),
    ] {
        let mut best = f64::INFINITY;
        let (actual, _, stats) = measure(0, || operation(black_box(&relation)));
        assert_eq!(actual, expected);
        drop(actual);
        for _ in 0..repeats {
            let (actual, ms) = timed(|| operation(black_box(&relation)));
            assert_eq!(actual, expected);
            if ms < best {
                best = ms;
            }
        }
        println!(
            "interpreter,{name},{n},{best:.3},{},{},{},{}",
            stats.calls, stats.bytes, stats.live, stats.peak
        );
    }
}

fn numeric_probe(n: usize, repeats: usize) {
    // Ordinary exact cents: no exceptional magnitude so the common fast path
    // stays inline. Mixed ledgers above also exercise wide and arbitrary values.
    let big = (0..n)
        .map(|i| BigRational::new((i as i64 % 100_000 - 50_000).into(), 100.into()))
        .collect::<Vec<_>>();
    let small = (0..n)
        .map(|i| number::Exact64 {
            numerator: i as i64 % 100_000 - 50_000,
            denominator: 100,
            exception: 0,
        })
        .collect::<Vec<_>>();
    let expected = big.iter().fold(BigRational::zero(), |a, b| a + b);
    for compact in [false, true] {
        let mut best = f64::INFINITY;
        let operation = || {
            let mut sum = Sum::zero();
            if compact {
                for &value in black_box(&small) {
                    sum.add(NumberRef::Compact(value, &[]));
                }
            } else {
                for value in black_box(&big) {
                    sum.add(NumberRef::Big(value));
                }
            }
            sum.into_big()
        };
        let (actual, _, stats) = measure(0, operation);
        assert_eq!(actual, expected);
        drop(actual);
        for _ in 0..repeats {
            let (actual, ms) = timed(operation);
            assert_eq!(actual, expected);
            if ms < best {
                best = ms;
            }
        }
        println!(
            "numeric,{},{n},{best:.3},{},{},{},{}",
            if compact {
                "inline-i128"
            } else {
                "big-rational"
            },
            stats.calls,
            stats.bytes,
            stats.live,
            stats.peak
        );
    }
}

fn sizes() {
    macro_rules! size {
        ($t:ty) => {
            println!("sizeof,{},{}", stringify!($t), size_of::<$t>())
        };
    }
    size!(Value);
    size!(data::Row);
    size!(BigRational);
    size!(data::Cell);
    size!(data::DenseRow);
    size!(number::Exact64);
    size!(number::Exceptional);
    size!(number::BoxedNumber);
    size!(data::Location);
    size!(data::RowId);
    size!(Option<data::RowId>);
    size!(num_rational::Ratio<i128>);
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let sizes_arg = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("10000,100000,1000000");
    let repeats = args
        .get(2)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(3);
    assert!(repeats > 0);
    sizes();
    println!(
        "columns,layout,n,build_ms,build_allocs,requested_bytes,live_bytes,peak_bytes,query_ms,query_allocs,query_bytes,lookup_ms,lookup_allocs,lookup_bytes,selected"
    );
    for n in sizes_arg.split(',').map(|s| s.parse::<usize>().unwrap()) {
        assert!(n > 0 && n < u32::MAX as usize);
        let mut expected = None;
        let mut expected_lookup = None;
        run(
            "btree-values",
            n,
            repeats,
            || Baseline::new(n),
            &mut expected,
            &mut expected_lookup,
        );
        run(
            "dense-big",
            n,
            repeats,
            || Dense::<BigPool>::new(n),
            &mut expected,
            &mut expected_lookup,
        );
        run(
            "dense-compact",
            n,
            repeats,
            || Dense::<CompactPool>::new(n),
            &mut expected,
            &mut expected_lookup,
        );
        run(
            "soa-compact",
            n,
            repeats,
            || Soa::new(n),
            &mut expected,
            &mut expected_lookup,
        );
    }
    interpreter_probe(10_000, repeats);
    numeric_probe(100_000, repeats);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_equivalence_all_layouts() {
        let baseline = Baseline::new(20_000);
        let expected = query(&baseline);
        assert_eq!(query(&Dense::<BigPool>::new(20_000)), expected);
        assert_eq!(query(&Dense::<CompactPool>::new(20_000)), expected);
        assert_eq!(query(&Soa::new(20_000)), expected);
        let expected = lookup(&baseline, 20_000);
        assert_eq!(lookup(&Dense::<CompactPool>::new(20_000), 20_000), expected);
        assert_eq!(lookup(&Soa::new(20_000), 20_000), expected);
    }
    #[test]
    fn borrowed_projection_preserves_exact_result() {
        let baseline = Baseline::new(2000);
        let relation = baseline.relation();
        assert_eq!(owned_eval(&relation), borrowed_eval(&relation));
    }
}
