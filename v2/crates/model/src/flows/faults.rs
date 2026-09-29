//! What to say when a transaction's sides or legs do not fit together.

use axiom_core::num::POW10;
use axiom_core::{Diagnostic, Loc, Qty, Ratio};

use super::pairing::{Mismatch, Price, SplitError};
use crate::book::Amount;
use crate::declare::World;
use crate::prices::{implied_rate, rescale};

/// Where in the source a pairing was written.
#[derive(Clone, Copy)]
pub(super) struct Written<'a> {
    /// The header, or the leg.
    pub flow: Loc,
    pub price: Option<Loc>,
    /// The two places as written, to phrase a fix in the author's own words.
    pub from: &'a str,
    pub to: &'a str,
}

pub(super) fn mismatch(world: &World, why: Mismatch, at: Written) -> Diagnostic {
    let show = |amount: Amount| world.book.show(amount).to_string();
    let price_loc = at.price.unwrap_or(at.flow);
    match why {
        Mismatch::Missing => Diagnostic::error("missing-amount", "this flow states no amount")
            .label(at.flow, "how much moves?")
            .help("write the amount after the target, as in `checking -> food 84.20 USD`"),
        Mismatch::Differ { out, arrive } => {
            Diagnostic::error("amounts-differ", "the two sides state different amounts")
                .label(at.flow, format!("{} leaves, but {} arrives", show(out), show(arrive)))
                .note("a transfer keeps its amount: value is never created or lost between the two places")
                .help("if the difference is a fee or a rounding, write it as its own leg, or state the amount once")
        }
        Mismatch::PriceOnTransfer => Diagnostic::error("price-on-transfer", "a price needs two different commodities")
            .label(price_loc, "both sides are the same commodity")
            .help("remove the `@` price, or write the commodity that is being bought or sold"),
        Mismatch::PriceUnit { price } => {
            let quote = world.book.name(world.book.commodities[price.quote].symbol);
            Diagnostic::error("price-unit", format!("this price is in {quote}, but neither side is"))
                .label(price_loc, format!("a price in {quote}"))
                .note("`@ PRICE` says what one unit of one side costs in the commodity of the other")
        }
        Mismatch::PriceNeedsTwo { price } => {
            let quote = world.book.name(world.book.commodities[price.quote].symbol);
            Diagnostic::error("price-needs-two", "a price needs the commodity being bought or sold")
                .label(price_loc, format!("this prices something in {quote}, but the amount written is in {quote}"))
                .help("write the other side's amount, as in `checking -> brokerage 7 VTI @ 285.70 USD`")
        }
        Mismatch::Disagree { priced, stated, computed, price } => {
            disagreement(world, priced, stated, computed, price, at)
        }
        Mismatch::Vanishes { priced } => {
            Diagnostic::error("price-vanishes", "at this price the amount is worth nothing")
                .label(price_loc, format!("{} converts to less than the smallest unit", show(priced)))
                .help("state both amounts instead of one amount and a price")
        }
        Mismatch::Overflow => Diagnostic::error("amount-range", "this conversion is too large to count exactly")
            .label(price_loc, "beyond what an amount can hold"),
    }
}

/// `7 VTI × 285.70 USD = 1,999.90 USD, but 2,000.00 USD was written`, with the
/// rate the written amounts imply, and how to write the difference down.
fn disagreement(
    world: &World,
    priced: Amount,
    stated: Amount,
    computed: Amount,
    price: Price,
    at: Written,
) -> Diagnostic {
    let show = |amount: Amount| world.book.show(amount).to_string();
    let commodities = &world.book.commodities;
    let (priced_scale, quote_scale) = (commodities[priced.unit].scale, commodities[stated.unit].scale);
    let one_unit = Qty(POW10[priced_scale as usize] as i64);
    let price_written = rescale(one_unit, priced_scale, quote_scale, price.rate)
        .map_or_else(|| price.rate.to_string(), |qty| show(Amount::new(qty, price.quote)));
    let unit = world.book.name(commodities[priced.unit].symbol);
    let quote = world.book.name(commodities[stated.unit].symbol);
    let difference = Amount::new((stated.qty - computed.qty).abs(), stated.unit);
    let mut diagnostic = Diagnostic::error("price-disagrees", "the price does not match the amounts")
        .label(at.price.unwrap_or(at.flow), "this price")
        .context(at.flow, format!("{} for {}", show(stated), show(priced)))
        .note(format!("{} × {} = {}, but {} was written", show(priced), price_written, show(computed), show(stated),));
    if let Some(rate) = implied_rate(commodities, priced, stated) {
        diagnostic = diagnostic.note(format!(
            "{} ÷ {} = {} {quote} per {unit}, which is not the price written",
            show(stated),
            show(priced),
            truncated(rate, quote_scale as usize + 3),
        ));
    }
    let code = |amount: Amount| show(amount).replace(',', "_");
    diagnostic.help(format!(
        "if the {} difference is a fee or a rounding, write it as a leg:\n  {} -> {}\n    {} {} @ {}\n    expenses/fees {}",
        show(difference),
        at.from,
        code(stated),
        at.to,
        code(priced),
        price_written,
        code(difference),
    ))
}

