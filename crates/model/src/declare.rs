//! Native S5 declarations: create the immutable Book trees and name indexes.
//!
//! This pass borrows source AST nodes directly. It creates kinds and purposes
//! first so forward references from entities, commodities and accounts resolve
//! against complete scoped indexes, then freezes the place tree once.

use axiom_core::facts::Builder;
use axiom_core::tagless::{Datum, Field};
use axiom_core::{
    Arena, Days, Diagnostic, Facts, Groups, Id, Interner, Key, Loc, Many, Map, Ratio, Set, SlotId, Sym, Tree,
};
use axiom_syntax::{Change, Decl, DeclKind, ExprKind, Setting, Verb};

use crate::book::{Book, Class, Entity, Kind, KindRoots, Lookup, Place, Purpose, Roots, Share, Sort, System};
use crate::builtin;
use crate::collect::{Collected, Order, Written};
use crate::errors::Word;
use crate::holders::{Holder, HolderIndex};
use crate::names::Scoped;
use crate::problem::{self, Among, Noun};
use crate::resolve::End;
use crate::scope::{Home, Scopes, Seeing};
use crate::sources::{Site, SystemIndex};
use crate::taxonomy::{self, Taxonomy};
use crate::{kinds, purposes};

/// Quanta are `i64`; eighteen decimals is as fine as one can count.
pub(crate) const MAX_SCALE: u8 = 18;

/// The book under construction, with the indexes needed by native lowerers.
pub(crate) struct World<'s> {
    pub book: Book<'s>,
    pub scopes: Scopes,
    pub systems: SystemIndex<'s>,
    /// Everything said of the things so far: frozen into the book once it is all said.
    pub painter: Builder,
    pub tallies: Set<&'s str>,
    /// Claim tabs allocated from the bounded syntax survey before place IDs
    /// freeze. A later lookup that was not surveyed is an error.
    tabs: Tabs,
    /// Loan contract names resolve to their actual debt tab, before and after
    /// contract terms have been compiled.
    pub(crate) contract_endpoints: Map<Sym, End>,
}

impl World<'_> {
    /// `value` holds of `thing`'s slot over `days`: a statement of the facts, painted in the order made.
    pub(crate) fn paint(&mut self, thing: impl Into<Holder>, slot: SlotId, days: Days, value: Datum) {
        self.painter.paint_datum(self.book.holders.number(thing), slot, days, value);
    }

    /// What a line of the language says of `thing`, from the beginning of time: a declaration of one of its own slots.
    pub(crate) fn say<V: Field>(&mut self, thing: impl Into<Holder>, key: Key<V>, value: V) {
        self.painter.paint_always(self.book.holders.number(thing), key, value);
    }

    /// Where a line of the language is written, for a diagnostic that points back to it: of a thing's slot, and for a
    /// slot of several of the member the line is about.
    pub(crate) fn say_site(&mut self, thing: impl Into<Holder>, slot: SlotId, member: u32, loc: Loc) {
        self.book.sites.insert((self.book.holders.number(thing), slot.0, member), loc);
    }

    /// What a line of the language says of `thing`: the whole set a slot of several holds.
    pub(crate) fn say_set<V: Field>(
        &mut self,
        thing: impl Into<Holder>,
        key: Key<Many<V>>,
        members: impl IntoIterator<Item = V>,
    ) {
        self.painter.paint_many(self.book.holders.number(thing), key, Days::ALWAYS, members);
    }

    /// What a line of the language says of `thing` over `days`: the set a slot of several holds then.
    pub(crate) fn say_set_over<V: Field>(
        &mut self,
        thing: impl Into<Holder>,
        key: Key<Many<V>>,
        days: Days,
        members: impl IntoIterator<Item = V>,
    ) {
        self.painter.paint_many(self.book.holders.number(thing), key, days, members);
    }

    /// The set of `members` holds of `thing`'s slot over `days`.
    pub(crate) fn paint_set(&mut self, thing: impl Into<Holder>, slot: SlotId, days: Days, members: Vec<Datum>) {
        self.painter.paint_set_datum(self.book.holders.number(thing), slot, days, members);
    }

    /// The facts, frozen from everything painted: the book says no more of its things after this.
    pub(crate) fn freeze_facts(&mut self) {
        self.book.facts = self.painter.freeze();
    }

    pub(crate) fn tab(
        &self,
        party: Id<Entity>,
        owner: Id<Entity>,
        class: Class,
        loc: Loc,
    ) -> Result<Id<Place>, Diagnostic> {
        self.tabs.get(&(party, owner, class)).copied().ok_or_else(|| {
            Diagnostic::error("unregistered-tab", "this claim tab was not found during the declaration survey")
                .label(loc, "a claim relationship must be visible before the place tree is frozen")
                .help("check that the party, owner and flow direction match the claim or contract declaration")
        })
    }
}

