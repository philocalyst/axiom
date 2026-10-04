//! When a claim counts as income or spending: LANGUAGE §7, "A claim's purpose is its recognition".
//!
//! A flow counts toward its own purpose, in full, when it moves. A claim is a flow whose value has not come yet, and
//! counting it again when it is paid would count it twice. The owner's `books` say which of the two counts: in `cash`
//! books a claim counts when it is settled, as the purpose the claim was made with, and in `accrual` books when it is
//! made, and its settlement counts nothing. This module is that rule, written once. The fold asks it as it posts a
//! flow, and the reports ask it of the run afterwards, so what a limit, a law, `flow` and the forecast count of a
//! purpose cannot differ.
//!
//! The rule is a function of what the flow *is* to the claims ([`Dealing`]), not of how the caller found out, and it
//! fills a vector the caller reuses. A flow that nobody settled or made is one piece that says "all of it", so it costs
//! its reader what it cost before there was a rule.

use axiom_core::{Day, Days, Id, Qty};
use axiom_model::{Book, Books, Class, Commodity, Dir, Flow, Place, Purposed, RuntimeTxn};

use crate::motion::Motion;
use crate::plan::Plan;
use crate::{Parcel, Posted};

/// When accrual books count a claim: LANGUAGE §6 and §7 and the doc of [`Books::Accrual`] say "when invoiced", that is when
/// the claim is made. The other reading, the day it falls due, is the days a claim is recognized over (`Counting::made`), which
/// is the one place [`ACCRUAL_AT`] reaches: changing the reading is changing that constant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccrualAt {
    /// The day the claim is made.
    Made,
    /// The day it falls due.
    Due,
}

/// The reading of §7 that the books follow.
const ACCRUAL_AT: AccrualAt = AccrualAt::Made;

/// Whether every piece of a flow is recognized over the flow's own days, as a claim made is not when accrual books count it
/// when it falls due. A reader that wants the pieces of one set of days may then pass over a flow of other days unbuilt.
pub const KEEPS_FLOW_DAYS: bool = matches!(ACCRUAL_AT, AccrualAt::Made);

/// The way a claim counts in its tab: what a party owes the owner is income's way in, what the owner owes spending's way out.
/// Settling it counts that way, and taking it back (a write-off, a payment returned) the other.
pub fn claim_dir(class: Class) -> Dir {
    match class {
        Class::Debt => Dir::Out,
        Class::Asset | Class::Outside => Dir::In,
    }
}

/// What a flow is to the claims.
#[derive(Clone, Copy)]
pub enum Dealing<'a> {
    /// Nothing to do with a claim.
    Ordinary,
    /// Value between a party and a claim place: a claim is made, or taken back.
    Making,
    /// The flow settled claims, or, run backwards, opened them again, in the way `dir` counts them. `moved` is what it
    /// moved of one commodity, which settlement requires.
    Settling { settlement: &'a Settlement, dir: Dir, moved: Qty },
    /// A claim is forgiven, `qty` of it, in the way `dir` took it back.
    Forgiving { tab: Id<Place>, qty: Qty, dir: Dir },
}

/// The claims a flow out of a party's place (or a claim place) settled: the parcels they were, taken out of the tab
/// that held them. A payment that is returned puts them back.
#[derive(Clone, Hash, Debug)]
pub struct Settlement {
    pub tab: Id<Place>,
    pub unit: Id<Commodity>,
    pub parcels: Box<[Parcel]>,
    pub reaches: Reaches,
}

/// Where the value of a flow that settles claims goes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Reaches {
    /// Into the owner's money: what arrives is what the claims were, so they count in its place.
    Owner,
    /// To a third party on the owner's behalf, or out of a claim place: what it moves is its own, the owner's cost,
    /// which the claims it settled do not replace.
    Elsewhere,
}

