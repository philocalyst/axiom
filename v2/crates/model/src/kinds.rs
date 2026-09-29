//! Kinds: what things are.
//!
//! Kinds form one tree rooted in the built-in `asset liability income expense
//! equity` (place kinds), `commodity` and `entity`. A kind adds properties
//! (typed, with defaults) and laws to everything beneath it. This stage links
//! each declared kind to its parent and resolves what flows down the chain:
//! `restricted`, `deferred`, `select`, `liquidity`, `has`. Default values wait
//! until entities and places exist to be named.

use axiom_core::{Diagnostic, Id, Interner, Loc, Map, Span, Sym, Tree};
use axiom_syntax::{Decl, ExprKind, Name, Policy};

use crate::args::{Args, Builtin, FIELD_WORDS, list};
use crate::book::{Class, Has, Kind, Miss, Sort, System};
use crate::catalog::{Catalog, Written};
use crate::cx::Cx;
use crate::errors::{Candidate, ambiguous, article, duplicate, not_used, unknown};
use crate::law::Ty;
use crate::names::Scoped;
use crate::scope::Home;

pub(crate) const ROOTS: [(&str, Sort); 7] = [
    ("asset", Sort::Place(Class::Asset)),
    ("liability", Sort::Place(Class::Liability)),
    ("income", Sort::Place(Class::Income)),
    ("expense", Sort::Place(Class::Expense)),
    ("equity", Sort::Place(Class::Equity)),
    ("commodity", Sort::Commodity),
    ("entity", Sort::Entity),
];

/// Where the built-in roots landed in the tree.
#[derive(Clone, Copy)]
pub(crate) struct RootKinds {
    pub asset: Id<Kind>,
    pub liability: Id<Kind>,
    pub income: Id<Kind>,
    pub expense: Id<Kind>,
    pub equity: Id<Kind>,
    pub commodity: Id<Kind>,
    pub entity: Id<Kind>,
}

impl RootKinds {
    fn new(ids: &[Id<Kind>]) -> RootKinds {
        RootKinds {
            asset: ids[0],
            liability: ids[1],
            income: ids[2],
            expense: ids[3],
            equity: ids[4],
            commodity: ids[5],
            entity: ids[6],
        }
    }

    pub fn of_class(&self, class: Class) -> Id<Kind> {
        match class {
            Class::Asset => self.asset,
            Class::Liability => self.liability,
            Class::Income => self.income,
            Class::Expense => self.expense,
            Class::Equity => self.equity,
        }
    }
}

pub(crate) struct Kinds {
    pub tree: Tree<Kind>,
    pub index: Scoped<Kind>,
    pub roots: RootKinds,
    /// The id of each declaration in the catalog, in catalog order.
    pub declared: Vec<Id<Kind>>,
    pub properties: PropTable,
}

pub(crate) fn declare<'s>(catalog: &Catalog<'_, 's>, cx: &mut Cx<'_, 's>) -> Kinds {
    let mut kinds: Vec<Kind> = ROOTS.iter().map(|&(name, sort)| root(cx.names, name, sort)).collect();
    let mut texts: Vec<&str> = ROOTS.iter().map(|root| root.0).collect();
    let mut homes = vec![Home::Builtin; ROOTS.len()];
    // A kind written twice in one place is one kind and an error.
    let mut first: Map<(Home, &str), usize> = Map::default();
    let mut draft_of = Vec::with_capacity(catalog.kinds.len());
    for written in &catalog.kinds {
        let name = written.what.name;
        let draft = match first.get(&(written.home(), name.text)) {
            Some(&earlier) => {
                cx.diags.push(duplicate("kind", name, kinds[earlier].loc));
                earlier
            }
            None => {
                first.insert((written.home(), name.text), kinds.len());
                kinds.push(own_kind(written, cx));
                texts.push(name.text);
                homes.push(written.home());
                kinds.len() - 1
            }
        };
        draft_of.push(draft);
    }
    // Ids are draft positions until the tree renumbers them.
    let drafts = texts.iter().zip(&homes).enumerate().map(|(at, (&text, &home))| (Id::new(at as u32), text, home));
    let index = Scoped::build(cx.names, drafts);

    let mut parents: Vec<Option<usize>> = vec![None; ROOTS.len()];
    for (at, written) in catalog.kinds.iter().enumerate() {
        if draft_of[at] == parents.len() {
            parents.push(Some(parent_of(written, &index, &kinds, cx)));
        }
    }
    let (mut tree, new_id) = arrange(kinds, parents, cx);
    let mut properties = PropTable::default();
    inherit(&mut tree, &mut properties, cx);

    Kinds {
        roots: RootKinds::new(&new_id[..ROOTS.len()]),
        declared: draft_of.iter().map(|&draft| new_id[draft]).collect(),
        index: index.renumbered(&new_id),
        tree,
        properties,
    }
}