pub(crate) struct Settings<'s> {
    pub base: Option<Word<'s>>,
    pub relaxed: bool,
}

pub(crate) fn settings<'s>(collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) -> Settings<'s> {
    let mut settings = Settings { base: None, relaxed: false };
    for written in &collected.settings {
        match *written.node {
            Setting::Base(name) => {
                let word = Word::of(written.file(), name.0);
                match settings.base {
                    Some(first) if first.text != word.text => {
                        diags.push(problem::twice("base currency", word.loc, first.loc))
                    }
                    Some(_) => {}
                    None => settings.base = Some(word),
                }
            }
            Setting::Relaxed => settings.relaxed = true,
            Setting::System(_) | Setting::Use(_) | Setting::Currency(_) | Setting::Rates(_) => {}
        }
    }
    settings
}

/// What each home has brought into scope: its `use` lines, the system of every
/// `lives` line (which implies a `use`), and `std` for everyone.
pub(crate) fn scopes(
    collected: &Collected,
    systems: &SystemIndex,
    tree: &Tree<System>,
    diags: &mut Vec<Diagnostic>,
) -> Scopes {
    let mut used: Vec<(Home, Id<System>)> = Vec::new();
    for written in &collected.settings {
        if let Setting::Use(name) = *written.node {
            match systems.find(name.0) {
                Some(system) => used.push((written.home(), system)),
                None => diags.push(systems.unknown(Word::of(written.file(), name.0))),
            }
        }
    }
    for written in collected.decls_of(DeclKind::Entity) {
        let file = written.file();
        for prop in file[written.node.props].iter().filter(|prop| prop.name.0 == "lives") {
            add_lives(&mut used, written.home(), systems, file, prop);
        }
    }
    for written in &collected.statements {
        if let Verb::Now(Change::Property(prop)) = &written.node.verb
            && prop.name.0 == "lives"
        {
            add_lives(&mut used, written.home(), systems, written.file(), prop);
        }
    }
    let std = systems.find("std");
    Scopes::new(tree, |home| {
        let own = used.iter().filter(|&&(user, _)| user == home).map(|&(_, system)| system);
        own.chain(std).collect()
    })
}

fn add_lives(
    used: &mut Vec<(Home, Id<System>)>,
    home: Home,
    systems: &SystemIndex<'_>,
    file: &axiom_syntax::File<'_>,
    prop: &axiom_syntax::Prop<'_>,
) {
    for &arg in &file[prop.args] {
        if let ExprKind::Name(path) = file.exprs[arg].kind
            && let Some(system) = systems.find(path.0)
        {
            used.push((home, system));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NameSpace {
    Account,
    Entity,
    Asset,
    Contract,
    Purpose,
    Commodity,
}

impl NameSpace {
    /// The namespace a declaration's name lives in; a kind's is its own.
    fn of(what: DeclKind) -> Option<NameSpace> {
        match what {
            DeclKind::Account => Some(NameSpace::Account),
            DeclKind::Entity => Some(NameSpace::Entity),
            DeclKind::Asset => Some(NameSpace::Asset),
            DeclKind::Purpose => Some(NameSpace::Purpose),
            DeclKind::Commodity => Some(NameSpace::Commodity),
            DeclKind::Kind => None,
        }
    }

    /// Whether a `/`-path in it answers to each of its suffixes.
    fn has_suffixes(self) -> bool {
        !matches!(self, NameSpace::Commodity | NameSpace::Contract)
    }

    fn article(self) -> &'static str {
        match self {
            NameSpace::Account => "account",
            NameSpace::Entity => "entity",
            NameSpace::Asset => "asset",
            NameSpace::Contract => "contract",
            NameSpace::Purpose => "purpose",
            NameSpace::Commodity => "unit",
        }
    }

    fn indefinite(self) -> &'static str {
        match self {
            NameSpace::Account => "an account",
            NameSpace::Entity => "an entity",
            NameSpace::Asset => "an asset",
            NameSpace::Contract => "a contract",
            NameSpace::Purpose => "a purpose",
            NameSpace::Commodity => "a unit",
        }
    }
}

struct NameClaim<'s> {
    spelling: &'s str,
    declared: &'s str,
    space: NameSpace,
    home: Home,
    loc: Loc,
    order: Order,
    /// Contracts may take the spelling of their party, but no other entity.
    contract_party: Option<&'s str>,
}

