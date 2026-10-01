// Types for the second half of v4: terms that change, first-class budgets, sync,
// and the calendar vocabulary every crate re-derives today. Written by the
// orchestrator; lane C5 applies them. NEW = add, CHANGED = replace, MOVED = relocate.
//
// Spec: LANGUAGE.md §3 (statements; changes hold from their day), §4 (budgets,
// known-as), §5 (terms change), §13 (sync).

// ════════════════════════════════════════════════════════════════════════════
// core/src/calendar.rs  (NEW module; `day.rs` keeps Day and its arithmetic)
// ════════════════════════════════════════════════════════════════════════════

/// An inclusive range of days, never empty. Replaces every `(Day, Day)` pair,
/// `Recognition`, `Residence`'s from/until, and the hand-written `first <= last`
/// checks (audit C4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Days {
    first: Day,
    last: Day,
}

impl Days {
    /// Every day there is: an unbounded span, where a declaration holds.
    pub const ALWAYS: Days;
    /// `None` when `last` is before `first`.
    pub fn new(first: Day, last: Day) -> Option<Days>;
    pub fn on(day: Day) -> Days;
    pub fn first(self) -> Day;
    pub fn last(self) -> Day;
    pub fn contains(self, day: Day) -> bool;
    pub fn overlaps(self, other: Days) -> bool;
    pub fn intersect(self, other: Days) -> Option<Days>;
    /// How many days: 1 for `on(day)`.
    pub fn len(self) -> u32;
}

/// MOVED from syntax (`ast::Period`): a calendar month or year.
pub enum Period { Month, Year }

/// NEW: one calendar month or year. The engine's `window_of`, the report's
/// `calendar.rs` and three `YYYY-MM` formatters become this (audit E7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Window {
    period: Period,
    first: Day,
}

impl Window {
    pub fn containing(period: Period, day: Day) -> Window;
    pub fn days(self) -> Days;
    pub fn next(self) -> Window;
    pub fn previous(self) -> Window;
}
impl Display for Window {} // `2026-01`, `2026`

/// NEW: the share of `qty`, spread evenly per day over `over`, that falls in
/// `within`. Telescoping, so the shares of a partition of `over` sum to `qty`
/// exactly. The engine's `totals::share` and the report's `apportion` are this
/// function (audit E7).
pub fn spread(qty: Qty, over: Days, within: Days) -> Qty;

/// MOVED from syntax (`ast::Cadence`, `ast::On`), with `On::Last` added on main.
pub enum Cadence { Every(Span), TwiceMonthly }
pub enum On { MonthDay(u8), Last, YearDay { month: u8, day: u8 }, Weekday(u8) }

/// NEW: the days a schedule falls due in `within`, stepping from `anchor`:
/// `every` from the anchor, landed on `on`. Steps are counted from the anchor,
/// never from the previous day, so a month-end clamp does not drag later months.
/// `TwiceMonthly` takes both of `on`'s days in each month. What
/// `Contract::due_days` does today, moved here and completed.
pub fn due(every: Cadence, on: &[On], anchor: Day, within: Days) -> impl Iterator<Item = Day>;

// ════════════════════════════════════════════════════════════════════════════
// core/src/timeline.rs  (NEW)
// ════════════════════════════════════════════════════════════════════════════

/// A value that changes on days (LANGUAGE §3): a declaration's value, then
/// each statement's from its day. Built by painting, in the order the
/// statements are written; read by binary search. Contract terms, budget limits
/// and dated properties are timelines.
#[derive(Clone, Debug)]
pub struct Timeline<T> {
    /// Sorted by day; the first step starts at `Day::MIN`; never empty.
    steps: Vec<(Day, T)>,
}

impl<T: Clone + PartialEq> Timeline<T> {
    pub fn new(value: T) -> Timeline<T>;
    /// `value` holds over `days`; outside them what held before stands.
    /// Painting an unbounded end (`Days::new(day, Day::MAX)`) is "from now on".
    /// Adjacent equal steps merge.
    pub fn paint(&mut self, days: Days, value: T);
    pub fn at(&self, day: Day) -> &T;
    /// Every (days, value) segment that meets `within`, in order.
    pub fn within(&self, within: Days) -> impl Iterator<Item = (Days, &T)>;
    /// The steps after the first: what changed, and from when.
    pub fn changes(&self) -> impl Iterator<Item = (Day, &T)>;
}

// ════════════════════════════════════════════════════════════════════════════
// core/src/id.rs
// ════════════════════════════════════════════════════════════════════════════

