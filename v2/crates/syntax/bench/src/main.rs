use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use axiom_core::FileId;

const WARMUPS: usize = 2;
const SAMPLES: usize = 30;
const RECORD: &str = "2026-01-15 checking -> 5 USD\n  groceries 5 USD\n";

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every allocation operation delegates unchanged to `System`; the
// atomics only observe request counts and sizes.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn fixture(records: usize) -> String {
    let mut source = String::with_capacity(records * 54);
    for _ in 0..records {
        source.push_str(RECORD);
    }
    source
}

fn bench(label: &str, source: &str) {
    for _ in 0..WARMUPS {
        let (file, diagnostics) = axiom_syntax::parse(FileId(0), black_box(source));
        assert!(diagnostics.is_empty(), "{} diagnostics", diagnostics.len());
        black_box(file.items.len());
    }

    let mut parse_times = Vec::with_capacity(SAMPLES);
    let mut allocation_calls = 0;
    let mut requested_bytes = 0;
    let mut items = 0;

    for _ in 0..SAMPLES {
        ALLOCATION_CALLS.store(0, Ordering::SeqCst);
        REQUESTED_BYTES.store(0, Ordering::SeqCst);

        let start = Instant::now();
        let (file, diagnostics) = axiom_syntax::parse(FileId(0), black_box(source));
        parse_times.push(start.elapsed().as_nanos());

        assert!(diagnostics.is_empty(), "{} diagnostics", diagnostics.len());
        items = file.items.len();
        black_box(&file);
        allocation_calls += ALLOCATION_CALLS.load(Ordering::SeqCst);
        requested_bytes += REQUESTED_BYTES.load(Ordering::SeqCst);
    }

    parse_times.sort_unstable();
    println!(
        "{label}: input_bytes={} items={items} median_parse_ns={} mean_allocation_calls={} mean_requested_bytes={}",
        source.len(),
        parse_times[SAMPLES / 2],
        allocation_calls / SAMPLES,
        requested_bytes / SAMPLES,
    );
}

fn main() {
    let small = fixture(700);
    let large = fixture(30_000);
    bench("small", &small);
    bench("large", &large);
}
