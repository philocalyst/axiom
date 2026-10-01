//! Native S5 declarations: create the immutable Book trees and name indexes.
//!
//! This pass borrows source AST nodes directly. It creates kinds and purposes
//! first so forward references from entities, commodities and accounts resolve
//! against complete scoped indexes, then freezes the place tree once.

use axiom_core::{Arena, Diagnostic, Groups, Id, Interner, Loc, Map, Ratio, Set, Sym, Tree};
use axiom_syntax::{Decl, DeclKind, ExprKind, ItemKind, Setting, Verb};

use crate::book::{
    Asset, At, Basis, Book, Books, Class, Commodity, Entity, Kind, KindRoots, Lookup, Place, Prop,
    Purpose, Role, Roots, Share, Sort, System,
};
use crate::errors::{Word, unknown};
use crate::kinds::{self, NativeKinds};
use crate::names::{Names, Scoped};
use crate::props::PropTable;
use crate::resolve::End;
use crate::scope::{Home, Scopes};
use crate::sources::{Site, SystemIndex};

/// Quanta are `i64`; eighteen decimals is as fine as one can count.
pub(crate) const MAX_SCALE: u8 = 18;

/// The book under construction, with the indexes needed by native lowerers.
pub(crate) struct World<'s> {
    pub book: Book<'s>,
    pub scopes: Scopes,
    pub systems: SystemIndex<'s>,
    pub props: PropTable,
    pub prop_writes: Vec<(PropTarget, Prop)>,
    pub tallies: Set<&'s str>,
    /// Claim tabs allocated from the bounded syntax survey before place IDs
    /// freeze. A later lookup that was not surveyed is an error.
    tabs: Map<(Id<Entity>, Id<Entity>, Class), Id<Place>>,
    /// Loan contract names resolve to their actual debt tab, before and after
    /// contract terms have been compiled.
    pub(crate) contract_endpoints: Map<Sym, End>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PropTarget {
    Kind(Id<Kind>),
    Entity(Id<Entity>),
    Commodity(Id<Commodity>),
    Place(Id<Place>),
    Asset(Id<Asset>),
}

impl PropTarget {
    fn key(self) -> (u8, u32) {
        match self {
            PropTarget::Kind(id) => (0, id.index() as u32),
            PropTarget::Entity(id) => (1, id.index() as u32),
            PropTarget::Commodity(id) => (2, id.index() as u32),
            PropTarget::Place(id) => (3, id.index() as u32),
            PropTarget::Asset(id) => (4, id.index() as u32),
        }
    }
}

impl World<'_> {
    pub(crate) fn set_prop(&mut self, target: PropTarget, prop: Prop) {
        self.prop_writes.push((target, prop));
    }

    /// Freeze staged custom-property rows once, grouped by their typed owner.
    pub(crate) fn finish_props(&mut self) {
        let names = &self.book.names;
        self.prop_writes.sort_by(|(ta, a), (tb, b)| {
            ta.key()
                .cmp(&tb.key())
                .then_with(|| names.name(a.name).cmp(names.name(b.name)))
                .then_with(|| a.since.cmp(&b.since))
        });
        let mut start = 0;
        while start < self.prop_writes.len() {
            let target = self.prop_writes[start].0;
            let mut end = start + 1;
            while end < self.prop_writes.len() && self.prop_writes[end].0 == target {
                end += 1;
            }
            let additions = &self.prop_writes[start..end];
            match target {
                PropTarget::Kind(id) => {
                    let rows = merge_props(names, &self.book.kinds[id].props, additions);
                    self.book.kinds[id].props = rows;
                }
                PropTarget::Entity(id) => {
                    let rows = merge_props(names, &self.book.entities[id].props, additions);
                    self.book.entities[id].props = rows;
                }
                PropTarget::Commodity(id) => {
                    let rows = merge_props(names, &self.book.commodities[id].props, additions);
                    self.book.commodities[id].props = rows;
                }
                PropTarget::Place(id) => {
                    let rows = merge_props(names, &self.book.places[id].props, additions);
                    self.book.places[id].props = rows;
                }
                PropTarget::Asset(id) => {
                    let rows = merge_props(names, &self.book.assets[id].props, additions);
                    self.book.assets[id].props = rows;
                }
            }
            start = end;
        }
        self.prop_writes.clear();
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

/// Merge another frozen property batch without discarding earlier rows. The
/// stable sort preserves write order for same-name, same-day rows, so an
/// earlier value remains the one in force as specified by [`crate::prop`].
fn merge_props(
    names: &Interner<'_>,
    existing: &[Prop],
    additions: &[(PropTarget, Prop)],
) -> Box<[Prop]> {
    let mut rows = Vec::with_capacity(existing.len() + additions.len());
    rows.extend_from_slice(existing);
    rows.extend(additions.iter().map(|(_, prop)| *prop));
    rows.sort_by(|a, b| {
        names
            .name(a.name)
            .cmp(names.name(b.name))
            .then_with(|| a.since.cmp(&b.since))
    });
    rows.into_boxed_slice()
}

#[cfg(test)]
mod property_finalization_tests {
    use super::{PropTarget, merge_props};
    use crate::{Prop, Value, prop};
    use axiom_core::{Day, FileId, Id, Interner, Loc, Ratio};

    #[test]
    fn successive_property_finalization_preserves_rows_and_first_same_day_value() {
        let mut names = Interner::default();
        let label = names.intern("label");
        let amount = names.intern("amount");
        let first_day = Day::from_ymd(2026, 1, 1).unwrap();
        let later_day = Day::from_ymd(2026, 2, 1).unwrap();
        let first_batch = [Prop {
            name: label,
            value: Value::Num(Ratio::int(1)),
            since: first_day,
            loc: Some(Loc::new(FileId(0), 1, 2)),
        }];
        let first_writes = [(PropTarget::Kind(Id::new(0)), first_batch[0])];
        let once = merge_props(&names, &[], &first_writes);

        let second_batch = [
            Prop {
                name: label,
                value: Value::Num(Ratio::int(2)),
                since: first_day,
                loc: Some(Loc::new(FileId(0), 3, 4)),
            },
            Prop {
                name: label,
                value: Value::Num(Ratio::int(3)),
                since: later_day,
                loc: Some(Loc::new(FileId(0), 5, 6)),
            },
            Prop {
                name: amount,
                value: Value::Num(Ratio::int(4)),
                since: first_day,
                loc: Some(Loc::new(FileId(0), 7, 8)),
            },
        ];
        let second_writes = second_batch.map(|prop| (PropTarget::Kind(Id::new(0)), prop));
        let twice = merge_props(&names, &once, &second_writes);

        assert_eq!(twice.len(), 4);
        assert_eq!(names.name(twice[0].name), "amount");
        assert_eq!(
            prop(&twice, label, first_day).unwrap().value,
            Value::Num(Ratio::int(1))
        );
        assert_eq!(
            prop(&twice, label, later_day).unwrap().value,
            Value::Num(Ratio::int(3))
        );

        let again = merge_props(&names, &twice, &[]);
        assert_eq!(again.len(), twice.len());
        for (again, twice) in again.iter().zip(twice.iter()) {
            assert_eq!(again.name, twice.name);
            assert_eq!(again.value, twice.value);
            assert_eq!(again.since, twice.since);
            assert_eq!(again.loc, twice.loc);
        }
    }
}

pub(crate) struct Settings<'s> {
    pub base: Option<Word<'s>>,
    pub relaxed: bool,
}

