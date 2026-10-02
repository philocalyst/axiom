//! What a split gives each of its legs, solved once.
//!
//! A group (`split.rs`) is a header, legs that take from it and items under it. Resolving it is one piece of
//! arithmetic: the header says an amount on each of its two sides, each leg takes a quantity, a share of the
//! header, or what the others leave, and an item carves, adds or takes off. [`solve`] is that arithmetic and nothing
//! else. It reads no flow and makes none: the caller says what the group *is* for this occurrence (the template's
//! legs, each replaced by a written one; the items of both) and puts the answer back into flows.
//!
//! # Why an [`Env`], and why it has no balances
//!
//! An amount is a literal or a node of a program, and what a node comes to depends on the phase that asks: the model
//! has nothing to evaluate it against, the fold evaluates it against a flow, a day and the state of the book. The
//! solver is the same in both, so it asks. [`LiteralEnv`] answers [`Answer::Later`] for a node, and the amount keeps
//! the zero a flow carries meanwhile: that is constant folding, and what is left is the fold's.
//!
//! The environment does not say what a place holds. `all` and `=` mean the balance at the moment their flow lands,
//! after the legs before it have landed, and a reversal must undo exactly what was done; a group solved in one go
//! against balances would read them from before its first leg. So they stay what they are, markers
//! ([`Infer::All`], [`Infer::Target`]) with the amount a flow carries until it lands, and the solver says they are not
//! [`exact`](Resolved::exact).
//!
//! # Why the answer is a list
//!
//! A leg's amount is not a number the header leaves: it is a quantity with a mode and a way of being known, and the
//! flow it becomes needs all three. [`Solved`] is one [`Drawn`] per leg in the order given, one amount per item, and
//! what the header has left; a caller that makes a flow of each reads them by position, with no search.

use std::convert::Infallible;

use axiom_core::{Id, Qty, Ratio};

use crate::book::{Amount, Commodity};
use crate::journal::{End, Infer, Mode};
use crate::law::Fault;
use crate::split::{Expr, FlowSide, Part, Quantity};

/// Which flow of a group an expression is read against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    /// One side of the header.
    Header(FlowSide),
    /// The `n`th leg, in the order given to [`solve`].
    Leg(usize),
    /// The `n`th item.
    Item(usize),
}

/// What an expression comes to, as far as the phase that is asked can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Amount(Amount),
    /// It reads an input the occurrence did not bind: whatever it is part of is left out.
    Omitted,
    /// It reads the book as it will be when the flow lands, which this phase cannot.
    Later,
}

/// What a group's expressions are evaluated against: nothing at model time, the fold's state at run time.
pub trait Env {
    /// What evaluating can fail with, in the words of the phase that evaluates.
    type Failure;

    /// What `expr` comes to read against the flow of `at`. `left` is what the header has left: only an item is read
    /// against it, because only an item is a part of the header as the legs before it left it.
    fn amount(&mut self, at: Line, left: &Remaining, expr: Expr) -> Result<Answer, Self::Failure>;

    /// What the contract's own rule says the header's side is: a loan's payment.
    fn payment(&mut self, _at: Line) -> Result<Answer, Self::Failure> {
        Ok(Answer::Later)
    }
}

/// The model's environment: a literal is what it says, and a computed amount is for the fold.
pub struct LiteralEnv;

impl Env for LiteralEnv {
    type Failure = Infallible;

    fn amount(&mut self, _at: Line, _left: &Remaining, expr: Expr) -> Result<Answer, Infallible> {
        Ok(match expr {
            Expr::Literal(amount) => Answer::Amount(amount),
            Expr::Computed(_) => Answer::Later,
        })
    }
}

impl Amount {
    /// `self` times `ratio`, in the same commodity, or the overflow it would be.
    pub fn scaled(self, ratio: Ratio) -> Result<Amount, Fault> {
        self.qty.scale(ratio).map(|qty| Amount::new(qty, self.unit)).ok_or(Fault::Overflow)
    }
}

