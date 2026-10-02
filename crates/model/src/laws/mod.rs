//! Laws: written in systems, kinds, accounts, entities and the project, compiled
//! to typed nodes.
//!
//! Where a law is written decides what it governs (its [`Owner`]) and what
//! `self` is inside it. A trigger only fits some owners: value entering a place
//! is for laws about places, and money leaving a restricted entity's reach is
//! for laws about entities. Laws are numbered in the order written, systems
//! first, which is the order that decides ties between laws that do not depend
//! on each other.

mod budget;
mod compile;
mod order;
mod types;
mod vars;

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Set};
use axiom_syntax::{self as ast, DeclKind, ItemKind, Trigger as Written};

pub(crate) use self::compile::compile_template;
use self::compile::{Placement, compile};
pub(crate) use self::order::rank;
use crate::book::{AlsoOn, Kind, Sort, System};
use crate::declare::World;
use crate::errors::{Candidate, Reported, Word};
use crate::law::{Law, Owner, Rank, RankClass, Ty};
use crate::lower::also::{AlsoCx, lower_alsos};
use crate::names::Rank as NameRank;
use crate::problem::{self, Noun};
use crate::scope::Home;
use crate::sources::Site;

pub(crate) fn declare<'s>(world: &mut World<'s>, sites: &[Site<'_, 's>], diags: &mut Vec<Diagnostic>) {
    world.tallies = counted(sites);
    let mut seen: Set<(DeclKind, u32)> = Set::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Law(id) => {
                    let owner = match site.home {
                        Home::System(system) => Owner::System(system),
                        Home::Project | Home::Builtin => Owner::Book,
                    };
                    compile_native(world, diags, file, site.home, owner, Ty::Entity, &file[id]);
                }
                ItemKind::Decl(id) => declare_in(world, site, &file[id], &mut seen, diags),
                // Contract laws are compiled by the contract pass after every
                // contract id exists, so references can point forward.
                ItemKind::Contract(_)
                | ItemKind::Opening(_)
                | ItemKind::Txn(_)
                | ItemKind::Statement(_)
                | ItemKind::Budget(_)
                | ItemKind::Code(_)
                | ItemKind::Param(_)
                | ItemKind::Sync(_)
                | ItemKind::Pattern(_)
                | ItemKind::Format(_)
                | ItemKind::Setting(_) => {}
            }
        }
    }
    budget::declare(world, sites, diags);
}

/// What the laws written under a declaration govern: their owner, what `self` is inside them, and the key that
/// tells the declaration from the others of its kind.
struct Governed {
    owner: Owner,
    subject: Ty,
    key: u32,
}

/// The laws and `also` lines written under a declaration. A declaration's laws are compiled once, however many
/// sources spell it.
fn declare_in<'s>(
    world: &mut World<'s>,
    site: &Site<'_, 's>,
    decl: &ast::Decl<'s>,
    seen: &mut Set<(DeclKind, u32)>,
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let Some(Governed { owner, subject, key }) = governed(world, site.home, file, decl, diags) else {
        return;
    };
    if seen.insert((decl.what, key)) {
        for law in &file[decl.laws] {
            compile_native(world, diags, file, site.home, owner, subject, law);
        }
    }
    declare_alsos(world, diags, file, site.home, decl, owner, decl.what);
}

