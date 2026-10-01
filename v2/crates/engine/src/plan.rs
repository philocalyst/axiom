//! The plan: everything the fold decides before it begins, once.
//!
//! A book has loose ends. `?` amounts are solved from the assertions around
//! them, settlement events change flows' states, and the laws' static shape
//! (which ones only cap a total, which totals must be counted, when the timed
//! ones fall due) and who contains what can be read off before the first flow.
//! None of it depends on the day the fold stops or on what the fold finds, so
//! a [`Plan`] is built once, never changes, and is shared by reference: every
//! [`Ledger`] borrows it, a fork copies only the world, the clock and the
//! records, and any number of threads can fold from it at once.

use axiom_core::{Day, Diagnostic, Groups, Id, Map, Set, Sym};
use axiom_model::{Book, Commodity, Entity, Flow, Func, Kind, Op, Place, Rule, Subject, Ty, Value};

use crate::bridge::{Sides, V3};
use crate::events::{self, Events};
use crate::facts::{self, LawFacts, Readers};
use crate::ledger::{Ledger, fold, fold_to_view};
use crate::motion::Amounts;
use crate::scope::containing;
use crate::state::World;
use crate::timeline::{self, Schedule};
use crate::totals::Watch;
use crate::{Options, Run, infer};

/// The names the fold and the views look for by spelling, resolved once: what
/// a law reads (`born`), what marks a currency, a loan's term (`maturity`) and
/// the law a `budget` line compiles to. Each is `None` in a book that never
/// mentions it, and is compared as a `Sym` or an id, never as text.
#[derive(Clone, Copy, Debug)]
pub struct Known {
    pub born: Option<Sym>,
    pub maturity: Option<Sym>,
    pub budget: Option<Sym>,
    pub currency: Option<Id<Kind>>,
}

