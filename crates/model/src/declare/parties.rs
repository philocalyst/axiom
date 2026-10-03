//! Parties: the entities a book has, those written and those a journal implies, and who owns what.

use axiom_core::{Diagnostic, Id, Interner, Loc, Map, Set, Sym, Tree};
use axiom_syntax::{Decl, DeclKind, ExprKind};

use super::commodities::Commodities;
use super::mentions::Mentions;
use super::{Resolving, Said, add_path_spellings, owner_names_in, strict_path_suffixes};
use crate::book::{At, Entity, Purpose, Sort};
use crate::collect::{Collected, Written};
use crate::errors::Word;
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

/// The entities written, those the journal and the contracts name that nothing declares, and the built-in ones; and the
/// names of two words or more that a journal or a contract writes as an end, in the order they sort: what a reference
/// may be an address of.
pub(super) fn find<'a, 's>(
    said: Said<'_, 'a, 's>,
    resolving: &Resolving<'_>,
    commodities: &Commodities<'s>,
    names: &mut Interner<'s>,
    diags: &mut Vec<Diagnostic>,
) -> (Parties<'a, 's>, Vec<&'s str>) {
    let (written, first_paths, owner_names) = written_entities(said.collected, names, diags);
    let (implied, references) = implied_parties(said, resolving, commodities, names, &written);
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
    (Parties { drafts, written, implied, owner_names }, references)
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
/// where it is first mentioned: a name that is an account, an asset, a kind or any such thing is not a party. And
/// every name of two words or more that is mentioned, whatever it is.
fn implied_parties<'a, 's>(
    said: Said<'_, 'a, 's>,
    resolving: &Resolving<'_>,
    commodities: &Commodities<'s>,
    names: &Interner<'s>,
    written: &Map<&'s str, Written_<'a, 's>>,
) -> (Map<&'s str, Loc>, Vec<&'s str>) {
    let collected = said.collected;
    let Mentions { first: mentioned, parties, .. } = Mentions::of(said.sites);
    let references = of_two_words(&mentioned);
    let mut places = Set::default();
    for decl in collected.decls.iter().filter(|decl| matches!(decl.node.what, DeclKind::Account | DeclKind::Asset)) {
        add_path_spellings(&mut places, decl.node.name.0);
    }
    let mut entities = spellings(written.keys().copied());
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
    let contracts = spellings(collected.contracts.iter().map(|contract| contract.node.name.0));
    let meant = Addressed::of(collected, written);
    let mut implied = Map::default();
    for (&path, &loc) in &mentioned {
        if entities.contains(path) || places.contains(path) || others.contains(path) || meant.is_an_address(path) {
            continue;
        }
        // A contract may be named for its party, but is not a party for being named.
        if matches!(path, "self" | "issuer") || (contracts.contains(path) && !parties.contains(path)) {
            continue;
        }
        implied.insert(path, loc);
    }
    (implied, references)
}

/// Every spelling a path of `paths` is written by: the path, each prefix of it, and each suffix of those.
fn spellings<'s>(paths: impl Iterator<Item = &'s str>) -> Set<&'s str> {
    let mut spellings = Set::default();
    for path in paths {
        add_path_spellings(&mut spellings, path);
    }
    spellings
}

/// The names of two words or more, in the order they sort: what a reference may be an address of.
fn of_two_words<'s>(mentioned: &Map<&'s str, Loc>) -> Vec<&'s str> {
    let mut references: Vec<&'s str> = mentioned.keys().copied().filter(|name| name.contains('/')).collect();
    references.sort_unstable();
    references
}

/// What the accounts' declarations say a reference of two words or more may be meant as: the written entities that fill
/// some account's slots (before its name, after `at`, or as an argument of one of its lines), and the names accounts
/// are called. A path that begins with one of the first or ends in one of the second is meant as an address: if no
/// account has it, that is for the lookup to say, and the journal brings no party into being by it. Only in a book that
/// writes some account as an address: any other is read as it always was, and its mentions make the parties they made.
struct Addressed<'s> {
    fillers: Set<&'s str>,
    names: Set<&'s str>,
    used: bool,
}

impl<'s> Addressed<'s> {
    fn of(collected: &Collected<'_, 's>, written: &Map<&'s str, Written_<'_, 's>>) -> Addressed<'s> {
        let (mut fillers, mut names, mut used) = (Set::default(), Set::default(), false);
        let is_entity = |&word: &&str| written.contains_key(word) || word == "me";
        for decl in collected.decls_of(DeclKind::Account) {
            let (file, node) = (decl.file(), decl.node);
            let leading: Vec<&str> =
                node.name.0.rsplit_once('/').map_or(Vec::new(), |(words, _)| words.split('/').collect());
            used |= !leading.is_empty() && leading.iter().all(is_entity);
            let lines = file[node.props].iter().flat_map(|line| file[line.args].iter());
            let args = lines.filter_map(|&arg| match file.exprs[arg].kind {
                ExprKind::Name(name) => Some(name.0),
                _ => None,
            });
            let named = leading.into_iter().chain(node.at.map(|at| at.0)).chain(args);
            fillers.extend(named.filter(is_entity));
            names.extend(node.name.0.rsplit('/').next());
        }
        Addressed { fillers, names, used }
    }

    /// Whether a path of two words or more begins with a filler or ends in an account's name.
    fn is_an_address(&self, path: &str) -> bool {
        self.used
            && (path.split_once('/').is_some_and(|(first, _)| self.fillers.contains(first))
                || path.rsplit_once('/').is_some_and(|(_, last)| self.names.contains(last)))
    }
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
