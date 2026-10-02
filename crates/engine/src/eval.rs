//! Law evaluation.
//!
//! A law's nodes are one post-order arena, so a step is a forward scan over
//! `first..=root` that writes each node's value into a scratch slot. The slots
//! are indexed by node and every node is evaluated at most once per run, so
//! when the run ends the scratch holds the value of every subexpression that
//! ran. That table is what the power-assert diagnostic prints, and why nothing
//! is allocated per evaluation: the same vector serves every law.
//!
//! [`run`] reports what happened as [`Outcome`]s and changes nothing; the
//! ledger decides what to record. A dry run (does this flow satisfy the laws?)
//! is therefore the same call with the outcomes ignored.

use std::ops::Deref;

use axiom_core::glob::glob;
use axiom_core::{Arena, Day, Days, Id, Qty, Ratio, Severity, Span, Sym, day::days_in_month, spread};
use axiom_model::{
    self, Amount, Asset, BinOp, Book, Commodity, Dir, Effect as LawEffect, Entity, Fault, Field, FlowCodes, Func,
    Holder, Law, NodeId, Object, Op, Param, Purposed, RuntimeDetail, RuntimeFlow, SelectKey, StepKind, Subject, Text,
    Ty, Value, Var, Window,
};

use crate::assets::PartId;
use crate::budget::{
    carry_start as budget_carry_start, segment_end as budget_segment_end, segment_start as budget_segment_start,
    window as budget_window,
};
use crate::calc::{Calc, progressive};
use crate::lots::{Holdings, Slot};
use crate::motion::Motion;
use crate::plan::Plan;
use crate::scope::is_money;
use crate::state::World;
use crate::temporal::Key as TemporalKey;
use crate::{Cause, Owed};

/// What laws read: the plan (and through it the book) and the state as of now.
#[derive(Clone, Copy)]
pub(crate) struct Env<'a, 's> {
    pub plan: &'a Plan<'a, 's>,
    pub world: &'a World,
}

/// What happened that fires laws: a flow moved, a parcel realized a gain, a
/// period ended or a deadline passed.
#[derive(Clone, Copy)]
pub(crate) struct Occasion<'a> {
    /// When it happened: `date`, and the day prices are read.
    pub day: Day,
    /// The days it belongs to. `year` and `month` are its first day's, and a
    /// count is shared out over it. A flow's is its recognition; a period's is
    /// its last day.
    pub over: Days,
    /// The days a rule must be in force for: a flow's day, or a whole period.
    pub span: Days,
    pub cause: Cause,
    /// The flow that fired the law, if one did (not for `each` and `by`).
    pub motion: Option<&'a Motion<'a>>,
    /// What the triggering flow was for, if it had a purpose.
    pub purpose: Option<Purposed>,
    /// The triggering flow's description, if written.
    pub description: Option<Text>,
    /// `amount`: what the trigger says is moving.
    pub amount: Option<Amount>,
    /// `gain`, `proceeds`, `basis`, `held`, for `on gain`.
    pub realized: Option<Realized>,
    /// Rules whose subject contains both ends of the flow do not fire: value
    /// moved around inside the subject neither entered nor left it.
    pub skip_internal: bool,
    /// Only limits are read: what a comparison counted is recorded, and a
    /// broken one is reported, but nothing is counted into a tally or owed.
    pub checking: bool,
    /// A purpose window opening evaluates only matching `require` steps and
    /// their preceding gates; flow amounts and unrelated effects are absent.
    pub purpose_window: Option<Window>,
    /// A sale is being evaluated before disposal. A `mid-month` depreciation
    /// schedule takes only half of this terminal month.
    pub partial_terminal: bool,
}

impl<'a> Occasion<'a> {
    /// Something that happened on `day`, with nothing more said about it yet.
    fn on(day: Day, over: Days, span: Days, cause: Cause, motion: Option<&'a Motion<'a>>) -> Occasion<'a> {
        let (amount, realized, skip_internal, checking) = (None, None, false, false);
        Occasion {
            day,
            over,
            span,
            cause,
            motion,
            purpose: None,
            description: None,
            amount,
            realized,
            skip_internal,
            checking,
            purpose_window: None,
            partial_terminal: false,
        }
    }

    pub fn flow(m: &'a Motion<'a>) -> Occasion<'a> {
        Occasion {
            purpose: m.purpose,
            description: m.description,
            ..Occasion::on(m.day, m.recognized, Days::on(m.day), m.cause, Some(m))
        }
    }

    /// A period ending, or a deadline passing, on `day`.
    pub fn time(day: Day, period: Days) -> Occasion<'static> {
        Occasion::on(day, Days::on(period.last()), period, Cause::Time, None)
    }

    /// A window that some flow recognized value into ahead of time, entered on
    /// `day`: the laws about its total are read as no flow will make them.
    pub fn window(day: Day, period: Days) -> Occasion<'static> {
        Occasion { checking: true, ..Occasion::time(day, period) }
    }

    /// A purpose total reaches a new month or year before its flows land.
    pub fn purpose_window(day: Day, period: Days, window: Window) -> Occasion<'static> {
        Occasion { purpose_window: Some(window), ..Occasion::window(day, period) }
    }

    /// A partial final period evaluated immediately before an asset is sold.
    pub fn partial_terminal(day: Day, period: Days) -> Occasion<'static> {
        Occasion { partial_terminal: true, ..Occasion::on(day, Days::on(day), period, Cause::Time, None) }
    }

    /// The day whose window totals are read: the day a flow moved, or the last
    /// day of the period a law closes.
    pub fn anchor(&self) -> Day {
        if self.motion.is_some() { self.day } else { self.over.first() }
    }
}

/// What a law's variables are bound to for one firing.
pub(crate) struct Context<'a> {
    /// What `self` is.
    pub subject: Subject,
    pub owner: Id<Entity>,
    /// Governing purpose for purpose-law or template expression evaluation.
    pub governing_purpose: Option<Id<axiom_model::Purpose>>,
    /// A compiled journal or contract-template expression has `self: flow`;
    /// ordinary law subjects remain place/entity/asset values.
    pub flow_subject: bool,
    /// A dated budget limit is re-evaluated at each prior window's end. These
    /// reads use retained facts instead of only the current rolling window.
    pub budget_history: bool,
    /// Declaration-order bindings for a contract occurrence.
    pub inputs: Option<&'a [Option<Amount>]>,
    /// The source-order flows of the current materialized occurrence. A
    /// contract expression's `[selector]` reads these borrowed views only.
    pub template_flows: Option<TemplateFlows<'a>>,
    /// The asset part whose per-part law is running, if any.
    pub asset_part: Option<PartId>,
    /// Stable law identity for temporal expression history.
    pub law_id: Option<Id<Law>>,
    on: &'a Occasion<'a>,
}

/// Borrowed occurrence members used by `Op::Select`. The materializer owns the
/// flows and runtime detail arena for the duration of expression evaluation.
#[derive(Clone, Copy)]
pub(crate) struct TemplateFlows<'a> {
    flows: &'a [RuntimeFlow],
    details: &'a Arena<RuntimeDetail>,
}

impl<'a> TemplateFlows<'a> {
    pub fn new(flows: &'a [RuntimeFlow], details: &'a Arena<RuntimeDetail>) -> Self {
        Self { flows, details }
    }
}

impl<'a> Context<'a> {
    pub fn new(subject: Subject, owner: Id<Entity>, on: &'a Occasion<'a>) -> Context<'a> {
        Context {
            subject,
            owner,
            governing_purpose: None,
            flow_subject: false,
            budget_history: false,
            inputs: None,
            template_flows: None,
            asset_part: None,
            law_id: None,
            on,
        }
    }

    pub fn with_inputs(mut self, inputs: &'a [Option<Amount>]) -> Self {
        self.inputs = Some(inputs);
        self
    }

    pub fn with_template_flows(mut self, flows: &'a [RuntimeFlow], details: &'a Arena<RuntimeDetail>) -> Self {
        self.template_flows = Some(TemplateFlows::new(flows, details));
        self
    }

    pub fn for_asset_part(mut self, part: PartId) -> Self {
        self.asset_part = Some(part);
        self
    }

    pub fn for_law(mut self, law: Id<Law>) -> Self {
        self.law_id = Some(law);
        self
    }

    pub fn for_purpose(mut self, purpose: Id<axiom_model::Purpose>) -> Self {
        self.governing_purpose = Some(purpose);
        self
    }

    pub fn for_flow(mut self) -> Self {
        self.flow_subject = true;
        self
    }
}

impl<'a> Deref for Context<'a> {
    type Target = Occasion<'a>;
    fn deref(&self) -> &Occasion<'a> {
        self.on
    }
}

/// One parcel's realization.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Realized {
    pub gain: Qty,
    pub proceeds: Qty,
    pub basis: Qty,
    /// The quantity of this individual parcel, which may be only part of the
    /// flow when a sale relieves multiple lots.
    pub quantity: Qty,
    /// Original parcel acquisition date for wash-sale holding-period tacking.
    pub acquired: Day,
    /// Effective holding-period start, which may be tacked from an earlier
    /// replacement lot even though `acquired` remains the actual buy date.
    pub held_since: Day,
    pub held: Span,
    pub part: Option<PartId>,
    pub codes: FlowCodes,
}

