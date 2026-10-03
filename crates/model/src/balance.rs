//! What a statement's split, or header with items, comes to, and whether it can.
//!
//! LANGUAGE §3 says what a split means: its legs are the other side of the header, "their total is the header
//! amount, or the sum of the legs", `...` is the remainder, and an item is carved out of the header's amount, comes
//! on top of it or is taken off it. Lowering makes a flow for each leg and item with the amount it was written as.
//! [`settle`] solves the group those flows are, with the same [`solve`] the fold uses for a promise's, when every
//! amount of it is written out, and puts what it came to back into the flows: constant folding. Such a group is
//! closed, and the fold only posts it. One with a computed amount, `=`, `all` or `?` stays open, with the zero its
//! flows carry meanwhile, and the fold solves it when its first flow lands, asking this module what is asked of the
//! solver and whether the answer adds up, so that the model and the fold judge a split by one rule.
//!
//! That rule is the static conservation check. A split whose parts take more than its total has, or leave some of
//! it that no leg is the remainder of, or count it in another commodity, cannot add up; that is an error in the
//! book, said at the legs, and not money that appears on some day.

use axiom_core::{Diagnostic, Id, Loc, Qty, Run};

use crate::book::{Amount, Book, Commodity};
use crate::journal::Flow;
use crate::law::Fault;
use crate::problem::{self, Unbalanced};
use crate::solve::{Bear, Draw, Drawn, Failed, Line, LiteralEnv, Remainder, Remaining, Solved, solve};
use crate::split::{Expr, Heading, Item, Made, Sign};

/// What a statement's header says it moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Total {
    /// It says no amount, or one that is the book's to say (`=`, `all`, `?`): the parts are what they are.
    Nothing,
    /// It says an amount the fold computes.
    Later,
    /// It says this.
    Is(Remaining),
}

/// A statement's group and the flows it names, as the solver reads them.
pub struct Statement<'a> {
    pub group: &'a Made,
    /// The transaction's own flows, which the group's offsets index.
    pub flows: &'a [Flow],
    /// The header, where it is written.
    pub loc: Loc,
}

impl Statement<'_> {
    /// What the solver is asked about this group: each leg, and each item with whether it comes out of the header.
    /// `unit` is what an item counts in when the header says no amount to count it in.
    pub fn asked(&self, header: Option<Remaining>, unit: Id<Commodity>) -> (Vec<Draw>, Vec<Bear>) {
        let side = self.group.takes_from();
        let draws = self
            .group
            .legs
            .iter()
            .map(|leg| Draw { part: leg.part, side, unit: self.flows[leg.flow as usize].out.unit })
            .collect();
        // An exchange has two amounts and an item is carved from neither: its costs are the fold's, as they always were.
        let carves = header.is_some_and(|header| header.out.unit == header.arrive.unit);
        let unit = header.map_or(unit, |header| header.of(side).unit);
        let bears = self
            .group
            .items
            .iter()
            .map(|item| Bear { amount: item.amount, side, unit, takes: carves && takes(item, unit) })
            .collect();
        (draws, bears)
    }

    /// Why the group cannot add up, if it cannot: a split's legs are all of its total, so what they and the items
    /// leave must be nothing, or taken by a remainder that is not itself negative; a header with items keeps what
    /// the items leave, which may not be less than nothing. Judged only of an exact `solved`.
    pub fn imbalance(&self, book: &Book<'_>, header: Remaining, solved: &Solved) -> Option<Diagnostic> {
        let how = self.unbalanced(solved)?;
        let unit = header.of(self.group.takes_from()).unit;
        let legs = self.group.legs.iter().zip(solved.legs.iter()).filter_map(|(leg, drawn)| match drawn {
            Drawn::Value(resolved) => Some((self.flows[leg.flow as usize].loc, resolved.amount)),
            Drawn::Rest(amount) => Some((self.flows[leg.flow as usize].loc, *amount)),
            Drawn::Omitted => None,
        });
        let items = self.group.items.iter().zip(solved.items.iter()).filter_map(|(item, amount)| {
            let amount = (*amount)?;
            takes(item, unit).then_some((item.loc, amount))
        });
        let parts: Vec<(Loc, Amount)> = legs.chain(items).collect();
        let sum: i128 = parts.iter().map(|(_, amount)| i128::from(amount.qty.0)).sum();
        let taken = i64::try_from(sum).map_or_else(
            |_| "more than can be counted".to_owned(),
            |sum| book.show(Amount::new(Qty(sum), unit)).to_string(),
        );
        let bearing = self.group.items.iter().any(|item| takes(item, unit));
        let what = match (self.group.legs.is_empty(), bearing) {
            (_, false) => "legs",
            (true, true) => "items",
            (false, true) => "legs and items",
        };
        let shown: Vec<(Loc, String)> = parts.iter().map(|&(at, amount)| (at, book.show(amount).to_string())).collect();
        let total = book.show(header.of(self.group.takes_from())).to_string();
        Some(problem::split_imbalance(how, what, (self.loc, &total), &taken, &shown))
    }

    fn unbalanced(&self, solved: &Solved) -> Option<Unbalanced> {
        let left = solved.header?.of(self.group.takes_from()).qty.0;
        let rest =
            solved.legs.iter().find_map(|drawn| if let Drawn::Rest(rest) = drawn { Some(rest.qty.0) } else { None });
        match (self.group.header, rest) {
            (Heading::Flow(_), _) => (left < 0).then_some(Unbalanced::Over),
            (Heading::Source { .. }, Some(rest)) => (rest < 0).then_some(Unbalanced::Over),
            (Heading::Source { .. }, None) => match left {
                0 => None,
                left if left > 0 => Some(Unbalanced::Short),
                _ => Some(Unbalanced::Over),
            },
        }
    }

    /// Why the solver could not take what a part takes from the header: where `at` is.
    pub fn fault(&self, book: &Book<'_>, header: Option<Remaining>, at: Line, fault: Fault) -> Diagnostic {
        let loc = match at {
            Line::Leg(index) => self.flows[self.group.legs[index].flow as usize].loc,
            Line::Item(index) => self.group.items[index].loc,
            Line::Header(_) => self.loc,
        };
        match fault {
            Fault::UnitMismatch { found, .. } => {
                let total =
                    header.map_or_else(String::new, |header| book.show(header.of(self.group.takes_from())).to_string());
                problem::split_unit(loc, book.name(book.commodities[found].symbol), (self.loc, &total))
            }
            _ => problem::split_overflow(self.loc),
        }
    }
}

