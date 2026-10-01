//! The state machine.
//!
//! A [`Ledger`] is the fold's position: a clock of cursors into the timeline,
//! the [`World`] holdings/totals/tallies, and the [`Record`] of what happened.
//! It borrows the [`Plan`], which holds everything the fold decided beforehand.
//! Advancing it consumes moments in their total order; applying a flow runs a
//! fact the journal does not hold through the same `post` the journal's own
//! flows take.
//!
//! A clone copies what the future depends on (the world, the timeline's cursors
//! and its handful of pending deadlines) and the records so far: flat vectors,
//! so a clone is a handful of memory copies, never a replay. A [`fork`] copies
//! the same but forgets the records, and neither copies a table, for the tables
//! are the plan's.
//!
//! [`fork`]: Ledger::fork

use axiom_core::{Arena, Day, Diagnostic, Id, Qty, par};
use axiom_model::{Book, Commodity, End, Flow, FlowView, Infer, Place, RuntimeDetail, RuntimeFlow};

use crate::checkpoint::CheckpointPhase;
use crate::motion::{Amounts, Motion};
use crate::plan::Plan;
use crate::scope::is_money;
use crate::state::{Record, Scratch, World};
use crate::timeline::{Fact, Moment, Timeline};
use crate::{Applied, Cause, Holding, Options, Posted, Recorded, Run, State, explain};

/// The book's state as of some day. Cheap to clone relative to a replay.
#[derive(Clone)]
pub struct Ledger<'p, 'b, 's> {
    pub(crate) plan: &'p Plan<'b, 's>,
    pub(crate) options: Options,
    /// The last day whose deadlines fire: `options.today`, or the journal's
    /// last fact if that is later.
    horizon: Day,
    pub(crate) clock: Clock,
    pub(crate) world: World,
    pub(crate) record: Record,
    pub(crate) scratch: Scratch,
}

