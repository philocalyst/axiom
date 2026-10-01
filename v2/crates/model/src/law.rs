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

use axiom_core::calendar;
use axiom_core::day::days_in_month;
use axiom_core::{Day, Days, Dim, Groups, Id, Loc, Period, Ratio, Severity, Span, Sym};

use crate::book::{
    Amount, Asset, Budget, Commodity, Contract, Entity, Kind, Param, Place, Purpose, Schedule,
    System, Text,
};
use crate::journal::Object;

pub use axiom_syntax::BinOp;

pub struct Law {
    pub name: Sym,
    pub doc: Option<Sym>,
    pub owner: Owner,
    /// The system that declared it: tallies it counts and params it reads
    /// are scoped here.
    pub system: Option<Id<System>>,
    pub trigger: Trigger,
    /// The `budget` item that reports through this law: its cap reads the
    /// budget's limits and `carries` rather than a constant.
    pub budget: Option<Id<Budget>>,
    /// `overrides NAME`: the law it replaces where both govern.
    pub overrides: Option<Id<Law>>,
    /// The source name, retained until every law (including nested contract
    /// laws) has been indexed and override references can resolve forward.
    pub override_name: Option<Sym>,
    /// Specificity, for conflicts (LANGUAGE §8): thing over kind over parent
    /// kind, project over child system over parent system. Computed at build
    /// time.
    pub rank: Rank,
    pub steps: Box<[Step]>,
    pub nodes: Box<[Node]>,
    pub loc: Loc,
}

/// How specific a law is: the greater wins a conflict.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Rank {
    class: RankClass,
    depth: u32,
}

/// Lexicographic owner scope, then ancestry depth. A deeper hierarchy can
/// never overtake a more specific owner class.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum RankClass {
    System,
    Book,
    Kind,
    Purpose,
    Explicit,
    Contract,
}

impl Rank {
    /// Placeholder before law registration calculates scope and depth.
    pub const ZERO: Rank = Rank {
        class: RankClass::System,
        depth: 0,
    };

    pub(crate) const fn scoped(class: RankClass, depth: u32) -> Rank {
        Rank { class, depth }
    }
}

#[cfg(test)]
mod rank_tests {
    use super::{Rank, RankClass};

    #[test]
    fn owner_specificity_is_lexicographic_and_never_saturates() {
        assert!(Rank::scoped(RankClass::System, 8) > Rank::scoped(RankClass::System, 7));
        assert!(Rank::scoped(RankClass::Book, 0) > Rank::scoped(RankClass::System, u32::MAX));
        assert!(Rank::scoped(RankClass::Kind, u32::MAX) < Rank::scoped(RankClass::Purpose, 0));
        assert!(Rank::scoped(RankClass::Purpose, u32::MAX) < Rank::scoped(RankClass::Explicit, 0));
        assert!(Rank::scoped(RankClass::Explicit, 0) < Rank::scoped(RankClass::Contract, 0));
    }
}

impl Law {
    /// The nodes of `root`'s expression, in evaluation order: `first..=root`.
    pub fn range(&self, root: NodeId) -> std::ops::RangeInclusive<usize> {
        self.nodes[root.index()].first.index()..=root.index()
    }

    /// The cap this law is, if all it says is that a flow total stays under a
    /// written limit: `warn total(in, month) <= 500 USD`, which is what
    /// `budget 500 USD monthly` means. A total is read straight from the
    /// ledger, so a cap that holds takes no evaluating.
    pub fn cap(&self) -> Option<Cap> {
        let [
            Step {
                kind: StepKind::Require {
                    cond, otherwise, ..
                },
                ..
            },
        ] = &*self.steps
        else {
            return None;
        };
        if !otherwise.is_empty() {
            return None;
        }
        let Op::Bin(cmp @ (BinOp::Le | BinOp::Lt), total, limit) = self.nodes[cond.index()].op
        else {
            return None;
        };
        match (&self.nodes[total.index()].op, &self.nodes[limit.index()].op) {
            // A kind among the arguments widens the total to every place of that kind.
            (Op::Call(Func::Total(dir, window), args), Op::Const(Value::Amount(limit)))
                if args
                    .iter()
                    .all(|arg| self.nodes[arg.index()].ty != Ty::Kind) =>
            {
                Some(Cap {
                    dir: *dir,
                    window: *window,
                    limit: *limit,
                    strict: cmp == BinOp::Lt,
                })
            }
            _ => None,
        }
    }
}

