//! The solve pass: amounts written `? USD`.
//!
//! An assertion pins a place's balance at the end of a day. Between two
//! consecutive anchors (assertions, or a leg written `= AMOUNT`, and the start
//! of the book, when everything is zero) the balance moves by the sum of the
//! flows in between, so exactly one unknown amount there is the difference.
//! Zero unknowns is nothing to solve; two or more, or an amount only the fold
//! can know (`all`, the far end of a `=` leg) in the same stretch, is
//! reported with the flows that stand in the way.
//!
//! Each place is independent, so places are solved in parallel and merged in
//! place order, which keeps the result deterministic.

use axiom_core::{Diagnostic, Groups, Id, Map, Qty, par};
use axiom_model::{Assert, Book, Commodity, End, Flow, Infer, Place};

use crate::State;
use crate::events::Events;
use crate::motion::Amounts;
use crate::scope::display;
use crate::timeline::{Fact, Moment};

/// Solves every `?` amount it can. Returns the quantities of the flows it
/// solved, and a diagnostic for each real flow it could not.
pub(crate) fn solve(book: &Book, events: &Events) -> (Map<Id<Flow>, Amounts>, Vec<Diagnostic>) {
    let unknown: Vec<Id<Flow>> = book
        .flows
        .iter()
        .filter(|(id, flow)| flow.infer == Infer::Unknown && is_real(events.state(*id, flow)))
        .map(|(id, _)| id)
        .collect();
    if unknown.is_empty() {
        return (Map::default(), Vec::new());
    }
    let mut places: Vec<Id<Place>> = unknown.iter().flat_map(|&id| [book.flows[id].from, book.flows[id].to]).collect();
    places.sort_unstable();
    places.dedup();
    let anchors = Groups::build(book.places.len(), book.asserts.iter().enumerate().map(|(i, a)| (a.place, i as u32)));

    let world = Stretches { book, events, anchors: &anchors };
    let results = par::map(&places, |&place| world.solve_place(place));

    let mut solved: Map<Id<Flow>, Amounts> = Map::default();
    let mut stuck: Map<Id<Flow>, Vec<Stuck>> = Map::default();
    for result in results {
        for Solution { flow, end, qty } in result.solved {
            solved.entry(flow).or_insert_with(|| fill(&book.flows[flow], end, qty));
        }
        for reason in result.stuck {
            stuck.entry(reason.flow).or_default().push(reason);
        }
    }
    let diagnostics = unknown
        .iter()
        .filter(|id| !solved.contains_key(id))
        .map(|&id| cannot_infer(book, id, stuck.get(&id).map_or(&[][..], Vec::as_slice)))
        .collect();
    (solved, diagnostics)
}

fn is_real(state: State) -> bool {
    matches!(state, State::Actual | State::Settled(_) | State::Returned(_))
}

/// The flow's quantities with `qty` at the solved end. A transfer has one
/// quantity, so it fills both.
fn fill(flow: &Flow, end: End, qty: Qty) -> Amounts {
    let mut amounts = Amounts::written(flow);
    if !flow.is_exchange() {
        return Amounts { out: qty, arrive: qty };
    }
    match end {
        End::From => amounts.out = qty,
        End::To => amounts.arrive = qty,
    }
    amounts
}

struct Solution {
    flow: Id<Flow>,
    end: End,
    qty: Qty,
}

struct Stuck {
    flow: Id<Flow>,
    place: Id<Place>,
    why: Why,
}

enum Why {
    /// No anchor follows: nothing says what the balance became.
    NoAnchor,
    /// The assertion that follows accepts a gap (`!`), so it cannot pin an
    /// amount down.
    Accepted,
    /// Other unknown amounts share the stretch.
    Several(Vec<Id<Flow>>),
    /// A quantity only the fold knows shares the stretch.
    Opaque,
    /// The assertions demand a negative amount: the flow runs the other way.
    Negative(Qty),
    /// The flow lands and reverses inside one stretch, so it nets to nothing.
    Cancels,
}

#[derive(Default)]
struct PlaceResult {
    solved: Vec<Solution>,
    stuck: Vec<Stuck>,
}

/// What a place's balance does at one moment, in one commodity.
#[derive(Clone, Copy)]
enum Step {
    Delta(Qty),
    /// `sign`: +1 if the unknown amount arrives here, -1 if it leaves.
    Unknown {
        flow: Id<Flow>,
        end: End,
        sign: i64,
    },
    /// Changes by an amount only the fold can compute.
    Opaque,
    /// The balance after this moment is known.
    Anchor(Qty),
    /// The balance after this moment is asserted with `!`: it starts the next
    /// stretch, but the gap it accepts means it cannot solve the last one.
    Accepted(Qty),
}

/// The read-only tables every place is solved against.
struct Stretches<'a> {
    book: &'a Book<'a>,
    events: &'a Events,
    anchors: &'a Groups<Place, u32>,
}

