//! A promise compiled to a term, and the schedules its terms fall due on.
//!
//! A contract declares its schedules once (`Contract { terms, standing, waived, .. }`: a regular one and a standing
//! `buy`, and the stretches statements waived). This module is what they compile to, once, when the book is built:
//! a [`Term`] per contract, in an arena, and beside it the [`Schedule`] each `Every` of it falls due on. The fold, the
//! lowering of a kept line and the reports ask these, and nothing else, which due day, which ordinal and what factor.
//!
//! # Why the layout is the one it is
//!
//! * **Terms are stored post-order**, children before their parent, as the nodes of a law's program are: an id says
//!   what a term is made of and nothing needs a pointer. A term is a handful of ids and is [`Copy`], so a
//!   [`Residual`], which is a cursor into the arena and never a copy of what it points at, is 24 bytes.
//! * **Everything of variable length is a [`Run`] into one flat pool of the [`Promises`]**: the days of an `on`, the
//!   children of an `All`, the stretches a waiver takes out of a schedule. There is no `Box<[_]>` per schedule and no
//!   `Vec<Vec<_>>`.
//! * **Contracts are a dense `Vec`**, the promise of a contract at the contract's own index, so the fold reaches it
//!   without a map.
//! * **A waiver is a hole in the days, not a change of terms.** A contract holds its terms once and the days they are
//!   waived (`Contract::waived`), so there is nothing for a schedule to disagree with. Its holes are the waived stretches
//!   with the number of due days each swallows, so that an ordinal counts the days that are owed.
//! * **A term names what is paid and does not hold it.** `Pay` says that each due day of a stream pays the template of
//!   the contract's terms of that kind: the template, its program and its inputs live in that one place.
//!
//! # What is not here
//!
//! A constructor with no source in today's language is not built: no promise has an `At`, an `If`, a `Let`, an `Accrue` or
//! a `Choose` (see `docs/v5/lanes/K5a-map.md`, section 6). `deposit`, `match`, `resets` and `prepay` are written, checked
//! and read by nothing, and so they compile to nothing.

mod annuity;
mod reckon;
mod residual;
mod schedule;

pub use annuity::{Annuity, Paid};
pub use reckon::{Proration, Reckoning, Recognition};
pub use residual::Residual;
pub use schedule::{Keep, Nearest, Sched, Schedule, Skip};

use axiom_core::{Arena, Cadence, DaySet, Days, Dues, Id, On, Run, Span};

use crate::book::{Book, Contract, Entity, ScheduleKind, Terms};
use crate::split::FlowSide;

/// The id of a term.
pub type TermId = Id<Term>;

/// What is promised, as a term. Children are earlier in the arena than their parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Term {
    /// Nothing is owed.
    Done,
    /// One occurrence's flows: the template of the contract's terms of this kind.
    Pay(ScheduleKind),
    /// Every one of these: a promise with a regular schedule and a standing one.
    All(Run<TermId>),
    /// On each day a schedule falls due, `body`.
    Every { schedule: Id<Schedule>, body: TermId },
    /// `body` is owed by `blame` on the day it is due and, if it is still owed `after` later, what the contract's `due`
    /// clause says follows (`Terms::due`).
    Due { after: Span, blame: Blame, body: TermId },
    /// `body` is the payment of a loan: the debt it pays down is `annuity`'s.
    Annuity { annuity: Id<Annuity>, body: TermId },
}

const _: () = assert!(size_of::<Term>() <= 16);

/// Who a promise is owed by: the contract's party, where money comes into the owner's holding, or the owner, where
/// it goes out of it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Blame {
    Party,
    Owner,
}

impl Blame {
    /// The entity it is, in `contract`.
    pub fn of(self, contract: &Contract) -> Id<Entity> {
        match self {
            Blame::Party => contract.party,
            Blame::Owner => contract.owner,
        }
    }
}

impl Terms {
    /// Who is blamed when what the terms promise is not kept: the party when the header leaves the party's place, and the
    /// owner when it leaves the owner's.
    pub fn blame(&self) -> Blame {
        match self.template.first().map(|group| group.side) {
            Some(FlowSide::Out) => Blame::Party,
            _ => Blame::Owner,
        }
    }
}

/// One stream of a contract's occurrences: the `Every` that owes them, and the schedule it falls due on. A residual
/// starts at the `Every`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stream {
    pub schedule: Id<Schedule>,
    pub every: TermId,
}

/// One contract's promise: its term and its streams.
#[derive(Clone, Copy, Debug)]
pub struct Promise {
    pub root: TermId,
    /// The days the contract lives: nothing falls due outside them.
    pub life: Days,
    pub regular: Option<Stream>,
    pub standing: Option<Stream>,
}

