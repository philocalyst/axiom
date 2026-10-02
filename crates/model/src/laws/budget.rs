//! Native purpose-budget declaration and dated terms lowering.

use axiom_core::{Arena, Day, Days, Diagnostic, Id, Map, Ratio, Set, Severity, Timeline};
use axiom_syntax::{self as ast, DeclKind, ItemKind};

use super::compile;
use crate::book::{Amount, Budget, BudgetTerms, Limit};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::law::{BinOp, Func, Law, Node, NodeId, Op, Owner, Rank, Step, StepKind, Trigger, Ty, Value, Var};
use crate::scope::Home;
use crate::sources::Site;

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
pub(super) fn declare<'a, 's>(world: &mut World<'s>, sites: &'a [Site<'a, 's>], diags: &mut Vec<Diagnostic>) {
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
                                Diagnostic::error("budget-owner", "a declaration budget belongs to a purpose")
                                    .label(item.loc, "write this inside a purpose declaration"),
                            );
                            continue;
                        }
                        let word = Word::of(file, decl.name.0);
                        let Some(purpose) = world.purpose(source.home, word).or_report(diags) else {
                            continue;
                        };
                        if !declared.insert(purpose) {
                            diags.push(
                                Diagnostic::error(
                                    "duplicate-budget",
                                    format!("purpose `{}` has more than one starting budget", word.text),
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
                    let word = Word::of(file, budget.purpose.0);
                    let Some(purpose) = world.purpose(source.home, word).or_report(diags) else {
                        continue;
                    };
                    if !declared.insert(purpose) {
                        diags.push(
                            Diagnostic::error(
                                "duplicate-budget",
                                format!("purpose `{}` has more than one starting budget", word.text),
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
                            Diagnostic::error("budget-owner", "a dated budget change must name a purpose with `#`")
                                .label(item.loc, "write `#purpose now budget …`"),
                        );
                        continue;
                    };
                    let word = Word::of(file, name.0);
                    let Some(purpose) = world.purpose(source.home, word).or_report(diags) else {
                        continue;
                    };
                    let mut until = None;
                    let mut invalid_until = false;
                    for clause in &file[statement.tail] {
                        if let axiom_syntax::ClauseKind::Until(day) = clause.kind {
                            if until.replace(day).is_some() {
                                invalid_until = true;
                                diags.push(
                                    Diagnostic::error("duplicate-until", "a budget change has one `until` date")
                                        .label(clause.at, "remove this extra end date"),
                                );
                            }
                        }
                    }
                    if invalid_until {
                        continue;
                    }
                    let last = until.unwrap_or(Day::MAX);
                    let Some(days) = Days::new(statement.date, last) else {
                        diags.push(
                            Diagnostic::error("budget-until", "a budget change ends before it begins")
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

    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort_by_key(|(purpose, _)| purpose.index());
    for (purpose, mut entries) in entries {
        entries.sort_by_key(|entry| {
            if entry.declared { Day::MIN } else { entry.days.expect("dated budget entries have a range").first() }
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
    let starts = first.days.map_or(Day::MIN, |days| days.first());
    let initial_terms = BudgetTerms {
        limit: Limit::Amount(Amount::zero(world.book.base)),
        period: first.allowance.per,
        carries: false,
        funded: None,
    };
    let mut timeline = Timeline::new(initial_terms);
    let mut has_starting_terms = false;
    for entry in entries.iter().copied() {
        let Some(days) = entry.days else {
            if !entry.declared {
                diags.push(
                    Diagnostic::error("duplicate-budget", "a purpose has more than one starting budget")
                        .label(entry.loc, "remove the duplicate starting budget"),
                );
                continue;
            }
            // A declaration is active from the beginning of the timeline.
            // Its own values replace the initial zero/default terms.
            let prior = *timeline.at(Day::MIN);
            let nodes_before = nodes.len();
            let Some(terms) = lower_budget_terms(world, purpose, entry, purpose_name, prior, &mut nodes, diags) else {
                nodes.truncate(nodes_before);
                return None;
            };
            timeline = Timeline::new(terms);
            has_starting_terms = true;
            continue;
        };
        let prior = *timeline.at(days.first());
        let nodes_before = nodes.len();
        let Some(terms) = lower_budget_terms(world, purpose, entry, purpose_name, prior, &mut nodes, diags) else {
            nodes.truncate(nodes_before);
            if !has_starting_terms {
                return None;
            }
            continue;
        };
        timeline.paint(days, terms);
        has_starting_terms = true;
    }

    // The Plan reads Share dependencies from this effective terms timeline;
    // do not duplicate those subscriptions as dead expression nodes.
    let budget_id = Id::new(world.book.budgets.len() as u32);
    let law_id = Id::new(world.book.laws.len() as u32);

    let mut steps = Vec::new();
    if starts != Day::MIN {
        let date = push_node(&mut nodes, Op::Var(Var::Date), Ty::Day, first.loc, None);
        let day = push_node(&mut nodes, Op::Const(Value::Day(starts)), Ty::Day, first.loc, None);
        let after_start = push_node(&mut nodes, Op::Bin(BinOp::Ge, date, day), Ty::Bool, first.loc, Some(NodeId(0)));
        steps.push(Step { loc: first.loc, kind: StepKind::When(after_start) });
    }
    let total =
        push_node(&mut nodes, Op::Call(Func::BudgetTotal(budget_id), Box::default()), Ty::AMOUNT, first.loc, None);
    let cap =
        push_node(&mut nodes, Op::Call(Func::BudgetLimit(budget_id), Box::default()), Ty::AMOUNT, first.loc, None);
    let condition = push_node(&mut nodes, Op::Bin(BinOp::Le, total, cap), Ty::Bool, first.loc, Some(NodeId(0)));
    steps.push(Step {
        loc: first.loc,
        kind: StepKind::Require {
            cond: condition,
            otherwise: Box::default(),
            message: None,
            severity: Severity::Warning,
        },
    });

    let budget = Budget { purpose, starts, terms: timeline, law: law_id, loc: first.loc };
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

fn lower_budget_terms<'s>(
    world: &mut World<'s>,
    purpose: Id<crate::book::Purpose>,
    entry: BudgetEntry<'_, 's>,
    name: axiom_core::Sym,
    prior: BudgetTerms,
    nodes: &mut Arena<Node>,
    diags: &mut Vec<Diagnostic>,
) -> Option<BudgetTerms> {
    let limit = lower_budget_limit(world, purpose, entry, name, nodes, diags)?;
    let funded = if entry.allowance.funded.is_some() {
        funding(world, entry.file, entry.allowance.funded, diags)?
    } else if entry.declared {
        None
    } else {
        prior.funded
    };
    Some(BudgetTerms {
        limit: match limit {
            BudgetLimit::Ready(limit) => limit,
            BudgetLimit::Computed(root) => Limit::Computed(root),
        },
        period: entry.allowance.per,
        carries: entry.allowance.carries.unwrap_or_else(|| if entry.declared { false } else { prior.carries }),
        funded,
    })
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
            let amount = world.literal_amount(entry.file, literal, Some(world.book.base)).or_report(diags)?;
            Some(BudgetLimit::Ready(Limit::Amount(amount)))
        }
        ast::Limit::Amount(ast::Amount::Computed(root)) => {
            let (program, local_root) =
                compile::compile_budget_limit(world, diags, entry.file, entry.home, purpose, name, root)?;
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
            let word = Word::of(entry.file, of.0);
            let of = world.purpose(entry.home, word).or_report(diags)?;
            let rate = Ratio::percent(percent.mantissa as i128, percent.scale)?;
            Some(BudgetLimit::Ready(Limit::Share { rate, of }))
        }
    }
}

fn funding(
    world: &World<'_>,
    file: &ast::File<'_>,
    funded: Option<ast::Funding<'_>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<(Id<crate::book::Place>, Id<crate::book::Place>)>> {
    funded
        .map(|funding| {
            let from = world.place(Word::of(file, funding.from.0));
            let to = world.place(Word::of(file, funding.into.0));
            match (from, to) {
                (Ok(from), Ok(to)) => Some((from, to)),
                (from, to) => {
                    from.or_report(diags);
                    to.or_report(diags);
                    None
                }
            }
        })
        .map_or(Some(None), |funded| funded.map(Some))
}

fn push_node(nodes: &mut Arena<Node>, op: Op, ty: Ty, loc: axiom_core::Loc, first: Option<NodeId>) -> NodeId {
    let id = NodeId(nodes.len() as u32);
    nodes.push(Node { op, ty: Some(ty), loc, first: first.unwrap_or(id) });
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
        Op::Is(node, alternatives) => Op::Is(one(*node), alternatives.iter().copied().map(one).collect()),
        Op::Resides(entity, systems) => Op::Resides(one(*entity), systems.clone()),
        Op::If(condition, yes, no) => Op::If(one(*condition), one(*yes), one(*no)),
    }
}
