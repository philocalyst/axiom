//! `gains`: every disposal in a year, in the shape of Form 8949: what was
//! sold, when it was acquired and sold, what it fetched, what it cost, and
//! what was gained, with short-term and long-term subtotals.

use axiom_core::{Day, Days, Id, Qty, Span};
use axiom_engine::Gain;
use axiom_model::{Amount, Book, Commodity};

use crate::lens::Lens;
use crate::places::path;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

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

    pub fn cell<'s>(self) -> Cell<'s> {
        match self {
            Term::Short => "short".into(),
            Term::Long => "long".into(),
            Term::Untimed => Cell::Blank,
        }
    }
}

pub fn view<'s>(lens: Lens<'_, 's>, year: Option<i32>) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let year = year.unwrap_or_else(|| lens.day.year());
    // Money leaving at its own basis (a grant spent, a deposit returned) realized nothing.
    let realized = |gain: &&Gain| gain.unit != book.base || gain.proceeds != gain.basis;
    let disposals: Vec<&Gain> =
        run.gains.iter().filter(|gain| gain.day.year() == year && lens.owns(gain.from)).filter(realized).collect();
    let mut table = section(book, &disposals);
    if table.rows.is_empty() {
        table.note(["Nothing was sold in".into(), Cell::year(year), ".".into()]);
    }
    Report::new(["Gains realized in".into(), Cell::year(year)]).with(table)
}

/// Disposals as Form 8949 lays them out: short-term, then long-term, then
/// money withdrawn from tax-deferred places, each with its subtotal.
pub fn section<'s>(book: &Book<'s>, disposals: &[&Gain]) -> Section<'s> {
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
    let mut total = (Qty::ZERO, Qty::ZERO);
    let mut groups = 0;
    for (term, heading) in [(Term::Short, "Short-term"), (Term::Long, "Long-term"), (Term::Untimed, "Money withdrawn")]
    {
        let mut group: Vec<&&Gain> =
            disposals.iter().filter(|gain| Term::of(book, gain.unit, gain.acquired, gain.day) == term).collect();
        group.sort_by_key(|gain| (gain.day, gain.acquired));
        for gain in &group {
            let owner = book.name(book.entities[book.places[gain.from].owner].path);
            for (concept, qty) in [("proceeds", gain.proceeds), ("basis", gain.basis), ("gain", gain.gain())] {
                table.fact(
                    concept,
                    Some(path(book, gain.from)),
                    owner,
                    When::During(Days::on(gain.day)),
                    Money::base(book, qty),
                );
            }
            let cells = [
                Cell::Day(gain.day),
                Cell::Day(gain.acquired),
                Cell::amount(book, Amount::new(gain.qty, gain.unit)),
                Cell::Name(path(book, gain.from)),
                Cell::base(book, gain.proceeds),
                Cell::base(book, gain.basis),
                Cell::base(book, gain.gain()),
                term.cell(),
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
    if disposals.iter().any(|gain| gain.ambiguous) {
        table.note("Muted disposals had no lot policy, so the oldest shares were assumed sold.");
    }
    table
}

fn subtotal<'s>(book: &Book<'s>, label: &'static str, (proceeds, basis): (Qty, Qty)) -> Row<'s> {
    let cells = [
        label.into(),
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