/// Every contract's promise, compiled once when the book is built.
pub struct Promises {
    terms: Arena<Term>,
    /// The children of every `All`, one after another.
    children: Vec<TermId>,
    schedules: Arena<Schedule>,
    /// The `on` of every schedule, one after another.
    landings: Vec<On>,
    /// The holes of every schedule, one after another.
    skips: Vec<Skip>,
    reckonings: Arena<Reckoning>,
    annuities: Arena<Annuity>,
    /// By contract.
    promises: Vec<Promise>,
}

impl Default for Promises {
    /// No contracts, and the one term every promise ends in.
    fn default() -> Promises {
        let mut terms = Arena::new();
        terms.push(Term::Done);
        Promises {
            terms,
            children: Vec::new(),
            schedules: Arena::new(),
            landings: Vec::new(),
            skips: Vec::new(),
            reckonings: Arena::new(),
            annuities: Arena::new(),
            promises: Vec::new(),
        }
    }
}

impl Promises {
    /// The term of nothing owed.
    pub const DONE: TermId = Id::new(0);

    /// Compiles every contract of `book`: its schedules and what the loan and the deadline say.
    pub fn compile(book: &Book<'_>) -> Promises {
        let mut promises = Promises::default();
        for (_, contract) in book.contracts.iter() {
            let promise = promises.promise(contract);
            promises.promises.push(promise);
        }
        promises
    }

    /// One contract's promise on its own, as the contract stands: what lowering asks of a line while the waivers and
    /// endings written after it are not yet read.
    pub fn alone(contract: &Contract) -> (Promises, Promise) {
        let mut promises = Promises::default();
        let promise = promises.promise(contract);
        (promises, promise)
    }

    pub fn term(&self, id: TermId) -> Term {
        self.terms[id]
    }

    /// The promise of a contract the book was built with.
    pub fn of(&self, contract: Id<Contract>) -> &Promise {
        &self.promises[contract.index()]
    }

    /// The promise of a contract, if it was compiled: a book that is made by hand and given a contract later has none.
    pub fn get(&self, contract: Id<Contract>) -> Option<&Promise> {
        self.promises.get(contract.index())
    }

    /// Every compiled promise, by contract.
    pub fn by_contract(&self) -> impl Iterator<Item = (Id<Contract>, &Promise)> {
        self.promises.iter().enumerate().map(|(at, promise)| (Id::new(at as u32), promise))
    }

    /// The children of an `All`, in the order they were written.
    pub fn children(&self, run: Run<TermId>) -> &[TermId] {
        run.get(&self.children).expect("a run of this arena")
    }

    pub fn annuity(&self, id: Id<Annuity>) -> &Annuity {
        &self.annuities[id]
    }

    pub fn reckoning(&self, id: Id<Reckoning>) -> &Reckoning {
        &self.reckonings[id]
    }