pub(crate) fn settings<'a, 's>(
    sites: &[Site<'a, 's>],
    diags: &mut Vec<Diagnostic>,
) -> Settings<'s> {
    let mut settings = Settings {
        base: None,
        relaxed: false,
    };
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Setting(id) = item.kind else {
                continue;
            };
            match file[id] {
                Setting::Base(name) => {
                    let word = Word {
                        text: name.0,
                        loc: file.loc(name.0),
                    };
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
                Setting::System(_) | Setting::Use(_) | Setting::Currency(_) | Setting::Rates(_) => {
                }
            }
        }
    }
    settings
}

/// What each home has brought into scope: its `use` lines, the system of every
/// `lives` line (which implies a `use`), and `std` for everyone.
pub(crate) fn scopes(
    sites: &[Site<'_, '_>],
    systems: &SystemIndex,
    tree: &Tree<System>,
    diags: &mut Vec<Diagnostic>,
) -> Scopes {
    let mut used: Vec<(Home, Id<System>)> = Vec::new();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Setting(id) => {
                    if let Setting::Use(name) = file[id] {
                        match systems.find(name.0) {
                            Some(system) => used.push((site.home, system)),
                            None => diags.push(systems.unknown(Word {
                                text: name.0,
                                loc: file.loc(name.0),
                            })),
                        }
                    }
                }
                ItemKind::Decl(id) if file[id].what == DeclKind::Entity => {
                    for prop in &file[file[id].props] {
                        if prop.name.0 == "lives" {
                            add_lives(&mut used, site.home, systems, file, prop);
                        }
                    }
                }
                ItemKind::Statement(id) => {
                    if let Verb::Now(ax) = &file[id].verb
                        && let axiom_syntax::Change::Property(prop) = ax
                        && prop.name.0 == "lives"
                    {
                        add_lives(&mut used, site.home, systems, file, prop);
                    }
                }
                _ => {}
            }
        }
    }
    let std = systems.find("std");
    Scopes::new(tree, |home| {
        let own = used
            .iter()
            .filter(|&&(user, _)| user == home)
            .map(|&(_, system)| system);
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
        if let ExprKind::Name(path) = file.exprs[arg].kind {
            if let Some(system) = systems.find(path.0) {
                used.push((home, system));
            }
        }
    }
}

struct EntityDraft<'s> {
    path: &'s str,
    home: Home,
}

struct AccountDraft<'s> {
    path: &'s str,
    class: Class,
    kind: Id<Kind>,
    owner: Id<Entity>,
    shares: Box<[Share]>,
    institution: Option<Id<Entity>>,
    loc: Loc,
}

struct TabDraft {
    party: Id<Entity>,
    owner: Id<Entity>,
    class: Class,
    loc: Loc,
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
    order: usize,
    /// Contracts may take the spelling of their party, but no other entity.
    contract_party: Option<&'s str>,
}

fn check_cross_namespace_names(
    sites: &[Site<'_, '_>],
    scopes: &Scopes,
    systems: &Tree<System>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut claims = Vec::new();
    let mut order = 0;
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Decl(id) => {
                    let decl = &file[id];
                    if decl.what == DeclKind::Entity && matches!(decl.name.0, "opening" | "market" | "?") {
                        diags.push(
                            Diagnostic::error(
                                "reserved-entity-name",
                                format!("`{}` is reserved for a built-in entity", decl.name.0),
                            )
                            .label(file.loc(decl.name.0), "choose a different entity name"),
                        );
                    }
                    let (space, aliases) = match decl.what {
                        DeclKind::Account => (NameSpace::Account, true),
                        DeclKind::Entity => (NameSpace::Entity, true),
                        DeclKind::Asset => (NameSpace::Asset, true),
                        DeclKind::Purpose => (NameSpace::Purpose, true),
                        DeclKind::Commodity => (NameSpace::Commodity, false),
                        DeclKind::Kind => {
                            order += 1;
                            continue;
                        }
                    };
                    push_name_claims(
                        &mut claims,
                        decl.name.0,
                        space,
                        site.home,
                        file.loc(decl.name.0),
                        order,
                        None,
                        aliases,
                    );
                }
                ItemKind::Contract(id) => {
                    let contract = &file[id];
                    push_name_claims(
                        &mut claims,
                        contract.name.0,
                        NameSpace::Contract,
                        site.home,
                        file.loc(contract.name.0),
                        order,
                        Some(contract.party.unwrap_or(contract.name).0),
                        false,
                    );
                }
                _ => {}
            }
            order += 1;
        }
    }

    claims.sort_unstable_by(|a, b| {
        a.spelling
            .cmp(b.spelling)
            .then(a.order.cmp(&b.order))
            .then(a.space.cmp(&b.space))
    });

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
                diagnostic = diagnostic.context(
                    first.loc,
                    format!("{} `{}` was declared earlier", first.space.article(), first.declared),
                );
            }
            diagnostic = diagnostic.help("rename one declaration or use a different account path");
            diags.push(diagnostic);
        }
        start = end;
    }
}

