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

use axiom_core::{Arena, Day, Days, Diagnostic, Id, Map, Period, Ratio, Set, Severity, Timeline};
use axiom_syntax::{self as ast, DeclKind, ExprId, ItemKind, Trigger as Written};

pub(crate) use self::compile::compile_template;
use self::compile::{Placement, compile};
pub(crate) use self::order::rank;
use crate::book::{
    Also, AlsoOn, Amount, Budget, Implied, Input, Kind, Limit, Sign, Sort, System, TemplateAmount,
};
use crate::declare::World;
use crate::errors::{Word, suggest, unknown};
use crate::law::{
    BinOp, Func, Law, Node, NodeId, Op, Owner, Rank, RankClass, Step, StepKind, Trigger, Ty, Value,
    Var, Window,
};
use crate::names::Rank as NameRank;
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
                            // Purpose laws govern purpose-bearing flows, but
                            // `self` is the owner of the flow (LANGUAGE §8).
                            // `total(window)` retains the purpose context in
                            // the law owner instead of changing `self`'s type.
                            (Owner::Purpose(purpose), Ty::Entity, purpose.index() as u32)
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
                    declare_alsos(world, diags, file, source.home, decl, owner, decl.what);
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
    declare_budgets(world, sites, diags);
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
        if let Some(nearest) = candidates
            .iter()
            .map(|&candidate| scope.rank(law_home(&world.book.laws[candidate])))
            .min()
        {
            candidates
                .retain(|&candidate| scope.rank(law_home(&world.book.laws[candidate])) == nearest);
        }
        match candidates.as_slice() {
            [target] if *target != id => world.book.laws[id].overrides = Some(*target),
            [_target] => diags.push(
                Diagnostic::error("law-override", "a law cannot override itself")
                    .label(loc, "this is the law's own name"),
            ),
            [] => {
                let word = Word { text, loc };
                let diagnostic = unknown("unknown-law", "law", word, None);
                let keys: Vec<_> = table
                    .keys(names)
                    .filter(|key| {
                        table
                            .candidates(names, key)
                            .iter()
                            .any(|&candidate| scope.sees(law_home(&world.book.laws[candidate])))
                    })
                    .collect();
                diags.push(suggest(diagnostic, loc, text, keys));
            }
            targets => {
                let mut diagnostic = Diagnostic::error(
                    "ambiguous-law",
                    format!("law `{text}` names more than one law"),
                )
                .label(loc, "qualify which law this one overrides");
                for &target in targets {
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
            Owner::System(system) => Rank::scoped(
                RankClass::System,
                book.systems.lineage(system).count() as u32,
            ),
            Owner::Kind(kind) => {
                Rank::scoped(RankClass::Kind, book.kinds.lineage(kind).count() as u32)
            }
            Owner::Purpose(purpose) => Rank::scoped(
                RankClass::Purpose,
                book.purposes.lineage(purpose).count() as u32,
            ),
            Owner::Place(_) | Owner::Entity(_) | Owner::Asset(_) => {
                Rank::scoped(RankClass::Explicit, 0)
            }
            Owner::Contract(_) => Rank::scoped(RankClass::Contract, 0),
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
        rank: Rank::ZERO,
        steps: Box::default(),
        nodes,
        loc,
    };
    Some((book.laws.push(law), roots))
}

#[derive(Clone, Copy)]
enum PendingAmount {
    Literal(Amount),
    Computed(usize),
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
                    Diagnostic::error(
                        "also-owner",
                        "declaration-level `also` needs an entity, kind, or purpose",
                    )
                    .label(
                        also.loc,
                        format!("`also` is not supported on this {what:?}"),
                    ),
                );
            }
            return;
        }
    };

    for also in &file[decl.alsos] {
        let mut roots = Vec::new();
        let when_index = also.when.map(|when| {
            let index = roots.len();
            roots.push((when, Ty::Bool));
            index
        });
        let (what, amount_index, metadata_clauses) = match &also.line {
            ast::AlsoLine::Item(item) => {
                let amount = match item.amount {
                    ast::Amount::Literal(literal) => {
                        let unit = match literal.unit() {
                            Some(unit) => match world.commodity_of(Word {
                                text: unit.0,
                                loc: file.loc(unit.0),
                            }) {
                                Ok(unit) => unit,
                                Err(problem) => {
                                    diags.push(problem);
                                    continue;
                                }
                            },
                            None => fallback_currency(world, owner),
                        };
                        let Some(amount) = world
                            .amount(literal.num(), unit, file.loc(literal.0))
                            .map_err(|problem| diags.push(problem))
                            .ok()
                        else {
                            continue;
                        };
                        PendingAmount::Literal(amount)
                    }
                    ast::Amount::Computed(root) => {
                        let index = roots.len();
                        roots.push((root, Ty::AMOUNT));
                        PendingAmount::Computed(index)
                    }
                };
                (
                    Some(Implied::Item {
                        sign: match item.sign {
                            ast::Sign::Carve => Sign::Carve,
                            ast::Sign::Add => Sign::Add,
                            ast::Sign::Less => Sign::Less,
                        },
                        amount: TemplateAmount::Literal(Amount::zero(fallback_currency(
                            world, owner,
                        ))),
                    }),
                    Some(amount),
                    item.tail,
                )
            }
            ast::AlsoLine::Flow(flow) => {
                if !file[flow.body.legs].is_empty() || !file[flow.body.items].is_empty() {
                    diags.push(
                        Diagnostic::error(
                            "also-flow-body",
                            "a declaration `also` flow cannot have split legs or items",
                        )
                        .label(also.loc, "write one implied flow here"),
                    );
                    continue;
                }
                let mut valid_ends = true;
                let from = match flow.from.end {
                    Some(end) if end.name.0 != "self" => match world.end(
                        home,
                        Word {
                            text: end.name.0,
                            loc: file.loc(end.name.0),
                        },
                    ) {
                        Ok(end) => Some(end.place),
                        Err(problem) => {
                            diags.push(problem);
                            valid_ends = false;
                            None
                        }
                    },
                    _ => None,
                };
                let to = match flow.to.end {
                    Some(end) if end.name.0 != "self" => match world.end(
                        home,
                        Word {
                            text: end.name.0,
                            loc: file.loc(end.name.0),
                        },
                    ) {
                        Ok(end) => Some(end.place),
                        Err(problem) => {
                            diags.push(problem);
                            valid_ends = false;
                            None
                        }
                    },
                    _ => None,
                };
                if !valid_ends {
                    continue;
                }
                let from_amount = match flow.from.amount {
                    Some(ast::Quantity::Amount(amount)) => Some(amount),
                    Some(other) => {
                        diags.push(
                            Diagnostic::error(
                                "also-flow-amount",
                                "an implied flow amount must be an amount expression",
                            )
                            .label(
                                quantity_loc(file, other, also.loc),
                                "this quantity cannot be implied",
                            ),
                        );
                        continue;
                    }
                    None => None,
                };
                let to_amount = match flow.to.amount {
                    Some(ast::Quantity::Amount(amount)) => Some(amount),
                    Some(other) => {
                        diags.push(
                            Diagnostic::error(
                                "also-flow-amount",
                                "an implied flow amount must be an amount expression",
                            )
                            .label(
                                quantity_loc(file, other, also.loc),
                                "this quantity cannot be implied",
                            ),
                        );
                        continue;
                    }
                    None => None,
                };
                let amount = match (from_amount, to_amount) {
                    (Some(_), Some(_)) => {
                        diags.push(
                            Diagnostic::error(
                                "also-flow-amount",
                                "an implied flow states its amount on one side only",
                            )
                            .label(also.loc, "remove one of these amounts"),
                        );
                        continue;
                    }
                    (Some(amount), None) | (None, Some(amount)) => amount,
                    (None, None) => {
                        diags.push(
                            Diagnostic::error(
                                "also-flow-amount",
                                "an implied flow needs an amount",
                            )
                            .label(also.loc, "write an amount on one side of the arrow"),
                        );
                        continue;
                    }
                };
                let amount = match amount {
                    ast::Amount::Literal(literal) => {
                        let unit = match literal.unit() {
                            Some(unit) => match world.commodity_of(Word {
                                text: unit.0,
                                loc: file.loc(unit.0),
                            }) {
                                Ok(unit) => unit,
                                Err(problem) => {
                                    diags.push(problem);
                                    continue;
                                }
                            },
                            None => fallback_currency(world, owner),
                        };
                        let Some(amount) = world
                            .amount(literal.num(), unit, file.loc(literal.0))
                            .map_err(|problem| diags.push(problem))
                            .ok()
                        else {
                            continue;
                        };
                        PendingAmount::Literal(amount)
                    }
                    ast::Amount::Computed(root) => {
                        let index = roots.len();
                        roots.push((root, Ty::AMOUNT));
                        PendingAmount::Computed(index)
                    }
                };
                (
                    Some(Implied::Flow {
                        from,
                        to,
                        amount: TemplateAmount::Literal(Amount::zero(fallback_currency(
                            world, owner,
                        ))),
                    }),
                    Some(amount),
                    flow.tail,
                )
            }
        };
        let (Some(mut what), Some(amount)) = (what, amount_index) else {
            continue;
        };
        let metadata = crate::lower::also::tail(world, home, file, metadata_clauses, diags);
        let name = world.book.names.intern("also");
        let Some((law, compiled_roots)) = compile_also(
            world,
            diags,
            file,
            home,
            owner,
            Ty::Flow,
            name,
            &[],
            &roots,
            also.loc,
        ) else {
            continue;
        };
        let amount = match amount {
            PendingAmount::Literal(amount) => TemplateAmount::Literal(amount),
            PendingAmount::Computed(index) => TemplateAmount::Computed(compiled_roots[index]),
        };
        match &mut what {
            Implied::Item { amount: slot, .. } | Implied::Flow { amount: slot, .. } => {
                *slot = amount
            }
        }
        let when = when_index.map(|index| compiled_roots[index]);
        world.book.also.push(Also {
            on,
            what,
            when,
            law,
            purpose: metadata.purpose,
            description: metadata.description,
            codes: metadata.codes,
            select: metadata.select,
            detail: metadata.detail,
            waive: metadata.waive,
            loc: also.loc,
        });
    }
}