#[derive(Clone)]
pub(crate) struct Clock {
    pub day: Day,
    pub phase: CheckpointPhase,
    pub timeline: Timeline,
    /// How many flows `apply` has taken.
    pub applied: u32,
}

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
    /// Stands at the day before the first fact.
    pub(crate) fn start(plan: &'p Plan<'b, 's>, options: Options) -> Ledger<'p, 'b, 's> {
        let timeline = Timeline::new(plan);
        let day = timeline
            .peek()
            .map_or(Day::default(), |first| first.day.add_days(-1));
        let (world, record) = (
            World::new(plan.book, &plan.watch),
            Record::new(plan.book.laws.len(), plan.problems()),
        );
        Ledger::resumed(
            plan,
            options,
            Clock {
                day,
                phase: CheckpointPhase::EndOfDay,
                timeline,
                applied: 0,
            },
            (world, record),
        )
    }

    /// Stands wherever the clock, the world and the record say.
    pub(crate) fn resumed(
        plan: &'p Plan<'b, 's>,
        options: Options,
        clock: Clock,
        (world, record): (World, Record),
    ) -> Ledger<'p, 'b, 's> {
        Ledger {
            plan,
            options,
            horizon: plan.horizon(options.today),
            clock,
            world,
            record,
            scratch: Scratch::default(),
        }
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.plan.book
    }

    /// A copy to drive further with flows the journal does not hold, like
    /// `clone`, but forgetting the records so far: what the fork's
    /// [`finish`](Ledger::finish) reports is only what the fork itself caused,
    /// so a hypothetical withdrawal's gains, taxes and violations are not mixed
    /// with the journal's. Costs the state, not the history, and shares the plan.
    pub fn fork(&self) -> Ledger<'p, 'b, 's> {
        Ledger {
            plan: self.plan,
            options: self.options,
            horizon: self.horizon,
            clock: self.clock.clone(),
            world: self.world.clone(),
            record: self.record.forked(),
            scratch: Scratch::default(),
        }
    }

    /// The last day folded.
    pub fn day(&self) -> Day {
        self.clock.day
    }

    /// Lets the deadlines that fall due by `day` fire, as they would had the
    /// ledger been started for a later `today`: a view that judges a year
    /// beyond the day the run stopped asks for it.
    pub fn reach(&mut self, day: Day) {
        self.horizon = self.horizon.max(day);
    }

    /// Folds the journal's facts, and the deadlines and period ends that fall
    /// due, through the end of `day`.
    pub fn advance(&mut self, day: Day) {
        self.fold_through(day, Moment::end_of(day), CheckpointPhase::EndOfDay);
    }

    /// Folds what the journal holds through `day`, its flows and assertions,
    /// and stops before the deadlines and period ends of that day: a month or
    /// a year that ends on it is not closed yet. What is applied on `day`
    /// next is a fact of that day and comes before them, as a journal flow of
    /// that day does; [`advance`](Ledger::advance) closes the day afterwards.
    /// The holdings are the same as at the end of the day, since a closing
    /// counts and owes and moves nothing.
    pub fn advance_to_closing(&mut self, day: Day) {
        self.fold_through(
            day,
            Moment::before_closings(day),
            CheckpointPhase::BeforeClosings,
        );
    }

    fn fold_through(&mut self, day: Day, limit: Moment, phase: CheckpointPhase) {
        let (was, before) = (self.clock.day, self.clock.phase);
        self.advance_through(limit);
        self.clock.day = self.clock.day.max(day);
        self.clock.phase = if day > was {
            phase
        } else if day == was {
            before.max(phase)
        } else {
            before
        };
        self.enter(self.clock.day);
        self.world.holdings.tidy();
    }

    /// Advances to `flow.day`, then applies a flow the journal does not hold
    /// (planned or hypothetical) exactly as if it did: relief, gains, laws.
    /// Returns what the flow caused, not what the journal did on the way.
    ///
    /// The flow takes its place after the journal's own flows of its day and
    /// before that day's assertions and closings, so long as the ledger has
    /// not folded them: after [`advance`](Ledger::advance) has closed the
    /// day, the flow is late for its closings, as a journal flow written
    /// after them would be. A flow dated before the ledger's day is applied
    /// on the ledger's day: the fold does not travel back. Its `mode` is
    /// ignored, since applying is what makes it real.
    pub fn apply(&mut self, flow: &Flow) -> Applied {
        let view = self.plan.book.flow_view(flow);
        let txn = axiom_model::RuntimeTxn::journal(flow.txn)
            .expect("a Book flow cannot use the template transaction sentinel");
        self.apply_view(flow, view, txn)
    }

    /// Applies a forecast flow whose metadata is pooled in the Book and whose
    /// detail may be overridden in the forecast's immutable runtime arena.
    pub fn apply_runtime(&mut self, flow: &RuntimeFlow, details: &Arena<RuntimeDetail>) -> Applied {
        let view = self.plan.book.runtime_flow_view(flow, details);
        self.apply_view(&flow.flow, view, flow.txn)
    }

    fn apply_view(
        &mut self,
        flow: &Flow,
        view: FlowView<'_>,
        txn: axiom_model::RuntimeTxn,
    ) -> Applied {
        let (was, before) = (self.clock.day, self.clock.phase);
        let day = flow.day.max(self.clock.day);
        self.advance_through(Moment::after_flows(day));
        self.clock.day = day;
        self.clock.phase = if day > was {
            CheckpointPhase::AfterFlows
        } else {
            before.max(CheckpointPhase::AfterFlows)
        };
        self.enter(day);
        let marks = self.record.marks();
        let number = self.clock.applied;
        self.clock.applied += 1;
        let amounts = self.amounts(flow, None);
        self.post(&Motion::from_view(
            self.plan.book,
            view,
            txn,
            Cause::Applied(number),
            day,
            amounts,
        ));
        self.world.holdings.tidy();
        self.record.since(marks)
    }

    /// What `place` alone holds of `unit`, in quanta.
    pub fn balance(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        self.world.holdings.qty(place, unit)
    }

    /// Every non-empty holding, by place then commodity.
    pub fn holdings(&self) -> impl Iterator<Item = &Holding> {
        self.world
            .holdings
            .iter()
            .map(|slot| &slot.holding)
            .filter(|holding| !holding.is_empty())
    }

    /// What has been recorded since this ledger began, or was forked: the
    /// vectors an [`Applied`] range indexes. Reading a fork's own records
    /// costs what they hold, where [`finish`](Ledger::finish) costs the size
    /// of the journal.
    pub fn recorded(&self) -> Recorded<'_> {
        let record = &self.record;
        Recorded {
            gains: &record.gains,
            effects: &record.effects,
            violations: &record.violations,
            diagnostics: &record.diagnostics,
        }
    }

    /// Stops and hands over everything recorded along the way: the claims
    /// still open past their day and the waivers that waived nothing are
    /// reported now, when it is known they stayed so.
    pub fn finish(mut self) -> Run {
        self.world.holdings.tidy();
        let (book, today) = (self.plan.book, self.options.today);
        let overdue = self
            .world
            .holdings
            .iter()
            .filter(|slot| book.places[slot.place].claim)
            .flat_map(|slot| {
                let claims = slot.holding.lots.iter().filter(|lot| lot.qty > Qty::ZERO);
                claims.filter_map(move |lot| {
                    explain::overdue(book, slot.place, slot.unit, lot, today)
                })
            });
        let mut unused: Vec<_> = self
            .record
            .waivers
            .iter()
            .filter(|&(_, &used)| !used)
            .map(|(&loc, _)| loc)
            .collect();
        unused.sort_unstable();
        let reports: Vec<Diagnostic> = overdue
            .chain(unused.into_iter().map(explain::unused_waiver))
            .collect();
        self.record.diagnostics.extend(reports);
        let mut headroom = std::mem::take(&mut self.record.passed);
        headroom.extend(self.record.headroom.drain().map(|(_, reading)| reading));
        headroom.sort_unstable_by_key(|h| {
            (
                h.law,
                h.step,
                crate::show::subject_key(h.subject),
                h.days.first(),
            )
        });
        let Ledger {
            plan,
            options,
            horizon,
            world,
            record,
            ..
        } = self;
        Run {
            today: options.today,
            horizon,
            posted: posted(plan, &record),
            holdings: world.holdings.into_sorted(),
            gains: record.gains,
            effects: record.effects,
            violations: record.violations,
            headroom,
            pads: record.pads,
            // These collections are populated by the native state monitors.
            assets: world.assets.into_states(),
            promises: Vec::new(),
            promised_flows: Box::default(),
            runtime_details: Arena::new(),
            missing_inputs: Box::default(),
            open_claims: Box::default(),
            monitor_complete: false,
            adjustments: Vec::new(),
            checks: record.checks.into(),
            diagnostics: record.diagnostics,
        }
    }

    /// Consumes every moment up to and including `limit`, and no deadline
    /// beyond the horizon.
    pub(crate) fn advance_through(&mut self, limit: Moment) {
        let limit = limit.min(Moment::end_of(self.horizon));
        loop {
            let Some(moment) = self.clock.timeline.peek().filter(|&moment| moment <= limit) else {
                return;
            };
            self.clock.timeline.consume(moment, self.plan);
            self.clock.day = moment.day;
            self.enter(moment.day);
            self.step(moment);
        }
    }

    fn step(&mut self, moment: Moment) {
        match moment.fact {
            Fact::Split(at) => {
                let split = self.plan.book.splits[at as usize];
                self.world.holdings.scale(split.unit, split.ratio);
            }
            Fact::Flow(id) => {
                let motion = self.journal_motion(id, moment.day);
                self.post(&motion);
            }
            // A settlement lands a pending flow; a return runs an actual one backwards.
            Fact::Settle(id) => {
                let motion = self.journal_motion(id, moment.day);
                let returned = matches!(
                    self.plan.events.state(id, &self.plan.book.flows[id]),
                    State::Returned(_)
                );
                self.post(&if returned { motion.reversed() } else { motion });
            }
            Fact::Assert(index) => self.reconcile(index as usize),
            Fact::Deadline(rule, period) => self.deadline(rule as usize, moment.day, period),
        }
    }

    /// A journal flow as it moves on `day`, its quantities solved.
    fn journal_motion(&mut self, id: Id<Flow>, day: Day) -> Motion<'b> {
        let book: &'b Book<'s> = self.plan.book;
        let flow = &book.flows[id];
        let amounts = self.amounts(flow, Some(id));
        Motion::new(book, flow, Cause::Flow(id), day, amounts)
    }

    /// A flow's quantities. `?` amounts were solved before the fold, and are
    /// the plan's; `=` and `all` depend on the balance and are resolved now,
    /// once, and remembered (a reversal must undo exactly what was done).
    fn amounts(&mut self, flow: &Flow, id: Option<Id<Flow>>) -> Amounts {
        if let Some(done) = id.and_then(|id| settled(self.plan, &self.record, id, flow)) {
            return done;
        }
        let written = Amounts::written(flow);
        let resolved = match flow.infer {
            Infer::Known | Infer::Unknown => return written,
            Infer::Target { end, balance } => self.resolve_target(flow, end, balance, written),
            Infer::All => self.everything(flow, written),
        };
        if let Some(id) = id {
            self.record.resolved.insert(id, resolved);
        }
        resolved
    }

    /// `all`: everything the selected parcels at the source hold.
    fn everything(&self, flow: &Flow, written: Amounts) -> Amounts {
        let book = self.plan.book;
        let slot = self.world.holdings.get(flow.from, flow.out.unit);
        let qty = if book.places[flow.from].class.holds_parcels() {
            let money = is_money(book, flow.from, flow.out.unit);
            let view = book.flow_view(flow);
            slot.map_or(Qty::ZERO, |slot| {
                slot.admitted(money, view.select(), &book.codes)
            })
        } else {
            slot.map_or(Qty::ZERO, |slot| slot.plain.max(Qty::ZERO))
        };
        Amounts {
            out: qty,
            arrive: if flow.is_exchange() {
                written.arrive
            } else {
                qty
            },
        }
    }

    /// `= 5_000 USD`: whatever leaves the source, or arrives at the target,
    /// so that its place holds `balance` afterwards.
    fn resolve_target(&mut self, flow: &Flow, end: End, balance: Qty, written: Amounts) -> Amounts {
        let book = self.plan.book;
        let (place, unit) = match end {
            End::From => (flow.from, flow.out.unit),
            End::To => (flow.to, flow.arrive.unit),
        };
        // The target is written in the place's display sign.
        let (held, target) = (
            self.world.holdings.qty(place, unit),
            self.plan.sides.display(place, balance),
        );
        let gap = if end == End::From {
            held - target
        } else {
            target - held
        };
        let qty = if gap.is_negative() {
            let shown = (self.plan.sides.display(place, held), balance);
            self.record
                .report(explain::past_target(book, flow, place, unit, shown, end));
            Qty::ZERO
        } else {
            gap
        };
        match (end, flow.is_exchange()) {
            (_, false) => Amounts {
                out: qty,
                arrive: qty,
            },
            (End::From, true) => Amounts {
                out: qty,
                ..written
            },
            (End::To, true) => Amounts {
                arrive: qty,
                ..written
            },
        }
    }
}

