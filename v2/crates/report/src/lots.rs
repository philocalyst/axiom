//! `lots`: parcels with their basis, and what they would fetch at the latest price.

use axiom_core::{Day, Diagnostic, Qty};
use axiom_engine::{Holding, Parcel};
use axiom_model::Amount;

use crate::claims::holdings_at;
use crate::gains::Term;
use crate::lens::{Lens, Priced};
use crate::places::path;
use crate::resolve;
use crate::table::code_labels;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(lens: Lens<'_, 's>, place: Option<&str>, at: Option<Day>) -> Result<Report<'s>, Diagnostic> {
    let book = lens.book;
    let scope = place.map(|text| resolve::place(book, text)).transpose()?;
    let at = at.unwrap_or(lens.day);
    let lens = lens.on(at);

    let mut section = Section::new([
        Column::left("Place"),
        Column::right("Quantity"),
        Column::right("Basis"),
        Column::left("Acquired"),
        Column::left("Held"),
        Column::right("Value"),
        Column::right("Unrealized"),
        Column::left("Term"),
        Column::left("Note"),
    ]);

    let (mut basis, mut value, mut unrealized) = (Qty::ZERO, Priced::default(), Qty::ZERO);
    let holdings = holdings_at(lens);
    let held = holdings.iter().filter(|holding| {
        lens.owns(holding.place) && scope.is_none_or(|scope| book.places.covers(scope, holding.place))
    });
    for holding in held {
        for lot in &holding.lots {
            let worth = value.add(lens.value(Amount::new(lot.qty, holding.unit)));
            basis += lot.basis;
            unrealized += worth.map_or(Qty::ZERO, |worth| worth - lot.basis);
            section.push(row(lens, holding, lot, worth));
        }
    }

    if section.rows.is_empty() {
        section.note("No parcels: everything held is plain money.");
    } else {
        let cells = [
            Cell::text("Total"),
            Cell::Blank,
            Cell::base(book, basis),
            Cell::Blank,
            Cell::Blank,
            Cell::base(book, value.total),
            Cell::base(book, unrealized),
        ];
        section.total(cells);
    }
    section.unpriced(value.missing(), "parcel");
    Ok(Report::new(format!("Lots at {at}")).with(section))
}

fn row<'s>(lens: Lens<'_, 's>, holding: &Holding, lot: &Parcel, worth: Option<Qty>) -> Row<'s> {
    let book = lens.book;
    let tie = lot.tied.map(|entity| format!("tied to {}", book.name(book.entities[entity].path)));
    let notes: Vec<String> = code_labels(book, &book.txns[lot.txn].codes).chain(tie).collect();
    let cells = [
        Cell::text(path(book, holding.place)),
        Cell::amount(book, Amount::new(lot.qty, holding.unit)),
        Cell::base(book, lot.basis),
        Cell::Day(lot.acquired),
        Cell::text(lens.day.since(lot.acquired).to_string()),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth - lot.basis)),
        Cell::text(Term::of(book, holding.unit, lot.acquired, lens.day).word()),
        if notes.is_empty() { Cell::Blank } else { Cell::text(notes.join(" · ")) },
    ];
    Row::new(cells).style(if worth.is_some() { Style::Normal } else { Style::Muted })
}
