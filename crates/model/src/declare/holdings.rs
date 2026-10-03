//! What the owners hold: accounts and assets.

use axiom_core::{Arena, Diagnostic, Id, Interner, Loc, Map};
use axiom_syntax::DeclKind;

use super::commodities::{Commodities, commodity};
use super::parties::Entities;
use super::{Resolving, first_name_prop};
use crate::book::{Asset, Class, Entity, Kind, Share, Sort};
use crate::collect::Collected;
use crate::errors::Word;
use crate::problem::{self, Noun};
use crate::spelled::leading;

pub(super) struct AccountDraft<'s> {
    pub path: &'s str,
    pub class: Class,
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    pub shares: Box<[Share]>,
    pub institution: Option<Id<Entity>>,
    /// Written with the entities that fill its slots before its name: a root of the tree, with no place for a prefix.
    pub spelled: bool,
    pub loc: Loc,
}

/// The accounts written, each once (a repeat is said). They can refer forward to kinds and owners because both
/// indexes are complete.
pub(super) fn declare_accounts<'a, 's>(
    collected: &Collected<'a, 's>,
    resolving: &Resolving<'_>,
    entities: &Entities<'s>,
    names: &Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<AccountDraft<'s>> {
    let mut drafts = Vec::new();
    let mut declared: Map<&'s str, Loc> = Map::default();
    for written in collected.decls_of(DeclKind::Account) {
        let (file, decl) = (written.file(), written.node);
        let path = decl.name.0;
        if let Some(&first) = declared.get(path) {
            diags.push(problem::duplicate(Noun::Account, Word::of(file, path), Some(first)));
            continue;
        }
        declared.insert(path, file.loc(path));
        let scope = resolving.seeing.scopes.of(written.home());
        let fillers = leading(&entities.index, &entities.tree, names, scope, path);
        let kind = resolving.account_kind(names, written, fillers.as_deref(), diags);
        let class = match resolving.kinds.tree[kind].sort {
            Sort::Place(class) => class,
            found => {
                diags.push(
                    axiom_core::Diagnostic::error("account-kind-sort", "an account needs a place kind")
                        .label(file.loc(path), format!("this kind classifies {found:?}")),
                );
                Class::Asset
            }
        };
        let shares = resolving.owners(names, written, &entities.index, &entities.tree, entities.me, diags);
        let owner = shares.first().map_or_else(
            || {
                first_name_prop(file, decl, "owner")
                    .and_then(|name| entities.ids.get(name).copied())
                    .unwrap_or(entities.me)
            },
            |share| share.entity,
        );
        let institution = decl.at.and_then(|name| {
            entities
                .index
                .resolve(names, scope, name.0)
                .map_err(|_| {
                    diags.push(
                        Diagnostic::error("unknown-institution", format!("institution `{}` is not visible", name.0))
                            .label(file.loc(name.0), "not a visible entity"),
                    )
                })
                .ok()
        });
        drafts.push(AccountDraft {
            path,
            class,
            kind,
            owner,
            shares: shares.into_boxed_slice(),
            institution,
            spelled: fillers.is_some(),
            loc: file.loc(path),
        });
    }
    drafts
}

/// The assets written, each with the commodity that counts it.
pub(super) struct Assets<'s> {
    pub arena: Arena<Asset>,
    /// In the order written.
    pub paths: Vec<(&'s str, Id<Asset>)>,
    pub by_name: Map<&'s str, Id<Asset>>,
    pub shares: Map<Id<Asset>, Box<[Share]>>,
}

/// The assets written, each once (a repeat is said); an asset is counted in a commodity of its own.
pub(super) fn declare_assets<'a, 's>(
    collected: &Collected<'a, 's>,
    resolving: &Resolving<'_>,
    entities: &Entities<'s>,
    commodities: &mut Commodities<'s>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Assets<'s> {
    let mut assets = Assets { arena: Arena::new(), paths: Vec::new(), by_name: Map::default(), shares: Map::default() };
    for written in collected.decls_of(DeclKind::Asset) {
        let (file, decl) = (written.file(), written.node);
        let path = decl.name.0;
        if let Some(&first) = assets.by_name.get(path) {
            diags.push(problem::duplicate(Noun::Asset, Word::of(file, path), Some(assets.arena[first].loc)));
            continue;
        }
        let kind = resolving.kind(names, written, Sort::Thing, resolving.kind_roots.thing, diags);
        let name = names.intern(path);
        let unit =
            commodities.arena.push(commodity(name, resolving.kind_roots.commodity, 0, None, Some(file.loc(path))));
        commodities.by_name.entry(path).or_insert(unit);
        let owners = resolving.owners(names, written, &entities.index, &entities.tree, entities.me, diags);
        let owner = owners.first().map_or(entities.me, |share| share.entity);
        let asset = assets.arena.push(Asset {
            name,
            kind,
            owner,
            place: Id::new(0),
            unit,
            part_of: None,
            doc: written.item.doc.map(|doc| names.intern(doc.0)),
            loc: file.loc(path),
        });
        if !owners.is_empty() {
            assets.shares.insert(asset, owners.into_boxed_slice());
        }
        assets.by_name.insert(path, asset);
        assets.paths.push((path, asset));
    }
    assets
}
