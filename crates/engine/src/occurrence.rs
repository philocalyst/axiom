//! A contract's occurrence made into flows.
//!
//! A promise says what each occurrence moves: a header, the legs that take from it and the items under it, as
//! amounts of the day's escalation. A kept occurrence says what it changes of that. Both are made here, by the
//! same path, so what the forecast expects and what the fold posts cannot disagree.
//!
//! This module reads the occurrence and writes flows. What each leg and item *comes to* is
//! [`axiom_model::solve`]'s: here the occurrence is turned into the group it is (the template's legs, each replaced
//! by the written leg that goes to the same end, then the written legs that replace none), the solver is given an
//! [`Env`] that evaluates an expression against the right flow, and its answer is put back into flows.
//! An expression is read against a flow, so the environment keeps the flows by line; it owns no state of its own
//! but what the fold lends it.

use std::borrow::Cow;

use axiom_core::{Arena, Day, Days, Id, Loc, Ratio};
use axiom_model::promise::Sched;
use axiom_model::{
    Amount, Answer, Bear, Book, Commodity, Contract, Detail, Draw, Drawn, End, Env, Expr, Failed, Fault, Flow,
    FlowSide, Infer, Item, Line, Made, Mode, OccurrenceTail, Origin, Part, Program, Promised, Remainder, Remaining,
    Resolved, RuntimeDetail, RuntimeFlow, RuntimeTxn, Says, ScheduleKind, Sign, Terms, Value, WrittenOccurrence, solve,
};

use crate::evaluate::{Binds, Evaluating, Lent};
use crate::ledger::Ledger;
use crate::{Cause, OmittedInputs, PromisedFlows};

/// Why one native contract occurrence could not be materialized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateError {
    /// The requested schedule has no active terms on its due day.
    OutsideTerms { contract: Id<Contract>, day: Day },
    /// A typed expression failed while this occurrence was evaluated.
    Expression { fault: Fault, loc: Loc },
    /// Contract escalation or recognition could not be resolved.
    Forecast(axiom_model::ForecastError),
    /// A grouped template refers to an invalid parent or unsupported derived quantity.
    InvalidTemplate { loc: Loc },
}

/// The shared-pool ranges appended by one materialization call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OccurrenceOutput {
    pub flows: PromisedFlows,
    pub missing_inputs: OmittedInputs,
}

impl OccurrenceOutput {
    pub fn flows<'a>(self, pool: &'a [RuntimeFlow]) -> Option<&'a [RuntimeFlow]> {
        self.flows.get(pool)
    }

    pub fn missing<'a>(self, pool: &'a [u16]) -> Option<&'a [u16]> {
        self.missing_inputs.get(pool)
    }
}

/// What an occurrence's own written amount does to its first group.
#[derive(Clone, Copy)]
enum OccurrenceAmount {
    Inherit,
    Omitted,
    Value(Amount),
}

/// What the quantities of one occurrence are read against: the promise, the day it is due
/// and who it is, the inputs it binds, and the program its expressions are nodes of, with the scale its terms'
/// escalation puts on what they compute. A promise's quantities are read against the terms' own program and the
/// day's escalation; a written occurrence's against its own program, and unscaled: it said what it said.
#[derive(Clone, Copy)]
struct Reading<'a> {
    contract: Id<Contract>,
    program: &'a Program,
    scale: Ratio,
    due: Day,
    txn: RuntimeTxn,
    inputs: &'a [Option<Amount>],
}

/// What the occurrence itself says of every flow it makes: whose it is, how real, when it is recognized, and the
/// codes and waiver its tail gives them.
#[derive(Clone, Copy)]
struct Stamp<'a> {
    contract: Id<Contract>,
    recognized: Days,
    mode: Mode,
    tail: Option<&'a OccurrenceTail>,
}

impl Stamp<'_> {
    fn on(self, flow: &mut Flow) {
        flow.recognized = self.recognized;
        flow.origin = Origin::Occurrence(self.contract);
        flow.mode = self.mode;
        if let Some(tail) = self.tail {
            if !tail.codes.is_empty() {
                flow.header_codes = tail.codes;
            }
            if let Some(waive) = tail.waive {
                flow.waive = Some(waive);
            }
        }
    }
}

/// What every group of one occurrence is made against.
struct Making<'a> {
    contract: Id<Contract>,
    /// The schedule the occurrence falls due on.
    sched: Sched<'a>,
    due: Day,
    /// The day it was written, or the due day for one that was not.
    source_day: Day,
    txn: RuntimeTxn,
    terms: &'a Terms,
    ratio: Ratio,
    amount: OccurrenceAmount,
    tail: Option<&'a OccurrenceTail>,
    /// The program of what the occurrence wrote.
    program: Option<&'a Program>,
    inputs: &'a [Option<Amount>],
    /// The flows of the occurrence's own transaction, which its groups name by offset.
    source_flows: &'a [Flow],
}

/// One group of the template, and what the occurrence wrote over it.
struct GroupAt<'a> {
    template: &'a Promised,
    index: usize,
    /// The ordinal of the flow the group starts at.
    base: u32,
    written: Option<&'a Made>,
}

/// Where a group's flows, details and omitted inputs go.
struct Pools<'a> {
    flows: &'a mut Vec<RuntimeFlow>,
    details: &'a mut Arena<RuntimeDetail>,
    missing: &'a mut Vec<u16>,
}

/// What one group is made against: the occurrence, the group, and how its template's and its written parts read.
struct Cx<'a> {
    making: &'a Making<'a>,
    group: &'a GroupAt<'a>,
    stamp: Stamp<'a>,
    template_at: Reading<'a>,
    written_at: Reading<'a>,
}

