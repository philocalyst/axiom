//! The pairing rules of LANGUAGE §2, on resolved amounts.
//!
//! *Same commodity* on both sides is a transfer, and stated amounts must be
//! equal. *Different commodities* are an exchange, priced `out ÷ in`; an `@`
//! price with both amounts stated must agree with them, and a difference is
//! never booked silently. In a split, legs in the header commodity take their
//! stated amounts, and a single leg in another commodity receives the
//! remainder as its cost. Nothing here reads a name or a file, so the rules
//! can be checked on numbers alone.

use axiom_core::{Arena, Id, Qty, Ratio};

use crate::book::{Amount, Commodity};
use crate::prices::rescale;

/// `@ 285.70 USD`: whole units of `quote` for one whole unit of what is priced.
#[derive(Clone, Copy, Debug)]
pub(super) struct Price {
    pub rate: Ratio,
    pub quote: Id<Commodity>,
}

/// Why two sides do not make one flow.
#[derive(Debug)]
pub(super) enum Mismatch {
    /// Neither side states an amount.
    Missing,
    /// A transfer whose two sides state different quantities.
    Differ { out: Amount, arrive: Amount },
    /// A price on a flow that keeps one commodity.
    PriceOnTransfer,
    /// The price is in a commodity that neither side uses.
    PriceUnit { price: Price },
    /// The only stated amount is in the price's own commodity, so what is
    /// bought is unknown.
    PriceNeedsTwo { price: Price },
    /// Stated amounts and price disagree.
    Disagree { priced: Amount, stated: Amount, computed: Amount, price: Price },
    /// The priced amount converts to nothing.
    Vanishes { priced: Amount },
    /// The conversion left the range of amounts.
    Overflow,
}

/// `priced` valued at `price`, rounded half to even.
pub(super) fn cost(commodities: &Arena<Commodity>, priced: Amount, price: Price) -> Result<Amount, Mismatch> {
    let (from, to) = (commodities[priced.unit].scale, commodities[price.quote].scale);
    let qty = rescale(priced.qty, from, to, price.rate).ok_or(Mismatch::Overflow)?;
    if qty.is_zero() {
        return Err(Mismatch::Vanishes { priced });
    }
    Ok(Amount::new(qty, price.quote))
}

/// The quantities that leave and arrive, given what a header or leg states.
pub(super) fn pair(
    commodities: &Arena<Commodity>,
    out: Option<Amount>,
    arrive: Option<Amount>,
    price: Option<Price>,
) -> Result<(Amount, Amount), Mismatch> {
    match (out, arrive, price) {
        (None, None, _) => Err(Mismatch::Missing),
        (Some(a), Some(b), None) if a.unit == b.unit => {
            if a.qty == b.qty {
                Ok((a, b))
            } else {
                Err(Mismatch::Differ { out: a, arrive: b })
            }
        }
        (Some(a), Some(b), None) => Ok((a, b)),
        (Some(a), Some(b), Some(_)) if a.unit == b.unit => Err(Mismatch::PriceOnTransfer),
        (Some(a), Some(b), Some(price)) => {
            let (priced, stated) = match price.quote {
                unit if unit == b.unit => (a, b),
                unit if unit == a.unit => (b, a),
                _ => return Err(Mismatch::PriceUnit { price }),
            };
            let computed = cost(commodities, priced, price)?;
            if computed.qty == stated.qty {
                Ok((a, b))
            } else {
                Err(Mismatch::Disagree { priced, stated, computed, price })
            }
        }
        (Some(only), None, None) | (None, Some(only), None) => Ok((only, only)),
        (Some(a), None, Some(price)) if a.unit == price.quote => Err(Mismatch::PriceNeedsTwo { price }),
        (None, Some(b), Some(price)) if b.unit == price.quote => Err(Mismatch::PriceNeedsTwo { price }),
        (Some(a), None, Some(price)) => Ok((a, cost(commodities, a, price)?)),
        (None, Some(b), Some(price)) => Ok((cost(commodities, b, price)?, b)),
    }
}

/// One leg of a split, as far as the pairing rules care.
#[derive(Clone, Copy, Debug)]
pub(super) enum Leg {
    Fixed(Amount, Option<Price>),
    /// `...`
    Rest,
    /// `? UNIT`, `= AMOUNT` or `all`: the engine solves it. Only its
    /// commodity is known.
    Inferred(Id<Commodity>),
}

/// What one leg moves: the amount on the header's end, and on the leg's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Share {
    pub header: Amount,
    pub leg: Amount,
}