/// `285.714…`: a positive rate cut after `places` decimals, with `…` if cut.
fn truncated(rate: Ratio, places: usize) -> String {
    let scaled = i128::from(rate.num()) * POW10[places];
    let (digits, cut) = (scaled / i128::from(rate.den()), scaled % i128::from(rate.den()) != 0);
    let whole = digits / POW10[places];
    let fraction = digits % POW10[places];
    format!("{whole}.{fraction:0places$}{}", if cut { "…" } else { "" })
}

pub(super) fn split(world: &World, why: SplitError, header: Loc, legs: &[Loc], ends: &[(&str, &str)]) -> Diagnostic {
    let show = |amount: Amount| world.book.show(amount).to_string();
    match why {
        SplitError::NoCommodity => Diagnostic::error("split-commodity", "nothing says what this split is in")
            .label(header, "no amount here")
            .help("give the header an amount, as in `acme -> 5_200 USD`, or give a leg one"),
        SplitError::Mixed { at } => Diagnostic::error(
            "split-commodity",
            "the legs are in different commodities and there is no total to divide",
        )
        .label(legs[at], "a different commodity")
        .help("state the total in the header, so the legs can be reconciled against it"),
        SplitError::ManyRests { at } => Diagnostic::error("split-rest", "only one leg can be the remainder")
            .label(legs[at], "a second `...`")
            .help("give this leg an amount"),
        SplitError::NeedsPrices { at } => {
            let mut diagnostic = Diagnostic::error(
                "split-price",
                "several legs are in another commodity, and only one can take the remainder as its cost",
            )
            .help("give each of them but one an `@` price, so each cost is known");
            for leg in at {
                diagnostic = diagnostic.label(legs[leg], "needs an `@` price");
            }
            diagnostic
        }
        SplitError::TwoRemainders { rest, foreign } => {
            Diagnostic::error("split-rest", "two legs both want the remainder")
                .label(legs[rest], "`...` takes what is left")
                .label(legs[foreign], "and so does this leg's cost")
                .help("give the leg in the other commodity an `@` price")
        }
        SplitError::RestNeedsTotal { at } => {
            Diagnostic::error("split-total", "`...` is the remainder of a total, but the header states none")
                .label(legs[at], "the remainder of what?")
                .help("add the total to the header, as in `acme -> 5_200 USD`")
        }
        SplitError::Unresolvable { at } => Diagnostic::error(
            "split-inferred",
            "the remainder cannot be computed while another leg's amount is inferred",
        )
        .label(legs[at], "this leg waits for the remainder")
        .note("`? USD`, `= AMOUNT` and `all` are solved later, from balances; a remainder needs known amounts")
        .help("state the inferred leg's amount, or the remainder's"),
        SplitError::OverAllocated { total, allocated } => Diagnostic::error(
            "split-over",
            format!("the legs use {}, but the total is {}", show(allocated), show(total)),
        )
        .label(header, format!("a total of {}", show(total)))
        .note("nothing is left for the remainder, or the legs already exceed the total")
        .help("lower a leg, or raise the total"),
        SplitError::Short { total, allocated } => {
            let missing = Amount::new(total.qty - allocated.qty, total.unit);
            Diagnostic::error(
                "split-short",
                format!("the legs add up to {}, but the total is {}", show(allocated), show(total)),
            )
            .label(header, format!("a total of {}", show(total)))
            .help(format!("add a `...` leg to take the remaining {}", show(missing)))
        }
        SplitError::Priced { at, why } => {
            let written = Written { flow: legs[at], price: None, from: ends[at].0, to: ends[at].1 };
            mismatch(world, why, written)
        }
    }
}