fn push_name_claims<'s>(
    claims: &mut Vec<NameClaim<'s>>,
    declared: &'s str,
    space: NameSpace,
    home: Home,
    loc: Loc,
    order: usize,
    contract_party: Option<&'s str>,
    aliases: bool,
) {
    let mut push = |spelling| {
        claims.push(NameClaim {
            spelling,
            declared,
            space,
            home,
            loc,
            order,
            contract_party,
        });
    };
    push(declared);
    if aliases {
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

/// Construct a native v4 Book from the syntax sites and a small survey of only
/// claim-bearing journal relationships. No v3 chart-account collection is used.
pub(crate) fn declare<'a, 's>(
    sites: &[Site<'a, 's>],
    settings: &Settings<'s>,
    mut names: Interner<'s>,
    systems_tree: Tree<System>,
    systems: SystemIndex<'s>,
    scopes: Scopes,
    survey: &crate::lower::JournalSurvey<'s>,
    diags: &mut Vec<Diagnostic>,
) -> World<'s> {
    let native_kinds = kinds::declare_sites(sites, &mut names, &systems_tree, &scopes, diags);
    let native_purposes = crate::purposes::declare_sites(
        sites,
        &mut names,
        &systems_tree,
        &scopes,
        &native_kinds.index,
        diags,
    );
    check_cross_namespace_names(sites, &scopes, &systems_tree, diags);

    let mut commodities = Arena::new();
    let mut commodity_by_name: Map<&'s str, Id<Commodity>> = Map::default();
    let commodity_root = native_kinds.roots.commodity;
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            if decl.what != DeclKind::Commodity {
                continue;
            }
            let symbol = decl.name.0;
            if let Some(&first) = commodity_by_name.get(symbol) {
                diags.push(duplicate_decl(
                    "commodity",
                    symbol,
                    file.loc(symbol),
                    commodities[first].loc,
                ));
                continue;
            }
            let kind = resolve_kind(
                decl,
                site.home,
                Sort::Commodity,
                commodity_root,
                &native_kinds,
                &names,
                &systems_tree,
                &scopes,
                file,
                diags,
            );
            let id = commodities.push(Commodity {
                symbol: names.intern(symbol),
                kind,
                scale: 0,
                title: None,
                liquidity: None,
                select: None,
                growth: None,
                props: Box::default(),
                doc: item.doc.map(|doc| names.intern(doc.0)),
                loc: Some(file.loc(symbol)),
            });
            commodity_by_name.insert(symbol, id);
        }
    }
    let mut synthetic_base = None;
    if commodities.is_empty() {
        let usd = names.intern("USD");
        let id = commodities.push(Commodity {
            symbol: usd,
            kind: commodity_root,
            scale: 2,
            title: None,
            liquidity: None,
            select: None,
            growth: None,
            props: Box::default(),
            doc: None,
            loc: None,
        });
        commodity_by_name.insert("USD", id);
        synthetic_base = Some(id);
    }
    if settings.base.is_none()
        && synthetic_base.is_none()
        && !commodity_by_name.contains_key("USD")
        && commodity_by_name.len() > 1
    {
        let first = commodity_by_name
            .values()
            .next()
            .and_then(|&id| commodities[id].loc)
            .unwrap_or_default();
        diags.push(
            Diagnostic::error("base-currency-required", "a book with several currencies needs a base currency")
                .label(first, "choose the currency amounts are converted into")
                .help("write `base UNIT` once, such as `base USD`"),
        );
    }
    let base = settings
        .base
        .and_then(|word| {
            commodity_by_name.get(word.text).copied().or_else(|| {
                let suggestion =
                    axiom_core::diag::closest(word.text, commodity_by_name.keys().copied())
                        .map(|near| near as &str);
                diags.push(unknown(
                    "unknown-commodity",
                    "base commodity",
                    word,
                    suggestion,
                ));
                None
            })
        })
        .or_else(|| commodity_by_name.get("USD").copied())
        .or_else(|| synthetic_base)
        .or_else(|| commodities.ids().next())
        .expect("a book always has a base commodity");

    let mut entity_drafts: Vec<EntityDraft<'s>> = Vec::new();
    let mut explicit_entities: Map<
        &'s str,
        (Home, &'a axiom_syntax::File<'s>, &'a Decl<'s>, Option<Sym>),
    > = Map::default();
    let mut first_entity_paths = Vec::new();
    let mut owner_names: Set<&'s str> = Set::default();
    let mut declared_entity_names = Set::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            if matches!(
                decl.what,
                DeclKind::Account | DeclKind::Entity | DeclKind::Asset
            ) {
                owner_names.extend(owner_names_in(file, decl));
            }
            if decl.what != DeclKind::Entity {
                continue;
            }
            let path = decl.name.0;
            if let Some((_, first_file, first, _)) = explicit_entities.get(path) {
                diags.push(duplicate_decl(
                    "entity",
                    path,
                    file.loc(path),
                    Some(first_file.loc(first.name.0)),
                ));
                continue;
            }
            let doc = item.doc.map(|doc| names.intern(doc.0));
            explicit_entities.insert(path, (site.home, file, decl, doc));
            declared_entity_names.insert(path);
            first_entity_paths.push(path);
        }
    }

    // Endpoint names that are not declared account/asset paths, other typed
    // names, or explicit entities are parties. Keep only one borrowed name and
    // its first source location, even when it occurs in many journal rows.
    let mut place_names = Set::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else { continue };
            let decl = &file[id];
            if matches!(decl.what, DeclKind::Account | DeclKind::Asset) {
                add_path_spellings(&mut place_names, decl.name.0);
            }
        }
    }

    let mut entity_spellings = Set::default();
    for &path in explicit_entities.keys() {
        add_path_spellings(&mut entity_spellings, path);
    }
    for root in ["me", "?", "opening", "market"] {
        entity_spellings.insert(root);
    }

    let mut other_spellings = Set::default();
    for name in native_kinds.index.names.keys(&names) {
        other_spellings.insert(name);
    }
    for name in native_purposes.index.names.keys(&names) {
        other_spellings.insert(name);
    }
    for &name in commodity_by_name.keys() {
        add_path_spellings(&mut other_spellings, name);
    }
    for (_, system) in systems_tree.iter() {
        add_path_spellings(&mut other_spellings, names.name(system.path));
    }
    let mut contract_names = Set::default();
    let mut candidates: Map<&'s str, Loc> = Map::default();
    let mut entity_roles = Set::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Decl(id) if file[id].what == DeclKind::Asset => {
                    add_path_spellings(&mut other_spellings, file[id].name.0);
                }
                ItemKind::Contract(id) => {
                    add_path_spellings(&mut contract_names, file[id].name.0);
                }
                _ => {}
            }
        }
    }
    crate::lower::visit_endpoints(sites, |_, name, loc, _| {
        candidates.entry(name.0).or_insert(loc);
    });
    for mention in &survey.mentions {
        match *mention {
            crate::lower::Mention::Claim { subject, creditor, loc } => {
                candidates.entry(subject.0).or_insert(loc);
                candidates.entry(creditor.0).or_insert(loc);
                entity_roles.insert(subject.0);
                entity_roles.insert(creditor.0);
            }
            crate::lower::Mention::For { other, loc, .. } => {
                candidates.entry(other.0).or_insert(loc);
                entity_roles.insert(other.0);
            }
            crate::lower::Mention::Promise { party, loc, .. } => {
                candidates.entry(party.0).or_insert(loc);
                entity_roles.insert(party.0);
            }
            crate::lower::Mention::Ends { .. } | crate::lower::Mention::Due { .. } => {}
        }
    }

    let mut implicit_party_locs: Map<&'s str, Loc> = Map::default();
    for (&path, &loc) in &candidates {
        if entity_spellings.contains(path) || place_names.contains(path) || other_spellings.contains(path) {
            continue;
        }
        if matches!(path, "self" | "issuer") || (contract_names.contains(path) && !entity_roles.contains(path)) {
            continue;
        }
        implicit_party_locs.insert(path, loc);
    }
    // A written suffix such as `acme` can resolve to one implicit path such as
    // `vendors/acme`; adding a second `acme` entity would make that reference
    // ambiguous. Preserve all full paths so genuinely ambiguous suffixes are
    // diagnosed by the normal scoped entity resolver.
    let mut implicit_paths: Vec<&'s str> = implicit_party_locs.keys().copied().collect();
    implicit_paths.sort_unstable();
    let shadowed_suffixes = strict_path_suffixes(&implicit_paths);
    for &path in &implicit_paths {
        entity_spellings.insert(path);
        add_path_spellings(&mut entity_spellings, path);
    }
    for &path in &first_entity_paths {
        let (home, ..) = explicit_entities[&path];
        entity_drafts.push(EntityDraft { path, home });
    }
    for root in ["me", "?", "opening", "market"] {
        if !declared_entity_names.contains(root) {
            entity_drafts.push(EntityDraft {
                path: root,
                home: Home::Builtin,
            });
        }
    }
    for path in implicit_paths {
        if explicit_entities.contains_key(path) || shadowed_suffixes.contains(path) {
            continue;
        }
        entity_drafts.push(EntityDraft {
            path,
            home: Home::Builtin,
        });
    }
    let entity_paths = entity_drafts
        .iter()
        .map(|draft| draft.path)
        .collect::<Vec<_>>();
    let entity_home_by_path: Map<&str, Home> = entity_drafts
        .iter()
        .map(|draft| (draft.path, draft.home))
        .collect();
    let (mut entities, entity_ids) = crate::paths::build(entity_paths.iter().copied(), |path| {
        let draft = explicit_entities.get(path);
        let (kind, purpose, doc, loc) = if let Some((home, file, decl, doc)) = draft {
            let kind = resolve_kind(
                decl,
                *home,
                Sort::Entity,
                native_kinds.roots.entity,
                &native_kinds,
                &names,
                &systems_tree,
                &scopes,
                file,
                diags,
            );
            let purpose = decl.purpose.and_then(|name| {
                resolve_purpose(
                    name.0,
                    file.loc(name.0),
                    *home,
                    &native_purposes,
                    &names,
                    &scopes,
                    diags,
                )
                .map(|value| At {
                    value,
                    loc: file.loc(name.0),
                })
            });
            (kind, purpose, *doc, Some(file.loc(decl.name.0)))
        } else {
            (
                native_kinds.roots.entity,
                None,
                None,
                implicit_party_locs.get(path).copied(),
            )
        };
        Entity {
            path: names.intern(path),
            kind,
            purpose,
            place: None,
            restricted: false,
            lives: Box::default(),
            member: None,
            owner: None,
            client_of: None,
            owned_by: Box::default(),
            currency: base,
            citizen: Box::default(),
            books: Books::default(),
            known_as: Box::default(),
            props: Box::default(),
            doc,
            loc,
        }
    });
    let me = *entity_ids
        .get("me")
        .expect("the book's owner entity is created");
    let unknown = *entity_ids.get("?").expect("the unknown entity is created");
    let opening = *entity_ids
        .get("opening")
        .expect("the opening entity is created");
    let market = *entity_ids
        .get("market")
        .expect("the market entity is created");
    let mut entity_homes = vec![Home::Builtin; entities.len()];
    for (path, &id) in &entity_ids {
        entity_homes[id.index()] = entity_home_by_path
            .get(path)
            .copied()
            .unwrap_or(Home::Builtin);
    }
    let entity_items: Vec<_> = entities
        .iter()
        .map(|(id, entity)| (id, names.name(entity.path), entity_homes[id.index()]))
        .collect();
    let entity_index = Scoped::build(&mut names, entity_items);
    let entity_by_name: Map<&str, Id<Entity>> =
        entity_ids.iter().map(|(&path, &id)| (path, id)).collect();

    // Ownership belongs to entities as well as accounts. Build it after every
    // entity id exists, preserving the declaration's source order.
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            if decl.what != DeclKind::Entity {
                continue;
            }
            let Some(&entity) = entity_ids.get(decl.name.0) else {
                continue;
            };
            let owners = resolve_owner_shares(
                file,
                decl,
                site.home,
                &entity_index,
                &names,
                &scopes,
                me,
                diags,
            );
            if let Some(share) = owners.first() {
                entities[entity].owner = Some(share.entity);
                entities[entity].owned_by = owners.into_boxed_slice();
            }
        }
    }

    // Account drafts can refer forward to both kinds and owners because both
    // indexes are already complete.
    let mut account_drafts = Vec::new();
    let mut declared_account_paths: Map<&'s str, Loc> = Map::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            if decl.what != DeclKind::Account {
                continue;
            }
            let path = decl.name.0;
            if let Some(&first) = declared_account_paths.get(path) {
                diags.push(duplicate_decl("account", path, file.loc(path), Some(first)));
                continue;
            }
            declared_account_paths.insert(path, file.loc(path));
            let kind = resolve_kind(
                decl,
                site.home,
                Sort::Place(Class::Asset),
                native_kinds.roots.asset,
                &native_kinds,
                &names,
                &systems_tree,
                &scopes,
                file,
                diags,
            );
            let class = match native_kinds.tree[kind].sort {
                Sort::Place(class) => class,
                found => {
                    diags.push(
                        Diagnostic::error("account-kind-sort", "an account needs a place kind")
                            .label(file.loc(path), format!("this kind classifies {found:?}")),
                    );
                    Class::Asset
                }
            };
            let shares = resolve_owner_shares(
                file,
                decl,
                site.home,
                &entity_index,
                &names,
                &scopes,
                me,
                diags,
            );
            let owner = shares.first().map_or_else(
                || {
                    first_name_prop(file, decl, "owner")
                        .and_then(|name| entity_by_name.get(name).copied())
                        .unwrap_or(me)
                },
                |share| share.entity,
            );
            let institution = decl.at.and_then(|name| {
                entity_index
                    .resolve(&names, scopes.of(site.home), name.0)
                    .map_err(|_| {
                        diags.push(
                            Diagnostic::error(
                                "unknown-institution",
                                format!("institution `{}` is not visible", name.0),
                            )
                            .label(file.loc(name.0), "not a visible entity"),
                        )
                    })
                    .ok()
            });
            let loc = file.loc(path);
            account_drafts.push(AccountDraft {
                path,
                class,
                kind,
                owner,
                shares: shares.into_boxed_slice(),
                institution,
                loc,
            });
        }
    }

    // The entities referenced as account owners are the owners' holdings;
    // other parties keep an outside endpoint. `me` always owns a holding.
    let mut owners = owner_names;
    owners.insert("me");
    let mut entity_owner = vec![false; entities.len()];
    for (path, &id) in &entity_ids {
        entity_owner[id.index()] = owners.contains(path);
    }

    let mut assets = Arena::new();
    let mut asset_paths = Vec::new();
    let mut asset_names: Map<&'s str, Id<Asset>> = Map::default();
    let mut asset_shares: Map<Id<Asset>, Box<[Share]>> = Map::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ItemKind::Decl(id) = item.kind else {
                continue;
            };
            let decl = &file[id];
            if decl.what != DeclKind::Asset {
                continue;
            }
            let path = decl.name.0;
            if let Some(&first) = asset_names.get(path) {
                diags.push(duplicate_decl(
                    "asset",
                    path,
                    file.loc(path),
                    Some(assets[first].loc),
                ));
                continue;
            }
            let kind = resolve_kind(
                decl,
                site.home,
                Sort::Thing,
                native_kinds.roots.thing,
                &native_kinds,
                &names,
                &systems_tree,
                &scopes,
                file,
                diags,
            );
            let name = names.intern(path);
            let unit = commodities.push(Commodity {
                symbol: name,
                kind: commodity_root,
                scale: 0,
                title: None,
                liquidity: None,
                select: None,
                growth: None,
                props: Box::default(),
                doc: None,
                loc: Some(file.loc(path)),
            });
            commodity_by_name.entry(path).or_insert(unit);
            let owners = resolve_owner_shares(
                file,
                decl,
                site.home,
                &entity_index,
                &names,
                &scopes,
                me,
                diags,
            );
            let owner = owners.first().map_or(me, |share| share.entity);
            let asset = assets.push(Asset {
                name,
                kind,
                owner,
                place: Id::new(0),
                unit,
                part_of: None,
                props: Box::default(),
                doc: item.doc.map(|doc| names.intern(doc.0)),
                loc: file.loc(path),
            });
            if !owners.is_empty() {
                asset_shares.insert(asset, owners.into_boxed_slice());
            }
            asset_names.insert(path, asset);
            asset_paths.push((path, asset));
        }
    }

    let mut account_owner_by_path = Map::default();
    for account in &account_drafts {
        account_owner_by_path.insert(account.path, account.owner);
    }
    let mut tab_drafts: Vec<TabDraft> = Vec::new();
    let mut tab_keys = Set::default();
    let mut add_tab = |party: Id<Entity>, owner: Id<Entity>, class: Class, loc: Loc| {
        if party != owner && tab_keys.insert((party, owner, class)) {
            tab_drafts.push(TabDraft {
                party,
                owner,
                class,
                loc,
            });
        }
    };
    for mention in &survey.mentions {
        match *mention {
            crate::lower::Mention::Claim {
                subject,
                creditor,
                loc,
            } => {
                if let (Some(&subject), Some(&creditor)) = (
                    entity_by_name.get(subject.0),
                    entity_by_name.get(creditor.0),
                ) {
                    let (party, owner, class) = if entity_owner[creditor.index()] {
                        (subject, creditor, Class::Asset)
                    } else if entity_owner[subject.index()] {
                        (creditor, subject, Class::Debt)
                    } else {
                        (subject, creditor, Class::Asset)
                    };
                    add_tab(party, owner, class, loc);
                }
            }
            crate::lower::Mention::Promise {
                party,
                holding,
                loc,
                ..
            } => {
                if let Some(&party_id) = entity_by_name.get(party.0) {
                    let owner = holding
                        .and_then(|name| account_owner_by_path.get(name.0).copied())
                        .unwrap_or(me);
                    add_tab(party_id, owner, Class::Asset, loc);
                    add_tab(party_id, owner, Class::Debt, loc);
                }
            }
            crate::lower::Mention::For { other, ends, loc } => {
                if let Some(&party) = entity_by_name.get(other.0) {
                    let owner = ends
                        .from
                        .and_then(|name| account_owner_by_path.get(name.0).copied())
                        .or_else(|| {
                            ends.to
                                .and_then(|name| account_owner_by_path.get(name.0).copied())
                        })
                        .unwrap_or(me);
                    add_tab(party, owner, Class::Asset, loc);
                    add_tab(party, owner, Class::Debt, loc);
                }
            }
            crate::lower::Mention::Due { ends, loc }
            | crate::lower::Mention::Ends { ends, loc } => {
                if let (Some(from), Some(to)) = (ends.from, ends.to) {
                    let (from_entity, to_entity) =
                        (entity_by_name.get(from.0), entity_by_name.get(to.0));
                    let (from_owner, to_owner) = (
                        account_owner_by_path.get(from.0),
                        account_owner_by_path.get(to.0),
                    );
                    if let (Some(&party), Some(&owner)) = (from_entity, to_owner) {
                        add_tab(party, owner, Class::Asset, loc);
                    }
                    if let (Some(&party), Some(&owner)) = (to_entity, from_owner) {
                        add_tab(party, owner, Class::Debt, loc);
                    }
                }
            }
        }
    }

    // Keep path namespaces disjoint while giving each account, entity endpoint
    // and asset place a stable tree id. Every path and prefix is source-borrowed.
    const ACCOUNTS: u8 = 0;
    const ENTITY_PLACES: u8 = 1;
    const ASSET_PLACES: u8 = 2;
    let mut path_keys: Set<(u8, &'s str)> = Set::default();
    for account in &account_drafts {
        for path in crate::paths::prefixes(account.path) {
            path_keys.insert((ACCOUNTS, path));
        }
    }
    for draft in &entity_drafts {
        for path in crate::paths::prefixes(draft.path) {
            path_keys.insert((ENTITY_PLACES, path));
        }
    }
    for &(path, _) in &asset_paths {
        for prefix in crate::paths::prefixes(path) {
            path_keys.insert((ASSET_PLACES, prefix));
        }
    }
    let mut paths: Vec<_> = path_keys.into_iter().collect();
    paths.sort_unstable_by(|(ns_a, a), (ns_b, b)| {
        ns_a.cmp(ns_b).then_with(|| path_key(a).cmp(path_key(b)))
    });
    let path_positions: Map<(u8, &str), usize> = paths
        .iter()
        .enumerate()
        .map(|(at, &key)| (key, at))
        .collect();
    let mut place_nodes = Vec::with_capacity(paths.len() + account_drafts.len() + tab_drafts.len());
    let mut parents = Vec::with_capacity(place_nodes.capacity());
    let mut place_index = Map::default();
    let account_by_path: Map<&str, &AccountDraft<'s>> = account_drafts
        .iter()
        .map(|draft| (draft.path, draft))
        .collect();
    let asset_by_path: Map<&str, Id<Asset>> = asset_paths.iter().copied().collect();
    let entity_by_path: Map<&str, Id<Entity>> =
        entity_ids.iter().map(|(&path, &id)| (path, id)).collect();
    for &(namespace, path) in &paths {
        let explicit_account = if namespace == ACCOUNTS {
            account_by_path.get(path).copied()
        } else {
            None
        };
        let explicit_asset = if namespace == ASSET_PLACES {
            asset_by_path.get(path).copied()
        } else {
            None
        };
        let entity = if namespace == ENTITY_PLACES {
            entity_by_path.get(path).copied()
        } else {
            None
        };
        let descendant_account = if namespace == ACCOUNTS {
            account_drafts
                .iter()
                .find(|draft| is_path_child(path, draft.path))
        } else {
            None
        };
        let descendant_asset = if namespace == ASSET_PLACES {
            asset_paths
                .iter()
                .find(|(child, _)| is_path_child(path, child))
                .map(|(_, asset)| &assets[*asset])
        } else {
            None
        };
        let (class, role, kind, owner, loc, indexed) = if let Some(account) = explicit_account {
            (
                account.class,
                Role::Account {
                    institution: account.institution,
                },
                account.kind,
                account.owner,
                Some(account.loc),
                true,
            )
        } else if let Some(asset) = explicit_asset {
            let record = &assets[asset];
            (
                Class::Asset,
                Role::Asset(asset),
                record.kind,
                record.owner,
                Some(record.loc),
                true,
            )
        } else if let Some(entity) = entity {
            let held = entity_owner[entity.index()];
            (
                if held { Class::Asset } else { Class::Outside },
                if held {
                    Role::Holding(entity)
                } else {
                    Role::Outside(Some(entity))
                },
                if held {
                    native_kinds.roots.asset
                } else {
                    native_kinds.roots.entity
                },
                if held { entity } else { me },
                entities[entity].loc,
                false,
            )
        } else if let Some(account) = descendant_account {
            (
                account.class,
                Role::Account { institution: None },
                account.kind,
                account.owner,
                None,
                true,
            )
        } else if let Some(asset) = descendant_asset {
            (
                Class::Asset,
                Role::Account { institution: None },
                asset.kind,
                asset.owner,
                None,
                true,
            )
        } else {
            (
                Class::Asset,
                Role::Account { institution: None },
                native_kinds.roots.asset,
                me,
                None,
                false,
            )
        };
        let node = Place {
            path: names.intern(path),
            class,
            role,
            kind,
            owner,
            holds: None,
            select: None,
            deferred: false,
            basis: Basis::Cost,
            claim: false,
            liquidity: None,
            opened: None,
            closed: None,
            shares: explicit_account.map_or_else(
                || {
                    explicit_asset
                        .and_then(|asset| asset_shares.get(&asset).cloned())
                        .unwrap_or_default()
                },
                |account| account.shares.clone(),
            ),
            known_as: Box::default(),
            props: Box::default(),
            doc: None,
            loc,
        };
        let at = place_nodes.len();
        place_nodes.push(node);
        let parent = path
            .rsplit_once('/')
            .and_then(|(parent, _)| path_positions.get(&(namespace, parent)).copied());
        parents.push(parent);
        place_index.insert((namespace, path), (at, indexed));
    }
    // Tabs have no source path of their own; their printed label is the party
    // name, while identity is the typed (party, owner, class) tuple.
    let tab_node_indices: Vec<_> = tab_drafts
        .iter()
        .map(|tab| {
            let at = place_nodes.len();
            place_nodes.push(Place {
                path: entities[tab.party].path,
                class: tab.class,
                role: Role::Tab(tab.party),
                kind: if tab.class == Class::Debt {
                    native_kinds.roots.debt
                } else {
                    native_kinds.roots.asset
                },
                owner: tab.owner,
                holds: None,
                select: None,
                deferred: false,
                basis: Basis::Cost,
                claim: true,
                liquidity: None,
                opened: None,
                closed: None,
                shares: Box::default(),
                known_as: Box::default(),
                props: Box::default(),
                doc: None,
                loc: Some(tab.loc),
            });
            parents.push(None);
            at
        })
        .collect();
    let (places, remap) =
        Tree::build(place_nodes, &parents).expect("place parents are prefixes without cycles");

    let mut place_names = Names::default();
    for (&(namespace, path), &(old, indexed)) in &place_index {
        if indexed && (namespace == ACCOUNTS || namespace == ASSET_PLACES) {
            place_names.insert_path(&mut names, path, remap[old]);
        }
    }
    for (&path, &entity) in &entity_ids {
        if let Some(&(old, _)) = place_index.get(&(ENTITY_PLACES, path)) {
            entities[entity].place = Some(remap[old]);
        }
    }
    for &(path, asset) in &asset_paths {
        if let Some(&(old, _)) = place_index.get(&(ASSET_PLACES, path)) {
            assets[asset].place = remap[old];
        }
    }
    let mut tabs = Map::default();
    for (tab, &old) in tab_drafts.iter().zip(&tab_node_indices) {
        tabs.insert((tab.party, tab.owner, tab.class), remap[old]);
    }
    let mut contract_endpoints = Map::default();
    for mention in &survey.mentions {
        let crate::lower::Mention::Promise {
            name,
            party,
            holding,
            loan_party: Some(_),
            ..
        } = *mention
        else {
            continue;
        };
        let Some(&party) = entity_by_name.get(party.0) else {
            continue;
        };
        let owner = holding
            .and_then(|name| account_owner_by_path.get(name.0).copied())
            .unwrap_or(me);
        if let Some(&place) = tabs.get(&(party, owner, Class::Debt)) {
            contract_endpoints.insert(
                names.intern(name.0),
                End {
                    place,
                    entity: Some(party),
                },
            );
        }
    }
    let kind_index = native_kinds.index;
    let purpose_index = native_purposes.index;
    let roots = Roots {
        me,
        unknown,
        opening,
        market,
        kinds: KindRoots {
            asset: native_kinds.roots.asset,
            debt: native_kinds.roots.debt,
            thing: native_kinds.roots.thing,
            commodity: native_kinds.roots.commodity,
            measure: native_kinds.roots.measure,
            entity: native_kinds.roots.entity,
        },
        purposes: native_purposes.roots,
    };
    let mut lookup = Lookup::default();
    lookup.places = place_names;
    lookup.entities = entity_index;
    lookup.kinds = kind_index;
    lookup.purposes = purpose_index;
    lookup.assets = asset_names
        .into_iter()
        .map(|(name, id)| (names.intern(name), id))
        .collect();
    lookup.commodities = commodity_by_name
        .into_iter()
        .map(|(name, id)| (names.intern(name), id))
        .collect();
    let book = Book {
        names,
        text_values: Arena::new(),
        base,
        relaxed: settings.relaxed,
        roots,
        places,
        entities,
        kinds: native_kinds.tree,
        purposes: native_purposes.tree,
        systems: systems_tree,
        commodities,
        assets,
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
        prices: Default::default(),
        splits: Vec::new(),
        measures: Arena::new(),
        readings: Vec::new(),
        filed: Vec::new(),
        plans: Arena::new(),
        sources: Vec::new(),
        lookup,
    };
    World {
        book,
        scopes,
        systems,
        props: PropTable::default(),
        prop_writes: Vec::new(),
        tallies: Set::default(),
        tabs,
        contract_endpoints,
    }
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
            candidate
                .bytes()
                .rev()
                .cmp(path.bytes().rev().chain(std::iter::once(b'/')))
                .is_lt()
        });
        if let Some(&candidate) = reversed.get(after_prefix)
            && candidate != path
            && candidate
                .strip_suffix(path)
                .is_some_and(|prefix| prefix.ends_with('/'))
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

