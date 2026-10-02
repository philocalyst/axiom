//! Trees of names written `NAME : PARENT`: the kinds and the purposes.
//!
//! A kind names its parent, which may be written later, in another file, or in a system that only some files use.
//! So declarations are first drafts: the roots, then one draft for each declaration that is neither a root again nor
//! an earlier one again. Parents are found among all the drafts, in the scope of the file that wrote the child. A
//! chain of parents that never reaches a root is said once and cut by hanging its members from the tree's orphan
//! root. Only then is the tree made, once, in pre-order, so that a subtree is a range of ids and "is this a 401(k)" is
//! an interval test.
//!
//! One builder serves every such tree. What differs between them is data and a few words, which a [`Node`] gives.

use axiom_core::{Diagnostic, Id, Interner, Loc, Map, Sym, Tree};
use axiom_syntax::{Decl, DeclKind};

use crate::book::{Miss, System};
use crate::collect::{Collected, Written};
use crate::errors::Word;
use crate::names::Scoped;
use crate::problem::{self, Among, Noun};
use crate::scope::{Home, Scope, Seeing};

/// Whether a declaration that names a root again is an error: a kind is built in, and a purpose may be written
/// again to add laws to its root.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repeated {
    Said,
    Allowed,
}

/// A thing in a tree of names.
pub(crate) trait Node: Sized {
    const NOUN: Noun;
    /// The declarations that make the tree.
    const DECL: DeclKind;
    /// The built-in roots: each one's word, and the root it hangs from, if it hangs from one.
    const ROOTS: &'static [(&'static str, Option<usize>)];
    /// The root that a node hangs from when its parent is missing, unknown or on a cycle.
    const ORPHAN: usize;
    const REPEATED_ROOT: Repeated;
    /// What a declaration with no parent is asked.
    const PARENT_QUESTION: &'static str;

    /// The root numbered `at` in [`Node::ROOTS`].
    fn root(name: Sym, at: usize) -> Self;
    /// A node as written: not yet in the tree, so what it takes from its parent is for [`Node::adopt`].
    fn declared<'s>(at: &Written<'_, 's, Decl<'s>>, names: &mut Interner<'s>) -> Self;
    /// What a node takes from its parent once the tree is made, parents first.
    fn adopt(&mut self, parent: &Self);
    fn name(&self) -> Sym;
    fn loc(&self) -> Option<Loc>;

    /// The one node `text` names, among those `scope` can see.
    fn resolve(
        index: &Scoped<Self>,
        names: &Interner,
        _systems: &Tree<System>,
        text: &str,
        scope: &Scope,
    ) -> Result<Id<Self>, Miss<Self>> {
        index.resolve(names, scope, text)
    }

    /// Why `word` named no single node, with the closest as the fix. `nodes` are the drafts, by id.
    fn unresolved(miss: Miss<Self>, word: Word, among: &Among<Self>, nodes: &[Self]) -> Diagnostic;
}

/// A tree of nodes, and what answers to their names.
pub(crate) struct Taxonomy<T> {
    pub tree: Tree<T>,
    pub index: Scoped<T>,
    /// The built-in roots, in the order of [`Node::ROOTS`].
    pub roots: Box<[Id<T>]>,
    /// The node each declaration stands for, in the order written: a root or an earlier node for a repeat.
    pub declarations: Vec<Id<T>>,
}

/// The tree of every declaration of a kind of node.
pub(crate) fn declare<'s, T: Node>(
    collected: &Collected<'_, 's>,
    names: &mut Interner<'s>,
    seeing: Seeing<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Taxonomy<T> {
    let mut drafts = Drafts::of(collected, names, diags);
    let index = drafts.index(names);
    drafts.link_parents(&index, names, seeing, diags);
    drafts.cut_cycles(names, diags);
    drafts.freeze(names)
}

/// One written declaration and the draft it stands for: its own, the first of its name, or a root.
struct Declared<'a, 's> {
    written: Written<'a, 's, Decl<'s>>,
    draft: usize,
}

/// The nodes as written, before they are a tree.
struct Drafts<'a, 's, T> {
    nodes: Vec<T>,
    homes: Vec<Home>,
    parents: Vec<Option<usize>>,
    declared: Vec<Declared<'a, 's>>,
}

