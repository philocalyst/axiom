//! Slots: what the things of a kind have, how many of each, and what each takes.
//!
//! A kind declares slots with `has NAME RANGE [MULT] [by WEIGHT]`. The range says what a slot takes: things of some
//! kinds (`person | household`), one of some words (`one of self-only | family`), or a value of a type (`date`,
//! `USD`). The count says how many: one, `optional`, `some` or `many`. Its things fill the slots, and the model
//! checks a filling once, when it is made; nothing downstream asks again.
//!
//! # Why this layout
//!
//! Every slot of every kind is one record in one arena, and a kind's own are a run of it, so a kind owns no vector
//! and a lookup is a short walk up the kind's ancestors over a few records. The kinds and the words that ranges name
//! are runs of two more flat arenas, for the same reason. A slot that two kinds declare under one name is one slot
//! with one number, which is what the facts store things under: both of them take values of one type, and a kind
//! beneath one may take fewer, never more.

use axiom_core::{Arena, Diagnostic, Dim, Id, Loc, Map, Run, Set, SlotId, Sym, Tree};
use axiom_syntax::{Decl, DeclKind, ExprKind, File, Has, Name, Takes, Verb};

pub use axiom_syntax::Mult;

use crate::book::{Book, Commodity, Kind, Sort};
use crate::builtin;
use crate::collect::{Collected, Written};
use crate::declare::World;
use crate::errors::{Word, article, list, suggest};
use crate::law::Ty;
use crate::problem;
use crate::scope::Home;

/// What a slot takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    /// Things of these kinds, or of kinds beneath them.
    Kinds(Run<Id<Kind>>),
    /// One of these words.
    Words(Run<Sym>),
    /// A value of this type.
    Value(Ty),
}

/// A range, as the slices it holds: what [`Schema::view`] gives, and what a slot not yet in the schema is read as.
#[derive(Clone, Copy)]
pub enum View<'a> {
    Kinds(&'a [Id<Kind>]),
    Words(&'a [Sym]),
    Value(Ty),
}

impl View<'_> {
    /// `a person or a household`, ``one of `a` or `b` ``, `a date`: what a range takes, in words.
    pub fn describe(self, book: &Book) -> String {
        match self {
            View::Kinds(kinds) => {
                let named = kinds.iter().map(|&kind| article(book.name(book.kinds[kind].name)));
                named.collect::<Vec<_>>().join(" or ")
            }
            View::Words(words) => {
                let words: Vec<&str> = words.iter().map(|&word| book.name(word)).collect();
                format!("one of {}", list(&words))
            }
            View::Value(ty) => article(ty.word()),
        }
    }

    /// The range as a `has` line writes it.
    pub fn spell(self, book: &Book) -> String {
        match self {
            View::Kinds(kinds) => {
                kinds.iter().map(|&kind| book.name(book.kinds[kind].name)).collect::<Vec<_>>().join(" | ")
            }
            View::Words(words) => {
                format!("one of {}", words.iter().map(|&word| book.name(word)).collect::<Vec<_>>().join(" | "))
            }
            View::Value(Ty::Amount(Dim::Of(unit))) => book.name(book.commodities[unit].symbol).to_string(),
            View::Value(Ty::Amount(Dim::Per(top, bottom))) => {
                format!("{}/{}", book.name(book.commodities[top].symbol), book.name(book.commodities[bottom].symbol))
            }
            View::Value(ty) => ty.word().to_string(),
        }
    }
}

/// What weighs the values of a slot that holds several: `by share`, `by rent USD`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weight {
    pub name: Sym,
    /// The commodity a weight is an amount of, if it is one.
    pub unit: Option<Id<Commodity>>,
}

/// One slot of one kind, as that kind declares it.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub name: Sym,
    pub range: Range,
    pub mult: Mult,
    pub weight: Option<Weight>,
    /// The whole `has` line.
    pub loc: Loc,
}

const _: () = assert!(size_of::<Slot>() <= 64);

/// The words of the value types that a range may be.
const VALUE_TYPES: [(&str, Ty); 12] = [
    ("date", Ty::Day),
    ("amount", Ty::AMOUNT),
    ("number", Ty::Num),
    ("percent", Ty::Num),
    ("span", Ty::Span),
    ("text", Ty::Text),
    ("place", Ty::Place),
    ("kind", Ty::Kind),
    ("unit", Ty::Unit),
    ("bool", Ty::Bool),
    ("purpose", Ty::Purpose),
    ("asset", Ty::Asset),
];

/// The words that read a value off a thing in a law (`self.balance`), so a slot may not take them.
const FIELD_WORDS: [&str; 8] = ["balance", "basis", "owner", "kind", "age", "unit", "year", "month"];

