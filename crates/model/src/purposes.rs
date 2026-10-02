//! Native v4 purpose declarations.
//!
//! The four roots and every written purpose are assembled from borrowed S5
//! declarations before one immutable tree is built. Forward parents work
//! because resolution happens against the complete draft index.

use axiom_core::{Diagnostic, Id, Interner, Map, Tree};
use axiom_syntax::{Decl, DeclKind, ExprKind};

use crate::book::{At, Kind, Purpose, PurposeRoot};
use crate::collect::{Collected, Written};
use crate::errors::Word;
use crate::kinds;
use crate::names::Scoped;
use crate::problem::{self, Among, Noun};
use crate::scope::{Home, Seeing};

pub(crate) struct NativePurposes {
    pub tree: Tree<Purpose>,
    pub index: Scoped<Purpose>,
    pub roots: crate::book::PurposeRoots,
}

/// The four roots, which are the first four drafts.
const ROOTS: [(&str, PurposeRoot); 4] = [
    ("income", PurposeRoot::Income),
    ("spending", PurposeRoot::Spending),
    ("capital", PurposeRoot::Capital),
    ("transfer", PurposeRoot::Transfer),
];

/// The draft a purpose hangs from when it names no parent, or one that is not there.
const TRANSFER: usize = 3;

pub(crate) fn declare_sites<'s>(
    collected: &Collected<'_, 's>,
    names: &mut Interner<'s>,
    seeing: Seeing<'_>,
    kind_index: &Scoped<Kind>,
    diags: &mut Vec<Diagnostic>,
) -> NativePurposes {
    let mut drafts = Drafts::of(collected, names, diags);
    let draft_index = drafts.index(names);
    drafts.link_parents(&draft_index, names, seeing, diags);
    drafts.cut_cycles(names, diags);
    let (mut tree, remap) = drafts.freeze();
    drafts.attach_objects(&mut tree, &remap, names, seeing, kind_index, diags);
    let index = drafts.index_of(&tree, &remap, names);
    let root = |at: usize| remap[at];
    let roots =
        crate::book::PurposeRoots { income: root(0), spending: root(1), capital: root(2), transfer: root(TRANSFER) };
    NativePurposes { tree, index, roots }
}

/// One written purpose and the draft it stands for: its own, the first of its name, or a root.
struct Declared<'a, 's> {
    written: Written<'a, 's, Decl<'s>>,
    draft: usize,
}

/// The purposes as written, before they are a tree. Forward parents work because they are resolved against all of
/// them.
struct Drafts<'a, 's> {
    purposes: Vec<Purpose>,
    homes: Vec<Home>,
    parents: Vec<Option<usize>>,
    declared: Vec<Declared<'a, 's>>,
}

