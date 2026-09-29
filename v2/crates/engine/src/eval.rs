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
use axiom_core::{Day, Id, Qty, Ratio, Span, Sym, day::days_in_month};
use axiom_model::{
    Amount, BinOp, Book, Dir, Effect as Consequence, Entity, Fault, Field, Func, Law, NodeId, Op, Param, Prop,
    Recognition, StepKind, Subject, Value, Var, Window,
};

use crate::calc::{Calc, progressive};
use crate::lots::{Holdings, Slot};
use crate::motion::Motion;
use crate::scope::{inside, is_money};
use crate::state::World;
use crate::totals::share_in_first_year;
use crate::{Cause, Owed};

/// What laws read: the book and the state as of now.
#[derive(Clone, Copy)]
pub(crate) struct Env<'a, 's> {
    pub book: &'a Book<'s>,
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
    pub over: Recognition,
    /// The days a rule must be in force for: a flow's day, or a whole period.
    pub span: Recognition,
    pub cause: Cause,
    /// The flow that fired the law, if one did (not for `each` and `by`).
    pub motion: Option<&'a Motion<'a>>,
    /// `amount`: what the trigger says is moving.
    pub amount: Option<Amount>,
    /// `gain`, `proceeds`, `basis`, `held`, for `on gain`.
    pub realized: Option<Realized>,
    /// Rules whose subject contains both ends of the flow do not fire: value
    /// moved around inside the subject neither entered nor left it.
    pub skip_internal: bool,
}

impl<'a> Occasion<'a> {
    pub fn flow(m: &'a Motion<'a>) -> Occasion<'a> {
        let (over, span) = (m.recognized, Recognition::on(m.day));
        Occasion { day: m.day, over, span, cause: m.cause, motion: Some(m), amount: None, realized: None, skip_internal: false }
    }

    /// A period ending, or a deadline passing, on `day`.
    pub fn time(day: Day, period: Recognition) -> Occasion<'static> {
        let over = Recognition::on(period.until);
        Occasion { day, over, span: period, cause: Cause::Time, motion: None, amount: None, realized: None, skip_internal: false }
    }

    /// The day whose window totals are read: the day a flow moved, or the last
    /// day of the period a law closes.
    pub fn anchor(&self) -> Day {
        if self.motion.is_some() { self.day } else { self.over.from }
    }
}

/// What a law's variables are bound to for one firing.
pub(crate) struct Context<'a> {
    /// What `self` is.
    pub subject: Subject,
    pub owner: Id<Entity>,
    on: &'a Occasion<'a>,
}

impl<'a> Context<'a> {
    pub fn new(subject: Subject, owner: Id<Entity>, on: &'a Occasion<'a>) -> Context<'a> {
        Context { subject, owner, on }
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
}

/// Runs `law`'s steps in order. Returns whether it ran to the end rather than
/// stopping at a `when` that was false (or faulted).
pub(crate) fn run(env: Env, law: &Law, ctx: &Context, values: &mut Vec<Value>, out: &mut Vec<Outcome>) -> bool {
    values.resize(law.nodes.len(), Value::Empty);
    let mut machine = Machine { env, law, ctx, values, out };
    (0..law.steps.len()).all(|index| machine.step(index))
}

/// Evaluates one expression of `law` (a `by` date, say).
pub(crate) fn expression(env: Env, law: &Law, root: NodeId, ctx: &Context, values: &mut Vec<Value>) -> Value {
    values.resize(law.nodes.len(), Value::Empty);
    Machine { env, law, ctx, values, out: &mut Vec::new() }.scan(root)
}

struct Machine<'a, 's> {
    env: Env<'a, 's>,
    law: &'a Law,
    ctx: &'a Context<'a>,
    values: &'a mut Vec<Value>,
    /// What the steps so far found. A later step's `tally(…)` sees the `count`s
    /// among them, which the ledger will not have applied until the law is done.
    out: &'a mut Vec<Outcome>,
}

