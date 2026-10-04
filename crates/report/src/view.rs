//! The context of one view of a run, and one answer, for every view, to "whose is this, what is it worth, and how
//! liquid is it".
//!
//! A [`View`] is a day and an owner scope over the book, the plan the run was folded with, and the run itself. Every
//! view reads all four together, so they travel as one value: a `Copy` of three borrows, which a view makes anew for
//! another day with [`View::on`]. `balance --value`, `available`, `forecast` and the summary all read money through it,
//! so a euro is worth the same in each, and a house is out of reach in each. The three borrows share one lifetime
//! because they are always borrows of one [`Context`](crate::Context): there is no view of a plan and a run that were
//! not made together.

use std::collections::BTreeMap;
use std::iter;

use axiom_core::num::{POW10, div_round, mul_div};
use axiom_core::{Day, Diagnostic, Id, Qty, Span};
use axiom_engine::{Holding, Known, OwnerShare, Plan, Run};
use axiom_model::{Amount, Book, Class, Commodity, Entity, Flow, Place, Subject};

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
        let members = book.entities.ids().filter(|&other| book.member(other) == Some(entity));
        let mut owners: Vec<_> = iter::once(entity).chain(members).collect();
        owners.sort_unstable();
        Whose { owners: Some(owners), label: Some(entity) }
    }

    pub fn includes(&self, entity: Id<Entity>) -> bool {
        self.owners.as_ref().is_none_or(|owners| owners.binary_search(&entity).is_ok())
    }

    /// Whether this view covers every owner in the book.
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

/// The books on one day, seen for one owner scope, as one run folded them.
#[derive(Clone, Copy)]
pub struct View<'b, 's, 'v> {
    pub whose: &'v Whose,
    pub day: Day,
    /// The journal as folded: its postings, holdings, effects and readings.
    pub run: &'v Run,
    /// The exact plan that owns the book view, ownership map and display signs.
    plan: &'v Plan<'b, 's>,
}

impl<'b, 's, 'v> View<'b, 's, 'v> {
    /// Uses the plan's exact book, ownership graph and display signs so a
    /// view cannot pair unrelated arenas or fall back to raw entity owners.
    pub fn new(plan: &'v Plan<'b, 's>, whose: &'v Whose, run: &'v Run, day: Day) -> View<'b, 's, 'v> {
        View { whose, day, run, plan }
    }

    /// The immutable model owned by this view's exact prepared plan.
    pub fn book(&self) -> &'b Book<'s> {
        self.plan.book()
    }

    /// Names and kinds resolved with the same plan as this view.
    pub fn known(&self) -> Known {
        self.plan.known()
    }

    /// The same books at another day's prices.
    pub fn on(self, day: Day) -> View<'b, 's, 'v> {
        View { day, ..self }
    }

    pub fn owns(self, place: Id<Place>) -> bool {
        self.owns_shares(self.plan.owners_of(place))
    }

    pub fn owns_entity(self, entity: Id<Entity>) -> bool {
        self.owns_shares(self.plan.owners_of_entity(entity))
    }

    /// The place a flow's money is counted at: where it arrives if it comes in from outside, else where it leaves, which
    /// is the end the fold counts a flow's own pieces at.
    pub fn movement_place(self, flow: &Flow) -> Id<Place> {
        let places = &self.book().places;
        if places[flow.from].class == Class::Outside && places[flow.to].class != Class::Outside {
            flow.to
        } else {
            flow.from
        }
    }

    /// Whether the flow moves money through a place these owners own.
    pub fn owns_flow(self, flow: &Flow) -> bool {
        self.owns(self.movement_place(flow))
    }

    /// `qty` of what a flow moves, as the owners of the place it moves through own it.
    pub fn flow_qty(self, flow: &Flow, qty: Qty) -> Qty {
        self.place_qty(self.movement_place(flow), qty)
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

    pub(crate) fn plan(self) -> &'v Plan<'b, 's> {
        self.plan
    }

    /// The display sign for a place from the canonical plan.
    pub fn display_sign(self, place: Id<Place>) -> i64 {
        self.plan.sides().sign(place)
    }

    /// `amount` in the base currency at the view day's prices; `None` without
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
        if book.places[place].class != Class::Asset {
            return None;
        }
        if book.is_claim(place) {
            return Some(Liquidity::Claim);
        }
        let quick = |span: Option<Span>| span.is_none_or(|span| span == Span::default());
        let (place_span, unit_span) = (book.liquidity(place), book.liquidity(unit));
        if self.is_currency(unit) && !book.is_deferred(place) && quick(place_span) && quick(unit_span) {
            return Some(Liquidity::Cash);
        }
        // Whichever is slower, the place or the commodity, sets the pace.
        let span = |span: Option<Span>| span.unwrap_or_default();
        let (by_place, by_unit) = (span(place_span), span(unit_span));
        Some(Liquidity::Slow(if self.day + by_place >= self.day + by_unit { by_place } else { by_unit }))
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
pub struct Basket(BTreeMap<Id<Commodity>, Qty>);

impl Basket {
    pub fn add(&mut self, unit: Id<Commodity>, qty: Qty) {
        *self.0.entry(unit).or_default() += qty;
    }

    pub fn merge(&mut self, other: &Basket) {
        for (&unit, &qty) in &other.0 {
            self.add(unit, qty);
        }
    }

    pub fn get(&self, unit: Id<Commodity>) -> Qty {
        self.0.get(&unit).copied().unwrap_or_default()
    }

    /// The commodities held, without those that net to nothing.
    pub fn amounts(&self) -> impl Iterator<Item = Amount> + '_ {
        self.0.iter().filter(|(_, qty)| !qty.is_zero()).map(|(&unit, &qty)| Amount::new(qty, unit))
    }

    /// Everything priceable summed in the base currency, each commodity priced once as a whole; the rest listed apart.
    pub fn value(&self, view: View) -> Valued {
        let (mut valued, mut exact) = (Valued::default(), 0);
        for amount in self.amounts() {
            match view.exact(amount) {
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