/// `total(dir, window) <= limit`, or `<` when `strict`: see [`Law::cap`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cap {
    pub dir: Dir,
    pub window: Window,
    pub limit: Amount,
    pub strict: bool,
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
    /// Governs every flow of the purpose and those beneath it.
    Purpose(Id<Purpose>),
    /// Governs the asset, part by part.
    Asset(Id<Asset>),
    /// Governs the contract's flows.
    Contract(Id<Contract>),
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
    /// A flow of the governed purpose (and those beneath it), or, under an
    /// asset or an asset kind, a flow whose purpose is `of` it.
    Flow,
    /// At the end of each period, or on its `closing` day in the next one.
    Each(Period, Option<Closing>),
    /// Fires once the journal reaches this date, evaluated per subject.
    By(NodeId),
    Always,
}

/// `each year closing 04-15`: the law runs for a year on this day of the year
/// after, so that what is recognized `for` the year until then counts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Closing {
    pub month: u8,
    pub day: u8,
}

impl Closing {
    /// The day the law judges `year`: this day of the next one. February 29
    /// falls on the 28th in a year that has no 29th.
    pub fn day_for(self, year: i32) -> Option<Day> {
        let (next, month) = (year + 1, u32::from(self.month));
        Day::from_ymd(
            next,
            month,
            u32::from(self.day).min(days_in_month(next, month)),
        )
    }
}

#[derive(Debug)]
pub struct Step {
    pub loc: Loc,
    pub kind: StepKind,
}

#[derive(Debug)]
pub enum StepKind {
    When(NodeId),
    /// An exception the law itself knows: while it holds, the law does not apply.
    Unless(NodeId),
    /// The bound value is the node's; later nodes read it with [`Op::Local`].
    Let(NodeId),
    Require {
        cond: NodeId,
        /// `else B else C`: reparations, in order.
        otherwise: Box<[Effect]>,
        message: Option<Sym>,
        /// `warn` is a `require` whose failure costs nothing.
        severity: Severity,
    },
    Effect(Effect),
}

#[derive(Debug)]
pub enum Effect {
    /// An obligation from the subject's owner to `to`, due by `due` (default:
    /// the triggering day). `name` defaults to the law's name.
    Owe {
        amount: NodeId,
        to: Id<Entity>,
        due: Option<NodeId>,
        name: Sym,
    },
    /// Adds to a tally keyed by owner, year, name and the law's system.
    Count { amount: NodeId, name: Sym },
    /// Lowers the governed asset part's basis (depreciation, depletion).
    Consume { amount: NodeId },
    /// Holds a disallowed loss and adds it to the basis of the nearest
    /// acquisition of `unit` within the span `within`, before or after (a wash
    /// sale).
    Carry {
        amount: NodeId,
        unit: NodeId,
        within: NodeId,
    },
}

/// Index of a node in its law's arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub op: Op,
    /// Statically checked: evaluation never meets a type it did not expect.
    pub ty: Ty,
    pub loc: Loc,
    /// The first node of this node's subtree.
    pub first: NodeId,
}

#[derive(Clone, PartialEq, Debug)]
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
    /// Attach an identified object to a purpose (`repair of self`).
    Of(NodeId, NodeId),
    /// Price a quantity (`44 MI @ 0.70 USD/MI`).
    At(NodeId, NodeId),
    /// Select the amount of the current contract occurrence's materialized
    /// native groups (`50% of [retirement]`). Keys are resolved while the book
    /// is built; evaluation only filters the already available groups.
    Select(Box<[SelectKey]>),
    Neg(NodeId),
    Not(NodeId),
    Bin(BinOp, NodeId, NodeId),
    /// True when the left value matches any alternative: a kind, place,
    /// entity, glob or code.
    Is(NodeId, Box<[NodeId]>),
    If(NodeId, NodeId, NodeId),
}

/// A compile-resolved selector over a contract occurrence's native groups.
/// Multiple keys are conjunctive and retain their source order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectKey {
    Purpose(Id<Purpose>),
    Code(Sym),
    Unit(Id<Commodity>),
    End(Id<Place>),
    Range(Days),
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
    /// What the triggering flow is for.
    Purpose,
    /// The triggering flow's words: `"food for the routine"`.
    Description,
    /// An input bound by one contract occurrence, indexed by its declaration
    /// order in `Terms.inputs`.
    Input(u16),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Field {
    Balance,
    /// A place's total basis, in the base currency.
    Basis,
    /// An amount's commodity.
    Unit,
    Owner,
    Kind,
    /// From the `born` property to the context date.
    Age,
    /// An asset's cost: the sum of its parts' costs, in the base currency.
    Cost,
    /// The day an asset's first part was acquired.
    InService,
    /// How many parts an asset has.
    Parts,
    /// A purpose's object: `purpose.of`.
    Of,
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
    /// Total of a purpose in the current window. `None` means the purpose
    /// whose law is running; a value names an explicit purpose.
    PurposeTotal {
        purpose: Option<Id<Purpose>>,
        window: Window,
    },
    /// Open amount of claims carrying this stable source code.
    Open(Sym),
    /// What laws counted under the name for the owner, in the current year or,
    /// with a second argument (a year or a date), in that one.
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
    Peak,
    Low,
    Days,
    /// `straight-line(cost, life, from, period [, mid-month])`: this period's
    /// share of a cost written off evenly over a life.
    StraightLine,
}

