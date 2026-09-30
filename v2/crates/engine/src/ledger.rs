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

use axiom_core::{Day, Diagnostic, Id, Map, Qty};
use axiom_model::{Book, Cap, Commodity, End, Flow, Infer, Place};

use crate::eval::Env;
use crate::events::{self, Events};
use crate::fire::{self, Readers};
use crate::motion::{Amounts, Motion};
use crate::scope::{display, is_money};
use crate::state::{Record, Scratch, World};
use crate::timeline::{self, Deadline, Fact, Moment, Sources, Timeline};
use crate::{Applied, Cause, Holding, Options, Posted, Recorded, Run, State, explain, infer};

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
    /// The last day the fold reaches ([`Run::horizon`]).
    pub horizon: Day,
    pub deadlines: Vec<Deadline>,
    /// Some list of rules brings one law to one subject twice: `fire` must not run it twice.
    pub repeats: bool,
    /// By law id: the laws that are one cap on a total in the base currency.
    pub caps: Vec<Option<Cap>>,
    /// The laws to read as a window opens with value already recognized into it.
    pub readers: Readers,
    /// The first day of each place and commodity whose balance depends on an
    /// amount that could not be solved, and the flow to blame.
    pub unsolved: Map<(Id<Place>, Id<Commodity>), (Day, Id<Flow>)>,
}

impl Solved {
    pub fn sources<'a>(&'a self, book: &'a Book<'a>) -> Sources<'a> {
        Sources { book, events: &self.events, deadlines: &self.deadlines }
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
        let solution = infer::solve(book, &events);
        diagnostics.extend(solution.problems);
        let world = World::new(book);
        let mut scratch = Scratch::default();
        let horizon = timeline::horizon(book, &events, options.today);
        let start = timeline::start(book, &events);
        let deadlines = timeline::deadlines(Env { book, world: &world }, horizon, start, &mut scratch.values);
        let unsolved = solution.unsolved.iter().flat_map(|&id| {
            let flow = &book.flows[id];
            [((flow.from, flow.out.unit), (flow.day, id)), ((flow.to, flow.arrive.unit), (flow.day, id))]
        });
        let mut blocked: Map<_, (Day, Id<Flow>)> = Map::default();
        for (key, first) in unsolved {
            blocked.entry(key).and_modify(|known| *known = (*known).min(first)).or_insert(first);
        }
        let (repeats, caps, readers) = (fire::repeats(book), fire::caps(book), fire::readers(book));
        let solved = Solved { events, horizon, deadlines, repeats, caps, readers, unsolved: blocked };
        let timeline = Timeline::new(&solved.sources(book));
        let day = timeline.peek().map_or(Day::default(), |first| first.day.add_days(-1));
        Ledger {
            book,
            options,
            solved,
            clock: Clock { day, timeline, applied: 0 },
            world,
            record: Record::new(book, solution.amounts, diagnostics),
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
        self.fold_through(day, Moment::end_of(day));
    }

    /// Folds what the journal holds through `day`, its flows and assertions,
    /// and stops before the deadlines and period ends of that day: a month or
    /// a year that ends on it is not closed yet. What is applied on `day`
    /// next is a fact of that day and comes before them, as a journal flow of
    /// that day does; [`advance`](Ledger::advance) closes the day afterwards.
    /// The holdings are the same as at the end of the day, since a closing
    /// counts and owes and moves nothing.
    pub fn advance_to_closing(&mut self, day: Day) {
        self.fold_through(day, Moment::before_closings(day));
    }

    fn fold_through(&mut self, day: Day, limit: Moment) {
        self.advance_through(limit);
        self.clock.day = self.clock.day.max(day);
        self.enter(day);
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
        let day = flow.day.max(self.clock.day);
        self.advance_through(Moment::after_flows(day));
        self.clock.day = day;
        self.enter(day);
        let marks = self.record.marks();
        let number = self.clock.applied;
        self.clock.applied += 1;
        let amounts = self.amounts(flow, None);
        self.post(&Motion::new(self.book, flow, Cause::Applied(number), day, amounts));
        self.world.holdings.tidy();
        self.record.since(marks)
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
            violations: &record.violations,
            diagnostics: &record.diagnostics,
        }
    }

    /// Stops and hands over everything recorded along the way: the claims
    /// still open past their day and the waivers that waived nothing are
    /// reported now, when it is known they stayed so.
    pub fn finish(mut self) -> Run {
        self.world.holdings.tidy();
        let (book, today) = (self.book, self.options.today);
        let overdue = self.world.holdings.iter().filter(|slot| book.places[slot.place].claim).flat_map(|slot| {
            let claims = slot.holding.lots.iter().filter(|lot| lot.qty > Qty::ZERO);
            claims.filter_map(move |lot| explain::overdue(book, slot.place, slot.unit, lot, today))
        });
        let mut unused: Vec<_> = self.record.waivers.iter().filter(|&(_, &used)| !used).map(|(&loc, _)| loc).collect();
        unused.sort_unstable();
        let reports: Vec<Diagnostic> = overdue.chain(unused.into_iter().map(explain::unused_waiver)).collect();
        self.record.diagnostics.extend(reports);
        let mut headroom = std::mem::take(&mut self.record.passed);
        headroom.extend(self.record.headroom.drain().map(|(_, reading)| reading.headroom));
        headroom.sort_unstable_by_key(|h| (h.law, h.step, crate::show::subject_key(h.subject), h.days.first()));
        let Ledger { book, options, solved, world, record, .. } = self;
        let posted = book.flows.iter().map(|(id, flow)| {
            let amounts = record.amounts.get(&id).copied().unwrap_or_else(|| Amounts::written(flow));
            Posted { out: amounts.out, arrive: amounts.arrive, state: solved.events.state(id, flow) }
        });
        Run {
            today: options.today,
            horizon: solved.horizon,
            posted: posted.collect(),
            holdings: world.holdings.into_sorted(),
            gains: record.gains,
            effects: record.effects,
            violations: record.violations,
            headroom,
            pads: record.pads,
            // v3 bridge: the v3 fold keeps no assets, promises or adjustments.
            assets: Vec::new(),
            promises: Vec::new(),
            adjustments: Vec::new(),
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
            self.enter(moment.day);
            self.step(moment);
        }
    }

    fn step(&mut self, moment: Moment) {
        match moment.fact {
            Fact::Split(at) => {
                let split = self.book.splits[at as usize];
                self.world.holdings.scale(split.unit, split.ratio);
            }
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
            Fact::Deadline(at) => self.deadline(at as usize),
        }
    }

    /// A journal flow as it moves on `day`, its quantities solved.
    fn journal_motion(&mut self, id: Id<Flow>, day: Day) -> Motion<'b> {
        let book: &'b Book<'s> = self.book;
        let flow = &book.flows[id];
        let amounts = self.amounts(flow, Some(id));
        Motion::new(book, flow, Cause::Flow(id), day, amounts)
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
        let slot = self.world.holdings.get(flow.from, flow.out.unit);
        let qty = if book.places[flow.from].class.holds_parcels() {
            let money = is_money(book, flow.from, flow.out.unit);
            slot.map_or(Qty::ZERO, |slot| slot.admitted(money, &flow.select, &book.txns))
        } else {
            slot.map_or(Qty::ZERO, |slot| slot.plain.max(Qty::ZERO))
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
