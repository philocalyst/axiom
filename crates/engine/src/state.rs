//! What a [`Ledger`](crate::Ledger) carries besides the plan and its
//! timeline.
//!
//! The clonable *world* is what the future depends on: holdings, totals and
//! tallies. The *record* is what happened, append-only: gains, effects,
//! violations, diagnostics, and beside them what was already reported, so it is
//! not reported again. Both are cloned with the ledger (a clone costs a few flat
//! copies, never a replay), and a fork keeps only the world and what was
//! reported. The *scratch* buffers are reused between flows so nothing
//! allocates in steady state, and are deliberately not cloned.

use std::hash::{Hash, Hasher};

use axiom_core::hash::FxHasher;
use axiom_core::{Arena, Day, Diagnostic, Id, Loc, Map, Qty, Set, Sym};
use axiom_model::{
    Amount, Book, Commodity, Entity, Flow, Law, Param, Place, Rule, RuntimeDetail, RuntimeFlow, Select, Subject, Value,
};

use crate::assets::Assets;
use crate::eval::Outcome;
use crate::lots::{Holdings, Relief};
use crate::monitor::Monitor;
use crate::motion::Amounts;
use crate::recognition::{Piece, Settlement};
use crate::temporal::History as TemporalHistory;
use crate::totals::{Tallies, Totals, Watch};
use crate::{Adjustment, Applied, Effect, Gain, Headroom, Pad, Violation, WriteOff};

#[derive(Clone)]
pub(crate) struct World {
    /// Where each contract's streams stand: what has still to be kept or missed.
    pub monitor: Monitor,
    pub holdings: Holdings,
    pub totals: Totals,
    pub tallies: Tallies,
    pub assets: Assets,
    pub temporal: TemporalHistory,
}

impl World {
    pub fn new(book: &Book, watch: &Watch) -> World {
        World {
            monitor: Monitor::default(),
            holdings: Holdings::new(book.places.len()),
            totals: Totals::new(watch),
            tallies: Tallies::default(),
            assets: Assets::from_book(book),
            temporal: TemporalHistory::default(),
        }
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
    /// Computed basis details reused by settlement returns and included in
    /// checkpoint identity, keyed by the source flow rather than copied into
    /// every Flow record.
    pub computed_basis: Map<Id<Flow>, Qty>,
    /// Statements whose split could not be solved when its first flow landed: said once, and none of its flows posts.
    pub unsolved: Set<Id<axiom_model::Txn>>,
    /// The claims each flow from a party settled, as the parcels they were: a flow that is returned puts them back.
    pub settled: Map<Id<Flow>, Settlement>,
    /// What each journal flow settled, in the order the fold did it, and kept after a return forgets it from `settled`:
    /// the readers of a day before the return see the flow as it was.
    pub settlements: Vec<(Id<Flow>, Settlement)>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    /// Basis changes caused by timed asset laws and deferred-loss matching.
    pub adjustments: Vec<Adjustment>,
    /// The claim parcels forgiven by `waived` statements.
    pub written_off: Vec<WriteOff>,
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
    /// `always` steps currently failing, so each lasting condition is reported
    /// when it starts rather than after every flow.
    pub failing: Set<(Id<Law>, u32, Subject)>,
    /// Laws already reported for a subject in a window, by law, step, subject
    /// and the window's first day: a limit is broken once per window, at the
    /// flow that crossed it.
    pub reported: Set<(Id<Law>, u32, Subject, Day)>,
    /// Places whose lots were already reported ambiguous: one policy fixes them all.
    pub ambiguous: Set<Id<Place>>,
    /// What was already reported missing.
    pub missing: Set<Missing>,
    /// Native contract occurrences already kept by a journal transaction.
    pub promises: Vec<crate::Promise>,
    /// The occurrences a forecast ledger posted as they fell due.
    pub planned: Vec<crate::Planned>,
    /// Shared item-level flows for the kept promise ranges.
    pub promised_flows: Vec<RuntimeFlow>,
    /// Runtime detail overrides referenced by `promised_flows`.
    pub promise_runtime_details: Arena<RuntimeDetail>,
    /// Missing required template inputs, in declaration order within each promise.
    pub promise_missing_inputs: Vec<u16>,
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
            computed_basis: self.computed_basis.clone(),
            unsolved: self.unsolved.clone(),
            settled: self.settled.clone(),
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
            adjustments: end(self.adjustments.len()),
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
            adjustments: to(marks.adjustments, self.adjustments.len()),
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
    /// Separate reusable node values for historical budget-limit expressions.
    /// Those roots must not overwrite the current law's locals while it runs.
    pub budget_values: Vec<Value>,
    pub outcomes: Vec<Outcome>,
    pub relief: Relief,
    /// Entities parcels are tied to, and whether their laws permit the flow.
    pub permits: Vec<(Id<Entity>, bool)>,
    /// What a flow out of a claim place selects: its written selectors and the claims its own codes name.
    pub selectors: Vec<Select>,
    /// What the flow being posted counts toward which purposes (`recognition`), for the laws that fire on each.
    pub pieces: Vec<Piece>,
    /// What each amount of the flow being posted is worth in the base currency
    /// on its day: the totals, the proceeds and the fee each ask, and a price
    /// is looked up once.
    pub worth: Vec<(Amount, Option<Qty>)>,
    /// The rules of the list being fired that have run, when a law can reach one subject twice.
    pub done: Vec<(Id<Law>, Subject)>,
    /// Reused union of the purpose/window reader lists for one opening.
    pub purpose_rules: Vec<Rule>,
    /// Reused output pools for the canonical contract materializer.
    pub runtime_flows: Vec<RuntimeFlow>,
    pub runtime_details: Arena<RuntimeDetail>,
    pub missing_inputs: Vec<u16>,
}

impl Scratch {
    /// The pools an occurrence is made into, emptied: they are lent for one occurrence and given back, so that the
    /// materializer's borrow is apart from the mutable posting path.
    pub fn take_pools(&mut self) -> (Vec<RuntimeFlow>, Arena<RuntimeDetail>, Vec<u16>) {
        let (mut flows, mut details, mut missing) = (
            std::mem::take(&mut self.runtime_flows),
            std::mem::take(&mut self.runtime_details),
            std::mem::take(&mut self.missing_inputs),
        );
        flows.clear();
        details.truncate(0);
        missing.clear();
        (flows, details, missing)
    }

    pub fn give_pools(&mut self, flows: Vec<RuntimeFlow>, details: Arena<RuntimeDetail>, missing: Vec<u16>) {
        (self.runtime_flows, self.runtime_details, self.missing_inputs) = (flows, details, missing);
    }
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
        unordered(&self.computed_basis).hash(state);
        unordered(&self.unsolved).hash(state);
        unordered(&self.settled).hash(state);
        unordered(&self.checkpoints).hash(state);
        unordered(&self.headroom).hash(state);
        unordered(&self.waivers).hash(state);
        unordered(&self.failing).hash(state);
        unordered(&self.reported).hash(state);
        unordered(&self.ambiguous).hash(state);
        unordered(&self.missing).hash(state);
    }
}
