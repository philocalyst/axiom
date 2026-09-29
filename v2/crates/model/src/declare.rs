//! Declaration: everything that has a name is created, and nothing yet says
//! what it holds.
//!
//! Kinds, commodities, entities and places exist after this, with their kinds
//! and paths. Their properties, and the names written in laws and in the
//! journal, are resolved against them later.

use axiom_core::diag::closest;
use axiom_core::{Arena, Diagnostic, Groups, Id, Interner, Loc, Map, Set, Sym, Tree};
use axiom_syntax::{DeclKind, ExprKind, Setting};

use crate::book::{Book, Class, Commodity, Entity, Kind, Lookup, Place, Roots, Sort};
use crate::collect::{Entry, Seen, Surveyed, Written, class_of, decls};
use crate::cx::Cx;
use crate::errors::{Word, duplicate, list_and, unknown};
use crate::kinds::{self, Kinds};
use crate::law::Rules;
use crate::names::{Names, Rank, Scoped, edits};
use crate::paths;
use crate::props::PropTable;
use crate::scope::{Home, Scopes};
use crate::sources::SystemIndex;

/// Quanta are `i64`; eighteen decimals is as fine as one can count.
pub(crate) const MAX_SCALE: u8 = 18;

/// `?` in a flow: where value of unknown origin comes from and unexplained
/// value goes.
const UNKNOWN: &str = "equity/unknown";
/// Where `opening` balances come from.
const OPENING: &str = "equity/opening";
/// The built-in place of kind `market`: `via market` on an assertion.
const MARKET: &str = "income/market";

/// The book under construction, with what only the build needs.
pub(crate) struct World<'s> {
    pub book: Book<'s>,
    pub scopes: Scopes,
    pub systems: SystemIndex<'s>,
    pub props: PropTable,
    /// The id of everything declared, in the order written.
    pub declared: Declared,
    /// Names some law counts.
    pub tallies: Set<&'s str>,
    /// The order each place was declared in; places opened by use come last.
    pub ordinal: Vec<u32>,
    /// Where the lines that say when a place is open and what it holds were
    /// written (`opened`, `closed`, `holds`), for the errors that enforce them.
    pub lines: Map<(Id<Place>, &'static str), Loc>,
}

pub(crate) struct Declared {
    pub kinds: Vec<Id<Kind>>,
    /// Indexed by kind: its parent chain is broken, and said so once.
    pub unrooted: Vec<bool>,
    pub commodities: Vec<Id<Commodity>>,
    pub entities: Vec<Id<Entity>>,
    pub places: Vec<Option<Id<Place>>>,
}

/// The project-wide directives.
pub(crate) struct Settings<'s> {
    pub base: Option<Word<'s>>,
    pub relaxed: bool,
    pub layout_free: bool,
}

pub(crate) fn settings<'s>(entries: &[Entry<'_, 's>], diags: &mut Vec<Diagnostic>) -> Settings<'s> {
    let mut settings = Settings { base: None, relaxed: false, layout_free: false };
    for entry in entries {
        let Entry::Setting(site, setting) = entry else {
            continue;
        };
        match *setting {
            Setting::Base(name) => {
                let word = Word { text: name.0, loc: site.source.file.loc(name.0) };
                match settings.base {
                    Some(first) if first.text != word.text => diags.push(
                        Diagnostic::error("duplicate-base", "the base currency is set twice")
                            .label(word.loc, format!("`base {}` here", word.text))
                            .context(first.loc, "and here")
                            .help("a book has one base currency: remove one of them"),
                    ),
                    Some(_) => {}
                    None => settings.base = Some(word),
                }
            }
            Setting::Relaxed => settings.relaxed = true,
            Setting::LayoutFree => settings.layout_free = true,
            Setting::System(_) | Setting::Use(_) => {}
        }
    }
    settings
}

