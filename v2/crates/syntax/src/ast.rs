//! The syntax tree.
//!
//! It borrows the source: every name is a `&'s str` slice of the file, so
//! parsing copies no text. Expressions live in one post-order arena per file
//! (see [`Exprs`]); everything else is plain owned structure.

use axiom_core::{Day, Dec, FileId, Loc, Span};

/// One parsed source file.
#[derive(Debug)]
pub struct File<'s> {
    pub id: FileId,
    pub items: Vec<Item<'s>>,
    pub exprs: Exprs<'s>,
}

/// A written name with where it was written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Name<'s> {
    pub text: &'s str,
    pub loc: Loc,
}

/// A raw block of consecutive `///` lines, prefixes included. [`Doc::lines`]
/// strips them without allocating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Doc<'s>(pub &'s str);

impl<'s> Doc<'s> {
    /// The text of each line, `///` and one following space removed.
    pub fn lines(self) -> impl Iterator<Item = &'s str> {
        self.0.lines().map(|line| {
            let text = line.trim_start().trim_start_matches("///");
            text.strip_prefix(' ').unwrap_or(text)
        })
    }
}

/// A top-level item: a column-0 line and the indented block under it.
#[derive(Debug)]
pub struct Item<'s> {
    pub doc: Option<Doc<'s>>,
    /// The header line only, up to the end of its last token: a trailing
    /// comment and the item's block are not part of it.
    pub loc: Loc,
    pub kind: ItemKind<'s>,
}

