//! `why ^CODE`: the flows a code marks and the events that changed their state.

use std::collections::BTreeSet;

use axiom_core::glob::glob;
use axiom_core::{Diagnostic, Sym};
use axiom_engine::Run;
use axiom_model::Book;

use super::{event_words, flows_table};
use crate::history::postings;
use crate::lens::Whose;
use crate::resolve;
use crate::{Cell, Column, Report, Row, Section};

/// `pattern` may be a glob: `check-*`.
pub fn report<'s>(
    book: &'s Book<'_>,
    run: &Run,
    whose: &Whose,
    pattern: &str,
) -> Result<Report<'s>, Diagnostic> {
    let marked = |code: Sym| glob(pattern, book.name(code));
    let flows: Vec<_> = postings(book, run)
        .filter(|posting| {
            whose.includes(posting.flow.owner) && book.flow_view(posting.flow).codes().any(marked)
        })
        .map(|posting| posting.id)
        .collect();
    let visible_codes = super::line::scoped_codes(book, whose);
    let event_visible = |code| whose.is_everyone() || visible_codes.contains(&code);
    let mut happened = Section::new([
        Column::left("Date"),
        Column::left("Event"),
        Column::left("From"),
    ])
    .headed("Events");
    for event in book
        .events
        .iter()
        .filter(|event| event_visible(event.code) && marked(event.code))
    {
        happened.push(Row::new([
            Cell::Day(event.day),
            Cell::text(event_words(event.state)),
            Cell::Source(event.loc),
        ]));
    }

    if flows.is_empty() && happened.rows.is_empty() {
        let events = book.events.iter().filter(|event| event_visible(event.code)).map(|event| event.code);
        let known: BTreeSet<&str> = visible_codes
            .iter()
            .copied()
            .chain(events)
            .map(|code| book.name(code))
            .collect();
        return Err(resolve::nothing_named("code", pattern, known));
    }
    Ok(Report::new(format!("Why ^{pattern}"))
        .with(flows_table(book, run, &flows, "Flows"))
        .with(happened))
}
