//! The book: everything the sources declare and record, resolved and typed.
//!
//! Names are interned [`Sym`]s and references are typed ids, so nothing below
//! carries a lifetime except the [`Book`] itself, which owns the interner that
//! borrows the source text. Every hierarchy is a pre-ordered [`Tree`]: "is this
//! place under that one", "is this kind a 401k", and "does this jurisdiction
//! include that one" are all interval tests.

use axiom_core::day::days_in_month;
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
    /// What flows are for: `income`, `spending`, `capital` and the tree beneath
    /// them, pre-ordered so "is groceries food" is an interval test.
    pub purposes: Tree<Purpose>,
    pub systems: Tree<System>,
    pub commodities: Arena<Commodity>,
    /// Identified things: `condo`, `laptop`.
    pub assets: Arena<Asset>,
    /// Promises of flows: `phone with mint`, `mortgage with rocket`.
    pub contracts: Arena<Contract>,

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
    /// v3's plans. The v3 model still fills it; the v4 model leaves it empty,
    /// and it is deleted once nothing reads it.
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
    pub(crate) purposes: Scoped<Purpose>,
    pub(crate) assets: Map<Sym, Id<Asset>>,
    pub(crate) contracts: Map<Sym, Id<Contract>>,
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
    /// The account is not the entity's own place, so what a line that
    /// writes the name meant is unclear.
    pub clash: bool,
}

/// Built-in things every book has.
#[derive(Clone, Copy, Debug)]
pub struct Roots {
    /// The person keeping the book. Owns every place by default.
    pub me: Id<Entity>,
    /// `?`: the unknown party. Value of unknown origin comes from it and
    /// unexplained value goes to it.
    pub unknown: Id<Place>,
    /// Where `opening` holdings come from: an outside place no law watches.
    pub opening: Id<Place>,
    /// The market, a party: flows with it are revaluations. A flow between an
    /// asset place and the market's place changes quantity and keeps basis: it
    /// realizes nothing.
    pub market: Id<Entity>,
    /// Root kinds of accounts, by class.
    pub asset: Id<Kind>,
    pub debt: Id<Kind>,
    /// The root kind of assets (identified things).
    pub thing: Id<Kind>,
    pub commodity: Id<Kind>,
    pub entity: Id<Kind>,
    /// The roots of the purpose tree.
    pub income: Id<Purpose>,
    pub spending: Id<Purpose>,
    pub capital: Id<Purpose>,
}

/// Where a place sits: what the owners hold, what they owe, or outside them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Class {
    /// Held by an owner: parcels with basis. Accounts of `asset` kinds,
    /// owners' own holdings, assets, and claims a party owes an owner.
    Asset,
    /// Owed by an owner: a plain balance. Accounts of `debt` kinds, and
    /// claims a party holds on an owner.
    Debt,
    /// Everyone else: parties, `?`, and where openings come from. Value that
    /// reaches one has left the owners; value from one is new to them.
    Outside,
}

impl Class {
    /// Balances are inflow minus outflow. Debts are naturally negative; this
    /// sign shows them the way people read them.
    pub fn display_sign(self) -> i64 {
        match self {
            Class::Asset | Class::Outside => 1,
            Class::Debt => -1,
        }
    }

    /// Whether a place of this class holds parcels (with basis and lots) rather
    /// than a plain signed balance.
    pub fn holds_parcels(self) -> bool {
        self == Class::Asset
    }
}

// v3 bridge: deleted with the v3 model, whose path roots the v4 model does not have.
/// The five roots of v3's chart of accounts. A path root gives a place its
/// class and its kind its root kind.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum PathRoot {
    Assets,
    Liabilities,
    Income,
    Expenses,
    Equity,
}

impl PathRoot {
    pub const ALL: [PathRoot; 5] =
        [PathRoot::Assets, PathRoot::Liabilities, PathRoot::Income, PathRoot::Expenses, PathRoot::Equity];

    /// The root a full path starts at, if it starts at one.
    pub fn of(path: &str) -> Option<PathRoot> {
        let root = PathRoot::ALL.into_iter().find(|root| path.starts_with(root.path()))?;
        let rest = &path[root.path().len()..];
        (rest.is_empty() || rest.starts_with('/')).then_some(root)
    }

