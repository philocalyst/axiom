//! What happened, as posted: the journal's flows with their solved quantities and settlement, in the order the journal
//! has them, and the flows laws derived from them.
//!
//! This is the stream the flow views walk. A flow is a [`Posting`] once the run has said how it came out: what left,
//! what arrived, whether it stands, and which claims it settled. What the postings add up to on a day is not read from
//! here: the fold records every balance as it changes (`Run::histories`), and [`crate::balances`] reads those.
//!
//! A flow a law derived (a card's cash back) is no line of the journal, and is a posting like any other. [`postings`]
//! stays the journal's, so that a view that asks about lines is not told of what no line wrote; [`all_postings`] puts each
//! derived flow right after the flow it came from, where a line that wrote it would be, and the views that list what
//! moved read it.

use std::ops::Range;

use axiom_core::{Day, Id};
use axiom_engine::{Cause, Pad, Posted, Run, Settlement, State};
use axiom_model::{Amount, Book, End, Flow, Offspring, Place};

/// What a posting is of: a line of the journal, or a flow a law derived.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostingId {
    Journal(Id<Flow>),
    Derived(Id<Offspring>),
}

impl PostingId {
    /// The flow of the journal, if it is one.
    pub fn journal(self) -> Option<Id<Flow>> {
        match self {
            PostingId::Journal(flow) => Some(flow),
            PostingId::Derived(_) => None,
        }
    }
}

/// A flow together with what the run made of it.
#[derive(Clone, Copy)]
pub struct Posting<'a> {
    pub id: PostingId,
    pub flow: &'a Flow,
    pub posted: Posted,
    /// The claims the flow settled, if it settled any.
    pub settlement: Option<&'a Settlement>,
}

impl<'a> Posting<'a> {
    pub fn at(book: &'a Book, run: &'a Run, id: Id<Flow>) -> Posting<'a> {
        let settled = run.settlements.binary_search_by_key(&id, |&(flow, _)| flow);
        let settlement = settled.ok().map(|at| &run.settlements[at].1);
        let (id, flow, posted) = (PostingId::Journal(id), &book.flows[id], run.posted[id.index()]);
        Posting { id, flow, posted, settlement }
    }

    /// A flow a law derived. It stands as the flow it descends from does: returned with it, and real from the day it
    /// posted. It settles nothing.
    pub fn derived(run: &'a Run, id: Id<Offspring>) -> Posting<'a> {
        let flow = &run.offspring[id.index()].flow;
        let state = match run.offspring[id.index()].root {
            Cause::Flow(root) => match run.posted[root.index()].state {
                State::Returned(on) => State::Returned(on),
                _ => State::Actual,
            },
            _ => State::Actual,
        };
        let posted = Posted { out: flow.out.qty, arrive: flow.arrive.qty, state };
        Posting { id: PostingId::Derived(id), flow, posted, settlement: None }
    }

    /// Where it stands among the postings of its day: a line by its number, and a flow a law derived right after the line it
    /// came from, in the order it posted. What no line began comes after every line, and nothing comes after a pad.
    pub fn sequence(&self, run: &Run) -> (u32, u32) {
        match self.id {
            PostingId::Journal(id) => (id.index() as u32, 0),
            PostingId::Derived(id) => match run.offspring[id.index()].root {
                Cause::Flow(root) => (root.index() as u32, 1 + id.index() as u32),
                _ => (u32::MAX - 1, id.index() as u32),
            },
        }
    }

    /// The offspring record behind a derived posting.
    pub fn offspring(&self, run: &'a Run) -> Option<&'a Offspring> {
        match self.id {
            PostingId::Derived(id) => run.offspring.get(id.index()),
            PostingId::Journal(_) => None,
        }
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
    book.flows.iter().zip(run.posted.iter()).map(move |((id, flow), &posted)| {
        let settlement = settlements.next_if(|&&(found, _)| found == id).map(|(_, settlement)| settlement);
        Posting { id: PostingId::Journal(id), flow, posted, settlement }
    })
}

/// The journal flow that began what `cause` says: the flow itself, or the one a chain of derived flows started from.
pub fn journal_root(run: &Run, cause: Cause) -> Option<Id<Flow>> {
    match cause {
        Cause::Derived(offspring) => run.offspring.get(offspring.index()).and_then(|offspring| offspring.root.flow()),
        cause => cause.flow(),
    }
}

/// What laws derived from each journal flow: runs of the run's offspring, by the flow they started from. A flow is
/// posted once, and what it derives is posted right after it, so each flow's offspring are one run.
pub struct Derivatives<'a> {
    run: &'a Run,
    /// The runs, by journal flow.
    groups: Vec<(Id<Flow>, Range<usize>)>,
}

impl<'a> Derivatives<'a> {
    pub fn of(run: &'a Run) -> Derivatives<'a> {
        let mut groups: Vec<(Id<Flow>, Range<usize>)> = Vec::new();
        for (at, offspring) in run.offspring.iter().enumerate() {
            let Cause::Flow(root) = offspring.root else { continue };
            match groups.last_mut() {
                Some((last, range)) if *last == root => range.end = at + 1,
                _ => groups.push((root, at..at + 1)),
            }
        }
        groups.sort_unstable_by_key(|(root, _)| *root);
        Derivatives { run, groups }
    }

    /// What was derived from journal flow `flow`, in the order it posted.
    pub fn after(&self, flow: Id<Flow>) -> impl Iterator<Item = Posting<'a>> {
        let found = self.groups.binary_search_by_key(&flow, |(root, _)| *root).ok();
        let run = self.run;
        found
            .into_iter()
            .flat_map(move |at| self.groups[at].1.clone())
            .map(move |at| Posting::derived(run, Id::new(at as u32)))
    }

    /// What was derived from flows that are no line of the journal (an occurrence's, a hypothetical flow), in the order it posted.
    pub fn apart(&self) -> Vec<Posting<'a>> {
        let run = self.run;
        let apart = run.offspring.iter().enumerate().filter(|(_, offspring)| !matches!(offspring.root, Cause::Flow(_)));
        apart.map(|(at, _)| Posting::derived(run, Id::new(at as u32))).collect()
    }
}

/// Every flow that moved, in journal order, each flow a law derived right after the flow it came from. A book no law
/// derived anything in has exactly the journal's [`postings`].
pub fn all_postings<'a>(book: &'a Book, run: &'a Run) -> impl Iterator<Item = Posting<'a>> {
    let derivatives = Derivatives::of(run);
    let apart = derivatives.apart();
    let journal = postings(book, run).flat_map(move |posting| {
        let derived = posting.id.journal().into_iter().flat_map(|flow| derivatives.after(flow)).collect::<Vec<_>>();
        std::iter::once(posting).chain(derived)
    });
    journal.chain(apart)
}
