//! `why`: the provenance of a figure.
//!
//! Every report line points at the source that caused it; `why` explains that
//! source, and everything it in turn caused. A target is a place, an entity, a
//! system, a `^code`, `#purpose`, an asset, a contract, a law, a tax line, or a source position resolved
//! through the caller's borrowed source provider.

mod asset;
mod code;
mod contract;
mod entity;
mod law;
mod line;
mod place;
mod purpose;
mod system;
mod taxline;
mod text;

use std::borrow::Cow;

use axiom_core::{Diagnostic, Id, Sym};
use axiom_engine::{Effect, Run, State};
use axiom_model::{
    Amount, Asset, Book, Closing, Contract, Effect as Consequence, Entity, EventState, Flow, Law, Miss, Period, Place,
    StepKind, System, Trigger,
};

use crate::history::Posting;
use crate::lens::Lens;
use crate::places::{names, route};
use crate::resolve;
use crate::table::{cause_cell, creditor, doc_headline, plural};
use crate::{Cell, Column, Report, Row, Section};

/// How many of the latest records a page lists; the rest are counted in a note.
const RECENT: usize = 12;

/// The latest `RECENT` of `items`, and how many earlier ones were left out.
fn recent<T>(items: &[T]) -> (&[T], usize) {
    let left_out = items.len().saturating_sub(RECENT);
    (&items[left_out..], left_out)
}

/// Flows, dated, with where each stands.
fn flows_table<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, ids: &[Id<Flow>], heading: &str) -> Section<'s> {
    let book = lens.book();
    let columns = [
        Column::left("Date"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("State"),
        Column::left("From"),
    ];
    let mut section = Section::new(columns).headed(Cell::Said(Cow::Owned(heading.to_owned())));
    let (shown, left_out) = recent(ids);
    for &id in shown {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        let cells = [
            Cell::Day(flow.day),
            Cell::text(route(book, flow)),
            Cell::amount(
                book,
                Amount::new(crate::flow::scoped_movement_qty(lens, flow, posting.out().qty), posting.out().unit),
            ),
            Cell::text(state_words(posting.posted.state)),
            Cell::Source(flow.loc),
        ];
        section.push(Row::new(cells));
    }
    if left_out > 0 {
        section.note(format!("{left_out} earlier flows not shown."));
    }
    section
}

fn state_words(state: State) -> Cow<'static, str> {
    match state {
        State::Actual => "actual".into(),
        State::Pending => "pending".into(),
        State::Settled(on) => format!("settled {on}").into(),
        State::Void => "void".into(),
        State::Returned(on) => format!("returned {on}").into(),
        State::Planned => "planned".into(),
    }
}

/// What laws counted or owed, when, and for whom.
fn effects_table<'s>(book: &'s Book<'_>, effects: &[&Effect], heading: &str) -> Section<'s> {
    let columns = ["Date", "Effect", "Owner"].map(Column::left).into_iter();
    let mut section =
        Section::new(columns.chain([Column::right("Amount")]).chain(["Owed to", "From"].map(Column::left)));
    section.heading = Some(Cell::Said(Cow::Owned(heading.to_owned())));
    let (shown, left_out) = recent(effects);
    for effect in shown {
        let owed = effect.owed().map_or(Cell::Blank, |owed| Cell::text(creditor(book, owed)));
        let cells = [
            Cell::Day(effect.day),
            Cell::text(book.name(effect.name)),
            Cell::text(book.name(book.entities[effect.owner].path)),
            Cell::amount(book, effect.amount),
            owed,
            cause_cell(book, effect.cause),
        ];
        section.push(Row::new(cells));
    }
    if left_out > 0 {
        section.note(format!("{left_out} earlier effects not shown."));
    }
    section
}

