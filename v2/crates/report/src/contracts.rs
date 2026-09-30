//! `contracts`: every promise, with what it says today, what falls due next,
//! how many were kept, what is late and whom it blames, and, for a loan, what
//! is still owed.

use axiom_core::{Day, Days, Qty, Span};
use axiom_model::Amount;

use crate::lens::Lens;
use crate::places::path;
use crate::promises::{late, terms_on};
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

/// How far ahead the next due day is looked for.
const AHEAD: Span = Span::months(13);

pub fn view<'s>(lens: Lens<'_, 's>, at: Option<Day>) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let at = at.unwrap_or(lens.day);
    let lens = lens.on(at);
    let columns = ["Contract", "With", "Terms", "Next due"].map(Column::left).into_iter();
    let columns = columns.chain([Column::right("Kept")]).chain(["Late", "Blames"].map(Column::left));
    let mut table = Section::new(columns.chain([Column::right("Owed")]));
    let overdue = late(lens);
    for (id, contract) in book.contracts.iter().filter(|(_, contract)| lens.whose.includes(contract.owner)) {
        let kept = run.promises.iter().filter(|p| p.contract == id && p.kept.is_some_and(|(day, _)| day <= at));
        let missing: Vec<_> = overdue.iter().filter(|late| late.contract == id).collect();
        let next = Days::new(at.add_days(1), at.add(AHEAD)).and_then(|ahead| contract.due_days(ahead).first().copied());
        let owing = contract.loan.map(|loan| {
            let unit = loan.principal.unit;
            Amount::new(Qty(lens.held(loan.debt, unit).0 * lens.sides.sign(loan.debt)), unit)
        });
        let cells = [
            Cell::Name(book.name(contract.name)),
            Cell::Name(book.name(book.entities[contract.party].path)),
            terms_on(lens, contract, at),
            next.map_or(Cell::Blank, Cell::Day),
            Cell::Count(kept.count(), ""),
            missing.first().map_or(Cell::Blank, |oldest| Cell::Span(oldest.days(at))),
            missing.first().map_or(Cell::Blank, |oldest| Cell::Name(book.name(book.entities[oldest.blame].path))),
            owing.map_or(Cell::Blank, |owing| Cell::amount(book, owing)),
        ];
        table.push(Row::new(cells).style(if missing.is_empty() { Style::Normal } else { Style::Alert }));
        if let (Some(loan), Some(owing)) = (contract.loan, owing) {
            let owner = book.name(book.entities[contract.owner].path);
            table.fact("owed", Some(path(book, loan.debt)), owner, When::Instant(at), Money::of(book, owing));
        }
    }
    if table.rows.is_empty() {
        table.note("No contract is declared.");
    }
    Report::new(["Contracts on".into(), Cell::Day(at)]).with(table)
}
