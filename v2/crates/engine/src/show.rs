//! Words for values: every place the engine prints one goes through here.

use axiom_core::{Day, Id};
use axiom_model::{Book, Commodity, Fault, Place, Subject, Value};

/// A value the way a person writes it: `24,500.00 USD`, `2026-04-15`, `59y6m`.
/// `day` is the day being evaluated, which a missing price names.
pub(crate) fn value(book: &Book, day: Day, value: Value) -> String {
    match value {
        Value::Empty => "empty".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => n.to_string(),
        Value::Amount(a) => book.show(a).to_string(),
        Value::Day(d) => d.to_string(),
        Value::Span(s) => s.to_string(),
        Value::Text(t) => format!("\"{}\"", book.name(t)),
        Value::Name(n) | Value::Glob(n) => book.name(n).into(),
        Value::Code(c) => format!("#{}", book.name(c).trim_start_matches('#')),
        Value::Place(p) => place(book, p).into(),
        Value::Entity(e) => book.name(book.entities[e].path).into(),
        Value::Kind(k) => book.name(book.kinds[k].name).into(),
        Value::Unit(u) => book.name(book.commodities[u].symbol).into(),
        Value::Purpose(p, _) => format!("#{}", book.name(book.purposes[p].name)),
        Value::Asset(a) => book.name(book.assets[a].name).into(),
        Value::Schedule(_) => "a schedule".into(),
        Value::Flow => "this flow".into(),
        Value::Fault(f) => format!("no value ({})", fault(book, f, day).0),
    }
}

pub(crate) fn place<'a>(book: &Book<'a>, place: Id<Place>) -> &'a str {
    book.name(book.places[place].path)
}

pub(crate) fn subject<'a>(book: &Book<'a>, subject: Subject) -> &'a str {
    match subject {
        Subject::Place(p) => place(book, p),
        Subject::Entity(e) => book.name(book.entities[e].path),
        Subject::Asset(a) => book.name(book.assets[a].name),
    }
}

/// A subject as a sortable key: places, then entities, then assets, each by id.
pub(crate) fn subject_key(subject: Subject) -> (u8, usize) {
    match subject {
        Subject::Place(place) => (0, place.index()),
        Subject::Entity(entity) => (1, entity.index()),
        Subject::Asset(asset) => (2, asset.index()),
    }
}

/// Why a value could not be computed, and the fix. `day` is the day the law
/// was evaluated for, which the missing-price message names.
pub(crate) fn fault(book: &Book, fault: Fault, day: Day) -> (String, Option<String>) {
    let symbol = |unit: Id<Commodity>| book.name(book.commodities[unit].symbol);
    match fault {
        Fault::NoPrice { unit, quote } => {
            let (unit, quote) = (symbol(unit), symbol(quote));
            let help = format!("add a price line such as `{day} {unit} <price> {quote}`");
            (format!("no price for {unit} in {quote} on {day}"), Some(help))
        }
        Fault::Unset(name) => {
            let name = book.name(name);
            (format!("`{name}` is not set"), Some(format!("give it a value where the thing is declared: `{name} …`")))
        }
        Fault::NoRow(param) => {
            let name = book.name(book.params[param].name);
            (
                format!("`{name}` has no row for {day}"),
                Some(format!("add a row to `param {name}` that starts on or before that day")),
            )
        }
        Fault::DivideByZero => ("division by zero".into(), None),
        Fault::Overflow => ("a value out of range".into(), None),
    }
}