/// What a `why` is about, once what was typed has been found.
pub(crate) enum Target<'a> {
    Place(Id<Place>),
    Entity(Id<Entity>),
    System(Id<System>),
    Law(Id<Law>),
    /// Several laws share the name.
    Laws(Box<[Id<Law>]>),
    /// A name some law counted or owed under.
    TaxLine(&'a str),
    Asset(Id<Asset>),
    Contract(Id<Contract>),
    /// `^pattern`: the flows a code marks, and the events that changed them.
    Code(&'a str),
    /// `#name`: a purpose, which its page looks up and says if there is none.
    Purpose(&'a str),
    /// A description, and the written flows of the lens's owners that have exactly it.
    Description(&'a str, Vec<Id<Flow>>),
}

impl<'a> Target<'a> {
    /// What `text` asks about. A prefix says which kind of thing (`contract:`, `entity:`, `asset:`, `^`, `#`); a bare word is
    /// an asset, a contract, the description of some flow, and then a place, an entity, a system, a law or a tax line, in
    /// that order, the first that it names.
    pub fn of(lens: Lens<'_, '_, '_, '_>, run: &Run, text: &'a str) -> Result<Target<'a>, Diagnostic> {
        let book = lens.book();
        if let Some(name) = text.strip_prefix("contract:") {
            return Ok(Target::Contract(resolve::contract(book, name)?));
        }
        if let Some(name) = text.strip_prefix("entity:") {
            return Ok(Target::Entity(resolve::entity(book, name)?));
        }
        if let Some(name) = text.strip_prefix("asset:") {
            return Ok(Target::Asset(resolve::asset(book, name)?));
        }
        if let Some(pattern) = text.strip_prefix('^') {
            return Ok(Target::Code(pattern));
        }
        if let Some(name) = text.strip_prefix('#') {
            return Ok(Target::Purpose(name));
        }
        if let Some(asset) = book.asset(text) {
            return Ok(Target::Asset(asset));
        }
        if let Some(contract) = book.contract(text) {
            return Ok(Target::Contract(contract));
        }
        let quoted = text.strip_prefix('"').and_then(|text| text.strip_suffix('"')).unwrap_or(text);
        let described = self::text::flows(lens, run, quoted);
        if !described.is_empty() {
            return Ok(Target::Description(quoted, described));
        }
        Target::named(book, run, text)
    }

    /// A name is a place if it can be one, else an entity, a system, a law, or something a law tallied or owed. An ambiguous
    /// place is an error, not a reason to look on. An entity that only stands in for its place is asked about as the entity:
    /// its own laws and ties are what was wanted.
    fn named(book: &Book, run: &Run, text: &'a str) -> Result<Target<'a>, Diagnostic> {
        let entity = book.entity(text).ok();
        match book.place(text) {
            Ok(place) => {
                return Ok(match entity {
                    Some(entity) if book.entities[entity].place == Some(place) => Target::Entity(entity),
                    _ => Target::Place(place),
                });
            }
            Err(miss @ Miss::Ambiguous(_)) => return Err(resolve::place_miss(book, text, miss)),
            Err(Miss::Unknown { .. }) => {}
        }
        if let Some(entity) = entity {
            return Ok(Target::Entity(entity));
        }
        let named = |system: &System| {
            let path = book.name(system.path);
            path == text || path.strip_suffix(text).is_some_and(|before| before.ends_with('/'))
        };
        if let Some((system, _)) = book.systems.iter().find(|(_, system)| named(system)) {
            return Ok(Target::System(system));
        }
        match book.law(text) {
            Ok(law) => return Ok(Target::Law(law)),
            Err(Miss::Ambiguous(candidates)) => return Ok(Target::Laws(candidates)),
            Err(Miss::Unknown { .. }) => {}
        }
        if run.effects.iter().any(|effect| book.name(effect.name) == text) {
            return Ok(Target::TaxLine(text));
        }
        let laws = book.laws.values().map(|law| book.name(law.name));
        let tallies = run.effects.iter().map(|effect| book.name(effect.name));
        let things = names(book).chain(book.entities.values().map(|entity| book.name(entity.path)));
        Err(resolve::nothing_named(
            "place, entity:NAME, system, ^code, #purpose, asset:NAME, contract:NAME, law, tax line or description",
            text,
            things
                .chain(laws)
                .chain(tallies)
                .chain(book.purposes.values().map(|purpose| book.name(purpose.name)))
                .chain(book.assets.values().map(|asset| book.name(asset.name)))
                .chain(book.contracts.values().map(|contract| book.name(contract.name))),
        ))
    }

    /// The page about it.
    pub fn report<'s>(self, lens: Lens<'s, '_, '_, '_>, run: &Run) -> Result<Report<'s>, Diagnostic> {
        let book = lens.book();
        Ok(match self {
            Target::Place(place) => place::report(lens, run, place),
            Target::Entity(entity) => entity::report(lens, run, entity),
            Target::System(system) => system::report(lens, run, system),
            Target::Law(law) => law::report(lens, run, law),
            Target::Laws(candidates) => law::which(book, &candidates),
            Target::TaxLine(name) => taxline::report(lens, run, name),
            Target::Asset(asset) => asset::report(lens, run, asset),
            Target::Contract(contract) => contract::report(lens, run, contract),
            Target::Code(pattern) => return code::report(lens, run, pattern),
            Target::Purpose(name) => return purpose::report(lens, run, name),
            Target::Description(description, flows) => self::text::report(lens, run, description, &flows),
        })
    }
}

pub(crate) fn line_with_lens<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: axiom_core::Loc) -> Report<'s> {
    line::line(lens, run, at)
}

/// What a law does to a move: forbids it or warns of it (a limit), puts a
/// price on it, or only counts it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Limit,
    Price,
    Tally,
}

fn role(law: &Law) -> Role {
    let mut role = Role::Tally;
    for step in law.steps.iter() {
        match &step.kind {
            StepKind::Require { otherwise, .. } if !otherwise.is_empty() => return Role::Price,
            StepKind::Effect(Consequence::Owe { .. }) => return Role::Price,
            StepKind::Require { .. } => role = Role::Limit,
            _ => {}
        }
    }
    role
}

/// The tallies a law counts into.
fn counted(law: &Law) -> impl Iterator<Item = Sym> + '_ {
    law.steps.iter().filter_map(|step| match step.kind {
        StepKind::Effect(Consequence::Count { name, .. }) => Some(name),
        _ => None,
    })
}

/// The laws as one table, grouped by what they do: limits, prices, and the
/// tallies, which are one line however many laws there are.
fn laws_table<'s>(book: &'s Book<'_>, ids: &[Id<Law>]) -> Section<'s> {
    let columns = [Column::left("Law"), Column::left("When"), Column::left("Explains"), Column::left("Written")];
    let mut section = Section::new(columns).headed("Governed by");
    let mut unique: Vec<Id<Law>> = Vec::new();
    for &id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    for (wanted, heading) in [(Role::Limit, "Limits"), (Role::Price, "Prices")] {
        let group: Vec<&Id<Law>> = unique.iter().filter(|&&id| role(&book.laws[id]) == wanted).collect();
        if group.is_empty() {
            continue;
        }
        section.push(Row::padded([Cell::text(heading)], 4).style(crate::Style::Total));
        for &id in group {
            let law = &book.laws[id];
            let explains = doc_headline(book, law.doc).map_or(Cell::Blank, Cell::text);
            let cells = [
                Cell::text(book.name(law.name)),
                Cell::text(trigger_words(law.trigger)),
                explains,
                Cell::Source(law.loc),
            ];
            section.push(Row::new(cells).depth(1));
        }
    }
    let tallies: Vec<&Law> = unique.iter().map(|&id| &book.laws[id]).filter(|law| role(law) == Role::Tally).collect();
    if !tallies.is_empty() {
        let mut names: Vec<&str> = tallies.iter().flat_map(|law| counted(law)).map(|name| book.name(name)).collect();
        names.sort_unstable();
        names.dedup();
        section.note(format!("{} only count, into {}.", plural(tallies.len(), "more law"), names.join(", ")));
    }
    section
}

/// What an event did to its flows, in the word it is written with.
fn event_words(state: EventState) -> &'static str {
    match state {
        EventState::Settled => "settled",
        EventState::Void => "void",
        EventState::Returned => "returned",
    }
}

/// When a law fires, in the words it is written with.
fn trigger_words(trigger: Trigger) -> Cow<'static, str> {
    match trigger {
        Trigger::In => "on in".into(),
        Trigger::Out => "on out".into(),
        Trigger::Gain => "on gain".into(),
        Trigger::Spend => "on spend".into(),
        Trigger::Flow => "on flow".into(),
        Trigger::Each(Period::Month, _) => "each month".into(),
        Trigger::Each(Period::Year, None) => "each year".into(),
        Trigger::Each(Period::Year, Some(Closing { month, day })) => {
            format!("each year closing {month:02}-{day:02}").into()
        }
        Trigger::By(_) => "by a date".into(),
        Trigger::Always => "always".into(),
    }
}