/// What a declaration's laws govern, or None after saying why they govern nothing.
fn governed<'s>(
    world: &World<'s>,
    home: Home,
    file: &ast::File<'s>,
    decl: &ast::Decl<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Governed> {
    let word = Word::of(file, decl.name.0);
    let (owner, subject, key) = match decl.what {
        DeclKind::Kind => {
            let kind = world.kind(home, word).or_report(diags)?;
            let subject = match world.book.kinds[kind].sort {
                Sort::Place(_) => Ty::Place,
                Sort::Entity => Ty::Entity,
                Sort::Thing => Ty::Asset,
                Sort::Commodity => {
                    misplaced(diags, file, decl.laws, "a commodity kind");
                    return None;
                }
            };
            (Owner::Kind(kind), subject, kind.index())
        }
        DeclKind::Account => {
            let place = world.place(word).or_report(diags)?;
            (Owner::Place(place), Ty::Place, place.index())
        }
        DeclKind::Entity => {
            let entity = world.entity(home, word).or_report(diags)?;
            (Owner::Entity(entity), Ty::Entity, entity.index())
        }
        DeclKind::Asset => {
            let asset = world.book.asset(word.text).ok_or_else(|| world.missing_asset(word)).or_report(diags)?;
            (Owner::Asset(asset), Ty::Asset, asset.index())
        }
        DeclKind::Purpose => {
            // Purpose laws govern purpose-bearing flows, but `self` is the owner of the flow (LANGUAGE §8).
            // `total(window)` retains the purpose context in the law owner instead of changing `self`'s type.
            let purpose = world.purpose(home, word).or_report(diags)?;
            (Owner::Purpose(purpose), Ty::Entity, purpose.index())
        }
        DeclKind::Commodity => {
            misplaced(diags, file, decl.laws, "a commodity");
            return None;
        }
    };
    Some(Governed { owner, subject, key: key as u32 })
}

/// Compiles a nested or native S5 law using the same typed compiler as
/// top-level declaration laws, and adds it to the owning book.
pub(crate) fn compile_native<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    file: &ast::File<'s>,
    home: Home,
    owner: Owner,
    subject: Ty,
    law: &ast::Law<'s>,
) -> Option<Id<Law>> {
    if let Err(problem) = fits(world, owner, law) {
        diags.push(problem);
        return None;
    }
    if law.damaged {
        return None;
    }
    let site = Placement { file, home, owner, subject };
    let compiled = compile(world, diags, &site, law)?;
    Some(push(world, compiled))
}

/// Rebuilds the per-kind and per-system law runs after native lowering added
/// nested laws to the Book arena.
pub(crate) fn register_native(world: &mut World<'_>, diags: &mut Vec<Diagnostic>) {
    register(world);
    resolve_overrides(world, diags);
    set_specificity(world);
    let order = rank(&world.book, diags);
    crate::rules::govern(&mut world.book, &order);
}

/// Resolve `overrides` after every top-level and nested law has a stable id.
fn resolve_overrides(world: &mut World<'_>, diags: &mut Vec<Diagnostic>) {
    let pending: Vec<_> =
        world.book.laws.iter().filter_map(|(id, law)| law.override_name.map(|name| (id, name, law.loc))).collect();
    for (id, name, loc) in pending {
        let text = world.book.name(name);
        let home = law_home(&world.book.laws[id]);
        let scope = world.scopes.of(home);
        let names = &world.book.names;
        let table = &world.book.lookup.laws;
        let mut candidates: Vec<_> = table
            .candidates(names, text)
            .iter()
            .copied()
            .filter(|&candidate| scope.sees(law_home(&world.book.laws[candidate])))
            .collect();
        if let Some(nearest) =
            candidates.iter().map(|&candidate| scope.rank(law_home(&world.book.laws[candidate]))).min()
        {
            candidates.retain(|&candidate| scope.rank(law_home(&world.book.laws[candidate])) == nearest);
        }
        match candidates.as_slice() {
            [target] if *target != id => world.book.laws[id].overrides = Some(*target),
            [_target] => diags.push(
                Diagnostic::error("law-override", "a law cannot override itself")
                    .label(loc, "this is the law's own name"),
            ),
            [] => {
                let visible = table.keys(names).filter(|key| {
                    (table.candidates(names, key).iter())
                        .any(|&candidate| scope.sees(law_home(&world.book.laws[candidate])))
                });
                let nearest = closest(text, visible);
                diags.push(problem::unknown(Noun::Law, Word { text, loc }, nearest));
            }
            targets => {
                let describe = |&target: &Id<Law>| {
                    let law = &world.book.laws[target];
                    Candidate { is: format!("`{}`", world.book.name(law.name)), declared: Some(law.loc), write: None }
                };
                let candidates: Vec<_> = targets.iter().map(describe).collect();
                diags.push(problem::ambiguous(Noun::Law, Word { text, loc }, &candidates));
            }
        }
    }
}

