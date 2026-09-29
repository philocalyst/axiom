//! The timeline: a book folded through time.
//!
//! A [`Ledger`] is the book's state on some day: parcels at rest in every
//! place, flow totals, tallies, and obligations. It is a state machine: it
//! advances through the journal one fact at a time, and it can be cloned and
//! driven further with flows the journal never recorded. That one mechanism
//! serves the journal itself ([`run`]), forecasts (planned flows), and "what
//! would I net if I drew this account down today" (a hypothetical withdrawal
//! run through the same laws).

use axiom_core::{Day, Diagnostic, Id, Qty, Sym};
use axiom_model::{Amount, Book, Commodity, Entity, Flow, Law, Place, Subject, System, Txn};

/// How to run.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Laws with deadlines (`by`, `each`) fire once the journal reaches them,
    /// and never past this day.
    pub today: Day,
    /// Law violations are warnings (also set by `relaxed` in the book).
    pub relaxed: bool,
}

/// The book's state as of some day. Cheap to clone relative to a replay.
#[derive(Clone)]
pub struct Ledger<'b, 's> {
    book: &'b Book<'s>,
}

impl<'b, 's> Ledger<'b, 's> {
    /// Solves what the journal leaves open (`?` amounts, `=` targets, `all`,
    /// settlement events) and stands at the day before the first fact.
    pub fn new(book: &'b Book<'s>, options: Options) -> Ledger<'b, 's> {
        let _ = options;
        let _ = book;
        todo!("lane C")
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// The last day folded.
    pub fn day(&self) -> Day {
        todo!("lane C")
    }

    /// Folds the journal's facts, and the deadlines and period ends that fall
    /// due, through the end of `day`.
    pub fn advance(&mut self, day: Day) {
        let _ = day;
        todo!("lane C")
    }

    /// Advances to `flow.day`, then applies a flow the journal does not hold
    /// (planned or hypothetical) exactly as if it did: relief, gains, laws.
    /// Returns what it caused.
    pub fn apply(&mut self, flow: &Flow) -> Applied {
        let _ = flow;
        todo!("lane C")
    }

    /// What `place` alone holds of `unit`, in quanta.
    pub fn balance(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        let _ = (place, unit);
        todo!("lane C")
    }

    /// Every non-empty holding, by place then commodity.
    pub fn holdings(&self) -> impl Iterator<Item = &Holding> {
        std::iter::empty()
    }

    /// Stops and hands over everything recorded along the way.
    pub fn finish(self) -> Run {
        todo!("lane C")
    }
}

/// The journal folded through `options.today` (and every later journal fact).
pub fn run(book: &Book, options: Options) -> Run {
    let _ = (book, options);
    todo!("lane C")
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
#[derive(Clone, Debug)]
pub struct Holding {
    pub place: Id<Place>,
    pub unit: Id<Commodity>,
    /// Interchangeable value: base currency whose basis is its face, tied to
    /// nothing. Plain money never allocates. Signed: liabilities, income,
    /// expenses and equity hold only this, and an overdraft makes it negative.
    pub plain: Qty,
    /// Everything that must be told apart, oldest first.
    pub lots: Vec<Parcel>,
}

impl Holding {
    pub fn qty(&self) -> Qty {
        self.plain + self.lots.iter().map(|lot| lot.qty).sum()
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
    pub amount: Amount,
    pub day: Day,
}

/// What one applied flow caused: ranges into the ledger's records.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    pub gains: std::ops::Range<usize>,
    pub effects: std::ops::Range<usize>,
    pub violations: std::ops::Range<usize>,
}