/// Why a split does not add up.
#[derive(Debug)]
pub(super) enum SplitError {
    /// Nothing says what commodity the total is in.
    NoCommodity,
    /// Without a header total, every leg must be in one commodity.
    Mixed { at: usize },
    /// More than one `...`; the second.
    ManyRests { at: usize },
    /// Several legs in another commodity, and only one can take the remainder.
    NeedsPrices { at: Vec<usize> },
    /// A `...` and a leg in another commodity both want the remainder.
    TwoRemainders { rest: usize, foreign: usize },
    /// `...` with no header amount to take a remainder of.
    RestNeedsTotal { at: usize },
    /// A remainder cannot be computed while another leg is inferred.
    Unresolvable { at: usize },
    /// The legs use more than the total, or leave nothing for the remainder.
    OverAllocated { total: Amount, allocated: Amount },
    /// The legs use less than the total and nothing takes the rest.
    Short { total: Amount, allocated: Amount },
    /// A leg's own price does not work.
    Priced { at: usize, why: Mismatch },
}

/// Divides the header's `total` among `legs`.
pub(super) fn split(
    commodities: &Arena<Commodity>,
    total: Option<Amount>,
    legs: &[Leg],
) -> Result<Vec<Share>, SplitError> {
    let header_unit = total.map(|total| total.unit).or_else(|| {
        legs.iter().find_map(|leg| match *leg {
            Leg::Fixed(amount, _) => Some(amount.unit),
            Leg::Inferred(unit) => Some(unit),
            Leg::Rest => None,
        })
    });
    let unit = header_unit.ok_or(SplitError::NoCommodity)?;

    let mut shares: Vec<Option<Share>> = vec![None; legs.len()];
    let (mut rests, mut foreign, mut inferred) = (Vec::new(), Vec::new(), false);
    let mut allocated = Qty::ZERO;
    for (at, &leg) in legs.iter().enumerate() {
        let share = match leg {
            Leg::Fixed(amount, None) if amount.unit == unit => Share { header: amount, leg: amount },
            Leg::Fixed(amount, Some(price)) if amount.unit != unit => {
                let header = cost(commodities, amount, price).map_err(|why| SplitError::Priced { at, why })?;
                if header.unit != unit {
                    return Err(SplitError::Priced { at, why: Mismatch::PriceUnit { price } });
                }
                Share { header, leg: amount }
            }
            Leg::Fixed(_, Some(_)) => {
                return Err(SplitError::Priced { at, why: Mismatch::PriceOnTransfer });
            }
            Leg::Fixed(..) if total.is_none() => return Err(SplitError::Mixed { at }),
            Leg::Fixed(..) => {
                foreign.push(at);
                continue;
            }
            Leg::Rest => {
                rests.push(at);
                continue;
            }
            Leg::Inferred(leg_unit) if leg_unit == unit => {
                inferred = true;
                Share { header: Amount::zero(unit), leg: Amount::zero(unit) }
            }
            Leg::Inferred(_) => return Err(SplitError::Mixed { at }),
        };
        allocated += share.header.qty;
        shares[at] = Some(share);
    }

    if let [_, second, ..] = rests[..] {
        return Err(SplitError::ManyRests { at: second });
    }
    if foreign.len() > 1 {
        return Err(SplitError::NeedsPrices { at: foreign });
    }
    if let (&[rest], &[taker]) = (&rests[..], &foreign[..]) {
        return Err(SplitError::TwoRemainders { rest, foreign: taker });
    }

    let Some(total) = total else {
        return match rests.first() {
            Some(&at) => Err(SplitError::RestNeedsTotal { at }),
            None => Ok(shares.into_iter().flatten().collect()),
        };
    };
    let allocated_amount = Amount::new(allocated, unit);
    let remainder = total.qty - allocated;
    match (rests.first().or(foreign.first()), inferred) {
        (None, true) => {}
        (None, false) if remainder.is_zero() => {}
        (None, false) if remainder.is_negative() => {
            return Err(SplitError::OverAllocated { total, allocated: allocated_amount });
        }
        (None, false) => {
            return Err(SplitError::Short { total, allocated: allocated_amount });
        }
        (Some(&at), true) => return Err(SplitError::Unresolvable { at }),
        (Some(_), false) if remainder <= Qty::ZERO => {
            return Err(SplitError::OverAllocated { total, allocated: allocated_amount });
        }
        (Some(&at), false) => {
            let header = Amount::new(remainder, unit);
            let leg = match legs[at] {
                Leg::Fixed(amount, _) => amount,
                Leg::Rest | Leg::Inferred(_) => header,
            };
            shares[at] = Some(Share { header, leg });
        }
    }
    Ok(shares.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use axiom_core::Interner;

    use super::*;
    use crate::prices::implied_rate;

    struct Money {
        commodities: Arena<Commodity>,
        usd: Id<Commodity>,
        vti: Id<Commodity>,
        eur: Id<Commodity>,
    }

    fn money() -> Money {
        let mut names = Interner::default();
        let mut commodities = Arena::new();
        let mut add = |symbol: &'static str, scale: u8| {
            commodities.push(Commodity {
                symbol: names.intern(symbol),
                kind: Id::new(0),
                scale,
                title: None,
                liquidity: None,
                select: None,
                growth: None,
                props: Box::default(),
                doc: None,
                loc: None,
            })
        };
        let (usd, vti, eur) = (add("USD", 2), add("VTI", 0), add("EUR", 2));
        Money { commodities, usd, vti, eur }
    }

    fn usd(m: &Money, cents: i64) -> Amount {
        Amount::new(Qty(cents), m.usd)
    }

    fn vti(m: &Money, shares: i64) -> Amount {
        Amount::new(Qty(shares), m.vti)
    }

    fn price(m: &Money, rate: Ratio) -> Price {
        Price { rate, quote: m.usd }
    }

    #[test]
    fn a_transfer_needs_equal_amounts() {
        let m = money();
        assert!(pair(&m.commodities, Some(usd(&m, 500)), None, None).is_ok());
        assert!(matches!(
            pair(&m.commodities, Some(usd(&m, 500)), Some(usd(&m, 499)), None),
            Err(Mismatch::Differ { .. })
        ));
    }

    #[test]
    fn a_price_completes_an_exchange_and_must_agree_with_stated_amounts() {
        let m = money();
        let p = price(&m, Ratio::new(28570, 100).unwrap());
        // 7 VTI at 285.70 USD is 1,999.90 USD.
        let (out, arrive) = pair(&m.commodities, None, Some(vti(&m, 7)), Some(p)).unwrap();
        assert_eq!((out.qty, arrive.qty), (Qty(199_990), Qty(7)));
        match pair(&m.commodities, Some(usd(&m, 200_000)), Some(vti(&m, 7)), Some(p)) {
            Err(Mismatch::Disagree { stated, computed, .. }) => {
                assert_eq!((stated.qty, computed.qty), (Qty(200_000), Qty(199_990)))
            }
            other => panic!("expected a disagreement, got {other:?}"),
        }
        assert!(matches!(pair(&m.commodities, Some(usd(&m, 100)), None, Some(p)), Err(Mismatch::PriceNeedsTwo { .. })));
    }

    #[test]
    fn implied_rates_are_exact() {
        let m = money();
        let rate = implied_rate(&m.commodities, vti(&m, 7), usd(&m, 200_000)).unwrap();
        assert_eq!(rate, Ratio::new(2000, 7).unwrap());
    }

    #[test]
    fn a_split_gives_the_remainder_to_the_rest_leg() {
        let m = money();
        let legs = [Leg::Fixed(usd(&m, 80_000), None), Leg::Fixed(usd(&m, 91_000), None), Leg::Rest];
        let shares = split(&m.commodities, Some(usd(&m, 520_000)), &legs).unwrap();
        assert_eq!(shares[2].header.qty, Qty(349_000));
    }

    #[test]
    fn a_single_leg_in_another_commodity_costs_the_remainder() {
        let m = money();
        let legs = [Leg::Fixed(vti(&m, 7), None), Leg::Fixed(usd(&m, 500), None)];
        let shares = split(&m.commodities, Some(usd(&m, 200_000)), &legs).unwrap();
        assert_eq!((shares[0].header.qty, shares[0].leg.qty), (Qty(199_500), Qty(7)));
    }

    #[test]
    fn splits_that_do_not_add_up_are_errors() {
        let m = money();
        let total = Some(usd(&m, 1_000));
        let short = [Leg::Fixed(usd(&m, 400), None)];
        assert!(matches!(split(&m.commodities, total, &short), Err(SplitError::Short { .. })));
        let over = [Leg::Fixed(usd(&m, 900), None), Leg::Fixed(usd(&m, 900), None)];
        assert!(matches!(split(&m.commodities, total, &over), Err(SplitError::OverAllocated { .. })));
        let two_rests = [Leg::Rest, Leg::Rest];
        assert!(matches!(split(&m.commodities, total, &two_rests), Err(SplitError::ManyRests { at: 1 })));
        let two_foreign = [Leg::Fixed(vti(&m, 1), None), Leg::Fixed(Amount::new(Qty(5), m.eur), None)];
        assert!(matches!(split(&m.commodities, total, &two_foreign), Err(SplitError::NeedsPrices { .. })));
        let no_total = [Leg::Rest, Leg::Fixed(usd(&m, 1), None)];
        assert!(matches!(split(&m.commodities, None, &no_total), Err(SplitError::RestNeedsTotal { at: 0 })));
    }
}
