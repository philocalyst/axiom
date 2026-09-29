//! Compiled laws.
//!
//! A law's expressions are one flat arena of [`Node`]s in post-order, one node
//! per source expression. Children precede parents, so the engine evaluates a
//! step by scanning its node range once, writing each node's [`Value`] into a
//! scratch slot. When a `require` fails, every subexpression's value is
//! already sitting there, ready to be shown under its source (power-assert).
//!
//! Evaluation is total: a missing price or an unset property is a
//! [`Value::Fault`], not an early exit. `if`, `and` and `or` pick among values
//! already computed, so a fault in a branch not taken is never observed.

use axiom_core::{Day, Groups, Id, Loc, Ratio, Span, Sym};

use crate::book::{Amount, Commodity, Entity, Kind, Param, Place, Schedule, System};

pub use axiom_syntax::{BinOp, Period};

pub struct Law {
    pub name: Sym,
    pub doc: Option<Sym>,
    pub owner: Owner,
    /// The system that declared it: tallies it counts and params it reads
    /// are scoped here.
    pub system: Option<Id<System>>,
    pub trigger: Trigger,
    pub steps: Box<[Step]>,
    pub nodes: Box<[Node]>,
    pub loc: Loc,
}

impl Law {
    /// The nodes of `root`'s expression, in evaluation order: `first..=root`.
    pub fn range(&self, root: NodeId) -> std::ops::RangeInclusive<usize> {
        self.nodes[root.index()].first.index()..=root.index()
    }
}

/// Where a law was written, which decides what it governs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    /// Governs every place, entity or commodity of the kind (and subkinds).
    Kind(Id<Kind>),
    /// Governs the place and its subtree. Budgets land here.
    Place(Id<Place>),
    /// Governs the entity (`on spend`, `by`, `each`).
    Entity(Id<Entity>),
    /// A system's top-level law: governs its residents and all they own.
    System(Id<System>),
    /// A law written at the top level of a project file: governs every place
    /// in the book, with the place's owner as `self`.
    Book,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    In,
    Out,
    Gain,
    Spend,
    Each(Period),
    /// Fires once the journal reaches this date, evaluated per subject.
    By(NodeId),
    Always,
}

#[derive(Debug)]
pub struct Step {
    pub loc: Loc,
    pub kind: StepKind,
}

#[derive(Debug)]
pub enum StepKind {
    When(NodeId),
    /// The bound value is the node's; later nodes read it with [`Op::Local`].
    Let(NodeId),
    Require {
        cond: NodeId,
        otherwise: Option<Effect>,
        message: Option<Sym>,
        warn: bool,
    },
    Effect(Effect),
}

#[derive(Debug)]
pub enum Effect {
    /// An obligation from the subject's owner to `to`, due by `due` (default:
    /// the triggering day). `name` defaults to the law's name.
    Owe { amount: NodeId, to: Id<Entity>, due: Option<NodeId>, name: Sym },
    /// Adds to a tally keyed by owner, year, name and the law's system.
    Count { amount: NodeId, name: Sym },
}

/// Index of a node in its law's arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug)]
pub struct Node {
    pub op: Op,
    /// Statically checked: evaluation never meets a type it did not expect.
    pub ty: Ty,
    pub loc: Loc,
    /// The first node of this node's subtree.
    pub first: NodeId,
}

#[derive(Debug)]
pub enum Op {
    /// A literal, or a name resolved at compile time. Arguments folded into a
    /// [`Func`] (`in`, `year` in `total(in, year)`) stay as constants.
    Const(Value),
    Var(Var),
    /// The value bound by a `let`.
    Local(NodeId),
    Field(NodeId, Field),
    /// `limit[year]`: a param looked up by the key nodes. No keys, as for a
    /// bare `catch-up`, means the row in force on the day the law runs.
    Param(Id<Param>, Box<[NodeId]>),
    Call(Func, Box<[NodeId]>),
    Neg(NodeId),
    Not(NodeId),
    Bin(BinOp, NodeId, NodeId),
    /// True when the left value matches any alternative: a kind, place,
    /// entity, glob or code.
    Is(NodeId, Box<[NodeId]>),
    If(NodeId, NodeId, NodeId),
}

