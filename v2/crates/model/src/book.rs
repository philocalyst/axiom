//! The book: everything the sources declare and record, resolved and typed.
//!
//! Names are interned [`Sym`]s and references are typed ids, so nothing below
//! carries a lifetime except the [`Book`] itself, which owns the interner that
//! borrows the source text. Every hierarchy is a pre-ordered [`Tree`]: "is this
//! place under that one", "is this kind a 401k", and "does this jurisdiction
//! include that one" are all interval tests.

use axiom_core::day::days_in_month;
use axiom_core::{
    Arena, Day, Days, Dim, Groups, Id, Interner, Loc, Map, Qty, Ratio, Span, Sym, Timeline, Tree, calendar,
};

use crate::journal::{Assert, Event, Filed, Flow, Infer, Measure, Mode, Origin, Plan, Prices, Purposed, Reading, Split, Txn};
use crate::law::{Fault, Law, NodeId, Rules, Ty, Value};
use crate::names::{Names, Scoped};
use crate::sync::{Format, Pattern, Source};

pub use axiom_core::{Cadence, On, Period};
pub use axiom_syntax::{EventState, Policy};

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
    /// `also ITEM | FLOW`: what every matching flow implies, declared once.
    pub also: Arena<Also>,

    pub laws: Arena<Law>,
    pub rules: Rules,
    /// `budget food 900 USD monthly`, one per budgeted purpose.
    pub budgets: Arena<Budget>,
    pub params: Arena<Param>,
    pub schedules: Arena<Schedule>,
    pub codes: Vec<CodeRule>,
    /// Every pattern that recognizes a memo: named ones (`pattern ach = …`) and
    /// the `known-as` of things.
    pub patterns: Arena<Pattern>,
    /// `format NAME`: how a source's records read.
    pub formats: Arena<Format>,

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
    /// Work done and things used, sorted by day, then declaration order.
    pub measures: Arena<Measure>,
    /// Named values, sorted by code, then day.
    pub readings: Vec<Reading>,
    /// Returns as filed, in the order they were written.
    pub filed: Vec<Filed>,
    /// v3's plans. The v3 model still fills it; the v4 model leaves it empty,
    /// and it is deleted once nothing reads it.
    pub plans: Arena<Plan>,
    /// The `sync` declarations: where facts from outside come from.
    pub sources: Vec<Source>,
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
    /// `owner me 50%, jordan 50%`: who owns it, in what shares. Empty for one
    /// owner, which is `owner`.
    pub shares: Box<[Share]>,
    /// `known-as PATTERN, …`: what recognizes it in a statement's memo (§14).
    pub known_as: Box<[Id<Pattern>]>,
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
    /// Jurisdictions, sorted by first day. They may overlap.
    pub lives: Box<[Residence]>,
    /// `member household`: the household this person belongs to, which is
    /// governed in their place by the systems it lives in.
    pub member: Option<Id<Entity>>,
    /// `owner me` on a business: it is one of the owners, owned by that one.
    pub owner: Option<Id<Entity>>,
    /// `of studio` on a client: what it pays is that owner's.
    pub client_of: Option<Id<Entity>>,
    /// `owner me 60%, theo 40%` on a business: its tallies reach them in these
    /// shares. Empty for a sole owner (`owner`).
    pub owned_by: Box<[Share]>,
    /// Its own `currency`, else its residence's system's, else the book's base:
    /// resolved at build time.
    pub currency: Id<Commodity>,
    /// `citizen SYSTEM`: taxed by these wherever it lives.
    pub citizen: Box<[Id<System>]>,
    /// When a claim is income or spending.
    pub books: Books,
    /// `known-as PATTERN, …`: what recognizes it in a statement's memo (§14).
    pub known_as: Box<[Id<Pattern>]>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// `books cash|accrual`: when a claim counts as income or spending.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Books {
    /// When it is settled.
    #[default]
    Cash,
    /// When it is due.
    Accrual,
}

