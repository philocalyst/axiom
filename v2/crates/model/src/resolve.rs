//! Turning written names into ids, with a diagnostic when it cannot be done.
//!
//! Everything here reads the world and changes nothing, so the parallel
//! elaboration of the journal can use it from every thread. A lookup that
//! misses is cheap; the diagnostic, with its did-you-mean, is built only if
//! the caller decides the miss is an error, and for the journal only once per
//! name however often it was written.

use axiom_core::num::DecError;
use axiom_core::{Dec, Diagnostic, Id, Loc, Sym};

use crate::book::{Amount, Class, Commodity, Entity, Kind, Miss, Param, Place, System};
use crate::declare::{World, near_place};
use crate::errors::{Candidate, Word, ambiguous, count, list, not_used, unknown};
use crate::kinds;
use crate::names::{Found, Names, near};
use crate::scope::Home;

/// A place written in a flow, and the entity it stood for if it was one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct End {
    pub place: Id<Place>,
    /// An entity in place position resolves to its `via` place and becomes the
    /// counterparty of the flow.
    pub entity: Option<Id<Entity>>,
}

/// Why a name written in the journal names nothing usable. Each cause is
/// explained once per name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Cause {
    Place,
    AmbiguousPlace,
    Entity,
    AmbiguousEntity,
    /// An entity in place position that has no `via`.
    NoVia,
    Commodity,
    /// An occurrence of a plan the project does not declare.
    Plan,
}

/// The outcome of looking a name up among one sort of thing.
pub(crate) enum Sought<T> {
    Found(Id<T>),
    /// Nothing answers. Other sorts of thing may.
    Missing,
    /// Several answer. That is an error whatever else exists.
    Ambiguous(Diagnostic),
}

impl<T> Sought<T> {
    /// The id, or the diagnostic: `missing` says what a miss means.
    pub fn or_else(self, missing: impl FnOnce() -> Diagnostic) -> Result<Id<T>, Diagnostic> {
        match self {
            Sought::Found(id) => Ok(id),
            Sought::Missing => Err(missing()),
            Sought::Ambiguous(diagnostic) => Err(diagnostic),
        }
    }
}

impl<'s> World<'s> {
    /// The symbol of a text the survey interned: codes, docs, waiver reasons.
    pub fn sym(&self, text: &str) -> Sym {
        self.book.names.get(text).expect("the survey interned every code, doc and reason it found")
    }

    pub fn commodity(&self, symbol: &str) -> Option<Id<Commodity>> {
        self.book.commodity(symbol)
    }

    /// A commodity by name, or the error that says it is not one, with the
    /// declared commodity it may have been meant for.
    pub fn commodity_of(&self, word: Word) -> Result<Id<Commodity>, Diagnostic> {
        self.commodity(word.text).ok_or_else(|| self.explain_commodity(word))
    }

    fn explain_commodity(&self, word: Word) -> Diagnostic {
        let symbols = self.book.commodities.values().map(|commodity| self.book.name(commodity.symbol));
        let diagnostic = unknown("unknown-commodity", "commodity", word, near(word.text, symbols));
        diagnostic.note(format!(
            "commodities are declared with `commodity {}`; USD, EUR, GBP… come with `use std`",
            word.text
        ))
    }

    /// `number` of the commodity `unit`, in its quanta.
    pub fn amount(&self, number: Dec, unit: Id<Commodity>, loc: Loc) -> Result<Amount, Diagnostic> {
        let commodity = &self.book.commodities[unit];
        let (scale, symbol) = (commodity.scale, self.book.name(commodity.symbol));
        match number.to_qty(scale) {
            Ok(qty) => Ok(Amount::new(qty, unit)),
            Err(DecError::Inexact) => Err(Diagnostic::error(
                "amount-precision",
                format!("{} has more decimals than {symbol} allows", written(number, symbol)),
            )
            .label(loc, format!("{symbol} counts {scale} decimal places"))
            .help(format!("round it, or declare `commodity {symbol}` with `precision {}`", number.places()))),
            Err(DecError::Range) => Err(Diagnostic::error("amount-range", "this amount is too large to count exactly")
                .label(loc, "beyond 100,000,000,000,000,000 quanta")
                .help("a single amount stays below that; use a coarser unit")),
        }
    }

    // ─── Kinds ──────────────────────────────────────────────────────────────

    fn find_kind(&self, home: Home, word: Word) -> Result<Id<Kind>, Miss<Kind>> {
        let scope = self.scopes.of(home);
        kinds::find(&self.book.lookup.kinds, &self.book.names, &self.book.systems, word.text, |home| scope.sees(home))
    }

    fn kind_miss(&self, miss: Miss<Kind>, word: Word) -> Diagnostic {
        let loc_of = |id: Id<Kind>| self.book.kinds[id].loc;
        kinds::unresolved(miss, word, &self.book.lookup.kinds, &self.book.names, &self.book.systems, loc_of)
    }

