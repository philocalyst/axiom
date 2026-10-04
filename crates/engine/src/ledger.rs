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
use axiom_model::{
    Book, Class, Commodity, Cut, End, Expr, Fault, Flow, FlowExpressions, FlowView, Infer, Item, Made, Place,
    RuntimeDetail, RuntimeFlow, RuntimeTxn,
};

use crate::Promise;
use crate::checkpoint::CheckpointPhase;
use crate::monitor;
use crate::motion::{Amounts, Course, Motion};
use crate::plan::Plan;
use crate::promising::Promising;
use crate::scope::is_money;
use crate::state::{Record, Scratch, World};
use crate::statement::{exchange_costs_of, exchange_of};
use crate::timeline::{Fact, Moment, SourceFact, Timeline};
use crate::{Applied, Cause, Holding, Options, Posted, Recorded, Run, State, explain};

/// The book's state as of some day. Cheap to clone relative to a replay.
#[derive(Clone)]
pub struct Ledger<'p, 'b, 's> {
    pub(crate) plan: &'p Plan<'b, 's>,
    pub(crate) options: Options,
    /// The last day whose deadlines fire: `options.today`, or the journal's
    /// last fact if that is later.
    pub(crate) horizon: Day,
    pub(crate) clock: Clock,
    pub(crate) world: World,
    /// What a forecast still has to promise: nothing, until [`promise`](Ledger::promise) says what.
    pub(crate) promising: Promising,
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
    /// Last day whose temporal state has been sampled. The world history
    /// retains changes sparsely, so this cursor is needed to resume daily
    /// date-dependent queries without replaying old days.
    pub temporal_through: Option<Day>,
}

/// What the fold does next.
#[derive(Clone, Copy)]
pub(crate) enum Upcoming {
    /// A fact the journal holds.
    Fact(Moment),
    /// An occurrence a forecast promises, at the moment a line that kept it would have: after every flow of its day.
    Promised(Moment),
}

