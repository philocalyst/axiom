//! Turning written names into ids, with a diagnostic when it cannot be done.
//!
//! Everything here reads the world and changes nothing, so the parallel
//! elaboration of the journal can use it from every thread. A lookup that
//! misses is cheap; the diagnostic, with its did-you-mean, is built only if
//! the caller decides the miss is an error, and for the journal only once per
//! name however often it was written.

use axiom_core::diag::closest;
use axiom_core::num::DecError;
use axiom_core::{Dec, Diagnostic, Id, Loc, Sym};

use crate::book::{Amount, Commodity, Entity, Kind, Miss, Param, Place, Purpose, Role, System};
use crate::declare::{World, near_place};
use crate::errors::{Candidate, Word};
use crate::kinds;
use crate::names::{Found, Names, Scoped};
use crate::problem::{self, Noun, Unused};
use crate::scope::Home;

/// A place written in a flow, and the entity it stood for if it was one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct End {
    pub place: Id<Place>,
    /// An entity in place position resolves to its `via` place and becomes the
    /// counterparty of the flow.
    pub entity: Option<Id<Entity>>,
}

/// What looking a name up among one sort of thing found: the thing, nothing
/// (other sorts of thing may answer), or several, which is an error whatever
/// else exists.
pub(crate) type Seek<T> = Result<Option<Id<T>>, Diagnostic>;

