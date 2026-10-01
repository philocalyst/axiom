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
use axiom_core::{Day, Days, Id, Qty, Ratio, Severity, Span, Sym, day::days_in_month, spread};
use axiom_model::{
    self, Amount, BinOp, Book, Dir, Effect as LawEffect, Entity, Fault, Field, Func, Law, NodeId,
    Object, Op, Param, Prop, Purposed, StepKind, Subject, Text, Ty, Value, Var, Window,
};

use crate::calc::{Calc, progressive};
use crate::lots::{Holdings, Slot};
use crate::motion::Motion;
use crate::plan::Plan;
use crate::scope::is_money;
use crate::state::World;
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
}

impl<'a> Occasion<'a> {
    /// Something that happened on `day`, with nothing more said about it yet.
    fn on(
        day: Day,
        over: Days,
        span: Days,
        cause: Cause,
        motion: Option<&'a Motion<'a>>,
    ) -> Occasion<'a> {
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
        Occasion {
            checking: true,
            ..Occasion::time(day, period)
        }
    }

    /// A purpose total reaches a new month or year before its flows land.
    pub fn purpose_window(day: Day, period: Days, window: Window) -> Occasion<'static> {
        Occasion {
            purpose_window: Some(window),
            ..Occasion::window(day, period)
        }
    }

    /// The day whose window totals are read: the day a flow moved, or the last
    /// day of the period a law closes.
    pub fn anchor(&self) -> Day {
        if self.motion.is_some() {
            self.day
        } else {
            self.over.first()
        }
    }
}

/// What a law's variables are bound to for one firing.
pub(crate) struct Context<'a> {
    /// What `self` is.
    pub subject: Subject,
    pub owner: Id<Entity>,
    /// Governing purpose for purpose-law or template expression evaluation.
    pub governing_purpose: Option<Id<axiom_model::Purpose>>,
    /// A dated budget limit is re-evaluated at each prior window's end. These
    /// reads use retained facts instead of only the current rolling window.
    pub budget_history: bool,
    /// Declaration-order bindings for a contract occurrence.
    pub inputs: Option<&'a [Option<Amount>]>,
    on: &'a Occasion<'a>,
}

impl<'a> Context<'a> {
    pub fn new(subject: Subject, owner: Id<Entity>, on: &'a Occasion<'a>) -> Context<'a> {
        Context {
            subject,
            owner,
            governing_purpose: None,
            budget_history: false,
            inputs: None,
            on,
        }
    }

    pub fn with_inputs(mut self, inputs: &'a [Option<Amount>]) -> Self {
        self.inputs = Some(inputs);
        self
    }

    pub fn for_purpose(mut self, purpose: Id<axiom_model::Purpose>) -> Self {
        self.governing_purpose = Some(purpose);
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
    pub held: Span,
}

/// What evaluating a law found.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Outcome {
    /// A `require` or `warn` that does not hold.
    Broken { step: u32, warn: bool },
    /// A `require … else owe …` that did not hold: priced, and owed.
    Priced {
        step: u32,
        name: Sym,
        amount: Amount,
        owed: Owed,
    },
    /// A fault reached a step.
    Faulted { step: u32, fault: Fault },
    /// `count`: adds `amount` (base currency) to a tally.
    Count { name: Sym, amount: Qty },
    /// `owe`.
    Owe {
        name: Sym,
        amount: Amount,
        owed: Owed,
    },
    /// What a `require` or `warn` compared: `counted <= limit`, the sides of a
    /// `>=` swapped.
    Read {
        step: u32,
        counted: Amount,
        limit: Amount,
    },
}

