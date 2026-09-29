//! Data parallelism over borrowed slices.
//!
//! Workers borrow their inputs through [`std::thread::scope`] and return their
//! results by value, in order. Nothing is shared mutably, so there is no `Arc`
//! and no `Mutex`.

use std::thread;

/// Below this many items per worker, threads cost more than they save.
const GRAIN: usize = 256;

fn workers(items: usize) -> usize {
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    cores.min(items / GRAIN).max(1)
}

/// `items.iter().map(f).collect()`, on every core.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = workers(items.len());
    if n == 1 {
        return items.iter().map(f).collect();
    }
    let f = &f;
    thread::scope(|scope| {
        let parts: Vec<_> = items
            .chunks(items.len().div_ceil(n))
            .map(|chunk| scope.spawn(move || chunk.iter().map(f).collect::<Vec<R>>()))
            .collect();
        let mut out = Vec::with_capacity(items.len());
        for part in parts {
            out.extend(part.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
        }
        out
    })
}

/// Runs `f` on every item, in place, on every core.
pub fn for_each_mut<T: Send>(items: &mut [T], f: impl Fn(&mut T) + Sync) {
    let n = workers(items.len());
    if n == 1 {
        return items.iter_mut().for_each(f);
    }
    let f = &f;
    let size = items.len().div_ceil(n);
    thread::scope(|scope| {
        for chunk in items.chunks_mut(size) {
            scope.spawn(move || chunk.iter_mut().for_each(f));
        }
    });
}

/// Runs two closures at once and returns both results.
pub fn join<A: Send, B: Send>(a: impl FnOnce() -> A + Send, b: impl FnOnce() -> B + Send) -> (A, B) {
    thread::scope(|scope| {
        let left = scope.spawn(a);
        let right = b();
        (left.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)), right)
    })
}