/// What evaluating a law found.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Outcome {
    /// A `require` or `warn` that does not hold.
    Broken { step: u32, warn: bool },
    /// A `require … else owe …` that did not hold: priced, and owed.
    Priced { step: u32, name: Sym, amount: Amount, owed: Owed },
    /// A fault reached a step.
    Faulted { step: u32, fault: Fault },
    /// `count`: adds `amount` (base currency) to a tally.
    Count { name: Sym, amount: Qty },
    /// `owe`.
    Owe { name: Sym, amount: Amount, owed: Owed },
    /// What a `require` or `warn` compared: `counted <= limit`, the sides of a
    /// `>=` swapped.
    Read { step: u32, counted: Amount, limit: Amount },
    /// A part-scoped reduction of an asset's remaining basis.
    Consume { step: u32, amount: Amount },
    /// A disallowed loss that the asset monitor carries to a matching part.
    Carry { step: u32, amount: Amount, unit: Id<Commodity>, within: Span },
}

/// Runs `law`'s steps in order. Returns whether it ran to the end rather than
/// stopping at a `when` that was false (or faulted).
pub(crate) fn run(
    env: Env,
    law: &Law,
    ctx: &Context,
    values: &mut Vec<Value>,
    budget_values: &mut Vec<Value>,
    out: &mut Vec<Outcome>,
) -> bool {
    values.resize(law.nodes.len(), Value::Empty);
    let law_id = law_identity(env.plan, law, ctx);
    let mut machine = Machine { env, nodes: &law.nodes, law: Some(law), law_id, ctx, values, budget_values, out };
    if let Some(window) = ctx.purpose_window {
        let last = law
            .steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                matches!(&step.kind, StepKind::Require { .. })
                    .then_some(index)
                    .filter(|&index| purpose_reader_step(env.plan.book, law, index, window, ctx.day))
            })
            .last();
        let Some(last) = last else { return true };
        (0..=last)
            .filter(|&index| purpose_reader_step(env.plan.book, law, index, window, ctx.day))
            .all(|index| machine.step(index))
    } else {
        (0..law.steps.len()).all(|index| machine.step(index))
    }
}

/// Gates before the last matching requirement remain meaningful; unrelated
/// requirements and effects are for an actual flow, not a window opening.
fn purpose_reader_step(book: &Book, law: &Law, index: usize, window: Window, day: Day) -> bool {
    match &law.steps[index].kind {
        StepKind::When(_) | StepKind::Unless(_) | StepKind::Let(_) => true,
        StepKind::Require { cond, .. } => law.range(*cond).any(|at| match law.nodes[NodeId(at as u32)].op {
            Op::Call(Func::PurposeTotal { window: read, .. }, _) => read == window,
            Op::Call(Func::BudgetTotal(id), _) => book.budgets.get(id).is_some_and(|budget| {
                if day < budget.starts {
                    return false;
                }
                budget_window(budget.terms.at(day).period) == window
            }),
            _ => false,
        }),
        StepKind::Effect(_) => false,
    }
}

fn law_identity(plan: &Plan<'_, '_>, law: &Law, ctx: &Context<'_>) -> Option<Id<Law>> {
    ctx.law_id.or_else(|| {
        plan.temporal.iter().find_map(|query| {
            (query.key.subject == ctx.subject
                && query.key.owner == ctx.owner
                && std::ptr::eq(&plan.book.laws[query.key.law], law))
            .then_some(query.key.law)
        })
    })
}

/// Records each compiled temporal query at a state boundary. Only expression
/// roots named by `Plan.temporal` are retained; no world snapshot is cloned.
pub(crate) fn sample_temporal<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    world: &mut World,
    day: Day,
    values: &mut Vec<Value>,
) {
    for query in plan.temporal.iter().copied() {
        if let Subject::Asset(asset) = query.key.subject {
            let part_count = world.assets.asset(asset).map_or(0, |state| state.parts().len());
            for index in 0..part_count {
                let part = world.assets.asset(asset).expect("asset state was counted").parts()[index].id;
                sample_temporal_query(plan, world, day, query, Some(part), values);
            }
        } else {
            sample_temporal_query(plan, world, day, query, None, values);
        }
    }
}

fn sample_temporal_query(
    plan: &Plan<'_, '_>,
    world: &mut World,
    day: Day,
    query: crate::temporal::Query,
    part: Option<PartId>,
    values: &mut Vec<Value>,
) {
    let law = &plan.book.laws[query.key.law];
    let occasion = Occasion::time(day, Days::on(day));
    let mut context = Context::new(query.key.subject, query.key.owner, &occasion).for_law(query.key.law);
    if let Some(part) = part {
        context = context.for_asset_part(part);
    }
    let value = expression(Env { plan, world: &*world }, law, query.root, &context, values);
    let mut key = query.key;
    key.part = part;
    world.temporal.record(key, day, value);
}

/// Evaluates one expression of `law` (a `by` date, say).
pub(crate) fn expression(env: Env, law: &Law, root: NodeId, ctx: &Context, values: &mut Vec<Value>) -> Value {
    values.resize(law.nodes.len(), Value::Empty);
    let law_id = law_identity(env.plan, law, ctx);
    Machine {
        env,
        nodes: &law.nodes,
        law: Some(law),
        law_id,
        ctx,
        values,
        budget_values: &mut Vec::new(),
        out: &mut Vec::new(),
    }
    .scan(root)
}

/// Evaluate a typed expression program for a journal, assertion or contract
/// occurrence using caller-owned context and scratch storage.
pub(crate) fn program_expression(
    env: Env,
    program: &axiom_model::TemplateProgram,
    root: NodeId,
    ctx: &Context,
    values: &mut Vec<Value>,
) -> Value {
    values.resize(program.nodes.len(), Value::Empty);
    Machine {
        env,
        nodes: &program.nodes,
        law: None,
        law_id: None,
        ctx,
        values,
        budget_values: &mut Vec::new(),
        out: &mut Vec::new(),
    }
    .scan(root)
}

struct Machine<'a, 's> {
    env: Env<'a, 's>,
    nodes: &'a axiom_core::Arena<axiom_model::Node>,
    law: Option<&'a Law>,
    law_id: Option<Id<Law>>,
    ctx: &'a Context<'a>,
    values: &'a mut Vec<Value>,
    budget_values: &'a mut Vec<Value>,
    /// What the steps so far found. A later step's `tally(…)` sees the `count`s
    /// among them, which the ledger will not have applied until the law is done.
    out: &'a mut Vec<Outcome>,
}

const TYPED: &str = "the model type-checks operands";

impl<'a, 's> Machine<'a, 's> {
    fn book(&self) -> &'a Book<'s> {
        self.env.plan.book
    }

    fn calc(&self) -> Calc<'a, 's> {
        Calc { book: self.env.plan.book, day: self.ctx.day }
    }

    fn base(&self, qty: Qty) -> Value {
        Value::Amount(Amount::new(qty, self.book().base))
    }

    /// An undated declaration uses `Day::MIN` as a timeline sentinel. For a
    /// carrying budget, its first meaningful window is the first window the
    /// journal can reach, rather than millions of empty calendar windows
    /// before the book's first fact.
    fn budget_start(&self, budget: &axiom_model::Budget) -> Day {
        if budget.starts != Day::MIN {
            return budget.starts;
        }
        let first_fact = self.env.plan.period_start.unwrap_or_else(|| self.ctx.anchor());
        budget_window(budget.terms.at(first_fact).period).around(first_fact).first()
    }

    fn at(&self, node: NodeId) -> Value {
        self.values[node.index()]
    }

    /// Evaluates a step's expression: one forward scan, each node stored.
    fn scan(&mut self, root: NodeId) -> Value {
        for at in self.nodes[root].first.index()..=root.index() {
            let Some(ty) = self.nodes[NodeId(at as u32)].typed_ty() else {
                self.values[at] = Value::Fault(Fault::InvalidProgram);
                continue;
            };
            let value = self.node(at);
            self.values[at] = self.calc().typed_value(value, ty);
        }
        self.at(root)
    }

    /// Runs one step. `false` stops the law.
    fn step(&mut self, index: usize) -> bool {
        let law = self.law.expect("only law programs have steps");
        let step = index as u32;
        match &law.steps[index].kind {
            StepKind::When(cond) => match self.scan(*cond) {
                Value::Bool(pass) => pass,
                Value::Fault(fault) => {
                    self.out.push(Outcome::Faulted { step, fault });
                    false
                }
                _ => unreachable!("{TYPED}"),
            },
            StepKind::Let(bound) => {
                self.scan(*bound);
                true
            }
            StepKind::Unless(cond) => match self.scan(*cond) {
                Value::Bool(unless) => !unless,
                Value::Fault(fault) => {
                    self.out.push(Outcome::Faulted { step, fault });
                    false
                }
                _ => unreachable!("{TYPED}"),
            },
            StepKind::Require { cond, otherwise, severity, .. } => {
                let held = self.scan(*cond);
                self.read(step, *cond);
                match held {
                    Value::Bool(true) => {}
                    // v3 bridge: a v3 `require` has one reparation at most.
                    Value::Bool(false) => match otherwise.first() {
                        Some(effect) => self.price(step, effect),
                        None => self.out.push(Outcome::Broken { step, warn: *severity == Severity::Warning }),
                    },
                    Value::Fault(fault) => self.out.push(Outcome::Faulted { step, fault }),
                    _ => unreachable!("{TYPED}"),
                }
                true
            }
            StepKind::Effect(effect) => {
                self.effect(step, effect);
                true
            }
        }
    }

    /// Notes what a comparison of amounts compared: the counted side and its limit.
    fn read(&mut self, step: u32, cond: NodeId) {
        let Some((cmp, left, right)) =
            compared(self.law.expect("only law steps produce comparisons"), self.values, cond)
        else {
            return;
        };
        let (counted, limit) = if matches!(cmp, BinOp::Lt | BinOp::Le) { (left, right) } else { (right, left) };
        self.out.push(Outcome::Read { step, counted, limit });
    }

