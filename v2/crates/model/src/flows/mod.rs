//! The journal: transactions elaborated into flows, and everything else that
//! is written down about them.
//!
//! Every dated item is independent of the others, so runs of consecutive items
//! are elaborated on every core against the read-only world, each into its own
//! dense vectors, its [`Sink`]. The runs are laid end to end, in day order and
//! declaration order deciding ties, as each finishes, so that laying them out
//! overlaps with elaborating the rest; the book learns which flows touch which
//! places as they go.

mod faults;
mod forms;
mod moves;
mod pairing;
mod records;
mod shape;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, FileId, Groups, Id, Loc, Map, Sym, par};
use axiom_syntax::ItemKind;

use self::forms::Plans;
use self::records::RawEvent;
use self::shape::Elab;
use crate::book::Place;
use crate::collect::{Entry, Run};
use crate::declare::World;
use crate::errors::{Word, unknown};
use crate::journal::{Assert, Event, Flow, Mode, Plan, Prices, Quote, Split, Txn};
use crate::layout::{Misfiled, dated};
use crate::resolve::Cause;
use crate::sources::Site;

/// Where a worker leaves what it elaborates: its transactions, their flows,
/// everything else the journal records, and what went wrong. A transaction's
/// flows are the run `first .. first + len` of `flows`, counted from the start
/// of this sink; the merge makes them global.
#[derive(Default)]
pub(crate) struct Sink<'s> {
    pub txns: Vec<Txn>,
    pub flows: Vec<Flow>,
    pub asserts: Vec<Assert>,
    pub events: Vec<RawEvent<'s>>,
    pub quotes: Vec<Quote>,
    pub splits: Vec<Split>,
    /// Names that named nothing usable, each explained once whatever the count.
    pub misses: Vec<(Cause, &'s str, Loc)>,
    pub misfiled: Vec<Misfiled>,
    pub diags: Vec<Diagnostic>,
    /// Some transaction is dated before the one written before it.
    pub unordered: bool,
}

impl Sink<'_> {
    /// Room for `items` items, most of which are one flow.
    fn with_room_for(items: usize) -> Sink<'static> {
        Sink { txns: Vec::with_capacity(items), flows: Vec::with_capacity(items + items / 8), ..Sink::default() }
    }
}

pub(crate) fn record<'a, 's>(
    world: &mut World<'s>,
    sites: &[Site<'a, 's>],
    entries: &[Entry<'a, 's>],
    journal_runs: &[(Run<'a, 's>, u32)],
    txns: u32,
    layout_free: bool,
    diags: &mut Vec<Diagnostic>,
) {
    records::code_rules(world, entries, diags);
    world.book.syncs = records::syncs(world, entries);
    let mut misses = Vec::new();
    let (plans, plan_txns) = build_plans(world, entries, txns, &mut misses, diags);

    let shared: &World = world;
    let mut journal = Journal::new(txns as usize, misses);
    par::map_each_ordered(
        journal_runs,
        |(run, first)| elaborate(shared, run, *first, &plans, layout_free),
        |sink| journal.take(sink),
    );
    diags.append(&mut journal.diags);
    diags.extend(explain_misses(world, &plans, journal.misses));
    diags.extend(explain_misfiled(sites, &journal.misfiled));

    let book = &mut world.book;
    journal.asserts.sort_by_key(|assert| assert.day);
    book.asserts = journal.asserts;
    journal.splits.sort_by_key(|split| split.day);
    book.splits = journal.splits;
    let places = book.places.len();
    let (txns, flows, touching) = Journal::lay_out(journal.txns, journal.flows, journal.ends, journal.in_order, places);
    book.txns = txns.into();
    book.flows = flows.into();
    for txn in plan_txns {
        book.txns.push(txn);
    }
    book.touching = touching;
    book.prices = Prices::new(journal.quotes);
    let events = check_events(world, journal.events, diags);
    world.book.events = events;
}

// ─── Elaborating ────────────────────────────────────────────────────────────

