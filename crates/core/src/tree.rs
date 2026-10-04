//! Hierarchies in pre-order.
//!
//! Places, entities, kinds and jurisdictions all form trees. Numbering a tree's
//! nodes in pre-order makes every subtree a contiguous range of ids, so "is `b`
//! under `a`" is two integer comparisons, a subtree walk is a range, and a
//! value per subtree is a sum over a slice.

use std::iter::FusedIterator;
use std::ops::{Index, IndexMut};

use crate::groups::bucket;
use crate::id::{Id, Ids, Run};

const NONE: u32 = u32::MAX;

/// A forest whose ids are assigned in pre-order: the subtree of `id` is exactly
/// `id..tree.end(id)`.
pub struct Tree<T> {
    items: Vec<T>,
    links: Vec<Link>,
}

#[derive(Clone, Copy)]
struct Link {
    parent: u32,
    end: u32,
    depth: u32,
}

/// A tree built from items in any order, and the new id of each input index.
pub type Arranged<T> = (Tree<T>, Vec<Id<T>>);

impl<T> Tree<T> {
    /// Arranges `items` in pre-order. `parents[i]` is the input index of item
    /// `i`'s parent; siblings keep their input order. Returns the tree and the
    /// new id of each input index, or, if parents form a cycle, the input
    /// indices that are on or beneath it.
    pub fn build(items: Vec<T>, parents: &[Option<usize>]) -> Result<Arranged<T>, Vec<usize>> {
        let n = items.len();
        assert_eq!(parents.len(), n, "one parent slot per item");
        // Children grouped by parent; roots sit under a virtual parent `n`.
        // `kids[starts[p]..starts[p + 1]]` are p's children.
        let (starts, kids) = bucket(n + 1, n, |child| parents[child].unwrap_or(n));

        // Depth-first from the virtual root. Each stack frame is a node and the
        // cursor of its next unvisited child.
        let mut order = Vec::with_capacity(n);
        let mut links: Vec<Link> = Vec::with_capacity(n);
        let mut new_id = vec![NONE; n];
        let mut stack = vec![(n, starts[n])];
        while let Some((node, cursor)) = stack.last_mut() {
            let node = *node;
            if *cursor < starts[node + 1] {
                let child = kids[*cursor as usize] as usize;
                *cursor += 1;
                new_id[child] = order.len() as u32;
                let parent = if node == n { NONE } else { new_id[node] };
                links.push(Link { parent, end: NONE, depth: stack.len() as u32 - 1 });
                order.push(child);
                stack.push((child, starts[child]));
            } else {
                stack.pop();
                if node != n {
                    links[new_id[node] as usize].end = order.len() as u32;
                }
            }
        }
        if order.len() < n {
            return Err((0..n).filter(|&i| new_id[i] == NONE).collect());
        }

        let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
        let items = order.iter().map(|&i| slots[i].take().expect("each item placed once")).collect();
        let ids = new_id.into_iter().map(Id::new).collect();
        Ok((Tree { items, links }, ids))
    }

    /// Appends `item` as the last root. In pre-order the last root's subtree is the end of the numbering, so a new one
    /// moves no id, no `end` and no depth: the tree is what [`Tree::build`] would make of the same items with this one last.
    pub fn push_root(&mut self, item: T) -> Id<T> {
        let id = self.items.len() as u32;
        self.items.push(item);
        self.links.push(Link { parent: NONE, end: id + 1, depth: 0 });
        Id::new(id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.items.get(id.index())
    }

    pub fn parent(&self, id: Id<T>) -> Option<Id<T>> {
        let parent = self.links[id.index()].parent;
        (parent != NONE).then(|| Id::new(parent))
    }

    /// `id`'s parent to read and `id` to change, at once: pre-order puts a
    /// parent before its children, so the two never overlap. `None` for a root.
    pub fn with_parent_mut(&mut self, id: Id<T>) -> Option<(&T, &mut T)> {
        let parent = self.parent(id)?;
        let (before, from) = self.items.split_at_mut(id.index());
        Some((&before[parent.index()], &mut from[0]))
    }

    /// Roots have depth zero.
    pub fn depth(&self, id: Id<T>) -> u32 {
        self.links[id.index()].depth
    }

    /// One past the last id in `id`'s subtree.
    pub fn end(&self, id: Id<T>) -> Id<T> {
        Id::new(self.links[id.index()].end)
    }

    /// Whether `id` is `ancestor` or lies beneath it.
    pub fn covers(&self, ancestor: Id<T>, id: Id<T>) -> bool {
        ancestor <= id && id < self.end(ancestor)
    }

    /// `id` and everything beneath it, in pre-order.
    pub fn subtree(&self, id: Id<T>) -> Ids<T> {
        Run::new(id, self.links[id.index()].end - id.index() as u32).ids()
    }

    /// `id`, then its parent, and so on up to its root.
    pub fn lineage(&self, id: Id<T>) -> Lineage<'_, T> {
        Lineage { tree: self, next: Some(id) }
    }