    /// A `require … else owe …` that failed: the violation is priced.
    fn price(&mut self, step: u32, effect: &LawEffect) {
        let before = self.out.len();
        self.effect(step, effect);
        if let Some(&Outcome::Owe { name, amount, owed }) = self.out.get(before) {
            self.out[before] = Outcome::Priced { step, name, amount, owed };
        }
    }

    fn effect(&mut self, step: u32, effect: &LawEffect) {
        match *effect {
            LawEffect::Count { amount, name } => {
                let Some(amount) = self.nonzero_amount(step, amount) else {
                    return;
                };
                match self.calc().convert(amount, self.book().base) {
                    Ok(base) => self.out.push(Outcome::Count { name, amount: base.qty }),
                    Err(fault) => self.out.push(Outcome::Faulted { step, fault }),
                }
            }
            LawEffect::Owe { amount, to, due, name } => {
                let Some(amount) = self.nonzero_amount(step, amount) else {
                    return;
                };
                let due = match due.map(|node| self.scan(node)) {
                    None => self.ctx.day,
                    Some(Value::Day(day)) => day,
                    Some(Value::Fault(fault)) => {
                        return self.out.push(Outcome::Faulted { step, fault });
                    }
                    Some(_) => unreachable!("{TYPED}"),
                };
                self.out.push(Outcome::Owe { name, amount, owed: Owed { to, due } });
            }
            LawEffect::Consume { amount } => {
                if let Some(amount) = self.nonzero_amount(step, amount) {
                    self.out.push(Outcome::Consume { step, amount });
                }
            }
            LawEffect::Carry { amount, unit, within } => {
                let Some(amount) = self.nonzero_amount(step, amount) else { return };
                let unit = match self.scan(unit) {
                    Value::Unit(unit) => unit,
                    Value::Fault(fault) => return self.out.push(Outcome::Faulted { step, fault }),
                    _ => return self.out.push(Outcome::Faulted { step, fault: Fault::InvalidProgram }),
                };
                let within = match self.scan(within) {
                    Value::Span(within) => within,
                    Value::Fault(fault) => return self.out.push(Outcome::Faulted { step, fault }),
                    _ => return self.out.push(Outcome::Faulted { step, fault: Fault::InvalidProgram }),
                };
                self.out.push(Outcome::Carry { step, amount, unit, within });
            }
        }
    }

    /// The amount an effect carries; `None` for nothing to record (`empty`, or
    /// zero) or, after reporting the fault, for one that could not be computed.
    fn nonzero_amount(&mut self, step: u32, root: NodeId) -> Option<Amount> {
        match self.scan(root) {
            Value::Amount(amount) if !amount.qty.is_zero() => Some(amount),
            Value::Amount(_) | Value::Empty => None,
            Value::Fault(fault) => {
                self.out.push(Outcome::Faulted { step, fault });
                None
            }
            _ => unreachable!("{TYPED}"),
        }
    }

    /// A tally as it stands, in the year asked (a number, or a date in it) or
    /// this one: what the world holds, plus, for this year, what earlier steps
    /// of this same law have counted, so `count amount as x` then `require
    /// tally(x) <= …` sees the flow it is checking.
    fn tally(&self, name: Sym, asked: Option<Value>) -> Value {
        let (ctx, tallies) = (self.ctx, &self.env.world.tallies);
        let this_year = ctx.over.first().year();
        let year = match asked {
            None => this_year,
            Some(Value::Num(year)) => year.round() as i32,
            Some(Value::Day(day)) => day.year(),
            Some(Value::Fault(fault)) => return Value::Fault(fault),
            Some(_) => unreachable!("{TYPED}"),
        };
        let counted = self.out.iter().filter_map(|o| match *o {
            Outcome::Count { name: counted, amount } if counted == name && year == this_year => {
                Some(spread(amount, ctx.over, Window::Year.around(ctx.over.first())))
            }
            _ => None,
        });
        self.base(tallies.read(ctx.owner, year, name) + counted.sum())
    }

    fn node(&mut self, at: usize) -> Value {
        match &self.nodes[NodeId(at as u32)].op {
            Op::Const(value) => *value,
            Op::Var(var) => self.var(*var),
            Op::Local(bound) => self.at(*bound),
            Op::Field(base, field) => self.field(self.at(*base), *field),
            Op::Param(param, keys) => self.param(*param, keys),
            Op::Call(func, args) => self.call(NodeId(at as u32), *func, args),
            Op::Neg(x) => match self.at(*x) {
                Value::Amount(a) => Value::Amount(Amount::new(-a.qty, a.unit)),
                Value::Num(n) => Value::Num(-n),
                other => other,
            },
            Op::Not(x) => match self.at(*x) {
                Value::Bool(b) => Value::Bool(!b),
                other => other,
            },
            Op::Bin(op, l, r) => self.calc().binary_typed(
                *op,
                self.at(*l),
                self.at(*r),
                self.nodes[*l].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[*r].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[NodeId(at as u32)].typed_ty().unwrap_or(Ty::Empty),
            ),
            Op::At(quantity, price) => self.calc().binary_typed(
                BinOp::Mul,
                self.at(*quantity),
                self.at(*price),
                self.nodes[*quantity].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[*price].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[NodeId(at as u32)].typed_ty().unwrap_or(Ty::Empty),
            ),
            Op::Of(purpose, object) => match (self.at(*purpose), self.at(*object)) {
                (Value::Fault(fault), _) | (_, Value::Fault(fault)) => Value::Fault(fault),
                (Value::Purpose(purpose, _), Value::Asset(asset)) => {
                    Value::Purpose(purpose, Some(Object::Asset(asset)))
                }
                (Value::Purpose(purpose, _), Value::Place(place)) => {
                    Value::Purpose(purpose, Some(Object::Place(place)))
                }
                (Value::Purpose(purpose, _), Value::Entity(entity)) => {
                    Value::Purpose(purpose, Some(Object::Entity(entity)))
                }
                _ => Value::Fault(Fault::InvalidProgram),
            },
            Op::Select(keys) => self.select(keys),
            Op::Is(x, alternatives) => match self.at(*x) {
                fault @ Value::Fault(_) => fault,
                left => Value::Bool(alternatives.iter().any(|&alt| self.matches(left, self.at(alt)))),
            },
            Op::Resides(entity, systems) => match self.at(*entity) {
                Value::Fault(fault) => Value::Fault(fault),
                Value::Entity(entity) => Value::Bool(
                    self.book().entities[entity]
                        .lives
                        .iter()
                        .any(|residence| residence.days.contains(self.ctx.day) && systems.contains(&residence.system)),
                ),
                _ => Value::Fault(Fault::InvalidProgram),
            },
            Op::If(cond, then, otherwise) => match self.at(*cond) {
                Value::Bool(true) => self.at(*then),
                Value::Bool(false) => self.at(*otherwise),
                other => other,
            },
        }
    }

    /// The trigger's variables. One the trigger does not supply reads as
    /// `empty`; the model refuses laws that ask.
    fn var(&self, var: Var) -> Value {
        let ctx = self.ctx;
        let flow = |pick: fn(&Motion) -> Value| ctx.motion.map_or(Value::Empty, pick);
        let realized = |pick: fn(&Realized) -> Qty| ctx.realized.map_or(Value::Empty, |r| self.base(pick(&r)));
        match var {
            Var::Amount => ctx.amount.map_or(Value::Empty, Value::Amount),
            Var::From => flow(|m| Value::Place(m.from)),
            Var::To => flow(|m| Value::Place(m.to)),
            Var::Payee => flow(|m| m.payee.map_or(Value::Empty, Value::Entity)),
            Var::Date => Value::Day(ctx.day),
            Var::Year => Value::Num(Ratio::int(ctx.over.first().year() as i64)),
            Var::Month => Value::Num(Ratio::int(ctx.over.first().ymd().1 as i64)),
            Var::Subject if ctx.flow_subject => Value::Flow,
            Var::Subject => match ctx.subject {
                Subject::Place(place) => Value::Place(place),
                Subject::Entity(entity) => Value::Entity(entity),
                Subject::Asset(asset) => Value::Asset(asset),
                Subject::Contract(_) => Value::Flow,
            },
            Var::Owner => Value::Entity(ctx.owner),
            Var::Gain => realized(|r| r.gain),
            Var::Proceeds => realized(|r| r.proceeds),
            Var::Basis => realized(|r| r.basis),
            Var::Held => ctx.realized.map_or(Value::Empty, |r| Value::Span(r.held)),
            Var::Balance => self.balance(ctx.subject),
            Var::Remaining => self.remaining(),
            Var::Flow => Value::Flow,
            // v3 bridge: no v3 trigger says what a flow is for.
            Var::Purpose => ctx.purpose.map_or(Value::Empty, |purpose| Value::Purpose(purpose.purpose, purpose.of)),
            Var::Description => ctx.description.map_or(Value::Empty, Value::Text),
            Var::Input(index) => match ctx.inputs.and_then(|inputs| inputs.get(index as usize)).copied().flatten() {
                Some(amount) => Value::Amount(amount),
                None => Value::Fault(Fault::MissingInput(index)),
            },
        }
    }