fn duplicate_decl(kind: &str, name: &str, again: Loc, first: Option<Loc>) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        "duplicate-declaration",
        format!("{kind} `{name}` is declared twice"),
    )
    .label(again, "declared again here");
    if let Some(first) = first {
        diagnostic = diagnostic.context(first, "first declared here");
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::strict_path_suffixes;

    #[test]
    fn implicit_party_suffixes_are_found_without_prefix_collisions() {
        let paths = [
            "acme",
            "vendors/acme",
            "archive/vendors/acme",
            "acme2",
            "other/acme2",
            "vendor/acme/branch",
        ];
        let shadowed = strict_path_suffixes(&paths);

        assert!(shadowed.contains("acme"));
        assert!(shadowed.contains("vendors/acme"));
        assert!(shadowed.contains("acme2"));
        assert!(!shadowed.contains("other/acme2"));
        assert!(!shadowed.contains("vendor/acme/branch"));
    }
}

fn resolve_kind<'s>(
    decl: &Decl<'s>,
    home: Home,
    expected: Sort,
    fallback: Id<Kind>,
    kinds: &NativeKinds,
    names: &Interner<'s>,
    _systems: &Tree<System>,
    scopes: &Scopes,
    file: &axiom_syntax::File<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Id<Kind> {
    let Some(word) = decl.kind else {
        return fallback;
    };
    let loc = file.loc(word.0);
    let kind = match kinds.index.resolve(names, scopes.of(home), word.0) {
        Ok(kind) => kind,
        Err(crate::book::Miss::Unknown { suggestion }) => {
            diags.push(unknown(
                "unknown-kind",
                "kind",
                Word { text: word.0, loc },
                suggestion.map(|sym| names.name(sym)),
            ));
            return fallback;
        }
        Err(crate::book::Miss::Ambiguous(ids)) => {
            let candidates: Vec<_> = ids
                .iter()
                .map(|&id| names.name(kinds.tree[id].name))
                .collect();
            diags.push(
                Diagnostic::error("ambiguous-kind", format!("kind `{}` is ambiguous", word.0))
                    .label(loc, format!("could mean {}", candidates.join(" or "))),
            );
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
            Diagnostic::error(
                "kind-sort",
                format!("kind `{}` cannot classify this declaration", word.0),
            )
            .label(loc, format!("expected {expected:?}, found {found:?}")),
        );
        fallback
    }
}

