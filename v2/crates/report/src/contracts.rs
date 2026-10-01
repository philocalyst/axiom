//! `contracts`: promises, their current terms and the next time due.

use axiom_core::{Days, Qty};
use axiom_engine::Run;
use axiom_model::{Book, Cadence, Contract, Flow, On, Terms, TermsState};

use crate::lens::Whose;
use crate::places::route;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &Book<'s>, run: &Run, whose: &Whose) -> Report<'s> {
    let mut section = Section::new([
        Column::left("Contract"),
        Column::left("Party"),
        Column::left("Terms"),
        Column::left("Next due"),
        Column::right("Kept"),
        Column::left("Late"),
        Column::right("Loan balance"),
    ]);
    for (id, contract) in book.contracts.iter() {
        if !whose.includes(contract.owner) {
            continue;
        }
        let name = book.name(contract.name);
        let terms = contract.terms_on(run.today);
        let next = contract
            .days
            .intersect(Days::new(run.today, contract.days.last()).unwrap_or(Days::on(run.today)))
            .and_then(|days| {
                contract
                    .occurrences(days)
                    .next()
                    .map(|occurrence| occurrence.day)
            });
        let promises = run.promises.iter().filter(|promise| promise.contract == id);
        let (kept, late, age) = promises.fold((0, 0, 0i64), |(kept, late, age), promise| {
            let late_days = promise.late(run.today);
            (
                kept + usize::from(promise.kept.is_some()),
                late + usize::from(late_days > 0),
                age + i64::from(late_days),
            )
        });
        let loan_balance = contract.loan.map(|loan| {
            run.holdings
                .iter()
                .filter(|holding| holding.place == loan.debt)
                .map(|holding| holding.qty())
                .sum::<Qty>()
        });
        let cells = [
            Cell::Name(name),
            Cell::Name(book.name(book.entities[contract.party].path)),
            terms_cell(book, contract, terms),
            next.map_or(Cell::Blank, Cell::Day),
            Cell::Count(kept, "kept"),
            if late == 0 {
                Cell::Blank
            } else {
                Cell::text(format!("{late} occurrences, {age} days"))
            },
            loan_balance.map_or(Cell::Blank, |qty| Cell::base(book, qty)),
        ];
        section.push(Row::new(cells).style(if late > 0 {
            Style::Alert
        } else {
            Style::Normal
        }));
    }
    if section.rows.is_empty() {
        section.note("No contracts are declared.");
    }
    Report::new("Contracts").with(section)
}

fn terms_cell<'s>(book: &Book<'s>, contract: &Contract, terms: &Terms) -> Cell<'s> {
    if terms.state == TermsState::Waived {
        return Cell::Word("waived");
    }
    let mut parts = vec![cadence(terms.every)];
    parts.extend(terms.on.iter().map(on_day));
    if let Some(purpose) = contract.purpose {
        parts.push(Cell::Purpose(
            book.name(book.purposes[purpose.value.purpose].name),
        ));
    }
    if let Some(description) = contract.description {
        parts.push(Cell::text(book.name(description)));
    }
    parts.extend(terms.template.iter().map(|flow| template_flow(book, flow)));
    Cell::list(" ", parts)
}

fn cadence(cadence: Cadence) -> Cell<'static> {
    match cadence {
        Cadence::Every(span) => Cell::text(format!("every {span}")),
        Cadence::TwiceMonthly => Cell::Word("twice monthly"),
    }
}

fn on_day(on: &On) -> Cell<'static> {
    match *on {
        On::MonthDay(day) => Cell::text(format!("on {day}")),
        On::Last => Cell::Word("on last"),
        On::YearDay { month, day } => Cell::text(format!("on {month:02}-{day:02}")),
        On::Weekday(day) => Cell::text(format!(
            "on {}",
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday"
            ][usize::from(day).min(6)]
        )),
    }
}

fn template_flow<'s>(book: &Book<'s>, flow: &Flow) -> Cell<'s> {
    Cell::list(
        " ",
        [Cell::text(route(book, flow)), Cell::amount(book, flow.out)],
    )
}