/// NEW: a contiguous run of ids in one arena (audit C12). `Txn { first, len }`,
/// `Book::paid_into`'s rebuilt ids, and the syntax `Many<T>` idea.
pub struct Run<T> { start: u32, len: u32, of: PhantomData<fn() -> T> }
impl<T> Run<T> {
    pub fn new(start: Id<T>, len: u32) -> Run<T>;
    pub fn ids(self) -> impl Iterator<Item = Id<T>>;
    pub fn len(self) -> u32;
    pub fn contains(self, id: Id<T>) -> bool;
}
impl<T> Index<Run<T>> for Arena<T> { type Output = [T]; }

// ════════════════════════════════════════════════════════════════════════════
// model/src/book.rs
// ════════════════════════════════════════════════════════════════════════════

pub struct Book<'s> {
    // … as now, plus:
    /// NEW: `budget food 900 USD monthly`, one per budgeted purpose.
    pub budgets: Arena<Budget>,
    /// CHANGED: `syncs: Vec<SyncSpec>` becomes
    pub sources: Vec<Source>,
}

/// CHANGED. A promise of flows with one party.
pub struct Contract {
    pub name: Sym,
    pub party: Id<Entity>,
    /// Whose promise: the owner of the holding it pays from or into.
    pub owner: Id<Entity>,
    /// `from … until …`, cut short by `ends` or extended by a statement: the
    /// days anything is expected at all. REPLACES `Recur.from/until`.
    pub days: Days,
    /// What the contract says, from each day on: the declaration's terms, then
    /// each statement's (LANGUAGE §5). REPLACES `schedule`, `template`,
    /// `covers`, `shares`, `escrow`, and `Loan.rate`.
    pub terms: Timeline<Terms>,
    /// `buy VTI for 500 USD`: occurrences say how much was bought.
    pub buys: Option<Id<Commodity>>,
    /// `deposit 2_350 USD`: a claim the party holds, and money held for it,
    /// over `days`.
    pub deposit: Option<Amount>,
    pub loan: Option<Loan>,
    pub matching: Option<Match>,
    /// `DATE NAME ends`: the statement that cut `days` short.
    pub ended: Option<Loc>,
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// NEW. What a contract says for a while.
#[derive(Clone, PartialEq, Debug)]
pub struct Terms {
    pub every: Cadence,
    pub on: Box<[On]>,
    /// Occurrences step from here: the contract's first day, or the day a
    /// statement changed the cadence.
    pub anchor: Day,
    /// One occurrence's flows, dated `anchor`. An occurrence re-dates a copy,
    /// with the journal's overrides. Empty while waived: nothing is expected.
    pub template: Box<[Flow]>,
    /// `about`: each occurrence states its own amount; the template's is the
    /// forecast's estimate, and promises do not compare amounts.
    pub estimate: bool,
    pub covers: Option<Span>,
    pub shares: Box<[Share]>,
    pub escrow: Option<(Amount, Id<Place>)>,
    /// A loan's yearly rate while these terms hold.
    pub rate: Option<Ratio>,
    /// The statement that set these terms; `None` for the declaration's.
    pub change: Option<Change>,
}

impl Terms {
    /// Nothing is expected while these terms hold (`waived`).
    pub fn is_waived(&self) -> bool;
}

/// NEW. A statement that changed something from a day (LANGUAGE §3): kept
/// with what it set, so `why` and diagnostics can point at it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Change {
    pub days: Days,
    pub description: Option<Sym>,
    pub code: Option<Sym>,
    pub loc: Loc,
}

impl Contract {
    /// CHANGED: the days occurrences fall due in `within`, segment by segment
    /// of `terms`, skipping waived ones, bounded by `days`. Built on
    /// `calendar::due`.
    pub fn due_days(&self, within: Days) -> Vec<Day>;
    pub fn terms_on(&self, day: Day) -> &Terms;
}

/// CHANGED: `Recur` and the model's own `Cadence` are REMOVED (core's are used).
/// `Loan` loses `rate` (it is in `Terms`).
pub struct Loan {
    pub principal: Amount,
    pub on: Day,
    pub term: Span,
    pub asset: Option<Id<Asset>>,
    pub debt: Id<Place>,
}

