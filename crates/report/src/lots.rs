//! `lots`: parcels with their basis, and what they would fetch at the latest price.

use axiom_core::Qty;
use axiom_engine::{Holding, Parcel};
use axiom_model::{Amount, Book, Place};

use crate::gains::Term;
use crate::lens::Lens;
use crate::places::path;
use crate::table::code_labels;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Builds a lots view from holdings supplied by a shared context ledger.
pub(crate) fn view_from<'h, 's>(
    lens: Lens<'s, '_, '_, '_>,
    scope: Option<axiom_core::Id<Place>>,
    holdings: impl IntoIterator<Item = &'h Holding>,
) -> Report<'s> {
    let (book, at) = (lens.book(), lens.day);

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

    let mut sum = Sum::default();
    let mut held: Vec<&Holding> = holdings
        .into_iter()
        .filter(|holding| {
            lens.owns(holding.place) && scope.is_none_or(|scope| book.places.covers(scope, holding.place))
        })
        .collect();
    held.sort_by_key(|holding| book.listing(holding.place));
    for holding in held {
        for lot in &holding.lots {
            let quantity = lens.place_qty(holding.place, lot.qty);
            let lot_basis = lens.place_qty(holding.place, lot.basis);
            if quantity.is_zero() && lot_basis.is_zero() {
                continue;
            }
            let worth = lens.value(Amount::new(quantity, holding.unit));
            sum.add(lot_basis, worth);
            section.push(row(lens, holding, lot, quantity, lot_basis, worth));
        }
    }

    if section.rows.is_empty() {
        section.note("No parcels: everything held is plain money.");
    } else {
        section.push(sum.row(book));
    }
    if sum.unpriced > 0 {
        section.note(format!(
            "{} parcels have no price; they are muted and left out of Value and Unrealized.",
            sum.unpriced
        ));
    }
    Report::new(format!("Lots at {at}")).with(section)
}

/// What the parcels shown come to.
#[derive(Default)]
struct Sum {
    basis: Qty,
    value: Qty,
    unrealized: Qty,
    /// How many have no price.
    unpriced: usize,
}

impl Sum {
    /// Adds a parcel with its basis and what it would fetch, if that is known.
    fn add(&mut self, basis: Qty, worth: Option<Qty>) {
        self.basis += basis;
        match worth {
            Some(worth) => {
                self.value += worth;
                self.unrealized += worth - basis;
            }
            None => self.unpriced += 1,
        }
    }

    fn row<'s>(&self, book: &'s Book<'_>) -> Row<'s> {
        let cells = [
            Cell::text("Total"),
            Cell::Blank,
            Cell::base(book, self.basis),
            Cell::Blank,
            Cell::Blank,
            Cell::base(book, self.value),
            Cell::base(book, self.unrealized),
        ];
        Row::padded(cells, 9).style(Style::Total)
    }
}

fn row<'s>(
    lens: Lens<'s, '_, '_, '_>,
    holding: &Holding,
    lot: &Parcel,
    quantity: Qty,
    basis: Qty,
    worth: Option<Qty>,
) -> Row<'s> {
    let book = lens.book();
    let tie = lot.tied.map(|entity| format!("tied to {}", book.name(book.entities[entity].path)));
    let notes =
        code_labels(book, book.codes[lot.codes.header].iter().chain(book.codes[lot.codes.local].iter()).copied())
            .chain(tie.map(Cell::text))
            .collect::<Vec<_>>();
    let cells = [
        Cell::text(path(book, holding.place)),
        Cell::amount(book, Amount::new(quantity, holding.unit)),
        Cell::base(book, basis),
        Cell::Day(lot.acquired),
        Cell::text(lens.day.since(lot.acquired).to_string()),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth)),
        worth.map_or(Cell::Blank, |worth| Cell::base(book, worth - basis)),
        Cell::text(Term::of(book, holding.unit, lot.acquired, lens.day).word()),
        Cell::list_or_blank(" · ", notes),
    ];
    Row::new(cells).style(if worth.is_some() { Style::Normal } else { Style::Muted })
}