impl Kinds {
    /// The kind a declaration names, which must be a kind of `sort`; the root
    /// of that sort when it names none, or one that cannot fit.
    pub fn declared(&self, decl: &Decl, home: Home, sort: Sort, thing: &str, cx: &mut Cx) -> Id<Kind> {
        let root = match sort {
            Sort::Place(class) => self.roots.of_class(class),
            Sort::Commodity => self.roots.commodity,
            Sort::Entity => self.roots.entity,
        };
        let Some(written) = decl.kind else { return root };
        let scope = cx.scopes.of(home);
        let found = find(&self.index, cx.names, cx.systems, written.text, |home| scope.sees(home));
        match found {
            Err(miss) => {
                cx.diags.push(unresolved(miss, written, &self.index, cx.names, cx.systems, |id| self.tree[id].loc));
                root
            }
            Ok(kind) if self.tree[kind].sort == sort => kind,
            Ok(kind) => {
                let declared = &self.tree[kind];
                let is = declared.sort.noun();
                let mut diagnostic = Diagnostic::error(
                    "kind-sort",
                    format!("kind `{}` is {} kind, so it cannot describe {thing}", written.text, article(is)),
                )
                .label(written.loc, format!("{} kind", is));
                if let Some(loc) = declared.loc {
                    diagnostic = diagnostic.context(loc, "declared here");
                }
                cx.diags.push(diagnostic);
                root
            }
        }
    }
}

fn root<'s>(names: &mut Interner<'s>, name: &'s str, sort: Sort) -> Kind {
    Kind {
        name: names.intern(name),
        sort,
        system: None,
        restricted: false,
        deferred: false,
        select: None,
        liquidity: None,
        has: Box::default(),
        props: Box::default(),
        laws: Box::default(),
        doc: None,
        loc: None,
    }
}

/// A declared kind with the properties that need no other name resolved.
fn own_kind<'s>(written: &Written<'_, 's, Decl<'s>>, cx: &mut Cx<'_, 's>) -> Kind {
    let decl = written.what;
    let mut kind = root(cx.names, decl.name.text, Sort::Entity);
    kind.system = match written.home() {
        Home::System(system) => Some(system),
        Home::Project | Home::Builtin => None,
    };
    kind.doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
    kind.loc = Some(decl.name.loc);

    let mut has = Vec::new();
    for prop in &decl.props {
        let mut args = Args::new(written.exprs(), written.home(), prop);
        let read = match Builtin::parse(prop.name.text) {
            Some(Builtin::Restricted) => args.done().map(|()| kind.restricted = true),
            Some(Builtin::Deferred) => args.done().map(|()| kind.deferred = true),
            Some(Builtin::Select) => policy(&mut args).map(|policy| kind.select = Some(policy)),
            Some(Builtin::Liquidity) => span(&mut args).map(|span| kind.liquidity = Some(span)),
            Some(Builtin::Has) => declaration(&mut args, cx.names).map(|declared| has.push(declared)),
            _ => Ok(()),
        };
        cx.diags.extend(read.err());
    }
    kind.has = has.into();
    kind
}

pub(crate) const POLICIES: [(&str, Policy); 4] =
    [("fifo", Policy::Fifo), ("lifo", Policy::Lifo), ("hifo", Policy::Hifo), ("prorata", Policy::Prorata)];

/// `select fifo`
pub(crate) fn policy(args: &mut Args) -> Result<Policy, Diagnostic> {
    let words: Vec<&str> = POLICIES.iter().map(|policy| policy.0).collect();
    let word = args.word(&words)?;
    args.done()?;
    Ok(POLICIES.iter().find(|policy| policy.0 == word).expect("word() returns one of the allowed words").1)
}

/// `liquidity 5d`
pub(crate) fn span(args: &mut Args) -> Result<Span, Diagnostic> {
    let span = args.span()?;
    args.done()?;
    Ok(span)
}

const TYPES: [(&str, Ty); 12] = [
    ("date", Ty::Day),
    ("amount", Ty::Amount),
    ("number", Ty::Num),
    ("percent", Ty::Num),
    ("span", Ty::Span),
    ("text", Ty::Text),
    ("name", Ty::Name),
    ("entity", Ty::Entity),
    ("place", Ty::Place),
    ("kind", Ty::Kind),
    ("unit", Ty::Unit),
    ("bool", Ty::Bool),
];