/// Every spelling a declaration answers to, for the declarations that share one namespace-free name table.
fn name_claims<'s>(collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) -> Vec<NameClaim<'s>> {
    let mut claims = Vec::new();
    for written in &collected.decls {
        let (file, decl) = (written.file(), written.node);
        if decl.what == DeclKind::Entity && matches!(decl.name.0, "opening" | "market" | "?") {
            diags.push(
                Diagnostic::error(
                    "reserved-entity-name",
                    format!("`{}` is reserved for a built-in entity", decl.name.0),
                )
                .label(file.loc(decl.name.0), "choose a different entity name"),
            );
        }
        if let Some(space) = NameSpace::of(decl.what) {
            push_name_claims(&mut claims, written, decl.name.0, space, None);
        }
    }
    for written in &collected.contracts {
        let contract = written.node;
        let party = Some(contract.party.unwrap_or(contract.name).0);
        push_name_claims(&mut claims, written, contract.name.0, NameSpace::Contract, party);
    }
    claims
}

fn check_cross_namespace_names(
    collected: &Collected,
    scopes: &Scopes,
    systems: &Tree<System>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut claims = name_claims(collected, diags);
    claims.sort_unstable_by(|a, b| a.spelling.cmp(b.spelling).then(a.order.cmp(&b.order)).then(a.space.cmp(&b.space)));

    let mut start = 0;
    while start < claims.len() {
        let spelling = claims[start].spelling;
        let mut end = start + 1;
        while end < claims.len() && claims[end].spelling == spelling {
            end += 1;
        }
        for index in start..end {
            let later = &claims[index];
            let mut earlier = Vec::new();
            for first in &claims[start..index] {
                if first.space == later.space
                    || !visible_in_any_scope(scopes, systems, first.home, later.home)
                    || contract_party_name_exception(first, later, spelling)
                {
                    continue;
                }
                earlier.push(first);
            }
            if earlier.is_empty() {
                continue;
            }
            let mut diagnostic = Diagnostic::error(
                "ambiguous-name",
                format!(
                    "`{spelling}` answers to both {} and {}",
                    earlier[0].space.indefinite(),
                    later.space.indefinite()
                ),
            )
            .label(later.loc, format!("this {} also answers to `{spelling}`", later.space.article()));
            for first in earlier {
                diagnostic = diagnostic
                    .context(first.loc, format!("{} `{}` was declared earlier", first.space.article(), first.declared));
            }
            diagnostic = diagnostic.help("rename one declaration or use a different account path");
            diags.push(diagnostic);
        }
        start = end;
    }
}

fn push_name_claims<'s, T>(
    claims: &mut Vec<NameClaim<'s>>,
    written: &Written<'_, 's, T>,
    declared: &'s str,
    space: NameSpace,
    contract_party: Option<&'s str>,
) {
    let (home, loc, order) = (written.home(), written.file().loc(declared), written.order);
    let mut push = |spelling| {
        claims.push(NameClaim { spelling, declared, space, home, loc, order, contract_party });
    };
    push(declared);
    if space.has_suffixes() {
        for (at, _) in declared.match_indices('/') {
            push(&declared[at + 1..]);
        }
    }
}

fn visible_in_any_scope(scopes: &Scopes, systems: &Tree<System>, a: Home, b: Home) -> bool {
    let sees_both = |home| {
        let scope = scopes.of(home);
        scope.sees(a) && scope.sees(b)
    };
    sees_both(Home::Project) || systems.ids().any(|system| sees_both(Home::System(system)))
}

fn contract_party_name_exception(a: &NameClaim<'_>, b: &NameClaim<'_>, spelling: &str) -> bool {
    let (contract, entity) = match (a.space, b.space) {
        (NameSpace::Contract, NameSpace::Entity) => (a, b),
        (NameSpace::Entity, NameSpace::Contract) => (b, a),
        _ => return false,
    };
    contract.declared == spelling && contract.contract_party == Some(spelling) && entity.declared == spelling
}