/// One leg as the occurrence has it: its flow as the template or the written leg made it (and the mode that flow
/// had, which the leg keeps), what it takes, and from which side of the header.
struct LegAt {
    flow: Flow,
    mode: Mode,
    part: Part,
    side: FlowSide,
    written: bool,
    ordinal: u32,
}

/// What an item says of the flow it makes.
enum ItemKind<'a> {
    Template(&'a Item<Says>),
    Written { flow: Option<&'a Flow> },
}

struct ItemAt<'a> {
    kind: ItemKind<'a>,
    ordinal: u32,
    loc: Loc,
}

/// The header of a group once its quantities are read: the flow, and the detail its occurrence tail gives it.
struct Headed {
    flow: Flow,
    detail: Option<Detail>,
}

impl<'p, 'b, 's> Ledger<'p, 'b, 's> {
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
        let found = find(book, contract_id, schedule, due, source)?;
        let (flow_start, detail_start, missing_start) = (flows.len(), details.len(), missing_inputs.len());
        let program = found.written.and_then(|written| written.program).and_then(|id| book.journal_programs.get(id));
        let inputs = source.map_or(&[][..], |txn| book.txn_inputs(txn));
        let source_flows = source.and_then(|txn| book.txns.get(txn)).map_or(&[][..], |txn| &book.flows[txn.flows]);
        let mut making = Making {
            contract: contract_id,
            sched: found.sched,
            due,
            source_day: found.source_day,
            txn: RuntimeTxn::contract_occurrence(contract_id, schedule, due, ordinal, source),
            terms: found.terms,
            ratio: found.sched.factor(book, due).map_err(TemplateError::Forecast)?,
            amount: OccurrenceAmount::Inherit,
            tail: found.written.map(|written| &written.tail),
            program,
            inputs,
            source_flows,
        };
        making.amount = self.written_amount(&making, found.written, missing_inputs)?;
        let mut pools = Pools { flows, details, missing: missing_inputs };
        if let Err(error) = self.materialize(&making, found.written, &mut pools) {
            flows.truncate(flow_start);
            details.truncate(detail_start);
            missing_inputs.truncate(missing_start);
            return Err(error);
        }
        // A caller may reuse these pools across occurrences. Keep each output
        // range self-contained even when the same binding is missing twice.
        tidy(missing_inputs, missing_start);
        Ok(OccurrenceOutput {
            flows: PromisedFlows::of(flow_start..flows.len()),
            missing_inputs: OmittedInputs::of(missing_start..missing_inputs.len()),
        })
    }

    /// What the occurrence's written amount does to its first group, evaluated if it is computed.
    fn written_amount(
        &mut self,
        making: &Making<'_>,
        written: Option<&WrittenOccurrence>,
        missing: &mut Vec<u16>,
    ) -> Result<OccurrenceAmount, TemplateError> {
        let contract = &self.plan.book.contracts[making.contract];
        let root = match written.and_then(|written| written.amount) {
            None => return Ok(OccurrenceAmount::Inherit),
            Some(Expr::Literal(amount)) => return Ok(OccurrenceAmount::Value(amount)),
            Some(Expr::Computed(root)) => root,
        };
        let invalid = TemplateError::InvalidTemplate { loc: contract.loc };
        let (Some(program), Some(template)) = (making.program, making.terms.template.first()) else {
            return Err(invalid);
        };
        let reading = Reading { program, scale: Ratio::ONE, due: making.source_day, ..template_at(making) };
        let flow = Cow::Borrowed(&template.header.flow);
        Ok(match compute(&mut self.lent(), missing, &reading, flow, root, 0)? {
            Answer::Amount(amount) => OccurrenceAmount::Value(amount),
            _ => OccurrenceAmount::Omitted,
        })
    }

    /// Every group of the template, in order, each after the flows of the one before.
    fn materialize(
        &mut self,
        making: &Making<'_>,
        written: Option<&WrittenOccurrence>,
        pools: &mut Pools<'_>,
    ) -> Result<(), TemplateError> {
        let mut base = 0u32;
        for (index, template) in making.terms.template.iter().enumerate() {
            let written = written.and_then(|written| written.groups.get(index)?.as_ref());
            let loc = template.header.flow.loc;
            let (written_legs, written_items) = written.map_or((0, 0), |group| (group.legs.len(), group.items.len()));
            let width = [1, template.legs.len(), written_legs, template.items.len(), written_items]
                .into_iter()
                .try_fold(0usize, usize::checked_add)
                .and_then(|width| u32::try_from(width).ok())
                .ok_or(TemplateError::InvalidTemplate { loc })?;
            self.materialize_group(making, &GroupAt { template, index, base, written }, pools)?;
            base = base.checked_add(width).ok_or(TemplateError::InvalidTemplate { loc })?;
        }
        Ok(())
    }

    /// One group: its header, the legs that take from it and the items under it, as flows pushed in that order.
    fn materialize_group(
        &mut self,
        making: &Making<'_>,
        group: &GroupAt<'_>,
        pools: &mut Pools<'_>,
    ) -> Result<(), TemplateError> {
        let template = group.template;
        let recognized = making.sched.recognized(making.due).map_err(TemplateError::Forecast)?;
        let stamp = Stamp {
            contract: making.contract,
            recognized: making.tail.and_then(|tail| tail.recognized).unwrap_or(recognized),
            mode: if making.txn.source_txn().is_some() { Mode::Actual } else { Mode::Planned },
            tail: making.tail,
        };
        let none = Program::default();
        let cx = Cx { making, group, stamp, template_at: template_at(making), written_at: written_at(making, &none) };
        let mut header = template.header.flow.clone();
        stamp.on(&mut header);
        if let Some(tail) = making.tail.filter(|_| group.index == 0) {
            header.purpose = tail.purpose.or(header.purpose);
            header.description = tail.description.or(header.description);
            header.payee = tail.payee.or(header.payee);
        }
        let Some(Headed { flow: mut header, detail }) = self.header(&cx, header, pools.missing)? else {
            return Ok(());
        };
        let legs = legs_at(&cx)?;
        let (bears, items) = items_at(&cx)?;
        let draws: Vec<_> = legs
            .iter()
            .map(|leg| Draw { part: leg.part, side: leg.side, unit: leg.flow.amount_at(leg.side.end()).unit })
            .collect();
        let remaining = Remaining { out: header.out, arrive: header.arrive };
        let mut env =
            Reads { lent: self.lent(), missing: pools.missing, cx: &cx, header: &header, legs: &legs, items: &items };
        let solved = solve(Some(remaining), &draws, &bears, Remainder::BeforeItems, &mut env)
            .map_err(|failed| failure(failed, &header, &legs, &items))?;
        let left = solved.header.expect("a promise's header has an amount");
        (header.out, header.arrive) = (left.out, left.arrive);
        self.push_occurrence_flow(header.clone(), making, group.base, detail, pools)?;
        for (leg, drawn) in legs.into_iter().zip(solved.legs.iter()) {
            self.push_leg(&cx, leg, *drawn, pools)?;
        }
        for (item, amount) in items.iter().zip(solved.items.iter()) {
            if let Some(amount) = *amount {
                self.push_item(&cx, &header, item, amount, pools)?;
            }
        }
        Ok(())
    }

    /// The header with its quantities read, the occurrence's own amount applied, and the detail its tail gives
    /// it. A header side that reads an input that is not bound leaves the whole group out: None.
    fn header(
        &mut self,
        cx: &Cx<'_>,
        mut header: Flow,
        missing: &mut Vec<u16>,
    ) -> Result<Option<Headed>, TemplateError> {
        let (book, making, template) = (self.plan.book, cx.making, cx.group.template);
        let own_mode = header.mode;
        let (out, arrive) = {
            let mut env = Reads { lent: self.lent(), missing, cx, header: &header, legs: &[], items: &[] };
            let left = Remaining { out: header.out, arrive: header.arrive };
            let out = template.header.out.resolve(
                &mut env,
                Line::Header(FlowSide::Out),
                Some(&left),
                End::From,
                header.out.unit,
            )?;
            let arrive = template.header.arrive.resolve(
                &mut env,
                Line::Header(FlowSide::Arrive),
                Some(&left),
                End::To,
                header.arrive.unit,
            )?;
            (out, arrive)
        };
        // A missing binding must not silently reuse the compile-time Flow
        // placeholder. Omit this group and retain the missing-input index.
        let missing_side =
            |quantity: axiom_model::Quantity, value: &Option<Resolved>| quantity.root().is_some() && value.is_none();
        if missing_side(template.header.out, &out) || missing_side(template.header.arrive, &arrive) {
            return Ok(None);
        }
        if cx.group.index == 0 && matches!(making.amount, OccurrenceAmount::Omitted) {
            return Ok(None);
        }
        say_sides(&mut header, template, (out, arrive), own_mode);
        let loc = template.header.flow.loc;
        let buys = book.contracts[making.contract].buys;
        if let (0, OccurrenceAmount::Value(amount)) = (cx.group.index, making.amount) {
            apply_occurrence_amount(book, &mut header, buys, making.due, amount)
                .map_err(|fault| TemplateError::Expression { fault, loc })?;
        }
        if let Some(bought) = bought_quantity(book, buys, making.due, header.out, header.arrive, header.infer)? {
            (header.out, header.arrive, header.infer) = (bought.0, bought.1, Infer::Known);
        }
        let Some(tail) = making.tail.filter(|_| cx.group.index == 0) else {
            return Ok(Some(Headed { flow: header, detail: None }));
        };
        let mut detail = merge_detail(*book.flow_view(&header).detail(), tail.detail);
        if let Some(root) = tail.basis {
            let reading = Reading { due: making.source_day, ..cx.written_at };
            let answer = compute(&mut self.lent(), missing, &reading, Cow::Borrowed(&header), root, cx.group.base)?;
            let Answer::Amount(amount) = answer else {
                return Ok(None);
            };
            if amount.unit != book.base {
                let fault = Fault::UnitMismatch { found: amount.unit, expected: book.base };
                return Err(TemplateError::Expression { fault, loc: header.loc });
            }
            detail.basis = Some(amount.qty);
        }
        Ok(Some(Headed { flow: header, detail: Some(detail) }))
    }

    /// A leg of the group as a flow, with the amount it came to; a leg that read an unbound input makes none.
    fn push_leg(&self, cx: &Cx<'_>, leg: LegAt, drawn: Drawn, pools: &mut Pools<'_>) -> Result<(), TemplateError> {
        let (amount, infer, mode) = match drawn {
            Drawn::Omitted => return Ok(()),
            Drawn::Value(value) => (value.amount, value.infer, value.mode.unwrap_or(leg.mode)),
            Drawn::Rest(amount) => (amount, Infer::Known, cx.stamp.mode),
        };
        let mut flow = leg.flow;
        cx.stamp.on(&mut flow);
        *flow.amount_at_mut(leg.side.end()) = amount;
        if !flow.is_exchange() {
            flow.out = amount;
            flow.arrive = amount;
        }
        flow.infer = infer;
        flow.mode = mode;
        self.push_occurrence_flow(flow, cx.making, leg.ordinal, None, pools)
    }

    /// An item of the group as a flow, if it says what it is for: a purpose, which is what makes it a flow of its own.
    fn push_item(
        &self,
        cx: &Cx<'_>,
        header: &Flow,
        item: &ItemAt<'_>,
        amount: Amount,
        pools: &mut Pools<'_>,
    ) -> Result<(), TemplateError> {
        let due = cx.making.due;
        let mut flow = match item.kind {
            ItemKind::Template(item) => {
                let Some(purpose) = item.flow.purpose else { return Ok(()) };
                let mut flow = header.clone();
                flow.day = due;
                cx.stamp.on(&mut flow);
                flow.purpose = Some(purpose);
                flow.description = item.flow.description.or(flow.description);
                flow.codes = item.flow.codes;
                flow.select = item.flow.select;
                flow.detail = None;
                flow.waive = item.flow.waive.or(flow.waive);
                flow.loc = item.loc;
                if item.sign == Sign::Less {
                    std::mem::swap(&mut flow.from, &mut flow.to);
                }
                flow
            }
            ItemKind::Written { flow: Some(source) } if source.purpose.is_some() => {
                let mut flow = source.clone();
                flow.day = due;
                cx.stamp.on(&mut flow);
                flow
            }
            ItemKind::Written { .. } => return Ok(()),
        };
        flow.out = amount;
        flow.arrive = amount;
        flow.infer = Infer::Known;
        self.push_occurrence_flow(flow, cx.making, item.ordinal, None, pools)
    }

    fn push_occurrence_flow(
        &self,
        mut flow: Flow,
        making: &Making<'_>,
        ordinal: u32,
        detail_override: Option<Detail>,
        pools: &mut Pools<'_>,
    ) -> Result<(), TemplateError> {
        let book = self.plan.book;
        let stored = *book.flow_view(&flow).detail();
        let original = detail_override.unwrap_or(stored);
        let shifted = if making.source_day == flow.day {
            original
        } else if flow.day == Day::MIN {
            // The no-`from` terms anchor is a sentinel rather than an
            // authored date. In particular, do not overflow while moving an
            // empty/default Detail from that anchor to the occurrence day.
            original
        } else {
            let shift =
                making.source_day.0.checked_sub(flow.day.0).ok_or(TemplateError::InvalidTemplate { loc: flow.loc })?;
            original.moved(shift)
        };
        flow.day = making.source_day;
        let detail = (shifted != stored).then(|| pools.details.push(RuntimeDetail(shifted)));
        pools.flows.push(RuntimeFlow { flow, detail, ordinal, txn: making.txn });
        Ok(())
    }
}