impl Known {
    pub fn of(book: &Book) -> Known {
        let name = |text| book.names.get(text);
        Known {
            born: name("born"),
            maturity: name("maturity"),
            budget: name("budget"),
            currency: book.kind("currency").ok(),
        }
    }
}

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
    pub(crate) known: Known,
    /// The sign each place's balance is shown in.
    pub(crate) sides: Sides,
    /// By law id: what is true of the law whatever runs it.
    pub(crate) laws: Box<[LawFacts]>,
    /// Some list of rules brings one law to one subject twice: `fire` must not run it twice.
    pub(crate) repeats: bool,
    /// The laws to read as a window opens with value already recognized into it.
    pub(crate) readers: Readers,
    /// The subjects whose flow totals some law reads.
    pub(crate) watch: Watch,
    /// The asset places each entity holds: its own, its subsidiaries' and its
    /// members', in place order.
    members: Groups<Entity, Id<Place>>,
    /// Places under kinds read by a widened total. Only law-referenced kinds
    /// are indexed, so books without those reads pay no grouping cost.
    pub(crate) kind_places: Map<Id<Kind>, Box<[Id<Place>]>>,
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
        let sides = Sides::of(book);
        let solution = infer::solve(book, &events, &sides);
        problems.extend(solution.problems);
        let unsolved = solution.unsolved.iter().flat_map(|&id| {
            let flow = &book.flows[id];
            [((flow.from, flow.out.unit), (flow.day, id)), ((flow.to, flow.arrive.unit), (flow.day, id))]
        });
        let mut blocked: Map<_, (Day, Id<Flow>)> = Map::default();
        for (key, first) in unsolved {
            blocked.entry(key).and_modify(|known| *known = (*known).min(first)).or_insert(first);
        }
        let laws: Box<[LawFacts]> = book.laws.values().map(|law| LawFacts::of(book, law)).collect();
        let places = (0..book.places.len() as u32).map(Id::new);
        let held = places.flat_map(|place| {
            containing(book, place).filter_map(move |subject| match subject {
                Subject::Entity(entity) => Some((entity, place)),
                _ => None,
            })
        });
        let kind_places = kind_places(book);
        let mut plan = Plan {
            book,
            amounts: solution.amounts,
            unsolved: blocked,
            problems,
            known: Known::of(book),
            sides,
            repeats: repeats(book),
            readers: facts::readers(book, &laws),
            watch: Watch::of(book, &laws),
            members: Groups::build(book.entities.len(), held),
            kind_places,
            period_start: timeline::start(book, &events),
            last_fact: timeline::last_fact(book, &events),
            events,
            laws,
            timed: Box::default(),
        };
        let (world, mut values) = (World::new(book), Vec::new());
        plan.timed = book.rules.timed.iter().map(|rule| Schedule::of(&plan, rule, &world, &mut values)).collect();
        plan
    }

    pub fn book(&self) -> &'b Book<'s> {
        self.book
    }

    /// The names looked up by spelling, resolved once.
    pub fn known(&self) -> Known {
        self.known
    }

    /// The sign each place's balance is shown in.
    pub fn sides(&self) -> &Sides {
        &self.sides
    }

    /// A ledger at the day before the first fact, ready to fold.
    pub fn start(&self, options: Options) -> Ledger<'_, 'b, 's> {
        Ledger::start(self, options)
    }

    /// The journal folded through `options.today` (and every later journal fact).
    pub fn run(&self, options: Options) -> Run {
        fold(self, options)
    }

    /// [`run`](Plan::run), and with it the ledger as it stood on `options.today`
    /// before that day's closings. A view that asks what a withdrawal would
    /// cost, or what the year would owe, forks that ledger (from any number of
    /// threads) instead of folding the journal again.
    pub fn run_with_view(&self, options: Options) -> (Run, Ledger<'_, 'b, 's>) {
        fold_to_view(self, options)
    }

    /// Whether `place` lies within `subject`: a place's subtree is a stretch of
    /// the pre-order, and an entity's places are listed once.
    pub(crate) fn inside(&self, subject: Subject, place: Id<Place>) -> bool {
        match subject {
            Subject::Place(root) => self.book.places.covers(root, place),
            Subject::Entity(root) => self.members[root].binary_search(&place).is_ok(),
            Subject::Asset(_) => unreachable!("{V3}"),
        }
    }

    /// The asset places an entity holds, in place order.
    pub(crate) fn places_of(&self, entity: Id<Entity>) -> &[Id<Place>] {
        &self.members[entity]
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

/// Whether some list of rules brings one law to one subject twice, as two
/// residences under one system do.
fn repeats(book: &Book) -> bool {
    let rules = &book.rules;
    let per_place = rules.per_place().into_iter().flat_map(|table| table.iter().map(|(_, list)| list));
    let lists = per_place.chain(rules.on_spend.iter().map(|(_, list)| list)).chain([&rules.timed[..]]);
    lists.into_iter().any(|list: &[Rule]| {
        let mut seen = Set::default();
        list.iter().any(|rule| !seen.insert((rule.law, rule.subject)))
    })
}

/// Builds the static place set for every kind a `total` reads. If a typed
/// kind argument is computed at run time, any kind can be selected, so all
/// kinds are indexed for that book.
fn kind_places(book: &Book) -> Map<Id<Kind>, Box<[Id<Place>]>> {
    let mut requested = Set::default();
    let mut dynamic = false;
    for law in book.laws.values() {
        for node in &law.nodes {
            let Op::Call(Func::Total(..), args) = &node.op else { continue };
            for &argument in args.iter().filter(|&&argument| law.nodes[argument.index()].ty == Ty::Kind) {
                match &law.nodes[argument.index()].op {
                    Op::Const(Value::Kind(kind)) => {
                        requested.insert(*kind);
                    }
                    _ => dynamic = true,
                }
            }
        }
    }
    if dynamic {
        // A computed kind may select any bucket; walk each place's ancestry
        // once instead of rescanning the place table for every book kind.
        let mut places: Map<Id<Kind>, Vec<Id<Place>>> =
            book.kinds.ids().map(|kind| (kind, Vec::new())).collect();
        for (place, value) in book.places.iter() {
            for kind in book.kinds.lineage(value.kind) {
                places.get_mut(&kind).expect("every book kind is indexed").push(place);
            }
        }
        return places
            .into_iter()
            .map(|(kind, matching)| (kind, matching.into_boxed_slice()))
            .collect();
    }
    if requested.is_empty() {
        return Map::default();
    }
    requested
        .into_iter()
        .map(|kind| {
            let places = book
                .places
                .iter()
                .filter(|(_, place)| book.is_a(place.kind, kind))
                .map(|(place, _)| place)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            (kind, places)
        })
        .collect()
}

/// The journal folded through `options.today` (and every later journal fact):
/// a plan built to be used once.
pub fn run(book: &Book, options: Options) -> Run {
    Plan::new(book).run(options)
}
