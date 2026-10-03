//! Accounts written with the things that fill their slots before the name: `jordan/bluefin/401k`.
//!
//! The words before the name of an account's path are the entities that fill its slots, and the last word is its name:
//! `account jordan/bluefin/401k` is a 401(k) of jordan's, sponsored by bluefin, and says it once, where
//! `account jordan-401k` and its `owner jordan` and `employer bluefin` lines said it twice and left the two to agree.
//!
//! # What makes a path spelled
//!
//! A path is **spelled** when it has two words or more and every word but the last is the name of one entity. Anything
//! else (`assets/bank/checking`, where `assets` is no entity) is a path in a tree, as it has always been. So the words
//! are fillers first and tree segments second, and a book that has no account whose leading words are all entities reads
//! as it did. A spelled account is a root of the place tree with its whole path, and no place for any prefix.
//!
//! # Why placement is a pass of its own
//!
//! Whether a path is spelled needs only the entities, which exist when the accounts are made. *Which slot* each word
//! fills needs the kind's slots, which are declared after the places are. So the placement runs once they are, in
//! [`place_words`], after the role lines have said what they say (a slot a line filled is not free) and before the
//! slots nothing filled are called missing. It fixes up the account it was made for: its owner, and the owner's place,
//! which is a holding once something is owned by it.
//!
//! The rule is `core::placement`'s: a word is placed only where every way of placing all the words puts it. The slots a
//! word may take are `owner`, which takes any entity, and each slot of the kind that takes entities of the word's kind.
//! `with` is not one of them, for any entity could be a custodian and no owner would be placed; `at` says it.

use axiom_core::placement::{self, Placed, Unplaceable};
use axiom_core::tagless::Datum;
use axiom_core::{Days, Diagnostic, Id, Interner, Loc, Set, SlotId, Tree};
use axiom_syntax::{Decl, DeclKind};

use crate::book::{Book, Class, Entity, Place, Role};
use crate::collect::{Collected, Written};
use crate::declare::World;
use crate::errors::Word;
use crate::fill::{Filled, holds_one};
use crate::holders::Holder;
use crate::names::{Found, Scoped};
use crate::problem::{self, Placing, Role as Way};
use crate::scope::Scope;
use crate::slots::{Range, Slot};

/// A word before the name of a spelled path, and the entity it is.
#[derive(Clone, Copy)]
pub(crate) struct Leading<'s> {
    pub text: &'s str,
    pub entity: Id<Entity>,
}

/// The entities written before the name of `path`, each by its own flat name, if every one is one: what makes a path
/// spelled. A path with one word has none.
pub(crate) fn leading<'s>(
    entities: &Scoped<Entity>,
    tree: &Tree<Entity>,
    names: &Interner<'s>,
    scope: &Scope,
    path: &'s str,
) -> Option<Vec<Leading<'s>>> {
    let (words, _) = path.rsplit_once('/')?;
    let flat = |text: &'s str| match entities.find(names, scope, text) {
        Found::One(entity) if names.name(tree[entity].path) == text => Some(Leading { text, entity }),
        _ => None,
    };
    words.split('/').map(flat).collect()
}

impl Book<'_> {
    /// Whether the account was written with the entities that fill its slots before its name. Such an account is a root
    /// of the tree whose path has a `/`: an account written as a path in a tree always has a place for its prefix.
    pub fn is_spelled(&self, place: Id<Place>) -> bool {
        let place_ = &self.places[place];
        matches!(place_.role, Role::Account { .. })
            && place_.loc.is_some()
            && self.places.parent(place).is_none()
            && self.name(place_.path).contains('/')
    }
}

/// A slot that a word before the name may be placed in.
#[derive(Clone, Copy)]
enum Free {
    /// The owner, which takes any entity.
    Owner,
    /// A slot the kind declares that takes entities.
    Slot { number: SlotId, slot: Slot },
}

impl Free {
    fn name<'b>(self, book: &'b Book) -> &'b str {
        match self {
            Free::Owner => "owner",
            Free::Slot { slot, .. } => book.name(slot.name),
        }
    }

    fn takes(self, book: &Book) -> String {
        match self {
            Free::Owner => "any entity".to_string(),
            Free::Slot { slot, .. } => book.schema.view(slot.range).describe(book),
        }
    }

    fn takes_one(self) -> bool {
        match self {
            Free::Owner => true,
            Free::Slot { slot, .. } => holds_one(slot.mult),
        }
    }

    fn fits(self, book: &Book, entity: Id<Entity>) -> bool {
        match self {
            Free::Owner => true,
            Free::Slot { slot: Slot { range: Range::Kinds(run), .. }, .. } => {
                let kind = book.entities[entity].kind;
                book.schema.kinds_of(run).iter().any(|&of| book.is_a(kind, of))
            }
            Free::Slot { .. } => false,
        }
    }
}