/// NEW. `budget food 900 USD monthly [carries]` (LANGUAGE §4): a warning when
/// the purpose's total for a window passes the limit in force in it.
pub struct Budget {
    pub purpose: Id<Purpose>,
    pub period: Period,
    /// The declaration's limit, then each `DATE budget …` statement's.
    pub limits: Timeline<Limit>,
    /// Judged on the total since it began against its limits summed through
    /// the window: an unspent month lends to the next, an overspent one borrows.
    pub carries: bool,
    /// The law that reports it (`warn total(window) <= limit`), so violations,
    /// headroom and `why` treat a budget as every other cap. Its `Law::budget`
    /// points back here.
    pub law: Id<Law>,
    pub loc: Loc,
}

/// NEW.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    Amount(Amount),
    /// `10% of #income`: that share of another purpose's total, same window.
    Share { rate: Ratio, of: Id<Purpose> },
}

// law.rs: `Law` gains `pub budget: Option<Id<Budget>>` — the engine's cap path
// reads the budget's timeline and `carries` instead of a constant.

/// CHANGED: `Entity` and `Place` gain
    /// `known-as "TRADER JOE*"`: globs matched against statement memos, in any
    /// case (LANGUAGE §13).
    pub known_as: Box<[Sym]>,
/// and `Entity.lives: Box<[Residence]>` keeps its type, with `Residence {
/// days: Days, system }`.

/// CHANGED: `Prop` gains the day it holds from; a thing's props are sorted by
/// name, then `since`, and a property statement (`06-15 me lives us/ny`, `07-01
/// flat business 20% for studio`) adds rows. `until` adds a row restoring the
/// value before.
pub struct Prop {
    pub name: Sym,
    pub value: Value,
    /// `Day::MIN` for a declaration's.
    pub since: Day,
    pub loc: Option<Loc>,
}
/// NEW helper: the row of `name` in force on `day`.
pub fn prop(props: &[Prop], name: Sym, day: Day) -> Option<&Prop>;

/// NEW, REPLACES `SyncSpec`. `sync NAME` (LANGUAGE §13): a source of facts
/// from outside. (Not `Sync`, which would shadow the marker trait, nor `Into`
/// below.)
pub struct Source {
    pub name: Sym,
    /// The command, with `{since}`, `{today}`, `{units}` and `{year}` unexpanded.
    pub run: Sym,
    pub sink: Sink,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// Where a source's facts go.
pub enum Sink {
    /// A sync named after an account: its records, reconciled into the
    /// journal. `csv: None` means the command prints Axiom.
    Feed { account: Id<Place>, csv: Option<Csv> },
    /// `into PATH`: Axiom text, merged into that file (`{year}` splits it).
    File(Sym),
    /// `into param NAME`: rows merged into that param.
    Param(Id<Param>),
    /// Neither: Axiom statements (invoices, bills) into the journal.
    Journal,
}

pub struct Csv {
    pub date: Column,
    /// `"MM/DD/YYYY"`; ISO when absent.
    pub date_format: Option<Sym>,
    pub amount: Money,
    pub memo: Option<Column>,
    pub balance: Option<Column>,
    pub pending: Option<Column>,
}

pub enum Money {
    /// Money into the account is positive, unless the export is `flipped`.
    Signed { column: Column, flipped: bool },
    Split { debit: Column, credit: Column },
}

pub enum Column {
    Header(Sym),
    /// 1-based, as written.
    Index(u16),
}

// ════════════════════════════════════════════════════════════════════════════
// model/src/journal.rs
// ════════════════════════════════════════════════════════════════════════════

/// RENAMED: the flow's `Terms` becomes `Detail` (`Flow::detail()`), so that
/// `Terms` is a contract's. Its `basis_end` stays until the v3 model goes.
pub struct Detail { /* as today's flow Terms */ }

/// CHANGED: `Flow.recognized: Days` (was `Recognition`); `Recognition` is removed.

/// CHANGED: `Txn { first: Id<Flow>, len: u32 }` becomes `Txn { flows: Run<Flow>, … }`.

pub enum Provenance {
    // … as now, plus:
    /// NEW: the party's own `#purpose` (`entity corner-store #groceries`).
    Entity(Id<Entity>),
}

pub enum Derivation {
    // … as now, plus:
    /// NEW: `for PARTY` on a payment: the party owes it, and the payment is its,
    /// passed through the owner.
    PaidFor(Id<Entity>),
    /// NEW: `^code waived` on a claim: what remained of it is forgiven.
    WriteOff,
    /// NEW: `DATE ASSET ends`: the asset leaves the owners for nothing.
    Disposal(Id<Asset>),
}
