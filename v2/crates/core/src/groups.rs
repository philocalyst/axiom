//! Values grouped by a typed key, stored contiguously.
//!
//! The compressed-row layout: one flat vector of values and one offset per
//! key. Building is a stable counting sort; a lookup is two loads and a slice.
//! Governance tables (which laws watch which place) and per-place flow indices
//! use it instead of a `Vec<Vec<_>>`.

use std::ops::Index;

use crate::id::Id;

pub struct Groups<K, V> {
    starts: Vec<u32>,
    values: Vec<V>,
    of: std::marker::PhantomData<fn() -> K>,
}

impl<K, V> Groups<K, V> {
    /// Groups `pairs` under `keys` keys. Values under one key keep their input
    /// order.
    pub fn build(keys: usize, pairs: impl IntoIterator<Item = (Id<K>, V)>) -> Groups<K, V> {
        let pairs: Vec<(Id<K>, V)> = pairs.into_iter().collect();
        let mut starts = vec![0u32; keys + 1];
        for (key, _) in &pairs {
            starts[key.index() + 1] += 1;
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        let mut fill = starts.clone();
        let mut slots: Vec<Option<V>> = std::iter::repeat_with(|| None).take(pairs.len()).collect();
        for (key, value) in pairs {
            let at = &mut fill[key.index()];
            slots[*at as usize] = Some(value);
            *at += 1;
        }
        let values = slots.into_iter().map(|v| v.expect("every slot filled")).collect();
        Groups { starts, values, of: std::marker::PhantomData }
    }

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
        let g = Groups::build(4, pairs);
        assert_eq!(&g[Id::new(2)], &['a', 'c']);
        assert_eq!(&g[Id::new(0)], &['b']);
        assert!(g[Id::new(3)].is_empty() && g[Id::new(9)].is_empty());
    }
}
