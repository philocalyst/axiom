//! `register`: one place's flows, dated, with a running balance.
//!
//! Amounts and balances are in the place's display sign, the way a statement
//! shows them: what a card owes is positive, and a charge adds to it.

use std::collections::BTreeMap;

use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_engine::{Pad, Run, State};
use axiom_model::{
    Amount, Asset, Book, Commodity, Contract, Derivation, Entity, Flow, Object, Origin, Place, Role, Subject,
};

use crate::history::{Change, Posting, all_postings, pad_ends};
use crate::lens::Lens;
use crate::places::path;
use crate::resolve;
use crate::table::gap_words;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Builds a register using owner scope and display signs from the shared lens.
pub(crate) fn view_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    place: &str,
    from: Option<Day>,
    to: Option<Day>,
) -> Result<Report<'s>, Diagnostic> {
    let book = lens.book();
    let window = Window::new(from, to, run);
    if let Some(contract) = place.strip_prefix("contract:") {
        let contract = resolve::contract(book, contract)?;
        return Ok(contract_register(lens, run, contract, window));
    }
    if let Some(asset) = place.strip_prefix("asset:") {
        let asset = resolve::asset(book, asset)?;
        return Ok(asset_register(lens, run, asset, window));
    }
    if let Some(entity) = place.strip_prefix("entity:") {
        let entity = book.entity(entity).map_err(|miss| resolve::entity_miss(book, entity, miss))?;
        let target = book.name(book.entities[entity].path);
        return Ok(entity_view(lens, run, entity, target, window));
    }
    // A contract may share its name with its party. Follow `why`'s
    // precedence so the contract register remains addressable by its declared
    // name; the party's own register can be selected with `entity:NAME`.
    if let Some(contract) = book.contract(place) {
        return Ok(contract_register(lens, run, contract, window));
    }
    if let Some(asset) = book.asset(place) {
        return Ok(asset_register(lens, run, asset, window));
    }
    if let Ok(entity) = book.entity(place) {
        return Ok(entity_view(lens, run, entity, place, window));
    }
    let place = resolve::place(book, place)?;
    // A place is somebody's: another owner's register is not part of whose money this is.
    let owner = book.places[place].owner;
    let register = match lens.owns(place) {
        true => section_with_sign(lens, run, place, window, lens.display_sign(place)),
        false => Section::note_only(format!(
            "{} belongs to {}, whose money this is not.",
            path(book, place),
            book.name(book.entities[owner].path)
        )),
    };
    Ok(Report::new(format!("Register: {}", path(book, place))).with(register))
}

/// The days a register covers: from a day if there is one, up to a cutoff.
#[derive(Clone, Copy)]
struct Window {
    from: Option<Day>,
    cutoff: Day,
}

impl Window {
    /// The days from `from` to `to`, which is today if there is none.
    fn new(from: Option<Day>, to: Option<Day>, run: &Run) -> Window {
        Window { from, cutoff: to.unwrap_or(run.today) }
    }

    fn holds(self, day: Day) -> bool {
        day <= self.cutoff && self.from.is_none_or(|from| day >= from)
    }
}

/// What a register says of what is not the owner's: whose it is.
fn foreign<'s>(book: &Book<'s>, name: &str, owner: Id<Entity>) -> Report<'s> {
    Report::new(format!("Register: {name}")).with(Section::note_only(format!(
        "{name} belongs to {}, whose money this is not.",
        book.name(book.entities[owner].path)
    )))
}

/// A register of rows dated in any order, shown in order of their days. `nothing` says why a register is empty.
fn dated_register<'s>(
    name: &str,
    columns: impl IntoIterator<Item = Column<'s>>,
    mut rows: Vec<(Day, Row<'s>)>,
    nothing: String,
) -> Report<'s> {
    rows.sort_by_key(|(day, _)| *day);
    let mut section = Section::new(columns);
    section.rows.extend(rows.into_iter().map(|(_, row)| row));
    if section.rows.is_empty() {
        section.note(nothing);
    }
    Report::new(format!("Register: {name}")).with(section)
}

