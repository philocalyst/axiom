//! Data parallelism over borrowed slices.
//!
//! Workers borrow their inputs through [`std::thread::scope`] and return their
//! results by value, in order. Nothing is shared mutably, so there is no `Arc`
//! and no `Mutex`: the only shared state is one atomic cursor that hands out
//! chunks, so a worker that drew cheap chunks simply draws more.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// Items per chunk for light work (flows, transactions, places): below this,
/// handing out a chunk costs more than doing it.
const GRAIN: usize = 256;

/// `items.iter().map(f).collect()`, on every core, for many light items.
pub fn map<'t, T: Sync, R: Send>(items: &'t [T], f: impl Fn(&'t T) -> R + Sync) -> Vec<R> {
    chunked(items, GRAIN, f)
}

/// `items.iter().map(f).collect()`, on every core, for a few heavy items (files):
/// each item is its own unit of work.
pub fn map_each<'t, T: Sync, R: Send>(items: &'t [T], f: impl Fn(&'t T) -> R + Sync) -> Vec<R> {
    chunked(items, 1, f)
}

/// Runs `f` on every item, in place, on every core.
pub fn for_each_mut<T: Send>(items: &mut [T], f: impl Fn(&mut T) + Sync) {
    let workers = workers(items.len().div_ceil(GRAIN));
    if workers <= 1 {
        return items.iter_mut().for_each(f);
    }
    let f = &f;
    thread::scope(|scope| {
        for part in items.chunks_mut(items.len().div_ceil(workers)) {
            scope.spawn(move || part.iter_mut().for_each(f));
        }
    });
}

/// Runs two closures at once and returns both results.
pub fn join<A: Send, B: Send>(a: impl FnOnce() -> A + Send, b: impl FnOnce() -> B + Send) -> (A, B) {
    thread::scope(|scope| {
        let left = scope.spawn(a);
        let right = b();
        (settle(left.join()), right)
    })
}

/// Maps chunks of `grain` items, drawn from a shared cursor by one worker per
/// core, then puts the results back in item order.
fn chunked<'t, T: Sync, R: Send>(items: &'t [T], grain: usize, f: impl Fn(&'t T) -> R + Sync) -> Vec<R> {
    let chunks: Vec<&'t [T]> = items.chunks(grain).collect();
    let workers = workers(chunks.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let (f, chunks, cursor) = (&f, &chunks, &AtomicUsize::new(0));
    let drawn: Vec<Vec<(usize, Vec<R>)>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let at = cursor.fetch_add(1, Ordering::Relaxed);
                        let Some(chunk) = chunks.get(at) else { return mine };
                        mine.push((at, chunk.iter().map(f).collect()));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|handle| settle(handle.join())).collect()
    });
    let mut parts: Vec<(usize, Vec<R>)> = drawn.into_iter().flatten().collect();
    parts.sort_unstable_by_key(|&(at, _)| at);
    let mut out = Vec::with_capacity(items.len());
    for (_, part) in parts {
        out.extend(part);
    }
    out
}

fn workers(units: usize) -> usize {
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    cores.min(units)
}

/// A worker's result, or its panic carried on into the caller.
fn settle<R>(joined: thread::Result<R>) -> R {
    joined.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

#[cfg(test)]
mod tests {
    #[test]
    fn results_keep_item_order_and_may_borrow_items() {
        let words: Vec<String> = (0..10_000).map(|n| n.to_string()).collect();
        let borrowed: Vec<&str> = super::map(&words, |word| word.as_str());
        assert!(borrowed.iter().zip(&words).all(|(a, b)| *a == b));
        let lengths = super::map_each(&words[..9], |word| word.len());
        assert_eq!(lengths, vec![1; 9]);
    }
}
