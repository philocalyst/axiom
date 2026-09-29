//! Firing laws: which rules run for a moment, and what becomes of what they
//! find.
//!
//! The book's `Rules` already say which laws watch which place, in order, dated
//! by residence. This module filters them by day, binds a law's variables
//! ([`Firing`]), evaluates, and turns [`Outcome`]s into records: tallies and
//! obligations, violations with their diagnostics, faults as their own errors.

use std::mem::discriminant;

use axiom_core::{Day, Id};
use axiom_model::{Amount, Book, Entity, Fault, Law, Period, Rule, Trigger};

use crate::eval::{self, Context, Env, Outcome, Realized};
use crate::explain::{self, Frame, Waiver};
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::scope::{inside, owner_of};
use crate::{Cause, Effect, Violation};

/// What one firing binds `amount`, `gain` and the rest to.
#[derive(Clone, Copy)]
pub(crate) struct Firing<'a> {
    day: Day,
    cause: Cause,
    motion: Option<&'a Motion<'a>>,
    amount: Option<Amount>,
    realized: Option<Realized>,
    skip_internal: bool,
}

impl<'a> Firing<'a> {
    /// A flow fires its laws.
    pub fn flow(motion: &'a Motion<'a>) -> Firing<'a> {
        Firing {
            day: motion.day,
            cause: motion.cause,
            motion: Some(motion),
            amount: None,
            realized: None,
            skip_internal: false,
        }
    }

    /// A period ending or a deadline passing fires its laws.
    pub fn time(day: Day) -> Firing<'static> {
        Firing { day, cause: Cause::Time, motion: None, amount: None, realized: None, skip_internal: false }
    }

    /// `amount` is what the trigger says is moving.
    pub fn moving(self, amount: Amount) -> Firing<'a> {
        Firing { amount: Some(amount), ..self }
    }

    pub fn realizing(self, realized: Realized) -> Firing<'a> {
        Firing { realized: Some(realized), ..self }
    }

    /// Rules whose subject contains both ends of the flow do not fire: value
    /// moved around inside the subject neither entered nor left it.
    pub fn skipping_internal(self) -> Firing<'a> {
        Firing { skip_internal: true, ..self }
    }

    fn applies(&self, book: &Book, rule: &Rule) -> bool {
        let internal = self.skip_internal
            && self.motion.is_some_and(|m| inside(book, rule.subject, m.from) && inside(book, rule.subject, m.to));
        (rule.from..=rule.until).contains(&self.day) && !internal
    }

    fn context(&self, book: &Book, rule: &Rule) -> Context<'a> {
        let owner = owner_of(book, rule.subject);
        Context {
            day: self.day,
            subject: rule.subject,
            owner,
            motion: self.motion,
            amount: self.amount,
            realized: self.realized,
        }
    }
}

impl<'b, 's> Ledger<'b, 's> {
    /// Runs every rule in `rules` that applies to this firing, in order.
    pub(crate) fn fire(&mut self, rules: &[Rule], firing: Firing) {
        let book = self.book;
        for rule in rules.iter().filter(|rule| firing.applies(book, rule)) {
            let ctx = firing.context(book, rule);
            self.enforce(rule, &ctx, firing.cause);
        }
    }

    /// Whether the `on spend` laws of `entity` permit the flow: a dry run that
    /// records nothing. It is the same evaluation the real firing will do.
    pub(crate) fn permits_spend(&mut self, entity: Id<Entity>, m: &Motion) -> bool {
        let book = self.book;
        let firing = Firing::flow(m).moving(m.out);
        book.rules.on_spend[entity].iter().filter(|rule| firing.applies(book, rule)).all(|rule| {
            self.evaluate(rule.law, &firing.context(book, rule));
            let holds = !self
                .scratch
                .outcomes
                .iter()
                .any(|o| matches!(o, Outcome::Broken { warn: false, .. } | Outcome::Faulted { .. }));
            self.scratch.outcomes.clear();
            holds
        })
    }

