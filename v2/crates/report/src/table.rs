//! Small constructors so views read as what they show, not how it is built.

use std::borrow::Cow;

use axiom_core::{Qty, Sym};
use axiom_engine::{Cause, Owed};
use axiom_model::{Amount, Book};

use crate::{Align, Cell, Column, Report, Row, Section, Style};

impl Column {
    pub fn left(title: impl Into<Cow<'static, str>>) -> Column {
        Column { title: title.into(), align: Align::Left }
    }

    pub fn right(title: impl Into<Cow<'static, str>>) -> Column {
        Column { title: title.into(), align: Align::Right }
    }
}

impl<'s> Report<'s> {
    pub fn new(title: impl Into<String>) -> Report<'s> {
        Report { title: title.into(), sections: Vec::new() }
    }

    /// Adds a section, skipping one that has nothing to show.
    pub fn with(mut self, section: Section<'s>) -> Report<'s> {
        if !section.rows.is_empty() || !section.notes.is_empty() {
            self.sections.push(section);
        }
        self
    }
}

impl<'s> Section<'s> {
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Section<'s> {
        Section { heading: None, columns: columns.into_iter().collect(), rows: Vec::new(), notes: Vec::new() }
    }

    pub fn headed(mut self, heading: impl Into<String>) -> Section<'s> {
        self.heading = Some(heading.into());
        self
    }

    pub fn push(&mut self, row: Row<'s>) {
        debug_assert_eq!(row.cells.len(), self.columns.len(), "a cell for every column");
        self.rows.push(row);
    }

    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }
}

impl<'s> Row<'s> {
    pub fn new(cells: impl IntoIterator<Item = Cell<'s>>) -> Row<'s> {
        Row { depth: 0, style: Style::Normal, cells: cells.into_iter().collect() }
    }

    pub fn depth(mut self, depth: usize) -> Row<'s> {
        self.depth = u8::try_from(depth).unwrap_or(u8::MAX);
        self
    }

    pub fn style(mut self, style: Style) -> Row<'s> {
        self.style = style;
        self
    }
}

impl<'s> Cell<'s> {
    pub fn text(text: impl Into<Cow<'s, str>>) -> Cell<'s> {
        Cell::Text(text.into())
    }

    pub fn amount(book: &Book<'s>, amount: Amount) -> Cell<'s> {
        let unit = &book.commodities[amount.unit];
        Cell::Amount { qty: amount.qty, scale: unit.scale, unit: book.name(unit.symbol) }
    }

    /// An amount of the base currency.
    pub fn base(book: &Book<'s>, qty: Qty) -> Cell<'s> {
        Cell::amount(book, Amount::new(qty, book.base))
    }

    /// The amount, or nothing for zero: statements read better without rows of `0.00`.
    pub fn base_or_blank(book: &Book<'s>, qty: Qty) -> Cell<'s> {
        if qty.is_zero() { Cell::Blank } else { Cell::base(book, qty) }
    }
}

/// The first line of a diagnostic's message: enough for a table cell.
pub fn headline(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

/// `#house`, `#check-1041`: codes as they are written.
pub fn code_labels<'a>(book: &'a Book, codes: &'a [Sym]) -> impl Iterator<Item = String> + 'a {
    codes.iter().map(|&code| format!("#{}", book.name(code).trim_start_matches('#')))
}

/// `irs by 2027-04-15`
pub fn creditor(book: &Book, owed: Owed) -> String {
    format!("{} by {}", book.name(book.entities[owed.to].path), owed.due)
}

/// Where a consequence comes from: the line that caused it, so it can be
/// traced with `why`.
pub fn cause_cell<'s>(book: &Book<'s>, cause: Cause) -> Cell<'s> {
    match cause {
        Cause::Flow(flow) => Cell::Source(book.flows[flow].loc),
        Cause::Applied(_) => Cell::text("hypothetical flow"),
        Cause::Time => Cell::text("period end"),
    }
}
