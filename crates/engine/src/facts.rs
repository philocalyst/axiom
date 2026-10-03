//! What a law says that never changes, read off its syntax tree once.
//!
//! A law is a tree of nodes, and the fold asks the same few questions of it for
//! every flow: is it only a cap on a total, which windows does it read, what
//! does its `require` compare, and does it only warn? The answers depend on the
//! law alone, so the plan works them out once into [`LawFacts`] and the fold
//! reads them: nothing walks a law's nodes again.

use axiom_core::{Days, Id, Map, Severity, Sym};
use axiom_model::{
    BinOp, Book, Cap, Dir, Effect as LawEffect, Func, Law, NodeId, Op, Owner, Purpose, Rule, StepKind, Subject, Table,
    Trigger, Ty, Value, Var, Window,
};

use crate::Bound;
use crate::budget;
use crate::eval::Occasion;

/// Everything static about one law.
pub(crate) struct LawFacts {
    /// A cheaper way to decide that the law holds, if it has one.
    pub shortcut: Option<Shortcut>,
    /// The finest window of flow total it reads, month or year: the laws to
    /// read as a window opens with value already recognized into it.
    pub window: Option<Window>,
    /// What its `total(…)` calls read, which decides whose flows are counted.
    pub totals: TotalsRead,
    /// By step; only a `require` or `warn` has anything to say.
    pub steps: Box<[StepFacts]>,
}

/// How a law can be decided to hold without running it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Shortcut {
    /// One cap on a total in the base currency (see [`Law::cap`]): the total
    /// in its window against the limit is all the law compares.
    Cap(Cap),
    /// `balance >= empty`, the standard overdraft law: the holdings of the
    /// subject, added up, against nothing.
    FloorOfNothing,
}

/// What a law's `total(…)` calls read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TotalsRead {
    Nothing,
    /// The totals of the subject it governs.
    Subject,
    /// A kind among the arguments widens a total to every place of that kind.
    Kind,
}

/// What one step of a law compares. Only a `require` or `warn` is anything.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StepFacts {
    /// The finest total or tally the comparison reads.
    pub reads: Option<Reads>,
    /// Whether the comparison is a ceiling or a floor, when it orders amounts.
    pub bound: Option<Bound>,
    /// It only warns.
    pub warn: bool,
    /// What the compared side is a running sum of, if this flow moves it by its own amount.
    pub follows: Option<Follows>,
}

impl StepFacts {
    const NONE: StepFacts = StepFacts { reads: None, bound: None, warn: false, follows: None };
}

/// What a comparison reads, which decides the window its limit lives in: a
/// total in its own window, or a tally in the year.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reads {
    Total(Dir, Window),
    Purpose(Window),
    Budget(Id<axiom_model::Budget>),
    Tally(Sym),
}

impl Reads {
    /// The finest total or tally the condition rooted at `cond` reads: the
    /// window a comparison is about is the shortest it reads.
    fn of(law: &Law, cond: NodeId) -> Option<Reads> {
        let read = law.range(cond).filter_map(|at| match &law.nodes[NodeId(at as u32)].op {
            Op::Call(Func::Total(dir, window), _) => Some(Reads::Total(*dir, *window)),
            Op::Call(Func::PurposeTotal { window, .. }, _) => Some(Reads::Purpose(*window)),
            Op::Call(Func::BudgetTotal(budget), _) => Some(Reads::Budget(*budget)),
            // A tally of another year is settled, not a window this flow is adding to.
            Op::Call(Func::Tally(name), args) if Func::tally_year(args).is_none() => Some(Reads::Tally(*name)),
            _ => None,
        });
        read.min_by_key(|read| match read {
            Reads::Total(_, Window::Month) => 0,
            Reads::Purpose(Window::Month) => 0,
            Reads::Budget(_) => 0,
            Reads::Total(_, Window::Year) | Reads::Purpose(Window::Year) | Reads::Tally(_) => 1,
            Reads::Total(_, Window::Ever) => 2,
            Reads::Purpose(Window::Ever) => 2,
        })
    }

