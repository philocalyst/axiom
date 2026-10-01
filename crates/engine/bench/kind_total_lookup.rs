use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

const PLACE_COUNT: usize = 500_000;
const REPEATS: usize = 300;
const OWNER: u8 = 3;
const BANK_FIRST: u16 = 1;
const BANK_END: u16 = 17;

fn scanned(kinds: &[u16], owners: &[u8], values: &[u64]) -> u64 {
    let mut total = 0;
    for _ in 0..REPEATS {
        let mut sum = 0;
        for at in 0..PLACE_COUNT {
            let kind = kinds[at];
            if (BANK_FIRST..BANK_END).contains(&kind) && owners[at] == OWNER {
                sum += values[at];
            }
        }
        total += black_box(sum);
    }
    black_box(total)
}

fn indexed(groups: &HashMap<u32, Vec<usize>>, owners: &[u8], values: &[u64]) -> u64 {
    let mut total = 0;
    for _ in 0..REPEATS {
        let mut sum = 0;
        for &at in groups.get(&7).unwrap() {
            if owners[at] == OWNER {
                sum += values[at];
            }
        }
        total += black_box(sum);
    }
    black_box(total)
}

fn median_duration(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn median_ratio(mut samples: Vec<f64>) -> f64 {
    samples.sort_unstable_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let kinds: Vec<_> = (0..PLACE_COUNT).map(|at| (at % 256 + 1) as u16).collect();
    let owners: Vec<_> = (0..PLACE_COUNT).map(|at| (at % 8) as u8).collect();
    let values: Vec<_> = (0..PLACE_COUNT).map(|at| (at % 97 + 1) as u64).collect();
    let build = || {
        kinds
            .iter()
            .enumerate()
            .filter_map(|(at, kind)| (BANK_FIRST..BANK_END).contains(kind).then_some(at))
            .collect::<Vec<_>>()
    };
    let mut build_times = Vec::new();
    for _ in 0..9 {
        let start = Instant::now();
        black_box(build());
        build_times.push(start.elapsed());
    }
    let matching = build();
    let build_time = median_duration(build_times);
    let membership_bytes = matching.capacity() * std::mem::size_of::<usize>();
    let groups = HashMap::from([(7, matching)]);
    assert_eq!(
        scanned(&kinds, &owners, &values),
        indexed(&groups, &owners, &values)
    );

    let mut scan_times = Vec::new();
    let mut index_times = Vec::new();
    let mut speedups = Vec::new();
    for sample in 0..9 {
        let (scan, index) = if sample % 2 == 0 {
            let start = Instant::now();
            scanned(&kinds, &owners, &values);
            let scan = start.elapsed();
            let start = Instant::now();
            indexed(&groups, &owners, &values);
            (scan, start.elapsed())
        } else {
            let start = Instant::now();
            indexed(&groups, &owners, &values);
            let index = start.elapsed();
            let start = Instant::now();
            scanned(&kinds, &owners, &values);
            (start.elapsed(), index)
        };
        scan_times.push(scan);
        index_times.push(index);
        speedups.push(scan.as_secs_f64() / index.as_secs_f64());
    }
    let (scan, index) = (median_duration(scan_times), median_duration(index_times));
    println!(
        "places={PLACE_COUNT}, repetitions={REPEATS}, matching_kind_places={}",
        groups[&7].len()
    );
    println!("index_build_median={build_time:?}, membership_storage≈{membership_bytes} bytes");
    println!(
        "scan_median={scan:?}, sparse_index_median={index:?}, paired_speedup_median={:.2}x",
        median_ratio(speedups)
    );
}
