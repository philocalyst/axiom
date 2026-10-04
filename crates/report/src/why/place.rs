//! `why PLACE`: what it holds and how, how close it is to every limit, which
//! laws govern it, what touched it lately.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty};
use axiom_engine::{Holding, Run};
use axiom_model::{Amount, Book, Commodity, Law, Place, Rule, Subject, Watch};

use super::{flows_table, laws_table};
use crate::headroom::{current, latest};
use crate::history::all_postings;
use crate::limits;
use crate::places::path;
use crate::table::plural;
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(view: View<'s, '_, '_>, place: Id<Place>) -> Report<'s> {
    let book = view.book();
    let owner = book.places[place].owner;
    let name = path(book, place);
    if !view.owns(place) {
        return Report::new(format!("Why {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[owner].path)
        )));
    }
    let held: Vec<&Holding> = view
        .run
        .holdings
        .iter()
        .filter(|holding| book.places.covers(place, holding.place) && view.owns(holding.place))
        .collect();
    let flows = all_postings(book, view.run).filter(|posting| posting.flow.from == place || posting.flow.to == place);

    // A limit is about this place when it measures it, or a place around it, or one within it.
    let all = &current(book, view.run, view.run.today, view.run.today);
    let about = |subject: Subject| matches!(subject, Subject::Place(other) if book.places.covers(other, place) || book.places.covers(place, other));
    let limits = limits::section(
        book,
        latest(all.iter().filter(|reading| about(reading.subject) && view.governs(reading.subject))),
    )
    .headed("Limits");

    let (governing, elsewhere) = governing(book, view.run, place);
    let mut laws = laws_table(book, &governing);
    if elsewhere > 0 {
        laws.note(format!(
            "{} not in force today: their residence has ended, or has not begun.",
            plural(elsewhere, "law")
        ));
    }
    Report::new(format!("Why {}", path(book, place)))
        .with(composition(view, &held))
        .with(parcels(view, &held))
        .with(limits)
        .with(laws)
        .with(flows_table(view, flows, "Flows"))
}

/// What is held, by commodity, and how much of it is plain money.
fn composition<'s>(view: View<'s, '_, '_>, held: &[&Holding]) -> Section<'s> {
    let book = view.book();
    let mut units: BTreeMap<Id<Commodity>, (Qty, Qty, usize)> = BTreeMap::new();
    for holding in held {
        let (total, plain, parcels) = units.entry(holding.unit).or_default();
        *total += view.place_qty(holding.place, holding.qty());
        *plain += view.place_qty(holding.place, holding.plain);
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
fn parcels<'s>(view: View<'s, '_, '_>, held: &[&Holding]) -> Section<'s> {
    let book = view.book();
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
            let qty = view.place_qty(holding.place, lot.qty);
            let basis = view.place_qty(holding.place, lot.basis);
            if qty.is_zero() && basis.is_zero() {
                continue;
            }
            let tie = lot.tied.map_or(Cell::Blank, |entity| Cell::text(book.name(book.entities[entity].path)));
            let cells = [
                Cell::text(path(book, holding.place)),
                Cell::amount(book, Amount::new(qty, holding.unit)),
                Cell::base(book, basis),
                Cell::Day(lot.acquired),
                tie,
                lot.txn
                    .source_txn()
                    .and_then(|txn| book.txns.get(txn))
                    .map_or(Cell::Blank, |txn| Cell::Source(txn.loc)),
            ];
            section.push(Row::new(cells));
        }
    }
    section
}

/// The laws in force on the place today, resolved once by the model (its own
/// and its ancestors', its kind's, its owner's jurisdictions'), and how many
/// more are written for it that today is outside.
fn governing(book: &Book, run: &Run, place: Id<Place>) -> (Vec<Id<Law>>, usize) {
    let rules = &book.rules;
    let watching =
        [Watch::In(place), Watch::Out(place), Watch::Gain(place), Watch::Always(place), Watch::Touching(place)]
            .into_iter()
            .flat_map(|watch| rules.at(watch));
    let timed = rules.timed().iter().filter(|rule| rule.subject == Subject::Place(place));
    let (now, later): (Vec<&Rule>, Vec<&Rule>) = watching.chain(timed).partition(|rule| rule.days.contains(run.today));
    let laws: Vec<Id<Law>> = now.iter().map(|rule| rule.law).collect();
    let elsewhere = later.iter().filter(|rule| !laws.contains(&rule.law)).count();
    (laws, elsewhere)
}
