//! The place tree: somewhere to put a flow for every account, asset, party, issuer and claim, frozen once.
//!
//! Account, entity and asset paths are kept in disjoint namespaces while each gets a stable id in the tree, and
//! every path and prefix is borrowed from the source.

use axiom_core::{Id, Interner, Map, Set, Tree};
use axiom_syntax::DeclKind;

use super::commodities::Commodities;
use super::holdings::{AccountDraft, Assets, TabDraft};
use super::parties::Entities;
use super::{Resolving, Tabs, is_path_child, path_key};
use crate::book::{Asset, Basis, Class, Commodity, Entity, Place, Role, Sort};
use crate::collect::Collected;
use crate::names::Names;

const ACCOUNTS: u8 = 0;
const ENTITY_PLACES: u8 = 1;
const ASSET_PLACES: u8 = 2;

/// A path in the namespace of what it names.
type Key<'s> = (u8, &'s str);

/// What the tree is made of, written.
pub(super) struct PlaceInputs<'x, 'a, 's> {
    pub collected: &'x Collected<'a, 's>,
    pub resolving: &'x Resolving<'x>,
    pub commodities: &'x Commodities<'s>,
    pub accounts: &'x [AccountDraft<'s>],
    pub tabs: &'x [TabDraft],
}

/// The tree, and the places that have a name of their own.
pub(super) struct Places {
    pub tree: Tree<Place>,
    /// The place each commodity that pays is issued from.
    pub issuers: Map<Id<Commodity>, Id<Place>>,
    /// The place of each claim, by (party, owner, class).
    pub tabs: Tabs,
    pub names: Names<Place>,
}

/// The tree of every place, with each entity and asset told where its own is.
pub(super) fn declare<'s>(
    inputs: &PlaceInputs<'_, '_, 's>,
    entities: &mut Entities<'s>,
    assets: &mut Assets<'s>,
    names: &mut Interner<'s>,
) -> Places {
    let paths = keys(inputs.accounts, entities, assets);
    let positions: Map<Key<'s>, usize> = paths.iter().enumerate().map(|(at, &key)| (key, at)).collect();
    let issuer_units = issuer_units(inputs);
    let nodes = Nodes::of(inputs, entities, assets);
    let mut place_nodes = Vec::with_capacity(paths.len() + inputs.tabs.len() + issuer_units.len());
    let mut parents = Vec::with_capacity(place_nodes.capacity());
    let mut indexed: Map<Key<'s>, (usize, bool)> = Map::default();
    for &(namespace, path) in &paths {
        let (node, is_indexed) = nodes.node(names, namespace, path);
        indexed.insert((namespace, path), (place_nodes.len(), is_indexed));
        place_nodes.push(node);
        parents.push(path.rsplit_once('/').and_then(|(parent, _)| positions.get(&(namespace, parent)).copied()));
    }
    let issuer_at: Vec<_> = issuer_units
        .iter()
        .map(|&unit| {
            place_nodes.push(issuer_node(inputs, entities, unit));
            parents.push(None);
            (unit, place_nodes.len() - 1)
        })
        .collect();
    let tab_at: Vec<_> = inputs
        .tabs
        .iter()
        .map(|tab| {
            place_nodes.push(tab_node(inputs, entities, tab));
            parents.push(None);
            place_nodes.len() - 1
        })
        .collect();
    let (tree, remap) = Tree::build(place_nodes, &parents).expect("place parents are prefixes without cycles");

    let mut place_names = Names::default();
    for (&(namespace, path), &(old, is_indexed)) in &indexed {
        if is_indexed && (namespace == ACCOUNTS || namespace == ASSET_PLACES) {
            place_names.insert_path(names, path, remap[old]);
        }
    }
    for (&path, &entity) in &entities.ids {
        if let Some(&(old, _)) = indexed.get(&(ENTITY_PLACES, path)) {
            entities.tree[entity].place = Some(remap[old]);
        }
    }
    for &(path, asset) in &assets.paths {
        if let Some(&(old, _)) = indexed.get(&(ASSET_PLACES, path)) {
            assets.arena[asset].place = remap[old];
        }
    }
    Places {
        tree,
        issuers: issuer_at.into_iter().map(|(unit, old)| (unit, remap[old])).collect(),
        tabs: inputs
            .tabs
            .iter()
            .zip(tab_at)
            .map(|(tab, old)| ((tab.party, tab.owner, tab.class), remap[old]))
            .collect(),
        names: place_names,
    }
}