/// What a header has left on each of its two sides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Remaining {
    pub out: Amount,
    pub arrive: Amount,
}

impl Remaining {
    pub fn of(self, side: FlowSide) -> Amount {
        match side {
            FlowSide::Out => self.out,
            FlowSide::Arrive => self.arrive,
        }
    }

    /// Takes `amount` from `side`, and from the other side too when it counts the same commodity: a transfer has one
    /// magnitude on both, and an exchange keeps its own on the other.
    pub fn take(&mut self, side: FlowSide, amount: Amount) -> Result<(), Fault> {
        let (from, other) = match side {
            FlowSide::Out => (&mut self.out, &mut self.arrive),
            FlowSide::Arrive => (&mut self.arrive, &mut self.out),
        };
        if from.unit != amount.unit {
            return Err(Fault::UnitMismatch { found: amount.unit, expected: from.unit });
        }
        let less = |qty: Qty| qty.0.checked_sub(amount.qty.0).map(Qty).ok_or(Fault::Overflow);
        from.qty = less(from.qty)?;
        if from.unit == other.unit {
            other.qty = less(other.qty)?;
        }
        Ok(())
    }
}

/// What a quantity comes to: the amount a flow carries, and how it is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resolved {
    pub amount: Amount,
    pub infer: Infer,
    /// `Some(Pending)` for an amount written in brackets; otherwise `None`, which says the flow's own mode stands.
    pub mode: Option<Mode>,
    /// Whether `amount` is what the flow will move. It is not for `=`, `all` and `?`, whose amounts are the balance's
    /// to say, nor for an amount that is [`Answer::Later`].
    pub exact: bool,
}

impl Resolved {
    /// An amount a flow will move, if it is not `later`.
    fn amount(amount: Amount, exact: bool) -> Resolved {
        Resolved { amount, infer: Infer::Known, mode: None, exact }
    }

    /// A marker (`=`, `all`, `?`) and the amount a flow carries until it lands.
    fn marker(amount: Amount, infer: Infer) -> Resolved {
        Resolved { amount, infer, mode: None, exact: false }
    }
}

impl Quantity {
    /// What it comes to on side `end` of a flow that counts that side in `unit`: the commodity of `all` when it names
    /// none, and of the zero a flow carries for what is not yet known. None when it reads an input that is not bound.
    pub fn resolve<E: Env>(
        self,
        env: &mut E,
        at: Line,
        left: &Remaining,
        end: End,
        unit: Id<Commodity>,
    ) -> Result<Option<Resolved>, E::Failure> {
        let said = |answer| match answer {
            Answer::Amount(amount) => Some((amount, true)),
            Answer::Later => Some((Amount::zero(unit), false)),
            Answer::Omitted => None,
        };
        Ok(match self {
            Quantity::Amount(expr) => {
                said(env.amount(at, left, expr)?).map(|(amount, exact)| Resolved::amount(amount, exact))
            }
            Quantity::Pending(expr) => said(env.amount(at, left, expr)?)
                .map(|(amount, exact)| Resolved { mode: Some(Mode::Pending), ..Resolved::amount(amount, exact) }),
            Quantity::Target(expr) => said(env.amount(at, left, expr)?)
                .map(|(amount, _)| Resolved::marker(amount, Infer::Target { end, balance: amount.qty })),
            Quantity::Unknown(named) => Some(Resolved::marker(Amount::zero(named), Infer::Unknown)),
            Quantity::All(named) => Some(Resolved::marker(Amount::zero(named.unwrap_or(unit)), Infer::All)),
            Quantity::Derived => said(env.payment(at)?).map(|(amount, exact)| Resolved::amount(amount, exact)),
        })
    }
}

/// A leg as the solver is asked about it: what it takes, from which side of the header, and what its side counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Draw {
    pub part: Part,
    pub side: FlowSide,
    pub unit: Id<Commodity>,
}

