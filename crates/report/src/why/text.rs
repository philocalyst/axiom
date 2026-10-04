//! `why "description"`: the written flows with that exact description.

use axiom_core::Id;
use axiom_model::Flow;

use super::flows_table;
use crate::history::{Posting, postings};
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section};

/// The written flows of the view's owners whose description is exactly `description`.
pub fn flows(view: View<'_, '_, '_>, description: &str) -> Vec<Id<Flow>> {
    let book = view.book();
    let described = |posting: &crate::history::Posting<'_>| {
        view.owns_flow(posting.flow) && posting.flow.description.is_some_and(|text| book.text(text) == description)
    };
    postings(book, view.run).filter(described).filter_map(|posting| posting.id.journal()).collect()
}

pub fn report<'s>(view: View<'s, '_, '_>, description: &str, ids: &[Id<Flow>]) -> Report<'s> {
    let book = view.book();
    let mut matches = Section::new([Column::left("Date"), Column::left("Description"), Column::left("From")]);
    for &id in ids {
        let flow = &book.flows[id];
        let Some(text) = flow.description else {
            continue;
        };
        matches.push(Row::new([Cell::Day(flow.day), Cell::Text(book.text(text).into()), Cell::Source(flow.loc)]));
    }
    let flows = ids.iter().map(|&id| Posting::at(book, view.run, id));
    Report::new(format!("Why \"{description}\"")).with(matches).with(flows_table(view, flows, "Flows"))
}
