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
//! crossed the line, and its diagnostic is built only then.

use std::mem::discriminant;

use axiom_core::{Day, Id, Sym};
use axiom_model::{Amount, Book, Dir, Entity, Fault, Func, Law, NodeId, Op, Period, Recognition, Rule, StepKind, Trigger, Window};

use crate::eval::{self, Context, Env, Occasion, Outcome};
use crate::explain::{self, Frame, Waiver};
use crate::ledger::Ledger;
use crate::motion::Motion;
use crate::scope::{inside, owner_of};
use crate::state::Reading;
use crate::totals::{by_year, window_of};
use crate::{Effect, Headroom, Owed, Violation};

/// What a comparison reads, which decides the window its limit lives in: a
/// total in its own window, or a tally in the year.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reads {
    Total(Dir, Window),
    Tally(Sym),
}

impl Reads {
    /// The finest total or tally the condition rooted at `cond` reads: the
    /// window a comparison is about is the shortest it reads.
    pub fn of(law: &Law, cond: NodeId) -> Option<Reads> {
        let read = law.range(cond).filter_map(|at| match law.nodes[at].op {
            Op::Call(Func::Total(dir, window), _) => Some(Reads::Total(dir, window)),
            Op::Call(Func::Tally(name), _) => Some(Reads::Tally(name)),
            _ => None,
        });
        read.min_by_key(|read| match read {
            Reads::Total(_, Window::Month) => 0,
            Reads::Total(_, Window::Year) | Reads::Tally(_) => 1,
            Reads::Total(_, Window::Ever) => 2,
        })
    }

    /// The days the reading covers on this occasion.
    pub fn window(self, on: &Occasion) -> Recognition {
        match self {
            Reads::Total(_, window) => window_of(window, on.anchor()),
            Reads::Tally(_) => window_of(Window::Year, on.over.from),
        }
    }
}

/// What each `require` and `warn` step reads, found once from the law's nodes.
pub(crate) fn reads(book: &Book) -> Vec<((Id<Law>, u32), Reads)> {
    let mut found = Vec::new();
    for (id, law) in book.laws.iter() {
        for (step, kind) in law.steps.iter().enumerate() {
            if let StepKind::Require { cond, .. } = kind.kind {
                found.extend(Reads::of(law, cond).map(|read| ((id, step as u32), read)));
            }
        }
    }
    found
}

/// Whether the rule is in force for some day of the occasion.
fn applies(book: &Book, rule: &Rule, on: &Occasion) -> bool {
    let internal = on.skip_internal
        && on.motion.is_some_and(|m| inside(book, rule.subject, m.from) && inside(book, rule.subject, m.to));
    rule.from <= on.span.until && on.span.from <= rule.until && !internal
}

impl<'b, 's> Ledger<'b, 's> {
    /// Runs every rule in `rules` that applies to this occasion, in order.
    pub(crate) fn fire(&mut self, rules: &[Rule], on: Occasion) {
        let book = self.book;
        for rule in rules.iter().filter(|rule| applies(book, rule, &on)) {
            self.enforce(rule, &Context::new(rule.subject, owner_of(book, rule.subject), &on));
        }
    }

    /// Whether the `on spend` laws of `entity` permit the flow: a dry run that
    /// records nothing. It is the same evaluation the real firing will do.
    pub(crate) fn permits_spend(&mut self, entity: Id<Entity>, m: &Motion) -> bool {
        let book = self.book;
        let on = Occasion { amount: Some(m.out), ..Occasion::flow(m) };
        book.rules.on_spend[entity].iter().filter(|rule| applies(book, rule, &on)).all(|rule| {
            self.evaluate(rule.law, &Context::new(rule.subject, owner_of(book, rule.subject), &on));
            let holds = !self
                .scratch
                .outcomes
                .iter()
                .any(|o| matches!(o, Outcome::Broken { warn: false, .. } | Outcome::Priced { .. } | Outcome::Faulted { .. }));
            self.scratch.outcomes.clear();
            holds
        })
    }

    /// Closes the periods that end on `day`: `each month` always, `each year`
    /// on December 31.
    pub(crate) fn close_period(&mut self, day: Day) {
        let book = self.book;
        for rule in &book.rules.timed {
            let period = match book.laws[rule.law].trigger {
                Trigger::Each(Period::Month, _) => Recognition { from: day.month_start(), until: day },
                Trigger::Each(Period::Year, None) if day == day.year_end() => Recognition { from: day.year_start(), until: day },
                _ => continue,
            };
            self.run_timed(rule, Occasion::time(day, period));
        }
    }

    /// Fires one `by` law whose date the journal has reached, or closes a year
    /// for an `each year closing` law.
    pub(crate) fn deadline(&mut self, at: usize) {
        let due = self.solved.deadlines[at];
        self.run_timed(&self.book.rules.timed[due.rule], Occasion::time(due.day, due.period));
    }

    /// A period's law runs when its rule was in force for some day of the period.
    fn run_timed(&mut self, rule: &Rule, on: Occasion) {
        if applies(self.book, rule, &on) {
            self.enforce(rule, &Context::new(rule.subject, owner_of(self.book, rule.subject), &on));
        }
    }

    pub(crate) fn evaluate(&mut self, law: Id<Law>, ctx: &Context) -> bool {
        let env = Env { book: self.book, world: &self.world };
        eval::run(env, &self.book.laws[law], ctx, &mut self.scratch.values, &mut self.scratch.outcomes)
    }

