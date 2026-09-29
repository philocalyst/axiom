//! The book: everything the sources declare and record, resolved and typed.
//!
//! Names are interned [`Sym`]s and references are typed ids, so nothing below
//! carries a lifetime except the [`Book`] itself, which owns the interner that
//! borrows the source text. Every hierarchy is a pre-ordered [`Tree`]: "is this
//! place under that one", "is this kind a 401k", and "does this jurisdiction
//! include that one" are all interval tests.

use axiom_core::{Arena, Day, Groups, Id, Interner, Loc, Map, Qty, Ratio, Span, Sym, Tree};

use crate::journal::{Assert, Event, Flow, Plan, Prices, Split, Txn};
use crate::law::{Law, Rules, Ty, Value};
use crate::names::{Names, Scoped};

pub use axiom_syntax::{EventState, On, Period, Policy};

pub struct Book<'s> {
    pub names: Interner<'s>,
    /// The currency that basis, totals and net worth are counted in.
    pub base: Id<Commodity>,
    /// `relaxed`: law violations are warnings.
    pub relaxed: bool,
    pub roots: Roots,

    pub places: Tree<Place>,
    pub entities: Tree<Entity>,
    pub kinds: Tree<Kind>,
    pub systems: Tree<System>,
    pub commodities: Arena<Commodity>,

    pub laws: Arena<Law>,
    pub rules: Rules,
    pub params: Arena<Param>,
    pub schedules: Arena<Schedule>,
    pub codes: Vec<CodeRule>,

    pub txns: Arena<Txn>,
    /// Sorted by day; within a day, in declaration order (files in path order,
    /// then source order). Order of declaration decides ties.
    pub flows: Arena<Flow>,
    /// Every flow touching each place (as source or target), in flow order.
    pub touching: Groups<Place, Id<Flow>>,
    /// Sorted by day, then declaration order.
    pub asserts: Vec<Assert>,
    /// Sorted by day, then declaration order.
    pub events: Vec<Event>,
    pub prices: Prices,
    /// Sorted by day, then declaration order.
    pub splits: Vec<Split>,
    pub plans: Arena<Plan>,
    pub syncs: Vec<SyncSpec>,
    /// How names are found. [`build`](crate::build) fills it; in a book made by
    /// hand it is empty, and `Book::place` and its siblings find nothing.
    pub lookup: Lookup,
}

/// Every way of finding a thing by name: each name and each of its `/`
/// suffixes, mapped to what they may mean.
#[derive(Default)]
pub struct Lookup {
    pub(crate) places: Names<Place>,
    pub(crate) entities: Scoped<Entity>,
    pub(crate) kinds: Scoped<Kind>,
    pub(crate) params: Scoped<Param>,
    pub(crate) laws: Names<Law>,
    pub(crate) commodities: Map<Sym, Id<Commodity>>,
    /// The names a flow writes that mean an entity although an account's path
    /// also ends with them: see [`World::taken`](crate::declare::World).
    pub(crate) taken: Map<Sym, Taken>,
}

/// An entity that has a name in flows which an account's path also ends with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Taken {
    pub entity: Id<Entity>,
    /// The account is not the entity's own `via` place, so what a line that
    /// writes the name meant is unclear.
    pub clash: bool,
}

/// Built-in things every book has.
#[derive(Clone, Copy, Debug)]
pub struct Roots {
    /// The person keeping the book. Owns every place by default.
    pub me: Id<Entity>,
    /// `?`: where value of unknown origin comes from and unexplained value goes.
    pub unknown: Id<Place>,
    /// `equity/opening`: where `opening` balances come from.
    pub opening: Id<Place>,
    /// `market : income`: the kind of place a revaluation comes from or goes
    /// to. A flow between an asset place and one of these changes quantity and
    /// keeps basis: it realizes nothing.
    pub market: Id<Kind>,
    pub asset: Id<Kind>,
    pub liability: Id<Kind>,
    pub income: Id<Kind>,
    pub expense: Id<Kind>,
    pub equity: Id<Kind>,
    pub commodity: Id<Kind>,
    pub entity: Id<Kind>,
}

