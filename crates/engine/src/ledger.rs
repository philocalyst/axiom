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

use axiom_core::{Arena, Cadence, Day, Diagnostic, Id, Qty, Ratio, Span, par};
use axiom_model::{
    Amount, Book, Commodity, Contract, End, Fault, Flow, FlowExpressions, FlowSide, FlowView,
    Infer, JournalGroup, JournalItem, JournalProgram, JournalQuantity, Mode, OccurrenceTail,
    Origin, Place, PurposeRoot, RuntimeDetail, RuntimeFlow, RuntimeTxn, ScheduleKind, Sign,
    Subject, TemplateAmount, TemplateFlow, TemplateItemParent, TemplateLeg, TemplateProgram,
    TemplateQuantity, Terms, Value,
};

use crate::checkpoint::CheckpointPhase;
use crate::motion::{Amounts, Motion};
use crate::plan::Plan;
use crate::scope::is_money;
use crate::state::{Record, Scratch, World};
use crate::timeline::{Fact, Moment, Timeline};
use crate::{Applied, Cause, Holding, Options, Posted, Recorded, Run, State, explain};

/// Why one native contract occurrence could not be materialized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateError {
    /// The requested schedule has no active terms on its due day.
    OutsideTerms { contract: Id<Contract>, day: Day },
    /// A typed expression failed while this occurrence was evaluated.
    Expression { fault: Fault, loc: axiom_core::Loc },
    /// Contract escalation or recognition could not be resolved.
    Forecast(axiom_model::ForecastError),
    /// A grouped template refers to an invalid parent or unsupported derived quantity.
    InvalidTemplate { loc: axiom_core::Loc },
}

/// The shared-pool ranges appended by one materialization call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OccurrenceOutput {
    pub flows: crate::RuntimeRange,
    pub missing_inputs: crate::RuntimeRange,
}

impl OccurrenceOutput {
    pub fn flows<'a>(self, pool: &'a [RuntimeFlow]) -> Option<&'a [RuntimeFlow]> {
        self.flows.get(pool)
    }

    pub fn missing<'a>(self, pool: &'a [u16]) -> Option<&'a [u16]> {
        self.missing_inputs.get(pool)
    }
}

#[derive(Clone, Copy)]
struct ResolvedQuantity {
    amount: Amount,
    infer: Infer,
    mode: Mode,
}

#[derive(Clone, Copy)]
enum ResolvedLeg {
    Rest,
    Omitted,
    Value(ResolvedQuantity),
}

