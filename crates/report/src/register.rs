//! `register`: one place's flows, dated, with a running balance; or what touches a party, an asset or a contract.
//!
//! Amounts and balances of a place are in its display sign, the way a statement
//! shows them: what a card owes is positive, and a charge adds to it. A party, an asset or a contract is
//! at the end of flows in many places and many commodities, so there is no one balance to run: its register is the
//! flows that touch it, in the place register's columns but that one.

use std::collections::BTreeMap;

use axiom_core::{Day, Diagnostic, Id, Qty};
use axiom_engine::{Pad, Run, State};
use axiom_model::{
    Amount, Asset, Book, Commodity, Contract, Derivation, Entity, Flow, Object, Origin, Place, Role, Subject,
};

use crate::history::{Change, Posting, all_postings, pad_ends};
use crate::places::{path, route};
use crate::resolve;
use crate::table::gap_words;
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section, Style};

/// What a register is of.
enum Of {
    Place(Id<Place>),
    /// A party or an owner, and what the title calls it.
    Entity(Id<Entity>, String),
    Asset(Id<Asset>),
    Contract(Id<Contract>),
}

impl Of {
    /// What `text` asks for. A prefix says which kind (`contract:`, `asset:`, `entity:`); a bare name is a contract, an
    /// asset, a party and then a place, the first that it names. A contract may share its name with its party, and the
    /// party's own register is then `entity:NAME`.
    fn named(book: &Book, text: &str) -> Result<Of, Diagnostic> {
        if let Some(name) = text.strip_prefix("contract:") {
            return resolve::contract(book, name).map(Of::Contract);
        }
        if let Some(name) = text.strip_prefix("asset:") {
            return resolve::asset(book, name).map(Of::Asset);
        }
        if let Some(name) = text.strip_prefix("entity:") {
            let entity = resolve::entity(book, name)?;
            return Ok(Of::Entity(entity, book.name(book.entities[entity].path).to_string()));
        }
        if let Some(contract) = book.contract(text) {
            return Ok(Of::Contract(contract));
        }
        if let Some(asset) = book.asset(text) {
            return Ok(Of::Asset(asset));
        }
        match book.entity(text) {
            Ok(entity) => Ok(Of::Entity(entity, text.to_string())),
            Err(_) => resolve::place(book, text).map(Of::Place),
        }
    }
}

/// Builds a register using owner scope and display signs from the shared view.
pub(crate) fn report<'s>(
    view: View<'s, '_, '_>,
    target: &str,
    from: Option<Day>,
    to: Option<Day>,
) -> Result<Report<'s>, Diagnostic> {
    let window = Window::new(from, to, view.run);
    Ok(match Of::named(view.book(), target)? {
        Of::Place(place) => place_register(view, place, window),
        Of::Entity(entity, shown) => entity_register(view, entity, &shown, window),
        Of::Asset(asset) => asset_register(view, asset, window),
        Of::Contract(contract) => contract_register(view, contract, window),
    })
}

/// A place is somebody's: another owner's register is not part of whose money this is.
pub(crate) fn place_register<'s>(view: View<'s, '_, '_>, place: Id<Place>, window: Window) -> Report<'s> {
    let book = view.book();
    let register = match view.owns(place) {
        true => section_with_sign(view, place, window),
        false => Section::note_only(format!(
            "{} belongs to {}, whose money this is not.",
            path(book, place),
            book.name(book.entities[book.places[place].owner].path)
        )),
    };
    Report::new(format!("Register: {}", path(book, place))).with(register)
}

/// The days a register covers: from a day if there is one, up to a cutoff.
#[derive(Clone, Copy)]
pub(crate) struct Window {
    from: Option<Day>,
    cutoff: Day,
}

impl Window {
    /// The days from `from` to `to`, which is today if there is none.
    pub(crate) fn new(from: Option<Day>, to: Option<Day>, run: &Run) -> Window {
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

/// A party or owner register is a history of everything it touched, including flows it owns that have no account end
/// under its name, and the gaps accepted against its place: that keeps market revaluations and unexplained `?`
/// balances visible from the other end.
fn entity_register<'s>(view: View<'s, '_, '_>, entity: Id<Entity>, shown: &str, window: Window) -> Report<'s> {
    let book = view.book();
    let place = book.entities[entity].place;
    let is_owner = place.is_some_and(|place| matches!(book.places[place].role, Role::Holding(_)));
    if is_owner && !view.owns_entity(entity) {
        return Report::new(format!("Register: {shown}")).with(Section::note_only(format!(
            "{} is outside this owner's scope.",
            book.name(book.entities[entity].path)
        )));
    }
    // At the party's end of a flow: the place it owns, or the outside it stands for.
    let at_party = |end: Id<Place>| {
        place_owned_by(view, end, entity)
            || matches!(book.places[end].role, Role::Outside(Some(party)) if party == entity)
    };
    let touching = |flow: &Flow| {
        flow.owner == entity
            || flow.payee == Some(entity)
            || at_party(flow.from)
            || at_party(flow.to)
            || place_owned_by(view, view.movement_place(flow), entity)
    };
    let nothing = format!("No flows touch {shown} in this window.");
    listing(view, shown, window, nothing, touching, |pad| place == Some(pad.counter))
}

