//! What a statement's split, or header with items, comes to when every amount of it is written out.
//!
//! LANGUAGE §3 says what a split means: its legs are the other side of the header, "their total is the header
//! amount, or the sum of the legs", `...` is the remainder, and an item is carved out of the header's amount, comes
//! on top of it or is taken off it. Lowering makes a flow for each leg and item with the amount it was written as.
//! This solves the group those flows are, with the same [`solve`] the fold uses for a promise's, and puts what it
//! came to back into the flows: constant folding. A group that is all literal is closed, and the fold only posts
//! it. One with a computed amount, `=`, `all` or `?` stays open, with the zero a flow carries meanwhile, and the
//! fold solves it when its first flow lands.
//!
//! The same arithmetic is the static conservation check. A split whose parts take more than its total has, or
//! leave some of it that no leg is the remainder of, or count it in another commodity, cannot add up; that is an
//! error in the book, said at the legs, and not money that appears on some day.

use std::convert::Infallible;

use axiom_core::{Diagnostic, Id, Loc, Qty};

use crate::book::{Amount, Book, Commodity};
use crate::journal::Flow;
use crate::law::Fault;
use crate::problem::{self, Unbalanced};
use crate::solve::{Bear, Draw, Drawn, Failed, Line, LiteralEnv, Remainder, Remaining, Solved, solve};
use crate::split::{Expr, FlowSide, Heading, Item, Made, Sign};

/// What a statement's header says it moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Total {
    /// It says no amount, or one that is the book's to say (`=`, `all`, `?`): the parts are what they are.
    Nothing,
    /// It says an amount the fold computes.
    Later,
    /// It says this.
    Is(Remaining),
}

/// A statement whose flows are made, and the group they are.
pub(crate) struct Statement<'a> {
    pub group: &'a Made,
    /// The first flow of the statement's transaction, which the group's offsets count from.
    pub first: Id<Flow>,
    pub total: Total,
    /// The side the legs and items take from.
    pub side: FlowSide,
    /// The header, where it is written.
    pub loc: Loc,
}

/// Whether the fold has anything left to solve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Settled {
    /// Every amount is in the flows.
    Closed,
    /// The fold solves it when its first flow lands.
    Open,
}

/// Solves the group if everything it says is written out, and says what cannot add up.
pub(crate) fn settle(book: &mut Book<'_>, statement: Statement<'_>) -> Result<Settled, Diagnostic> {
    let Statement { group, first, total, side, loc } = statement;
    let header = match total {
        Total::Later => return Ok(Settled::Open),
        Total::Nothing => None,
        Total::Is(header) => Some(header),
    };
    let unit = header.map_or(book.base, |header| header.of(side).unit);
    // An exchange has two amounts and an item is carved from neither: its costs are the fold's, as they always were.
    let carves = header.is_some_and(|header| header.out.unit == header.arrive.unit);
    let draws: Vec<Draw> = group
        .legs
        .iter()
        .map(|leg| Draw { part: leg.part, side, unit: book.flows[at(first, leg.flow)].out.unit })
        .collect();
    let bears: Vec<Bear> = group
        .items
        .iter()
        .map(|item| Bear { amount: item.amount, side, unit, takes: carves && takes(item, unit) })
        .collect();
    let solved = match solve(header, &draws, &bears, Remainder::AfterItems, &mut LiteralEnv) {
        Ok(solved) => solved,
        Err(Failed::Env(never)) => match never {},
        Err(failed) => return Err(failure(book, failed, group, first, header, loc)),
    };
    if !solved.exact {
        return Ok(Settled::Open);
    }
    if let (Some(header), Some(how)) = (header, unbalanced(&solved, group, side)) {
        return Err(imbalance(book, how, (group, first, loc), header.of(side), &solved));
    }
    put(book, group, first, &solved);
    Ok(Settled::Closed)
}

fn at(first: Id<Flow>, offset: u32) -> Id<Flow> {
    Id::new(first.index() as u32 + offset)
}

/// Whether a statement's item comes out of the header: a carve that makes a flow of its own, which the flow then
/// carries, and a `Less` that makes none, which is only a smaller header. An `Add` and a `Less` with a flow are
/// beside it, and an item in another commodity than the side it would take from is an exchange of its own.
pub(crate) fn takes(item: &Item<Option<u32>>, unit: Id<Commodity>) -> bool {
    let bears = match item.sign {
        Sign::Carve => item.flow.is_some(),
        Sign::Add => false,
        Sign::Less => item.flow.is_none(),
    };
    bears
        && match item.amount {
            Expr::Literal(amount) => amount.unit == unit,
            Expr::Computed(_) => true,
        }
}