/// Sorts and de-duplicates what an occurrence added to the omitted inputs from `start` on.
fn tidy(missing: &mut Vec<u16>, start: usize) {
    missing[start..].sort_unstable();
    let mut write = start;
    for read in start..missing.len() {
        if write == start || missing[write - 1] != missing[read] {
            missing[write] = missing[read];
            write += 1;
        }
    }
    missing.truncate(write);
}

/// The contract, the terms of the schedule the occurrence falls due on, and what was written of the occurrence if it
/// was kept.
struct Found<'b> {
    terms: &'b Terms,
    sched: Sched<'b>,
    source_day: Day,
    written: Option<&'b WrittenOccurrence>,
}

fn find<'b>(
    book: &'b Book<'_>,
    contract_id: Id<Contract>,
    schedule: ScheduleKind,
    due: Day,
    source: Option<Id<axiom_model::Txn>>,
) -> Result<Found<'b>, TemplateError> {
    let outside = TemplateError::OutsideTerms { contract: contract_id, day: due };
    let contract = book.contracts.get(contract_id).ok_or(outside)?;
    if !contract.days.contains(due) {
        return Err(outside);
    }
    let terms = contract.terms_of(schedule).ok_or(outside)?;
    let sched = book.promises.schedule(contract_id, schedule).ok_or(outside)?;
    sched.owed(due).map_err(TemplateError::Forecast)?;
    let Some(txn_id) = source else {
        return Ok(Found { terms, sched, source_day: due, written: None });
    };
    let txn = book.txns.get(txn_id).ok_or(outside)?;
    let exact = txn.occurrence.and_then(|id| book.written_occurrences.get(id));
    let keeps = exact.is_some_and(|occurrence| occurrence.due == due && occurrence.schedule == schedule);
    if txn.contract != Some(contract_id) || txn.contract_schedule != Some(schedule) || !keeps {
        return Err(outside);
    }
    Ok(Found { terms, sched, source_day: txn.day, written: exact })
}

