//! Firing laws: which rules run for an occasion, and what becomes of what they
//! find.
//!
//! The book's `Rules` already say which laws watch which place, in order, dated
//! by residence. This module filters them by day, binds a law's variables
//! ([`Occasion`]), evaluates, and turns [`Outcome`]s into records: tallies and
//! obligations, violations with their diagnostics, faults as their own errors,
//! and the last reading of every limit.
//!
//! A violated law is reported once per subject and window, at the flow that
//! crossed the line (or, for a window that value recognized ahead of time
//! broke by itself, as the window opens), and its diagnostic is built only then.

use axiom_core::{Day, Days, Diagnostic, Id, Qty, Span, Sym};
use axiom_model::{
    Amount, Cap, CapTarget, Effect as LawEffect, Entity, Fault, Law, Period, Rule, StepKind, Subject, Trigger, Window,
};

use crate::eval::{self, Context, Env, Occasion, Outcome};
use crate::explain::{self, Frame};
use crate::facts::{Reads, Shortcut};
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::plan::Plan;
use crate::scope::owner_of;
use crate::state::Missing;
use crate::totals::{Reached, by_year};
use crate::{
    Adjustment, AdjustmentKind, Consequence, Effect, EventKey, Headroom, Owed, PartId, Verdict, Violation, Waiver,
};

/// Whether the rule is in force for some day of the occasion.
fn applies(plan: &Plan, rule: &Rule, on: &Occasion) -> bool {
    let internal = on.skip_internal
        && on.motion.is_some_and(|m| plan.inside(rule.subject, m.from) && plan.inside(rule.subject, m.to));
    rule.days.overlaps(on.span) && !internal
}

fn law_consumes(book: &axiom_model::Book<'_>, law: Id<Law>) -> bool {
    book.laws[law].steps.iter().any(|step| match &step.kind {
        StepKind::Effect(LawEffect::Consume { .. }) => true,
        StepKind::Require { otherwise, .. } => {
            otherwise.iter().any(|effect| matches!(effect, LawEffect::Consume { .. }))
        }
        _ => false,
    })
}