/// What each home has brought into scope: its `use` lines, the system of every
/// `lives` line (which implies a `use`), and `std` for everyone.
pub(crate) fn scopes(
    entries: &[Entry],
    systems: &SystemIndex,
    tree: &Tree<crate::book::System>,
    diags: &mut Vec<Diagnostic>,
) -> Scopes {
    let mut used: Vec<(Home, Id<crate::book::System>)> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Setting(site, Setting::Use(name)) => match systems.find(name.0) {
                Some(system) => used.push((site.home, system)),
                None => diags.push(systems.unknown(Word { text: name.0, loc: site.source.file.loc(name.0) })),
            },
            Entry::Decl(written) if written.node.what == DeclKind::Entity => {
                let file = written.file();
                for prop in file[written.node.props].iter().filter(|prop| prop.name.0 == "lives") {
                    let named = file[prop.args].first().and_then(|&arg| match file.exprs[arg].kind {
                        ExprKind::Name(path) => systems.find(path.0),
                        _ => None,
                    });
                    used.extend(named.map(|system| (written.home(), system)));
                }
            }
            _ => {}
        }
    }
    let std = systems.find("std");
    Scopes::new(tree, |home| {
        let own = used.iter().filter(|&&(user, _)| user == home).map(|&(_, system)| system);
        own.chain(std).collect()
    })
}

pub(crate) fn declare<'a, 's>(
    surveyed: &Surveyed<'a, 's>,
    settings: &Settings<'s>,
    mut names: Interner<'s>,
    tree: Tree<crate::book::System>,
    systems: SystemIndex<'s>,
    scopes: Scopes,
    diags: &mut Vec<Diagnostic>,
) -> World<'s> {
    let mut cx = Cx { names: &mut names, systems: &tree, scopes: &scopes, diags };
    let entries = &surveyed.entries;
    let kinds = kinds::declare(entries, &mut cx);
    let commodities = commodities(entries, &surveyed.units, settings, &kinds, &mut cx);
    let entities = entities(entries, &kinds, &mut cx);
    let places = places(entries, &surveyed.paths, &kinds, &entities, &mut cx);

    let roots = Roots {
        me: entities.me,
        unknown: places.unknown,
        opening: places.opening,
        market: kinds.roots.market(),
        asset: kinds.roots.of_class(Class::Asset),
        liability: kinds.roots.of_class(Class::Liability),
        income: kinds.roots.of_class(Class::Income),
        expense: kinds.roots.of_class(Class::Expense),
        equity: kinds.roots.of_class(Class::Equity),
        commodity: kinds.roots.commodity(),
        entity: kinds.roots.entity(),
    };
    let book = Book {
        names,
        base: commodities.base,
        relaxed: settings.relaxed,
        roots,
        places: places.tree,
        entities: entities.tree,
        kinds: kinds.tree,
        systems: tree,
        commodities: commodities.arena,
        laws: Arena::new(),
        rules: Rules::default(),
        params: Arena::new(),
        schedules: Arena::new(),
        codes: Vec::new(),
        txns: Arena::new(),
        flows: Arena::new(),
        touching: Groups::default(),
        asserts: Vec::new(),
        events: Vec::new(),
        prices: Default::default(),
        splits: Vec::new(),
        plans: Arena::new(),
        syncs: Vec::new(),
        lookup: Lookup {
            places: places.names,
            entities: entities.index,
            kinds: kinds.index,
            params: Scoped::default(),
            laws: Names::default(),
            commodities: commodities.by_symbol,
        },
    };
    let declared = Declared {
        kinds: kinds.declared,
        unrooted: kinds.unrooted,
        commodities: commodities.declared,
        entities: entities.declared,
        places: places.declared,
    };
    World {
        book,
        scopes,
        systems,
        props: PropTable::default(),
        declared,
        tallies: Set::default(),
        ordinal: places.ordinal,
        lines: Map::default(),
    }
}

// ─── Commodities ────────────────────────────────────────────────────────────

