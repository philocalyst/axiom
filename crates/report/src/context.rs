//! A reusable, coherent view of one engine run.
//!
//! A client that renders several reports can construct this once: the plan,
//! run and pre-closing checkpoint all describe the same fold. Day-specific
//! views resume that checkpoint when they look forward, and replay from the
//! beginning only when they ask for a day before it.
//!
//! The fold's results ([`Folded`]) do not borrow the plan, and the plan borrows
//! the book, so a client that keeps the book cannot keep the plan beside it. It
//! keeps the `Folded` and builds a plan each time it answers: a [`Context`]
//! holds the plan it is given and either owns the `Folded` or borrows the client's.

use std::borrow::Borrow;
use std::sync::OnceLock;

use axiom_core::{Day, Diagnostic};
use axiom_engine::{Checkpoint, Ledger, Options, Plan, Run};
use axiom_model::Book;

use crate::balance::Unpriced;
use crate::closings;
use crate::forecast::Past;
use crate::lens::{Lens, Whose};
use crate::resolve;
use crate::{FlowBy, Query, Report, SourceProvider};

/// What folding a plan left behind, and nothing that borrows it: the run, and the state the fold stood in on its day
/// before that day's closings, with how many effects it had recorded by then. The checkpoint is paired with the run
/// here so that a client cannot resume one under another's book.
pub struct Folded {
    options: Options,
    run: Run,
    checkpoint: Checkpoint,
    effects_prefix_len: usize,
    /// The flow ends `balance --value` cannot price, found by the first view that asks and read by every one after.
    unpriced: OnceLock<Unpriced>,
}

impl Folded {
    /// Folds `plan` through `options.today` (and every later journal fact), keeping the checkpoint of today.
    pub fn of(plan: &Plan<'_, '_>, options: Options) -> Folded {
        let (run, ledger, effects_prefix_len) = plan.run_with_view_and_effects_prefix(options);
        let checkpoint = ledger.checkpoint();
        drop(ledger);
        Folded { options, run, checkpoint, effects_prefix_len, unpriced: OnceLock::new() }
    }

    /// The flow ends of the run no price is known for, whoever's money `lens` is about.
    pub(crate) fn unpriced(&self, lens: Lens) -> &Unpriced {
        self.unpriced.get_or_init(|| Unpriced::of(lens, &self.run))
    }

    /// The journal as folded.
    pub fn run(&self) -> &Run {
        &self.run
    }
}

/// Reusable report inputs from one plan and one run. `F` is how the run is held: by value, or borrowed from a client
/// that keeps it while it rebuilds the plan.
pub struct Context<'b, 's, F = Folded> {
    plan: Plan<'b, 's>,
    folded: F,
    whose: Whose,
}

impl<'b, 's, F: Borrow<Folded>> Context<'b, 's, F> {
    /// The reports of `whose` money, from `plan` and the fold of the book it was built for. An owner nobody declared is
    /// an error with a suggestion.
    pub fn over(plan: Plan<'b, 's>, folded: F, whose: Option<&str>) -> Result<Context<'b, 's, F>, Diagnostic> {
        let whose = Whose::resolve(plan.book(), whose)?;
        Ok(Context { plan, folded, whose })
    }

    fn folded(&self) -> &Folded {
        self.folded.borrow()
    }

    /// The source book this context reads.
    pub fn book(&self) -> &'b Book<'s> {
        self.plan.book()
    }

    /// The run built with this context's plan and checkpoint.
    pub fn run(&self) -> &Run {
        &self.folded().run
    }

