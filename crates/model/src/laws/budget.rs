//! Native purpose-budget declaration and dated terms lowering.

use axiom_core::{Arena, Day, Days, Diagnostic, Id, Loc, Map, Ratio, Set, Severity, Timeline};
use axiom_syntax::{self as ast, DeclKind, ItemKind};

use super::compile;
use crate::book::{Amount, Budget, BudgetTerms, Limit, Purpose};
use crate::declare::World;
use crate::errors::{Reported, Word};
use crate::law::{BinOp, Func, Law, Node, NodeId, Op, Owner, Rank, Step, StepKind, Trigger, Ty, Value, Var};
use crate::scope::Home;
use crate::sources::Site;

/// When a budget row takes effect.
#[derive(Clone, Copy)]
enum Term {
    /// A purpose's own starting budget: from the beginning of time, with nothing before it to carry.
    Starting,
    /// A dated change, in force on these days and on top of what was in force before.
    Dated(Days),
}

impl Term {
    fn begins(self) -> Day {
        match self {
            Term::Starting => Day::MIN,
            Term::Dated(days) => days.first(),
        }
    }
}

/// One row of a purpose's budget.
#[derive(Clone, Copy)]
struct BudgetEntry<'a, 's> {
    purpose: Id<Purpose>,
    file: &'a ast::File<'s>,
    home: Home,
    allowance: ast::Allowance<'s>,
    term: Term,
    loc: Loc,
}

/// The budget rows of every file, by purpose, in the order written.
#[derive(Default)]
struct Found<'a, 's> {
    entries: Map<Id<Purpose>, Vec<BudgetEntry<'a, 's>>>,
    /// The purposes with a starting budget already, which a second is said to repeat.
    declared: Set<Id<Purpose>>,
}

/// Lowers declaration and dated budget rows after every purpose has an id.
/// Each generated warning law reads the effective limit through one typed
/// `BudgetLimit` call, so later rows and bounded `until` changes are not baked
/// into a stale literal.
pub(super) fn declare<'a, 's>(world: &mut World<'s>, sites: &'a [Site<'a, 's>]) {
    let mut found = Found::default();
    for site in sites {
        for item in &site.source.file.items {
            found.read(world, site, item);
        }
    }
    let mut by_purpose: Vec<_> = found.entries.into_iter().collect();
    by_purpose.sort_by_key(|(purpose, _)| purpose.index());
    for (_, mut entries) in by_purpose {
        entries.sort_by_key(|entry| entry.term.begins());
        lower_budget(world, &entries);
    }
}

impl<'a, 's> Found<'a, 's> {
    /// What one item says about budgets, which is nothing for most.
    fn read(&mut self, world: &mut World<'s>, site: &Site<'a, 's>, item: &ast::Item<'s>) {
        let file: &'a ast::File<'s> = &site.source.file;
        match item.kind {
            ItemKind::Decl(reference) => {
                let decl = &file[reference];
                let Some(allowance) = decl.budget else {
                    return;
                };
                if decl.what != DeclKind::Purpose {
                    world.diags.push(
                        Diagnostic::error("budget-owner", "a declaration budget belongs to a purpose")
                            .label(item.loc, "write this inside a purpose declaration"),
                    );
                    return;
                }
                self.starting(world, site, decl.name, file[allowance], item.loc);
            }
            ItemKind::Budget(reference) => {
                let budget = &file[reference];
                self.starting(world, site, budget.purpose, budget.allowance, item.loc);
            }
            ItemKind::Statement(reference) => self.change(world, site, item, &file[reference]),
            _ => {}
        }
    }

    /// A purpose's starting budget, written once.
    fn starting(
        &mut self,
        world: &mut World<'s>,
        site: &Site<'a, 's>,
        purpose: ast::Name<'s>,
        allowance: ast::Allowance<'s>,
        loc: Loc,
    ) {
        let file: &'a ast::File<'s> = &site.source.file;
        let word = Word::of(file, purpose.0);
        let Some(purpose) = world.purpose(site.home, word).or_report(world) else {
            return;
        };
        if !self.declared.insert(purpose) {
            world.diags.push(
                Diagnostic::error(
                    "duplicate-budget",
                    format!("purpose `{}` has more than one starting budget", word.text),
                )
                .label(loc, "keep one initial budget declaration"),
            );
            return;
        }
        let entry = BudgetEntry { purpose, file, home: site.home, allowance, term: Term::Starting, loc };
        self.entries.entry(purpose).or_default().push(entry);
    }

