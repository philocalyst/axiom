//! `why SYSTEM`: its laws, and what each counted or owed for the people living under it.

use std::collections::BTreeMap;

use axiom_core::{Id, Qty, Sym};
use axiom_engine::Run;
use axiom_model::{Amount, Book, Commodity, System};

use crate::lens::Whose;
use crate::table::doc_headline;
use crate::{Cell, Column, Report, Row, Section};

use super::trigger_words;

pub fn report<'s>(book: &Book<'s>, run: &Run, whose: &Whose, system: Id<System>) -> Report<'s> {
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
        let did: Vec<String> = totals
            .into_iter()
            .map(|((name, owes, unit), qty)| {
                let amount = book.show(Amount::new(qty, unit));
                if owes {
                    format!("owes {} {amount}", book.name(name))
                } else {
                    format!("{} {amount}", book.name(name))
                }
            })
            .collect();
        let doc = doc_headline(book, law.doc).unwrap_or_default();
        let cells = [
            Cell::text(book.name(law.name)),
            Cell::text(trigger_words(law.trigger)),
            Cell::text(did.join(" · ")),
            Cell::Source(law.loc),
        ];
        laws.push(Row::new(cells));
        if !doc.is_empty() {
            laws.push(Row::new([Cell::text(doc)]).depth(1).style(crate::Style::Muted));
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
        "Nobody in this book lives here.".to_string()
    } else {
        format!("Lives here: {}.", residents.join(", "))
    });
    Report::new(format!("Why {}", book.name(book.systems[system].path))).with(who).with(laws)
}