    fn enforce(&mut self, rule: &Rule, ctx: &Context) {
        let (book, law) = (self.book, &self.book.laws[rule.law]);
        if self.evaluate(rule.law, ctx) {
            self.record.checks[rule.law.index()] += 1;
        }
        let mut outcomes = std::mem::take(&mut self.scratch.outcomes);
        if law.trigger == Trigger::Always && !outcomes.iter().any(|o| matches!(o, Outcome::Broken { .. })) {
            self.record.failing.remove(&(rule.law, rule.subject));
        }
        for outcome in outcomes.drain(..) {
            match outcome {
                Outcome::Count { name, amount } => {
                    for (day, part) in by_year(amount, ctx.over).filter(|(_, part)| !part.is_zero()) {
                        self.world.tallies.add(ctx.owner, day.year(), name, part);
                        let effect = self.effect(rule, ctx, day, name, Amount::new(part, book.base));
                        self.record.effects.push(effect);
                    }
                }
                Outcome::Owe { name, amount, owed } => {
                    let effect = Effect { owe: Some(owed), ..self.effect(rule, ctx, ctx.over.from, name, amount) };
                    self.record.effects.push(effect);
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
    fn effect(&self, rule: &Rule, ctx: &Context, day: Day, name: Sym, amount: Amount) -> Effect {
        let law = &self.book.laws[rule.law];
        Effect { law: rule.law, subject: rule.subject, owner: ctx.owner, system: law.system, day, name, amount, owe: None, cause: ctx.cause, priced: false }
    }

    /// The last reading of a limit in its window: updated in place while the
    /// window lasts, and kept apart from the next window's.
    fn read(&mut self, rule: &Rule, ctx: &Context, step: u32, counted: Amount, limit: Amount) {
        let key = (rule.law, step, rule.subject);
        if let Some(reading) = self.record.headroom.get_mut(&key) {
            let day = if reading.tally { ctx.over.from } else { ctx.anchor() };
            let h = &mut reading.headroom;
            if h.from <= day && day <= h.until {
                (h.counted, h.limit, h.day) = (counted, limit, ctx.day);
                return;
            }
        }
        let warn = matches!(self.book.laws[rule.law].steps[step as usize].kind, StepKind::Require { warn: true, .. });
        let reads = self.solved.reads.get(&(rule.law, step)).copied();
        let window = reads.map_or(Recognition::on(ctx.anchor()), |reads| reads.window(ctx));
        let headroom = Headroom {
            law: rule.law,
            step,
            subject: rule.subject,
            owner: ctx.owner,
            from: window.from,
            until: window.until,
            counted,
            limit,
            day: ctx.day,
            warn,
        };
        let tally = matches!(reads, Some(Reads::Tally(_)));
        if let Some(old) = self.record.headroom.insert(key, Reading { headroom, tally }) {
            self.record.passed.push(old.headroom);
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
            None if self.options.relaxed || self.book.relaxed => Some(Waiver::Relaxed),
            None => None,
        }
    }

    /// Records a `require` or `warn` that does not hold. A lasting `always`
    /// condition is recorded when it begins, and a limit once per window.
    fn violate(&mut self, rule: &Rule, ctx: &Context, step: u32, warn: bool) {
        let (book, law) = (self.book, &self.book.laws[rule.law]);
        let waiver = self.waiver(ctx);
        let fresh = match (law.trigger, self.solved.window(rule.law, step, ctx)) {
            (Trigger::Always, _) => self.record.failing.insert((rule.law, rule.subject)),
            (_, Some(window)) => self.record.reported.insert((rule.law, step, rule.subject, window.from)),
            _ => true,
        };
        if !fresh {
            return;
        }
        let frame = Frame { book, law, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let diagnostic = explain::broken(&frame, step as usize, warn, waiver);
        let diagnostic = self.record.report(diagnostic);
        let (day, subject) = (ctx.day, rule.subject);
        let violation = Violation { law: rule.law, subject, day, cause: ctx.cause, warn, waived: waiver.is_some(), priced: false, diagnostic };
        self.record.violations.push(violation);
    }

    /// A `require … else owe …` that does not hold costs what the law says,
    /// unless a `!` waives it.
    fn charge(&mut self, rule: &Rule, ctx: &Context, step: u32, (name, amount, owed): (Sym, Amount, Owed)) {
        let (book, law) = (self.book, &self.book.laws[rule.law]);
        let waive = ctx.motion.and_then(|m| m.waive);
        if let Some(waive) = waive {
            self.record.waivers.insert(waive.loc, true);
        }
        let frame = Frame { book, law, ctx, values: &self.scratch.values, effects: &self.record.effects };
        let diagnostic = explain::priced(&frame, step as usize, (name, amount, owed), waive);
        let diagnostic = self.record.report(diagnostic);
        let (day, subject) = (ctx.day, rule.subject);
        let violation =
            Violation { law: rule.law, subject, day, cause: ctx.cause, warn: false, waived: waive.is_some(), priced: true, diagnostic };
        self.record.violations.push(violation);
        if waive.is_none() {
            let effect = Effect { owe: Some(owed), priced: true, ..self.effect(rule, ctx, ctx.over.from, name, amount) };
            self.record.effects.push(effect);
        }
    }

    /// A fault reached a step. One report per law, step and kind of fault: a
    /// missing price would otherwise repeat on every flow.
    fn fault(&mut self, rule: &Rule, ctx: &Context, step: usize, fault: Fault) {
        if !self.record.faulted.insert((rule.law, step as u32, discriminant(&fault))) {
            return;
        }
        let frame = Frame { book: self.book, law: &self.book.laws[rule.law], ctx, values: &self.scratch.values, effects: &self.record.effects };
        let diagnostic = explain::faulted(&frame, step, fault);
        self.record.report(diagnostic);
    }
}
