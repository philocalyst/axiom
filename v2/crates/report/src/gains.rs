//! `gains`: every disposal in a year, in the shape of Form 8949: what was
//! sold, when it was acquired and sold, what it fetched, what it cost, and
//! what was gained, with short-term and long-term subtotals.

use axiom_core::{Day, Id, Qty, Span};
use axiom_engine::{Gain, Run};
use axiom_model::{Amount, Book, Commodity};

use crate::lens::{Lens, Whose};
use crate::places::path;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Held longer than this, a gain is long-term. The boundary is the US one;
/// systems that draw it elsewhere still see their own `held` in laws.
const LONG_TERM: Span = Span::months(12);

/// How long something was held when it was sold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Term {
    Short,
    Long,
    /// The base currency has no holding period: money that leaves a
    /// tax-deferred place is not a sale of anything.
    Untimed,
}

impl Term {
    pub fn of(book: &Book, unit: Id<Commodity>, acquired: Day, sold: Day) -> Term {
        if unit == book.base {
            Term::Untimed
        } else if sold.since(acquired) > LONG_TERM {
            Term::Long
        } else {
            Term::Short
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Term::Short => "short",
            Term::Long => "long",
            Term::Untimed => "",
        }
    }
}

pub fn view<'s>(book: &Book<'s>, run: &Run, whose: &Whose, year: Option<i32>) -> Report<'s> {
    let year = year.unwrap_or_else(|| run.today.year());
    let lens = Lens::new(book, whose, run.today);
    let columns = [
        Column::left("Sold"),
        Column::left("Acquired"),
        Column::right("Quantity"),
        Column::left("From"),
        Column::right("Proceeds"),
        Column::right("Basis"),
        Column::right("Gain"),
        Column::left("Term"),
    ];
    let mut table = Section::new(columns);
    let disposals: Vec<&Gain> =
        run.gains.iter().filter(|gain| gain.day.year() == year && lens.owns(gain.from)).collect();
    let mut total = (Qty::ZERO, Qty::ZERO);
    let mut groups = 0;
    for (term, heading) in [(Term::Short, "Short-term"), (Term::Long, "Long-term"), (Term::Untimed, "Money withdrawn")]
    {
        let mut group: Vec<&Gain> = disposals
            .iter()
            .copied()
            .filter(|gain| Term::of(book, gain.unit, gain.acquired, gain.day) == term)
            .collect();
        group.sort_by_key(|gain| (gain.day, gain.acquired));
        for gain in &group {
            let cells = [
                Cell::Day(gain.day),
                Cell::Day(gain.acquired),
                Cell::amount(book, Amount::new(gain.qty, gain.unit)),
                Cell::text(path(book, gain.from)),
                Cell::base(book, gain.proceeds),
                Cell::base(book, gain.basis),
                Cell::base(book, gain.gain()),
                Cell::text(term.word()),
            ];
            table.push(Row::new(cells).style(if gain.ambiguous { Style::Muted } else { Style::Normal }));
        }
        if !group.is_empty() {
            let sum = (group.iter().map(|gain| gain.proceeds).sum(), group.iter().map(|gain| gain.basis).sum());
            table.push(subtotal(book, heading, sum));
            (total, groups) = ((total.0 + sum.0, total.1 + sum.1), groups + 1);
        }
    }
    if groups > 1 {
        table.push(subtotal(book, "Total", total));
    }
    if table.rows.is_empty() {
        table.note(format!("Nothing was sold in {year}."));
    }
    if disposals.iter().any(|gain| gain.ambiguous) {
        table.note("Muted disposals had no lot policy, so the oldest shares were assumed sold.");
    }
    Report::new(format!("Gains realized in {year}")).with(table)
}

fn subtotal<'s>(book: &Book<'s>, label: &'static str, (proceeds, basis): (Qty, Qty)) -> Row<'s> {
    let cells = [
        Cell::text(label),
        Cell::Blank,
        Cell::Blank,
        Cell::Blank,
        Cell::base(book, proceeds),
        Cell::base(book, basis),
        Cell::base(book, proceeds - basis),
        Cell::Blank,
    ];
    Row::new(cells).style(Style::Total)
}
