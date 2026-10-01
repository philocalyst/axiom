//! Values grouped by a typed key, stored contiguously.
//!
//! The compressed-row layout: one flat vector of values and one offset per
//! key. Building is a stable counting sort; a lookup is two loads and a slice.
//! Governance tables (which laws watch which place) and per-place flow indices
//! use it instead of a `Vec<Vec<_>>`.

use std::ops::Index;

use crate::id::Id;

/// A stable counting sort of `n` items into `buckets` buckets, where `key(i)`
/// is the bucket of item `i`. Returns each bucket's offset (`buckets + 1`
/// of them, the last being `n`) and the items' indices in bucket order: the
/// ones in bucket `b` are `order[starts[b]..starts[b + 1]]`, in input order.
pub(crate) fn bucket(buckets: usize, n: usize, key: impl Fn(usize) -> usize) -> (Vec<u32>, Vec<u32>) {
    let mut starts = vec![0u32; buckets + 1];
    for item in 0..n {
        starts[key(item) + 1] += 1;
    }
    for at in 1..starts.len() {
        starts[at] += starts[at - 1];
    }
    // Each bucket fills from its start, so the offsets end up one bucket late:
    // `starts[b]` becomes where bucket `b + 1` begins, and shifts back.
    let mut order = vec![0u32; n];
    for item in 0..n {
        let next = &mut starts[key(item)];
        order[*next as usize] = item as u32;
        *next += 1;
    }
    starts.copy_within(..buckets, 1);
    starts[0] = 0;
    (starts, order)
}

pub struct Groups<K, V> {
    starts: Vec<u32>,
    values: Vec<V>,
    of: std::marker::PhantomData<fn() -> K>,
}

impl<K, V: Copy> Groups<K, V> {
    /// Groups `pairs` under `keys` keys. Values under one key keep their input
    /// order. The iterator must be cloneable so the first pass can count each
    /// bucket and the second can fill the initialized output directly; callers
    /// with borrowed input can pass a cheap cloned iterator such as
    /// `items.iter().copied()`.
    pub fn build(
        keys: usize,
        pairs: impl Iterator<Item = (Id<K>, V)> + Clone,
    ) -> Groups<K, V> {
        let mut starts = vec![0u32; keys + 1];
        let mut count = 0;
        let mut first = None;
        for (key, value) in pairs.clone() {
            starts[key.index() + 1] += 1;
            first.get_or_insert(value);
            count += 1;
        }
        for at in 1..starts.len() {
            starts[at] += starts[at - 1];
        }

        let mut values = first.map_or_else(Vec::new, |seed| vec![seed; count]);
        for (key, value) in pairs {
            let next = &mut starts[key.index()];
            values[*next as usize] = value;
            *next += 1;
        }
        starts.copy_within(..keys, 1);
        starts[0] = 0;
        Groups { starts, values, of: std::marker::PhantomData }
    }
}

impl<K, V> Groups<K, V> {
    pub fn keys(&self) -> usize {
        self.starts.len() - 1
    }

    /// Every value, grouped by key in key order.
    pub fn values(&self) -> &[V] {
        &self.values
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id<K>, &[V])> {
        (0..self.keys()).map(|k| (Id::new(k as u32), &self[Id::new(k as u32)]))
    }
}

impl<K, V: Clone> Clone for Groups<K, V> {
    fn clone(&self) -> Groups<K, V> {
        Groups { starts: self.starts.clone(), values: self.values.clone(), of: std::marker::PhantomData }
    }
}

impl<K, V> Default for Groups<K, V> {
    fn default() -> Groups<K, V> {
        Groups { starts: vec![0], values: Vec::new(), of: std::marker::PhantomData }
    }
}

impl<K, V> Index<Id<K>> for Groups<K, V> {
    type Output = [V];
    /// The values under `key`; empty for keys beyond the table.
    fn index(&self, key: Id<K>) -> &[V] {
        match (self.starts.get(key.index()), self.starts.get(key.index() + 1)) {
            (Some(&a), Some(&b)) => &self.values[a as usize..b as usize],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_stable() {
        let pairs = [(2, 'a'), (0, 'b'), (2, 'c'), (1, 'd')].map(|(k, v)| (Id::<()>::new(k), v));
        let g = Groups::build(4, pairs.into_iter());
        assert_eq!(&g[Id::new(2)], &['a', 'c']);
        assert_eq!(&g[Id::new(0)], &['b']);
        assert!(g[Id::new(3)].is_empty() && g[Id::new(9)].is_empty());
    }

    #[test]
    fn a_bucket_sort_is_stable_and_offsets_end_at_the_count() {
        let keys = [2, 0, 2, 1, 0];
        let (starts, order) = bucket(4, keys.len(), |at| keys[at]);
        assert_eq!(starts, [0, 2, 3, 5, 5]);
        assert_eq!(order, [1, 4, 3, 0, 2]);
    }

    #[test]
    fn nothing_grouped_is_a_table_of_empty_keys() {
        let (starts, order) = bucket(0, 0, |_| unreachable!("no items"));
        assert_eq!((starts, order), (vec![0], vec![]));
        let g = Groups::<(), char>::build(3, [].into_iter());
        assert_eq!((g.keys(), g.values().len()), (3, 0));
        assert!(g[Id::new(0)].is_empty() && g[Id::new(2)].is_empty());
    }
}
