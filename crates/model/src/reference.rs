//! Reading a written reference as an address, and saying so when it is none or several.
//!
//! The names every account has (its path and each suffix of it) are tried first and unchanged: a reference that meant one
//! account before means it still, and nothing declared later changes it. The index of [`Addresses`] is asked in two
//! cases only, and only in a book that writes some account as an address. Those names found **several** accounts and one
//! of them is spelled, so the line's day may tell them apart, or they found **nothing**, and the reference has two words
//! or more: the party pass has made a party of every such mention that is not meant as an address (see
//! `declare/parties.rs`), so one that no table knows is an address that nothing has.
//!
//! Nothing here changes the world: like the rest of `resolve`, it reads the book, so the journal can be elaborated from
//! every thread.

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic, Id, Sym};

use crate::addresses::Part;
use crate::book::{Book, Entity, Place};
use crate::declare::World;
use crate::errors::{Candidate, Word};
use crate::names::Found;
use crate::problem::{self, Noun};
use crate::resolve::End;
use crate::scope::Home;

/// What a reference found among the names every account has, which is why the index is asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reached {
    /// Several accounts, and the line's day may tell them apart.
    Several,
    /// None, and the reference may be an address no name spells.
    Nothing,
}

impl Book<'_> {
    /// The accounts `text` is an address of, on any day, when it is written as one: each word before the last an entity.
    /// For a setting or a report, which have no line and no home to look the words up from.
    pub(crate) fn address_place(&self, text: &str) -> Found<Place> {
        if !self.lookup.addresses.is_used() {
            return Found::Nothing;
        }
        let (leading, name) = text.rsplit_once('/').unwrap_or(("", text));
        let words = leading.split('/').filter(|word| !word.is_empty());
        let fillers: Result<Vec<Id<Entity>>, _> = words.map(|word| self.entity(word)).collect();
        match (fillers, self.names.get(name)) {
            (Ok(fillers), Some(name)) => self.lookup.addresses.resolve(&fillers, name, None),
            _ => Found::Nothing,
        }
    }
}