/// What an item is, by its first word.
#[derive(Debug)]
pub enum ItemKind<'s> {
    Txn(Txn<'s>),
    Assert(Assert<'s>),
    Event(Event<'s>),
    Price(Price<'s>),
    Plan(Plan<'s>),
    Decl(Decl<'s>),
    Code(CodeRule<'s>),
    Param(Param<'s>),
    Law(Law<'s>),
    Sync(Sync<'s>),
    Setting(Setting<'s>),
}

/// One-line directives.
#[derive(Debug)]
pub enum Setting<'s> {
    /// `system PATH`: this file defines a system. Must be the first item.
    System(Name<'s>),
    /// `use PATH`
    Use(Name<'s>),
    /// `base UNIT`
    Base(Name<'s>),
    /// `relaxed`: law violations become warnings.
    Relaxed(Loc),
    /// `layout free`: folder names stop constraining dates.
    LayoutFree(Loc),
}

// ─── Journal ────────────────────────────────────────────────────────────────

/// `DATE [..DATE] FLOW`
#[derive(Debug)]
pub struct Txn<'s> {
    pub date: Day,
    /// The last day of a spread (`2026-01-01..2026-12-31`).
    pub until: Option<Day>,
    pub flow: Flow<'s>,
}

/// `SOURCE -> TARGET [@ PRICE] TAIL` with optional indented legs.
///
/// When both sides name a place there are no legs. When exactly one side does,
/// the legs are the other side ("one side split").
#[derive(Debug)]
pub struct Flow<'s> {
    pub from: Side<'s>,
    pub to: Side<'s>,
    /// The `->`.
    pub arrow: Loc,
    pub price: Option<Amount<'s>>,
    pub tail: Tail<'s>,
    pub legs: Vec<Leg<'s>>,
}

/// One end of a header: `checking`, `checking 2_000 USD`, `7 VTI`, or nothing.
#[derive(Debug)]
pub struct Side<'s> {
    pub place: Option<PlaceRef<'s>>,
    pub amount: Option<Quantity<'s>>,
}

/// A place as written, with any lot selectors: `brokerage[fifo, 2024]`.
#[derive(Debug)]
pub struct PlaceRef<'s> {
    /// A path, a unique suffix of one, an entity, or `?` (the unknown place).
    pub name: Name<'s>,
    pub select: Vec<Select<'s>>,
}

impl PlaceRef<'_> {
    /// Whether this is `?`, the place for money whose other end is not known.
    pub fn is_unknown(&self) -> bool {
        self.name.text == "?"
    }
}

/// A lot selector. Days, months and years are normalized to inclusive ranges.
#[derive(Debug)]
pub enum Select<'s> {
    Range(Day, Day, Loc),
    /// A `#code`: the name is without the `#`, its location includes it.
    Code(Name<'s>),
    Policy(Policy, Loc),
}

/// How parcels are chosen when several could leave.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Policy {
    Fifo,
    Lifo,
    Hifo,
    Prorata,
}

/// How much moves on one side or leg.
#[derive(Debug)]
pub enum Quantity<'s> {
    /// `84.20 USD`, or `empty`.
    Fixed(Amount<'s>),
    /// `(350 USD)`: written, not yet real. The amount's location includes the
    /// parentheses.
    Pending(Amount<'s>),
    /// `? USD`: inferred from surrounding balance assertions.
    Unknown { unit: Name<'s>, loc: Loc },
    /// `...`: whatever balances the transaction.
    Rest(Loc),
    /// `= 5_000 USD`: whatever makes the place's balance equal this after the flow.
    Target(Amount<'s>),
    /// `all`: everything the selected parcels hold.
    All(Loc),
}

/// `NUMBER UNIT`, or `empty`. Written numbers carry no sign.
#[derive(Clone, Copy, Debug)]
pub struct Amount<'s> {
    pub num: Dec,
    /// `None` exactly for `empty`, the zero of every commodity.
    pub unit: Option<Name<'s>>,
    pub loc: Loc,
}

/// An indented line of a split: `retirement 800 USD #pretax`.
#[derive(Debug)]
pub struct Leg<'s> {
    pub doc: Option<Doc<'s>>,
    pub place: PlaceRef<'s>,
    pub amount: Quantity<'s>,
    pub price: Option<Amount<'s>>,
    pub tail: Tail<'s>,
    pub loc: Loc,
}

/// What may follow a flow or leg: `/ payee #code #code ! "reason"`.
#[derive(Default, Debug)]
pub struct Tail<'s> {
    pub payee: Option<Name<'s>>,
    /// Each code is without its `#`; its location includes it.
    pub codes: Vec<Name<'s>>,
    pub waive: Option<Waive<'s>>,
}

/// `!` or `! "reason"`: accept this item's law violations (or, on an
/// assertion, its gap) explicitly.
#[derive(Clone, Copy, Debug)]
pub struct Waive<'s> {
    pub loc: Loc,
    pub reason: Option<&'s str>,
}

/// `DATE PLACE = AMOUNT [!]`, checked at the end of the day.
#[derive(Debug)]
pub struct Assert<'s> {
    pub date: Day,
    pub place: PlaceRef<'s>,
    pub amount: Amount<'s>,
    pub waive: Option<Waive<'s>>,
}

/// `DATE #code settled|void|returned`
#[derive(Debug)]
pub struct Event<'s> {
    pub date: Day,
    /// Without the `#`; the location includes it.
    pub code: Name<'s>,
    pub state: EventState,
    pub state_loc: Loc,
}

/// What a `DATE #code STATE` line does to the flows carrying that code.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EventState {
    /// Pending becomes actual on this day.
    Settled,
    /// Pending never happens.
    Void,
    /// Actual is reversed on this day.
    Returned,
}

/// `DATE UNIT PRICE`: one `unit` costs `price` on that day.
#[derive(Debug)]
pub struct Price<'s> {
    pub date: Day,
    pub unit: Name<'s>,
    pub price: Amount<'s>,
}

/// `every CADENCE [on DAY] [from DATE] [until DATE|MONTH] FLOW`
#[derive(Debug)]
pub struct Plan<'s> {
    /// `month` is one month, `2w` fourteen days, `quarter` three months.
    pub every: Span,
    pub on: Option<On>,
    pub from: Option<Day>,
    /// Inclusive. A month bound is normalized to that month's last day.
    pub until: Option<Day>,
    pub flow: Flow<'s>,
}

