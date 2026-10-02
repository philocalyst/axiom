//! Kinds: what things are.
//!
//! Kinds form typed trees rooted in the built-in place, asset, commodity,
//! measure and entity kinds. This stage creates the trees and resolves parents.

use axiom_core::{Diagnostic, Id, Interner, Map, Tree};
use axiom_syntax::DeclKind;

use crate::book::{Class, Kind, Miss, Sort, System};
use crate::collect::Collected;
use crate::errors::{Candidate, Word};
use crate::names::Scoped;
use crate::problem::{self, Among, Noun};
use crate::scope::{Home, Scopes};

/// The kind tree built from the promoted S5 declaration nodes. `declarations`
/// is aligned to the source-order `kind` declarations, including duplicates,
/// so later diagnostics can still point at each written item.
pub(crate) struct NativeKinds {
    pub tree: Tree<Kind>,
    pub index: Scoped<Kind>,
    pub roots: crate::book::KindRoots,
    pub declarations: Vec<Id<Kind>>,
    pub unrooted: Vec<bool>,
}

fn draft<'s>(names: &mut Interner<'s>, name: &'s str, sort: Sort) -> Kind {
    Kind {
        name: names.intern(name),
        sort,
        system: None,
        restricted: false,
        deferred: false,
        basis: None,
        claim: false,
        select: None,
        liquidity: None,
        purpose: None,
        pays: None,
        takes: Box::default(),
        sales_tax: None,
        shares: Box::default(),
        has: Box::default(),
        props: Box::default(),
        laws: Box::default(),
        doc: None,
        loc: None,
    }
}

