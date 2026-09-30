//! What a [`Ledger`](crate::Ledger) carries besides the book and its
//! timeline.
//!
//! The clonable *world* is what the future depends on: holdings, totals and
//! tallies. The *record* is what happened, append-only: gains, effects,
//! violations, diagnostics. Both are cloned with the ledger (a clone costs a few
//! flat copies, never a replay). The *scratch* buffers are reused between flows
//! so nothing allocates in steady state, and are deliberately not cloned.

use std::hash::{Hash, Hasher};

use axiom_core::hash::FxHasher;
use axiom_core::{Day, Diagnostic, Id, Loc, Map, Qty, Set, Sym};
use axiom_model::{Amount, Book, Commodity, Entity, Flow, Law, Param, Place, Subject, Value};

use crate::eval::Outcome;
use crate::lots::{Holdings, Relief};
use crate::motion::Amounts;
use crate::totals::{Tallies, Totals};
use crate::{Applied, Effect, Gain, Headroom, Pad, Violation};

#[derive(Clone)]
pub(crate) struct World {
    pub holdings: Holdings,
    pub totals: Totals,
    pub tallies: Tallies,
}

impl World {
    pub fn new(book: &Book) -> World {
        World { holdings: Holdings::new(book.places.len()), totals: Totals::new(book), tallies: Tallies::default() }
    }
}

/// Where the assertions on one place and commodity left off.
#[derive(Clone, Copy, Default, Hash)]
pub(crate) struct LastCheck {
    /// The last day one was checked.
    pub day: Option<Day>,
    /// How far the statement was from the ledger then (statement minus ledger,
    /// in the sign the assertion is written in). It is carried: an assertion
    /// that fails by the same amount is not reported again.
    pub gap: Qty,
    /// An amount the assertions depend on could not be solved, and one
    /// assertion has said so.
    pub unsolved_said: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Record {
    /// The quantities the fold resolved when it reached the flows (`=` and
    /// `all` depend on the balance), remembered because a reversal must undo
    /// exactly what was done. The plan holds the `?` amounts solved before it began.
    pub resolved: Map<Id<Flow>, Amounts>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    pub violations: Vec<Violation>,
    pub pads: Vec<Pad>,
    pub diagnostics: Vec<Diagnostic>,
    /// How many times each law ran past its `when` filters.
    pub checks: Vec<u32>,
    pub checkpoints: Map<(Id<Place>, Id<Commodity>), LastCheck>,
    /// A limit's latest reading in its window, by law, step and subject: updated
    /// in place while the window lasts, and set aside in `passed` when a reading
    /// falls in the next one.
    pub headroom: Map<(Id<Law>, u32, Subject), Headroom>,
    /// Readings of windows that have passed.
    pub passed: Vec<Headroom>,
    /// Every `!` a posted flow carried, and whether it waived anything.
    pub waivers: Map<Loc, bool>,
    /// `always` laws currently failing, so a lasting condition is reported
    /// when it starts rather than after every flow.
    pub failing: Set<(Id<Law>, Subject)>,
    /// Laws already reported for a subject in a window, by law, step, subject
    /// and the window's first day: a limit is broken once per window, at the
    /// flow that crossed it.
    pub reported: Set<(Id<Law>, u32, Subject, Day)>,
    /// Places whose lots were already reported ambiguous: one policy fixes them all.
    pub ambiguous: Set<Id<Place>>,
    /// What was already reported missing.
    pub missing: Set<Missing>,
}

/// What a value could not be computed without. A report is about this, not
/// about the law that ran into it: a price nobody wrote, or a `filing` never
/// set, is one mistake however many laws and flows need it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Missing {
    /// A price of the first commodity in the second.
    Price(Id<Commodity>, Id<Commodity>),
    /// A property of one entity or place (`filing` on `me`), or of whatever
    /// a law read it from when that is neither.
    Property(Option<Subject>, Sym),
    /// Rows of a project's param for the day asked.
    Row(Id<Param>),
    /// A system's figures for a year: every table a system ships starts in
    /// some year, and a journal older than that lacks them all at once.
    Figures(i32),
    /// Arithmetic that failed, per law and step.
    Arithmetic(Id<Law>, u32),
}

