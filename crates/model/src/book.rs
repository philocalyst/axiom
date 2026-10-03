//! The book: everything the sources declare and record, resolved and typed.
//!
//! Names are interned [`Sym`]s and references are typed ids, so nothing below
//! carries a lifetime except the [`Book`] itself, which owns the interner that
//! borrows the source text. Every hierarchy is a pre-ordered [`Tree`]: "is this
//! place under that one", "is this kind a 401k", and "does this jurisdiction
//! include that one" are all interval tests.

use std::cmp::Ordering;

use axiom_core::calendar::Window;
use axiom_core::{
    Arena, Day, Days, Dim, Facts, Groups, Id, Interner, Loc, Map, Qty, Ratio, Run, Span, Sym, Timeline, Tree, calendar,
};

use crate::addresses::Addresses;
use crate::holders::HolderIndex;
use crate::journal::{
    Assert, ClaimChange, Detail, EndEvent, Event, Filed, Flow, FlowView, Measure, Prices, Program, Purposed, Reading,
    RuntimeDetail, RuntimeFlow, Select, Split, Txn, Waive, WrittenOccurrence,
};
use crate::law::{Fault, Law, NodeId, Rules, Value};
use crate::names::{Found, Names, Scoped};
use crate::slots::{Schema, Slot};
use crate::split::{Expr, Item, Promised, Says, Sign};
use crate::sync::{Format, Pattern, Source};

pub use axiom_core::{Cadence, On, Period};
pub use axiom_syntax::{EventState, Policy};

pub struct Book<'s> {
    pub names: Interner<'s>,
    /// Decoded escaped strings. Unescaped source strings stay as interned
    /// symbols; only text whose meaning differs from its source bytes enters
    /// this compact pool.
    pub text_values: Arena<TextString>,
    /// The currency that basis, totals and net worth are counted in.
    pub base: Id<Commodity>,
    /// `relaxed`: law violations are warnings.
    pub relaxed: bool,
    pub roots: Roots,

    pub places: Tree<Place>,
    /// Sparse outside endpoints for commodities whose kind chain declares
    /// `pays`, keyed by commodity id.
    pub issuer_places: Map<Id<Commodity>, Id<Place>>,
    pub entities: Tree<Entity>,
    pub kinds: Tree<Kind>,
    /// What every kind's things have, and what each takes.
    pub schema: Schema,
    /// The things above, numbered for the facts.
    pub holders: HolderIndex,
    /// Everything the book says of its things, as steps on days: the values of the slots.
    pub facts: Facts,
    /// Where the lines of the language that a diagnostic points back to were written, by the number of the thing, the
    /// number of the slot and, for a slot of several, the member: see [`Book::site`].
    pub sites: Map<(u32, u32, u32), Loc>,
    /// What flows are for: `income`, `spending`, `capital` and the tree beneath
    /// them, pre-ordered so "is groceries food" is an interval test.
    pub purposes: Tree<Purpose>,
    pub systems: Tree<System>,
    pub commodities: Arena<Commodity>,
    /// Identified things: `condo`, `laptop`.
    pub assets: Arena<Asset>,
    /// Promises of flows: `phone with mint`, `mortgage with rocket`.
    pub contracts: Arena<Contract>,
    /// What each contract promises, compiled once when the book is built: its terms and the schedules they fall due on.
    pub promises: crate::promise::Promises,
    /// `also ITEM | FLOW`: what every matching flow implies, declared once.
    pub also: Arena<Also>,

    pub laws: Arena<Law>,
    pub rules: Rules,
    /// `budget food 900 USD monthly`, one per budgeted purpose.
    pub budgets: Arena<Budget>,
    pub params: Arena<Param>,
    pub schedules: Arena<Schedule>,
    /// Code placement rules declared by the book. Flow codes live in the
    /// separate flat `codes` arena below.
    pub code_rules: Vec<CodeRule>,
    /// Flow codes, shared by ranges so cloning a forecast flow copies no text.
    pub codes: Arena<Sym>,
    /// Resolved lot selectors used by flows, in one flat arena.
    pub selectors: Arena<Select>,
    /// Rare per-flow facts. Empty details are represented by `None`.
    pub details: Arena<Detail>,
    /// Every pattern that recognizes a memo: named ones (`pattern ach = …`) and
    /// the `known-as` of things.
    pub patterns: Arena<Pattern>,
    /// `format NAME`: how a source's records read.
    pub formats: Arena<Format>,

    pub txns: Arena<Txn>,
    /// Computed journal expressions and grouped line items. Only transactions
    /// that need them have a program handle in `Txn`.
    pub journal_programs: Arena<Program>,
    /// Exact scheduled identities for written contract occurrences. Ordinary
    /// transactions allocate nothing in this sparse pool.
    pub written_occurrences: Arena<WrittenOccurrence>,
    /// Occurrence inputs in the order of the contract terms' `inputs` list.
    /// Each transaction stores a range so the common case of no named inputs
    /// does not allocate, and forecasts can borrow the bindings directly.
    pub input_values: Arena<Option<Amount>>,
    /// Sorted by day; within a day, in declaration order (files in path order,
    /// then source order). Order of declaration decides ties.
    pub flows: Arena<Flow>,
    /// Every flow touching each place (as source or target), in flow order.
    pub touching: Groups<Place, Id<Flow>>,
    /// Sorted by day, then declaration order.
    pub asserts: Vec<Assert>,
    /// Sparse typed expression programs retained by computed value assertions.
    /// Most assertions are written literals and allocate no program.
    pub assertion_programs: Arena<Program>,
    /// Sorted by day, then declaration order.
    pub events: Vec<Event>,
    /// Sorted by day, then source order. Includes promise/place ends and asset
    /// disposals; assets are consumed by the engine without a synthetic flow.
    pub endings: Vec<EndEvent>,
    /// Sorted by day and source order. Claim changes target the source
    /// transaction so itemized claims remain one atomic reference.
    pub claim_changes: Vec<ClaimChange>,
    pub prices: Prices,
    /// Sorted by day, then declaration order.
    pub splits: Vec<Split>,
    /// Work done and things used, sorted by day, then declaration order.
    pub measures: Arena<Measure>,
    /// Named values, sorted by code, then day.
    pub readings: Vec<Reading>,
    /// Returns as filed, in the order they were written.
    pub filed: Vec<Filed>,
    /// The `sync` declarations: where facts from outside come from.
    pub sources: Vec<Source>,
    /// How names are found. [`build`](crate::build) fills it; in a book made by
    /// hand it is empty, and `Book::place` and its siblings find nothing.
    pub lookup: Lookup,
}