/// The place that keeps what one party owes another, by the party, the owner and the class of the claim.
pub(crate) type Tabs = Map<(Id<Entity>, Id<Entity>, Class), Id<Place>>;

/// What the sources say, three ways: in the order they are written, by kind of item, and as the mentions the
/// journal makes of parties and ends.
#[derive(Clone, Copy)]
pub(crate) struct Said<'c, 'a, 's> {
    pub sites: &'c [Site<'a, 's>],
    pub collected: &'c Collected<'a, 's>,
    pub survey: &'c crate::lower::JournalSurvey<'s>,
}

/// The systems of a build: as a tree, by what they define, and what each home of them sees.
pub(crate) struct Systems<'s> {
    pub tree: Tree<System>,
    pub index: SystemIndex<'s>,
    pub scopes: Scopes,
}

/// Construct a native v4 Book from the syntax sites and a small survey of only
/// claim-bearing journal relationships. No v3 chart-account collection is used.
pub(crate) fn declare<'a, 's>(
    said: Said<'_, 'a, 's>,
    settings: &Settings<'s>,
    mut names: Interner<'s>,
    systems: Systems<'s>,
    diags: &mut Vec<Diagnostic>,
) -> World<'s> {
    let Said { collected, survey, .. } = said;
    let Systems { tree: systems_tree, index: systems, scopes } = systems;
    let seeing = Seeing { systems: &systems_tree, scopes: &scopes };
    let native_kinds = taxonomy::declare::<Kind>(collected, &mut names, seeing, diags);
    let mut native_purposes = taxonomy::declare::<Purpose>(collected, &mut names, seeing, diags);
    purposes::attach_objects(&mut native_purposes, collected, &names, seeing, &native_kinds.index, diags);
    check_cross_namespace_names(collected, &scopes, &systems_tree, diags);
    let kind_roots = kinds::roots(&native_kinds.roots);
    let resolving = Resolving { seeing, kinds: &native_kinds, kind_roots, purposes: &native_purposes };

    let mut commodities = commodities::declare(collected, settings, &resolving, &mut names, diags);
    let parties = parties::find(said, &resolving, &commodities, &mut names, diags);
    let mut entities = parties::declare(collected, parties, &resolving, &mut names, diags);
    let accounts = holdings::declare_accounts(collected, &resolving, &entities, &names, diags);
    let mut assets = holdings::declare_assets(collected, &resolving, &entities, &mut commodities, &mut names, diags);
    let account_owners = holdings::owners_by_path(&accounts);
    let tabs = holdings::find_tabs(survey, &entities, &account_owners);
    let inputs =
        PlaceInputs { collected, resolving: &resolving, commodities: &commodities, accounts: &accounts, tabs: &tabs };
    let places = places::declare(&inputs, &mut entities, &mut assets, &mut names);
    let contract_endpoints = contract_endpoints(survey, &entities, &account_owners, &places.tabs, &mut names);

    let entity_purposes = std::mem::take(&mut entities.purposes);
    let made = Made { commodities, entities, assets, places, kinds: native_kinds, purposes: native_purposes };
    let tabs = made.places.tabs.clone();
    let book = book(made, names, systems_tree, settings);
    let painter = Facts::builder(book.holders.len());
    let mut world = World { book, scopes, systems, painter, tallies: Set::default(), tabs, contract_endpoints };
    // What an entity's own declaration says its purpose is, said as a line under it would.
    for (entity, purpose) in entity_purposes {
        world.say(entity, builtin::PURPOSE, purpose.value);
        world.say_site(entity, builtin::PURPOSE.slot(), 0, purpose.loc);
    }
    world
}

/// What the passes made, to be put together into a book.
struct Made<'s> {
    commodities: Commodities<'s>,
    entities: Entities<'s>,
    assets: Assets<'s>,
    places: Places,
    kinds: Taxonomy<Kind>,
    purposes: Taxonomy<Purpose>,
}