    /// The path that opens places under this root: `assets`, `liabilities`, …
    pub fn path(self) -> &'static str {
        match self {
            PathRoot::Assets => "assets",
            PathRoot::Liabilities => "liabilities",
            PathRoot::Income => "income",
            PathRoot::Expenses => "expenses",
            PathRoot::Equity => "equity",
        }
    }

    pub fn class(self) -> Class {
        match self {
            PathRoot::Assets => Class::Asset,
            PathRoot::Liabilities => Class::Debt,
            PathRoot::Income | PathRoot::Expenses | PathRoot::Equity => Class::Outside,
        }
    }

    /// Balances are inflow minus outflow. Liabilities, income and equity are
    /// naturally negative; this sign shows them the way people read them.
    pub fn display_sign(self) -> i64 {
        match self {
            PathRoot::Assets | PathRoot::Expenses => 1,
            PathRoot::Liabilities | PathRoot::Income | PathRoot::Equity => -1,
        }
    }
}

/// A place value can be: `assets/bank/checking`, `expenses/food`, `income/salary`.
pub struct Place {
    /// The full path.
    pub path: Sym,
    pub class: Class,
    pub role: Role,
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
    pub opened: Option<Day>,
    pub closed: Option<Day>,
    /// Own properties first, then defaults inherited from the kind chain.
    pub props: Props,
    pub doc: Option<Sym>,
    /// `None` for places opened implicitly by a full path.
    pub loc: Option<Loc>,
}

/// What a place is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Role {
    /// A position with an institution: `checking : deposit at chase`.
    Account { institution: Option<Id<Entity>> },
    /// What an owner holds with no institution: cash in hand, and the one
    /// place every owner has.
    Holding(Id<Entity>),
    /// A party as a flow's end. `None` for `?`, the opening, and (until the v4
    /// model lands) v3's income, expense and equity places.
    Outside(Option<Id<Entity>>),
    /// Claims between a party and an owner: an `Asset`-class tab holds what the
    /// party owes, a `Debt`-class tab what the owner owes it. Tabs are `claim`
    /// places: each claim stays its own parcel.
    Tab(Id<Entity>),
    /// An identified thing's place: its one unit, and its parts' basis.
    Asset(Id<Asset>),
}