/// A string stored only when source escapes were decoded.
#[derive(Debug)]
pub struct TextString(Box<str>);

/// A model string: borrowed source text or an id in the book's decoded-text
/// pool. The handle is `Copy`, so values and journal records stay allocation
/// free after lowering.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Text {
    Borrowed(Sym),
    Owned(Id<TextString>),
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
    /// The accounts, by the entities that fill their slots: made once the facts are frozen.
    pub(crate) addresses: Addresses,
}

/// Built-in things every book has.
#[derive(Clone, Copy, Debug)]
pub struct Roots {
    /// The person keeping the book. Owns every place by default.
    pub me: Id<Entity>,
    /// `?`: the unknown party. Value of unknown origin comes from it and
    /// unexplained value goes to it.
    pub unknown: Id<Entity>,
    /// Where `opening` holdings come from: an outside place no law watches.
    pub opening: Id<Entity>,
    /// The market, a party: flows with it are revaluations. A flow between an
    /// asset place and the market's place changes quantity and keeps basis: it
    /// realizes nothing.
    pub market: Id<Entity>,
    /// Root kinds of accounts, by class.
    pub kinds: KindRoots,
    /// The roots of the purpose tree.
    pub purposes: PurposeRoots,
}

/// Built-in kind roots, named so callers never rely on arena positions.
#[derive(Clone, Copy, Debug)]
pub struct KindRoots {
    pub asset: Id<Kind>,
    pub debt: Id<Kind>,
    pub thing: Id<Kind>,
    pub commodity: Id<Kind>,
    pub measure: Id<Kind>,
    pub entity: Id<Kind>,
    /// What a party owes an owner: the kind of an `Asset`-class tab, which says `claim`, so that its parcels stay apart.
    pub claim: Id<Kind>,
    /// What an owner owes a party: the kind of a `Debt`-class tab, which says `claim` as well.
    pub debt_claim: Id<Kind>,
}

/// The four disjoint roots of the purpose tree.
#[derive(Clone, Copy, Debug)]
pub struct PurposeRoots {
    pub income: Id<Purpose>,
    pub spending: Id<Purpose>,
    pub capital: Id<Purpose>,
    pub transfer: Id<Purpose>,
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

/// A place value can be: `assets/bank/checking`, `expenses/food`, `income/salary`.
pub struct Place {
    /// The full path.
    pub path: Sym,
    pub class: Class,
    pub role: Role,
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    /// `owner me 50%, jordan 50%`: who owns it, in what shares. Empty for one
    /// owner, which is `owner`.
    pub shares: Box<[Share]>,
    /// `known-as PATTERN, …`: what recognizes it in a statement's memo (§14).
    pub known_as: Box<[Id<Pattern>]>,
    pub doc: Option<Sym>,
    /// `None` for places opened implicitly by a full path.
    pub loc: Option<Loc>,
}

/// Where a report lists a place. The places the book declares come in the tree's order, which is by path. The claim tabs
/// come after them, and the tree has them in the order their claims were first recorded, which nothing a reader sees
/// says: a reader finds a claim by whom it is with, so they are listed by that party's name, what is owed to the owner
/// before what the owner owes, then by owner.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Listing<'s> {
    /// The place's number in the tree, and past every one of them for a tab.
    at: u32,
    with: &'s str,
    class: Class,
    owner: Option<Id<Entity>>,
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
    /// A commodity issuer is a party in its own right, distinct for each
    /// commodity even when several commodities share the same kind.
    Issuer(Id<Commodity>),
    /// Claims between a party and an owner: an `Asset`-class tab holds what the
    /// party owes, a `Debt`-class tab what the owner owes it. Tabs are `claim`
    /// places: each claim stays its own parcel. A tab is not declared: the first claim or loan that
    /// needs it makes it, as the last root of the tree, and `Book::listing` says where reports list it.
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
    /// `owner me` on a business: it is one of the owners, owned by that one.
    pub owner: Option<Id<Entity>>,
    /// `of studio` on a client: what it pays is that owner's.
    pub client_of: Option<Id<Entity>>,
    /// `owner me 60%, theo 40%` on a business: its tallies reach them in these
    /// shares. Empty for a sole owner (`owner`).
    pub owned_by: Box<[Share]>,
    /// `known-as PATTERN, …`: what recognizes it in a statement's memo (§14).
    pub known_as: Box<[Id<Pattern>]>,
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
    /// The slots this kind declares itself, a run of [`Schema`]'s: its things have these and its ancestors'.
    pub slots: Run<Slot>,
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

/// A declared relationship together with the line that established it.
/// Keeping the source beside its resolved value lets diagnostics identify the
/// actual setting even after declarations have been lowered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct At<T> {
    pub value: T,
    pub loc: Loc,
}