    fn field(&self, base: Value, field: Field) -> Value {
        let book = self.book();
        if let Value::Fault(_) = base {
            return base;
        }
        match (field, base) {
            (Field::Balance, Value::Place(place)) => self.balance(Subject::Place(place)),
            (Field::Balance, Value::Entity(entity)) => self.balance(Subject::Entity(entity)),
            (Field::Basis, Value::Place(place)) => self.basis(Subject::Place(place)),
            (Field::Basis, Value::Entity(entity)) => self.basis(Subject::Entity(entity)),
            (Field::Basis, Value::Asset(asset)) => self.asset_basis(asset),
            (Field::Cost, Value::Asset(asset)) => self.asset_cost(asset),
            (Field::InService, Value::Asset(asset)) => self.asset_in_service(asset),
            (Field::Parts, Value::Asset(asset)) => {
                let count = self.env.world.assets.asset(asset).map_or(0, |state| state.part_count());
                i64::try_from(count).map_or(Value::Fault(Fault::Overflow), |count| Value::Num(Ratio::int(count)))
            }
            (Field::Unit, Value::Amount(amount)) => Value::Unit(amount.unit),
            (Field::Unit, Value::Empty) => Value::Unit(book.base),
            (Field::Owner, Value::Place(place)) => Value::Entity(book.places[place].owner),
            (Field::Owner, Value::Entity(entity)) => Value::Entity(entity),
            (Field::Kind, Value::Place(place)) => Value::Kind(book.places[place].kind),
            (Field::Kind, Value::Entity(entity)) => Value::Kind(book.entities[entity].kind),
            (Field::Kind, Value::Unit(unit)) => Value::Kind(book.commodities[unit].kind),
            (Field::Age, Value::Entity(entity)) => self.age(entity),
            (Field::Of, Value::Purpose(_, object)) => object.map_or(Value::Empty, object_value),
            (Field::Year, Value::Day(day)) => Value::Num(Ratio::int(day.year() as i64)),
            (Field::Month, Value::Day(day)) => Value::Num(Ratio::int(day.ymd().1 as i64)),
            (Field::Prop(name), base) => self.prop(base, name),
            _ => unreachable!("{TYPED}"),
        }
    }

    /// Sums the selected sides of the current template occurrence without
    /// materializing strings or cloning flow metadata. Side-specific keys
    /// (`end`, `unit`) must agree on the same side; neutral keys select `out`.
    fn select(&self, keys: &[SelectKey]) -> Value {
        let Some(occurrence) = self.ctx.template_flows else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let book = self.book();
        let mut total = None::<Amount>;
        for runtime in occurrence.flows {
            let view = book.runtime_flow_view(runtime, occurrence.details);
            let mut from_matches = true;
            let mut to_matches = true;
            let mut has_side_key = false;
            let mut other_matches = true;
            for key in keys {
                match *key {
                    SelectKey::End(end) => {
                        has_side_key = true;
                        from_matches &= view.from == end;
                        to_matches &= view.to == end;
                    }
                    SelectKey::Unit(unit) => {
                        has_side_key = true;
                        from_matches &= view.out.unit == unit;
                        to_matches &= view.arrive.unit == unit;
                    }
                    SelectKey::Purpose(wanted) => {
                        other_matches &=
                            view.purpose.is_some_and(|actual| book.purposes.covers(wanted, actual.purpose));
                    }
                    SelectKey::Code(code) => {
                        other_matches &= view.codes().any(|candidate| candidate == code);
                    }
                    SelectKey::Range(days) => {
                        other_matches &= view.recognized.intersect(days).is_some();
                    }
                }
            }
            if !other_matches || (has_side_key && !from_matches && !to_matches) {
                continue;
            }
            let amount = if has_side_key && !from_matches { view.arrive } else { view.out };
            total = Some(match total {
                None => amount,
                Some(sum) if sum.unit == amount.unit => {
                    let Some(qty) = sum.qty.0.checked_add(amount.qty.0) else {
                        return Value::Fault(Fault::Overflow);
                    };
                    Amount::new(Qty(qty), sum.unit)
                }
                Some(sum) => return Value::Fault(Fault::UnitMismatch { found: amount.unit, expected: sum.unit }),
            });
        }
        total.map_or(Value::Empty, Value::Amount)
    }

    /// From the entity's `born` date to the day of evaluation.
    fn age(&self, entity: Id<Entity>) -> Value {
        let born = self.env.plan.known.born.expect("a law that reads `.age` makes the model intern `born`");
        match self.book().own(entity, born, self.ctx.day) {
            Some(Value::Day(day)) => Value::Span(self.ctx.day.since(day)),
            _ => Value::Fault(Fault::Unset(born)),
        }
    }

    fn asset_cost(&self, asset: Id<Asset>) -> Value {
        let Some(state) = self.env.world.assets.asset(asset) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let cost = if let Some(part) = self.ctx.asset_part {
            let Some((owner, record)) = self.env.world.assets.part(part) else {
                return Value::Fault(Fault::InvalidProgram);
            };
            if owner != asset {
                return Value::Fault(Fault::InvalidProgram);
            }
            record.cost
        } else {
            match state.total_cost() {
                Ok(cost) => cost,
                Err(_) => return Value::Fault(Fault::Overflow),
            }
        };
        self.base(cost)
    }

    fn asset_basis(&self, asset: Id<Asset>) -> Value {
        let Some(state) = self.env.world.assets.asset(asset) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let basis = if let Some(part) = self.ctx.asset_part {
            let Some((owner, record)) = self.env.world.assets.part(part) else {
                return Value::Fault(Fault::InvalidProgram);
            };
            if owner != asset {
                return Value::Fault(Fault::InvalidProgram);
            }
            record.basis
        } else {
            match state.total_basis() {
                Ok(basis) => basis,
                Err(_) => return Value::Fault(Fault::Overflow),
            }
        };
        self.base(basis)
    }

    fn asset_in_service(&self, asset: Id<Asset>) -> Value {
        let Some(state) = self.env.world.assets.asset(asset) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let Some(part) = self.ctx.asset_part.or_else(|| state.parts().first().map(|part| part.id)) else {
            return Value::Empty;
        };
        if self.env.world.assets.part(part).is_none_or(|(owner, _)| owner != asset) {
            return Value::Fault(Fault::InvalidProgram);
        }
        let property =
            self.book().names.get("in-service").map_or(Value::Empty, |name| self.prop(Value::Asset(asset), name));
        let property = match property {
            Value::Day(day) => Some(day),
            Value::Fault(fault) => return Value::Fault(fault),
            Value::Empty => None,
            _ => return Value::Fault(Fault::InvalidProgram),
        };
        state.in_service(part, property).map_or(Value::Fault(Fault::InvalidProgram), Value::Day)
    }

    /// A declared property: the thing's own, else its kind's default.
    fn prop(&self, base: Value, name: Sym) -> Value {
        let book = self.book();
        let (own, kind, asset_property_applies) = match base {
            Value::Place(place) => (Some(Holder::Place(place)), book.places[place].kind, true),
            Value::Entity(entity) => (Some(Holder::Entity(entity)), book.entities[entity].kind, true),
            Value::Unit(unit) => (Some(Holder::Commodity(unit)), book.commodities[unit].kind, true),
            Value::Kind(kind) => (None, kind, true),
            Value::Asset(asset) => {
                let applies = if let Some(part) = self.ctx.asset_part {
                    let Some((owner, _)) = self.env.world.assets.part(part) else {
                        return Value::Fault(Fault::InvalidProgram);
                    };
                    if owner != asset {
                        return Value::Fault(Fault::InvalidProgram);
                    }
                    self.env
                        .world
                        .assets
                        .asset(asset)
                        .and_then(|state| state.property_applies(part, false).ok())
                        .unwrap_or(false)
                } else {
                    true
                };
                (Some(Holder::Asset(asset)), book.assets[asset].kind, applies)
            }
            _ => unreachable!("{TYPED}"),
        };
        let explicit = own.and_then(|thing| book.own(thing, name, self.ctx.day));
        match explicit.filter(|_| asset_property_applies).or_else(|| book.by_kind(kind, name, self.ctx.day)) {
            Some(value) => value,
            None if explicit.is_some() => Value::Empty,
            None => Value::Fault(Fault::Unset(name)),
        }
    }

    /// `limit[year]`: among rows whose name keys equal the lookup's, the latest
    /// that starts on or before the day asked. A number key asks for the first
    /// of that year, a date key for that day; with neither, the context day.
    fn param(&self, param: Id<Param>, keys: &[NodeId]) -> Value {
        let mut when = self.ctx.anchor();
        let mut name_count = 0usize;
        for (index, &key) in keys.iter().enumerate() {
            match self.at(key) {
                Value::Fault(fault) => return Value::Fault(fault),
                Value::Num(year) if index == 0 => match Day::from_ymd(year.round() as i32, 1, 1) {
                    Some(start) => when = start,
                    None => return Value::Fault(Fault::Overflow),
                },
                Value::Day(day) if index == 0 => when = day,
                value if self.key_name(value).is_some() => name_count += 1,
                _ => return Value::Fault(Fault::NoRow(param)),
            }
        }
        if keys.iter().enumerate().any(|(index, &key)| {
            !(index == 0 && matches!(self.at(key), Value::Num(_) | Value::Day(_)))
                && self.key_name(self.at(key)).is_none()
        }) {
            return Value::Fault(Fault::NoRow(param));
        }
        self.book().params[param]
            .row_index_by(when, |row_names| {
                if row_names.len() != name_count {
                    return row_names.len().cmp(&name_count);
                }
                let mut names = keys.iter().enumerate().filter_map(|(index, &key)| {
                    if index == 0 && matches!(self.at(key), Value::Num(_) | Value::Day(_)) {
                        None
                    } else {
                        self.key_name(self.at(key))
                    }
                });
                for (row_name, requested) in row_names.iter().zip(&mut names) {
                    match row_name.cmp(&requested) {
                        std::cmp::Ordering::Equal => {}
                        different => return different,
                    }
                }
                std::cmp::Ordering::Equal
            })
            .map_or(Value::Fault(Fault::NoRow(param)), |(_, row)| row.value)
    }