    /// `#purpose now budget …`, which may end `until` a day.
    fn change(
        &mut self,
        world: &mut World<'s>,
        site: &Site<'a, 's>,
        item: &ast::Item<'s>,
        statement: &ast::Statement<'s>,
    ) {
        let file: &'a ast::File<'s> = &site.source.file;
        let ast::Verb::Now(ast::Change::Budget(allowance)) = statement.verb else {
            return;
        };
        let ast::Subject::Purpose(name) = statement.subject else {
            world.diags.push(
                Diagnostic::error("budget-owner", "a dated budget change must name a purpose with `#`")
                    .label(item.loc, "write `#purpose now budget …`"),
            );
            return;
        };
        let Some(purpose) = world.purpose(site.home, Word::of(file, name.0)).or_report(world) else {
            return;
        };
        let Some(days) = Days::new(statement.date, last_day(file, statement)) else {
            world.diags.push(
                Diagnostic::error("budget-until", "a budget change ends before it begins")
                    .label(item.loc, "the `until` date precedes this change"),
            );
            return;
        };
        let entry = BudgetEntry {
            purpose,
            file,
            home: site.home,
            allowance: file[allowance],
            term: Term::Dated(days),
            loc: item.loc,
        };
        self.entries.entry(purpose).or_default().push(entry);
    }
}

/// The last day a budget change is in force: its `until`, if it has one, or never. The parser lets a statement
/// say `until` once, so there is no second to disagree with.
fn last_day(file: &ast::File<'_>, statement: &ast::Statement<'_>) -> Day {
    let until = file[statement.tail].iter().find_map(|clause| match clause.kind {
        ast::ClauseKind::Until(day) => Some(day),
        _ => None,
    });
    until.unwrap_or(Day::MAX)
}

/// One purpose's budget as a law that warns when its spending passes the limit.
fn lower_budget<'s>(world: &mut World<'s>, entries: &[BudgetEntry<'_, 's>]) {
    let Some(&first) = entries.first() else {
        return;
    };
    let mut nodes = Arena::new();
    let Some(terms) = terms_over_time(world, entries, &mut nodes) else {
        return;
    };
    // The Plan reads Share dependencies from this effective terms timeline;
    // do not duplicate those subscriptions as dead expression nodes.
    let (budget_id, law_id) = (Id::new(world.book.budgets.len() as u32), Id::new(world.book.laws.len() as u32));
    let starts = first.term.begins();
    let steps = warning_steps(&mut nodes, budget_id, starts, first.loc);
    let budget = Budget { purpose: first.purpose, starts, terms, law: law_id, loc: first.loc };
    // The law and the budget name each other, so each is told the other's id before it is made.
    let pushed = world.book.budgets.push(budget);
    debug_assert_eq!(pushed, budget_id);
    let purpose = &world.book.purposes[first.purpose];
    let law = Law {
        name: purpose.name,
        doc: None,
        owner: Owner::Purpose(first.purpose),
        system: purpose.system,
        trigger: Trigger::Flow,
        budget: Some(budget_id),
        overrides: None,
        override_name: None,
        rank: Rank::ZERO,
        steps,
        nodes,
        loc: first.loc,
    };
    let pushed = world.book.laws.push(law);
    debug_assert_eq!(pushed, law_id);
}

/// The terms in force on each day: the starting ones, then each dated change on top. A row that cannot be
/// lowered is said and skipped once something is in force, and ends the budget before that.
fn terms_over_time<'s>(
    world: &mut World<'s>,
    entries: &[BudgetEntry<'_, 's>],
    nodes: &mut Arena<Node>,
) -> Option<Timeline<BudgetTerms>> {
    let first = entries.first()?;
    let nothing = BudgetTerms {
        limit: Limit::Amount(Amount::zero(world.book.base)),
        period: first.allowance.per,
        carries: false,
        funded: None,
    };
    let mut timeline = Timeline::new(nothing);
    let mut in_force = false;
    for &entry in entries {
        let prior = *timeline.at(entry.term.begins());
        match (lower_budget_terms(world, entry, prior, nodes), entry.term) {
            // A starting budget replaces the zero it began with.
            (Some(terms), Term::Starting) => timeline = Timeline::new(terms),
            (Some(terms), Term::Dated(days)) => timeline.paint(days, terms),
            (None, Term::Dated(_)) if in_force => continue,
            (None, _) => return None,
        }
        in_force = true;
    }
    Some(timeline)
}