impl Func {
    /// The year a `tally` call asks for: the operand after the tally's name, if
    /// there is one.
    pub fn tally_year(args: &[NodeId]) -> Option<NodeId> {
        args.get(1).copied()
    }
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

impl Window {
    /// The days this window is on the day `day` falls in: its calendar month or
    /// year, or all of time.
    pub fn around(self, day: Day) -> Days {
        let period = match self {
            Window::Month => Period::Month,
            Window::Year => Period::Year,
            Window::Ever => return Days::ALWAYS,
        };
        calendar::Window::containing(period, day).days()
    }
}

/// A static type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ty {
    /// In a dimension: what it is counted in, known before anything runs.
    Amount(Dim<Id<Commodity>>),
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
    Purpose,
    Asset,
    Schedule,
    Code,
    Glob,
    Flow,
    /// `empty`: unifies with any amount.
    Empty,
}

impl Ty {
    /// An amount of some commodity. v3 bridge: the v3 compiler knows no units,
    /// so every amount it types is this, and mixing them is never an error.
    pub const AMOUNT: Ty = Ty::Amount(Dim::Any);

    /// The word used in `has NAME TYPE` and in type errors.
    pub fn word(self) -> &'static str {
        match self {
            Ty::Amount(_) => "amount",
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
            Ty::Purpose => "purpose",
            Ty::Asset => "asset",
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
    Text(Text),
    Name(Sym),
    Place(Id<Place>),
    Entity(Id<Entity>),
    Kind(Id<Kind>),
    Unit(Id<Commodity>),
    /// A purpose, with the object it was written `of`, if any.
    Purpose(Id<Purpose>, Option<Object>),
    Asset(Id<Asset>),
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
    /// A runtime amount disagrees with the commodity the expression declared.
    UnitMismatch { found: Id<Commodity>, expected: Id<Commodity> },
    /// A property the subject never set and whose kind gives no default.
    Unset(Sym),
    /// A contract template input was not bound for this occurrence.
    MissingInput(u16),
    /// No param row at or before the day asked for, or no row for the names.
    NoRow(Id<Param>),
    DivideByZero,
    Overflow,
}

/// Which laws watch what, resolved once so the engine never searches.
///
/// A place's rule list holds its own and its ancestors' laws, its kind chain's
/// laws, and the top-level laws of its owner's jurisdictions (each rule dated
/// by the residence that brings it; a household's for its members' places).
/// Every list, `timed` included, is in dependency order: a law that reads
/// `tally(x)` comes after every law that counts into `x`, and declaration
/// order decides the rest.
#[derive(Default)]
pub struct Rules {
    pub on_in: Groups<Place, Rule>,
    pub on_out: Groups<Place, Rule>,
    pub on_gain: Groups<Place, Rule>,
    pub always: Groups<Place, Rule>,
    /// `on spend` laws of each restricted entity.
    pub on_spend: Groups<Entity, Rule>,
    /// `on flow` laws of each purpose, ancestors' included, in dependency order.
    pub purposes: Groups<Purpose, Rule>,
    /// `on flow` laws of each asset's place (its kind chain's and its own):
    /// flows whose purpose is `of` it.
    pub about: Groups<Place, Rule>,
    /// Laws of a contract, indexed separately from its party so two promises
    /// with one party keep independent scope and accounting.
    pub contracts: Groups<Contract, Rule>,
    /// `each` and `by` laws, once per subject they govern.
    pub timed: Vec<Rule>,
}

impl Rules {
    /// The four tables of laws that watch a place: `on in`, `on out`, `on gain`
    /// and `always`.
    pub fn per_place(&self) -> [&Groups<Place, Rule>; 4] {
        [&self.on_in, &self.on_out, &self.on_gain, &self.always]
    }

    /// Every rule that runs while the fold does, in each list it is in: the
    /// per-place tables, `on spend`, and the timed rules.
    pub fn all(&self) -> impl Iterator<Item = &Rule> {
        let per_place = self
            .per_place()
            .into_iter()
            .flat_map(|table| table.values());
        per_place
            .chain(self.on_spend.values())
            .chain(self.purposes.values())
            .chain(self.about.values())
            .chain(self.contracts.values())
            .chain(&self.timed)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    pub law: Id<Law>,
    /// What `self` is when the law runs: the governing place for place laws,
    /// the place itself for kind laws, the resident for system laws, and the
    /// flow's owner for purpose laws.
    pub subject: Subject,
    /// The days the rule applies on.
    pub days: Days,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Subject {
    Place(Id<Place>),
    Entity(Id<Entity>),
    Asset(Id<Asset>),
    Contract(Id<Contract>),
}
