// Types for the third pass of v4 (LANGUAGE 5f15e35): promises, measures, units,
// `also`, norms, systems' currencies, filed returns, declared sync. Written by
// the orchestrator; applied by lane C5 after types-v4b.rs. NEW = add,
// CHANGED = replace. Where a type below cannot be applied as written, make the
// smallest change that works and say so.

// ════════════════════════════════════════════════════════════════════════════
// core/src/unit.rs  (NEW): the unit algebra, as far as books use it
// ════════════════════════════════════════════════════════════════════════════

/// What an amount is counted in, for type checking (LANGUAGE §8): a
/// commodity, a rate between two (`USD/MI`, the price `USD/VTI`), a momentum
/// (`USD` a month), or a pure number. Kennedy's free abelian group, restricted to
/// the shapes books use; anything deeper is a type error, not a unit.
/// Generic over the commodity id so core stays below the model.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dim<C> {
    /// `%`, a count, a fraction.
    Number,
    /// `USD`, `VTI`, `MI`.
    Of(C),
    /// `USD/MI`: multiplied by `MI` it is `USD`.
    Per(C, C),
    /// `USD` a month: what a promise's schedule amount is.
    Rate(C, Period),
    /// "Some commodity", known only when the flow is: `amount` where the subject
    /// may hold several. Mixing it with a fixed one needs `value(x, U)`.
    Any,
}

impl<C: Copy + Eq> Dim<C> {
    /// `a + b`, `a - b`, `a < b`: the same dimension, or `None`.
    pub fn add(self, other: Dim<C>) -> Option<Dim<C>>;
    /// `a * b`: `Per(u, m) * Of(m) = Of(u)`, `Number * x = x`, …
    pub fn mul(self, other: Dim<C>) -> Option<Dim<C>>;
    pub fn div(self, other: Dim<C>) -> Option<Dim<C>>;
}

// ════════════════════════════════════════════════════════════════════════════
// model: promises
// ════════════════════════════════════════════════════════════════════════════

/// CHANGED (types-v4b `Terms`): what a contract says for a while.
pub struct Terms {
    pub every: Cadence,
    /// Several days are each due (`yearly on 04-15, 06-15, 09-15, 01-15`).
    pub on: Box<[On]>,
    pub anchor: Day,
    /// One occurrence's flows. An item reading an input the occurrence does not
    /// state is left out; an input it does state is bound for that occurrence.
    pub template: Box<[Flow]>,
    /// `input water USD`: names occurrences may state (`water = 155.00 USD`).
    pub inputs: Box<[Input]>,
    pub estimate: bool,
    /// `due 5d else + 5% #late-fee`.
    pub due: Option<Deadline>,
    /// How late an occurrence may come and still keep its due day. Default:
    /// half a cadence.
    pub grace: Span,
    /// `for last month`: each occurrence recognized over a period relative to
    /// its day.
    pub period: Option<Relative>,
    pub covers: Option<Coverage>,
    /// `prorated`: an occurrence that starts or ends inside its period is that
    /// share of it, by days.
    pub prorated: bool,
    /// `rising 3% yearly`, `indexed to cpi yearly`.
    pub escalation: Option<Escalation>,
    pub shares: Box<[Share]>,
    /// `also …` lines of this contract (see `Also`).
    pub also: Box<[Id<Also>]>,
    /// A loan's yearly rate while these terms hold.
    pub rate: Option<Ratio>,
    pub change: Option<Change>,
}

pub struct Input { pub name: Sym, pub unit: Option<Id<Commodity>>, pub loc: Loc }

/// A deadline after the due day, and what its passing adds.
pub struct Deadline {
    pub after: Span,
    /// The `else` item, compiled like a template item; `None` if the deadline
    /// only makes the claim late.
    pub otherwise: Option<Box<Flow>>,
}

/// `for last month|last quarter|last year`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relative { Last(Period), LastQuarter }

/// `covers the month` (the calendar period containing the due day) or
/// `covers 6m` (a span from it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Coverage { Calendar(Period), Quarter, Span(Span) }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Escalation {
    /// `rising 3% yearly`: from each anniversary of `Contract.days.first()`.
    Rising(Ratio),
    /// `indexed to cpi yearly`: scaled by the param's ratio between anniversaries.
    Indexed(Id<Param>),
}

/// CHANGED: `Loan` gains the ACTUS pieces.
pub struct Loan {
    pub principal: Amount,
    pub on: Day,
    pub term: Span,
    pub asset: Option<Id<Asset>>,
    pub debt: Id<Place>,
    /// `resets yearly from 2029-03-01 to sofr + 2.75% cap 2% life 5%`.
    pub resets: Option<Reset>,
    /// What a flow to the contract does: the default `Shortens`.
    pub prepay: Prepay,
}