/// The book the passes made, empty of everything the lowerers will fill in.
fn book<'s>(made: Made<'s>, mut names: Interner<'s>, systems: Tree<System>, settings: &Settings<'s>) -> Book<'s> {
    let Made { commodities, entities, assets, places, kinds, purposes } = made;
    let holders = HolderIndex::new(
        kinds.tree.len(),
        places.tree.len(),
        entities.tree.len(),
        commodities.arena.len(),
        assets.arena.len(),
    );
    let roots = Roots {
        me: entities.me,
        unknown: entities.unknown,
        opening: entities.opening,
        market: entities.market,
        kinds: kinds::roots(&kinds.roots),
        purposes: purposes::roots(&purposes.roots),
    };
    let lookup = Lookup {
        places: places.names,
        entities: entities.index,
        kinds: kinds.index,
        purposes: purposes.index,
        assets: assets.by_name.into_iter().map(|(name, id)| (names.intern(name), id)).collect(),
        commodities: commodities.by_name.into_iter().map(|(name, id)| (names.intern(name), id)).collect(),
        ..Lookup::default()
    };
    Book {
        names,
        text_values: Arena::new(),
        base: commodities.base,
        relaxed: settings.relaxed,
        roots,
        places: places.tree,
        issuer_places: places.issuers,
        entities: entities.tree,
        kinds: kinds.tree,
        schema: Default::default(),
        holders,
        facts: Facts::default(),
        sites: Map::default(),
        purposes: purposes.tree,
        systems,
        commodities: commodities.arena,
        assets: assets.arena,
        contracts: Arena::new(),
        also: Arena::new(),
        laws: Arena::new(),
        rules: Default::default(),
        budgets: Arena::new(),
        params: Arena::new(),
        schedules: Arena::new(),
        code_rules: Vec::new(),
        codes: Arena::new(),
        selectors: Arena::new(),
        details: Arena::new(),
        patterns: Arena::new(),
        formats: Arena::new(),
        txns: Arena::new(),
        journal_programs: Arena::new(),
        assertion_programs: Arena::new(),
        written_occurrences: Arena::new(),
        input_values: Arena::new(),
        flows: Arena::new(),
        touching: Groups::default(),
        asserts: Vec::new(),
        events: Vec::new(),
        endings: Vec::new(),
        claim_changes: Vec::new(),
        prices: Default::default(),
        splits: Vec::new(),
        measures: Arena::new(),
        readings: Vec::new(),
        filed: Vec::new(),
        sources: Vec::new(),
        lookup,
    }
}

/// The debt tab a loan contract's name stands for, by the contract's name: the loan's party owes the owner
/// of the account the contract is paid from.
fn contract_endpoints<'s>(
    survey: &crate::lower::JournalSurvey<'s>,
    entities: &Entities<'s>,
    account_owners: &Map<&'s str, Id<Entity>>,
    tabs: &Tabs,
    names: &mut Interner<'s>,
) -> Map<Sym, End> {
    let mut endpoints = Map::default();
    for mention in &survey.mentions {
        let crate::lower::Mention::Promise { name, party, holding, loan_party: Some(_), .. } = *mention else {
            continue;
        };
        let Some(&party) = entities.ids.get(party.0) else {
            continue;
        };
        let owner = holding.and_then(|name| account_owners.get(name.0).copied()).unwrap_or(entities.me);
        if let Some(&place) = tabs.get(&(party, owner, Class::Debt)) {
            endpoints.insert(names.intern(name.0), End { place, entity: Some(party) });
        }
    }
    endpoints
}

fn path_key(path: &str) -> impl Iterator<Item = u8> + '_ {
    path.bytes().map(|byte| if byte == b'/' { 0 } else { byte })
}

fn add_path_spellings<'s>(spellings: &mut Set<&'s str>, path: &'s str) {
    for prefix in crate::paths::prefixes(path) {
        spellings.insert(prefix);
        for (at, _) in prefix.match_indices('/') {
            spellings.insert(&prefix[at + 1..]);
        }
    }
}

/// Find candidate paths shadowed by a longer path with the same final
/// component. Sorting by reversed spelling makes each suffix range contiguous,
/// avoiding a quadratic scan when a journal introduces many parties.
fn strict_path_suffixes<'s>(paths: &[&'s str]) -> Set<&'s str> {
    let mut reversed: Vec<&str> = paths.to_vec();
    reversed.sort_unstable_by(|left, right| left.bytes().rev().cmp(right.bytes().rev()));

    let mut shadowed = Set::default();
    for &path in paths {
        let after_prefix = reversed.partition_point(|candidate| {
            candidate.bytes().rev().cmp(path.bytes().rev().chain(std::iter::once(b'/'))).is_lt()
        });
        if let Some(&candidate) = reversed.get(after_prefix)
            && candidate != path
            && candidate.strip_suffix(path).is_some_and(|prefix| prefix.ends_with('/'))
        {
            shadowed.insert(path);
        }
    }
    shadowed
}