fn fallback_currency(world: &World<'_>, owner: Owner) -> Id<crate::book::Commodity> {
    match owner {
        Owner::Entity(entity) => world.book.entities[entity].currency,
        _ => world.book.base,
    }
}

fn quantity_loc(
    file: &ast::File<'_>,
    quantity: ast::Quantity<'_>,
    fallback: axiom_core::Loc,
) -> axiom_core::Loc {
    match quantity {
        ast::Quantity::Amount(ast::Amount::Literal(literal)) => file.loc(literal.0),
        ast::Quantity::Amount(ast::Amount::Computed(root))
        | ast::Quantity::Pending(ast::Amount::Computed(root))
        | ast::Quantity::Target(ast::Amount::Computed(root)) => file.exprs[root].loc,
        ast::Quantity::Pending(ast::Amount::Literal(literal))
        | ast::Quantity::Target(ast::Amount::Literal(literal)) => file.loc(literal.0),
        _ => fallback,
    }
}

#[derive(Clone, Copy)]
struct BudgetEntry<'a, 's> {
    file: &'a ast::File<'s>,
    home: Home,
    allowance: ast::Allowance<'s>,
    days: Option<Days>,
    declared: bool,
    loc: axiom_core::Loc,
}

#[derive(Clone, Copy)]
enum BudgetLimit {
    Ready(Limit),
    Computed(NodeId),
}