impl Stretches<'_> {
    fn solve_place(&self, place: Id<Place>) -> PlaceResult {
        let mut units: Vec<Id<Commodity>> = Vec::new();
        for &id in &self.book.touching[place] {
            let flow = &self.book.flows[id];
            if flow.infer == Infer::Unknown && is_real(self.events.state(id, flow)) {
                units.extend(ends(flow, place).map(|(_, unit, _)| unit));
            }
        }
        units.sort_unstable();
        units.dedup();
        let mut result = PlaceResult::default();
        for unit in units {
            self.solve_unit(place, unit, &mut result);
        }
        result
    }

    fn solve_unit(&self, place: Id<Place>, unit: Id<Commodity>, out: &mut PlaceResult) {
        let mut steps = self.steps(place, unit);
        steps.sort_unstable_by_key(|(moment, _)| *moment);
        let (mut before, mut stretch) = (Qty::ZERO, Stretch::default());
        for (_, step) in steps {
            match step {
                Step::Delta(qty) => stretch.known += qty,
                Step::Unknown { flow, end, sign } => stretch.add_unknown(flow, end, sign),
                Step::Opaque => stretch.opaque = true,
                Step::Anchor(after) | Step::Accepted(after) => {
                    let closing = if matches!(step, Step::Accepted(_)) {
                        Closing::Accepted
                    } else {
                        Closing::Exact(after - before)
                    };
                    stretch.close(place, closing, out);
                    (before, stretch) = (after, Stretch::default());
                }
            }
        }
        stretch.close(place, Closing::Open, out);
    }

    /// Every fact that moves or pins `place`'s balance of `unit`.
    fn steps(&self, place: Id<Place>, unit: Id<Commodity>) -> Vec<(Moment, Step)> {
        let mut steps = Vec::new();
        for &id in &self.book.touching[place] {
            let flow = &self.book.flows[id];
            for (moment, direction) in lands(id, flow, self.events.state(id, flow)).into_iter().flatten() {
                for (end, _, sign) in ends(flow, place).filter(|&(_, u, _)| u == unit) {
                    steps.push((moment, self.step_of(place, flow, id, end, sign * direction)));
                }
            }
        }
        for &index in &self.anchors[place] {
            let Assert { day, amount, pad, .. } = self.book.asserts[index as usize];
            if amount.unit == unit {
                let balance = display(self.book, place, amount.qty);
                let step = if pad.is_some() { Step::Accepted(balance) } else { Step::Anchor(balance) };
                steps.push((Moment { day, fact: Fact::Assert(index) }, step));
            }
        }
        steps
    }

    /// What one end of one flow does to the balance. `sign` is +1 for
    /// arriving here and -1 for leaving, negated on a reversal.
    fn step_of(&self, place: Id<Place>, flow: &Flow, id: Id<Flow>, end: End, sign: i64) -> Step {
        let known = match end {
            End::From => flow.out.qty,
            End::To => flow.arrive.qty,
        };
        let forward = sign == if end == End::To { 1 } else { -1 };
        match flow.infer {
            Infer::Known => Step::Delta(Qty(sign * known.0)),
            // Of an exchange written `? USD -> 7 VTI`, only the side left
            // unwritten is unknown.
            Infer::Unknown if known.is_zero() => Step::Unknown { flow: id, end, sign },
            Infer::Unknown => Step::Delta(Qty(sign * known.0)),
            Infer::All if end == End::To && flow.is_exchange() => Step::Delta(Qty(sign * known.0)),
            // A `=` leg pins its own end's balance, written in display sign.
            Infer::Target { end: pinned, balance } if pinned == end && forward => {
                Step::Anchor(display(self.book, place, balance))
            }
            Infer::All | Infer::Target { .. } => Step::Opaque,
        }
    }
}

/// When a flow's value moves, and in which direction: a real flow lands once,
/// a settled one when it settles, and a returned one lands and later reverses.
/// Pending, void and planned flows never move value.
fn lands(id: Id<Flow>, flow: &Flow, state: State) -> [Option<(Moment, i64)>; 2] {
    let on = |day, fact| Moment { day, fact };
    match state {
        State::Actual => [Some((on(flow.day, Fact::Flow(id)), 1)), None],
        State::Settled(day) => [Some((on(day, Fact::Settle(id)), 1)), None],
        State::Returned(day) => [Some((on(flow.day, Fact::Flow(id)), 1)), Some((on(day, Fact::Settle(id)), -1))],
        State::Pending | State::Void | State::Planned => [None, None],
    }
}

/// The ends of `flow` at `place`: which end, in what commodity, and the sign
/// of its effect on the balance.
fn ends(flow: &Flow, place: Id<Place>) -> impl Iterator<Item = (End, Id<Commodity>, i64)> {
    let from = (flow.from == place).then_some((End::From, flow.out.unit, -1));
    let to = (flow.to == place).then_some((End::To, flow.arrive.unit, 1));
    from.into_iter().chain(to)
}

