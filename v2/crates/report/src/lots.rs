//! `lots`: parcels with their basis, and what they would fetch at the latest price.

use axiom_core::{Diagnostic, Qty, Span};
use axiom_engine::{Holding, Parcel, Run};
use axiom_model::{Amount, Book};

use crate::places::path;
use crate::resolve;
use crate::table::code_labels;
use crate::value::Valuer;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Held longer than this, a gain is long-term. The boundary is the US
/// one; systems that draw it elsewhere still see their own `held` in laws.
const LONG_TERM: Span = Span::months(12);

pub fn view<'s>(book: &Book<'s>, run: &Run, place: Option<&str>) -> Result<Report<'s>, Diagnostic> {
    let scope = place.map(|text| resolve::place(book, text)).transpose()?;
    let valuer = Valuer::new(book, run.today);

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

    let (mut basis, mut value, mut unrealized, mut unpriced) = (Qty::ZERO, Qty::ZERO, Qty::ZERO, 0);
    let parcels =
        run.holdings.iter().filter(|holding| scope.is_none_or(|scope| book.places.covers(scope, holding.place)));
    for holding in parcels {
        for lot in &holding.lots {
            let worth = valuer.qty(Amount::new(lot.qty, holding.unit));
            basis += lot.basis;
            match worth {
                Some(worth) => {
                    value += worth;
                    unrealized += worth - lot.basis;
                }
                None => unpriced += 1,
            }
            section.push(row(book, run, holding, lot, worth));
        }
    }

    if !section.rows.is_empty() {
        let cells = [
            Cell::text("Total"),
            Cell::Blank,
            Cell::base(book, basis),
            Cell::Blank,
            Cell::Blank,
            Cell::base(book, value),
            Cell::base(book, unrealized),
            Cell::Blank,
            Cell::Blank,
        ];
        section.push(Row::new(cells).style(Style::Total));
    } else {
        section.note("No parcels: everything held is plain money.");
    }
    if unpriced > 0 {
        section.note(format!("{unpriced} parcels have no price; they are muted and left out of Value and Unrealized."));
    }
    Ok(Report::new(format!("Lots at {}", run.today)).with(section))
}

fn row<'s>(book: &Book<'s>, run: &Run, holding: &Holding, lot: &Parcel, worth: Option<Qty>) -> Row<'s> {
    let held = run.today.since(lot.acquired);
    let term = if held > LONG_TERM { "long" } else { "short" };
    let tie = lot.tied.map(|entity| format!("tied to {}", book.name(book.entities[entity].path)));
    let notes: Vec<String> = code_labels(book, &book.txns[lot.txn].codes).chain(tie).collect();
    let cells = [
        Cell::text(path(book, holding.place)),
        Cell::amount(book, Amount::new(lot.qty, holding.unit)),
        Cell::base(book, lot.basis),
        Cell::Day(lot.acquired),
        Cell::text(held.to_string()),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth - lot.basis)),
        Cell::text(term),
        if notes.is_empty() { Cell::Blank } else { Cell::text(notes.join(" · ")) },
    ];
    Row::new(cells).style(if worth.is_some() { Style::Normal } else { Style::Muted })
}
