//! The timeline: a book folded through time.
//!
//! A [`Ledger`] is the book's state on some day: parcels at rest in every
//! place, flow totals, tallies, and obligations. It is a state machine: it
//! advances through the journal one fact at a time, and it can be cloned and
//! driven further with flows the journal never recorded. That one mechanism
//! serves the journal itself ([`run`]), forecasts (planned flows), and "what
//! would I net if I draw this account down today" (a hypothetical withdrawal
//! run through the same laws).
//!
//! # How a fold is arranged
//!
//! Before it starts, the book's loose ends are solved into a [`Plan`]: `events`
//! turns settlement events into flow states, and `infer` solves `? USD`
//! amounts from the assertions around them, one place per thread. The plan is
//! immutable and shared: every ledger, fork and thread borrows the one, and
//! `facts` holds what is true of each law whatever runs it, and `traits` what the
//! fold asks of each place, resolved from the book's facts. `timeline` then
//! orders every fact into one total order of moments, and `ledger` consumes
//! them.
//!
//! For each flow, `post` moves value: `lots` keeps what rests where and chooses
//! which parcels leave, `totals` keeps the windowed sums laws
//! read, `fire` runs the laws that watch the flow, `eval` (with `calc`)
//! evaluates a law, and `explain` (with `show`) turns a failure into a
//! diagnostic. `reconcile` checks balance assertions and `scope` says whose
//! value a flow enters or leaves. Contract occurrences use the same ledger
//! path as journal and hypothetical flows, with their monitor state reported
//! separately from physical holdings.
//!
//! The fold itself is sequential, because each flow's relief, totals and laws
//! depend on every flow before it. Everything around it is not: the plan is
//! solved place by place, forks run side by side, and a [`Checkpoint`] at a
//! month's end lets an edit refold only what it touched.

#![forbid(unsafe_code)]

mod assets;
mod assets_runtime;
mod budget;
mod calc;
mod checkpoint;
mod eval;
mod evaluate;
mod events;
mod explain;
mod facts;
mod fire;
mod infer;
mod ledger;
mod lots;
mod motion;
mod occurrence;
mod owners;
mod plan;
mod post;
mod reconcile;
mod scope;
mod show;
mod sides;
mod state;
mod temporal;
mod timeline;
mod totals;
mod traits;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod split_tests;
#[cfg(test)]
mod tests;

use std::hash::{Hash, Hasher};

use axiom_core::{Arena, Day, Days, Diagnostic, Id, Qty, Ratio, Sym};
use axiom_model::{
    Amount, Asset, Commodity, Contract, Dir, Entity, Flow, FlowCodes, Law, Place, PurposeRoot, RuntimeDetail,
    RuntimeFlow, RuntimeTxn, ScheduleKind, Subject, System, Txn, Waive,
};

pub use assets::{
    AssetError, AssetState, Assets, CarryUpdate, Consumption, Disposal, DisposalBoundary, EventKey, Part, PartId,
    PartKind, PendingCarry,
};
pub use checkpoint::Checkpoint;
pub use ledger::Ledger;
pub use occurrence::{OccurrenceOutput, TemplateError};
pub use plan::{Known, Plan, run};
pub use sides::Sides;

/// One entity's effective financial share in a place or entity after
/// following its declared ownership chain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OwnerShare {
    pub owner: Id<Entity>,
    pub share: Ratio,
}

/// Classifies a purpose flow by whether it crosses the owner's boundary.
/// Internal transfers have no income/spending direction; a capital-purpose
/// acquisition between owned places is the one internal movement counted as
/// outgoing capital.
pub fn purpose_direction(source_owned: bool, target_owned: bool, root: PurposeRoot) -> Option<Dir> {
    match (source_owned, target_owned) {
        (true, false) => Some(Dir::Out),
        (false, true) => Some(Dir::In),
        (true, true) if root == PurposeRoot::Capital => Some(Dir::Out),
        (true, true) | (false, false) => None,
    }
}

