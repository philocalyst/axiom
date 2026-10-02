//! One answer, for every view, to "whose is this, what is it worth, and how
//! liquid is it".
//!
//! A [`Lens`] is a day and an owner scope over the book. `balance --value`,
//! `available`, `forecast` and the summary all read money through it, so a euro
//! is worth the same in each, and a house is out of reach in each.

use std::collections::BTreeMap;
use std::iter;

use axiom_core::num::{POW10, div_round, mul_div};
use axiom_core::{Day, Diagnostic, Id, Qty, Span};
use axiom_engine::{Holding, Known, OwnerShare, Plan};
use axiom_model::{Amount, Book, Class, Commodity, Entity, Place, Subject};

use crate::history::Held;
use crate::resolve;

/// Whose money a view is about: everyone's, or one entity's, which for a
/// household includes its members'.
#[derive(Clone, Debug, Default)]
pub struct Whose {
    owners: Option<Vec<Id<Entity>>>,
    label: Option<Id<Entity>>,
}

impl Whose {
    /// `--for NAME`, or everyone. An unknown name is an error with a suggestion.
    pub fn resolve(book: &Book, name: Option<&str>) -> Result<Whose, Diagnostic> {
        let Some(name) = name else {
            return Ok(Whose::default());
        };
        Ok(Whose::of(book, resolve::entity(book, name)?))
    }

    /// One entity's, and its members' if it is a household.
    pub fn of(book: &Book, entity: Id<Entity>) -> Whose {
        let members = book.entities.iter().filter(|(_, other)| other.member == Some(entity)).map(|(id, _)| id);
        let mut owners: Vec<_> = iter::once(entity).chain(members).collect();
        owners.sort_unstable();
        Whose { owners: Some(owners), label: Some(entity) }
    }

    pub fn includes(&self, entity: Id<Entity>) -> bool {
        self.owners.as_ref().is_none_or(|owners| owners.binary_search(&entity).is_ok())
    }

    /// Whether this lens covers every owner in the book.
    pub fn is_everyone(&self) -> bool {
        self.owners.is_none()
    }

    /// The selected owners, or `None` for an unfiltered household view.
    pub fn owners(&self) -> Option<&[Id<Entity>]> {
        self.owners.as_deref()
    }

    /// The entity under which machine-readable facts are reported.
    pub fn label<'b>(&self, book: &'b Book<'_>) -> &'b str {
        self.label.map_or("everyone", |entity| book.name(book.entities[entity].path))
    }

    /// Whether a law's subject is one of these owners': the entity itself, or
    /// the owner of the place.
    pub fn governs(&self, book: &Book, subject: Subject) -> bool {
        self.includes(match subject {
            Subject::Place(place) => book.places[place].owner,
            Subject::Entity(entity) => entity,
            Subject::Asset(asset) => book.assets[asset].owner,
            Subject::Contract(contract) => book.contracts[contract].owner,
        })
    }
}

/// How spendable a holding is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Liquidity {
    /// Money in hand: a currency in an account that holds it freely.
    Cash,
    /// What others owe: it arrives on its due day.
    Claim,
    /// Reachable only by drawing it down, which takes this long and costs what
    /// the laws say it costs.
    Slow(Span),
}

/// The books on one day, seen for one owner scope.
#[derive(Clone, Copy)]
pub struct Lens<'b, 's, 'w, 'p> {
    pub whose: &'w Whose,
    pub day: Day,
    /// The exact plan that owns the book view, ownership map and display signs.
    plan: &'p Plan<'b, 's>,
}

impl<'b, 's, 'w, 'p> Lens<'b, 's, 'w, 'p> {
    /// Uses the plan's exact book, ownership graph and display signs so a
    /// lens cannot pair unrelated arenas or fall back to raw entity owners.
    pub fn new(plan: &'p Plan<'b, 's>, whose: &'w Whose, day: Day) -> Lens<'b, 's, 'w, 'p> {
        Lens { whose, day, plan }
    }

    /// The immutable model owned by this lens's exact prepared plan.
    pub fn book(&self) -> &'b Book<'s> {
        self.plan.book()
    }