    /// Closes the periods that end on `day`: `each month` always, `each year`
    /// on December 31.
    pub(crate) fn close_period(&mut self, day: Day) {
        let book = self.book;
        let ends_year = day == day.year_end();
        for rule in &book.rules.timed {
            let due = match book.laws[rule.law].trigger {
                Trigger::Each(Period::Month) => true,
                Trigger::Each(Period::Year) => ends_year,
                _ => false,
            };
            let firing = Firing::time(day);
            if due && firing.applies(book, rule) {
                self.enforce(rule, &firing.context(book, rule), Cause::Time);
            }
        }
    }

    /// Fires one `by` rule whose date the journal has reached.
    pub(crate) fn deadline(&mut self, at: usize) {
        let book = self.book;
        let due = self.solved.deadlines[at];
        let rule = &book.rules.timed[due.rule];
        let firing = Firing::time(due.day);
        self.enforce(rule, &firing.context(book, rule), Cause::Time);
    }

    fn evaluate(&mut self, law: Id<Law>, ctx: &Context) -> bool {
        let env = Env { book: self.book, world: &self.world };
        eval::run(env, &self.book.laws[law], ctx, &mut self.scratch.values, &mut self.scratch.outcomes)
    }

    fn enforce(&mut self, rule: &Rule, ctx: &Context, cause: Cause) {
        let (book, law) = (self.book, &self.book.laws[rule.law]);
        let effect = |name, amount, owe| Effect {
            law: rule.law,
            subject: rule.subject,
            owner: ctx.owner,
            system: law.system,
            day: ctx.day,
            name,
            amount,
            owe,
            cause,
        };
        if self.evaluate(rule.law, ctx) {
            self.record.checks[rule.law.index()] += 1;
        }
        if law.trigger == Trigger::Always && !self.scratch.outcomes.iter().any(|o| matches!(o, Outcome::Broken { .. }))
        {
            self.record.failing.remove(&(rule.law, rule.subject));
        }
        let mut outcomes = std::mem::take(&mut self.scratch.outcomes);
        for outcome in outcomes.drain(..) {
            match outcome {
                Outcome::Count { name, amount } => {
                    self.world.tallies.add(ctx.owner, ctx.day.year(), name, amount);
                    self.record.effects.push(effect(name, Amount::new(amount, book.base), None));
                }
                Outcome::Owe { name, amount, owed } => self.record.effects.push(effect(name, amount, Some(owed))),
                Outcome::Broken { step, warn } => self.violate(rule, ctx, cause, step as usize, warn),
                Outcome::Faulted { step, fault } => self.fault(rule, ctx, step as usize, fault),
            }
        }
        self.scratch.outcomes = outcomes;
    }

    /// Records a `require` or `warn` that does not hold. A lasting `always`
    /// condition is recorded when it begins, not after every flow.
    fn violate(&mut self, rule: &Rule, ctx: &Context, cause: Cause, step: usize, warn: bool) {
        let book = self.book;
        let law = &book.laws[rule.law];
        if law.trigger == Trigger::Always && !self.record.failing.insert((rule.law, rule.subject)) {
            return;
        }
        let marked = ctx.motion.and_then(|m| book.txns.get(m.txn)).and_then(|txn| txn.waive);
        let waiver = match marked {
            Some(waive) => Some(Waiver::Marked(waive)),
            None if self.options.relaxed || book.relaxed => Some(Waiver::Relaxed),
            None => None,
        };
        let frame = Frame { book, law, ctx, values: &self.scratch.values };
        let diagnostic = self.record.report(explain::broken(&frame, step, warn, waiver));
        let (day, subject) = (ctx.day, rule.subject);
        self.record.violations.push(Violation {
            law: rule.law,
            subject,
            day,
            cause,
            warn,
            waived: waiver.is_some(),
            diagnostic,
        });
    }

    /// A fault reached a step. One report per law, step and kind of fault: a
    /// missing price would otherwise repeat on every flow.
    fn fault(&mut self, rule: &Rule, ctx: &Context, step: usize, fault: Fault) {
        if !self.record.faulted.insert((rule.law, step as u32, discriminant(&fault))) {
            return;
        }
        let frame = Frame { book: self.book, law: &self.book.laws[rule.law], ctx, values: &self.scratch.values };
        self.record.report(explain::faulted(&frame, step, fault));
    }
}
