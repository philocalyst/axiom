//! Typed parameter tables: dated and named values such as `limit[year]`.
//!
//! Rows are read from the S5 items in each [`Site`](crate::sources::Site).
//! Their first key, when present, is a year or date; any remaining keys are
//! names. A parameter's optional unit supplies the static dimension of its
//! values. Literal values stay in their natural representation so evaluation
//! can apply the declared unit with the same rounding rules as other amounts.

use axiom_core::{Day, Diagnostic, Dim, Sym};
use axiom_syntax::{ExprKind, File, Key, Param as Written, ParamRow as WrittenRow};

use crate::book::{Param, ParamRow};
use crate::collect::Collected;
use crate::declare::World;
use crate::errors::{Word, article};
use crate::law::{Ty, Value};
use crate::names::Scoped;
use crate::problem::{self, Noun};
use crate::scope::Home;

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

/// Declare native S5 params directly from their arranged source sites.
pub(crate) fn declare<'s>(world: &mut World<'s>, collected: &Collected<'_, 's>, diags: &mut Vec<Diagnostic>) {
    for param in &collected.params {
        let (file, written, home) = (param.file(), param.node, param.home());
        let system = if let Home::System(system) = home { Some(system) } else { None };
        let name = written.name.0;
        let sym = world.book.names.intern(name);
        let earlier = world.book.params.iter().find(|(_, param)| param.name == sym && param.system == system);
        if let Some((_, first)) = earlier {
            let (word, first) = (Word::of(file, name), Some(first.loc));
            diags.push(problem::duplicate(Noun::Param, word, first));
            continue;
        }

        let unit = match declared_unit(world, file, written) {
            Ok(unit) => unit,
            Err(error) => {
                diags.push(error);
                continue;
            }
        };
        let rows = rows(world, home, file, written, unit, diags);
        if !rows.is_empty() {
            world.book.params.push(Param { name: sym, unit, system, rows: rows.into(), loc: file.loc(name) });
        }
    }

    let things: Vec<_> = world
        .book
        .params
        .iter()
        .map(|(id, param)| (id, world.book.names.name(param.name), param.system.map_or(Home::Project, Home::System)))
        .collect();
    world.book.lookup.params = Scoped::build(&mut world.book.names, things);
}

fn declared_unit<'s>(
    world: &mut World<'s>,
    file: &File<'s>,
    param: &Written<'s>,
) -> Result<Option<Dim<axiom_core::Id<crate::book::Commodity>>>, Diagnostic> {
    let Some(unit) = param.unit else {
        return Ok(None);
    };
    parse_unit(world, unit.0, file.loc(unit.0)).map(Some)
}

/// Parse a supported dimension (`USD` or `USD/MI`) through the book's declared
/// commodities, retaining the exact source location for unknown components.
fn parse_unit<'s>(
    world: &mut World<'s>,
    text: &str,
    loc: axiom_core::Loc,
) -> Result<Dim<axiom_core::Id<crate::book::Commodity>>, Diagnostic> {
    let (top, bottom) = match text.split_once('/') {
        Some((top, bottom)) if !top.is_empty() && !bottom.is_empty() && !bottom.contains('/') => (top, Some(bottom)),
        Some(_) => {
            return Err(Diagnostic::error("param-unit", "a param unit is one commodity or a rate between two")
                .label(loc, "write `USD` or `USD/MI`"));
        }
        None => (text, None),
    };
    let numerator = world.commodity_of(Word { text: top, loc })?;
    match bottom {
        Some(bottom) => {
            let denominator = world.commodity_of(Word { text: bottom, loc })?;
            Ok(Dim::Per(numerator, denominator))
        }
        None => Ok(Dim::Of(numerator)),
    }
}