    fn key_name(&self, key: Value) -> Option<Sym> {
        let book = self.book();
        match key {
            Value::Name(sym) | Value::Code(sym) => Some(sym),
            Value::Text(Text::Borrowed(sym)) => Some(sym),
            Value::Unit(unit) => Some(book.commodities[unit].symbol),
            Value::Kind(kind) => Some(book.kinds[kind].name),
            _ => None,
        }
    }

    /// Whether `left` satisfies one alternative of an `is` test.
    fn matches(&self, left: Value, alternative: Value) -> bool {
        let book = self.book();
        let named = |pattern: Sym, path: Sym| glob(book.name(pattern), book.name(path));
        match (left, alternative) {
            (Value::Place(p), Value::Kind(k)) => match book.places[p].role {
                axiom_model::Role::Outside(Some(party))
                | axiom_model::Role::Tab(party)
                | axiom_model::Role::Holding(party) => book.is_a(book.entities[party].kind, k),
                axiom_model::Role::Issuer(unit) => book.is_a(book.commodities[unit].kind, k),
                axiom_model::Role::Outside(None) | axiom_model::Role::Account { .. } | axiom_model::Role::Asset(_) => {
                    book.is_a(book.places[p].kind, k)
                }
            },
            (Value::Entity(e), Value::Kind(k)) => book.is_a(book.entities[e].kind, k),
            (Value::Unit(u), Value::Kind(k)) => book.is_a(book.commodities[u].kind, k),
            (Value::Kind(a), Value::Kind(k)) => book.is_a(a, k),
            (Value::Purpose(actual, actual_of), Value::Purpose(wanted, wanted_of)) => {
                book.purposes.covers(wanted, actual)
                    && wanted_of
                        .is_none_or(|wanted| actual_of.is_some_and(|actual| object_matches(book, actual, wanted)))
            }
            (Value::Place(p), Value::Place(root)) => book.places.covers(root, p),
            (Value::Place(place), Value::Entity(root)) => {
                let endpoint = match book.places[place].role {
                    axiom_model::Role::Outside(Some(party))
                    | axiom_model::Role::Tab(party)
                    | axiom_model::Role::Holding(party) => Some(party),
                    axiom_model::Role::Outside(None) | axiom_model::Role::Issuer(_) => None,
                    axiom_model::Role::Account { .. } | axiom_model::Role::Asset(_) => Some(book.places[place].owner),
                };
                endpoint.is_some_and(|entity| book.entities.covers(root, entity))
            }
            (Value::Entity(e), Value::Entity(root)) => book.entities.covers(root, e),
            (Value::Unit(a), Value::Unit(b)) => a == b,
            (Value::Place(p), Value::Glob(pattern)) => named(pattern, book.places[p].path),
            (Value::Entity(e), Value::Glob(pattern)) => named(pattern, book.entities[e].path),
            (Value::Unit(u), Value::Glob(pattern)) => named(pattern, book.commodities[u].symbol),
            (Value::Flow, Value::Code(code)) => {
                self.ctx.motion.is_some_and(|m| m.codes().any(|mark| named(code, mark)))
            }
            _ => false,
        }
    }

    fn call(&mut self, at: NodeId, func: Func, args: &[NodeId]) -> Value {
        let arg = |i: usize| self.at(args[i]);
        // `total` and `tally` take their operands from the function itself.
        let operands =
            !matches!(func, Func::Total(..) | Func::PurposeTotal { .. } | Func::BudgetTotal(_) | Func::Tally(_));
        if operands && let Some(fault) = args.iter().map(|&a| self.at(a)).find(|v| matches!(v, Value::Fault(_))) {
            return fault;
        }
        match func {
            Func::Total(dir, window) => self.total(dir, window, args),
            Func::PurposeTotal { purpose, window } => self.purpose_total(purpose, window),
            Func::BudgetTotal(budget) => self.budget_total(budget),
            Func::BudgetLimit(budget) => self.budget_limit(budget, at),
            Func::Tally(name) => self.tally(name, Func::tally_year(args).map(|year| self.at(year))),
            Func::Min => self.pick(BinOp::Le, arg(0), arg(1)),
            Func::Max => self.pick(BinOp::Ge, arg(0), arg(1)),
            Func::Abs => match arg(0) {
                Value::Amount(a) => Value::Amount(Amount::new(a.qty.abs(), a.unit)),
                Value::Num(n) => Value::Num(n.abs()),
                other => other,
            },
            Func::Progressive => self.progressive(arg(0), arg(1)),
            Func::Value => match (arg(0), arg(1)) {
                (Value::Amount(a), Value::Unit(unit)) => {
                    self.calc().convert(a, unit).map_or_else(Value::Fault, Value::Amount)
                }
                (Value::Empty, Value::Unit(unit)) => Value::Amount(Amount::zero(unit)),
                _ => unreachable!("{TYPED}"),
            },
            Func::Date => civil_date(arg(0), arg(1), arg(2)).map_or_else(Value::Fault, Value::Day),
            Func::StraightLine => self.straight_line(args),
            Func::Open(code) => self.open(code),
            Func::Peak | Func::Low | Func::Days => self.temporal(at, func, args),
        }
    }

    fn temporal(&self, call: NodeId, func: Func, args: &[NodeId]) -> Value {
        let Some(law) = self.law_id else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let (Some(&root), Some(&window_arg)) = (args.first(), args.get(1)) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let mut days = match self.at(window_arg) {
            Value::Name(name) => match self.book().name(name) {
                "month" => Window::Month.around(self.ctx.anchor()),
                "year" => Window::Year.around(self.ctx.anchor()),
                "ever" => Days::ALWAYS,
                _ => return Value::Fault(Fault::InvalidProgram),
            },
            _ => return Value::Fault(Fault::InvalidProgram),
        };
        let key =
            TemporalKey { law, subject: self.ctx.subject, owner: self.ctx.owner, call, part: self.ctx.asset_part };
        let samples = self.env.world.temporal.get(key);
        if matches!(self.at(window_arg), Value::Name(name) if self.book().name(name) == "ever") {
            days = samples
                .first()
                .and_then(|sample| Days::new(sample.day, days.last()))
                .unwrap_or_else(|| Days::on(self.ctx.anchor()));
        }
        let current = self.at(root);
        match func {
            Func::Peak | Func::Low => self.extreme(func, days, samples, current),
            Func::Days => self.day_count(days, samples, self.ctx.day, current),
            _ => Value::Fault(Fault::InvalidProgram),
        }
    }

    fn extreme(&self, func: Func, days: Days, samples: &[crate::temporal::Sample], current: Value) -> Value {
        let peak = func == Func::Peak;
        let mut best = samples
            .iter()
            .take_while(|sample| sample.day < days.first())
            .last()
            .map(|sample| sample.value)
            .filter(|value| *value != Value::Empty);
        if let Some(Value::Fault(fault)) = best {
            return Value::Fault(fault);
        }
        for sample in samples.iter().filter(|sample| days.contains(sample.day)) {
            if let Value::Fault(fault) = sample.value {
                return Value::Fault(fault);
            }
            if sample.value == Value::Empty {
                continue;
            }
            best = match best {
                None => Some(sample.value),
                Some(previous) => {
                    match self.calc().binary(if peak { BinOp::Ge } else { BinOp::Le }, sample.value, previous) {
                        Value::Bool(true) => Some(sample.value),
                        Value::Bool(false) => Some(previous),
                        Value::Fault(fault) => return Value::Fault(fault),
                        _ => return Value::Fault(Fault::InvalidProgram),
                    }
                }
            };
        }
        if days.contains(self.ctx.day) {
            if let Value::Fault(fault) = current {
                return Value::Fault(fault);
            }
            if current != Value::Empty {
                best = match best {
                    None => Some(current),
                    Some(previous) => {
                        match self.calc().binary(if peak { BinOp::Ge } else { BinOp::Le }, current, previous) {
                            Value::Bool(true) => Some(current),
                            Value::Bool(false) => Some(previous),
                            Value::Fault(fault) => return Value::Fault(fault),
                            _ => return Value::Fault(Fault::InvalidProgram),
                        }
                    }
                };
            }
        }
        best.unwrap_or(Value::Empty)
    }

    fn day_count(&self, days: Days, samples: &[crate::temporal::Sample], current_day: Day, current: Value) -> Value {
        let mut cursor = days.first();
        let mut state = samples
            .iter()
            .take_while(|sample| sample.day < days.first())
            .last()
            .map_or(Value::Bool(false), |sample| sample.value);
        let mut total = 0i64;
        let mut at = samples.partition_point(|sample| sample.day < days.first());
        while at < samples.len() && samples[at].day <= days.last() {
            let day = samples[at].day;
            let mut after = at + 1;
            while after < samples.len() && samples[after].day == day {
                after += 1;
            }
            if day > cursor {
                if let Value::Fault(fault) = state {
                    return Value::Fault(fault);
                }
                if state == Value::Bool(true) {
                    total += i64::from(day.0) - i64::from(cursor.0);
                } else if !matches!(state, Value::Bool(false) | Value::Empty) {
                    return Value::Fault(Fault::InvalidProgram);
                }
            }
            state = samples[after - 1].value;
            cursor = day;
            at = after;
        }
        if days.contains(current_day) {
            if current_day > cursor {
                if let Value::Fault(fault) = state {
                    return Value::Fault(fault);
                }
                if state == Value::Bool(true) {
                    total += i64::from(current_day.0) - i64::from(cursor.0);
                } else if !matches!(state, Value::Bool(false) | Value::Empty) {
                    return Value::Fault(Fault::InvalidProgram);
                }
            }
            state = current;
            cursor = current_day;
        }
        if let Value::Fault(fault) = state {
            return Value::Fault(fault);
        }
        if state == Value::Bool(true) {
            total += i64::from(days.last().0) - i64::from(cursor.0) + 1;
        } else if !matches!(state, Value::Bool(false) | Value::Empty) {
            return Value::Fault(Fault::InvalidProgram);
        }
        Value::Num(Ratio::int(total))
    }

