//! The plan: everything the fold decides before it begins, once.
//!
//! A book has loose ends. `?` amounts are solved from the assertions around
//! them, settlement events change flows' states, and the laws' static shape (which
//! ones only cap a total, which totals must be counted, when the timed ones
//! fall due) can be read off before the first flow. None of it depends on the
//! day the fold stops or on what the fold finds, so a [`Plan`] is built once,
//! never changes, and is shared by reference: every [`Ledger`] borrows it, a
//! fork copies only the world, the clock and the records, and any number of
//! threads can fold from it at once.

use axiom_core::{Day, Diagnostic, Id, Map};
use axiom_model::{Book, Cap, Commodity, Flow, Place};

use crate::events::{self, Events};
use crate::fire::{self, Readers};
use crate::ledger::{Ledger, fold};
use crate::motion::Amounts;
use crate::state::World;
use crate::timeline::{self, Schedule};
use crate::totals::Watch;
use crate::{Options, Run, infer};

/// What the solve pass decided about a book. Immutable and `Sync`.
pub struct Plan<'b, 's> {
    pub(crate) book: &'b Book<'s>,
    pub(crate) events: Events,
    /// The quantities of the flows written `? USD`, as far as the assertions
    /// around them could solve them. Flows the fold resolves (`=`, `all`) are
    /// the ledger's to remember, for they depend on the balance.
    pub(crate) amounts: Map<Id<Flow>, Amounts>,
    /// The first day of each place and commodity whose balance depends on an
    /// amount that could not be solved, and the flow to blame.
    pub(crate) unsolved: Map<(Id<Place>, Id<Commodity>), (Day, Id<Flow>)>,
    /// What reading the events and solving reported: every ledger starts with them.
    problems: Vec<Diagnostic>,
    /// Some list of rules brings one law to one subject twice: `fire` must not run it twice.
    pub(crate) repeats: bool,
    /// By law id: the laws that are one cap on a total in the base currency.
    pub(crate) caps: Vec<Option<Cap>>,
    /// The laws to read as a window opens with value already recognized into it.
    pub(crate) readers: Readers,
    /// The subjects whose flow totals some law reads.
    pub(crate) watch: Watch,
    /// The day of the first fact that starts a period; before it there is nothing to close.
    pub(crate) period_start: Option<Day>,
    last_fact: Option<Day>,
    /// By index in `Rules::timed`: when each falls due.
    pub(crate) timed: Box<[Schedule]>,
}

impl<'b, 's> Plan<'b, 's> {
    /// Solves what the journal leaves open (`?` amounts, settlement events;
    /// `=` targets and `all` wait for the fold, which knows the balance) and
    /// works out what the laws need before the first flow.
    pub fn new(book: &'b Book<'s>) -> Plan<'b, 's> {
        let (events, mut problems) = events::read(book);
        let solution = infer::solve(book, &events);
        problems.extend(solution.problems);
        let unsolved = solution.unsolved.iter().flat_map(|&id| {
            let flow = &book.flows[id];
            [((flow.from, flow.out.unit), (flow.day, id)), ((flow.to, flow.arrive.unit), (flow.day, id))]
        });
        let mut blocked: Map<_, (Day, Id<Flow>)> = Map::default();
        for (key, first) in unsolved {
            blocked.entry(key).and_modify(|known| *known = (*known).min(first)).or_insert(first);
        }
        let mut plan = Plan {
            book,
            amounts: solution.amounts,
            unsolved: blocked,
            problems,
            repeats: fire::repeats(book),
            caps: fire::caps(book),
            readers: fire::readers(book),
            watch: Watch::of(book),
            period_start: timeline::start(book, &events),
            last_fact: timeline::last_fact(book, &events),
            events,
            timed: Box::default(),
        };
        let (world, mut values) = (World::new(book), Vec::new());
        plan.timed = book.rules.timed.iter().map(|rule| Schedule::of(&plan, rule, &world, &mut values)).collect();
        plan
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// A ledger at the day before the first fact, ready to fold.
    pub fn start(&self, options: Options) -> Ledger<'_, 'b, 's> {
        Ledger::start(self, options)
    }

    /// The journal folded through `options.today` (and every later journal fact).
    pub fn run(&self, options: Options) -> Run {
        fold(self, options)
    }

    /// The last day the fold reaches when it is run for `today`: `today`, or
    /// the journal's last fact if that is later.
    pub(crate) fn horizon(&self, today: Day) -> Day {
        self.last_fact.map_or(today, |last| last.max(today))
    }

    /// The problems solving found, for a ledger's record to begin with.
    pub(crate) fn problems(&self) -> Vec<Diagnostic> {
        self.problems.clone()
    }
}

/// The journal folded through `options.today` (and every later journal fact):
/// a plan built to be used once.
pub fn run(book: &Book, options: Options) -> Run {
    Plan::new(book).run(options)
}

/// A plan is shared by every thread that folds from it.
const _: () = {
    const fn is_sync<T: Sync>() {}
    is_sync::<Plan<'static, 'static>>();
    is_sync::<Run>();
};
