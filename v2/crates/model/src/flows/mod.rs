//! The journal: transactions elaborated into flows, and everything else that
//! is written down about them.
//!
//! Every dated item is independent of the others, so runs of consecutive items
//! are elaborated on every core against the read-only world, each into its own
//! dense vectors, its [`Sink`]. Then the runs are laid end to end in day order,
//! declaration order deciding ties, and the book learns which flows touch which
//! places.

mod faults;
mod forms;
mod moves;
mod pairing;
mod records;
mod shape;

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
use crate::names::near;
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
    journal: &[(Run<'a, 's>, u32)],
    txns: u32,
    layout_free: bool,
    diags: &mut Vec<Diagnostic>,
) {
    records::code_rules(world, entries, diags);
    world.book.syncs = records::syncs(world, entries);
    let mut misses = Vec::new();
    let (plans, plan_txns) = build_plans(world, entries, txns, &mut misses, diags);

    let shared: &World = world;
    let mut sinks = par::map_each(journal, |(run, first)| elaborate(shared, run, *first, &plans, layout_free));
    let mut misfiled: Vec<Misfiled> = Vec::new();
    let mut quotes = Vec::new();
    for sink in &mut sinks {
        diags.append(&mut sink.diags);
        misses.append(&mut sink.misses);
        misfiled.append(&mut sink.misfiled);
        quotes.append(&mut sink.quotes);
    }
    diags.extend(explain_misses(world, &plans, misses));
    diags.extend(explain_misfiled(sites, &misfiled));

    let book = &mut world.book;
    book.asserts = sinks.iter_mut().flat_map(|sink| std::mem::take(&mut sink.asserts)).collect();
    book.asserts.sort_by_key(|assert| assert.day);
    book.splits = sinks.iter_mut().flat_map(|sink| std::mem::take(&mut sink.splits)).collect();
    book.splits.sort_by_key(|split| split.day);
    let raw: Vec<RawEvent> = sinks.iter_mut().flat_map(|sink| std::mem::take(&mut sink.events)).collect();
    let (txns, flows) = lay_out(sinks);
    book.txns = txns.into();
    book.flows = flows.into();
    for txn in plan_txns {
        book.txns.push(txn);
    }
    book.touching = touching(book.places.len(), book.flows.iter());
    book.prices = Prices::new(quotes);
    let events = check_events(world, raw, diags);
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

/// Whether the runs, laid end to end, are already in day order: usually true,
/// since journals are written as time passes.
fn in_day_order(sinks: &[Sink]) -> bool {
    let mut last = None;
    for sink in sinks {
        let (Some(first), Some(end)) = (sink.txns.first(), sink.txns.last()) else {
            continue;
        };
        if sink.unordered || last.is_some_and(|last| last > first.day) {
            return false;
        }
        last = Some(end.day);
    }
    true
}

/// Every transaction and flow, in day order; ties keep declaration order. The
/// flows already know their transactions' numbers when the runs are in order.
fn lay_out(sinks: Vec<Sink>) -> (Vec<Txn>, Vec<Flow>) {
    let (txn_count, flow_count) = sinks.iter().fold((0, 0), |(t, f), sink| (t + sink.txns.len(), f + sink.flows.len()));
    let (mut txns, mut flows) = (Vec::with_capacity(txn_count), Vec::with_capacity(flow_count));
    if in_day_order(&sinks) {
        for sink in sinks {
            let base = flows.len() as u32;
            txns.extend(sink.txns.into_iter().map(|mut txn| {
                txn.first = Id::new(base + txn.first.index() as u32);
                txn
            }));
            flows.extend(sink.flows);
        }
        return (txns, flows);
    }
    let mut records: Vec<(Txn, Vec<Flow>)> = Vec::with_capacity(txn_count);
    for sink in sinks {
        let mut own = sink.flows.into_iter();
        for txn in sink.txns {
            let len = txn.len as usize;
            records.push((txn, own.by_ref().take(len).collect()));
        }
    }
    records.sort_by_key(|(txn, _)| txn.day);
    for (at, (mut txn, own)) in records.into_iter().enumerate() {
        txn.first = Id::new(flows.len() as u32);
        flows.extend(own.into_iter().map(|mut flow| {
            flow.txn = Id::new(at as u32);
            flow
        }));
        txns.push(txn);
    }
    (txns, flows)
}

/// Every flow that touches each place, as source or as target, in flow order.
fn touching<'a>(places: usize, flows: impl Iterator<Item = (Id<Flow>, &'a Flow)>) -> Groups<Place, Id<Flow>> {
    let ends = flows.flat_map(|(id, flow)| {
        let target = (flow.to != flow.from).then_some((flow.to, id));
        std::iter::once((flow.from, id)).chain(target)
    });
    Groups::build(places, ends)
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
                near(text, plans.names.iter().copied().filter(|name| !name.is_empty())),
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
            if let Some(near) = near(event.code, known) {
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
