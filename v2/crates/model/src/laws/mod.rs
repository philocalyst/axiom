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
use axiom_syntax::{self as ast, BinOp, DeclKind, Trigger as Written};

pub(crate) use self::order::rank;
use self::compile::{Site, compile};
use crate::book::{Kind, Sort, System};
use crate::collect::Entry;
use crate::declare::World;
use crate::law::{Dir, Func, Law, Node, NodeId, Op, Owner, Step, StepKind, Trigger, Ty, Value, Window};
use crate::names::Rank;
use crate::props::Budget;
use crate::scope::Home;

pub(crate) fn declare<'s>(world: &mut World<'s>, entries: &[Entry<'_, 's>], budgets: Vec<Budget>, diags: &mut Vec<Diagnostic>) {
    world.tallies = counted(entries);
    let mut written: [usize; 4] = [0; 4];
    let mut seen: Set<(DeclKind, usize)> = Set::default();
    for entry in entries {
        match entry {
            Entry::Law(law) => {
                let owner = match law.home() {
                    Home::System(system) => Owner::System(system),
                    Home::Project | Home::Builtin => Owner::Book,
                };
                let site = Site { file: law.file(), home: law.home(), owner, subject: Ty::Entity };
                add(world, diags, &site, law.node);
            }
            Entry::Decl(decl) => {
                let (file, node) = (decl.file(), decl.node);
                let at = &mut written[node.what as usize];
                let position = *at;
                *at += 1;
                let (owner, subject, id) = match node.what {
                    DeclKind::Commodity => {
                        misplaced(diags, file, node.laws, "a commodity");
                        continue;
                    }
                    DeclKind::Kind => {
                        let id = world.declared.kinds[position];
                        let subject = match world.book.kinds[id].sort {
                            Sort::Place(_) => Ty::Place,
                            Sort::Entity => Ty::Entity,
                            Sort::Commodity => {
                                misplaced(diags, file, node.laws, "a commodity kind");
                                continue;
                            }
                        };
                        (Owner::Kind(id), subject, id.index())
                    }
                    DeclKind::Account => match world.declared.places[position] {
                        Some(id) => (Owner::Place(id), Ty::Place, id.index()),
                        None => continue,
                    },
                    DeclKind::Entity => {
                        let id = world.declared.entities[position];
                        (Owner::Entity(id), Ty::Entity, id.index())
                    }
                };
                // A repeated declaration is reported once, and its laws not compiled twice.
                if seen.insert((node.what, id)) {
                    let site = Site { file, home: decl.home(), owner, subject };
                    file[node.laws].iter().for_each(|law| add(world, diags, &site, law));
                }
            }
            _ => {}
        }
    }
    for budget in budgets {
        let law = budget_law(world, &budget);
        push(world, law);
    }
    register(world);
}

/// The names some law counts into.
fn counted<'s>(entries: &[Entry<'_, 's>]) -> Set<&'s str> {
    let mut names = Set::default();
    let mut laws = |file: &ast::File<'s>, laws: &[ast::Law<'s>]| {
        for law in laws {
            for step in &file[law.steps] {
                match &step.kind {
                    ast::StepKind::Effect(ast::Effect::Count { name, .. })
                    | ast::StepKind::Require { otherwise: Some(ast::Effect::Count { name, .. }), .. } => {
                        names.insert(name.0);
                    }
                    _ => {}
                }
            }
        }
    };
    for entry in entries {
        match entry {
            Entry::Law(law) => laws(law.file(), std::slice::from_ref(law.node)),
            Entry::Decl(decl) => laws(decl.file(), &decl.file()[decl.node.laws]),
            _ => {}
        }
    }
    names
}

/// Compiles `law` and adds it to the book, if it fits where it was written.
fn add<'s>(world: &mut World<'s>, diags: &mut Vec<Diagnostic>, site: &Site<'_, 's>, law: &ast::Law<'s>) {
    if let Err(problem) = fits(world, site.owner, law) {
        diags.push(problem);
        return;
    }
    if let Some(compiled) = compile(world, diags, site, law) {
        push(world, compiled);
    }
}

fn push(world: &mut World, law: Law) {
    let name = world.book.name(law.name);
    let id = world.book.laws.push(law);
    world.book.lookup.laws.insert(&mut world.book.names, name, Rank::Path, id);
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
    let about_entities = match owner {
        Owner::Entity(_) => true,
        Owner::Kind(kind) => world.book.kinds[kind].sort == Sort::Entity,
        Owner::Place(_) | Owner::System(_) | Owner::Book => false,
    };
    let trigger_loc = law.trigger_loc;
    match (law.trigger, about_entities) {
        (Written::Spend, false) => {
            Err(Diagnostic::error("law-trigger", "`on spend` laws govern a restricted entity's money")
                .label(trigger_loc, "this law is not attached to an entity")
                .help("write it inside an entity kind such as `kind grant : entity`, or inside an `entity`"))
        }
        (Written::In | Written::Out | Written::Gain | Written::Always, true) => {
            Err(Diagnostic::error("law-trigger", "this trigger governs places, but the law is attached to an entity")
                .label(trigger_loc, "value moves through places, not entities")
                .help("write it inside an account, a place kind, a system, or the project"))
        }
        _ => Ok(()),
    }
}

/// `budget 500 USD monthly` is `on in`, `warn total(in, month) <= 500 USD`,
/// every node located at the property.
fn budget_law(world: &mut World, budget: &Budget) -> Law {
    let names = &mut world.book.names;
    let window_word = match budget.window {
        Window::Month => "month",
        Window::Year => "year",
        Window::Ever => "ever",
    };
    let (in_word, window_word) = (names.intern("in"), names.intern(window_word));
    let node = |op, ty, first| Node { op, ty, loc: budget.loc, first: NodeId(first) };
    let nodes = [
        node(Op::Const(Value::Name(in_word)), Ty::Name, 0),
        node(Op::Const(Value::Name(window_word)), Ty::Name, 1),
        node(Op::Call(Func::Total(Dir::In, budget.window), Box::new([NodeId(0), NodeId(1)])), Ty::Amount, 0),
        node(Op::Const(Value::Amount(budget.amount)), Ty::Amount, 3),
        node(Op::Bin(BinOp::Le, NodeId(2), NodeId(3)), Ty::Bool, 0),
    ];
    let step = Step {
        loc: budget.loc,
        kind: StepKind::Require { cond: NodeId(4), otherwise: None, message: None, warn: true },
    };
    Law {
        name: names.intern("budget"),
        doc: None,
        owner: Owner::Place(budget.place),
        system: None,
        trigger: Trigger::In,
        steps: Box::new([step]),
        nodes: Box::new(nodes),
        loc: budget.loc,
    }
}

/// Tells kinds and systems which laws are theirs.
fn register(world: &mut World) {
    let book = &mut world.book;
    let mut of_kind: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.kinds.len()];
    let mut of_system: Vec<Vec<Id<Law>>> = vec![Vec::new(); book.systems.len()];
    for (id, law) in book.laws.iter() {
        match law.owner {
            Owner::Kind(kind) => of_kind[kind.index()].push(id),
            Owner::System(system) => of_system[system.index()].push(id),
            Owner::Place(_) | Owner::Entity(_) | Owner::Book => {}
        }
    }
    for id in book.kinds.ids().collect::<Vec<Id<Kind>>>() {
        book.kinds[id].laws = std::mem::take(&mut of_kind[id.index()]).into();
    }
    for id in book.systems.ids().collect::<Vec<Id<System>>>() {
        book.systems[id].laws = std::mem::take(&mut of_system[id.index()]).into();
    }
}