fn template_at<'a>(making: &Making<'a>) -> Reading<'a> {
    Reading {
        contract: making.contract,
        program: &making.terms.program,
        scale: making.ratio,
        due: making.due,
        txn: making.txn,
        inputs: making.inputs,
    }
}

/// How what the occurrence wrote is read: against its own program, and unscaled.
fn written_at<'a>(making: &Making<'a>, none: &'a Program) -> Reading<'a> {
    Reading { program: making.program.unwrap_or(none), scale: Ratio::ONE, ..template_at(making) }
}

/// The legs of the group as this occurrence has them: the template's, each replaced by the written leg that goes to
/// the same end, then the written legs that replace none, which are further members of the same split.
fn legs_at(cx: &Cx<'_>) -> Result<Vec<LegAt>, TemplateError> {
    let (template, written, source_flows) = (cx.group.template, cx.group.written, cx.making.source_flows);
    let mut legs = Vec::with_capacity(template.legs.len() + written.map_or(0, |group| group.legs.len()));
    for (index, leg) in template.legs.iter().enumerate() {
        let ordinal = ordinal(cx.group.base, 1 + index, leg.flow.loc)?;
        let (flow, part, written) = match written_leg_for_template(written, source_flows, leg) {
            Some((source, part)) => (source.clone(), part, true),
            None => (leg.flow.clone(), leg.part, false),
        };
        legs.push(LegAt { mode: flow.mode, flow, part, side: template.side, written, ordinal });
    }
    for (side, leg) in written.into_iter().flat_map(|group| group.legs.iter().map(move |leg| (group.side, leg))) {
        let Some(flow) = source_flows.get(leg.flow as usize) else {
            return Err(TemplateError::InvalidTemplate { loc: template.header.flow.loc });
        };
        if template.legs.iter().any(|candidate| same_flow_ends(flow, &candidate.flow)) {
            continue;
        }
        // Use the next effective-leg slot rather than the source
        // written-leg offset: matched replacement legs already use
        // their template slot and must not leave a hole or collide.
        let ordinal = ordinal(cx.group.base, 1 + legs.len(), flow.loc)?;
        legs.push(LegAt { mode: flow.mode, flow: flow.clone(), part: leg.part, side, written: true, ordinal });
    }
    Ok(legs)
}