/// Every path of the three namespaces and each of its prefixes, ordered as the tree will have them.
fn keys<'s>(accounts: &[AccountDraft<'s>], entities: &Entities<'s>, assets: &Assets<'s>) -> Vec<Key<'s>> {
    let mut keys: Set<Key<'s>> = Set::default();
    let mut add =
        |namespace: u8, path: &'s str| keys.extend(crate::paths::prefixes(path).map(|prefix| (namespace, prefix)));
    for account in accounts {
        add(ACCOUNTS, account.path);
    }
    for &path in entities.ids.keys() {
        add(ENTITY_PLACES, path);
    }
    for &(path, _) in &assets.paths {
        add(ASSET_PLACES, path);
    }
    let mut keys: Vec<_> = keys.into_iter().collect();
    keys.sort_unstable_by(|(ns_a, a), (ns_b, b)| ns_a.cmp(ns_b).then_with(|| path_key(a).cmp(path_key(b))));
    keys
}

/// The commodities whose kind pays (is, or inherits from, a kind that says `pays`), each of which is issued from
/// a place of its own. `pays` is inherited once the place tree is frozen, so this reads the same first
/// declarations the property pass does: per-commodity issuer identity, and no endpoint per kind.
fn issuer_units(inputs: &PlaceInputs<'_, '_, '_>) -> Vec<Id<Commodity>> {
    let kinds = inputs.resolving.kinds;
    let mut own_pays = vec![false; kinds.tree.len()];
    let mut seen = Set::default();
    for (written, &kind) in inputs.collected.decls_of(DeclKind::Kind).zip(&kinds.declarations) {
        let (file, decl) = (written.file(), written.node);
        if seen.insert(kind) && kinds.tree[kind].sort == Sort::Commodity {
            own_pays[kind.index()] = file[decl.props].iter().any(|prop| prop.name.0 == "pays");
        }
    }
    let mut inherited = vec![false; kinds.tree.len()];
    for (kind, _) in kinds.tree.iter() {
        inherited[kind.index()] =
            own_pays[kind.index()] || kinds.tree.parent(kind).is_some_and(|parent| inherited[parent.index()]);
    }
    let commodities = &inputs.commodities.arena;
    commodities.iter().filter_map(|(unit, commodity)| inherited[commodity.kind.index()].then_some(unit)).collect()
}

/// What decides the place a path is: the accounts, assets and entities written at it or beneath it.
struct Nodes<'x, 's> {
    inputs: &'x PlaceInputs<'x, 'x, 's>,
    entities: &'x Entities<'s>,
    assets: &'x Assets<'s>,
    account_at: Map<&'s str, &'x AccountDraft<'s>>,
    asset_at: Map<&'s str, Id<Asset>>,
}

/// The five facts a place has from where it is written.
struct Origin {
    class: Class,
    role: Role,
    kind: Id<crate::book::Kind>,
    owner: Id<Entity>,
    loc: Option<axiom_core::Loc>,
    /// Whether a name written for it finds it, or it exists only as a prefix of one that does.
    indexed: bool,
}

impl<'x, 's> Nodes<'x, 's> {
    fn of(inputs: &'x PlaceInputs<'x, 'x, 's>, entities: &'x Entities<'s>, assets: &'x Assets<'s>) -> Nodes<'x, 's> {
        Nodes {
            inputs,
            entities,
            assets,
            account_at: inputs.accounts.iter().map(|account| (account.path, account)).collect(),
            asset_at: assets.paths.iter().copied().collect(),
        }
    }

    /// The place a path in a namespace is, and whether a name finds it.
    fn node(&self, names: &mut Interner<'s>, namespace: u8, path: &'s str) -> (Place, bool) {
        let account = (namespace == ACCOUNTS).then(|| self.account_at.get(path).copied()).flatten();
        let asset = (namespace == ASSET_PLACES).then(|| self.asset_at.get(path).copied()).flatten();
        let origin = self.origin(namespace, path, account, asset);
        let shares = account.map_or_else(
            || asset.and_then(|asset| self.assets.shares.get(&asset).cloned()).unwrap_or_default(),
            |account| account.shares.clone(),
        );
        let place = Place {
            path: names.intern(path),
            class: origin.class,
            role: origin.role,
            kind: origin.kind,
            owner: origin.owner,
            holds: None,
            select: None,
            deferred: false,
            basis: Basis::Cost,
            claim: false,
            liquidity: None,
            opened: None,
            closed: None,
            shares,
            known_as: Box::default(),
            props: Box::default(),
            doc: None,
            loc: origin.loc,
        };
        (place, origin.indexed)
    }