/// Where a place sits in the accounting equation. Given by the root of its path.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Class {
    Asset,
    Liability,
    Income,
    Expense,
    Equity,
}

impl Class {
    pub const ALL: [Class; 5] = [Class::Asset, Class::Liability, Class::Income, Class::Expense, Class::Equity];

    /// The path root that opens places of this class: `assets`, `liabilities`, …
    pub fn root(self) -> &'static str {
        match self {
            Class::Asset => "assets",
            Class::Liability => "liabilities",
            Class::Income => "income",
            Class::Expense => "expenses",
            Class::Equity => "equity",
        }
    }

    /// Balances are inflow minus outflow. Income, liabilities and equity are
    /// naturally negative; this sign shows them the way people read them.
    pub fn display_sign(self) -> i64 {
        match self {
            Class::Asset | Class::Expense => 1,
            Class::Liability | Class::Income | Class::Equity => -1,
        }
    }

    /// Whether a place of this class holds parcels (with basis and lots) rather
    /// than a plain signed balance.
    pub fn holds_parcels(self) -> bool {
        self == Class::Asset
    }
}

/// A place value can be: `assets/bank/checking`, `expenses/food`, `income/salary`.
pub struct Place {
    /// The full path.
    pub path: Sym,
    pub class: Class,
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    /// The commodities this place may hold; `None` for any.
    pub holds: Option<Box<[Id<Commodity>]>>,
    /// Resolved: the place's own policy, else its kind chain's.
    pub select: Option<Policy>,
    /// Resolved from the kind chain: gains are not realized inside.
    pub deferred: bool,
    /// Resolved from the kind chain: what basis arriving value takes.
    pub basis: Basis,
    /// Resolved from the kind chain: this place holds what others owe, and its
    /// parcels stay apart by the transaction that made them.
    pub claim: bool,
    /// Resolved: own, else the kind chain's.
    pub liquidity: Option<Span>,
    /// `account expenses/business as biz`.
    pub alias: Option<Sym>,
    pub opened: Option<Day>,
    pub closed: Option<Day>,
    /// Own properties first, then defaults inherited from the kind chain.
    pub props: Props,
    pub doc: Option<Sym>,
    /// `None` for places opened implicitly by a full path.
    pub loc: Option<Loc>,
}

/// Someone: `me`, `acme`, `landlord`, `irs`, `paypal/john`.
pub struct Entity {
    pub path: Sym,
    pub kind: Id<Kind>,
    /// The place used when this entity is written as a flow end.
    pub via: Option<Id<Place>>,
    /// Resolved from the kind chain: money from this entity stays tied to it.
    pub restricted: bool,
    /// Jurisdictions, sorted by start day. They may overlap.
    pub lives: Box<[Residence]>,
    /// `member household`: the household this person belongs to, which is
    /// governed in their place by the systems it lives in.
    pub member: Option<Id<Entity>>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// `lives us/ca from 2025-01-01 until 2025-06-30`: inclusive, and open-ended
/// on either side when unwritten.
#[derive(Clone, Copy, Debug)]
pub struct Residence {
    pub from: Day,
    pub until: Day,
    pub system: Id<System>,
}

/// What basis value arriving from outside the owner's asset places takes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Basis {
    /// Its face value in the base currency, or its `@` price: money already
    /// taxed, a purchase.
    #[default]
    Cost,
    /// Nothing: pre-tax deferrals, deducted contributions. All of it is gain
    /// when it leaves.
    Zero,
}