/// What each phase hands on is shared by reference between threads: the plan
/// every fold reads, the ledgers and checkpoints forked from it, and the run.
const _: () = {
    const fn is_sync<T: Sync>() {}
    is_sync::<Plan<'static, 'static>>();
    is_sync::<Ledger<'static, 'static, 'static>>();
    is_sync::<Checkpoint>();
    is_sync::<Run>();
};

/// How to run.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Laws with deadlines (`by`, `each`) fire once the journal reaches them,
    /// and never past this day, or past the journal's last fact if that is
    /// later: a deadline the journal itself reaches has been reached.
    pub today: Day,
    /// Law violations are warnings (also set by `relaxed` in the book).
    pub relaxed: bool,
}

/// Everything a fold produced. Indices in [`Cause`] and [`Applied`] point into
/// these vectors.
pub struct Run {
    pub today: Day,
    /// The last day the fold reached: `today`, or the journal's last fact if
    /// that is later. Periods close and deadlines fire up to here and no
    /// further, so a law with a deadline after it has not run.
    pub horizon: Day,
    /// Parallel to `Book::flows`: each journal flow as solved and settled.
    pub posted: Box<[Posted]>,
    /// The final state, by place then commodity.
    pub holdings: Vec<Holding>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    pub violations: Vec<Violation>,
    /// The last reading of every limit, per law step, subject and window.
    pub headroom: Vec<Headroom>,
    pub pads: Vec<Pad>,
    /// Every asset's parts at the end of the fold, by asset.
    pub assets: Vec<AssetState>,
    /// Every occurrence a contract expected up to the horizon, and whether and
    /// when the journal kept it. These are complete only when
    /// `monitor_complete` is true.
    pub promises: Vec<Promise>,
    /// Item-level instantiated flows for promises, in promise order. The range
    /// on each Promise indexes this shared pool.
    pub promised_flows: Box<[RuntimeFlow]>,
    /// Runtime detail overrides used by `promised_flows`.
    pub runtime_details: Arena<RuntimeDetail>,
    /// Unbound required inputs, stored as declaration-order indices. A promise
    /// range identifies only the inputs omitted by that occurrence.
    pub missing_inputs: Box<[u16]>,
    /// Claims still open after all settlements, as projected by the same
    /// monitor that produced the fold's holdings. These are complete only
    /// when `monitor_complete` is true.
    pub open_claims: Box<[OpenClaim]>,
    /// Whether native contract occurrences and claims were monitored for this
    /// run. Empty result vectors alone do not mean the book has no promises or
    /// claims.
    pub monitor_complete: bool,
    /// Basis the laws moved: consumed (depreciation) or carried (wash sales).
    pub adjustments: Vec<Adjustment>,
    /// Carry losses whose statutory replacement window is still open at the
    /// run horizon. Matched and expired requests are removed from this list.
    pub pending_carries: Vec<PendingCarry>,
    /// How many times each law ran past its `when` filters, by law id.
    pub checks: Box<[u32]>,
    pub diagnostics: Vec<Diagnostic>,
}

/// What one occurrence instantiated: a stretch of [`Run::promised_flows`].
pub type PromisedFlows = axiom_core::Run<RuntimeFlow>;

/// The inputs one occurrence left out, in declaration order: a stretch of [`Run::missing_inputs`].
pub type OmittedInputs = axiom_core::Run<u16>;

/// One expected occurrence of a contract.
#[derive(Clone, Copy, Debug)]
pub struct Promise {
    pub contract: Id<Contract>,
    pub schedule: ScheduleKind,
    /// Stable ordinal within this contract schedule.
    pub ordinal: u32,
    pub due: Day,
    /// The occurrence that kept it (its transaction), or `None` if the journal
    /// has not written it by the horizon.
    pub kept: Option<(Day, Id<Txn>)>,
    /// The contract occurrence was explicitly waived by the active terms.
    pub waived: bool,
    /// Runtime flow and omitted-input ranges in the parent Run's pools.
    pub flows: PromisedFlows,
    pub missing_inputs: OmittedInputs,
}

