//! A promise compiled to a term, and the schedules its terms fall due on.
//!
//! A contract is `Contract { terms, standing, buys, deposit, loan, matching, ended, .. }`: two timelines of whole
//! `Terms`, each stretch of them carrying its own copy of a template, a program and eighteen fields, and the schedule
//! arithmetic of four readers spread over `book.rs`, `calendar.rs`, the engine and the forecast. This module is what
//! those become: a [`Term`] per contract, in an arena, and beside it the [`Schedule`] each `Every` of it falls due on.
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
//! * **A waiver is a hole in the days, not a change of terms.** In every contract the language can express, the
//!   stretches of a timeline differ only in whether they are waived (and the statement that waived them): the compile
//!   asserts it. So one schedule per `Every` is exact, and its holes are the waived stretches with the number of due days
//!   each swallows, so that an ordinal counts the days that are owed.
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

use axiom_core::{Arena, Day, DaySet, Days, Dues, Id, On, Run, Span, Timeline};

use crate::book::{Book, Contract, Deadline, Entity, Input, ScheduleKind, Terms};
use crate::journal::Program;
use crate::split::{FlowSide, Promised};

/// The id of a term.
pub type TermId = Id<Term>;

/// What is promised, as a term. Children are earlier in the arena than their parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Term {
    /// Nothing is owed.
    Done,
    /// One occurrence's flows: the template, its program and its inputs.
    Pay(Id<Payment>),
    /// Every one of these: a promise with a regular schedule and a standing one.
    All(Run<TermId>),
    /// On each day a schedule falls due, `body`.
    Every { schedule: Id<Schedule>, body: TermId },
    /// `body` is owed on the day it is due and, if it is still owed `deadline.after` later, `blame` has missed it
    /// and what the deadline says follows.
    Due { deadline: Id<Deadline>, blame: Id<Entity>, body: TermId },
    /// `body` is the payment of a loan: the debt it pays down is `annuity`'s.
    Annuity { annuity: Id<Annuity>, body: TermId },
}

const _: () = assert!(size_of::<Term>() <= 16);

/// What one occurrence pays: the group a contract's template is, with the program its computed amounts read, and the
/// inputs an occurrence may state.
#[derive(Clone, PartialEq, Debug)]
pub struct Payment {
    pub group: Promised,
    pub program: Program,
    pub inputs: Box<[Input]>,
}

/// One stream of a contract's occurrences: the `Every` that owes them, and the schedule it falls due on. A residual
/// starts at the `Every`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stream {
    pub schedule: Id<Schedule>,
    pub every: TermId,
}

/// One contract's promise: its term, its streams and the reach the matching of its lines reads.
#[derive(Clone, Copy, Debug)]
pub struct Promise {
    pub root: TermId,
    /// The days the contract lives: nothing falls due outside them.
    pub life: Days,
    /// How far from a due day a line may be dated and still keep it.
    pub reach: i32,
    /// `grace SPAN`: written, checked and, as in `Terms`, read by nothing: the matching's reach is a cadence, and the
    /// language says it is this. K5b decides.
    pub grace: Option<Span>,
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
    payments: Arena<Payment>,
    deadlines: Arena<Deadline>,
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
            payments: Arena::new(),
            deadlines: Arena::new(),
            annuities: Arena::new(),
            promises: Vec::new(),
        }
    }
}

impl Promises {
    /// The term of nothing owed.
    pub const DONE: TermId = Id::new(0);

    /// Compiles every contract of `book`: its schedules, its payments and what the loan and the deadline say.
    pub fn compile(book: &Book<'_>) -> Promises {
        let mut promises = Promises::default();
        for (_, contract) in book.contracts.iter() {
            let promise = promises.promise(contract);
            promises.promises.push(promise);
        }
        promises
    }

    pub fn term(&self, id: TermId) -> Term {
        self.terms[id]
    }

    /// The promise of a contract.
    pub fn of(&self, contract: Id<Contract>) -> &Promise {
        &self.promises[contract.index()]
    }

    /// The children of an `All`, in the order they were written.
    pub fn children(&self, run: Run<TermId>) -> &[TermId] {
        run.get(&self.children).expect("a run of this arena")
    }

    pub fn payment(&self, id: Id<Payment>) -> &Payment {
        &self.payments[id]
    }

    pub fn deadline(&self, id: Id<Deadline>) -> &Deadline {
        &self.deadlines[id]
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

    /// The deadline a body is owed by, if it has one, however a loan wraps it.
    pub fn deadline_of(&self, body: TermId) -> Option<Id<Deadline>> {
        match self.term(body) {
            Term::Due { deadline, .. } => Some(deadline),
            Term::Annuity { body, .. } => self.deadline_of(body),
            _ => None,
        }
    }

    /// The schedule of the given kind of a contract, and what it does with a day.
    pub fn schedule(&self, contract: Id<Contract>, kind: ScheduleKind) -> Option<schedule::Sched<'_>> {
        let promise = self.of(contract);
        let stream = match kind {
            ScheduleKind::Regular => promise.regular,
            ScheduleKind::Standing => promise.standing,
        }?;
        Some(self.schedule_of(stream.schedule))
    }