/// What something is: `bank`, `401k`, `stock`, `grant`, `person`.
#[derive(Clone)]
pub struct Kind {
    pub name: Sym,
    pub sort: Sort,
    /// The system that declared it; `None` for built-ins and project kinds.
    pub system: Option<Id<System>>,
    // Resolved down the kind chain.
    pub restricted: bool,
    pub deferred: bool,
    pub basis: Option<Basis>,
    pub claim: bool,
    pub select: Option<Policy>,
    pub liquidity: Option<Span>,
    /// Properties instances may set: own declarations, then inherited ones.
    pub has: Box<[Has]>,
    /// Defaults for instances: own, then inherited.
    pub props: Props,
    /// Only this kind's own laws; ancestors' laws are found through the tree.
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// What a kind classifies. Place kinds carry the class of their root.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sort {
    Place(Class),
    Commodity,
    Entity,
}

impl Sort {
    /// The word for a kind of this sort: `asset`, `expense`, `commodity`, …
    pub fn noun(self) -> &'static str {
        match self {
            Sort::Place(Class::Asset) => "asset",
            Sort::Place(Class::Liability) => "liability",
            Sort::Place(Class::Income) => "income",
            Sort::Place(Class::Expense) => "expense",
            Sort::Place(Class::Equity) => "equity",
            Sort::Commodity => "commodity",
            Sort::Entity => "entity",
        }
    }
}

/// A declared property: `has beneficiary entity`.
#[derive(Clone, Copy, Debug)]
pub struct Has {
    pub name: Sym,
    pub ty: Ty,
    pub loc: Option<Loc>,
}

pub type Props = Box<[Prop]>;

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub name: Sym,
    pub value: Value,
    pub loc: Option<Loc>,
}

/// A unit of account: `USD`, `VTI`, `BTC`, `HOUSE`.
pub struct Commodity {
    pub symbol: Sym,
    pub kind: Id<Kind>,
    /// Decimal places: declared, or the most seen in any written amount.
    pub scale: u8,
    pub title: Option<Sym>,
    pub liquidity: Option<Span>,
    /// `grows 5% yearly`: the valuation model forecasts use.
    pub growth: Option<Ratio>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// A body of kinds, params and laws: `us`, `us/ca`, `us/401k`. Children
/// include their ancestors: `us/ca/san-francisco` is governed by all three.
pub struct System {
    pub path: Sym,
    /// Top-level laws, which govern residents and every place they own.
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    /// `None` for an ancestor implied by a deeper path.
    pub loc: Option<Loc>,
}

/// `param limit`: values by time and name keys.
pub struct Param {
    pub name: Sym,
    pub system: Option<Id<System>>,
    /// Sorted by names, then `since`.
    pub rows: Box<[ParamRow]>,
    pub loc: Loc,
}

pub struct ParamRow {
    /// A year key `2026` means from 2026-01-01; a date key means from that
    /// day. Lookup takes the latest row at or before the day asked for.
    pub since: Option<Day>,
    pub names: Box<[Sym]>,
    pub value: Value,
    pub loc: Loc,
}

/// Marginal brackets: `0 USD 10% | 12_400 USD 12% | …`.
pub struct Schedule {
    pub unit: Id<Commodity>,
    /// Ascending thresholds; the first is zero.
    pub brackets: Box<[Bracket]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Bracket {
    pub from: Qty,
    pub rate: Ratio,
}

/// `code trip-*` / `on expenses/travel/*`: where codes may appear.
pub struct CodeRule {
    pub pattern: Sym,
    pub on: Box<[CodeScope]>,
    pub loc: Loc,
}

#[derive(Clone, Copy, Debug)]
pub enum CodeScope {
    Places(Sym),
    Kind(Id<Kind>),
}

/// `sync FILE` / `run COMMAND`
pub struct SyncSpec {
    pub file: Sym,
    pub run: Sym,
    pub loc: Loc,
}

/// A quantity of one commodity. 16 bytes, `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Amount {
    pub qty: Qty,
    pub unit: Id<Commodity>,
}

impl Amount {
    pub fn new(qty: Qty, unit: Id<Commodity>) -> Amount {
        Amount { qty, unit }
    }