/// Every slot of every kind, and the numbers of the slots.
#[derive(Default)]
pub struct Schema {
    slots: Arena<Slot>,
    kinds: Arena<Id<Kind>>,
    words: Arena<Sym>,
    numbers: Map<Sym, SlotId>,
    /// By number: the record that declared the slot first, and what its values are.
    first: Vec<(Id<Slot>, Ty)>,
    /// By number: the sorts of thing that have the slot, as a set of [`family`] bits.
    families: Vec<u8>,
}

/// The bit of a sort of thing, which a law's receiver is: `Ty::Place`, `Ty::Asset`, `Ty::Unit` or `Ty::Entity`.
fn family(receiver: Ty) -> u8 {
    match receiver {
        Ty::Place => 1,
        Ty::Asset => 2,
        Ty::Unit => 4,
        _ => 8,
    }
}

/// What a thing of a kind of this sort is, as a law's receiver.
pub(crate) fn receiver(sort: Sort) -> Ty {
    match sort {
        Sort::Place(_) => Ty::Place,
        Sort::Thing => Ty::Asset,
        Sort::Commodity => Ty::Unit,
        Sort::Entity => Ty::Entity,
        // What a contract is as a law's receiver is the flow of its occurrence (its laws say `self` of that).
        Sort::Contract => Ty::Flow,
    }
}

impl Schema {
    /// The number of the slot at `index` among those a kind declares: the language's own come first.
    fn slot_id(index: usize) -> SlotId {
        SlotId(builtin::COUNT + index as u32)
    }

    /// Where a slot a kind declares is among them.
    fn index(slot: SlotId) -> usize {
        (slot.0 - builtin::COUNT) as usize
    }

    /// A schema of slots that take one value each, numbered in the order given and declared by no kind: the slots of
    /// a book that is built by hand, where nothing is written to declare them. Every sort of thing has them.
    pub fn of_values(values: impl IntoIterator<Item = (Sym, Ty)>) -> Schema {
        let mut schema = Schema::default();
        for (name, ty) in values {
            let range = Range::Value(ty);
            let slot = Slot { name, range, mult: Mult::Optional, weight: None, loc: Loc::default() };
            let id = schema.slots.push(slot);
            let number = Schema::slot_id(schema.first.len());
            assert!(schema.numbers.insert(name, number).is_none(), "a slot is declared once");
            schema.first.push((id, ty));
            schema.families.push(u8::MAX);
        }
        schema
    }

    /// The slots `kind` declares itself.
    pub fn own(&self, kind: &Kind) -> &[Slot] {
        &self.slots[kind.slots]
    }

    /// The slot `name` of things of `kind`: the nearest declaration, which is the narrowest.
    pub fn find(&self, kinds: &Tree<Kind>, kind: Id<Kind>, name: Sym) -> Option<&Slot> {
        kinds.lineage(kind).find_map(|above| self.own(&kinds[above]).iter().find(|slot| slot.name == name))
    }

    /// Every slot things of `kind` have, nearest declaration first, each name once.
    pub fn effective<'a>(&'a self, kinds: &'a Tree<Kind>, kind: Id<Kind>) -> impl Iterator<Item = &'a Slot> {
        let mut seen = Set::default();
        kinds.lineage(kind).flat_map(move |above| self.own(&kinds[above])).filter(move |slot| seen.insert(slot.name))
    }

    /// The slots things of `kind` have that take entities, in the order they are declared: the outermost kind's first,
    /// each name once, with the narrowest range among those that repeat it. An address lists what fills them in this
    /// order, so that it does not change when a kind beneath narrows a slot.
    pub(crate) fn entity_slots(&self, kinds: &Tree<Kind>, kind: Id<Kind>) -> Vec<(SlotId, Slot)> {
        let mut lineage: Vec<Id<Kind>> = kinds.lineage(kind).collect();
        lineage.reverse();
        let mut seen = Set::default();
        let names = lineage.iter().flat_map(|&above| self.own(&kinds[above])).map(|slot| slot.name);
        let names: Vec<Sym> = names.filter(|&name| seen.insert(name)).collect();
        let takes_entities = |slot: &Slot| match slot.range {
            Range::Kinds(run) => receiver(kinds[self.kinds_of(run)[0]].sort) == Ty::Entity,
            Range::Words(_) | Range::Value(_) => false,
        };
        let found = names.into_iter().filter_map(|name| self.find(kinds, kind, name));
        found
            .filter(|slot| takes_entities(slot))
            .map(|&slot| (self.number(slot.name).expect("numbered"), slot))
            .collect()
    }