/// Someone: `me`, `acme`, `landlord`, `irs`, `paypal/john`.
pub struct Entity {
    pub path: Sym,
    pub kind: Id<Kind>,
    /// Its place as a flow's end: an owner's `Holding`, a party's `Outside`.
    pub place: Option<Id<Place>>,
    /// Resolved from the kind chain: money from this entity stays tied to it.
    pub restricted: bool,
    /// Jurisdictions, sorted by start day. They may overlap.
    pub lives: Box<[Residence]>,
    /// `member household`: the household this person belongs to, which is
    /// governed in their place by the systems it lives in.
    pub member: Option<Id<Entity>>,
    /// `owner me` on a business: it is one of the owners, owned by that one.
    pub owner: Option<Id<Entity>>,
    /// `of studio` on a client: what it pays is that owner's.
    pub client_of: Option<Id<Entity>>,
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
    /// On a party kind: what flows with its parties are for (`grocer`:
    /// groceries).
    pub purpose: Option<Id<Purpose>>,
    /// On a commodity kind: what its issuer pays is for (`fund`: dividend).
    pub pays: Option<Id<Purpose>>,
    /// On an account kind: what arrives from flows of the second purpose is
    /// the first (`401k`: pre-tax-deferral from wages).
    pub takes: Box<[(Id<Purpose>, Id<Purpose>)]>,
    /// On a party kind: the tax inside every price paid to its parties.
    pub sales_tax: Option<Ratio>,
    /// `business 60% for studio` on a party kind: every flow with its parties
    /// is shared.
    pub shares: Box<[Share]>,
    /// Properties instances may set: own declarations, then inherited ones.
    pub has: Box<[Has]>,
    /// Defaults for instances: own, then inherited.
    pub props: Props,
    /// Only this kind's own laws; ancestors' laws are found through the tree.
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// What a kind classifies. `Place` covers accounts (`Asset`, `Debt`) and, for
/// the v3 model, `Outside`; `Thing` is for asset kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sort {
    Place(Class),
    Thing,
    Commodity,
    Entity,
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
    /// Resolved from the kind chain (`select fifo` on `currency`): how parcels
    /// of it are relieved where neither the flow nor the place says.
    pub select: Option<Policy>,
    /// `grows 5% yearly`: the valuation model forecasts use.
    pub growth: Option<Ratio>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// A node of the purpose tree: `groceries : food`.
pub struct Purpose {
    pub name: Sym,
    /// Which of the three roots it descends from.
    pub root: PurposeRoot,
    pub system: Option<Id<System>>,
    /// `of KIND`: it takes an object of this kind (`improvement of thing`).
    pub of: Option<Id<Kind>>,
    /// `business 12% for studio`: every flow of this purpose is shared.
    pub shares: Box<[Share]>,
    /// Only this purpose's own laws; ancestors' laws are found through the tree.
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

impl Purpose {
    /// The purpose tree of a book that declares none: `income`, `spending` and
    /// `capital`, in that order.
    pub fn roots<'s>(names: &mut Interner<'s>) -> (Tree<Purpose>, [Id<Purpose>; 3]) {
        let roots =
            [("income", PurposeRoot::Income), ("spending", PurposeRoot::Spending), ("capital", PurposeRoot::Capital)];
        let items = roots.map(|(name, root)| Purpose {
            name: names.intern(name),
            root,
            system: None,
            of: None,
            shares: Box::default(),
            laws: Box::default(),
            doc: None,
            loc: None,
        });
        let (tree, ids) = Tree::build(items.into(), &[None; 3]).expect("roots have no parents");
        (tree, [ids[0], ids[1], ids[2]])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PurposeRoot {
    /// Value coming to the owners: wages, rent received, dividends.
    Income,
    /// Value leaving them for good: groceries, rent paid, tax, interest paid.
    Spending,
    /// Value that joins something they keep: an improvement, a purchase of a
    /// thing. A flow of a capital purpose with an object adds a part to it.
    Capital,
}

/// `business 60% for studio`: that share of each flow is borne (or, for
/// income, earned) by that owner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Share {
    pub rate: Ratio,
    pub owner: Id<Entity>,
    pub loc: Loc,
}

/// An identified thing: `asset condo : rental-home`.
pub struct Asset {
    pub name: Sym,
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    /// Where its one unit sits (`Role::Asset`), unless `at ACCOUNT` names an
    /// institution's place.
    pub place: Id<Place>,
    /// Its own commodity: one unit, precision 0, named after it.
    pub unit: Id<Commodity>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// A promise of flows with one party.
pub struct Contract {
    pub name: Sym,
    pub party: Id<Entity>,
    /// Whose promise: the owner of the holding it pays from or into.
    pub owner: Id<Entity>,
    pub schedule: Recur,
    /// One occurrence, as the contract writes it: flows of mode `Planned`
    /// dated `schedule.from`. An occurrence in the journal re-dates a copy,
    /// with the journal's overrides.
    pub template: Box<[Flow]>,
    /// `buy VTI for 500 USD`: occurrences say how much was bought.
    pub buys: Option<Id<Commodity>>,
    /// `covers 1y`: each occurrence is recognized over this span from its day.
    pub covers: Option<Span>,
    pub shares: Box<[Share]>,
    /// `deposit 2_350 USD`: a claim the party holds, and money held for it,
    /// from `schedule.from` to `schedule.until`.
    pub deposit: Option<Amount>,
    pub loan: Option<Loan>,
    /// `escrow 410 USD into escrow`: added to each occurrence.
    pub escrow: Option<(Amount, Id<Place>)>,
    /// `match 50% of retirement up to 6%`.
    pub matching: Option<Match>,
    /// `DATE NAME ends`: nothing is expected after this day.
    pub ended: Option<(Day, Loc)>,
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// When a contract's occurrences fall due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Recur {
    /// `monthly` is one month, `twice monthly` is `Twice`, `every 2w` 14 days.
    pub every: Cadence,
    pub on: Option<On>,
    pub from: Day,
    pub until: Option<Day>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cadence {
    Every(Span),
    /// Twice a month, on two days (`on 15, last`).
    Twice,
}

impl Contract {
    /// The days occurrences fall due in `from..=until`, bounded by the
    /// schedule, `until` and `ended`.
    pub fn due_days(&self, from: Day, until: Day) -> Vec<Day> {
        let Recur { every, on, from: start, until: stop } = self.schedule;
        // v3 bridge: v3's `On` names one day, so `Twice` waits for the v4 syntax, which can name two.
        let Cadence::Every(every) = every else { return Vec::new() };
        if every == Span::default() {
            return Vec::new();
        }
        let last = [stop, self.ended.map(|(day, _)| day)].into_iter().flatten().fold(until, Ord::min);
        // Steps are taken from the start, never from the previous day, so a
        // month-end clamp does not drag every later month with it.
        let nth = |step: i32| land(start.add(Span { months: every.months * step, days: every.days * step }), on);
        (0..).map(nth).take_while(|&day| day <= last).filter(|&day| day >= from.max(start)).collect()
    }
}

/// Moves `base` to the day its period asks for.
fn land(base: Day, on: Option<On>) -> Day {
    let (year, month, _) = base.ymd();
    let clamped = |month: u32, day: u8| Day::from_ymd(year, month, u32::from(day).min(days_in_month(year, month)));
    match on {
        None => base,
        Some(On::MonthDay(day)) => clamped(month, day).unwrap_or(base),
        Some(On::YearDay { month, day }) => clamped(u32::from(month).clamp(1, 12), day).unwrap_or(base),
        Some(On::Weekday(weekday)) => base.add_days(((u32::from(weekday) + 7 - base.weekday()) % 7) as i32),
    }
}

/// `loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loan {
    pub principal: Amount,
    pub on: Day,
    /// Yearly, as a fraction.
    pub rate: Ratio,
    pub term: Span,
    /// The asset it financed: interest is `#interest of` it.
    pub asset: Option<Id<Asset>>,
    /// The owner's debt to the party: a `Tab`, `Debt`-class place.
    pub debt: Id<Place>,
}

/// `match 50% of retirement up to 6%`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    pub rate: Ratio,
    pub into: Id<Place>,
    /// Of the gross.
    pub up_to: Ratio,
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

