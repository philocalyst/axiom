//! The state machine.
//!
//! A [`Ledger`] is the fold's position: a clock of cursors into the timeline,
//! the [`World`] holdings/totals/tallies, and the [`Record`] of what happened.
//! Advancing it consumes moments in their total order; applying a flow runs a
//! fact the journal does not hold through the same `post` the journal's own
//! flows take.
//!
//! A clone copies what the future depends on (the world, five timeline cursors,
//! the small solved tables) and the records so far: flat vectors, so a clone is
//! a handful of memory copies, never a replay. Nothing is kept per journal flow:
//! the solve pass remembers only the flows an event or a `?` touched, and the
//! timeline is cursors into the book's own tables.

use axiom_core::{Day, Id, Qty};
use axiom_model::{Book, Commodity, End, Flow, Infer, Place, Trigger};

use crate::eval::Env;
use crate::events::{self, Events};
use crate::motion::{Amounts, Motion};
use crate::scope::display;
use crate::state::{Record, Scratch, World};
use crate::timeline::{self, Deadline, Fact, Moment, Sources, Timeline};
use crate::{Applied, Cause, Holding, Options, Posted, Run, State, explain, infer, relief};

/// The book's state as of some day. Cheap to clone relative to a replay.
#[derive(Clone)]
pub struct Ledger<'b, 's> {
    pub(crate) book: &'b Book<'s>,
    pub(crate) options: Options,
    pub(crate) solved: Solved,
    clock: Clock,
    pub(crate) world: World,
    pub(crate) record: Record,
    pub(crate) scratch: Scratch,
}

/// What the solve pass decided before the fold began. Never changes.
#[derive(Clone)]
pub(crate) struct Solved {
    pub events: Events,
    pub deadlines: Vec<Deadline>,
    /// Periods and deadlines fire up to here.
    pub horizon: Day,
}

impl Solved {
    pub fn sources<'a>(&'a self, book: &'a Book<'a>) -> Sources<'a> {
        Sources { book, events: &self.events, deadlines: &self.deadlines, horizon: self.horizon }
    }
}

#[derive(Clone)]
struct Clock {
    day: Day,
    timeline: Timeline,
    /// How many flows `apply` has taken.
    applied: u32,
}