/// `lives us/ca from 2025-01-01 until 2025-06-30`: inclusive, and open-ended
/// on either side when unwritten.
#[derive(Clone, Copy, Debug)]
pub struct Residence {
    pub days: Days,
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

/// A property's value from a day: a declaration's, or a statement's
/// (`06-15 me lives us/ny`, `07-01 flat business 20% for studio`). A thing's
/// props are sorted by name, then `since`; `until` adds a row that restores
/// the value before.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub name: Sym,
    pub value: Value,
    /// `Day::MIN` for a declaration's.
    pub since: Day,
    pub loc: Option<Loc>,
}

/// The row of `name` in force on `day`: the latest that has begun, the first
/// written where two begin together.
pub fn prop(props: &[Prop], name: Sym, day: Day) -> Option<&Prop> {
    let begun = props.iter().filter(|prop| prop.name == name && prop.since <= day);
    begun.reduce(|best, prop| if prop.since > best.since { prop } else { best })
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
    /// What only passes through the owners: a gift received, tax withheld, a
    /// distribution, a reimbursement.
    Transfer,
}

/// `share 60% for studio`: that share of each flow. A weight is resolved to a
/// rate at build time; `measure` keeps what it was written as, for `why`
/// (`120 SQFT` of `1,000 SQFT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Share {
    pub rate: Ratio,
    /// An owner bears it (an allocation); a party owes it (a claim).
    pub entity: Id<Entity>,
    pub measure: Option<(Amount, Amount)>,
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
    /// `part of building`: a unit of it, a room of it. What is `of` the whole is
    /// shared among its parts by their measures (`area`), which are props.
    pub part_of: Option<Id<Asset>>,
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
    /// `from … until …`, cut short by `ends` or extended by a statement: the
    /// days anything is expected at all.
    pub days: Days,
    /// What the contract says, from each day on: the declaration's terms, then
    /// each statement's (LANGUAGE §5).
    pub terms: Timeline<Terms>,
    /// `buy VTI for 500 USD`: occurrences say how much was bought.
    pub buys: Option<Id<Commodity>>,
    /// `deposit 2_350 USD`: a claim the party holds, and money held for it,
    /// over `days`.
    pub deposit: Option<Amount>,
    pub loan: Option<Loan>,
    /// `match 50% of retirement up to 6%`.
    pub matching: Option<Match>,
    /// `DATE NAME ends`: the statement that cut `days` short.
    pub ended: Option<Loc>,
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// What a contract says for a while.
#[derive(Clone, PartialEq, Debug)]
pub struct Terms {
    /// Whether these terms make a promise or explicitly waive one.
    pub state: TermsState,
    /// `monthly` is one month, `twice monthly` is `TwiceMonthly`, `every 2w` 14 days.
    pub every: Cadence,
    /// Several days are each due (`yearly on 04-15, 06-15, 09-15, 01-15`).
    pub on: Box<[On]>,
    /// Occurrences step from here: the contract's first day, or the day a
    /// statement changed the cadence.
    pub anchor: Day,
    /// One occurrence's flows, dated `anchor`. An occurrence re-dates a copy,
    /// with the journal's overrides. Input-dependent amounts need a supplied
    /// occurrence value; forecasting returns `MissingInput` while it is absent.
    /// Loan flows may be derived and have no explicit template.
    pub template: Box<[Flow]>,
    /// `input water USD`: names occurrences may state (`water = 155.00 USD`).
    pub inputs: Box<[Input]>,
    /// `about`: each occurrence states its own amount; the template's is the
    /// forecast's estimate, and promises do not compare amounts.
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
    /// `also …` lines of this contract.
    pub also: Box<[Id<Also>]>,
    /// A loan's yearly rate while these terms hold.
    pub rate: Option<Ratio>,
    /// The statement that set these terms; `None` for the declaration's.
    pub change: Option<Change>,
}

/// Whether a stretch of a contract expects scheduled occurrences.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TermsState {
    /// The terms describe a promise, even when its payment is derived (as for a loan).
    #[default]
    Active,
    /// No occurrences are expected while this state holds.
    Waived,
}

/// A name an occurrence may state: `water = 155.00 USD`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Input {
    pub name: Sym,
    pub unit: Option<Id<Commodity>>,
    pub loc: Loc,
}

