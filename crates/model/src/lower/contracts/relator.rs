//! What a contract of a kind is: the slots it fills, and the legs its kind writes once.
//!
//! `kind employment : contract` says what two sides are to each other (`has employee person`, `has employer org`) and
//! what always follows from it, in `also` lines whose ends are those roles (`also employer -> irs 7.65% of amount
//! #payroll-tax`). A contract of the kind (`contract alex-pay : employment`) says who fills each role (`employer
//! acme`). Written once, the kind is true from every side, because **which legs a book has, and where their ends stand,
//! is decided when the contract is lowered**: the owners of a book are known by then and nothing the fold does can add
//! one, so no fold has to know a relator exists.
//!
//! A role filled by the contract's owner (or a member of it) stands at the owner's position, the account the schedule
//! pays from or into; a role filled by anyone else stands outside, at the place its filler has. A leg with both ends
//! outside does not touch the book and is dropped. The household that pays `alex-pay` therefore has no employer half of
//! the payroll tax, and the employer, whose book it is, has it, from the one line. What this lane does not build is a
//! role whose position is not the schedule's holding (a plan's account): that is a part of the relator, and says so
//! with `relator-position`.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Loc, Sym};
use axiom_syntax::{self as ast, DeclKind};

use super::WrittenContract;
use crate::book::{Book, Contract, Entity, Filler, Kind, Place, Role, Sort};
use crate::collect::{Collected, Written};
use crate::declare::World;
use crate::errors::{Reported, Word, article, list};
use crate::law::{Law, Owner, Ty};
use crate::laws::{Placement, Positions, compile_also, flow_ends};
use crate::slots::{Mult, Range, Slot, View};

/// The kind a contract is `: KIND` of and who fills its slots, or nothing, after what is wrong with it has been said.
pub(super) fn relation<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
) -> Option<(Id<Kind>, Box<[Filler]>)> {
    let (file, node) = (written.file(), written.node);
    let named = Word::of(file, node.kind?.0);
    let kind = world.kind(written.home(), named).or_report(world)?;
    if world.book.kinds[kind].sort != Sort::Contract {
        world.diags.push(not_a_contract_kind(&world.book, named, kind));
        return None;
    }
    let slots: Vec<Slot> = world.book.schema.effective(&world.book.kinds, kind).copied().collect();
    let fillers = fillers(world, written, kind, &slots)?;
    unfilled(&world.book, written, kind, &slots, &fillers, &mut world.diags)?;
    Some((kind, fillers.into()))
}

/// Who fills what: each line of the contract that names a slot of its kind, checked once.
fn fillers<'a, 's>(
    world: &mut World<'s>,
    written: WrittenContract<'a, 's>,
    kind: Id<Kind>,
    slots: &[Slot],
) -> Option<Vec<Filler>> {
    let (file, home) = (written.file(), written.home());
    let mut found: Vec<Filler> = Vec::new();
    let errors = world.diags.len();
    for fill in &file[written.node.fills] {
        let slot_word = Word::of(file, fill.slot.0);
        let slot = world.book.names.get(slot_word.text).and_then(|name| slots.iter().find(|slot| slot.name == name));
        let Some(slot) = slot else {
            world.diags.push(unknown_slot(&world.book, slot_word, kind, slots));
            continue;
        };
        if let Some(first) = found.iter().find(|filled| filled.slot == slot.name).filter(|_| !many(slot)) {
            world.diags.push(filled_twice(&world.book, slot, slot_word, first.loc));
            continue;
        }
        let Some(entity) = world.entity(home, Word::of(file, fill.filler.0)).or_report(world) else {
            continue;
        };
        match fits(world, slot, entity) {
            Ok(()) => found.push(Filler { slot: slot.name, entity, loc: fill.loc }),
            Err(problem) => world.diags.push(problem.say(world, slot, Word::of(file, fill.filler.0))),
        }
    }
    (world.diags.len() == errors).then_some(found)
}

fn many(slot: &Slot) -> bool {
    matches!(slot.mult, Mult::Some | Mult::Many)
}

/// Why an entity does not fill a slot.
enum Misfit {
    /// The slot takes a word or a value, which no entity is.
    NotEntities,
    /// The slot takes entities of other kinds.
    Kind(Id<Kind>),
}

impl Misfit {
    fn say(self, world: &World<'_>, slot: &Slot, word: Word) -> Diagnostic {
        let book = &world.book;
        let name = book.name(slot.name);
        let describe = book.schema.view(slot.range).describe(book);
        match self {
            Misfit::NotEntities => {
                Diagnostic::error("relator-slot-value", format!("`{name}` takes {describe}, not an entity"))
                    .label(word.loc, "a contract fills a slot with an entity")
                    .context(slot.loc, "the slot is declared here")
                    .help("write the value on the kind, or give the slot a kind of entity: `has employer org`")
            }
            Misfit::Kind(kind) => {
                let found = article(book.name(book.kinds[kind].name));
                Diagnostic::error("relator-slot-kind", format!("`{}` cannot fill `{name}`", word.text))
                    .label(word.loc, format!("this is {found}, and `{name}` takes {describe}"))
                    .context(slot.loc, "the slot is declared here")
            }
        }
    }
}