/// The good rows of a param, sorted by names and then by date. Bad rows are
/// diagnosed and omitted, so one typo does not hide the rest of the table.
fn rows<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &File<'s>,
    param: &Written<'s>,
    unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<ParamRow> {
    let mut rows: Vec<ParamRow> = Vec::new();
    for row in &file[param.rows] {
        match one_row(world, home, file, row, unit) {
            Ok(parsed) => {
                let shape_error = rows.first().and_then(|first| check_shape(first, &parsed).err());
                let type_anchor = rows.iter().find(|first| !matches!(first.value, Value::Empty));
                let type_error = type_anchor.and_then(|first| check_like(world, first, &parsed, unit).err());
                if let Some(error) = shape_error.or(type_error) {
                    diags.push(error);
                } else {
                    rows.push(parsed);
                }
            }
            Err(error) => diags.push(error),
        }
    }
    sort_rows(&mut rows, diags);
    rows
}

/// What a lookup finds a row by.
fn keys(row: &ParamRow) -> (&[Sym], Option<Day>) {
    (&row.names, row.since)
}

fn sort_rows(rows: &mut [ParamRow], diags: &mut Vec<Diagnostic>) {
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
}

impl Param {
    /// The latest row for exactly these name keys, in force on `when`.
    ///
    /// A date from another name group never participates in the step lookup.
    pub fn row_at(&self, names: &[Sym], when: Day) -> Option<&ParamRow> {
        self.rows
            .iter()
            .filter(|row| row.names.as_ref() == names && row.since.is_none_or(|since| since <= when))
            .max_by_key(|row| row.since)
    }
}

fn one_row<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &File<'s>,
    row: &WrittenRow<'s>,
    unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
) -> Result<ParamRow, Diagnostic> {
    let mut since = None;
    let mut names: Vec<Sym> = Vec::new();
    for (at, key) in file[row.keys].iter().enumerate() {
        let (day, loc) = match *key {
            Key::Name(name) => {
                names.push(world.book.names.intern(name.0));
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
    let value = row_value(world, home, file, row, unit)?;
    validate_unit(world, value, unit, row.loc)?;
    Ok(ParamRow { since, names: names.into(), value, loc: row.loc })
}

/// Preserve literal values. A compound amount such as `0.70 USD/MI` has no
/// dedicated runtime Value variant, so it is stored as its exact scalar while
/// the declared dimension remains on Param for static checking/evaluation.
fn row_value<'s>(
    world: &mut World<'s>,
    home: Home,
    file: &File<'s>,
    row: &WrittenRow<'s>,
    unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
) -> Result<Value, Diagnostic> {
    let expr = &file.exprs[row.value];
    if let (Some(Dim::Per(want_top, want_bottom)), ExprKind::Amount(amount)) = (unit, &expr.kind) {
        if let Some(written_unit) = amount.unit() {
            if written_unit.0.contains('/') {
                let found = parse_unit(world, written_unit.0, file.loc(written_unit.0))?;
                if found != Dim::Per(want_top, want_bottom) {
                    return Err(unit_mismatch(world, unit.unwrap(), found, expr.loc));
                }
                let ratio = amount.num().to_ratio().ok_or_else(|| {
                    Diagnostic::error("number-range", "this param value is too large").label(expr.loc, "out of range")
                })?;
                return Ok(Value::Num(ratio));
            }
        }
    }
    let (value, _) = world.constant(home, file, row.value, None)?;
    Ok(value)
}

fn validate_unit<'s>(
    world: &World<'s>,
    value: Value,
    unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
    loc: axiom_core::Loc,
) -> Result<(), Diagnostic> {
    let Some(unit) = unit else { return Ok(()) };
    let schedule_unit = match value {
        Value::Schedule(schedule) => Some(world.book.schedules[schedule].unit),
        _ => None,
    };
    let valid = unit_matches(unit, value, schedule_unit);
    if valid {
        Ok(())
    } else {
        Err(Diagnostic::error("param-unit", "this value does not have the param's declared unit")
            .label(loc, format!("expected {}", dimension_name(world, unit)))
            .help("write a value in the declared unit, or remove the unit from the param"))
    }
}

fn unit_matches(
    unit: Dim<axiom_core::Id<crate::book::Commodity>>,
    value: Value,
    schedule_unit: Option<axiom_core::Id<crate::book::Commodity>>,
) -> bool {
    match (unit, value) {
        (_, Value::Empty) => true,
        (Dim::Of(want), Value::Amount(amount)) => amount.unit == want,
        // A raw scalar is expressed in the param's declared unit. The stored
        // scalar is rounded only when the evaluator materializes a commodity
        // amount, using that commodity's precision and normal amount rules.
        (Dim::Of(_) | Dim::Per(_, _) | Dim::Number, Value::Num(_)) => true,
        (Dim::Of(want), Value::Schedule(_)) => schedule_unit == Some(want),
        _ => false,
    }
}

fn dimension_name(world: &crate::declare::World<'_>, dim: Dim<axiom_core::Id<crate::book::Commodity>>) -> String {
    let name = |unit: axiom_core::Id<crate::book::Commodity>| world.book.name(world.book.commodities[unit].symbol);
    match dim {
        Dim::Of(unit) => name(unit).to_owned(),
        Dim::Per(top, bottom) => format!("{}/{}", name(top), name(bottom)),
        Dim::Number => "a number".to_owned(),
        Dim::Any => "an amount".to_owned(),
        Dim::Rate(unit, period) => format!("{} per {:?}", name(unit), period),
    }
}

fn unit_mismatch<'s>(
    world: &crate::declare::World<'s>,
    expected: Dim<axiom_core::Id<crate::book::Commodity>>,
    found: Dim<axiom_core::Id<crate::book::Commodity>>,
    loc: axiom_core::Loc,
) -> Diagnostic {
    Diagnostic::error("param-unit", "this value does not have the param's declared unit")
        .label(loc, format!("expected {}, found {}", dimension_name(world, expected), dimension_name(world, found)))
}