impl<'a, 's> Drafts<'a, 's> {
    /// The roots, then one draft for each declaration that is not a root's or an earlier one's again.
    fn of(collected: &Collected<'a, 's>, names: &mut Interner<'s>, diags: &mut Vec<Diagnostic>) -> Drafts<'a, 's> {
        let purposes: Vec<Purpose> = ROOTS
            .iter()
            .map(|&(name, root)| Purpose {
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
        let mut drafts = Drafts {
            homes: vec![Home::Builtin; purposes.len()],
            parents: vec![None; purposes.len()],
            purposes,
            declared: Vec::new(),
        };
        let mut seen: Map<(Home, &'s str), usize> = Map::default();
        for (at, &(root, _)) in ROOTS.iter().enumerate() {
            seen.insert((Home::Builtin, root), at);
        }
        for written in collected.decls_of(DeclKind::Purpose) {
            let draft = drafts.declare(written, &mut seen, names, diags);
            drafts.declared.push(Declared { written: *written, draft });
        }
        drafts
    }

    /// The draft a declaration is: a new one, or the earlier one it repeats (said, unless it is a root, which a
    /// declaration may attach laws to as a system may extend the built-in root with domain rules).
    fn declare(
        &mut self,
        purpose: &Written<'a, 's, Decl<'s>>,
        seen: &mut Map<(Home, &'s str), usize>,
        names: &mut Interner<'s>,
        diags: &mut Vec<Diagnostic>,
    ) -> usize {
        let (file, decl, home) = (purpose.file(), purpose.node, purpose.home());
        let text = decl.name.0;
        if let Some(root) = ROOTS.iter().position(|&(name, _)| name == text) {
            return root;
        }
        if let Some(&first) = seen.get(&(home, text)) {
            diags.push(problem::duplicate(Noun::Purpose, Word::of(file, text), self.purposes[first].loc));
            return first;
        }
        let at = self.purposes.len();
        self.purposes.push(Purpose {
            name: names.intern(text),
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
        });
        seen.insert((home, text), at);
        self.homes.push(home);
        self.parents.push(None);
        at
    }

    fn index(&self, names: &mut Interner<'s>) -> Scoped<Purpose> {
        let drafts: Vec<_> = (self.purposes.iter().enumerate())
            .map(|(at, purpose)| (Id::<Purpose>::new(at as u32), names.name(purpose.name), self.homes[at]))
            .collect();
        Scoped::build(names, drafts)
    }

    /// Each declaration's parent, found among all the drafts; one that names none, or none there, hangs from
    /// `transfer`.
    fn link_parents(
        &mut self,
        index: &Scoped<Purpose>,
        names: &Interner<'s>,
        seeing: Seeing<'_>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Drafts { purposes, parents, declared, .. } = self;
        for Declared { written, draft } in declared.iter() {
            let (file, decl) = (written.file(), written.node);
            if *draft < ROOTS.len() || parents[*draft].is_some() {
                continue;
            }
            let Some(parent) = decl.kind else {
                diags.push(
                    Diagnostic::error("purpose-parent", format!("purpose `{}` needs a parent", decl.name.0))
                        .label(file.loc(decl.name.0), "what is this purpose a kind of?")
                        .help("give it a parent such as `income`, `spending`, `capital`, or `transfer`"),
                );
                parents[*draft] = Some(TRANSFER);
                continue;
            };
            parents[*draft] = Some(match index.resolve(names, seeing.scopes.of(written.home()), parent.0) {
                Ok(parent_id) => parent_id.index(),
                Err(miss) => {
                    let among = Among { index, names, systems: seeing.systems };
                    let describe = |ids: &[Id<Purpose>]| {
                        let (name, loc) =
                            (|id: Id<Purpose>| purposes[id.index()].name, |id: Id<Purpose>| purposes[id.index()].loc);
                        problem::shortest(names, &index.names, ids, name, loc)
                    };
                    diags.push(among.failed(miss, Noun::Purpose, Word::of(file, parent.0), describe));
                    TRANSFER
                }
            });
        }
    }

    /// A parent chain that never reaches a root is said, and cut by hanging each of its members from `transfer`.
    fn cut_cycles(&mut self, names: &Interner<'s>, diags: &mut Vec<Diagnostic>) {
        for cycle in cycles(&self.parents) {
            let route: Vec<&str> = cycle.iter().map(|&at| names.name(self.purposes[at].name)).collect();
            let mut diagnostic =
                Diagnostic::error("purpose-cycle", format!("purpose `{}` inherits from itself", route[0]))
                    .note(format!("the chain is {}", route.join(" -> ")))
                    .help("give one of them a parent outside the loop");
            if let Some(loc) = self.purposes[cycle[0]].loc {
                diagnostic = diagnostic.label(loc, "this parent chain never reaches a root");
            }
            diags.push(diagnostic);
            for &at in &cycle {
                self.parents[at] = Some(TRANSFER);
            }
        }
    }

    /// The tree, and where each draft went in it. A purpose is of the root it hangs beneath.
    fn freeze(&mut self) -> (Tree<Purpose>, Vec<Id<Purpose>>) {
        let purposes = std::mem::take(&mut self.purposes);
        let (mut tree, remap) = Tree::build(purposes, &self.parents).expect("purpose cycles were cut before freezing");
        for id in tree.ids() {
            tree[id].root = tree.parent(id).map_or_else(
                || ROOTS[(0..ROOTS.len()).find(|&at| remap[at] == id).unwrap()].1,
                |parent| tree[parent].root,
            );
        }
        (tree, remap)
    }

    /// `of KIND` is the only property that determines a purpose's object type.
    fn attach_objects(
        &self,
        tree: &mut Tree<Purpose>,
        remap: &[Id<Purpose>],
        names: &Interner<'s>,
        seeing: Seeing<'_>,
        kind_index: &Scoped<Kind>,
        diags: &mut Vec<Diagnostic>,
    ) {
        for Declared { written, draft } in &self.declared {
            let (file, decl) = (written.file(), written.node);
            let id = remap[*draft];
            if remap[..ROOTS.len()].contains(&id) {
                continue;
            }
            for prop in file[decl.props].iter().filter(|prop| prop.name.0 == "of") {
                let [expr] = &file[prop.args][..] else {
                    diags.push(
                        Diagnostic::error("purpose-object", "`of` needs exactly one kind name")
                            .label(prop.loc, "write `of KIND`"),
                    );
                    continue;
                };
                let ExprKind::Name(kind_name) = file.exprs[*expr].kind else {
                    diags.push(
                        Diagnostic::error("purpose-object", "`of` needs a kind name")
                            .label(prop.loc, "write `of KIND`"),
                    );
                    continue;
                };
                let scope = seeing.scopes.of(written.home());
                match kinds::find(kind_index, names, seeing.systems, kind_name.0, |visible| scope.sees(visible)) {
                    Ok(kind) => tree[id].of = Some(At { value: kind, loc: prop.loc }),
                    Err(_) => diags.push(
                        Diagnostic::error("unknown-kind", format!("kind `{}` is not known here", kind_name.0))
                            .label(file.loc(kind_name.0), "not a visible kind"),
                    ),
                }
            }
        }
    }

    /// What answers to a purpose's name, now that the tree has put them in their final order.
    fn index_of(&self, tree: &Tree<Purpose>, remap: &[Id<Purpose>], names: &mut Interner<'s>) -> Scoped<Purpose> {
        let mut homes = vec![Home::Builtin; self.homes.len()];
        for (at, &home) in self.homes.iter().enumerate() {
            homes[remap[at].index()] = home;
        }
        let tree_names: Vec<_> =
            tree.iter().map(|(id, purpose)| (id, names.name(purpose.name), homes[id.index()])).collect();
        Scoped::build(names, tree_names)
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
