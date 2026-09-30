//! Small constructors so views read as what they show, not how it is built.

use axiom_core::{Day, Days, Qty, Sym, calendar};
use axiom_engine::{Cause, Owed, Pad};
use axiom_model::{Amount, Book, Period};

use crate::places::path;
use crate::{Align, Cell, Column, Fact, Money, Report, Row, Section, Style, When};

impl<'s> Column<'s> {
    pub fn left(title: impl Into<Cell<'s>>) -> Column<'s> {
        Column { title: title.into(), align: Align::Left }
    }

    pub fn right(title: impl Into<Cell<'s>>) -> Column<'s> {
        Column { title: title.into(), align: Align::Right }
    }
}

impl<'s> Report<'s> {
    pub fn new(title: impl Into<Cell<'s>>) -> Report<'s> {
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
    pub fn new(columns: impl IntoIterator<Item = Column<'s>>) -> Section<'s> {
        Section {
            heading: None,
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
            notes: Vec::new(),
            facts: Vec::new(),
        }
    }

    /// A section that is only prose.
    pub fn note_only(note: impl Into<Cell<'s>>) -> Section<'s> {
        let mut section = Section::new([]);
        section.note(note);
        section
    }

    pub fn headed(mut self, heading: &'static str) -> Section<'s> {
        self.heading = Some(heading);
        self
    }

    /// Adds a row. One with fewer cells than the table has columns is a label
    /// or a total, and is padded with blanks to the table's own width.
    pub fn push(&mut self, mut row: Row<'s>) {
        debug_assert!(row.cells.len() <= self.columns.len(), "a column for every cell");
        row.cells.resize_with(self.columns.len(), || Cell::Blank);
        self.rows.push(row);
    }

    /// A total row: the leading cells, and blanks under the rest.
    pub fn total(&mut self, lead: impl IntoIterator<Item = Cell<'s>>) {
        self.push(Row::new(lead).style(Style::Total));
    }

    /// Says what was left out of a total for lack of a price, if anything was.
    pub fn unpriced(&mut self, missing: usize, what: &'static str) {
        if missing > 0 {
            self.note([Cell::Count(missing, what), "left out for lack of a price.".into()]);
        }
    }

    pub fn note(&mut self, note: impl Into<Cell<'s>>) {
        self.notes.push(note.into());
    }

    /// Records a figure the rows show.
    pub fn fact(&mut self, concept: &'s str, of: Option<&'s str>, entity: &'s str, when: When, value: Money<'s>) {
        self.facts.push(Fact { concept, of, entity, when, value });
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

impl<'s> Money<'s> {
    pub fn of(book: &Book<'s>, amount: Amount) -> Money<'s> {
        let unit = &book.commodities[amount.unit];
        Money { qty: amount.qty, scale: unit.scale, unit: book.name(unit.symbol) }
    }

    /// An amount of the base currency.
    pub fn base(book: &Book<'s>, qty: Qty) -> Money<'s> {
        Money::of(book, Amount::new(qty, book.base))
    }
}

impl<'s> From<&'static str> for Cell<'s> {
    fn from(word: &'static str) -> Cell<'s> {
        Cell::Word(word)
    }
}

/// Parts of a sentence.
impl<'s, const N: usize> From<[Cell<'s>; N]> for Cell<'s> {
    fn from(parts: [Cell<'s>; N]) -> Cell<'s> {
        Cell::Join(" ", parts.into())
    }
}

impl<'s> Cell<'s> {
    pub fn amount(book: &Book<'s>, amount: Amount) -> Cell<'s> {
        Cell::Amount(Money::of(book, amount))
    }

    /// An amount of the base currency.
    pub fn base(book: &Book<'s>, qty: Qty) -> Cell<'s> {
        Cell::Amount(Money::base(book, qty))
    }

    /// The amount, or nothing for zero: statements read better without rows of `0.00`.
    pub fn base_or_blank(book: &Book<'s>, qty: Qty) -> Cell<'s> {
        if qty.is_zero() { Cell::Blank } else { Cell::base(book, qty) }
    }

    /// A calendar year.
    pub fn year(year: i32) -> Cell<'s> {
        year_days(year).map_or(Cell::Blank, Cell::Period)
    }

    /// A code, as the book named it, without the sigil it may be stored with.
    pub fn code(book: &Book<'s>, code: Sym) -> Cell<'s> {
        Cell::Code(book.name(code).trim_start_matches('#'))
    }

    /// The first line of a diagnostic's message: enough for a table cell.
    pub fn headline(message: &str) -> Cell<'s> {
        Cell::Said(message.lines().next().unwrap_or_default().to_string())
    }

    /// The parts, one after another, with `between` between them.
    pub fn list(between: &'static str, parts: impl IntoIterator<Item = Cell<'s>>) -> Cell<'s> {
        Cell::Join(between, parts.into_iter().collect())
    }

    /// The parts, or nothing when there are none.
    pub fn list_or_blank(between: &'static str, parts: impl IntoIterator<Item = Cell<'s>>) -> Cell<'s> {
        let parts: Vec<Cell<'s>> = parts.into_iter().collect();
        if parts.is_empty() { Cell::Blank } else { Cell::Join(between, parts) }
    }
}

/// The days of a calendar year.
pub fn year_days(year: i32) -> Option<Days> {
    let first = Day::from_ymd(year, 1, 1)?;
    Some(calendar::Window::containing(Period::Year, first).days())
}

/// A doc block as plain lines: the `///` markers and one space of indent
/// removed (a doc that is already plain passes through).
pub fn doc_lines(doc: &str) -> impl Iterator<Item = &str> {
    doc.lines().map(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("///").unwrap_or(line);
        line.strip_prefix(' ').unwrap_or(line)
    })
}

/// What a doc comment says the item is: its first line, marked with an
/// ellipsis when the sentence runs on.
pub fn doc_headline<'s>(book: &Book<'s>, doc: Option<Sym>) -> Option<Cell<'s>> {
    let mut lines = doc_lines(book.name(doc?));
    let first = lines.next()?;
    let runs_on = lines.next().is_some_and(|next| !next.trim().is_empty());
    Some(if runs_on { Cell::Join("", vec![Cell::Text(first), Cell::Word("…")]) } else { Cell::Text(first) })
}

/// `irs by 2027-04-15`
pub fn creditor<'s>(book: &Book<'s>, owed: Owed) -> Cell<'s> {
    [Cell::Name(book.name(book.entities[owed.to].path)), "by".into(), Cell::Day(owed.due)].into()
}

/// Where an accepted gap came from, in the words its assertion was written
/// with: `!`, or `via` a place, which for a `market` place is a revaluation.
pub fn gap_words<'s>(book: &Book<'s>, pad: &Pad) -> Cell<'s> {
    if pad.counter == book.roots.unknown {
        "unexplained gap, accepted with !".into()
    } else {
        let via = if book.entities[book.roots.market].place == Some(pad.counter) { "revalued via" } else { "gap via" };
        [via.into(), Cell::Name(path(book, pad.counter))].into()
    }
}

/// Where a consequence comes from: the line that caused it, so it can be
/// traced with `why`.
pub fn cause_cell<'s>(book: &Book<'s>, cause: Cause) -> Cell<'s> {
    match cause {
        Cause::Flow(flow) => Cell::Source(book.flows[flow].loc),
        Cause::Applied(_) => "hypothetical flow".into(),
        Cause::Time => "period end".into(),
    }
}