/// What the group's parts cannot do with its total, if they cannot: a split's legs are all of it, so what they and
/// the items leave must be nothing, or taken by a remainder that is not itself negative; a header with items keeps
/// what the items leave, which may not be less than nothing.
fn unbalanced(solved: &Solved, group: &Made, side: FlowSide) -> Option<Unbalanced> {
    let left = solved.header?.of(side).qty.0;
    let rest = solved.legs.iter().find_map(|drawn| if let Drawn::Rest(rest) = drawn { Some(rest.qty.0) } else { None });
    match (group.header, rest) {
        (Heading::Flow(_), _) => (left < 0).then_some(Unbalanced::Over),
        (Heading::Source { .. }, Some(rest)) => (rest < 0).then_some(Unbalanced::Over),
        (Heading::Source { .. }, None) => match left {
            0 => None,
            left if left > 0 => Some(Unbalanced::Short),
            _ => Some(Unbalanced::Over),
        },
    }
}

/// The diagnostic for a group that cannot add up: what took from the header, each where it is written, and how much
/// in all. `total` is what the header says.
fn imbalance(
    book: &Book<'_>,
    how: Unbalanced,
    (group, first, loc): (&Made, Id<Flow>, Loc),
    total: Amount,
    solved: &Solved,
) -> Diagnostic {
    let legs = group.legs.iter().zip(solved.legs.iter()).filter_map(|(leg, drawn)| match drawn {
        Drawn::Value(resolved) => Some((book.flows[at(first, leg.flow)].loc, resolved.amount)),
        Drawn::Rest(amount) => Some((book.flows[at(first, leg.flow)].loc, *amount)),
        Drawn::Omitted => None,
    });
    let items = group.items.iter().zip(solved.items.iter()).filter_map(|(item, amount)| {
        let amount = (*amount)?;
        takes(item, total.unit).then_some((item.loc, amount))
    });
    let parts: Vec<(Loc, Amount)> = legs.chain(items).collect();
    let sum: i128 = parts.iter().map(|(_, amount)| i128::from(amount.qty.0)).sum();
    let taken = i64::try_from(sum).map_or_else(
        |_| "more than can be counted".to_owned(),
        |sum| book.show(Amount::new(Qty(sum), total.unit)).to_string(),
    );
    let bearing = group.items.iter().any(|item| takes(item, total.unit));
    let what = match (group.legs.is_empty(), bearing) {
        (_, false) => "legs",
        (true, true) => "items",
        (false, true) => "legs and items",
    };
    let shown: Vec<(Loc, String)> = parts.iter().map(|&(at, amount)| (at, book.show(amount).to_string())).collect();
    problem::split_imbalance(how, what, (loc, &book.show(total).to_string()), &taken, &shown)
}

/// Why the solver could not take what the group takes from its header.
fn failure(
    book: &Book<'_>,
    failed: Failed<Infallible>,
    group: &Made,
    first: Id<Flow>,
    header: Option<Remaining>,
    loc: Loc,
) -> Diagnostic {
    let where_is = |line: Line| match line {
        Line::Leg(index) => book.flows[at(first, group.legs[index].flow)].loc,
        Line::Item(index) => group.items[index].loc,
        Line::Header(_) => loc,
    };
    match failed {
        Failed::Fault { at, fault: Fault::UnitMismatch { found, .. } } => {
            let total = header.map_or_else(String::new, |header| book.show(header.out).to_string());
            problem::split_unit(where_is(at), book.name(book.commodities[found].symbol), (loc, &total))
        }
        Failed::Fault { .. } | Failed::TwoRests { .. } => problem::split_overflow(loc),
        Failed::Env(never) => match never {},
    }
}

/// Says what the group came to in the flows that make it up.
fn put(book: &mut Book<'_>, group: &Made, first: Id<Flow>, solved: &Solved) {
    if let (Heading::Flow(offset), Some(left)) = (group.header, solved.header) {
        let flow = &mut book.flows[at(first, offset)];
        (flow.out, flow.arrive) = (left.out, left.arrive);
    }
    for (leg, drawn) in group.legs.iter().zip(solved.legs.iter()) {
        let amount = match drawn {
            Drawn::Value(resolved) => resolved.amount,
            Drawn::Rest(amount) => *amount,
            Drawn::Omitted => continue,
        };
        let flow = &mut book.flows[at(first, leg.flow)];
        (flow.out, flow.arrive) = (amount, amount);
    }
}
