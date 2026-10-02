//! `why ENTITY`: whose money it is, what governs it, what is held for it, what
//! it owes and is owed.
//!
//! An entity is a person, a household, a company, a grant. It owns places; a
//! restricted one has money tied to it that only its own laws let go; and the
//! claims against it are what others (or it) owe.

use axiom_core::{Id, Qty};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Entity, Law, Subject};

use super::laws_table;
use crate::claims;
use crate::lens::{Lens, on_balance_sheet};
use crate::places::path;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, entity: Id<Entity>) -> Report<'s> {
    let book = lens.book();
    let name = book.name(book.entities[entity].path);
    if !lens.owns_entity(entity) {
        return Report::new(format!("Why {name}"))
            .with(Section::note_only(format!("{name} is outside this owner's scope.")));
    }
    let open = claims::open(lens, run, run.holdings.iter());
    let with_it: Vec<&claims::Claim> = open.iter().filter(|claim| claim.with(entity)).collect();
    Report::new(format!("Why {name}"))
        .with(places_section(lens, run, entity))
        .with(laws_table(book, &governing_laws(book, run, entity)))
        .with(ties_section(lens, run, entity))
        .with(claims::section(lens, "Claims with it", &with_it))
}

/// What the entity holds in each place that is on the balance sheet.
fn places_section<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, entity: Id<Entity>) -> Section<'s> {
    let book = lens.book();
    let mut places = Section::new([Column::left("Place"), Column::right("Holds")]).headed("Places");
    for holding in run.holdings.iter().filter(|holding| on_balance_sheet(book.places[holding.place].class)) {
        // `why ENTITY` is about that entity's financial holdings even when the
        // caller's lens is the household. Allocate with the same cent
        // boundaries used by registers so shared places neither leak the
        // household total nor lose a cent across owners.
        let held = lens
            .plan()
            .allocate(holding.place, holding.qty())
            .find_map(|(share, qty)| (share.owner == entity).then_some(qty));
        let Some(held) = held.filter(|qty| !qty.is_zero()) else {
            continue;
        };
        let sign = book.places[holding.place].class.display_sign();
        let held = Amount::new(Qty(held.0 * sign), holding.unit);
        places.push(Row::new([Cell::text(path(book, holding.place)), Cell::amount(book, held)]));
    }
    places
}

/// What governs the entity itself: its `on spend` laws while it holds money for others, and its own timed laws.
fn governing_laws(book: &Book<'_>, run: &Run, entity: Id<Entity>) -> Vec<Id<Law>> {
    let rules = &book.rules;
    let timed = rules.timed.iter().filter(|rule| rule.subject == Subject::Entity(entity));
    rules.on_spend[entity]
        .iter()
        .chain(timed)
        .filter(|rule| rule.days.contains(run.today))
        .map(|rule| rule.law)
        .collect()
}

/// Money tied to the entity: it may leave the owner's places only as its laws allow.
fn ties_section<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, entity: Id<Entity>) -> Section<'s> {
    let book = lens.book();
    let mut ties =
        Section::new([Column::left("Place"), Column::right("Amount"), Column::left("Since"), Column::left("From")])
            .headed("Held for it");
    let mut remaining = Qty::ZERO;
    for holding in &run.holdings {
        if !lens.owns(holding.place) {
            continue;
        }
        for lot in holding.lots.iter().filter(|lot| lot.tied == Some(entity)) {
            let held = Amount::new(lens.place_qty(holding.place, lot.qty), holding.unit);
            remaining += lens.value(held).unwrap_or_default();
            let source = lot.txn.source_txn().and_then(|txn| book.txns.get(txn)).map(|txn| txn.loc);
            let cells = [
                Cell::text(path(book, holding.place)),
                Cell::amount(book, held),
                Cell::Day(lot.acquired),
                source.map_or(Cell::Blank, Cell::Source),
            ];
            ties.push(Row::new(cells));
        }
    }
    if !ties.rows.is_empty() {
        ties.push(Row::padded([Cell::text("Remaining"), Cell::base(book, remaining)], 4).style(Style::Total));
    }
    ties
}