    /// The days the reading covers on this occasion.
    pub fn window(self, book: &Book, on: &Occasion) -> Days {
        match self {
            Reads::Total(_, window) | Reads::Purpose(window) => window.around(on.anchor()),
            Reads::Budget(id) => book
                .budgets
                .get(id)
                .map_or_else(|| Days::on(on.anchor()), |budget| budget::segment_days(budget, on.anchor())),
            Reads::Tally(_) => Window::Year.around(on.over.first()),
        }
    }
}

/// What a comparison's left side is a running sum of, so that this flow moves
/// it by its own amount.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Follows {
    /// The flow's own amount.
    Flow,
    /// A window total the flow was just added to.
    Total(Dir, Window),
    /// A tally an earlier step of the same law counted this flow's amount into.
    Tally(Sym),
}

impl LawFacts {
    pub fn of(book: &Book, law: &Law) -> LawFacts {
        LawFacts {
            shortcut: shortcut(book, law),
            window: window_read(book, law),
            totals: totals_read(law),
            steps: (0..law.steps.len()).map(|at| step(law, at)).collect(),
        }
    }
}

fn shortcut(book: &Book, law: &Law) -> Option<Shortcut> {
    let cap = law.cap().filter(|cap| cap.limit.unit == book.base && law.trigger != Trigger::Always);
    cap.map(Shortcut::Cap).or_else(|| is_floor_of_nothing(law).then_some(Shortcut::FloorOfNothing))
}

/// One step, `require balance >= empty`, and nothing else.
fn is_floor_of_nothing(law: &Law) -> bool {
    let [step] = &*law.steps else { return false };
    let StepKind::Require { cond, otherwise, .. } = &step.kind else { return false };
    let Op::Bin(BinOp::Ge, balance, nothing) = law.nodes[*cond].op else { return false };
    law.trigger == Trigger::Always
        && otherwise.is_empty()
        && matches!(law.nodes[balance].op, Op::Var(Var::Balance))
        && matches!(law.nodes[nothing].op, Op::Const(Value::Empty))
}

/// The finest window of flow total a law reads, month or year.
fn window_read(book: &Book, law: &Law) -> Option<Window> {
    let windows = law.nodes.values().flat_map(|node| match node.op {
        Op::Call(Func::Total(_, window), _) if window != Window::Ever => vec![window],
        Op::Call(Func::PurposeTotal { window, .. }, _) if window != Window::Ever => vec![window],
        Op::Call(Func::BudgetTotal(id), _) => book
            .budgets
            .get(id)
            .into_iter()
            .flat_map(|budget| budget.terms.within(Days::ALWAYS).map(|(_, terms)| terms.period))
            .map(|period| match period {
                axiom_core::Period::Month => Window::Month,
                axiom_core::Period::Year => Window::Year,
            })
            .collect(),
        _ => Vec::new(),
    });
    windows.min_by_key(|&window| window == Window::Year)
}

fn totals_read(law: &Law) -> TotalsRead {
    let widened = |args: &[NodeId]| args.iter().any(|arg| law.nodes[*arg].typed_ty() == Some(Ty::Kind));
    let reads = law.nodes.values().filter_map(|node| match &node.op {
        Op::Call(Func::Total(..), args) => Some(widened(args)),
        Op::Call(Func::PurposeTotal { .. }, _) => Some(false),
        Op::Call(Func::BudgetTotal(_), _) => Some(false),
        _ => None,
    });
    match reads.reduce(|a, b| a || b) {
        None => TotalsRead::Nothing,
        Some(false) => TotalsRead::Subject,
        Some(true) => TotalsRead::Kind,
    }
}

fn step(law: &Law, at: usize) -> StepFacts {
    let StepKind::Require { cond, severity, .. } = law.steps[at].kind else { return StepFacts::NONE };
    let bound = match law.nodes[cond].op {
        Op::Bin(BinOp::Le | BinOp::Lt, ..) => Some(Bound::Cap),
        Op::Bin(BinOp::Ge | BinOp::Gt, ..) => Some(Bound::Floor),
        _ => None,
    };
    let follows = bound.and_then(|_| follows(law, at, cond));
    StepFacts { reads: Reads::of(law, cond), bound, warn: severity == Severity::Warning, follows }
}