/// An item as the solver is asked about it: its amount, and whether it comes out of the header. A carve does, and so
/// does a `Less` that makes no flow of its own; an `Add` never does, and a `Less` with a purpose is its own flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bear {
    pub amount: Expr,
    pub side: FlowSide,
    pub takes: bool,
}

/// What one leg came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drawn {
    /// It reads an input that is not bound: there is no flow for it, and it took nothing.
    Omitted,
    Value(Resolved),
    /// The header's remaining amount on its side, after every other leg and carve had taken theirs.
    Rest(Amount),
}

/// What a group came to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Solved {
    /// What the header has left.
    pub header: Remaining,
    /// One for each leg given, in order.
    pub legs: Box<[Drawn]>,
    /// One for each item given, in order: its amount, or None if it reads an input that is not bound.
    pub items: Box<[Option<Amount>]>,
    /// Whether every amount is exact: only then does `header` say what the flows will leave.
    pub exact: bool,
}

/// Why a group could not be solved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failed<E> {
    /// The environment could not say what an expression comes to.
    Env(E),
    /// Taking from the header, or a share of it, failed where `at` is.
    Fault { at: Line, fault: Fault },
    /// A second leg wants the remainder of a side that another leg already has: it is `at`.
    TwoRests { at: Line },
}