/// Everything about one account that the placement reads: where it is written, the words before its name, and the
/// slots they may still take.
struct Spelling<'w, 'a, 's> {
    written: &'w Written<'a, 's, Decl<'s>>,
    place: Id<Place>,
    words: Vec<Leading<'s>>,
    free: Vec<Free>,
}

impl<'s> Spelling<'_, '_, 's> {
    /// The slots each word fits, as a set, and the slots that take one word.
    fn candidates(&self, book: &Book) -> (Vec<u16>, u16) {
        let fitting = |word: &Leading| self.slots_where(|free| free.fits(book, word.entity));
        (self.words.iter().map(fitting).collect(), self.slots_where(|free| free.takes_one()))
    }

    /// The set of free slots that `holds` of.
    fn slots_where(&self, holds: impl Fn(&Free) -> bool) -> u16 {
        self.free.iter().enumerate().filter(|(_, free)| holds(free)).fold(0, |set, (at, _)| set | 1 << at)
    }

    /// The names of the free slots in a set.
    fn names<'b>(&self, book: &'b Book, set: u16) -> Vec<&'b str> {
        let free = self.free.iter().enumerate();
        free.filter(|(at, _)| set >> at & 1 == 1).map(|(_, free)| free.name(book)).collect()
    }

    /// Where the word is written: a slice of the path, which is a slice of the source.
    fn loc(&self, word: usize) -> Loc {
        self.written.file().loc(self.words[word].text)
    }

    /// What a diagnostic says of the word, the account and its kind.
    fn placing<'b>(&self, book: &'b Book, word: usize) -> Placing<'b>
    where
        's: 'b,
    {
        Placing {
            word: Word { text: self.words[word].text, loc: self.loc(word) },
            path: self.written.node.name.0,
            kind: book.name(book.kinds[book.places[self.place].kind].name),
        }
    }

    /// The edit that writes the word as a role: the path without it, and a line under the header that says it.
    fn role_edit(&self, word: usize, slot: &str) -> (Loc, String) {
        let (file, written) = (self.written.file(), self.written);
        let (path, header) = (file.loc(written.node.name.0), written.item.loc);
        let words = written.node.name.0.split('/').enumerate();
        let kept: Vec<&str> = words.filter(|&(at, _)| at != word).map(|(_, text)| text).collect();
        let after = &file.src[path.end as usize..header.end as usize];
        let line = format!("{}{after}\n  {slot} {}", kept.join("/"), self.words[word].text);
        (Loc::new(path.file, path.start, header.end), line)
    }

    fn ambiguous(&self, book: &Book, word: usize, set: u16) -> Diagnostic {
        let slots = self.names(book, set);
        let roles = slots.iter().map(|&slot| Way { slot, edit: self.role_edit(word, slot) }).collect();
        problem::ambiguous_placement(&self.placing(book, word), &slots, roles)
    }

    fn fits_no_slot(&self, book: &Book, word: usize) -> Diagnostic {
        let left: Vec<(&str, String)> = self.free.iter().map(|free| (free.name(book), free.takes(book))).collect();
        problem::word_fits_no_slot(&self.placing(book, word), &left)
    }

    fn too_many_words(&self, book: &Book) -> Diagnostic {
        let mut placing = self.placing(book, 0);
        placing.word.loc = placing.word.loc.to(self.loc(self.words.len() - 1));
        let fits = |word: &Leading<'s>| {
            let fitting = self.slots_where(|free| free.fits(book, word.entity));
            (word.text, self.names(book, fitting))
        };
        let fits: Vec<(&str, Vec<&str>)> = self.words.iter().map(fits).collect();
        problem::words_that_do_not_fit_together(&placing, &fits)
    }
}

/// Places the words before the name of every spelled account into the slots they fill, and says them as a role line
/// would. `filled` is every slot a line has said so far, and is told every slot a word fills, so that none is called
/// missing for it.
pub(crate) fn place_words<'a, 's>(
    world: &mut World<'s>,
    collected: &Collected<'a, 's>,
    filled: &mut Filled,
    diags: &mut Vec<Diagnostic>,
) {
    let mut done = Set::default();
    for written in collected.decls_of(DeclKind::Account) {
        let path = written.node.name.0;
        let Ok(place) = world.place(Word { text: path, loc: written.item.loc }) else { continue };
        if !world.book.is_spelled(place) || !done.insert(place) {
            continue;
        }
        let book = &world.book;
        let scope = world.scopes.of(written.home());
        let Some(words) = leading(&book.lookup.entities, &book.entities, &book.names, scope, path) else { continue };
        let free = free_slots(world, written, place, filled);
        place_one(world, &Spelling { written, place, words, free }, filled, diags);
    }
}