/// A unit of account: `USD`, `VTI`, `BTC`, `HOUSE`.
pub struct Commodity {
    pub symbol: Sym,
    pub kind: Id<Kind>,
    /// Decimal places: declared, or the most seen in any written amount.
    pub scale: u8,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// A node of the purpose tree: `groceries : food`.
pub struct Purpose {
    pub name: Sym,
    /// Which of the four roots it descends from.
    pub root: PurposeRoot,
    pub system: Option<Id<System>>,
    /// `of KIND`: it takes an object of this kind (`improvement of thing`).
    pub of: Option<At<Id<Kind>>>,
    /// `business 12% for studio`: every flow of this purpose is shared.
    pub shares: Box<[Share]>,
    /// Only this purpose's own laws; ancestors' laws are found through the tree.
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

impl Purpose {
    /// The four disjoint purpose roots, in stable declaration order.
    pub fn roots<'s>(names: &mut Interner<'s>) -> (Tree<Purpose>, [Id<Purpose>; 4]) {
        let roots = [
            ("income", PurposeRoot::Income),
            ("spending", PurposeRoot::Spending),
            ("capital", PurposeRoot::Capital),
            ("transfer", PurposeRoot::Transfer),
        ];
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
        let (tree, ids) = Tree::build(items.into(), &[None; 4]).expect("roots have no parents");
        (tree, [ids[0], ids[1], ids[2], ids[3]])
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
    /// shared among its parts by their measures (`area`), which are slots.
    pub part_of: Option<At<Id<Asset>>>,
    pub doc: Option<Sym>,
    pub loc: Loc,
}

/// A promise of flows with one party.
pub struct Contract {
    pub name: Sym,
    pub party: Id<Entity>,
    /// Whose promise: the owner of the holding it pays from or into.
    pub owner: Id<Entity>,
    /// The purpose inferred for each promised flow, when declared.
    pub purpose: Option<At<crate::journal::Purposed>>,
    /// The promised flow's description.
    pub description: Option<Text>,
    /// `area 1_000 SQFT`: the denominator for measured contract shares.
    pub area: Option<Amount>,
    /// `from … until …`, cut short by `ends` or extended by a statement: the
    /// days anything is expected at all.
    pub days: Days,
    /// The regular payment schedule, then each dated change (LANGUAGE §5).
    /// Absent for a standing-order-only contract; no schedule is represented
    /// by invented active or waived terms.
    pub terms: Option<Timeline<Terms>>,
    /// An optional standing `buy` order. Its cadence and changes are independent
    /// of the contract's regular payment schedule.
    pub standing: Option<Timeline<Terms>>,
    /// `buy VTI for 500 USD`: occurrences say how much was bought.
    pub buys: Option<Id<Commodity>>,
    /// `deposit 2_350 USD`: a claim the party holds, and money held for it,
    /// over `days`.
    pub deposit: Option<Amount>,
    /// The holding account named by `deposit ... into HOLDING`.
    pub deposit_holding: Option<Id<Place>>,
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
    /// One occurrence's typed flow groups, dated `anchor`. Each group retains
    /// its header, split legs, and line items. Computed amounts point into
    /// `program`; the engine evaluates them with this occurrence's inputs.
    pub template: Box<[Promised]>,
    /// Shared law IR for the computed sides of this term's flow templates.
    pub program: Program,
    /// `input water USD`: names occurrences may state (`water = 155.00 USD`).
    pub inputs: Box<[Input]>,
    /// `about`: each occurrence states its own amount; the template's is the
    /// forecast's estimate, and promises do not compare amounts.
    pub estimate: bool,
    /// `due 5d else + 5% #late-fee`.
    pub due: Option<Deadline>,
    /// Explicit tolerance for a late occurrence. `None` derives half the
    /// actual adjacent due-date interval for this schedule.
    pub grace: Option<Span>,
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
    /// The `else` item, compiled into the enclosing term's shared program;
    /// `None` if the deadline only makes the claim late.
    pub otherwise: Option<Item<Says>>,
}

/// Which of a contract's independent schedules an occurrence names.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScheduleKind {
    Regular,
    Standing,
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
    /// The legacy single-flow iterator cannot materialize grouped templates.
    GroupedTemplate,
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
    UnsupportedFeature {
        feature: ForecastFeature,
        day: Day,
    },
    /// A ratio past what `Ratio` holds (an escalation compounded over the years from a contract with no start,
    /// which counts from `Day::MIN`: K5 removes that anchor), or a span that runs off the calendar.
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
    pub description: Option<Text>,
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
    /// promise identified by those terms.
    pub fn covers(&self, template: &Flow, day: Day) -> ContractCoverage {
        if !self.days.contains(day) {
            return ContractCoverage::None;
        }
        let regular = self
            .terms
            .as_ref()
            .map_or(ContractCoverage::None, |terms| coverage_in_timeline(terms, self.days, template, day));
        let standing = self
            .standing
            .as_ref()
            .map_or(ContractCoverage::None, |terms| coverage_in_timeline(terms, self.days, template, day));
        match (regular, standing) {
            (ContractCoverage::Active, _) | (_, ContractCoverage::Active) => ContractCoverage::Active,
            (ContractCoverage::Waived, _) | (_, ContractCoverage::Waived) => ContractCoverage::Waived,
            _ => ContractCoverage::None,
        }
    }

    /// The scheduled occurrences in `within`, borrowing the terms that govern
    /// each one. Terms changes split the schedule; waived stretches yield none.
    pub fn occurrences(&self, within: Days) -> impl Iterator<Item = ContractOccurrence<'_>> + '_ {
        let window = within.intersect(self.days);
        let search = window.unwrap_or(within);
        let regular = occurrences_for(self.terms.as_ref(), search, window.is_some(), ScheduleKind::Regular);
        let standing = occurrences_for(self.standing.as_ref(), search, window.is_some(), ScheduleKind::Standing);
        ContractOccurrences { regular: regular.peekable(), standing: standing.peekable() }
    }

    /// The days occurrences fall due in `within`, in order. Kept as a
    /// collecting convenience for callers that need owned dates.
    pub fn due_days(&self, within: Days) -> Vec<Day> {
        self.occurrences(within).map(|occurrence| occurrence.day).collect()
    }

    /// The multiplier for the terms in force on `day`, including any
    /// anniversary rise or the named index's movement since the contract began.
    pub fn amount_on(&self, book: &Book<'_>, day: Day) -> Result<Ratio, ForecastError> {
        self.amount_on_schedule(book, ScheduleKind::Regular, day)
    }

