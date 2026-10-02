//! Filling slots: what a property line gives the slot it names, checked once.
//!
//! A line `NAME VALUE[, VALUE …]` under a thing fills the slot called NAME of the thing's kind, and under a kind it
//! fills it for every thing of that kind that does not say otherwise. The line is read against the slot's range, so a
//! thing of the wrong kind or a word the slot does not know is said where it is written; it is counted against the
//! slot's multiplicity, and for a slot that weighs its values each value is read with its weight
//! (`owners dana 60%, theo 40%`). What comes out is typed values. Nothing downstream asks again.
//!
//! A required slot that nothing fills, neither the thing nor any kind above it, is said once all the declarations are
//! read.

use axiom_core::{Diagnostic, Id, Loc, Ratio, Set, SlotId};
use axiom_syntax::{ExprId, ExprKind, File, Mult, Prop as Line};

use crate::book::{Amount, Kind};
use crate::declare::World;
use crate::errors::{Word, article};
use crate::law::{Ty, Value};
use crate::problem;
use crate::scope::Home;
use crate::slots::{Range, Slot, receiver};
use crate::values::describe;

/// What weighs one value of a slot: a rate (a percentage, a fraction or a number), or an amount of a commodity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Measure {
    Rate(Ratio),
    Amount(Amount),
}

/// What a line gave a slot: its values in the order written and, if the slot weighs them, a weight for each.
#[derive(Default, Debug)]
pub(crate) struct Filling {
    pub values: Vec<Value>,
    pub weights: Vec<Measure>,
}

/// The line being read, and where it is written.
pub(crate) struct At<'a, 's> {
    pub home: Home,
    pub file: &'a File<'s>,
    pub line: &'a Line<'s>,
}

/// The most values a slot takes.
fn capacity(mult: Mult) -> usize {
    match mult {
        Mult::One | Mult::Optional => 1,
        Mult::Some | Mult::Many => usize::MAX,
    }
}

/// Whether a slot takes one value at most.
pub(crate) fn holds_one(mult: Mult) -> bool {
    capacity(mult) == 1
}

/// Whether a slot must be filled.
pub(crate) fn is_required(mult: Mult) -> bool {
    matches!(mult, Mult::One | Mult::Some)
}

/// The values a line gives `slot`.
pub(crate) fn fill<'s>(world: &mut World<'s>, at: &At<'_, 's>, slot: &Slot) -> Result<Filling, Diagnostic> {
    let name = world.book.name(slot.name);
    let line = at.line;
    let args = &at.file[line.args];
    if !at.file[line.lines].is_empty() {
        return Err(Diagnostic::error("property-nested-lines", "a property value cannot have nested lines")
            .label(line.loc, "remove the nested lines"));
    }
    let width = 1 + usize::from(slot.weight.is_some());
    if args.is_empty() {
        return Err(Diagnostic::error("property-value", format!("`{name}` needs a value"))
            .label(line.loc, "write a value here"));
    }
    if args.len() % width != 0 {
        let last = at.file.exprs[args[args.len() - 1]].loc;
        return Err(problem::missing_weight(name, world.book.name(slot.weight.expect("a weight").name), last));
    }
    let given = args.len() / width;
    if given > capacity(slot.mult) {
        let (kept, extra) = (args[capacity(slot.mult) * width - 1], args[capacity(slot.mult) * width]);
        let (kept, last) = (at.file.exprs[kept].loc, at.file.exprs[args[args.len() - 1]].loc);
        return Err(problem::too_many(name, given, at.file.exprs[extra].loc, Loc::new(kept.file, kept.end, last.end)));
    }
    let mut filling = Filling::default();
    for pair in args.chunks(width) {
        filling.values.push(value(world, at, slot, pair[0])?);
        if let (Some(weight), Some(&arg)) = (slot.weight, pair.get(1)) {
            filling.weights.push(weighed(world, at, name, weight.unit, arg)?);
        }
    }
    Ok(filling)
}

