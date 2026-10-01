// ═══════════════════════════════════════════════════════════════════════════
// The v4 public types: what model, engine and report agree on.
// Apply these to crates/model/src/{book,journal,law}.rs and crates/engine/src/lib.rs.
// `// NEW`, `// CHANGED` and `// REMOVED` mark the deltas; everything unmarked stays.
// ═══════════════════════════════════════════════════════════════════════════

// ─── model/src/book.rs ──────────────────────────────────────────────────────

pub struct Book<'s> {
    pub names: Interner<'s>,
    pub base: Id<Commodity>,
    pub relaxed: bool,
    pub roots: Roots,

    pub places: Tree<Place>,
    pub entities: Tree<Entity>,
    pub kinds: Tree<Kind>,
    /// What flows are for: `income`, `spending`, `capital` and the tree beneath
    /// them, pre-ordered so "is groceries food" is an interval test.       // NEW
    pub purposes: Tree<Purpose>,
    pub systems: Tree<System>,
    pub commodities: Arena<Commodity>,
    /// Identified things: `condo`, `laptop`.                               // NEW
    pub assets: Arena<Asset>,
    /// Promises of flows: `phone with mint`, `mortgage with rocket`.       // NEW
    pub contracts: Arena<Contract>,

    pub laws: Arena<Law>,
    pub rules: Rules,
    pub params: Arena<Param>,
    pub schedules: Arena<Schedule>,
    pub codes: Vec<CodeRule>,

    pub txns: Arena<Txn>,
    pub flows: Arena<Flow>,
    pub touching: Groups<Place, Id<Flow>>,
    pub asserts: Vec<Assert>,
    pub events: Vec<Event>,
    pub prices: Prices,
    pub splits: Vec<Split>,
    /// v3's plans. The v3 model still fills it; the v4 model leaves it empty
    /// and it is deleted once nothing reads it.                             // KEPT FOR NOW
    pub plans: Arena<Plan>,
    pub syncs: Vec<SyncSpec>,
    pub lookup: Lookup,
}

/// CHANGED: `Lookup` gains `purposes`, `assets` and `contracts` (each a
/// `Names<T>` or a `Map<Sym, Id<T>>`, whichever fits) and loses nothing yet.

/// Built-in things every book has.
#[derive(Clone, Copy, Debug)]
pub struct Roots {
    pub me: Id<Entity>,
    /// `?`: the unknown party. Value of unknown origin comes from it and
    /// unexplained value goes to it.                                       // CHANGED (was equity/unknown)
    pub unknown: Id<Place>,
    /// Where `opening` holdings come from: an outside place no law watches. // CHANGED (was equity/opening)
    pub opening: Id<Place>,
    /// The market, a party: flows with it are revaluations.                 // CHANGED (was a kind)
    pub market: Id<Entity>,
    /// Root kinds of accounts, by class.                                    // CHANGED
    pub asset: Id<Kind>,
    pub debt: Id<Kind>,
    /// The root kind of assets (identified things).                         // NEW
    pub thing: Id<Kind>,
    pub commodity: Id<Kind>,
    pub entity: Id<Kind>,
    /// The roots of the purpose tree.                                        // NEW
    pub income: Id<Purpose>,
    pub spending: Id<Purpose>,
    pub capital: Id<Purpose>,
}
// REMOVED from Roots: liability, income (kind), expense, equity, market (kind).

/// Where a place sits: what the owners hold, what they owe, or outside them.
/// CHANGED: was Asset | Liability | Income | Expense | Equity.
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
// display_sign: Asset 1, Debt -1, Outside 1. holds_parcels: Asset.

/// What a place is. CHANGED: `Place` gains `role`, and `alias` is removed.
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

pub struct Place {
    pub path: Sym,
    pub class: Class,
    pub role: Role,                                                     // NEW
    pub kind: Id<Kind>,
    pub owner: Id<Entity>,
    pub holds: Option<Box<[Id<Commodity>]>>,
    pub select: Option<Policy>,
    pub deferred: bool,
    pub basis: Basis,
    pub claim: bool,
    pub liquidity: Option<Span>,
    pub opened: Option<Day>,
    pub closed: Option<Day>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}
// REMOVED: alias.

/// Someone. CHANGED: `via` is removed (a party is its own end); `place`,
/// `owner` and `client_of` are new.
pub struct Entity {
    pub path: Sym,
    pub kind: Id<Kind>,
    /// Its place as a flow's end: an owner's `Holding`, a party's `Outside`.  // NEW
    pub place: Option<Id<Place>>,
    pub restricted: bool,
    pub lives: Box<[Residence]>,
    pub member: Option<Id<Entity>>,
    /// `owner me` on a business: it is one of the owners, owned by that one. // NEW
    pub owner: Option<Id<Entity>>,
    /// `of studio` on a client: what it pays is that owner's.              // NEW
    pub client_of: Option<Id<Entity>>,
    pub props: Props,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}
// The v3 model keeps `via` working internally until it is replaced; `place`
// is then the via place, and `Book::place` resolves an entity to it.