/// A deadline after the due day, and what its passing adds.
#[derive(Clone, PartialEq, Debug)]
pub struct Deadline {
    pub after: Span,
    /// The `else` item, compiled like a template item; `None` if the deadline
    /// only makes the claim late.
    pub otherwise: Option<Box<Flow>>,
}

/// `for last month|last quarter|last year`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relative {
    Last(Period),
    LastQuarter,
}

/// `covers the month` (the calendar period containing the due day) or
/// `covers 6m` (a span from it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Coverage {
    Calendar(Period),
    Quarter,
    Span(Span),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Escalation {
    /// `rising 3% yearly`: from each anniversary of `Contract.days.first()`.
    Rising(Ratio),
    /// `indexed to cpi yearly`: scaled by the param's ratio between anniversaries.
    Indexed(Id<Param>),
}

/// A contract feature that changes occurrence flows but is not yet lowered by
/// the forecast model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForecastFeature {
    Deadline,
    Shares,
    Also,
    Buy,
    Deposit,
    Matching,
}

/// Whether a contract covers a typed flow on a particular date.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContractCoverage {
    None,
    Active,
    Waived,
}

/// Why a contract's forecast amount could not be derived for a day.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForecastError {
    OutsideContract(Day),
    Waived(Day),
    MissingInput {
        input: Sym,
        day: Day,
    },
    MissingIndex {
        param: Id<Param>,
        day: Day,
    },
    InvalidIndex {
        param: Id<Param>,
        day: Day,
    },
    IndexFault {
        param: Id<Param>,
        day: Day,
        fault: Fault,
    },
    InvalidRate,
    UnresolvedAmount(Day),
    ConflictingRecognition(Day),
    InvalidCoverage(Day),
    UnsupportedProration(Day),
    MissingTemplate(Day),
    UnsupportedLoan(Day),
    UnsupportedFeature { feature: ForecastFeature, day: Day },
    Overflow,
}

impl Terms {
    /// Nothing is expected while these terms hold (`waived`).
    pub fn is_waived(&self) -> bool {
        self.state == TermsState::Waived
    }
}

/// A statement that changed something from a day (LANGUAGE §3): kept with
/// what it set, so `why` and diagnostics can point at it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Change {
    /// The days it holds, as written: from its day, or through its `until`.
    pub days: Days,
    pub description: Option<Sym>,
    /// The code that names it, so a later statement can extend or release it.
    pub code: Option<Sym>,
    pub loc: Loc,
}

impl Contract {
    /// Whether these terms cover a typed flow on `day`, independently of the
    /// contract's own due date. This lets an explicit contract cadence replace
    /// a learned or v3 schedule for the same movement throughout its term.
    ///
    /// An empty waiver borrows the template from the nearest active terms in
    /// this contract's timeline (preferring the prior stretch when equally
    /// near); that template's identity is then compared with `template`.
    /// This keeps a suspension from reviving a fallback schedule for the
    /// promise that was active immediately before it.
    pub fn covers(&self, template: &Flow, day: Day) -> ContractCoverage {
        if !self.days.contains(day) {
            return ContractCoverage::None;
        }
        let terms = self.terms_on(day);
        if !terms.is_waived() {
            return if terms.template.iter().any(|candidate| same_flow_kind(template, candidate)) {
                ContractCoverage::Active
            } else {
                ContractCoverage::None
            };
        }
        if terms.template.iter().any(|candidate| same_flow_kind(template, candidate)) {
            return ContractCoverage::Waived;
        }

        let nearest = self
            .terms
            .within(self.days)
            .filter(|(_, candidate)| !candidate.is_waived())
            .map(|(stretch, terms)| {
                let distance = if stretch.last() < day {
                    i64::from(day.0) - i64::from(stretch.last().0)
                } else if stretch.first() > day {
                    i64::from(stretch.first().0) - i64::from(day.0)
                } else {
                    0
                };
                (distance, u8::from(stretch.first() > day), terms)
            })
            .min_by_key(|(distance, prefers_future, _)| (*distance, *prefers_future));
        if nearest.is_some_and(|(_, _, terms)| {
            terms.template.iter().any(|candidate| same_flow_kind(template, candidate))
        }) {
            ContractCoverage::Waived
        } else {
            ContractCoverage::None
        }
    }