    /// The number of the slot named `name`, if any kind declares it.
    pub fn number(&self, name: Sym) -> Option<SlotId> {
        self.numbers.get(&name).copied()
    }

    /// What the values of the numbered slot are.
    pub fn ty(&self, slot: SlotId) -> Ty {
        self.first[Schema::index(slot)].1
    }

    /// The kinds a range names.
    pub fn kinds_of(&self, range: Run<Id<Kind>>) -> &[Id<Kind>] {
        &self.kinds[range]
    }

    /// The words a range names.
    pub fn words_of(&self, range: Run<Sym>) -> &[Sym] {
        &self.words[range]
    }

    /// The type of what a law reads off a thing of sort `receiver` by `.name`, if some kind of that sort has such a
    /// slot.
    pub fn field(&self, receiver: Ty, name: Sym) -> Option<Ty> {
        let number = self.number(name)?;
        (self.families[Schema::index(number)] & family(receiver) != 0).then(|| self.ty(number))
    }

    /// The names of the slots a law may read off a thing of sort `receiver`.
    pub fn fields(&self, receiver: Ty) -> impl Iterator<Item = Sym> + '_ {
        let bit = family(receiver);
        let all = self.numbers.iter();
        all.filter(move |&(_, &number)| self.families[Schema::index(number)] & bit != 0).map(|(&name, _)| name)
    }

    /// What a range holds, as slices.
    pub fn view(&self, range: Range) -> View<'_> {
        match range {
            Range::Kinds(run) => View::Kinds(&self.kinds[run]),
            Range::Words(run) => View::Words(&self.words[run]),
            Range::Value(ty) => View::Value(ty),
        }
    }

    /// Records a slot of a kind of a sort, numbering it if its name is new.
    fn add(&mut self, draft: &Draft, receiver: Ty) {
        let range = match &draft.range {
            Drawn::Kinds(kinds) => Range::Kinds(self.kinds.extend(kinds.iter().copied())),
            Drawn::Words(words) => Range::Words(self.words.extend(words.iter().copied())),
            Drawn::Value(ty) => Range::Value(*ty),
        };
        let slot = Slot { name: draft.name, range, mult: draft.mult, weight: draft.weight, loc: draft.loc };
        let id = self.slots.push(slot);
        let number = *self.numbers.entry(draft.name).or_insert_with(|| {
            self.first.push((id, draft.ty));
            self.families.push(0);
            Schema::slot_id(self.first.len() - 1)
        });
        self.families[Schema::index(number)] |= family(receiver);
    }
}

// ─── Declaring ──────────────────────────────────────────────────────────────

/// A slot as written and resolved, not yet in the schema.
struct Draft {
    name: Sym,
    range: Drawn,
    mult: Mult,
    weight: Option<Weight>,
    loc: Loc,
    /// What its values are.
    ty: Ty,
}

/// What a range resolved to.
enum Drawn {
    Kinds(Vec<Id<Kind>>),
    Words(Vec<Sym>),
    Value(Ty),
}

impl Drawn {
    fn view(&self) -> View<'_> {
        match self {
            Drawn::Kinds(kinds) => View::Kinds(kinds),
            Drawn::Words(words) => View::Words(words),
            Drawn::Value(ty) => View::Value(*ty),
        }
    }
}

/// Where a slot is written: the home of the declaration and its file, and everything written, to say what a slot that
/// takes too much could take instead.
struct At<'c, 'a, 's> {
    home: Home,
    file: &'a File<'s>,
    collected: &'c Collected<'a, 's>,
}

/// Reads the slots every kind declares. A kind's are declared after its ancestors', so what a kind repeats can be
/// checked against what it narrows.
pub(crate) fn declare<'a, 's>(world: &mut World<'s>, collected: &Collected<'a, 's>, diags: &mut Vec<Diagnostic>) {
    let declarations = declarations(world, collected);
    for group in declarations.chunk_by(|a, b| a.0 == b.0) {
        let kind = group[0].0;
        let mut own: Vec<Draft> = Vec::new();
        for (_, written) in group {
            let at = At { home: written.home(), file: written.file(), collected };
            for has in &at.file[written.node.slots] {
                match read(world, &at, has, diags).and_then(|draft| admit(world, kind, &own, draft)) {
                    Ok(draft) => own.push(draft),
                    Err(diagnostic) => diags.push(diagnostic),
                }
            }
        }
        let receiver = receiver(world.book.kinds[kind].sort);
        let before = world.book.schema.slots.len();
        for draft in &own {
            world.book.schema.add(draft, receiver);
        }
        world.book.kinds[kind].slots = Run::of(before..world.book.schema.slots.len());
    }
    for written in
        collected.decls.iter().filter(|written| !matches!(written.node.what, DeclKind::Kind | DeclKind::Purpose))
    {
        let noun = match written.node.what {
            DeclKind::Account => "account",
            DeclKind::Entity => "entity",
            DeclKind::Asset => "asset",
            _ => "commodity",
        };
        diags.extend(written.file()[written.node.slots].iter().map(|has| problem::slot_on_a_thing(has.loc, noun)));
    }
}