/// Whether `entity` is of a kind the slot takes.
fn fits(world: &World<'_>, slot: &Slot, entity: Id<Entity>) -> Result<(), Misfit> {
    let kind = world.book.entities[entity].kind;
    match world.book.schema.view(slot.range) {
        View::Kinds(takes) if takes.iter().any(|&takes| world.book.kinds.covers(takes, kind)) => Ok(()),
        View::Kinds(_) => Err(Misfit::Kind(kind)),
        View::Words(_) | View::Value(_) => Err(Misfit::NotEntities),
    }
}

/// Every slot of entities that takes one and was not given one is said, each with the line to write.
fn unfilled(
    book: &Book,
    written: WrittenContract<'_, '_>,
    kind: Id<Kind>,
    slots: &[Slot],
    fillers: &[Filler],
    diags: &mut Vec<Diagnostic>,
) -> Option<()> {
    let errors = diags.len();
    let needs = |slot: &&Slot| matches!(slot.mult, Mult::One | Mult::Some) && matches!(slot.range, Range::Kinds(_));
    for slot in slots.iter().filter(needs).filter(|slot| !fillers.iter().any(|filled| filled.slot == slot.name)) {
        diags.push(missing_slot(book, written, kind, slot));
    }
    (diags.len() == errors).then_some(())
}

fn not_a_contract_kind(book: &Book, word: Word, kind: Id<Kind>) -> Diagnostic {
    let found = article(book.name(book.kinds[kind].name));
    let kinds: Vec<&str> = book
        .kinds
        .iter()
        .filter(|(id, own)| own.sort == Sort::Contract && *id != book.roots.kinds.contract)
        .map(|(_, own)| book.name(own.name))
        .collect();
    let help = match kinds.as_slice() {
        [] => "declare one: `kind employment : contract`".to_string(),
        known => format!("the kinds of contract are {}", list(known)),
    };
    Diagnostic::error("relator-kind-sort", format!("`{}` is not a kind of contract", word.text))
        .label(word.loc, format!("this is {found}, and a contract is not one"))
        .note("a contract's kind says what its two sides are to each other, and what always follows from it")
        .help(help)
}

fn unknown_slot(book: &Book, word: Word, kind: Id<Kind>, slots: &[Slot]) -> Diagnostic {
    let kind_name = book.name(book.kinds[kind].name);
    let names: Vec<&str> = slots.iter().map(|slot| book.name(slot.name)).collect();
    let mut diagnostic =
        Diagnostic::error("relator-slot-unknown", format!("`{kind_name}` has no slot `{}`", word.text))
            .label(word.loc, format!("this is not one of the roles of {}", article(kind_name)));
    diagnostic = match names.as_slice() {
        [] => diagnostic
            .note(format!("`{kind_name}` declares no slots"))
            .help("declare one on the kind: `has employer org`"),
        names => diagnostic.note(format!("its slots are {}", list(names))),
    };
    match closest(word.text, names.iter().copied()) {
        Some(near) => diagnostic.fix(format!("did you mean `{near}`?"), word.loc, near),
        None => diagnostic,
    }
}

fn filled_twice(book: &Book, slot: &Slot, word: Word, first: Loc) -> Diagnostic {
    let takes = book.schema.view(slot.range).spell(book);
    Diagnostic::error("relator-slot-twice", format!("`{}` is filled twice", word.text))
        .label(word.loc, "a second line for the same slot")
        .context(first, "it was filled here")
        .help("a slot takes one entity: keep the line that is right")
        .note(format!("a slot that takes several says so: `has {} {takes} many`", word.text))
}

fn missing_slot(book: &Book, written: WrittenContract<'_, '_>, kind: Id<Kind>, slot: &Slot) -> Diagnostic {
    let (kind_name, name) = (book.name(book.kinds[kind].name), book.name(slot.name));
    let describe = book.schema.view(slot.range).describe(book);
    Diagnostic::error(
        "relator-slot-missing",
        format!("`{}` is {}, and nothing says its {name}", book.name(written.name), article(kind_name)),
    )
    .label(written.loc, format!("{} needs its {name}", article(kind_name)))
    .context(slot.loc, format!("`{kind_name}` declares it as {describe}"))
    .help(format!("write who it is under the contract: `{name} NAME`"))
}

/// The laws a contract's kind writes once, as this book has them: each `also` of the kind and of the kinds above it,
/// its roles standing where the contract's fillers do, and the legs that touch nothing of the book left out.
pub(super) fn legs<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    written: WrittenContract<'a, 's>,
) -> Vec<Id<Law>> {
    let Some(kind) = world.book.contracts[written.id].kind else { return Vec::new() };
    let Some((stands, empty)) = positions(world, written, kind) else { return Vec::new() };
    let positions = Positions { stands: &stands, empty: &empty };
    let mut laws = Vec::new();
    for (decl, alsos) in kind_alsos(world, collected, kind) {
        let site =
            Placement { file: decl.file(), home: decl.home(), owner: Owner::Contract(written.id), subject: Ty::Flow };
        for also in &decl.file()[alsos] {
            if touches_nothing(world, &site, also, positions) {
                continue;
            }
            laws.extend(compile_also(world, &site, also, positions));
        }
    }
    laws
}