    /// Names and kinds resolved with the same plan as this lens.
    pub fn known(&self) -> Known {
        self.plan.known()
    }

    /// The same books at another day's prices.
    pub fn on(self, day: Day) -> Lens<'b, 's, 'w, 'p> {
        Lens { day, ..self }
    }

    pub fn owns(self, place: Id<Place>) -> bool {
        self.owns_shares(self.plan.owners_of(place))
    }

    pub fn owns_entity(self, entity: Id<Entity>) -> bool {
        self.owns_shares(self.plan.owners_of_entity(entity))
    }

    fn owns_shares(self, owners: &[OwnerShare]) -> bool {
        self.whose.is_everyone()
            || owners.iter().any(|owner| !owner.share.is_zero() && self.whose.includes(owner.owner))
    }

    pub fn governs(self, subject: Subject) -> bool {
        match subject {
            Subject::Place(place) => self.owns(place),
            Subject::Entity(entity) => self.owns_entity(entity),
            Subject::Asset(asset) => self.owns_entity(self.book().assets[asset].owner),
            Subject::Contract(contract) => self.owns_entity(self.book().contracts[contract].owner),
        }
    }

    pub fn subject_qty(self, subject: Subject, qty: Qty) -> Qty {
        match subject {
            Subject::Place(place) => self.place_qty(place, qty),
            Subject::Entity(entity) => self.entity_qty(entity, qty),
            Subject::Asset(asset) => self.entity_qty(self.book().assets[asset].owner, qty),
            Subject::Contract(contract) => self.entity_qty(self.book().contracts[contract].owner, qty),
        }
    }

    pub fn place_qty(self, place: Id<Place>, qty: Qty) -> Qty {
        if self.whose.is_everyone() {
            return qty;
        }
        self.plan
            .allocate(place, qty)
            .filter(|(owner, _)| self.whose.includes(owner.owner))
            .map(|(_, amount)| amount)
            .sum()
    }

    pub fn entity_qty(self, entity: Id<Entity>, qty: Qty) -> Qty {
        if self.whose.is_everyone() {
            return qty;
        }
        self.plan
            .allocate_entity(entity, qty)
            .filter(|(owner, _)| self.whose.includes(owner.owner))
            .map(|(_, amount)| amount)
            .sum()
    }

    pub fn purpose_direction(
        self,
        from: Id<Place>,
        to: Id<Place>,
        root: axiom_model::PurposeRoot,
    ) -> Option<axiom_model::Dir> {
        axiom_engine::purpose_direction(
            self.book().places[from].class != Class::Outside && self.owns(from),
            self.book().places[to].class != Class::Outside && self.owns(to),
            root,
        )
    }