impl Promise {
    /// Days late: kept after `due`, or still missing at `horizon`.
    pub fn late(&self, horizon: Day) -> i32 {
        let seen = self.kept.map_or(horizon, |(day, _)| day);
        (seen.0 - self.due.0).max(0)
    }
}

impl Run {
    /// Instantiated item flows for one expected occurrence, borrowed from the
    /// shared run pool. Group order and source order are preserved.
    pub fn promise_flows(&self, promise: &Promise) -> &[RuntimeFlow] {
        promise.flows.get(&self.promised_flows).expect("promise flow range belongs to this Run")
    }

    /// Input declaration indices omitted from one expected occurrence.
    pub fn promise_missing_inputs(&self, promise: &Promise) -> &[u16] {
        promise.missing_inputs.get(&self.missing_inputs).expect("promise input range belongs to this Run")
    }
}

/// One canonical open claim parcel. `origin` is the stable runtime identity;
/// source ids are present only for journal flows, so future occurrences never
/// masquerade as Book arena indices. `codes` borrows the source's pooled code
/// ranges and does not copy strings.
#[derive(Clone, Copy, Debug)]
pub struct OpenClaim {
    pub origin: RuntimeTxn,
    pub source: Option<Id<Flow>>,
    pub ordinal: u32,
    pub due: Day,
    pub claimant: Id<Place>,
    pub counterpart: Id<Place>,
    pub debtor: Id<Entity>,
    pub creditor: Id<Entity>,
    pub owner: Id<Entity>,
    pub unit: Id<Commodity>,
    pub amount: Qty,
    pub codes: FlowCodes,
}

impl OpenClaim {
    /// Actual journal transaction provenance, if this claim came from one.
    pub fn source_txn(&self) -> Option<Id<Txn>> {
        self.origin.source_txn()
    }
}

/// Basis a law moved.
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
    Consumed { asset: Id<Asset>, part: PartId },
    /// A disallowed loss held from a sale and added to a later (or earlier)
    /// acquisition: a wash sale.
    Carried { from: PartId, to: Option<PartId> },
}

/// A journal flow with its quantities solved and its settlement known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Posted {
    pub out: Qty,
    pub arrive: Qty,
    pub state: State,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Actual,
    /// Written, not yet real: counts against what can be spent only.
    Pending,
    /// Pending until this day, then actual.
    Settled(Day),
    /// Pending, and then it never happened.
    Void,
    /// Actual until this day, then reversed.
    Returned(Day),
    Planned,
}

impl State {
    /// Whether the flow moves real value on `day`.
    pub fn is_real_on(self, day: Day) -> bool {
        match self {
            State::Actual => true,
            State::Settled(on) => day >= on,
            State::Returned(on) => day < on,
            State::Pending | State::Void | State::Planned => false,
        }
    }

    /// Whether the flow still reduces what can be spent on `day` without
    /// being real yet.
    pub fn is_pending_on(self, day: Day) -> bool {
        match self {
            State::Pending => true,
            State::Settled(on) => day < on,
            _ => false,
        }
    }
}

/// What one place holds of one commodity.
///
/// Asset places hold parcels; every other class holds only `plain`.
#[derive(Clone, Hash, Debug)]
pub struct Holding {
    pub place: Id<Place>,
    pub unit: Id<Commodity>,
    /// Interchangeable value: base currency whose basis is its face, tied to
    /// nothing. Plain money never allocates. Signed: liabilities, income,
    /// expenses and equity hold only this, and an overdraft makes it negative.
    ///
    /// For a commodity other than the base, `plain` is what has no parcel
    /// behind it: the balance of a non-asset place, or, in an asset place, the
    /// unfilled shortfall of a sale of more than was held (an error the fold
    /// has already reported; a negative quantity never lives in a lot).
    pub plain: Qty,
    /// Everything that must be told apart, oldest first.
    pub lots: Vec<Parcel>,
}