/// Whether a statement's item comes out of the header: a carve that makes a flow of its own, which the flow then
/// carries, and a `Less` that makes none, which is only a smaller header. An `Add` and a `Less` with a flow are
/// beside it, and an item in another commodity than the side it would take from is an exchange of its own.
pub fn takes(item: &Item<Option<u32>>, unit: Id<Commodity>) -> bool {
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

/// Whether the fold has anything left to solve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Settled {
    /// Every amount is in the flows.
    Closed,
    /// The fold solves it when its first flow lands.
    Open,
}

/// Solves the group if everything it says is written out, and says what cannot add up. `flows` are the
/// transaction's own so far.
pub fn settle(
    book: &mut Book<'_>,
    group: &Made,
    flows: Run<Flow>,
    total: Total,
    loc: Loc,
) -> Result<Settled, Diagnostic> {
    let header = match total {
        Total::Later => return Ok(Settled::Open),
        Total::Nothing => None,
        Total::Is(header) => Some(header),
    };
    let solved = {
        let statement = Statement { group, flows: &book.flows[flows], loc };
        let (draws, bears) = statement.asked(header, book.base);
        let solved = match solve(header, &draws, &bears, Remainder::AfterItems, &mut LiteralEnv) {
            Ok(solved) => solved,
            Err(Failed::Env(never)) => match never {},
            Err(Failed::Fault { at, fault }) => return Err(statement.fault(book, header, at, fault)),
            Err(Failed::TwoRests { at }) => return Err(statement.fault(book, header, at, Fault::Overflow)),
        };
        if !solved.exact {
            return Ok(Settled::Open);
        }
        if let Some(problem) = header.and_then(|header| statement.imbalance(book, header, &solved)) {
            return Err(problem);
        }
        solved
    };
    put(book, group, flows.start(), &solved);
    Ok(Settled::Closed)
}

/// Says what the group came to in the flows that make it up.
fn put(book: &mut Book<'_>, group: &Made, first: Id<Flow>, solved: &Solved) {
    let at = |offset: u32| Id::new(first.index() as u32 + offset);
    if let (Heading::Flow(offset), Some(left)) = (group.header, solved.header) {
        let flow = &mut book.flows[at(offset)];
        (flow.out, flow.arrive) = (left.out, left.arrive);
    }
    for (leg, drawn) in group.legs.iter().zip(solved.legs.iter()) {
        let amount = match drawn {
            Drawn::Value(resolved) => resolved.amount,
            Drawn::Rest(amount) => *amount,
            Drawn::Omitted => continue,
        };
        let flow = &mut book.flows[at(leg.flow)];
        (flow.out, flow.arrive) = (amount, amount);
    }
}
