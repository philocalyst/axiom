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

use axiom_core::{Day, Days, Diagnostic, Id, Qty, Sym};
use axiom_model::{Amount, Cap, Entity, Fault, Law, Rule, Subject, Trigger, Window};

use crate::eval::{self, Context, Env, Occasion, Outcome};
use crate::explain::{self, Frame};
use crate::facts::{Reads, Shortcut};
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::plan::Plan;
use crate::scope::owner_of;
use crate::state::Missing;
use crate::totals::by_year;
use crate::{Consequence, Effect, Headroom, Owed, Verdict, Violation, Waiver};

/// Whether the rule is in force for some day of the occasion.
fn applies(plan: &Plan, rule: &Rule, on: &Occasion) -> bool {
    let internal = on.skip_internal
        && on.motion.is_some_and(|m| plan.inside(rule.subject, m.from) && plan.inside(rule.subject, m.to));
    rule.days.overlaps(on.span) && !internal
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
        let (plan, mut done) = (self.plan, std::mem::take(&mut self.scratch.done));
        let book = plan.book;
        done.clear();
        for written in rules {
            // Purpose rules use the flow owner at run time. It must govern
            // filtering, de-duplication, headroom and diagnostics alike.
            let rule = Rule { subject: subject.unwrap_or(written.subject), ..*written };
            if !applies(plan, &rule, &on) {
                continue;
            }
            // A law that two rules bring to one subject runs once.
            let subject = rule.subject;
            if plan.repeats {
                if done.contains(&(rule.law, subject)) {
                    continue;
                }
                done.push((rule.law, subject));
            }
            self.enforce(&rule, &Context::new(subject, owner_of(book, subject), &on));
        }
        self.scratch.done = done;
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
        let rule = &self.plan.book.rules.timed[rule];
        self.fire(std::slice::from_ref(rule), &Occasion::time(day, period));
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
        while let Some((subject, from)) = self.world.totals.reached(&plan.watch, day) {
            for window in [Window::Month, Window::Year] {
                let period = window.around(from);
                let rules = plan.readers.get(&(subject, window));
                if let Some(rules) = rules.filter(|_| period.first() == from) {
                    self.fire(rules, &Occasion::window(from, period));
                }
            }
        }
    }

    pub(crate) fn evaluate(&mut self, law: Id<Law>, ctx: &Context) -> bool {
        let env = Env { plan: self.plan, world: &self.world };
        eval::run(env, &self.plan.book.laws[law], ctx, &mut self.scratch.values, &mut self.scratch.outcomes)
    }

    /// Reads a cap that holds without evaluating it: the total in its window
    /// against the limit is all the law compares. Returns whether it held; a
    /// cap that is broken is evaluated in full, which explains why.
    fn within(&mut self, rule: &Rule, ctx: &Context, cap: Cap) -> bool {
        let read = self.world.totals.read(&self.plan.watch, rule.subject, cap.dir, cap.window, ctx.anchor());
        let counted = Amount::new(read, self.plan.book.base);
        let holds = if cap.strict { counted.qty < cap.limit.qty } else { counted.qty <= cap.limit.qty };
        if holds {
            self.record.checks[rule.law.index()] += 1;
            self.read(rule, ctx, 0, counted, cap.limit);
        }
        holds
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
            if !self.record.failing.is_empty() {
                self.record.failing.remove(&(rule.law, rule.subject));
            }
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
        if law.trigger == Trigger::Always && !outcomes.iter().any(|o| matches!(o, Outcome::Broken { .. })) {
            self.record.failing.remove(&(rule.law, rule.subject));
        }
        for outcome in outcomes.drain(..) {
            match outcome {
                Outcome::Count { name, amount } if !ctx.checking => {
                    for (day, part) in by_year(amount, ctx.over).filter(|(_, part)| !part.is_zero()) {
                        self.world.tallies.add(ctx.owner, day.year(), name, part);
                        // What a member's own laws count is a line of the household's year too: the joint return
                        // reads it, and a limit that is the member's own reads only the member's.
                        if let (Subject::Place(_), Some(house)) = (ctx.subject, book.entities[ctx.owner].member) {
                            self.world.tallies.add(house, day.year(), name, part);
                        }
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
        if let Some(h) = self.record.headroom.get_mut(&key) {
            // The window of a tally is the year of what it counts, not the day a total is read.
            let day = if matches!(facts.reads, Some(Reads::Tally(_))) { ctx.over.first() } else { ctx.anchor() };
            if h.days.contains(day) {
                (h.counted, h.limit, h.day) = (counted, limit, ctx.day);
                return;
            }
        }
        // Only a comparison of amounts in order is read, and it says which way it holds.
        let Some(bound) = facts.bound else { return };
        let window = facts.reads.map_or(Days::on(ctx.anchor()), |reads| reads.window(ctx));
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
        let (book, law) = (self.plan.book, &self.plan.book.laws[rule.law]);
        let facts = &self.plan.laws[rule.law.index()];
        let waiver = self.waiver(ctx);
        let fresh = match (law.trigger, facts.steps[step as usize].reads) {
            (Trigger::Always, _) => self.record.failing.insert((rule.law, rule.subject)),
            (_, Some(reads)) => self.record.reported.insert((rule.law, step, rule.subject, reads.window(ctx).first())),
            _ => true,
        };
        if !fresh {
            return;
        }
        let frame = Frame { book, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
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
        let (book, law) = (self.plan.book, &self.plan.book.laws[rule.law]);
        let waive = ctx.motion.and_then(|m| m.waive);
        if let Some(waive) = waive {
            self.record.waivers.insert(waive.loc, true);
        }
        let facts = &self.plan.laws[rule.law.index()];
        let frame = Frame { book, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
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
        let frame = Frame { book, law, facts, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let origin = explain::first_fault(&frame, step);
        let holder = origin.and_then(|at| frame.holder(at));
        let missing = match fault {
            Fault::NoPrice { unit, quote } => Missing::Price(unit, quote),
            Fault::Unset(name) => Missing::Property(holder, name),
            Fault::NoRow(param) if book.params[param].system.is_some() => Missing::Figures(ctx.over.first().year()),
            Fault::NoRow(param) => Missing::Row(param),
            Fault::DivideByZero | Fault::Overflow => Missing::Arithmetic(rule.law, step as u32),
        };
        if self.record.missing.insert(missing) {
            let diagnostic = explain::faulted(&frame, fault, origin, holder);
            self.record.report(diagnostic);
        }
    }
}