    /// The direct children of `id`, in order.
    pub fn children(&self, id: Id<T>) -> Children<'_, T> {
        Children { tree: self, next: id.index() as u32 + 1, end: self.links[id.index()].end }
    }

    /// The top-level nodes, in order.
    pub fn roots(&self) -> Children<'_, T> {
        Children { tree: self, next: 0, end: self.items.len() as u32 }
    }

    pub fn ids(&self) -> Ids<T> {
        Run::new(Id::new(0), self.items.len() as u32).ids()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Id<T>, &T)> {
        self.items.iter().enumerate().map(|(i, item)| (Id::new(i as u32), item))
    }

    pub fn values(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

impl<T> Default for Tree<T> {
    fn default() -> Tree<T> {
        Tree { items: Vec::new(), links: Vec::new() }
    }
}

impl<T> Index<Id<T>> for Tree<T> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &T {
        &self.items[id.index()]
    }
}

impl<T> IndexMut<Id<T>> for Tree<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut T {
        &mut self.items[id.index()]
    }
}

pub struct Lineage<'t, T> {
    tree: &'t Tree<T>,
    next: Option<Id<T>>,
}

impl<T> Clone for Lineage<'_, T> {
    fn clone(&self) -> Self {
        Self { tree: self.tree, next: self.next }
    }
}

impl<T> Iterator for Lineage<'_, T> {
    type Item = Id<T>;
    fn next(&mut self) -> Option<Id<T>> {
        let id = self.next?;
        self.next = self.tree.parent(id);
        Some(id)
    }
}

impl<T> FusedIterator for Lineage<'_, T> {}

/// Siblings in order: each child's subtree ends where the next child begins.
pub struct Children<'t, T> {
    tree: &'t Tree<T>,
    next: u32,
    end: u32,
}

impl<T> Iterator for Children<'_, T> {
    type Item = Id<T>;
    fn next(&mut self) -> Option<Id<T>> {
        (self.next < self.end).then(|| {
            let id = Id::new(self.next);
            self.next = self.tree.links[id.index()].end;
            id
        })
    }
}

impl<T> FusedIterator for Children<'_, T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preorder_makes_subtrees_ranges() {
        // Input order is scrambled; parents by input index.
        let items = vec!["assets/bank", "expenses", "assets", "assets/bank/checking", "assets/broker", "expenses/food"];
        let parents = [Some(2), None, None, Some(0), Some(2), Some(1)];
        let (tree, ids) = Tree::build(items, &parents).unwrap();
        let names: Vec<_> = tree.values().copied().collect();
        assert_eq!(
            names,
            ["expenses", "expenses/food", "assets", "assets/bank", "assets/bank/checking", "assets/broker"]
        );
        let assets = ids[2];
        assert_eq!(tree.subtree(assets).count(), 4);
        assert!(tree.covers(assets, ids[3]) && !tree.covers(ids[0], ids[4]));
        assert_eq!(
            tree.lineage(ids[3]).map(|id| tree[id]).collect::<Vec<_>>(),
            ["assets/bank/checking", "assets/bank", "assets"]
        );
        assert_eq!(tree.children(assets).map(|id| tree[id]).collect::<Vec<_>>(), ["assets/bank", "assets/broker"]);
        assert_eq!(tree.roots().count(), 2);
        assert_eq!(tree.depth(ids[3]), 2);
    }

    #[test]
    fn a_child_changes_beside_its_parent() {
        let (mut tree, ids) = Tree::build(vec![1, 10, 100], &[None, Some(0), Some(1)]).unwrap();
        let (parent, child) = tree.with_parent_mut(ids[2]).unwrap();
        *child += *parent;
        assert_eq!(tree.as_slice(), [1, 10, 110]);
        assert!(tree.with_parent_mut(ids[0]).is_none());
    }

    /// What a tree says of each id besides the item: its parent, where its subtree ends and how deep it is.
    fn links<T>(tree: &Tree<T>) -> Vec<(Option<Id<T>>, Id<T>, u32)> {
        tree.ids().map(|id| (tree.parent(id), tree.end(id), tree.depth(id))).collect()
    }

    #[test]
    fn a_pushed_root_is_the_tree_built_with_it_last() {
        let items = vec!["assets", "assets/bank", "expenses"];
        let parents = [None, Some(0), None];
        let (mut pushed, ids) = Tree::build(items.clone(), &parents).unwrap();
        let before = links(&pushed);

        let tab = pushed.push_root("dana");

        let (built, built_ids) = Tree::build([items, vec!["dana"]].concat(), &[None, Some(0), None, None]).unwrap();
        assert_eq!(tab, built_ids[3], "a root is made last");
        assert_eq!(pushed.as_slice(), built.as_slice());
        assert_eq!(links(&pushed), links(&built));
        assert_eq!(links(&pushed)[..3], before[..], "no id, end or depth of what was there moved");
        assert_eq!(pushed.subtree(tab).count(), 1);
        assert!(pushed.covers(tab, tab) && !pushed.covers(ids[0], tab));
        assert_eq!(pushed.roots().collect::<Vec<_>>(), [ids[0], ids[2], tab]);
        assert_eq!(pushed.lineage(tab).collect::<Vec<_>>(), [tab]);
    }

    #[test]
    fn a_root_can_be_pushed_onto_nothing() {
        let mut tree = Tree::default();
        let first = tree.push_root('a');
        let second = tree.push_root('b');
        assert_eq!((first.index(), second.index()), (0, 1));
        assert_eq!(tree.roots().count(), 2);
        assert_eq!(tree.end(first), second);
    }

    #[test]
    fn cycles_are_reported() {
        let parents = [Some(1), Some(0), None, Some(0)];
        let err = Tree::build(vec!['a', 'b', 'c', 'd'], &parents).err().unwrap();
        assert_eq!(err, [0, 1, 3]);
    }
}