/// A party or owner register is a history of everything it touched, including
/// flows it owns that have no account end under its name.
fn entity_view<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    entity: Id<Entity>,
    target: &str,
    window: Window,
) -> Report<'s> {
    let book = lens.book();
    let is_owner = book.entities[entity].place.is_some_and(|place| matches!(book.places[place].role, Role::Holding(_)));
    if is_owner && !lens.owns_entity(entity) {
        return Report::new(format!("Register: {target}")).with(Section::note_only(format!(
            "{} is outside this owner's scope.",
            book.name(book.entities[entity].path)
        )));
    }
    let mut section = Section::new([
        Column::left("Date"),
        Column::left("Purpose"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("Note"),
        Column::left("State"),
        Column::left("From"),
    ]);
    let touching = all_postings(book, run).filter(|posting| touches_entity(lens, posting.flow, entity, window));
    for posting in touching {
        section.push(entity_flow_row(lens, run, posting));
    }
    // An accepted assertion gap has no journal flow, but its counterparty is
    // still part of the entity's register. In particular, this keeps market
    // revaluations and unexplained `?` balances visible from the other end.
    let entity_place = book.entities[entity].place;
    for row in run.pads.iter().filter_map(|pad| gap_row(lens, pad, entity_place, window)) {
        section.push(row);
    }
    section.rows.sort_by_key(|row| match row.cells.first() {
        Some(Cell::Day(day)) => *day,
        _ => Day::MAX,
    });
    if section.rows.is_empty() {
        section.note(format!("No flows touch {target} in this window."));
    }
    Report::new(format!("Register: {target}")).with(section)
}

/// Whether a flow is in an entity's register: in the window and the owner's scope, and the entity's own, or
/// paid to it, or at one end of it, as the party outside or as the owner of a place.
fn touches_entity(lens: Lens<'_, '_, '_, '_>, flow: &Flow, entity: Id<Entity>, window: Window) -> bool {
    let book = lens.book();
    let at_party = flow.payee == Some(entity)
        || [flow.from, flow.to].into_iter().any(|place| {
            place_owned_by(lens, place, entity)
                || matches!(book.places[place].role, Role::Outside(Some(party)) if party == entity)
        });
    let moved = crate::flow::movement_place(lens, flow);
    window.holds(flow.day)
        && lens.owns(moved)
        && (flow.owner == entity || at_party || place_owned_by(lens, moved, entity))
}

fn entity_flow_row<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, posting: Posting<'_>) -> Row<'s> {
    let book = lens.book();
    let flow = posting.flow;
    let mut note = book.flow_view(flow).codes().map(|code| Cell::Code(book.name(code))).collect::<Vec<_>>();
    if let Some(description) = flow.description {
        note.push(Cell::text(book.text(description)));
    }
    // A flow a law derived is no statement of its own: it says what derived it, and not what its cause's line says of itself.
    match posting.offspring(run) {
        Some(offspring) => note.push(crate::table::origin_cell(book, offspring)),
        None => note
            .extend(book.txns.get(flow.txn).and_then(|txn| crate::table::doc_headline(book, txn.doc)).map(Cell::text)),
    }
    let state = match posting.posted.state {
        State::Actual => Cell::Word("actual"),
        State::Pending => Cell::Word("pending"),
        State::Settled(day) => Cell::text(format!("settled {day}")),
        State::Void => Cell::Word("void"),
        State::Returned(day) => Cell::text(format!("returned {day}")),
        State::Planned => Cell::Word("planned"),
    };
    Row::new([
        Cell::Day(flow.day),
        purpose_cell(book, flow),
        Cell::text(crate::places::route(book, flow)),
        Cell::amount(book, scoped_flow_amount(lens, flow, posting.out())),
        Cell::list_or_blank(" · ", note),
        state,
        Cell::Source(flow.loc),
    ])
}

/// The row for an accepted gap whose counterparty is the entity's place, if it is in the window and the owner's
/// scope.
fn gap_row<'s>(
    lens: Lens<'s, '_, '_, '_>,
    pad: &Pad,
    entity_place: Option<Id<Place>>,
    window: Window,
) -> Option<Row<'s>> {
    let book = lens.book();
    if entity_place != Some(pad.counter) || !lens.owns(pad.place) || !window.holds(pad.day) {
        return None;
    }
    let qty = lens.place_qty(pad.place, pad.amount.qty).0.checked_abs()?;
    let direction = if pad.amount.qty >= Qty::ZERO {
        format!("{} → {}", path(book, pad.counter), path(book, pad.place))
    } else {
        format!("{} → {}", path(book, pad.place), path(book, pad.counter))
    };
    let note = crate::table::gap_words(book, pad);
    let loc = book.asserts[pad.assert as usize].loc;
    Some(Row::new([
        Cell::Day(pad.day),
        Cell::Blank,
        Cell::text(direction),
        Cell::amount(book, Amount::new(Qty(qty), pad.amount.unit)),
        Cell::text(note),
        Cell::Word("actual"),
        Cell::Source(loc),
    ]))
}