    /// The schedule `id` names, and what it does with a day.
    pub fn schedule_of(&self, id: Id<Schedule>) -> schedule::Sched<'_> {
        schedule::Sched::new(self, id)
    }

    /// The loan a body pays down, if it is a loan's, however a deadline wraps it.
    pub fn annuity_of(&self, body: TermId) -> Option<Id<Annuity>> {
        match self.term(body) {
            Term::Annuity { annuity, .. } => Some(annuity),
            Term::Due { body, .. } => self.annuity_of(body),
            _ => None,
        }
    }

    /// Who a body's deadline blames, if it has one, however a loan wraps it.
    pub fn blame_of(&self, body: TermId) -> Option<Blame> {
        match self.term(body) {
            Term::Due { blame, .. } => Some(blame),
            Term::Annuity { body, .. } => self.blame_of(body),
            _ => None,
        }
    }

    /// Who is blamed when an occurrence of a contract's stream is missed, if the stream has a deadline.
    pub fn blame(&self, contract: Id<Contract>, kind: ScheduleKind) -> Option<Blame> {
        let Term::Every { body, .. } = self.term(self.get(contract)?.stream(kind)?.every) else { return None };
        self.blame_of(body)
    }

    /// How long after its due day a body is owed, if it has a deadline, however a loan wraps it.
    pub fn deadline_of(&self, body: TermId) -> Option<Span> {
        match self.term(body) {
            Term::Due { after, .. } => Some(after),
            Term::Annuity { body, .. } => self.deadline_of(body),
            _ => None,
        }
    }

    /// The schedule of the given kind of a contract, and what it does with a day.
    pub fn schedule(&self, contract: Id<Contract>, kind: ScheduleKind) -> Option<schedule::Sched<'_>> {
        let stream = self.get(contract)?.stream(kind)?;
        Some(self.schedule_of(stream.schedule))
    }

    /// The loan a contract's regular schedule pays down.
    pub fn loan(&self, contract: Id<Contract>) -> Option<&Annuity> {
        let stream = self.get(contract)?.regular?;
        let Term::Every { body, .. } = self.term(stream.every) else { return None };
        self.annuity_of(body).map(|annuity| self.annuity(annuity))
    }

    fn promise(&mut self, contract: &Contract) -> Promise {
        let waived = contract.waived_days();
        let regular = contract.terms.as_ref().map(|terms| self.every(contract, ScheduleKind::Regular, terms, &waived));
        let standing =
            contract.standing.as_ref().map(|terms| self.every(contract, ScheduleKind::Standing, terms, &waived));
        let everys: Vec<TermId> = [regular, standing].iter().flatten().map(|stream| stream.every).collect();
        let root = match everys.as_slice() {
            [] => Promises::DONE,
            &[only] => only,
            _ => self.all(&everys),
        };
        Promise { root, life: contract.days, regular, standing }
    }

    fn all(&mut self, children: &[TermId]) -> TermId {
        let start = self.children.len();
        self.children.extend_from_slice(children);
        self.terms.push(Term::All(Run::of(start..self.children.len())))
    }

    /// The schedule of one of a contract's terms and the `Every` that falls due on it.
    fn every(&mut self, contract: &Contract, kind: ScheduleKind, terms: &Terms, waived: &DaySet) -> Stream {
        let body = self.body(contract, kind, terms);
        let on = self.land(&terms.on);
        let skips = self.skips_of(contract, terms, on, waived);
        let reckoning = self.reckonings.push(Reckoning::of(terms));
        let schedule =
            Schedule { kind, every: terms.every, on, life: contract.days, skips, reckoning, reach: reach(terms) };
        let schedule = self.schedules.push(schedule);
        Stream { schedule, every: self.terms.push(Term::Every { schedule, body }) }
    }

    /// What each due day owes: the payment, as a loan's if the contract has a loan, and with the deadline it is due by.
    fn body(&mut self, contract: &Contract, kind: ScheduleKind, terms: &Terms) -> TermId {
        let mut body = if terms.template.is_empty() { Promises::DONE } else { self.terms.push(Term::Pay(kind)) };
        let annuity = contract
            .loan
            .filter(|_| kind == ScheduleKind::Regular)
            .and_then(|loan| Annuity::new(&loan, terms.every, terms.rate));
        if let Some(annuity) = annuity {
            let annuity = self.annuities.push(annuity);
            body = self.terms.push(Term::Annuity { annuity, body });
        }
        if let Some(deadline) = &terms.due {
            body = self.terms.push(Term::Due { after: deadline.after, blame: terms.blame(), body });
        }
        body
    }

    fn land(&mut self, on: &[On]) -> Run<On> {
        let start = self.landings.len();
        self.landings.extend_from_slice(on);
        Run::of(start..self.landings.len())
    }

    /// The waived stretches, each with the due days of the schedule that fall in it.
    fn skips_of(&mut self, contract: &Contract, terms: &Terms, on: Run<On>, waived: &DaySet) -> Run<Skip> {
        let landings = on.get(&self.landings).expect("a run of this arena");
        let dues = Dues::new(terms.every, landings, contract.days.first());
        let mut lost = 0;
        let start = self.skips.len();
        for &days in waived.intervals() {
            let skip = Skip::of(days, &dues, lost);
            lost += skip.gone();
            self.skips.push(skip);
        }
        Run::of(start..self.skips.len())
    }
}

impl Promise {
    /// The stream of the given kind.
    pub fn stream(&self, kind: ScheduleKind) -> Option<Stream> {
        match kind {
            ScheduleKind::Regular => self.regular,
            ScheduleKind::Standing => self.standing,
        }
    }
}

/// How far from a due day a line may be dated and still keep it: the `grace` the contract says, else half a cadence of
/// this schedule (LANGUAGE §7). A span counts a month as 31 days, as a cadence always has for a reach, and half is rounded
/// down, since a distance is whole days.
fn reach(terms: &Terms) -> i32 {
    let days = |span: Span| i64::from(span.months).saturating_mul(31).saturating_add(i64::from(span.days));
    let cadence = match terms.every {
        Cadence::Every(span) => days(span),
        Cadence::TwiceMonthly => 31,
    };
    let reach = terms.grace.map_or(cadence / 2, days);
    reach.clamp(0, i64::from(i32::MAX)) as i32
}