/// The items of the group, the template's and then the occurrence's, as the solver is asked about them and as the
/// fold has them.
fn items_at<'a>(cx: &Cx<'a>) -> Result<(Vec<Bear>, Vec<ItemAt<'a>>), TemplateError> {
    let (template, written, source_flows) = (cx.group.template, cx.group.written, cx.making.source_flows);
    let after_legs = 1 + template.legs.len() + written.map_or(0, |group| group.legs.len());
    let header = &template.header.flow;
    let side_unit = |side: FlowSide| header.amount_at(side.end()).unit;
    let mut bears = Vec::with_capacity(template.items.len() + written.map_or(0, |group| group.items.len()));
    let mut items = Vec::with_capacity(bears.capacity());
    for (index, item) in template.items.iter().enumerate() {
        let ordinal = ordinal(cx.group.base, after_legs + index, item.loc)?;
        let takes = item.sign == Sign::Carve || (item.sign == Sign::Less && item.flow.purpose.is_none());
        bears.push(Bear { amount: item.amount, side: template.side, unit: side_unit(template.side), takes });
        items.push(ItemAt { kind: ItemKind::Template(item), ordinal, loc: item.loc });
    }
    let written_items = written.into_iter().flat_map(|group| group.items.iter().map(move |item| (group.side, item)));
    for (index, (side, item)) in written_items.enumerate() {
        let flow = item.flow.and_then(|offset| source_flows.get(offset as usize));
        let ordinal = ordinal(cx.group.base, after_legs + template.items.len() + index, item.loc)?;
        let purposed = flow.is_some_and(|flow| flow.purpose.is_some());
        let takes = item.sign == Sign::Carve || (item.sign == Sign::Less && !purposed);
        bears.push(Bear { amount: item.amount, side, unit: side_unit(side), takes });
        items.push(ItemAt { kind: ItemKind::Written { flow }, ordinal, loc: item.loc });
    }
    Ok((bears, items))
}

/// The ordinal of the flow `offset` places on from the one the occurrence's group starts at.
fn ordinal(base: u32, offset: usize, loc: Loc) -> Result<u32, TemplateError> {
    u32::try_from(offset).ok().and_then(|offset| base.checked_add(offset)).ok_or(TemplateError::InvalidTemplate { loc })
}

/// What went wrong in the solver, as the error the occurrence fails with.
fn failure(failed: Failed<TemplateError>, header: &Flow, legs: &[LegAt], items: &[ItemAt<'_>]) -> TemplateError {
    let loc = |at| match at {
        Line::Header(_) => header.loc,
        Line::Leg(index) => legs[index].flow.loc,
        Line::Item(index) => items[index].loc,
    };
    match failed {
        Failed::Env(error) => error,
        Failed::Fault { at, fault } => TemplateError::Expression { fault, loc: loc(at) },
        Failed::TwoRests { at } => TemplateError::InvalidTemplate { loc: loc(at) },
    }
}

/// What the occurrence's expressions are evaluated against: the flow of each line of the group, and the
/// reading of its template or of what the occurrence wrote.
struct Reads<'a, 'p, 'b, 's> {
    lent: Lent<'a, 'p, 'b, 's>,
    missing: &'a mut Vec<u16>,
    cx: &'a Cx<'a>,
    header: &'a Flow,
    legs: &'a [LegAt],
    items: &'a [ItemAt<'a>],
}

impl<'a> Reads<'a, '_, '_, '_> {
    /// How the line is read, where it is in the occurrence, and where a literal of it that cannot be scaled is.
    fn site(&self, at: Line) -> (Reading<'a>, u32, Loc) {
        let (cx, written) = (self.cx, |written| if written { self.cx.written_at } else { self.cx.template_at });
        match at {
            Line::Header(_) => (cx.template_at, cx.group.base, self.header.loc),
            Line::Leg(index) => {
                let leg = &self.legs[index];
                (written(leg.written), leg.ordinal, leg.flow.loc)
            }
            Line::Item(index) => {
                let item = &self.items[index];
                (written(matches!(item.kind, ItemKind::Written { .. })), item.ordinal, item.loc)
            }
        }
    }

    /// The flow the line is read against. An item's is the one it was written as, or else the header as the legs and
    /// the items before it left it.
    fn flow(&self, at: Line, left: Option<&Remaining>) -> Cow<'a, Flow> {
        let (header, legs, items) = (self.header, self.legs, self.items);
        match at {
            Line::Header(_) => Cow::Borrowed(header),
            Line::Leg(index) => Cow::Borrowed(&legs[index].flow),
            Line::Item(index) => match items[index].kind {
                ItemKind::Written { flow: Some(flow) } => Cow::Borrowed(flow),
                _ => Cow::Owned(left.map_or_else(
                    || header.clone(),
                    |left| Flow { out: left.out, arrive: left.arrive, ..header.clone() },
                )),
            },
        }
    }
}