/// Every journal flow as solved and settled. Each depends on nothing but the
/// plan and the record, so they are made side by side, a stretch of flows to a
/// worker.
fn posted(plan: &Plan, record: &Record) -> Box<[Posted]> {
    const STRETCH: usize = 4096;
    let book = plan.book;
    let stretches: Vec<usize> = (0..book.flows.len()).step_by(STRETCH).collect();
    let mut all = Vec::with_capacity(book.flows.len());
    let post = |id: Id<Flow>| {
        let flow = &book.flows[id];
        let amounts = settled(plan, record, id, flow).unwrap_or_else(|| Amounts::written(flow));
        Posted {
            out: amounts.out,
            arrive: amounts.arrive,
            state: plan.events.state(id, flow),
        }
    };
    let stretch = |&first: &usize| {
        let ids = (first..(first + STRETCH).min(book.flows.len())).map(|at| Id::new(at as u32));
        ids.map(post).collect::<Vec<_>>()
    };
    par::map_each_ordered(&stretches, stretch, |made| all.extend(made));
    all.into()
}

/// A flow's quantities where they are already settled: as written, as the
/// plan solved a `?`, or as the fold resolved an `=` or `all`. Only the last
/// depends on the fold, and only it is looked up in the record.
fn settled(plan: &Plan, record: &Record, id: Id<Flow>, flow: &Flow) -> Option<Amounts> {
    match flow.infer {
        Infer::Known => Some(Amounts::written(flow)),
        Infer::Unknown => Some(
            plan.amounts
                .get(&id)
                .copied()
                .unwrap_or_else(|| Amounts::written(flow)),
        ),
        Infer::All | Infer::Target { .. } => record.resolved.get(&id).copied(),
    }
}

