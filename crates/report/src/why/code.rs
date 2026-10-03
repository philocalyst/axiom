//! `why ^CODE`: the flows a code marks and the events that changed their state: a check cleared, a claim waived.

use std::collections::BTreeSet;

use axiom_core::glob::glob;
use axiom_core::{Diagnostic, Sym};
use axiom_engine::Run;
use axiom_model::{Amount, Book, ClaimChange};

use super::{event_words, flows_table};
use crate::history::postings;
use crate::lens::Lens;
use crate::resolve;
use crate::{Cell, Column, Report, Row, Section};

/// `pattern` may be a glob: `check-*`.
pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, pattern: &str) -> Result<Report<'s>, Diagnostic> {
    let book = lens.book();
    let marked = |code: Sym| glob(pattern, book.name(code));
    let flows: Vec<_> = postings(book, run)
        .filter(|posting| {
            lens.owns(crate::flow::movement_place(lens, posting.flow))
                && book.flow_view(posting.flow).codes().any(marked)
        })
        .map(|posting| posting.id)
        .collect();
    let visible_codes = super::line::scoped_codes(book, lens);
    let event_visible = |code| lens.whose.is_everyone() || visible_codes.contains(&code);
    let mut happened =
        Section::new([Column::left("Date"), Column::left("Event"), Column::left("From")]).headed("Events");
    for event in book.events.iter().filter(|event| event_visible(event.code) && marked(event.code)) {
        happened.push(Row::new([Cell::Day(event.day), Cell::text(event_words(event.state)), Cell::Source(event.loc)]));
    }

    for (at, change) in book.claim_changes.iter().enumerate() {
        let mut codes = book.codes[book.txns[change.target].codes].iter().copied();
        if codes.any(|code| event_visible(code) && marked(code)) {
            happened.push(waiver(book, run, at, change));
        }
    }

    if flows.is_empty() && happened.rows.is_empty() {
        let events = book.events.iter().filter(|event| event_visible(event.code)).map(|event| event.code);
        let known: BTreeSet<&str> = visible_codes.iter().copied().chain(events).map(|code| book.name(code)).collect();
        return Err(resolve::nothing_named("code", pattern, known));
    }
    Ok(Report::new(format!("Why ^{pattern}")).with(flows_table(lens, run, &flows, "Flows")).with(happened))
}

/// A claim written off: the day it was said, and what it forgave.
fn waiver<'s>(book: &Book<'s>, run: &Run, at: usize, change: &ClaimChange) -> Row<'s> {
    let forgiven: Vec<String> = run
        .written_off
        .iter()
        .filter(|off| off.change as usize == at)
        .map(|off| book.show(Amount::new(off.qty, off.unit)).to_string())
        .collect();
    let said = if forgiven.is_empty() {
        "waived, nothing was open".to_string()
    } else {
        format!("waived, {} forgiven", forgiven.join(", "))
    };
    Row::new([Cell::Day(change.day), Cell::text(said), Cell::Source(change.loc)])
}
