//! Parties: the entities a book has, those written and those a journal implies, and who owns what.

use axiom_core::{Diagnostic, Id, Interner, Loc, Map, Set, Sym, Tree};
use axiom_syntax::{Decl, DeclKind};

use super::commodities::Commodities;
use super::{Resolving, Said, add_path_spellings, owner_names_in, strict_path_suffixes};
use crate::book::{At, Entity, Purpose, Sort};
use crate::collect::{Collected, Written};
use crate::errors::Word;
use crate::lower::Mention;
use crate::names::Scoped;
use crate::problem::{self, Noun};
use crate::scope::Home;

/// The entities every book has: the owner, the unknown party, where openings come from, and the market.
const BUILT_IN: [&str; 4] = ["me", "?", "opening", "market"];

pub(super) struct EntityDraft<'s> {
    pub path: &'s str,
    pub home: Home,
}

/// An entity written in a source, and the doc comment on it.
type Written_<'a, 's> = (Written<'a, 's, Decl<'s>>, Option<Sym>);

/// Which entities there are to be made, before any is.
pub(super) struct Parties<'a, 's> {
    /// In the order they are made: the written, the built in, then the implied.
    pub drafts: Vec<EntityDraft<'s>>,
    written: Map<&'s str, Written_<'a, 's>>,
    /// Where each implied party is first mentioned.
    implied: Map<&'s str, Loc>,
    /// The names some account, entity or asset is owned by.
    pub owner_names: Set<&'s str>,
}

/// The entities, in the tree they are in for good, and how a name finds one.
pub(super) struct Entities<'s> {
    pub tree: Tree<Entity>,
    pub ids: Map<&'s str, Id<Entity>>,
    pub index: Scoped<Entity>,
    pub me: Id<Entity>,
    pub unknown: Id<Entity>,
    pub opening: Id<Entity>,
    pub market: Id<Entity>,
    /// Whether each entity holds what it owns, which it does when something is written as owned by it; the
    /// others keep an outside endpoint. `me` always holds.
    pub holds: Vec<bool>,
    /// The purposes written after entities, by the entity, and where.
    pub purposes: Vec<(Id<Entity>, At<Id<Purpose>>)>,
}

/// The entities written, those the journal and the contracts name that nothing declares, and the built-in ones.
pub(super) fn find<'a, 's>(
    said: Said<'_, 'a, 's>,
    resolving: &Resolving<'_>,
    commodities: &Commodities<'s>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Parties<'a, 's> {
    let (written, first_paths, owner_names) = written_entities(said.collected, names, diags);
    let implied = implied_parties(said, resolving, commodities, names, &written);
    // A written suffix such as `acme` can resolve to one implied path such as `vendors/acme`; a second `acme`
    // entity would make that reference ambiguous. All full paths are kept, so genuinely ambiguous suffixes are
    // diagnosed by the scoped entity resolver.
    let mut implied_paths: Vec<&'s str> = implied.keys().copied().collect();
    implied_paths.sort_unstable();
    let shadowed = strict_path_suffixes(&implied_paths);

    let mut drafts: Vec<_> =
        first_paths.iter().map(|&path| EntityDraft { path, home: written[path].0.home() }).collect();
    for root in BUILT_IN {
        if !written.contains_key(root) {
            drafts.push(EntityDraft { path: root, home: Home::Builtin });
        }
    }
    for path in implied_paths {
        if !written.contains_key(path) && !shadowed.contains(path) {
            drafts.push(EntityDraft { path, home: Home::Builtin });
        }
    }
    Parties { drafts, written, implied, owner_names }
}

/// The entities written, each once (a repeat is said), in the order written, and the names things are owned by.
fn written_entities<'a, 's>(
    collected: &Collected<'a, 's>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> (Map<&'s str, Written_<'a, 's>>, Vec<&'s str>, Set<&'s str>) {
    let mut written: Map<&'s str, Written_<'a, 's>> = Map::default();
    let mut first_paths = Vec::new();
    let mut owner_names = Set::default();
    for decl in &collected.decls {
        let (file, node) = (decl.file(), decl.node);
        if matches!(node.what, DeclKind::Account | DeclKind::Entity | DeclKind::Asset) {
            owner_names.extend(owner_names_in(file, node));
        }
        if node.what != DeclKind::Entity {
            continue;
        }
        let path = node.name.0;
        if let Some((first, _)) = written.get(path) {
            let first_at = Some(first.file().loc(first.node.name.0));
            diags.push(problem::duplicate(Noun::Entity, Word::of(file, path), first_at));
            continue;
        }
        let doc = decl.item.doc.map(|doc| names.intern(doc.0));
        written.insert(path, (*decl, doc));
        first_paths.push(path);
    }
    (written, first_paths, owner_names)
}

/// The parties that endpoint names and claims, contracts and `for` clauses mention and nothing declares, each with
/// where it is first mentioned: a name that is an account, an asset, a kind or any such thing is not a party.
fn implied_parties<'a, 's>(
    said: Said<'_, 'a, 's>,
    resolving: &Resolving<'_>,
    commodities: &Commodities<'s>,
    names: &Interner<'s>,
    written: &Map<&'s str, Written_<'a, 's>>,
) -> Map<&'s str, Loc> {
    let collected = said.collected;
    let (mentioned, roles) = mentions(said);
    let mut places = Set::default();
    for decl in collected.decls.iter().filter(|decl| matches!(decl.node.what, DeclKind::Account | DeclKind::Asset)) {
        add_path_spellings(&mut places, decl.node.name.0);
    }
    let mut entities = Set::default();
    for &path in written.keys() {
        add_path_spellings(&mut entities, path);
    }
    entities.extend(BUILT_IN);
    // Kinds, purposes, commodities, systems and assets.
    let mut others = Set::default();
    others.extend(resolving.kinds.index.names.keys(names));
    others.extend(resolving.purposes.index.names.keys(names));
    for &name in commodities.by_name.keys() {
        add_path_spellings(&mut others, name);
    }
    for (_, system) in resolving.seeing.systems.iter() {
        add_path_spellings(&mut others, names.name(system.path));
    }
    for decl in collected.decls_of(DeclKind::Asset) {
        add_path_spellings(&mut others, decl.node.name.0);
    }
    let mut contracts = Set::default();
    for contract in &collected.contracts {
        add_path_spellings(&mut contracts, contract.node.name.0);
    }
    let mut implied = Map::default();
    for (&path, &loc) in &mentioned {
        if entities.contains(path) || places.contains(path) || others.contains(path) {
            continue;
        }
        // A contract may be named for its party, but is not a party for being named.
        if matches!(path, "self" | "issuer") || (contracts.contains(path) && !roles.contains(path)) {
            continue;
        }
        implied.insert(path, loc);
    }
    implied
}