/// The declarations that give kinds slots, in the order of the kinds: the first of each kind, and every one of a
/// root, which a system or a project may open again to give it more.
fn declarations<'a, 's>(
    world: &World<'s>,
    collected: &Collected<'a, 's>,
) -> Vec<(Id<Kind>, Written<'a, 's, Decl<'s>>)> {
    let mut seen = Set::default();
    let mut found = Vec::new();
    for written in collected.decls_of(DeclKind::Kind) {
        let word = Word::of(written.file(), written.node.name.0);
        let Ok(kind) = world.kind(written.home(), word) else { continue };
        let root = world.book.kinds[kind].loc.is_none();
        if (root && written.node.kind.is_none()) || (!root && seen.insert(kind)) {
            found.push((kind, *written));
        }
    }
    found.sort_by_key(|&(kind, _)| kind);
    found
}

/// One `has` line, resolved.
fn read<'s>(
    world: &mut World<'s>,
    at: &At<'_, '_, 's>,
    has: &Has<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Result<Draft, Diagnostic> {
    let name = Word::of(at.file, has.name.0);
    let (range, ty) = range(world, at, has, diags)?;
    let mult = has.mult;
    let weight = match has.weight {
        Some(written) if matches!(mult, Mult::Some | Mult::Many) => {
            let unit = written.unit.map(|unit| world.commodity_of(Word::of(at.file, unit.0))).transpose()?;
            Some(Weight { name: world.book.names.intern(written.name.0), unit })
        }
        Some(_) => return Err(problem::weighted_one(name, has.loc)),
        None => None,
    };
    Ok(Draft { name: world.book.names.intern(name.text), range, mult, weight, loc: has.loc, ty })
}

/// What a `has` line takes, and what its values are.
fn range<'s>(
    world: &mut World<'s>,
    at: &At<'_, '_, 's>,
    has: &Has<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(Drawn, Ty), Diagnostic> {
    match has.takes {
        Takes::Unit(unit) => {
            let ty = Ty::Amount(unit_dim(world, at.file, unit)?);
            Ok((Drawn::Value(ty), ty))
        }
        Takes::Words(words) => {
            let words: Vec<Sym> = at.file[words].iter().map(|word| world.book.names.intern(word.0)).collect();
            let mut seen = Set::default();
            Ok((Drawn::Words(words.into_iter().filter(|&word| seen.insert(word)).collect()), Ty::Name))
        }
        Takes::Names(names) => {
            let names = &at.file[names];
            let value_type = |name: &Name| VALUE_TYPES.iter().find(|(word, _)| *word == name.0).map(|&(_, ty)| ty);
            match names {
                [one] if value_type(one).is_some() => {
                    let ty = value_type(one).expect("checked");
                    Ok((Drawn::Value(ty), ty))
                }
                [one] if one.0 == "name" => {
                    diags.push(untyped(world, at, has, Word::of(at.file, one.0)));
                    Ok((Drawn::Words(Vec::new()), Ty::Name))
                }
                names => kinds_range(world, at, has, names, diags),
            }
        }
    }
}

