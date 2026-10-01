//! Compare the existing pair/order materialization with a two-pass stable fill.
//! Run with `cargo bench -p axiom-core --bench groups --locked --offline`.

use axiom_core::Id;
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Meter;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: Meter = Meter;

// This allocator is benchmark-only. The actual candidate builder below uses
// initialized `Vec<V>` storage and contains no unsafe code.
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            if ACTIVE.load(Ordering::Relaxed) {
                REQUESTED.fetch_add(layout.size(), Ordering::Relaxed);
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                PEAK.fetch_max(live, Ordering::Relaxed);
            }
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, old, new_size) };
        if !new_pointer.is_null() {
            LIVE.fetch_sub(old.size(), Ordering::Relaxed);
            let live = LIVE.fetch_add(new_size, Ordering::Relaxed) + new_size;
            if ACTIVE.load(Ordering::Relaxed) {
                REQUESTED.fetch_add(new_size, Ordering::Relaxed);
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
                PEAK.fetch_max(live, Ordering::Relaxed);
            }
        }
        new_pointer
    }
}

struct Packed<K, V> {
    starts: Vec<u32>,
    values: Vec<V>,
    key: std::marker::PhantomData<fn() -> K>,
}

fn current<K, V: Copy>(keys: usize, pairs: impl Iterator<Item = (Id<K>, V)> + Clone) -> axiom_core::Groups<K, V> {
    axiom_core::Groups::build(keys, pairs)
}

fn old_build<K, V: Copy>(keys: usize, pairs: impl Iterator<Item = (Id<K>, V)>) -> Packed<K, V> {
    let pairs: Vec<_> = pairs.collect();
    let mut starts = vec![0u32; keys + 1];
    for (key, _) in &pairs {
        starts[key.index() + 1] += 1;
    }
    for at in 1..starts.len() {
        starts[at] += starts[at - 1];
    }
    let mut order = vec![0u32; pairs.len()];
    for (item, (key, _)) in pairs.iter().enumerate() {
        let next = &mut starts[key.index()];
        order[*next as usize] = item as u32;
        *next += 1;
    }
    starts.copy_within(..keys, 1);
    starts[0] = 0;
    let values = order.iter().map(|&at| pairs[at as usize].1).collect();
    Packed { starts, values, key: std::marker::PhantomData }
}

fn rows(count: usize, keys: usize) -> Vec<(Id<()>, u32)> {
    (0..count)
        .map(|at| {
            let key = at.wrapping_mul(2_654_435_761) % keys;
            (Id::new(key as u32), at as u32)
        })
        .collect()
}

fn sample<T>(name: &str, build: impl FnOnce() -> T) -> T {
    let baseline = LIVE.load(Ordering::Relaxed);
    REQUESTED.store(0, Ordering::Relaxed);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    let result = build();
    ACTIVE.store(false, Ordering::Relaxed);
    let current = LIVE.load(Ordering::Relaxed);
    eprintln!(
        "{name}: alloc_calls={} requested_bytes={} peak_extra_bytes={} retained_extra_bytes={}",
        ALLOCATIONS.load(Ordering::Relaxed),
        REQUESTED.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed).saturating_sub(baseline),
        current.saturating_sub(baseline),
    );
    result
}

fn time<T>(name: &str, repetitions: usize, build: impl Fn() -> T) -> Duration {
    let start = Instant::now();
    for _ in 0..repetitions {
        black_box(build());
    }
    let elapsed = start.elapsed();
    let each = elapsed / repetitions as u32;
    eprintln!("{name}: {repetitions} builds, {each:?}/build");
    each
}

fn compare(count: usize, keys: usize) {
    let input = rows(count, keys);
    let old = sample("old", || old_build(keys, input.iter().copied()));
    let new = sample("two-pass", || current(keys, input.iter().copied()));
    assert_eq!(old.values.len(), new.values().len());
    for key in 0..keys {
        let id = Id::new(key as u32);
        let start = old.starts[key] as usize;
        let end = old.starts[key + 1] as usize;
        assert_eq!(&old.values[start..end], &new[id]);
    }

    let repetitions = if count <= 100_000 { 12 } else { 3 };
    time("old", repetitions, || old_build(keys, input.iter().copied()));
    time("two-pass", repetitions, || current(keys, input.iter().copied()));
}

fn main() {
    compare(100_000, 2_000);
    compare(1_000_000, 20_000);
}