impl Record {
    pub fn new(laws: usize, diagnostics: Vec<Diagnostic>) -> Record {
        Record { diagnostics, checks: vec![0; laws], ..Record::default() }
    }

    /// What the future depends on, without what happened so far: the resolved
    /// quantities and what has already been reported (so it is not reported
    /// again), and empty records.
    pub fn forked(&self) -> Record {
        Record {
            resolved: self.resolved.clone(),
            checks: vec![0; self.checks.len()],
            checkpoints: self.checkpoints.clone(),
            failing: self.failing.clone(),
            reported: self.reported.clone(),
            ambiguous: self.ambiguous.clone(),
            missing: self.missing.clone(),
            ..Record::default()
        }
    }

    /// Adds a diagnostic and returns its index.
    pub fn report(&mut self, diagnostic: Diagnostic) -> u32 {
        self.diagnostics.push(diagnostic);
        (self.diagnostics.len() - 1) as u32
    }

    /// Where each record ends now, as the empty ranges [`since`](Record::since) grows.
    pub fn marks(&self) -> Applied {
        let end = |len: usize| len..len;
        Applied {
            gains: end(self.gains.len()),
            effects: end(self.effects.len()),
            violations: end(self.violations.len()),
            diagnostics: end(self.diagnostics.len()),
        }
    }

    /// Everything recorded since `marks`.
    pub fn since(&self, marks: Applied) -> Applied {
        let to = |start: std::ops::Range<usize>, len: usize| start.start..len;
        Applied {
            gains: to(marks.gains, self.gains.len()),
            effects: to(marks.effects, self.effects.len()),
            violations: to(marks.violations, self.violations.len()),
            diagnostics: to(marks.diagnostics, self.diagnostics.len()),
        }
    }
}

/// Buffers reused by every flow.
#[derive(Default)]
pub(crate) struct Scratch {
    /// The value of every node of the law that ran last.
    pub values: Vec<Value>,
    pub outcomes: Vec<Outcome>,
    pub relief: Relief,
    /// Entities parcels are tied to, and whether their laws permit the flow.
    pub permits: Vec<(Id<Entity>, bool)>,
    /// What each amount of the flow being posted is worth in the base currency
    /// on its day: the totals, the proceeds and the fee each ask, and a price
    /// is looked up once.
    pub worth: Vec<(Amount, Option<Qty>)>,
    /// The rules of the list being fired that have run, when a law can reach one subject twice.
    pub done: Vec<(Id<Law>, Subject)>,
}

impl Clone for Scratch {
    fn clone(&self) -> Scratch {
        Scratch::default()
    }
}

/// A hash of a map's or set's entries that does not depend on the order they
/// are stored in, which is the order they were inserted in.
pub(crate) fn unordered<T: Hash>(entries: impl IntoIterator<Item = T>) -> u64 {
    let hashed = entries.into_iter().map(|entry| {
        let mut hasher = FxHasher::default();
        entry.hash(&mut hasher);
        hasher.finish()
    });
    hashed.fold(0, u64::wrapping_add)
}

/// What the rest of a fold depends on that is not in the world: what was
/// already reported (so it is not reported again), where the assertions left
/// off, and the limits' current readings. The records themselves are only
/// what happened, and are not part of it.
impl Hash for Record {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unordered(&self.resolved).hash(state);
        unordered(&self.checkpoints).hash(state);
        unordered(&self.headroom).hash(state);
        unordered(&self.waivers).hash(state);
        unordered(&self.failing).hash(state);
        unordered(&self.reported).hash(state);
        unordered(&self.ambiguous).hash(state);
        unordered(&self.missing).hash(state);
    }
}