    /// Builds a report using the shared run state wherever possible.
    pub fn report(&self, query: &Query<'_>) -> Result<Report<'b>, Diagnostic> {
        let run = self.run();
        match query {
            Query::Balance { globs, at, value, monthly } => {
                let lens = self.lens(at.unwrap_or(run.today));
                let unpriced = value.then(|| self.folded().unpriced(lens));
                super::balance::view_with_lens(lens, run, globs, unpriced, *monthly)
            }
            Query::Register { place, from, to } => {
                super::register::view_with_lens(self.lens(to.unwrap_or(run.today)), run, place, *from, *to)
            }
            Query::Flow { by: FlowBy::Period(by), from, to } => {
                let to = to.unwrap_or(run.today);
                Ok(super::flow::view_with_lens(self.lens(to), run, *by, *from))
            }
            Query::Flow { by: FlowBy::Party, from, to } => {
                let to = to.unwrap_or(run.today);
                Ok(super::flow::view_by_party_with_lens(self.lens(to), run, *from))
            }
            Query::Available { at } => Ok(self.available(at.unwrap_or(run.today))),
            Query::Budget { at, by } => {
                Ok(super::budget::view_with_lens(self.lens(at.unwrap_or(run.today)), run, *at, *by))
            }
            Query::Limits { year } => Ok(super::limits::view_with_lens(self.lens(run.today), run, *year)),
            Query::Claims { at } => {
                let at = at.unwrap_or(run.today);
                let ledger = self.ledger_at(at, run.today);
                Ok(super::claims::view_from(self.lens(at), run, ledger.holdings()))
            }
            Query::Contracts => Ok(super::contracts::view_with_lens(self.lens(run.today), run)),
            Query::Tax { year } => Ok(super::tax::view_with_lens(self.lens(run.today), run, *year)),
            Query::Gains { year } => Ok(super::gains::view_with_lens(self.lens(run.today), run, *year)),
            Query::Lots { place, at } => {
                let scope = place.map(|text| resolve::place(self.plan.book(), text)).transpose()?;
                let at = at.unwrap_or(run.today);
                let ledger = self.ledger_at(at, run.today);
                Ok(super::lots::view_from(self.lens(at), scope, ledger.holdings()))
            }
            Query::Forecast { until, paths } => Ok(self.forecast(*until, *paths)),
            Query::Why { target } => {
                let lens = self.lens(run.today);
                super::why::Target::of(lens, run, target)?.report(lens, run)
            }
            Query::Line { loc } => Ok(super::why::line_with_lens(self.lens(run.today), run, *loc)),
        }
    }

    /// What can be spent at `at`, and what more costs.
    fn available(&self, at: Day) -> Report<'b> {
        let horizon = closings::judged_through(self.plan.book(), at);
        let ledger = self.ledger_at(at, horizon);
        super::available::from_ledger(self.lens(at), self.run(), &ledger, horizon)
    }

    /// The forecast from the stored pre-close state, over `paths` simulated futures.
    fn forecast(&self, until: Option<Day>, paths: u32) -> Report<'b> {
        let folded = self.folded();
        let effects = &folded.run.effects[..folded.effects_prefix_len];
        let past = Past { at: &folded.checkpoint, effects };
        let options = Options { today: folded.run.today, relaxed: folded.options.relaxed };
        super::forecast::view(past, &folded.run, self.lens(folded.run.today), options, until, paths)
    }

    /// Resolves source-backed `why FILE:LINE` queries through the client's
    /// source catalog, then builds the same report as [`Context::report`].
    pub fn report_with_sources(
        &self,
        query: &Query<'_>,
        sources: &dyn SourceProvider,
    ) -> Result<Report<'b>, Diagnostic> {
        if let Some(loc) = super::resolve_source_line(query, sources) {
            self.report(&Query::Line { loc })
        } else {
            self.report(query)
        }
    }

    pub(crate) fn lens<'a>(&'a self, day: Day) -> Lens<'b, 's, 'a, 'a> {
        Lens::new(&self.plan, &self.whose, day)
    }

    /// A ledger at `day` before that day's closings. Future views resume the
    /// stored pre-close state; past views must replay because a later
    /// checkpoint cannot be moved backward.
    fn ledger_at(&self, day: Day, horizon: Day) -> Ledger<'_, 'b, 's> {
        let folded = self.folded();
        let options = Options { today: horizon.max(folded.run.today), relaxed: folded.options.relaxed };
        let mut ledger = if day < folded.checkpoint.day() {
            self.plan.start(options)
        } else {
            self.plan.resume(&folded.checkpoint, options)
        };
        ledger.advance_to_closing(day);
        ledger
    }
}