    /// The scheduled occurrences in `within`, borrowing the terms that govern
    /// each one. Terms changes split the schedule; waived stretches yield none.
    pub fn occurrences(&self, within: Days) -> impl Iterator<Item = ContractOccurrence<'_>> + '_ {
        within
            .intersect(self.days)
            .into_iter()
            .flat_map(move |within| {
                self.terms
                    .within(within)
                    .filter(|(_, terms)| !terms.is_waived())
                    .flat_map(move |(stretch, terms)| {
                        let days = stretch
                            .intersect(within)
                            .expect("timeline stretch intersects its window");
                        calendar::due(terms.every, &terms.on, terms.anchor, days)
                            .map(move |day| ContractOccurrence { day, terms })
                    })
            })
    }

    /// The days occurrences fall due in `within`, in order. Kept as a
    /// collecting convenience for callers that need owned dates.
    pub fn due_days(&self, within: Days) -> Vec<Day> {
        self.occurrences(within)
            .map(|occurrence| occurrence.day)
            .collect()
    }

    /// The multiplier for the terms in force on `day`, including any
    /// anniversary rise or the named index's movement since the contract began.
    pub fn amount_on(&self, book: &Book<'_>, day: Day) -> Result<Ratio, ForecastError> {
        if !self.days.contains(day) {
            return Err(ForecastError::OutsideContract(day));
        }
        let terms = self.terms_on(day);
        if terms.is_waived() {
            return Err(ForecastError::Waived(day));
        }
        let amount = match terms.escalation {
            None => Ok(Ratio::ONE),
            Some(Escalation::Rising(rate)) => {
                let yearly = Ratio::ONE
                    .checked_add(rate)
                    .ok_or(ForecastError::Overflow)?;
                if yearly.is_negative() {
                    return Err(ForecastError::InvalidRate);
                }
                ratio_pow(yearly, anniversary_count(self.days.first(), day)?)
            }
            Some(Escalation::Indexed(param)) => {
                let first = self.days.first();
                let anniversary = anniversary_on(first, day)?;
                let base = index_at(book, param, first)?;
                let current = index_at(book, param, anniversary)?;
                current.checked_div(base).ok_or(ForecastError::Overflow)
            }
        }?;
        if !terms.prorated {
            return Ok(amount);
        }
        let period = self
            .recognition_period(day)?
            .ok_or(ForecastError::UnsupportedProration(day))?;
        amount
            .checked_mul(prorated_share(self.days, period)?)
            .ok_or(ForecastError::Overflow)
    }

    /// The recognition window for one occurrence, including its relative
    /// `for` period or `covers` rule when present.
    pub fn recognition_on(&self, template: &Flow, day: Day) -> Result<Days, ForecastError> {
        if !self.days.contains(day) {
            return Err(ForecastError::OutsideContract(day));
        }
        if self.terms_on(day).is_waived() {
            return Err(ForecastError::Waived(day));
        }
        // An explicit `for` or `covers` window controls recognition even when
        // only part of it overlaps the contract. Proration scales the amount
        // by that overlap; it does not move recognition outside the declared window.
        if let Some(period) = self.recognition_period(day)? {
            return Ok(period);
        }
        let shift = day
            .0
            .checked_sub(template.day.0)
            .ok_or(ForecastError::Overflow)?;
        move_days(template.recognized, shift)
    }

