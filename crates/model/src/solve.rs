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
use crate::split::{Cut, Expr, FlowSide, Part, Quantity};

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

    /// What `expr` comes to read against the flow of `at`. `left` is what the header has left, if it has an amount:
    /// an item is read against it, because an item is a part of the header as the legs before it left it.
    fn amount(&mut self, at: Line, left: Option<&Remaining>, expr: Expr) -> Result<Answer, Self::Failure>;

    /// What the contract's own rule says the header's side is: a loan's payment.
    fn payment(&mut self, _at: Line) -> Result<Answer, Self::Failure> {
        Ok(Answer::Later)
    }

    /// What a leg that is the book's to say will move, if this phase reads the book: the gap to an `=`, everything
    /// `all` selects, the amount the assertions solved a `?` to. `None` leaves it the marker it is, with the amount a
    /// flow carries until it lands; that is what a promise does, which is materialized before anything lands.
    fn lands(&mut self, _at: Line, _marker: &Resolved) -> Result<Option<Amount>, Self::Failure> {
        Ok(None)
    }
}

/// The model's environment: a literal is what it says, and a computed amount is for the fold.
pub struct LiteralEnv;

impl Env for LiteralEnv {
    type Failure = Infallible;

    fn amount(&mut self, _at: Line, _left: Option<&Remaining>, expr: Expr) -> Result<Answer, Infallible> {
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
        left: Option<&Remaining>,
        end: End,
        unit: Id<Commodity>,
    ) -> Result<Option<Resolved>, E::Failure> {
        let said = |answer| match answer {
            Answer::Amount(amount) => Some((amount, true)),
            Answer::Later => Some((Amount::zero(unit), false)),
            Answer::Omitted => None,
        };
        let marker = match self {
            Quantity::Amount(expr) => {
                return Ok(said(env.amount(at, left, expr)?).map(|(amount, exact)| Resolved::amount(amount, exact)));
            }
            Quantity::Pending(expr) => {
                let pending =
                    |(amount, exact)| Resolved { mode: Some(Mode::Pending), ..Resolved::amount(amount, exact) };
                return Ok(said(env.amount(at, left, expr)?).map(pending));
            }
            Quantity::Derived => {
                return Ok(said(env.payment(at)?).map(|(amount, exact)| Resolved::amount(amount, exact)));
            }
            Quantity::Target(expr) => said(env.amount(at, left, expr)?)
                .map(|(amount, _)| Resolved::marker(amount, Infer::Target { end, balance: amount.qty })),
            Quantity::Unknown(named) => Some(Resolved::marker(Amount::zero(named), Infer::Unknown)),
            Quantity::All(named) => Some(Resolved::marker(Amount::zero(named.unwrap_or(unit)), Infer::All)),
        };
        let Some(marker) = marker else { return Ok(None) };
        Ok(Some(match env.lands(at, &marker)? {
            Some(amount) => Resolved { amount, exact: true, ..marker },
            None => marker,
        }))
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
    pub amount: Cut,
    pub side: FlowSide,
    /// What the amount is counted in until the environment can say: the zero a flow carries for it.
    pub unit: Id<Commodity>,
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

/// Where the remainder leg sits among what takes from the header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Remainder {
    /// It takes what the legs leave, before any item does: a promise's header is a flow of its own, and keeps what
    /// the items leave of it.
    BeforeItems,
    /// It takes what the legs and the items leave: a split has no header flow, and its total is the sum of what it
    /// pays.
    AfterItems,
}

/// What a group came to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Solved {
    /// What the header has left, if it had an amount.
    pub header: Option<Remaining>,
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

/// What the header has left, and whether everything taken from it was exact, as the solver goes.
struct Account {
    /// What the header was given as.
    given: Option<Remaining>,
    left: Option<Remaining>,
    exact: bool,
}

impl Account {
    /// Takes `amount` from `side` of the header, if it has an amount: a header with none has nothing to take from.
    fn take<E>(&mut self, at: Line, side: FlowSide, amount: Amount) -> Result<(), Failed<E>> {
        match self.left.as_mut() {
            Some(left) => left.take(side, amount).map_err(|fault| Failed::Fault { at, fault }),
            None => Ok(()),
        }
    }