/// CHANGED: `Kind` gains the inference facts, each resolved down the chain.
pub struct Kind {
    pub name: Sym,
    pub sort: Sort,
    pub system: Option<Id<System>>,
    pub restricted: bool,
    pub deferred: bool,
    pub basis: Option<Basis>,
    pub claim: bool,
    pub select: Option<Policy>,
    pub liquidity: Option<Span>,
    /// On a party kind: what flows with its parties are for (`grocer`:
    /// groceries).                                                          // NEW
    pub purpose: Option<Id<Purpose>>,
    /// On a commodity kind: what its issuer pays is for (`fund`: dividend).  // NEW
    pub pays: Option<Id<Purpose>>,
    /// On an account kind: what arrives from flows of the second purpose is
    /// the first (`401k`: pre-tax-deferral from wages).                     // NEW
    pub takes: Box<[(Id<Purpose>, Id<Purpose>)]>,
    /// On a party kind: the tax inside every price paid to its parties.       // NEW
    pub sales_tax: Option<Ratio>,
    /// `business 60% for studio` on a party kind: every flow with its parties
    /// is shared.                                                            // NEW
    pub shares: Box<[Share]>,
    pub has: Box<[Has]>,
    pub props: Props,
    pub laws: Box<[Id<Law>]>,
    pub doc: Option<Sym>,
    pub loc: Option<Loc>,
}

/// CHANGED: `Place(Class)` covers accounts (`Asset`, `Debt`) and, for the
/// v3 model, `Outside`; `Thing` is new, for asset kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sort {
    Place(Class),
    Thing,
    Commodity,
    Entity,
}

/// A node of the purpose tree: `groceries : food`.                         // NEW
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
/// income, earned) by that owner.                                          // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Share {
    pub rate: Ratio,
    pub owner: Id<Entity>,
    pub loc: Loc,
}

/// An identified thing: `asset condo : rental-home`.                       // NEW
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

/// A promise of flows with one party.                                     // NEW
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

/// When a contract's occurrences fall due.                                 // NEW
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
    /// schedule, `until` and `ended`. Model-side, pure.
    pub fn due_days(&self, from: Day, until: Day) -> Vec<Day>;
}

/// `loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo`.          // NEW
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

/// `match 50% of retirement up to 6%`.                                     // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    pub rate: Ratio,
    pub into: Id<Place>,
    /// Of the gross.
    pub up_to: Ratio,
}

// ─── model/src/journal.rs ───────────────────────────────────────────────────

pub struct Flow {
    pub day: Day,
    pub recognized: Recognition,
    pub from: Id<Place>,
    pub to: Id<Place>,
    pub out: Amount,
    pub arrive: Amount,
    pub mode: Mode,
    pub infer: Infer,
    pub txn: Id<Txn>,
    pub payee: Option<Id<Entity>>,
    /// Who bears it, or earns it: tallies and budgets follow this. The owner
    /// of the flow's account ends by default; a share makes it another.   // NEW
    pub owner: Id<Entity>,
    /// What it is for, and why the book thinks so.                          // NEW
    pub purpose: Option<Purposed>,
    /// `"food for the routine"`.                                             // NEW
    pub description: Option<Sym>,
    /// Written, an occurrence of a contract, or derived.                     // NEW
    pub origin: Origin,
    pub select: Box<[Select]>,
    pub codes: Box<[Sym]>,
    pub loc: Loc,
    pub waive: Option<Waive>,
    pub terms: Option<Box<Terms>>,
}

/// A flow's purpose, its object, and where it came from.                   // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Purposed {
    pub purpose: Id<Purpose>,
    /// `of condo`.
    pub of: Option<Object>,
    pub source: Source,
}

/// What a purpose is `of`.                                                   // NEW
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Object {
    Asset(Id<Asset>),
    Place(Id<Place>),
    Entity(Id<Entity>),
}

/// Where a flow's purpose came from, first match winning (LANGUAGE §2).    // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// On the leg or its header.
    Written(Loc),
    Contract(Id<Contract>),
    /// The kind of the party at the flow's other end.
    Party(Id<Kind>),
    /// The kind of the commodity paying, in party position.
    Commodity(Id<Kind>),
    /// A `takes … from …` of an account's kind.
    Account(Id<Kind>),
    /// A derived flow's own purpose (interest, sales tax, a match).
    Derived,
}

/// How a flow came to be.                                                   // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Written,
    /// An occurrence of a contract, written in the journal as `DATE NAME`.
    Occurrence(Id<Contract>),
    /// Implied by something written; never in the journal.
    Derived(Derivation),
}

/// What a derived flow is, and what it came from.                           // NEW
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Derivation {
    /// A loan payment's interest, or its principal.
    Interest(Id<Contract>),
    Principal(Id<Contract>),
    Escrow(Id<Contract>),
    Match(Id<Contract>),
    /// An owner's share of a flow: `business 60% for studio`, from a
    /// contract, a party kind or a purpose. `Loc` is the share's declaration.
    Share(Loc),
    /// The tax inside a price paid to a party with `sales-tax`.
    SalesTax(Id<Kind>),
    /// What an exchange rate cost: what was given less what was got.
    ExchangeCost,
    /// A leg between two parties, split into its two halves through the owner.
    PassThrough,
    /// A contract deposit or a missing occurrence: a claim.
    Claim(Id<Contract>),
}