impl<'b, 's> Ledger<'b, 's> {
    /// Solves what the journal leaves open (`?` amounts, settlement events;
    /// `=` targets and `all` wait for the fold, which knows the balance) and
    /// stands at the day before the first fact.
    pub fn new(book: &'b Book<'s>, options: Options) -> Ledger<'b, 's> {
        let (events, mut diagnostics) = events::read(book);
        let (amounts, problems) = infer::solve(book, &events);
        diagnostics.extend(problems);
        let world = World::new(book);
        let mut scratch = Scratch::default();
        let horizon = timeline::horizon(book, &events, options.today);
        let deadlines = timeline::deadlines(Env { book, world: &world }, horizon, &mut scratch.values);
        let solved = Solved { events, deadlines, horizon };
        let periodic = book.rules.timed.iter().any(|rule| matches!(book.laws[rule.law].trigger, Trigger::Each(..)));
        let timeline = Timeline::new(&solved.sources(book), periodic);
        let day = timeline.peek().map_or(Day::default(), |first| first.day.add_days(-1));
        Ledger {
            book,
            options,
            solved,
            clock: Clock { day, timeline, applied: 0 },
            world,
            record: Record::new(book, amounts, diagnostics),
            scratch,
        }
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// A copy to drive further with flows the journal does not hold, like
    /// `clone`, but forgetting the records so far: what the fork's
    /// [`finish`](Ledger::finish) reports is only what the fork itself caused,
    /// so a hypothetical withdrawal's gains, taxes and violations are not mixed
    /// with the journal's. Costs the state, not the history.
    pub fn fork(&self) -> Ledger<'b, 's> {
        Ledger {
            book: self.book,
            options: self.options,
            solved: self.solved.clone(),
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

    /// Folds the journal's facts, and the deadlines and period ends that fall
    /// due, through the end of `day`.
    pub fn advance(&mut self, day: Day) {
        self.advance_through(Moment::end_of(day));
        self.clock.day = self.clock.day.max(day);
    }

    /// Advances to `flow.day`, then applies a flow the journal does not hold
    /// (planned or hypothetical) exactly as if it did: relief, gains, laws.
    /// Returns what the flow caused, not what the journal did on the way.
    ///
    /// The flow takes its place after the journal's own flows of its day and
    /// before that day's assertions. A flow dated before the ledger's day is
    /// applied on the ledger's day: the fold does not travel back. Its `mode`
    /// is ignored, since applying is what makes it real.
    pub fn apply(&mut self, flow: &Flow) -> Applied {
        let day = flow.day.max(self.clock.day);
        self.advance_through(Moment::after_flows(day));
        self.clock.day = day;
        let marks = self.record.marks();
        let number = self.clock.applied;
        self.clock.applied += 1;
        let amounts = self.amounts(flow, None);
        self.post(&Motion::new(flow, Cause::Applied(number), day, amounts));
        self.record.since(marks)
    }

    /// What `place` alone holds of `unit`, in quanta.
    pub fn balance(&self, place: Id<Place>, unit: Id<Commodity>) -> Qty {
        self.world.holdings.qty(place, unit)
    }

    /// Every non-empty holding, by place then commodity.
    pub fn holdings(&self) -> impl Iterator<Item = &Holding> {
        self.world.holdings.iter().filter(|holding| !holding.is_empty())
    }

    /// Stops and hands over everything recorded along the way.
    pub fn finish(self) -> Run {
        let Ledger { book, options, solved, world, record, .. } = self;
        let posted = book.flows.iter().map(|(id, flow)| {
            let amounts = record.amounts.get(&id).copied().unwrap_or_else(|| Amounts::written(flow));
            Posted { out: amounts.out, arrive: amounts.arrive, state: solved.events.state(id, flow) }
        });
        Run {
            today: options.today,
            posted: posted.collect(),
            holdings: world.holdings.into_sorted(),
            gains: record.gains,
            effects: record.effects,
            violations: record.violations,
            headroom: Vec::new(),
            pads: record.pads,
            checks: record.checks.into(),
            diagnostics: record.diagnostics,
        }
    }

    /// Consumes every moment up to and including `limit`.
    fn advance_through(&mut self, limit: Moment) {
        loop {
            let Some(moment) = self.clock.timeline.peek().filter(|&moment| moment <= limit) else { return };
            self.clock.timeline.consume(moment, &self.solved.sources(self.book));
            self.clock.day = moment.day;
            self.step(moment);
        }
    }

    fn step(&mut self, moment: Moment) {
        match moment.fact {
            Fact::Flow(id) => {
                let motion = self.journal_motion(id, moment.day);
                self.post(&motion);
            }
            // A settlement lands a pending flow; a return runs an actual one backwards.
            Fact::Settle(id) => {
                let motion = self.journal_motion(id, moment.day);
                let returned = matches!(self.solved.events.state(id, &self.book.flows[id]), State::Returned(_));
                self.post(&if returned { motion.reversed() } else { motion });
            }
            Fact::Assert(index) => self.reconcile(index as usize),
            Fact::Period => self.close_period(moment.day),
            Fact::Deadline(at) => self.deadline(at as usize),
        }
    }

    /// A journal flow as it moves on `day`, its quantities solved.
    fn journal_motion(&mut self, id: Id<Flow>, day: Day) -> Motion<'b> {
        let book: &'b Book<'s> = self.book;
        let flow = &book.flows[id];
        let amounts = self.amounts(flow, Some(id));
        Motion::new(flow, Cause::Flow(id), day, amounts)
    }

    /// A flow's quantities. `?` amounts were solved before the fold; `=` and
    /// `all` depend on the balance and are resolved now, once, and remembered
    /// (a reversal must undo exactly what was done).
    fn amounts(&mut self, flow: &Flow, id: Option<Id<Flow>>) -> Amounts {
        if let Some(&done) = id.and_then(|id| self.record.amounts.get(&id)) {
            return done;
        }
        let written = Amounts::written(flow);
        let resolved = match flow.infer {
            Infer::Known | Infer::Unknown => return written,
            Infer::All => self.everything(flow, written),
            Infer::Target { end, balance } => self.resolve_target(flow, end, balance, written),
        };
        if let Some(id) = id {
            self.record.amounts.insert(id, resolved);
        }
        resolved
    }

    /// `all`: everything the selected parcels at the source hold.
    fn everything(&self, flow: &Flow, written: Amounts) -> Amounts {
        let book = self.book;
        let holding = self.world.holdings.get(flow.from, flow.out.unit);
        let qty = if book.places[flow.from].class.holds_parcels() {
            let is_base = flow.out.unit == book.base;
            holding.map_or(Qty::ZERO, |h| relief::admitted(h, is_base, &flow.select, &book.txns))
        } else {
            holding.map_or(Qty::ZERO, |h| h.plain.max(Qty::ZERO))
        };
        Amounts { out: qty, arrive: if flow.is_exchange() { written.arrive } else { qty } }
    }

    /// `= 5_000 USD`: whatever leaves the source, or arrives at the target,
    /// so that its place holds `balance` afterwards.
    fn resolve_target(&mut self, flow: &Flow, end: End, balance: Qty, written: Amounts) -> Amounts {
        let (place, unit) = match end {
            End::From => (flow.from, flow.out.unit),
            End::To => (flow.to, flow.arrive.unit),
        };
        // The target is written in the place's display sign.
        let (held, target) = (self.world.holdings.qty(place, unit), display(self.book, place, balance));
        let gap = if end == End::From { held - target } else { target - held };
        let qty = if gap.is_negative() {
            let shown = (display(self.book, place, held), balance);
            self.record.report(explain::past_target(self.book, flow, place, unit, shown, end));
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

/// The journal folded through `options.today` (and every later journal fact).
pub fn run(book: &Book, options: Options) -> Run {
    let mut ledger = Ledger::new(book, options);
    ledger.advance(options.today);
    ledger.advance_through(Moment::LAST);
    ledger.finish()
}
