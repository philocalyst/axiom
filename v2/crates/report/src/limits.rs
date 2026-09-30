//! `limits`: every cap and budget a person lives under, before anything breaks.
//!
//! One table from the run's headroom: what each law counted, what it allowed,
//! what room is left, and how much of it is used. The ones nearest their limit,
//! or past it, come first.

use axiom_core::Day;
use axiom_core::calendar::Window;
use axiom_engine::Headroom;
use axiom_model::{Amount, Book, Period, Subject};

use crate::headroom::{current, is_floor, is_over, latest, room, used};
use crate::lens::Lens;
use crate::places::path;
use crate::{Cell, Column, Money, Report, Row, Section, Style, When};

pub fn view<'s>(lens: Lens<'_, 's>, year: Option<i32>) -> Report<'s> {
    let (book, whose, today) = (lens.book, lens.whose, lens.day);
    let year = year.unwrap_or_else(|| today.year());
    let window = Window::containing(Period::Year, Day::from_ymd(year, 1, 1).unwrap_or(today)).days();
    let today = window.last().min(today);
    let all = &current(lens, today, today);
    // A floor of nothing (`balance >= empty`) is an invariant, not a limit:
    // what stands above it is the balance, which `balance` already shows.
    let limit = |reading: &&Headroom| !(is_floor(book, reading) && reading.counted.qty.is_zero());
    let touching = all.iter().filter(|reading| whose.includes(reading.owner) && reading.days.overlaps(window));
    let readings = latest(touching.filter(limit));
    let floors = readings.iter().any(|reading| is_floor(book, reading));
    let mut table = section(book, readings);
    if table.rows.is_empty() {
        table.note([
            "No limit was read in".into(),
            Cell::year(year),
            ". Laws record what they count when a `require` or `warn` compares two amounts.".into(),
        ]);
    } else if floors {
        table.note("A floor, like a minimum payment, shows what stands as counted and the room above it.");
    }
    Report::new(["Limits in".into(), Cell::year(year)]).with(table)
}

/// The readings as a table, past their limit first, then by share used;
/// floors, which have none, last.
pub fn section<'s>(book: &Book<'s>, mut readings: Vec<&Headroom>) -> Section<'s> {
    let key = |reading: &Headroom| {
        let share = used(reading).filter(|_| !is_floor(book, reading));
        (!is_over(reading), share.is_none(), share.map(|share| -share))
    };
    let name = |reading: &Headroom| (book.name(book.laws[reading.law].name), subject_name(book, reading.subject));
    readings.sort_by(|a, b| key(a).cmp(&key(b)).then_with(|| name(a).cmp(&name(b))));
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
        let (of, owner) = (subject_name(book, reading.subject), book.name(book.entities[reading.owner].path));
        table.fact("counted", Some(of), owner, When::During(reading.days), Money::of(book, reading.counted));
        table.fact("limit", Some(of), owner, When::During(reading.days), Money::of(book, reading.limit));
        table.push(row(book, reading));
    }
    table
}

/// What a law watches, by name.
fn subject_name<'s>(book: &Book<'s>, subject: Subject) -> &'s str {
    match subject {
        Subject::Place(place) => path(book, place),
        Subject::Entity(entity) => book.name(book.entities[entity].path),
        Subject::Asset(asset) => book.name(book.assets[asset].name),
    }
}

/// `deferral-limit on assets/retirement`: the law and what it watches.
fn what<'s>(book: &Book<'s>, reading: &Headroom) -> Cell<'s> {
    let word = if matches!(reading.subject, Subject::Entity(_)) { "for" } else { "on" };
    [Cell::Name(book.name(book.laws[reading.law].name)), word.into(), Cell::Name(subject_name(book, reading.subject))]
        .into()
}

/// One reading as a row of the table.
pub fn row<'s>(book: &Book<'s>, reading: &Headroom) -> Row<'s> {
    let floor = is_floor(book, reading);
    // A floor is written the other way about: what stands, and what it may not go below.
    let (counted, cap) = if floor {
        (reading.limit, ["floor".into(), Cell::amount(book, reading.counted)].into())
    } else {
        (reading.counted, Cell::amount(book, reading.limit))
    };
    let share = used(reading).filter(|_| !floor).map_or(Cell::Blank, Cell::Percent);
    let cells = [
        Cell::Name(book.name(book.entities[reading.owner].path)),
        what(book, reading),
        Cell::Period(reading.days),
        Cell::amount(book, counted),
        cap,
        Cell::amount(book, Amount::new(room(reading), reading.limit.unit)),
        share,
    ];
    Row::new(cells).style(if is_over(reading) { Style::Alert } else { Style::Normal })
}