    fn purpose_total(&self, purpose: Option<Id<axiom_model::Purpose>>, window: Window) -> Value {
        let purpose = purpose
            .or(self.ctx.governing_purpose)
            .or_else(|| {
                self.law.and_then(|law| match law.owner {
                    axiom_model::Owner::Purpose(purpose) => Some(purpose),
                    _ => None,
                })
            })
            .expect("a purpose total without an explicit purpose requires a purpose context");
        let root = self.book().purposes[purpose].root;
        let (incoming, outgoing) = if self.ctx.budget_history {
            let span = window.around(self.ctx.anchor());
            match self.env.world.totals.read_purpose_between(self.ctx.owner, purpose, span) {
                Ok(flowed) => flowed,
                Err(fault) => return Value::Fault(fault),
            }
        } else {
            self.env.world.totals.read_purpose(self.ctx.owner, purpose, window, self.ctx.anchor())
        };
        let total = match purpose_net(root, incoming, outgoing) {
            Ok(total) => total,
            Err(fault) => return Value::Fault(fault),
        };
        self.base(total)
    }

    /// The purpose movement allowed by a native budget. A carrying budget
    /// retains only the sparse history of purposes referenced by budgets and
    /// sums it from the declaration's effective start.
    fn budget_total(&self, id: Id<axiom_model::Budget>) -> Value {
        let Some(budget) = self.book().budgets.get(id) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let anchor = self.ctx.anchor();
        let start = self.budget_start(budget);
        if anchor < start {
            return self.base(Qty::ZERO);
        }
        let terms = budget.terms.at(anchor);
        let first = if terms.carries {
            budget_carry_start(budget, start, anchor).unwrap_or(start)
        } else {
            budget_segment_start(budget, start, anchor)
        };
        let active_start = budget_segment_start(budget, start, anchor);
        let active_end = budget_segment_end(budget, active_start, budget_window(terms.period).around(anchor).last());
        let Some(span) = Days::new(first, active_end) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let (incoming, outgoing) =
            match self.env.world.totals.read_purpose_between(self.ctx.owner, budget.purpose, span) {
                Ok(flowed) => flowed,
                Err(fault) => return Value::Fault(fault),
            };
        let total = match purpose_net(self.book().purposes[budget.purpose].root, incoming, outgoing) {
            Ok(total) => total,
            Err(fault) => return Value::Fault(fault),
        };
        self.base(total)
    }

    /// The allowance in force for a generated native budget law. Computed
    /// limits are roots earlier in that law's shared node arena; share limits
    /// read the declared purpose total for the budget's own period.
    fn budget_limit(&mut self, id: Id<axiom_model::Budget>, at: NodeId) -> Value {
        let Some(budget) = self.book().budgets.get(id) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let Some(_law) = self.law.filter(|law| law.budget == Some(id)) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let day = self.ctx.anchor();
        let start = self.budget_start(budget);
        if day < start {
            return self.base(Qty::ZERO);
        }
        let terms = *budget.terms.at(day);
        if !terms.carries {
            let first = budget_segment_start(budget, start, day);
            let last = budget_segment_end(budget, first, budget_window(terms.period).around(day).last());
            let span = Days::new(first, last);
            let Some(span) = span else {
                return Value::Fault(Fault::InvalidProgram);
            };
            return self.one_budget_limit(id, terms.limit, span, at, day);
        }

        let mut cursor = budget_carry_start(budget, start, day).unwrap_or(start);
        let mut total = Qty::ZERO;
        while cursor <= day {
            let period_end = budget_window(budget.terms.at(cursor).period).around(cursor).last();
            let end = budget_segment_end(budget, cursor, period_end);
            let Some(span) = Days::new(cursor, end) else {
                return Value::Fault(Fault::InvalidProgram);
            };
            // A limit restatement replaces the allowance of this open segment.
            let on = end.min(day);
            let effective = *budget.terms.at(on);
            let value = self.one_budget_limit(id, effective.limit, span, at, on);
            let Value::Amount(amount) = value else {
                return value;
            };
            let calc = Calc { book: self.book(), day: on };
            match calc.convert(amount, self.book().base) {
                Ok(amount) => {
                    let Some(sum) = total.0.checked_add(amount.qty.0) else {
                        return Value::Fault(Fault::Overflow);
                    };
                    total = Qty(sum);
                }
                Err(fault) => return Value::Fault(fault),
            }
            if end == Day::MAX {
                break;
            }
            cursor = end.add_days(1);
        }
        self.base(total)
    }

    fn one_budget_limit(
        &mut self,
        id: Id<axiom_model::Budget>,
        limit: axiom_model::Limit,
        span: Days,
        at: NodeId,
        on: Day,
    ) -> Value {
        match limit {
            axiom_model::Limit::Amount(amount) => Value::Amount(amount),
            axiom_model::Limit::Share { rate, of } => {
                let (incoming, outgoing) = match self.env.world.totals.read_purpose_between(self.ctx.owner, of, span) {
                    Ok(flowed) => flowed,
                    Err(fault) => return Value::Fault(fault),
                };
                let root = self.book().purposes[of].root;
                let total = match purpose_net(root, incoming, outgoing) {
                    Ok(total) => total,
                    Err(fault) => return Value::Fault(fault),
                };
                total
                    .scale(rate)
                    .map_or(Value::Fault(Fault::Overflow), |qty| Value::Amount(Amount::new(qty, self.book().base)))
            }
            axiom_model::Limit::Computed(root) => {
                let Some(law) = self.law.filter(|law| law.budget == Some(id)) else {
                    return Value::Fault(Fault::InvalidProgram);
                };
                if root >= at || root.index() >= law.nodes.len() {
                    return Value::Fault(Fault::InvalidProgram);
                }
                let occasion = Occasion::time(on, Days::on(on));
                let mut context = Context::new(self.ctx.subject, self.ctx.owner, &occasion);
                context.governing_purpose = self.ctx.governing_purpose;
                context.inputs = self.ctx.inputs;
                context.budget_history = true;
                let value = expression(
                    Env { plan: self.env.plan, world: self.env.world },
                    law,
                    root,
                    &context,
                    self.budget_values,
                );
                match value {
                    value @ (Value::Amount(_) | Value::Fault(_)) => value,
                    _ => Value::Fault(Fault::InvalidProgram),
                }
            }
        }
    }

    fn straight_line(&self, args: &[NodeId]) -> Value {
        let (Value::Amount(cost), Value::Span(life), Value::Day(from), Value::Name(period)) =
            (self.at(args[0]), self.at(args[1]), self.at(args[2]), self.at(args[3]))
        else {
            unreachable!("{TYPED}");
        };
        let window = match self.book().name(period) {
            "month" => Window::Month,
            "year" => Window::Year,
            _ => unreachable!("{TYPED}"),
        };
        let mid_month = args
            .get(4)
            .is_some_and(|&node| matches!(self.at(node), Value::Name(name) if self.book().name(name) == "mid-month"));
        crate::calc::straight_line_with_terminal(
            cost.qty,
            life,
            from,
            self.ctx.span,
            window,
            mid_month,
            self.ctx.partial_terminal,
        )
        .map_or(Value::Fault(Fault::Overflow), |qty| Value::Amount(Amount::new(qty, cost.unit)))
    }

    fn open(&self, code: Sym) -> Value {
        let book = self.book();
        let unit = book.entities[self.ctx.owner].currency;
        let mut total = Qty::ZERO;
        for (place, _) in book.places.iter() {
            if !self.env.plan.traits.place(place).claim {
                continue;
            }
            for slot in self.env.world.holdings.of(place) {
                for lot in &slot.lots {
                    let matches = |run: axiom_core::Run<Sym>| book.codes[run].contains(&code);
                    if matches(lot.codes.header) || matches(lot.codes.local) {
                        // Claims are physical parcels, but `open(^code)` is
                        // read in one owner's currency. Allocate each parcel
                        // once using the same cent boundaries as reports.
                        for (owner, qty) in self.env.plan.allocate(place, lot.qty) {
                            if owner.owner != self.ctx.owner || qty.is_zero() {
                                continue;
                            }
                            match self.calc().convert(Amount::new(qty, slot.unit), unit) {
                                Ok(converted) => total += converted.qty,
                                Err(fault) => return Value::Fault(fault),
                            }
                        }
                    }
                }
            }
        }
        Value::Amount(Amount::new(total, unit))
    }

    /// `min` and `max`: whichever operand `op` (`<=` or `>=`) puts first.
    fn pick(&self, op: BinOp, a: Value, b: Value) -> Value {
        match self.calc().binary(op, a, b) {
            Value::Bool(true) => a,
            Value::Bool(false) => b,
            fault => fault,
        }
    }

    /// Flow through the subject in the current window, in the base currency. A
    /// kind argument widens it to every place of that kind the owner has.
    fn total(&self, dir: Dir, window: Window, args: &[NodeId]) -> Value {
        let (book, ctx, totals) = (self.book(), self.ctx, &self.env.world.totals);
        let read = |subject| -> Result<Qty, Fault> {
            if ctx.budget_history {
                totals.read_subject_between(subject, dir, window.around(ctx.anchor()))
            } else {
                Ok(totals.read(&self.env.plan.watch, subject, dir, window, ctx.anchor()))
            }
        };
        let widen = args.iter().find_map(|&a| if let Value::Kind(kind) = self.at(a) { Some(kind) } else { None });
        let sum = match widen {
            None => read(ctx.subject),
            Some(kind) => self
                .env
                .plan
                .kind_places
                .get(&kind)
                .into_iter()
                .flat_map(|places| places.iter())
                .filter(|&&place| book.places[place].owner == ctx.owner)
                .try_fold(Qty::ZERO, |sum, &place| {
                    let next = read(Subject::Place(place))?;
                    sum.0.checked_add(next.0).map(Qty).ok_or(Fault::Overflow)
                }),
        };
        sum.map_or_else(Value::Fault, |sum| self.base(sum))
    }