/// The kinds a range names, all of one sort of thing. `entity` is said to take too much, and still declares the slot
/// as widely as it said, so that what fills it adds no errors of its own.
fn kinds_range<'s>(
    world: &World<'s>,
    at: &At<'_, '_, 's>,
    has: &Has<'s>,
    names: &[Name<'s>],
    diags: &mut Vec<Diagnostic>,
) -> Result<(Drawn, Ty), Diagnostic> {
    let mut kinds: Vec<Id<Kind>> = Vec::new();
    for name in names {
        let word = Word::of(at.file, name.0);
        let kind = match world.seek_kind(at.home, word)? {
            Some(kind) => kind,
            None if names.len() == 1 => return Err(not_a_type_or_kind(world, word)),
            None => world.kind(at.home, word)?,
        };
        if kind == world.book.roots.kinds.entity {
            diags.push(untyped(world, at, has, word));
        }
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    let ty = receiver(world.book.kinds[kinds[0]].sort);
    match kinds.iter().position(|&kind| receiver(world.book.kinds[kind].sort) != ty) {
        Some(odd) => Err(problem::mixed_sorts(
            Word::of(at.file, names[odd].0),
            world.book.name(world.book.kinds[kinds[odd]].name),
        )),
        None => Ok((Drawn::Kinds(kinds), ty)),
    }
}

/// `x` is neither a type word nor a kind: the closest of either is the fix.
fn not_a_type_or_kind(world: &World<'_>, word: Word) -> Diagnostic {
    let kinds = world.book.kinds.values().map(|kind| world.book.name(kind.name));
    let types: Vec<&str> = VALUE_TYPES.iter().map(|&(word, _)| word).collect();
    let error = Diagnostic::error("has-type", format!("`{}` is not a type or a kind", word.text))
        .label(word.loc, "a slot takes a kind, `one of` some words, or a type")
        .note(format!("the types are {}", list(&types)));
    suggest(error, word.loc, word.text, types.iter().copied().chain(kinds))
}

/// `entity` or `name` as a range accepts any entity or any word: what the things of the book already say it takes is
/// the fix, when they say anything.
fn untyped<'s>(world: &World<'s>, at: &At<'_, '_, 's>, has: &Has<'s>, wide: Word) -> Diagnostic {
    let written = values_written(at.collected, has.name.0);
    let proposal = match wide.text {
        "name" => (!written.is_empty()).then(|| format!("one of {}", written.join(" | "))),
        _ => {
            let kinds: Vec<&str> = written
                .iter()
                .filter_map(|&value| world.seek_entity(at.home, Word { text: value, loc: wide.loc }).ok().flatten())
                .map(|entity| world.book.name(world.book.kinds[world.book.entities[entity].kind].name))
                .collect();
            let mut seen = Set::default();
            let distinct: Vec<&str> = kinds.into_iter().filter(|&kind| seen.insert(kind)).collect();
            (!distinct.is_empty()).then(|| distinct.join(" | "))
        }
    };
    problem::untyped_slot(Word::of(at.file, has.name.0), wide, proposal.as_deref())
}

/// `USD` or `USD/MI`: a quantity counted in a commodity, or in a rate between two.
fn unit_dim<'s>(world: &World<'s>, file: &File<'s>, written: Name<'s>) -> Result<Dim<Id<Commodity>>, Diagnostic> {
    let loc = file.loc(written.0);
    let unit = |text: &str| world.commodity_of(Word { text, loc }).map(Dim::Of);
    let Some((top, bottom)) = written.0.split_once('/') else { return unit(written.0) };
    match unit(top)?.div(unit(bottom)?) {
        Some(dim) if dim != Dim::Number && dim != Dim::Any => Ok(dim),
        _ => Err(Diagnostic::error("has-unit", "this is not a supported amount unit")
            .label(loc, "write one commodity or a rate such as `USD/MI`")),
    }
}

/// Whether a slot may be declared: not a built-in's name, not twice in a kind, of the type everyone else gives its
/// name, and no wider than the one it repeats from above.
fn admit(world: &World<'_>, kind: Id<Kind>, own: &[Draft], draft: Draft) -> Result<Draft, Diagnostic> {
    let book = &world.book;
    let name = book.name(draft.name);
    if crate::props::is_builtin_line(name) || FIELD_WORDS.contains(&name) {
        return Err(problem::built_in_property(Word { text: name, loc: draft.loc }));
    }
    if let Some(earlier) = own.iter().find(|earlier| earlier.name == draft.name) {
        return Err(problem::slot_twice(name, draft.loc, earlier.loc));
    }
    let schema = &book.schema;
    if let Some(number) = schema.number(draft.name)
        && schema.ty(number) != draft.ty
    {
        let first = &schema.slots[schema.first[Schema::index(number)].0];
        let (now, then) = (draft.range.view().describe(book), schema.view(first.range).describe(book));
        return Err(problem::slot_type(name, &now, &then, draft.loc, first.loc));
    }
    let above = book.kinds.parent(kind).and_then(|parent| schema.find(&book.kinds, parent, draft.name));
    match above.and_then(|above| widens(book, &draft, above).map(|how| (above, how))) {
        Some((above, how)) => {
            Err(problem::slot_widening(name, how, &spelled(book, &draft, above), draft.loc, above.loc))
        }
        None => Ok(draft),
    }
}

/// How a slot takes more than the slot it repeats, if it does.
fn widens(book: &Book, draft: &Draft, above: &Slot) -> Option<problem::Widening> {
    let schema = &book.schema;
    let narrower_range = match (&draft.range, schema.view(above.range)) {
        (Drawn::Kinds(now), View::Kinds(over)) => {
            now.iter().all(|&kind| over.iter().any(|&over| book.kinds.covers(over, kind)))
        }
        (Drawn::Words(now), View::Words(over)) => now.iter().all(|word| over.contains(word)),
        (Drawn::Value(now), View::Value(over)) => *now == over,
        _ => false,
    };
    let (now, over) = (bounds(draft.mult), bounds(above.mult));
    if !narrower_range {
        Some(problem::Widening::Range)
    } else if now.0 < over.0 || now.1 > over.1 {
        Some(problem::Widening::Count)
    } else if above.weight.is_some() && draft.weight != above.weight {
        Some(problem::Widening::Weight)
    } else {
        None
    }
}