struct Commodities {
    arena: Arena<Commodity>,
    by_symbol: Map<Sym, Id<Commodity>>,
    /// Where each was written, to say who declared it first.
    homes: Vec<Home>,
    declared: Vec<Id<Commodity>>,
    base: Id<Commodity>,
}

impl Commodities {
    fn find(&self, names: &Interner, symbol: &str) -> Option<Id<Commodity>> {
        names.get(symbol).and_then(|sym| self.by_symbol.get(&sym).copied())
    }

    fn add<'s>(
        &mut self,
        names: &mut Interner<'s>,
        symbol: &'s str,
        scale: u8,
        kind: Id<Kind>,
        home: Home,
    ) -> Id<Commodity> {
        let symbol = names.intern(symbol);
        let commodity = Commodity {
            symbol,
            kind,
            scale: scale.min(MAX_SCALE),
            title: None,
            liquidity: None,
            growth: None,
            props: Box::default(),
            doc: None,
            loc: None,
        };
        self.homes.push(home);
        let id = self.arena.push(commodity);
        self.by_symbol.insert(symbol, id);
        id
    }

    /// Whether `symbol` is a typo of a declared commodity, and which.
    fn typo_of<'a>(&self, names: &'a Interner, symbol: &str) -> Option<&'a str> {
        typo_of(symbol, self.by_symbol.keys().map(|&sym| names.name(sym)))
    }
}

/// The one of `known` that `symbol` is a few letters away from, if any.
fn typo_of<'a>(symbol: &str, known: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (symbol.len() / 3).max(1);
    known.map(|other| (edits(symbol, other), other)).filter(|&(distance, _)| distance <= limit).min().map(|(_, o)| o)
}

/// Commodities are declared, or opened on first use, unless what is written is
/// a near miss of a declared one: that is a typo, and nothing is invented for
/// it. A commodity's precision is what its declaration says, or else the most
/// decimals any amount written in it uses anywhere in the sources.
fn commodities<'s>(
    entries: &[Entry<'_, 's>],
    units: &[Seen<'s>],
    settings: &Settings<'s>,
    kinds: &Kinds,
    cx: &mut Cx<'_, 's>,
) -> Commodities {
    let precision: Map<&str, u8> = units.iter().map(|unit| (unit.symbol, unit.places)).collect();
    let scale_of = |symbol: &str| precision.get(symbol).copied().unwrap_or(0);
    let mut table = Commodities {
        arena: Arena::new(),
        by_symbol: Map::default(),
        homes: Vec::new(),
        declared: Vec::new(),
        base: Id::new(0),
    };
    for written in decls(entries, DeclKind::Commodity) {
        let (file, symbol) = (written.file(), written.node.name.0);
        let word = Word { text: symbol, loc: file.loc(symbol) };
        if let Some(first) = table.find(cx.names, symbol) {
            let system = system_of(table.homes[first.index()], written.home(), cx);
            cx.diags.push(duplicate("commodity", word, table.arena[first].loc, system));
            table.declared.push(first);
            continue;
        }
        let kind_word = written.node.kind.map(|kind| Word { text: kind.0, loc: file.loc(kind.0) });
        let thing = format!("the commodity `{symbol}`");
        let kind = kinds.declared(kind_word, written.home(), Sort::Commodity, &thing, cx);
        let id = table.add(cx.names, symbol, scale_of(symbol), kind, written.home());
        table.arena[id].doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
        table.arena[id].loc = Some(word.loc);
        table.declared.push(id);
    }
    // What the sources declare, and the currency they name as the base, are
    // what a stray symbol may be a typo of; two undeclared symbols are equals.
    let root = kinds.roots.commodity();
    let named = settings.base.map(|word| word.text);
    let known: Vec<&str> = table.by_symbol.keys().map(|&sym| cx.names.name(sym)).chain(named).collect();
    for unit in units {
        let declared = table.find(cx.names, unit.symbol).is_some();
        if declared {
            continue;
        }
        if named == Some(unit.symbol) || typo_of(unit.symbol, known.iter().copied()).is_none() {
            table.add(cx.names, unit.symbol, unit.places, root, Home::Builtin);
        }
    }
    table.base = base(&mut table, settings, units, kinds, cx);
    table
}