/// Every name the endpoints of the journal and the survey mention, with where it is first mentioned, and the
/// names mentioned as parties (a claim's, a `for`'s, a promise's) rather than only as ends.
fn mentions<'s>(said: Said<'_, '_, 's>) -> (Map<&'s str, Loc>, Set<&'s str>) {
    // Only one borrowed name and its first source location is kept, even when it occurs in many journal rows.
    let mut mentioned: Map<&'s str, Loc> = Map::default();
    let mut roles: Set<&'s str> = Set::default();
    crate::lower::visit_endpoints(said.sites, |_, name, loc, _| {
        mentioned.entry(name.0).or_insert(loc);
    });
    for mention in &said.survey.mentions {
        match *mention {
            Mention::Claim { subject, creditor, loc } => {
                mentioned.entry(subject.0).or_insert(loc);
                mentioned.entry(creditor.0).or_insert(loc);
                roles.extend([subject.0, creditor.0]);
            }
            Mention::For { other, loc, .. } => {
                mentioned.entry(other.0).or_insert(loc);
                roles.insert(other.0);
            }
            Mention::Promise { party, loc, .. } => {
                mentioned.entry(party.0).or_insert(loc);
                roles.insert(party.0);
            }
            Mention::Ends { .. } | Mention::Due { .. } => {}
        }
    }
    (mentioned, roles)
}

/// The entities made from the drafts, with their kinds and purposes resolved, indexed, and owned.
pub(super) fn declare<'a, 's>(
    collected: &Collected<'a, 's>,
    parties: Parties<'a, 's>,
    resolving: &Resolving<'_>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Entities<'s> {
    let Parties { drafts, written, implied, owner_names } = parties;
    let home_of: Map<&str, Home> = drafts.iter().map(|draft| (draft.path, draft.home)).collect();
    let mut purposes: Vec<(&str, At<Id<Purpose>>)> = Vec::new();
    let (mut tree, ids) = crate::paths::build(drafts.iter().map(|draft| draft.path), |path| {
        let (kind, purpose, doc, loc) = match written.get(path) {
            Some((decl, doc)) => {
                let (file, node) = (decl.file(), decl.node);
                let kind = resolving.kind(names, decl, Sort::Entity, resolving.kind_roots.entity, diags);
                let purpose = node.purpose.and_then(|name| {
                    resolving
                        .purpose(names, name.0, file.loc(name.0), decl.home(), diags)
                        .map(|value| At { value, loc: file.loc(name.0) })
                });
                (kind, purpose, *doc, Some(file.loc(node.name.0)))
            }
            None => (resolving.kind_roots.entity, None, None, implied.get(path).copied()),
        };
        purposes.extend(purpose.map(|purpose| (path, purpose)));
        Entity {
            path: names.intern(path),
            kind,
            place: None,
            owner: None,
            client_of: None,
            owned_by: Box::default(),
            known_as: Box::default(),
            doc,
            loc,
        }
    });
    let built_in = |name: &str, what: &str| *ids.get(name).unwrap_or_else(|| panic!("the {what} entity is created"));
    let (me, unknown) = (built_in("me", "book's owner"), built_in("?", "unknown"));
    let (opening, market) = (built_in("opening", "opening"), built_in("market", "market"));
    let mut homes = vec![Home::Builtin; tree.len()];
    for (path, &id) in &ids {
        homes[id.index()] = home_of.get(path).copied().unwrap_or(Home::Builtin);
    }
    let indexed: Vec<_> = tree.iter().map(|(id, entity)| (id, names.name(entity.path), homes[id.index()])).collect();
    let index = Scoped::build(names, indexed);

    // Ownership belongs to entities as well as accounts. It is built after every entity id exists, in the order
    // the declarations are written.
    for decl in collected.decls_of(DeclKind::Entity) {
        let Some(&entity) = ids.get(decl.node.name.0) else {
            continue;
        };
        let owners = resolving.owners(names, decl, &index, &tree, me, diags);
        if let Some(share) = owners.first() {
            tree[entity].owner = Some(share.entity);
            tree[entity].owned_by = owners.into_boxed_slice();
        }
    }
    let mut holds = vec![false; tree.len()];
    for (path, &id) in &ids {
        holds[id.index()] = *path == "me" || owner_names.contains(path);
    }
    let purposes = purposes.into_iter().map(|(path, purpose)| (ids[path], purpose)).collect();
    Entities { tree, ids, index, me, unknown, opening, market, holds, purposes }
}