    pub fn seek_kind(&self, home: Home, word: Word) -> Sought<Kind> {
        match self.find_kind(home, word) {
            Ok(kind) => Sought::Found(kind),
            Err(miss @ Miss::Ambiguous(_)) => Sought::Ambiguous(self.kind_miss(miss, word)),
            Err(Miss::Unknown { .. }) => Sought::Missing,
        }
    }

    pub fn kind(&self, home: Home, word: Word) -> Result<Id<Kind>, Diagnostic> {
        self.find_kind(home, word).map_err(|miss| self.kind_miss(miss, word))
    }

    // ─── Entities ───────────────────────────────────────────────────────────

    /// The entity `home` can see under `word`.
    pub fn seek_entity(&self, home: Home, word: Word) -> Sought<Entity> {
        let (lookup, scope) = (&self.book.lookup.entities, self.scopes.of(home));
        match lookup.find(&self.book.names, scope, word.text) {
            Found::One(entity) => Sought::Found(entity),
            Found::Nothing => Sought::Missing,
            Found::Several(ids) => Sought::Ambiguous(self.ambiguous_entity(word, &ids)),
        }
    }

    fn ambiguous_entity(&self, word: Word, ids: &[Id<Entity>]) -> Diagnostic {
        let entities = &self.book.entities;
        let candidates =
            self.candidates(&self.book.lookup.entities.names, ids, |id| entities[id].path, |id| entities[id].loc);
        ambiguous("ambiguous-entity", "entities", word, &candidates)
    }

    fn missing_entity(&self, home: Home, word: Word) -> Diagnostic {
        let (lookup, names) = (&self.book.lookup.entities, &self.book.names);
        let suggestion = lookup.resolve(names, self.scopes.of(home), word.text).err().and_then(|miss| match miss {
            Miss::Unknown { suggestion } => suggestion,
            Miss::Ambiguous(_) => None,
        });
        let mut diagnostic = unknown("unknown-entity", "entity", word, suggestion.map(|sym| names.name(sym)));
        for &hidden in lookup.names.candidates(names, word.text) {
            if let Home::System(system) = lookup.home(hidden) {
                diagnostic = not_used(diagnostic, "entity", word.text, self.book.name(self.book.systems[system].path));
            }
        }
        diagnostic
    }

    pub fn entity(&self, home: Home, word: Word) -> Result<Id<Entity>, Diagnostic> {
        self.seek_entity(home, word).or_else(|| self.missing_entity(home, word))
    }

    // ─── Places ─────────────────────────────────────────────────────────────

    /// A place by full path, unique suffix or alias. Not an entity.
    pub fn seek_place(&self, word: Word) -> Sought<Place> {
        match self.book.lookup.places.find(&self.book.names, word.text, |_| true) {
            Found::One(place) => Sought::Found(place),
            Found::Nothing => Sought::Missing,
            Found::Several(ids) => {
                let places = &self.book.places;
                let names = &self.book.lookup.places;
                let candidates = self.candidates(names, &ids, |id| places[id].path, |id| places[id].loc);
                Sought::Ambiguous(ambiguous("ambiguous-place", "accounts", word, &candidates))
            }
        }
    }

    pub fn place(&self, word: Word) -> Result<Id<Place>, Diagnostic> {
        self.seek_place(word).or_else(|| self.explain_unknown_place(word, false))
    }

    /// A place written as one end of a flow: a place, `?`, or an entity, which
    /// stands for its `via` place.
    pub fn find_end(&self, text: &str) -> Result<End, Cause> {
        if text == "?" {
            return Ok(End { place: self.book.roots.unknown, entity: None });
        }
        match self.book.lookup.places.find(&self.book.names, text, |_| true) {
            Found::One(place) => {
                return Ok(End { place, entity: None });
            }
            Found::Several(_) => return Err(Cause::AmbiguousPlace),
            Found::Nothing => {}
        }
        let (lookup, scope) = (&self.book.lookup.entities, self.scopes.of(Home::Project));
        match lookup.find(&self.book.names, scope, text) {
            Found::One(entity) => match self.book.entities[entity].via {
                Some(place) => Ok(End { place, entity: Some(entity) }),
                None => Err(Cause::NoVia),
            },
            Found::Several(_) => Err(Cause::AmbiguousEntity),
            Found::Nothing => Err(Cause::Place),
        }
    }