/// CHANGED: `Terms::basis_end` is removed (no `.basis` places in v4). The v3
/// model keeps reading `PLACE.basis` until it is replaced: keep the field
/// until the v4 model lands, and document it as v3-only.

/// CHANGED: `Txn::plan` becomes `contract: Option<Id<Contract>>`; the v3
/// model keeps filling `plan` for now. Also `Txn` gains:
///   `pub ends: bool` — this is `DATE NAME ends`, and has no flows.       // NEW

// ─── model/src/law.rs ───────────────────────────────────────────────────────

/// CHANGED: `Owner` gains `Purpose(Id<Purpose>)`, `Asset(Id<Asset>)` and
/// `Contract(Id<Contract>)`.
/// CHANGED: `Trigger` gains `Flow`: a flow of the governed purpose (and those
/// beneath it), or, under an asset or an asset kind, a flow whose purpose is
/// `of` it.
/// CHANGED: `Effect` gains:
///   `Consume { amount: NodeId }` — lowers the governed asset part's basis;
///   `Carry { amount: NodeId, unit: NodeId, within: NodeId }` — holds a
///      disallowed loss and adds it to the basis of the nearest acquisition
///      of `unit` within the span, before or after.
/// CHANGED: `Var` gains `Purpose` and `Description`; `Field` gains `Cost`,
///   `InService`, `Parts`, `Of` (a purpose's object); `Func` gains
///   `StraightLine`; `Ty` gains `Purpose` and `Asset`; `Value` gains
///   `Purpose(Id<Purpose>, Option<Object>)` and `Asset(Id<Asset>)`.
/// CHANGED: `Rules` gains
///   `pub purposes: Groups<Purpose, Rule>` — `on flow` laws of each purpose,
///      ancestors' included, in dependency order;
///   `pub about: Groups<Place, Rule>` — `on flow` laws of each asset's place
///      (its kind chain's and its own): flows whose purpose is `of` it.
/// `Subject` gains `Asset(Id<Asset>)`; a purpose law's subject is the flow's
/// owner (`Subject::Entity`).

// ─── engine/src/lib.rs ──────────────────────────────────────────────────────

pub struct Run {
    pub today: Day,
    pub horizon: Day,
    pub posted: Box<[Posted]>,
    pub holdings: Vec<Holding>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    pub violations: Vec<Violation>,
    pub headroom: Vec<Headroom>,
    pub pads: Vec<Pad>,
    /// Every asset's parts at the end of the fold, by asset.                 // NEW
    pub assets: Vec<AssetState>,
    /// Every occurrence a contract expected up to the horizon, and whether and
    /// when the journal kept it.                                               // NEW
    pub promises: Vec<Promise>,
    /// Basis the laws moved: consumed (depreciation) or carried (wash sales). // NEW
    pub adjustments: Vec<Adjustment>,
    pub checks: Box<[u32]>,
    pub diagnostics: Vec<Diagnostic>,
}

/// An asset and its parts.                                                  // NEW
pub struct AssetState {
    pub asset: Id<Asset>,
    pub parts: Vec<Part>,
    /// When it left the owners, and to whom.
    pub disposed: Option<(Day, Id<Flow>)>,
}

/// One part of an asset: its acquisition, or an improvement.               // NEW
#[derive(Clone, Copy, Debug)]
pub struct Part {
    /// The flow that made it.
    pub flow: Id<Flow>,
    pub day: Day,
    /// What it cost, in base quanta.
    pub cost: Qty,
    /// What remains of its cost after what the laws consumed.
    pub basis: Qty,
}

/// One expected occurrence of a contract.                                   // NEW
#[derive(Clone, Copy, Debug)]
pub struct Promise {
    pub contract: Id<Contract>,
    pub due: Day,
    /// The occurrence that kept it (its transaction), or `None` if the journal
    /// has not written it by the horizon.
    pub kept: Option<(Day, Id<Txn>)>,
}

impl Promise {
    /// Days late: kept after `due`, or still missing at `horizon`.
    pub fn late(&self, horizon: Day) -> i32;
}

/// Basis a law moved.                                                        // NEW
#[derive(Clone, Copy, Debug)]
pub struct Adjustment {
    pub day: Day,
    pub law: Id<Law>,
    pub kind: AdjustmentKind,
    pub amount: Qty,
}

#[derive(Clone, Copy, Debug)]
pub enum AdjustmentKind {
    /// A part's basis consumed: depreciation.
    Consumed { asset: Id<Asset>, part: u32 },
    /// A disallowed loss held from a sale and added to a later (or earlier)
    /// acquisition: a wash sale.
    Carried { from: Id<Flow>, to: Option<Id<Flow>> },
}

/// `Effect` (the obligation and tally record) keeps its shape; `Cause` gains
/// `Derived(Id<Flow>)` if a derived flow needs to be named apart from a
/// written one (the flow's `origin` already says it; add only if needed).