fn asset_register<'s>(view: View<'s, '_, '_>, id: Id<Asset>, window: Window) -> Report<'s> {
    let book = view.book();
    let asset = &book.assets[id];
    let name = book.name(asset.name);
    if !view.owns_entity(asset.owner) {
        return foreign(book, name, asset.owner);
    }
    let touching = |flow: &Flow| {
        flow.from == asset.place
            || flow.to == asset.place
            || flow.purpose.is_some_and(|purpose| purpose.of == Some(Object::Asset(id)))
            || matches!(flow.origin, Origin::Derived(Derivation::Disposal(found)) if found == id)
    };
    listing(view, name, window, format!("Nothing happened to {name} in this window."), touching, |_| false)
}

fn contract_register<'s>(view: View<'s, '_, '_>, id: Id<Contract>, window: Window) -> Report<'s> {
    let book = view.book();
    let contract = &book.contracts[id];
    let name = book.name(contract.name);
    if !view.owns_entity(contract.owner) {
        return foreign(book, name, contract.owner);
    }
    let touching = |flow: &Flow| contract_flow(flow.origin, id);
    listing(view, name, window, format!("Nothing happened under {name} in this window."), touching, |_| false)
}

/// What touches a thing, from the first day of the window to its last: the flows `touching` picks, and the gaps `gap`
/// picks, each the place register's row but for its balance. `nothing` says why a register is empty.
fn listing<'s>(
    view: View<'s, '_, '_>,
    name: &str,
    window: Window,
    nothing: String,
    touching: impl Fn(&Flow) -> bool,
    gap: impl Fn(&Pad) -> bool,
) -> Report<'s> {
    let book = view.book();
    let flows = all_postings(book, view.run)
        .filter(|posting| window.holds(posting.flow.day) && view.owns_flow(posting.flow) && touching(posting.flow));
    let gaps = view.run.pads.iter().filter(|pad| window.holds(pad.day) && view.owns(pad.place) && gap(pad));
    let mut entries: Vec<(Day, Source)> = flows.map(|posting| (posting.flow.day, Source::Flow(posting))).collect();
    entries.extend(gaps.map(|pad| (pad.day, Source::Gap(pad))));
    entries.sort_by_key(|&(day, _)| day);
    let columns = ["Date", "Flow", "Payee", "Note"].map(Column::left).into_iter().chain([Column::right("Amount")]);
    let mut section = Section::new(columns);
    for (day, source) in &entries {
        section.push(entry_row(view, window.cutoff, *day, source));
    }
    if section.rows.is_empty() {
        section.note(nothing);
    }
    Report::new(format!("Register: {name}")).with(section)
}

/// One flow or gap of a thing's register: the day, what moved from where to where and what for, whom it was paid to,
/// what is said of it, and how much.
fn entry_row<'s>(view: View<'s, '_, '_>, cutoff: Day, day: Day, source: &Source<'_>) -> Row<'s> {
    let book = view.book();
    let (flow, amount) = match *source {
        Source::Flow(posting) => {
            let flow = posting.flow;
            let purpose = flow
                .purpose
                .map_or(Cell::Blank, |purpose| Cell::Purpose(book.name(book.purposes[purpose.purpose].name)));
            let out = posting.out();
            let qty = view.flow_qty(flow, out.qty);
            (Cell::list(" ", [Cell::text(route(book, flow)), purpose]), Amount::new(qty, out.unit))
        }
        Source::Gap(pad) => {
            let (from, to) =
                if pad.amount.qty >= Qty::ZERO { (pad.counter, pad.place) } else { (pad.place, pad.counter) };
            let qty = view.place_qty(pad.place, pad.amount.qty).abs();
            (Cell::text(format!("{} → {}", path(book, from), path(book, to))), Amount::new(qty, pad.amount.unit))
        }
    };
    let counts = source.counts(cutoff);
    let note = note(book, view.run, source, counts).unwrap_or(Cell::Blank);
    let cells = [Cell::Day(day), flow, payee(book, source), note, Cell::amount(book, amount)];
    Row::new(cells).style(if counts { Style::Normal } else { Style::Muted })
}