/// The book's base currency: the one `base` says, or else the one currency
/// the journal uses.
fn base<'s>(
    table: &mut Commodities,
    settings: &Settings<'s>,
    units: &[Seen<'s>],
    kinds: &Kinds,
    cx: &mut Cx<'_, 's>,
) -> Id<Commodity> {
    let root = kinds.roots.commodity();
    if let Some(word) = settings.base {
        if let Some(id) = table.find(cx.names, word.text) {
            return id;
        }
        match table.typo_of(cx.names, word.text) {
            None => return table.add(cx.names, word.text, 0, root, Home::Builtin),
            Some(near) => cx.diags.push(unknown("unknown-commodity", "commodity", word, Some(near))),
        }
    }
    // `currency` is a convention of the standard kinds, as `person` is.
    let currency = cx.names.get("currency");
    let is_currency = |id: Id<Commodity>| {
        let mut chain = kinds.tree.lineage(table.arena[id].kind);
        chain.any(|kind| Some(kinds.tree[kind].name) == currency)
    };
    let used: Vec<(&Seen, Id<Commodity>)> = units
        .iter()
        .filter(|unit| unit.journal)
        .filter_map(|unit| Some((unit, table.find(cx.names, unit.symbol)?)))
        .collect();
    let currencies: Vec<_> = used.iter().copied().filter(|&(_, id)| is_currency(id)).collect();
    let candidates = if currencies.is_empty() { used } else { currencies };
    match candidates.as_slice() {
        [] => table.find(cx.names, "USD").unwrap_or_else(|| table.add(cx.names, "USD", 0, root, Home::Builtin)),
        [(_, only)] => *only,
        [(first, id), (second, _), ..] => {
            let all: Vec<&str> = candidates.iter().map(|(unit, _)| unit.symbol).collect();
            cx.diags.push(
                Diagnostic::error(
                    "no-base",
                    format!("the journal uses {}, and no `base` says which one values the others", list_and(&all)),
                )
                .label(second.first, format!("{} is a second currency", second.symbol))
                .context(first.first, format!("{} is first written here", first.symbol))
                .help(format!("declare the base currency, for example `base {}`", first.symbol)),
            );
            *id
        }
    }
}

/// The system that declared what `first` was written in, when a project's
/// declaration repeats it.
fn system_of<'s>(first: Home, again: Home, cx: &Cx<'_, 's>) -> Option<&'s str> {
    match (first, again) {
        (Home::System(system), Home::Project) => Some(cx.names.name(cx.systems[system].path)),
        _ => None,
    }
}

// ─── Entities ───────────────────────────────────────────────────────────────

struct Entities {
    tree: Tree<Entity>,
    index: Scoped<Entity>,
    me: Id<Entity>,
    declared: Vec<Id<Entity>>,
}

