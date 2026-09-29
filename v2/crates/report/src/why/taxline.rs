//! `why NAME` for a tally or obligation: every effect by that name, and the
//! flows and gains behind them.

use std::collections::BTreeSet;

use axiom_core::Id;
use axiom_engine::{Cause, Effect, Run};
use axiom_model::{Book, Flow};

use crate::history::Posting;
use crate::places::{path, route};
use crate::table::{cause_cell, creditor};
use crate::{Cell, Column, Report, Row, Section};

/// How many of each to list; older ones are counted in a note.
const LIMIT: usize = 20;

pub fn report<'s>(book: &Book<'s>, run: &Run, name: &str) -> Report<'s> {
    let effects: Vec<&Effect> = run.effects.iter().filter(|effect| book.name(effect.name) == name).collect();
    let flows: BTreeSet<Id<Flow>> = effects
        .iter()
        .filter_map(|effect| if let Cause::Flow(flow) = effect.cause { Some(flow) } else { None })
        .collect();

    let mut tallied = effects_section(book, &effects);
    if effects.iter().any(|effect| effect.cause == Cause::Time) {
        tallied.note("Effects from a period ending were computed from the tallies counted during it.");
    }
    Report::new(format!("Why {name}"))
        .with(tallied)
        .with(flows_section(book, run, &flows))
        .with(gains_section(book, run, &flows))
}

fn effects_section<'s>(book: &Book<'s>, effects: &[&Effect]) -> Section<'s> {
    let columns = [
        Column::left("Date"),
        Column::left("Owner"),
        Column::right("Amount"),
        Column::left("Owed to"),
        Column::left("From"),
    ];
    let mut section = Section::new(columns).headed("Effects");
    let shown = effects.len().saturating_sub(LIMIT);
    for effect in &effects[shown..] {
        let owed = effect.owe.map_or(Cell::Blank, |owed| Cell::text(creditor(book, owed)));
        let owner = Cell::text(book.name(book.entities[effect.owner].path));
        section.push(Row::new([
            Cell::Day(effect.day),
            owner,
            Cell::amount(book, effect.amount),
            owed,
            cause_cell(book, effect.cause),
        ]));
    }
    if shown > 0 {
        section.note(format!("{shown} earlier effects not shown."));
    }
    section
}

fn flows_section<'s>(book: &Book<'s>, run: &Run, flows: &BTreeSet<Id<Flow>>) -> Section<'s> {
    let mut section =
        Section::new([Column::left("Date"), Column::left("Flow"), Column::right("Amount"), Column::left("From")])
            .headed("Flows behind it");
    let shown = flows.len().saturating_sub(LIMIT);
    for &id in flows.iter().skip(shown) {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        section.push(Row::new([
            Cell::Day(flow.day),
            Cell::text(route(book, flow)),
            Cell::amount(book, posting.out()),
            Cell::Source(flow.loc),
        ]));
    }
    if shown > 0 {
        section.note(format!("{shown} earlier flows not shown."));
    }
    section
}

/// Gains realized by those flows: the taxable side of them.
fn gains_section<'s>(book: &Book<'s>, run: &Run, flows: &BTreeSet<Id<Flow>>) -> Section<'s> {
    let columns = [
        Column::left("Date"),
        Column::left("Sold from"),
        Column::right("Proceeds"),
        Column::right("Basis"),
        Column::right("Gain"),
        Column::left("Held since"),
    ];
    let mut section = Section::new(columns).headed("Gains realized");
    let behind = run.gains.iter().filter(|gain| matches!(gain.cause, Cause::Flow(flow) if flows.contains(&flow)));
    for gain in behind {
        let cells = [
            Cell::Day(gain.day),
            Cell::text(path(book, gain.from)),
            Cell::base(book, gain.proceeds),
            Cell::base(book, gain.basis),
            Cell::base(book, gain.gain()),
            Cell::Day(gain.acquired),
        ];
        section.push(Row::new(cells));
    }
    section
}