/// Elaborates one run of items. `first` is the global number of the first
/// transaction it makes.
fn elaborate<'s>(world: &World<'s>, run: &Run<'_, 's>, first: u32, plans: &Plans<'s>, layout_free: bool) -> Sink<'s> {
    let file = &run.site.source.file;
    let mut sink = Sink::with_room_for(run.items.len());
    let mut elab = Elab::new(world, file, &mut sink, first);
    for item in run.items {
        if !layout_free
            && let Some((day, noun)) = dated(item, file)
            && !run.site.layout.holds(day)
        {
            elab.sink.misfiled.push(Misfiled { loc: item.loc, day, noun });
        }
        match item.kind {
            ItemKind::Txn(id) => {
                let txn = &file[id];
                elab.transaction(item, txn.date, &txn.flow, Mode::Actual);
            }
            ItemKind::Occurrence(id) => elab.occurrence(item, &file[id], plans),
            ItemKind::Opening(id) => elab.opening(item, &file[id]),
            ItemKind::Assert(id) => elab.assertion(item, &file[id]),
            ItemKind::Event(id) => elab.event(item, &file[id]),
            ItemKind::Price(id) => elab.price_line(item, &file[id]),
            ItemKind::Split(id) => elab.split_line(item, &file[id]),
            _ => {}
        }
    }
    sink
}

/// The plans: each one's flow as it would be written once, kept as a template
/// for the forecast and as the shape an occurrence in the journal fills in.
/// Their transactions come after the journal's, and hold no journal flows.
fn build_plans<'s>(
    world: &mut World<'s>,
    entries: &[Entry<'_, 's>],
    journal_txns: u32,
    misses: &mut Vec<(Cause, &'s str, Loc)>,
    diags: &mut Vec<Diagnostic>,
) -> (Plans<'s>, Vec<Txn>) {
    let mut plans = Plans { by_name: Map::default(), built: Vec::new(), names: Vec::new() };
    let mut made = Vec::new();
    let shared: &World = world;
    for (at, written) in
        entries.iter().filter_map(|entry| if let Entry::Plan(w) = entry { Some(w) } else { None }).enumerate()
    {
        let (file, plan) = (written.file(), written.node);
        let mut sink = Sink::default();
        let mut elab = Elab::new(shared, file, &mut sink, journal_txns + at as u32);
        let day = plan.from.unwrap_or(Day(0));
        let shape = elab.transaction(written.item, day, &plan.flow, Mode::Planned);
        diags.append(&mut sink.diags);
        misses.append(&mut sink.misses);
        if let Some(name) = plan.name {
            if plans.by_name.insert(name.0, at).is_some() {
                diags.push(crate::errors::duplicate("plan", Word { text: name.0, loc: file.loc(name.0) }, None, None));
            }
        }
        plans.names.push(plan.name.map_or("", |name| name.0));
        made.push((written, sink, shape));
    }
    let mut txns = Vec::new();
    for (written, mut sink, shape) in made {
        let plan = written.node;
        // The plan's transaction holds no journal flows: its template is not in the book's flows.
        let mut txn = sink.txns.swap_remove(0);
        (txn.first, txn.len) = (Id::new(0), 0);
        txns.push(txn);
        if sink.flows.is_empty() {
            plans.built.push(None);
            continue;
        }
        let name = plan.name.map(|name| world.book.names.intern(name.0));
        let id = world.book.plans.push(Plan {
            name,
            every: plan.every,
            on: plan.on,
            from: plan.from,
            until: plan.until,
            template: sink.flows.into(),
            loc: written.item.loc,
        });
        plans.built.push(shape.map(|shape| (id, shape)));
    }
    (plans, txns)
}

// ─── Laying out ─────────────────────────────────────────────────────────────

/// What the runs of the journal add up to, taken in order as each finishes.
struct Journal<'s> {
    txns: Vec<Txn>,
    flows: Vec<Flow>,
    /// Both ends of every flow so far, for `Book::touching`, while the runs
    /// are in day order and the flows keep the numbers they have.
    ends: Vec<(Id<Place>, Id<Flow>)>,
    asserts: Vec<Assert>,
    splits: Vec<Split>,
    events: Vec<RawEvent<'s>>,
    quotes: Vec<Quote>,
    misses: Vec<(Cause, &'s str, Loc)>,
    misfiled: Vec<Misfiled>,
    diags: Vec<Diagnostic>,
    /// The day of the last transaction laid.
    last_day: Option<Day>,
    /// Whether every transaction so far is on or after the one before it:
    /// usually true, since journals are written as time passes.
    in_order: bool,
}