/// The day within each period a plan falls on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    /// `on 15`: past the month's end clamps to its last day.
    MonthDay(u8),
    /// `on 04-15`
    YearDay { month: u8, day: u8 },
    /// `on monday`: Monday = 0 … Sunday = 6, as [`Day::weekday`].
    Weekday(u8),
}

// ─── Declarations ───────────────────────────────────────────────────────────

/// `account|entity|commodity|kind NAME [: KIND]` with indented properties and laws.
#[derive(Debug)]
pub struct Decl<'s> {
    pub what: DeclKind,
    pub name: Name<'s>,
    /// After `:`: the kind (or, for `kind`, the parent kind).
    pub kind: Option<Name<'s>>,
    pub props: Vec<Prop<'s>>,
    pub laws: Vec<Law<'s>>,
}

/// Which keyword introduced a declaration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DeclKind {
    Account,
    Entity,
    Commodity,
    Kind,
}

/// `NAME ARG*`: arguments are primary expressions, commas skipped.
/// `has born date`, `budget 500 USD monthly`, `lives us/ca from 2026-01-01`.
#[derive(Debug)]
pub struct Prop<'s> {
    pub name: Name<'s>,
    pub args: Vec<ExprId>,
    pub loc: Loc,
}

/// `code GLOB` with indented `on PLACE-GLOB | KIND` lines.
#[derive(Debug)]
pub struct CodeRule<'s> {
    pub pattern: Name<'s>,
    pub on: Vec<Name<'s>>,
}

/// `param NAME` with indented `KEY+ VALUE` rows.
#[derive(Debug)]
pub struct Param<'s> {
    pub name: Name<'s>,
    pub rows: Vec<ParamRow<'s>>,
}

/// `KEY+ VALUE`: the value is a schedule or any expression.
#[derive(Debug)]
pub struct ParamRow<'s> {
    pub keys: Vec<Key<'s>>,
    pub value: ExprId,
    pub loc: Loc,
}

/// One key of a parameter row.
#[derive(Clone, Copy, Debug)]
pub enum Key<'s> {
    /// Step lookup: the latest year at or before the one asked for.
    Year(i32, Loc),
    Date(Day, Loc),
    Name(Name<'s>),
}

/// `sync FILE` with an indented `run COMMAND…` line (raw text).
#[derive(Debug)]
pub struct Sync<'s> {
    pub file: Name<'s>,
    pub run: Name<'s>,
}

// ─── Laws ───────────────────────────────────────────────────────────────────

/// `law NAME` with an indented trigger and steps.
#[derive(Debug)]
pub struct Law<'s> {
    /// For a top-level law this is also the [`Item::doc`].
    pub doc: Option<Doc<'s>>,
    pub name: Name<'s>,
    pub trigger: Trigger,
    pub trigger_loc: Loc,
    pub steps: Vec<Step<'s>>,
    /// The `law NAME` line.
    pub loc: Loc,
}

/// When a law applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    In,
    Out,
    Gain,
    Spend,
    Each(Period),
    By(ExprId),
    Always,
}

/// The period a law's `each` trigger ends.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Period {
    Month,
    Year,
}

/// One line of a law's body after its trigger. Steps run top to bottom.
#[derive(Debug)]
pub struct Step<'s> {
    pub loc: Loc,
    pub kind: StepKind<'s>,
}