    /// The multiplier for a specific independent schedule in force on `day`.
    pub fn amount_on_schedule(
        &self,
        book: &Book<'_>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Result<Ratio, ForecastError> {
        if !self.days.contains(day) {
            return Err(ForecastError::OutsideContract(day));
        }
        let terms = self.terms_on_schedule(schedule, day).ok_or(ForecastError::OutsideContract(day))?;
        if terms.is_waived() {
            return Err(ForecastError::Waived(day));
        }
        let amount = match terms.escalation {
            None => Ok(Ratio::ONE),
            Some(Escalation::Rising(rate)) => {
                let yearly = Ratio::ONE.checked_add(rate).ok_or(ForecastError::Overflow)?;
                if yearly.is_negative() {
                    return Err(ForecastError::InvalidRate);
                }
                let (_, years) = calendar::anniversary(self.days.first(), day).ok_or(ForecastError::Overflow)?;
                ratio_pow(yearly, u32::try_from(years).map_err(|_| ForecastError::Overflow)?)
            }
            Some(Escalation::Indexed(param)) => {
                let first = self.days.first();
                let (anniversary, _) = calendar::anniversary(first, day).ok_or(ForecastError::Overflow)?;
                let base = index_at(book, param, first)?;
                let current = index_at(book, param, anniversary)?;
                current.checked_div(base).ok_or(ForecastError::Overflow)
            }
        }?;
        if !terms.prorated {
            return Ok(amount);
        }
        let period =
            self.recognition_period_for_schedule(schedule, day)?.ok_or(ForecastError::UnsupportedProration(day))?;
        amount.checked_mul(prorated_share(self.days, period)?).ok_or(ForecastError::Overflow)
    }

    /// The recognition window for one occurrence, including its relative
    /// `for` period or `covers` rule when present.
    pub fn recognition_on(&self, template: &Flow, day: Day) -> Result<Days, ForecastError> {
        self.recognition_on_schedule(template, ScheduleKind::Regular, day)
    }

    /// The recognition window for one occurrence in its independent schedule.
    pub fn recognition_on_schedule(
        &self,
        template: &Flow,
        schedule: ScheduleKind,
        day: Day,
    ) -> Result<Days, ForecastError> {
        if !self.days.contains(day) {
            return Err(ForecastError::OutsideContract(day));
        }
        let Some(terms) = self.terms_on_schedule(schedule, day) else {
            return Err(ForecastError::OutsideContract(day));
        };
        if terms.is_waived() {
            return Err(ForecastError::Waived(day));
        }
        // An explicit `for` or `covers` window controls recognition even when
        // only part of it overlaps the contract. Proration scales the amount
        // by that overlap; it does not move recognition outside the declared window.
        if let Some(period) = self.recognition_period_for_schedule(schedule, day)? {
            return Ok(period);
        }
        let shift = day.0.checked_sub(template.day.0).ok_or(ForecastError::Overflow)?;
        Ok(template.recognized.moved(shift))
    }

    fn recognition_period_for_schedule(&self, schedule: ScheduleKind, day: Day) -> Result<Option<Days>, ForecastError> {
        let terms = self.terms_on_schedule(schedule, day).ok_or(ForecastError::OutsideContract(day))?;
        if terms.period.is_some() && terms.covers.is_some() {
            return Err(ForecastError::ConflictingRecognition(day));
        }
        match (terms.period, terms.covers) {
            (Some(Relative::Last(period)), None) => Ok(Some(Window::containing(period, day).previous().days())),
            (Some(Relative::LastQuarter), None) => Ok(Some(calendar::quarter(day, -1))),
            (None, Some(Coverage::Calendar(period))) => Ok(Some(Window::containing(period, day).days())),
            (None, Some(Coverage::Quarter)) => Ok(Some(calendar::quarter(day, 0))),
            (None, Some(Coverage::Span(span))) => Ok(Some(covered_span(day, span)?)),
            (None, None) => Ok(None),
            (Some(_), Some(_)) => Err(ForecastError::ConflictingRecognition(day)),
        }
    }

    /// The terms in force on `day`.
    pub fn terms_on(&self, day: Day) -> Option<&Terms> {
        self.terms.as_ref().map(|terms| terms.at(day))
    }

    /// The terms in force on `day` for one independent schedule, if it exists.
    pub fn terms_on_schedule(&self, schedule: ScheduleKind, day: Day) -> Option<&Terms> {
        match schedule {
            ScheduleKind::Regular => self.terms.as_ref().map(|terms| terms.at(day)),
            ScheduleKind::Standing => self.standing.as_ref().map(|terms| terms.at(day)),
        }
    }
}

fn occurrences_for<'a>(
    timeline: Option<&'a Timeline<Terms>>,
    within: Days,
    enabled: bool,
    schedule: ScheduleKind,
) -> impl Iterator<Item = ContractOccurrence<'a>> + 'a {
    timeline.into_iter().flat_map(move |timeline| {
        timeline.within(within).filter(move |(_, terms)| enabled && !terms.is_waived()).flat_map(
            move |(stretch, terms)| {
                let days = stretch.intersect(within).expect("timeline stretch intersects its window");
                calendar::due(terms.every, &terms.on, terms.anchor, days).map(move |day| ContractOccurrence {
                    day,
                    schedule,
                    terms,
                })
            },
        )
    })
}

fn template_covers_flow(terms: &Terms, template: &Flow) -> bool {
    terms.template.iter().any(|candidate| {
        same_flow_kind(template, &candidate.header.flow)
            || candidate.legs.iter().any(|leg| same_flow_kind(template, &leg.flow))
    })
}