impl<'s> Journal<'s> {
    /// An empty journal with room for `txns` transactions, most of which are
    /// one flow, and the misses already found.
    fn new(txns: usize, misses: Vec<(Cause, &'s str, Loc)>) -> Journal<'s> {
        Journal {
            txns: Vec::with_capacity(txns),
            flows: Vec::with_capacity(txns + txns / 8),
            ends: Vec::with_capacity(2 * txns),
            asserts: Vec::new(),
            splits: Vec::new(),
            events: Vec::new(),
            quotes: Vec::new(),
            misses,
            misfiled: Vec::new(),
            diags: Vec::new(),
            last_day: None,
            in_order: true,
        }
    }

    /// Lays the next run after those before it. Its flows already know their
    /// transactions' numbers; only where they start has to be added.
    fn take(&mut self, mut sink: Sink<'s>) {
        self.diags.append(&mut sink.diags);
        self.misses.append(&mut sink.misses);
        self.misfiled.append(&mut sink.misfiled);
        self.quotes.append(&mut sink.quotes);
        self.asserts.append(&mut sink.asserts);
        self.splits.append(&mut sink.splits);
        self.events.append(&mut sink.events);
        if let (Some(first), Some(last)) = (sink.txns.first(), sink.txns.last()) {
            self.in_order &= !sink.unordered && self.last_day.is_none_or(|before| before <= first.day);
            self.last_day = Some(last.day);
        }
        let base = self.flows.len() as u32;
        let ends = sink.flows.iter().enumerate().flat_map(|(at, flow)| ends_of(Id::new(base + at as u32), flow));
        self.ends.extend(ends);
        self.txns.extend(sink.txns.into_iter().map(|mut txn| {
            txn.first = Id::new(base + txn.first.index() as u32);
            txn
        }));
        self.flows.append(&mut sink.flows);
    }

    /// The transactions and flows in day order, and which flows touch each of
    /// `places` places. Runs laid end to end are almost always in order; when
    /// they are not, every transaction moves with its flows and ties keep
    /// declaration order.
    fn lay_out(
        txns: Vec<Txn>,
        flows: Vec<Flow>,
        ends: Vec<(Id<Place>, Id<Flow>)>,
        in_order: bool,
        places: usize,
    ) -> (Vec<Txn>, Vec<Flow>, Groups<Place, Id<Flow>>) {
        if in_order {
            return (txns, flows, Groups::build(places, ends));
        }
        let mut own = flows.into_iter();
        let mut records: Vec<(Txn, Vec<Flow>)> = Vec::with_capacity(txns.len());
        for txn in txns {
            let len = txn.len as usize;
            records.push((txn, own.by_ref().take(len).collect()));
        }
        records.sort_by_key(|(txn, _)| txn.day);
        let (mut txns, mut flows) = (Vec::with_capacity(records.len()), Vec::new());
        for (at, (mut txn, own)) in records.into_iter().enumerate() {
            txn.first = Id::new(flows.len() as u32);
            flows.extend(own.into_iter().map(|mut flow| {
                flow.txn = Id::new(at as u32);
                flow
            }));
            txns.push(txn);
        }
        let ends = flows.iter().enumerate().flat_map(|(at, flow)| ends_of(Id::new(at as u32), flow));
        let touching = Groups::build(places, ends);
        (txns, flows, touching)
    }
}

/// The places a flow touches: its source and, if another, its target.
fn ends_of(id: Id<Flow>, flow: &Flow) -> impl Iterator<Item = (Id<Place>, Id<Flow>)> + use<> {
    let target = (flow.to != flow.from).then_some((flow.to, id));
    std::iter::once((flow.from, id)).chain(target)
}

// ─── What went wrong ────────────────────────────────────────────────────────

/// One diagnostic for each name that named nothing usable, however often it
/// was written, at its first use.
fn explain_misses<'s>(world: &World<'s>, plans: &Plans<'s>, misses: Vec<(Cause, &'s str, Loc)>) -> Vec<Diagnostic> {
    let mut order: Vec<(Cause, &str)> = Vec::new();
    let mut groups: Map<(Cause, &str), (Loc, usize)> = Map::default();
    for (cause, text, loc) in misses {
        groups.entry((cause, text)).and_modify(|group| group.1 += 1).or_insert_with(|| {
            order.push((cause, text));
            (loc, 1)
        });
    }
    let explain = |(cause, text): (Cause, &'s str)| {
        let (loc, uses) = groups[&(cause, text)];
        let word = Word { text, loc };
        match cause {
            Cause::Plan => unknown(
                "unknown-plan",
                "plan",
                word,
                closest(text, plans.names.iter().copied().filter(|name| !name.is_empty())),
            ),
            cause => world.explain(cause, word, uses),
        }
    };
    order.into_iter().map(explain).collect()
}

