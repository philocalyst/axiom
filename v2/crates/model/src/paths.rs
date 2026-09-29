//! Trees of `/`-separated paths.
//!
//! Systems, entities and places are all named by paths that nest
//! (`paypal/john` sits under `paypal`). Given the paths that were written, this
//! creates every missing ancestor and lays the whole family out in pre-order
//! with siblings alphabetical by segment.

use axiom_core::{Id, Map, Set, Tree};

/// Each prefix of `path` that ends at a `/`, then `path` itself.
pub(crate) fn prefixes(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(|(at, _)| &path[..at]).chain(std::iter::once(path))
}

/// The first segment of `path`.
pub(crate) fn root_of(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// Builds the tree of `written` paths and their ancestors. `make` creates the
/// node for each path. Also returns the id of every path.
pub(crate) fn build<'s, T>(
    written: impl IntoIterator<Item = &'s str>,
    mut make: impl FnMut(&'s str) -> T,
) -> (Tree<T>, Map<&'s str, Id<T>>) {
    let mut seen = Set::default();
    let mut paths: Vec<&str> = written.into_iter().flat_map(prefixes).filter(|&path| seen.insert(path)).collect();
    // Comparing segment by segment keeps `bank` before `bank-x`, which a plain
    // string comparison would put after `bank/checking`.
    paths.sort_unstable_by(|a, b| a.split('/').cmp(b.split('/')));

    let position: Map<&str, usize> = paths.iter().enumerate().map(|(at, &path)| (path, at)).collect();
    let parents: Vec<Option<usize>> =
        paths.iter().map(|path| path.rsplit_once('/').map(|(parent, _)| position[parent])).collect();
    let nodes = paths.iter().map(|&path| make(path)).collect();

    let (tree, ids) = Tree::build(nodes, &parents).expect("a path's parent is a shorter path, so there is no cycle");
    (tree, paths.into_iter().zip(ids).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestors_are_created_and_siblings_sorted() {
        let (tree, ids) = build(["assets/bank/checking", "assets/bank-x", "assets/bank/savings"], |path| path);
        let order: Vec<&str> = tree.values().copied().collect();
        assert_eq!(order, ["assets", "assets/bank", "assets/bank/checking", "assets/bank/savings", "assets/bank-x"]);
        assert_eq!(tree.parent(ids["assets/bank/checking"]), Some(ids["assets/bank"]));
    }
}
