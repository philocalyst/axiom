//! Prices: quotes by commodity pair and day, and converting through them.

use axiom_core::num::POW10;
use axiom_core::num::mul_div;
use axiom_core::{Arena, Day, Id, Qty, Ratio};

use crate::book::{Amount, Book, Commodity};
use crate::journal::{Flow, Infer, Mode, Prices, Quote};

/// `qty` of a commodity with `from` decimals, valued in a commodity with `to`
/// decimals at `rate` whole units per whole unit, rounded half to even.
pub(crate) fn rescale(qty: Qty, from: u8, to: u8, rate: Ratio) -> Option<Qty> {
    let numerator = i128::from(rate.num()).checked_mul(*POW10.get(to as usize)?)?;
    let denominator = i128::from(rate.den()).checked_mul(*POW10.get(from as usize)?)?;
    let scaled = mul_div(i128::from(qty.0), numerator, denominator)?;
    i64::try_from(scaled).ok().map(Qty)
}

/// `price × qty ÷ stated`: what a stated exchange implies, as whole quote
/// units per whole priced unit.
pub(crate) fn implied_rate(commodities: &Arena<Commodity>, priced: Amount, quoted: Amount) -> Option<Ratio> {
    let (ps, qs) = (commodities[priced.unit].scale, commodities[quoted.unit].scale);
    let numerator = i128::from(quoted.qty.0).checked_mul(*POW10.get(ps as usize)?)?;
    let denominator = i128::from(priced.qty.0).checked_mul(*POW10.get(qs as usize)?)?;
    Ratio::new(numerator, denominator)
}

impl Prices {
    /// Sorts `quotes`. On the same pair and day, written prices beat implied
    /// ones and later declarations beat earlier ones.
    pub fn new(mut quotes: Vec<Quote>) -> Prices {
        // Stable, with the winner last: lookups take the last quote of a day.
        quotes.sort_by_key(|quote| (quote.unit, quote.quote, quote.day, !quote.implied));
        Prices { quotes }
    }

    /// Whole `quote` units per whole `unit` on `day`: the latest quote at or
    /// before it, used directly, inverted, or through `via` (the base).
    pub fn rate(&self, unit: Id<Commodity>, quote: Id<Commodity>, day: Day, via: Id<Commodity>) -> Option<Ratio> {
        if unit == quote {
            return Some(Ratio::ONE);
        }
        self.direct(unit, quote, day).or_else(|| {
            let (to_via, from_via) = (self.direct(unit, via, day)?, self.direct(via, quote, day)?);
            to_via.checked_mul(from_via)
        })
    }

    /// The latest quote between the pair on or before `day`, in either
    /// direction; the more recent of the two when both exist.
    fn direct(&self, unit: Id<Commodity>, quote: Id<Commodity>, day: Day) -> Option<Ratio> {
        let forward = self.latest(unit, quote, day).map(|found| (found.day, found.rate));
        let backward = self.latest(quote, unit, day).and_then(|found| Some((found.day, found.rate.recip()?)));
        match (forward, backward) {
            (Some(a), Some(b)) => Some(if b.0 > a.0 { b.1 } else { a.1 }),
            (a, b) => a.or(b).map(|found| found.1),
        }
    }

    fn latest(&self, unit: Id<Commodity>, quote: Id<Commodity>, day: Day) -> Option<&Quote> {
        let upto = self.quotes.partition_point(|q| (q.unit, q.quote, q.day) <= (unit, quote, day));
        self.quotes[..upto].last().filter(|found| found.unit == unit && found.quote == quote)
    }
}

/// The price an exchange implies: what one unit of the commodity that is not
/// the base cost in the one that is; else, what the arriving commodity cost.
pub(crate) fn implied_quote(book: &Book, flow: &Flow) -> Option<Quote> {
    if !flow.is_exchange() || flow.infer != Infer::Known || flow.mode == Mode::Planned {
        return None;
    }
    let (priced, quoted) = if flow.out.unit == book.base {
        (flow.arrive, flow.out)
    } else if flow.arrive.unit == book.base {
        (flow.out, flow.arrive)
    } else {
        (flow.arrive, flow.out)
    };
    let rate = implied_rate(&book.commodities, priced, quoted)?;
    Some(Quote { unit: priced.unit, quote: quoted.unit, day: flow.day, rate, implied: true, loc: flow.loc })
}