#[derive(Debug)]
pub enum StepKind<'s> {
    /// A filter: the law stops silently when false.
    When(ExprId),
    Let(Name<'s>, ExprId),
    /// `require EXPR [else EFFECT] ["message"]`, or `warn EXPR ["message"]`.
    Require { cond: ExprId, otherwise: Option<Effect<'s>>, message: Option<Name<'s>>, warn: bool },
    Effect(Effect<'s>),
}

/// What a law does to the world: an obligation, or a tally.
#[derive(Debug)]
pub enum Effect<'s> {
    /// `owe EXPR to ENTITY [by EXPR] [as NAME]`
    Owe { amount: ExprId, to: Name<'s>, due: Option<ExprId>, name: Option<Name<'s>> },
    /// `count EXPR as NAME`
    Count { amount: ExprId, name: Name<'s> },
}

// ─── Expressions ────────────────────────────────────────────────────────────

/// Index of an expression in its file's [`Exprs`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExprId(pub u32);

impl ExprId {
    /// The position in [`Exprs`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A file's expressions in post-order: children precede their parent, so every
/// subtree is the contiguous range `first..=root`. Later phases keep this
/// shape, which makes type checking and evaluation a single forward scan in
/// which every subexpression's result is already at hand.
#[derive(Default, Debug)]
pub struct Exprs<'s> {
    nodes: Vec<Expr<'s>>,
}

/// One node of an expression: what it is, where it was written, and where its
/// subtree starts.
#[derive(Debug)]
pub struct Expr<'s> {
    pub kind: ExprKind<'s>,
    pub loc: Loc,
    /// The first node of this expression's subtree.
    pub first: ExprId,
}

impl<'s> Exprs<'s> {
    /// Appends a node whose children were all pushed since `first`.
    pub fn push(&mut self, kind: ExprKind<'s>, loc: Loc, first: ExprId) -> ExprId {
        let id = ExprId(self.nodes.len() as u32);
        debug_assert!(first <= id, "a subtree starts at or before its root");
        self.nodes.push(Expr { kind, loc, first });
        id
    }

    /// The id the next pushed node will get: where a new subtree starts.
    pub fn next(&self) -> ExprId {
        ExprId(self.nodes.len() as u32)
    }

    /// The nodes of `root`'s subtree, in evaluation order, ending with `root`.
    pub fn subtree(&self, root: ExprId) -> &[Expr<'s>] {
        &self.nodes[self.nodes[root.index()].first.index()..=root.index()]
    }

    /// How many nodes the file has in all.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl<'s> std::ops::Index<ExprId> for Exprs<'s> {
    type Output = Expr<'s>;
    fn index(&self, id: ExprId) -> &Expr<'s> {
        &self.nodes[id.index()]
    }
}

/// What an expression node is. Children are ids of earlier nodes.
#[derive(Debug)]
pub enum ExprKind<'s> {
    /// `24_500`, `0.5`
    Num(Dec),
    /// `10%`, `3.5%`: the written number, not yet divided by 100.
    Pct(Dec),
    /// `24_500 USD`
    Amount(Dec, Name<'s>),
    Date(Day),
    Span(Span),
    /// String contents between the quotes, escapes not yet processed.
    Str(&'s str),
    Empty,
    /// Lowercase identifiers, paths and globs: `year`, `self`, `wages`,
    /// `expenses/food/*`, `401k`.
    Name(&'s str),
    /// `USD`
    Unit(&'s str),
    /// `#house`, as `house`.
    Code(&'s str),
    Field(ExprId, Name<'s>),
    /// `limit[year]`, `ordinary[year, owner.filing]`
    Index(ExprId, Box<[ExprId]>),
    /// `total(in, year)`, `progressive(ordinary[year], x)`
    Call(Name<'s>, Box<[ExprId]>),
    Unary(UnOp, ExprId),
    Binary(BinOp, ExprId, ExprId),
    /// `x is 401k | ira`: true when `x` matches any alternative.
    Is(ExprId, Box<[ExprId]>),
    /// `if c then a else b`
    If(ExprId, ExprId, ExprId),
    /// `0 USD 10% | 12_400 USD 12% | …`: thresholds and marginal rates.
    Schedule(Box<[(ExprId, ExprId)]>),
}

/// Prefix operators: `-x` and `not x`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UnOp {
    Neg,
    Not,
}

/// Infix operators, loosest binding first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

impl BinOp {
    /// The operator as written.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "or",
            BinOp::And => "and",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        }
    }
}