/// `has employer entity`
fn declaration<'s>(args: &mut Args<'_, 's>, names: &mut Interner<'s>) -> Result<Has, Diagnostic> {
    let name = args.take("a property name")?;
    let ExprKind::Name(text) = name.kind else { return Err(args.wrong(name, "a property name")) };
    let word = args.take("a type")?;
    let words: Vec<&str> = TYPES.iter().map(|ty| ty.0).collect();
    let ty = match word.kind {
        ExprKind::Name(written) => TYPES.iter().find(|ty| ty.0 == written).map(|ty| ty.1),
        _ => None,
    };
    let Some(ty) = ty else {
        return Err(args.wrong(word, &format!("a type: {}", list(&words))));
    };
    args.done()?;
    if Builtin::parse(text).is_some() || FIELD_WORDS.contains(&text) {
        return Err(Diagnostic::error("reserved-property", format!("`{text}` is a built-in property"))
            .label(name.loc, "choose another name")
            .note("built-in properties keep their meaning everywhere, so a kind cannot redefine them"));
    }
    Ok(Has { name: names.intern(text), ty, loc: Some(name.loc) })
}

/// The draft position of `decl`'s parent kind.
fn parent_of<'s>(
    written: &Written<'_, 's, Decl<'s>>,
    index: &Scoped<Kind>,
    drafts: &[Kind],
    cx: &mut Cx<'_, 's>,
) -> usize {
    const ENTITY: usize = ROOTS.len() - 1;
    let decl = written.what;
    let Some(parent) = decl.kind else {
        cx.diags.push(
            Diagnostic::error("kind-parent", format!("kind `{}` needs a parent", decl.name.text))
                .label(decl.name.loc, "what kind of thing is this?")
                .help("write `: asset`, `: liability`, `: income`, `: expense`, `: equity`, `: commodity`, `: entity`, or another kind"),
        );
        return ENTITY;
    };
    let scope = cx.scopes.of(written.home());
    match find(index, cx.names, cx.systems, parent.text, |home| scope.sees(home)) {
        Ok(id) => id.index(),
        Err(miss) => {
            cx.diags.push(unresolved(miss, parent, index, cx.names, cx.systems, |id| drafts[id.index()].loc));
            ENTITY
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

/// Why `name` named no single kind.
pub(crate) fn unresolved(
    miss: Miss<Kind>,
    name: Name,
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
            let mut diagnostic = unknown("unknown-kind", "kind", name, suggestion.map(|sym| names.name(sym)));
            for &hidden in kinds.names.candidates(names, name.text) {
                let Some(system) = system_path(hidden) else { continue };
                diagnostic = not_used(diagnostic, "kind", name.text, system);
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
                                if last == name.text { system.to_string() } else { format!("{system}/{}", name.text) };
                            Candidate { is: format!("`{}` from `{system}`", name.text), declared, write: Some(write) }
                        }
                        (Home::Builtin, _) => {
                            Candidate { is: format!("the built-in `{}`", name.text), declared, write: None }
                        }
                        _ => Candidate { is: format!("the project's `{}`", name.text), declared, write: None },
                    }
                })
                .collect();
            ambiguous("ambiguous-kind", "kinds", name, &candidates)
        }
    }
}

/// Builds the tree. Kinds that inherit from themselves are reported and hung
/// from the `entity` root instead, so the rest of the book still stands.
fn arrange(kinds: Vec<Kind>, mut parents: Vec<Option<usize>>, cx: &mut Cx) -> (Tree<Kind>, Vec<Id<Kind>>) {
    match Tree::build(kinds.clone(), &parents) {
        Ok(built) => built,
        Err(offenders) => {
            report_cycles(&offenders, &parents, &kinds, cx);
            for &at in &offenders {
                parents[at] = Some(ROOTS.len() - 1);
            }
            Tree::build(kinds, &parents).expect("kinds cut loose from a cycle hang from a root")
        }
    }
}

/// One report for each cycle. Kinds that only hang beneath a cycle are not to
/// blame, and are put right when it is.
fn report_cycles(offenders: &[usize], parents: &[Option<usize>], kinds: &[Kind], cx: &mut Cx) {
    let mut reported: Vec<usize> = Vec::new();
    for &at in offenders {
        let Some(members) = cycle_through(at, parents) else { continue };
        if members.iter().any(|member| reported.contains(member)) {
            continue;
        }
        reported.extend(&members);
        cx.diags.push(cycle(&members, kinds, cx.names));
    }
}

