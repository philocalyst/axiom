//! `why "description"`: the written flows with that exact description.

use axiom_engine::Run;
use axiom_model::Book;

use super::flows_table;
use crate::history::postings;
use crate::lens::Whose;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(
    book: &Book<'s>,
    run: &Run,
    whose: &Whose,
    description: &str,
) -> Option<Report<'s>> {
    let ids = postings(book, run)
        .filter(|posting| {
            whose.includes(posting.flow.owner)
                && posting
                    .flow
                    .description
                    .is_some_and(|text| book.name(text) == description)
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
            Cell::Text(book.name(text).into()),
            Cell::Source(flow.loc),
        ]));
    }
    Some(
        Report::new(format!("Why \"{description}\""))
            .with(matches)
            .with(flows_table(book, run, &ids, "Flows")),
    )
}