impl Holding {
    pub fn qty(&self) -> Qty {
        self.plain + self.lots.iter().map(|lot| lot.qty).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.plain.is_zero() && self.lots.is_empty()
    }
}

/// Value at rest, remembered: a quantity with its basis, when and how it was
/// acquired, and the restricted source it is still tied to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parcel {
    pub qty: Qty,
    /// Value already accounted for (cost, contributions, after-tax money), in
    /// base-currency quanta.
    pub basis: Qty,
    pub acquired: Day,
    /// Tax holding-period start. Usually equal to `acquired`; a wash-sale
    /// carry may tack an earlier holding date onto only the matched quantity.
    pub held_since: Day,
    /// This exact parcel quantity has already served as a replacement for a
    /// disallowed loss and cannot be matched again. Partial carries split the
    /// lot so this marker stays attached to the matched shares.
    pub wash_matched: bool,
    pub txn: RuntimeTxn,
    /// The canonical asset part this parcel belongs to, if it came from an
    /// identified thing. Partial relief and transfers keep this identity.
    pub part: Option<PartId>,
    /// The originating flow's pooled codes. Selectors can match a lot after
    /// it has moved or a forecast has copied its flow, without looking up a
    /// synthetic transaction id or cloning code text.
    pub codes: FlowCodes,
    pub tied: Option<Id<Entity>>,
}

impl Hash for Parcel {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.qty.hash(state);
        self.basis.hash(state);
        self.acquired.hash(state);
        self.held_since.hash(state);
        self.wash_matched.hash(state);
        self.txn.hash(state);
        self.part.hash(state);
        self.codes.header.start().hash(state);
        self.codes.header.len().hash(state);
        self.codes.local.start().hash(state);
        self.codes.local.len().hash(state);
        self.tied.hash(state);
    }
}

/// Which flow caused something: one in the journal, or one handed to
/// [`Ledger::apply`] (numbered in the order applied).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cause {
    Flow(Id<Flow>),
    /// A source transaction whose grouped contract occurrence was materialized
    /// by the engine. This keeps occurrence provenance distinct from a
    /// hypothetical `Applied` flow and from template metadata flow IDs.
    Transaction(Id<Txn>),
    Applied(u32),
    /// A period ending or a deadline passing.
    Time,
}

/// Parcels leaving a place and realizing a gain.
#[derive(Clone, Copy, Debug)]
pub struct Gain {
    pub cause: Cause,
    pub day: Day,
    pub from: Id<Place>,
    pub to: Id<Place>,
    pub unit: Id<Commodity>,
    pub qty: Qty,
    /// Base-currency quanta.
    pub basis: Qty,
    pub proceeds: Qty,
    pub acquired: Day,
    /// No policy decided which parcels left; FIFO was assumed.
    pub ambiguous: bool,
}

impl Gain {
    pub fn gain(&self) -> Qty {
        self.proceeds - self.basis
    }
}

/// A consequence a law recorded: a tally line, or an obligation.
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    pub law: Id<Law>,
    pub subject: Subject,
    /// Who the effect belongs to: the subject's owner.
    pub owner: Id<Entity>,
    pub system: Option<Id<System>>,
    pub day: Day,
    pub name: Sym,
    pub amount: Amount,
    pub consequence: Consequence,
    pub cause: Cause,
}

impl Effect {
    /// What is owed, and to whom and by when, if this is an obligation.
    pub fn owed(&self) -> Option<Owed> {
        match self.consequence {
            Consequence::Count => None,
            Consequence::Owe(owed) | Consequence::Penalty(owed) => Some(owed),
        }
    }

    /// The price of a violated `require … else owe …`.
    pub fn is_penalty(&self) -> bool {
        matches!(self.consequence, Consequence::Penalty(_))
    }
}

