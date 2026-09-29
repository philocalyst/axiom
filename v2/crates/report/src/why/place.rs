//! `why PLACE`: what it holds and how, which laws govern it, what touched it lately.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty};
use axiom_engine::{Holding, Run};
use axiom_model::{Amount, Book, Commodity, Place, Rule, Subject};

use super::{doc_lines, governs, trigger_words};
use crate::places::path;
use crate::register;
use crate::{Cell, Column, Report, Row, Section};

/// How many recent flows to show.
const RECENT: usize = 8;

pub fn report<'s>(book: &Book<'s>, run: &Run, place: Id<Place>) -> Report<'s> {
    let held: Vec<&Holding> = run.holdings.iter().filter(|holding| book.places.covers(place, holding.place)).collect();
    let recent_from = book.touching[place].iter().rev().nth(RECENT - 1).map(|&flow| book.flows[flow].day);
    Report::new(format!("Why {}", path(book, place)))
        .with(composition(book, &held))
        .with(parcels(book, &held))
        .with(laws(book, place))
        .with(register::section(book, run, place, recent_from, None).headed("Recent flows"))
}

/// What is held, by commodity, and how much of it is plain money.
fn composition<'s>(book: &Book<'s>, held: &[&Holding]) -> Section<'s> {
    let mut units: BTreeMap<Id<Commodity>, (Qty, Qty, usize)> = BTreeMap::new();
    for holding in held {
        let (total, plain, parcels) = units.entry(holding.unit).or_default();
        *total += holding.qty();
        *plain += holding.plain;
        *parcels += holding.lots.len();
    }
    let columns =
        [Column::left("Holds"), Column::right("Quantity"), Column::right("Plain money"), Column::right("Parcels")];
    let mut section = Section::new(columns).headed("Composition");
    for (unit, (total, plain, parcels)) in units {
        let cells = [
            Cell::text(book.name(book.commodities[unit].symbol)),
            Cell::amount(book, Amount::new(total, unit)),
            if plain.is_zero() { Cell::Blank } else { Cell::amount(book, Amount::new(plain, unit)) },
            if parcels == 0 { Cell::Blank } else { Cell::text(parcels.to_string()) },
        ];
        section.push(Row::new(cells));
    }
    section
}

/// Parcels: what is remembered about value at rest, and the line that brought it.
fn parcels<'s>(book: &Book<'s>, held: &[&Holding]) -> Section<'s> {
    let columns = [
        Column::left("Place"),
        Column::right("Quantity"),
        Column::right("Basis"),
        Column::left("Acquired"),
        Column::left("Tied to"),
        Column::left("From"),
    ];
    let mut section = Section::new(columns).headed("Parcels");
    for holding in held {
        for lot in &holding.lots {
            let tie = lot.tied.map_or(Cell::Blank, |entity| Cell::text(book.name(book.entities[entity].path)));
            let cells = [
                Cell::text(path(book, holding.place)),
                Cell::amount(book, Amount::new(lot.qty, holding.unit)),
                Cell::base(book, lot.basis),
                Cell::Day(lot.acquired),
                tie,
                Cell::Source(book.txns[lot.txn].loc),
            ];
            section.push(Row::new(cells));
        }
    }
    section
}

/// The laws that govern the place, resolved once by the model: its own and
/// its ancestors', its kind's, and its owner's jurisdictions'.
fn laws<'s>(book: &Book<'s>, place: Id<Place>) -> Section<'s> {
    let rules = &book.rules;
    let watching =
        [&rules.on_in, &rules.on_out, &rules.on_gain, &rules.always].into_iter().flat_map(|table| table[place].iter());
    let timed = rules.timed.iter().filter(|rule| rule.subject == Subject::Place(place));
    let mut governing: Vec<&Rule> = Vec::new();
    for rule in watching.chain(timed) {
        if !governing.iter().any(|seen| seen.law == rule.law) {
            governing.push(rule);
        }
    }

    let columns = [
        Column::left("Law"),
        Column::left("When"),
        Column::left("Governs"),
        Column::left("Explains"),
        Column::left("Written"),
    ];
    let mut section = Section::new(columns).headed("Governed by");
    for rule in governing {
        let law = &book.laws[rule.law];
        let explains = law.doc.and_then(|doc| doc_lines(book.name(doc)).next()).map_or(Cell::Blank, Cell::text);
        let cells = [
            Cell::text(book.name(law.name)),
            Cell::text(trigger_words(law.trigger)),
            Cell::text(governs(book, law.owner)),
            explains,
            Cell::Source(law.loc),
        ];
        section.push(Row::new(cells));
    }
    section
}