    /// What the header has left on `side`, or nothing in `unit` when it has no amount.
    fn of(&self, side: FlowSide, unit: Id<Commodity>) -> Amount {
        self.left.map_or(Amount::zero(unit), |left| left.of(side))
    }
}

/// Solves a group: every leg's quantity, then every explicit leg carved from the header, then the remainder and
/// the items in the order `settles` says, each item read against the header as it then stands. The order is the
/// contract: an error is the first that order meets, and a computed item sees what the legs and the items before it
/// left.
///
/// A leg that is a share takes it of the header as given, before any leg has carved it. A header with no amount has
/// nothing to carve, so a remainder is nothing and an item bears on nothing: what each leg and item says is what it is.
pub fn solve<E: Env>(
    header: Option<Remaining>,
    legs: &[Draw],
    items: &[Bear],
    settles: Remainder,
    env: &mut E,
) -> Result<Solved, Failed<E::Failure>> {
    let mut account = Account { given: header, left: header, exact: true };
    let mut drawn = draw(&mut account, header, legs, env)?;
    for (index, one) in drawn.iter().enumerate() {
        if let Drawn::Value(resolved) = one {
            account.take(Line::Leg(index), legs[index].side, resolved.amount)?;
        }
    }
    if settles == Remainder::BeforeItems {
        settle(&mut account, legs, &mut drawn)?;
    }
    let amounts = bear(&mut account, items, env)?;
    if settles == Remainder::AfterItems {
        settle(&mut account, legs, &mut drawn)?;
    }
    Ok(Solved { header: account.left, legs: drawn.into(), items: amounts.into(), exact: account.exact })
}

/// What each leg says it takes: its quantity, its share of the header, or the place the remainder goes.
fn draw<E: Env>(
    account: &mut Account,
    header: Option<Remaining>,
    legs: &[Draw],
    env: &mut E,
) -> Result<Vec<Drawn>, Failed<E::Failure>> {
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
                let of = header.map_or(Amount::zero(leg.unit), |header| header.of(leg.side));
                let amount = of.scaled(rate).map_err(|fault| Failed::Fault { at, fault })?;
                Drawn::Value(Resolved { amount, infer: Infer::Known, mode: None, exact: true })
            }
            Part::Of(quantity) => {
                let left = account.left.as_ref();
                match quantity.resolve(env, at, left, leg.side.end(), leg.unit).map_err(Failed::Env)? {
                    Some(resolved) => Drawn::Value(resolved),
                    None => Drawn::Omitted,
                }
            }
        };
        account.exact &= !matches!(one, Drawn::Value(Resolved { exact: false, .. }));
        drawn.push(one);
    }
    Ok(drawn)
}

/// The remainder legs take what the header has left on their side.
fn settle<E>(account: &mut Account, legs: &[Draw], drawn: &mut [Drawn]) -> Result<(), Failed<E>> {
    for (index, one) in drawn.iter_mut().enumerate() {
        if let Drawn::Rest(_) = one {
            let amount = account.of(legs[index].side, legs[index].unit);
            account.take(Line::Leg(index), legs[index].side, amount)?;
            *one = Drawn::Rest(amount);
        }
    }
    Ok(())
}

