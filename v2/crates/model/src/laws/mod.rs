//! Laws: written in systems, kinds, accounts, entities and the project, compiled
//! to typed nodes.
//!
//! Where a law is written decides what it governs (its [`Owner`]) and what
//! `self` is inside it. A trigger only fits some owners: value entering a place
//! is for laws about places, and money leaving a restricted entity's reach is
//! for laws about entities. Laws are numbered in the order written, systems
//! first, which is the order that decides ties between laws that do not depend
//! on each other.

mod compile;
mod order;
mod types;
mod vars;

use axiom_core::{Diagnostic, Id, Set};
use axiom_syntax::{self as ast, DeclKind, ExprId, ItemKind, Trigger as Written};

pub(crate) use self::compile::compile_template;
use self::compile::{Placement, compile};
pub(crate) use self::order::rank;
use crate::book::{Input, Kind, Sort, System};
use crate::declare::World;
use crate::errors::{Word, suggest, unknown};
use crate::law::{Law, NodeId, Owner, Rank, Trigger, Ty};
use crate::names::{Found, Rank as NameRank};
use crate::scope::Home;
use crate::sources::Site;

pub(crate) fn declare<'s>(
    world: &mut World<'s>,
    sites: &[Site<'_, 's>],
    diags: &mut Vec<Diagnostic>,
) {
    world.tallies = counted(sites);
    let mut seen: Set<(DeclKind, u32)> = Set::default();
    for source in sites {
        let file = &source.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Law(id) => {
                    let law = &file[id];
                    let owner = match source.home {
                        Home::System(system) => Owner::System(system),
                        Home::Project | Home::Builtin => Owner::Book,
                    };
                    compile_native(world, diags, file, source.home, owner, Ty::Entity, law);
                }
                ItemKind::Decl(id) => {
                    let decl = &file[id];
                    let word = Word {
                        text: decl.name.0,
                        loc: file.loc(decl.name.0),
                    };
                    let resolved = match decl.what {
                        DeclKind::Kind => world.kind(source.home, word).map(|kind| {
                            let subject = match world.book.kinds[kind].sort {
                                Sort::Place(_) => Ty::Place,
                                Sort::Entity => Ty::Entity,
                                Sort::Thing => Ty::Asset,
                                Sort::Commodity => Ty::Unit,
                            };
                            (Owner::Kind(kind), subject, kind.index() as u32)
                        }),
                        DeclKind::Account => world
                            .place(word)
                            .map(|place| (Owner::Place(place), Ty::Place, place.index() as u32)),
                        DeclKind::Entity => world.entity(source.home, word).map(|entity| {
                            (Owner::Entity(entity), Ty::Entity, entity.index() as u32)
                        }),
                        DeclKind::Asset => world
                            .book
                            .asset(word.text)
                            .ok_or_else(|| unknown_named(world, "asset", word))
                            .map(|asset| (Owner::Asset(asset), Ty::Asset, asset.index() as u32)),
                        DeclKind::Purpose => world.purpose(source.home, word).map(|purpose| {
                            (Owner::Purpose(purpose), Ty::Flow, purpose.index() as u32)
                        }),
                        DeclKind::Commodity => {
                            misplaced(diags, file, decl.laws, "a commodity");
                            continue;
                        }
                    };
                    let (owner, subject, key) = match resolved {
                        Ok(resolved) => resolved,
                        Err(problem) => {
                            diags.push(problem);
                            continue;
                        }
                    };
                    if decl.what == DeclKind::Kind && subject == Ty::Unit {
                        misplaced(diags, file, decl.laws, "a commodity kind");
                        continue;
                    }
                    if seen.insert((decl.what, key)) {
                        for law in &file[decl.laws] {
                            compile_native(world, diags, file, source.home, owner, subject, law);
                        }
                    }
                }
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
    let site = Placement {
        file,
        home,
        owner,
        subject,
    };
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
    let pending: Vec<_> = world
        .book
        .laws
        .iter()
        .filter_map(|(id, law)| law.override_name.map(|name| (id, name, law.loc)))
        .collect();
    for (id, name, loc) in pending {
        let text = world.book.name(name);
        let names = &world.book.names;
        match world.book.lookup.laws.find(names, text, |_| true) {
            Found::One(target) if target != id => world.book.laws[id].overrides = Some(target),
            Found::One(_) => diags.push(
                Diagnostic::error("law-override", "a law cannot override itself")
                    .label(loc, "this is the law's own name"),
            ),
            Found::Nothing => {
                let word = Word { text, loc };
                let diagnostic = unknown("unknown-law", "law", word, None);
                let keys: Vec<_> = world.book.lookup.laws.keys(&world.book.names).collect();
                diags.push(suggest(diagnostic, loc, text, keys));
            }
            Found::Several(targets) => {
                let mut diagnostic = Diagnostic::error(
                    "ambiguous-law",
                    format!("law `{text}` names more than one law"),
                )
                .label(loc, "qualify which law this one overrides");
                for target in targets {
                    let law = &world.book.laws[target];
                    diagnostic = diagnostic.context(
                        law.loc,
                        format!("`{}` is declared here", world.book.name(law.name)),
                    );
                }
                diags.push(diagnostic);
            }
        }
    }
}

/// More specific owners win when two laws govern the same occasion.
fn set_specificity(world: &mut World<'_>) {
    let book = &world.book;
    let ranks: Vec<_> = book
        .laws
        .iter()
        .map(|(_, law)| {
            let raw = match law.owner {
                Owner::Book => 500,
                Owner::System(system) => 100 + book.systems.lineage(system).count() as u32,
                Owner::Kind(kind) => 1_000 + book.kinds.lineage(kind).count() as u32,
                Owner::Purpose(purpose) => 4_000 + book.purposes.lineage(purpose).count() as u32,
                Owner::Place(_) | Owner::Entity(_) | Owner::Asset(_) => 8_000,
                Owner::Contract(_) => 9_000,
            };
            Rank(raw.min(u32::from(u16::MAX)) as u16)
        })
        .collect();
    for (index, rank) in ranks.into_iter().enumerate() {
        world.book.laws[Id::new(index as u32)].rank = rank;
    }
}

/// Compiles the expression roots of an `also` declaration into a law arena.
/// The caller owns the returned root mapping and stores the law id in its
/// `book::Also`; roots must be ordered as written (`when`, then amounts).
pub(crate) fn compile_also<'s>(
    world: &mut World<'s>,
    diags: &mut Vec<Diagnostic>,
    file: &ast::File<'s>,
    home: Home,
    owner: Owner,
    subject: Ty,
    name: axiom_core::Sym,
    inputs: &[Input],
    roots: &[(ExprId, Ty)],
    loc: axiom_core::Loc,
) -> Option<(Id<Law>, Box<[NodeId]>)> {
    let (program, roots) =
        compile_template(world, diags, file, home, subject, name, inputs, roots)?;
    let (book, nodes) = (&mut world.book, program.nodes);
    let law = Law {
        name,
        doc: None,
        owner,
        system: if let Home::System(system) = home {
            Some(system)
        } else {
            None
        },
        trigger: Trigger::Flow,
        budget: None,
        overrides: None,
        override_name: None,
        rank: crate::law::Rank(0),
        steps: Box::default(),
        nodes,
        loc,
    };
    Some((book.laws.push(law), roots))
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
    world
        .book
        .lookup
        .laws
        .insert(&mut world.book.names, name, NameRank::Path, id);
    id
}

