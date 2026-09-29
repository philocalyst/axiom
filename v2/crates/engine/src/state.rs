//! What a [`Ledger`](crate::Ledger) carries besides the book and its
//! timeline.
//!
//! The clonable *world* is what the future depends on: holdings, totals and
//! tallies. The *record* is what happened, append-only: gains, effects,
//! violations, diagnostics. Both are cloned with the ledger (a clone costs a few
//! flat copies, never a replay). The *scratch* buffers are reused between flows
//! so nothing allocates in steady state, and are deliberately not cloned.

use std::mem::Discriminant;

use axiom_core::{Day, Diagnostic, Id, Loc, Map, Qty, Set};
use axiom_model::{Book, Commodity, Entity, Fault, Flow, Law, Place, Subject, Value};

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
#[derive(Clone, Copy, Default)]
pub(crate) struct Checkpoint {
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

/// A limit's latest reading in its window. It is updated in place while the
/// window lasts, and set aside when a reading falls in the next one.
#[derive(Clone, Copy)]
pub(crate) struct Reading {
    pub headroom: Headroom,
    /// The window is the year of what a tally counts, not the day a total is read.
    pub tally: bool,
}

#[derive(Clone, Default)]
pub(crate) struct Record {
    /// The quantities of flows that were not fully written: solved from
    /// assertions, or from the balance when the fold reached them.
    pub amounts: Map<Id<Flow>, Amounts>,
    pub gains: Vec<Gain>,
    pub effects: Vec<Effect>,
    pub violations: Vec<Violation>,
    pub pads: Vec<Pad>,
    pub diagnostics: Vec<Diagnostic>,
    /// How many times each law ran past its `when` filters.
    pub checks: Vec<u32>,
    pub checkpoints: Map<(Id<Place>, Id<Commodity>), Checkpoint>,
    /// By law, step and subject.
    pub headroom: Map<(Id<Law>, u32, Subject), Reading>,
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
    /// Prices already reported missing: `(commodity, day)`.
    pub unpriced: Set<(Id<Commodity>, Day)>,
    /// Faults already reported: `(law, step, kind of fault)`.
    pub faulted: Set<(Id<Law>, u32, Discriminant<Fault>)>,
}

/// How long each record was when something began.
#[derive(Clone, Copy)]
pub(crate) struct Marks {
    gains: usize,
    effects: usize,
    violations: usize,
    diagnostics: usize,
}

impl Record {
    pub fn new(book: &Book, amounts: Map<Id<Flow>, Amounts>, diagnostics: Vec<Diagnostic>) -> Record {
        Record { amounts, diagnostics, checks: vec![0; book.laws.len()], ..Record::default() }
    }

    /// What the future depends on, without what happened so far: the resolved
    /// quantities and what has already been reported (so it is not reported
    /// again), and empty records.
    pub fn forked(&self) -> Record {
        Record {
            amounts: self.amounts.clone(),
            checks: vec![0; self.checks.len()],
            checkpoints: self.checkpoints.clone(),
            failing: self.failing.clone(),
            reported: self.reported.clone(),
            ambiguous: self.ambiguous.clone(),
            unpriced: self.unpriced.clone(),
            faulted: self.faulted.clone(),
            ..Record::default()
        }
    }

    /// Adds a diagnostic and returns its index.
    pub fn report(&mut self, diagnostic: Diagnostic) -> u32 {
        self.diagnostics.push(diagnostic);
        (self.diagnostics.len() - 1) as u32
    }

    pub fn marks(&self) -> Marks {
        Marks {
            gains: self.gains.len(),
            effects: self.effects.len(),
            violations: self.violations.len(),
            diagnostics: self.diagnostics.len(),
        }
    }

    /// Everything recorded since `marks`.
    pub fn since(&self, marks: Marks) -> Applied {
        Applied {
            gains: marks.gains..self.gains.len(),
            effects: marks.effects..self.effects.len(),
            violations: marks.violations..self.violations.len(),
            diagnostics: marks.diagnostics..self.diagnostics.len(),
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
}

impl Clone for Scratch {
    fn clone(&self) -> Scratch {
        Scratch::default()
    }
}