/// Solves a group: every leg's quantity, then every explicit leg carved from the header, then the remainder, then
/// each item against the header as it then stands. The order is the contract: an error is the first that order
/// meets, and a computed item sees what the legs and the items before it left.
///
/// A leg that is a share takes it of the header as given, before any leg has carved it.
pub fn solve<E: Env>(
    header: Remaining,
    legs: &[Draw],
    items: &[Bear],
    env: &mut E,
) -> Result<Solved, Failed<E::Failure>> {
    let mut left = header;
    let mut exact = true;
    let mut drawn = Vec::with_capacity(legs.len());
    let mut rests = [None; 2];
    for (index, leg) in legs.iter().enumerate() {
        let at = Line::Leg(index);
        let one = match leg.part {
            Part::Rest => {
                if rests[leg.side.index()].replace(index).is_some() {
                    return Err(Failed::TwoRests { at });
                }
                Drawn::Rest(Amount::zero(leg.unit))
            }
            Part::Share(rate) => {
                let amount = header.of(leg.side).scaled(rate).map_err(|fault| Failed::Fault { at, fault })?;
                Drawn::Value(Resolved { amount, infer: Infer::Known, mode: None, exact: true })
            }
            Part::Of(quantity) => {
                match quantity.resolve(env, at, &left, leg.side.end(), leg.unit).map_err(Failed::Env)? {
                    Some(resolved) => Drawn::Value(resolved),
                    None => Drawn::Omitted,
                }
            }
        };
        exact &= !matches!(one, Drawn::Value(Resolved { exact: false, .. }));
        drawn.push(one);
    }
    for (index, one) in drawn.iter().enumerate() {
        if let Drawn::Value(resolved) = one {
            left.take(legs[index].side, resolved.amount)
                .map_err(|fault| Failed::Fault { at: Line::Leg(index), fault })?;
        }
    }
    for index in 0..drawn.len() {
        if let Drawn::Rest(_) = drawn[index] {
            let amount = left.of(legs[index].side);
            left.take(legs[index].side, amount).map_err(|fault| Failed::Fault { at: Line::Leg(index), fault })?;
            drawn[index] = Drawn::Rest(amount);
        }
    }
    let mut amounts = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = Line::Item(index);
        let amount = match env.amount(at, &left, item.amount).map_err(Failed::Env)? {
            Answer::Amount(amount) => amount,
            Answer::Omitted => {
                amounts.push(None);
                continue;
            }
            Answer::Later => {
                exact = false;
                Amount::zero(left.of(item.side).unit)
            }
        };
        if item.takes {
            left.take(item.side, amount).map_err(|fault| Failed::Fault { at, fault })?;
        }
        amounts.push(Some(amount));
    }
    Ok(Solved { header: left, legs: drawn.into(), items: amounts.into(), exact })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::law::NodeId;

    const USD: Id<Commodity> = Id::new(0);
    const EUR: Id<Commodity> = Id::new(1);

    fn usd(qty: i64) -> Amount {
        Amount::new(Qty(qty), USD)
    }

    fn eur(qty: i64) -> Amount {
        Amount::new(Qty(qty), EUR)
    }

    fn transfer(qty: i64) -> Remaining {
        Remaining { out: usd(qty), arrive: usd(qty) }
    }

    fn pays(part: Part) -> Draw {
        Draw { part, side: FlowSide::Out, unit: USD }
    }

    fn of(amount: Amount) -> Part {
        Part::Of(Quantity::Amount(Expr::Literal(amount)))
    }

    fn carve(amount: i64) -> Bear {
        Bear { amount: Expr::Literal(usd(amount)), side: FlowSide::Out, takes: true }
    }

    fn amounts(solved: &Solved) -> Vec<i64> {
        solved
            .legs
            .iter()
            .map(|drawn| match drawn {
                Drawn::Value(resolved) => resolved.amount.qty.0,
                Drawn::Rest(amount) => amount.qty.0,
                Drawn::Omitted => i64::MIN,
            })
            .collect()
    }

    fn literal(header: Remaining, legs: &[Draw], items: &[Bear]) -> Result<Solved, Failed<Infallible>> {
        solve(header, legs, items, &mut LiteralEnv)
    }

    /// An environment that says what a test wants a node to say, and which nodes it was asked about.
    #[derive(Default)]
    struct Says {
        nodes: Vec<(u32, Answer)>,
        asked: Vec<(Line, Expr)>,
    }

    impl Env for Says {
        type Failure = u32;

        fn amount(&mut self, at: Line, left: &Remaining, expr: Expr) -> Result<Answer, u32> {
            self.asked.push((at, expr));
            Ok(match expr {
                Expr::Literal(amount) => Answer::Amount(amount),
                // Node 99 is a tenth of what the header has left on its out side: the item that reads `amount`.
                Expr::Computed(NodeId(99)) => Answer::Amount(Amount::new(Qty(left.out.qty.0 / 10), USD)),
                Expr::Computed(NodeId(fails)) if fails >= 100 => return Err(fails),
                Expr::Computed(NodeId(node)) => {
                    self.nodes.iter().find(|(known, _)| *known == node).map_or(Answer::Later, |(_, said)| *said)
                }
            })
        }
    }

    #[test]
    fn a_header_with_nothing_under_it_is_left_whole() {
        let solved = literal(transfer(1_000), &[], &[]).unwrap();
        assert_eq!(solved.header, transfer(1_000));
        assert!(solved.legs.is_empty() && solved.items.is_empty() && solved.exact);
    }

    #[test]
    fn a_leg_takes_its_amount_from_both_sides_of_a_transfer() {
        let solved = literal(transfer(1_000), &[pays(of(usd(300)))], &[]).unwrap();
        assert_eq!(solved.header, transfer(700));
        assert_eq!(amounts(&solved), [300]);
    }

    #[test]
    fn a_leg_of_an_exchange_leaves_the_other_side_alone() {
        let header = Remaining { out: usd(1_000), arrive: eur(50) };
        let solved = literal(header, &[pays(of(usd(300)))], &[]).unwrap();
        assert_eq!(solved.header, Remaining { out: usd(700), arrive: eur(50) });
    }

    #[test]
    fn a_share_is_of_the_header_before_any_leg_took_from_it() {
        let tenth = Ratio::new(1, 10).unwrap();
        let legs = [pays(of(usd(500))), pays(Part::Share(tenth)), pays(Part::Share(tenth))];
        let solved = literal(transfer(1_000), &legs, &[]).unwrap();
        assert_eq!(amounts(&solved), [500, 100, 100]);
        assert_eq!(solved.header, transfer(300));
    }

    #[test]
    fn the_remainder_is_what_every_other_leg_left_even_when_it_is_written_first() {
        let legs = [pays(Part::Rest), pays(of(usd(300))), pays(of(usd(200)))];
        let solved = literal(transfer(1_000), &legs, &[]).unwrap();
        assert_eq!(amounts(&solved), [500, 300, 200]);
        assert_eq!(solved.header, transfer(0));
        assert_eq!(solved.legs[0], Drawn::Rest(usd(500)));
    }

    #[test]
    fn a_remainder_can_be_negative_and_the_legs_are_not_asked_to_conserve() {
        let solved = literal(transfer(100), &[pays(of(usd(300))), pays(Part::Rest)], &[]).unwrap();
        assert_eq!(amounts(&solved), [300, -200]);
    }

    #[test]
    fn a_second_remainder_on_one_side_is_refused_where_it_is_read() {
        let mut env = Says::default();
        let legs = [pays(Part::Rest), pays(Part::Rest), pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(100)))))];
        let failed = solve(transfer(1_000), &legs, &[], &mut env).unwrap_err();
        assert_eq!(failed, Failed::TwoRests { at: Line::Leg(1) });
        assert!(env.asked.is_empty(), "the third leg was never asked about");
    }

    #[test]
    fn a_remainder_on_each_side_is_fine() {
        let legs = [pays(Part::Rest), Draw { side: FlowSide::Arrive, ..pays(Part::Rest) }];
        let header = Remaining { out: usd(1_000), arrive: eur(70) };
        let solved = literal(header, &legs, &[]).unwrap();
        assert_eq!(solved.legs[..], [Drawn::Rest(usd(1_000)), Drawn::Rest(eur(70))]);
    }

    #[test]
    fn a_leg_in_another_commodity_is_a_unit_mismatch_at_that_leg() {
        let legs = [pays(of(usd(100))), pays(of(eur(5))), pays(Part::Rest)];
        let failed = literal(transfer(1_000), &legs, &[]).unwrap_err();
        let fault = Fault::UnitMismatch { found: EUR, expected: USD };
        assert_eq!(failed, Failed::Fault { at: Line::Leg(1), fault });
    }

    #[test]
    fn an_amount_the_header_cannot_hold_is_an_overflow_at_that_leg() {
        let failed = literal(transfer(0), &[pays(of(usd(i64::MAX))), pays(of(usd(i64::MAX)))], &[]).unwrap_err();
        assert_eq!(failed, Failed::Fault { at: Line::Leg(1), fault: Fault::Overflow });
        let huge = Ratio::new(i64::MAX as i128, 1).unwrap();
        let failed = literal(transfer(1_000), &[pays(Part::Share(huge))], &[]).unwrap_err();
        assert_eq!(failed, Failed::Fault { at: Line::Leg(0), fault: Fault::Overflow });
    }

    #[test]
    fn a_carve_takes_from_the_header_and_an_add_does_not() {
        let add = Bear { takes: false, ..carve(40) };
        let solved = literal(transfer(1_000), &[pays(of(usd(100)))], &[carve(30), add, carve(20)]).unwrap();
        assert_eq!(solved.header, transfer(850));
        assert_eq!(solved.items[..], [Some(usd(30)), Some(usd(40)), Some(usd(20))]);
    }

    #[test]
    fn an_item_sees_what_the_legs_and_the_items_before_it_left() {
        let tenth = Bear { amount: Expr::Computed(NodeId(99)), side: FlowSide::Out, takes: true };
        let mut env = Says::default();
        let solved = solve(transfer(1_000), &[pays(of(usd(500)))], &[tenth, tenth], &mut env).unwrap();
        // a tenth of 500, then a tenth of 450
        assert_eq!(solved.items[..], [Some(usd(50)), Some(usd(45))]);
        assert_eq!(solved.header, transfer(405));
    }

    #[test]
    fn an_item_that_takes_from_another_side_takes_from_that_side() {
        let header = Remaining { out: usd(1_000), arrive: eur(70) };
        let bear = Bear { amount: Expr::Literal(eur(20)), side: FlowSide::Arrive, takes: true };
        let solved = literal(header, &[], &[bear]).unwrap();
        assert_eq!(solved.header, Remaining { out: usd(1_000), arrive: eur(50) });
    }

    #[test]
    fn a_part_that_reads_an_input_not_bound_is_left_out_and_takes_nothing() {
        let mut env = Says { nodes: vec![(1, Answer::Omitted)], ..Says::default() };
        let missing = pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(1)))));
        let item = Bear { amount: Expr::Computed(NodeId(1)), side: FlowSide::Out, takes: true };
        let solved = solve(transfer(1_000), &[missing, pays(Part::Rest)], &[item], &mut env).unwrap();
        assert_eq!(solved.legs[..], [Drawn::Omitted, Drawn::Rest(usd(1_000))]);
        assert_eq!(solved.items[..], [None]);
        assert_eq!(solved.header, transfer(0));
    }

    #[test]
    fn what_the_environment_cannot_say_yet_is_the_zero_a_flow_carries_and_is_not_exact() {
        let computed = Quantity::Amount(Expr::Computed(NodeId(1)));
        let solved = literal(transfer(1_000), &[pays(Part::Of(computed))], &[]).unwrap();
        assert_eq!(amounts(&solved), [0]);
        assert!(!solved.exact);
        let item = Bear { amount: Expr::Computed(NodeId(1)), side: FlowSide::Out, takes: true };
        assert!(!literal(transfer(1_000), &[], &[item]).unwrap().exact);
        assert_eq!(literal(transfer(1_000), &[], &[item]).unwrap().items[..], [Some(usd(0))]);
    }

    #[test]
    fn a_failure_of_the_environment_stops_the_solve_where_it_happened() {
        let mut env = Says::default();
        let fails = pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(100)))));
        let legs = [pays(of(usd(1))), fails, pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(101)))))];
        assert_eq!(solve(transfer(1_000), &legs, &[], &mut env).unwrap_err(), Failed::Env(100));
        assert_eq!(env.asked.len(), 2);
    }

    #[test]
    fn each_quantity_comes_to_what_a_flow_carries_for_it() {
        let header = transfer(1_000);
        let resolve = |quantity: Quantity| {
            quantity.resolve(&mut LiteralEnv, Line::Leg(0), &header, End::To, USD).unwrap().unwrap()
        };
        let literal = Expr::Literal(usd(250));
        let amount = resolve(Quantity::Amount(literal));
        assert_eq!(amount, Resolved { amount: usd(250), infer: Infer::Known, mode: None, exact: true });
        let pending = resolve(Quantity::Pending(literal));
        assert_eq!((pending.amount, pending.mode, pending.exact), (usd(250), Some(Mode::Pending), true));
        let target = resolve(Quantity::Target(literal));
        assert_eq!(target.infer, Infer::Target { end: End::To, balance: Qty(250) });
        assert!(!target.exact);
        assert_eq!(resolve(Quantity::Unknown(EUR)).amount, eur(0));
        assert_eq!(resolve(Quantity::Unknown(EUR)).infer, Infer::Unknown);
        assert_eq!(resolve(Quantity::All(None)).amount, usd(0));
        assert_eq!(resolve(Quantity::All(Some(EUR))).amount, eur(0));
        assert_eq!(resolve(Quantity::All(None)).infer, Infer::All);
        assert_eq!(resolve(Quantity::Derived).amount, usd(0));
        assert!(!resolve(Quantity::Derived).exact);
    }

    #[test]
    fn a_target_leg_carves_its_balance_from_the_header_as_though_it_were_an_amount() {
        // What the fold does today, kept: the stand-in of `= 90_000 USD` is its balance.
        let target = pays(Part::Of(Quantity::Target(Expr::Literal(usd(90_000)))));
        let solved = literal(transfer(1_000), &[pays(of(usd(200))), target, pays(Part::Rest)], &[]).unwrap();
        assert_eq!(amounts(&solved), [200, 90_000, -89_200]);
        assert!(!solved.exact);
    }
}