/// One value of a slot, read against its range.
fn value<'s>(world: &mut World<'s>, at: &At<'_, 's>, slot: &Slot, id: ExprId) -> Result<Value, Diagnostic> {
    match slot.range {
        Range::Kinds(run) => {
            let kinds = world.book.schema.kinds_of(run).to_vec();
            thing(world, at, slot, &kinds, id)
        }
        Range::Words(run) => word(world, at, slot, run, id),
        Range::Value(ty) => typed(world, at, slot, ty, id),
    }
}

/// A thing of one of `kinds`, named.
fn thing<'s>(
    world: &mut World<'s>,
    at: &At<'_, 's>,
    slot: &Slot,
    kinds: &[Id<Kind>],
    id: ExprId,
) -> Result<Value, Diagnostic> {
    let expr = &at.file.exprs[id];
    let takes = world.book.schema.view(slot.range).describe(&world.book);
    let ExprKind::Name(written) = expr.kind else {
        let text = &at.file.src[expr.loc.range()];
        let found = describe(&expr.kind);
        return Err(problem::wrong_kind(world.book.name(slot.name), Word { text, loc: expr.loc }, found, &takes, &[]));
    };
    let word = Word { text: written.0, loc: expr.loc };
    let ty = receiver(world.book.kinds[kinds[0]].sort);
    let (value, kind) = match ty {
        Ty::Entity => {
            world.entity(at.home, word).map(|entity| (Value::Entity(entity), world.book.entities[entity].kind))?
        }
        Ty::Place => world.place(word).map(|place| (Value::Place(place), world.book.places[place].kind))?,
        Ty::Asset => match world.book.asset(word.text) {
            Some(asset) => (Value::Asset(asset), world.book.assets[asset].kind),
            None => return Err(world.missing_asset(word)),
        },
        _ => world.commodity_of(word).map(|unit| (Value::Unit(unit), world.book.commodities[unit].kind))?,
    };
    if kinds.iter().any(|&of| world.book.is_a(kind, of)) {
        return Ok(value);
    }
    let found = article(world.book.name(world.book.kinds[kind].name));
    let candidates = fitting(world, kinds);
    Err(problem::wrong_kind(world.book.name(slot.name), word, &found, &takes, &candidates))
}

/// The names of the things of one of `kinds`: what a slot that took the wrong thing could take.
pub(crate) fn fitting<'w>(world: &'w World<'_>, kinds: &[Id<Kind>]) -> Vec<&'w str> {
    let book = &world.book;
    let ty = receiver(book.kinds[kinds[0]].sort);
    let fits = |kind: Id<Kind>| kinds.iter().any(|&of| book.is_a(kind, of));
    let named = |names: &mut Vec<&'w str>, name, kind| {
        if fits(kind) {
            names.push(book.name(name));
        }
    };
    let mut names = Vec::new();
    match ty {
        Ty::Entity => book.entities.values().for_each(|entity| named(&mut names, entity.path, entity.kind)),
        Ty::Place => book.places.values().for_each(|place| named(&mut names, place.path, place.kind)),
        Ty::Asset => book.assets.values().for_each(|asset| named(&mut names, asset.name, asset.kind)),
        _ => book.commodities.values().for_each(|unit| named(&mut names, unit.symbol, unit.kind)),
    }
    names
}

/// One of the words of a range.
fn word<'s>(
    world: &mut World<'s>,
    at: &At<'_, 's>,
    slot: &Slot,
    run: axiom_core::Run<axiom_core::Sym>,
    id: ExprId,
) -> Result<Value, Diagnostic> {
    let expr = &at.file.exprs[id];
    let words = world.book.schema.words_of(run);
    let spelled: Vec<&str> = words.iter().map(|&word| world.book.name(word)).collect();
    if let ExprKind::Name(written) = expr.kind
        && let Some(&found) = words.iter().find(|&&word| world.book.name(word) == written.0)
    {
        return Ok(Value::Name(found));
    }
    let text = &at.file.src[expr.loc.range()];
    Err(problem::wrong_word(world.book.name(slot.name), Word { text, loc: expr.loc }, &spelled))
}