    pub(crate) fn plan(self) -> &'p Plan<'b, 's> {
        self.plan
    }

    /// The display sign for a place from the canonical plan.
    pub fn display_sign(self, place: Id<Place>) -> i64 {
        self.plan.sides().sign(place)
    }

    /// `amount` in the base currency at the lens day's prices; `None` without
    /// a price path. Nothing to price is worth nothing, so an empty holding
    /// never counts as unpriced.
    pub fn value(self, amount: Amount) -> Option<Qty> {
        self.exact(amount).and_then(rounded)
    }

    /// The same, in millionths of a base quantum, so that values can be added
    /// before anything is rounded.
    fn exact(self, amount: Amount) -> Option<i128> {
        let book = self.book();
        if amount.unit == book.base || amount.qty.is_zero() {
            return Some(i128::from(amount.qty.0) * POW10[EXTRA_DIGITS]);
        }
        let rate = book.prices.rate(amount.unit, book.base, self.day, book.base)?;
        let (from, to) = (book.commodities[amount.unit].scale, book.commodities[book.base].scale);
        let numerator = i128::from(rate.num()) * POW10[usize::from(to) + EXTRA_DIGITS];
        mul_div(amount.qty.0.into(), numerator, i128::from(rate.den()) * POW10[usize::from(from)])
    }

    fn is_currency(self, unit: Id<Commodity>) -> bool {
        let book = self.book();
        unit == book.base || self.known().currency.is_some_and(|kind| book.is_a(book.commodities[unit].kind, kind))
    }

    /// How spendable `unit` is in `place`, from what kind of place it is and
    /// what kind of thing it is. Only assets are spendable at all.
    pub fn liquidity(self, place: Id<Place>, unit: Id<Commodity>) -> Option<Liquidity> {
        let book = self.book();
        let place = &book.places[place];
        if place.class != Class::Asset {
            return None;
        }
        if place.claim {
            return Some(Liquidity::Claim);
        }
        let quick = |span: Option<Span>| span.is_none_or(|span| span == Span::default());
        let unit_span = book.commodities[unit].liquidity;
        if self.is_currency(unit) && !place.deferred && quick(place.liquidity) && quick(unit_span) {
            return Some(Liquidity::Cash);
        }
        // Whichever is slower, the place or the commodity, sets the pace.
        let span = |span: Option<Span>| span.unwrap_or_default();
        let (by_place, by_unit) = (span(place.liquidity), span(unit_span));
        Some(Liquidity::Slow(if self.day.add(by_place) >= self.day.add(by_unit) { by_place } else { by_unit }))
    }

    /// What is in hand in a holding: its plain money and the parcels tied to no one.
    pub fn free(self, holding: &Holding) -> Qty {
        holding.plain + holding.lots.iter().filter(|lot| lot.tied.is_none()).map(|lot| lot.qty).sum::<Qty>()
    }
}

/// Values are added in units this many digits finer than the base currency's
/// quantum, and rounded once at the end.
const EXTRA_DIGITS: usize = 6;

/// A value in millionths of a quantum, rounded half to even to whole quanta.
fn rounded(exact: i128) -> Option<Qty> {
    div_round(exact, POW10[EXTRA_DIGITS]).and_then(|whole| i64::try_from(whole).ok()).map(Qty)
}

/// Only owned assets and debts form the balance sheet; purpose-classified
/// outside flows are reported on the activity statement instead.
pub fn on_balance_sheet(class: Class) -> bool {
    matches!(class, Class::Asset | Class::Debt)
}

/// Quantities of several commodities: what a place, or a whole subtree, holds.
#[derive(Clone, Default, Debug)]
pub struct Basket(BTreeMap<Id<Commodity>, Held>);

impl Basket {
    pub fn add(&mut self, unit: Id<Commodity>, held: Held) {
        *self.0.entry(unit).or_default() += held;
    }

    pub fn merge(&mut self, other: &Basket) {
        for (&unit, &held) in &other.0 {
            self.add(unit, held);
        }
    }

    pub fn get(&self, unit: Id<Commodity>) -> Qty {
        self.0.get(&unit).map_or(Qty::ZERO, |held| held.qty)
    }

    /// The commodities held, without those that net to nothing.
    pub fn amounts(&self) -> impl Iterator<Item = Amount> + '_ {
        self.0.iter().filter(|(_, held)| !held.qty.is_zero()).map(|(&unit, held)| Amount::new(held.qty, unit))
    }

    /// Everything priceable summed in the base currency, each commodity priced
    /// once as a whole; the rest listed apart. Places of `class` that are not
    /// on the balance sheet are worth what their flows booked, never a new price.
    pub fn value(&self, lens: Lens, class: Class) -> Valued {
        let (mut valued, mut exact) = (Valued::default(), 0);
        for amount in self.amounts() {
            let booked = i128::from(self.0[&amount.unit].booked.0) * POW10[EXTRA_DIGITS];
            match if on_balance_sheet(class) { lens.exact(amount) } else { Some(booked) } {
                Some(worth) => {
                    exact += worth;
                    valued.priced += 1;
                }
                None => valued.unpriced.push(amount),
            }
        }
        valued.total = rounded(exact).unwrap_or_default();
        valued
    }
}

/// A basket priced in the base currency.
#[derive(Default, Debug)]
pub struct Valued {
    pub total: Qty,
    /// How many commodities went into `total`.
    pub priced: usize,
    /// Commodities with no price path, left out of `total`.
    pub unpriced: Vec<Amount>,
}