/// Entities form a path tree (`paypal/john` sits under `paypal`), with `me`
/// always present.
fn entities<'s>(entries: &[Entry<'_, 's>], kinds: &Kinds, cx: &mut Cx<'_, 's>) -> Entities {
    let written = decls(entries, DeclKind::Entity).map(|written| written.node.name.0).chain(["me"]);
    let (mut tree, by_path) = paths::build(written, |path| Entity {
        path: cx.names.intern(path),
        kind: kinds.roots.entity(),
        via: None,
        restricted: false,
        lives: Box::default(),
        member: None,
        props: Box::default(),
        doc: None,
        loc: None,
    });
    let mut homes = vec![Home::Builtin; tree.len()];
    let mut declared = Vec::new();
    for written in decls(entries, DeclKind::Entity) {
        let (file, path) = (written.file(), written.node.name.0);
        let id = by_path[path];
        declared.push(id);
        if let Some(first) = tree[id].loc {
            let system = system_of(homes[id.index()], written.home(), cx);
            cx.diags.push(duplicate("entity", Word { text: path, loc: file.loc(path) }, Some(first), system));
            continue;
        }
        homes[id.index()] = written.home();
        let kind_word = written.node.kind.map(|kind| Word { text: kind.0, loc: file.loc(kind.0) });
        let kind = kinds.declared(kind_word, written.home(), Sort::Entity, &format!("the entity `{path}`"), cx);
        let entity = &mut tree[id];
        entity.kind = kind;
        entity.doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
        entity.loc = Some(file.loc(path));
    }
    let me = by_path["me"];
    if tree[me].loc.is_none() {
        tree[me].kind = person_or_root(kinds, cx);
    }
    let things: Vec<_> = tree.iter().map(|(id, entity)| (id, cx.names.name(entity.path), homes[id.index()])).collect();
    Entities { index: Scoped::build(cx.names, things), tree, me, declared }
}

/// `me` is a `person` when the standard kinds are in scope.
fn person_or_root(kinds: &Kinds, cx: &Cx) -> Id<Kind> {
    let scope = cx.scopes.of(Home::Project);
    match kinds::find(&kinds.index, cx.names, cx.systems, "person", |home| scope.sees(home)) {
        Ok(person) if kinds.tree[person].sort == Sort::Entity => person,
        _ => kinds.roots.entity(),
    }
}

// ─── Places ─────────────────────────────────────────────────────────────────

struct Places {
    tree: Tree<Place>,
    names: Names<Place>,
    unknown: Id<Place>,
    opening: Id<Place>,
    declared: Vec<Option<Id<Place>>>,
    ordinal: Vec<u32>,
}

