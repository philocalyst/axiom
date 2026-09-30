//! `limits`: every cap and budget a person lives under, before anything breaks.
//!
//! One table from the run's headroom: what each law counted, what it allowed,
//! what room is left, and how much of it is used. The ones nearest their limit,
//! or past it, come first.

use axiom_core::Day;
use axiom_core::calendar::Window;
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, Book, Period, Subject};

use crate::headroom::{current, is_floor, is_over, latest, room, used, window_words};
use crate::lens::Whose;
use crate::places::path;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn view<'s>(book: &Book<'s>, run: &Run, whose: &Whose, year: Option<i32>) -> Report<'s> {
    let year = year.unwrap_or_else(|| run.today.year());
    let window = Window::containing(Period::Year, Day::from_ymd(year, 1, 1).unwrap_or(run.today)).days();
    let today = window.last().min(run.today);
    let all = &current(book, run, today, today);
    // A floor of nothing (`balance >= empty`) is an invariant, not a limit:
    // what stands above it is the balance, which `balance` already shows.
    let limit = |reading: &&Headroom| !(is_floor(book, reading) && reading.counted.qty.is_zero());
    let touching = all.iter().filter(|reading| whose.includes(reading.owner) && reading.days.overlaps(window));
    let readings = latest(touching.filter(limit));
    let floors = readings.iter().any(|reading| is_floor(book, reading));
    let mut table = section(book, readings);
    if table.rows.is_empty() {
        table.note(format!(
            "No limit was read in {year}. Laws record what they count when a `require` or `warn` compares two amounts."
        ));
    } else if floors {
        table.note("A floor, like a minimum payment, shows what stands as counted and the room above it.");
    }
    Report::new(format!("Limits in {year}")).with(table)
}

/// The readings as a table, past their limit first, then by share used;
/// floors, which have none, last.
pub fn section<'s>(book: &Book<'s>, mut readings: Vec<&Headroom>) -> Section<'s> {
    let key = |reading: &Headroom| {
        let share = used(reading).filter(|_| !is_floor(book, reading));
        (!is_over(reading), share.is_none(), share.map(|share| -share))
    };
    readings.sort_by(|a, b| key(a).cmp(&key(b)).then_with(|| what(book, a).cmp(&what(book, b))));
    let columns = [
        Column::left("Who"),
        Column::left("Limit"),
        Column::left("Window"),
        Column::right("Counted"),
        Column::right("Cap"),
        Column::right("Room left"),
        Column::right("Used"),
    ];
    let mut table = Section::new(columns);
    for reading in readings {
        table.push(row(book, reading));
    }
    table
}

/// `deferral-limit on assets/retirement`: the law and what it watches.
pub fn what(book: &Book, reading: &Headroom) -> String {
    let law = book.name(book.laws[reading.law].name);
    match reading.subject {
        Subject::Place(place) => format!("{law} on {}", path(book, place)),
        Subject::Entity(entity) => format!("{law} for {}", book.name(book.entities[entity].path)),
        Subject::Asset(asset) => format!("{law} on {}", book.name(book.assets[asset].name)),
    }
}

/// One reading as a row of the table.
pub fn row<'s>(book: &Book<'s>, reading: &Headroom) -> Row<'s> {
    let floor = is_floor(book, reading);
    // A floor is written the other way about: what stands, and what it may not go below.
    let (counted, cap) = if floor {
        (reading.limit, Cell::text(format!("floor {}", book.show(reading.counted))))
    } else {
        (reading.counted, Cell::amount(book, reading.limit))
    };
    let share = used(reading).filter(|_| !floor).map_or(Cell::Blank, Cell::Percent);
    let cells = [
        Cell::text(book.name(book.entities[reading.owner].path)),
        Cell::text(what(book, reading)),
        Cell::text(window_words(reading)),
        Cell::amount(book, counted),
        cap,
        Cell::amount(book, Amount::new(room(reading), reading.limit.unit)),
        share,
    ];
    Row::new(cells).style(if is_over(reading) { Style::Alert } else { Style::Normal })
}