/// The journal folded through `options.today` (and every later journal fact).
pub(crate) fn fold(plan: &Plan, options: Options) -> Run {
    conclude(plan.start(options))
}

/// Like [`fold`], and the ledger as it stood on `options.today` before that
/// day's closings, with no records: a view forks it, and does not fold the
/// journal again to get there.
pub(crate) fn fold_to_view<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    options: Options,
) -> (Run, Ledger<'p, 'b, 's>) {
    let (run, view, _) = fold_to_view_and_effects_prefix(plan, options);
    (run, view)
}

/// As `fold_to_view`, with the number of effects recorded before today's
/// closings. The run is then concluded from the same ledger, so this length is
/// the exact prefix in `run.effects` belonging to the paired view checkpoint.
pub(crate) fn fold_to_view_and_effects_prefix<'p, 'b, 's>(
    plan: &'p Plan<'b, 's>,
    options: Options,
) -> (Run, Ledger<'p, 'b, 's>, usize) {
    let mut ledger = plan.start(options);
    ledger.advance_to_closing(options.today);
    let effects_prefix_len = ledger.recorded().effects.len();
    let view = ledger.fork();
    (conclude(ledger), view, effects_prefix_len)
}

fn conclude(mut ledger: Ledger) -> Run {
    ledger.advance(ledger.options.today);
    ledger.advance_through(Moment::LAST);
    ledger.finish()
}