/// A row must have the first row's key shape.
fn check_shape(first: &ParamRow, row: &ParamRow) -> Result<(), Diagnostic> {
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
    Ok(())
}

/// Values need one static type, and amounts/schedules with no declared unit
/// must also agree on their concrete commodity.
fn check_like<'s>(
    world: &crate::declare::World<'s>,
    first: &ParamRow,
    row: &ParamRow,
    unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>,
) -> Result<(), Diagnostic> {
    let same_storage_unit = match (first.value, row.value) {
        (Value::Amount(a), Value::Amount(b)) => a.unit == b.unit,
        (Value::Schedule(a), Value::Schedule(b)) => world.book.schedules[a].unit == world.book.schedules[b].unit,
        _ => true,
    };
    let (want, got) = value_types(first.value, row.value, unit);
    if same_storage_unit && crate::values::fits(want, got) {
        return Ok(());
    }
    Err(Diagnostic::error(
        "param-type",
        format!("this row's value is {}, but the first row's is {}", article(got.word()), article(want.word())),
    )
    .label(row.loc, format!("{} here", got.word()))
    .context(first.loc, format!("{} here", want.word())))
}

fn value_types(first: Value, row: Value, unit: Option<Dim<axiom_core::Id<crate::book::Commodity>>>) -> (Ty, Ty) {
    let one = |value| match (unit, value) {
        (Some(_), Value::Schedule(_)) => Ty::Schedule,
        (Some(unit), _) => Ty::Amount(unit),
        (None, Value::Amount(amount)) => Ty::Amount(Dim::Of(amount.unit)),
        (None, Value::Schedule(_)) => Ty::Schedule,
        (None, value) => value.ty().unwrap_or(Ty::Empty),
    };
    (one(first), one(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::{Id, Interner, Loc, Qty, Ratio};

    #[test]
    fn dated_lookup_uses_the_complete_name_key_before_the_latest_date() {
        let mut interner = Interner::default();
        let a = interner.intern("a");
        let b = interner.intern("b");
        let day = |year, month, day| Day::from_ymd(year, month, day).unwrap();
        let param = Param {
            name: a,
            unit: None,
            system: None,
            rows: vec![
                ParamRow {
                    since: Some(day(2024, 1, 1)),
                    names: Box::new([a]),
                    value: Value::Empty,
                    loc: Loc::default(),
                },
                ParamRow {
                    since: Some(day(2025, 1, 1)),
                    names: Box::new([b]),
                    value: Value::Empty,
                    loc: Loc::default(),
                },
                ParamRow {
                    since: Some(day(2026, 1, 1)),
                    names: Box::new([a]),
                    value: Value::Empty,
                    loc: Loc::default(),
                },
            ]
            .into(),
            loc: Loc::default(),
        };
        assert_eq!(param.row_at(&[a], day(2025, 12, 31)).unwrap().since, Some(day(2024, 1, 1)));
        assert_eq!(param.row_at(&[a], day(2026, 6, 1)).unwrap().since, Some(day(2026, 1, 1)));
        assert_eq!(param.row_at(&[b], day(2026, 6, 1)).unwrap().since, Some(day(2025, 1, 1)));
        assert!(param.row_at(&[b, a], day(2026, 6, 1)).is_none());
    }

    #[test]
    fn sorting_and_duplicate_detection_use_the_complete_key() {
        let mut interner = Interner::default();
        let a = interner.intern("a");
        let b = interner.intern("b");
        let day = |year| Day::from_ymd(year, 1, 1).unwrap();
        let mut rows = vec![
            ParamRow {
                since: Some(day(2026)),
                names: Box::new([a]),
                value: Value::Empty,
                loc: Loc::new(Default::default(), 20, 21),
            },
            ParamRow {
                since: Some(day(2025)),
                names: Box::new([b]),
                value: Value::Empty,
                loc: Loc::new(Default::default(), 10, 11),
            },
            ParamRow {
                since: Some(day(2025)),
                names: Box::new([a]),
                value: Value::Empty,
                loc: Loc::new(Default::default(), 30, 31),
            },
            ParamRow {
                since: Some(day(2025)),
                names: Box::new([a]),
                value: Value::Empty,
                loc: Loc::new(Default::default(), 40, 41),
            },
        ];
        let mut diags = Vec::new();
        sort_rows(&mut rows, &mut diags);
        assert_eq!(
            rows.iter().map(|row| (row.names[0], row.since.unwrap().year())).collect::<Vec<_>>(),
            [(a, 2025), (a, 2025), (a, 2026), (b, 2025)]
        );
        assert_eq!(diags.iter().map(|diag| &*diag.code).collect::<Vec<_>>(), ["duplicate-row"]);
    }

    #[test]
    fn declared_dimensions_reject_mixed_units_and_type_scalar_rows() {
        let usd = Id::new(0);
        let eur = Id::new(1);
        let mi = Id::new(2);
        let money = Dim::Of(usd);
        let rate = Dim::Per(usd, mi);
        let amount_usd = Value::Amount(crate::book::Amount::new(Qty::ZERO, usd));
        let amount_eur = Value::Amount(crate::book::Amount::new(Qty::ZERO, eur));

        assert!(unit_matches(money, amount_usd, None));
        assert!(!unit_matches(money, amount_eur, None));
        assert!(unit_matches(money, Value::Num(Ratio::ONE), None));
        assert!(unit_matches(rate, Value::Num(Ratio::ONE), None));
        assert!(!unit_matches(rate, amount_usd, None));
        assert!(unit_matches(rate, Value::Empty, None));
        let (want, got) = value_types(amount_usd, amount_eur, None);
        assert_ne!(want, got);
    }

    #[test]
    fn rows_with_different_key_shapes_are_rejected() {
        let mut interner = Interner::default();
        let a = interner.intern("a");
        let day = Day::from_ymd(2026, 1, 1).unwrap();
        let timed = ParamRow { since: Some(day), names: Box::new([a]), value: Value::Empty, loc: Loc::default() };
        let names_only = ParamRow { since: None, names: Box::new([a]), value: Value::Empty, loc: Loc::default() };
        assert!(check_shape(&timed, &names_only).is_err());
        assert_eq!(Shape::of(&timed).keys(), 2);
        assert_eq!(Shape::of(&names_only).keys(), 1);
    }
}