fn resolve_purpose<'s>(
    name: &str,
    loc: Loc,
    home: Home,
    purposes: &crate::purposes::NativePurposes,
    names: &Interner<'s>,
    scopes: &Scopes,
    diags: &mut Vec<Diagnostic>,
) -> Option<Id<Purpose>> {
    match purposes.index.resolve(names, scopes.of(home), name) {
        Ok(purpose) => Some(purpose),
        Err(crate::book::Miss::Unknown { suggestion }) => {
            diags.push(unknown(
                "unknown-purpose",
                "purpose",
                Word { text: name, loc },
                suggestion.map(|sym| names.name(sym)),
            ));
            None
        }
        Err(crate::book::Miss::Ambiguous(ids)) => {
            let candidates: Vec<_> = ids
                .iter()
                .map(|&id| names.name(purposes.tree[id].name))
                .collect();
            diags.push(
                Diagnostic::error(
                    "ambiguous-purpose",
                    format!("purpose `{name}` is ambiguous"),
                )
                .label(loc, format!("could mean {}", candidates.join(" or "))),
            );
            None
        }
    }
}

fn first_name_prop<'a, 's>(
    file: &'a axiom_syntax::File<'s>,
    decl: &Decl<'s>,
    name: &str,
) -> Option<&'s str> {
    file[decl.props]
        .iter()
        .find(|prop| prop.name.0 == name)
        .and_then(|prop| file[prop.args].first())
        .and_then(|&arg| match file.exprs[arg].kind {
            ExprKind::Name(name) => Some(name.0),
            _ => None,
        })
}

