//! `why`: the provenance of a figure.
//!
//! Every report line points at the source that caused it; `why` explains that
//! source, and everything it in turn caused. A target is a place, an entity, a
//! system, a `#code`, a law, or a tax line by name; the command line turns
//! `file:line` into a [`Query::Line`](crate::Query::Line).

mod code;
mod entity;
mod law;
mod line;
mod place;
mod system;
mod taxline;

pub use self::line::line;

use axiom_core::{Diagnostic, Id, Sym};
use axiom_engine::Run;
use axiom_model::{Book, Effect, Entity, EventState, Law, Miss, Period, Place, StepKind, System, Trigger};

use crate::lens::Whose;
use crate::places::names;
use crate::resolve;
use crate::table::{doc_headline, plural};
use crate::{Cell, Column, Report, Row, Section};

pub fn target<'s>(book: &Book<'s>, run: &Run, whose: &Whose, text: &str) -> Result<Report<'s>, Diagnostic> {
    if let Some(code) = text.strip_prefix('#') {
        return code::report(book, run, code);
    }
    Ok(explain(book, run, whose, identify(book, run, text)?))
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

pub(crate) fn explain<'s>(book: &Book<'s>, run: &Run, whose: &Whose, found: Found) -> Report<'s> {
    match found {
        Found::Place(place) => place::report(book, run, place),
        Found::Entity(entity) => entity::report(book, run, entity),
        Found::System(system) => system::report(book, run, whose, system),
        Found::Law(law) => law::report(book, run, law),
        Found::Laws(candidates) => law::which(book, &candidates),
        Found::TaxLine(name) => taxline::report(book, run, name),
    }
}

/// A name is a place if it can be one, else an entity, a system, a law, or
/// something a law tallied or owed. An ambiguous place is an error, not a
/// reason to look on. An entity that only stands in for its `via` place is
/// asked about as the entity: its own laws and ties are what was wanted.
fn identify<'a>(book: &Book, run: &Run, text: &'a str) -> Result<Found<'a>, Diagnostic> {
    let entity = book.entity(text).ok();
    match book.place(text) {
        Ok(place) => {
            return Ok(match entity {
                Some(entity) if book.entities[entity].via == Some(place) => Found::Entity(entity),
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
        path == text || path.strip_suffix(text).is_some_and(|before| before.ends_with('/'))
    };
    if let Some((system, _)) = book.systems.iter().find(|(_, system)| named(system)) {
        return Ok(Found::System(system));
    }
    match book.law(text) {
        Ok(law) => return Ok(Found::Law(law)),
        Err(Miss::Ambiguous(candidates)) => return Ok(Found::Laws(candidates)),
        Err(Miss::Unknown { .. }) => {}
    }
    if run.effects.iter().any(|effect| book.name(effect.name) == text) {
        return Ok(Found::TaxLine(text));
    }
    let laws = book.laws.values().map(|law| book.name(law.name));
    let tallies = run.effects.iter().map(|effect| book.name(effect.name));
    let things = names(book).chain(book.entities.values().map(|entity| book.name(entity.path)));
    Err(resolve::nothing_named(
        "place, entity, system, #code, law or tax line",
        text,
        things.chain(laws).chain(tallies),
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
        match step.kind {
            StepKind::Require { otherwise: Some(_), .. } | StepKind::Effect(Effect::Owe { .. }) => return Role::Price,
            StepKind::Require { .. } => role = Role::Limit,
            _ => {}
        }
    }
    role
}

/// The tallies a law counts into.
fn counted(law: &Law) -> impl Iterator<Item = Sym> + '_ {
    law.steps.iter().filter_map(|step| match step.kind {
        StepKind::Effect(Effect::Count { name, .. }) => Some(name),
        _ => None,
    })
}

/// The laws as one table, grouped by what they do: limits, prices, and the
/// tallies, which are one line however many laws there are.
fn laws_table<'s>(book: &Book<'s>, ids: &[Id<Law>]) -> Section<'s> {
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
fn trigger_words(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::In => "on in",
        Trigger::Out => "on out",
        Trigger::Gain => "on gain",
        Trigger::Spend => "on spend",
        Trigger::Each(Period::Month, _) => "each month",
        Trigger::Each(Period::Year, _) => "each year",
        Trigger::By(_) => "by a date",
        Trigger::Always => "always",
    }
}