/// Builds the typed kind hierarchy directly from borrowed S5 items. Draft
/// indices let forward parents resolve before the tree is frozen; `Tree::build`
/// then assigns the final pre-order ids once.
pub(crate) fn declare_sites<'s>(
    collected: &Collected<'_, 's>,
    names: &mut Interner<'s>,
    systems: &Tree<System>,
    scopes: &Scopes,
    diags: &mut Vec<Diagnostic>,
) -> NativeKinds {
    const ROOTS: [(&str, Sort); 6] = [
        ("asset", Sort::Place(Class::Asset)),
        ("debt", Sort::Place(Class::Debt)),
        ("thing", Sort::Thing),
        ("commodity", Sort::Commodity),
        ("measure", Sort::Commodity),
        ("entity", Sort::Entity),
    ];
    const ROOT_ASSET: usize = 0;
    const ROOT_DEBT: usize = 1;
    const ROOT_THING: usize = 2;
    const ROOT_COMMODITY: usize = 3;
    const ROOT_MEASURE: usize = 4;
    const ROOT_ENTITY: usize = 5;

    let mut drafts: Vec<Kind> = ROOTS.iter().map(|&(name, sort)| draft(names, name, sort)).collect();
    let mut homes = vec![Home::Builtin; ROOTS.len()];
    let mut parents = vec![None; ROOTS.len()];
    parents[ROOT_MEASURE] = Some(ROOT_COMMODITY);
    let mut broken = vec![false; ROOTS.len()];
    let mut duplicate_of: Map<(Home, &'s str), usize> = Map::default();
    for (at, &(name, _)) in ROOTS.iter().enumerate() {
        duplicate_of.insert((Home::Builtin, name), at);
    }
    let mut written: Vec<(&axiom_syntax::File<'s>, &axiom_syntax::Decl<'s>, Home)> = Vec::new();
    let mut draft_of = Vec::new();

    for kind_decl in collected.decls_of(DeclKind::Kind) {
        let (file, decl, home) = (kind_decl.file(), kind_decl.node, kind_decl.home());
        let name = decl.name.0;
        written.push((file, decl, home));
        if let Some(&first) = duplicate_of.get(&(home, name)) {
            let (word, earlier) = (Word::of(file, name), drafts[first].loc);
            diags.push(problem::duplicate(Noun::Kind, word, earlier));
            draft_of.push(first);
            continue;
        }
        if let Some(first) = ROOTS.iter().position(|&(root, _)| root == name) {
            diags.push(problem::duplicate(Noun::Kind, Word::of(file, name), None));
            duplicate_of.insert((home, name), first);
            draft_of.push(first);
            continue;
        }

        let mut kind = draft(names, name, Sort::Thing);
        kind.system = match home {
            Home::System(system) => Some(system),
            Home::Project | Home::Builtin => None,
        };
        kind.doc = kind_decl.item.doc.map(|doc| names.intern(doc.0));
        kind.loc = Some(file.loc(name));
        let at = drafts.len();
        duplicate_of.insert((home, name), at);
        drafts.push(kind);
        homes.push(home);
        parents.push(None);
        broken.push(false);
        draft_of.push(at);
    }

    let draft_names: Vec<_> =
        drafts.iter().enumerate().map(|(at, kind)| (Id::new(at as u32), names.name(kind.name), homes[at])).collect();
    let draft_index = Scoped::build(names, draft_names);
    for (at, &(file, decl, home)) in written.iter().enumerate() {
        let child = draft_of[at];
        if child < ROOTS.len() || parents[child].is_some() {
            continue;
        }
        let Some(parent) = decl.kind else {
            diags.push(
                Diagnostic::error("kind-parent", format!("kind `{}` needs a parent", decl.name.0))
                    .label(file.loc(decl.name.0), "what kind of thing is this?")
                    .help(
                        "write `: asset`, `: debt`, `: thing`, `: commodity`, `: measure`, `: entity`, or another kind",
                    ),
            );
            parents[child] = Some(ROOT_THING);
            broken[child] = true;
            continue;
        };
        match find(&draft_index, names, systems, parent.0, |visible| scopes.of(home).sees(visible)) {
            Ok(parent) => parents[child] = Some(parent.index()),
            Err(miss) => {
                let among = Among { index: &draft_index, names, systems };
                diags.push(unresolved(miss, Word::of(file, parent.0), &among, |id| &drafts[id.index()]));
                parents[child] = Some(ROOT_THING);
                broken[child] = true;
            }
        }
    }

    for cycle in cycles(&parents) {
        diags.push(cycle_diagnostic(&cycle, &drafts, names));
        for &member in &cycle {
            parents[member] = Some(ROOT_THING);
            broken[member] = true;
        }
    }
    let (mut tree, remap) = Tree::build(drafts, &parents).expect("kind cycles were cut before freezing");
    for id in tree.ids() {
        if let Some(parent) = tree.parent(id) {
            tree[id].sort = tree[parent].sort;
        }
    }

    let mut final_homes = vec![Home::Builtin; homes.len()];
    let mut unrooted = vec![false; homes.len()];
    for (old, &home) in homes.iter().enumerate() {
        final_homes[remap[old].index()] = home;
        unrooted[remap[old].index()] = broken[old];
    }
    for id in tree.ids() {
        if let Some(parent) = tree.parent(id) {
            unrooted[id.index()] |= unrooted[parent.index()];
        }
    }
    let visible_names: Vec<_> =
        tree.iter().map(|(id, kind)| (id, names.name(kind.name), final_homes[id.index()])).collect();
    let index = Scoped::build(names, visible_names);
    let declarations = draft_of.into_iter().map(|draft| remap[draft]).collect();
    NativeKinds {
        tree,
        index,
        roots: crate::book::KindRoots {
            asset: remap[ROOT_ASSET],
            debt: remap[ROOT_DEBT],
            thing: remap[ROOT_THING],
            commodity: remap[ROOT_COMMODITY],
            measure: remap[ROOT_MEASURE],
            entity: remap[ROOT_ENTITY],
        },
        declarations,
        unrooted,
    }
}

/// A kind by name (`401k`), qualified by its system (`us/401k/401k`), or by a
/// system named for it (`us/401k`). Only bare names depend on what `seen`
/// admits: a qualified name always resolves.
pub(crate) fn find(
    kinds: &Scoped<Kind>,
    names: &Interner,
    systems: &Tree<System>,
    text: &str,
    seen: impl Fn(Home) -> bool,
) -> Result<Id<Kind>, Miss<Kind>> {
    let Some((head, name)) = text.rsplit_once('/') else {
        return kinds.names.resolve(names, text, |id| seen(kinds.home(id)));
    };
    let declared_by = |id: Id<Kind>| match kinds.home(id) {
        Home::System(system) => {
            let path = names.name(systems[system].path);
            path == head || path == text
        }
        Home::Project | Home::Builtin => false,
    };
    kinds.names.resolve(names, name, declared_by)
}

/// Why `word` named no single kind. A kind of a system is written `system/kind`, which is how it is offered.
pub(crate) fn unresolved<'k>(
    miss: Miss<Kind>,
    word: Word,
    among: &Among<Kind>,
    kind_of: impl Fn(Id<Kind>) -> &'k Kind,
) -> Diagnostic {
    let describe = |&id: &Id<Kind>| {
        let (declared, name) = (kind_of(id).loc, among.names.name(kind_of(id).name));
        match (among.index.home(id), among.system_of(id)) {
            (Home::System(_), Some(system)) => {
                let last = system.rsplit('/').next().unwrap_or(system);
                // `us/401k` names the kind `401k` of that system, and is the shorter way to say it.
                let write = if last == word.text { system.to_string() } else { format!("{system}/{}", word.text) };
                Candidate { is: format!("`{}` from `{system}`", word.text), declared, write: Some(write) }
            }
            (Home::Builtin, _) => Candidate { is: format!("the built-in `{name}`"), declared, write: None },
            _ => Candidate { is: format!("the project's `{name}`"), declared, write: None },
        }
    };
    among.failed(miss, Noun::Kind, word, |ids| ids.iter().map(describe).collect())
}