/// What the left side of the ordering `cond` is a running sum of.
fn follows(law: &Law, step: usize, cond: NodeId) -> Option<Follows> {
    let Op::Bin(_, lhs, _) = law.nodes[cond].op else { return None };
    match (&law.nodes[lhs].op, law.trigger) {
        (Op::Var(Var::Amount), _) => Some(Follows::Flow),
        (Op::Call(Func::Total(Dir::In, window), _), Trigger::In) => Some(Follows::Total(Dir::In, *window)),
        (Op::Call(Func::Total(Dir::Out, window), _), Trigger::Out) => Some(Follows::Total(Dir::Out, *window)),
        (Op::Call(Func::Tally(name), args), _) if Func::tally_year(args).is_none() => {
            let counts_amount = |kind: &StepKind| match kind {
                StepKind::Effect(LawEffect::Count { amount, name: counted }) => {
                    counted == name && matches!(law.nodes[*amount].op, Op::Var(Var::Amount))
                }
                _ => false,
            };
            law.steps[..step].iter().any(|s| counts_amount(&s.kind)).then_some(Follows::Tally(*name))
        }
        _ => None,
    }
}

/// The rules that read window totals, by what they watch and the window whose
/// opening they want to see: the finest one they read.
pub(crate) type Readers = Map<(Subject, Window), Vec<Rule>>;

/// Every rule that reads a month's or a year's total, once each.
pub(crate) fn readers(book: &Book, laws: &[LawFacts]) -> Readers {
    let mut readers = Readers::default();
    for &rule in Table::PLACE.into_iter().flat_map(|table| book.rules.table(table)) {
        let Some(window) = laws[rule.law.index()].window else { continue };
        let known = readers.entry((rule.subject, window)).or_default();
        if !known.contains(&rule) {
            known.push(rule);
        }
    }
    readers
}

/// Purpose-window readers, keyed by the purpose value that advances rather
/// than by the law table that happens to inherit the rule. One law may read
/// several explicit purposes; each becomes a subscription. Inherited purpose
/// tables can repeat a Rule, so each `(purpose, window, Rule)` is stored once.
pub(crate) type PurposeReaders = Map<(Id<Purpose>, Window), Vec<Rule>>;

pub(crate) fn purpose_readers(book: &Book) -> PurposeReaders {
    let mut readers = PurposeReaders::default();
    let mut seen = Vec::new();
    for &rule in book.rules.table(Table::Purpose) {
        if seen.contains(&rule) {
            continue;
        }
        seen.push(rule);
        let law = &book.laws[rule.law];
        let implicit = match law.owner {
            Owner::Purpose(purpose) => Some(purpose),
            _ => None,
        };
        // A read in a let, gate, unused branch, or effect does not by itself
        // make a window obligation. Subscribe to purpose totals only when a
        // require compares them; the evaluator filters the rest at the open.
        for step in &law.steps {
            let axiom_model::StepKind::Require { cond, .. } = &step.kind else { continue };
            for at in law.range(*cond) {
                match &law.nodes[NodeId(at as u32)].op {
                    Op::Call(Func::PurposeTotal { purpose, window }, _) if *window != Window::Ever => {
                        let Some(purpose) = (*purpose).or(implicit) else { continue };
                        add_purpose_reader(&mut readers, purpose, *window, rule);
                    }
                    Op::Call(Func::BudgetTotal(id), _) => {
                        let Some(budget) = book.budgets.get(*id) else { continue };
                        for (_, terms) in budget.terms.within(Days::ALWAYS) {
                            let window = match terms.period {
                                axiom_core::Period::Month => Window::Month,
                                axiom_core::Period::Year => Window::Year,
                            };
                            add_purpose_reader(&mut readers, budget.purpose, window, rule);
                            if let axiom_model::Limit::Share { of, .. } = terms.limit {
                                add_purpose_reader(&mut readers, of, window, rule);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    readers
}

fn add_purpose_reader(readers: &mut PurposeReaders, purpose: Id<Purpose>, window: Window, rule: Rule) {
    let rules = readers.entry((purpose, window)).or_default();
    if !rules.contains(&rule) {
        rules.push(rule);
    }
}