const TYPED: &str = "the model type-checks operands";

impl<'a, 's> Machine<'a, 's> {
    fn book(&self) -> &'a Book<'s> {
        self.env.book
    }

    fn calc(&self) -> Calc<'a, 's> {
        Calc { book: self.env.book, day: self.ctx.day }
    }

    fn base(&self, qty: Qty) -> Value {
        Value::Amount(Amount::new(qty, self.book().base))
    }

    fn at(&self, node: NodeId) -> Value {
        self.values[node.index()]
    }

    /// Evaluates a step's expression: one forward scan, each node stored.
    fn scan(&mut self, root: NodeId) -> Value {
        for at in self.law.range(root) {
            let value = self.node(at);
            self.values[at] = value;
        }
        self.at(root)
    }

    /// Runs one step. `false` stops the law.
    fn step(&mut self, index: usize) -> bool {
        let step = index as u32;
        match &self.law.steps[index].kind {
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
            StepKind::Require { cond, otherwise, warn, .. } => {
                let held = self.scan(*cond);
                self.read(step, *cond);
                match held {
                    Value::Bool(true) => {}
                    Value::Bool(false) => match otherwise {
                        Some(effect) => self.price(step, effect),
                        None => self.out.push(Outcome::Broken { step, warn: *warn }),
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
        let Op::Bin(cmp @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), left, right) = self.law.nodes[cond.index()].op
        else {
            return;
        };
        let (left, right) = match (self.at(left), self.at(right)) {
            (Value::Amount(l), Value::Amount(r)) => (l, r),
            (Value::Amount(l), Value::Empty) => (l, Amount::zero(l.unit)),
            (Value::Empty, Value::Amount(r)) => (Amount::zero(r.unit), r),
            _ => return,
        };
        let (counted, limit) = if matches!(cmp, BinOp::Lt | BinOp::Le) { (left, right) } else { (right, left) };
        self.out.push(Outcome::Read { step, counted, limit });
    }

    /// A `require … else owe …` that failed: the violation is priced.
    fn price(&mut self, step: u32, effect: &Consequence) {
        let before = self.out.len();
        self.effect(step, effect);
        if let Some(&Outcome::Owe { name, amount, owed }) = self.out.get(before) {
            self.out[before] = Outcome::Priced { step, name, amount, owed };
        }
    }

    fn effect(&mut self, step: u32, effect: &Consequence) {
        match *effect {
            Consequence::Count { amount, name } => {
                let Some(amount) = self.nonzero_amount(step, amount) else { return };
                match self.calc().convert(amount, self.book().base) {
                    Ok(base) => self.out.push(Outcome::Count { name, amount: base.qty }),
                    Err(fault) => self.out.push(Outcome::Faulted { step, fault }),
                }
            }
            Consequence::Owe { amount, to, due, name } => {
                let Some(amount) = self.nonzero_amount(step, amount) else { return };
                let due = match due.map(|node| self.scan(node)) {
                    None => self.ctx.day,
                    Some(Value::Day(day)) => day,
                    Some(Value::Fault(fault)) => return self.out.push(Outcome::Faulted { step, fault }),
                    Some(_) => unreachable!("{TYPED}"),
                };
                self.out.push(Outcome::Owe { name, amount, owed: Owed { to, due } });
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

    /// A tally as it stands: what the world holds, plus what earlier steps of
    /// this same law have counted, so `count amount as x` then `require
    /// tally(x) <= …` sees the flow it is checking.
    fn tally(&self, name: Sym) -> Value {
        let (ctx, tallies) = (self.ctx, &self.env.world.tallies);
        let counted = self.out.iter().filter_map(|o| match *o {
            Outcome::Count { name: counted, amount } if counted == name => Some(share_in_first_year(amount, ctx.over)),
            _ => None,
        });
        self.base(tallies.read(ctx.owner, ctx.over.from.year(), name) + counted.sum())
    }

    fn node(&self, at: usize) -> Value {
        match &self.law.nodes[at].op {
            Op::Const(value) => *value,
            Op::Var(var) => self.var(*var),
            Op::Local(bound) => self.at(*bound),
            Op::Field(base, field) => self.field(self.at(*base), *field),
            Op::Param(param, keys) => self.param(*param, keys),
            Op::Call(func, args) => self.call(*func, args),
            Op::Neg(x) => match self.at(*x) {
                Value::Amount(a) => Value::Amount(Amount::new(-a.qty, a.unit)),
                Value::Num(n) => Value::Num(-n),
                other => other,
            },
            Op::Not(x) => match self.at(*x) {
                Value::Bool(b) => Value::Bool(!b),
                other => other,
            },
            Op::Bin(op, l, r) => self.calc().binary(*op, self.at(*l), self.at(*r)),
            Op::Is(x, alternatives) => match self.at(*x) {
                fault @ Value::Fault(_) => fault,
                left => Value::Bool(alternatives.iter().any(|&alt| self.matches(left, self.at(alt)))),
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
            Var::Year => Value::Num(Ratio::int(ctx.over.from.year() as i64)),
            Var::Month => Value::Num(Ratio::int(ctx.over.from.ymd().1 as i64)),
            Var::Subject => match ctx.subject {
                Subject::Place(place) => Value::Place(place),
                Subject::Entity(entity) => Value::Entity(entity),
            },
            Var::Owner => Value::Entity(ctx.owner),
            Var::Gain => realized(|r| r.gain),
            Var::Proceeds => realized(|r| r.proceeds),
            Var::Basis => realized(|r| r.basis),
            Var::Held => ctx.realized.map_or(Value::Empty, |r| Value::Span(r.held)),
            Var::Balance => self.balance(ctx.subject),
            Var::Remaining => self.remaining(),
            Var::Flow => Value::Flow,
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
            (Field::Year, Value::Day(day)) => Value::Num(Ratio::int(day.year() as i64)),
            (Field::Month, Value::Day(day)) => Value::Num(Ratio::int(day.ymd().1 as i64)),
            (Field::Prop(name), base) => self.prop(base, name),
            _ => unreachable!("{TYPED}"),
        }
    }

    /// From the entity's `born` date to the day of evaluation.
    fn age(&self, entity: Id<Entity>) -> Value {
        let born = self.book().names.get("born").expect("a law that reads `.age` makes the model intern `born`");
        match self.book().entities[entity].props.iter().find(|p| p.name == born) {
            Some(Prop { value: Value::Day(day), .. }) => Value::Span(self.ctx.day.since(*day)),
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
            _ => unreachable!("{TYPED}"),
        };
        props.iter().find(|p| p.name == name).map_or(Value::Fault(Fault::Unset(name)), |p| p.value)
    }

    /// `limit[year]`: among rows whose name keys equal the lookup's, the latest
    /// that starts on or before the day asked. A number key asks for the first
    /// of that year, a date key for that day; with neither, the context day.
    fn param(&self, param: Id<Param>, keys: &[NodeId]) -> Value {
        let mut when = self.ctx.anchor();
        for &key in keys {
            match self.at(key) {
                Value::Fault(fault) => return Value::Fault(fault),
                Value::Num(year) => match Day::from_ymd(year.round() as i32, 1, 1) {
                    Some(start) => when = start,
                    None => return Value::Fault(Fault::Overflow),
                },
                Value::Day(day) => when = day,
                _ => {}
            }
        }
        let wanted = keys.iter().filter_map(|&key| self.key_name(self.at(key)));
        let rows = self.book().params[param].rows.iter();
        let best = rows
            .filter(|row| row.since.is_none_or(|since| since <= when) && row.names.iter().copied().eq(wanted.clone()));
        best.max_by_key(|row| row.since).map_or(Value::Fault(Fault::NoRow(param)), |row| row.value)
    }

    fn key_name(&self, key: Value) -> Option<Sym> {
        let book = self.book();
        match key {
            Value::Name(sym) | Value::Text(sym) | Value::Code(sym) => Some(sym),
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
            (Value::Place(p), Value::Place(root)) => book.places.covers(root, p),
            (Value::Place(p), Value::Entity(root)) => book.entities.covers(root, book.places[p].owner),
            (Value::Entity(e), Value::Entity(root)) => book.entities.covers(root, e),
            (Value::Unit(a), Value::Unit(b)) => a == b,
            (Value::Place(p), Value::Glob(pattern)) => named(pattern, book.places[p].path),
            (Value::Entity(e), Value::Glob(pattern)) => named(pattern, book.entities[e].path),
            (Value::Unit(u), Value::Glob(pattern)) => named(pattern, book.commodities[u].symbol),
            (Value::Flow, Value::Code(code)) => {
                self.ctx.motion.is_some_and(|m| m.codes.iter().any(|&mark| named(code, mark)))
            }
            _ => false,
        }
    }

    fn call(&self, func: Func, args: &[NodeId]) -> Value {
        let arg = |i: usize| self.at(args[i]);
        // `total` and `tally` take their operands from the function itself.
        let operands = !matches!(func, Func::Total(..) | Func::Tally(_));
        if operands && let Some(fault) = args.iter().map(|&a| self.at(a)).find(|v| matches!(v, Value::Fault(_))) {
            return fault;
        }
        match func {
            Func::Total(dir, window) => self.total(dir, window, args),
            Func::Tally(name) => self.tally(name),
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
        }
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
        let widen = args.iter().find_map(|&a| if let Value::Kind(kind) = self.at(a) { Some(kind) } else { None });
        let read = |subject| totals.read(subject, dir, window, ctx.anchor());
        let sum = match widen {
            None => read(ctx.subject),
            Some(kind) => {
                let mine = book.places.iter().filter(|(_, p)| p.owner == ctx.owner && book.is_a(p.kind, kind));
                mine.map(|(id, _)| read(Subject::Place(id))).sum()
            }
        };
        self.base(sum)
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

    /// The holdings within the subject: a place's subtree, or the asset places
    /// an entity owns.
    fn held(&self, subject: Subject) -> impl Iterator<Item = &'a Slot> {
        let (book, holdings): (&'a Book<'s>, &'a Holdings) = (self.book(), &self.env.world.holdings);
        let span = match subject {
            Subject::Place(root) => root.index()..book.places.end(root).index(),
            Subject::Entity(_) => 0..book.places.len(),
        };
        holdings.within(span).filter(move |slot| inside(book, subject, slot.place))
    }

    /// The sign people read a subject's balance in: a credit card's balance is
    /// what is owed, as an assertion writes it. An entity's is natural.
    fn sign(&self, subject: Subject) -> i64 {
        match subject {
            Subject::Place(place) => self.book().places[place].class.display_sign(),
            Subject::Entity(_) => 1,
        }
    }

    /// Everything the subject holds, valued in the base currency.
    fn balance(&self, subject: Subject) -> Value {
        let sign = self.sign(subject);
        self.sum_in_base(self.held(subject).map(|slot| Amount::new(Qty(slot.qty.0 * sign), slot.unit)))
    }

    /// What everything the subject holds has already accounted for, in the base
    /// currency: the total basis of its parcels.
    fn basis(&self, subject: Subject) -> Value {
        let (book, sign) = (self.book(), self.sign(subject));
        let basis: Qty = self.held(subject).map(|slot| slot.basis(is_money(book, slot.place, slot.unit))).sum();
        self.base(Qty(basis.0 * sign))
    }

    /// Money still tied to the subject, a restricted entity.
    fn remaining(&self) -> Value {
        let Subject::Entity(entity) = self.ctx.subject else { return Value::Empty };
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
