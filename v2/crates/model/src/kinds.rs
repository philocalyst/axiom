//! Kinds: what things are.
//!
//! Kinds form one tree rooted in the built-in `asset liability income expense
//! equity` (place kinds), `thing`, `commodity` and `entity`. A kind adds properties
//! (typed, with defaults) and laws to everything beneath it. This stage creates
//! the kinds and links each to its parent; what flows down the chain is
//! resolved with the other properties, once everything can be named.

use axiom_core::{Diagnostic, Id, Interner, Loc, Map, Tree};
use axiom_syntax::DeclKind;

use crate::book::{Class, Kind, Miss, PathRoot, Sort, System};
use crate::collect::{Entry, decls};
use crate::cx::Cx;
use crate::errors::{Candidate, Word, article, duplicate, not_used, unknown};
use crate::names::Scoped;
use crate::scope::{Home, Scopes};

/// The five place roots come first, in the order of [`PathRoot`].
pub(crate) const ROOTS: [(&str, Sort); 9] = [
    ("asset", Sort::Place(Class::Asset)),
    ("liability", Sort::Place(Class::Debt)),
    ("income", Sort::Place(Class::Outside)),
    ("expense", Sort::Place(Class::Outside)),
    ("equity", Sort::Place(Class::Outside)),
    ("commodity", Sort::Commodity),
    ("entity", Sort::Entity),
    // v3 bridge: built in, under `income`, for `is market` in laws, which the
    // standard systems write. The market itself is now an entity.
    ("market", Sort::Place(Class::Outside)),
    ("thing", Sort::Thing),
];

/// The positions in `ROOTS` of `market`, of its parent `income`, of `entity`, and of `thing`.
const MARKET: usize = 7;
const INCOME: usize = 2;
const ENTITY: usize = 6;
const THING: usize = 8;

/// Where the built-in roots landed in the tree.
#[derive(Clone, Copy)]
pub(crate) struct RootKinds(pub [Id<Kind>; ROOTS.len()]);

impl RootKinds {
    /// The root kind of the places under a path root.
    pub fn of_root(&self, root: PathRoot) -> Id<Kind> {
        self.0[root as usize]
    }

    pub fn commodity(&self) -> Id<Kind> {
        self.0[5]
    }

    pub fn entity(&self) -> Id<Kind> {
        self.0[ENTITY]
    }

    pub fn thing(&self) -> Id<Kind> {
        self.0[THING]
    }

    // v3 bridge: the kind of the market's place.
    pub fn market(&self) -> Id<Kind> {
        self.0[MARKET]
    }
}