/// The fewest and the most values a count takes.
fn bounds(mult: Mult) -> (u8, u8) {
    match mult {
        Mult::One => (1, 1),
        Mult::Optional => (0, 1),
        Mult::Some => (1, u8::MAX),
        Mult::Many => (0, u8::MAX),
    }
}

/// The line that mends a widening: what the narrower slot says, as far as it does not widen the slot above, and
/// the rest as the slot above says it.
fn spelled(book: &Book, draft: &Draft, above: &Slot) -> String {
    let (now, over) = (bounds(draft.mult), bounds(above.mult));
    let mult = if now.0 >= over.0 && now.1 <= over.1 { draft.mult } else { above.mult };
    let count = match mult {
        Mult::One => "",
        Mult::Optional => " optional",
        Mult::Some => " some",
        Mult::Many => " many",
    };
    let weight = above.weight.map_or(String::new(), |weight| {
        let unit = weight.unit.map_or(String::new(), |unit| format!(" {}", book.name(book.commodities[unit].symbol)));
        format!(" by {}{unit}", book.name(weight.name))
    });
    let range = match (&draft.range, book.schema.view(above.range)) {
        (Drawn::Kinds(now), View::Kinds(over)) => {
            let fits: Vec<Id<Kind>> =
                now.iter().copied().filter(|&kind| over.iter().any(|&over| book.kinds.covers(over, kind))).collect();
            View::Kinds(if fits.is_empty() { over } else { &fits }).spell(book)
        }
        (Drawn::Words(now), View::Words(over)) => {
            let fits: Vec<Sym> = now.iter().copied().filter(|word| over.contains(word)).collect();
            View::Words(if fits.is_empty() { over } else { &fits }).spell(book)
        }
        (_, over) => over.spell(book),
    };
    format!("has {} {range}{count}{weight}", book.name(draft.name))
}