fn coverage_in_timeline(timeline: &Timeline<Terms>, within: Days, template: &Flow, day: Day) -> ContractCoverage {
    let contains = |terms: &Terms| template_covers_flow(terms, template);
    let current = timeline.at(day);
    if !current.is_waived() {
        return if contains(current) { ContractCoverage::Active } else { ContractCoverage::None };
    }
    if contains(current) {
        return ContractCoverage::Waived;
    }
    let nearest = timeline
        .within(within)
        .filter(|(_, candidate)| !candidate.is_waived())
        .map(|(stretch, terms)| {
            let stretch = stretch.intersect(within).expect("timeline stretch intersects the contract");
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
    if nearest.is_some_and(|(_, _, terms)| contains(terms)) { ContractCoverage::Waived } else { ContractCoverage::None }
}

struct ContractOccurrences<I: Iterator> {
    regular: std::iter::Peekable<I>,
    standing: std::iter::Peekable<I>,
}

impl<'a, I> Iterator for ContractOccurrences<I>
where
    I: Iterator<Item = ContractOccurrence<'a>>,
{
    type Item = ContractOccurrence<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match (self.regular.peek(), self.standing.peek()) {
            (Some(regular), Some(standing)) if standing.day < regular.day => self.standing.next(),
            (Some(_), _) => self.regular.next(),
            (None, Some(_)) => self.standing.next(),
            (None, None) => None,
        }
    }
}

fn same_flow_kind(a: &Flow, b: &Flow) -> bool {
    let same_purpose = match (a.purpose, b.purpose) {
        (Some(a), Some(b)) => a.purpose == b.purpose && a.of == b.of,
        (None, None) => true,
        _ => false,
    };
    (a.from, a.to, a.out.unit, a.arrive.unit, a.owner, a.payee)
        == (b.from, b.to, b.out.unit, b.arrive.unit, b.owner, b.payee)
        && same_purpose
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

/// The index `param` stood at on `day`: its latest row at or before it that has no keys.
fn index_at(book: &Book<'_>, param: Id<Param>, day: Day) -> Result<Ratio, ForecastError> {
    let missing = ForecastError::MissingIndex { param, day };
    let row = book.params.get(param).and_then(|data| data.row(day, &[])).ok_or(missing)?;
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

fn covered_span(start: Day, span: Span) -> Result<Days, ForecastError> {
    let after = start.checked_add(span).ok_or(ForecastError::Overflow)?;
    if after <= start {
        return Err(ForecastError::InvalidCoverage(start));
    }
    Days::new(start, Day(after.0 - 1)).ok_or(ForecastError::InvalidCoverage(start))
}

/// One contract occurrence and the terms that govern its flow template.
#[derive(Clone, Copy, Debug)]
pub struct ContractOccurrence<'a> {
    /// The day this scheduled payment falls due.
    pub day: Day,
    /// The independent cadence this occurrence belongs to. Equal-day ties
    /// emit Regular before Standing.
    pub schedule: ScheduleKind,
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
    pub description: Option<Text>,
    /// Pooled metadata written on the implied item or flow.
    pub codes: Run<Sym>,
    pub select: Run<Select>,
    pub detail: Option<Id<Detail>>,
    pub waive: Option<Waive>,
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
    Item { sign: Sign, amount: Expr },
    /// `lumen -> retirement 50% of …`, `-> escrow 410 USD`: a flow of its own.
    /// `None` ends mean the implying flow's own ends (`issuer -> self`).
    Flow { from: Option<Id<Place>>, to: Option<Id<Place>>, amount: Expr },
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
    /// The first day this budget has an active allowance. Earlier dates have
    /// no budget, rather than an implicit zero or unlimited allowance.
    pub starts: Day,
    /// The complete terms in force, with date changes restoring the preceding
    /// terms after a bounded `until` interval.
    pub terms: Timeline<BudgetTerms>,
    /// The law that reports it (`warn total(window) <= limit`), so violations,
    /// headroom and `why` treat a budget as every other cap. Its `Law::budget`
    /// points back here.
    pub law: Id<Law>,
    pub loc: Loc,
}

/// The effective allowance and policy at one date.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetTerms {
    pub limit: Limit,
    pub period: Period,
    /// Judged on the total since it began against its limits summed through
    /// the window: an unspent month lends to the next, an overspent one borrows.
    pub carries: bool,
    /// `funded from HOLDING into HOLDING`: its limit moves each window into
    /// money held for it, which what the purpose spends is drawn from first.
    pub funded: Option<(Id<Place>, Id<Place>)>,
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
    /// A law expression, compiled into the node arena of `Budget::law`.
    Computed(crate::law::NodeId),
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

/// A conversion together with the exact evidence used to obtain its rate.
/// The variants make it impossible to attach quote evidence to a zero amount
/// or to mistake that amount for a cross-commodity identity rate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Conversion {
    Identity {
        amount: Amount,
    },
    /// A zero amount converted across commodities without requiring a quote.
    Zero {
        amount: Amount,
    },
    Rates {
        amount: Amount,
        rate: Ratio,
        path: RatePath,
    },
}

impl Conversion {
    pub const fn amount(self) -> Amount {
        match self {
            Self::Identity { amount } | Self::Zero { amount } | Self::Rates { amount, .. } => amount,
        }
    }

    pub const fn rate(self) -> Option<Ratio> {
        match self {
            Self::Identity { .. } => Some(Ratio::ONE),
            Self::Zero { .. } => None,
            Self::Rates { rate, .. } => Some(rate),
        }
    }

    pub const fn path(self) -> Option<RatePath> {
        match self {
            Self::Rates { path, .. } => Some(path),
            Self::Identity { .. } | Self::Zero { .. } => None,
        }
    }
}

/// One direct or inverse rate, or the two rates used through the base unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RatePath {
    pub first: RateUse,
    pub second: Option<RateUse>,
}

/// A rate applied from one commodity to another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateUse {
    pub from: Id<Commodity>,
    pub to: Id<Commodity>,
    pub rate: Ratio,
    pub source: RateSource,
}