/// Whether a flow is one the contract wrote or derived.
pub(crate) fn contract_flow(origin: Origin, contract: Id<Contract>) -> bool {
    match origin {
        Origin::Occurrence(found) => found == contract,
        Origin::Derived(
            Derivation::Interest(found)
            | Derivation::Principal(found)
            | Derivation::Opening(found)
            | Derivation::Claim(found)
            | Derivation::Otherwise(found)
            | Derivation::Refund(found),
        ) => found == contract,
        _ => false,
    }
}

fn place_owned_by(view: View<'_, '_, '_>, place: Id<Place>, entity: Id<Entity>) -> bool {
    view.plan().owners_of(place).iter().any(|owner| owner.owner == entity && !owner.share.is_zero())
}

fn section_with_sign<'s>(view: View<'s, '_, '_>, place: Id<Place>, window: Window) -> Section<'s> {
    let (book, sign) = (view.book(), view.display_sign(place));
    let steps = steps(view, place, window.cutoff);
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
    let mut running = Running::after(view, place, &steps[..split]);
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
        let (unit, amount, balance) = running.advance(view, place, step);
        section.push(step_row(book, view.run, step, shown(amount, unit), shown(balance, unit)));
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
    fn after(view: View<'_, '_, '_>, place: Id<Place>, steps: &[Step<'_>]) -> Running {
        let mut raw: BTreeMap<Id<Commodity>, Qty> = BTreeMap::new();
        for step in steps {
            let Change::Moved(moved) = step.change;
            *raw.entry(moved.unit).or_default() += step.counted();
        }
        let shown = raw.iter().map(|(&unit, &qty)| (unit, view.place_qty(place, qty))).collect();
        Running { raw, shown }
    }

    /// Takes one step: the commodity it moves, how much as the place shows it, and what the place then holds.
    fn advance(&mut self, view: View<'_, '_, '_>, place: Id<Place>, step: &Step<'_>) -> (Id<Commodity>, Qty, Qty) {
        let Change::Moved(moved) = step.change;
        let raw = self.raw.entry(moved.unit).or_default();
        let balance = self.shown.entry(moved.unit).or_default();
        let amount = if step.counts {
            let before = view.place_qty(place, *raw);
            *raw += moved.qty;
            let after = view.place_qty(place, *raw);
            *balance = after;
            after - before
        } else {
            view.place_qty(place, moved.qty)
        };
        (moved.unit, amount, *balance)
    }
}

fn step_row<'s>(book: &'s Book<'_>, run: &Run, step: &Step<'_>, amount: Cell<'s>, balance: Cell<'s>) -> Row<'s> {
    let note = note(book, run, &step.source, step.counts).unwrap_or(Cell::Blank);
    let cells =
        [Cell::Day(step.day), Cell::text(path(book, step.with)), payee(book, &step.source), note, amount, balance];
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

    /// Whether it is real at the end of the window: a gap always is.
    fn counts(&self, cutoff: Day) -> bool {
        self.posting().is_none_or(|posting| posting.is_real_on(cutoff))
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
fn steps<'a>(view: View<'a, '_, 'a>, place: Id<Place>, cutoff: Day) -> Vec<Step<'a>> {
    let book = view.book();
    let journal = book.touching[place].iter().map(|&id| Posting::at(book, view.run, id));
    let derived = (0..view.run.offspring.len()).map(|at| Posting::derived(view.run, Id::new(at as u32)));
    let derived = derived.filter(|posting| posting.flow.from == place || posting.flow.to == place);
    let flows = journal.chain(derived).flat_map(|posting| {
        let in_scope = view.owns(place);
        posting.changes_at(place).filter(move |_| in_scope).map(move |change| Step {
            day: posting.flow.day,
            change,
            counts: posting.is_real_on(cutoff),
            with: posting.counterparty(place),
            source: Source::Flow(posting),
            order: posting.sequence(view.run),
        })
    });
    let pads = view.run.pads.iter().flat_map(|pad| {
        let in_scope = view.owns(place) && view.governs(Subject::Place(pad.place));
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

/// Who a flow was paid to.
fn payee<'s>(book: &'s Book<'_>, source: &Source<'_>) -> Cell<'s> {
    let payee = source.posting().and_then(|posting| posting.flow.payee);
    payee.map_or(Cell::Blank, |entity| Cell::text(book.name(book.entities[entity].path)))
}

/// Codes, settlement and what derived it, as one line of small print: or where a gap came from.
fn note<'s>(book: &'s Book<'_>, run: &Run, source: &Source<'_>, counts: bool) -> Option<Cell<'s>> {
    let parts: Vec<Cell<'s>> = match *source {
        Source::Gap(pad) => vec![Cell::Said(gap_words(book, pad).into())],
        Source::Flow(posting) => {
            let status = match posting.posted.state {
                State::Actual | State::Planned => None,
                State::Pending => Some(Cell::Word("pending")),
                State::Void => Some(Cell::Word("void")),
                State::Settled(on) if !counts => Some(Cell::text(format!("pending until {on}"))),
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