/// What the triggering event provides. The compiler rejects a variable its
/// law's trigger does not supply.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Var {
    /// The quantity moving (`on in|out|spend`), or relieved (`on gain`).
    Amount,
    From,
    To,
    Payee,
    Date,
    Year,
    Month,
    /// `self`: the governed place or entity.
    Subject,
    /// The subject's owner (for an entity, itself).
    Owner,
    /// `on gain`: proceeds − basis, in the base currency.
    Gain,
    Proceeds,
    Basis,
    /// `on gain`: how long the parcel was held.
    Held,
    /// `always`: the subject's balance after the change.
    Balance,
    /// Money still tied to the subject, a restricted entity.
    Remaining,
    /// The triggering flow, for `flow is #code`.
    Flow,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Field {
    Balance,
    Owner,
    Kind,
    /// From the `born` property to the context date.
    Age,
    Year,
    Month,
    Prop(Sym),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Func {
    /// Flow total through the subject's subtree in the current window,
    /// including the triggering flow, valued in the base currency. An optional
    /// argument widens it to every place of a kind the owner owns.
    Total(Dir, Window),
    Tally(Sym),
    Min,
    Max,
    Abs,
    /// Tax on an amount under a schedule's marginal brackets.
    Progressive,
    /// `value(x, UNIT)`: `x` in another commodity at the context date.
    Value,
    /// `date(y, m, d)`
    Date,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dir {
    In,
    Out,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Window {
    Month,
    Year,
    Ever,
}

/// A static type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ty {
    Amount,
    Num,
    Bool,
    Day,
    Span,
    Text,
    Name,
    Place,
    Entity,
    Kind,
    Unit,
    Schedule,
    Code,
    Glob,
    Flow,
    /// `empty`: unifies with any amount.
    Empty,
}

impl Ty {
    /// The word used in `has NAME TYPE` and in type errors.
    pub fn word(self) -> &'static str {
        match self {
            Ty::Amount => "amount",
            Ty::Num => "number",
            Ty::Bool => "bool",
            Ty::Day => "date",
            Ty::Span => "span",
            Ty::Text => "text",
            Ty::Name => "name",
            Ty::Place => "place",
            Ty::Entity => "entity",
            Ty::Kind => "kind",
            Ty::Unit => "unit",
            Ty::Schedule => "schedule",
            Ty::Code => "code",
            Ty::Glob => "pattern",
            Ty::Flow => "flow",
            Ty::Empty => "empty",
        }
    }
}

/// A runtime value. `Copy`, 24 bytes, never allocates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    Empty,
    Bool(bool),
    Num(Ratio),
    Amount(Amount),
    Day(Day),
    Span(Span),
    Text(Sym),
    Name(Sym),
    Place(Id<Place>),
    Entity(Id<Entity>),
    Kind(Id<Kind>),
    Unit(Id<Commodity>),
    Schedule(Id<Schedule>),
    Code(Sym),
    Glob(Sym),
    Flow,
    /// Why no value could be computed. Carried along, and reported only if it
    /// reaches a step.
    Fault(Fault),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    NoPrice {
        unit: Id<Commodity>,
        quote: Id<Commodity>,
    },
    /// A property the subject never set and whose kind gives no default.
    Unset(Sym),
    /// No param row at or before the day asked for, or no row for the names.
    NoRow(Id<Param>),
    DivideByZero,
    Overflow,
}

/// Which laws watch what, resolved once so the engine never searches.
///
/// A place's rule list holds, in order: its own and its ancestors' laws, its
/// kind chain's laws, and the top-level laws of its owner's jurisdictions
/// (each rule dated by the residence that brings it).
#[derive(Default)]
pub struct Rules {
    pub on_in: Groups<Place, Rule>,
    pub on_out: Groups<Place, Rule>,
    pub on_gain: Groups<Place, Rule>,
    pub always: Groups<Place, Rule>,
    /// `on spend` laws of each restricted entity.
    pub on_spend: Groups<Entity, Rule>,
    /// `each` and `by` laws, once per subject they govern.
    pub timed: Vec<Rule>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    pub law: Id<Law>,
    /// What `self` is when the law runs: the governing place for place laws,
    /// the place itself for kind laws, the resident for system laws.
    pub subject: Subject,
    /// Inclusive: the rule applies on days in `from..=until`.
    pub from: Day,
    pub until: Day,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Subject {
    Place(Id<Place>),
    Entity(Id<Entity>),
}
