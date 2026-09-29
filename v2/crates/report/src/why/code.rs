//! `why #CODE`: the flows a code marks and the events that changed their state.

use std::borrow::Cow;
use std::collections::BTreeSet;

use axiom_core::glob::glob;
use axiom_core::{Diagnostic, Sym};
use axiom_engine::{Run, State};
use axiom_model::Book;

use super::event_words;
use crate::history::postings;
use crate::places::route;
use crate::resolve;
use crate::{Cell, Column, Report, Row, Section};

/// `pattern` may be a glob: `check-*`.
pub fn report<'s>(book: &Book<'s>, run: &Run, pattern: &str) -> Result<Report<'s>, Diagnostic> {
    let marked = |code: Sym| glob(pattern, bare(book.name(code)));
    let flows = postings(book, run).filter(|posting| posting.flow.codes.iter().any(|&code| marked(code)));
    let events = book.events.iter().filter(|event| marked(event.code));

    let columns = [
        Column::left("Date"),
        Column::left("Flow"),
        Column::right("Amount"),
        Column::left("State"),
        Column::left("From"),
    ];
    let mut linked = Section::new(columns).headed("Flows");
    for posting in flows {
        let flow = posting.flow;
        let cells = [
            Cell::Day(flow.day),
            Cell::text(route(book, flow)),
            Cell::amount(book, posting.out()),
            Cell::text(state_words(posting.posted.state)),
            Cell::Source(flow.loc),
        ];
        linked.push(Row::new(cells));
    }

    let mut happened =
        Section::new([Column::left("Date"), Column::left("Event"), Column::left("From")]).headed("Events");
    for event in events {
        happened.push(Row::new([Cell::Day(event.day), Cell::text(event_words(event.state)), Cell::Source(event.loc)]));
    }

    if linked.rows.is_empty() && happened.rows.is_empty() {
        let known: BTreeSet<&str> = book
            .flows
            .values()
            .flat_map(|flow| flow.codes.iter())
            .chain(book.events.iter().map(|event| &event.code))
            .map(|&code| bare(book.name(code)))
            .collect();
        return Err(resolve::nothing_named("code", pattern, known));
    }
    Ok(Report::new(format!("Why #{pattern}")).with(linked).with(happened))
}

/// Codes may be stored with or without their `#`.
fn bare(code: &str) -> &str {
    code.trim_start_matches('#')
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
