//! `why "description"`: the written flows with that exact description.

use axiom_core::Id;
use axiom_engine::Run;
use axiom_model::Flow;

use super::flows_table;
use crate::history::postings;
use crate::lens::Lens;
use crate::{Cell, Column, Report, Row, Section};

/// The written flows of the lens's owners whose description is exactly `description`.
pub fn flows(lens: Lens<'_, '_, '_, '_>, run: &Run, description: &str) -> Vec<Id<Flow>> {
    let book = lens.book();
    let described = |posting: &crate::history::Posting<'_>| {
        lens.owns(crate::flow::movement_place(lens, posting.flow))
            && posting.flow.description.is_some_and(|text| book.text(text) == description)
    };
    postings(book, run).filter(described).filter_map(|posting| posting.id.journal()).collect()
}

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, description: &str, ids: &[Id<Flow>]) -> Report<'s> {
    let book = lens.book();
    let mut matches = Section::new([Column::left("Date"), Column::left("Description"), Column::left("From")]);
    for &id in ids {
        let flow = &book.flows[id];
        let Some(text) = flow.description else {
            continue;
        };
        matches.push(Row::new([Cell::Day(flow.day), Cell::Text(book.text(text).into()), Cell::Source(flow.loc)]));
    }
    Report::new(format!("Why \"{description}\"")).with(matches).with(flows_table(lens, run, ids, "Flows"))
}