/// The names written after `name` in every declaration's line of that name and every `now` statement, in the order
/// written: the values the slot's things already have.
pub(crate) fn values_written<'s>(collected: &Collected<'_, 's>, name: &str) -> Vec<&'s str> {
    let mut found: Vec<&'s str> = Vec::new();
    let mut note = |file: &File<'s>, args: axiom_syntax::Many<axiom_syntax::ExprId>| {
        for &arg in &file[args] {
            if let ExprKind::Name(value) = file.exprs[arg].kind
                && !found.contains(&value.0)
            {
                found.push(value.0);
            }
        }
    };
    for written in &collected.decls {
        let file = written.file();
        file[written.node.props].iter().filter(|line| line.name.0 == name).for_each(|line| note(file, line.args));
    }
    for written in &collected.statements {
        if let Verb::Now(axiom_syntax::Change::Property(line)) = &written.node.verb
            && line.name.0 == name
        {
            note(written.file(), line.args);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use axiom_core::{FileId, Interner};
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
commodity MI : measure
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

    fn sym(book: &Book, name: &str) -> Sym {
        book.names.get(name).unwrap_or_else(|| panic!("`{name}` is interned"))
    }

    fn slot<'b>(book: &'b Book, kind: &str, name: &str) -> &'b Slot {
        let kind = book.kind(kind).expect("the kind");
        book.schema.find(&book.kinds, kind, sym(book, name)).unwrap_or_else(|| panic!("`{kind:?}` has `{name}`"))
    }

    fn takes(book: &Book, kind: &str, name: &str) -> String {
        book.schema.view(slot(book, kind, name).range).spell(book)
    }

    #[test]
    fn a_kind_has_the_slots_of_its_ancestors_and_its_own_and_the_nearest_declaration_wins() {
        let project = "\
use std
kind plan : asset
  has holder person | household
  has beneficiary person optional
kind 529 : plan
  has holder person
  has note text many
";
        let (book, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
        let names = |kind: &str| -> Vec<&str> {
            let kind = book.kind(kind).unwrap();
            book.schema.effective(&book.kinds, kind).map(|slot| book.name(slot.name)).collect()
        };
        assert_eq!(names("plan"), ["holder", "beneficiary"]);
        assert_eq!(names("529"), ["holder", "note", "beneficiary"], "nearest first, each name once");
        assert_eq!(takes(&book, "529", "holder"), "person", "the kind that narrows it is the one that is found");
        assert_eq!(takes(&book, "plan", "holder"), "person | household");
        assert_eq!(takes(&book, "529", "beneficiary"), "person", "and what it does not repeat comes from above");
        assert_eq!(book.schema.own(&book.kinds[book.kind("529").unwrap()]).len(), 2);
    }

    #[test]
    fn a_range_is_kinds_words_or_a_value_and_says_what_it_takes_in_words() {
        let project = "\
use std
kind thing-with-slots : asset
  has holder person | household
  has coverage one of self-only | family | self-only
  has born date
  has price USD/MI
  has fee USD many
  has years number
";
        let (book, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
        let describe = |name: &str| slot(&book, "thing-with-slots", name).range;
        let describe = |name: &str| book.schema.view(describe(name)).describe(&book);
        assert_eq!(describe("holder"), "a person or a household");
        assert_eq!(describe("coverage"), "one of `self-only` or `family`", "a repeated word is one word");
        assert_eq!(describe("born"), "a date");
        assert_eq!(describe("price"), "an amount");
        let price = slot(&book, "thing-with-slots", "price");
        let usd = book.commodity("USD").unwrap();
        let mi = book.commodity("MI").unwrap();
        assert_eq!(price.range, Range::Value(Ty::Amount(Dim::Per(usd, mi))));
        assert_eq!(slot(&book, "thing-with-slots", "fee").mult, Mult::Many);
        assert_eq!(slot(&book, "thing-with-slots", "years").mult, Mult::One, "no word means exactly one");
        assert_eq!(takes(&book, "thing-with-slots", "price"), "USD/MI");
    }

    #[test]
    fn a_slot_has_one_number_wherever_it_is_declared_and_one_type() {
        let project = "\
use std
kind plan : asset
  has holder person
kind card : debt
  has holder household optional
";
        let (book, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
        let number = book.schema.number(sym(&book, "holder")).expect("numbered");
        assert_eq!(book.schema.ty(number), Ty::Entity);
        assert_eq!(book.schema.field(Ty::Place, sym(&book, "holder")), Some(Ty::Entity), "both are accounts");
        assert_eq!(book.schema.field(Ty::Unit, sym(&book, "holder")), None, "no commodity kind has it");

        let (_, diags) = self::book("use std\nkind a : asset\n  has size date\nkind b : debt\n  has size amount\n");
        let error = only(&diags, "property-type");
        assert_eq!(error.message, "`size` takes an amount here, but a date elsewhere");
        assert_eq!(error.labels.len(), 2);
    }

    #[test]
    fn a_kind_beneath_may_narrow_a_slot_by_its_range_or_its_count() {
        let project = "\
use std
kind plan : asset
  has holder person | household optional
  has dependents person many
  has tags one of a | b | c many
kind narrow : plan
  has holder student
  has dependents person some
  has tags one of c | a optional
";
        let (_, diags) = book(project);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_kind_beneath_may_not_widen_a_slot_and_is_told_the_line_that_would_not() {
        let widen = |has: &str, above: &str| {
            let project = format!("use std\nkind plan : asset\n  {above}\nkind wide : plan\n  {has}\n");
            let (diags, project) = {
                let (_, diags) = book(Box::leak(project.clone().into_boxed_str()));
                (diags, project)
            };
            (only(&diags, "slot-widening").clone(), project)
        };
        let (error, project) = widen("has holder person | household", "has holder person");
        assert_eq!(error.message, "`holder` is wider here than the slot it repeats");
        assert_eq!(error.labels[0].text, "takes things the slot above does not");
        let (loc, text) = error.help[0].edit.clone().expect("an edit");
        assert_eq!((&project[loc.range()], &*text), ("has holder person | household", "has holder person"));

        let (error, _) = widen("has holder person many", "has holder person optional");
        assert_eq!(error.labels[0].text, "takes more values than the slot above");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "has holder person optional");

        let (error, _) = widen("has holder person | household optional", "has holder person optional");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "has holder person optional", "what narrows is kept");

        let (error, _) = widen("has tags one of a | z many", "has tags one of a | b many");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "has tags one of a many", "the words that fit are kept");
        let (error, _) = widen("has holder person | org", "has holder person | household optional");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "has holder person", "and the kinds that fit");
        let (error, _) = widen("has note text some", "has note text many by share");
        assert_eq!(error.labels[0].text, "weighs its values differently from the slot above");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "has note text some by share");
    }

    #[test]
    fn a_slot_that_takes_any_entity_or_any_word_is_refused_and_told_what_the_book_fills_it_with() {
        let project = "\
use std
kind plan : asset
  has sponsor entity
  has coverage name
account a : plan
  sponsor acme
  coverage family
account b : plan
  coverage self-only
  coverage family
entity acme : employer
entity riley : student
account c : plan
  sponsor riley
";
        let (_, diags) = book(project);
        let sponsor = diags.iter().find(|diagnostic| diagnostic.message.contains("sponsor")).expect("sponsor");
        assert_eq!(sponsor.code, "untyped-slot");
        assert_eq!(sponsor.message, "`sponsor` takes any entity");
        assert_eq!(sponsor.help[0].edit.as_ref().unwrap().1, "employer | student", "the kinds of what is written");
        let coverage = diags.iter().find(|diagnostic| diagnostic.message.contains("coverage")).expect("coverage");
        assert_eq!(coverage.message, "`coverage` takes any word");
        assert_eq!(coverage.help[0].edit.as_ref().unwrap().1, "one of family | self-only");

        let (_, diags) = book("use std\nkind plan : asset\n  has sponsor entity\n");
        let error = only(&diags, "untyped-slot");
        assert!(error.help[0].edit.is_none() && error.help[0].text.contains("has sponsor person | household"));
    }

    #[test]
    fn a_range_that_is_not_a_type_a_kind_or_one_sort_is_said_with_the_closest_word() {
        let (_, diags) = book("use std\nkind plan : asset\n  has born dat\n");
        let error = only(&diags, "has-type");
        assert_eq!(error.message, "`dat` is not a type or a kind");
        assert_eq!(error.help[0].edit.as_ref().unwrap().1, "date");

        let (_, diags) = book("use std\nkind plan : asset\n  has holder person | bank\n");
        let error = only(&diags, "slot-range-sorts");
        assert_eq!(error.message, "`bank` is not the sort of thing the other kinds of this range are");

        let (_, diags) = book("use std\nkind plan : asset\n  has holder perzon | household\n");
        assert_eq!(only(&diags, "unknown-kind").help[0].edit.as_ref().unwrap().1, "person");
    }

    #[test]
    fn a_weight_needs_a_slot_that_holds_several_and_a_commodity_that_exists() {
        let (book, diags) = book(
            "use std\nkind plan : asset\n  has holders person some by share\n  has rents person many by rent USD\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let rent = slot(&book, "plan", "rents").weight.expect("weighed");
        assert_eq!((book.name(rent.name), rent.unit), ("rent", book.commodity("USD")));
        assert_eq!(slot(&book, "plan", "holders").weight.map(|weight| weight.unit), Some(None));

        let (_, diags) = self::book("use std\nkind plan : asset\n  has holder person by share\n");
        assert_eq!(only(&diags, "weight-on-one").message, "`holder` takes one value, so there is nothing to weigh");
        let (_, diags) = self::book("use std\nkind plan : asset\n  has holders person some by rent EUR\n");
        only(&diags, "unknown-commodity");
    }

    #[test]
    fn a_slot_is_declared_once_by_a_kind_and_never_under_a_thing_or_with_a_built_in_name() {
        let (_, diags) = book("use std\nkind plan : asset\n  has holder person\n  has holder household\n");
        let error = only(&diags, "duplicate-property-declaration");
        assert_eq!(error.message, "property `holder` is declared twice on this kind");
        assert_eq!(error.labels.len(), 2);

        let (_, diags) = book("use std\naccount checking : bank\n  has note text\n");
        assert_eq!(only(&diags, "unknown-property").message, "`has` is not a property of an account");

        let (_, diags) = book("use std\nkind plan : asset\n  has opened date\n");
        only(&diags, "reserved-property");
        let (_, diags) = book("use std\nkind plan : asset\n  has balance number\n");
        only(&diags, "reserved-property");
    }

    #[test]
    fn a_system_or_a_project_may_open_a_root_kind_to_give_it_slots() {
        let std = "system std\nkind entity\n  has favourite text optional\nkind person : entity\n";
        let project = "use std\nkind entity\n  has pet text optional\nentity me : person\n";
        let sources = [
            {
                let (file, _) = parse(FileId(0), std, Folder::default());
                Source { path: "std.ax", file, embedded: true }
            },
            {
                let (file, _) = parse(FileId(1), project, Folder::default());
                Source { path: "axiom.ax", file, embedded: false }
            },
        ];
        let (book, diags) = build(&sources);
        assert!(diags.is_empty(), "{diags:?}");
        let names: Vec<&str> =
            book.schema.effective(&book.kinds, book.kind("person").unwrap()).map(|slot| book.name(slot.name)).collect();
        assert_eq!(names, ["favourite", "pet"], "every opening of the root adds to the one run");
        let _ = Interner::default();
    }
}
