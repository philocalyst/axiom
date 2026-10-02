//! Native v4 purpose declarations.
//!
//! The four roots and every written purpose are assembled from borrowed S5
//! declarations before one immutable tree is built. Forward parents work
//! because resolution happens against the complete draft index.

use axiom_core::{Diagnostic, Id, Interner, Map, Tree};
use axiom_syntax::{DeclKind, ExprKind};

use crate::book::{At, Kind, Purpose, PurposeRoot, System};
use crate::collect::Collected;
use crate::errors::Word;
use crate::kinds;
use crate::names::Scoped;
use crate::problem::{Noun, Problem, Reads, unresolved};
use crate::scope::{Home, Scopes};

pub(crate) struct NativePurposes {
    pub tree: Tree<Purpose>,
    pub index: Scoped<Purpose>,
    pub roots: crate::book::PurposeRoots,
    pub declarations: Vec<Id<Purpose>>,
}

pub(crate) fn declare_sites<'s>(
    collected: &Collected<'_, 's>,
    names: &mut Interner<'s>,
    systems: &Tree<System>,
    scopes: &Scopes,
    kind_index: &Scoped<Kind>,
    diags: &mut Vec<Diagnostic>,
) -> NativePurposes {
    let root_names = ["income", "spending", "capital", "transfer"];
    let root_kinds = [PurposeRoot::Income, PurposeRoot::Spending, PurposeRoot::Capital, PurposeRoot::Transfer];
    let mut drafts: Vec<Purpose> = root_names
        .iter()
        .zip(root_kinds)
        .map(|(&name, root)| Purpose {
            name: names.intern(name),
            root,
            system: None,
            of: None,
            shares: Box::default(),
            laws: Box::default(),
            doc: None,
            loc: None,
        })
        .collect();
    let root_ids: [Id<Purpose>; 4] = [Id::new(0), Id::new(1), Id::new(2), Id::new(3)];
    let mut homes = vec![Home::Builtin; drafts.len()];
    let mut parents = vec![None; drafts.len()];
    let mut seen: Map<(Home, &'s str), usize> = Map::default();
    for (at, root) in root_names.into_iter().enumerate() {
        seen.insert((Home::Builtin, root), at);
    }

    let mut written: Vec<(&axiom_syntax::File<'s>, &axiom_syntax::Decl<'s>, Home)> = Vec::new();
    let mut draft_of = Vec::new();
    for purpose in collected.decls_of(DeclKind::Purpose) {
        let (file, decl, home) = (purpose.file(), purpose.node, purpose.home());
        let text = decl.name.0;
        written.push((file, decl, home));
        if root_names.contains(&text) {
            // Purpose roots are built in identities. A declaration of a
            // root may attach laws to that identity without creating a
            // second tree node, just as a shared system may extend the
            // built-in root with domain rules.
            draft_of.push(root_ids[root_names.iter().position(|&name| name == text).unwrap()].index());
            continue;
        }
        if let Some(&first) = seen.get(&(home, text)) {
            let (word, earlier) = (Word::of(file, text), drafts.get(first).and_then(|purpose| purpose.loc));
            diags.push(Problem::Duplicate { noun: Noun::Purpose, word, first: earlier }.diagnostic());
            draft_of.push(first);
            continue;
        }
        let symbol = names.intern(text);
        let at = drafts.len();
        let draft = Purpose {
            name: symbol,
            root: PurposeRoot::Transfer,
            system: match home {
                Home::System(id) => Some(id),
                _ => None,
            },
            of: None,
            shares: Box::default(),
            laws: Box::default(),
            doc: purpose.item.doc.map(|doc| names.intern(doc.0)),
            loc: Some(file.loc(text)),
        };
        seen.insert((home, text), at);
        drafts.push(draft);
        homes.push(home);
        parents.push(None);
        draft_of.push(at);
    }

    let draft_names: Vec<_> = drafts
        .iter()
        .enumerate()
        .map(|(at, purpose)| (Id::<Purpose>::new(at as u32), names.name(purpose.name), homes[at]))
        .collect();
    let index = Scoped::build(names, draft_names);

    for (written_at, (file, decl, home)) in written.iter().enumerate() {
        let child = draft_of[written_at];
        if root_ids.iter().any(|&root| root.index() == child) || parents[child].is_some() {
            continue;
        }
        let Some(parent) = decl.kind else {
            diags.push(
                Diagnostic::error("purpose-parent", format!("purpose `{}` needs a parent", decl.name.0))
                    .label(file.loc(decl.name.0), "what is this purpose a kind of?")
                    .help("give it a parent such as `income`, `spending`, `capital`, or `transfer`"),
            );
            parents[child] = Some(root_ids[3].index());
            continue;
        };
        let scope = scopes.of(*home);
        match index.resolve(names, scope, parent.0) {
            Ok(parent_id) => parents[child] = Some(parent_id.index()),
            Err(miss) => {
                let describe = |id: Id<Purpose>| names.name(drafts[id.index()].name).to_string();
                let word = Word::of(file, parent.0);
                diags.push(unresolved(miss, Noun::Purpose, word, names, Reads::Name, describe));
                parents[child] = Some(root_ids[3].index());
            }
        }
    }

    for cycle in cycles(&parents) {
        let route: Vec<&str> = cycle.iter().map(|&at| names.name(drafts[at].name)).collect();
        let loc = drafts[cycle[0]].loc;
        let mut diagnostic = Diagnostic::error("purpose-cycle", format!("purpose `{}` inherits from itself", route[0]))
            .note(format!("the chain is {}", route.join(" -> ")))
            .help("give one of them a parent outside the loop");
        if let Some(loc) = loc {
            diagnostic = diagnostic.label(loc, "this parent chain never reaches a root");
        }
        diags.push(diagnostic);
        for &at in &cycle {
            parents[at] = Some(root_ids[3].index());
        }
    }

    let (mut tree, remap) = Tree::build(drafts, &parents).expect("purpose cycles were cut before freezing");
    for id in tree.ids() {
        tree[id].root = tree.parent(id).map_or_else(
            || {
                let at = root_ids.iter().position(|&root| remap[root.index()] == id).unwrap();
                [PurposeRoot::Income, PurposeRoot::Spending, PurposeRoot::Capital, PurposeRoot::Transfer][at]
            },
            |parent| tree[parent].root,
        );
    }

    // `of KIND` is the only property that determines a purpose's object type.
    for (at, (file, decl, home)) in written.iter().enumerate() {
        let id = remap[draft_of[at]];
        if root_ids.iter().any(|&root| remap[root.index()] == id) {
            continue;
        }
        for prop in &file[decl.props] {
            if prop.name.0 != "of" {
                continue;
            }
            let [expr] = &file[prop.args][..] else {
                diags.push(
                    Diagnostic::error("purpose-object", "`of` needs exactly one kind name")
                        .label(prop.loc, "write `of KIND`"),
                );
                continue;
            };
            let ExprKind::Name(kind_name) = file.exprs[*expr].kind else {
                diags.push(
                    Diagnostic::error("purpose-object", "`of` needs a kind name").label(prop.loc, "write `of KIND`"),
                );
                continue;
            };
            let scope = scopes.of(*home);
            match kinds::find(kind_index, names, systems, kind_name.0, |visible| scope.sees(visible)) {
                Ok(kind) => tree[id].of = Some(At { value: kind, loc: prop.loc }),
                Err(_) => diags.push(
                    Diagnostic::error("unknown-kind", format!("kind `{}` is not known here", kind_name.0))
                        .label(file.loc(kind_name.0), "not a visible kind"),
                ),
            }
        }
    }

    let mut final_homes = vec![Home::Builtin; homes.len()];
    for (at, &home) in homes.iter().enumerate() {
        final_homes[remap[at].index()] = home;
    }
    let final_names: Vec<_> =
        tree.iter().map(|(id, purpose)| (id, names.name(purpose.name), final_homes[id.index()])).collect();
    let index = Scoped::build(names, final_names);
    NativePurposes {
        tree,
        index,
        roots: crate::book::PurposeRoots {
            income: remap[root_ids[0].index()],
            spending: remap[root_ids[1].index()],
            capital: remap[root_ids[2].index()],
            transfer: remap[root_ids[3].index()],
        },
        declarations: draft_of.into_iter().map(|at| remap[at]).collect(),
    }
}

fn cycles(parents: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut state = vec![0_u8; parents.len()];
    let mut found = Vec::new();
    for start in 0..parents.len() {
        let mut path = Vec::new();
        let mut at = Some(start);
        while let Some(id) = at {
            match state[id] {
                0 => {
                    state[id] = 1;
                    path.push(id);
                    at = parents[id];
                }
                1 => {
                    if let Some(from) = path.iter().position(|&member| member == id) {
                        found.push(path[from..].to_vec());
                    }
                    break;
                }
                _ => break,
            }
        }
        for id in path {
            state[id] = 2;
        }
    }
    found
}
