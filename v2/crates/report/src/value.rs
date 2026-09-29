//! Pricing holdings in the base currency.

use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty};
use axiom_model::{Amount, Book, Commodity};

/// Values amounts in the base currency at the prices of one day.
#[derive(Clone, Copy)]
pub struct Valuer<'b, 's> {
    book: &'b Book<'s>,
    day: Day,
}

impl<'b, 's> Valuer<'b, 's> {
    pub fn new(book: &'b Book<'s>, day: Day) -> Valuer<'b, 's> {
        Valuer { book, day }
    }

    /// `amount` in the base currency; `None` without a price path. Nothing is
    /// priced when there is nothing to price, so an empty holding never counts
    /// as unpriced.
    pub fn value(&self, amount: Amount) -> Option<Amount> {
        if amount.unit == self.book.base || amount.qty.is_zero() {
            return Some(Amount::new(amount.qty, self.book.base));
        }
        self.book.convert(amount, self.book.base, self.day)
    }

    pub fn qty(&self, amount: Amount) -> Option<Qty> {
        self.value(amount).map(|value| value.qty)
    }
}

/// Quantities of several commodities: what a place, or a whole subtree, holds.
#[derive(Clone, Default, Debug)]
pub struct Basket(BTreeMap<Id<Commodity>, Qty>);

impl Basket {
    pub fn add(&mut self, amount: Amount) {
        *self.0.entry(amount.unit).or_default() += amount.qty;
    }

    pub fn get(&self, unit: Id<Commodity>) -> Qty {
        self.0.get(&unit).copied().unwrap_or_default()
    }

    /// The commodities held, without those that net to nothing.
    pub fn iter(&self) -> impl Iterator<Item = Amount> + '_ {
        self.0.iter().filter(|(_, qty)| !qty.is_zero()).map(|(&unit, &qty)| Amount::new(qty, unit))
    }

    /// Everything priceable, summed in the base currency; the rest listed apart.
    pub fn value(&self, valuer: &Valuer) -> Valued {
        let mut valued = Valued::default();
        for amount in self.iter() {
            match valuer.qty(amount) {
                Some(qty) => {
                    valued.total += qty;
                    valued.priced += 1;
                }
                None => valued.unpriced.push(amount),
            }
        }
        valued
    }
}

impl FromIterator<Amount> for Basket {
    fn from_iter<I: IntoIterator<Item = Amount>>(amounts: I) -> Basket {
        let mut basket = Basket::default();
        amounts.into_iter().for_each(|amount| basket.add(amount));
        basket
    }
}

/// A basket priced in the base currency.
#[derive(Default, Debug)]
pub struct Valued {
    pub total: Qty,
    /// How many holdings went into `total`.
    pub priced: usize,
    /// Holdings with no price path, left out of `total`.
    pub unpriced: Vec<Amount>,
}