    /// Forecast flows directly from the contract's scheduled terms. The
    /// iterator borrows the book and contract, allocating only each owned flow
    /// that the ledger projection must apply. This consumes model-constructed
    /// terms; source v4 contract parsing is not present. Missing values and
    /// unsupported flow-affecting features are typed errors rather than omitted
    /// or guessed amounts.
    pub fn forecast_flows<'a>(
        &'a self,
        book: &'a Book<'_>,
        id: Id<Contract>,
        within: Days,
    ) -> impl Iterator<Item = Result<Flow, ForecastError>> + 'a {
        self.occurrences(within).flat_map(move |occurrence| {
            let amount = self.amount_on(book, occurrence.day);
            let input = occurrence.terms.inputs.first().map(|input| input.name);
            let terms = occurrence.terms;
            let template_count = if self.loan.is_some() {
                1
            } else {
                terms.template.len().max(1)
            };
            std::iter::repeat(())
                .take(template_count)
                .enumerate()
                .map(move |(index, ())| {
                    if self.loan.is_some() {
                        return Err(ForecastError::UnsupportedLoan(occurrence.day));
                    }
                    if let Some(feature) = self.unsupported_feature(terms) {
                        return Err(ForecastError::UnsupportedFeature { feature, day: occurrence.day });
                    }
                    let Some(template) = terms.template.get(index) else {
                        return Err(ForecastError::MissingTemplate(occurrence.day));
                    };
                    if let Some(input) = input {
                        return Err(ForecastError::MissingInput {
                            input,
                            day: occurrence.day,
                        });
                    }
                    if template.infer != Infer::Known {
                        return Err(ForecastError::UnresolvedAmount(occurrence.day));
                    }
                    let recognized = self.recognition_on(template, occurrence.day)?;
                    amount.and_then(|factor| {
                        forecast_flow(template, occurrence.day, recognized, id, factor)
                    })
                })
        })
    }

    fn unsupported_feature(&self, terms: &Terms) -> Option<ForecastFeature> {
        if self.buys.is_some() {
            Some(ForecastFeature::Buy)
        } else if self.deposit.is_some() {
            Some(ForecastFeature::Deposit)
        } else if self.matching.is_some() {
            Some(ForecastFeature::Matching)
        } else if terms.due.is_some() {
            Some(ForecastFeature::Deadline)
        } else if !terms.shares.is_empty() {
            Some(ForecastFeature::Shares)
        } else if !terms.also.is_empty() {
            Some(ForecastFeature::Also)
        } else {
            None
        }
    }

    fn recognition_period(&self, day: Day) -> Result<Option<Days>, ForecastError> {
        let terms = self.terms_on(day);
        if terms.period.is_some() && terms.covers.is_some() {
            return Err(ForecastError::ConflictingRecognition(day));
        }
        match (terms.period, terms.covers) {
            (Some(Relative::Last(period)), None) => Ok(Some(previous_window(period, day)?)),
            (Some(Relative::LastQuarter), None) => Ok(Some(quarter_window(day, true)?)),
            (None, Some(Coverage::Calendar(period))) => Ok(Some(calendar_window(period, day)?)),
            (None, Some(Coverage::Quarter)) => Ok(Some(quarter_window(day, false)?)),
            (None, Some(Coverage::Span(span))) => Ok(Some(covered_span(day, span)?)),
            (None, None) => Ok(None),
            (Some(_), Some(_)) => Err(ForecastError::ConflictingRecognition(day)),
        }
    }

    /// The terms in force on `day`.
    pub fn terms_on(&self, day: Day) -> &Terms {
        self.terms.at(day)
    }
}

fn same_flow_kind(a: &Flow, b: &Flow) -> bool {
    let same_purpose = match (a.purpose, b.purpose) {
        (Some(a), Some(b)) => a.purpose == b.purpose && a.of == b.of,
        (None, None) => true,
        _ => false,
    };
    (a.from, a.to, a.out.unit, a.arrive.unit, a.owner, a.payee) ==
        (b.from, b.to, b.out.unit, b.arrive.unit, b.owner, b.payee)
        && same_purpose
}

fn anniversary_count(start: Day, day: Day) -> Result<u32, ForecastError> {
    let anniversary = anniversary_on(start, day)?;
    let years = anniversary
        .ymd()
        .0
        .checked_sub(start.ymd().0)
        .ok_or(ForecastError::Overflow)?;
    u32::try_from(years).map_err(|_| ForecastError::Overflow)
}