/// The moves between two anchors.
#[derive(Default)]
struct Stretch {
    known: Qty,
    /// Each unknown flow once, with the net sign of its ends here.
    unknowns: Vec<(Id<Flow>, End, i64)>,
    opaque: bool,
}

impl Stretch {
    fn add_unknown(&mut self, flow: Id<Flow>, end: End, sign: i64) {
        match self.unknowns.iter_mut().find(|(f, ..)| *f == flow) {
            Some((_, _, net)) => *net += sign,
            None => self.unknowns.push((flow, end, sign)),
        }
    }

    /// Solves the stretch, or says why it cannot be solved.
    fn close(self, place: Id<Place>, closing: Closing, out: &mut PlaceResult) {
        let stick = |why: &dyn Fn(Id<Flow>) -> Why, out: &mut PlaceResult| {
            out.stuck.extend(self.unknowns.iter().map(|&(flow, ..)| Stuck { flow, place, why: why(flow) }));
        };
        match (closing, self.unknowns.as_slice()) {
            (_, []) => {}
            (Closing::Open, _) => stick(&|_| Why::NoAnchor, out),
            (Closing::Accepted, _) => stick(&|_| Why::Accepted, out),
            (Closing::Exact(_), _) if self.opaque => stick(&|_| Why::Opaque, out),
            (Closing::Exact(_), unknowns @ [_, _, ..]) => stick(
                &|me| Why::Several(unknowns.iter().map(|&(flow, ..)| flow).filter(|&other| other != me).collect()),
                out,
            ),
            (Closing::Exact(moved), &[(flow, end, net)]) => match Qty(net * (moved - self.known).0) {
                _ if net == 0 => out.stuck.push(Stuck { flow, place, why: Why::Cancels }),
                qty if qty.is_negative() => out.stuck.push(Stuck { flow, place, why: Why::Negative(qty) }),
                qty => out.solved.push(Solution { flow, end, qty }),
            },
        }
    }
}

/// What ends a stretch.
enum Closing {
    /// An assertion or `=` leg: the balance moved by this much across it.
    Exact(Qty),
    /// A `!` assertion.
    Accepted,
    /// The end of the book.
    Open,
}

fn cannot_infer(book: &Book, id: Id<Flow>, reasons: &[Stuck]) -> Diagnostic {
    let flow = &book.flows[id];
    let mut d = Diagnostic::error("cannot-infer", "cannot infer the amount of this flow")
        .label(flow.loc, "this amount is unknown");
    for reason in reasons {
        let place = crate::show::place(book, reason.place);
        d = match &reason.why {
            Why::NoAnchor => d.help(format!(
                "assert {place}'s balance after {} so the amount can be solved: `DATE {place} = AMOUNT`",
                flow.day
            )),
            Why::Accepted => d.note(format!(
                "the next assertion on {place} accepts a gap with `!`, so it cannot pin this amount down"
            )),
            Why::Several(others) => {
                for &other in others {
                    d = d.context(book.flows[other].loc, "also unknown");
                }
                d.note(format!("more than one amount is unknown between two assertions on {place}; one equation cannot solve them all"))
            }
            Why::Opaque => d.note(format!(
                "an amount that depends on the running balance (`all` or `=`) shares the stretch on {place}"
            )),
            Why::Negative(qty) => {
                let shown = book.show(axiom_model::Amount::new(*qty, flow.out.unit));
                d.note(format!("the assertions on {place} imply {shown}, which runs this flow backwards; check the assertions or the direction"))
            }
            Why::Cancels => d.note(format!(
                "this flow lands and reverses between the same assertions on {place}, so it changes nothing there"
            )),
        };
    }
    d
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;

    use super::*;
    use crate::fixture::Fixture;

    #[test]
    fn one_unknown_between_assertions_is_the_difference() {
        let mut f = Fixture::new();
        f.flow(1, f.equity, f.checking, 1_000_00);
        f.assert(2, f.checking, 1_000_00);
        let atm = f.unknown(3, f.checking, f.cash);
        f.flow(4, f.checking, f.food, 20_00);
        f.assert(5, f.checking, 700_00);
        let book = f.book();
        let (solved, problems) = solve(&book, &Events::default());
        assert!(problems.is_empty());
        assert_eq!(solved[&atm], Amounts { out: Qty(280_00), arrive: Qty(280_00) });
        let _ = Day(0);
    }

    #[test]
    fn two_unknowns_in_one_stretch_are_reported_together() {
        let mut f = Fixture::new();
        f.flow(1, f.equity, f.checking, 500_00);
        let a = f.unknown(2, f.checking, f.cash);
        let b = f.unknown(3, f.checking, f.food);
        f.assert(4, f.checking, 100_00);
        let book = f.book();
        let (solved, problems) = solve(&book, &Events::default());
        assert!(solved.is_empty());
        assert_eq!(problems.len(), 2);
        assert!(problems[0].labels.iter().any(|l| l.loc == book.flows[b].loc && l.text == "also unknown"));
        assert!(problems[1].labels.iter().any(|l| l.loc == book.flows[a].loc));
    }
}
