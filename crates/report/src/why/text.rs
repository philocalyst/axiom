//! `why "description"`: the written flows with that exact description.

use super::flows_table;
use crate::history::postings;
use crate::lens::Lens;
use crate::{Cell, Column, Report, Row, Section};
use axiom_engine::Run;

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, description: &str) -> Option<Report<'s>> {
    let book = lens.book();
    let ids = postings(book, run)
        .filter(|posting| {
            lens.owns(crate::flow::movement_place(lens, posting.flow))
                && posting
                    .flow
                    .description
                    .is_some_and(|text| book.text(text) == description)
        })
        .map(|posting| posting.id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return None;
    }

    let mut matches = Section::new([
        Column::left("Date"),
        Column::left("Description"),
        Column::left("From"),
    ]);
    for &id in &ids {
        let flow = &book.flows[id];
        let Some(text) = flow.description else {
            continue;
        };
        matches.push(Row::new([
            Cell::Day(flow.day),
            Cell::Text(book.text(text).into()),
            Cell::Source(flow.loc),
        ]));
    }
    Some(
        Report::new(format!("Why \"{description}\""))
            .with(matches)
            .with(flows_table(lens, run, &ids, "Flows")),
    )
}