/// The latest anniversary not after `day`, clamping Feb 29 in non-leap years.
fn anniversary_on(start: Day, day: Day) -> Result<Day, ForecastError> {
    let year_delta = day
        .ymd()
        .0
        .checked_sub(start.ymd().0)
        .ok_or(ForecastError::Overflow)?;
    let month_delta = year_delta.checked_mul(12).ok_or(ForecastError::Overflow)?;
    let candidate = add_months(start, month_delta)?;
    if candidate <= day {
        Ok(candidate)
    } else {
        let previous = month_delta.checked_sub(12).ok_or(ForecastError::Overflow)?;
        add_months(start, previous)
    }
}

fn ratio_pow(mut base: Ratio, mut exponent: u32) -> Result<Ratio, ForecastError> {
    let mut result = Ratio::ONE;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result.checked_mul(base).ok_or(ForecastError::Overflow)?;
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base.checked_mul(base).ok_or(ForecastError::Overflow)?;
        }
    }
    Ok(result)
}

fn index_at(book: &Book<'_>, param: Id<Param>, day: Day) -> Result<Ratio, ForecastError> {
    let Some(param_data) = book.params.get(param) else {
        return Err(ForecastError::MissingIndex { param, day });
    };
    let Some(row) = param_data
        .rows
        .iter()
        .filter(|row| row.names.is_empty() && row.since.is_none_or(|since| since <= day))
        .reduce(|best, row| if row.since > best.since { row } else { best })
    else {
        return Err(ForecastError::MissingIndex { param, day });
    };
    match row.value {
        Value::Num(value) if value > Ratio::ZERO => Ok(value),
        Value::Fault(fault) => Err(ForecastError::IndexFault { param, day, fault }),
        _ => Err(ForecastError::InvalidIndex { param, day }),
    }
}

fn prorated_share(contract: Days, period: Days) -> Result<Ratio, ForecastError> {
    let Some(overlap) = contract.intersect(period) else {
        return Ok(Ratio::ZERO);
    };
    let part = i64::from(overlap.last().0) - i64::from(overlap.first().0) + 1;
    let whole = i64::from(period.last().0) - i64::from(period.first().0) + 1;
    Ratio::new(i128::from(part), i128::from(whole)).ok_or(ForecastError::Overflow)
}

fn calendar_window(period: Period, day: Day) -> Result<Days, ForecastError> {
    let (year, month, _) = day.ymd();
    let (first_month, last_month) = match period {
        Period::Month => (month, month),
        Period::Year => (1, 12),
    };
    let first = Day::from_ymd(year, first_month, 1).ok_or(ForecastError::Overflow)?;
    let last = Day::from_ymd(year, last_month, days_in_month(year, last_month))
        .ok_or(ForecastError::Overflow)?;
    Days::new(first, last).ok_or(ForecastError::Overflow)
}

fn previous_window(period: Period, day: Day) -> Result<Days, ForecastError> {
    let first = calendar_window(period, day)?.first();
    let months = period.months().checked_neg().ok_or(ForecastError::Overflow)?;
    calendar_window(period, add_months(first, months)?)
}

fn quarter_window(day: Day, previous: bool) -> Result<Days, ForecastError> {
    let (year, month, _) = day.ymd();
    let first_month = ((month - 1) / 3) * 3 + 1;
    let first = Day::from_ymd(year, first_month, 1).ok_or(ForecastError::Overflow)?;
    let start = if previous {
        add_months(first, -3)?
    } else {
        first
    };
    let after = add_months(start, 3)?;
    let last = after
        .0
        .checked_sub(1)
        .map(Day)
        .ok_or(ForecastError::Overflow)?;
    Days::new(start, last).ok_or(ForecastError::Overflow)
}

fn covered_span(start: Day, span: Span) -> Result<Days, ForecastError> {
    let after = add_span(start, span)?;
    if after <= start {
        return Err(ForecastError::InvalidCoverage(start));
    }
    let last = after
        .0
        .checked_sub(1)
        .map(Day)
        .ok_or(ForecastError::Overflow)?;
    Days::new(start, last).ok_or(ForecastError::InvalidCoverage(start))
}