/// A value of a type: a literal or a name, as a property's value always was.
fn typed<'s>(world: &mut World<'s>, at: &At<'_, 's>, slot: &Slot, ty: Ty, id: ExprId) -> Result<Value, Diagnostic> {
    let expr = &at.file.exprs[id];
    let wanted = match ty {
        Ty::Amount(_) if matches!(expr.kind, ExprKind::Num(_) | ExprKind::Pct(_)) => Ty::Num,
        Ty::Amount(_) => Ty::AMOUNT,
        other => other,
    };
    let (value, found) = world.constant(at.home, at.file, id, Some(wanted))?;
    if fits_value(ty, value, found) {
        return Ok(value);
    }
    let expected = match ty {
        Ty::Amount(dim) => format!("an amount in {}", dimension_name(world, dim)),
        ty => article(ty.word()),
    };
    Err(Diagnostic::error("property-type", format!("`{}` needs {expected}", world.book.name(slot.name)))
        .label(expr.loc, format!("this is {}", describe(&expr.kind))))
}

fn fits_value(expected: Ty, value: Value, found: Ty) -> bool {
    use axiom_core::Dim;
    match (expected, value) {
        (Ty::Amount(Dim::Any), Value::Amount(_) | Value::Empty) => true,
        (Ty::Amount(Dim::Of(want)), Value::Amount(amount)) => amount.unit == want,
        (Ty::Amount(Dim::Of(_) | Dim::Per(..) | Dim::Rate(..) | Dim::Number), Value::Num(_)) => true,
        (Ty::Amount(_), Value::Empty) => true,
        (ty, _) => crate::values::fits(ty, found),
    }
}

fn dimension_name(world: &World<'_>, dim: axiom_core::Dim<Id<crate::book::Commodity>>) -> String {
    use axiom_core::Dim;
    let unit = |unit: Id<crate::book::Commodity>| world.book.name(world.book.commodities[unit].symbol).to_owned();
    match dim {
        Dim::Number => "a number".to_owned(),
        Dim::Of(id) => unit(id),
        Dim::Per(top, bottom) => format!("{}/{}", unit(top), unit(bottom)),
        Dim::Rate(id, period) => format!("{} per {period:?}", unit(id)),
        Dim::Any => "an amount".to_owned(),
    }
}

/// A weight: a percentage, a fraction or a number, or an amount of the commodity the slot weighs by.
fn weighed<'s>(
    world: &mut World<'s>,
    at: &At<'_, 's>,
    slot: &str,
    unit: Option<Id<crate::book::Commodity>>,
    id: ExprId,
) -> Result<Measure, Diagnostic> {
    let expr = &at.file.exprs[id];
    let (value, _) = world.literal(at.home, at.file, expr)?.ok_or_else(|| {
        problem::weight_type(slot, unit.map(|unit| world.book.name(world.book.commodities[unit].symbol)), expr.loc)
    })?;
    match (value, unit) {
        (Value::Num(rate), None) if !rate.is_negative() => Ok(Measure::Rate(rate)),
        (Value::Amount(amount), Some(unit)) if amount.unit == unit && amount.qty.0 >= 0 => Ok(Measure::Amount(amount)),
        _ => Err(problem::weight_type(
            slot,
            unit.map(|unit| world.book.name(world.book.commodities[unit].symbol)),
            expr.loc,
        )),
    }
}

/// The slots of a thing that something has filled, by the thing and the slot's number.
pub(crate) type Filled = Set<(u8, u32, SlotId)>;

#[cfg(test)]
mod tests {
    use axiom_core::FileId;
    use axiom_syntax::{Folder, parse};

    use super::*;
    use crate::{Book, Source, build};

    const STD: &str = "\
system std
kind bank : asset
kind org : entity
kind employer : org
kind person : entity
kind student : person
kind household : entity
kind currency : commodity
commodity USD : currency
  precision 2
";