/// What a law's effect does. Being a penalty implies an obligation, so it
/// cannot be one without the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consequence {
    /// A tally line: counted, owed to no one.
    Count,
    /// `owe`: an obligation.
    Owe(Owed),
    /// The price of a violated `require … else owe …`: an obligation the same
    /// firing's violation is [`Verdict::Priced`] for.
    Penalty(Owed),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Owed {
    pub to: Id<Entity>,
    pub due: Day,
}

/// A `require` or `warn` that failed. Its diagnostic is in
/// `Run::diagnostics[diagnostic]`.
#[derive(Clone, Copy, Debug)]
pub struct Violation {
    pub law: Id<Law>,
    pub subject: Subject,
    pub day: Day,
    pub cause: Cause,
    pub verdict: Verdict,
    pub diagnostic: u32,
}

/// What became of a failed `require` or `warn`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// An error: the flow breaks the law.
    Blocks,
    /// A `warn`: reported, and stops nothing.
    Warns,
    /// Accepted, by a `!` or by `relaxed`.
    Waived(Waiver),
    /// A `require … else owe …`: the violation was priced, and the
    /// [`Consequence::Penalty`] the same firing recorded is its price, unless a
    /// `!` waived it.
    Priced { waived: bool },
}

impl Verdict {
    /// Whether a `!` or `relaxed` accepted it.
    pub fn is_waived(self) -> bool {
        matches!(self, Verdict::Waived(_) | Verdict::Priced { waived: true })
    }
}

/// Why a violation is not an error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Waiver {
    /// `!` on the flow's leg or transaction.
    Marked(Waive),
    /// The book or the command line is `relaxed`.
    Relaxed,
}

/// What a limit had counted and what it allowed, the last time one of its
/// comparisons ran in one window: `counted <= limit`, with the sides of a
/// `>=` swapped, so the room left is always `limit - counted`.
#[derive(Clone, Copy, Hash, Debug)]
pub struct Headroom {
    pub law: Id<Law>,
    /// The index of the `require` or `warn` step.
    pub step: u32,
    pub subject: Subject,
    pub owner: Id<Entity>,
    /// The window: the month or year of the total or tally the comparison
    /// reads, or the day itself when it reads neither.
    pub days: Days,
    pub counted: Amount,
    pub limit: Amount,
    /// When it was last read.
    pub day: Day,
    pub warn: bool,
    pub bound: Bound,
}

/// Which way a limit was written. A cap (`total <= 500 USD`) stays under what
/// it allows; a floor (`balance >= empty`) stays above what it requires, and
/// is stored with its sides swapped, so that the room is `limit - counted`
/// either way.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bound {
    Cap,
    Floor,
}

/// An assertion's gap, accepted with `!` as a flow from `unknown`, or with
/// `via PLACE` as a flow from that place.
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    /// Index into `Book::asserts`.
    pub assert: u32,
    pub place: Id<Place>,
    /// Where the gap came from: `equity/unknown`, or the `via` place.
    pub counter: Id<Place>,
    /// What moved into `place` from `unknown` (negative: out of `place`), in
    /// balance terms, not the display sign the assertion is written in.
    pub amount: Amount,
    pub day: Day,
}

/// A ledger's records so far, borrowed: what [`Ledger::recorded`] returns.
#[derive(Clone, Copy)]
pub struct Recorded<'a> {
    pub gains: &'a [Gain],
    pub effects: &'a [Effect],
    pub adjustments: &'a [Adjustment],
    pub violations: &'a [Violation],
    pub diagnostics: &'a [Diagnostic],
}

/// What one applied flow caused: ranges into the ledger's records. The
/// diagnostic range covers everything reported while applying it, including
/// the diagnostics behind its violations.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub gains: std::ops::Range<usize>,
    pub effects: std::ops::Range<usize>,
    pub adjustments: std::ops::Range<usize>,
    pub violations: std::ops::Range<usize>,
    pub diagnostics: std::ops::Range<usize>,
}