/// Lowers declaration and dated budget rows after every purpose has an id.
/// Each generated warning law reads the effective limit through one typed
/// `BudgetLimit` call, so later rows and bounded `until` changes are not baked
/// into a stale literal.
fn declare_budgets<'a, 's>(
    world: &mut World<'s>,
    sites: &'a [Site<'a, 's>],
    diags: &mut Vec<Diagnostic>,
) {
    let mut entries: Map<Id<crate::book::Purpose>, Vec<BudgetEntry<'a, 's>>> = Map::default();
    let mut declared: Set<Id<crate::book::Purpose>> = Set::default();

    for source in sites {
        let file = &source.source.file;
        for item in &file.items {
            match item.kind {
                ItemKind::Decl(reference) => {
                    let decl = &file[reference];
                    if let Some(allowance) = decl.budget {
                        if decl.what != DeclKind::Purpose {
                            diags.push(
                                Diagnostic::error(
                                    "budget-owner",
                                    "a declaration budget belongs to a purpose",
                                )
                                .label(item.loc, "write this inside a purpose declaration"),
                            );
                            continue;
                        }
                        let word = Word {
                            text: decl.name.0,
                            loc: file.loc(decl.name.0),
                        };
                        let purpose = match world.purpose(source.home, word) {
                            Ok(purpose) => purpose,
                            Err(problem) => {
                                diags.push(problem);
                                continue;
                            }
                        };
                        if !declared.insert(purpose) {
                            diags.push(
                                Diagnostic::error(
                                    "duplicate-budget",
                                    format!(
                                        "purpose `{}` has more than one starting budget",
                                        word.text
                                    ),
                                )
                                .label(item.loc, "keep one initial budget declaration"),
                            );
                            continue;
                        }
                        entries.entry(purpose).or_default().push(BudgetEntry {
                            file,
                            home: source.home,
                            allowance: file[allowance],
                            days: None,
                            declared: true,
                            loc: item.loc,
                        });
                    }
                }
                ItemKind::Budget(reference) => {
                    let budget = &file[reference];
                    let word = Word {
                        text: budget.purpose.0,
                        loc: file.loc(budget.purpose.0),
                    };
                    let purpose = match world.purpose(source.home, word) {
                        Ok(purpose) => purpose,
                        Err(problem) => {
                            diags.push(problem);
                            continue;
                        }
                    };
                    if !declared.insert(purpose) {
                        diags.push(
                            Diagnostic::error(
                                "duplicate-budget",
                                format!(
                                    "purpose `{}` has more than one starting budget",
                                    word.text
                                ),
                            )
                            .label(item.loc, "keep one initial budget declaration"),
                        );
                        continue;
                    }
                    entries.entry(purpose).or_default().push(BudgetEntry {
                        file,
                        home: source.home,
                        allowance: budget.allowance,
                        days: None,
                        declared: true,
                        loc: item.loc,
                    });
                }
                ItemKind::Statement(reference) => {
                    let statement = &file[reference];
                    let ast::Verb::Now(ast::Change::Budget(allowance)) = statement.verb else {
                        continue;
                    };
                    let ast::Subject::Purpose(name) = statement.subject else {
                        diags.push(
                            Diagnostic::error(
                                "budget-owner",
                                "a dated budget change must name a purpose with `#`",
                            )
                            .label(item.loc, "write `#purpose now budget …`"),
                        );
                        continue;
                    };
                    let word = Word {
                        text: name.0,
                        loc: file.loc(name.0),
                    };
                    let purpose = match world.purpose(source.home, word) {
                        Ok(purpose) => purpose,
                        Err(problem) => {
                            diags.push(problem);
                            continue;
                        }
                    };
                    let mut until = None;
                    for clause in &file[statement.tail] {
                        if let axiom_syntax::ClauseKind::Until(day) = clause.kind {
                            if until.replace(day).is_some() {
                                diags.push(
                                    Diagnostic::error(
                                        "duplicate-until",
                                        "a budget change has one `until` date",
                                    )
                                    .label(clause.at, "remove this extra end date"),
                                );
                            }
                        }
                    }
                    let last = until.unwrap_or(Day::MAX);
                    let Some(days) = Days::new(statement.date, last) else {
                        diags.push(
                            Diagnostic::error(
                                "budget-until",
                                "a budget change ends before it begins",
                            )
                            .label(item.loc, "the `until` date precedes this change"),
                        );
                        continue;
                    };
                    entries.entry(purpose).or_default().push(BudgetEntry {
                        file,
                        home: source.home,
                        allowance: file[allowance],
                        days: Some(days),
                        declared: false,
                        loc: item.loc,
                    });
                }
                _ => {}
            }
        }
    }

    for (purpose, mut entries) in entries {
        entries.sort_by_key(|entry| {
            if entry.declared {
                Day::MIN
            } else {
                entry
                    .days
                    .expect("dated budget entries have a range")
                    .first()
            }
        });
        lower_budget(world, purpose, &entries, diags);
    }
}