fn is_path_child(parent: &str, child: &str) -> bool {
    child.len() > parent.len()
        && child.as_bytes().starts_with(parent.as_bytes())
        && child.as_bytes().get(parent.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::strict_path_suffixes;

    #[test]
    fn implicit_party_suffixes_are_found_without_prefix_collisions() {
        let paths = ["acme", "vendors/acme", "archive/vendors/acme", "acme2", "other/acme2", "vendor/acme/branch"];
        let shadowed = strict_path_suffixes(&paths);

        assert!(shadowed.contains("acme"));
        assert!(shadowed.contains("vendors/acme"));
        assert!(shadowed.contains("acme2"));
        assert!(!shadowed.contains("other/acme2"));
        assert!(!shadowed.contains("vendor/acme/branch"));
    }
}

mod commodities;
mod holdings;
mod parties;
mod places;

use self::commodities::Commodities;
use self::holdings::Assets;
use self::parties::Entities;
use self::places::{PlaceInputs, Places};

/// What the words of a declaration are resolved against once the kinds and the purposes are built.
struct Resolving<'a> {
    seeing: Seeing<'a>,
    kinds: &'a Taxonomy<Kind>,
    kind_roots: KindRoots,
    purposes: &'a Taxonomy<Purpose>,
}

impl Resolving<'_> {
    /// The kind a declaration is written as, or `fallback` when it names none, or one of another sort than `expected`.
    fn kind(
        &self,
        names: &Interner,
        written: &Written<Decl>,
        expected: Sort,
        fallback: Id<Kind>,
        diags: &mut Vec<Diagnostic>,
    ) -> Id<Kind> {
        let Some(word) = written.node.kind else {
            return fallback;
        };
        let loc = written.file().loc(word.0);
        let (kinds, scope) = (self.kinds, self.seeing.scopes.of(written.home()));
        let kind = match kinds.index.resolve(names, scope, word.0) {
            Ok(kind) => kind,
            Err(miss) => {
                let among = Among { index: &kinds.index, names, systems: self.seeing.systems };
                diags.push(kinds::unresolved(miss, Word { text: word.0, loc }, &among, |id| &kinds.tree[id]));
                return fallback;
            }
        };
        let found = kinds.tree[kind].sort;
        let valid = match expected {
            Sort::Place(_) => matches!(found, Sort::Place(_)),
            other => found == other,
        };
        if valid {
            kind
        } else {
            diags.push(
                Diagnostic::error("kind-sort", format!("kind `{}` cannot classify this declaration", word.0))
                    .label(loc, format!("expected {expected:?}, found {found:?}")),
            );
            fallback
        }
    }

    /// The purpose `name` is, for a declaration in `home`.
    fn purpose(
        &self,
        names: &Interner,
        name: &str,
        loc: Loc,
        home: Home,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Id<Purpose>> {
        let (purposes, scope) = (self.purposes, self.seeing.scopes.of(home));
        match purposes.index.resolve(names, scope, name) {
            Ok(purpose) => Some(purpose),
            Err(miss) => {
                let among = Among { index: &purposes.index, names, systems: self.seeing.systems };
                let describe = |ids: &[Id<Purpose>]| {
                    problem::shortest(
                        names,
                        &purposes.index.names,
                        ids,
                        |id| purposes.tree[id].name,
                        |id| purposes.tree[id].loc,
                    )
                };
                diags.push(among.failed(miss, Noun::Purpose, Word { text: name, loc }, describe));
                None
            }
        }
    }
}

fn first_name_prop<'s>(file: &axiom_syntax::File<'s>, decl: &Decl<'s>, name: &str) -> Option<&'s str> {
    file[decl.props].iter().find(|prop| prop.name.0 == name).and_then(|prop| file[prop.args].first()).and_then(|&arg| {
        match file.exprs[arg].kind {
            ExprKind::Name(name) => Some(name.0),
            _ => None,
        }
    })
}