/// Each cycle of parents, as the kinds on it in the order each inherits from
/// the next. Kinds that only hang beneath a cycle are not on it.
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
        while let Some(kind) = at {
            match state[kind] {
                State::Unseen => {
                    state[kind] = State::OnPath;
                    path.push(kind);
                    at = parents[kind];
                }
                State::OnPath => {
                    let from = path.iter().position(|&member| member == kind).expect("on the path being walked");
                    found.push(path[from..].to_vec());
                    break;
                }
                State::Done => break,
            }
        }
        path.iter().for_each(|&kind| state[kind] = State::Done);
    }
    found
}

fn cycle_diagnostic(members: &[usize], drafts: &[Kind], names: &Interner) -> Diagnostic {
    let name = |kind: usize| names.name(drafts[kind].name);
    let route: Vec<&str> = members.iter().chain(&members[..1]).map(|&kind| name(kind)).collect();
    let mut diagnostic = Diagnostic::error("kind-cycle", format!("kind `{}` inherits from itself", route[0]))
        .note(format!("the chain is {}", route.join(" -> ")))
        .help("give one of them a parent outside the loop, such as a root kind like `asset`");
    if let Some(loc) = drafts[members[0]].loc {
        diagnostic = diagnostic.label(loc, "its parent chain never reaches a root");
    }
    for (&child, parent) in members.iter().zip(&route[1..]).skip(1) {
        if let Some(loc) = drafts[child].loc {
            diagnostic = diagnostic.context(loc, format!("`{}` inherits from `{parent}` here", name(child)));
        }
    }
    diagnostic
}

#[cfg(test)]
mod native_tests {
    use axiom_core::{FileId, Interner};
    use axiom_syntax::{Folder, parse};

    use super::*;
    use crate::Source;
    use crate::sources;

    fn build_kinds<'s>(texts: &[(&'s str, &'s str, bool)]) -> (NativeKinds, Vec<Diagnostic>) {
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
        let (sites, systems_tree, systems) = sources::arrange(&sources, &mut names, &mut diags);
        let collected = Collected::of(&sites);
        let scopes = crate::declare::scopes(&collected, &systems, &systems_tree, &mut diags);
        let kinds = declare_sites(&collected, &mut names, &systems_tree, &scopes, &mut diags);
        (kinds, diags)
    }

    #[test]
    fn native_kinds_resolve_forward_and_scoped_parents_before_freezing() {
        let (kinds, diags) = build_kinds(&[
            ("std.ax", "system std\nkind employer : entity\nkind manager : employer\n", true),
            ("axiom.ax", "use std\nkind worker : manager\n", false),
        ]);
        assert!(diags.is_empty(), "{diags:?}");
        let employer = kinds.declarations[0];
        let manager = kinds.declarations[1];
        let worker = kinds.declarations[2];
        assert!(kinds.tree.covers(employer, manager));
        assert!(kinds.tree.covers(manager, worker));
        assert_eq!(kinds.tree[kinds.roots.entity].sort, Sort::Entity);
        assert_eq!(kinds.tree[kinds.roots.asset].sort, Sort::Place(Class::Asset));
        assert_eq!(kinds.tree[kinds.roots.debt].sort, Sort::Place(Class::Debt));
        assert_eq!(kinds.tree.depth(worker), 3);
    }

    #[test]
    fn native_kind_cycles_are_diagnosed_and_cut_before_tree_build() {
        let (kinds, diags) = build_kinds(&[("std.ax", "system std\nkind alpha : beta\nkind beta : alpha\n", true)]);
        assert!(diags.iter().any(|diag| diag.code == "kind-cycle"));
        assert!(kinds.declarations.iter().all(|&id| kinds.unrooted[id.index()]));
        assert!(kinds.tree.ids().all(|id| kinds.tree.parent(id).is_none() || kinds.tree.parent(id).unwrap() < id));
    }
}