/// The purpose a flow serves, or nothing.
fn purpose_cell<'s>(book: &Book<'s>, flow: &Flow) -> Cell<'s> {
    flow.purpose.map_or(Cell::Blank, |purpose| Cell::Purpose(book.name(book.purposes[purpose.purpose].name)))
}

fn asset_register<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, asset_id: Id<Asset>, window: Window) -> Report<'s> {
    let book = lens.book();
    let asset = &book.assets[asset_id];
    let name = book.name(asset.name);
    if !lens.owns_entity(asset.owner) {
        return foreign(book, name, asset.owner);
    }
    let flows = all_postings(book, run).filter(|posting| {
        let flow = posting.flow;
        window.holds(flow.day)
            && lens.owns(crate::flow::movement_place(lens, flow))
            && (flow.from == asset.place
                || flow.to == asset.place
                || flow.purpose.is_some_and(|purpose| purpose.of == Some(Object::Asset(asset_id)))
                || matches!(flow.origin, Origin::Derived(Derivation::Disposal(found)) if found == asset_id))
    });
    let mut rows: Vec<_> = flows.map(|posting| (posting.flow.day, asset_flow_row(lens, posting))).collect();
    let consumed = run.adjustments.iter().filter(|adjustment| {
        window.holds(adjustment.day)
            && matches!(adjustment.kind, axiom_engine::AdjustmentKind::Consumed { asset, .. } if asset == asset_id)
    });
    rows.extend(consumed.map(|adjustment| (adjustment.day, basis_row(lens, asset, adjustment))));
    let columns = [
        Column::left("Date"),
        Column::left("Activity"),
        Column::left("Purpose or law"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("From"),
    ];
    dated_register(name, columns, rows, format!("Nothing happened to {name} in this window."))
}

fn asset_flow_row<'s>(lens: Lens<'s, '_, '_, '_>, posting: Posting<'_>) -> Row<'s> {
    let (book, flow) = (lens.book(), posting.flow);
    let (activity, style) = match flow.origin {
        Origin::Derived(Derivation::Disposal(_)) => ("disposed", Style::Alert),
        Origin::Derived(_) => ("derived", Style::Muted),
        Origin::Occurrence(_) => ("contract occurrence", Style::Muted),
        Origin::Written => ("flow", Style::Normal),
    };
    Row::new([
        Cell::Day(flow.day),
        Cell::Word(activity),
        purpose_cell(book, flow),
        Cell::text(crate::places::route(book, flow)),
        Cell::amount(book, scoped_flow_amount(lens, flow, posting.out())),
        Cell::Source(flow.loc),
    ])
    .style(style)
}

/// The basis a law consumed from an asset.
fn basis_row<'s>(lens: Lens<'s, '_, '_, '_>, asset: &Asset, adjustment: &axiom_engine::Adjustment) -> Row<'s> {
    let book = lens.book();
    Row::new([
        Cell::Day(adjustment.day),
        Cell::Word("basis consumed"),
        Cell::Name(book.name(book.laws[adjustment.law].name)),
        Cell::Blank,
        Cell::base(book, lens.entity_qty(asset.owner, adjustment.amount)),
        Cell::Source(book.laws[adjustment.law].loc),
    ])
}