fn lower_budget<'s>(
    world: &mut World<'s>,
    purpose: Id<crate::book::Purpose>,
    entries: &[BudgetEntry<'_, 's>],
    diags: &mut Vec<Diagnostic>,
) -> Option<()> {
    let first = *entries.first()?;
    let purpose_name = world.book.purposes[purpose].name;
    let mut nodes = Arena::new();
    let initial_limit = lower_budget_limit(world, purpose, first, purpose_name, &mut nodes, diags)?;
    let initial_funded = funding(
        world,
        first.home,
        first.file,
        first.allowance.funded,
        first.loc,
        diags,
    )?;
    let (period, carries) = (first.allowance.per, first.allowance.carries);
    let mut timeline = Timeline::new(match initial_limit {
        BudgetLimit::Ready(limit) => limit,
        BudgetLimit::Computed(root) => Limit::Computed(root),
    });
    let starts = first.days.map_or(Day::MIN, |days| days.first());
    if !first.declared {
        timeline = Timeline::new(Limit::Amount(Amount::zero(world.book.base)));
        timeline.paint(
            first.days.expect("late budgets have a start"),
            match initial_limit {
                BudgetLimit::Ready(limit) => limit,
                BudgetLimit::Computed(root) => Limit::Computed(root),
            },
        );
    }

    for entry in entries.iter().copied().skip(1) {
        if entry.allowance.per != period || entry.allowance.carries != carries {
            diags.push(
                Diagnostic::error(
                    "budget-shape-change",
                    "a dated budget change keeps its starting period and carry policy",
                )
                .label(
                    entry.loc,
                    "this change alters the budget's window or carry policy",
                ),
            );
            continue;
        }
        let changed_funding = funding(
            world,
            entry.home,
            entry.file,
            entry.allowance.funded,
            entry.loc,
            diags,
        )?;
        if changed_funding != initial_funded {
            diags.push(
                Diagnostic::error(
                    "budget-shape-change",
                    "a dated budget change keeps its starting funding route",
                )
                .label(entry.loc, "this change alters the budget's funding route"),
            );
            continue;
        }
        let Some(limit) =
            lower_budget_limit(world, purpose, entry, purpose_name, &mut nodes, diags)
        else {
            continue;
        };
        let Some(days) = entry.days else {
            diags.push(
                Diagnostic::error(
                    "duplicate-budget",
                    "a purpose has more than one starting budget",
                )
                .label(entry.loc, "remove the duplicate starting budget"),
            );
            continue;
        };
        timeline.paint(
            days,
            match limit {
                BudgetLimit::Ready(limit) => limit,
                BudgetLimit::Computed(root) => Limit::Computed(root),
            },
        );
    }

    // Every value read by a `Share` limit is a static dependency of this
    // require. These zero-argument calls let the plan subscribe the budget law
    // to each source purpose without allocating a second dependency table.
    let budget_id = Id::new(world.book.budgets.len() as u32);
    let law_id = Id::new(world.book.laws.len() as u32);
    let window = match period {
        Period::Month => Window::Month,
        Period::Year => Window::Year,
    };
    let mut observed = Set::default();
    for (_, limit) in timeline.within(Days::ALWAYS) {
        if let Limit::Share { of, .. } = *limit
            && observed.insert(of)
        {
            push_node(
                &mut nodes,
                Op::Call(
                    Func::PurposeTotal {
                        purpose: Some(of),
                        window,
                    },
                    Box::default(),
                ),
                Ty::AMOUNT,
                first.loc,
                None,
            );
        }
    }

    let mut steps = Vec::new();
    if starts != Day::MIN {
        let date = push_node(&mut nodes, Op::Var(Var::Date), Ty::Day, first.loc, None);
        let day = push_node(
            &mut nodes,
            Op::Const(Value::Day(starts)),
            Ty::Day,
            first.loc,
            None,
        );
        let after_start = push_node(
            &mut nodes,
            Op::Bin(BinOp::Ge, date, day),
            Ty::Bool,
            first.loc,
            Some(NodeId(0)),
        );
        steps.push(Step {
            loc: first.loc,
            kind: StepKind::When(after_start),
        });
    }
    let total = push_node(
        &mut nodes,
        Op::Call(
            Func::PurposeTotal {
                purpose: None,
                window,
            },
            Box::default(),
        ),
        Ty::AMOUNT,
        first.loc,
        None,
    );
    let cap = push_node(
        &mut nodes,
        Op::Call(Func::BudgetLimit(budget_id), Box::default()),
        Ty::AMOUNT,
        first.loc,
        None,
    );
    let condition = push_node(
        &mut nodes,
        Op::Bin(BinOp::Le, total, cap),
        Ty::Bool,
        first.loc,
        Some(NodeId(0)),
    );
    steps.push(Step {
        loc: first.loc,
        kind: StepKind::Require {
            cond: condition,
            otherwise: Box::default(),
            message: None,
            severity: Severity::Warning,
        },
    });

    let budget = Budget {
        purpose,
        period,
        limits: timeline,
        carries,
        law: law_id,
        funded: initial_funded,
        loc: first.loc,
    };
    let actual_budget = world.book.budgets.push(budget);
    if actual_budget != budget_id {
        return None;
    }
    world.book.laws.push(Law {
        name: purpose_name,
        doc: None,
        owner: Owner::Purpose(purpose),
        system: world.book.purposes[purpose].system,
        trigger: Trigger::Flow,
        budget: Some(budget_id),
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps: steps.into_boxed_slice(),
        nodes,
        loc: first.loc,
    });
    if world.book.laws.len() != law_id.index() + 1 {
        return None;
    }
    Some(())
}