    fn book<'s>(project: &'s str) -> (Book<'s>, Vec<Diagnostic>) {
        let source = |id, path, text, embedded| {
            let (file, diags) = parse(FileId(id), text, Folder::default());
            assert!(diags.is_empty(), "{path}: {diags:?}");
            Source { path, file, embedded }
        };
        build(&[source(0, "std.ax", STD, true), source(1, "axiom.ax", project, false)])
    }

    fn only<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
        let found: Vec<_> = diags.iter().filter(|diagnostic| diagnostic.code == code).collect();
        assert_eq!((found.len(), diags.len()), (1, 1), "exactly one diagnostic, {code}: {diags:?}");
        found[0]
    }

    fn edit(diagnostic: &Diagnostic) -> (&str, &str) {
        let help = &diagnostic.help[0];
        (&help.text, &help.edit.as_ref().expect("an edit").1)
    }

    const PLAN: &str = "\
use std
kind plan : asset
  has beneficiary person
  has coverage one of self-only | family optional
entity me : person
entity acme : employer
entity riley : student
";

    #[test]
    fn a_thing_of_the_wrong_kind_is_named_with_what_it_is_and_what_the_slot_takes() {
        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary acme\n"));
        let error = only(&diags, "wrong-kind");
        assert_eq!(error.message, "`acme` is an employer, and `beneficiary` takes a person");
        assert_eq!(error.labels[0].text, "an employer, not a person");
        assert_eq!(error.notes[0], "it could be `me` or `riley`", "a person, or a student, which is a person");
    }

    #[test]
    fn a_wrong_thing_is_offered_the_closest_fitting_one_by_spelling_or_the_only_one() {
        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary riely\n"));
        assert_eq!(edit(&only(&diags, "unknown-entity").clone()), ("did you mean `riley`?", "riley"));

        let (_, diags) = book(
            "use std\nkind plan : asset\n  has sponsor employer\nentity acme : org\nentity me : person\nentity boss : employer\naccount college : plan\n  sponsor acme\n",
        );
        let error = only(&diags, "wrong-kind");
        assert_eq!(
            edit(error),
            ("`boss` is the only one that fits", "boss"),
            "a person is no employer, an org is not either"
        );

        let (_, diags) = book(
            "use std\nkind plan : asset\n  has sponsor employer\nentity acme : org\nentity a : employer\nentity b : employer\naccount college : plan\n  sponsor acme\n",
        );
        let error = only(&diags, "wrong-kind");
        assert_eq!(error.notes[0], "it could be `a` or `b`");
    }

    #[test]
    fn a_word_that_is_not_one_of_the_words_is_said_with_the_words() {
        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary me\n  coverage famly\n"));
        let error = only(&diags, "wrong-word");
        assert_eq!(error.message, "`famly` is not one of the words `coverage` takes");
        assert_eq!(error.notes[0], "`coverage` takes one of `self-only` or `family`");
        assert_eq!(edit(error), ("did you mean `family`?", "family"));
        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary me\n  coverage 5\n"));
        only(&diags, "wrong-word");
    }

    #[test]
    fn a_value_of_a_type_keeps_the_errors_a_property_always_had() {
        let source = "use std\nkind plan : asset\n  has born date\n  has fee USD\naccount a : plan\n  born 2020-01-01\n  fee 5 USD\n";
        let (_, diags) = book(source);
        assert!(diags.iter().all(|diagnostic| diagnostic.code == "missing-role"), "{diags:?}");
        let (_, diags) = book("use std\nkind plan : asset\n  has born date optional\naccount a : plan\n  born 5 USD\n");
        assert_eq!(only(&diags, "type-mismatch").message, "expected a date, but this is an amount");
        let (_, diags) = book("use std\nkind plan : asset\n  has fee USD optional\naccount a : plan\n  fee 5 EUR\n");
        only(&diags, "unknown-commodity");
    }

    #[test]
    fn a_slot_of_one_value_takes_one_line_and_one_value() {
        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary me, riley\n"));
        let error = only(&diags, "too-many");
        assert_eq!(error.message, "`beneficiary` takes one value, and this line gives 2");
        assert_eq!(edit(error), ("keep the first", ""));

        let (_, diags) = book(&format!("{PLAN}account college : plan\n  beneficiary me\n  beneficiary riley\n"));
        let error = only(&diags, "too-many");
        assert_eq!(error.message, "`beneficiary` takes one value, and it is filled twice");
        assert_eq!(error.labels.len(), 2);
    }

    #[test]
    fn a_slot_that_holds_several_takes_as_many_as_are_written() {
        let project = "use std\nkind plan : asset\n  has dependents person many\n  has tags one of a | b some\nentity me : person\nentity riley : student\naccount college : plan\n  dependents me, riley\n  dependents me\n  tags a b\n";
        let (_, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_weighed_slot_reads_each_value_with_its_weight() {
        let source = |line: &str| {
            format!(
                "use std\nkind firm : entity\n  has owners person many by share\n  has rents person many by rent USD\nentity me : person\nentity theo : person\nentity acme : firm\n  {line}\n"
            )
        };
        let (_, diags) = book(&source("owners me 60%, theo 40%"));
        assert!(diags.is_empty(), "{diags:?}");
        let (_, diags) = book(&source("owners me 1/3, theo 2"));
        assert!(diags.is_empty(), "a fraction and a number are rates: {diags:?}");
        let (_, diags) = book(&source("rents me 880 USD, theo 120 USD"));
        assert!(diags.is_empty(), "{diags:?}");

        let (_, diags) = book(&source("owners me 60% theo"));
        assert_eq!(only(&diags, "missing-weight").message, "`owners` is weighed by share, and this value has none");
        let (_, diags) = book(&source("owners me 60%, theo 5 USD"));
        assert_eq!(only(&diags, "weight-type").message, "`owners` is weighed by a rate");
        let (_, diags) = book(&source("rents me 880 EUR"));
        only(&diags, "unknown-commodity");
        let (_, diags) = book(&source("rents me 880"));
        let error = only(&diags, "weight-type");
        assert_eq!(error.message, "`rents` is weighed in USD");
        assert_eq!(error.help[0].text, "write an amount in USD, such as `100 USD`");
    }

    #[test]
    fn a_required_slot_nothing_fills_is_said_at_the_thing_with_the_line_that_fills_it() {
        let (_, diags) = book(&format!("{PLAN}account college : plan\n"));
        let error = only(&diags, "missing-role");
        assert_eq!(error.message, "`college` has no `beneficiary`");
        assert_eq!(error.labels[0].text, "a plan takes a person as its `beneficiary`");
        assert!(error.notes.iter().any(|note| note.starts_with("it could be")), "{error:?}");

        let (_, diags) =
            book("use std\nkind plan : asset\n  has beneficiary person\nentity me : person\naccount college : plan\n");
        let error = only(&diags, "missing-role");
        let (loc, text) = error.help[0].edit.clone().expect("an edit");
        assert_eq!(text, "\n  beneficiary me");
        assert_eq!(loc.start, loc.end, "inserted after the header line");

        let (_, diags) = book("use std\nkind plan : asset\n  has born date\naccount college : plan\n");
        assert_eq!(only(&diags, "missing-role").help[0].text, "add a line giving a date: `born VALUE`");
    }

    #[test]
    fn a_kind_above_fills_a_slot_for_its_things_and_an_optional_slot_is_not_required() {
        let project = "use std\nkind plan : asset\n  has beneficiary person\n  has note text optional\n  has coverage one of a | b\nkind family-plan : plan\n  beneficiary me\n  coverage a\nentity me : person\naccount college : family-plan\n";
        let (_, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
        let (_, diags) = book(&format!("{project}account other : family-plan\n  beneficiary riley\n"));
        assert_eq!(diags.iter().map(|diagnostic| &*diagnostic.code).collect::<Vec<_>>(), ["unknown-entity"]);
    }
}
