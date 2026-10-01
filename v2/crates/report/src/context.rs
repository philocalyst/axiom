//! A reusable, coherent view of one engine run.
//!
//! A client that renders several reports can construct this once: the plan,
//! run and pre-closing checkpoint all describe the same fold. Day-specific
//! views resume that checkpoint when they look forward, and replay from the
//! beginning only when they ask for a day before it.

use axiom_core::{Day, Diagnostic};
use axiom_engine::{Checkpoint, Ledger, Options, Plan, Run};
use axiom_model::Book;

use crate::closings;
use crate::lens::{Lens, Whose};
use crate::resolve;
use crate::{FlowBy, Query, Report, SourceProvider};

/// Reusable report inputs from one plan and one run.
pub struct Context<'b, 's> {
    book: &'b Book<'s>,
    plan: Plan<'b, 's>,
    run: Run,
    checkpoint: Checkpoint,
    effects_prefix_len: usize,
    whose: Whose,
    relaxed: bool,
}

impl<'b, 's> Context<'b, 's> {
    /// Builds a plan, run and pre-closing checkpoint together so a client
    /// cannot accidentally pair a checkpoint with a different run or book.
    pub fn new(
        book: &'b Book<'s>,
        options: Options,
        whose: Option<&str>,
    ) -> Result<Context<'b, 's>, Diagnostic> {
        let whose = Whose::resolve(book, whose)?;
        let plan = Plan::new(book);
        let (run, ledger, effects_prefix_len) = plan.run_with_view_and_effects_prefix(options);
        let checkpoint = ledger.checkpoint();
        drop(ledger);
        Ok(Context {
            book,
            plan,
            run,
            checkpoint,
            effects_prefix_len,
            whose,
            relaxed: options.relaxed,
        })
    }

    /// The source book this context reads.
    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// The run built with this context's plan and checkpoint.
    pub fn run(&self) -> &Run {
        &self.run
    }

    /// Builds a report using the shared run state wherever possible.
    pub fn report(&self, query: &Query<'_>) -> Result<Report<'s>, Diagnostic> {
        match query {
            Query::Balance {
                globs,
                at,
                value,
                monthly,
            } => {
                let at = at.unwrap_or(self.run.today);
                super::balance::view_with_lens(self.lens(at), &self.run, globs, *value, *monthly)
            }
            Query::Register { place, from, to } => super::register::view_with_lens(
                self.lens(to.unwrap_or(self.run.today)),
                &self.run,
                place,
                *from,
                *to,
            ),
            Query::Flow {
                by: FlowBy::Period(by),
                from,
                to,
            } => {
                let to = to.unwrap_or(self.run.today);
                Ok(super::flow::view_with_lens(
                    self.lens(to),
                    &self.run,
                    *by,
                    *from,
                ))
            }
            Query::Flow {
                by: FlowBy::Party,
                from,
                to,
            } => {
                let cutoff = to.unwrap_or(self.run.today);
                Ok(super::flow::view_by_party_with_lens(
                    self.lens(cutoff),
                    &self.run,
                    *from,
                    cutoff,
                ))
            }
            Query::Available { at } => {
                let at = at.unwrap_or(self.run.today);
                let horizon = closings::judged_through(self.book, at);
                let ledger = self.ledger_at(at, horizon);
                Ok(super::available::from_ledger(
                    self.lens(at),
                    &self.run,
                    &ledger,
                    horizon,
                ))
            }
            Query::Budget { at, by } => Ok(super::budget::view(
                self.book,
                &self.run,
                &self.whose,
                *at,
                *by,
            )),
            Query::Limits { year } => Ok(super::limits::view(
                self.book,
                &self.run,
                &self.whose,
                *year,
            )),
            Query::Claims { at } => {
                let at = at.unwrap_or(self.run.today);
                let ledger = self.ledger_at(at, self.run.today);
                Ok(super::claims::view_from(
                    self.lens(at),
                    &self.run,
                    ledger.holdings(),
                ))
            }
            Query::Contracts => Ok(super::contracts::view(self.book, &self.run, &self.whose)),
            Query::Tax { year } => Ok(super::tax::view(self.book, &self.run, &self.whose, *year)),
            Query::Gains { year } => {
                Ok(super::gains::view(self.book, &self.run, &self.whose, *year))
            }
            Query::Lots { place, at } => {
                let scope = place
                    .map(|text| resolve::place(self.book, text))
                    .transpose()?;
                let at = at.unwrap_or(self.run.today);
                let ledger = self.ledger_at(at, self.run.today);
                Ok(super::lots::view_from(
                    self.lens(at),
                    scope,
                    ledger.holdings(),
                ))
            }
            Query::Forecast { until, paths } => Ok(super::forecast::view_from(
                &self.plan,
                &self.checkpoint,
                &self.run,
                &self.run.effects[..self.effects_prefix_len],
                self.lens(self.run.today),
                self.relaxed,
                *until,
                *paths,
            )),
            Query::Why { target } => super::why::target(self.book, &self.run, &self.whose, target),
            Query::Line { loc } => Ok(super::why::line(self.book, &self.run, &self.whose, *loc)),
        }
    }

    /// Resolves source-backed `why FILE:LINE` queries through the client's
    /// source catalog, then builds the same report as [`Context::report`].
    pub fn report_with_sources(
        &self,
        query: &Query<'_>,
        sources: &dyn SourceProvider,
    ) -> Result<Report<'s>, Diagnostic> {
        if let Some(loc) = super::resolve_source_line(query, sources) {
            self.report(&Query::Line { loc })
        } else {
            self.report(query)
        }
    }

    pub(crate) fn lens(&self, day: Day) -> Lens<'_, 's> {
        Lens::with_plan(
            self.book,
            &self.whose,
            day,
            self.plan.known(),
            self.plan.sides(),
        )
    }

    /// A ledger at `day` before that day's closings. Future views resume the
    /// stored pre-close state; past views must replay because a later
    /// checkpoint cannot be moved backward.
    fn ledger_at(&self, day: Day, horizon: Day) -> Ledger<'_, 'b, 's> {
        let options = Options {
            today: horizon.max(self.run.today),
            relaxed: self.relaxed,
        };
        let mut ledger = if day < self.checkpoint.day() {
            self.plan.start(options)
        } else {
            self.plan.resume(&self.checkpoint, options)
        };
        ledger.advance_to_closing(day);
        ledger
    }
}