fn lower_budget_limit<'s>(
    world: &mut World<'s>,
    purpose: Id<crate::book::Purpose>,
    entry: BudgetEntry<'_, 's>,
    name: axiom_core::Sym,
    nodes: &mut Arena<Node>,
    diags: &mut Vec<Diagnostic>,
) -> Option<BudgetLimit> {
    match entry.allowance.limit {
        ast::Limit::Amount(ast::Amount::Literal(literal)) => {
            let unit = match literal.unit() {
                Some(unit) => world
                    .commodity_of(Word {
                        text: unit.0,
                        loc: entry.file.loc(unit.0),
                    })
                    .map_err(|problem| diags.push(problem))
                    .ok()?,
                None => world.book.base,
            };
            world
                .amount(literal.num(), unit, entry.file.loc(literal.0))
                .map_err(|problem| diags.push(problem))
                .ok()
                .map(|amount| BudgetLimit::Ready(Limit::Amount(amount)))
        }
        ast::Limit::Amount(ast::Amount::Computed(root)) => {
            let (program, local_root) = compile::compile_budget_limit(
                world, diags, entry.file, entry.home, purpose, name, root,
            )?;
            let law_offset = nodes.len() as u32;
            for (_, node) in program.nodes.iter() {
                nodes.push(Node {
                    op: offset_op(&node.op, law_offset),
                    ty: node.ty,
                    loc: node.loc,
                    first: offset_node(node.first, law_offset),
                });
            }
            Some(BudgetLimit::Computed(NodeId(local_root.0 + law_offset)))
        }
        ast::Limit::Share { percent, of } => {
            let word = Word {
                text: of.0,
                loc: entry.file.loc(of.0),
            };
            let of = world
                .purpose(entry.home, word)
                .map_err(|problem| diags.push(problem))
                .ok()?;
            let rate = Ratio::percent(percent.mantissa as i128, percent.scale)?;
            Some(BudgetLimit::Ready(Limit::Share { rate, of }))
        }
    }
}