impl<'s> World<'s> {
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
        let nearest = closest(word.text, symbols);
        problem::unknown(Noun::Commodity, word, nearest, &[]).note(format!(
            "commodities are declared with `commodity {}`; USD, EUR, GBP… come with `use std`",
            word.text
        ))
    }

    /// `number` of the commodity `unit`, in its quanta.
    pub fn amount(&self, number: Dec, unit: Id<Commodity>, loc: Loc) -> Result<Amount, Diagnostic> {
        let scale = self.book.commodities[unit].scale;
        let quantity = number.to_qty(scale);
        quantity.map(|qty| Amount::new(qty, unit)).map_err(|error| self.not_an_amount(error, number, unit, loc))
    }

    /// Why a written number is not an amount of `unit`.
    fn not_an_amount(&self, error: DecError, number: Dec, unit: Id<Commodity>, loc: Loc) -> Diagnostic {
        let commodity = &self.book.commodities[unit];
        let (scale, symbol) = (commodity.scale, self.book.name(commodity.symbol));
        match error {
            DecError::Inexact => Diagnostic::error(
                "amount-precision",
                format!("{} has more decimals than {symbol} allows", written(number, symbol)),
            )
            .label(loc, format!("{symbol} counts {scale} decimal places"))
            .help(format!("round it, or declare `commodity {symbol}` with `precision {}`", number.places())),
            DecError::Range => Diagnostic::error("amount-range", "this amount is too large to count exactly")
                .label(loc, "beyond 100,000,000,000,000,000 quanta")
                .help("a single amount stays below that; use a coarser unit"),
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

    pub fn seek_kind(&self, home: Home, word: Word) -> Seek<Kind> {
        match self.find_kind(home, word) {
            Ok(kind) => Ok(Some(kind)),
            Err(Miss::Unknown { .. }) => Ok(None),
            Err(miss) => Err(self.kind_miss(miss, word)),
        }
    }

    pub fn kind(&self, home: Home, word: Word) -> Result<Id<Kind>, Diagnostic> {
        self.find_kind(home, word).map_err(|miss| self.kind_miss(miss, word))
    }

    // ─── Entities ───────────────────────────────────────────────────────────

    /// The entity `home` can see under `word`.
    pub fn seek_entity(&self, home: Home, word: Word) -> Seek<Entity> {
        let (lookup, scope) = (&self.book.lookup.entities, self.scopes.of(home));
        match lookup.find(&self.book.names, scope, word.text) {
            Found::One(entity) => Ok(Some(entity)),
            Found::Nothing => Ok(None),
            Found::Several(ids) => Err(self.ambiguous_entity(word, &ids)),
        }
    }

    fn ambiguous_entity(&self, word: Word, ids: &[Id<Entity>]) -> Diagnostic {
        let entities = &self.book.entities;
        let candidates =
            self.candidates(&self.book.lookup.entities.names, ids, |id| entities[id].path, |id| entities[id].loc);
        problem::ambiguous(Noun::Entity, word, &candidates)
    }

    /// Why `word` names nothing `home` can see: the closest name it can see, and the systems that declare it
    /// without being used.
    fn missing<T>(&self, noun: Noun, lookup: &Scoped<T>, home: Home, word: Word) -> Diagnostic {
        let names = &self.book.names;
        let nearest = match lookup.resolve(names, self.scopes.of(home), word.text) {
            Err(Miss::Unknown { suggestion }) => suggestion.map(|sym| names.name(sym)),
            Ok(_) | Err(Miss::Ambiguous(_)) => None,
        };
        let unused: Vec<_> = (lookup.names.candidates(names, word.text).iter())
            .filter_map(|&hidden| match lookup.home(hidden) {
                Home::System(system) => Some(self.book.name(self.book.systems[system].path)),
                Home::Project | Home::Builtin => None,
            })
            .map(|system| Unused { name: word.text, system })
            .collect();
        problem::unknown(noun, word, nearest, &unused)
    }

    pub fn entity(&self, home: Home, word: Word) -> Result<Id<Entity>, Diagnostic> {
        self.seek_entity(home, word)?.ok_or_else(|| self.missing(Noun::Entity, &self.book.lookup.entities, home, word))
    }

    // ─── Purposes ───────────────────────────────────────────────────────────

    /// A purpose visible from this declaration's system or the project.
    pub fn seek_purpose(&self, home: Home, word: Word) -> Seek<Purpose> {
        let (lookup, scope) = (&self.book.lookup.purposes, self.scopes.of(home));
        match lookup.find(&self.book.names, scope, word.text) {
            Found::One(purpose) => Ok(Some(purpose)),
            Found::Nothing => Ok(None),
            Found::Several(ids) => Err(self.ambiguous_purpose(word, &ids)),
        }
    }

    fn ambiguous_purpose(&self, word: Word, ids: &[Id<Purpose>]) -> Diagnostic {
        let purposes = &self.book.purposes;
        let candidates =
            self.candidates(&self.book.lookup.purposes.names, ids, |id| purposes[id].name, |id| purposes[id].loc);
        problem::ambiguous(Noun::Purpose, word, &candidates)
    }

    pub fn purpose(&self, home: Home, word: Word) -> Result<Id<Purpose>, Diagnostic> {
        self.seek_purpose(home, word)?
            .ok_or_else(|| self.missing(Noun::Purpose, &self.book.lookup.purposes, home, word))
    }

    // ─── Places ─────────────────────────────────────────────────────────────

    /// A place by full path, unique suffix or alias. Not an entity.
    pub fn seek_place(&self, word: Word) -> Seek<Place> {
        match self.book.lookup.places.find(&self.book.names, word.text, |_| true) {
            Found::One(place) => Ok(Some(place)),
            Found::Nothing => Ok(None),
            Found::Several(ids) => {
                let places = &self.book.places;
                let names = &self.book.lookup.places;
                let candidates = self.candidates(names, &ids, |id| places[id].path, |id| places[id].loc);
                Err(problem::ambiguous(Noun::Place, word, &candidates))
            }
        }
    }

    pub fn place(&self, word: Word) -> Result<Id<Place>, Diagnostic> {
        self.seek_place(word)?.ok_or_else(|| self.explain_unknown_place(word))
    }

    /// Resolve a journal endpoint in its source home. Places and entities are
    /// considered together so a suffix in one namespace cannot hide a match
    /// in the other. Entities stand for their configured holding/outside place
    /// and retain their identity as the counterparty.
    pub(crate) fn end(&self, home: Home, word: Word) -> Result<End, Diagnostic> {
        if word.text == "?" {
            return Ok(End {
                place: self.book.entities[self.book.roots.unknown].place.expect("unknown has an endpoint"),
                entity: None,
            });
        }
        if let Some(sym) = self.book.names.get(word.text)
            && let Some(&end) = self.contract_endpoints.get(&sym)
        {
            return Ok(end);
        }
        if let Some(contract) = self.book.contract(word.text) {
            if let Some(loan) = self.book.contracts[contract].loan {
                return Ok(End { place: loan.debt, entity: Some(self.book.contracts[contract].party) });
            }
            if let Some(entity) = self.seek_entity(home, word)? {
                return self.entity_end(entity, word);
            }
            return Err(Diagnostic::error(
                "contract-endpoint",
                format!("contract `{}` is not a flow endpoint", word.text),
            )
            .label(word.loc, "name its party or holding account instead")
            .help("loan contracts name their debt tab; other contracts are not places"));
        }
        if self.book.asset(word.text).is_some() {
            return Err(Diagnostic::error("asset-endpoint", format!("asset `{}` is not a flow endpoint", word.text))
                .label(word.loc, "this names the asset itself")
                .help(format!("use `#purchase of {}` to acquire the asset", word.text)));
        }

        let place_candidates = self.book.lookup.places.candidates(&self.book.names, word.text);
        let entity_candidates = self.book.lookup.entities.names.candidates(&self.book.names, word.text);
        let visible = || {
            entity_candidates
                .iter()
                .copied()
                .filter(|&entity| self.scopes.of(home).sees(self.book.lookup.entities.home(entity)))
        };
        let mut visible_entities = visible();
        let entity = visible_entities.next();
        let multiple_entities = visible_entities.next().is_some();

        if place_candidates.len() > 1 && entity.is_none() {
            return Err(self.seek_place(word).expect_err("multiple visible places must be ambiguous"));
        }
        if multiple_entities && place_candidates.is_empty() {
            return Err(self.seek_entity(home, word).expect_err("multiple visible entities must be ambiguous"));
        }
        if (place_candidates.len() > 1 || multiple_entities || entity.is_some())
            && !(place_candidates.len() == 1
                && !multiple_entities
                && entity.is_some_and(|entity| self.book.entities[entity].place == Some(place_candidates[0])))
            && !place_candidates.is_empty()
            && entity.is_some()
        {
            return Err(self.ambiguous_end(word, place_candidates, &visible().collect::<Vec<_>>()));
        }
        if let Some(entity) = entity {
            return self.entity_end(entity, word);
        }
        if let Some(&place) = place_candidates.first() {
            if let Role::Asset(asset) = self.book.places[place].role {
                let name = self.book.name(self.book.assets[asset].name);
                return Err(Diagnostic::error("asset-endpoint", format!("asset `{name}` is not a flow endpoint"))
                    .label(word.loc, "this names the asset itself")
                    .help(format!("use `#purchase of {name}` to acquire the asset")));
            }
            return Ok(End { place, entity: None });
        }
        if let Some(unit) = self.book.commodity(word.text) {
            if let Some(place) = self.book.issuer_place(unit) {
                return Ok(End { place, entity: None });
            }
            return Err(Diagnostic::error(
                "commodity-endpoint",
                format!("commodity `{}` has no issuer endpoint", word.text),
            )
            .label(word.loc, "this commodity's kind chain declares no `pays` purpose")
            .help("write `pays PURPOSE` on its commodity kind before using it as a party"));
        }
        let entity = self.entity(home, word)?;
        self.entity_end(entity, word)
    }

    fn ambiguous_end(&self, word: Word, places: &[Id<Place>], entities: &[Id<Entity>]) -> Diagnostic {
        let mut candidates = Vec::with_capacity(places.len() + entities.len());
        for &place in places {
            let place = &self.book.places[place];
            let label = match place.role {
                Role::Asset(_) => "asset place",
                _ => "account",
            };
            candidates.push((place.loc, format!("{label} `{}`", self.book.name(place.path))));
        }
        for &entity in entities {
            let entity = &self.book.entities[entity];
            candidates.push((entity.loc, format!("entity `{}`", self.book.name(entity.path))));
        }
        candidates.sort_by_key(|(loc, _)| *loc);
        let choices = candidates.iter().map(|(_, label)| label.as_str()).collect::<Vec<_>>().join(" or ");
        let mut diagnostic = Diagnostic::error("ambiguous-end", format!("`{}` could mean {choices}", word.text))
            .label(word.loc, "qualify the path to make the endpoint clear");
        for (loc, label) in candidates {
            if let Some(loc) = loc {
                diagnostic = diagnostic.context(loc, format!("{label} declared here"));
            }
        }
        diagnostic
    }

    fn entity_end(&self, entity: Id<Entity>, word: Word) -> Result<End, Diagnostic> {
        self.book.entities[entity].place.map(|place| End { place, entity: Some(entity) }).ok_or_else(|| {
            Diagnostic::error("entity-no-place", format!("entity `{}` has no flow endpoint", word.text))
                .label(word.loc, "this entity has no holding or outside place")
                .help("give the entity an account or resolve its `via` location")
        })
    }

    fn explain_unknown_place(&self, word: Word) -> Diagnostic {
        let (places, names) = (&self.book.lookup.places, &self.book.names);
        let known = places.keys(names);
        let closest = closest(word.text, known);
        let mut diagnostic = problem::unknown(Noun::Place, word, closest, &[]);
        // Old chart roots no longer assign place classes. Preserve a useful
        // refusal for paths that look like an attempt to use the v3 chart.
        if legacy_chart_path(word.text) {
            let declared: Vec<&str> = self.book.places.values().map(|place| self.book.name(place.path)).collect();
            if let Some(typo) = near_place(word.text, &declared) {
                diagnostic = diagnostic
                    .note(format!("it is close to `{typo}`, so it is not opened as a new account"))
                    .help(format!("if this path is a new account, declare it: `account {}`", word.text));
                if closest != Some(typo) {
                    diagnostic = diagnostic.fix(format!("did you mean `{typo}`?"), word.loc, typo);
                }
            } else {
                diagnostic = diagnostic
                    .note("assets, liabilities, income, expenses and equity are not special account roots")
                    .help(format!("declare a full account path and its kind: `account {}`", word.text));
            }
            return diagnostic;
        }
        if closest.is_none() {
            diagnostic = diagnostic
                .help(format!("to open a new account, declare its full path and kind: `account {}`", word.text));
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
    pub fn seek_param(&self, home: Home, word: Word) -> Seek<Param> {
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
            [only] => Ok(Some(*only)),
            [] => Ok(None),
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
                Err(problem::ambiguous(Noun::Param, word, &candidates))
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
        let nearest = closest(word.text, visible);
        let unused: Vec<_> = (lookup.names.candidates(names, leaf).iter())
            .filter_map(|&hidden| self.param_system(hidden))
            .map(|system| Unused { name: leaf, system })
            .collect();
        problem::unknown(Noun::Param, word, nearest, &unused)
    }

    pub fn system(&self, word: Word) -> Result<Id<System>, Diagnostic> {
        self.systems.find(word.text).ok_or_else(|| self.systems.unknown(word))
    }
}

fn legacy_chart_path(path: &str) -> bool {
    ["assets", "liabilities", "income", "expenses", "equity"]
        .into_iter()
        .any(|root| path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')))
}

/// `84.2 USD`, for messages about a number as written.
fn written(number: Dec, unit: &str) -> String {
    match number.to_ratio() {
        Some(ratio) => format!("`{ratio} {unit}`"),
        None => format!("this amount of {unit}"),
    }
}