impl Ledger<'_, '_, '_> {
    /// Runs every rule in `rules` that applies to this occasion, in order.
    pub(crate) fn fire(&mut self, rules: &[Rule], on: &Occasion) {
        self.fire_as(rules, on, None);
    }

    /// A purpose rule's subject is the owner of this particular flow, not the
    /// purpose declaration. The model's rule table has already expanded the
    /// purpose ancestors, so the same firing path handles both scopes.
    pub(crate) fn fire_as(&mut self, rules: &[Rule], on: &Occasion, subject: Option<Subject>) {
        // Most places have no rule for most occasions: nothing to set up then.
        if rules.is_empty() {
            return;
        }
        let mut done = std::mem::take(&mut self.scratch.done);
        done.clear();
        self.fire_as_with_done(rules, on, subject, &mut done, self.plan.repeats);
        self.scratch.done = done;
    }

    fn fire_as_with_done(
        &mut self,
        rules: &[Rule],
        on: &Occasion,
        subject: Option<Subject>,
        done: &mut Vec<(axiom_core::Id<axiom_model::Law>, Subject)>,
        deduplicate: bool,
    ) {
        if rules.is_empty() {
            return;
        }
        let plan = self.plan;
        let book = plan.book;
        for written in rules {
            // Purpose rules use the flow owner at run time. It must govern
            // filtering, de-duplication, headroom and diagnostics alike.
            let rule = Rule { subject: subject.unwrap_or(written.subject), ..*written };
            if !applies(plan, &rule, &on) {
                continue;
            }
            // A law that two rules bring to one subject runs once.
            let subject = rule.subject;
            if deduplicate {
                if done.contains(&(rule.law, subject)) {
                    continue;
                }
                done.push((rule.law, subject));
            }
            self.enforce(&rule, &Context::new(subject, owner_of(book, subject), &on));
        }
    }

    /// Whether the `on spend` laws of `entity` permit the flow: a dry run that
    /// records nothing. It is the same evaluation the real firing will do.
    pub(crate) fn permits_spend(&mut self, entity: Id<Entity>, m: &Motion) -> bool {
        let (plan, book) = (self.plan, self.plan.book);
        let on = Occasion { amount: Some(m.out), ..Occasion::flow(m) };
        book.rules.on_spend[entity].iter().filter(|rule| applies(plan, rule, &on)).all(|rule| {
            self.evaluate(rule.law, &Context::new(rule.subject, owner_of(book, rule.subject), &on));
            let holds = !self.scratch.outcomes.iter().any(|o| {
                matches!(o, Outcome::Broken { warn: false, .. } | Outcome::Priced { .. } | Outcome::Faulted { .. })
            });
            self.scratch.outcomes.clear();
            holds
        })
    }

    /// Fires one `by` law whose date the journal has reached, or closes a
    /// month or year for an `each` law, if its rule was in force then.
    pub(crate) fn deadline(&mut self, rule: usize, day: Day, period: Days) {
        let rule = self.plan.book.rules.timed[rule];
        if let Subject::Asset(asset) = rule.subject {
            let count = self.world.assets.asset(asset).map_or(0, |state| state.part_count());
            let on = Occasion::time(day, period);
            for index in 0..count {
                let Some(part) = self.world.assets.asset(asset).and_then(|state| state.parts().get(index)).copied()
                else {
                    continue;
                };
                if self.asset_part_held(asset, part.id, day) {
                    self.fire_asset_part(rule, &on, part.id);
                }
            }
        } else {
            self.fire(std::slice::from_ref(&rule), &Occasion::time(day, period));
        }
    }

    /// Runs the consuming laws due so far for each part before the asset is
    /// disposed. A sale precedes the end-of-period deadline in the timeline,
    /// so this closes only the partial sale period and ordinary deadlines
    /// later that day see the disposal boundary and skip the asset.
    pub(crate) fn pre_disposal(&mut self, asset: axiom_core::Id<axiom_model::Asset>, day: Day) {
        let count = self.world.assets.asset(asset).map_or(0, |state| state.part_count());
        if count == 0 {
            return;
        }
        let mut rules = Vec::new();
        for rule in self.plan.book.rules.timed.iter().copied() {
            if rule.subject != Subject::Asset(asset) || !law_consumes(self.plan.book, rule.law) {
                continue;
            }
            let window = match self.plan.book.laws[rule.law].trigger {
                Trigger::Each(Period::Month, None) => Window::Month,
                Trigger::Each(Period::Year, None) => Window::Year,
                // A once-only `by` deadline is its own event. A sale does not
                // advance unrelated deadlines early.
                _ => continue,
            };
            let whole = window.around(day);
            let Some(partial) = Days::new(whole.first(), day) else {
                continue;
            };
            if rule.days.overlaps(partial) {
                rules.push((rule, partial));
            }
        }
        if rules.is_empty() {
            return;
        }
        for index in 0..count {
            let Some(part) = self.world.assets.asset(asset).and_then(|state| state.parts().get(index)).copied() else {
                continue;
            };
            if !self.asset_part_held(asset, part.id, day) {
                continue;
            }
            for (rule, partial) in rules.iter().copied() {
                let on = Occasion::partial_terminal(day, partial);
                if applies(self.plan, &rule, &on) {
                    self.fire_asset_part(rule, &on, part.id);
                }
            }
        }
    }

    fn asset_part_held(&self, asset: axiom_core::Id<axiom_model::Asset>, part: PartId, day: Day) -> bool {
        let Some(state) = self.world.assets.asset(asset) else {
            return false;
        };
        let Some((owner, record)) = self.world.assets.part(part) else {
            return false;
        };
        owner == asset && record.recorded.day <= day && state.held_at(EventKey { day, sequence: u64::MAX })
    }

    fn fire_asset_part(&mut self, rule: Rule, on: &Occasion<'_>, part: PartId) {
        if !applies(self.plan, &rule, on) {
            return;
        }
        let book = self.plan.book;
        let context = Context::new(rule.subject, owner_of(book, rule.subject), on).for_asset_part(part);
        self.enforce(&rule, &context);
    }

    /// Reads the laws about every month that begins by `day` with value
    /// already recognized into it, and about the year that month opens. A
    /// limit on a window's total is checked whenever a flow adds to it, and
    /// value recognized ahead of time adds to windows no flow will land in: an
    /// annual premium paid in December `for` the next year, a cost spread over
    /// months. Reading it as the window opens puts what it counted in the
    /// headroom, and reports a limit that it alone breaks, once.
    #[inline]
    pub(crate) fn enter(&mut self, day: Day) {
        if self.world.totals.reaches_by(day) {
            self.enter_months(day);
        }
    }

    #[cold]
    fn enter_months(&mut self, day: Day) {
        let plan = self.plan;
        let mut done = std::mem::take(&mut self.scratch.done);
        let mut purpose_rules = std::mem::take(&mut self.scratch.purpose_rules);
        let mut opened = None;
        while let Some(reached) = self.world.totals.reached(&plan.watch, day) {
            let from = match &reached {
                Reached::Subject(_, from) | Reached::Purpose(_, _, from) => *from,
            };
            if opened != Some(from) {
                done.clear();
                opened = Some(from);
            }
            match reached {
                Reached::Subject(subject, from) => {
                    for window in [Window::Month, Window::Year] {
                        let period = window.around(from);
                        let rules = plan.readers.get(&(subject, window));
                        if let Some(rules) = rules.filter(|_| period.first() == from) {
                            self.fire_as_with_done(
                                rules,
                                &Occasion::window(from, period),
                                None,
                                &mut done,
                                plan.repeats,
                            );
                        }
                    }
                }
                Reached::Purpose(owner, purpose, from) => {
                    for window in [Window::Month, Window::Year] {
                        let period = window.around(from);
                        if period.first() != from {
                            continue;
                        }
                        purpose_rules.clear();
                        if let Some(rules) = plan.purpose_readers.get(&(purpose, window)) {
                            for &rule in rules {
                                if !purpose_rules.contains(&rule) {
                                    purpose_rules.push(rule);
                                }
                            }
                        }
                        self.fire_as_with_done(
                            &purpose_rules,
                            &Occasion::purpose_window(from, period, window),
                            Some(Subject::Entity(owner)),
                            &mut done,
                            true,
                        );
                    }
                }
            }
        }
        done.clear();
        self.scratch.done = done;
        purpose_rules.clear();
        self.scratch.purpose_rules = purpose_rules;
    }

    pub(crate) fn evaluate(&mut self, law: Id<Law>, ctx: &Context) -> bool {
        let env = Env { plan: self.plan, world: &self.world };
        eval::run(
            env,
            &self.plan.book.laws[law],
            ctx,
            &mut self.scratch.values,
            &mut self.scratch.budget_values,
            &mut self.scratch.outcomes,
        )
    }

    /// Reads a cap that holds without evaluating it: the total in its window
    /// against the limit is all the law compares. Returns whether it held; a
    /// cap that is broken is evaluated in full, which explains why.
    fn within(&mut self, rule: &Rule, ctx: &Context, cap: Cap) -> bool {
        // Rolling totals are stored in the book's base commodity. The cap
        // shortcut is only valid without conversion when its literal limit is
        // also in that commodity; otherwise the full typed evaluator handles
        // prices, scaling and missing-rate faults.
        if cap.limit.unit != self.plan.book.base {
            return false;
        }
        let read = match cap.target {
            CapTarget::Total(dir) => {
                self.world.totals.read(&self.plan.watch, rule.subject, dir, cap.window, ctx.anchor())
            }
            CapTarget::Purpose(purpose) => {
                let (incoming, outgoing) = self.world.totals.read_purpose(ctx.owner, purpose, cap.window, ctx.anchor());
                match self.plan.book.purposes[purpose].root {
                    axiom_model::PurposeRoot::Income => incoming - outgoing,
                    axiom_model::PurposeRoot::Spending
                    | axiom_model::PurposeRoot::Capital
                    | axiom_model::PurposeRoot::Transfer => outgoing - incoming,
                }
            }
        };
        let counted = Amount::new(read, self.plan.book.base);
        let holds = if cap.strict { counted.qty < cap.limit.qty } else { counted.qty <= cap.limit.qty };
        if holds {
            self.record.checks[rule.law.index()] += 1;
            self.read(rule, ctx, 0, counted, cap.limit);
        }
        holds
    }

    fn consume(&mut self, rule: &Rule, ctx: &Context, step: u32, amount: Amount) {
        let (Subject::Asset(asset), Some(part)) = (ctx.subject, ctx.asset_part) else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };
        let book = self.plan.book;
        let Some(amount) = book.convert(amount, book.base, ctx.day) else {
            self.fault(rule, ctx, step as usize, Fault::NoPrice { unit: amount.unit, quote: book.base });
            return;
        };
        match self.consume_asset_part(asset, part, amount.qty) {
            Ok(consumption) => {
                if !consumption.applied.is_zero() {
                    self.sample_temporal(ctx.day);
                }
                if !consumption.excess.is_zero() {
                    self.record.report(
                        Diagnostic::error(
                            "asset-consume-excess",
                            "the law consumes more basis than this asset part has remaining",
                        )
                        .label(book.laws[rule.law].loc, "consumption is limited to remaining basis"),
                    );
                }
                if !consumption.applied.is_zero() {
                    self.record.adjustments.push(Adjustment {
                        day: ctx.day,
                        law: rule.law,
                        kind: AdjustmentKind::Consumed { asset, part },
                        amount: consumption.applied,
                    });
                }
            }
            Err(error) => {
                self.record.report(
                    Diagnostic::error("asset-consume", format!("asset part basis could not be consumed: {error:?}"))
                        .label(book.laws[rule.law].loc, "basis update failed"),
                );
            }
        }
    }

    fn carry(
        &mut self,
        rule: &Rule,
        ctx: &Context,
        step: u32,
        amount: Amount,
        unit: axiom_core::Id<axiom_model::Commodity>,
        within: Span,
    ) {
        let Some(realized) = ctx.realized else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };
        let Some(from) = realized.part else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };
        if realized.gain >= Qty::ZERO || amount.qty <= Qty::ZERO || within.months < 0 || within.days < 0 {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        }
        let book = self.plan.book;
        let Some(amount) = book.convert(amount, book.base, ctx.day) else {
            self.fault(rule, ctx, step as usize, Fault::NoPrice { unit: amount.unit, quote: book.base });
            return;
        };
        let Some(loss) = realized.gain.0.checked_neg().map(Qty) else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };
        if amount.qty > loss {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        }
        let Some(sold) = ctx.amount.filter(|amount| amount.unit == unit).and_then(|_| {
            ctx.realized.filter(|realized| realized.quantity > Qty::ZERO).map(|realized| realized.quantity)
        }) else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };

        // Search only this owner's asset places and the matching commodity.
        // The held parcels are the canonical acquisition and quantity data.
        let mut candidates: Vec<(Day, PartId, Qty)> = Vec::new();
        let mut overflow = false;
        for &place in self.plan.places_of(ctx.owner) {
            let Some(slot) = self.world.holdings.get(place, unit) else { continue };
            for parcel in &slot.holding.lots {
                let Some(part) = parcel.part else { continue };
                if part == from
                    || parcel.qty <= Qty::ZERO
                    || parcel.wash_matched
                    || parcel.acquired > ctx.day
                    || !crate::Assets::within_carry_window(parcel.acquired, ctx.day, within)
                {
                    continue;
                }
                if let Some((_, _, quantity)) = candidates
                    .iter_mut()
                    .find(|(seen_day, seen_part, _)| *seen_day == parcel.acquired && *seen_part == part)
                {
                    if let Some(sum) = quantity.0.checked_add(parcel.qty.0) {
                        quantity.0 = sum;
                    } else {
                        overflow = true;
                        break;
                    }
                } else {
                    candidates.push((parcel.acquired, part, parcel.qty));
                }
            }
            if overflow {
                break;
            }
        }
        if overflow {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        }
        candidates.sort_by_key(|(day, _, _)| std::cmp::Reverse(*day));
        let Some(available) =
            candidates.iter().try_fold(Qty::ZERO, |sum, (_, _, qty)| sum.0.checked_add(qty.0).map(Qty))
        else {
            self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
            return;
        };
        let matched_qty = available.min(sold);
        let mut quantity_left = matched_qty;
        let mut shares = crate::lots::Shares::new(amount.qty, sold);
        let mut additions = Vec::new();
        let mut matched_amount = Qty::ZERO;
        for (acquired, part, available) in candidates {
            if quantity_left.is_zero() {
                break;
            }
            let quantity = available.min(quantity_left);
            let basis = shares.take(quantity);
            if !basis.is_zero() || !quantity.is_zero() {
                additions.push(crate::lots::CarryLotAddition {
                    part,
                    acquired,
                    held_since: realized.held_since,
                    quantity,
                    amount: basis,
                });
            }
            let Some(sum) = matched_amount.0.checked_add(basis.0).map(Qty) else {
                self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
                return;
            };
            matched_amount = sum;
            quantity_left -= quantity;
        }
        if !additions.is_empty() {
            if self.carry_basis_to_parts(&additions).is_err() {
                self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
                return;
            }
            self.sample_temporal(ctx.day);
            for addition in &additions {
                self.record.adjustments.push(Adjustment {
                    day: ctx.day,
                    law: rule.law,
                    kind: AdjustmentKind::Carried { from, to: Some(addition.part) },
                    amount: addition.amount,
                });
            }
        }
        let unmatched_qty = sold - matched_qty;
        if !unmatched_qty.is_zero() {
            let unmatched_amount = amount.qty - matched_amount;
            if !unmatched_amount.is_zero() {
                let carry = crate::PendingCarry {
                    law: rule.law,
                    from,
                    cause: ctx.cause,
                    owner: ctx.owner,
                    unit,
                    sold: ctx.day,
                    held_since: realized.held_since,
                    within,
                    quantity: unmatched_qty,
                    amount: unmatched_amount,
                    codes: realized.codes,
                };
                if self.world.assets.enqueue_carry(carry).is_err() {
                    self.fault(rule, ctx, step as usize, Fault::InvalidProgram);
                }
            }
        }
        self.sample_temporal(ctx.day);
    }

    /// Reads a floor of nothing (`balance >= empty`) straight off the holdings
    /// without evaluating it: the subject's holdings, when all are in the base
    /// currency, add up to no less than nothing. A subject that holds another
    /// commodity, or that falls short, is evaluated in full, which prices it or
    /// explains why. Nothing is recorded but that it held.
    fn afloat(&mut self, rule: &Rule) -> bool {
        let (plan, base) = (self.plan, self.plan.book.base);
        let sign = eval::sign(plan, rule.subject);
        let mut balance = Qty::ZERO;
        for slot in eval::held(plan, &self.world, rule.subject) {
            if slot.unit != base {
                return false;
            }
            balance += Qty(slot.qty.0 * sign);
        }
        let holds = balance >= Qty::ZERO;
        if holds {
            self.record.checks[rule.law.index()] += 1;
            self.record.failing.retain(|&(law, _, subject)| (law, subject) != (rule.law, rule.subject));
        }
        holds
    }

    fn enforce(&mut self, rule: &Rule, ctx: &Context) {
        let held = match self.plan.laws[rule.law.index()].shortcut {
            Some(Shortcut::Cap(cap)) => self.within(rule, ctx, cap),
            Some(Shortcut::FloorOfNothing) => self.afloat(rule),
            None => false,
        };
        if held {
            return;
        }
        let (book, law) = (self.plan.book, &self.plan.book.laws[rule.law]);
        if self.evaluate(rule.law, ctx) {
            self.record.checks[rule.law.index()] += 1;
        }
        let mut outcomes = std::mem::take(&mut self.scratch.outcomes);
        if law.trigger == Trigger::Always {
            self.record.failing.retain(|&(failed_law, step, subject)| {
                (failed_law, subject) != (rule.law, rule.subject)
                    || outcomes.iter().any(
                        |outcome| matches!(outcome, Outcome::Broken { step: failed_step, .. } if *failed_step == step),
                    )
            });
        }
        for outcome in outcomes.drain(..) {
            match outcome {
                Outcome::Count { name, amount } if !ctx.checking => {
                    for (day, part) in by_year(amount, ctx.over).filter(|(_, part)| !part.is_zero()) {
                        self.world.tallies.add(ctx.owner, day.year(), name, part);
                        // What a member's own laws count is a line of the household's year too: the joint return
                        // reads it, and a limit that is the member's own reads only the member's.
                        if let (Subject::Place(_), Some(house)) =
                            (ctx.subject, self.plan.traits.entity(ctx.owner).member)
                        {
                            self.world.tallies.add(house, day.year(), name, part);
                        }
                        self.sample_temporal(ctx.day);
                        let amount = Amount::new(part, book.base);
                        let effect = self.effect(rule, ctx, (day, name, amount), Consequence::Count);
                        self.record.effects.push(effect);
                    }
                }
                Outcome::Owe { name, amount, owed } if !ctx.checking => {
                    let effect = self.effect(rule, ctx, (ctx.over.first(), name, amount), Consequence::Owe(owed));
                    self.record.effects.push(effect);
                }
                Outcome::Count { .. } | Outcome::Owe { .. } => {}
                Outcome::Consume { step, amount } => {
                    self.consume(rule, ctx, step, amount);
                }
                Outcome::Carry { step, amount, unit, within } => {
                    self.carry(rule, ctx, step, amount, unit, within);
                }
                Outcome::Priced { step, name, amount, owed } => self.charge(rule, ctx, step, (name, amount, owed)),
                Outcome::Broken { step, warn } => self.violate(rule, ctx, step, warn),
                Outcome::Faulted { step, fault } => self.fault(rule, ctx, step as usize, fault),
                Outcome::Read { step, counted, limit } => self.read(rule, ctx, step, counted, limit),
            }
        }
        self.scratch.outcomes = outcomes;
    }

    /// What a law recorded for the day it belongs to: the tally line's year,
    /// and the year a report finds an obligation under, is that day's.
    fn effect(
        &self,
        rule: &Rule,
        ctx: &Context,
        (day, name, amount): (Day, Sym, Amount),
        consequence: Consequence,
    ) -> Effect {
        let law = &self.plan.book.laws[rule.law];
        let (law, subject, owner, system, cause) = (rule.law, rule.subject, ctx.owner, law.system, ctx.cause);
        Effect { law, subject, owner, system, day, name, amount, consequence, cause }
    }

    /// The last reading of a limit in its window: updated in place while the
    /// window lasts, and kept apart from the next window's.
    fn read(&mut self, rule: &Rule, ctx: &Context, step: u32, counted: Amount, limit: Amount) {
        let key = (rule.law, step, rule.subject);
        let facts = self.plan.laws[rule.law.index()].steps[step as usize];
        let window = facts.reads.map_or(Days::on(ctx.anchor()), |reads| reads.window(self.plan.book, ctx));
        if let Some(h) = self.record.headroom.get_mut(&key) {
            // The window of a tally is the year of what it counts, not the day a total is read.
            let day = if matches!(facts.reads, Some(Reads::Tally(_))) { ctx.over.first() } else { ctx.anchor() };
            let same_budget_segment = matches!(facts.reads, Some(Reads::Budget(_))) && h.days == window;
            if same_budget_segment || (!matches!(facts.reads, Some(Reads::Budget(_))) && h.days.contains(day)) {
                (h.counted, h.limit, h.day) = (counted, limit, ctx.day);
                return;
            }
        }
        // Only a comparison of amounts in order is read, and it says which way it holds.
        let Some(bound) = facts.bound else { return };
        let headroom = Headroom {
            law: rule.law,
            step,
            subject: rule.subject,
            owner: ctx.owner,
            days: window,
            counted,
            limit,
            day: ctx.day,
            warn: facts.warn,
            bound,
        };
        if let Some(old) = self.record.headroom.insert(key, headroom) {
            self.record.passed.push(old);
        }
    }

    /// Whether a violation raised by this occasion is accepted, and how. A `!`
    /// that accepts something has done its job.
    fn waiver(&mut self, ctx: &Context) -> Option<Waiver> {
        match ctx.motion.and_then(|m| m.waive) {
            Some(waive) => {
                self.record.waivers.insert(waive.loc, true);
                Some(Waiver::Marked(waive))
            }
            None if self.options.relaxed || self.plan.book.relaxed => Some(Waiver::Relaxed),
            None => None,
        }
    }

    /// Records a `require` or `warn` that does not hold. A lasting `always`
    /// condition is recorded when it begins, and a limit once per window.
    fn violate(&mut self, rule: &Rule, ctx: &Context, step: u32, warn: bool) {
        let law = &self.plan.book.laws[rule.law];
        let facts = &self.plan.laws[rule.law.index()];
        let waiver = self.waiver(ctx);
        let fresh = match (law.trigger, facts.steps[step as usize].reads) {
            (Trigger::Always, _) => self.record.failing.insert((rule.law, step, rule.subject)),
            (_, Some(reads)) => {
                self.record.reported.insert((rule.law, step, rule.subject, reads.window(self.plan.book, ctx).first()))
            }
            _ => true,
        };
        if !fresh {
            return;
        }
        let frame =
            Frame { plan: self.plan, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let diagnostic = explain::broken(&frame, step as usize, warn, waiver);
        let verdict = match (waiver, warn) {
            (Some(waiver), _) => Verdict::Waived(waiver),
            (None, true) => Verdict::Warns,
            (None, false) => Verdict::Blocks,
        };
        self.violation(rule, ctx, diagnostic, verdict);
    }

    /// Records a violation with its diagnostic.
    fn violation(&mut self, rule: &Rule, ctx: &Context, diagnostic: Diagnostic, verdict: Verdict) {
        let diagnostic = self.record.report(diagnostic);
        let (law, subject, day, cause) = (rule.law, rule.subject, ctx.day, ctx.cause);
        self.record.violations.push(Violation { law, subject, day, cause, verdict, diagnostic });
    }

    /// A `require … else owe …` that does not hold costs what the law says,
    /// unless a `!` waives it.
    fn charge(&mut self, rule: &Rule, ctx: &Context, step: u32, (name, amount, owed): (Sym, Amount, Owed)) {
        let law = &self.plan.book.laws[rule.law];
        let waive = ctx.motion.and_then(|m| m.waive);
        if let Some(waive) = waive {
            self.record.waivers.insert(waive.loc, true);
        }
        let facts = &self.plan.laws[rule.law.index()];
        let frame =
            Frame { plan: self.plan, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let diagnostic = explain::priced(&frame, step as usize, (name, amount, owed), waive);
        self.violation(rule, ctx, diagnostic, Verdict::Priced { waived: waive.is_some() });
        if waive.is_none() {
            let effect = self.effect(rule, ctx, (ctx.over.first(), name, amount), Consequence::Penalty(owed));
            self.record.effects.push(effect);
        }
    }

    /// A fault reached a step. It is reported once for what was missing (a
    /// price, a property of one thing, a param's rows), however many laws,
    /// steps and flows run into it.
    fn fault(&mut self, rule: &Rule, ctx: &Context, step: usize, fault: Fault) {
        let (book, law) = (self.plan.book, &self.plan.book.laws[rule.law]);
        let facts = &self.plan.laws[rule.law.index()];
        let frame =
            Frame { plan: self.plan, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let origin = explain::first_fault(&frame, step);
        let holder = origin.and_then(|at| frame.holder(at));
        let missing = match fault {
            Fault::NoPrice { unit, quote } => Missing::Price(unit, quote),
            Fault::Unset(name) => Missing::Property(holder, name),
            Fault::NoRow(param) if book.params[param].system.is_some() => Missing::Figures(ctx.over.first().year()),
            Fault::NoRow(param) => Missing::Row(param),
            Fault::UnitMismatch { .. } => Missing::Arithmetic(rule.law, step as u32),
            Fault::InvalidProgram => Missing::Arithmetic(rule.law, step as u32),
            Fault::MissingInput(_) => Missing::Arithmetic(rule.law, step as u32),
            Fault::DivideByZero | Fault::Overflow => Missing::Arithmetic(rule.law, step as u32),
        };
        if self.record.missing.insert(missing) {
            let diagnostic = explain::faulted(&frame, fault, origin, holder);
            self.record.report(diagnostic);
        }
    }
}