fn contract_register<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    contract_id: Id<Contract>,
    window: Window,
) -> Report<'s> {
    let book = lens.book();
    let contract = &book.contracts[contract_id];
    let name = book.name(contract.name);
    if !lens.owns_entity(contract.owner) {
        return foreign(book, name, contract.owner);
    }
    let changes =
        contract.terms.iter().flat_map(|terms| contract.stretches().map(move |(days, waiver)| (days, terms, waiver)));
    let mut rows: Vec<_> = changes
        .filter(|(days, ..)| window.holds(days.first()))
        .map(|(days, terms, waiver)| (days.first(), terms_row(lens, contract, days.first(), terms, waiver)))
        .collect();
    let promises = run.promises.iter().filter(|promise| {
        promise.contract == contract_id && lens.owns_entity(contract.owner) && window.holds(promise.due)
    });
    rows.extend(promises.map(|promise| (promise.due, promise_row(contract, promise, window.cutoff))));
    let flows = book.flows.iter().filter(|(_, flow)| {
        window.holds(flow.day)
            && lens.owns(crate::flow::movement_place(lens, flow))
            && contract_flow(flow.origin, contract_id)
    });
    rows.extend(flows.map(|(id, flow)| (flow.day, contract_flow_row(lens, run, id, flow))));
    let columns = [
        Column::left("Date"),
        Column::left("Activity"),
        Column::left("Terms or purpose"),
        Column::left("Flow or kept"),
        Column::left("Amount or source"),
    ];
    dated_register(name, columns, rows, format!("Nothing happened under {name} in this window."))
}

/// A change to a contract's terms, on the day it starts.
fn terms_row<'s>(
    lens: Lens<'s, '_, '_, '_>,
    contract: &'s Contract,
    day: Day,
    terms: &'s axiom_model::Terms,
    waiver: Option<&axiom_model::Change>,
) -> Row<'s> {
    let statement = waiver.map_or(Cell::Source(contract.loc), |change| Cell::Source(change.loc));
    let activity = match waiver {
        None => Cell::Word("terms active"),
        Some(_) => Cell::Word("terms waived"),
    };
    Row::new([
        Cell::Day(day),
        activity,
        crate::contracts::terms_cell(lens, contract, terms, waiver),
        Cell::Blank,
        statement,
    ])
}

/// A promise of a contract: due, or late, and the day it was kept.
fn promise_row<'s>(contract: &Contract, promise: &axiom_engine::Promise, cutoff: Day) -> Row<'s> {
    let kept = promise.kept.map(|(day, _)| day);
    let late = promise.late(cutoff);
    let row = Row::new([
        Cell::Day(promise.due),
        Cell::Word(if late > 0 { "late promise" } else { "due" }),
        kept.map_or(Cell::Blank, Cell::Day),
        Cell::Blank,
        kept.map_or(Cell::Source(contract.loc), |day| Cell::Day(day)),
    ]);
    if late > 0 { row.style(Style::Alert) } else { row }
}

fn contract_flow_row<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, id: Id<Flow>, flow: &Flow) -> Row<'s> {
    let book = lens.book();
    let posting = Posting::at(book, run, id);
    Row::new([
        Cell::Day(flow.day),
        Cell::Word(contract_flow_word(flow.origin)),
        purpose_cell(book, flow),
        Cell::text(crate::places::route(book, flow)),
        Cell::list(" ", [Cell::amount(book, scoped_flow_amount(lens, flow, posting.out())), Cell::Source(flow.loc)]),
    ])
}

/// What a contract's flow is, in a word: the occurrence it wrote, or what it derived.
pub(crate) fn contract_flow_word(origin: Origin) -> &'static str {
    match origin {
        Origin::Occurrence(_) => "occurrence",
        Origin::Derived(Derivation::Interest(_)) => "interest",
        Origin::Derived(Derivation::Principal(_)) => "principal",
        Origin::Derived(Derivation::Claim(_)) => "claim",
        Origin::Derived(Derivation::Otherwise(_)) => "late fee",
        Origin::Derived(Derivation::Refund(_)) => "refund",
        Origin::Derived(_) => "derived",
        Origin::Written => "flow",
    }
}

/// Whether a flow is one the contract wrote or derived.
pub(crate) fn contract_flow(origin: Origin, contract: Id<Contract>) -> bool {
    match origin {
        Origin::Occurrence(found) => found == contract,
        Origin::Derived(
            Derivation::Interest(found)
            | Derivation::Principal(found)
            | Derivation::Claim(found)
            | Derivation::Otherwise(found)
            | Derivation::Refund(found),
        ) => found == contract,
        _ => false,
    }
}

