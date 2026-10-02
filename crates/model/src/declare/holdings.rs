//! What the owners hold and are owed: accounts, assets, and the tabs claims keep between parties.

use axiom_core::{Arena, Diagnostic, Id, Interner, Loc, Map, Set};
use axiom_syntax::DeclKind;

use super::commodities::{Commodities, commodity};
use super::parties::Entities;
use super::{Resolving, first_name_prop};
use crate::book::{Asset, Class, Entity, Kind, Share, Sort};
use crate::collect::Collected;
use crate::errors::Word;
use crate::lower::{JournalSurvey, Mention};
use crate::problem::{self, Noun};

pub(super) struct AccountDraft<'s> {
    pub path: &'s str,
    pub class: Class,
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    pub shares: Box<[Share]>,
    pub institution: Option<Id<Entity>>,
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
        let kind = resolving.kind(names, written, Sort::Place(Class::Asset), resolving.kind_roots.asset, diags);
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
            let scope = resolving.seeing.scopes.of(written.home());
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
            loc: file.loc(path),
        });
    }
    drafts
}

/// The owner of each account, by its path.
pub(super) fn owners_by_path<'s>(accounts: &[AccountDraft<'s>]) -> Map<&'s str, Id<Entity>> {
    accounts.iter().map(|account| (account.path, account.owner)).collect()
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
            props: Box::default(),
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

/// A claim between two parties, kept in the owner's books as a place of its own.
pub(super) struct TabDraft {
    pub party: Id<Entity>,
    pub owner: Id<Entity>,
    pub class: Class,
    pub loc: Loc,
}

/// The tabs the claims, contracts and `for` clauses a survey found need. They have no source path; their
/// identity is the (party, owner, class) they are between.
pub(super) fn find_tabs<'s>(
    survey: &JournalSurvey<'s>,
    entities: &Entities<'s>,
    account_owners: &Map<&'s str, Id<Entity>>,
) -> Vec<TabDraft> {
    let mut tabs: Vec<TabDraft> = Vec::new();
    let mut seen = Set::default();
    let mut add = |party: Id<Entity>, owner: Id<Entity>, class: Class, loc: Loc| {
        if party != owner && seen.insert((party, owner, class)) {
            tabs.push(TabDraft { party, owner, class, loc });
        }
    };
    let (me, entity) = (entities.me, |name: &str| entities.ids.get(name).copied());
    let account_owner =
        |name: Option<axiom_syntax::Name<'s>>| name.and_then(|name| account_owners.get(name.0).copied());
    for mention in &survey.mentions {
        match *mention {
            Mention::Claim { subject, creditor, loc } => {
                if let (Some(subject), Some(creditor)) = (entity(subject.0), entity(creditor.0)) {
                    let (party, owner, class) = if entities.holds[creditor.index()] {
                        (subject, creditor, Class::Asset)
                    } else if entities.holds[subject.index()] {
                        (creditor, subject, Class::Debt)
                    } else {
                        (subject, creditor, Class::Asset)
                    };
                    add(party, owner, class, loc);
                }
            }
            Mention::Promise { party, holding, loc, .. } => {
                if let Some(party) = entity(party.0) {
                    let owner = account_owner(holding).unwrap_or(me);
                    add(party, owner, Class::Asset, loc);
                    add(party, owner, Class::Debt, loc);
                }
            }
            Mention::For { other, ends, loc } => {
                if let Some(party) = entity(other.0) {
                    let owner = account_owner(ends.from).or_else(|| account_owner(ends.to)).unwrap_or(me);
                    add(party, owner, Class::Asset, loc);
                    add(party, owner, Class::Debt, loc);
                }
            }
            Mention::Due { ends, loc } | Mention::Ends { ends, loc } => {
                if let (Some(from), Some(to)) = (ends.from, ends.to) {
                    let (from_entity, to_entity) = (entity(from.0), entity(to.0));
                    let (from_owner, to_owner) = (account_owner(Some(from)), account_owner(Some(to)));
                    if let (Some(party), Some(owner)) = (from_entity, to_owner) {
                        add(party, owner, Class::Asset, loc);
                    }
                    if let (Some(party), Some(owner)) = (to_entity, from_owner) {
                        add(party, owner, Class::Debt, loc);
                    }
                }
            }
        }
    }
    tabs
}