impl Env for Reads<'_, '_, '_, '_> {
    type Failure = TemplateError;

    fn amount(&mut self, at: Line, left: Option<&Remaining>, expr: Expr) -> Result<Answer, TemplateError> {
        let (reading, ordinal, loc) = self.site(at);
        match expr {
            Expr::Literal(amount) => scale(amount, reading.scale)
                .map(Answer::Amount)
                .map_err(|fault| TemplateError::Expression { fault, loc }),
            Expr::Computed(root) => {
                let flow = self.flow(at, left);
                compute(&mut self.lent, self.missing, &reading, flow, root, ordinal)
            }
        }
    }

    /// An `=` leg moves the gap to its balance, which is what it takes from the header; an `all` or a `?` is left the
    /// marker it is.
    fn lands(&mut self, at: Line, marker: &Resolved) -> Result<Option<Amount>, TemplateError> {
        let (Line::Leg(index), Infer::Target { end, balance }) = (at, marker.infer) else { return Ok(None) };
        let flow = &self.legs[index].flow;
        Ok(Some(Amount::new(self.lent.gap(flow, end, balance), flow.amount_at(end).unit)))
    }

    fn payment(&mut self, at: Line) -> Result<Answer, TemplateError> {
        let reading = self.site(at).0;
        let loan = self.lent.plan.book.promises.loan(reading.contract);
        loan.map(|loan| Answer::Amount(loan.payment()))
            .ok_or(TemplateError::Forecast(axiom_model::ForecastError::UnsupportedLoan(reading.due)))
    }
}

/// The amount a node of the occurrence's program computes for `flow`, scaled as the occurrence says; Omitted, and
/// the input noted, when it reads one that is not bound.
fn compute(
    lent: &mut Lent<'_, '_, '_, '_>,
    missing: &mut Vec<u16>,
    reading: &Reading<'_>,
    flow: Cow<'_, Flow>,
    root: axiom_model::NodeId,
    ordinal: u32,
) -> Result<Answer, TemplateError> {
    let node = reading
        .program
        .nodes
        .as_slice()
        .get(root.0 as usize)
        .ok_or(TemplateError::InvalidTemplate { loc: flow.loc })?;
    let mut view = flow.into_owned();
    view.day = reading.due;
    let evaluating = Evaluating {
        program: reading.program,
        inputs: reading.inputs,
        txn: reading.txn,
        cause: Cause::Applied(ordinal),
        ordinal,
        day: reading.due,
        binds: Binds::Nothing,
    };
    match lent.value(&view, root, &evaluating) {
        Value::Amount(amount) => scale(amount, reading.scale)
            .map(Answer::Amount)
            .map_err(|fault| TemplateError::Expression { fault, loc: node.loc }),
        Value::Fault(Fault::MissingInput(index)) => {
            missing.push(index);
            Ok(Answer::Omitted)
        }
        Value::Fault(fault) => Err(TemplateError::Expression { fault, loc: node.loc }),
        _ => Err(TemplateError::Expression { fault: Fault::InvalidProgram, loc: node.loc }),
    }
}

fn scale(amount: Amount, ratio: Ratio) -> Result<Amount, Fault> {
    amount.scaled(ratio)
}

fn same_flow_ends(left: &Flow, right: &Flow) -> bool {
    left.from == right.from && left.to == right.to
}

/// The flow a written occurrence made for the template leg that goes to the same end, and what it takes.
fn written_leg_for_template<'a>(
    written: Option<&Made>,
    source_flows: &'a [Flow],
    template: &axiom_model::Leg<Flow>,
) -> Option<(&'a Flow, Part)> {
    written?.legs.iter().find_map(|leg| {
        let flow = source_flows.get(leg.flow as usize)?;
        same_flow_ends(flow, &template.flow).then_some((flow, leg.part))
    })
}

/// What the header's two sides come to, said on the header flow.
fn say_sides(
    header: &mut Flow,
    template: &Promised,
    (out, arrive): (Option<Resolved>, Option<Resolved>),
    own_mode: Mode,
) {
    for (end, side) in [(End::From, out), (End::To, arrive)] {
        if let Some(side) = side {
            set_quantity(header, end, side);
        }
    }
    if let (Some(out), Some(arrive)) = (out, arrive) {
        header.infer = if out.infer != Infer::Known { out.infer } else { arrive.infer };
        if out.mode == Some(Mode::Pending) || arrive.mode == Some(Mode::Pending) {
            header.mode = Mode::Pending;
        }
    }
    let (out_is_explicit, arrive_is_explicit) =
        (template.header.out.root().is_some(), template.header.arrive.root().is_some());
    if header.is_exchange() {
        return;
    }
    // One side computed and the other not: the computed amount is both.
    match (out, arrive, out_is_explicit, arrive_is_explicit) {
        (Some(out), _, true, false) => {
            (header.arrive, header.infer, header.mode) = (out.amount, out.infer, out.mode.unwrap_or(own_mode));
        }
        (_, Some(arrive), false, true) => {
            (header.out, header.infer, header.mode) = (arrive.amount, arrive.infer, arrive.mode.unwrap_or(own_mode));
        }
        _ => {}
    }
}