fn law_home(law: &Law) -> Home {
    law.system.map_or(Home::Project, Home::System)
}

/// More specific owners win when two laws govern the same occasion.
fn set_specificity(world: &mut World<'_>) {
    let book = &world.book;
    let ranks: Vec<_> = book
        .laws
        .iter()
        .map(|(_, law)| match law.owner {
            Owner::Book => Rank::scoped(RankClass::Book, 0),
            Owner::System(system) => Rank::scoped(RankClass::System, book.systems.lineage(system).count() as u32),
            Owner::Kind(kind) => Rank::scoped(RankClass::Kind, book.kinds.lineage(kind).count() as u32),
            Owner::Purpose(purpose) => Rank::scoped(RankClass::Purpose, book.purposes.lineage(purpose).count() as u32),
            Owner::Place(_) | Owner::Entity(_) | Owner::Asset(_) => Rank::scoped(RankClass::Explicit, 0),
            Owner::Contract(_) => Rank::scoped(RankClass::Contract, 0),
        })
        .collect();
    for (index, rank) in ranks.into_iter().enumerate() {
        world.book.laws[Id::new(index as u32)].rank = rank;
    }
}

/// Lower every declaration `also` while the complete declaration namespace is
/// available. Its law is auxiliary: `register` deliberately leaves it out of
/// the owner's ordinary law list, and the native group builder applies it to
/// the matching flow.
fn declare_alsos<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    file: &ast::File<'s>,
    home: Home,
    decl: &ast::Decl<'s>,
    owner: Owner,
    what: DeclKind,
) {
    if file[decl.alsos].is_empty() {
        return;
    }
    let on = match owner {
        Owner::Entity(id) => AlsoOn::Entity(id),
        Owner::Kind(id) => AlsoOn::Kind(id),
        Owner::Purpose(id) => AlsoOn::Purpose(id),
        _ => {
            for also in &file[decl.alsos] {
                diags.push(
                    Diagnostic::error("also-owner", "declaration-level `also` needs an entity, kind, or purpose")
                        .label(also.loc, format!("`also` is not supported on this {what:?}")),
                );
            }
            return;
        }
    };

    let currency = fallback_currency(world, owner);
    let cx = AlsoCx { file, home, owner, on, inputs: &[], currency };
    lower_alsos(world, &cx, decl.alsos, diags);
}

fn fallback_currency(world: &World<'_>, owner: Owner) -> Id<crate::book::Commodity> {
    match owner {
        Owner::Entity(entity) => world.book.entities[entity].currency,
        _ => world.book.base,
    }
}