fn scoped_flow_amount<'s>(lens: Lens<'s, '_, '_, '_>, flow: &axiom_model::Flow, amount: Amount) -> Amount {
    Amount::new(crate::flow::scoped_movement_qty(lens, flow, amount.qty), amount.unit)
}

fn place_owned_by(lens: Lens<'_, '_, '_, '_>, place: Id<Place>, entity: Id<Entity>) -> bool {
    lens.plan().owners_of(place).iter().any(|owner| owner.owner == entity && !owner.share.is_zero())
}

/// Builds a register for a place within an owner's view.
pub(crate) fn section_for_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    place: Id<Place>,
    from: Option<Day>,
    to: Option<Day>,
) -> Section<'s> {
    section_with_sign(lens, run, place, Window::new(from, to, run), lens.display_sign(place))
}

fn section_with_sign<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    place: Id<Place>,
    window: Window,
    sign: i64,
) -> Section<'s> {
    let book = lens.book();
    let steps = steps(lens, run, place, window.cutoff);
    let split = window.from.map_or(0, |from| steps.partition_point(|step| step.day < from));
    let shown = |qty: Qty, unit: Id<Commodity>| Cell::amount(book, Amount::new(Qty(qty.0 * sign), unit));
    let columns = [
        Column::left("Date"),
        Column::left("With"),
        Column::left("Payee"),
        Column::left("Note"),
        Column::right("Amount"),
        Column::right("Balance"),
    ];
    let mut section = Section::new(columns);
    let mut running = Running::after(lens, place, &steps[..split]);
    if let Some(from) = window.from {
        for (&unit, &qty) in running.shown.iter().filter(|(_, qty)| !qty.is_zero()) {
            let cells = [
                Cell::Day(from),
                Cell::text("opening balance"),
                Cell::Blank,
                Cell::Blank,
                Cell::Blank,
                shown(qty, unit),
            ];
            section.push(Row::new(cells).style(Style::Total));
        }
    }
    for step in &steps[split..] {
        let (unit, amount, balance) = running.advance(lens, place, step);
        section.push(step_row(book, run, step, shown(amount, unit), shown(balance, unit)));
    }
    if section.rows.is_empty() {
        section.note(format!("Nothing touches {} in this window.", path(book, place)));
    }
    if section.rows.iter().any(|row| row.style == Style::Muted) {
        section.note("Muted lines are pending, void or returned: they do not move the balance.");
    }
    section
}

/// What a place holds, by commodity, as the steps of its register go by: as the flows moved it, and as the
/// place shows it.
struct Running {
    raw: BTreeMap<Id<Commodity>, Qty>,
    shown: BTreeMap<Id<Commodity>, Qty>,
}

impl Running {
    /// What `place` holds once `steps` are over.
    fn after(lens: Lens<'_, '_, '_, '_>, place: Id<Place>, steps: &[Step<'_>]) -> Running {
        let mut raw: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
        for step in steps {
            let Change::Moved(moved) = step.change;
            *raw.entry(moved.unit).or_default() += step.counted();
        }
        let shown = raw.iter().map(|(&unit, &qty)| (unit, lens.place_qty(place, qty))).collect();
        Running { raw, shown }
    }

    /// Takes one step: the commodity it moves, how much as the place shows it, and what the place then holds.
    fn advance(&mut self, lens: Lens<'_, '_, '_, '_>, place: Id<Place>, step: &Step<'_>) -> (Id<Commodity>, Qty, Qty) {
        let Change::Moved(moved) = step.change;
        let raw = self.raw.entry(moved.unit).or_default();
        let balance = self.shown.entry(moved.unit).or_default();
        let amount = if step.counts {
            let before = lens.place_qty(place, *raw);
            *raw += moved.qty;
            let after = lens.place_qty(place, *raw);
            *balance = after;
            after - before
        } else {
            lens.place_qty(place, moved.qty)
        };
        (moved.unit, amount, *balance)
    }
}

fn step_row<'s>(book: &'s Book<'_>, run: &Run, step: &Step<'_>, amount: Cell<'s>, balance: Cell<'s>) -> Row<'s> {
    let payee = step.source.posting().and_then(|posting| posting.flow.payee);
    let cells = [
        Cell::Day(step.day),
        Cell::text(path(book, step.with)),
        payee.map_or(Cell::Blank, |entity| Cell::text(book.name(book.entities[entity].path))),
        note(book, run, step).unwrap_or(Cell::Blank),
        amount,
        balance,
    ];
    Row::new(cells).style(if step.counts { Style::Normal } else { Style::Muted })
}

