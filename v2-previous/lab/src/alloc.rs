//! Single-threaded, scoped allocator instrumentation. Counters are disabled
//! outside measurements; no allocation occurs in this allocator itself.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

pub struct Counting;
static ENABLED: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(bytes: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes, Relaxed);
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: Every operation forwards exactly the original pointer/Layout to the
// system allocator; bookkeeping never reads allocated memory or allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ENABLED.load(Relaxed) {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ENABLED.load(Relaxed) {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ENABLED.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(ptr, layout, size) };
        if !out.is_null() && ENABLED.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
            allocated(size);
        }
        out
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub calls: usize,
    pub bytes: usize,
    pub live: usize,
    pub peak: usize,
}

/// Start with the requested bytes of allocations that remain live from a prior
/// measured construction. No unrelated preexisting allocations may be freed
/// inside the scope. This benchmark is deliberately single threaded.
pub fn start(initial_live: usize) {
    assert!(!ENABLED.swap(false, Relaxed));
    CALLS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    LIVE.store(initial_live, Relaxed);
    PEAK.store(initial_live, Relaxed);
    ENABLED.store(true, Relaxed);
}
pub fn stop() -> Stats {
    ENABLED.store(false, Relaxed);
    Stats {
        calls: CALLS.load(Relaxed),
        bytes: BYTES.load(Relaxed),
        live: LIVE.load(Relaxed),
        peak: PEAK.load(Relaxed),
    }
}
