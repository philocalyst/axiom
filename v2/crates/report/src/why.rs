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

pub use self::line::line;

use std::borrow::Cow;

use axiom_core::{Diagnostic, Id, Sym};
use axiom_engine::{Effect, Run, State};
use axiom_model::{
    Amount, Book, Closing, Effect as Consequence, Entity, EventState, Flow, Law, Miss, Period,
    Place, StepKind, System, Trigger,
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
fn flows_table<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    ids: &[Id<Flow>],
    heading: &str,
) -> Section<'s> {
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
                Amount::new(
                    lens.entity_qty(flow.owner, posting.out().qty),
                    posting.out().unit,
                ),
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
    let mut section = Section::new(
        columns
            .chain([Column::right("Amount")])
            .chain(["Owed to", "From"].map(Column::left)),
    );
    section.heading = Some(Cell::Said(Cow::Owned(heading.to_owned())));
    let (shown, left_out) = recent(effects);
    for effect in shown {
        let owed = effect
            .owed()
            .map_or(Cell::Blank, |owed| Cell::text(creditor(book, owed)));
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

pub(crate) fn target_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    text: &str,
) -> Result<Report<'s>, Diagnostic> {
    let book = lens.book();
    if let Some(contract) = text.strip_prefix("contract:") {
        let contract = resolve::contract(book, contract)?;
        return Ok(contract::report(lens, run, contract));
    }
    if let Some(entity) = text.strip_prefix("entity:") {
        let entity = resolve::entity(book, entity)?;
        return Ok(entity::report(lens, run, entity));
    }
    if let Some(asset) = text.strip_prefix("asset:") {
        let asset = resolve::asset(book, asset)?;
        return Ok(asset::report(lens, run, asset));
    }
    if let Some(code) = text.strip_prefix('^') {
        return code::report(lens, run, code);
    }
    if let Some(purpose) = text.strip_prefix('#') {
        return purpose::report(lens, run, purpose);
    }
    if let Some(asset) = book.asset(text) {
        return Ok(asset::report(lens, run, asset));
    }
    if let Some(contract) = book.contract(text) {
        return Ok(contract::report(lens, run, contract));
    }
    let quoted = text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text);
    if let Some(report) = self::text::report(lens, run, quoted) {
        return Ok(report);
    }
    Ok(explain_with_lens(lens, run, identify(book, run, text)?))
}

/// What a name means, once found.
pub(crate) enum Found<'a> {
    Place(Id<Place>),
    Entity(Id<Entity>),
    System(Id<System>),
    Law(Id<Law>),
    /// Several laws share the name.
    Laws(Box<[Id<Law>]>),
    /// A name some law counted or owed under.
    TaxLine(&'a str),
}

pub(crate) fn explain_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    found: Found,
) -> Report<'s> {
    let book = lens.book();
    match found {
        Found::Place(place) => place::report(lens, run, place),
        Found::Entity(entity) => entity::report(lens, run, entity),
        Found::System(system) => system::report(lens, run, system),
        Found::Law(law) => law::report(lens, run, law),
        Found::Laws(candidates) => law::which(book, &candidates),
        Found::TaxLine(name) => taxline::report(lens, run, name),
    }
}

pub(crate) fn line_with_lens<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    at: axiom_core::Loc,
) -> Report<'s> {
    line::line(lens, run, at)
}

/// A name is a place if it can be one, else an entity, a system, a law, or
/// something a law tallied or owed. An ambiguous place is an error, not a
/// reason to look on. An entity that only stands in for its place is
/// asked about as the entity: its own laws and ties are what was wanted.
fn identify<'a>(book: &Book, run: &Run, text: &'a str) -> Result<Found<'a>, Diagnostic> {
    let entity = book.entity(text).ok();
    match book.place(text) {
        Ok(place) => {
            return Ok(match entity {
                Some(entity) if book.entities[entity].place == Some(place) => Found::Entity(entity),
                _ => Found::Place(place),
            });
        }
        Err(miss @ Miss::Ambiguous(_)) => return Err(resolve::place_miss(book, text, miss)),
        Err(Miss::Unknown { .. }) => {}
    }
    if let Some(entity) = entity {
        return Ok(Found::Entity(entity));
    }
    let named = |system: &System| {
        let path = book.name(system.path);
        path == text
            || path
                .strip_suffix(text)
                .is_some_and(|before| before.ends_with('/'))
    };
    if let Some((system, _)) = book.systems.iter().find(|(_, system)| named(system)) {
        return Ok(Found::System(system));
    }
    match book.law(text) {
        Ok(law) => return Ok(Found::Law(law)),
        Err(Miss::Ambiguous(candidates)) => return Ok(Found::Laws(candidates)),
        Err(Miss::Unknown { .. }) => {}
    }
    if run
        .effects
        .iter()
        .any(|effect| book.name(effect.name) == text)
    {
        return Ok(Found::TaxLine(text));
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
            .chain(
                book.purposes
                    .values()
                    .map(|purpose| book.name(purpose.name)),
            )
            .chain(book.assets.values().map(|asset| book.name(asset.name)))
            .chain(
                book.contracts
                    .values()
                    .map(|contract| book.name(contract.name)),
            ),
    ))
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
    let columns = [
        Column::left("Law"),
        Column::left("When"),
        Column::left("Explains"),
        Column::left("Written"),
    ];
    let mut section = Section::new(columns).headed("Governed by");
    let mut unique: Vec<Id<Law>> = Vec::new();
    for &id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    for (wanted, heading) in [(Role::Limit, "Limits"), (Role::Price, "Prices")] {
        let group: Vec<&Id<Law>> = unique
            .iter()
            .filter(|&&id| role(&book.laws[id]) == wanted)
            .collect();
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
    let tallies: Vec<&Law> = unique
        .iter()
        .map(|&id| &book.laws[id])
        .filter(|law| role(law) == Role::Tally)
        .collect();
    if !tallies.is_empty() {
        let mut names: Vec<&str> = tallies
            .iter()
            .flat_map(|law| counted(law))
            .map(|name| book.name(name))
            .collect();
        names.sort_unstable();
        names.dedup();
        section.note(format!(
            "{} only count, into {}.",
            plural(tallies.len(), "more law"),
            names.join(", ")
        ));
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