/// Where each role stands in this book, and which the contract leaves empty. Nothing, after it is said, when a role
/// stands somewhere this version cannot put it.
fn positions<'s>(
    world: &mut World<'s>,
    written: WrittenContract<'_, 's>,
    kind: Id<Kind>,
) -> Option<(Vec<(Sym, Id<Place>)>, Vec<Sym>)> {
    let contract = &world.book.contracts[written.id];
    let holding = holding_of(world, written, contract)?;
    let errors = world.diags.len();
    let mut stands = Vec::new();
    for filler in contract.fillers.iter() {
        match stand(&world.book, written, contract, holding, filler) {
            Ok(place) => stands.push((filler.slot, place)),
            Err(problem) => world.diags.push(problem),
        }
    }
    let takes_entities = |slot: &&Slot| matches!(slot.range, Range::Kinds(_));
    let slots = world.book.schema.effective(&world.book.kinds, kind).filter(takes_entities);
    let empty = slots.map(|slot| slot.name).filter(|name| !stands.iter().any(|(slot, _)| slot == name)).collect();
    (world.diags.len() == errors).then_some((stands, empty))
}

/// The account a contract pays from or into, which is where its owner stands in every leg of its kind.
fn holding_of(world: &World<'_>, written: WrittenContract<'_, '_>, contract: &Contract) -> Option<Id<Place>> {
    let _ = world;
    let schedule = written.node.schedule.or(written.node.standing)?;
    let direction = schedule.terms.holding?.direction;
    let header = &contract.terms.as_ref().or(contract.standing.as_ref())?.template.first()?.header.flow;
    Some(match direction {
        ast::Direction::From => header.from,
        ast::Direction::Into => header.to,
    })
}

/// Where the filler of a role stands: the holding, if it is the owner of the contract or a member of it, else the
/// outside place it has as a party.
fn stand(
    book: &Book,
    written: WrittenContract<'_, '_>,
    contract: &Contract,
    holding: Id<Place>,
    filler: &Filler,
) -> Result<Id<Place>, Diagnostic> {
    if is_member(book, filler.entity, contract.owner) {
        return Ok(holding);
    }
    let place = book.entities[filler.entity].place;
    match place.filter(|&place| !matches!(book.places[place].role, Role::Holding(_))) {
        Some(place) => Ok(place),
        None => Err(not_the_holding(book, written, contract, filler)),
    }
}

/// Whether `entity` is `owner` or is a member of it, however far up: the household's members stand where it does.
fn is_member(book: &Book, entity: Id<Entity>, owner: Id<Entity>) -> bool {
    let mut at = entity;
    for _ in 0..book.entities.len() {
        if at == owner {
            return true;
        }
        match book.member(at) {
            Some(up) => at = up,
            None => return false,
        }
    }
    false
}

fn not_the_holding(book: &Book, written: WrittenContract<'_, '_>, contract: &Contract, filler: &Filler) -> Diagnostic {
    let (slot, entity) = (book.name(filler.slot), book.name(book.entities[filler.entity].path));
    let owner = book.name(book.entities[contract.owner].path);
    Diagnostic::error("relator-position", format!("`{entity}` does not stand where `{}` pays", book.name(written.name)))
        .label(filler.loc, format!("`{entity}` is the {slot}, and is an owner here, but not of this contract's account"))
        .note(format!("the contract pays from or into an account of `{owner}`: a role stands there for `{owner}` and its members"))
        .help("a role at another owner's own position is not read yet: fill it with someone outside, or write the leg on the contract")
}

/// The `also` lines of a kind and of every kind above it, outermost first, with the declaration each was written in.
fn kind_alsos<'a, 's>(
    world: &World<'s>,
    collected: &Collected<'a, 's>,
    kind: Id<Kind>,
) -> Vec<(Written<'a, 's, ast::Decl<'s>>, ast::Many<ast::Also<'s>>)> {
    let mut lineage: Vec<Id<Kind>> = world.book.kinds.lineage(kind).collect();
    lineage.reverse();
    let declared = |above: Id<Kind>| {
        collected
            .decls_of(DeclKind::Kind)
            .find(|decl| Some(decl.file().loc(decl.node.name.0)) == world.book.kinds[above].loc)
    };
    lineage.into_iter().filter_map(declared).map(|decl| (*decl, decl.node.alsos)).collect()
}

/// Whether a leg has both its ends outside the book: it moves value between two parties, which no book of an owner of
/// neither has anything to say of.
fn touches_nothing<'s>(
    world: &World<'s>,
    site: &Placement<'_, 's>,
    also: &ast::Also<'s>,
    positions: Positions<'_>,
) -> bool {
    let outside =
        |end: Option<Id<Place>>| end.is_some_and(|place| matches!(world.book.places[place].role, Role::Outside(_)));
    flow_ends(world, site.home, site.file, &also.line, positions).is_some_and(|(from, to)| outside(from) && outside(to))
}