/// The kinds from `at` to the one that inherits from `at`, if the chain of
/// parents comes back to it.
fn cycle_through(at: usize, parents: &[Option<usize>]) -> Option<Vec<usize>> {
    let mut chain = vec![at];
    let mut next = parents[at];
    while let Some(kind) = next {
        if kind == at {
            return Some(chain);
        }
        if chain.contains(&kind) {
            return None;
        }
        chain.push(kind);
        next = parents[kind];
    }
    None
}

fn cycle(members: &[usize], kinds: &[Kind], names: &Interner) -> Diagnostic {
    let name = |kind: usize| names.name(kinds[kind].name);
    let route: Vec<&str> = members.iter().chain(&members[..1]).map(|&kind| name(kind)).collect();
    let mut diagnostic = Diagnostic::error("kind-cycle", format!("kind `{}` inherits from itself", route[0]))
        .note(format!("the chain is {}", route.join(" -> ")))
        .help("give one of them a parent outside the loop, such as a root kind like `asset`");
    if let Some(loc) = kinds[members[0]].loc {
        diagnostic = diagnostic.label(loc, "its parent chain never reaches a root");
    }
    for (&child, parent) in members.iter().zip(&route[1..]).skip(1) {
        if let Some(loc) = kinds[child].loc {
            diagnostic = diagnostic.context(loc, format!("`{}` inherits from `{parent}` here", name(child)));
        }
    }
    diagnostic
}

/// Passes down what a kind inherits, parents first (they have lower ids).
fn inherit(tree: &mut Tree<Kind>, properties: &mut PropTable, cx: &mut Cx) {
    for id in tree.ids() {
        let Some(parent) = tree.parent(id) else { continue };
        let above = tree[parent].clone();
        let kind = &mut tree[id];
        kind.sort = above.sort;
        kind.restricted |= above.restricted;
        kind.deferred |= above.deferred;
        kind.select = kind.select.or(above.select);
        kind.liquidity = kind.liquidity.or(above.liquidity);
        properties.declare(kind.sort, &kind.has, cx.names, cx.diags);
        let inherited = above.has.iter().filter(|theirs| !kind.has.iter().any(|own| own.name == theirs.name));
        kind.has = kind.has.iter().chain(inherited).copied().collect();
    }
}

/// Every declared property, typed once for all things of its sort: two kinds
/// that declare `filing` for entities agree on what it is, so `owner.filing`
/// has one type wherever it is written.
#[derive(Default)]
pub(crate) struct PropTable {
    declared: Map<(Ty, Sym), Has>,
}

impl PropTable {
    /// The type family a sort's properties are typed in: places, entities or
    /// commodities (as `Ty::Unit`), matching what a law's receiver is.
    pub fn family(sort: Sort) -> Ty {
        match sort {
            Sort::Place(_) => Ty::Place,
            Sort::Commodity => Ty::Unit,
            Sort::Entity => Ty::Entity,
        }
    }

    fn declare(&mut self, sort: Sort, own: &[Has], names: &Interner, diags: &mut Vec<Diagnostic>) {
        let family = PropTable::family(sort);
        for &has in own {
            let earlier = *self.declared.entry((family, has.name)).or_insert(has);
            if earlier.ty != has.ty {
                diags.push(disagreement(earlier, has, family, names));
            }
        }
    }

    pub fn get(&self, family: Ty, name: Sym) -> Option<Has> {
        self.declared.get(&(family, name)).copied()
    }

    /// The declared property names of one family.
    pub fn names(&self, family: Ty) -> impl Iterator<Item = Sym> {
        self.declared.keys().filter(move |key| key.0 == family).map(|key| key.1)
    }
}

fn disagreement(first: Has, again: Has, family: Ty, names: &Interner) -> Diagnostic {
    let name = names.name(again.name);
    let noun = family.word();
    let mut diagnostic = Diagnostic::error(
        "property-type",
        format!(
            "`{name}` is declared as {} here, but as {} elsewhere",
            crate::errors::article(again.ty.word()),
            crate::errors::article(first.ty.word())
        ),
    );
    if let Some(loc) = again.loc {
        diagnostic = diagnostic.label(loc, format!("{} here", again.ty.word()));
    }
    if let Some(loc) = first.loc {
        diagnostic = diagnostic.context(loc, format!("{} here", first.ty.word()));
    }
    diagnostic.note(format!(
        "a property has one type for every {noun}, so that `.{name}` means the same thing wherever it is written"
    ))
}