/// One diagnostic for each file with items dated outside what its place among
/// the folders says it holds.
fn explain_misfiled(sites: &[Site], misfiled: &[Misfiled]) -> Vec<Diagnostic> {
    let mut files: Vec<FileId> = Vec::new();
    for item in misfiled {
        if !files.contains(&item.loc.file) {
            files.push(item.loc.file);
        }
    }
    files
        .into_iter()
        .filter_map(|file| {
            let site = sites.iter().find(|site| site.source.file.id == file)?;
            let items: Vec<Misfiled> = misfiled.iter().filter(|item| item.loc.file == file).copied().collect();
            Some(site.layout.misfiled(site.source.path, &items))
        })
        .collect()
}

/// The events whose codes mark a flow, dated on or after the first flow they
/// mark: a check cannot clear before it is written.
fn check_events(world: &World, raw: Vec<RawEvent>, diags: &mut Vec<Diagnostic>) -> Vec<Event> {
    if raw.is_empty() {
        return Vec::new();
    }
    let book = &world.book;
    let mut written: Map<Sym, (Day, Loc)> = Map::default();
    for (_, flow) in book.flows.iter().filter(|(_, flow)| !flow.codes.is_empty()) {
        for &code in flow.codes.iter() {
            let first = written.entry(code).or_insert((flow.day, flow.loc));
            if flow.day < first.0 {
                *first = (flow.day, flow.loc);
            }
        }
    }
    let mut events = Vec::with_capacity(raw.len());
    for event in raw {
        let code = book.names.get(event.code).filter(|code| written.contains_key(code));
        let Some(code) = code else {
            let known = written.keys().map(|&code| book.name(code));
            let mut diagnostic =
                Diagnostic::error("unknown-code", format!("no transaction is marked `#{}`", event.code))
                    .label(event.code_loc, "nothing carries this code")
                    .note("an event names the transaction it changes by its code");
            if let Some(near) = closest(event.code, known) {
                diagnostic = diagnostic.fix(format!("did you mean `#{near}`?"), event.code_loc, format!("#{near}"));
            }
            diags.push(diagnostic);
            continue;
        };
        let (day, flow) = written[&code];
        if event.day < day {
            diags.push(
                Diagnostic::error(
                    "event-before-flow",
                    format!(
                        "`#{}` is {} {}, {} days before it was written",
                        event.code,
                        state_word(event.state),
                        crate::errors::iso(event.day),
                        day.0 - event.day.0
                    ),
                )
                .label(event.code_loc, format!("{} on {}", state_word(event.state), crate::errors::iso(event.day)))
                .context(flow, format!("written on {}", crate::errors::iso(day)))
                .help(format!(
                    "an event happens on or after the flow it changes: date it {} or later",
                    crate::errors::iso(day)
                )),
            );
            continue;
        }
        events.push(Event { day: event.day, code, state: event.state, loc: event.loc });
    }
    events.sort_by_key(|event| event.day);
    events
}

fn state_word(state: crate::book::EventState) -> &'static str {
    match state {
        crate::book::EventState::Settled => "settled",
        crate::book::EventState::Void => "voided",
        crate::book::EventState::Returned => "returned",
    }
}