/// The names some law counts into.
fn counted<'s>(sites: &[Site<'_, 's>]) -> Set<&'s str> {
    let mut names = Set::default();
    for site in sites {
        let file = &site.source.file;
        for step in file.iter::<ast::Step>() {
            match &step.kind {
                ast::StepKind::Effect(ast::Effect::Count { name, .. }) => {
                    names.insert(name.0);
                }
                ast::StepKind::Require { otherwise, .. } => {
                    for effect in &file[*otherwise] {
                        if let ast::Effect::Count { name, .. } = effect {
                            names.insert(name.0);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    names
}

fn push(world: &mut World, law: Law) -> Id<Law> {
    let name = world.book.name(law.name);
    let id = world.book.laws.push(law);
    world.book.lookup.laws.insert(&mut world.book.names, name, NameRank::Path, id);
    id
}

/// Laws written inside declarations that cannot own them.
fn misplaced(diags: &mut Vec<Diagnostic>, file: &ast::File, laws: ast::Many<ast::Law>, within: &str) {
    for law in &file[laws] {
        diags.push(
            Diagnostic::error("law-position", format!("a law cannot be written inside {within}"))
                .label(file.loc(law.name.0), "this law has nothing to govern")
                .help("laws belong in a place kind, an entity kind, an account, an entity, a system, or the project"),
        );
    }
}

/// Whether the trigger suits what the law governs.
fn fits(world: &World, owner: Owner, law: &ast::Law) -> Result<(), Diagnostic> {
    let thing_kind = matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Thing);
    let place_kind = matches!(owner, Owner::Kind(kind) if matches!(world.book.kinds[kind].sort, Sort::Place(_)));
    let entity_kind = matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Entity);
    let allowed = match law.trigger {
        Written::In | Written::Out | Written::Gain => {
            matches!(owner, Owner::Place(_) | Owner::System(_) | Owner::Book) || place_kind
        }
        Written::Spend => matches!(owner, Owner::Entity(_)) || entity_kind,
        Written::Flow => matches!(owner, Owner::Purpose(_) | Owner::Asset(_) | Owner::Contract(_)) || thing_kind,
        Written::Each(_) | Written::Closing { .. } | Written::By(_) => {
            !matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Commodity)
        }
        Written::Always => {
            matches!(owner, Owner::Place(_) | Owner::Entity(_) | Owner::System(_) | Owner::Book | Owner::Asset(_))
                || place_kind
                || thing_kind
                || entity_kind
        }
    };
    if allowed {
        return Ok(());
    }
    let trigger = match law.trigger {
        Written::In => "on in",
        Written::Out => "on out",
        Written::Gain => "on gain",
        Written::Spend => "on spend",
        Written::Flow => "on flow",
        Written::Each(_) => "each period",
        Written::Closing { .. } => "each year closing",
        Written::By(_) => "by",
        Written::Always => "always",
    };
    let (message, help) = match law.trigger {
        Written::In | Written::Out | Written::Gain => (
            format!("`{trigger}` laws govern accounts and account kinds"),
            "write this law inside an account, an account kind, a system, or the project",
        ),
        Written::Spend => (
            "`on spend` laws govern restricted entities and their kinds".to_owned(),
            "write this law inside an entity or a restricted entity kind",
        ),
        Written::Flow => (
            "`on flow` laws govern purposes, assets, contracts, and asset kinds".to_owned(),
            "write this law inside a purpose, asset, contract, or asset kind",
        ),
        Written::Always => (
            "`always` laws govern account balances and assets".to_owned(),
            "write this law inside an account, account kind, asset, asset kind, system, or the project",
        ),
        Written::Each(_) | Written::Closing { .. } | Written::By(_) => (
            "this law's owner has no dated subject to judge".to_owned(),
            "write a dated law inside an account, entity, purpose, asset, contract, kind, system, or the project",
        ),
    };
    Err(Diagnostic::error("law-trigger", message)
        .label(law.trigger_loc, "this trigger does not fit this owner")
        .help(help))
}

/// Tells kinds and systems which laws are theirs.
fn register(world: &mut World) {
    let book = &mut world.book;
    let mut of_kind: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.kinds.len()];
    let mut of_system: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.systems.len()];
    let mut of_purpose: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.purposes.len()];
    let mut of_contract: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.contracts.len()];
    let auxiliary: Set<Id<Law>> = book.also.iter().map(|(_, also)| also.law).collect();
    for (id, law) in book.laws.iter() {
        if auxiliary.contains(&id) {
            continue;
        }
        match law.owner {
            Owner::Kind(kind) => of_kind[kind.index()].push(id),
            Owner::System(system) => of_system[system.index()].push(id),
            Owner::Purpose(purpose) => of_purpose[purpose.index()].push(id),
            Owner::Contract(contract) => of_contract[contract.index()].push(id),
            Owner::Place(_) | Owner::Entity(_) | Owner::Book | Owner::Asset(_) => {}
        }
    }
    for id in book.kinds.ids().collect::<Vec<Id<Kind>>>() {
        book.kinds[id].laws = std::mem::take(&mut of_kind[id.index()]).into();
    }
    for id in book.systems.ids().collect::<Vec<Id<System>>>() {
        book.systems[id].laws = std::mem::take(&mut of_system[id.index()]).into();
    }
    for id in book.purposes.ids().collect::<Vec<Id<crate::book::Purpose>>>() {
        book.purposes[id].laws = std::mem::take(&mut of_purpose[id.index()]).into();
    }
    for id in book.contracts.ids().collect::<Vec<Id<crate::book::Contract>>>() {
        book.contracts[id].laws = std::mem::take(&mut of_contract[id.index()]).into();
    }
}