#[derive(Clone, Copy)]
enum OccurrenceAmount {
    Inherit,
    Omitted,
    Value(Amount),
}

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
        self.apply_view(flow, view, txn, 0)
    }

    /// Applies a forecast flow whose metadata is pooled in the Book and whose
    /// detail may be overridden in the forecast's immutable runtime arena.
    pub fn apply_runtime(&mut self, flow: &RuntimeFlow, details: &Arena<RuntimeDetail>) -> Applied {
        let view = self.plan.book.runtime_flow_view(flow, details);
        self.apply_view(&flow.flow, view, flow.txn, flow.ordinal)
    }

    /// Materializes one scheduled contract occurrence into caller-owned pools.
    /// Journaled and forecast occurrences use this same path; grouped header,
    /// split-leg, and item order is preserved, and an omitted input suppresses
    /// only the template component whose expression reads it.
    pub fn instantiate_occurrence(
        &mut self,
        contract_id: Id<Contract>,
        schedule: ScheduleKind,
        due: Day,
        ordinal: u32,
        source: Option<Id<axiom_model::Txn>>,
        flows: &mut Vec<RuntimeFlow>,
        details: &mut Arena<RuntimeDetail>,
        missing_inputs: &mut Vec<u16>,
    ) -> Result<OccurrenceOutput, TemplateError> {
        let book = self.plan.book;
        let contract = book
            .contracts
            .get(contract_id)
            .ok_or(TemplateError::OutsideTerms {
                contract: contract_id,
                day: due,
            })?;
        if !contract.days.contains(due) {
            return Err(TemplateError::OutsideTerms {
                contract: contract_id,
                day: due,
            });
        }
        let terms =
            contract
                .terms_on_schedule(schedule, due)
                .ok_or(TemplateError::OutsideTerms {
                    contract: contract_id,
                    day: due,
                })?;
        if terms.is_waived() {
            return Err(TemplateError::Forecast(axiom_model::ForecastError::Waived(
                due,
            )));
        }
        let (source_day, written) = if let Some(txn_id) = source {
            let txn = book.txns.get(txn_id).ok_or(TemplateError::OutsideTerms {
                contract: contract_id,
                day: due,
            })?;
            let exact = txn
                .occurrence
                .and_then(|id| book.written_occurrences.get(id));
            if txn.contract != Some(contract_id)
                || txn.contract_schedule != Some(schedule)
                || !exact.is_some_and(|occurrence| {
                    occurrence.due == due && occurrence.schedule == schedule
                })
            {
                return Err(TemplateError::OutsideTerms {
                    contract: contract_id,
                    day: due,
                });
            }
            (txn.day, exact)
        } else {
            (due, None)
        };
        let inputs = source.map_or(&[][..], |txn| book.txn_inputs(txn));
        let source_txn = source.and_then(|txn| book.txns.get(txn));
        let empty_flows: &[Flow] = &[];
        let source_flows = source_txn.map_or(empty_flows, |txn| &book.flows[txn.flows]);
        let ratio = contract
            .amount_on_schedule(book, schedule, due)
            .map_err(TemplateError::Forecast)?;
        let runtime_txn =
            RuntimeTxn::contract_occurrence(contract_id, schedule, due, ordinal, source);
        let (flow_start, detail_start, missing_start) =
            (flows.len(), details.len(), missing_inputs.len());

        let occurrence_journal = written
            .and_then(|written| written.program)
            .and_then(|program| book.journal_programs.get(program));
        let occurrence_program = occurrence_journal.map(|journal| &journal.program);
        if let Some(written) = written {
            for (index, group) in written.groups.iter().enumerate() {
                let valid_offsets = group
                    .group
                    .header
                    .into_iter()
                    .chain(group.group.legs.iter().copied())
                    .chain(group.group.items.iter().filter_map(|item| item.flow))
                    .all(|offset| (offset as usize) < source_flows.len());
                if group.template as usize >= terms.template.len()
                    || group.group.legs.len() != group.group.leg_quantities.len()
                    || !valid_offsets
                    || written.groups[..index]
                        .iter()
                        .any(|previous| previous.template == group.template)
                {
                    return Err(TemplateError::InvalidTemplate { loc: contract.loc });
                }
            }
        }
        let amount_override = match written.and_then(|written| written.amount) {
            None => OccurrenceAmount::Inherit,
            Some(TemplateAmount::Literal(amount)) => OccurrenceAmount::Value(amount),
            Some(TemplateAmount::Computed(root)) => {
                let Some(program) = occurrence_program else {
                    return Err(TemplateError::InvalidTemplate { loc: contract.loc });
                };
                let Some(template) = terms.template.first() else {
                    return Err(TemplateError::InvalidTemplate { loc: contract.loc });
                };
                match self.evaluate_program_root(
                    program,
                    &template.flow,
                    root,
                    source_day,
                    runtime_txn,
                    0,
                    inputs,
                    Ratio::ONE,
                    missing_inputs,
                )? {
                    Some(amount) => OccurrenceAmount::Value(amount),
                    None => OccurrenceAmount::Omitted,
                }
            }
        };
        let tail = written.map(|written| &written.tail);

        let mut ordinal_base = 0u32;
        let result = (|| {
            for (group_index, template) in terms.template.iter().enumerate() {
                let written_group = written.and_then(|written| {
                    written
                        .groups
                        .iter()
                        .find(|group| group.template as usize == group_index)
                });
                let width = 1usize
                    .checked_add(template.legs.len())
                    .and_then(|width| {
                        width.checked_add(written_group.map_or(0, |group| group.group.legs.len()))
                    })
                    .and_then(|width| width.checked_add(template.items.len()))
                    .and_then(|width| {
                        width.checked_add(written_group.map_or(0, |group| group.group.items.len()))
                    })
                    .and_then(|width| u32::try_from(width).ok())
                    .ok_or(TemplateError::InvalidTemplate {
                        loc: template.flow.loc,
                    })?;
                self.materialize_group(
                    contract_id,
                    schedule,
                    due,
                    source_day,
                    runtime_txn,
                    terms,
                    template,
                    group_index,
                    ordinal_base,
                    ratio,
                    amount_override,
                    written_group,
                    source_flows,
                    tail,
                    occurrence_journal,
                    inputs,
                    flows,
                    details,
                    missing_inputs,
                )?;
                ordinal_base =
                    ordinal_base
                        .checked_add(width)
                        .ok_or(TemplateError::InvalidTemplate {
                            loc: template.flow.loc,
                        })?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            flows.truncate(flow_start);
            details.truncate(detail_start);
            missing_inputs.truncate(missing_start);
            return Err(error);
        }
        // A caller may reuse these pools across occurrences. Keep each output
        // range self-contained even when the same binding is missing twice.
        missing_inputs[missing_start..].sort_unstable();
        let mut write = missing_start;
        for read in missing_start..missing_inputs.len() {
            let input = missing_inputs[read];
            if write == missing_start || missing_inputs[write - 1] != input {
                missing_inputs[write] = input;
                write += 1;
            }
        }
        missing_inputs.truncate(write);

        Ok(OccurrenceOutput {
            flows: crate::RuntimeRange::new(flow_start, flows.len() - flow_start),
            missing_inputs: crate::RuntimeRange::new(
                missing_start,
                missing_inputs.len() - missing_start,
            ),
        })
    }

    fn materialize_group(
        &mut self,
        contract_id: Id<Contract>,
        schedule: ScheduleKind,
        due: Day,
        source_day: Day,
        runtime_txn: RuntimeTxn,
        terms: &Terms,
        template: &TemplateFlow,
        group_index: usize,
        ordinal_base: u32,
        ratio: axiom_core::Ratio,
        amount_override: OccurrenceAmount,
        written_group: Option<&axiom_model::WrittenGroup>,
        source_flows: &[Flow],
        tail: Option<&OccurrenceTail>,
        occurrence_journal: Option<&JournalProgram>,
        inputs: &[Option<Amount>],
        out: &mut Vec<RuntimeFlow>,
        details: &mut Arena<RuntimeDetail>,
        missing: &mut Vec<u16>,
    ) -> Result<(), TemplateError> {
        let book = self.plan.book;
        let group_start = out.len();
        let recognized = book.contracts[contract_id]
            .recognition_on_schedule(&template.flow, schedule, due)
            .map_err(TemplateError::Forecast)?;

        let mut header = written_group
            .and_then(|group| group.group.header)
            .and_then(|offset| source_flows.get(offset as usize))
            .cloned()
            .unwrap_or_else(|| template.flow.clone());
        header.recognized = tail.and_then(|tail| tail.recognized).unwrap_or(recognized);
        header.origin = Origin::Occurrence(contract_id);
        header.mode = if runtime_txn.source_txn().is_some() {
            Mode::Actual
        } else {
            Mode::Planned
        };
        if let Some(tail) = tail {
            if tail.codes.len() > 0 {
                header.header_codes = tail.codes;
            }
            if let Some(waive) = tail.waive {
                header.waive = Some(waive);
            }
            if group_index == 0 {
                if let Some(purpose) = tail.purpose {
                    header.purpose = Some(purpose);
                }
                if let Some(description) = tail.description {
                    header.description = Some(description);
                }
                if let Some(payee) = tail.payee {
                    header.payee = Some(payee);
                }
            }
        }
        let header_ordinal = ordinal_base;
        let out_value = if let Some(quantity) = written_group.and_then(|group| group.out) {
            match self.written_quantity(
                contract_id,
                terms,
                &header,
                quantity,
                End::From,
                due,
                runtime_txn,
                header_ordinal,
                inputs,
                occurrence_journal,
                missing,
            )? {
                ResolvedLeg::Value(value) => Some(value),
                ResolvedLeg::Omitted => None,
                ResolvedLeg::Rest => {
                    return Err(TemplateError::InvalidTemplate { loc: header.loc });
                }
            }
        } else {
            self.template_quantity(
                contract_id,
                terms,
                &header,
                template.out,
                End::From,
                due,
                runtime_txn,
                header_ordinal,
                inputs,
                ratio,
                missing,
            )?
        };
        let arrive_value = if let Some(quantity) = written_group.and_then(|group| group.arrive) {
            match self.written_quantity(
                contract_id,
                terms,
                &header,
                quantity,
                End::To,
                due,
                runtime_txn,
                header_ordinal,
                inputs,
                occurrence_journal,
                missing,
            )? {
                ResolvedLeg::Value(value) => Some(value),
                ResolvedLeg::Omitted => None,
                ResolvedLeg::Rest => {
                    return Err(TemplateError::InvalidTemplate { loc: header.loc });
                }
            }
        } else {
            self.template_quantity(
                contract_id,
                terms,
                &header,
                template.arrive,
                End::To,
                due,
                runtime_txn,
                header_ordinal,
                inputs,
                ratio,
                missing,
            )?
        };
        // A missing binding must not silently reuse the compile-time Flow
        // placeholder. Omit this group and retain the missing-input index.
        if (has_computed_quantity(template.out) && out_value.is_none())
            || (has_computed_quantity(template.arrive) && arrive_value.is_none())
            || written_group
                .and_then(|group| group.out)
                .is_some_and(|quantity| journal_quantity_root(quantity).is_some())
                && out_value.is_none()
            || written_group
                .and_then(|group| group.arrive)
                .is_some_and(|quantity| journal_quantity_root(quantity).is_some())
                && arrive_value.is_none()
        {
            return Ok(());
        }
        if group_index == 0 && matches!(amount_override, OccurrenceAmount::Omitted) {
            return Ok(());
        }
        if let Some(value) = out_value {
            set_quantity(&mut header, End::From, value);
        }
        if let Some(value) = arrive_value {
            set_quantity(&mut header, End::To, value);
        }
        if let (Some(out), Some(arrive)) = (out_value, arrive_value) {
            header.infer = if out.infer != Infer::Known {
                out.infer
            } else {
                arrive.infer
            };
            if out.mode == Mode::Pending || arrive.mode == Mode::Pending {
                header.mode = Mode::Pending;
            }
        }
        let out_is_explicit = written_group.and_then(|group| group.out).is_some()
            || has_computed_quantity(template.out);
        let arrive_is_explicit = written_group.and_then(|group| group.arrive).is_some()
            || has_computed_quantity(template.arrive);
        if !header.is_exchange() {
            if out_is_explicit && !arrive_is_explicit {
                if let Some(out) = out_value {
                    header.arrive = out.amount;
                    header.infer = out.infer;
                    header.mode = out.mode;
                }
            } else if arrive_is_explicit && !out_is_explicit {
                if let Some(arrive) = arrive_value {
                    header.out = arrive.amount;
                    header.infer = arrive.infer;
                    header.mode = arrive.mode;
                }
            }
        }
        if group_index == 0 {
            if let OccurrenceAmount::Value(amount) = amount_override {
                apply_occurrence_amount(
                    book,
                    &mut header,
                    book.contracts[contract_id].buys,
                    due,
                    amount,
                )
                .map_err(|fault| TemplateError::Expression {
                    fault,
                    loc: template.flow.loc,
                })?;
            }
        }
        if let Some(value) = bought_quantity(
            book,
            book.contracts[contract_id].buys,
            due,
            header.out,
            header.arrive,
            header.infer,
        )? {
            header.out = value.0;
            header.arrive = value.1;
            header.infer = Infer::Known;
        }

        let mut detail_override = None;
        if group_index == 0 {
            if let Some(tail) = tail {
                let base = *book.flow_view(&header).detail();
                let mut detail = merge_detail(base, tail.detail);
                if let Some(root) = tail.basis {
                    let Some(program) = occurrence_journal.map(|journal| &journal.program) else {
                        return Err(TemplateError::InvalidTemplate { loc: header.loc });
                    };
                    let Some(amount) = self.evaluate_program_root(
                        program,
                        &header,
                        root,
                        source_day,
                        runtime_txn,
                        header_ordinal,
                        inputs,
                        Ratio::ONE,
                        missing,
                    )?
                    else {
                        return Ok(());
                    };
                    if amount.unit != book.base {
                        return Err(TemplateError::Expression {
                            fault: Fault::UnitMismatch {
                                found: amount.unit,
                                expected: book.base,
                            },
                            loc: header.loc,
                        });
                    }
                    detail.basis = Some(amount.qty);
                }
                detail_override = Some(detail);
            }
        }
        self.push_occurrence_flow(
            header,
            source_day,
            runtime_txn,
            header_ordinal,
            detail_override,
            details,
            out,
        );

        let extra_leg_capacity = written_group.map_or(0, |group| group.group.legs.len());
        let mut leg_values = Vec::with_capacity(template.legs.len() + extra_leg_capacity);
        let mut leg_flows = Vec::with_capacity(template.legs.len() + extra_leg_capacity);
        let mut leg_sides = Vec::with_capacity(template.legs.len() + extra_leg_capacity);
        let mut leg_ordinals = Vec::with_capacity(template.legs.len() + extra_leg_capacity);
        let mut rest_ends = [None; 2];
        for (leg_index, leg) in template.legs.iter().enumerate() {
            let flow_ordinal = ordinal_base
                .checked_add(1)
                .and_then(|base| base.checked_add(u32::try_from(leg_index).ok()?))
                .ok_or(TemplateError::InvalidTemplate { loc: leg.flow.loc })?;
            let written_leg = written_leg_for_template(written_group, source_flows, leg);
            let (mut flow, value) = if let Some((_, source_flow, quantity)) = written_leg {
                let flow = source_flow.clone();
                let value = self.written_quantity(
                    contract_id,
                    terms,
                    &flow,
                    quantity,
                    match leg.side {
                        FlowSide::Out => End::From,
                        FlowSide::Arrive => End::To,
                    },
                    due,
                    runtime_txn,
                    flow_ordinal,
                    inputs,
                    occurrence_journal,
                    missing,
                )?;
                (flow, value)
            } else {
                let flow = leg.flow.clone();
                let value = match leg.quantity {
                    TemplateQuantity::Rest => ResolvedLeg::Rest,
                    TemplateQuantity::Percent(rate) => {
                        // A bare percentage is relative to the original header
                        // value, before any leg carves have changed it.
                        let parent = match leg.side {
                            FlowSide::Out => out[group_start].flow.out,
                            FlowSide::Arrive => out[group_start].flow.arrive,
                        };
                        let amount = scale_template_amount(parent, rate).map_err(|fault| {
                            TemplateError::Expression {
                                fault,
                                loc: leg.flow.loc,
                            }
                        })?;
                        ResolvedLeg::Value(ResolvedQuantity {
                            amount,
                            infer: Infer::Known,
                            mode: flow.mode,
                        })
                    }
                    quantity => match self.template_quantity(
                        contract_id,
                        terms,
                        &flow,
                        quantity,
                        match leg.side {
                            FlowSide::Out => End::From,
                            FlowSide::Arrive => End::To,
                        },
                        due,
                        runtime_txn,
                        flow_ordinal,
                        inputs,
                        ratio,
                        missing,
                    )? {
                        Some(value) => ResolvedLeg::Value(value),
                        None if has_computed_quantity(leg.quantity) => ResolvedLeg::Omitted,
                        None => {
                            return Err(TemplateError::InvalidTemplate { loc: leg.flow.loc });
                        }
                    },
                };
                (flow, value)
            };
            flow.recognized = tail.and_then(|tail| tail.recognized).unwrap_or(recognized);
            flow.origin = Origin::Occurrence(contract_id);
            flow.mode = if runtime_txn.source_txn().is_some() {
                Mode::Actual
            } else {
                Mode::Planned
            };
            if let Some(tail) = tail {
                if tail.codes.len() > 0 {
                    flow.header_codes = tail.codes;
                }
                if let Some(waive) = tail.waive {
                    flow.waive = Some(waive);
                }
            }
            if matches!(value, ResolvedLeg::Rest) {
                let side = match leg.side {
                    FlowSide::Out => 0,
                    FlowSide::Arrive => 1,
                };
                if rest_ends[side].replace(leg_index).is_some() {
                    return Err(TemplateError::InvalidTemplate { loc: flow.loc });
                }
            }
            leg_flows.push(flow);
            leg_sides.push(leg.side);
            leg_ordinals.push(flow_ordinal);
            leg_values.push(value);
        }
        // Written legs that do not replace a named template leg are additional
        // members of the same split. Their explicit amounts reduce the header
        // before any remainder leg is resolved.
        if let Some(written_group) = written_group {
            if written_group.group.legs.len() != written_group.group.leg_quantities.len() {
                return Err(TemplateError::InvalidTemplate {
                    loc: template.flow.loc,
                });
            }
            for (written_index, (&offset, &quantity)) in written_group
                .group
                .legs
                .iter()
                .zip(written_group.group.leg_quantities.iter())
                .enumerate()
            {
                let Some(flow) = source_flows.get(offset as usize) else {
                    return Err(TemplateError::InvalidTemplate {
                        loc: template.flow.loc,
                    });
                };
                if template
                    .legs
                    .iter()
                    .any(|leg| same_flow_ends(flow, &leg.flow))
                {
                    continue;
                }
                let flow_ordinal = ordinal_base
                    .checked_add(1)
                    .and_then(|base| base.checked_add(u32::try_from(template.legs.len()).ok()?))
                    .and_then(|base| base.checked_add(u32::try_from(written_index).ok()?))
                    .ok_or(TemplateError::InvalidTemplate { loc: flow.loc })?;
                let mut flow = flow.clone();
                flow.recognized = tail.and_then(|tail| tail.recognized).unwrap_or(recognized);
                flow.origin = Origin::Occurrence(contract_id);
                flow.mode = if runtime_txn.source_txn().is_some() {
                    Mode::Actual
                } else {
                    Mode::Planned
                };
                if let Some(tail) = tail {
                    if !tail.codes.is_empty() {
                        flow.header_codes = tail.codes;
                    }
                    if let Some(waive) = tail.waive {
                        flow.waive = Some(waive);
                    }
                }
                let value = self.written_quantity(
                    contract_id,
                    terms,
                    &flow,
                    quantity,
                    match written_group.group.side {
                        FlowSide::Out => End::From,
                        FlowSide::Arrive => End::To,
                    },
                    due,
                    runtime_txn,
                    flow_ordinal,
                    inputs,
                    occurrence_journal,
                    missing,
                )?;
                if matches!(value, ResolvedLeg::Rest) {
                    let side = match written_group.group.side {
                        FlowSide::Out => 0,
                        FlowSide::Arrive => 1,
                    };
                    if rest_ends[side].replace(leg_values.len()).is_some() {
                        return Err(TemplateError::InvalidTemplate { loc: flow.loc });
                    }
                }
                leg_flows.push(flow);
                leg_sides.push(written_group.group.side);
                leg_ordinals.push(flow_ordinal);
                leg_values.push(value);
            }
        }
        // Resolve every explicit carve before Rest, even when the remainder
        // was written earlier in the source. Emission below stays source-ordered.
        for (index, value) in leg_values.iter().enumerate() {
            if let ResolvedLeg::Value(value) = value {
                subtract_parent(&mut out[group_start].flow, leg_sides[index], value.amount)
                    .map_err(|fault| TemplateError::Expression {
                        fault,
                        loc: leg_flows[index].loc,
                    })?;
            }
        }
        for leg_index in 0..leg_values.len() {
            if let ResolvedLeg::Rest = leg_values[leg_index] {
                let amount = match leg_sides[leg_index] {
                    FlowSide::Out => out[group_start].flow.out,
                    FlowSide::Arrive => out[group_start].flow.arrive,
                };
                leg_values[leg_index] = ResolvedLeg::Value(ResolvedQuantity {
                    amount,
                    infer: Infer::Known,
                    mode: if runtime_txn.source_txn().is_some() {
                        Mode::Actual
                    } else {
                        Mode::Planned
                    },
                });
                subtract_parent(&mut out[group_start].flow, leg_sides[leg_index], amount).map_err(
                    |fault| TemplateError::Expression {
                        fault,
                        loc: leg_flows[leg_index].loc,
                    },
                )?;
            }
        }

        let mut leg_positions = Vec::with_capacity(leg_flows.len());
        for ((mut flow, side), (value, flow_ordinal)) in leg_flows
            .into_iter()
            .zip(leg_sides.into_iter())
            .zip(leg_values.into_iter().zip(leg_ordinals.into_iter()))
        {
            let ResolvedLeg::Value(value) = value else {
                leg_positions.push(None);
                continue;
            };
            match side {
                FlowSide::Out => flow.out = value.amount,
                FlowSide::Arrive => flow.arrive = value.amount,
            }
            if !flow.is_exchange() {
                flow.out = value.amount;
                flow.arrive = value.amount;
            }
            flow.infer = value.infer;
            flow.mode = value.mode;
            let leg_position = out.len();
            self.push_occurrence_flow(
                flow,
                source_day,
                runtime_txn,
                flow_ordinal,
                None,
                details,
                out,
            );
            debug_assert!(leg_position >= group_start + 1);
            leg_positions.push(Some(leg_position));
        }

        for (item_index, item) in template.items.iter().enumerate() {
            let parent_index = match item.parent {
                TemplateItemParent::Header => group_start,
                TemplateItemParent::Leg(index) => {
                    match leg_positions.get(index as usize) {
                        Some(Some(position)) => *position,
                        Some(None) => {
                            // A missing input can omit a leg. Any item
                            // attached to that leg has no parent to reduce.
                            continue;
                        }
                        None => return Err(TemplateError::InvalidTemplate { loc: item.loc }),
                    }
                }
            };
            let Some(parent) = out.get(parent_index).map(|runtime| runtime.flow.clone()) else {
                return Err(TemplateError::InvalidTemplate { loc: item.loc });
            };
            let flow_ordinal = ordinal_base
                .checked_add(1)
                .and_then(|base| base.checked_add(u32::try_from(template.legs.len()).ok()?))
                .and_then(|base| {
                    base.checked_add(
                        u32::try_from(written_group.map_or(0, |group| group.group.legs.len()))
                            .ok()?,
                    )
                })
                .and_then(|base| base.checked_add(u32::try_from(item_index).ok()?))
                .ok_or(TemplateError::InvalidTemplate { loc: item.loc })?;
            let amount =
                match item.amount {
                    TemplateAmount::Literal(amount) => scale_template_amount(amount, ratio)
                        .map_err(|fault| TemplateError::Expression {
                            fault,
                            loc: item.loc,
                        })?,
                    TemplateAmount::Computed(root) => {
                        let Some(amount) = self.evaluate_template_root(
                            terms,
                            &parent,
                            root,
                            due,
                            runtime_txn,
                            flow_ordinal,
                            inputs,
                            ratio,
                            missing,
                        )?
                        else {
                            continue;
                        };
                        amount
                    }
                };
            if item.sign == Sign::Carve || (item.sign == Sign::Less && item.purpose.is_none()) {
                subtract_parent(&mut out[parent_index].flow, item.side, amount).map_err(
                    |fault| TemplateError::Expression {
                        fault,
                        loc: item.loc,
                    },
                )?;
            }
            let Some(purpose) = item.purpose else {
                continue;
            };
            let mut flow = parent;
            flow.day = due;
            flow.recognized = tail.and_then(|tail| tail.recognized).unwrap_or(recognized);
            flow.mode = if runtime_txn.source_txn().is_some() {
                Mode::Actual
            } else {
                Mode::Planned
            };
            flow.origin = Origin::Occurrence(contract_id);
            if let Some(tail) = tail {
                if tail.codes.len() > 0 {
                    flow.header_codes = tail.codes;
                }
                if let Some(waive) = tail.waive {
                    flow.waive = Some(waive);
                }
            }
            flow.purpose = Some(purpose);
            flow.description = item.description.or(flow.description);
            flow.codes = item.codes;
            flow.select = item.select;
            flow.detail = item.detail;
            flow.waive = item.waive.or(flow.waive);
            flow.loc = item.loc;
            if item.sign == Sign::Less {
                std::mem::swap(&mut flow.from, &mut flow.to);
            }
            flow.out = amount;
            flow.arrive = amount;
            flow.infer = Infer::Known;
            self.push_occurrence_flow(
                flow,
                source_day,
                runtime_txn,
                flow_ordinal,
                None,
                details,
                out,
            );
        }

        if let Some(written_group) = written_group {
            for (item_index, item) in written_group.group.items.iter().enumerate() {
                let parent_index = match item.parent {
                    TemplateItemParent::Header => group_start,
                    TemplateItemParent::Leg(index) => match leg_positions.get(index as usize) {
                        Some(Some(position)) => *position,
                        Some(None) => continue,
                        None => return Err(TemplateError::InvalidTemplate { loc: item.loc }),
                    },
                };
                let Some(parent) = out.get(parent_index).map(|runtime| runtime.flow.clone()) else {
                    return Err(TemplateError::InvalidTemplate { loc: item.loc });
                };
                let source_item = item
                    .flow
                    .and_then(|offset| source_flows.get(offset as usize));
                let context_flow = source_item.unwrap_or(&parent);
                let flow_ordinal = ordinal_base
                    .checked_add(1)
                    .and_then(|base| base.checked_add(u32::try_from(template.legs.len()).ok()?))
                    .and_then(|base| {
                        base.checked_add(u32::try_from(written_group.group.legs.len()).ok()?)
                    })
                    .and_then(|base| base.checked_add(u32::try_from(template.items.len()).ok()?))
                    .and_then(|base| base.checked_add(u32::try_from(item_index).ok()?))
                    .ok_or(TemplateError::InvalidTemplate { loc: item.loc })?;
                let amount = match item.amount {
                    TemplateAmount::Literal(amount) => amount,
                    TemplateAmount::Computed(root) => {
                        let Some(program) = occurrence_journal.map(|journal| &journal.program)
                        else {
                            return Err(TemplateError::InvalidTemplate { loc: item.loc });
                        };
                        let Some(amount) = self.evaluate_program_root(
                            program,
                            context_flow,
                            root,
                            due,
                            runtime_txn,
                            flow_ordinal,
                            inputs,
                            Ratio::ONE,
                            missing,
                        )?
                        else {
                            continue;
                        };
                        amount
                    }
                };
                let has_purpose = source_item.is_some_and(|flow| flow.purpose.is_some());
                if item.sign == Sign::Carve || (item.sign == Sign::Less && !has_purpose) {
                    subtract_parent(&mut out[parent_index].flow, item.side, amount).map_err(
                        |fault| TemplateError::Expression {
                            fault,
                            loc: item.loc,
                        },
                    )?;
                }
                let Some(source_item) = source_item.filter(|_| has_purpose) else {
                    continue;
                };
                let mut flow = source_item.clone();
                flow.day = due;
                flow.recognized = tail.and_then(|tail| tail.recognized).unwrap_or(recognized);
                flow.mode = if runtime_txn.source_txn().is_some() {
                    Mode::Actual
                } else {
                    Mode::Planned
                };
                flow.origin = Origin::Occurrence(contract_id);
                if let Some(tail) = tail {
                    if !tail.codes.is_empty() {
                        flow.header_codes = tail.codes;
                    }
                    if let Some(waive) = tail.waive {
                        flow.waive = Some(waive);
                    }
                }
                flow.out = amount;
                flow.arrive = amount;
                flow.infer = Infer::Known;
                self.push_occurrence_flow(
                    flow,
                    source_day,
                    runtime_txn,
                    flow_ordinal,
                    None,
                    details,
                    out,
                );
            }
        }
        Ok(())
    }

    fn template_quantity(
        &mut self,
        contract_id: Id<Contract>,
        terms: &Terms,
        flow: &Flow,
        quantity: TemplateQuantity,
        end: End,
        due: Day,
        txn: RuntimeTxn,
        ordinal: u32,
        inputs: &[Option<Amount>],
        ratio: axiom_core::Ratio,
        missing: &mut Vec<u16>,
    ) -> Result<Option<ResolvedQuantity>, TemplateError> {
        let (book, loc) = (self.plan.book, flow.loc);
        let source = match end {
            End::From => flow.out,
            End::To => flow.arrive,
        };
        let resolved = match quantity {
            TemplateQuantity::Amount(root) => match root {
                Some(root) => self.evaluate_template_root(
                    terms, flow, root, due, txn, ordinal, inputs, ratio, missing,
                )?,
                None => Some(
                    scale_template_amount(source, ratio)
                        .map_err(|fault| TemplateError::Expression { fault, loc })?,
                ),
            }
            .map(|amount| ResolvedQuantity {
                amount,
                infer: Infer::Known,
                mode: flow.mode,
            }),
            TemplateQuantity::Pending(root) => match root {
                Some(root) => self.evaluate_template_root(
                    terms, flow, root, due, txn, ordinal, inputs, ratio, missing,
                )?,
                None => Some(
                    scale_template_amount(source, ratio)
                        .map_err(|fault| TemplateError::Expression { fault, loc })?,
                ),
            }
            .map(|amount| ResolvedQuantity {
                amount,
                infer: Infer::Known,
                mode: Mode::Pending,
            }),
            TemplateQuantity::Target(root) => match root {
                Some(root) => self.evaluate_template_root(
                    terms, flow, root, due, txn, ordinal, inputs, ratio, missing,
                )?,
                None => Some(
                    scale_template_amount(source, ratio)
                        .map_err(|fault| TemplateError::Expression { fault, loc })?,
                ),
            }
            .map(|amount| ResolvedQuantity {
                amount,
                infer: Infer::Target {
                    end,
                    balance: amount.qty,
                },
                mode: flow.mode,
            }),
            TemplateQuantity::Unknown(unit) => Some(ResolvedQuantity {
                amount: Amount::zero(unit),
                infer: Infer::Unknown,
                mode: flow.mode,
            }),
            TemplateQuantity::All(unit) => Some(ResolvedQuantity {
                amount: Amount::zero(unit.unwrap_or(source.unit)),
                infer: Infer::All,
                mode: flow.mode,
            }),
            TemplateQuantity::Percent(_) => {
                return Err(TemplateError::InvalidTemplate { loc });
            }
            TemplateQuantity::Rest => return Err(TemplateError::InvalidTemplate { loc }),
            TemplateQuantity::Whole => Some(ResolvedQuantity {
                amount: Amount::new(Qty(1), source.unit),
                infer: Infer::Known,
                mode: Mode::Opening,
            }),
            TemplateQuantity::Derived => {
                let contract = &book.contracts[contract_id];
                let amount = loan_payment(contract, terms).ok_or(TemplateError::Forecast(
                    axiom_model::ForecastError::UnsupportedLoan(due),
                ))?;
                Some(ResolvedQuantity {
                    amount,
                    infer: Infer::Known,
                    mode: flow.mode,
                })
            }
        };
        Ok(resolved)
    }

    fn written_quantity(
        &mut self,
        contract_id: Id<Contract>,
        terms: &Terms,
        flow: &Flow,
        quantity: JournalQuantity,
        end: End,
        due: Day,
        txn: RuntimeTxn,
        ordinal: u32,
        inputs: &[Option<Amount>],
        journal: Option<&JournalProgram>,
        missing: &mut Vec<u16>,
    ) -> Result<ResolvedLeg, TemplateError> {
        let source = match end {
            End::From => flow.out,
            End::To => flow.arrive,
        };
        let (amount, infer, mode, root) = match quantity {
            JournalQuantity::Amount(amount, root) => (amount, Infer::Known, flow.mode, root),
            JournalQuantity::Pending(amount, root) => (amount, Infer::Known, Mode::Pending, root),
            JournalQuantity::Target(amount, root) => (
                amount,
                Infer::Target {
                    end,
                    balance: amount.qty,
                },
                flow.mode,
                root,
            ),
            JournalQuantity::Unknown(unit) => {
                return Ok(ResolvedLeg::Value(ResolvedQuantity {
                    amount: Amount::zero(unit),
                    infer: Infer::Unknown,
                    mode: flow.mode,
                }));
            }
            JournalQuantity::All(unit) => {
                return Ok(ResolvedLeg::Value(ResolvedQuantity {
                    amount: Amount::zero(unit.unwrap_or(source.unit)),
                    infer: Infer::All,
                    mode: flow.mode,
                }));
            }
            JournalQuantity::Rest => return Ok(ResolvedLeg::Rest),
            JournalQuantity::Whole => {
                return Ok(ResolvedLeg::Value(ResolvedQuantity {
                    amount: Amount::new(Qty(1), source.unit),
                    infer: Infer::Known,
                    mode: Mode::Opening,
                }));
            }
            JournalQuantity::Derived => {
                let amount = loan_payment(&self.plan.book.contracts[contract_id], terms).ok_or(
                    TemplateError::Forecast(axiom_model::ForecastError::UnsupportedLoan(due)),
                )?;
                return Ok(ResolvedLeg::Value(ResolvedQuantity {
                    amount,
                    infer: Infer::Known,
                    mode: flow.mode,
                }));
            }
        };
        let amount = if let Some(root) = root {
            let Some(program) = journal.map(|journal| &journal.program) else {
                return Err(TemplateError::InvalidTemplate { loc: flow.loc });
            };
            let Some(amount) = self.evaluate_program_root(
                program,
                flow,
                root,
                due,
                txn,
                ordinal,
                inputs,
                Ratio::ONE,
                missing,
            )?
            else {
                return Ok(ResolvedLeg::Omitted);
            };
            amount
        } else {
            amount
        };
        let infer = match infer {
            Infer::Target { end, .. } => Infer::Target {
                end,
                balance: amount.qty,
            },
            infer => infer,
        };
        Ok(ResolvedLeg::Value(ResolvedQuantity {
            amount,
            infer,
            mode,
        }))
    }

    fn evaluate_template_root(
        &mut self,
        terms: &Terms,
        flow: &Flow,
        root: axiom_model::NodeId,
        due: Day,
        txn: RuntimeTxn,
        ordinal: u32,
        inputs: &[Option<Amount>],
        ratio: axiom_core::Ratio,
        missing: &mut Vec<u16>,
    ) -> Result<Option<Amount>, TemplateError> {
        self.evaluate_program_root(
            &terms.program,
            flow,
            root,
            due,
            txn,
            ordinal,
            inputs,
            ratio,
            missing,
        )
    }

    fn evaluate_program_root(
        &mut self,
        program: &TemplateProgram,
        flow: &Flow,
        root: axiom_model::NodeId,
        due: Day,
        txn: RuntimeTxn,
        ordinal: u32,
        inputs: &[Option<Amount>],
        ratio: axiom_core::Ratio,
        missing: &mut Vec<u16>,
    ) -> Result<Option<Amount>, TemplateError> {
        let book = self.plan.book;
        let node = program
            .nodes
            .as_slice()
            .get(root.0 as usize)
            .ok_or(TemplateError::InvalidTemplate { loc: flow.loc })?;
        let mut view_flow = flow.clone();
        view_flow.day = due;
        let motion = Motion::from_view_at(
            book,
            book.flow_view(&view_flow),
            txn,
            Cause::Applied(ordinal),
            due,
            Amounts::written(&view_flow),
            ordinal,
        );
        let occasion = crate::eval::Occasion::flow(&motion);
        let context = crate::eval::Context::new(Subject::Place(flow.from), flow.owner, &occasion)
            .for_flow()
            .with_inputs(inputs);
        let value = crate::eval::program_expression(
            crate::eval::Env {
                plan: self.plan,
                world: &self.world,
            },
            program,
            root,
            &context,
            &mut self.scratch.values,
        );
        match value {
            Value::Amount(amount) => {
                scale_template_amount(amount, ratio)
                    .map(Some)
                    .map_err(|fault| TemplateError::Expression {
                        fault,
                        loc: node.loc,
                    })
            }
            Value::Fault(Fault::MissingInput(index)) => {
                missing.push(index);
                Ok(None)
            }
            Value::Fault(fault) => Err(TemplateError::Expression {
                fault,
                loc: node.loc,
            }),
            _ => Err(TemplateError::Expression {
                fault: Fault::InvalidProgram,
                loc: node.loc,
            }),
        }
    }

    fn push_occurrence_flow(
        &self,
        mut flow: Flow,
        source_day: Day,
        txn: RuntimeTxn,
        ordinal: u32,
        detail_override: Option<axiom_model::Detail>,
        details: &mut Arena<RuntimeDetail>,
        out: &mut Vec<RuntimeFlow>,
    ) {
        let book = self.plan.book;
        let stored = *book.flow_view(&flow).detail();
        let original = detail_override.unwrap_or(stored);
        let shifted = if source_day == flow.day {
            original
        } else {
            original.moved(source_day.0 - flow.day.0)
        };
        flow.day = source_day;
        let detail = (shifted != stored).then(|| details.push(RuntimeDetail(shifted)));
        out.push(RuntimeFlow {
            flow,
            detail,
            ordinal,
            txn,
        });
    }

    fn apply_view(
        &mut self,
        flow: &Flow,
        view: FlowView<'_>,
        txn: axiom_model::RuntimeTxn,
        flow_ordinal: u32,
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
        self.post(&Motion::from_view_at(
            self.plan.book,
            view,
            txn,
            Cause::Applied(number),
            day,
            amounts,
            flow_ordinal,
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
            adjustments: &record.adjustments,
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
            mut world,
            record,
            ..
        } = self;
        world.assets.expire_carries_through(horizon);
        let (assets, pending_carries) = world.assets.into_run_parts();
        Run {
            today: options.today,
            horizon,
            posted: posted(plan, &record),
            holdings: world.holdings.into_sorted(),
            gains: record.gains,
            effects: record.effects,
            adjustments: record.adjustments,
            pending_carries,
            violations: record.violations,
            headroom,
            pads: record.pads,
            // These collections are populated by the native state monitors.
            assets,
            promises: Vec::new(),
            promised_flows: Box::default(),
            runtime_details: Arena::new(),
            missing_inputs: Box::default(),
            open_claims: Box::default(),
            monitor_complete: false,
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
            Fact::Flow(id) => self.post_journal(id, moment.day, false),
            // A settlement lands a pending flow; a return runs an actual one backwards.
            Fact::Settle(id) => {
                let returned = matches!(
                    self.plan.events.state(id, &self.plan.book.flows[id]),
                    State::Returned(_)
                );
                self.post_journal(id, moment.day, returned);
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
    fn post_journal(&mut self, id: Id<Flow>, day: Day, reversed: bool) {
        let book = self.plan.book;
        let source = &book.flows[id];
        let txn_id = source.txn;
        let transaction = &book.txns[txn_id];
        let local = id.index().checked_sub(transaction.flows.start().index());
        let offset = local.and_then(|local| u32::try_from(local).ok());
        let journal = transaction
            .program
            .and_then(|program_id| book.journal_programs.get(program_id));
        let roots = journal.and_then(|journal| {
            let offset = offset?;
            // Lowering appends these sparse roots in source-flow order.
            let at = journal
                .flow_roots
                .partition_point(|roots| roots.flow < offset);
            journal
                .flow_roots
                .get(at)
                .filter(|roots| roots.flow == offset)
                .copied()
        });
        let roots = roots
            .filter(|roots| roots.out.is_some() || roots.arrive.is_some() || roots.basis.is_some());
        let group =
            journal.and_then(|journal| journal.groups.iter().find(|group| group.header == offset));
        let item = journal.and_then(|journal| {
            let offset = offset?;
            journal.groups.iter().find_map(|group| {
                group
                    .items
                    .iter()
                    .find(|item| item.flow == Some(offset))
                    .map(|item| (group, item))
            })
        });
        let cost_item =
            item.filter(|(group, item)| is_exchange_cost(book, transaction.flows, group, item));
        let cost_header = group.filter(|group| {
            group
                .items
                .iter()
                .any(|item| is_exchange_cost(book, transaction.flows, group, item))
        });
        let computed_cost_item =
            cost_item.is_some_and(|(_, item)| matches!(item.amount, TemplateAmount::Computed(_)));
        if roots.is_none() && cost_header.is_none() && !computed_cost_item {
            let motion = self.journal_motion(id, day);
            self.post(&if reversed { motion.reversed() } else { motion });
            return;
        }
        let roots = roots.unwrap_or(FlowExpressions {
            flow: offset.unwrap_or_default(),
            out: None,
            arrive: None,
            basis: None,
        });
        let program_id = transaction
            .program
            .expect("flow roots belong to a journal program");
        let program = &book.journal_programs[program_id].program;
        let mut flow = source.clone();
        flow.day = day;
        let mut detail = *book.flow_view(source).detail();
        let quantity_roots = roots.out.is_some() || roots.arrive.is_some();
        let cached_amounts = quantity_roots
            .then(|| self.record.resolved.get(&id).copied())
            .flatten();
        if let Some(amounts) = cached_amounts {
            flow.out.qty = amounts.out;
            flow.arrive.qty = amounts.arrive;
        }
        let mut computed_quantity = cached_amounts.is_some();

        if cached_amounts.is_none() {
            for (root, target) in [(roots.out, End::From), (roots.arrive, End::To)] {
                let Some(root) = root else { continue };
                match journal_expression(
                    self.plan,
                    &self.world,
                    &mut self.scratch.values,
                    id,
                    txn_id,
                    &flow,
                    program,
                    root,
                ) {
                    Value::Amount(amount) => {
                        computed_quantity = true;
                        match target {
                            End::From => flow.out = amount,
                            End::To => flow.arrive = amount,
                        }
                    }
                    Value::Fault(fault) => {
                        self.record.report(explain::journal_expression_fault(
                            book, &flow, program, root, fault, day,
                        ));
                        return;
                    }
                    _ => {
                        self.record.report(explain::journal_expression_fault(
                            book,
                            &flow,
                            program,
                            root,
                            Fault::InvalidProgram,
                            day,
                        ));
                        return;
                    }
                }
            }
            // A single written quantity supplies both ends of an ordinary
            // transfer. The model stores its root on the written side only,
            // while the literal lowering has already mirrored the placeholder.
            if roots.out.is_some() && roots.arrive.is_none() && !flow.is_exchange() {
                flow.arrive = flow.out;
            } else if roots.arrive.is_some() && roots.out.is_none() && !flow.is_exchange() {
                flow.out = flow.arrive;
            }
        }
        if cached_amounts.is_none() {
            if let (Some(_), Infer::Target { end, .. }) = (roots.out.or(roots.arrive), flow.infer) {
                let amount = if end == End::From {
                    flow.out
                } else {
                    flow.arrive
                };
                flow.infer = Infer::Target {
                    end,
                    balance: amount.qty,
                };
            }
        }
        let mut computed_basis = None;
        if let Some(root) = roots.basis {
            if let Some(basis) = self.record.computed_basis.get(&id).copied() {
                detail.basis = Some(basis);
            } else {
                match journal_expression(
                    self.plan,
                    &self.world,
                    &mut self.scratch.values,
                    id,
                    txn_id,
                    &flow,
                    program,
                    root,
                ) {
                    Value::Amount(amount) if amount.unit == book.base => {
                        detail.basis = Some(amount.qty);
                        computed_basis = Some(amount.qty);
                    }
                    Value::Amount(amount) => {
                        self.record.report(explain::journal_expression_fault(
                            book,
                            &flow,
                            program,
                            root,
                            Fault::UnitMismatch {
                                found: amount.unit,
                                expected: book.base,
                            },
                            day,
                        ));
                        return;
                    }
                    Value::Fault(fault) => {
                        self.record.report(explain::journal_expression_fault(
                            book, &flow, program, root, fault, day,
                        ));
                        return;
                    }
                    _ => {
                        self.record.report(explain::journal_expression_fault(
                            book,
                            &flow,
                            program,
                            root,
                            Fault::InvalidProgram,
                            day,
                        ));
                        return;
                    }
                }
            }
        }

        // A purpose-bearing `Less` item on an exchange is also the exchange's
        // cost evidence. The item remains an ordinary posted flow for cash and
        // purpose totals; the header carries its aggregate cost so a sale's
        // realized proceeds shrink and a purchase's parcel basis grows.
        if let Some(group) = cost_header {
            let mut total = match detail.cost {
                Some(cost) => match book.convert(cost, book.base, day) {
                    Some(base) => base.qty.0,
                    None => {
                        self.record.report(
                            Diagnostic::error(
                                "exchange-cost-price",
                                format!("cannot value the exchange cost {}", book.show(cost)),
                            )
                            .label(source.loc, "needed to value this exchange cost"),
                        );
                        return;
                    }
                },
                None => 0,
            };
            let mut has_cost = detail.cost.is_some();
            for item in group
                .items
                .iter()
                .filter(|item| is_exchange_cost(book, transaction.flows, group, item))
            {
                let Some(item_local) = item.flow else {
                    continue;
                };
                let Some(start) = transaction
                    .flows
                    .start()
                    .index()
                    .checked_add(item_local as usize)
                else {
                    continue;
                };
                let Ok(raw) = u32::try_from(start) else {
                    continue;
                };
                let item_id = Id::new(raw);
                let item_flow = &book.flows[item_id];
                let amount = if let Some(cached) = self.record.resolved.get(&item_id).copied() {
                    Amount::new(cached.out, item_flow.out.unit)
                } else {
                    let amount = match item.amount {
                        TemplateAmount::Literal(amount) => amount,
                        TemplateAmount::Computed(root) => match journal_expression(
                            self.plan,
                            &self.world,
                            &mut self.scratch.values,
                            item_id,
                            txn_id,
                            item_flow,
                            program,
                            root,
                        ) {
                            Value::Amount(amount) => amount,
                            Value::Fault(fault) => {
                                self.record.report(explain::journal_expression_fault(
                                    book, item_flow, program, root, fault, day,
                                ));
                                return;
                            }
                            _ => {
                                self.record.report(explain::journal_expression_fault(
                                    book,
                                    item_flow,
                                    program,
                                    root,
                                    Fault::InvalidProgram,
                                    day,
                                ));
                                return;
                            }
                        },
                    };
                    self.record.resolved.insert(
                        item_id,
                        Amounts {
                            out: amount.qty,
                            arrive: amount.qty,
                        },
                    );
                    amount
                };
                let Some(cost) = book.convert(amount, book.base, day) else {
                    self.record.report(
                        Diagnostic::error(
                            "exchange-cost-price",
                            format!("cannot value the exchange cost {}", book.show(amount)),
                        )
                        .label(item.loc, "needed to value this exchange cost"),
                    );
                    return;
                };
                let Some(sum) = total.checked_add(cost.qty.0) else {
                    self.record.report(
                        Diagnostic::error(
                            "exchange-cost-overflow",
                            "exchange costs exceed the supported amount range",
                        )
                        .label(item.loc, "these costs do not fit in one amount"),
                    );
                    return;
                };
                total = sum;
                has_cost = true;
            }
            if has_cost {
                detail.cost = Some(Amount::new(Qty(total), book.base));
            }
        }

        let amounts = self.amounts(&flow, Some(id));
        if computed_quantity {
            // `posted` and a later return use the exact amount computed on its
            // first landing, just as they do for `all` and `=`.
            self.record.resolved.insert(id, amounts);
        }
        if let Some(basis) = computed_basis {
            self.record.computed_basis.insert(id, basis);
        }
        let txn =
            RuntimeTxn::journal(txn_id).expect("a journal flow cannot name the template sentinel");
        // A computed basis is a call-local override. Borrow it directly for
        // this motion instead of allocating a one-entry RuntimeDetail arena.
        let view = book.flow_view_with_detail(&flow, &detail);
        let flow_ordinal = offset.unwrap_or_default();
        let motion =
            Motion::from_view_at(book, view, txn, Cause::Flow(id), day, amounts, flow_ordinal);
        self.post(&if reversed { motion.reversed() } else { motion });
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

fn has_computed_quantity(quantity: TemplateQuantity) -> bool {
    matches!(
        quantity,
        TemplateQuantity::Amount(Some(_))
            | TemplateQuantity::Pending(Some(_))
            | TemplateQuantity::Target(Some(_))
    )
}

fn journal_quantity_root(quantity: JournalQuantity) -> Option<axiom_model::NodeId> {
    match quantity {
        JournalQuantity::Amount(_, root)
        | JournalQuantity::Pending(_, root)
        | JournalQuantity::Target(_, root) => root,
        JournalQuantity::Unknown(_)
        | JournalQuantity::All(_)
        | JournalQuantity::Rest
        | JournalQuantity::Whole
        | JournalQuantity::Derived => None,
    }
}

fn same_flow_ends(left: &Flow, right: &Flow) -> bool {
    left.from == right.from && left.to == right.to
}

fn written_leg_for_template<'a>(
    written: Option<&axiom_model::WrittenGroup>,
    source_flows: &'a [Flow],
    template: &TemplateLeg,
) -> Option<(usize, &'a Flow, JournalQuantity)> {
    let written = written?;
    if written.group.legs.len() != written.group.leg_quantities.len() {
        return None;
    }
    written
        .group
        .legs
        .iter()
        .zip(written.group.leg_quantities.iter())
        .enumerate()
        .find_map(|(index, (&offset, &quantity))| {
            source_flows
                .get(offset as usize)
                .filter(|flow| same_flow_ends(flow, &template.flow))
                .map(|flow| (index, flow, quantity))
        })
}

fn scale_template_amount(amount: Amount, ratio: Ratio) -> Result<Amount, Fault> {
    amount
        .qty
        .scale(ratio)
        .map(|qty| Amount::new(qty, amount.unit))
        .ok_or(Fault::Overflow)
}

fn set_quantity(flow: &mut Flow, end: End, quantity: ResolvedQuantity) {
    match end {
        End::From => flow.out = quantity.amount,
        End::To => flow.arrive = quantity.amount,
    }
    if quantity.infer != Infer::Known {
        flow.infer = quantity.infer;
    } else if flow.infer == Infer::Unknown || flow.infer == Infer::All {
        flow.infer = Infer::Known;
    }
    if quantity.mode == Mode::Pending || quantity.mode == Mode::Opening {
        flow.mode = quantity.mode;
    }
}

fn subtract_parent(flow: &mut Flow, side: FlowSide, amount: Amount) -> Result<(), Fault> {
    let (parent, opposite) = match side {
        FlowSide::Out => (&mut flow.out, &mut flow.arrive),
        FlowSide::Arrive => (&mut flow.arrive, &mut flow.out),
    };
    if parent.unit != amount.unit {
        return Err(Fault::UnitMismatch {
            found: amount.unit,
            expected: parent.unit,
        });
    }
    parent.qty = parent
        .qty
        .0
        .checked_sub(amount.qty.0)
        .map(Qty)
        .ok_or(Fault::Overflow)?;
    // An ordinary transfer has one magnitude on both sides. Splitting an
    // arrival leg must reduce the corresponding outflow as well; an exchange
    // keeps its distinct opposite-side amount.
    if parent.unit == opposite.unit {
        opposite.qty = opposite
            .qty
            .0
            .checked_sub(amount.qty.0)
            .map(Qty)
            .ok_or(Fault::Overflow)?;
    }
    Ok(())
}

fn apply_occurrence_amount(
    book: &Book<'_>,
    flow: &mut Flow,
    buys: Option<Id<Commodity>>,
    day: Day,
    amount: Amount,
) -> Result<(), Fault> {
    if let Some(unit) = buys {
        if amount.unit == unit {
            let spend = book
                .convert(amount, flow.out.unit, day)
                .ok_or(Fault::NoPrice {
                    unit,
                    quote: flow.out.unit,
                })?;
            flow.out = spend;
            flow.arrive = amount;
            flow.infer = Infer::Known;
            return Ok(());
        }
    }

    if amount.unit == flow.out.unit {
        flow.out = amount;
        if flow.arrive.unit == amount.unit {
            flow.arrive = amount;
        }
        // A buy occurrence stated in its spend unit still has to derive the
        // acquired quantity using the contract's active price.
        flow.infer = if buys.is_some() && flow.arrive.unit != amount.unit {
            Infer::Unknown
        } else {
            Infer::Known
        };
        return Ok(());
    }
    if amount.unit == flow.arrive.unit {
        flow.arrive = amount;
        if flow.out.unit == amount.unit {
            flow.out = amount;
        }
        flow.infer = Infer::Known;
        return Ok(());
    }
    Err(Fault::UnitMismatch {
        found: amount.unit,
        expected: flow.out.unit,
    })
}

fn merge_detail(base: axiom_model::Detail, over: axiom_model::Detail) -> axiom_model::Detail {
    axiom_model::Detail {
        basis: over.basis.or(base.basis),
        hold: over.hold.or(base.hold),
        since: over.since.or(base.since),
        spender: over.spender.or(base.spender),
        cost: over.cost.or(base.cost),
        due: over.due.or(base.due),
        against: over.against.or(base.against),
        reckoned: over.reckoned.or(base.reckoned),
    }
}

fn bought_quantity(
    book: &Book,
    buys: Option<Id<Commodity>>,
    day: Day,
    out: Amount,
    arrive: Amount,
    infer: Infer,
) -> Result<Option<(Amount, Amount)>, TemplateError> {
    let Some(unit) = buys else { return Ok(None) };
    if infer == Infer::Known {
        return Ok(None);
    }
    let converted = if arrive.unit == unit && out.unit != unit {
        book.convert(out, unit, day).map(|amount| (out, amount))
    } else if out.unit == unit && arrive.unit != unit {
        book.convert(arrive, unit, day)
            .map(|amount| (amount, arrive))
    } else {
        None
    };
    converted.map(Some).ok_or_else(|| {
        let missing = if out.unit != unit && arrive.unit == unit {
            out.unit
        } else {
            arrive.unit
        };
        TemplateError::Expression {
            fault: Fault::NoPrice {
                unit: missing,
                quote: unit,
            },
            loc: axiom_core::Loc::default(),
        }
    })
}

fn loan_payment(contract: &Contract, terms: &Terms) -> Option<Amount> {
    let loan = contract.loan?;
    let annual = terms.rate.unwrap_or(Ratio::ZERO);
    let (periods, period_rate) = match terms.every {
        Cadence::Every(Span { months, days: 0 }) if months > 0 => {
            let periods = loan
                .term
                .months
                .checked_add(months - 1)?
                .checked_div(months)?;
            let rate = annual.checked_mul(Ratio::new(months as i128, 12)?)?;
            (periods, rate)
        }
        Cadence::Every(Span { months: 0, days }) if days > 0 => {
            let periods = loan.term.days.checked_add(days - 1)?.checked_div(days)?;
            let rate = annual.checked_mul(Ratio::new(days as i128, 365)?)?;
            (periods, rate)
        }
        Cadence::TwiceMonthly => {
            let periods = loan.term.months.checked_mul(2)?;
            let rate = annual.checked_div(Ratio::int(24))?;
            (periods, rate)
        }
        _ => return None,
    };
    if periods <= 0 || period_rate.is_negative() {
        return None;
    }
    let factor = if period_rate.is_zero() {
        Ratio::new(1, periods as i128)?
    } else {
        // Keep the compound factor at 18 decimal places. Repeated exact Ratio
        // multiplication grows its numerator and denominator exponentially;
        // fixed-point intermediates stay bounded while retaining far more
        // precision than a currency quantum.
        const SCALE: i128 = 1_000_000_000_000_000_000;
        let rate = checked_mul_div(period_rate.num() as i128, SCALE, period_rate.den() as i128)?;
        let mut growth = SCALE;
        for _ in 0..periods {
            growth = checked_mul_div(growth, SCALE.checked_add(rate)?, SCALE)?;
        }
        let factor = checked_mul_div(rate, growth, growth.checked_sub(SCALE)?)?;
        Ratio::new(factor, SCALE)?
    };
    Some(Amount::new(
        loan.principal.qty.scale(factor)?,
        loan.principal.unit,
    ))
}

fn checked_mul_div(left: i128, right: i128, denominator: i128) -> Option<i128> {
    let numerator = left.checked_mul(right)?;
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let twice = remainder.unsigned_abs().checked_mul(2)?;
    let divisor = denominator.unsigned_abs();
    let away = twice > divisor || (twice == divisor && quotient & 1 != 0);
    Some(if away {
        quotient.checked_add(numerator.signum() * denominator.signum())?
    } else {
        quotient
    })
}

#[cfg(test)]
mod occurrence_tests {
    use axiom_core::{Arena, Day, FileId, Qty};
    use axiom_model::{Mode, RuntimeDetail, ScheduleKind, Source};
    use axiom_syntax::Folder;

    use crate::{Options, Plan};

    fn source_book(source: &'static str) -> axiom_model::Book<'static> {
        let (file, syntax) = axiom_syntax::parse(FileId(0), source, Folder::default());
        assert!(syntax.is_empty(), "source should parse: {syntax:?}");
        let (book, diagnostics) = axiom_model::build(&[Source {
            path: "occurrence.ax",
            file,
            embedded: false,
        }]);
        assert!(
            diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
            "source should build: {diagnostics:?}"
        );
        book
    }

    #[test]
    fn materializes_a_native_contract_occurrence_into_borrowed_runtime_pools() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
entity landlord
contract rent with landlord
  100 USD monthly on 1 from checking
  from 2026-01-01
";
        let book = source_book(source);
        let rent = book.contract("rent").expect("contract id");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                rent,
                ScheduleKind::Regular,
                due,
                0,
                None,
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("active occurrence materializes");
        let made_flows = made
            .flows(&flows)
            .expect("output range belongs to the pool");
        assert_eq!(made_flows.len(), 1);
        assert_eq!(made_flows[0].flow.day, due);
        assert_eq!(made_flows[0].flow.out.qty, Qty(10_000));
        assert_eq!(made_flows[0].flow.arrive.qty, Qty(10_000));
        assert_eq!(made_flows[0].flow.mode, Mode::Planned);
        assert_eq!(made_flows[0].ordinal, 0);
        assert!(made.missing(&missing).unwrap().is_empty());
        let view = book.runtime_flow_view(&made_flows[0], &details);
        assert_eq!(view.from, book.place("checking").unwrap());
        assert_eq!(
            view.to,
            book.entities[book.contracts[rent].party].place.unwrap()
        );
    }

    #[test]
    fn percentage_split_leg_uses_the_materialized_header_amount() {
        let source = "\
base USD
commodity USD
  precision 2
entity lumen
account assets/checking
account assets/retirement
contract job with lumen
  4_600 USD twice monthly on 15, last into checking
  retirement 6%
  from 2026-01-01
";
        let book = source_book(source);
        let contract = book.contract("job").expect("contract id");
        let due = Day::from_ymd(2026, 1, 15).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                contract,
                ScheduleKind::Regular,
                due,
                0,
                None,
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("percentage leg materializes against its header");
        let rows = made.flows(&flows).expect("range belongs to the pool");
        let checking = book.place("checking").unwrap();
        let retirement = book.place("retirement").unwrap();
        let header = rows
            .iter()
            .find(|flow| flow.flow.to == checking)
            .expect("header pays checking");
        let deferral = rows
            .iter()
            .find(|flow| flow.flow.to == retirement)
            .expect("percentage leg pays retirement");
        assert_eq!(
            (header.flow.out.qty, header.flow.arrive.qty),
            (Qty(432_400), Qty(432_400))
        );
        assert_eq!(
            (deferral.flow.out.qty, deferral.flow.arrive.qty),
            (Qty(27_600), Qty(27_600))
        );
        assert_eq!(
            header.flow.arrive.qty + deferral.flow.arrive.qty,
            Qty(460_000)
        );
        assert!(made.missing(&missing).unwrap().is_empty());
    }

    #[test]
    fn written_amount_and_header_tail_override_only_the_kept_occurrence() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
entity landlord
contract rent with landlord
  100 USD monthly on 1 from checking
  from 2026-01-01
  rising 10% yearly
2027-01-01 rent 50% of 110 USD ^paid \"January rent\"
";
        let book = source_book(source);
        let rent = book.contract("rent").expect("contract id");
        let due = Day::from_ymd(2027, 1, 1).unwrap();
        let written = book
            .txns
            .iter()
            .find_map(|(id, txn)| (txn.contract == Some(rent)).then_some(id))
            .expect("written occurrence transaction");
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                rent,
                ScheduleKind::Regular,
                due,
                0,
                Some(written),
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("computed written amount materializes");
        let materialized = made.flows(&flows).expect("output belongs to shared pool");
        assert_eq!(materialized.len(), 1);
        assert_eq!(materialized[0].flow.out.qty, Qty(5_500));
        assert_eq!(materialized[0].flow.arrive.qty, Qty(5_500));
        assert_eq!(materialized[0].flow.mode, Mode::Actual);
        assert_eq!(materialized[0].txn.source_txn(), Some(written));
        let view = book.runtime_flow_view(&materialized[0], &details);
        assert_eq!(
            view.codes().map(|code| book.name(code)).collect::<Vec<_>>(),
            ["paid"]
        );
        assert_eq!(
            book.text(materialized[0].flow.description.expect("tail description")),
            "January rent"
        );
        assert!(made.missing(&missing).unwrap().is_empty());

        let mut forecast = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut forecast_flows, mut forecast_details, mut forecast_missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let forecasted = forecast
            .instantiate_occurrence(
                rent,
                ScheduleKind::Regular,
                due,
                1,
                None,
                &mut forecast_flows,
                &mut forecast_details,
                &mut forecast_missing,
            )
            .expect("forecast uses the escalated contract amount");
        assert_eq!(
            forecasted.flows(&forecast_flows).unwrap()[0].flow.out.qty,
            Qty(11_000)
        );
    }

    #[test]
    fn written_occurrence_replaces_matching_leg_and_adds_an_unmatched_leg() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
account savings
account bonus
entity landlord
contract rent with landlord
  100 USD monthly on 1 from checking
  savings 30 USD
  from 2026-01-01
2026-02-01 rent
  savings 40 USD
  bonus 10 USD
";
        let book = source_book(source);
        let contract = book.contract("rent").expect("contract id");
        let written = book
            .txns
            .iter()
            .find_map(|(id, txn)| (txn.contract == Some(contract)).then_some(id))
            .expect("the kept occurrence transaction");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                contract,
                ScheduleKind::Regular,
                due,
                0,
                Some(written),
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("written split overlays the active terms");
        let rows = made.flows(&flows).expect("range belongs to the pool");
        assert_eq!(rows.len(), 3);
        let checking = book.place("checking").unwrap();
        let savings = book.place("savings").unwrap();
        let bonus = book.place("bonus").unwrap();
        let landlord = book.entities[book.contracts[contract].party].place.unwrap();
        let remainder = rows.iter().find(|row| row.flow.to == landlord).unwrap();
        assert_eq!(remainder.flow.arrive.qty, Qty(5_000));
        assert_eq!(
            rows.iter()
                .find(|row| row.flow.to == savings)
                .unwrap()
                .flow
                .arrive
                .qty,
            Qty(4_000)
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.flow.to == bonus)
                .unwrap()
                .flow
                .arrive
                .qty,
            Qty(1_000)
        );
        assert!(rows.iter().all(|row| row.flow.from == checking));
        assert_eq!(
            rows.iter().map(|row| row.ordinal).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            rows.iter().map(|row| row.flow.arrive.qty.0).sum::<i64>(),
            10_000
        );
        assert!(made.missing(&missing).unwrap().is_empty());
    }

    #[test]
    fn occurrence_inputs_drive_computed_items_and_missing_inputs_omit_only_that_item() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
entity landlord
purpose utilities : spending
contract flat with landlord
  100 USD monthly on 1 from checking
  input water USD
  + 12% of water #utilities
  from 2026-01-01
2026-02-01 flat
  water = 155 USD
";
        let book = source_book(source);
        let flat = book.contract("flat").expect("contract id");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let written = book
            .txns
            .iter()
            .find_map(|(id, txn)| (txn.contract == Some(flat) && txn.day == due).then_some(id))
            .expect("the source occurrence supplies water");
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                flat,
                ScheduleKind::Regular,
                due,
                0,
                Some(written),
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("the kept occurrence materializes");
        let made_flows = made.flows(&flows).unwrap();
        assert_eq!(
            made_flows.len(),
            2,
            "base rent plus the computed utilities item"
        );
        assert_eq!(made_flows[0].flow.out.qty, Qty(10_000));
        assert_eq!(made_flows[1].flow.out.qty, Qty(1_860));
        assert_eq!(made_flows[0].flow.mode, Mode::Actual);
        assert_eq!(made_flows[1].ordinal, 1);
        assert!(made.missing(&missing).unwrap().is_empty());

        let mut forecast = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut forecast_flows, mut forecast_details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let forecasted = forecast
            .instantiate_occurrence(
                flat,
                ScheduleKind::Regular,
                due,
                1,
                None,
                &mut forecast_flows,
                &mut forecast_details,
                &mut missing,
            )
            .expect("unbound optional input omits only its computed item");
        assert_eq!(forecasted.flows(&forecast_flows).unwrap().len(), 1);
        assert_eq!(forecasted.missing(&missing).unwrap(), [0]);
        assert_eq!(forecast_flows[0].flow.mode, Mode::Planned);

        let repeated = forecast
            .instantiate_occurrence(
                flat,
                ScheduleKind::Regular,
                due,
                2,
                None,
                &mut forecast_flows,
                &mut forecast_details,
                &mut missing,
            )
            .expect("the same shared pools accept another occurrence");
        assert_eq!(repeated.flows(&forecast_flows).unwrap().len(), 1);
        assert_eq!(repeated.missing(&missing).unwrap(), [0]);
    }

    #[test]
    fn escalation_scales_literal_header_and_item_amounts() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
entity landlord
purpose fees : spending
contract rent with landlord
  100 USD monthly on 1 from checking
  + 10 USD #fees
  from 2026-01-01
  rising 10% yearly
";
        let book = source_book(source);
        let rent = book.contract("rent").expect("contract id");
        let due = Day::from_ymd(2027, 1, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                rent,
                ScheduleKind::Regular,
                due,
                0,
                None,
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("anniversary terms materialize");
        let flows = made.flows(&flows).unwrap();
        assert_eq!(flows.len(), 2);
        assert_eq!(flows[0].flow.out.qty, Qty(11_000));
        assert_eq!(flows[0].ordinal, 0);
        assert_eq!(flows[1].flow.out.qty, Qty(1_100));
        assert_eq!(flows[1].ordinal, 1);
    }

    #[test]
    fn rest_leg_uses_the_amount_left_by_later_written_legs() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
account savings
entity landlord
contract split with landlord
  100 USD monthly on 1 from checking
  landlord ...
  savings 30 USD
  from 2026-01-01
";
        let book = source_book(source);
        let split = book.contract("split").expect("contract id");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options {
            today: due,
            relaxed: false,
        });
        let (mut flows, mut details, mut missing) =
            (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(
                split,
                ScheduleKind::Regular,
                due,
                0,
                None,
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("the remainder can precede its explicit carve");
        let flows = made.flows(&flows).unwrap();
        assert_eq!(flows.len(), 3);
        assert_eq!(flows[0].flow.out.qty, Qty::ZERO);
        assert_eq!(flows[0].flow.arrive.qty, Qty::ZERO);
        assert_eq!(flows[1].flow.arrive.qty, Qty(7_000));
        assert_eq!(flows[2].flow.arrive.qty, Qty(3_000));
        assert_eq!(flows[0].ordinal, 0);
        assert_eq!(flows[1].ordinal, 1);
        assert_eq!(flows[2].ordinal, 2);
        assert_eq!(
            flows.iter().map(|flow| flow.flow.out.qty.0).sum::<i64>(),
            10_000
        );
    }
}

/// Whether this line item is a `Less` cost attached to an exchange header.
/// The group retains the relationship; no endpoint guessing or transaction
/// range scan is needed when the flow is posted.
fn is_exchange_cost(
    book: &Book,
    flows: axiom_core::Run<Flow>,
    group: &JournalGroup,
    item: &JournalItem,
) -> bool {
    if item.sign != Sign::Less || item.parent != TemplateItemParent::Header {
        return false;
    }
    let (Some(header), Some(item)) = (group.header, item.flow) else {
        return false;
    };
    let Some(header_index) = flows.start().index().checked_add(header as usize) else {
        return false;
    };
    let Some(item_index) = flows.start().index().checked_add(item as usize) else {
        return false;
    };
    let (Ok(header_raw), Ok(item_raw)) = (u32::try_from(header_index), u32::try_from(item_index))
    else {
        return false;
    };
    let (header_id, item_id) = (Id::new(header_raw), Id::new(item_raw));
    let (Some(header), Some(item)) = (book.flows.get(header_id), book.flows.get(item_id)) else {
        return false;
    };
    header.is_exchange()
        && item
            .purpose
            .is_some_and(|purpose| book.purposes[purpose.purpose].root == PurposeRoot::Spending)
}

/// Evaluate one transaction-scoped expression with the current source flow as
/// its `self`, `amount`, `from`, and `to`. This borrows the Book's program and
/// code/detail pools; only the caller-owned node-value buffer is mutable.
fn journal_expression<'b, 's>(
    plan: &Plan<'b, 's>,
    world: &World,
    values: &mut Vec<axiom_model::Value>,
    flow_id: Id<Flow>,
    txn_id: Id<axiom_model::Txn>,
    flow: &Flow,
    program: &TemplateProgram,
    root: axiom_model::NodeId,
) -> Value {
    let book = plan.book;
    let txn = RuntimeTxn::journal(txn_id)
        .expect("journal expression cannot use the template transaction sentinel");
    let view = book.flow_view(flow);
    let flow_ordinal = plan
        .book
        .txns
        .get(txn_id)
        .and_then(|txn| flow_id.index().checked_sub(txn.flows.start().index()))
        .and_then(|at| u32::try_from(at).ok())
        .unwrap_or(0);
    let motion = Motion::from_view_at(
        book,
        view,
        txn,
        Cause::Flow(flow_id),
        flow.day,
        Amounts::written(flow),
        flow_ordinal,
    );
    let mut occasion = crate::eval::Occasion::flow(&motion);
    occasion.amount = Some(if flow.out.qty == Qty::ZERO {
        flow.arrive
    } else {
        flow.out
    });
    let context = crate::eval::Context::new(Subject::Place(flow.from), flow.owner, &occasion)
        .for_flow()
        .with_inputs(book.txn_inputs(txn_id));
    crate::eval::program_expression(
        crate::eval::Env { plan, world },
        program,
        root,
        &context,
        values,
    )
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
        Infer::Known => Some(
            record
                .resolved
                .get(&id)
                .copied()
                .unwrap_or_else(|| Amounts::written(flow)),
        ),
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