    pub fn zero(unit: Id<Commodity>) -> Amount {
        Amount { qty: Qty::ZERO, unit }
    }
}

/// Why a written name did not resolve.
pub enum Miss<T> {
    /// Nothing by that name; perhaps the closest name was meant.
    Unknown { suggestion: Option<Sym> },
    /// Several things end with that suffix.
    Ambiguous(Box<[Id<T>]>),
}

// Written out because deriving would demand `T: Debug` of the marker type.
impl<T> std::fmt::Debug for Miss<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Miss::Unknown { suggestion } => f.debug_struct("Unknown").field("suggestion", suggestion).finish(),
            Miss::Ambiguous(ids) => f.debug_tuple("Ambiguous").field(ids).finish(),
        }
    }
}

impl<'s> Book<'s> {
    pub fn name(&self, sym: Sym) -> &'s str {
        self.names.name(sym)
    }

    /// A place by full path or unique suffix (`checking`), or an entity's `via`.
    /// A place wins over an entity of the same name.
    pub fn place(&self, text: &str) -> Result<Id<Place>, Miss<Place>> {
        let miss = match self.lookup.places.resolve(&self.names, text, |_| true) {
            Err(miss @ Miss::Unknown { .. }) => miss,
            found => return found,
        };
        match self.entity(text) {
            Ok(entity) => self.entities[entity].via.ok_or(miss),
            Err(_) => Err(miss),
        }
    }

    pub fn entity(&self, text: &str) -> Result<Id<Entity>, Miss<Entity>> {
        self.lookup.entities.names.resolve(&self.names, text, |_| true)
    }

    pub fn commodity(&self, symbol: &str) -> Option<Id<Commodity>> {
        self.names.get(symbol).and_then(|sym| self.lookup.commodities.get(&sym).copied())
    }

    /// A kind by name (`401k`), qualified by its system (`us/401k/401k`), or by
    /// a system named after it (`us/401k`).
    pub fn kind(&self, text: &str) -> Result<Id<Kind>, Miss<Kind>> {
        crate::kinds::find(&self.lookup.kinds, &self.names, &self.systems, text, |_| true)
    }

    pub fn law(&self, name: &str) -> Result<Id<Law>, Miss<Law>> {
        self.lookup.laws.resolve(&self.names, name, |_| true)
    }

    /// Whether `kind` is `ancestor` or inherits from it.
    pub fn is_a(&self, kind: Id<Kind>, ancestor: Id<Kind>) -> bool {
        self.kinds.covers(ancestor, kind)
    }

    /// `amount` in `unit` at the latest prices on or before `day`, rounded to
    /// `unit`'s precision. `None` without a price path.
    pub fn convert(&self, amount: Amount, unit: Id<Commodity>, day: Day) -> Option<Amount> {
        if amount.unit == unit {
            return Some(amount);
        }
        let rate = self.prices.rate(amount.unit, unit, day, self.base)?;
        let (from, to) = (self.commodities[amount.unit].scale, self.commodities[unit].scale);
        Some(Amount::new(crate::prices::rescale(amount.qty, from, to, rate)?, unit))
    }

    /// The flow of `txn` that paid into `place`: what made a parcel there. A
    /// claim's counterparty is its payee and its due day is its `due`.
    pub fn paid_into(&self, txn: Id<Txn>, place: Id<Place>) -> Option<&Flow> {
        let txn = self.txns.get(txn)?;
        let first = txn.first.index();
        (first..first + txn.len as usize).map(|at| &self.flows[Id::new(at as u32)]).find(|flow| flow.to == place)
    }

    /// `1,234.56 USD`
    pub fn show(&self, amount: Amount) -> impl std::fmt::Display + '_ {
        let unit = &self.commodities[amount.unit];
        Shown { qty: amount.qty.brief(unit.scale), unit: self.name(unit.symbol) }
    }
}

struct Shown<'a> {
    qty: axiom_core::num::Shown,
    unit: &'a str,
}

impl std::fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.qty, self.unit)
    }
}