    fn promise(&mut self, contract: &Contract) -> Promise {
        let regular = contract.terms.as_ref().map(|timeline| self.every(contract, ScheduleKind::Regular, timeline));
        let standing =
            contract.standing.as_ref().map(|timeline| self.every(contract, ScheduleKind::Standing, timeline));
        let everys: Vec<TermId> = [regular, standing].iter().flatten().map(|stream| stream.every).collect();
        let root = match everys.as_slice() {
            [] => Promises::DONE,
            &[only] => only,
            _ => self.all(&everys),
        };
        let grace = [&contract.terms, &contract.standing]
            .into_iter()
            .flatten()
            .find_map(|timeline| timeline.at(Day::MIN).grace);
        Promise { root, life: contract.days, reach: reach(contract), grace, regular, standing }
    }

    fn all(&mut self, children: &[TermId]) -> TermId {
        let start = self.children.len();
        self.children.extend_from_slice(children);
        self.terms.push(Term::All(Run::of(start..self.children.len())))
    }

    /// The schedule of one timeline of terms and the `Every` that falls due on it.
    fn every(&mut self, contract: &Contract, kind: ScheduleKind, timeline: &Timeline<Terms>) -> Stream {
        let declared = timeline.at(Day::MIN);
        debug_assert!(
            alike(timeline),
            "the stretches of a timeline of terms differ in more than whether they are waived"
        );
        debug_assert_eq!(declared.anchor, contract.days.first(), "a schedule counts from the contract's first day");
        let body = self.body(contract, kind, declared);
        let on = self.land(&declared.on);
        let skips = self.skips_of(contract, timeline, declared, on);
        let reckoning = self.reckonings.push(Reckoning::of(declared));
        let schedule = self.schedules.push(Schedule::new(kind, declared.every, on, contract.days, skips, reckoning));
        Stream { schedule, every: self.terms.push(Term::Every { schedule, body }) }
    }

    /// What each due day owes: the payment, as a loan's if the contract has a loan, and with the deadline it is due by.
    fn body(&mut self, contract: &Contract, kind: ScheduleKind, declared: &Terms) -> TermId {
        let mut body = match declared.template.first() {
            Some(group) => {
                let payment = Payment {
                    group: group.clone(),
                    program: declared.program.clone(),
                    inputs: declared.inputs.clone(),
                };
                let payment = self.payments.push(payment);
                self.terms.push(Term::Pay(payment))
            }
            None => Promises::DONE,
        };
        let annuity = contract
            .loan
            .filter(|_| kind == ScheduleKind::Regular)
            .and_then(|loan| Annuity::new(&loan, declared.every, declared.rate));
        if let Some(annuity) = annuity {
            let annuity = self.annuities.push(annuity);
            body = self.terms.push(Term::Annuity { annuity, body });
        }
        if let Some(deadline) = &declared.due {
            let blame = match declared.template.first().map(|group| group.side) {
                Some(FlowSide::Out) => contract.party,
                _ => contract.owner,
            };
            let deadline = self.deadlines.push(deadline.clone());
            body = self.terms.push(Term::Due { deadline, blame, body });
        }
        body
    }

    fn land(&mut self, on: &[On]) -> Run<On> {
        let start = self.landings.len();
        self.landings.extend_from_slice(on);
        Run::of(start..self.landings.len())
    }

    /// The waived stretches of a timeline, merged where they touch, each with the due days of the schedule that fall in it.
    fn skips_of(
        &mut self,
        contract: &Contract,
        timeline: &Timeline<Terms>,
        declared: &Terms,
        on: Run<On>,
    ) -> Run<Skip> {
        let waived: DaySet =
            timeline.within(Days::ALWAYS).filter(|(_, terms)| terms.is_waived()).map(|(days, _)| days).collect();
        let landings = on.get(&self.landings).expect("a run of this arena");
        let dues = Dues::new(declared.every, landings, contract.days.first());
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

/// Whether every stretch of the timeline is the declared terms, or the declared terms waived.
fn alike(timeline: &Timeline<Terms>) -> bool {
    let declared = timeline.at(Day::MIN);
    timeline.within(Days::ALWAYS).all(|(_, terms)| {
        let mut same = terms.clone();
        same.state = declared.state;
        same.change = declared.change;
        same == *declared
    })
}

/// The reach of a contract's lines: the longest of its cadences, as the old matching measured it: a month is 31 days.
fn reach(contract: &Contract) -> i32 {
    let longest = [&contract.terms, &contract.standing]
        .into_iter()
        .flatten()
        .flat_map(|timeline| timeline.within(Days::ALWAYS))
        .map(|(_, terms)| match terms.every {
            axiom_core::Cadence::Every(Span { months, days }) => {
                i64::from(months).saturating_mul(31).saturating_add(i64::from(days))
            }
            axiom_core::Cadence::TwiceMonthly => 31,
        })
        .max()
        .unwrap_or(0);
    longest.clamp(0, i64::from(i32::MAX)) as i32
}