/// One journal flow, or one accepted gap, as seen from a place.
struct Step<'a> {
    day: Day,
    change: Change,
    /// Whether it is real at the end of the window.
    counts: bool,
    /// The other end.
    with: Id<Place>,
    source: Source<'a>,
    /// Where it stands among the steps of its day (`Posting::sequence`).
    order: (u32, u32),
}

/// What made a step.
enum Source<'a> {
    Flow(Posting<'a>),
    /// A pad, which the journal never wrote: the engine posted it to close the
    /// gap an assertion accepted.
    Gap(&'a Pad),
}

impl<'a> Source<'a> {
    fn posting(&self) -> Option<Posting<'a>> {
        match *self {
            Source::Flow(posting) => Some(posting),
            Source::Gap(_) => None,
        }
    }
}

impl Step<'_> {
    /// What it adds to the running balance.
    fn counted(&self) -> Qty {
        match self.change {
            Change::Moved(moved) if self.counts => moved.qty,
            Change::Moved(_) => Qty::ZERO,
        }
    }
}

/// Every step touching `place` up to `cutoff`, in order. A flow a law derived follows the flow it came from, and a pad,
/// made at the end of its day, follows that day's flows.
fn steps<'a>(lens: Lens<'a, '_, '_, '_>, run: &'a Run, place: Id<Place>, cutoff: Day) -> Vec<Step<'a>> {
    let book = lens.book();
    let journal = book.touching[place].iter().map(|&id| Posting::at(book, run, id));
    let derived = (0..run.offspring.len()).map(|at| Posting::derived(run, Id::new(at as u32)));
    let derived = derived.filter(|posting| posting.flow.from == place || posting.flow.to == place);
    let flows = journal.chain(derived).flat_map(|posting| {
        let in_scope = lens.owns(place);
        posting.changes_at(place).filter(move |_| in_scope).map(move |change| Step {
            day: posting.flow.day,
            change,
            counts: posting.is_real_on(cutoff),
            with: posting.counterparty(place),
            source: Source::Flow(posting),
            order: posting.sequence(run),
        })
    });
    let pads = run.pads.iter().flat_map(|pad| {
        let in_scope = lens.owns(place) && lens.governs(Subject::Place(pad.place));
        let with = if pad.place == place { pad.counter } else { pad.place };
        let here = pad_ends(pad).into_iter().filter(move |&(at, _)| at == place && in_scope);
        here.map(move |(_, moved)| Step {
            day: pad.day,
            change: Change::Moved(moved),
            counts: true,
            with,
            source: Source::Gap(pad),
            order: (u32::MAX, u32::MAX),
        })
    });
    let mut steps: Vec<Step> = flows.chain(pads).filter(|step| step.day <= cutoff).collect();
    steps.sort_by_key(|step| (step.day, step.order));
    steps
}

/// A change of basis, codes, and settlement, as one line of small print.
fn note<'s>(book: &'s Book<'_>, run: &Run, step: &Step<'_>) -> Option<Cell<'s>> {
    let parts: Vec<Cell<'s>> = match step.source {
        Source::Gap(pad) => vec![Cell::Said(gap_words(book, pad).into())],
        Source::Flow(posting) => {
            let status = match posting.posted.state {
                State::Actual | State::Planned => None,
                State::Pending => Some(Cell::Word("pending")),
                State::Void => Some(Cell::Word("void")),
                State::Settled(on) if !step.counts => Some(Cell::text(format!("pending until {on}"))),
                State::Settled(on) => Some(Cell::text(format!("settled {on}"))),
                State::Returned(on) => Some(Cell::text(format!("returned {on}"))),
            };
            let flow = book.flow_view(posting.flow);
            let codes = flow.codes().map(|code| Cell::Code(book.name(code)));
            let origin = posting.offspring(run).map(|offspring| crate::table::origin_cell(book, offspring));
            codes.chain(status).chain(origin).collect()
        }
    };
    (!parts.is_empty()).then(|| Cell::list(" · ", parts))
}