/// A flow as recognition sees it.
#[derive(Clone, Copy)]
pub struct Counting<'a> {
    pub books: Books,
    pub purpose: Option<Purposed>,
    pub day: Day,
    pub recognized: Days,
    /// When the claim it makes falls due, if it says.
    pub due: Option<Day>,
    pub dealing: Dealing<'a>,
}

impl<'a> Counting<'a> {
    /// A flow the fold is posting.
    pub(crate) fn moving(plan: &Plan, m: &Motion, dealing: Dealing<'a>) -> Counting<'a> {
        let books = plan.traits.entity(m.owner).books;
        let due = matches!(dealing, Dealing::Making).then(|| m.detail().due).flatten();
        Counting { books, purpose: m.purpose, day: m.day, recognized: m.recognized, due, dealing }
    }

    /// A claim forgiven on `day`, `qty` of it in `tab`: what its purpose took back.
    pub fn forgiving(plan: &Plan, claim: &Flow, tab: Id<Place>, day: Day, qty: Qty) -> Counting<'static> {
        let dealing = Dealing::Forgiving { tab, qty, dir: claim_dir(plan.book.places[tab].class).reversed() };
        let books = plan.traits.entity(claim.owner).books;
        Counting { books, purpose: claim.purpose, day, recognized: Days::on(day), due: None, dealing }
    }

    /// A journal flow as the run left it, with what it settled of claims, if it did.
    pub fn posted(plan: &Plan, flow: &Flow, posted: &Posted, settlement: Option<&'a Settlement>) -> Counting<'a> {
        let dealing = match settlement {
            Some(settlement) => {
                let dir = claim_dir(plan.book.places[settlement.tab].class);
                Dealing::Settling { settlement, dir, moved: posted.out }
            }
            None if plan.makes_claim(flow.from, flow.to) => Dealing::Making,
            None => Dealing::Ordinary,
        };
        let due = plan.book.flow_view(flow).detail().due;
        let books = plan.traits.entity(flow.owner).books;
        Counting { books, purpose: flow.purpose, day: flow.day, recognized: flow.recognized, due, dealing }
    }
}

/// A part of a flow that counts toward one purpose.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub purpose: Option<Purposed>,
    pub share: Share,
    pub recognized: Days,
    pub counts: Counts,
}

/// How much of a flow a piece is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Share {
    /// What the flow moved on the side its direction counts.
    Whole,
    /// This much of it, of its commodity.
    Part(Qty),
}

/// Where a piece counts: as the flow moved, or as the claim it settled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Counts {
    /// With the flow's own ends: the direction they say, at the place they move through.
    Flow,
    /// With the claim: in the direction it counted in, at the tab that held it.
    Claim { tab: Id<Place>, dir: Dir },
}

impl Piece {
    /// Whether it takes back what a claim counted: a claim forgiven in accrual books, or a payment that settled one and
    /// was returned, which count against the way the claim's tab counts.
    pub fn takes_back(&self, book: &Book) -> bool {
        matches!(self.counts, Counts::Claim { tab, dir } if dir == claim_dir(book.places[tab].class).reversed())
    }
}

