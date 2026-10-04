//! Values grouped by a typed key, stored contiguously.
//!
//! The compressed-row layout: one flat vector of values and one offset per
//! key. Building is a stable counting sort; a lookup is two loads and a slice.
//! Governance tables (which laws watch which place) and per-place flow indices
//! use it instead of a `Vec<Vec<_>>`.

use std::ops::Index;

use crate::id::Id;

#[derive(Debug)]
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
    pub fn build(keys: usize, pairs: impl Iterator<Item = (Id<K>, V)> + Clone) -> Groups<K, V> {
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

    /// The values under the keys `first..after`, which are contiguous: every row of a stretch of keys at once.
    pub fn span(&self, first: usize, after: usize) -> &[V] {
        match (self.starts.get(first), self.starts.get(after)) {
            (Some(&a), Some(&b)) => &self.values[a as usize..b as usize],
            _ => &[],
        }
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
        assert_eq!(g.span(0, 3), &['b', 'd', 'a', 'c'], "keys 0, 1 and 2 are the first three rows");
        assert_eq!(g.span(2, 4), &['a', 'c']);
        assert!(g.span(3, 3).is_empty() && g.span(2, 9).is_empty());
    }

    #[test]
    fn indices_grouped_by_key_are_a_stable_counting_sort() {
        let keys = [2, 0, 2, 1, 0];
        let g = Groups::<(), u32>::build(4, keys.iter().enumerate().map(|(at, &key)| (Id::new(key), at as u32)));
        assert_eq!(g.values(), [1, 4, 3, 0, 2], "the items in the order of their keys, each key's in input order");
        let sizes: Vec<_> = g.iter().map(|(_, row)| row.len()).collect();
        assert_eq!(sizes, [2, 1, 2, 0]);
    }

    #[test]
    fn nothing_grouped_is_a_table_of_empty_keys() {
        let none = Groups::<(), u32>::build(0, [].into_iter());
        assert_eq!((none.keys(), none.values().len()), (0, 0));
        let g = Groups::<(), char>::build(3, [].into_iter());
        assert_eq!((g.keys(), g.values().len()), (3, 0));
        assert!(g[Id::new(0)].is_empty() && g[Id::new(2)].is_empty());
    }
}
