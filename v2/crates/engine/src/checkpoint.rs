//! Checkpoints: a fold's state, small enough to keep at every month's end.
//!
//! What the rest of a fold depends on is the world (holdings, totals, tallies),
//! what has already been reported, and the day: not the records, which are only
//! what happened on the way. A [`Checkpoint`] keeps that and a digest of it, so
//! a fold can be resumed under the plan of an edited book from the last
//! checkpoint before the edit, and can stop as soon as one of its month ends has
//! the digest the old fold had there: from that day the two folds are the same,
//! and the old fold's records for the rest stand.

use std::hash::{Hash, Hasher};

use axiom_core::hash::FxHasher;
use axiom_core::{Day, Period, calendar::Window};

use crate::Options;
use crate::ledger::{Clock, Ledger};
use crate::plan::Plan;
use crate::state::{Record, World};
use crate::timeline::Timeline;

/// A fold's position at the end of a day.
#[derive(Clone)]
pub struct Checkpoint {
    day: Day,
    world: World,
    record: Record,
    digest: u64,
}

impl Checkpoint {
    /// The day it stands at the end of.
    pub fn day(&self) -> Day {
        self.day
    }

    /// A hash of everything the rest of the fold depends on: equal digests at
    /// the same day mean the same future, whatever led up to them.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
    /// The state as of the end of the last day folded.
    pub fn checkpoint(&self) -> Checkpoint {
        let (day, world, record) = (self.clock.day, self.world.clone(), self.record.forked());
        let mut hasher = FxHasher::default();
        (day, &world.holdings, &world.totals, &world.tallies, &record).hash(&mut hasher);
        Checkpoint { day, world, record, digest: hasher.finish() }
    }

    /// Folds on through `until` a month at a time, handing `month_end` a
    /// checkpoint at the end of each month (and of `until`). It says whether to
    /// go on: an editor refolding after an edit stops at the first month end
    /// whose digest matches the old fold's.
    pub fn advance_by_month(&mut self, until: Day, mut month_end: impl FnMut(Checkpoint) -> bool) {
        while self.clock.day < until {
            let next = Window::containing(Period::Month, self.clock.day.add_days(1)).days().last();
            self.advance(next.min(until));
            if !month_end(self.checkpoint()) {
                return;
            }
        }
    }
}

impl<'b, 's> Plan<'b, 's> {
    /// A ledger standing where `from` stood, folding the book this plan is for.
    /// The book may have been edited since, on any day after the checkpoint's:
    /// the state is the checkpoint's, and the facts to come are this book's.
    pub fn resume(&self, from: &Checkpoint, options: Options) -> Ledger<'_, 'b, 's> {
        let clock = Clock { day: from.day, timeline: Timeline::after(self, from.day), applied: 0 };
        Ledger::resumed(self, options, clock, (from.world.clone(), from.record.forked()))
    }
}