fn funding(
    world: &World<'_>,
    _home: Home,
    file: &ast::File<'_>,
    funded: Option<ast::Funding<'_>>,
    _loc: axiom_core::Loc,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<(Id<crate::book::Place>, Id<crate::book::Place>)>> {
    funded
        .map(|funding| {
            let from = world.place(Word {
                text: funding.from.0,
                loc: file.loc(funding.from.0),
            });
            let to = world.place(Word {
                text: funding.into.0,
                loc: file.loc(funding.into.0),
            });
            match (from, to) {
                (Ok(from), Ok(to)) => Some((from, to)),
                (from, to) => {
                    if let Err(problem) = from {
                        diags.push(problem);
                    }
                    if let Err(problem) = to {
                        diags.push(problem);
                    }
                    None
                }
            }
        })
        .map_or(Some(None), |funded| funded.map(Some))
}

fn push_node(
    nodes: &mut Arena<Node>,
    op: Op,
    ty: Ty,
    loc: axiom_core::Loc,
    first: Option<NodeId>,
) -> NodeId {
    let id = NodeId(nodes.len() as u32);
    nodes.push(Node {
        op,
        ty: Some(ty),
        loc,
        first: first.unwrap_or(id),
    });
    id
}

fn offset_node(node: NodeId, by: u32) -> NodeId {
    NodeId(node.0 + by)
}

/// Moves every child reference when appending one independently compiled
/// budget formula into its budget law's shared arena.
fn offset_op(op: &Op, by: u32) -> Op {
    let one = |node| offset_node(node, by);
    match op {
        Op::Const(value) => Op::Const(*value),
        Op::Var(var) => Op::Var(*var),
        Op::Local(node) => Op::Local(one(*node)),
        Op::Field(node, field) => Op::Field(one(*node), *field),
        Op::Param(param, keys) => Op::Param(*param, keys.iter().copied().map(one).collect()),
        Op::Call(func, args) => Op::Call(*func, args.iter().copied().map(one).collect()),
        Op::Of(left, right) => Op::Of(one(*left), one(*right)),
        Op::At(left, right) => Op::At(one(*left), one(*right)),
        Op::Select(keys) => Op::Select(keys.clone()),
        Op::Neg(node) => Op::Neg(one(*node)),
        Op::Not(node) => Op::Not(one(*node)),
        Op::Bin(op, left, right) => Op::Bin(*op, one(*left), one(*right)),
        Op::Is(node, alternatives) => {
            Op::Is(one(*node), alternatives.iter().copied().map(one).collect())
        }
        Op::If(condition, yes, no) => Op::If(one(*condition), one(*yes), one(*no)),
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
