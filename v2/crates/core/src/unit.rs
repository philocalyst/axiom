//! What an amount is counted in, for type checking.
//!
//! A quantity is of a commodity (`USD`), of a rate between two (`USD/MI`, the
//! price `USD/VTI`), of a rate in time (`USD` a month, what a promise's schedule
//! amount is), or a pure number. That is Kennedy's free abelian group restricted
//! to the shapes books use: anything deeper (`USD` times `MI`) is a type error
//! and not a unit. The commodity is a parameter so that core stays below the
//! model, which says `Dim<Id<Commodity>>`.

use crate::calendar::Period;

/// The dimension of an amount.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dim<C> {
    /// `%`, a count, a fraction.
    Number,
    /// `USD`, `VTI`, `MI`.
    Of(C),
    /// `USD/MI`: multiplied by `MI` it is `USD`.
    Per(C, C),
    /// `USD` a month: what a promise's schedule amount is.
    Rate(C, Period),
    /// Some commodity, known only when the flow is: `amount` where the subject
    /// may hold several. Mixing it with a fixed one needs `value(x, U)`.
    Any,
}

impl<C: Copy + Eq> Dim<C> {
    /// `a + b`, `a - b`, `a < b`: the same dimension, or `None`.
    pub fn add(self, other: Dim<C>) -> Option<Dim<C>> {
        (self == other).then_some(self)
    }

    /// `a * b`: `Per(u, m) * Of(m) = Of(u)`, `Number * x = x`. Nothing else
    /// combines: two commodities multiplied are no unit a book has.
    pub fn mul(self, other: Dim<C>) -> Option<Dim<C>> {
        match (self, other) {
            (Dim::Number, dim) | (dim, Dim::Number) => Some(dim),
            (Dim::Per(unit, per), Dim::Of(of)) | (Dim::Of(of), Dim::Per(unit, per)) if per == of => Some(Dim::Of(unit)),
            _ => None,
        }
    }

    /// `a / b`: `Of(u) / Of(m) = Per(u, m)`, `Of(u) / Per(u, m) = Of(m)`, a
    /// dimension over itself is a `Number`, and over a `Number` it is itself.
    /// "Some commodity" over "some commodity" is not: they may differ.
    pub fn div(self, other: Dim<C>) -> Option<Dim<C>> {
        match (self, other) {
            (dim, Dim::Number) => Some(dim),
            (a, b) if a == b && a != Dim::Any => Some(Dim::Number),
            (Dim::Of(unit), Dim::Of(of)) => Some(Dim::Per(unit, of)),
            (Dim::Of(unit), Dim::Per(over, per)) if unit == over => Some(Dim::Of(per)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type D = Dim<char>;

    const USD: D = Dim::Of('$');
    const MI: D = Dim::Of('m');
    const PER_MI: D = Dim::Per('$', 'm');

    #[test]
    fn only_the_same_dimension_adds() {
        assert_eq!(USD.add(USD), Some(USD));
        assert_eq!(D::Number.add(D::Number), Some(D::Number));
        assert_eq!(D::Any.add(D::Any), Some(D::Any));
        assert_eq!(USD.add(MI), None, "two commodities need value(x, U)");
        assert_eq!(D::Any.add(USD), None, "and so does some commodity with a fixed one");
        assert_eq!(D::Rate('$', Period::Month).add(D::Rate('$', Period::Year)), None);
        assert_eq!(D::Rate('$', Period::Month).add(USD), None);
    }

    #[test]
    fn a_rate_times_its_commodity_is_the_other() {
        assert_eq!(PER_MI.mul(MI), Some(USD), "0.70 USD/MI times 44 MI");
        assert_eq!(MI.mul(PER_MI), Some(USD));
        assert_eq!(PER_MI.mul(USD), None, "USD/MI times USD is no unit");
        assert_eq!(USD.mul(MI), None, "neither is USD times MI");
        assert_eq!(D::Number.mul(USD), Some(USD), "a percent of an amount");
        assert_eq!(USD.mul(D::Number), Some(USD));
        assert_eq!(D::Number.mul(D::Any), Some(D::Any));
        assert_eq!(D::Any.mul(PER_MI), None);
        assert_eq!(D::Rate('$', Period::Year).mul(D::Number), Some(D::Rate('$', Period::Year)));
    }

    #[test]
    fn division_makes_rates_and_takes_them_apart() {
        assert_eq!(USD.div(MI), Some(PER_MI), "150 USD over 3 HR is a price");
        assert_eq!(USD.div(PER_MI), Some(MI), "how many miles a fare buys");
        assert_eq!(USD.div(USD), Some(D::Number));
        assert_eq!(PER_MI.div(PER_MI), Some(D::Number));
        assert_eq!(USD.div(D::Number), Some(USD));
        assert_eq!(D::Any.div(D::Number), Some(D::Any));
        assert_eq!(D::Any.div(D::Any), None, "two amounts of some commodity may not be of the same");
        assert_eq!(D::Number.div(USD), None);
        assert_eq!(PER_MI.div(MI), None);
    }
}
