//! What a [`Ledger`](crate::Ledger) carries besides the book and its
//! timeline.
//!
//! The clonable *world* is what the future depends on: holdings, totals and
//! tallies. The *record* is what happened, append-only: gains, effects,
//! violations, diagnostics. Both are cloned with the ledger (a clone costs a few
//! flat copies, never a replay). The *scratch* buffers are reused between flows
//! so nothing allocates in steady state, and are deliberately not cloned.

use std::mem::Discriminant;

use axiom_core::{Day, Diagnostic, Id, Map, Set};
use axiom_model::{Book, Commodity, Entity, Fault, Flow, Law, Place, Subject, Value};

use crate::eval::Outcome;
use crate::lots::{Holdings, Relief};
use crate::motion::Amounts;
use crate::totals::{Tallies, Totals};
use crate::{Applied, Effect, Gain, Pad, Violation};

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
    /// The last day each `(place, commodity)` assertion held.
    pub reconciled: Map<(Id<Place>, Id<Commodity>), Day>,
    /// `always` laws currently failing, so a lasting condition is reported
    /// when it starts rather than after every flow.
    pub failing: Set<(Id<Law>, Subject)>,
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
            reconciled: self.reconciled.clone(),
            failing: self.failing.clone(),
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