    /// A place by full path or unique suffix (`checking`), or an entity's place.
    /// A place wins over an entity of the same name.
    pub fn place(&self, text: &str) -> Result<Id<Place>, Miss<Place>> {
        let miss = match self.lookup.places.resolve(&self.names, text, |_| true) {
            Err(miss @ Miss::Unknown { .. }) => miss,
            found => return found,
        };
        match self.entity(text) {
            Ok(entity) => self.entities[entity].place.ok_or(miss),
            Err(_) => Err(miss),
        }
    }

    // v3 bridge: deleted with the v3 model, whose places all sit under a path root. So are the callers that read
    // `v3_root(place).display_sign()`, where the class `Outside` would show income and equity the wrong way round.
    /// The root of `place`'s path: which of v3's five sides of the books it is on.
    pub fn v3_root(&self, place: Id<Place>) -> PathRoot {
        PathRoot::of(self.name(self.places[place].path)).expect("every v3 place sits under a path root")
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

    /// A purpose by name (`groceries`).
    pub fn purpose(&self, text: &str) -> Result<Id<Purpose>, Miss<Purpose>> {
        self.lookup.purposes.names.resolve(&self.names, text, |_| true)
    }

    pub fn asset(&self, name: &str) -> Option<Id<Asset>> {
        self.names.get(name).and_then(|sym| self.lookup.assets.get(&sym).copied())
    }

    pub fn contract(&self, name: &str) -> Option<Id<Contract>> {
        self.names.get(name).and_then(|sym| self.lookup.contracts.get(&sym).copied())
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