impl<'a, 's, T: Node> Drafts<'a, 's, T> {
    /// The roots, then one draft for each declaration.
    fn of(collected: &Collected<'a, 's>, names: &mut Interner<'s>, diags: &mut Vec<Diagnostic>) -> Self {
        let roots = T::ROOTS.iter().enumerate();
        let mut drafts = Drafts {
            nodes: roots.map(|(at, &(word, _))| T::root(names.intern(word), at)).collect(),
            homes: vec![Home::Builtin; T::ROOTS.len()],
            parents: T::ROOTS.iter().map(|&(_, parent)| parent).collect(),
            declared: Vec::new(),
        };
        let mut seen: Map<(Home, &'s str), usize> = Map::default();
        for written in collected.decls_of(T::DECL) {
            let draft = drafts.declare(written, &mut seen, names, diags);
            drafts.declared.push(Declared { written: *written, draft });
        }
        drafts
    }

    /// The draft a declaration is: a new one, or the root or earlier one it repeats (said, unless a root may be).
    fn declare(
        &mut self,
        written: &Written<'a, 's, Decl<'s>>,
        seen: &mut Map<(Home, &'s str), usize>,
        names: &mut Interner<'s>,
        diags: &mut Vec<Diagnostic>,
    ) -> usize {
        let (file, text, home) = (written.file(), written.node.name.0, written.home());
        if let Some(root) = T::ROOTS.iter().position(|&(word, _)| word == text) {
            if T::REPEATED_ROOT == Repeated::Said {
                diags.push(problem::duplicate(T::NOUN, Word::of(file, text), None));
            }
            return root;
        }
        if let Some(&first) = seen.get(&(home, text)) {
            diags.push(problem::duplicate(T::NOUN, Word::of(file, text), self.nodes[first].loc()));
            return first;
        }
        let at = self.nodes.len();
        seen.insert((home, text), at);
        self.nodes.push(T::declared(written, names));
        self.homes.push(home);
        self.parents.push(None);
        at
    }

    /// What answers to each draft's name, by the ids the drafts will have if their order is kept.
    fn index(&self, names: &mut Interner<'s>) -> Scoped<T> {
        let spelled: Vec<_> = self.nodes.iter().map(|node| names.name(node.name())).collect();
        Scoped::build(
            names,
            spelled
                .into_iter()
                .zip(&self.homes)
                .enumerate()
                .map(|(at, (text, &home))| (Id::new(at as u32), text, home)),
        )
    }

    /// Each declaration's parent, found among all the drafts. One that names none, or none there, hangs from the
    /// orphan root.
    fn link_parents(
        &mut self,
        index: &Scoped<T>,
        names: &Interner<'s>,
        seeing: Seeing<'_>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Drafts { nodes, parents, declared, .. } = self;
        let roots: Vec<&str> = T::ROOTS.iter().map(|&(word, _)| word).collect();
        for Declared { written, draft } in declared.iter() {
            let (file, decl) = (written.file(), written.node);
            if *draft < roots.len() || parents[*draft].is_some() {
                continue;
            }
            parents[*draft] = Some(match decl.kind {
                None => {
                    diags.push(problem::parentless(T::NOUN, Word::of(file, decl.name.0), T::PARENT_QUESTION, &roots));
                    T::ORPHAN
                }
                Some(parent) => {
                    let scope = seeing.scopes.of(written.home());
                    match T::resolve(index, names, seeing.systems, parent.0, scope) {
                        Ok(found) => found.index(),
                        Err(miss) => {
                            let among = Among { index, names, systems: seeing.systems };
                            diags.push(T::unresolved(miss, Word::of(file, parent.0), &among, nodes));
                            T::ORPHAN
                        }
                    }
                }
            });
        }
    }

    /// A parent chain that never reaches a root is said, and cut by hanging each of its members from the orphan root.
    fn cut_cycles(&mut self, names: &Interner<'s>, diags: &mut Vec<Diagnostic>) {
        for cycle in cycles(&self.parents) {
            let route: Vec<_> =
                cycle.iter().map(|&at| (names.name(self.nodes[at].name()), self.nodes[at].loc())).collect();
            diags.push(problem::cycle(T::NOUN, &route, T::ROOTS[0].0));
            for &member in &cycle {
                self.parents[member] = Some(T::ORPHAN);
            }
        }
    }

    /// The tree in its final order, every node having taken what its parent gives, and what answers to the names.
    fn freeze(self, names: &mut Interner<'s>) -> Taxonomy<T> {
        let Drafts { nodes, homes, parents, declared } = self;
        let (mut tree, remap) = Tree::build(nodes, &parents).expect("cycles were cut before freezing");
        for id in tree.ids() {
            if let Some((above, child)) = tree.with_parent_mut(id) {
                child.adopt(above);
            }
        }
        let mut final_homes = vec![Home::Builtin; homes.len()];
        for (old, &home) in homes.iter().enumerate() {
            final_homes[remap[old].index()] = home;
        }
        let spelled: Vec<_> =
            tree.iter().map(|(id, node)| (id, names.name(node.name()), final_homes[id.index()])).collect();
        Taxonomy {
            index: Scoped::build(names, spelled),
            roots: remap[..T::ROOTS.len()].into(),
            declarations: declared.into_iter().map(|declared| remap[declared.draft]).collect(),
            tree,
        }
    }
}

/// Each cycle of parents, as the nodes on it in the order each inherits from the next. Nodes that only hang beneath
/// a cycle are not on it.
fn cycles(parents: &[Option<usize>]) -> Vec<Vec<usize>> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unseen,
        OnPath,
        Done,
    }
    let mut state = vec![State::Unseen; parents.len()];
    let mut found = Vec::new();
    for start in 0..parents.len() {
        let (mut path, mut at) = (Vec::new(), Some(start));
        while let Some(node) = at {
            match state[node] {
                State::Unseen => {
                    state[node] = State::OnPath;
                    path.push(node);
                    at = parents[node];
                }
                State::OnPath => {
                    let from = path.iter().position(|&member| member == node).expect("on the path being walked");
                    found.push(path[from..].to_vec());
                    break;
                }
                State::Done => break,
            }
        }
        path.iter().for_each(|&node| state[node] = State::Done);
    }
    found
}