    fn progressive(&self, schedule: Value, income: Value) -> Value {
        let Value::Schedule(id) = schedule else { unreachable!("{TYPED}") };
        let schedule = &self.book().schedules[id];
        let income = match income {
            Value::Amount(a) => self.calc().convert(a, schedule.unit).map(|a| a.qty),
            _ => Ok(Qty::ZERO),
        };
        match income {
            Ok(income) => progressive(&schedule.brackets, income)
                .map_or(Value::Fault(Fault::Overflow), |tax| Value::Amount(Amount::new(tax, schedule.unit))),
            Err(fault) => Value::Fault(fault),
        }
    }

    fn held(&self, subject: Subject) -> impl Iterator<Item = &'a Slot> {
        held(self.env.plan, self.env.world, subject)
    }

    /// Everything the subject holds, valued in the base currency.
    fn balance(&self, subject: Subject) -> Value {
        let sign = sign(self.env.plan, subject);
        self.sum_in_base(self.held(subject).map(|slot| Amount::new(Qty(slot.qty.0 * sign), slot.unit)))
    }

    /// What everything the subject holds has already accounted for, in the base
    /// currency: the total basis of its parcels.
    fn basis(&self, subject: Subject) -> Value {
        let sign = sign(self.env.plan, subject);
        let basis: Qty =
            self.held(subject).map(|slot| slot.basis(is_money(self.env.plan, slot.place, slot.unit))).sum();
        self.base(Qty(basis.0 * sign))
    }

    /// Money still tied to the subject, a restricted entity.
    fn remaining(&self) -> Value {
        let Subject::Entity(entity) = self.ctx.subject else {
            return Value::Empty;
        };
        let tied = self.env.world.holdings.iter().flat_map(|h| {
            h.lots.iter().filter(move |lot| lot.tied == Some(entity)).map(move |lot| Amount::new(lot.qty, h.unit))
        });
        self.sum_in_base(tied)
    }

    fn sum_in_base(&self, amounts: impl Iterator<Item = Amount>) -> Value {
        let calc = self.calc();
        let mut total = Qty::ZERO;
        for amount in amounts {
            match calc.convert(amount, self.book().base) {
                Ok(base) => total += base.qty,
                Err(fault) => return Value::Fault(fault),
            }
        }
        self.base(total)
    }
}

fn object_value(object: Object) -> Value {
    match object {
        Object::Asset(asset) => Value::Asset(asset),
        Object::Place(place) => Value::Place(place),
        Object::Entity(entity) => Value::Entity(entity),
    }
}

fn object_matches(book: &Book, actual: Object, wanted: Object) -> bool {
    match (actual, wanted) {
        (Object::Place(actual), Object::Place(wanted)) => book.places.covers(wanted, actual),
        (Object::Entity(actual), Object::Entity(wanted)) => book.entities.covers(wanted, actual),
        (Object::Asset(actual), Object::Asset(wanted)) => {
            let mut at = Some(actual);
            while let Some(asset) = at {
                if asset == wanted {
                    return true;
                }
                at = book.assets[asset].part_of.map(|part| part.value);
            }
            false
        }
        _ => false,
    }
}

/// The holdings within the subject: a place's subtree, which is one stretch of
/// the holdings, or the asset places an entity holds.
pub(crate) fn held<'a>(plan: &'a Plan, world: &'a World, subject: Subject) -> impl Iterator<Item = &'a Slot> {
    let holdings: &'a Holdings = &world.holdings;
    let book = plan.book;
    let (subtree, places) = match subject {
        Subject::Place(root) => (root.index()..plan.book.places.end(root).index(), &[][..]),
        Subject::Entity(entity) => (0..0, plan.places_of(entity)),
        Subject::Asset(asset) => (0..0, plan.places_of_asset(asset)),
        Subject::Contract(contract) => (0..0, plan.places_of(book.contracts[contract].owner)),
    };
    let entity_places = places.iter().flat_map(move |&place| holdings.of(place));
    holdings.within(subtree).chain(entity_places)
}

/// The sign people read a subject's balance in: a credit card's balance is
/// what is owed, as an assertion writes it. An entity's is natural.
pub(crate) fn sign(plan: &Plan, subject: Subject) -> i64 {
    match subject {
        Subject::Place(place) => plan.sides.sign(place),
        Subject::Entity(_) => 1,
        Subject::Asset(_) | Subject::Contract(_) => 1,
    }
}

/// What an ordering comparison (`<`, `<=`, `>`, `>=`) compared, an `empty` side as zero. `None` for any other
/// condition, or one whose sides are not amounts.
pub(crate) fn compared(law: &Law, values: &[Value], cond: NodeId) -> Option<(BinOp, Amount, Amount)> {
    let Op::Bin(cmp @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), left, right) = law.nodes[cond].op else {
        return None;
    };
    match (values[left.index()], values[right.index()]) {
        (Value::Amount(l), Value::Amount(r)) => Some((cmp, l, r)),
        (Value::Amount(l), Value::Empty) => Some((cmp, l, Amount::zero(l.unit))),
        (Value::Empty, Value::Amount(r)) => Some((cmp, Amount::zero(r.unit), r)),
        _ => None,
    }
}

/// `date(y, m, d)`; a day past the month's end lands on its last day, as
/// recurrences do.
fn civil_date(year: Value, month: Value, day: Value) -> Result<Day, Fault> {
    let int = |v: Value| match v {
        Value::Num(n) if n.is_integer() => i32::try_from(n.num()).map_err(|_| Fault::Overflow),
        _ => Err(Fault::Overflow),
    };
    let (year, month, day) = (int(year)?, int(month)?, int(day)?);
    let month = u32::try_from(month).ok().filter(|m| (1..=12).contains(m)).ok_or(Fault::Overflow)?;
    let day = u32::try_from(day.max(1)).unwrap_or(1).min(days_in_month(year, month));
    Day::from_ymd(year, month, day).ok_or(Fault::Overflow)
}

