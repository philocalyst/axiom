//! `why #CODE`: the flows a code marks and the events that changed their state.

use std::collections::BTreeSet;

use axiom_core::glob::glob;
use axiom_core::{Diagnostic, Sym};
use axiom_engine::Run;
use axiom_model::Book;

use super::{event_words, flows_table};
use crate::history::postings;
use crate::resolve;
use crate::{Cell, Column, Report, Row, Section};

/// `pattern` may be a glob: `check-*`.
pub fn report<'s>(book: &Book<'s>, run: &Run, pattern: &str) -> Result<Report<'s>, Diagnostic> {
    let marked = |code: Sym| glob(pattern, bare(book.name(code)));
    let flows: Vec<_> = postings(book, run)
        .filter(|posting| posting.flow.codes.iter().any(|&code| marked(code)))
        .map(|posting| posting.id)
        .collect();
    let mut happened =
        Section::new([Column::left("Date"), Column::left("Event"), Column::left("From")]).headed("Events");
    for event in book.events.iter().filter(|event| marked(event.code)) {
        happened.push(Row::new([Cell::Day(event.day), Cell::text(event_words(event.state)), Cell::Source(event.loc)]));
    }

    if flows.is_empty() && happened.rows.is_empty() {
        let events = book.events.iter().map(|event| &event.code);
        let known: BTreeSet<&str> = book
            .flows
            .values()
            .flat_map(|flow| flow.codes.iter())
            .chain(events)
            .map(|&code| bare(book.name(code)))
            .collect();
        return Err(resolve::nothing_named("code", pattern, known));
    }
    Ok(Report::new(format!("Why #{pattern}")).with(flows_table(book, run, &flows, "Flows")).with(happened))
}

/// Codes may be stored with or without their `#`.
fn bare(code: &str) -> &str {
    code.trim_start_matches('#')
}
