//! Turning written names into ids, with a diagnostic when it cannot be done.
//!
//! Everything here reads the world and changes nothing, so the parallel
//! elaboration of the journal can use it from every thread. A lookup that
//! misses is cheap; the diagnostic, with its did-you-mean, is built only if
//! the caller decides the miss is an error, and for the journal only once per
//! name however often it was written.

use axiom_core::diag::closest;
use axiom_core::num::DecError;
use axiom_core::{Day, Dec, Diagnostic, Id, Loc};
use axiom_syntax::{File, Literal};

use crate::book::{Amount, Commodity, Entity, Kind, Miss, Param, Place, Purpose, Role, System};
use crate::declare::{World, near_place};
use crate::errors::{Candidate, Word};
use crate::kinds;
use crate::names::{Found, Scoped};
use crate::problem::{self, Among, Noun};
use crate::reference::Reached;
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
        problem::unknown(Noun::Commodity, word, nearest).note(format!(
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

    /// A written literal as an amount of the unit it names, or of `fallback` when it names none; with no
    /// fallback an amount without a unit is a mistake.
    pub fn literal_amount(
        &self,
        file: &File<'s>,
        literal: Literal<'s>,
        fallback: Option<Id<Commodity>>,
    ) -> Result<Amount, Diagnostic> {
        let unit = match (literal.unit(), fallback) {
            (Some(unit), _) => self.commodity_of(Word::of(file, unit.0))?,
            (None, Some(unit)) => unit,
            (None, None) => {
                return Err(Diagnostic::error("amount-unit", "this amount needs an explicit unit")
                    .label(file.loc(literal.0), "write a commodity after the amount"));
            }
        };
        self.amount(literal.num(), unit, file.loc(literal.0))
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
        kinds::unresolved(miss, word, &self.among(&self.book.lookup.kinds), |id| &self.book.kinds[id])
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

    pub(crate) fn ambiguous_entity(&self, word: Word, ids: &[Id<Entity>]) -> Diagnostic {
        let (entities, table) = (&self.book.entities, &self.book.lookup.entities.names);
        let candidates = problem::shortest(&self.book.names, table, ids, |id| entities[id].path, |id| entities[id].loc);
        problem::ambiguous(Noun::Entity, word, &candidates)
    }

    /// What a name is looked up among, for the diagnostics about a lookup that failed.
    fn among<'a, T>(&'a self, index: &'a Scoped<T>) -> Among<'a, 's, T> {
        Among { index, names: &self.book.names, systems: &self.book.systems }
    }

    /// Why `word` names nothing `home` can see: the closest name it can see, and the systems that declare it
    /// without being used.
    fn missing<T>(&self, noun: Noun, lookup: &Scoped<T>, home: Home, word: Word) -> Diagnostic {
        let names = &self.book.names;
        let nearest = match lookup.resolve(names, self.scopes.of(home), word.text) {
            Err(Miss::Unknown { suggestion }) => suggestion.map(|sym| names.name(sym)),
            Ok(_) | Err(Miss::Ambiguous(_)) => None,
        };
        self.among(lookup).unknown(noun, word, nearest)
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
        let (purposes, table) = (&self.book.purposes, &self.book.lookup.purposes.names);
        let candidates = problem::shortest(&self.book.names, table, ids, |id| purposes[id].name, |id| purposes[id].loc);
        problem::ambiguous(Noun::Purpose, word, &candidates)
    }

    pub fn purpose(&self, home: Home, word: Word) -> Result<Id<Purpose>, Diagnostic> {
        self.seek_purpose(home, word)?
            .ok_or_else(|| self.missing(Noun::Purpose, &self.book.lookup.purposes, home, word))
    }

    // ─── Places ─────────────────────────────────────────────────────────────

    /// A place by full path, unique suffix, alias or address. Not an entity.
    pub fn seek_place(&self, word: Word) -> Seek<Place> {
        match self.book.lookup.places.find(&self.book.names, word.text, |_| true) {
            Found::One(place) => Ok(Some(place)),
            Found::Nothing => match self.book.address_place(word.text) {
                Found::One(place) => Ok(Some(place)),
                Found::Several(places) => Err(self.ambiguous_address(word, &places, None)),
                Found::Nothing => Ok(None),
            },
            Found::Several(ids) => {
                let (places, table) = (&self.book.places, &self.book.lookup.places);
                let candidates =
                    problem::shortest(&self.book.names, table, &ids, |id| places[id].path, |id| places[id].loc);
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
        self.end_on(home, word, None)
    }

    /// A flow's end as of the line's `day`: the accounts it may mean are those open that day. With no day, a setting's
    /// or a report's, every account that is ever open.
    pub(crate) fn end_on(&self, home: Home, word: Word, day: Option<Day>) -> Result<End, Diagnostic> {
        if let Some(end) = self.special_end(home, word) {
            return end;
        }
        if let Some(end) = self.found_end(home, word, day) {
            return end;
        }
        if let Some(end) = self.address_end(home, word, day, Reached::Nothing) {
            return end;
        }
        if let Some(end) = self.commodity_end(word) {
            return end;
        }
        let entity = self.entity(home, word)?;
        self.entity_end(entity, word)
    }

    /// The names that are none of an account or a party: the unknown party, a contract and an asset. None for
    /// any other. A contract's name stands for its debt tab when it is a loan, and for its party otherwise.
    // Always inlined: it is half of `end_on`, which `settle_addresses` asks too; a call apiece costs 0.5% of a 100k check.
    #[inline(always)]
    pub(crate) fn special_end(&self, home: Home, word: Word) -> Option<Result<End, Diagnostic>> {
        if word.text == "?" {
            let place = self.book.entities[self.book.roots.unknown].place.expect("unknown has an endpoint");
            return Some(Ok(End { place, entity: None }));
        }
        if let Some(sym) = self.book.names.get(word.text)
            && let Some(&end) = self.contract_endpoints.get(&sym)
        {
            return Some(Ok(end));
        }
        if let Some(contract) = self.book.contract(word.text) {
            let contract = &self.book.contracts[contract];
            if let Some(loan) = contract.loan {
                return Some(Ok(End { place: loan.debt, entity: Some(contract.party) }));
            }
            return Some(match self.seek_entity(home, word) {
                Ok(Some(entity)) => self.entity_end(entity, word),
                Ok(None) => Err(Diagnostic::error(
                    "contract-endpoint",
                    format!("contract `{}` is not a flow endpoint", word.text),
                )
                .label(word.loc, "name its party or holding account instead")
                .help("loan contracts name their debt tab; other contracts are not places")),
                Err(problem) => Err(problem),
            });
        }
        if self.book.asset(word.text).is_some() {
            return Some(Err(asset_endpoint(word.text, word.loc)));
        }
        None
    }

    /// What the places and parties a name answers to say it is, if it answers to any.
    // Always inlined, for the same reason as `special_end`.
    #[inline(always)]
    pub(crate) fn found_end(&self, home: Home, word: Word, day: Option<Day>) -> Option<Result<End, Diagnostic>> {
        let places = self.book.lookup.places.candidates(&self.book.names, word.text);
        let entity_candidates = self.book.lookup.entities.names.candidates(&self.book.names, word.text);
        let visible = || {
            entity_candidates
                .iter()
                .copied()
                .filter(|&entity| self.scopes.of(home).sees(self.book.lookup.entities.home(entity)))
        };
        let mut visible_entities = visible();
        let entity = visible_entities.next();
        let several = visible_entities.next().is_some();
        if places.len() > 1 && entity.is_none() {
            // The line's day may tell apart accounts that are written as addresses; others are ambiguous as ever.
            let spelled = places.iter().any(|&place| self.book.is_spelled(place));
            if let Some(end) = spelled.then(|| self.address_end(home, word, day, Reached::Several)).flatten() {
                return Some(end);
            }
            return Some(Err(self.seek_place(word).expect_err("multiple visible places must be ambiguous")));
        }
        if several && places.is_empty() {
            return Some(Err(self.seek_entity(home, word).expect_err("multiple visible entities must be ambiguous")));
        }
        if let Some(entity) = entity {
            // A name that is an account and a party is one thing only when the party's own place is that
            // account and no other party answers to the name.
            let same = places.len() == 1 && !several && self.book.entities[entity].place == Some(places[0]);
            if !places.is_empty() && !same {
                return Some(Err(self.ambiguous_end(word, places, &visible().collect::<Vec<_>>())));
            }
            return Some(self.entity_end(entity, word));
        }
        let &place = places.first()?;
        if let Role::Asset(asset) = self.book.places[place].role {
            return Some(Err(asset_endpoint(self.book.name(self.book.assets[asset].name), word.loc)));
        }
        Some(Ok(End { place, entity: None }))
    }

    /// A commodity stands for the place its issuer is, when its kind chain says who that is.
    fn commodity_end(&self, word: Word) -> Option<Result<End, Diagnostic>> {
        let unit = self.book.commodity(word.text)?;
        if let Some(place) = self.book.issuer_place(unit) {
            return Some(Ok(End { place, entity: None }));
        }
        Some(Err(Diagnostic::error("commodity-endpoint", format!("commodity `{}` has no issuer endpoint", word.text))
            .label(word.loc, "this commodity's kind chain declares no `pays` purpose")
            .help("write `pays PURPOSE` on its commodity kind before using it as a party")))
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
        let mut diagnostic = problem::unknown(Noun::Place, word, closest);
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

    // ─── Params ─────────────────────────────────────────────────────────────

    /// The param `home` can see under `word`: its own system's first, then its
    /// ancestors', then the used systems' (which must not disagree). Written
    /// `us/401k/limit`, it names that system's param, used or not.
    pub fn seek_param(&self, home: Home, word: Word) -> Seek<Param> {
        let (lookup, names) = (&self.book.lookup.params, &self.book.names);
        let (scope, among) = (self.scopes.of(home), self.among(lookup));
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
                Some(system) => among.system_of(id) == Some(system),
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
                        match among.system_of(id) {
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
        let visible = lookup
            .names
            .keys(names)
            .filter(|&known| lookup.names.candidates(names, known).iter().any(|&id| scope.sees(lookup.home(id))));
        let nearest = closest(word.text, visible);
        self.among(lookup).unknown(Noun::Param, word, nearest)
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

/// Said of an asset's name used where a flow's end goes.
fn asset_endpoint(name: &str, at: Loc) -> Diagnostic {
    Diagnostic::error("asset-endpoint", format!("asset `{name}` is not a flow endpoint"))
        .label(at, "this names the asset itself")
        .help(format!("use `#purchase of {name}` to acquire the asset"))
}