impl Upcoming {
    pub fn at(self) -> Moment {
        match self {
            Upcoming::Fact(moment) | Upcoming::Promised(moment) => moment,
        }
    }
}

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
    /// Stands at the day before the first fact.
    pub(crate) fn start(plan: &'p Plan<'b, 's>, options: Options) -> Ledger<'p, 'b, 's> {
        let timeline = Timeline::new(plan);
        let day = timeline.peek().map_or(Day::default(), |first| first.day.add_days(-1));
        let (mut world, record) =
            (World::new(plan.book, &plan.watch), Record::new(plan.book.laws.len(), plan.problems()));
        world.monitor = monitor::Monitor::start(&plan.book.promises, plan.watch_from(options.today));
        Ledger::resumed(
            plan,
            options,
            Clock { day, phase: CheckpointPhase::EndOfDay, timeline, applied: 0, temporal_through: None },
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
            promising: Promising::default(),
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
            promising: self.promising.clone(),
            record: self.record.forked(),
            scratch: Scratch::default(),
        }
    }

    /// The last day folded.
    pub fn day(&self) -> Day {
        self.clock.day
    }

    /// Records the temporal expressions against the current world after a
    /// state mutation. Same-day samples are intentional: extrema observe each
    /// intraday state, while `days` uses the final sample for that day.
    pub(crate) fn sample_temporal(&mut self, day: Day) {
        if self.plan.temporal.is_empty() {
            return;
        }
        crate::eval::sample_temporal(self.plan, &mut self.world, day, &mut self.scratch.values);
        self.clock.temporal_through = Some(self.clock.temporal_through.map_or(day, |through| through.max(day)));
    }

    fn sample_temporal_through(&mut self, through: Day) {
        if self.plan.temporal.is_empty() {
            return;
        }
        let start = match self.clock.temporal_through {
            Some(last) => last.0.checked_add(1).map(Day),
            None => Some(self.temporal_start(through)),
        };
        let Some(start) = start.filter(|&start| start <= through) else {
            if self.clock.temporal_through.is_none() {
                self.clock.temporal_through = Some(through);
            }
            return;
        };

        if self.plan.needs_daily_temporal() {
            let mut day = start;
            loop {
                self.sample_temporal(day);
                if day >= through {
                    break;
                }
                let Some(next) = day.0.checked_add(1).map(Day) else {
                    break;
                };
                day = next;
            }
        } else {
            if self.clock.temporal_through.is_none() {
                self.sample_temporal(start);
            }
            let (first, last) = {
                let dates = self.plan.temporal_dates();
                (dates.partition_point(|&day| day < start), dates.partition_point(|&day| day <= through))
            };
            for index in first..last {
                let day = self.plan.temporal_dates()[index];
                if day != Day::MIN {
                    self.sample_temporal(day);
                }
            }
            self.clock.temporal_through = Some(self.clock.temporal_through.map_or(through, |last| last.max(through)));
        }
    }

    fn temporal_start(&self, through: Day) -> Day {
        self.plan
            .temporal_start()
            .filter(|&day| day != Day::MIN && day <= through)
            .or_else(|| {
                self.plan
                    .book
                    .flows
                    .iter()
                    .map(|(_, flow)| flow.day)
                    .filter(|&day| day != Day::MIN && day <= through)
                    .min()
            })
            .unwrap_or(through)
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
        self.fold_through(day, Moment::before_closings(day), CheckpointPhase::BeforeClosings);
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
        self.apply_view(flow, view, txn, 0)
    }

    /// Applies a forecast flow whose metadata is pooled in the Book and whose
    /// detail may be overridden in the forecast's immutable runtime arena.
    pub fn apply_runtime(&mut self, flow: &RuntimeFlow, details: &Arena<RuntimeDetail>) -> Applied {
        let view = self.plan.book.runtime_flow_view(flow, details);
        self.apply_view(&flow.flow, view, flow.txn, flow.ordinal)
    }

    fn apply_view(
        &mut self,
        flow: &Flow,
        view: FlowView<'_>,
        txn: axiom_model::RuntimeTxn,
        flow_ordinal: u32,
    ) -> Applied {
        let day = flow.day.max(self.clock.day);
        self.advance_to_flows(day);
        let marks = self.record.marks();
        let number = self.clock.applied;
        self.clock.applied += 1;
        let amounts = self.amounts(flow, None);
        self.post(&Motion::from_view_at(self.plan.book, view, txn, Cause::Applied(number), day, amounts, flow_ordinal));
        self.sample_temporal(day);
        self.world.holdings.tidy();
        self.record.since(marks)
    }

    /// Folds the journal's facts and the occurrences a forecast promises through the flows of `day`, and stands where a flow
    /// applied on `day` takes its place: after them, before that day's claim changes, assertions and closings.
    pub(crate) fn advance_to_flows(&mut self, day: Day) {
        let (was, before) = (self.clock.day, self.clock.phase);
        self.advance_through(Moment::after_flows(day));
        self.clock.day = day;
        self.clock.phase =
            if day > was { CheckpointPhase::AfterFlows } else { before.max(CheckpointPhase::AfterFlows) };
        self.enter(day);
    }

    /// What `place` alone holds of `unit`, in quanta.
    pub fn balance(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        self.world.holdings.qty(place, unit)
    }

    /// Every non-empty holding, by place then commodity.
    pub fn holdings(&self) -> impl Iterator<Item = &Holding> {
        self.world.holdings.iter().map(|slot| &slot.holding).filter(|holding| !holding.is_empty())
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
            adjustments: &record.adjustments,
            violations: &record.violations,
            diagnostics: &record.diagnostics,
            promises: &record.promises,
            planned: &record.planned,
            promised_flows: &record.promised_flows,
            offspring: &record.offspring,
            first_offspring: record.first_offspring,
            promised_inputs: &record.promise_missing_inputs,
            promised_details: &record.promise_runtime_details,
        }
    }

    /// Stops and hands over everything recorded along the way: the claims
    /// still open past their day and the waivers that waived nothing are
    /// reported now, when it is known they stayed so.
    pub fn finish(mut self) -> Run {
        self.world.holdings.tidy();
        let (book, today) = (self.plan.book, self.options.today);
        let overdue =
            self.world.holdings.iter().filter(|slot| self.plan.traits.place(slot.place).claim).flat_map(|slot| {
                let claims = slot.holding.lots.iter().filter(|lot| lot.qty > Qty::ZERO);
                claims.filter_map(move |lot| explain::overdue(book, slot.place, slot.unit, lot, today))
            });
        let mut unused: Vec<_> = self.record.waivers.iter().filter(|&(_, &used)| !used).map(|(&loc, _)| loc).collect();
        unused.sort_unstable();
        let missed = monitor::missed(book, &self.record.promises, self.horizon);
        let reports: Vec<Diagnostic> =
            overdue.chain(missed).chain(unused.into_iter().map(explain::unused_waiver)).collect();
        let open_claims = monitor::open_claims(self.plan, &self.world.holdings);
        self.record.diagnostics.extend(reports);
        let mut headroom = std::mem::take(&mut self.record.passed);
        headroom.extend(self.record.headroom.drain().map(|(_, reading)| reading));
        headroom.sort_unstable_by_key(|h| (h.law, h.step, crate::show::subject_key(h.subject), h.days.first()));
        let Ledger { plan, options, horizon, mut world, mut record, .. } = self;
        record.settlements.sort_unstable_by_key(|&(flow, _)| flow);
        let histories = record.balances.freeze(&world.holdings.positions(), book.places.len());
        world.assets.expire_carries_through(horizon);
        let (assets, pending_carries) = world.assets.into_run_parts();
        Run {
            today: options.today,
            horizon,
            posted: posted(plan, &record),
            holdings: world.holdings.into_sorted(),
            histories,
            gains: record.gains,
            effects: record.effects,
            adjustments: record.adjustments,
            written_off: record.written_off,
            settlements: record.settlements.into(),
            pending_carries,
            violations: record.violations,
            headroom,
            pads: record.pads,
            // These collections are populated by the native state monitors.
            assets,
            promises: record.promises,
            promised_flows: record.promised_flows.into_boxed_slice(),
            offspring: record.offspring.into_boxed_slice(),
            runtime_details: record.promise_runtime_details,
            missing_inputs: record.promise_missing_inputs.into_boxed_slice(),
            open_claims,
            monitor_complete: true,
            checks: record.checks.into(),
            diagnostics: record.diagnostics,
        }
    }

    /// Consumes every moment up to and including `limit`, and no deadline beyond the horizon.
    pub(crate) fn advance_through(&mut self, limit: Moment) {
        let limit = limit.min(Moment::end_of(self.horizon));
        while let Some(next) = self.upcoming().filter(|next| next.at() <= limit) {
            self.take(next);
        }
        self.miss_through(limit.day);
        self.sample_temporal_through(limit.day);
    }

    /// What the fold does next: the journal's next fact, or the next occurrence a forecast promises, whichever is first.
    pub(crate) fn upcoming(&self) -> Option<Upcoming> {
        let fact = self.clock.timeline.peek().map(Upcoming::Fact);
        let promised = self.promising.next_due().map(|due| Upcoming::Promised(Moment::after_flows(due)));
        match (fact, promised) {
            (Some(fact), Some(promised)) if promised.at() < fact.at() => Some(promised),
            (fact, promised) => fact.or(promised),
        }
    }

    /// Does what `next` is, which must be the one [`upcoming`](Ledger::upcoming) returned.
    pub(crate) fn take(&mut self, next: Upcoming) {
        match next {
            Upcoming::Fact(moment) => {
                self.clock.timeline.consume(moment, self.plan);
                self.on_day(moment.day, |ledger| ledger.step(moment));
            }
            Upcoming::Promised(moment) => self.on_day(moment.day.max(self.clock.day), Ledger::fall_due),
        }
    }

    /// Stands on `day` and does one fact of it, in the order every fact is done: what can no longer be kept is missed
    /// first (a line dated this day is out of its reach), and the temporal state is sampled before and after.
    fn on_day(&mut self, day: Day, fact: impl FnOnce(&mut Self)) {
        self.miss_through(day);
        self.sample_temporal_through(day);
        self.clock.day = day;
        self.enter(day);
        self.sample_temporal(day);
        fact(self);
        self.sample_temporal(day);
    }

    /// Records the occurrences that can no longer be kept as of `day`: each comes before the facts of the day it is
    /// missed on, since a line dated that day is out of its reach.
    fn miss_through(&mut self, day: Day) {
        let mut missed = Vec::new();
        self.world.monitor.miss_through(&self.plan.book.promises, day, |promise, found| missed.push((promise, found)));
        self.record_missed(missed);
    }

    /// Records occurrences nothing kept, each with the day it was found missed, and claims what a party owed.
    pub(crate) fn record_missed(&mut self, missed: Vec<(Promise, Day)>) {
        for (promise, found) in missed {
            let claimed = self.claim_missed(promise, found);
            self.record.promises.push(Promise { claimed, ..promise });
        }
    }

    fn step(&mut self, moment: Moment) {
        self.take_fact(moment);
        self.record_balances(moment.day);
    }

    /// Writes the balances that moved into the histories, as they stand when `day` has been done with them.
    pub(crate) fn record_balances(&mut self, day: Day) {
        let Ledger { world, record, .. } = self;
        for (slot, balance) in world.holdings.moved() {
            record.balances.push(slot, day, balance);
        }
    }

    fn take_fact(&mut self, moment: Moment) {
        match moment.fact {
            Fact::Split(at) => {
                let split = self.plan.book.splits[at as usize];
                self.world.holdings.scale(split.unit, split.ratio);
            }
            Fact::Source(_, SourceFact::Flow(id)) => self.post_journal(id, moment.day, Course::Forward),
            Fact::Source(_, SourceFact::Occurrence(txn)) => self.post_written_occurrence(txn, moment.day),
            Fact::ClaimChange(at) => self.write_off(at),
            // A settlement lands a pending flow; a return runs an actual one backwards.
            Fact::Settle(id) => {
                let returned = matches!(self.plan.events.state(id, &self.plan.book.flows[id]), State::Returned(_));
                self.post_journal(id, moment.day, if returned { Course::Back } else { Course::Forward });
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

    /// Evaluate sparse computed journal roots before posting their source
    /// flow. Literal-only flows retain the borrowed fast path above; computed
    /// quantities never fall back to the zero placeholders stored in Book.
    fn post_journal(&mut self, id: Id<Flow>, day: Day, course: Course) {
        let book = self.plan.book;
        let source = &book.flows[id];
        let txn_id = source.txn;
        let transaction = &book.txns[txn_id];
        let offset = transaction.offset(id);
        let journal = transaction.program.and_then(|program_id| book.journal_programs.get(program_id));
        if journal.is_some_and(|journal| journal.open) && !self.group_ready(txn_id, id, day) {
            return;
        }
        let roots = journal.zip(offset).and_then(|(journal, offset)| journal.roots_of(offset));
        let flows = &book.flows[transaction.flows];
        let in_group = journal.and_then(|journal| journal.group.as_deref());
        let cost_header = in_group
            .filter(|group| offset.is_some_and(|offset| exchange_of(flows, group) == Some(offset)))
            .filter(|group| exchange_costs_of(book, flows, group).next().is_some());
        let computed_cost_item = in_group.zip(offset).is_some_and(|(group, offset)| {
            let computed = |item: &Item<Option<u32>>| matches!(item.amount, Cut::Of(Expr::Computed(_)));
            group.items.iter().any(|item| item.flow == Some(offset) && computed(item))
                && exchange_costs_of(book, flows, group).any(|cost| cost == offset)
        });
        if roots.is_none() && cost_header.is_none() && !computed_cost_item {
            let motion = self.journal_motion(id, day);
            self.post(&motion.running(course));
            return;
        }
        if let Err(problem) = self.post_computed(id, day, course, roots, cost_header) {
            self.record.report(problem);
        }
    }

    /// A journal flow some expression gives an amount, a basis or an exchange cost, posted with them read; the
    /// first landing's amounts and basis are remembered, as `all` and `=` are, so that a return undoes exactly them.
    fn post_computed(
        &mut self,
        id: Id<Flow>,
        day: Day,
        course: Course,
        roots: Option<FlowExpressions>,
        cost_header: Option<&'b Made>,
    ) -> Result<(), Diagnostic> {
        let book = self.plan.book;
        let source = &book.flows[id];
        let transaction = &book.txns[source.txn];
        let offset = transaction.offset(id);
        let roots =
            roots.unwrap_or(FlowExpressions { flow: offset.unwrap_or_default(), out: None, arrive: None, basis: None });
        let program = &book.journal_programs[transaction.program.expect("flow roots belong to a journal program")];
        let mut flow = source.clone();
        flow.day = day;
        let mut detail = *book.flow_view(source).detail();
        let quantity_roots = roots.out.is_some() || roots.arrive.is_some();
        let cached = quantity_roots.then(|| self.record.resolved.get(&id).copied()).flatten();
        let mut computed_quantity = cached.is_some();
        match cached {
            Some(amounts) => {
                flow.out.qty = amounts.out;
                flow.arrive.qty = amounts.arrive;
            }
            None => computed_quantity |= self.read_quantities(id, &mut flow, roots, program, day)?,
        }
        let mut computed_basis = None;
        if let Some(root) = roots.basis {
            if let Some(basis) = self.record.computed_basis.get(&id).copied() {
                detail.basis = Some(basis);
            } else {
                let amount = self.amount_of(id, &flow, program, root, day)?;
                if amount.unit != book.base {
                    let fault = Fault::UnitMismatch { found: amount.unit, expected: book.base };
                    return Err(explain::journal_expression_fault(book, &flow, program, root, fault, day));
                }
                detail.basis = Some(amount.qty);
                computed_basis = Some(amount.qty);
            }
        }
        if let Some(group) = cost_header {
            let cost = self.exchange_costs(group, (transaction.flows, source.loc), detail.cost, day)?;
            detail.cost = cost.or(detail.cost);
        }
        let amounts = self.amounts(&flow, Some(id));
        if computed_quantity {
            self.record.resolved.insert(id, amounts);
        }
        if let Some(basis) = computed_basis {
            self.record.computed_basis.insert(id, basis);
        }
        let txn = RuntimeTxn::journal(source.txn).expect("a journal flow cannot name the template sentinel");
        // A computed basis is a call-local override: borrow it for this motion and allocate no runtime detail.
        let view = book.flow_view_with_detail(&flow, &detail);
        let motion = Motion::from_view_at(book, view, txn, Cause::Flow(id), day, amounts, offset.unwrap_or_default());
        self.post(&motion.running(course));
        Ok(())
    }

    /// A flow's quantities. `?` amounts were solved before the fold, and are
    /// the plan's; `=` and `all` depend on the balance and are resolved now,
    /// once, and remembered (a reversal must undo exactly what was done).
    pub(crate) fn amounts(&mut self, flow: &Flow, id: Option<Id<Flow>>) -> Amounts {
        if let Some(done) = id.and_then(|id| solved(self.plan, &self.record, id, flow)) {
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
    pub(crate) fn everything(&self, flow: &Flow, written: Amounts) -> Amounts {
        let book = self.plan.book;
        let slot = self.world.holdings.get(flow.from, flow.out.unit);
        let qty = if book.places[flow.from].class == Class::Asset {
            let money = is_money(self.plan, flow.from, flow.out.unit);
            let view = book.flow_view(flow);
            slot.map_or(Qty::ZERO, |slot| slot.admitted(money, view.select(), &book.codes))
        } else {
            slot.map_or(Qty::ZERO, |slot| slot.plain.max(Qty::ZERO))
        };
        Amounts { out: qty, arrive: if flow.is_exchange() { written.arrive } else { qty } }
    }

    /// `= 5_000 USD`: whatever leaves the source, or arrives at the target,
    /// so that its place holds `balance` afterwards.
    pub(crate) fn resolve_target(&mut self, flow: &Flow, end: End, balance: Qty, written: Amounts) -> Amounts {
        let book = self.plan.book;
        let (place, unit) = match end {
            End::From => (flow.from, flow.out.unit),
            End::To => (flow.to, flow.arrive.unit),
        };
        // The target is written in the place's display sign.
        let (held, target) = (self.world.holdings.qty(place, unit), self.plan.sides.display(place, balance));
        let gap = if end == End::From { held - target } else { target - held };
        let qty = if gap.is_negative() {
            let shown = (self.plan.sides.display(place, held), balance);
            self.record.report(explain::past_target(book, flow, place, unit, shown, end));
            Qty::ZERO
        } else {
            gap
        };
        match (end, flow.is_exchange()) {
            (_, false) => Amounts { out: qty, arrive: qty },
            (End::From, true) => Amounts { out: qty, ..written },
            (End::To, true) => Amounts { arrive: qty, ..written },
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
        let amounts = solved(plan, record, id, flow).unwrap_or_else(|| Amounts::written(flow));
        Posted { out: amounts.out, arrive: amounts.arrive, state: plan.events.state(id, flow) }
    };
    let stretch = |&first: &usize| {
        let ids = (first..(first + STRETCH).min(book.flows.len())).map(|at| Id::new(at as u32));
        ids.map(post).collect::<Vec<_>>()
    };
    par::map_each_ordered(&stretches, stretch, |made| all.extend(made));
    all.into()
}

/// A flow's quantities where they are already solved: as written, as the
/// plan solved a `?`, or as the fold resolved an `=` or `all`. Only the last
/// depends on the fold, and only it is looked up in the record.
pub(crate) fn solved(plan: &Plan, record: &Record, id: Id<Flow>, flow: &Flow) -> Option<Amounts> {
    match flow.infer {
        Infer::Known => Some(record.resolved.get(&id).copied().unwrap_or_else(|| Amounts::written(flow))),
        Infer::Unknown => Some(plan.amounts.get(&id).copied().unwrap_or_else(|| Amounts::written(flow))),
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
pub(crate) fn fold_to_view<'p, 'b, 's>(plan: &'p Plan<'b, 's>, options: Options) -> (Run, Ledger<'p, 'b, 's>) {
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