pub struct Reset {
    pub every: Span,
    pub from: Day,
    pub index: Id<Param>,
    pub margin: Ratio,
    /// Largest change at one reset, and over the life, as rates.
    pub cap: Option<Ratio>,
    pub life: Option<Ratio>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Prepay { #[default] Shortens, Recasts }

/// CHANGED: a share's weight is resolved to a rate at build time; `measure`
/// keeps what it was written as, for `why` (`120 SQFT of 1,000`).
pub struct Share {
    pub rate: Ratio,
    /// An owner bears it (an allocation); a party owes it (a claim).
    pub entity: Id<Entity>,
    pub measure: Option<(Amount, Amount)>,
    pub loc: Loc,
}

/// NEW. `also ITEM | FLOW [when EXPR]` (LANGUAGE §10): what every matching
/// flow implies, declared once. Escrow and matches are `also` lines.
pub struct Also {
    pub on: AlsoOn,
    pub what: Implied,
    /// Compiled like a law's `when`: nodes of `law`.
    pub when: Option<NodeId>,
    /// The expressions (amounts, `when`) live in this law's node arena, with no
    /// steps: one expression language for laws and declarations.
    pub law: Id<Law>,
    pub purpose: Option<Purposed>,
    pub description: Option<Sym>,
    pub loc: Loc,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlsoOn {
    Contract(Id<Contract>),
    Entity(Id<Entity>),
    Kind(Id<Kind>),
    Purpose(Id<Purpose>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Implied {
    /// `+ 5%`, `- 2.9% + 0.30 USD`: an item of the flow, between its ends.
    Item { sign: Sign, amount: NodeId },
    /// `lumen -> retirement 50% of …`, `-> escrow 410 USD`: a flow of its own.
    /// `None` ends mean the implying flow's own ends (`issuer -> self`).
    Flow { from: Option<End>, to: Option<End>, amount: NodeId },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign { Carve, Add, Less }

// Book gains `pub also: Arena<Also>` and `Rules`-style indexes by `AlsoOn`.

// ════════════════════════════════════════════════════════════════════════════
// model: measures, readings, filed returns
// ════════════════════════════════════════════════════════════════════════════

/// NEW. `12 me worked 6.5 HR for halcyon ^inv-12`, `21 car used 44 MI
/// #business-travel for studio` (LANGUAGE §5): an event that moves nothing.
/// Purpose laws fire on it, and `total` counts it in its unit.
pub struct Measure {
    pub day: Day,
    pub action: Action,
    /// Who worked, or what was used.
    pub subject: Subject,
    /// In a unit of a `measure` kind.
    pub quantity: Amount,
    /// Whom it was for: its owner, as a flow's.
    pub owner: Id<Entity>,
    /// A party it was done for (`for halcyon`).
    pub party: Option<Id<Entity>>,
    pub purpose: Option<Purposed>,
    pub description: Option<Sym>,
    pub codes: Box<[Sym]>,
    pub loc: Loc,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action { Work, Use }

// `Book.measures: Arena<Measure>`, sorted by day.

/// NEW. `01 ^bldg-water = 155.00 USD`: a named value on a day, for references.
pub struct Reading { pub day: Day, pub code: Sym, pub amount: Amount, pub loc: Loc }
// `Book.readings: Vec<Reading>`, sorted by code then day.

/// NEW. `2026-04-15 us filed 2025` with its tally lines (LANGUAGE §11).
pub struct Filed {
    pub day: Day,
    pub system: Id<System>,
    pub year: i32,
    pub owner: Id<Entity>,
    pub lines: Box<[(Sym, Amount, Loc)]>,
    pub loc: Loc,
}
// `Book.filed: Vec<Filed>`.

// ════════════════════════════════════════════════════════════════════════════
// model: flows
// ════════════════════════════════════════════════════════════════════════════

/// CHANGED: `Detail` (the flow's rare facts) gains
    /// `against ^code`: the transaction it refunds or reimburses.
    pub against: Option<Id<Txn>>,
    /// How a computed amount was reckoned (`12% of ^bldg-water`), for `why` and
    /// hints: the share and what it was of, with where that was stated.
    pub reckoned: Option<Reckoning>,

pub struct Reckoning { pub rate: Ratio, pub of: Amount, pub from: Loc }

pub enum Derivation {
    // … as in v4b, and:
    /// NEW: an `also` line.
    Also(Id<Also>),
    /// NEW: a deadline's `else`, when it passed (a late fee).
    Otherwise(Id<Contract>),
    /// NEW: a law's reparation (`require … else …`).
    Reparation(Id<Law>),
    /// NEW: the unused part of a `covers` promise that ended early.
    Refund(Id<Contract>),
    // REMOVED: `Escrow` and `Match` (they are `Also`).
}

/// CHANGED: `PurposeRoot` gains `Transfer`: what only passes through the owners.
pub enum PurposeRoot { Income, Spending, Capital, Transfer }

// ════════════════════════════════════════════════════════════════════════════
// model: entities, kinds, assets, budgets, systems
// ════════════════════════════════════════════════════════════════════════════

/// CHANGED: `Entity` gains
    /// `owner me 60%, theo 40%` on a business: its tallies reach them in these
    /// shares. Empty for a sole owner (`owner`).
    pub owned_by: Box<[Share]>,
    /// Its own `currency`, else its residence's system's, else the book's base:
    /// resolved at build time.
    pub currency: Id<Commodity>,
    pub citizen: Box<[Id<System>]>,
    pub books: Books,

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Books { #[default] Cash, Accrual }

/// CHANGED: `Place` gains `shares: Box<[Share]>` (`owner me 50%, jordan 50%` on
/// an account). `owner` stays the first.

/// CHANGED: `Asset` gains `part_of: Option<Id<Asset>>`. Its measures (`area`)
/// are props.

/// CHANGED: `Budget` gains `funded: Option<(Id<Place>, Id<Place>)>`.

/// CHANGED: `System` gains
    pub currency: Option<Id<Commodity>>,
    pub rates: Option<RatePolicy>,

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RatePolicy { Spot, Param(Id<Param>) }

/// CHANGED: `Param` gains `unit: Option<Dim<Id<Commodity>>>` (`param
/// mileage-rate USD/MI`).

// ════════════════════════════════════════════════════════════════════════════
// model: laws
// ════════════════════════════════════════════════════════════════════════════

/// CHANGED: `Law` gains
    /// `overrides NAME`.
    pub overrides: Option<Id<Law>>,
    /// Specificity, for conflicts (LANGUAGE §8): thing > kind > parent kind,
    /// project > child system > parent system. Computed at build time.
    pub rank: Rank,

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Rank(pub u16);

/// CHANGED: `StepKind` gains `Unless(NodeId)`, and `Require` becomes
    Require {
        cond: NodeId,
        /// `else B else C`: reparations, in order.
        otherwise: Box<[Effect]>,
        message: Option<Sym>,
        severity: Severity,
    },

// `Severity` is core's (`axiom_core::diag`), if it has the two levels; else add them there.

/// CHANGED: `Ty::Amount` carries its dimension: `Ty::Amount(Dim<Id<Commodity>>)`.

// ════════════════════════════════════════════════════════════════════════════
// model: sync
// ════════════════════════════════════════════════════════════════════════════

/// CHANGED (types-v4b `Source`):
pub struct Source {
    pub name: Sym,
    pub fetch: Fetch,
    pub format: Option<Id<Format>>,
    pub sink: Sink,
    pub system: Option<Id<System>>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// (Not `Origin`, which is a flow's.)
pub enum Fetch {
    /// `read "imports/chase-*.csv"`.
    Read(Sym),
    /// `run COMMAND`, with `{since}`, `{today}`, `{units}`, `{year}` unexpanded.
    Run(Sym),
}

/// NEW. `format NAME`, or a `format csv` block inline in a sync.
pub struct Format {
    pub name: Sym,
    pub shape: Shape,
    pub loc: Loc,
}

pub enum Shape {
    Csv(Csv),
    /// OFX, ISO 20022: named records, fields by path (`BookgDt/Dt`).
    Tagged { records: Sym, fields: Box<[(Field, Box<[Sym]>)]> },
}

/// CHANGED (types-v4b `Csv`): `columns: Box<[(Field, Column)]>`, with
/// `date_format`, and category mappings `Box<[(Sym, Id<Purpose>)]>`.
pub enum Field {
    Date, Amount, Debit, Credit, Memo, Balance, Pending, Code, Id, Party, Gross, Fee,
    Currency, Category, Object, Route, Via,
}

/// NEW. A compiled parsing expression (LANGUAGE §14): matched anywhere in a
/// memo, in any case. `known_as: Box<[Id<Pattern>]>` on `Entity` and `Place`
/// (CHANGED from `Box<[Sym]>`); `CodeRule` gains `known_as`.
pub struct Pattern {
    pub name: Option<Sym>,
    /// A program for a small matching machine: literals, classes, sequence,
    /// ordered choice, repetition, captures. Built from the syntax once.
    pub program: Box<[Op]>,
    pub loc: Loc,
}

pub enum Op {
    /// Case-insensitive literal (stored uppercased).
    Literal(Sym),
    Class(CharClass),
    /// Try the next `len` ops; on failure jump past them to the alternative.
    Choice { len: u16 },
    Repeat { min: u8, max: Option<u8>, len: u16 },
    Capture { name: Capture, len: u16 },
    Call(Id<Pattern>),
}

/// (Not `Class`, which is a place's.)
pub enum CharClass { Digit, Letter, Space, Alnum, Any, Rest, Start, End }

pub enum Capture { Payee, Code, Amount, Date, Named(Sym) }