fn owner_names_in<'a, 's>(file: &'a axiom_syntax::File<'s>, decl: &Decl<'s>) -> Vec<&'s str> {
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

fn resolve_owner_shares<'a, 's>(
    file: &'a axiom_syntax::File<'s>,
    decl: &Decl<'s>,
    home: Home,
    entities: &Scoped<Entity>,
    names: &Interner<'s>,
    scopes: &Scopes,
    fallback: Id<Entity>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Share> {
    let Some(line) = file[decl.props].iter().find(|prop| prop.name.0 == "owner") else {
        return Vec::new();
    };
    let mut resolved: Vec<(Id<Entity>, Option<Ratio>, Loc)> = Vec::new();
    for &arg in &file[line.args] {
        let expr = &file.exprs[arg];
        match expr.kind {
            ExprKind::Name(name) => match entities.resolve(names, scopes.of(home), name.0) {
                Ok(entity) => resolved.push((entity, None, expr.loc)),
                Err(crate::book::Miss::Unknown { suggestion }) => {
                    let suggestion = suggestion.map(|sym| names.name(sym));
                    diags.push(unknown(
                        "unknown-owner",
                        "owner",
                        Word {
                            text: name.0,
                            loc: expr.loc,
                        },
                        suggestion,
                    ));
                }
                Err(crate::book::Miss::Ambiguous(ids)) => {
                    let candidates: Vec<_> = ids
                        .iter()
                        .map(|&id| format!("entity #{}", id.index()))
                        .collect();
                    diags.push(
                        Diagnostic::error(
                            "ambiguous-owner",
                            format!("owner `{}` is ambiguous", name.0),
                        )
                        .label(expr.loc, format!("could mean {}", candidates.join(" or "))),
                    );
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
        return vec![Share {
            rate: Ratio::ONE,
            entity: fallback,
            measure: None,
            loc: line.loc,
        }];
    }
    let has_rate = resolved.iter().any(|(_, rate, _)| rate.is_some());
    let all_rate = resolved.iter().all(|(_, rate, _)| rate.is_some());
    if has_rate && !all_rate {
        diags.push(
            Diagnostic::error(
                "owner-share",
                "every owner in a shared place needs a percentage",
            )
            .label(line.loc, "write a percentage for each owner"),
        );
    }
    let equal = Ratio::new(1, resolved.len() as i128).unwrap_or(Ratio::ZERO);
    let shares: Vec<_> = resolved
        .into_iter()
        .map(|(entity, rate, loc)| Share {
            rate: rate.unwrap_or(equal),
            entity,
            measure: None,
            loc,
        })
        .collect();
    let total = shares
        .iter()
        .try_fold(Ratio::ZERO, |sum, share| sum.checked_add(share.rate));
    if shares.iter().any(|share| share.rate.is_negative()) || total != Some(Ratio::ONE) {
        diags.push(
            Diagnostic::error(
                "owner-share-total",
                "owner shares must be nonnegative and add to 100%",
            )
            .label(
                line.loc,
                "adjust the percentages so they total exactly 100%",
            ),
        );
    }
    shares
}

/// The declared place a full path is closest to, used in migration diagnostics.
pub(crate) fn near_place<'a>(path: &str, declared: &[&'a str]) -> Option<&'a str> {
    let limit = (path.len() / 8).clamp(1, 2);
    let root = path.split('/').next();
    declared
        .iter()
        .copied()
        .filter(|other| {
            other.len().abs_diff(path.len()) <= limit && other.split('/').next() == root
        })
        .map(|other| (axiom_core::diag::distance(path, other), other))
        .filter(|&(distance, _)| distance <= limit)
        .min()
        .map(|(_, other)| other)
}