/// The declaration selected for one rate leg, kept typed for `why`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateSource {
    Spot { quoted: Id<Commodity>, quote: Id<Commodity>, as_of: Day, inverted: bool, implied: bool, loc: Loc },
    Param { param: Id<Param>, row: u32, since: Option<Day>, inverted: bool, loc: Loc },
}

/// Why a requested conversion failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConversionError {
    Missing {
        from: Id<Commodity>,
        to: Id<Commodity>,
        day: Day,
        policy: RatePolicy,
    },
    /// Two simultaneously active, unrelated residences select different
    /// policies. Their stable ids let diagnostics point at both declarations.
    PolicyConflict {
        first: Id<System>,
        second: Id<System>,
    },
    Overflow,
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

impl Param {
    /// The row with exactly `keys` whose date is latest at or before `day`.
    /// Rows are grouped by their complete name-key tuple, then by date.
    pub fn row(&self, day: Day, keys: &[Sym]) -> Option<&ParamRow> {
        self.row_index(day, keys).map(|(_, row)| row)
    }

    /// The stable index and row with exactly `keys` whose date is latest at
    /// or before `day`. Complete key tuples and then dates must be sorted.
    /// Both tuple bounds and the dated row are found by binary search.
    pub fn row_index(&self, day: Day, keys: &[Sym]) -> Option<(u32, &ParamRow)> {
        self.row_index_by(day, |row_keys| row_keys.cmp(keys))
    }

    /// The stable index and row using an allocation-free comparison against a
    /// caller's typed key source. The comparator orders a row's full name
    /// tuple against the requested tuple.
    pub fn row_index_by(&self, day: Day, mut compare_keys: impl FnMut(&[Sym]) -> Ordering) -> Option<(u32, &ParamRow)> {
        let first = self.rows.partition_point(|row| compare_keys(&row.names) == Ordering::Less);
        if first == self.rows.len() || compare_keys(&self.rows[first].names) != Ordering::Equal {
            return None;
        }
        let after = first + self.rows[first..].partition_point(|row| compare_keys(&row.names) != Ordering::Greater);
        let matching = &self.rows[first..after];
        let upto = matching.partition_point(|row| row.since.is_none_or(|since| since <= day));
        let local_index = upto.checked_sub(1)?;
        let index = first + local_index;
        Some((u32::try_from(index).ok()?, &self.rows[index]))
    }
}

#[cfg(test)]
mod param_lookup_tests {
    use super::*;

    fn row(names: &[Sym], since: Option<Day>, value: i64) -> ParamRow {
        ParamRow { since, names: names.into(), value: Value::Num(Ratio::int(value)), loc: Loc::default() }
    }

    #[test]
    fn row_lookup_binary_searches_full_key_tuple_and_date() {
        let mut names = Interner::default();
        let family = names.intern("family");
        let missing_middle = names.intern("individual");
        let self_only = names.intern("self-only");
        let missing_end = names.intern("zzz");
        let (param_name, family_key) = (names.intern("limit"), [family]);
        let rows = vec![
            row(&family_key, Some(Day::from_ymd(2025, 1, 1).unwrap()), 100),
            row(&family_key, Some(Day::from_ymd(2026, 1, 1).unwrap()), 200),
            row(&[self_only], Some(Day::from_ymd(2025, 1, 1).unwrap()), 50),
        ];
        let param = Param { name: param_name, unit: None, system: None, rows: rows.into(), loc: Loc::default() };

        let day = Day::from_ymd(2026, 6, 1).unwrap();
        let (index, row) = param.row_index(day, &family_key).unwrap();
        assert_eq!(index, 1);
        assert_eq!(row.value, Value::Num(Ratio::int(200)));
        assert_eq!(
            param.row_index_by(day, |row_keys| row_keys.cmp(&family_key)).unwrap().0,
            index,
            "the borrowed comparator selects the same full tuple without a key vector"
        );
        assert_eq!(param.row_index(day, &[self_only]).unwrap().0, 2);
        assert!(param.row_index(day, &[missing_middle]).is_none());
        assert!(param.row_index(day, &[missing_end]).is_none());
        assert!(param.row_index(Day::from_ymd(2024, 12, 31).unwrap(), &[self_only]).is_none());
    }
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