fn purpose_net(root: axiom_model::PurposeRoot, incoming: Qty, outgoing: Qty) -> Result<Qty, Fault> {
    let (positive, negative) = match root {
        axiom_model::PurposeRoot::Income => (incoming, outgoing),
        axiom_model::PurposeRoot::Spending | axiom_model::PurposeRoot::Capital | axiom_model::PurposeRoot::Transfer => {
            (outgoing, incoming)
        }
    };
    positive.0.checked_sub(negative.0).map(Qty).ok_or(Fault::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{EventKey, Part, PartKind};

    #[test]
    fn property_reads_use_the_nearest_kind_default_and_expired_overrides_fall_through() {
        let text = "\
base USD
commodity USD
kind flagged-account : asset
  has marked bool
  marked true

  law mark
    on in
    require not self.marked \"the inherited kind default is active\"
kind inherited-account : flagged-account
kind overridden-account : flagged-account
  marked false
kind temporary-account : flagged-account
account checking
account inherited : inherited-account
account overridden : overridden-account
account temporary : temporary-account
2026-01-02 temporary now marked false until 2026-01-03
";
        let (file, parsed) = axiom_syntax::parse(axiom_core::FileId(0), text, axiom_syntax::Folder::default());
        assert!(parsed.is_empty(), "{parsed:?}");
        let (book, diagnostics) =
            axiom_model::build(&[axiom_model::Source { path: "axiom.ax", file, embedded: false }]);
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{diagnostics:?}");

        let plan = Plan::new(&book);
        let world = World::new(&book, &plan.watch);
        let law = book.laws.values().next().expect("the source kind declares one law");
        let nodes = &law.nodes;
        let mut values = Vec::new();
        let mut budget_values = Vec::new();
        let mut outcomes = Vec::new();
        let marked = book.names.get("marked").unwrap();
        let inherited = book.place("inherited").unwrap();
        let overridden = book.place("overridden").unwrap();

        for (place, date, expected) in [
            (inherited, (2026, 1, 1), Value::Bool(true)),
            (overridden, (2026, 1, 1), Value::Bool(false)),
            (overridden, (2026, 1, 4), Value::Bool(false)),
            (book.place("temporary").unwrap(), (2026, 1, 2), Value::Bool(false)),
            (book.place("temporary").unwrap(), (2026, 1, 4), Value::Bool(true)),
        ] {
            let day = Day::from_ymd(date.0, date.1, date.2).unwrap();
            let occasion = Occasion::time(day, Days::on(day));
            let context = Context::new(Subject::Place(place), book.places[place].owner, &occasion);
            let machine = Machine {
                env: Env { plan: &plan, world: &world },
                nodes,
                law: Some(law),
                law_id: None,
                ctx: &context,
                values: &mut values,
                budget_values: &mut budget_values,
                out: &mut outcomes,
            };
            assert_eq!(machine.prop(Value::Place(place), marked), expected, "{date:?}");
        }
    }

    #[test]
    fn selected_template_groups_filter_by_purpose_and_choose_the_matching_side() {
        let text = "\
base USD
kind currency : commodity
kind security : commodity
kind bank : asset
commodity USD : currency
  precision 2
commodity VTI : security
  precision 3
purpose wages : income
purpose rollover : spending
purpose fees : spending
account checking : bank
account ira : bank
account savings
entity employer
2026-01-01 checking 25_000 USD -> ira 7 VTI #rollover
  10 USD #fees
2026-01-02 checking 90_000 USD -> savings 3 VTI #wages
  5 USD #fees
";
        let (file, parsed) = axiom_syntax::parse(axiom_core::FileId(0), text, axiom_syntax::Folder::default());
        assert!(parsed.is_empty(), "{parsed:?}");
        let (mut book, diagnostics) =
            axiom_model::build(&[axiom_model::Source { path: "axiom.ax", file, embedded: false }]);
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{diagnostics:?}");
        let checking = book.place("checking").unwrap();
        let retirement = book.place("ira").unwrap();
        let savings = book.place("savings").unwrap();
        let owner = book.entities[book.roots.me].place.expect("me has an outside place");
        let owner = book.places[owner].owner;
        let wages = book.purpose("wages").unwrap();
        let retirement_purpose = book.purpose("rollover").unwrap();
        let usd = book.base;
        let vti = book.commodity("VTI").unwrap();
        let missing_code = book.names.intern("missing");

        let first_header = book.txns.get(Id::new(0)).unwrap().flows.start();
        let second_header = book.txns.get(Id::new(1)).unwrap().flows.start();
        let mut selected = book.flows.get(first_header).unwrap().clone();
        selected.from = checking;
        selected.to = retirement;
        selected.out = Amount::new(Qty(25_000), usd);
        selected.arrive = Amount::new(Qty(7), vti);
        selected.purpose =
            Some(Purposed { purpose: retirement_purpose, of: None, source: axiom_model::Provenance::Written });
        let mut unrelated = book.flows.get(second_header).unwrap().clone();
        unrelated.from = checking;
        unrelated.to = savings;
        unrelated.out = Amount::new(Qty(90_000), usd);
        unrelated.arrive = unrelated.out;
        unrelated.purpose = Some(Purposed { purpose: wages, of: None, source: axiom_model::Provenance::Written });
        let mut mixed = book.flows.get(second_header).unwrap().clone();
        mixed.from = checking;
        mixed.to = retirement;
        mixed.out = Amount::new(Qty(1_000), usd);
        mixed.arrive = mixed.out;
        mixed.purpose = Some(Purposed { purpose: wages, of: None, source: axiom_model::Provenance::Written });
        let flows = [
            RuntimeFlow::source_at(selected, 0),
            RuntimeFlow::source_at(unrelated, 1),
            RuntimeFlow::source_at(mixed, 2),
        ];
        let details = Arena::new();
        let plan = Plan::new(&book);
        let world = World::new(&book, &plan.watch);
        let day = Day::from_ymd(2026, 1, 2).unwrap();
        let occasion = Occasion::time(day, Days::on(day));
        let context = Context::new(Subject::Entity(owner), owner, &occasion).with_template_flows(&flows, &details);
        let nodes = Arena::new();
        let mut values = Vec::new();
        let mut budget_values = Vec::new();
        let mut out = Vec::new();
        let machine = Machine {
            env: Env { plan: &plan, world: &world },
            nodes: &nodes,
            law: None,
            law_id: None,
            ctx: &context,
            values: &mut values,
            budget_values: &mut budget_values,
            out: &mut out,
        };

        assert_eq!(
            machine.select(&[SelectKey::Purpose(retirement_purpose)]),
            Value::Amount(Amount::new(Qty(25_000), usd)),
            "a purpose-only selector takes the selected flow's outgoing amount"
        );
        assert_eq!(
            machine.select(&[SelectKey::End(retirement), SelectKey::Unit(vti)]),
            Value::Amount(Amount::new(Qty(7), vti)),
            "endpoint and unit keys identify the arrival side of an exchange"
        );
        assert_eq!(
            machine.select(&[SelectKey::End(retirement), SelectKey::Code(missing_code)]),
            Value::Empty,
            "a selector with no matching group is empty, so a percentage of it is zero"
        );
        assert_eq!(
            machine.select(&[SelectKey::End(retirement)]),
            Value::Fault(Fault::UnitMismatch { found: usd, expected: vti }),
            "matching both sides with incompatible units refuses to invent a sum"
        );
    }

    #[test]
    fn asset_part_context_reads_each_basis_and_limits_asset_properties_to_acquisition() {
        let text = "\
base USD
commodity USD
  precision 2
kind property : thing
  has land amount
  has in-service date optional
asset house : property
  land 120 USD
  in-service 2024-03-01
";
        let (file, parsed) = axiom_syntax::parse(axiom_core::FileId(0), text, axiom_syntax::Folder::default());
        assert!(parsed.is_empty(), "{parsed:?}");
        let (book, diagnostics) =
            axiom_model::build(&[axiom_model::Source { path: "axiom.ax", file, embedded: false }]);
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{diagnostics:?}");
        let (me, usd) = (book.roots.me, book.base);
        let asset = book.asset("house").unwrap();
        let land = book.names.get("land").unwrap();
        let plan = Plan::new(&book);
        let mut world = World::new(&book, &plan.watch);
        let origin = axiom_model::RuntimeTxn::journal(Id::new(0)).unwrap();
        let acquisition = PartId { origin, ordinal: 0 };
        let improvement = PartId { origin, ordinal: 1 };
        world
            .assets
            .add_part(
                asset,
                Part {
                    id: acquisition,
                    flow: None,
                    kind: PartKind::Acquisition,
                    recorded: EventKey { day: Day::from_ymd(2024, 2, 20).unwrap(), sequence: 0 },
                    day: Day::from_ymd(2024, 2, 20).unwrap(),
                    cost: Qty(402_000),
                    basis: Qty(382_000),
                },
            )
            .unwrap();
        world
            .assets
            .add_part(
                asset,
                Part {
                    id: improvement,
                    flow: None,
                    kind: PartKind::Improvement,
                    recorded: EventKey { day: Day::from_ymd(2026, 2, 2).unwrap(), sequence: 1 },
                    day: Day::from_ymd(2026, 2, 2).unwrap(),
                    cost: Qty(1_480),
                    basis: Qty(1_480),
                },
            )
            .unwrap();
        let occasion_day = Day::from_ymd(2026, 2, 28).unwrap();
        let occasion = Occasion::time(occasion_day, Days::on(occasion_day));
        let context = Context::new(Subject::Asset(asset), me, &occasion).for_asset_part(acquisition);
        let nodes = Arena::new();
        let mut values = Vec::new();
        let mut budget_values = Vec::new();
        let mut out = Vec::new();
        let machine = Machine {
            env: Env { plan: &plan, world: &world },
            nodes: &nodes,
            law: None,
            law_id: None,
            ctx: &context,
            values: &mut values,
            budget_values: &mut budget_values,
            out: &mut out,
        };
        assert_eq!(machine.asset_cost(asset), Value::Amount(Amount::new(Qty(402_000), book.base)));
        assert_eq!(machine.asset_basis(asset), Value::Amount(Amount::new(Qty(382_000), book.base)));
        assert_eq!(machine.prop(Value::Asset(asset), land), Value::Amount(Amount::new(Qty(12_000), usd)));
        assert_eq!(machine.asset_in_service(asset), Value::Day(Day::from_ymd(2024, 3, 1).unwrap()));

        let improvement_context = Context::new(Subject::Asset(asset), me, &occasion).for_asset_part(improvement);
        let machine = Machine { ctx: &improvement_context, ..machine };
        assert_eq!(machine.asset_cost(asset), Value::Amount(Amount::new(Qty(1_480), book.base)));
        assert_eq!(machine.asset_basis(asset), Value::Amount(Amount::new(Qty(1_480), book.base)));
        assert_eq!(machine.prop(Value::Asset(asset), land), Value::Empty);
        assert_eq!(machine.asset_in_service(asset), Value::Day(Day::from_ymd(2026, 2, 2).unwrap()));
    }

    #[test]
    fn temporal_extrema_keep_intraday_values_but_days_count_the_final_daily_state() {
        let fixture = crate::fixture::Fixture::new();
        let owner = fixture.me;
        let book = fixture.book();
        let plan = Plan::new(&book);
        let world = World::new(&book, &plan.watch);
        let day = |d| Day::from_ymd(2026, 1, d).unwrap();
        let occasion = Occasion::time(day(4), Days::new(day(1), day(4)).unwrap());
        let context = Context::new(Subject::Entity(owner), owner, &occasion);
        let nodes = Arena::new();
        let (mut values, mut budget_values, mut out) = (Vec::new(), Vec::new(), Vec::new());
        let machine = Machine {
            env: Env { plan: &plan, world: &world },
            nodes: &nodes,
            law: None,
            law_id: None,
            ctx: &context,
            values: &mut values,
            budget_values: &mut budget_values,
            out: &mut out,
        };
        let amount = |qty| Value::Amount(Amount::new(Qty(qty), book.base));
        let intraday = [
            crate::temporal::Sample { day: day(1), value: amount(100) },
            crate::temporal::Sample { day: day(2), value: amount(200) },
            crate::temporal::Sample { day: day(2), value: amount(150) },
        ];
        let span = Days::new(day(1), day(4)).unwrap();
        assert_eq!(machine.extreme(Func::Peak, span, &intraday, amount(150)), amount(200));
        assert_eq!(machine.extreme(Func::Low, span, &intraday, amount(150)), amount(100));

        let states = [
            crate::temporal::Sample { day: day(1), value: Value::Bool(true) },
            crate::temporal::Sample { day: day(2), value: Value::Bool(true) },
            crate::temporal::Sample { day: day(2), value: Value::Bool(false) },
            crate::temporal::Sample { day: day(3), value: Value::Bool(true) },
        ];
        assert_eq!(machine.day_count(span, &states, day(4), Value::Bool(false)), Value::Num(Ratio::int(2)),);
    }
}