fn add_months(start: Day, months: i32) -> Result<Day, ForecastError> {
    add_span(start, Span::months(months))
}

fn add_span(start: Day, span: Span) -> Result<Day, ForecastError> {
    let (year, month, date) = start.ymd();
    let absolute_month = i64::from(year) * 12 + i64::from(month - 1) + i64::from(span.months);
    let year = i32::try_from(absolute_month.div_euclid(12)).map_err(|_| ForecastError::Overflow)?;
    let month =
        u32::try_from(absolute_month.rem_euclid(12) + 1).map_err(|_| ForecastError::Overflow)?;
    let date = date.min(days_in_month(year, month));
    let first = Day::from_ymd(year, month, date).ok_or(ForecastError::Overflow)?;
    let value = i64::from(first.0) + i64::from(span.days);
    i32::try_from(value)
        .map(Day)
        .map_err(|_| ForecastError::Overflow)
}

fn forecast_flow(
    template: &Flow,
    day: Day,
    recognized: Days,
    contract: Id<Contract>,
    factor: Ratio,
) -> Result<Flow, ForecastError> {
    let shift = day
        .0
        .checked_sub(template.day.0)
        .ok_or(ForecastError::Overflow)?;
    let scale = |amount: Amount| {
        amount
            .qty
            .scale(factor)
            .map(|qty| Amount::new(qty, amount.unit))
            .ok_or(ForecastError::Overflow)
    };
    let moved_detail = template
        .detail
        .as_deref()
        .map(|detail| {
            let mut moved = detail.clone();
            moved.basis = detail
                .basis
                .map(|basis| basis.scale(factor).ok_or(ForecastError::Overflow))
                .transpose()?;
            moved.cost = detail.cost.map(scale).transpose()?;
            moved.due = detail
                .due
                .map(|due| {
                    due.0
                        .checked_add(shift)
                        .map(Day)
                        .ok_or(ForecastError::Overflow)
                })
                .transpose()?;
            Ok::<_, ForecastError>(Box::new(moved))
        })
        .transpose()?;
    Ok(Flow {
        day,
        recognized,
        from: template.from,
        to: template.to,
        out: scale(template.out)?,
        arrive: scale(template.arrive)?,
        mode: Mode::Planned,
        infer: template.infer,
        txn: template.txn,
        payee: template.payee,
        owner: template.owner,
        purpose: template.purpose,
        description: template.description,
        origin: Origin::Occurrence(contract),
        select: template.select.clone(),
        codes: Box::default(),
        loc: template.loc,
        waive: template.waive,
        detail: moved_detail,
    })
}

fn move_days(days: Days, shift: i32) -> Result<Days, ForecastError> {
    let move_bound = |day: Day| {
        if day == Day::MIN || day == Day::MAX {
            Ok(day)
        } else {
            day.0
                .checked_add(shift)
                .map(Day)
                .ok_or(ForecastError::Overflow)
        }
    };
    Days::new(move_bound(days.first())?, move_bound(days.last())?).ok_or(ForecastError::Overflow)
}

/// One contract occurrence and the terms that govern its flow template.
#[derive(Clone, Copy, Debug)]
pub struct ContractOccurrence<'a> {
    /// The day this scheduled payment falls due.
    pub day: Day,
    /// The terms that supply this occurrence's flows and escalation.
    pub terms: &'a Terms,
}

/// `loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loan {
    pub principal: Amount,
    pub on: Day,
    pub term: Span,
    /// The asset it financed: interest is `#interest of` it.
    pub asset: Option<Id<Asset>>,
    /// The owner's debt to the party: a `Tab`, `Debt`-class place.
    pub debt: Id<Place>,
    /// `resets yearly from 2029-03-01 to sofr + 2.75% cap 2% life 5%`.
    pub resets: Option<Reset>,
    /// What a flow to the contract does: the default `Shortens`.
    pub prepay: Prepay,
}