/// Each item's amount, and what it takes from the header.
fn bear<E: Env>(account: &mut Account, items: &[Bear], env: &mut E) -> Result<Vec<Option<Amount>>, Failed<E::Failure>> {
    let mut amounts = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = Line::Item(index);
        let answer = match item.amount {
            Cut::Of(expr) => env.amount(at, account.left.as_ref(), expr).map_err(Failed::Env)?,
            Cut::Share(rate) => {
                let of = account.given.map_or(Amount::zero(item.unit), |given| given.of(item.side));
                Answer::Amount(of.scaled(rate).map_err(|fault| Failed::Fault { at, fault })?)
            }
        };
        let amount = match answer {
            Answer::Amount(amount) => amount,
            Answer::Omitted => {
                amounts.push(None);
                continue;
            }
            Answer::Later => {
                account.exact = false;
                Amount::zero(item.unit)
            }
        };
        if item.takes {
            account.take(at, item.side, amount)?;
        }
        amounts.push(Some(amount));
    }
    Ok(amounts)
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
        Bear { amount: Cut::Of(Expr::Literal(usd(amount))), side: FlowSide::Out, unit: USD, takes: true }
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
        solve(Some(header), legs, items, Remainder::BeforeItems, &mut LiteralEnv)
    }

    /// An environment that says what a test wants a node to say, and which nodes it was asked about.
    #[derive(Default)]
    struct Says {
        nodes: Vec<(u32, Answer)>,
        asked: Vec<(Line, Expr)>,
    }

    impl Env for Says {
        type Failure = u32;

        fn amount(&mut self, at: Line, left: Option<&Remaining>, expr: Expr) -> Result<Answer, u32> {
            self.asked.push((at, expr));
            Ok(match expr {
                Expr::Literal(amount) => Answer::Amount(amount),
                // Node 99 is a tenth of what the header has left on its out side: the item that reads `amount`.
                Expr::Computed(NodeId(99)) => {
                    Answer::Amount(Amount::new(Qty(left.map_or(0, |left| left.out.qty.0) / 10), USD))
                }
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
        assert_eq!(solved.header, Some(transfer(1_000)));
        assert!(solved.legs.is_empty() && solved.items.is_empty() && solved.exact);
    }

    #[test]
    fn a_leg_takes_its_amount_from_both_sides_of_a_transfer() {
        let solved = literal(transfer(1_000), &[pays(of(usd(300)))], &[]).unwrap();
        assert_eq!(solved.header, Some(transfer(700)));
        assert_eq!(amounts(&solved), [300]);
    }

    #[test]
    fn a_leg_of_an_exchange_leaves_the_other_side_alone() {
        let header = Remaining { out: usd(1_000), arrive: eur(50) };
        let solved = literal(header, &[pays(of(usd(300)))], &[]).unwrap();
        assert_eq!(solved.header, Some(Remaining { out: usd(700), arrive: eur(50) }));
    }

    #[test]
    fn a_share_is_of_the_header_before_any_leg_took_from_it() {
        let tenth = Ratio::new(1, 10).unwrap();
        let legs = [pays(of(usd(500))), pays(Part::Share(tenth)), pays(Part::Share(tenth))];
        let solved = literal(transfer(1_000), &legs, &[]).unwrap();
        assert_eq!(amounts(&solved), [500, 100, 100]);
        assert_eq!(solved.header, Some(transfer(300)));
    }

    #[test]
    fn the_remainder_is_what_every_other_leg_left_even_when_it_is_written_first() {
        let legs = [pays(Part::Rest), pays(of(usd(300))), pays(of(usd(200)))];
        let solved = literal(transfer(1_000), &legs, &[]).unwrap();
        assert_eq!(amounts(&solved), [500, 300, 200]);
        assert_eq!(solved.header, Some(transfer(0)));
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
        let failed = solve(Some(transfer(1_000)), &legs, &[], Remainder::BeforeItems, &mut env).unwrap_err();
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
        assert_eq!(solved.header, Some(transfer(850)));
        assert_eq!(solved.items[..], [Some(usd(30)), Some(usd(40)), Some(usd(20))]);
    }

    #[test]
    fn an_item_sees_what_the_legs_and_the_items_before_it_left() {
        let tenth = Bear { amount: Cut::Of(Expr::Computed(NodeId(99))), side: FlowSide::Out, unit: USD, takes: true };
        let mut env = Says::default();
        let solved =
            solve(Some(transfer(1_000)), &[pays(of(usd(500)))], &[tenth, tenth], Remainder::BeforeItems, &mut env)
                .unwrap();
        // a tenth of 500, then a tenth of 450
        assert_eq!(solved.items[..], [Some(usd(50)), Some(usd(45))]);
        assert_eq!(solved.header, Some(transfer(405)));
    }

    #[test]
    fn an_item_that_takes_from_another_side_takes_from_that_side() {
        let header = Remaining { out: usd(1_000), arrive: eur(70) };
        let bear = Bear { amount: Cut::Of(Expr::Literal(eur(20))), side: FlowSide::Arrive, unit: EUR, takes: true };
        let solved = literal(header, &[], &[bear]).unwrap();
        assert_eq!(solved.header, Some(Remaining { out: usd(1_000), arrive: eur(50) }));
    }

    #[test]
    fn a_part_that_reads_an_input_not_bound_is_left_out_and_takes_nothing() {
        let mut env = Says { nodes: vec![(1, Answer::Omitted)], ..Says::default() };
        let missing = pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(1)))));
        let item = Bear { amount: Cut::Of(Expr::Computed(NodeId(1))), side: FlowSide::Out, unit: USD, takes: true };
        let solved =
            solve(Some(transfer(1_000)), &[missing, pays(Part::Rest)], &[item], Remainder::BeforeItems, &mut env)
                .unwrap();
        assert_eq!(solved.legs[..], [Drawn::Omitted, Drawn::Rest(usd(1_000))]);
        assert_eq!(solved.items[..], [None]);
        assert_eq!(solved.header, Some(transfer(0)));
    }

    #[test]
    fn what_the_environment_cannot_say_yet_is_the_zero_a_flow_carries_and_is_not_exact() {
        let computed = Quantity::Amount(Expr::Computed(NodeId(1)));
        let solved = literal(transfer(1_000), &[pays(Part::Of(computed))], &[]).unwrap();
        assert_eq!(amounts(&solved), [0]);
        assert!(!solved.exact);
        let item = Bear { amount: Cut::Of(Expr::Computed(NodeId(1))), side: FlowSide::Out, unit: USD, takes: true };
        assert!(!literal(transfer(1_000), &[], &[item]).unwrap().exact);
        assert_eq!(literal(transfer(1_000), &[], &[item]).unwrap().items[..], [Some(usd(0))]);
    }

    #[test]
    fn a_failure_of_the_environment_stops_the_solve_where_it_happened() {
        let mut env = Says::default();
        let fails = pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(100)))));
        let legs = [pays(of(usd(1))), fails, pays(Part::Of(Quantity::Amount(Expr::Computed(NodeId(101)))))];
        assert_eq!(
            solve(Some(transfer(1_000)), &legs, &[], Remainder::BeforeItems, &mut env).unwrap_err(),
            Failed::Env(100)
        );
        assert_eq!(env.asked.len(), 2);
    }

    #[test]
    fn each_quantity_comes_to_what_a_flow_carries_for_it() {
        let header = transfer(1_000);
        let resolve = |quantity: Quantity| {
            quantity.resolve(&mut LiteralEnv, Line::Leg(0), Some(&header), End::To, USD).unwrap().unwrap()
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

    #[test]
    fn a_header_with_no_amount_has_nothing_to_carve_and_its_remainder_is_nothing() {
        let legs = [pays(of(usd(30))), pays(Part::Rest), pays(of(eur(5)))];
        let solved = solve(None, &legs, &[carve(7)], Remainder::AfterItems, &mut LiteralEnv).unwrap();
        assert_eq!(solved.header, None);
        assert_eq!(solved.legs[..].len(), 3);
        assert_eq!(solved.legs[1], Drawn::Rest(usd(0)));
        assert_eq!(solved.items[..], [Some(usd(7))]);
        assert!(solved.exact);
    }

    #[test]
    fn a_remainder_after_the_items_takes_what_the_items_left_too() {
        let legs = [pays(of(usd(300))), pays(Part::Rest)];
        let after =
            solve(Some(transfer(1_000)), &legs, &[carve(100), carve(50)], Remainder::AfterItems, &mut LiteralEnv);
        let solved = after.unwrap();
        assert_eq!(solved.legs[1], Drawn::Rest(usd(550)));
        assert_eq!(solved.header, Some(transfer(0)));
        // before them, the remainder is 700 and the items take the header below nothing
        let before = literal(transfer(1_000), &legs, &[carve(100), carve(50)]).unwrap();
        assert_eq!(before.legs[1], Drawn::Rest(usd(700)));
        assert_eq!(before.header, Some(transfer(-150)));
    }

    #[test]
    fn an_item_after_the_legs_is_read_against_what_they_left_before_the_remainder_is_settled() {
        let tenth = Bear { amount: Cut::Of(Expr::Computed(NodeId(99))), side: FlowSide::Out, unit: USD, takes: true };
        let legs = [pays(of(usd(500))), pays(Part::Rest)];
        let mut env = Says::default();
        let solved = solve(Some(transfer(1_000)), &legs, &[tenth], Remainder::AfterItems, &mut env).unwrap();
        assert_eq!(solved.items[..], [Some(usd(50))]);
        assert_eq!(solved.legs[1], Drawn::Rest(usd(450)));
    }

    /// An environment that reads the book: `=` is the gap to its balance, `all` is what is held, `?` is solved.
    struct Books;

    impl Env for Books {
        type Failure = Infallible;

        fn amount(&mut self, _: Line, _: Option<&Remaining>, expr: Expr) -> Result<Answer, Infallible> {
            LiteralEnv.amount(Line::Leg(0), None, expr)
        }

        fn lands(&mut self, _: Line, marker: &Resolved) -> Result<Option<Amount>, Infallible> {
            Ok(Some(match marker.infer {
                Infer::Target { balance, .. } => usd(balance.0 - 500),
                Infer::All => usd(120),
                _ => usd(7),
            }))
        }
    }

    #[test]
    fn a_marker_the_environment_reads_is_exact_and_the_remainder_follows_it() {
        let legs = [
            pays(Part::Of(Quantity::Target(Expr::Literal(usd(800))))),
            pays(Part::Of(Quantity::All(None))),
            pays(Part::Of(Quantity::Unknown(USD))),
            pays(Part::Rest),
        ];
        let solved = solve(Some(transfer(1_000)), &legs, &[], Remainder::BeforeItems, &mut Books).unwrap();
        assert_eq!(amounts(&solved), [300, 120, 7, 573]);
        assert!(solved.exact);
        let Drawn::Value(target) = solved.legs[0] else { panic!("a value") };
        assert_eq!(target.infer, Infer::Target { end: End::From, balance: Qty(800) }, "it stays the marker it is");
    }

    #[test]
    fn an_item_that_is_a_share_is_of_the_header_as_it_was_given() {
        let tenth = Ratio::new(1, 10).unwrap();
        let share = Bear { amount: Cut::Share(tenth), side: FlowSide::Out, unit: USD, takes: true };
        let solved = literal(transfer(1_000), &[pays(of(usd(500)))], &[share, share]).unwrap();
        // a tenth of 1,000 each time, not of what the leg and the first item left
        assert_eq!(solved.items[..], [Some(usd(100)), Some(usd(100))]);
        assert_eq!(solved.header, Some(transfer(300)));
        assert!(solved.exact);
        let none = solve(None, &[], &[share], Remainder::AfterItems, &mut LiteralEnv).unwrap();
        assert_eq!(none.items[..], [Some(usd(0))]);
    }

    // ─── The solver against a second account of the same rules, on generated groups ────────────────────────────

    /// What the generator draws from: a small deterministic source, so a failure names its seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }

        fn pick<T: Copy>(&mut self, of: &[T]) -> T {
            of[self.below(of.len() as u64) as usize]
        }
    }

    const UNITS: [Id<Commodity>; 3] = [Id::new(0), Id::new(1), Id::new(2)];

    /// A quantity of the sizes books have, now and then one at the edge of what a quantity can hold.
    fn quantity_of(rng: &mut Rng) -> i64 {
        match rng.below(40) {
            0 => i64::MAX - rng.below(10) as i64,
            1 => i64::MIN + 1 + rng.below(10) as i64,
            2 => 0,
            _ => rng.below(200_000) as i64 - 20_000,
        }
    }

    fn amount_of(rng: &mut Rng, unit: Id<Commodity>) -> Amount {
        let unit = if rng.below(10) == 0 { rng.pick(&UNITS) } else { unit };
        Amount::new(Qty(quantity_of(rng)), unit)
    }

    fn expr_of(rng: &mut Rng, unit: Id<Commodity>) -> Expr {
        if rng.below(10) < 6 {
            Expr::Literal(amount_of(rng, unit))
        } else {
            Expr::Computed(NodeId(rng.below(12) as u32))
        }
    }

    fn quantity_drawn(rng: &mut Rng, unit: Id<Commodity>) -> Quantity {
        match rng.below(20) {
            0..=7 => Quantity::Amount(expr_of(rng, unit)),
            8..=10 => Quantity::Pending(expr_of(rng, unit)),
            11..=13 => Quantity::Target(expr_of(rng, unit)),
            14 => Quantity::Unknown(rng.pick(&UNITS)),
            15 => Quantity::All(None),
            16 => Quantity::All(Some(rng.pick(&UNITS))),
            _ => Quantity::Derived,
        }
    }

    fn share_drawn(rng: &mut Rng) -> Ratio {
        match rng.below(30) {
            0 => Ratio::new(i64::MAX as i128 / 3, 1).unwrap(),
            _ => Ratio::new(rng.below(150) as i128, 100).unwrap(),
        }
    }

    struct Group {
        header: Option<Remaining>,
        legs: Vec<Draw>,
        items: Vec<Bear>,
        settles: Remainder,
    }

    fn group_drawn(rng: &mut Rng) -> Group {
        let unit = UNITS[0];
        let other = if rng.below(4) == 0 { UNITS[1] } else { unit };
        let header = (rng.below(10) != 0)
            .then(|| Remaining { out: amount_of(rng, unit), arrive: Amount::new(Qty(quantity_of(rng)), other) });
        let legs = (0..rng.below(6))
            .map(|_| {
                let side = if rng.below(10) < 6 { FlowSide::Out } else { FlowSide::Arrive };
                let unit = header.map_or(unit, |header| header.of(side).unit);
                let part = match rng.below(20) {
                    0..=3 => Part::Rest,
                    4..=7 => Part::Share(share_drawn(rng)),
                    _ => Part::Of(quantity_drawn(rng, unit)),
                };
                Draw { part, side, unit }
            })
            .collect();
        let items = (0..rng.below(5))
            .map(|_| {
                let side = if rng.below(10) < 7 { FlowSide::Out } else { FlowSide::Arrive };
                let unit = header.map_or(unit, |header| header.of(side).unit);
                let amount = if rng.below(8) == 0 { Cut::Share(share_drawn(rng)) } else { Cut::Of(expr_of(rng, unit)) };
                Bear { amount, side, unit, takes: rng.below(2) == 0 }
            })
            .collect();
        let settles = if rng.below(3) == 0 { Remainder::AfterItems } else { Remainder::BeforeItems };
        Group { header, legs, items, settles }
    }

    /// An environment whose answers are a function of what is asked and the seed, and which keeps what it was asked.
    struct Scripted {
        seed: u64,
        asked: Vec<(Line, Option<Remaining>, Expr)>,
    }

    impl Env for Scripted {
        type Failure = u8;

        fn amount(&mut self, at: Line, left: Option<&Remaining>, expr: Expr) -> Result<Answer, u8> {
            self.asked.push((at, left.copied(), expr));
            let Expr::Computed(NodeId(node)) = expr else {
                let Expr::Literal(amount) = expr else { unreachable!() };
                return Ok(Answer::Amount(amount));
            };
            let mut rng = Rng(self.seed
                ^ (u64::from(node) << 20)
                ^ (match at {
                    Line::Header(_) => 1,
                    Line::Leg(i) => 100 + i as u64,
                    Line::Item(i) => 200 + i as u64,
                })
                .wrapping_mul(0x9e37_79b9));
            rng.next();
            Ok(match rng.below(100) {
                0..=3 => return Err(rng.below(250) as u8),
                4..=13 => Answer::Omitted,
                14..=23 => Answer::Later,
                // a tenth of what the header has left, when it is read against it
                24..=33 if left.is_some() => {
                    Answer::Amount(Amount::new(Qty(left.unwrap().out.qty.0 / 10), left.unwrap().out.unit))
                }
                _ => Answer::Amount(Amount::new(Qty(rng.below(100_000) as i64), UNITS[rng.below(8).min(1) as usize])),
            })
        }

        fn payment(&mut self, _: Line) -> Result<Answer, u8> {
            self.asked.push((Line::Header(FlowSide::Out), None, Expr::Computed(NodeId(u32::MAX))));
            Ok(if self.seed % 3 == 0 { Answer::Later } else { Answer::Amount(usd(777)) })
        }

        fn lands(&mut self, at: Line, marker: &Resolved) -> Result<Option<Amount>, u8> {
            self.asked.push((at, None, Expr::Computed(NodeId(u32::MAX - 1))));
            Ok((self.seed % 5 == 0).then(|| Amount::new(Qty(marker.amount.qty.0 + 5), marker.amount.unit)))
        }
    }

    /// The same rules written again in a different shape: two running totals in `i128`, where `solve` takes from a
    /// pair of amounts one at a time, and the order of events spelled out as three passes over the legs and one over
    /// the items.
    fn reference(group: &Group, env: &mut Scripted) -> Result<Solved, Failed<u8>> {
        type Totals = Option<[(Id<Commodity>, i128); 2]>;
        fn left_of(totals: &Totals) -> Option<Remaining> {
            totals.map(|t| Remaining {
                out: Amount::new(Qty(t[0].1 as i64), t[0].0),
                arrive: Amount::new(Qty(t[1].1 as i64), t[1].0),
            })
        }
        fn take(totals: &mut Totals, side: FlowSide, amount: Amount, at: Line) -> Result<(), Failed<u8>> {
            let Some(totals) = totals else { return Ok(()) };
            let me = side.index();
            let wrong = |fault| Err(Failed::Fault { at, fault });
            if totals[me].0 != amount.unit {
                return wrong(Fault::UnitMismatch { found: amount.unit, expected: totals[me].0 });
            }
            totals[me].1 -= i128::from(amount.qty.0);
            if i64::try_from(totals[me].1).is_err() {
                return wrong(Fault::Overflow);
            }
            if totals[1 - me].0 == totals[me].0 {
                totals[1 - me].1 -= i128::from(amount.qty.0);
                if i64::try_from(totals[1 - me].1).is_err() {
                    return wrong(Fault::Overflow);
                }
            }
            Ok(())
        }
        fn share_of(base: Amount, rate: Ratio, at: Line) -> Result<Amount, Failed<u8>> {
            base.qty
                .scale(rate)
                .map(|qty| Amount::new(qty, base.unit))
                .ok_or(Failed::Fault { at, fault: Fault::Overflow })
        }
        let given = group.header;
        let mut totals: Totals =
            given.map(|h| [(h.out.unit, i128::from(h.out.qty.0)), (h.arrive.unit, i128::from(h.arrive.qty.0))]);
        let mut exact = true;
        let mut drawn: Vec<Drawn> = Vec::new();
        let mut remainder_on = [false, false];
        for (index, leg) in group.legs.iter().enumerate() {
            let at = Line::Leg(index);
            let one = match leg.part {
                Part::Rest => {
                    if std::mem::replace(&mut remainder_on[leg.side.index()], true) {
                        return Err(Failed::TwoRests { at });
                    }
                    Drawn::Rest(Amount::zero(leg.unit))
                }
                Part::Share(rate) => {
                    let base = given.map_or(Amount::zero(leg.unit), |header| header.of(leg.side));
                    let amount = share_of(base, rate, at)?;
                    Drawn::Value(Resolved { amount, infer: Infer::Known, mode: None, exact: true })
                }
                Part::Of(quantity) => {
                    match quantity
                        .resolve(env, at, left_of(&totals).as_ref(), leg.side.end(), leg.unit)
                        .map_err(Failed::Env)?
                    {
                        Some(resolved) => Drawn::Value(resolved),
                        None => Drawn::Omitted,
                    }
                }
            };
            exact &= !matches!(one, Drawn::Value(Resolved { exact: false, .. }));
            drawn.push(one);
        }
        let carve = |totals: &mut Totals, drawn: &[Drawn]| -> Result<(), Failed<u8>> {
            for (index, one) in drawn.iter().enumerate() {
                if let Drawn::Value(value) = one {
                    take(totals, group.legs[index].side, value.amount, Line::Leg(index))?;
                }
            }
            Ok(())
        };
        let settle = |totals: &mut Totals, drawn: &mut Vec<Drawn>| -> Result<(), Failed<u8>> {
            for index in 0..drawn.len() {
                if let Drawn::Rest(_) = drawn[index] {
                    let (side, unit) = (group.legs[index].side, group.legs[index].unit);
                    let amount = totals
                        .map_or(Amount::zero(unit), |t| Amount::new(Qty(t[side.index()].1 as i64), t[side.index()].0));
                    take(totals, side, amount, Line::Leg(index))?;
                    drawn[index] = Drawn::Rest(amount);
                }
            }
            Ok(())
        };
        carve(&mut totals, &drawn)?;
        if group.settles == Remainder::BeforeItems {
            settle(&mut totals, &mut drawn)?;
        }
        let mut amounts = Vec::new();
        for (index, item) in group.items.iter().enumerate() {
            let at = Line::Item(index);
            let said = match item.amount {
                Cut::Share(rate) => {
                    let base = given.map_or(Amount::zero(item.unit), |header| header.of(item.side));
                    Answer::Amount(share_of(base, rate, at)?)
                }
                Cut::Of(expr) => env.amount(at, left_of(&totals).as_ref(), expr).map_err(Failed::Env)?,
            };
            let amount = match said {
                Answer::Omitted => {
                    amounts.push(None);
                    continue;
                }
                Answer::Later => {
                    exact = false;
                    Amount::zero(item.unit)
                }
                Answer::Amount(amount) => amount,
            };
            if item.takes {
                take(&mut totals, item.side, amount, at)?;
            }
            amounts.push(Some(amount));
        }
        if group.settles == Remainder::AfterItems {
            settle(&mut totals, &mut drawn)?;
        }
        Ok(Solved { header: left_of(&totals), legs: drawn.into(), items: amounts.into(), exact })
    }

    /// 250,000 generated groups: both give the same answer or the same error, ask the environment the same things in
    /// the same order, and what is solved conserves: what each side had is what is left and what was taken.
    #[test]
    fn the_solver_and_a_second_account_of_its_rules_agree_on_generated_groups() {
        const GROUPS: u64 = 250_000;
        let mut seen = std::collections::BTreeMap::<&str, u64>::new();
        let mut note = |form: &'static str| *seen.entry(form).or_default() += 1;
        for seed in 1..=GROUPS {
            let group = group_drawn(&mut Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1));
            let (mut new, mut old) = (Scripted { seed, asked: Vec::new() }, Scripted { seed, asked: Vec::new() });
            let solved = solve(group.header, &group.legs, &group.items, group.settles, &mut new);
            let expected = reference(&group, &mut old);
            assert_eq!(solved, expected, "seed {seed}");
            assert_eq!(new.asked, old.asked, "seed {seed}: the environment is asked the same things in the same order");
            match &solved {
                Ok(solved) => {
                    note(if solved.exact { "ok, exact" } else { "ok, not exact" });
                    for drawn in solved.legs.iter() {
                        note(match drawn {
                            Drawn::Omitted => "leg omitted",
                            Drawn::Value(_) => "leg a value",
                            Drawn::Rest(_) => "leg the remainder",
                        });
                    }
                    for amount in solved.items.iter() {
                        note(if amount.is_some() { "item an amount" } else { "item omitted" });
                    }
                    if let Some(header) = group.header {
                        conserves(&group, header, solved);
                    }
                    note(if group.header.is_some() { "header with an amount" } else { "header with none" });
                }
                Err(Failed::Env(_)) => note("error: the environment's"),
                Err(Failed::Fault { fault: Fault::Overflow, .. }) => note("error: overflow"),
                Err(Failed::Fault { .. }) => note("error: a unit mismatch"),
                Err(Failed::TwoRests { .. }) => note("error: two remainders"),
            }
        }
        for form in [
            "ok, exact",
            "ok, not exact",
            "leg omitted",
            "leg a value",
            "leg the remainder",
            "item an amount",
            "item omitted",
            "header with an amount",
            "header with none",
            "error: the environment's",
            "error: overflow",
            "error: a unit mismatch",
            "error: two remainders",
        ] {
            assert!(
                seen.get(form).copied().unwrap_or(0) >= GROUPS / 200,
                "form `{form}` reached only {:?} times",
                seen.get(form)
            );
        }
        eprintln!("{GROUPS} groups: {seen:?}");
    }

    /// What the legs, the remainders and the items that take took from each side, with what is left, is what the
    /// header had: nothing is made or lost, and a transfer's two sides are taken from together.
    fn conserves(group: &Group, header: Remaining, solved: &Solved) {
        let left = solved.header.expect("a header with an amount has something left");
        let mut taken = [0i128; 2];
        let mut cross = [0i128; 2];
        let mut note = |side: FlowSide, amount: Amount| {
            taken[side.index()] += i128::from(amount.qty.0);
            cross[side.other().index()] += i128::from(amount.qty.0);
        };
        for (leg, drawn) in group.legs.iter().zip(solved.legs.iter()) {
            match drawn {
                Drawn::Value(value) => note(leg.side, value.amount),
                Drawn::Rest(amount) => note(leg.side, *amount),
                Drawn::Omitted => {}
            }
        }
        for (item, amount) in group.items.iter().zip(solved.items.iter()) {
            if let (true, Some(amount)) = (item.takes, amount) {
                note(item.side, *amount);
            }
        }
        for side in [FlowSide::Out, FlowSide::Arrive] {
            let same_unit = header.of(side).unit == header.of(side.other()).unit;
            let removed = taken[side.index()] + if same_unit { cross[side.index()] } else { 0 };
            assert_eq!(
                i128::from(left.of(side).qty.0),
                i128::from(header.of(side).qty.0) - removed,
                "{side:?} conserves"
            );
        }
    }
}
