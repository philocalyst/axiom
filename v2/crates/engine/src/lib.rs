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
//! Before it starts, the book's loose ends are solved: `events` turns
//! settlement events into flow states, and `infer` solves `? USD` amounts from
//! the assertions around them, one place per thread. `timeline` then orders
//! every fact into one total order of moments, and `ledger` consumes them.
//!
//! For each flow, `post` moves value: `relief` chooses which parcels leave,
//! `holdings` keeps what rests where, `totals` keeps the windowed sums laws
//! read, `fire` runs the laws that watch the flow, `eval` (with `calc`)
//! evaluates a law, and `explain` (with `show`) turns a failure into a
//! diagnostic. `reconcile` checks balance assertions and `scope` says whose
//! value a flow enters or leaves.
//!
//! The fold itself is sequential, because each flow's relief, totals and laws
//! depend on every flow before it. Everything around it is not.

#![forbid(unsafe_code)]

mod calc;
mod eval;
mod events;
mod explain;
mod fire;
mod holdings;
mod infer;
mod ledger;
mod motion;
mod post;
mod reconcile;
mod relief;
mod scope;
mod show;
mod state;
mod timeline;
mod totals;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests;

use axiom_core::{Day, Diagnostic, Id, Qty, Sym};
use axiom_model::{Amount, Commodity, Entity, Flow, Law, Place, Subject, System, Txn};

pub use ledger::{Ledger, run};

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
    /// Parallel to `Book::flows`: each journal flow as solved and settled.
    pub posted: Box<[Posted]>,
    /// The final state, by place then commodity.
    pub holdings: Vec<Holding>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    pub violations: Vec<Violation>,
    pub pads: Vec<Pad>,
    /// How many times each law ran past its `when` filters, by law id.
    pub checks: Box<[u32]>,
    pub diagnostics: Vec<Diagnostic>,
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
#[derive(Clone, Debug)]
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
    pub txn: Id<Txn>,
    pub tied: Option<Id<Entity>>,
}

/// Which flow caused something: one in the journal, or one handed to
/// [`Ledger::apply`] (numbered in the order applied).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cause {
    Flow(Id<Flow>),
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
    pub owe: Option<Owed>,
    pub cause: Cause,
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
    pub warn: bool,
    /// By `!` or relaxed mode.
    pub waived: bool,
    pub diagnostic: u32,
}

/// An assertion's unexplained gap, accepted with `!` as a flow from `unknown`.
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    /// Index into `Book::asserts`.
    pub assert: u32,
    pub place: Id<Place>,
    /// What moved into `place` from `unknown` (negative: out of `place`), in
    /// balance terms, not the display sign the assertion is written in.
    pub amount: Amount,
    pub day: Day,
}

/// What one applied flow caused: ranges into the ledger's records. The
/// diagnostic range covers everything reported while applying it, including
/// the diagnostics behind its violations.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub gains: std::ops::Range<usize>,
    pub effects: std::ops::Range<usize>,
    pub violations: std::ops::Range<usize>,
    pub diagnostics: std::ops::Range<usize>,
}