    /// The diagnostic for a name that resolved to nothing usable, written
    /// `uses` times, first at `word`.
    pub fn explain(&self, cause: Cause, word: Word, uses: usize) -> Diagnostic {
        let mut diagnostic = match cause {
            Cause::Place => self.explain_unknown_place(word, true),
            Cause::AmbiguousPlace => self.explain_ambiguous_place(word),
            Cause::Entity => self.explain_unknown_entity(word),
            Cause::AmbiguousEntity => match self.seek_entity(Home::Project, word) {
                Sought::Ambiguous(diagnostic) => diagnostic,
                _ => unknown("unknown-entity", "entity", word, None),
            },
            Cause::NoVia => self.explain_no_via(word),
            Cause::Commodity => self.explain_commodity(word),
            Cause::Plan => unknown("unknown-plan", "plan", word, None),
        };
        if uses > 1 && cause != Cause::AmbiguousPlace {
            diagnostic = diagnostic.note(format!("{} write it, and none of them is kept", count(uses, "line")));
        }
        diagnostic
    }

    fn explain_unknown_place(&self, word: Word, also_entities: bool) -> Diagnostic {
        let (places, entities, names) = (&self.book.lookup.places, &self.book.lookup.entities.names, &self.book.names);
        let known = places.keys(names);
        let closest = match also_entities {
            true => near(word.text, known.chain(entities.keys(names))),
            false => near(word.text, known),
        };
        let mut diagnostic = unknown("unknown-place", "place", word, closest);
        // A full path is opened unless it is a typo; say which one it was taken for.
        if crate::collect::class_of(word.text).is_some() {
            let declared: Vec<&str> = self.book.places.values().map(|place| self.book.name(place.path)).collect();
            if let Some(typo) = near_place(word.text, &declared) {
                diagnostic = diagnostic
                    .note(format!("it is close to `{typo}`, so it is not opened as a new account"))
                    .help(format!("if it is a new account, declare it: `account {}`", word.text));
                if closest != Some(typo) {
                    diagnostic = diagnostic.fix(format!("did you mean `{typo}`?"), word.loc, typo);
                }
            }
            return diagnostic;
        }
        if closest.is_none() {
            let roots: Vec<&str> = Class::ALL.iter().map(|class| class.root()).collect();
            let leaf = word.text.rsplit('/').next().unwrap_or(word.text);
            diagnostic = diagnostic.help(format!(
                "to open a new account, write its full path under {}, for example `expenses/{leaf}`",
                list(&roots),
            ));
        }
        diagnostic
    }

    /// One report for a suffix that several accounts answer to, at the
    /// declaration that made it ambiguous: the later one.
    fn explain_ambiguous_place(&self, word: Word) -> Diagnostic {
        let (names, places) = (&self.book.names, &self.book.places);
        let mut ids: Vec<Id<Place>> = self.book.lookup.places.candidates(names, word.text).to_vec();
        ids.sort_by_key(|id| self.ordinal[id.index()]);
        let (&later, earlier) = ids.split_last().expect("ambiguous names have candidates");
        let path = |id: Id<Place>| self.book.name(places[id].path);
        let mut diagnostic = Diagnostic::error(
            "ambiguous-place",
            format!("`{}` makes `{}` ambiguous, and lines still write it", path(later), word.text),
        );
        diagnostic = match places[later].loc {
            Some(loc) => diagnostic.label(loc, format!("`{}` now also ends `{}`", path(later), word.text)),
            None => diagnostic.label(word.loc, "which account is meant?"),
        };
        for &id in earlier {
            if let Some(loc) = places[id].loc {
                diagnostic = diagnostic.context(loc, format!("`{}` was declared before it", path(id)));
            }
        }
        diagnostic = diagnostic.context(word.loc, format!("`{}` is first written here", word.text));
        if let Some(loc) = places[earlier[0]].loc {
            let end = Loc::new(loc.file, loc.end, loc.end);
            diagnostic = diagnostic.fix(
                format!("keep `{}` meaning `{}` with an alias", word.text, path(earlier[0])),
                end,
                format!(" as {}", word.text),
            );
        }
        for &id in &ids {
            let shortest = self.book.lookup.places.shortest_unique(names, path(id), id);
            diagnostic = diagnostic.help(format!("or write `{shortest}` for `{}` where it is meant", path(id)));
        }
        diagnostic
    }

    fn explain_unknown_entity(&self, word: Word) -> Diagnostic {
        let mut diagnostic = self.missing_entity(Home::Project, word);
        diagnostic = diagnostic.note("a payee must be a declared entity, so that it is typed like everything else");
        if let Sought::Found(place) = self.seek_place(word) {
            let path = self.book.name(self.book.places[place].path);
            diagnostic = Diagnostic::error(
                "unknown-entity",
                format!("`{}` is a place, not an entity, so it cannot be a payee", word.text),
            )
            .label(word.loc, "a payee is an entity: a person or a company")
            .note(format!("`{}` is the place `{path}`", word.text))
            .help("drop the payee, or use the entity that stands for this place");
        }
        diagnostic
    }

