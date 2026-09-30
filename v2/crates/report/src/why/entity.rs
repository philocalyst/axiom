//! `why ENTITY`: whose money it is, what governs it, what is held for it, what
//! it owes and is owed.
//!
//! An entity is a person, a household, a company, a grant. It owns places; a
//! restricted one has money tied to it that only its own laws let go; and the
//! claims against it are what others (or it) owe.

use axiom_core::{Id, Qty};
use axiom_model::{Amount, Entity, Law, Subject};

use super::laws_table;
use crate::claims;
use crate::lens::{Lens, Priced, Whose, on_balance_sheet};
use crate::places::path;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(lens: Lens<'_, 's>, entity: Id<Entity>) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let name = book.name(book.entities[entity].path);
    let scope = Whose::of(book, entity);
    let lens = lens.on(run.today).scoped(&scope);

    let mut places = Section::new([Column::left("Place"), Column::right("Holds")]).headed("Places");
    for holding in run
        .holdings
        .iter()
        .filter(|holding| lens.owns(holding.place) && on_balance_sheet(book.places[holding.place].class))
    {
        let held = Amount::new(Qty(holding.qty().0 * lens.sides.sign(holding.place)), holding.unit);
        places.push(Row::new([Cell::Name(path(book, holding.place)), Cell::amount(book, held)]));
    }

    // What governs the entity itself: its `on spend` laws while it holds money for others, and its own timed laws.
    let rules = &book.rules;
    let timed = rules.timed.iter().filter(|rule| rule.subject == Subject::Entity(entity));
    let laws: Vec<Id<Law>> = rules.on_spend[entity]
        .iter()
        .chain(timed)
        .filter(|rule| rule.days.contains(run.today))
        .map(|rule| rule.law)
        .collect();

    // Money tied to it: it may leave the owner's places only as its laws allow.
    let mut ties =
        Section::new([Column::left("Place"), Column::right("Amount"), Column::left("Since"), Column::left("From")])
            .headed("Held for it");
    let mut remaining = Priced::default();
    let everyone = Whose::default();
    let lens = lens.scoped(&everyone);
    for holding in &run.holdings {
        for lot in holding.lots.iter().filter(|lot| lot.tied == Some(entity)) {
            let held = Amount::new(lot.qty, holding.unit);
            remaining.add(lens.value(held));
            let cells = [
                Cell::Name(path(book, holding.place)),
                Cell::amount(book, held),
                Cell::Day(lot.acquired),
                Cell::Source(book.txns[lot.txn].loc),
            ];
            ties.push(Row::new(cells));
        }
    }
    if !ties.rows.is_empty() {
        ties.total(["Remaining".into(), Cell::base(book, remaining.total)]);
    }
    ties.unpriced(remaining.missing(), "amount");

    let open = claims::open(lens, run.holdings.iter());
    let with_it: Vec<&claims::Claim> = open.iter().filter(|claim| claim.with(entity)).collect();
    Report::new(["Why".into(), Cell::Name(name)])
        .with(places)
        .with(laws_table(book, &laws))
        .with(ties)
        .with(claims::section(lens, "Claims with it", &with_it))
}