/// A loan's rate follows an index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reset {
    pub every: Span,
    pub from: Day,
    pub index: Id<Param>,
    pub margin: Ratio,
    /// Largest change at one reset, and over the life, as rates.
    pub cap: Option<Ratio>,
    pub life: Option<Ratio>,
}

/// What a flow to a loan's contract does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Prepay {
    /// The loan ends sooner and the payment stays.
    #[default]
    Shortens,
    /// The payment is lowered.
    Recasts,
}

/// `also ITEM | FLOW [when EXPR]` (LANGUAGE §10): what every matching flow
/// implies, declared once. Escrow and an employer's match are `also` lines.
pub struct Also {
    pub on: AlsoOn,
    pub what: Implied,
    /// Compiled like a law's `when`: a node of `law`.
    pub when: Option<NodeId>,
    /// The expressions (amounts, `when`) live in this law's node arena, with no
    /// steps: one expression language for laws and declarations.
    pub law: Id<Law>,
    pub purpose: Option<Purposed>,
    pub description: Option<Sym>,
    pub loc: Loc,
}

/// What an `also` was written under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlsoOn {
    Contract(Id<Contract>),
    Entity(Id<Entity>),
    Kind(Id<Kind>),
    Purpose(Id<Purpose>),
}

/// What an `also` implies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Implied {
    /// `+ 5%`, `- 2.9% + 0.30 USD`: an item of the flow, between its ends.
    Item { sign: Sign, amount: NodeId },
    /// `lumen -> retirement 50% of …`, `-> escrow 410 USD`: a flow of its own.
    /// `None` ends mean the implying flow's own ends (`issuer -> self`).
    Flow { from: Option<Id<Place>>, to: Option<Id<Place>>, amount: NodeId },
}

/// How a line item bears on the flow it is under (LANGUAGE §3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign {
    /// Carved out of the header's amount.
    Carve,
    /// Comes on top of it.
    Add,
    /// Taken off it.
    Less,
}

/// `match 50% of retirement up to 6%`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    pub rate: Ratio,
    pub into: Id<Place>,
    /// Of the gross.
    pub up_to: Ratio,
}

/// `budget food 900 USD monthly [carries]` (LANGUAGE §4): a warning when the
/// purpose's total for a window passes the limit in force in it.
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
    /// `funded from HOLDING into HOLDING`: its limit moves each window into
    /// money held for it, which what the purpose spends is drawn from first.
    pub funded: Option<(Id<Place>, Id<Place>)>,
    pub loc: Loc,
}

/// What a budget allows in a window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    Amount(Amount),
    /// `10% of #income`: that share of another purpose's total, same window.
    Share {
        rate: Ratio,
        of: Id<Purpose>,
    },
}

/// A body of kinds, params and laws: `us`, `us/ca`, `us/401k`. Children
/// include their ancestors: `us/ca/san-francisco` is governed by all three.
pub struct System {
    pub path: Sym,
    /// Top-level laws, which govern residents and every place they own.
    pub laws: Box<[Id<Law>]>,
    /// `currency UNIT`: what its laws count in.
    pub currency: Option<Id<Commodity>>,
    /// `rates POLICY`: how it converts.
    pub rates: Option<RatePolicy>,
    pub doc: Option<Sym>,
    /// `None` for an ancestor implied by a deeper path.
    pub loc: Option<Loc>,
}

/// How a system converts between commodities.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RatePolicy {
    /// The day's price.
    Spot,
    /// A param's rates, such as the IRS's yearly averages.
    Param(Id<Param>),
}

/// `param limit`: values by time and name keys.
pub struct Param {
    pub name: Sym,
    /// `param mileage-rate USD/MI`: what its values are counted in.
    pub unit: Option<Dim<Id<Commodity>>>,
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
    /// `known-as PATTERN`: how the code appears in memos (§14).
    pub known_as: Box<[Id<Pattern>]>,
    pub loc: Loc,
}

#[derive(Clone, Copy, Debug)]
pub enum CodeScope {
    Places(Sym),
    Kind(Id<Kind>),
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
        self.flows[self.txns.get(txn)?.flows].iter().find(|flow| flow.to == place)
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