fn owner_names_in<'s>(file: &axiom_syntax::File<'s>, decl: &Decl<'s>) -> Vec<&'s str> {
    file[decl.props]
        .iter()
        .filter(|prop| prop.name.0 == "owner")
        .flat_map(|prop| file[prop.args].iter())
        .filter_map(|&arg| match file.exprs[arg].kind {
            ExprKind::Name(name) => Some(name.0),
            _ => None,
        })
        .collect()
}

impl Resolving<'_> {
    /// The shares a declaration gives its owners, `fallback` owning it whole when none is usable.
    fn owners(
        &self,
        names: &Interner,
        written: &Written<Decl>,
        index: &Scoped<Entity>,
        entities: &Tree<Entity>,
        fallback: Id<Entity>,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<Share> {
        let (file, decl) = (written.file(), written.node);
        let Some(line) = file[decl.props].iter().find(|prop| prop.name.0 == "owner") else {
            return Vec::new();
        };
        let mut resolved: Vec<(Id<Entity>, Option<Ratio>, Loc)> = Vec::new();
        for &arg in &file[line.args] {
            let expr = &file.exprs[arg];
            match expr.kind {
                ExprKind::Name(name) => match index.resolve(names, self.seeing.scopes.of(written.home()), name.0) {
                    Ok(entity) => resolved.push((entity, None, expr.loc)),
                    Err(miss) => {
                        let among = Among { index, names, systems: self.seeing.systems };
                        let describe = |ids: &[Id<Entity>]| {
                            problem::shortest(names, &index.names, ids, |id| entities[id].path, |id| entities[id].loc)
                        };
                        diags.push(among.failed(miss, Noun::Owner, Word { text: name.0, loc: expr.loc }, describe));
                    }
                },
                ExprKind::Pct(number) => {
                    if let Some((_, rate, _)) = resolved.last_mut() {
                        if rate.is_some() {
                            diags.push(
                                Diagnostic::error("owner-share", "an owner has more than one share")
                                    .label(expr.loc, "write one percentage after this owner"),
                            );
                        } else {
                            *rate = Ratio::percent(number.mantissa.into(), number.scale);
                        }
                    } else {
                        diags.push(
                            Diagnostic::error("owner-share", "a share percentage needs an owner name")
                                .label(expr.loc, "write `owner NAME 60%`"),
                        );
                    }
                }
                _ => diags.push(
                    Diagnostic::error("owner-name", "an owner must be an entity name")
                        .label(expr.loc, "write the owner before its optional percentage"),
                ),
            }
        }
        if resolved.is_empty() {
            // The diagnostic above is tied to the offending expression; keeping a
            // deterministic default prevents an invalid declaration cascading.
            return vec![Share { rate: Ratio::ONE, entity: fallback, measure: None, loc: line.loc }];
        }
        let has_rate = resolved.iter().any(|(_, rate, _)| rate.is_some());
        let all_rate = resolved.iter().all(|(_, rate, _)| rate.is_some());
        if has_rate && !all_rate {
            diags.push(
                Diagnostic::error("owner-share", "every owner in a shared place needs a percentage")
                    .label(line.loc, "write a percentage for each owner"),
            );
        }
        let equal = Ratio::new(1, resolved.len() as i128).unwrap_or(Ratio::ZERO);
        let shares: Vec<_> = resolved
            .into_iter()
            .map(|(entity, rate, loc)| Share { rate: rate.unwrap_or(equal), entity, measure: None, loc })
            .collect();
        let total = shares.iter().try_fold(Ratio::ZERO, |sum, share| sum.checked_add(share.rate));
        if shares.iter().any(|share| share.rate.is_negative()) || total != Some(Ratio::ONE) {
            diags.push(
                Diagnostic::error("owner-share-total", "owner shares must be nonnegative and add to 100%")
                    .label(line.loc, "adjust the percentages so they total exactly 100%"),
            );
        }
        shares
    }
}

/// The declared place a full path is closest to, used in migration diagnostics.
pub(crate) fn near_place<'a>(path: &str, declared: &[&'a str]) -> Option<&'a str> {
    let limit = (path.len() / 8).clamp(1, 2);
    let root = path.split('/').next();
    declared
        .iter()
        .copied()
        .filter(|other| other.len().abs_diff(path.len()) <= limit && other.split('/').next() == root)
        .map(|other| (axiom_core::diag::distance(path, other), other))
        .filter(|&(distance, _)| distance <= limit)
        .min()
        .map(|(_, other)| other)
}
