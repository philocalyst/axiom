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
use axiom_engine::Holding;
use axiom_model::{Amount, Book, Class, Commodity, Entity, Kind, Place};

use crate::history::Held;
use crate::resolve;

/// Whose money a view is about: everyone's, or one entity's, which for a
/// household includes its members'.
#[derive(Clone, Debug, Default)]
pub struct Whose(Option<Vec<Id<Entity>>>);

impl Whose {
    /// `--for NAME`, or everyone. An unknown name is an error with a suggestion.
    pub fn resolve(book: &Book, name: Option<&str>) -> Result<Whose, Diagnostic> {
        let Some(name) = name else { return Ok(Whose(None)) };
        Ok(Whose::of(book, resolve::entity(book, name)?))
    }

    /// One entity's, and its members' if it is a household.
    pub fn of(book: &Book, entity: Id<Entity>) -> Whose {
        let members = book.entities.iter().filter(|(_, other)| other.member == Some(entity)).map(|(id, _)| id);
        Whose(Some(iter::once(entity).chain(members).collect()))
    }

    pub fn includes(&self, entity: Id<Entity>) -> bool {
        self.0.as_ref().is_none_or(|owners| owners.contains(&entity))
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
pub struct Lens<'b, 's> {
    pub book: &'b Book<'s>,
    pub whose: &'b Whose,
    pub day: Day,
}

impl<'b, 's> Lens<'b, 's> {
    pub fn new(book: &'b Book<'s>, whose: &'b Whose, day: Day) -> Lens<'b, 's> {
        Lens { book, whose, day }
    }

    /// The same books at another day's prices.
    pub fn on(self, day: Day) -> Lens<'b, 's> {
        Lens { day, ..self }
    }

    pub fn owns(self, place: Id<Place>) -> bool {
        self.whose.includes(self.book.places[place].owner)
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
        let book = self.book;
        if amount.unit == book.base || amount.qty.is_zero() {
            return Some(i128::from(amount.qty.0) * POW10[EXTRA_DIGITS]);
        }
        let rate = book.prices.rate(amount.unit, book.base, self.day, book.base)?;
        let (from, to) = (book.commodities[amount.unit].scale, book.commodities[book.base].scale);
        let numerator = i128::from(rate.num()) * POW10[usize::from(to) + EXTRA_DIGITS];
        mul_div(amount.qty.0.into(), numerator, i128::from(rate.den()) * POW10[usize::from(from)])
    }

    fn is_currency(self, unit: Id<Commodity>) -> bool {
        let book = self.book;
        let currency: Option<Id<Kind>> = book.kind("currency").ok();
        unit == book.base || currency.is_some_and(|kind| book.is_a(book.commodities[unit].kind, kind))
    }

    /// How spendable `unit` is in `place`, from what kind of place it is and
    /// what kind of thing it is. Only assets are spendable at all.
    pub fn liquidity(self, place: Id<Place>, unit: Id<Commodity>) -> Option<Liquidity> {
        let (book, place) = (self.book, &self.book.places[place]);
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

/// Assets and liabilities are worth what they fetch today; income, expenses
/// and equity are past events, worth what they were on their own days.
pub fn on_balance_sheet(class: Class) -> bool {
    matches!(class, Class::Asset | Class::Liability)
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