    /// Where a report lists `place`, so that no report lists places in the order the model made them in.
    pub fn listing(&self, place: Id<Place>) -> Listing<'s> {
        let tab = &self.places[place];
        match tab.role {
            Role::Tab(_) => {
                Listing { at: u32::MAX, with: self.name(tab.path), class: tab.class, owner: Some(tab.owner) }
            }
            _ => Listing { at: place.index() as u32, with: "", class: Class::Asset, owner: None },
        }
    }

    /// Every place in the order a report lists them: see [`Listing`].
    pub fn listed_places(&self) -> Vec<Id<Place>> {
        let mut places: Vec<_> = self.places.ids().collect();
        places.sort_by_key(|&place| self.listing(place));
        places
    }

    /// Adds unescaped source text without an allocation.
    pub fn intern_text(&mut self, text: &'s str) -> Text {
        Text::Borrowed(self.names.intern(text))
    }

    /// Decodes the four escapes accepted by the lexer. Strings without an
    /// escape remain source-borrowed; decoded strings get one pooled allocation.
    pub fn quoted_text(&mut self, raw: &'s str) -> Text {
        if !raw.as_bytes().contains(&b'\\') {
            return self.intern_text(raw);
        }
        let decoded = decode_quoted(raw);
        if let Some(sym) = self.names.get(&decoded) {
            return Text::Borrowed(sym);
        }
        if let Some((id, _)) = self.text_values.iter().find(|(_, text)| text.0.as_ref() == decoded) {
            return Text::Owned(id);
        }
        Text::Owned(self.text_values.push(TextString(decoded.into_boxed_str())))
    }

    /// Gets text with a borrow tied to the book that owns any decoded value.
    pub fn text(&self, text: Text) -> &str {
        match text {
            Text::Borrowed(sym) => self.names.name(sym),
            Text::Owned(id) => &self.text_values[id].0,
        }
    }

    /// A place by full path or unique suffix (`checking`), by the address it is written as (`jordan/401k`), or an
    /// entity's place. A place wins over an entity of the same name.
    pub fn place(&self, text: &str) -> Result<Id<Place>, Miss<Place>> {
        let miss = match self.lookup.places.resolve(&self.names, text, |_| true) {
            Ok(place) => return Ok(place),
            Err(miss) => miss,
        };
        // The names found nothing, or several and one is written as an address: the index may tell which is meant.
        let by_address = match &miss {
            Miss::Unknown { .. } => true,
            Miss::Ambiguous(places) => places.iter().any(|&place| self.is_spelled(place)),
        };
        match self.address_place(text) {
            Found::One(place) if by_address => return Ok(place),
            Found::Several(places) if by_address => return Err(Miss::Ambiguous(places.into())),
            // Every account the names found is never open: there is none to mean, as for a line on any day.
            Found::Nothing if by_address && matches!(miss, Miss::Ambiguous(_)) => {
                return Err(Miss::Unknown { suggestion: None });
            }
            _ => {}
        }
        match (&miss, self.entity(text)) {
            (Miss::Unknown { .. }, Ok(entity)) => self.entities[entity].place.ok_or(miss),
            _ => Err(miss),
        }
    }

    pub fn entity(&self, text: &str) -> Result<Id<Entity>, Miss<Entity>> {
        self.lookup.entities.names.resolve(&self.names, text, |_| true)
    }

    pub fn commodity(&self, symbol: &str) -> Option<Id<Commodity>> {
        self.names.get(symbol).and_then(|sym| self.lookup.commodities.get(&sym).copied())
    }

    /// The outside endpoint for this commodity when its kind chain declares
    /// what the issuer pays. `None` for commodities with no such rule.
    pub fn issuer_place(&self, unit: Id<Commodity>) -> Option<Id<Place>> {
        self.issuer_places.get(&unit).copied()
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
        self.convert_at_rate(amount, unit, rate)
    }

    /// Applies an already selected exchange rate using the book's commodity
    /// precision and the same rounding as [`Book::convert`]. Callers that
    /// cache a typed [`RateUse`] can reuse its rate for later amounts without
    /// repeating quote or parameter lookup.
    pub fn convert_at_rate(&self, amount: Amount, unit: Id<Commodity>, rate: Ratio) -> Option<Amount> {
        if amount.unit == unit {
            return Some(amount);
        }
        let (from, to) = (self.commodities[amount.unit].scale, self.commodities[unit].scale);
        Some(Amount::new(crate::prices::rescale(amount.qty, from, to, rate)?, unit))
    }

    /// Converts an amount under an explicit policy or the effective owner's
    /// active residence policy, retaining the exact quote or parameter rows
    /// used. A system without a `rates` setting uses spot quotes.
    ///
    /// For overlapping residences, each residence inherits the nearest
    /// ancestor with a policy. Equal policies agree; different policies are
    /// an error rather than depending on declaration or tree iteration order.
    pub fn convert_for(
        &self,
        amount: Amount,
        unit: Id<Commodity>,
        owner: Id<Entity>,
        day: Day,
        explicit: Option<RatePolicy>,
    ) -> Result<Conversion, ConversionError> {
        if amount.unit == unit {
            return Ok(Conversion::Identity { amount });
        }
        if amount.qty.0 == 0 {
            return Ok(Conversion::Zero { amount: Amount::new(amount.qty, unit) });
        }
        let policy = match explicit {
            Some(policy) => policy,
            None => self.owner_rate_policy(owner, day)?.unwrap_or(RatePolicy::Spot),
        };
        let (rate, path) = self.conversion_path(amount.unit, unit, day, policy).ok_or(ConversionError::Missing {
            from: amount.unit,
            to: unit,
            day,
            policy,
        })?;
        let amount = self.convert_at_rate(amount, unit, rate).ok_or(ConversionError::Overflow)?;
        Ok(Conversion::Rates { amount, path, rate })
    }

    /// The rate policy of the systems an owner lives under on a day: the policy of a system that no other one's sits
    /// beneath, or an error if two of them say different things.
    fn owner_rate_policy(&self, owner: Id<Entity>, day: Day) -> Result<Option<RatePolicy>, ConversionError> {
        let nearest: Vec<_> = self.residing(owner, day).filter_map(|system| self.nearest_rate_policy(system)).collect();
        let sits_beneath = |system, other| other != system && self.systems.covers(system, other);
        let maximal: Vec<_> = nearest
            .iter()
            .copied()
            .filter(|&(system, _)| !nearest.iter().any(|&(other, _)| sits_beneath(system, other)))
            .collect();
        let mut conflict = None::<(Id<System>, Id<System>)>;
        for (index, &(first, policy)) in maximal.iter().enumerate() {
            for &(second, other) in &maximal[index + 1..] {
                if first != second && policy != other {
                    let pair = (first.min(second), first.max(second));
                    conflict = Some(conflict.map_or(pair, |current| current.min(pair)));
                }
            }
        }
        if let Some((first, second)) = conflict {
            return Err(ConversionError::PolicyConflict { first, second });
        }
        Ok(maximal.iter().min_by_key(|(system, _)| *system).map(|&(_, policy)| policy))
    }

    fn nearest_rate_policy(&self, mut system: Id<System>) -> Option<(Id<System>, RatePolicy)> {
        loop {
            let node = &self.systems[system];
            if let Some(policy) = node.rates {
                return Some((system, policy));
            }
            system = self.systems.parent(system)?;
        }
    }

    fn conversion_path(
        &self,
        from: Id<Commodity>,
        to: Id<Commodity>,
        day: Day,
        policy: RatePolicy,
    ) -> Option<(Ratio, RatePath)> {
        if let Some(first) = self.rate_use(from, to, day, policy) {
            return Some((first.rate, RatePath { first, second: None }));
        }
        let first = self.rate_use(from, self.base, day, policy)?;
        let second = self.rate_use(self.base, to, day, policy)?;
        let rate = first.rate.checked_mul(second.rate)?;
        Some((rate, RatePath { first, second: Some(second) }))
    }

    fn rate_use(&self, from: Id<Commodity>, to: Id<Commodity>, day: Day, policy: RatePolicy) -> Option<RateUse> {
        match policy {
            RatePolicy::Spot => self.spot_rate_use(from, to, day),
            RatePolicy::Param(param) => self.param_rate_use(param, from, to, day),
        }
    }

    fn spot_rate_use(&self, from: Id<Commodity>, to: Id<Commodity>, day: Day) -> Option<RateUse> {
        let forward = self.latest_quote(from, to, day);
        let reverse = self.latest_quote(to, from, day).and_then(|quote| Some((quote, quote.rate.recip()?)));
        let (quote, rate, inverted) = match (forward, reverse) {
            (Some(forward), Some((reverse, inverted))) if reverse.day > forward.day => (reverse, inverted, true),
            (Some(forward), _) => (forward, forward.rate, false),
            (None, Some((reverse, inverted))) => (reverse, inverted, true),
            (None, None) => return None,
        };
        (rate.num() > 0).then_some(RateUse {
            from,
            to,
            rate,
            source: RateSource::Spot {
                quoted: quote.unit,
                quote: quote.quote,
                as_of: quote.day,
                inverted,
                implied: quote.implied,
                loc: quote.loc,
            },
        })
    }

    fn latest_quote(&self, from: Id<Commodity>, to: Id<Commodity>, day: Day) -> Option<&crate::journal::Quote> {
        let upto = self.prices.quotes.partition_point(|quote| (quote.unit, quote.quote, quote.day) <= (from, to, day));
        self.prices.quotes[..upto].last().filter(|quote| quote.unit == from && quote.quote == to)
    }

    fn param_rate_use(&self, id: Id<Param>, from: Id<Commodity>, to: Id<Commodity>, day: Day) -> Option<RateUse> {
        let param = &self.params[id];
        let from_name = self.commodities[from].symbol;
        let to_name = self.commodities[to].symbol;
        let direct_unit = Some(Dim::Per(to, from));
        let inverse_unit = Some(Dim::Per(from, to));
        if (param.unit.is_none() || param.unit == Some(Dim::Number) || param.unit == direct_unit)
            && let Some((row, value)) = param.row_index(day, &[from_name, to_name])
            && let Value::Num(rate) = value.value
            && rate.num() > 0
        {
            let source = RateSource::Param { param: id, row, since: value.since, inverted: false, loc: value.loc };
            return Some(RateUse { from, to, rate, source });
        }
        if (param.unit.is_none() || param.unit == Some(Dim::Number) || param.unit == inverse_unit)
            && let Some((row, value)) = param.row_index(day, &[to_name, from_name])
            && let Value::Num(rate) = value.value
        {
            let rate = rate.recip()?;
            if rate.num() > 0 {
                let source = RateSource::Param { param: id, row, since: value.since, inverted: true, loc: value.loc };
                return Some(RateUse { from, to, rate, source });
            }
        }
        None
    }

    /// Borrows a flow with the metadata its compact ranges name.
    pub fn flow_view<'a>(&'a self, flow: &'a Flow) -> FlowView<'a> {
        let detail = flow.detail.map_or(&Detail::NONE, |id| &self.details[id]);
        self.flow_view_parts(flow, detail)
    }

    /// Borrows a flow with a call-local detail override. The flow's codes and
    /// selectors still resolve through the book's immutable pools; no runtime
    /// detail allocation is needed for a single computed expression.
    pub fn flow_view_with_detail<'a>(&'a self, flow: &'a Flow, detail: &'a Detail) -> FlowView<'a> {
        self.flow_view_parts(flow, detail)
    }

    /// Borrows a forecast flow, resolving its optional transformed detail in
    /// the runtime pool and reusing the book's code and selector pools.
    pub fn runtime_flow_view<'a>(
        &'a self,
        runtime: &'a RuntimeFlow,
        details: &'a Arena<RuntimeDetail>,
    ) -> FlowView<'a> {
        let flow = &runtime.flow;
        let detail = runtime
            .detail
            .map(|id| &details[id].0)
            .or_else(|| flow.detail.map(|id| &self.details[id]))
            .unwrap_or(&Detail::NONE);
        self.flow_view_parts(flow, detail)
    }

    fn flow_view_parts<'a>(&'a self, flow: &'a Flow, detail: &'a Detail) -> FlowView<'a> {
        let header_codes = &self.codes[flow.header_codes];
        let local_codes = &self.codes[flow.codes];
        let selectors = &self.selectors[flow.select];
        FlowView::new(flow, header_codes, local_codes, selectors, detail)
    }

    /// Borrows the flow with its pooled metadata by id.
    pub fn flow(&self, id: Id<Flow>) -> FlowView<'_> {
        self.flow_view(&self.flows[id])
    }

    /// Input bindings recorded on one transaction occurrence, aligned to the
    /// active terms' `Terms::inputs` declaration order.
    pub fn txn_inputs(&self, id: Id<Txn>) -> &[Option<Amount>] {
        &self.input_values[self.txns[id].inputs]
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

fn decode_quoted(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        match chars.next().expect("the lexer rejects a trailing backslash") {
            'n' => decoded.push('\n'),
            't' => decoded.push('\t'),
            '"' => decoded.push('"'),
            '\\' => decoded.push('\\'),
            _ => unreachable!("the lexer validates string escapes"),
        }
    }
    decoded
}

#[cfg(test)]
mod text_tests {
    use super::decode_quoted;

    #[test]
    fn decodes_only_the_escapes_accepted_by_the_lexer() {
        assert_eq!(decode_quoted("line\\ncolumn\\tquote\\\"slash\\\\"), "line\ncolumn\tquote\"slash\\");
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