impl World<'_> {
    /// The account the words of `word` are an address of on `day` (on any day with none), or why they are not.
    /// `None` when the reference is not an address attempt: it is for the party and place tables to say what it is.
    pub(crate) fn address_end(
        &self,
        home: Home,
        word: Word,
        day: Option<Day>,
        reached: Reached,
    ) -> Option<Result<End, Diagnostic>> {
        if !self.book.lookup.addresses.is_used() {
            return None;
        }
        if let Some(place) = self.settled(home, word) {
            return Some(Ok(End { place, entity: None }));
        }
        // A word alone that no name answers to is for the party and commodity tables.
        if reached == Reached::Nothing && !word.text.contains('/') {
            return None;
        }
        let (leading, name) = word.text.rsplit_once('/').map_or(("", word.text), |(leading, name)| (leading, name));
        let fillers = match self.fillers(home, word, leading, reached) {
            Ok(fillers) => fillers?,
            Err(problem) => return Some(Err(problem)),
        };
        let Some(name) = self.book.names.get(name) else { return Some(Err(self.unknown_address(word, &fillers, day))) };
        Some(match self.book.lookup.addresses.resolve(&fillers, name, day) {
            Found::One(place) => Ok(End { place, entity: None }),
            Found::Several(places) => Err(self.ambiguous_address(word, &places, day)),
            Found::Nothing => Err(self.unknown_address(word, &fillers, day)),
        })
    }

    /// The account the text means on every day, if it was worked out once. Only a reference that no name answers to was
    /// (see `settle_addresses`), so one that the names found several accounts of is never in it.
    fn settled(&self, home: Home, word: Word) -> Option<Id<Place>> {
        let text = (home == Home::Project).then(|| self.book.names.get(word.text)).flatten()?;
        self.book.lookup.addresses.settled(text)
    }

    /// Works out, once, what each reference of two words or more that the sources write means, wherever no line's day and
    /// no home can change it: no name answers to it, and the one account the index finds is open on every day. The names
    /// of the accounts, the entities and the contracts are all declared when this is called, so what it finds is what
    /// each of them would find again, and a hit costs one lookup of the text.
    pub(crate) fn settle_addresses(&mut self) {
        if !self.book.lookup.addresses.is_used() {
            return;
        }
        let mut once = axiom_core::Map::default();
        for text in std::mem::take(&mut self.references) {
            let word = Word { text, loc: axiom_core::Loc::default() };
            let named =
                self.special_end(Home::Project, word).is_some() || self.found_end(Home::Project, word, None).is_some();
            let Some(Ok(End { place, .. })) =
                (!named).then(|| self.address_end(Home::Project, word, None, Reached::Nothing)).flatten()
            else {
                continue;
            };
            if self.book.lookup.addresses.is_always_open(place) {
                once.insert(self.book.names.intern(text), place);
            }
        }
        self.book.lookup.addresses.settle(once);
    }

    /// The entities the words before the name are. None if the names found several accounts and a word is no entity: the
    /// reference stays ambiguous as it was. Otherwise the reference is an attempt at an address (the party pass has made
    /// a party of every mention that is not one), and a word that is no entity is a mistake in it.
    fn fillers(
        &self,
        home: Home,
        word: Word,
        leading: &str,
        reached: Reached,
    ) -> Result<Option<Vec<Id<Entity>>>, Diagnostic> {
        let (book, scope) = (&self.book, self.scopes.of(home));
        let mut fillers = Vec::new();
        for text in leading.split('/').filter(|text| !text.is_empty()) {
            match book.lookup.entities.find(&book.names, scope, text) {
                Found::One(entity) => fillers.push(entity),
                Found::Nothing if reached == Reached::Nothing => return Err(self.unknown_address(word, &fillers, None)),
                Found::Nothing => return Ok(None),
                Found::Several(ids) => return Err(self.ambiguous_entity(Word { text, loc: word.loc }, &ids)),
            }
        }
        Ok(Some(fillers))
    }

    /// Whether the reference of `fillers` and `name` is read as this account on `day`: by the names every account has if
    /// they find it alone, else by the index. A word of digits alone is a number, and cannot be written as a name.
    fn means(&self, place: Id<Place>, fillers: &[Id<Entity>], name: Sym, day: Option<Day>) -> bool {
        let book = &self.book;
        let text = self.spell_reference(fillers, name);
        let numeric = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'_' | b'.'));
        if !text.contains('/') && numeric(&text) {
            return false;
        }
        match book.lookup.places.candidates(&book.names, &text) {
            [only] => *only == place,
            several if several.len() > 1 && !several.iter().any(|&place| book.is_spelled(place)) => false,
            _ => matches!(book.lookup.addresses.resolve(fillers, name, day), Found::One(only) if only == place),
        }
    }

    fn spell_reference(&self, fillers: &[Id<Entity>], name: Sym) -> String {
        let parts: Vec<Part> = fillers.iter().map(|&entity| Part::Filler(entity)).chain([Part::Name(name)]).collect();
        self.spell(&parts)
    }

    /// An account's address, or a reference, written out.
    pub(crate) fn spell(&self, parts: &[Part]) -> String {
        let book = &self.book;
        let words = parts.iter().map(|part| match *part {
            Part::Filler(entity) => book.name(book.entities[entity].path),
            Part::Name(name) => book.names.name(name),
        });
        words.collect::<Vec<_>>().join("/")
    }

    /// Several accounts have this address on the day, each with the shortest address that means only it.
    pub(crate) fn ambiguous_address(&self, word: Word, places: &[Id<Place>], day: Option<Day>) -> Diagnostic {
        let addresses = &self.book.lookup.addresses;
        let describe = |&place: &Id<Place>| Candidate {
            is: format!("`{}`", self.spell(addresses.address(place))),
            declared: self.book.places[place].loc,
            write: Some(
                self.spell(&addresses.shortest_that(place, |fillers, name| self.means(place, fillers, name, day))),
            ),
        };
        let candidates: Vec<Candidate> = places.iter().map(describe).collect();
        let diagnostic = problem::ambiguous(Noun::Address, word, &candidates);
        match day {
            Some(day) => {
                diagnostic.note(format!("these are the accounts open on {day}: a later one may need a longer address"))
            }
            None => diagnostic,
        }
    }

    /// No account has this address on the day: the closest name among those its words fill, and when an account has it
    /// on another day, that.
    fn unknown_address(&self, word: Word, fillers: &[Id<Entity>], day: Option<Day>) -> Diagnostic {
        let (leading, name) = word.text.rsplit_once('/').unwrap_or(("", word.text));
        let named: Vec<&str> = self.named_by(fillers).collect();
        let nearest = closest(name, named.iter().copied()).map(|near| match leading.is_empty() {
            true => near.to_string(),
            false => format!("{leading}/{near}"),
        });
        let diagnostic = problem::unknown(Noun::Address, word, nearest.as_deref());
        match (day, self.on_another_day(fillers, name)) {
            (Some(day), Some(account)) => diagnostic.note(format!("`{account}` is not open on {day}")),
            _ if named.is_empty() || nearest.is_some() => diagnostic,
            _ => diagnostic.note(format!(
                "the accounts these words fill are called {}",
                crate::errors::list(&named[..named.len().min(6)])
            )),
        }
        .help("an address is the entities that fill an account's slots, in order, then its name: `jordan/bluefin/401k`")
    }

    /// The names of the accounts the first of `fillers` fills a slot of.
    fn named_by<'w>(&'w self, fillers: &[Id<Entity>]) -> impl Iterator<Item = &'w str> + 'w {
        let addresses = &self.book.lookup.addresses;
        let called = fillers.first().map(|&first| addresses.filled_by(first)).unwrap_or_default();
        let names = called.into_iter().filter_map(|place| match addresses.address(place).last() {
            Some(&Part::Name(name)) => Some(self.book.names.name(name)),
            _ => None,
        });
        let mut seen = axiom_core::Set::default();
        names.filter(move |&name| seen.insert(name)).collect::<Vec<_>>().into_iter()
    }

    /// An account with these words and this name that is open on some day, written out.
    fn on_another_day(&self, fillers: &[Id<Entity>], name: &str) -> Option<String> {
        let name = self.book.names.get(name)?;
        let addresses = &self.book.lookup.addresses;
        match addresses.resolve(fillers, name, None) {
            Found::One(place) => Some(self.spell(addresses.address(place))),
            Found::Several(places) => Some(self.spell(addresses.address(places[0]))),
            Found::Nothing => None,
        }
    }
}