fn unknown_named(world: &World<'_>, noun: &str, word: Word<'_>) -> Diagnostic {
    let (code, known): (&'static str, Vec<&str>) = match noun {
        "asset" => (
            "unknown-asset",
            world
                .book
                .assets
                .values()
                .map(|asset| world.book.name(asset.name))
                .collect(),
        ),
        "contract" => (
            "unknown-contract",
            world
                .book
                .contracts
                .values()
                .map(|contract| world.book.name(contract.name))
                .collect(),
        ),
        _ => ("unknown-name", Vec::new()),
    };
    suggest(unknown(code, noun, word, None), word.loc, word.text, known)
}

/// Laws written inside declarations that cannot own them.
fn misplaced(
    diags: &mut Vec<Diagnostic>,
    file: &ast::File,
    laws: ast::Many<ast::Law>,
    within: &str,
) {
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
    let thing_kind =
        matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Thing);
    let place_kind =
        matches!(owner, Owner::Kind(kind) if matches!(world.book.kinds[kind].sort, Sort::Place(_)));
    let entity_kind =
        matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Entity);
    let allowed = match law.trigger {
        Written::In | Written::Out | Written::Gain => {
            matches!(owner, Owner::Place(_) | Owner::System(_) | Owner::Book) || place_kind
        }
        Written::Spend => matches!(owner, Owner::Entity(_)) || entity_kind,
        Written::Flow => {
            matches!(
                owner,
                Owner::Purpose(_) | Owner::Asset(_) | Owner::Contract(_)
            ) || thing_kind
        }
        Written::Each(_) | Written::Closing { .. } | Written::By(_) => {
            !matches!(owner, Owner::Kind(kind) if world.book.kinds[kind].sort == Sort::Commodity)
        }
        Written::Always => {
            matches!(
                owner,
                Owner::Place(_)
                    | Owner::Entity(_)
                    | Owner::System(_)
                    | Owner::Book
                    | Owner::Asset(_)
            ) || place_kind
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
    let auxiliary: Set<Id<Law>> = book.also.iter().map(|also| also.law).collect();
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
    for id in book
        .purposes
        .ids()
        .collect::<Vec<Id<crate::book::Purpose>>>()
    {
        book.purposes[id].laws = std::mem::take(&mut of_purpose[id.index()]).into();
    }
    for id in book
        .contracts
        .ids()
        .collect::<Vec<Id<crate::book::Contract>>>()
    {
        book.contracts[id].laws = std::mem::take(&mut of_contract[id.index()]).into();
    }
}