/// The five class roots and the built-in places always exist. Declared
/// accounts add to them, and so does any full path under a class root that is
/// written anywhere: writing `expenses/food/snacks` opens it, unless it is one
/// letter from a declared place, which is a typo and is left unopened.
fn places<'s>(
    entries: &[Entry<'_, 's>],
    opened: &[&'s str],
    kinds: &Kinds,
    entities: &Entities,
    cx: &mut Cx<'_, 's>,
) -> Places {
    let accounts: Vec<Written<_>> = decls(entries, DeclKind::Account).collect();
    // An account outside every class root has no class, so it cannot exist.
    let valid: Vec<bool> = accounts.iter().map(|written| root_is_valid(written, cx)).collect();
    let declared_paths: Vec<&str> = Class::ALL
        .map(Class::root)
        .into_iter()
        .chain([UNKNOWN, OPENING, MARKET])
        .chain(accounts.iter().zip(&valid).filter(|&(_, &ok)| ok).map(|(written, _)| written.node.name.0))
        .collect();
    let declared_set: Set<&str> = declared_paths.iter().copied().collect();
    let opens =
        opened.iter().copied().filter(|path| declared_set.contains(path) || !typo_of_place(path, &declared_paths));
    let (mut tree, by_path) = paths::build(declared_paths.iter().copied().chain(opens), |path| {
        let class = class_of(path).expect("only paths under a class root are written");
        Place {
            path: cx.names.intern(path),
            class,
            kind: kinds.roots.of_class(class),
            owner: entities.me,
            holds: None,
            select: None,
            deferred: false,
            basis: Default::default(),
            claim: false,
            liquidity: None,
            alias: None,
            opened: None,
            closed: None,
            props: Box::default(),
            doc: None,
            loc: None,
        }
    });
    tree[by_path[MARKET]].kind = kinds.roots.market();

    let mut declared = vec![None; accounts.len()];
    let mut ordinal = vec![u32::MAX; tree.len()];
    let mut aliases: Map<&str, Id<Place>> = Map::default();
    for (at, written) in accounts.iter().enumerate().filter(|&(at, _)| valid[at]) {
        let (file, decl) = (written.file(), written.node);
        let word = Word { text: decl.name.0, loc: file.loc(decl.name.0) };
        let id = by_path[decl.name.0];
        declared[at] = Some(id);
        if let Some(first) = tree[id].loc {
            cx.diags.push(duplicate("account", word, Some(first), None));
            continue;
        }
        ordinal[id.index()] = at as u32;
        let class = tree[id].class;
        let thing = format!("the {} account `{}`", Sort::Place(class).noun(), decl.name.0);
        let kind_word = decl.kind.map(|kind| Word { text: kind.0, loc: file.loc(kind.0) });
        let kind = kinds.declared(kind_word, Home::Project, Sort::Place(class), &thing, cx);
        let place = &mut tree[id];
        place.kind = kind;
        place.doc = written.item.doc.map(|doc| cx.names.intern(doc.0));
        place.loc = Some(word.loc);
        if let Some(alias) = decl.alias {
            match aliases.get(alias.0) {
                Some(&first) => {
                    let loc = file.loc(alias.0);
                    let named = cx.names.name(tree[first].path);
                    cx.diags.push(
                        Diagnostic::error(
                            "duplicate-declaration",
                            format!("`{}` is already the alias of `{named}`", alias.0),
                        )
                        .label(loc, "an alias names one account")
                        .help("choose another alias"),
                    );
                }
                None => {
                    aliases.insert(alias.0, id);
                    tree[id].alias = Some(cx.names.intern(alias.0));
                }
            }
        }
    }

    let mut names = Names::default();
    let paths: Vec<(Id<Place>, &str)> = tree.iter().map(|(id, place)| (id, cx.names.name(place.path))).collect();
    for (id, path) in paths {
        names.insert_path(cx.names, path, id);
    }
    for (&alias, &id) in &aliases {
        names.insert(cx.names, alias, Rank::Alias, id);
    }
    Places { unknown: by_path[UNKNOWN], opening: by_path[OPENING], tree, names, declared, ordinal }
}

/// Whether a full path that no account declares is a typo of one that does.
pub(crate) fn typo_of_place(path: &str, declared: &[&str]) -> bool {
    near_place(path, declared).is_some()
}

/// The declared place a full path is a typo of, if any.
pub(crate) fn near_place<'a>(path: &str, declared: &[&'a str]) -> Option<&'a str> {
    let limit = (path.len() / 8).clamp(1, 2);
    let root = path.split('/').next();
    declared
        .iter()
        .copied()
        .filter(|other| other.len().abs_diff(path.len()) <= limit && other.split('/').next() == root)
        .map(|other| (edits(path, other), other))
        .filter(|&(distance, _)| distance <= limit)
        .min()
        .map(|(_, other)| other)
}

/// Whether `decl` starts at a class root; if not, says so.
fn root_is_valid(written: &Written<axiom_syntax::Decl>, cx: &mut Cx) -> bool {
    let (file, path) = (written.file(), written.node.name.0);
    if class_of(path).is_some() {
        return true;
    }
    let roots = Class::ALL.map(Class::root);
    let loc = file.loc(path);
    let mut diagnostic = Diagnostic::error("account-root", format!("`{path}` does not start at a class root"))
        .label(loc, "an account belongs to one of the five classes")
        .note("an account's path starts with `assets`, `liabilities`, `income`, `expenses` or `equity`, and that root is its class");
    let first = path.split('/').next().unwrap_or(path);
    if let Some(near) = closest(first, roots) {
        let fixed = format!("{near}{}", &path[first.len()..]);
        diagnostic = diagnostic.fix(format!("did you mean `{fixed}`?"), loc, fixed);
    }
    cx.diags.push(diagnostic);
    false
}