/// Runs `law`'s steps in order. Returns whether it ran to the end rather than
/// stopping at a `when` that was false (or faulted).
pub(crate) fn run(
    env: Env,
    law: &Law,
    ctx: &Context,
    values: &mut Vec<Value>,
    out: &mut Vec<Outcome>,
) -> bool {
    values.resize(law.nodes.len(), Value::Empty);
    let mut machine = Machine {
        env,
        nodes: &law.nodes,
        law: Some(law),
        ctx,
        values,
        out,
    };
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

/// Evaluates one expression of `law` (a `by` date, say).
pub(crate) fn expression(
    env: Env,
    law: &Law,
    root: NodeId,
    ctx: &Context,
    values: &mut Vec<Value>,
) -> Value {
    values.resize(law.nodes.len(), Value::Empty);
    Machine {
        env,
        nodes: &law.nodes,
        law: Some(law),
        ctx,
        values,
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
        ctx,
        values,
        out: &mut Vec::new(),
    }
    .scan(root)
}

struct Machine<'a, 's> {
    env: Env<'a, 's>,
    nodes: &'a axiom_core::Arena<axiom_model::Node>,
    law: Option<&'a Law>,
    ctx: &'a Context<'a>,
    values: &'a mut Vec<Value>,
    /// What the steps so far found. A later step's `tally(…)` sees the `count`s
    /// among them, which the ledger will not have applied until the law is done.
    out: &'a mut Vec<Outcome>,
}

const TYPED: &str = "the model type-checks operands";

fn budget_window(period: axiom_core::Period) -> Window {
    match period {
        axiom_core::Period::Month => Window::Month,
        axiom_core::Period::Year => Window::Year,
    }
}

impl<'a, 's> Machine<'a, 's> {
    fn book(&self) -> &'a Book<'s> {
        self.env.plan.book
    }

    fn calc(&self) -> Calc<'a, 's> {
        Calc {
            book: self.env.plan.book,
            day: self.ctx.day,
        }
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
            StepKind::Require {
                cond,
                otherwise,
                severity,
                ..
            } => {
                let held = self.scan(*cond);
                self.read(step, *cond);
                match held {
                    Value::Bool(true) => {}
                    // v3 bridge: a v3 `require` has one reparation at most.
                    Value::Bool(false) => match otherwise.first() {
                        Some(effect) => self.price(step, effect),
                        None => self.out.push(Outcome::Broken {
                            step,
                            warn: *severity == Severity::Warning,
                        }),
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
        let Some((cmp, left, right)) = compared(
            self.law.expect("only law steps produce comparisons"),
            self.values,
            cond,
        ) else {
            return;
        };
        let (counted, limit) = if matches!(cmp, BinOp::Lt | BinOp::Le) {
            (left, right)
        } else {
            (right, left)
        };
        self.out.push(Outcome::Read {
            step,
            counted,
            limit,
        });
    }

    /// A `require … else owe …` that failed: the violation is priced.
    fn price(&mut self, step: u32, effect: &LawEffect) {
        let before = self.out.len();
        self.effect(step, effect);
        if let Some(&Outcome::Owe { name, amount, owed }) = self.out.get(before) {
            self.out[before] = Outcome::Priced {
                step,
                name,
                amount,
                owed,
            };
        }
    }

    fn effect(&mut self, step: u32, effect: &LawEffect) {
        match *effect {
            LawEffect::Count { amount, name } => {
                let Some(amount) = self.nonzero_amount(step, amount) else {
                    return;
                };
                match self.calc().convert(amount, self.book().base) {
                    Ok(base) => self.out.push(Outcome::Count {
                        name,
                        amount: base.qty,
                    }),
                    Err(fault) => self.out.push(Outcome::Faulted { step, fault }),
                }
            }
            LawEffect::Owe {
                amount,
                to,
                due,
                name,
            } => {
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
                self.out.push(Outcome::Owe {
                    name,
                    amount,
                    owed: Owed { to, due },
                });
            }
            // These effects are applied by the native asset/claim monitor. A
            // law cannot silently fall back to the removed v3 asset bridge.
            LawEffect::Consume { .. } | LawEffect::Carry { .. } => {
                self.out.push(Outcome::Faulted {
                    step,
                    fault: Fault::InvalidProgram,
                });
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
            Outcome::Count {
                name: counted,
                amount,
            } if counted == name && year == this_year => Some(spread(
                amount,
                ctx.over,
                Window::Year.around(ctx.over.first()),
            )),
            _ => None,
        });
        self.base(tallies.read(ctx.owner, year, name) + counted.sum())
    }

    fn node(&self, at: usize) -> Value {
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
                self.nodes[NodeId(at as u32)]
                    .typed_ty()
                    .unwrap_or(Ty::Empty),
            ),
            Op::At(quantity, price) => self.calc().binary_typed(
                BinOp::Mul,
                self.at(*quantity),
                self.at(*price),
                self.nodes[*quantity].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[*price].typed_ty().unwrap_or(Ty::Empty),
                self.nodes[NodeId(at as u32)]
                    .typed_ty()
                    .unwrap_or(Ty::Empty),
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
            Op::Select(_) => Value::Fault(Fault::InvalidProgram),
            Op::Is(x, alternatives) => match self.at(*x) {
                fault @ Value::Fault(_) => fault,
                left => Value::Bool(
                    alternatives
                        .iter()
                        .any(|&alt| self.matches(left, self.at(alt))),
                ),
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
        let realized =
            |pick: fn(&Realized) -> Qty| ctx.realized.map_or(Value::Empty, |r| self.base(pick(&r)));
        match var {
            Var::Amount => ctx.amount.map_or(Value::Empty, Value::Amount),
            Var::From => flow(|m| Value::Place(m.from)),
            Var::To => flow(|m| Value::Place(m.to)),
            Var::Payee => flow(|m| m.payee.map_or(Value::Empty, Value::Entity)),
            Var::Date => Value::Day(ctx.day),
            Var::Year => Value::Num(Ratio::int(ctx.over.first().year() as i64)),
            Var::Month => Value::Num(Ratio::int(ctx.over.first().ymd().1 as i64)),
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
            Var::Purpose => ctx.purpose.map_or(Value::Empty, |purpose| {
                Value::Purpose(purpose.purpose, purpose.of)
            }),
            Var::Description => ctx.description.map_or(Value::Empty, Value::Text),
            Var::Input(index) => match ctx
                .inputs
                .and_then(|inputs| inputs.get(index as usize))
                .copied()
                .flatten()
            {
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

    /// From the entity's `born` date to the day of evaluation.
    fn age(&self, entity: Id<Entity>) -> Value {
        let born = self
            .env
            .plan
            .known
            .born
            .expect("a law that reads `.age` makes the model intern `born`");
        match axiom_model::prop(&self.book().entities[entity].props, born, self.ctx.day) {
            Some(Prop {
                value: Value::Day(day),
                ..
            }) => Value::Span(self.ctx.day.since(*day)),
            _ => Value::Fault(Fault::Unset(born)),
        }
    }

    /// A declared property: the thing's own, else its kind's default.
    fn prop(&self, base: Value, name: Sym) -> Value {
        let book = self.book();
        let props = match base {
            Value::Place(place) => &book.places[place].props,
            Value::Entity(entity) => &book.entities[entity].props,
            Value::Unit(unit) => &book.commodities[unit].props,
            Value::Kind(kind) => &book.kinds[kind].props,
            Value::Asset(asset) => &book.assets[asset].props,
            _ => unreachable!("{TYPED}"),
        };
        axiom_model::prop(props, name, self.ctx.day)
            .map_or(Value::Fault(Fault::Unset(name)), |property| property.value)
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
            (Value::Place(p), Value::Kind(k)) => book.is_a(book.places[p].kind, k),
            (Value::Entity(e), Value::Kind(k)) => book.is_a(book.entities[e].kind, k),
            (Value::Unit(u), Value::Kind(k)) => book.is_a(book.commodities[u].kind, k),
            (Value::Kind(a), Value::Kind(k)) => book.is_a(a, k),
            (Value::Purpose(actual, actual_of), Value::Purpose(wanted, wanted_of)) => {
                book.purposes.covers(wanted, actual)
                    && wanted_of.is_none_or(|wanted| {
                        actual_of.is_some_and(|actual| object_matches(book, actual, wanted))
                    })
            }
            (Value::Place(p), Value::Place(root)) => book.places.covers(root, p),
            (Value::Place(p), Value::Entity(root)) => {
                book.entities.covers(root, book.places[p].owner)
            }
            (Value::Entity(e), Value::Entity(root)) => book.entities.covers(root, e),
            (Value::Unit(a), Value::Unit(b)) => a == b,
            (Value::Place(p), Value::Glob(pattern)) => named(pattern, book.places[p].path),
            (Value::Entity(e), Value::Glob(pattern)) => named(pattern, book.entities[e].path),
            (Value::Unit(u), Value::Glob(pattern)) => named(pattern, book.commodities[u].symbol),
            (Value::Flow, Value::Code(code)) => self
                .ctx
                .motion
                .is_some_and(|m| m.codes().any(|mark| named(code, mark))),
            _ => false,
        }
    }

    fn call(&self, at: NodeId, func: Func, args: &[NodeId]) -> Value {
        let arg = |i: usize| self.at(args[i]);
        // `total` and `tally` take their operands from the function itself.
        let operands = !matches!(
            func,
            Func::Total(..) | Func::PurposeTotal { .. } | Func::BudgetTotal(_) | Func::Tally(_)
        );
        if operands
            && let Some(fault) = args
                .iter()
                .map(|&a| self.at(a))
                .find(|v| matches!(v, Value::Fault(_)))
        {
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
                (Value::Amount(a), Value::Unit(unit)) => self
                    .calc()
                    .convert(a, unit)
                    .map_or_else(Value::Fault, Value::Amount),
                (Value::Empty, Value::Unit(unit)) => Value::Amount(Amount::zero(unit)),
                _ => unreachable!("{TYPED}"),
            },
            Func::Date => civil_date(arg(0), arg(1), arg(2)).map_or_else(Value::Fault, Value::Day),
            Func::StraightLine => self.straight_line(args),
            Func::Open(code) => self.open(code),
            // Extrema and day counts need the occurrence-history recorder.
            // Do not silently substitute the current sample for a historical answer.
            Func::Peak | Func::Low | Func::Days => Value::Fault(Fault::InvalidProgram),
        }
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
            let days = window.around(self.ctx.anchor());
            let Some(span) = Days::new(days.first(), days.last().min(self.ctx.anchor())) else {
                return Value::Fault(Fault::InvalidProgram);
            };
            self.env.world.totals.read_purpose_between(self.ctx.owner, purpose, span)
        } else {
            self.env.world.totals.read_purpose(self.ctx.owner, purpose, window, self.ctx.anchor())
        };
        let total = match root {
            axiom_model::PurposeRoot::Income => incoming - outgoing,
            axiom_model::PurposeRoot::Spending
            | axiom_model::PurposeRoot::Capital
            | axiom_model::PurposeRoot::Transfer => outgoing - incoming,
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
            start
        } else {
            start.max(budget_window(terms.period).around(anchor).first())
        };
        let Some(span) = Days::new(first, anchor) else {
            return Value::Fault(Fault::InvalidProgram);
        };
        let (incoming, outgoing) = self.env.world.totals.read_purpose_between(self.ctx.owner, budget.purpose, span);
        let total = match self.book().purposes[budget.purpose].root {
            axiom_model::PurposeRoot::Income => incoming - outgoing,
            axiom_model::PurposeRoot::Spending
            | axiom_model::PurposeRoot::Capital
            | axiom_model::PurposeRoot::Transfer => outgoing - incoming,
        };
        self.base(total)
    }

    /// The allowance in force for a generated native budget law. Computed
    /// limits are roots earlier in that law's shared node arena; share limits
    /// read the declared purpose total for the budget's own period.
    fn budget_limit(&self, id: Id<axiom_model::Budget>, at: NodeId) -> Value {
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
        let mut historical_values = Vec::new();
        if !terms.carries {
            let window = budget_window(terms.period).around(day);
            let span = Days::new(start.max(window.first()), day.min(window.last()));
            let Some(span) = span else {
                return Value::Fault(Fault::InvalidProgram);
            };
            return self.one_budget_limit(id, terms.limit, span, at, day, &mut historical_values);
        }

        let mut cursor = start;
        let mut total = Qty::ZERO;
        while cursor <= day {
            let period = budget_window(budget.terms.at(cursor).period);
            let period_days = period.around(cursor);
            let end = period_days.last().min(day);
            let Some(span) = Days::new(cursor, end) else {
                return Value::Fault(Fault::InvalidProgram);
            };
            // A dated restatement changes the allowance in force for the
            // window containing it. The window contributes one allowance.
            let effective = *budget.terms.at(end);
            let value = self.one_budget_limit(id, effective.limit, span, at, end, &mut historical_values);
            let Value::Amount(amount) = value else {
                return value;
            };
            let calc = Calc { book: self.book(), day: end };
            match calc.convert(amount, self.book().base) {
                Ok(amount) => total += amount.qty,
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
        &self,
        id: Id<axiom_model::Budget>,
        limit: axiom_model::Limit,
        span: Days,
        at: NodeId,
        on: Day,
        values: &mut Vec<Value>,
    ) -> Value {
        match limit {
            axiom_model::Limit::Amount(amount) => Value::Amount(amount),
            axiom_model::Limit::Share { rate, of } => {
                let (incoming, outgoing) = self
                    .env
                    .world
                    .totals
                    .read_purpose_between(self.ctx.owner, of, span);
                let root = self.book().purposes[of].root;
                let total = match root {
                    axiom_model::PurposeRoot::Income => incoming - outgoing,
                    axiom_model::PurposeRoot::Spending
                    | axiom_model::PurposeRoot::Capital
                    | axiom_model::PurposeRoot::Transfer => outgoing - incoming,
                };
                total.scale(rate).map_or(Value::Fault(Fault::Overflow), |qty| {
                    Value::Amount(Amount::new(qty, self.book().base))
                })
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
                    values,
                );
                match value {
                    value @ (Value::Amount(_) | Value::Fault(_)) => value,
                    _ => Value::Fault(Fault::InvalidProgram),
                }
            }
        }
    }

    fn straight_line(&self, args: &[NodeId]) -> Value {
        let (Value::Amount(cost), Value::Span(life), Value::Day(from), Value::Name(period)) = (
            self.at(args[0]),
            self.at(args[1]),
            self.at(args[2]),
            self.at(args[3]),
        ) else {
            unreachable!("{TYPED}");
        };
        let window = match self.book().name(period) {
            "month" => Window::Month,
            "year" => Window::Year,
            _ => unreachable!("{TYPED}"),
        };
        let mid_month = args.get(4).is_some_and(|&node| {
            matches!(self.at(node), Value::Name(name) if self.book().name(name) == "mid-month")
        });
        crate::calc::straight_line(cost.qty, life, from, self.ctx.over, window, mid_month)
            .map_or(Value::Fault(Fault::Overflow), |qty| {
                Value::Amount(Amount::new(qty, cost.unit))
            })
    }

    fn open(&self, code: Sym) -> Value {
        let book = self.book();
        let unit = book.entities[self.ctx.owner].currency;
        let mut total = Qty::ZERO;
        for (place, declaration) in book.places.iter() {
            if !declaration.claim {
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
        let read = |subject| {
            if ctx.budget_history {
                let days = window.around(ctx.anchor());
                Days::new(days.first(), days.last().min(ctx.anchor()))
                    .map_or(Qty::ZERO, |span| totals.read_subject_between(subject, dir, span))
            } else {
                totals.read(&self.env.plan.watch, subject, dir, window, ctx.anchor())
            }
        };
        let widen = args.iter().find_map(|&a| {
            if let Value::Kind(kind) = self.at(a) {
                Some(kind)
            } else {
                None
            }
        });
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
                .map(|&place| read(Subject::Place(place)))
                .sum(),
        };
        self.base(sum)
    }

    fn progressive(&self, schedule: Value, income: Value) -> Value {
        let Value::Schedule(id) = schedule else {
            unreachable!("{TYPED}")
        };
        let schedule = &self.book().schedules[id];
        let income = match income {
            Value::Amount(a) => self.calc().convert(a, schedule.unit).map(|a| a.qty),
            _ => Ok(Qty::ZERO),
        };
        match income {
            Ok(income) => progressive(&schedule.brackets, income)
                .map_or(Value::Fault(Fault::Overflow), |tax| {
                    Value::Amount(Amount::new(tax, schedule.unit))
                }),
            Err(fault) => Value::Fault(fault),
        }
    }

    fn held(&self, subject: Subject) -> impl Iterator<Item = &'a Slot> {
        held(self.env.plan, self.env.world, subject)
    }

    /// Everything the subject holds, valued in the base currency.
    fn balance(&self, subject: Subject) -> Value {
        let sign = sign(self.env.plan, subject);
        self.sum_in_base(
            self.held(subject)
                .map(|slot| Amount::new(Qty(slot.qty.0 * sign), slot.unit)),
        )
    }

    /// What everything the subject holds has already accounted for, in the base
    /// currency: the total basis of its parcels.
    fn basis(&self, subject: Subject) -> Value {
        let (book, sign) = (self.book(), sign(self.env.plan, subject));
        let basis: Qty = self
            .held(subject)
            .map(|slot| slot.basis(is_money(book, slot.place, slot.unit)))
            .sum();
        self.base(Qty(basis.0 * sign))
    }

    /// Money still tied to the subject, a restricted entity.
    fn remaining(&self) -> Value {
        let Subject::Entity(entity) = self.ctx.subject else {
            return Value::Empty;
        };
        let tied = self.env.world.holdings.iter().flat_map(|h| {
            h.lots
                .iter()
                .filter(move |lot| lot.tied == Some(entity))
                .map(move |lot| Amount::new(lot.qty, h.unit))
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
pub(crate) fn held<'a>(
    plan: &'a Plan,
    world: &'a World,
    subject: Subject,
) -> impl Iterator<Item = &'a Slot> {
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
pub(crate) fn compared(
    law: &Law,
    values: &[Value],
    cond: NodeId,
) -> Option<(BinOp, Amount, Amount)> {
    let Op::Bin(cmp @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), left, right) =
        law.nodes[cond].op
    else {
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
    let month = u32::try_from(month)
        .ok()
        .filter(|m| (1..=12).contains(m))
        .ok_or(Fault::Overflow)?;
    let day = u32::try_from(day.max(1))
        .unwrap_or(1)
        .min(days_in_month(year, month));
    Day::from_ymd(year, month, day).ok_or(Fault::Overflow)
}
