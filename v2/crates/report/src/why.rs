//! `why`: the provenance of a figure.
//!
//! Every report line points at the source that caused it; `why` explains that
//! source, and everything it in turn caused. A target is a place, a `#code`, a
//! law, or a tax line by name; the command line turns `file:line` into a
//! [`Query::Line`](crate::Query::Line).

mod code;
mod law;
mod line;
mod place;
mod taxline;

pub use self::line::line;

use axiom_core::{Diagnostic, Id};
use axiom_engine::Run;
use axiom_model::{Book, EventState, Law, Miss, Owner, Period, Place, Trigger};

use crate::Report;
use crate::places::names;
use crate::resolve;

pub fn target<'s>(book: &Book<'s>, run: &Run, text: &str) -> Result<Report<'s>, Diagnostic> {
    if let Some(code) = text.strip_prefix('#') {
        return code::report(book, run, code);
    }
    Ok(match identify(book, run, text)? {
        Found::Place(place) => place::report(book, run, place),
        Found::Law(law) => law::report(book, run, law),
        Found::TaxLine => taxline::report(book, run, text),
    })
}

enum Found {
    Place(Id<Place>),
    Law(Id<Law>),
    TaxLine,
}

/// A name is a place if it can be one, else a law, else something a law
/// tallied or owed. An ambiguous place is an error, not a reason to look on.
fn identify(book: &Book, run: &Run, text: &str) -> Result<Found, Diagnostic> {
    match book.place(text) {
        Ok(place) => return Ok(Found::Place(place)),
        Err(miss @ Miss::Ambiguous(_)) => return Err(resolve::place_miss(book, text, miss)),
        Err(Miss::Unknown { .. }) => {}
    }
    match book.law(text) {
        Ok(law) => return Ok(Found::Law(law)),
        Err(miss @ Miss::Ambiguous(_)) => return Err(resolve::law_miss(book, text, miss)),
        Err(Miss::Unknown { .. }) => {}
    }
    if run.effects.iter().any(|effect| book.name(effect.name) == text) {
        return Ok(Found::TaxLine);
    }
    let laws = book.laws.values().map(|law| book.name(law.name));
    let tallies = run.effects.iter().map(|effect| book.name(effect.name));
    Err(resolve::nothing_named("place, #code, law or tax line", text, names(book).chain(laws).chain(tallies)))
}

/// A doc block as plain lines: the `///` markers and one space of indent
/// removed (a doc that is already plain passes through).
fn doc_lines(doc: &str) -> impl Iterator<Item = &str> {
    doc.lines().map(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("///").unwrap_or(line);
        line.strip_prefix(' ').unwrap_or(line)
    })
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
        Trigger::Each(Period::Month) => "each month",
        Trigger::Each(Period::Year) => "each year",
        Trigger::By(_) => "by a date",
        Trigger::Always => "always",
    }
}

/// Who a law governs, as the sentence that explains it.
fn governs<'s>(book: &Book<'s>, owner: Owner) -> String {
    match owner {
        Owner::Kind(kind) => format!("every {}", book.name(book.kinds[kind].name)),
        Owner::Place(place) => format!("{} and everything beneath it", book.name(book.places[place].path)),
        Owner::Entity(entity) => format!("{}", book.name(book.entities[entity].path)),
        Owner::System(system) => {
            format!("everyone living under {}, and all they own", book.name(book.systems[system].path))
        }
    }
}
