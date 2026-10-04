//! What happened, as posted: the journal's flows with their solved quantities and settlement, in the order the journal
//! has them.
//!
//! This is the stream the flow views walk. A flow is a [`Posting`] once the run has said how it came out: what left,
//! what arrived, whether it stands, and which claims it settled. What the postings add up to on a day is not read from
//! here: the fold records every balance as it changes (`Run::histories`), and [`crate::balances`] reads those.

use axiom_core::{Day, Id};
use axiom_engine::{Pad, Posted, Run, Settlement, State};
use axiom_model::{Amount, Book, End, Flow, Place};

/// A journal flow together with what the run made of it.
#[derive(Clone, Copy)]
pub struct Posting<'a> {
    pub id: Id<Flow>,
    pub flow: &'a Flow,
    pub posted: &'a Posted,
    /// The claims the flow settled, if it settled any.
    pub settlement: Option<&'a Settlement>,
}

impl<'a> Posting<'a> {
    pub fn at(book: &'a Book, run: &'a Run, id: Id<Flow>) -> Posting<'a> {
        let settled = run.settlements.binary_search_by_key(&id, |&(flow, _)| flow);
        let settlement = settled.ok().map(|at| &run.settlements[at].1);
        Posting { id, flow: &book.flows[id], posted: &run.posted[id.index()], settlement }
    }

    /// What left `flow.from`, as solved.
    pub fn out(&self) -> Amount {
        Amount::new(self.posted.out, self.flow.out.unit)
    }

    /// What reached `flow.to`, as solved.
    pub fn arrive(&self) -> Amount {
        Amount::new(self.posted.arrive, self.flow.arrive.unit)
    }

    /// Whether the flow has happened, and stands, at the end of `day`.
    pub fn is_real_on(&self, day: Day) -> bool {
        self.flow.day <= day && self.posted.state.is_real_on(day)
    }

    /// Whether the flow is written but not yet real at the end of `day`.
    pub fn is_pending_on(&self, day: Day) -> bool {
        self.flow.day <= day && self.posted.state.is_pending_on(day)
    }

    /// The days on which the flow stands, as the first and the first day past
    /// it: from its own day, or its settlement, until it is returned.
    pub(crate) fn standing(&self) -> Option<(Day, Day)> {
        let forever = Day::MAX;
        match self.posted.state {
            State::Actual => Some((self.flow.day, forever)),
            State::Settled(on) => Some((self.flow.day.max(on), forever)),
            State::Returned(on) => Some((self.flow.day, on)),
            State::Pending | State::Void | State::Planned => None,
        }
    }

    /// The other end of the flow, seen from `place`.
    pub fn counterparty(&self, place: Id<Place>) -> Id<Place> {
        if self.flow.from == place { self.flow.to } else { self.flow.from }
    }

    /// The place at one end of the flow.
    pub fn place(&self, end: End) -> Id<Place> {
        match end {
            End::From => self.flow.from,
            End::To => self.flow.to,
        }
    }

    /// What the flow did at `end`, as solved: what the place lost (negative)
    /// or gained (positive), or, at a `PLACE.basis` end, how far the basis of
    /// its parcels fell or rose.
    pub fn change(&self, end: End) -> Change {
        let amount = match end {
            End::From => Amount::new(-self.posted.out, self.flow.out.unit),
            End::To => self.arrive(),
        };
        Change::Moved(amount)
    }

    /// What happened at `place`: once for each end of the flow that is there.
    pub fn changes_at(self, place: Id<Place>) -> impl Iterator<Item = Change> {
        [End::From, End::To].into_iter().filter(move |&end| self.place(end) == place).map(move |end| self.change(end))
    }
}

/// What a flow did to the place at one of its ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// The place gained (positive) or lost (negative) this much.
    Moved(Amount),
}

/// A pad, seen as the flow it stands for: from its counter place into the
/// place the assertion is about (or out of it, for a negative gap).
pub fn pad_ends(pad: &Pad) -> [(Id<Place>, Amount); 2] {
    let (gain, loss) = (pad.amount, Amount::new(-pad.amount.qty, pad.amount.unit));
    [(pad.place, gain), (pad.counter, loss)]
}

/// Every journal flow with its posting, in journal order.
pub fn postings<'a>(book: &'a Book, run: &'a Run) -> impl Iterator<Item = Posting<'a>> {
    // The settlements are by flow, as the flows are: one walk through both.
    let mut settlements = run.settlements.iter().peekable();
    book.flows.iter().zip(run.posted.iter()).map(move |((id, flow), posted)| {
        let settlement = settlements.next_if(|&&(found, _)| found == id).map(|(_, settlement)| settlement);
        Posting { id, flow, posted, settlement }
    })
}