impl Counting<'_> {
    /// The pieces the flow counts in, in `out`: its own first, then one for each purpose of the claims it settled.
    pub fn pieces(&self, book: &Book, out: &mut Vec<Piece>) {
        out.clear();
        let own = |share, recognized| Piece { purpose: self.purpose, share, recognized, counts: Counts::Flow };
        match (self.dealing, self.books) {
            (Dealing::Making, Books::Cash) if self.purpose.is_some() => {}
            (Dealing::Making, _) => out.push(own(Share::Whole, self.made(ACCRUAL_AT))),
            (Dealing::Ordinary, _) => out.push(own(Share::Whole, self.recognized)),
            (Dealing::Forgiving { tab, qty, dir }, Books::Accrual) if self.purpose.is_some() => out.push(Piece {
                share: Share::Part(qty),
                counts: Counts::Claim { tab, dir },
                ..own(Share::Whole, self.recognized)
            }),
            (Dealing::Forgiving { .. }, _) => {}
            (Dealing::Settling { settlement, dir, moved }, books) => {
                let settled = self.settled(book, settlement, dir, books, out);
                let replaced = if settlement.reaches == Reaches::Owner { settled } else { Qty::ZERO };
                if moved > replaced {
                    let share = if replaced.is_zero() { Share::Whole } else { Share::Part(moved - replaced) };
                    out.insert(0, own(share, self.recognized));
                }
            }
        }
    }

    /// The days a claim made in accrual books counts over, as `at` reads when it counts.
    fn made(&self, at: AccrualAt) -> Days {
        match at {
            AccrualAt::Made => self.recognized,
            AccrualAt::Due => self.due.map_or(self.recognized, |due| Days::on(due.max(self.day))),
        }
    }

    /// Adds a piece to `out` for each purpose of the claims settled, in cash books, and says how much of what the flow
    /// moved they were: a claim with no purpose has no recognition, so what pays it counts as it would have.
    fn settled(&self, book: &Book, settlement: &Settlement, dir: Dir, books: Books, out: &mut Vec<Piece>) -> Qty {
        let counts = Counts::Claim { tab: settlement.tab, dir };
        let mut total = Qty::ZERO;
        for parcel in &settlement.parcels {
            let Some(purpose) = claim_purpose(book, parcel) else { continue };
            total += parcel.qty;
            if books == Books::Accrual {
                continue;
            }
            let found = out.iter_mut().find(|piece| piece.counts == counts && same_purpose(piece.purpose, purpose));
            match found {
                Some(Piece { share: Share::Part(qty), .. }) => *qty += parcel.qty,
                _ => out.push(Piece {
                    purpose: Some(purpose),
                    share: Share::Part(parcel.qty),
                    recognized: self.recognized,
                    counts,
                }),
            }
        }
        total
    }
}

/// What the claim a parcel is was made for: the purpose of the line that made it, if it was a line of the journal, or of the
/// header of the occurrence the monitor found missing (`Book::claim_of` says the same of its due day and its party).
pub fn claim_purpose(book: &Book, parcel: &Parcel) -> Option<Purposed> {
    let part = parcel.part?;
    if let RuntimeTxn::ContractOccurrence { contract, schedule, source: None, .. } = part.origin {
        return book.contracts[contract].terms_of(schedule)?.template.first()?.header.flow.purpose;
    }
    book.flows[book.txn_flow(part.origin, part.ordinal)?].purpose
}

/// Whether two purposes are the same to the totals: the same node, of the same thing.
fn same_purpose(left: Option<Purposed>, right: Purposed) -> bool {
    left.is_some_and(|left| left.purpose == right.purpose && left.of == right.of)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim_due(due: Option<Day>) -> Counting<'static> {
        let day = Day(10);
        Counting { books: Books::Accrual, purpose: None, day, recognized: Days::on(day), due, dealing: Dealing::Making }
    }

    /// `AccrualAt::Made` is the reading the books follow; `Due` is the one-line alternative, and counts a claim on the day it
    /// falls due, never before the day it is made.
    #[test]
    fn a_claim_counts_over_the_day_it_is_made_or_the_day_it_falls_due_as_the_reading_says() {
        let owed = claim_due(Some(Day(40)));
        assert_eq!((owed.made(AccrualAt::Made), owed.made(AccrualAt::Due)), (Days::on(Day(10)), Days::on(Day(40))));
        assert_eq!(
            claim_due(None).made(AccrualAt::Due),
            Days::on(Day(10)),
            "a claim that says no day falls due when made"
        );
        assert_eq!(
            claim_due(Some(Day(5))).made(AccrualAt::Due),
            Days::on(Day(10)),
            "one already past due counts today"
        );
        assert_eq!(ACCRUAL_AT, AccrualAt::Made, "the books count a claim when it is made (LANGUAGE §7)");
    }
}
