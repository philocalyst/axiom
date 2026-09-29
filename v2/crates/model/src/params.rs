//! Params: dated tables of values, `limit[year]`.
//!
//! A row is keys and a value: `2026 single 0 USD 10% | …`. The first key may be
//! a year or a date (a step lookup: the latest row at or before the day asked
//! for), the rest are names. Every row of a param has the same shape and the
//! same type of value, so a lookup can be checked when the law is compiled.

use axiom_core::{Day, Diagnostic, Sym};
use axiom_syntax::Key;

use crate::book::{Param, ParamRow};
use crate::catalog::{Catalog, Written};
use crate::errors::{article, duplicate};
use crate::law::Ty;
use crate::names::Scoped;
use crate::scope::Home;
use crate::world::World;

/// What every row of a param looks like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Shape {
    /// Whether the first key is a year or a date.
    pub timed: bool,
    /// How many name keys follow.
    pub names: usize,
}

impl Shape {
    pub fn of(row: &ParamRow) -> Shape {
        Shape { timed: row.since.is_some(), names: row.names.len() }
    }

    pub fn keys(self) -> usize {
        self.names + self.timed as usize
    }
}

pub(crate) fn declare<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    for written in &catalog.params {
        let system = match written.home() {
            Home::System(system) => Some(system),
            Home::Project | Home::Builtin => None,
        };
        let name = written.what.name;
        let sym = world.book.names.intern(name.text);
        let earlier = world.book.params.iter().find(|(_, param)| param.name == sym && param.system == system);
        if let Some((_, first)) = earlier {
            diags.push(duplicate("param", name, Some(first.loc)));
            continue;
        }
        let rows = rows(world, written, diags);
        if rows.is_empty() {
            continue;
        }
        world.book.params.push(Param { name: sym, system, rows: rows.into(), loc: name.loc });
    }
    let things: Vec<_> = world
        .book
        .params
        .iter()
        .map(|(id, param)| (id, world.book.names.name(param.name), param.system.map_or(Home::Project, Home::System)))
        .collect();
    world.book.lookup.params = Scoped::build(&mut world.book.names, things);
}

/// The good rows of a param, sorted by names and then by day. Bad rows are
/// reported and left out, so one typo does not hide the table.
fn rows<'s>(
    world: &mut World<'s>,
    written: &Written<'_, 's, axiom_syntax::Param<'s>>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<ParamRow> {
    let mut rows: Vec<ParamRow> = Vec::new();
    for row in &written.what.rows {
        match one_row(world, written, row) {
            Ok(parsed) => match rows.first().map(|first| check_like(first, &parsed)) {
                Some(Err(mismatch)) => diags.push(mismatch),
                _ => rows.push(parsed),
            },
            Err(error) => diags.push(error),
        }
    }
    rows.sort_by(|a, b| keys(a).cmp(&keys(b)));
    for pair in rows.windows(2) {
        if keys(&pair[0]) == keys(&pair[1]) {
            diags.push(
                Diagnostic::error("duplicate-row", "two rows have the same keys")
                    .label(pair[1].loc, "the same keys as the row before")
                    .context(pair[0].loc, "first row")
                    .help("a lookup could not choose between them"),
            );
        }
    }
    rows
}

/// What a lookup finds a row by.
fn keys(row: &ParamRow) -> (&[Sym], Option<Day>) {
    (&row.names, row.since)
}

fn one_row<'s>(
    world: &mut World<'s>,
    written: &Written<'_, 's, axiom_syntax::Param<'s>>,
    row: &axiom_syntax::ParamRow<'s>,
) -> Result<ParamRow, Diagnostic> {
    let mut since = None;
    let mut names: Vec<Sym> = Vec::new();
    for (at, key) in row.keys.iter().enumerate() {
        let (day, loc) = match *key {
            Key::Name(name) => {
                names.push(world.book.names.intern(name.text));
                continue;
            }
            Key::Year(year, loc) => (Day::from_ymd(year, 1, 1), loc),
            Key::Date(day, loc) => (Some(day), loc),
        };
        if at > 0 {
            return Err(Diagnostic::error("param-key-order", "the year or date comes first")
                .label(loc, "move it before the names"));
        }
        since =
            Some(day.ok_or_else(|| {
                Diagnostic::error("param-key", "this year does not exist").label(loc, "out of range")
            })?);
    }
    let (value, _) = world.constant(written.home(), written.exprs(), row.value, None)?;
    Ok(ParamRow { since, names: names.into(), value, loc: row.loc })
}

/// A row must look like the first: the same keys, the same kind of value.
fn check_like(first: &ParamRow, row: &ParamRow) -> Result<(), Diagnostic> {
    let (shape, found) = (Shape::of(first), Shape::of(row));
    if shape != found {
        return Err(Diagnostic::error(
            "param-shape",
            format!("this row has {} keys, but the first has {}", found.keys(), shape.keys()),
        )
        .label(row.loc, "a different shape")
        .context(first.loc, "the first row")
        .help("give every row the same keys, so that `param[key]` means one thing"));
    }
    let (want, got) = (first.value.ty(), row.value.ty());
    match (want, got) {
        (Some(want), Some(got)) if crate::values::fits(want, got) => Ok(()),
        _ => {
            let word = |ty: Option<Ty>| ty.map_or("nothing", Ty::word);
            Err(Diagnostic::error(
                "param-type",
                format!("this row's value is {}, but the first row's is {}", article(word(got)), article(word(want))),
            )
            .label(row.loc, format!("{} here", word(got)))
            .context(first.loc, format!("{} here", word(want))))
        }
    }
}