    fn explain_no_via(&self, word: Word) -> Diagnostic {
        let entity = match self.seek_entity(Home::Project, word) {
            Sought::Found(entity) => Some(&self.book.entities[entity]),
            _ => None,
        };
        let mut diagnostic = Diagnostic::error(
            "entity-without-via",
            format!("`{}` is not tied to a place, so a flow cannot end there", word.text),
        )
        .label(word.loc, "an entity written where a place is expected stands for its `via` place")
        .help(format!(
            "name the place its payments belong to: add `via expenses/{}` to `entity {}`",
            word.text, word.text
        ));
        if let Some(loc) = entity.and_then(|entity| entity.loc) {
            diagnostic = diagnostic.context(loc, "declared without `via`");
        }
        diagnostic
    }

    /// The things an ambiguous suffix could mean, each with the shortest
    /// written form that means only it.
    fn candidates<T>(
        &self,
        table: &Names<T>,
        ids: &[Id<T>],
        path: impl Fn(Id<T>) -> Sym,
        loc: impl Fn(Id<T>) -> Option<Loc>,
    ) -> Vec<Candidate> {
        let names = &self.book.names;
        let describe = |&id: &Id<T>| {
            let full = self.book.name(path(id));
            let write = table.shortest_unique(names, full, id).to_string();
            Candidate { is: format!("`{full}`"), declared: loc(id), write: Some(write) }
        };
        ids.iter().map(describe).collect()
    }

    // ─── Params ─────────────────────────────────────────────────────────────

    fn param_system(&self, id: Id<Param>) -> Option<&'s str> {
        match self.book.lookup.params.home(id) {
            Home::System(system) => Some(self.book.name(self.book.systems[system].path)),
            Home::Project | Home::Builtin => None,
        }
    }

    /// The param `home` can see under `word`: its own system's first, then its
    /// ancestors', then the used systems' (which must not disagree). Written
    /// `us/401k/limit`, it names that system's param, used or not.
    pub fn seek_param(&self, home: Home, word: Word) -> Sought<Param> {
        let (lookup, names) = (&self.book.lookup.params, &self.book.names);
        let scope = self.scopes.of(home);
        let (qualifier, leaf) = match word.text.rsplit_once('/') {
            Some((system, leaf)) => (Some(system), leaf),
            None => (None, word.text),
        };
        let seen: Vec<Id<Param>> = lookup
            .names
            .candidates(names, leaf)
            .iter()
            .copied()
            .filter(|&id| match qualifier {
                Some(system) => self.param_system(id) == Some(system),
                None => scope.sees(lookup.home(id)),
            })
            .collect();
        let nearest = seen.iter().map(|&id| scope.rank(lookup.home(id))).min();
        let best: Vec<Id<Param>> =
            seen.into_iter().filter(|&id| Some(scope.rank(lookup.home(id))) == nearest).collect();
        match best.as_slice() {
            [only] => Sought::Found(*only),
            [] => Sought::Missing,
            several => {
                let candidates: Vec<Candidate> = several
                    .iter()
                    .map(|&id| {
                        let declared = Some(self.book.params[id].loc);
                        match self.param_system(id) {
                            Some(system) => Candidate {
                                is: format!("`{leaf}` from `{system}`"),
                                declared,
                                write: Some(format!("{system}/{leaf}")),
                            },
                            None => Candidate { is: format!("the project's `{leaf}`"), declared, write: None },
                        }
                    })
                    .collect();
                Sought::Ambiguous(ambiguous("ambiguous-param", "params", word, &candidates))
            }
        }
    }

    pub fn missing_param(&self, home: Home, word: Word) -> Diagnostic {
        let (lookup, names) = (&self.book.lookup.params, &self.book.names);
        let scope = self.scopes.of(home);
        let leaf = word.text.rsplit('/').next().unwrap_or(word.text);
        let visible = lookup
            .names
            .keys(names)
            .filter(|&known| lookup.names.candidates(names, known).iter().any(|&id| scope.sees(lookup.home(id))));
        let mut diagnostic = unknown("unknown-param", "param", word, near(word.text, visible));
        for &hidden in lookup.names.candidates(names, leaf) {
            if let Some(system) = self.param_system(hidden) {
                diagnostic = not_used(diagnostic, "param", leaf, system);
            }
        }
        diagnostic
    }

    pub fn system(&self, word: Word) -> Result<Id<System>, Diagnostic> {
        self.systems.find(word.text).ok_or_else(|| self.systems.unknown(word))
    }
}

/// `84.2 USD`, for messages about a number as written.
fn written(number: Dec, unit: &str) -> String {
    match number.to_ratio() {
        Some(ratio) => format!("`{ratio} {unit}`"),
        None => format!("this amount of {unit}"),
    }
}