/// The steps of a budget's law: once it has begun, it warns when what the purpose spent is over the limit.
fn warning_steps(nodes: &mut Arena<Node>, budget: Id<Budget>, starts: Day, loc: Loc) -> Box<[Step]> {
    let mut steps = Vec::new();
    if starts != Day::MIN {
        let date = push_node(nodes, Op::Var(Var::Date), Ty::Day, loc, None);
        let day = push_node(nodes, Op::Const(Value::Day(starts)), Ty::Day, loc, None);
        let after_start = push_node(nodes, Op::Bin(BinOp::Ge, date, day), Ty::Bool, loc, Some(NodeId(0)));
        steps.push(Step { loc, kind: StepKind::When(after_start) });
    }
    let total = push_node(nodes, Op::Call(Func::BudgetTotal(budget), Box::default()), Ty::AMOUNT, loc, None);
    let cap = push_node(nodes, Op::Call(Func::BudgetLimit(budget), Box::default()), Ty::AMOUNT, loc, None);
    let within = push_node(nodes, Op::Bin(BinOp::Le, total, cap), Ty::Bool, loc, Some(NodeId(0)));
    let kind =
        StepKind::Require { cond: within, otherwise: Box::default(), message: None, severity: Severity::Warning };
    steps.push(Step { loc, kind });
    steps.into_boxed_slice()
}

/// What one row says the budget is, on top of `prior`; the nodes it adds are dropped when it cannot be lowered.
fn lower_budget_terms<'s>(
    world: &mut World<'s>,
    entry: BudgetEntry<'_, 's>,
    prior: BudgetTerms,
    nodes: &mut Arena<Node>,
) -> Option<BudgetTerms> {
    let nodes_before = nodes.len();
    let terms = lower_terms_on(world, entry, prior, nodes);
    if terms.is_none() {
        nodes.truncate(nodes_before);
    }
    terms
}

fn lower_terms_on<'s>(
    world: &mut World<'s>,
    entry: BudgetEntry<'_, 's>,
    prior: BudgetTerms,
    nodes: &mut Arena<Node>,
) -> Option<BudgetTerms> {
    let limit = lower_budget_limit(world, entry, nodes)?;
    // A dated change keeps what it does not say; a starting budget has nothing before it to keep.
    let dated = matches!(entry.term, Term::Dated(_));
    let funded = match entry.allowance.funded {
        Some(_) => funding(world, entry.file, entry.allowance.funded)?,
        None if dated => prior.funded,
        None => None,
    };
    Some(BudgetTerms {
        limit,
        period: entry.allowance.per,
        carries: entry.allowance.carries.unwrap_or(dated && prior.carries),
        funded,
    })
}

fn lower_budget_limit<'s>(world: &mut World<'s>, entry: BudgetEntry<'_, 's>, nodes: &mut Arena<Node>) -> Option<Limit> {
    match entry.allowance.limit {
        ast::Limit::Amount(ast::Amount::Literal(literal)) => {
            let amount = world.literal_amount(entry.file, literal, Some(world.book.base)).or_report(world)?;
            Some(Limit::Amount(amount))
        }
        ast::Limit::Amount(ast::Amount::Computed(root)) => {
            let root = compile::compile_budget_limit(world, entry.file, entry.home, entry.purpose, root, nodes)?;
            Some(Limit::Computed(root))
        }
        ast::Limit::Share { percent, of } => {
            let word = Word::of(entry.file, of.0);
            let of = world.purpose(entry.home, word).or_report(world)?;
            let rate = Ratio::percent(percent.mantissa as i128, percent.scale)?;
            Some(Limit::Share { rate, of })
        }
    }
}

fn funding(
    world: &mut World<'_>,
    file: &ast::File<'_>,
    funded: Option<ast::Funding<'_>>,
) -> Option<Option<(Id<crate::book::Place>, Id<crate::book::Place>)>> {
    funded
        .map(|funding| {
            let from = world.place(Word::of(file, funding.from.0));
            let to = world.place(Word::of(file, funding.into.0));
            match (from, to) {
                (Ok(from), Ok(to)) => Some((from, to)),
                (from, to) => {
                    from.or_report(world);
                    to.or_report(world);
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