fn place_one<'s>(
    world: &mut World<'s>,
    spelling: &Spelling<'_, '_, 's>,
    filled: &mut Filled,
    diags: &mut Vec<Diagnostic>,
) {
    let (candidates, single) = spelling.candidates(&world.book);
    match placement::place(&candidates, single) {
        Ok(placement) => fill_placed(world, spelling, placement.as_slice(), filled, diags),
        Err(Unplaceable::NoCandidate(word)) => {
            diags.push(spelling.fits_no_slot(&world.book, usize::from(word)));
            attempt(filled, spelling, u16::MAX);
        }
        Err(Unplaceable::NoPlacement) => {
            diags.push(spelling.too_many_words(&world.book));
            attempt(filled, spelling, u16::MAX);
        }
    }
}

/// Says what the words that were placed fill, and what the words that could go two ways are.
fn fill_placed<'s>(
    world: &mut World<'s>,
    spelling: &Spelling<'_, '_, 's>,
    placed: &[Placed],
    filled: &mut Filled,
    diags: &mut Vec<Diagnostic>,
) {
    let mut fills: Vec<Vec<usize>> = vec![Vec::new(); spelling.free.len()];
    for (word, placed) in placed.iter().enumerate() {
        match *placed {
            Placed::Forced(slot) => fills[usize::from(slot)].push(word),
            Placed::Ambiguous(set) => {
                diags.push(spelling.ambiguous(&world.book, word, set));
                attempt(filled, spelling, set);
            }
        }
    }
    for (free, words) in spelling.free.iter().zip(fills).filter(|(_, words)| !words.is_empty()) {
        fill_slot(world, spelling, *free, &words, filled);
    }
}

/// A word that is wrong is still an attempt to fill a slot: it is said once, and the slots are not said missing too.
fn attempt(filled: &mut Filled, spelling: &Spelling<'_, '_, '_>, set: u16) {
    for free in spelling.free.iter().enumerate().filter(|(at, _)| set >> at & 1 == 1).map(|(_, free)| free) {
        if let Free::Slot { number, .. } = free {
            filled.insert((Holder::Place(spelling.place), *number));
        }
    }
}

/// The slots the words may take: the owner, if no line gave one, and each slot of the kind that takes entities and that
/// no line of the account or of its kind fills.
fn free_slots(world: &World<'_>, written: &Written<'_, '_, Decl<'_>>, place: Id<Place>, filled: &Filled) -> Vec<Free> {
    let book = &world.book;
    let (file, decl) = (written.file(), written.node);
    let holder = Holder::Place(place);
    let kind = book.places[place].kind;
    let owner = (!file[decl.props].iter().any(|line| line.name.0 == "owner")).then_some(Free::Owner);
    let is_filled = |number| {
        filled.contains(&(holder, number))
            || book.kinds.lineage(kind).any(|above| filled.contains(&(Holder::Kind(above), number)))
    };
    let slots = book.schema.entity_slots(&book.kinds, kind).into_iter().filter(|&(number, _)| !is_filled(number));
    owner.into_iter().chain(slots.map(|(number, slot)| Free::Slot { number, slot })).take(16).collect()
}

/// Says that these words fill `free`: the owner is the account's, and a slot's values are said as a line would say them.
fn fill_slot<'s>(
    world: &mut World<'s>,
    spelling: &Spelling<'_, '_, 's>,
    free: Free,
    words: &[usize],
    filled: &mut Filled,
) {
    let holder = Holder::Place(spelling.place);
    let entities: Vec<Id<Entity>> = words.iter().map(|&word| spelling.words[word].entity).collect();
    match free {
        Free::Owner => own(&mut world.book, spelling.place, entities[0]),
        Free::Slot { number, slot } => {
            match holds_one(slot.mult) {
                true => world.paint(holder, number, Days::ALWAYS, Datum::of(entities[0])),
                false => {
                    world.paint_set(holder, number, Days::ALWAYS, entities.iter().map(|&e| Datum::of(e)).collect())
                }
            }
            world.say_site(holder, number, 0, spelling.loc(words[0]));
            filled.insert((holder, number));
        }
    }
}

/// `owner` owns `place`, and so holds: its own place is a holding, as it is when a line says it owns anything.
fn own(book: &mut Book<'_>, place: Id<Place>, owner: Id<Entity>) {
    book.places[place].owner = owner;
    book.places[place].shares = Box::default();
    let Some(own) = book.entities[owner].place else { return };
    if matches!(book.places[own].role, Role::Outside(_)) {
        let asset = book.roots.kinds.asset;
        let held = &mut book.places[own];
        (held.class, held.role, held.kind, held.owner) = (Class::Asset, Role::Holding(owner), asset, owner);
    }
}