    /// Where a path comes from: an account or an asset written at it, an entity, or one written beneath it, or
    /// nothing, and it is a prefix and no more.
    fn origin(
        &self,
        namespace: u8,
        path: &'s str,
        account: Option<&AccountDraft<'s>>,
        asset: Option<Id<Asset>>,
    ) -> Origin {
        let (roots, me) = (&self.inputs.resolving.kind_roots, self.entities.me);
        let entity = (namespace == ENTITY_PLACES).then(|| self.entities.ids.get(path).copied()).flatten();
        if let Some(account) = account {
            let role = Role::Account { institution: account.institution };
            return Origin {
                class: account.class,
                role,
                kind: account.kind,
                owner: account.owner,
                loc: Some(account.loc),
                indexed: true,
            };
        }
        if let Some(asset) = asset {
            let record = &self.assets.arena[asset];
            return Origin {
                class: Class::Asset,
                role: Role::Asset(asset),
                kind: record.kind,
                owner: record.owner,
                loc: Some(record.loc),
                indexed: true,
            };
        }
        if let Some(entity) = entity {
            let held = self.entities.holds[entity.index()];
            let (class, role) = if held {
                (Class::Asset, Role::Holding(entity))
            } else {
                (Class::Outside, Role::Outside(Some(entity)))
            };
            let (kind, owner) = if held { (roots.asset, entity) } else { (roots.entity, me) };
            return Origin { class, role, kind, owner, loc: self.entities.tree[entity].loc, indexed: false };
        }
        let beneath = |holder: &str| is_path_child(path, holder);
        let account_beneath =
            (namespace == ACCOUNTS).then(|| self.inputs.accounts.iter().find(|draft| beneath(draft.path))).flatten();
        let asset_beneath = (namespace == ASSET_PLACES)
            .then(|| {
                self.assets.paths.iter().find(|(child, _)| beneath(child)).map(|(_, asset)| &self.assets.arena[*asset])
            })
            .flatten();
        let role = Role::Account { institution: None };
        if let Some(account) = account_beneath {
            return Origin {
                class: account.class,
                role,
                kind: account.kind,
                owner: account.owner,
                loc: None,
                indexed: true,
            };
        }
        if let Some(asset) = asset_beneath {
            return Origin {
                class: Class::Asset,
                role,
                kind: asset.kind,
                owner: asset.owner,
                loc: None,
                indexed: true,
            };
        }
        Origin { class: Class::Asset, role, kind: roots.asset, owner: me, loc: None, indexed: false }
    }
}

/// A place nothing is owned in: where a commodity that pays is issued from.
fn issuer_node(inputs: &PlaceInputs<'_, '_, '_>, entities: &Entities<'_>, unit: Id<Commodity>) -> Place {
    let commodity = &inputs.commodities.arena[unit];
    Place {
        path: commodity.symbol,
        class: Class::Outside,
        role: Role::Issuer(unit),
        kind: inputs.resolving.kind_roots.entity,
        owner: entities.me,
        holds: None,
        select: None,
        deferred: false,
        basis: Basis::Cost,
        claim: false,
        liquidity: None,
        opened: None,
        closed: None,
        shares: Box::default(),
        known_as: Box::default(),
        props: Box::default(),
        doc: commodity.doc,
        loc: commodity.loc,
    }
}

/// A claim's place. Its printed label is the party's name, while its identity is the typed (party, owner, class).
fn tab_node(inputs: &PlaceInputs<'_, '_, '_>, entities: &Entities<'_>, tab: &TabDraft) -> Place {
    let roots = &inputs.resolving.kind_roots;
    Place {
        path: entities.tree[tab.party].path,
        class: tab.class,
        role: Role::Tab(tab.party),
        kind: if tab.class == Class::Debt { roots.debt } else { roots.asset },
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
    }
}