fn set_quantity(flow: &mut Flow, end: End, quantity: Resolved) {
    *flow.amount_at_mut(end) = quantity.amount;
    if quantity.infer != Infer::Known {
        flow.infer = quantity.infer;
    } else if flow.infer == Infer::Unknown || flow.infer == Infer::All {
        flow.infer = Infer::Known;
    }
    if let Some(mode) = quantity.mode {
        flow.mode = mode;
    }
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
            let spend =
                book.convert(amount, flow.out.unit, day).ok_or(Fault::NoPrice { unit, quote: flow.out.unit })?;
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
        flow.infer = if buys.is_some() && flow.arrive.unit != amount.unit { Infer::Unknown } else { Infer::Known };
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
    Err(Fault::UnitMismatch { found: amount.unit, expected: flow.out.unit })
}

fn merge_detail(base: Detail, over: Detail) -> Detail {
    Detail {
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
        book.convert(arrive, unit, day).map(|amount| (amount, arrive))
    } else {
        None
    };
    converted.map(Some).ok_or_else(|| {
        let missing = if out.unit != unit && arrive.unit == unit { out.unit } else { arrive.unit };
        TemplateError::Expression { fault: Fault::NoPrice { unit: missing, quote: unit }, loc: Loc::default() }
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
        let (book, diagnostics) = axiom_model::build(&[Source { path: "occurrence.ax", file, embedded: false }]);
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "source should build: {diagnostics:?}");
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(rent, ScheduleKind::Regular, due, 0, None, &mut flows, &mut details, &mut missing)
            .expect("active occurrence materializes");
        let made_flows = made.flows(&flows).expect("output range belongs to the pool");
        assert_eq!(made_flows.len(), 1);
        assert_eq!(made_flows[0].flow.day, due);
        assert_eq!(made_flows[0].flow.out.qty, Qty(10_000));
        assert_eq!(made_flows[0].flow.arrive.qty, Qty(10_000));
        assert_eq!(made_flows[0].flow.mode, Mode::Planned);
        assert_eq!(made_flows[0].ordinal, 0);
        assert!(made.missing(&missing).unwrap().is_empty());
        let view = book.runtime_flow_view(&made_flows[0], &details);
        assert_eq!(view.from, book.place("checking").unwrap());
        assert_eq!(view.to, book.entities[book.contracts[rent].party].place.unwrap());
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
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
        let header = rows.iter().find(|flow| flow.flow.to == checking).expect("header pays checking");
        let deferral = rows.iter().find(|flow| flow.flow.to == retirement).expect("percentage leg pays retirement");
        assert_eq!((header.flow.out.qty, header.flow.arrive.qty), (Qty(432_400), Qty(432_400)));
        assert_eq!((deferral.flow.out.qty, deferral.flow.arrive.qty), (Qty(27_600), Qty(27_600)));
        assert_eq!(header.flow.arrive.qty + deferral.flow.arrive.qty, Qty(460_000));
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
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
        assert_eq!(view.codes().map(|code| book.name(code)).collect::<Vec<_>>(), ["paid"]);
        assert_eq!(book.text(materialized[0].flow.description.expect("tail description")), "January rent");
        assert!(made.missing(&missing).unwrap().is_empty());

        let mut forecast = plan.start(Options { today: due, relaxed: false });
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
        assert_eq!(forecasted.flows(&forecast_flows).unwrap()[0].flow.out.qty, Qty(11_000));
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
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
        assert_eq!(rows.iter().find(|row| row.flow.to == savings).unwrap().flow.arrive.qty, Qty(4_000));
        assert_eq!(rows.iter().find(|row| row.flow.to == bonus).unwrap().flow.arrive.qty, Qty(1_000));
        assert!(rows.iter().all(|row| row.flow.from == checking));
        assert_eq!(rows.iter().map(|row| row.ordinal).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(rows.iter().map(|row| row.flow.arrive.qty.0).sum::<i64>(), 10_000);
        assert!(made.missing(&missing).unwrap().is_empty());
    }

    /// The flows one kept occurrence of `rent` makes, as `(to, amount in cents, ordinal)`.
    fn kept_rent(source: &'static str, written_on: Day) -> Vec<(String, i64, u32)> {
        let book = source_book(source);
        let contract = book.contract("rent").expect("contract id");
        let written = book
            .txns
            .iter()
            .find_map(|(id, txn)| (txn.contract == Some(contract) && txn.day == written_on).then_some(id))
            .expect("the kept occurrence transaction");
        let due = book.txns[written].day;
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        ledger.advance(Day(due.0 - 1));
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let occurrence = book.written_occurrences[book.txns[written].occurrence.unwrap()].due;
        let made = ledger
            .instantiate_occurrence(
                contract,
                ScheduleKind::Regular,
                occurrence,
                0,
                Some(written),
                &mut flows,
                &mut details,
                &mut missing,
            )
            .expect("the occurrence materializes");
        let rows = made.flows(&flows).expect("range belongs to the pool");
        rows.iter()
            .map(|row| (book.name(book.places[row.flow.to].path).to_owned(), row.flow.arrive.qty.0, row.ordinal))
            .collect()
    }

    #[test]
    fn written_legs_that_replace_none_take_the_next_slots_in_order() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
account savings
account bonus
account reserve
entity landlord
contract rent with landlord
  100 USD monthly on 1 from checking
  savings 30 USD
  from 2026-01-01
2026-02-01 rent
  bonus 10 USD
  reserve 20 USD
";
        let rows = kept_rent(source, Day::from_ymd(2026, 2, 1).unwrap());
        assert_eq!(
            rows,
            [
                ("landlord".to_owned(), 4_000, 0),
                ("savings".to_owned(), 3_000, 1),
                ("bonus".to_owned(), 1_000, 2),
                ("reserve".to_owned(), 2_000, 3)
            ]
        );
    }

    #[test]
    fn a_target_leg_takes_the_gap_to_its_balance_from_the_header_and_not_the_balance() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
account savings
entity landlord
opening 2026-01-01
  checking 5_000 USD
  savings 500 USD
contract rent with landlord
  100 USD monthly on 1 from checking
  savings = 530 USD
  from 2026-01-01
2026-02-01 rent
";
        let rows = kept_rent(source, Day::from_ymd(2026, 2, 1).unwrap());
        // savings holds 500.00, so the leg moves 30.00 and the landlord is paid what is left of the 100.00
        assert_eq!(rows, [("landlord".to_owned(), 7_000, 0), ("savings".to_owned(), 3_000, 1)]);
    }

    #[test]
    fn a_discount_that_makes_no_flow_is_taken_off_the_header_and_one_with_a_purpose_is_not() {
        let source = "\
base USD
commodity USD
  precision 2
purpose fees : spending
account checking
entity landlord
contract rent with landlord
  100 USD monthly on 1 from checking
  - 5 USD
  - 2 USD #fees
  + 7 USD #fees
  from 2026-01-01
2026-02-01 rent
";
        let rows = kept_rent(source, Day::from_ymd(2026, 2, 1).unwrap());
        // the header is 100 less the discount that says nothing; the `- 2 USD #fees` is a flow of its own, back, and
        // `+ 7 USD #fees` is one on top. An item's ordinal is its place among the items, whether or not it makes a flow.
        assert_eq!(
            rows,
            [("landlord".to_owned(), 9_500, 0), ("checking".to_owned(), 200, 2), ("landlord".to_owned(), 700, 3)]
        );
    }

    #[test]
    fn a_computed_basis_in_the_tail_of_a_kept_occurrence_is_evaluated_for_its_header() {
        let source = "\
base USD
commodity USD
  precision 2
account checking
entity landlord
contract flat with landlord
  100 USD monthly on 1 from checking
  from 2026-01-01
2026-03-01 flat 100 USD basis 10% of 1000 USD
";
        let book = source_book(source);
        let contract = book.contract("flat").expect("contract id");
        let written = book.txns.iter().find_map(|(id, txn)| (txn.contract == Some(contract)).then_some(id)).unwrap();
        let due = Day::from_ymd(2026, 3, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
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
            .expect("the occurrence materializes");
        let rows = made.flows(&flows).unwrap();
        assert_eq!(book.runtime_flow_view(&rows[0], &details).detail().basis, Some(Qty(10_000)));
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
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
        assert_eq!(made_flows.len(), 2, "base rent plus the computed utilities item");
        assert_eq!(made_flows[0].flow.out.qty, Qty(10_000));
        assert_eq!(made_flows[1].flow.out.qty, Qty(1_860));
        assert_eq!(made_flows[0].flow.mode, Mode::Actual);
        assert_eq!(made_flows[1].ordinal, 1);
        assert!(made.missing(&missing).unwrap().is_empty());

        let mut forecast = plan.start(Options { today: due, relaxed: false });
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
    fn occurrence_missing_input_omits_only_its_item_and_posts_known_flows() {
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
  input gas USD
  + 12% of water #utilities
  + 10% of gas #utilities
  from 2026-01-01
2026-02-01 flat
  water = 155 USD
";
        let book = source_book(source);
        let flat = book.contract("flat").expect("contract id");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let run = crate::run(&book, Options { today: due, relaxed: false });

        let checking = book.place("checking").unwrap();
        let landlord = book.entities[book.contracts[flat].party].place.unwrap();
        let usd = book.commodity("USD").unwrap();
        let held_at = |place| {
            run.holdings
                .iter()
                .find(|holding| holding.place == place && holding.unit == usd)
                .map_or(Qty::ZERO, |holding| holding.qty())
        };
        assert_eq!(held_at(checking), Qty(-11_860));
        assert_eq!(held_at(landlord), Qty(11_860));
        let promise = run
            .promises
            .iter()
            .find(|promise| promise.contract == flat && promise.due == due)
            .expect("the incomplete kept occurrence remains visible");
        assert!(promise.kept.is_some());
        assert_eq!(run.promise_missing_inputs(promise), [1]);
        assert_eq!(run.promise_flows(promise).len(), 2);
    }

    #[test]
    fn loan_occurrence_without_from_uses_its_due_day_as_default_recognition() {
        let source = "\
base USD
commodity USD
  precision 2
entity bank
asset car
account checking
contract car-loan with bank
  loan 3_000 USD on 2026-01-01 at 0% over 3m for car
  monthly on 1 from checking
";
        let book = source_book(source);
        let contract = book.contract("car-loan").expect("contract id");
        let due = Day::from_ymd(2026, 2, 1).unwrap();
        let plan = Plan::new(&book);
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let output = ledger
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
            .expect("the default Day::MIN template anchor is not a runtime date");
        let flows = output.flows(&flows).unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].flow.day, due);
        assert_eq!(flows[0].flow.out.qty, Qty(100_000));
        assert_eq!(flows[0].flow.recognized, axiom_core::Days::on(due));
        assert!(output.missing(&missing).unwrap().is_empty());
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(rent, ScheduleKind::Regular, due, 0, None, &mut flows, &mut details, &mut missing)
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
        let mut ledger = plan.start(Options { today: due, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), Arena::<RuntimeDetail>::new(), Vec::new());
        let made = ledger
            .instantiate_occurrence(split, ScheduleKind::Regular, due, 0, None, &mut flows, &mut details, &mut missing)
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
        assert_eq!(flows.iter().map(|flow| flow.flow.out.qty.0).sum::<i64>(), 10_000);
    }
}