pub(crate) struct Kinds {
    pub tree: Tree<Kind>,
    pub index: Scoped<Kind>,
    pub roots: RootKinds,
    /// The id of each declaration, in the order written.
    pub declared: Vec<Id<Kind>>,
    /// Kinds whose chain never reached a root: their parent is missing, or
    /// unknown, or on a cycle. That was reported once, and what is described
    /// by them, or written inside them, has no need to say it again.
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

pub(crate) fn declare<'a, 's>(entries: &[Entry<'a, 's>], cx: &mut Cx<'_, 's>) -> Kinds {
    let Cx { names, systems, scopes, diags } = cx;
    let mut drafts: Vec<Kind> = ROOTS.iter().map(|&(name, sort)| draft(names, name, sort)).collect();
    let mut texts: Vec<&str> = ROOTS.iter().map(|root| root.0).collect();
    let mut homes = vec![Home::Builtin; ROOTS.len()];
    // A kind written twice in one place is one kind and an error.
    let mut first: Map<(Home, &str), usize> = Map::default();
    let mut draft_of = Vec::new();
    let declared: Vec<_> = decls(entries, DeclKind::Kind).collect();
    for written in &declared {
        let (file, name) = (written.file(), written.node.name.0);
        let earlier = first.get(&(written.home(), name)).copied();
        draft_of.push(earlier.unwrap_or(drafts.len()));
        if let Some(earlier) = earlier {
            let system = None;
            diags.push(duplicate("kind", Word { text: name, loc: file.loc(name) }, drafts[earlier].loc, system));
            continue;
        }
        first.insert((written.home(), name), drafts.len());
        let mut kind = draft(names, name, Sort::Entity);
        kind.system = if let Home::System(system) = written.home() { Some(system) } else { None };
        kind.doc = written.item.doc.map(|doc| names.intern(doc.0));
        kind.loc = Some(file.loc(name));
        drafts.push(kind);
        texts.push(name);
        homes.push(written.home());
    }

    // Parents are found by name among the drafts, whose ids are their positions.
    let index = index_of(names, texts.iter().zip(&homes).map(|(&text, &home)| (text, home)));
    let mut parents: Vec<Option<usize>> = vec![None; ROOTS.len()];
    parents[MARKET] = Some(INCOME);
    let mut broken = vec![false; ROOTS.len()];
    for (written, &draft) in declared.iter().zip(&draft_of) {
        if draft == parents.len() {
            let parent = parent_of(written, &index, &drafts, names, systems, scopes, diags);
            broken.push(parent.is_none());
            parents.push(Some(parent.unwrap_or(ENTITY)));
        }
    }
    for cycle in cycles(&parents) {
        diags.push(cycle_diagnostic(&cycle, &drafts, names));
        cycle.iter().for_each(|&kind| (parents[kind], broken[kind]) = (Some(ENTITY), true));
    }
    let (mut tree, new_id) = Tree::build(drafts, &parents).expect("cycles were cut, so kinds form a forest");
    for id in tree.ids() {
        if let Some(parent) = tree.parent(id) {
            tree[id].sort = tree[parent].sort;
        }
    }
    let mut final_homes = vec![Home::Builtin; homes.len()];
    let mut unrooted = vec![false; homes.len()];
    for (old, &home) in homes.iter().enumerate() {
        final_homes[new_id[old].index()] = home;
        unrooted[new_id[old].index()] = broken[old];
    }
    // Parents come first in the tree, so a kind under a broken one is broken too.
    for id in tree.ids() {
        if let Some(parent) = tree.parent(id) {
            unrooted[id.index()] |= unrooted[parent.index()];
        }
    }
    let things = tree.iter().map(|(id, kind)| (id, names.name(kind.name), final_homes[id.index()])).collect::<Vec<_>>();
    let index = Scoped::build(names, things);
    let roots = RootKinds(std::array::from_fn(|at| new_id[at]));
    Kinds { tree, index, roots, declared: draft_of.iter().map(|&draft| new_id[draft]).collect(), unrooted }
}

fn index_of<'s>(names: &mut Interner<'s>, things: impl Iterator<Item = (&'s str, Home)>) -> Scoped<Kind> {
    Scoped::build(names, things.enumerate().map(|(at, (text, home))| (Id::new(at as u32), text, home)))
}

impl Kinds {
    /// The kind a declaration names, which must be a kind under `root`; `root`
    /// itself when it names none, or one that cannot fit.
    pub fn declared(&self, written: Option<Word>, home: Home, root: Id<Kind>, thing: &str, cx: &mut Cx) -> Id<Kind> {
        let Some(word) = written else { return root };
        let scope = cx.scopes.of(home);
        match find(&self.index, cx.names, cx.systems, word.text, |home| scope.sees(home)) {
            Err(miss) => {
                cx.diags.push(unresolved(miss, word, &self.index, cx.names, cx.systems, |id| self.tree[id].loc));
                root
            }
            Ok(kind) if self.tree.covers(root, kind) => kind,
            Ok(kind) if self.unrooted[kind.index()] => root,
            Ok(kind) => {
                let top = self.tree.lineage(kind).last().unwrap_or(kind);
                let is = cx.names.name(self.tree[top].name);
                let mut diagnostic = Diagnostic::error(
                    "kind-sort",
                    format!("kind `{}` is {} kind, so it cannot describe {thing}", word.text, article(is)),
                )
                .label(word.loc, format!("{is} kind"));
                if let Some(loc) = self.tree[kind].loc {
                    diagnostic = diagnostic.context(loc, "declared here");
                }
                cx.diags.push(diagnostic);
                root
            }
        }
    }
}

/// The draft position of the parent of a declared kind.
fn parent_of<'a, 's>(
    written: &crate::collect::Written<'a, 's, axiom_syntax::Decl<'s>>,
    index: &Scoped<Kind>,
    drafts: &[Kind],
    names: &Interner<'s>,
    systems: &Tree<System>,
    scopes: &Scopes,
    diags: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let (file, decl) = (written.file(), written.node);
    let Some(parent) = decl.kind else {
        diags.push(
            Diagnostic::error("kind-parent", format!("kind `{}` needs a parent", decl.name.0))
                .label(file.loc(decl.name.0), "what kind of thing is this?")
                .help("write `: asset`, `: liability`, `: income`, `: expense`, `: equity`, `: commodity`, `: entity`, or another kind"),
        );
        return None;
    };
    let scope = scopes.of(written.home());
    match find(index, names, systems, parent.0, |home| scope.sees(home)) {
        Ok(id) => Some(id.index()),
        Err(miss) => {
            let word = Word { text: parent.0, loc: file.loc(parent.0) };
            diags.push(unresolved(miss, word, index, names, systems, |id| drafts[id.index()].loc));
            None
        }
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

/// Why `word` named no single kind.
pub(crate) fn unresolved(
    miss: Miss<Kind>,
    word: Word,
    kinds: &Scoped<Kind>,
    names: &Interner,
    systems: &Tree<System>,
    loc_of: impl Fn(Id<Kind>) -> Option<Loc>,
) -> Diagnostic {
    let system_path = |id: Id<Kind>| match kinds.home(id) {
        Home::System(system) => Some(names.name(systems[system].path)),
        Home::Project | Home::Builtin => None,
    };
    match miss {
        Miss::Unknown { suggestion } => {
            let mut diagnostic = unknown("unknown-kind", "kind", word, suggestion.map(|sym| names.name(sym)));
            for &hidden in kinds.names.candidates(names, word.text) {
                if let Some(system) = system_path(hidden) {
                    diagnostic = not_used(diagnostic, "kind", word.text, system);
                }
            }
            diagnostic
        }
        Miss::Ambiguous(ids) => {
            let candidates: Vec<Candidate> = ids
                .iter()
                .map(|&id| {
                    let declared = loc_of(id);
                    match (kinds.home(id), system_path(id)) {
                        (Home::System(_), Some(system)) => {
                            let last = system.rsplit('/').next().unwrap_or(system);
                            // `us/401k` names the kind `401k` of that system, and
                            // is the shorter way to say it.
                            let write =
                                if last == word.text { system.to_string() } else { format!("{system}/{}", word.text) };
                            Candidate { is: format!("`{}` from `{system}`", word.text), declared, write: Some(write) }
                        }
                        (Home::Builtin, _) => {
                            Candidate { is: format!("the built-in `{}`", word.text), declared, write: None }
                        }
                        _ => Candidate { is: format!("the project's `{}`", word.text), declared, write: None },
                    }
                })
                .collect();
            crate::errors::ambiguous("ambiguous-kind", "kinds", word, &candidates)
        }
    }
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