#[cfg(test)]
mod tests {
    use axiom_core::FileId;
    use axiom_syntax::{Folder, parse};

    use super::*;
    use crate::Source;
    use crate::book::{Class, Kind, Purpose, PurposeRoot, Sort};
    use crate::{kinds, purposes, sources};

    fn build<'s, T: Node>(texts: &[(&'s str, &'s str, bool)]) -> (Taxonomy<T>, Vec<Diagnostic>) {
        let sources: Vec<_> = texts
            .iter()
            .enumerate()
            .map(|(at, &(path, text, embedded))| Source {
                path,
                file: parse(FileId(at as u16), text, Folder::default()).0,
                embedded,
            })
            .collect();
        let mut diags = Vec::new();
        let mut names = Interner::default();
        let (sites, systems, index) = sources::arrange(&sources, &mut names, &mut diags);
        let collected = Collected::of(&sites);
        let scopes = crate::declare::scopes(&collected, &index, &systems, &mut diags);
        let seeing = Seeing { systems: &systems, scopes: &scopes };
        let taxonomy = declare::<T>(&collected, &mut names, seeing, &mut diags);
        (taxonomy, diags)
    }

    #[test]
    fn kinds_resolve_forward_and_scoped_parents_before_the_tree_is_made() {
        let (kinds, diags) = build::<Kind>(&[
            ("std.ax", "system std\nkind employer : entity\nkind manager : employer\n", true),
            ("axiom.ax", "use std\nkind worker : manager\n", false),
        ]);
        assert!(diags.is_empty(), "{diags:?}");
        let [employer, manager, worker] = kinds.declarations[..] else { panic!("three declarations") };
        assert!(kinds.tree.covers(employer, manager));
        assert!(kinds.tree.covers(manager, worker));
        let roots = kinds::roots(&kinds.roots);
        assert_eq!(kinds.tree[roots.entity].sort, Sort::Entity, "a kind is of its parent's sort");
        assert_eq!(kinds.tree[worker].sort, Sort::Entity);
        assert_eq!(kinds.tree[roots.asset].sort, Sort::Place(Class::Asset));
        assert_eq!(kinds.tree[roots.debt].sort, Sort::Place(Class::Debt));
        assert_eq!(kinds.tree.parent(roots.measure), Some(roots.commodity), "a built-in root may have a parent");
        assert_eq!(kinds.tree.depth(worker), 3);
    }

    #[test]
    fn a_cycle_is_said_once_and_cut_by_hanging_its_members_from_the_orphan_root() {
        let (kinds, diags) = build::<Kind>(&[("std.ax", "system std\nkind alpha : beta\nkind beta : alpha\n", true)]);
        let cycles: Vec<_> = diags.iter().filter(|diag| diag.code == "kind-cycle").collect();
        assert_eq!(cycles.len(), 1, "{diags:?}");
        assert_eq!(cycles[0].message, "kind `alpha` inherits from itself");
        let thing = kinds::roots(&kinds.roots).thing;
        assert!(kinds.declarations.iter().all(|&id| kinds.tree.parent(id) == Some(thing)));
    }

    #[test]
    fn a_parent_that_is_missing_or_unknown_is_said_and_the_node_hangs_from_the_orphan_root() {
        let (kinds, diags) =
            build::<Kind>(&[("std.ax", "system std\nkind lonely\nkind lost : entty\nkind found : entity\n", true)]);
        let codes: Vec<_> = diags.iter().map(|diag| &*diag.code).collect();
        assert_eq!(codes, ["kind-parent", "unknown-kind"], "{diags:?}");
        assert_eq!(diags[1].help[0].text, "did you mean `entity`?");
        let roots = kinds::roots(&kinds.roots);
        let [lonely, lost, found] = kinds.declarations[..] else { panic!("three declarations") };
        assert_eq!([lonely, lost].map(|id| kinds.tree.parent(id)), [Some(roots.thing); 2]);
        assert_eq!(kinds.tree.parent(found), Some(roots.entity));
    }

    #[test]
    fn a_kind_that_names_a_root_again_is_told_it_is_built_in_and_a_purpose_may() {
        let (kinds, diags) = build::<Kind>(&[("std.ax", "system std\nkind asset : entity\nkind asset : debt\n", true)]);
        assert_eq!(diags.iter().map(|diag| &*diag.message).collect::<Vec<_>>(), ["kind `asset` is built in"; 2]);
        assert_eq!(kinds.declarations, [kinds.roots[0]; 2], "a repeat stands for the root");

        let (purposes, diags) = build::<Purpose>(&[("std.ax", "system std\npurpose income : transfer\n", true)]);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(purposes.declarations, [purposes.roots[0]]);
    }

    #[test]
    fn a_kind_declared_twice_in_one_home_is_one_node_and_a_second_home_has_its_own() {
        let (kinds, diags) = build::<Kind>(&[
            ("std.ax", "system std\nkind pet : entity\nkind pet : entity\n", true),
            ("axiom.ax", "kind pet : entity\n", false),
        ]);
        assert_eq!(diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(), ["duplicate-kind"]);
        let [first, again, project] = kinds.declarations[..] else { panic!("three declarations") };
        assert_eq!(first, again);
        assert_ne!(first, project);
    }

    #[test]
    fn purposes_take_their_root_from_the_parent_and_hang_from_transfer_when_orphaned() {
        let (purposes, diags) = build::<Purpose>(&[(
            "std.ax",
            "system std\npurpose groceries : food\npurpose food : spending\npurpose stray : nowhere\n",
            true,
        )]);
        assert_eq!(diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(), ["unknown-purpose"]);
        let [groceries, food, stray] = purposes.declarations[..] else { panic!("three declarations") };
        assert_eq!(purposes.tree[groceries].root, PurposeRoot::Spending);
        assert_eq!(purposes.tree.parent(groceries), Some(food));
        assert_eq!(purposes.tree[stray].root, PurposeRoot::Transfer);
        assert_eq!(purposes.tree.parent(stray), Some(purposes::roots(&purposes.roots).transfer));
    }

    #[test]
    fn a_purpose_cycle_is_said_in_the_same_words_as_a_kind_cycle() {
        let (_, diags) = build::<Purpose>(&[("std.ax", "system std\npurpose a : b\npurpose b : a\n", true)]);
        let cycle = diags.iter().find(|diag| diag.code == "purpose-cycle").expect("a cycle");
        assert_eq!(cycle.notes[0], "the chain is a -> b -> a");
        assert_eq!(
            cycle.help[0].text,
            "give one of them a parent outside the loop, such as a root purpose like `income`"
        );
        assert_eq!(cycle.labels.len(), 2, "the first member, and where the second inherits");
    }
}
