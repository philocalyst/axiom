//! `why SYSTEM`: its laws, and what each counted or owed for the people living under it.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty, Sym};
use axiom_model::{Amount, Commodity, System};

use crate::lens::Lens;
use crate::table::doc_headline;
use crate::{Cell, Column, Report, Row, Section};

pub fn report<'s>(lens: Lens<'_, 's>, system: Id<System>) -> Report<'s> {
    let (book, run, whose) = (lens.book, lens.run, lens.whose);
    let columns =
        [Column::left("Law"), Column::left("When"), Column::left("Counted for residents"), Column::left("Written")];
    let mut laws = Section::new(columns).headed("Laws");
    let declared =
        book.laws.iter().filter(|(_, law)| law.system.is_some_and(|declared| book.systems.covers(system, declared)));
    for (id, law) in declared {
        let mut totals: BTreeMap<(Sym, bool, Id<Commodity>), Qty> = BTreeMap::new();
        for effect in run.effects.iter().filter(|effect| effect.law == id && whose.includes(effect.owner)) {
            *totals.entry((effect.name, effect.owe.is_some(), effect.amount.unit)).or_default() += effect.amount.qty;
        }
        let did = totals.into_iter().map(|((name, owes, unit), qty)| {
            let (name, amount) = (Cell::Name(book.name(name)), Cell::amount(book, Amount::new(qty, unit)));
            if owes { ["owes".into(), name, amount].into() } else { [name, amount].into() }
        });
        let cells = [
            Cell::Name(book.name(law.name)),
            Cell::Trigger(law.trigger),
            Cell::list(" · ", did),
            Cell::Source(law.loc),
        ];
        laws.push(Row::new(cells));
        if let Some(doc) = doc_headline(book, law.doc) {
            laws.push(Row::new([doc]).depth(1).style(crate::Style::Muted));
        }
    }
    let residents: Vec<&str> = book
        .entities
        .values()
        .filter(|entity| entity.lives.iter().any(|home| book.systems.covers(system, home.system)))
        .map(|entity| book.name(entity.path))
        .collect();
    let mut who = Section::new([]);
    who.note(if residents.is_empty() {
        Cell::from("Nobody in this book lives here.")
    } else {
        ["Lives here:".into(), Cell::list(", ", residents.into_iter().map(Cell::Name)), ".".into()].into()
    });
    Report::new(["Why".into(), Cell::Name(book.name(book.systems[system].path))]).with(who).with(laws)
}
