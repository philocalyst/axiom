//! Turning written names into ids, with a diagnostic when it cannot be done.
//!
//! Everything here reads the world and changes nothing, so the parallel
//! elaboration of the journal can use it from every thread. A lookup that
//! misses is cheap; the diagnostic, with its did-you-mean, is built only if
//! the caller decides the miss is an error.

use axiom_core::diag::closest;
use axiom_core::num::DecError;
use axiom_core::{Dec, Diagnostic, Id, Loc, Sym};
use axiom_syntax::Name;

use crate::args::list;
use crate::book::{Amount, Class, Commodity, Entity, Kind, Miss, Param, Place, System};
use crate::errors::{Candidate, ambiguous, not_used, unknown};
use crate::kinds;
use crate::names::{Found, Names};
use crate::scope::Home;
use crate::world::World;

/// A place written in a flow, and the entity it stood for if it was one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct End {
    pub place: Id<Place>,
    /// An entity in place position resolves to its `via` place and becomes the
    /// counterparty of the flow.
    pub entity: Option<Id<Entity>>,
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

    pub fn commodity(&self, name: Name) -> Result<Id<Commodity>, Diagnostic> {
        self.book.commodity(name.text).ok_or_else(|| unknown("unknown-commodity", "commodity", name, None))
    }

    /// `number` of the commodity `unit`, in its quanta.
    pub fn amount(&self, number: Dec, unit: Name, loc: Loc) -> Result<Amount, Diagnostic> {
        let commodity = self.commodity(unit)?;
        let scale = self.book.commodities[commodity].scale;
        match number.to_qty(scale) {
            Ok(qty) => Ok(Amount::new(qty, commodity)),
            Err(DecError::Inexact) => Err(Diagnostic::error(
                "amount-precision",
                format!("{} has more decimals than {} allows", written(number, unit.text), unit.text),
            )
            .label(loc, format!("{} counts {scale} decimal places", unit.text))
            .help(format!(
                "round it, or declare `commodity {}` with `precision {}`",
                unit.text,
                number.places()
            ))),
            Err(DecError::Range) => Err(Diagnostic::error("amount-range", "this amount is too large to count exactly")
                .label(loc, "beyond 100,000,000,000,000,000 quanta")
                .help("a single amount stays below that; use a coarser unit")),
        }
    }

    // ─── Kinds ──────────────────────────────────────────────────────────────

    fn find_kind(&self, home: Home, name: Name) -> Result<Id<Kind>, Miss<Kind>> {
        let scope = self.scopes.of(home);
        let lookup = &self.book.lookup.kinds;
        kinds::find(lookup, &self.book.names, &self.book.systems, name.text, |home| scope.sees(home))
    }

    fn kind_miss(&self, miss: Miss<Kind>, name: Name) -> Diagnostic {
        let loc_of = |id: Id<Kind>| self.book.kinds[id].loc;
        kinds::unresolved(miss, name, &self.book.lookup.kinds, &self.book.names, &self.book.systems, loc_of)
    }

    pub fn seek_kind(&self, home: Home, name: Name) -> Sought<Kind> {
        match self.find_kind(home, name) {
            Ok(kind) => Sought::Found(kind),
            Err(miss @ Miss::Ambiguous(_)) => Sought::Ambiguous(self.kind_miss(miss, name)),
            Err(Miss::Unknown { .. }) => Sought::Missing,
        }
    }

    pub fn kind(&self, home: Home, name: Name) -> Result<Id<Kind>, Diagnostic> {
        self.find_kind(home, name).map_err(|miss| self.kind_miss(miss, name))
    }

    // ─── Entities ───────────────────────────────────────────────────────────

    /// The entity `home` can see under `name`.
    pub fn seek_entity(&self, home: Home, name: Name) -> Sought<Entity> {
        let (lookup, scope) = (&self.book.lookup.entities, self.scopes.of(home));
        match lookup.names.find(&self.book.names, name.text, |id| scope.sees(lookup.home(id))) {
            Found::One(entity) => Sought::Found(entity),
            Found::Nothing => Sought::Missing,
            Found::Several(ids) => {
                let entities = &self.book.entities;
                let candidates = self.candidates(&lookup.names, &ids, |id| entities[id].path, |id| entities[id].loc);
                Sought::Ambiguous(ambiguous("ambiguous-entity", "entities", name, &candidates))
            }
        }
    }

    fn missing_entity(&self, home: Home, name: Name) -> Diagnostic {
        let (lookup, names) = (&self.book.lookup.entities, &self.book.names);
        let suggestion = lookup.resolve(names, self.scopes.of(home), name.text).err().and_then(|miss| match miss {
            Miss::Unknown { suggestion } => suggestion,
            Miss::Ambiguous(_) => None,
        });
        let mut diagnostic = unknown("unknown-entity", "entity", name, suggestion.map(|sym| names.name(sym)));
        for &hidden in lookup.names.candidates(names, name.text) {
            if let Home::System(system) = lookup.home(hidden) {
                diagnostic = not_used(diagnostic, "entity", name.text, self.book.name(self.book.systems[system].path));
            }
        }
        diagnostic
    }

    pub fn entity(&self, home: Home, name: Name) -> Result<Id<Entity>, Diagnostic> {
        self.seek_entity(home, name).or_else(|| self.missing_entity(home, name))
    }

    // ─── Places ─────────────────────────────────────────────────────────────

    /// A place by full path or unique suffix. Not an entity.
    pub fn seek_place(&self, name: Name) -> Sought<Place> {
        let lookup = &self.book.lookup.places;
        match lookup.find(&self.book.names, name.text, |_| true) {
            Found::One(place) => Sought::Found(place),
            Found::Nothing => Sought::Missing,
            Found::Several(ids) => {
                let places = &self.book.places;
                let candidates = self.candidates(lookup, &ids, |id| places[id].path, |id| places[id].loc);
                Sought::Ambiguous(ambiguous("ambiguous-place", "accounts", name, &candidates))
            }
        }
    }

    fn missing_place(&self, name: Name, also_entities: bool) -> Diagnostic {
        let (places, entities, names) = (&self.book.lookup.places, &self.book.lookup.entities.names, &self.book.names);
        let known = places.keys(names);
        let near = match also_entities {
            true => closest(name.text, known.chain(entities.keys(names))),
            false => closest(name.text, known),
        };
        let roots: Vec<&str> = Class::ALL.iter().map(|class| class.root()).collect();
        let leaf = name.text.rsplit('/').next().unwrap_or(name.text);
        unknown("unknown-place", "place", name, near).help(format!(
            "to open a new account, write its full path under {}, for example `expenses/{leaf}`",
            list(&roots),
        ))
    }

    pub fn place(&self, name: Name) -> Result<Id<Place>, Diagnostic> {
        self.seek_place(name).or_else(|| self.missing_place(name, false))
    }

    /// A place written as one end of a flow: a place, `?`, or an entity, which
    /// stands for its `via` place.
    pub fn end(&self, name: Name) -> Result<End, Diagnostic> {
        if name.text == "?" {
            return Ok(End { place: self.book.roots.unknown, entity: None });
        }
        match self.seek_place(name) {
            Sought::Found(place) => return Ok(End { place, entity: None }),
            Sought::Ambiguous(diagnostic) => return Err(diagnostic),
            Sought::Missing => {}
        }
        let entity = match self.seek_entity(Home::Project, name) {
            Sought::Found(entity) => entity,
            Sought::Ambiguous(diagnostic) => return Err(diagnostic),
            Sought::Missing => return Err(self.missing_place(name, true)),
        };
        let declared = &self.book.entities[entity];
        let Some(place) = declared.via else {
            let mut diagnostic =
                Diagnostic::error("entity-without-via", format!("`{}` has no place to stand for", name.text))
                    .label(name.loc, "an entity written where a place is expected stands for its `via` place")
                    .help(format!("add `via` to `entity {}`, for example `via expenses/{}`", name.text, name.text));
            if let Some(loc) = declared.loc {
                diagnostic = diagnostic.context(loc, "declared here without `via`");
            }
            return Err(diagnostic);
        };
        Ok(End { place, entity: Some(entity) })
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

    /// The param `home` can see under `name`: its own system's first, then its
    /// ancestors', then the used systems' (which must not disagree). Written
    /// `us/401k/limit`, it names that system's param, used or not.
    pub fn seek_param(&self, home: Home, name: Name) -> Sought<Param> {
        let (lookup, names) = (&self.book.lookup.params, &self.book.names);
        let scope = self.scopes.of(home);
        let (qualifier, leaf) = match name.text.rsplit_once('/') {
            Some((system, leaf)) => (Some(system), leaf),
            None => (None, name.text),
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
                let declared = |id: Id<Param>| Some(self.book.params[id].loc);
                let candidates: Vec<Candidate> = several
                    .iter()
                    .map(|&id| match self.param_system(id) {
                        Some(system) => Candidate {
                            is: format!("`{leaf}` from `{system}`"),
                            declared: declared(id),
                            write: Some(format!("{system}/{leaf}")),
                        },
                        None => {
                            Candidate { is: format!("the project's `{leaf}`"), declared: declared(id), write: None }
                        }
                    })
                    .collect();
                Sought::Ambiguous(ambiguous("ambiguous-param", "params", name, &candidates))
            }
        }
    }

    pub fn missing_param(&self, home: Home, name: Name) -> Diagnostic {
        let (lookup, names) = (&self.book.lookup.params, &self.book.names);
        let scope = self.scopes.of(home);
        let leaf = name.text.rsplit('/').next().unwrap_or(name.text);
        let visible = lookup
            .names
            .keys(names)
            .filter(|&known| lookup.names.candidates(names, known).iter().any(|&id| scope.sees(lookup.home(id))));
        let mut diagnostic = unknown("unknown-param", "param", name, closest(name.text, visible));
        for &hidden in lookup.names.candidates(names, leaf) {
            if let Some(system) = self.param_system(hidden) {
                diagnostic = not_used(diagnostic, "param", leaf, system);
            }
        }
        diagnostic
    }

    pub fn system(&self, name: Name) -> Result<Id<System>, Diagnostic> {
        self.systems.find(name.text).ok_or_else(|| self.systems.unknown(name))
    }
}

/// `84.2 USD`, for messages about a number as written.
fn written(number: Dec, unit: &str) -> String {
    match number.to_ratio() {
        Some(ratio) => format!("`{ratio} {unit}`"),
        None => format!("this amount of {unit}"),
    }
}
