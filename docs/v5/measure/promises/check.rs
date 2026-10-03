//! The verdict: the fold must say what a walk from the contract's first day says.
//!
//! The reference is the plainest reading of the language there is: [`calendar::due`] asked from the first day through a
//! window that begins a month before the one asked (so that a day that falls after its step and before the window is
//! not lost), its days taken once, those in the contract's life, none that is waived. The fold must equal it on every
//! question: the days due in a window, the ordinal of each, the line each day keeps (within the reach LANGUAGE §7 says:
//! the contract's `grace`, else half a cadence of the schedule's own) and, in `monitor.rs`, what the journal kept and
//! what was missed. The factor and the recognition window of a day, and a loan's payment, have no second
//! implementation here: the old code that was their judge is gone, and its answers are the frozen dump the python
//! driver compares (`contracts.py compare`). What can be said of them without it is said below: a day that is not
//! owed has none, a contract that neither escalates nor prorates has the factor one.
//!
//! The old rule for the due days, the ordinals and the line a day keeps is rebuilt below from `calendar::due` as the old
//! walkers asked it (`walked_old`, `keep_old`, `ordinal_old`), and every answer the fold gives otherwise is given a
//! [`Cause`]: the reach, a day the old walk lost, a day it found twice or out of order, or a walk with no first day, which
//! it counted from the beginning of time and is not rebuilt. An answer that differs in none of those ways fails.

use std::collections::BTreeMap;

use axiom_core::{Day, Days, Id, Ratio, calendar};
use axiom_model::{Book, Contract, ForecastError, ScheduleKind};

use crate::ask::{Answers, Facts};
use crate::reading::Keep;

/// The verdicts of one contract, by question.
#[derive(Default)]
pub struct Tally {
    agreed: BTreeMap<String, usize>,
    pub fails: Vec<String>,
}

impl Tally {
    pub(crate) fn new(&mut self, what: &'static str, agrees: bool, context: impl FnOnce() -> String) {
        if agrees {
            *self.agreed.entry(what.to_string()).or_default() += 1;
        } else {
            self.fails.push(format!("the fold's {what} is not the reference's: {}", context()));
        }
    }

    /// What the fold says against the old rule: the same, or different in one of the ways `Cause` allows. A difference
    /// nothing explains is a failure.
    pub(crate) fn against_old(&mut self, what: &'static str, cause: Cause, context: impl FnOnce() -> String) {
        if cause == Cause::Unexplained {
            self.fails
                .push(format!("the fold's {what} differs from the old rule in no way that is known: {}", context()));
        } else {
            *self.agreed.entry(format!("{what} against the old rule: {}", cause.name())).or_default() += 1;
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.agreed.iter().map(|(what, n)| format!("check new {what} {n}")).collect();
        lines.extend(self.fails.iter().map(|fail| format!("FAIL {fail}")));
        lines
    }
}

/// The reading of a contract that every other is held to.
pub struct Reference<'a> {
    pub id: Id<Contract>,
    pub contract: &'a Contract,
}

impl Reference<'_> {
    /// The days the schedule owes in `window`: each once, in order.
    pub(crate) fn owed(&self, kind: ScheduleKind, window: Days) -> Vec<Day> {
        let (Some(declared), Some(window)) = (self.contract.terms_of(kind), window.intersect(self.contract.days))
        else {
            return Vec::new();
        };
        let anchor = self.anchor(declared);
        let early = Day(window.first().0.saturating_sub(45).max(anchor.0));
        let wide = Days::new(early.min(window.first()), window.last()).expect("days");
        let walked = calendar::due(declared.every, &declared.on, anchor, wide);
        let mut days: Vec<Day> =
            walked.filter(|day| window.contains(*day) && self.contract.waiver_on(*day).is_none()).collect();
        days.sort_unstable();
        days.dedup();
        days
    }

    /// Where a schedule counts from: the contract's first day, or, for a schedule only a walk can say that has no first
    /// day, 1970-01-01: a walk from the beginning of time would collect four billion days.
    pub(crate) fn anchor(&self, declared: &axiom_model::Terms) -> Day {
        let first = self.contract.days.first();
        let walked = !axiom_core::Dues::new(declared.every, &declared.on, first).is_counted();
        if walked && first == Day::MIN { Day(0) } else { first }
    }

    /// What the old walkers found in `within`, rebuilt from `calendar::due` as they asked it: each stretch of the contract
    /// that is not waived, cut to the window and to the contract's days, walked on its own. A day a step lands on twice
    /// is found twice (`weekly on 15`), and one that falls after its step and before its stretch's window is lost (`on
    /// last` after a waiver). Nothing here is the fold's, and nothing of the fold's reads it: it is what the frozen dump
    /// is held to, so that the ways the fold differs from it are the ways these two say.
    fn walked_old(&self, kind: ScheduleKind, within: Days) -> Vec<Day> {
        let (Some(terms), Some(window)) = (self.contract.terms_of(kind), within.intersect(self.contract.days)) else {
            return Vec::new();
        };
        let stretches = self.contract.waived.within(window).filter(|(_, waiver)| waiver.is_none());
        stretches
            .flat_map(|(stretch, _)| {
                let days = stretch.intersect(window).expect("a stretch of the window meets it");
                calendar::due(terms.every, &terms.on, self.contract.days.first(), days)
            })
            .collect()
    }

    /// The days due in a window as the old merge of both schedules gave them: a standing day only if strictly earlier.
    pub(crate) fn due_old(&self, window: Days) -> Vec<(Day, ScheduleKind)> {
        let (regular, standing) =
            (self.walked_old(ScheduleKind::Regular, window), self.walked_old(ScheduleKind::Standing, window));
        let (mut regular, mut standing) = (regular.into_iter().peekable(), standing.into_iter().peekable());
        let mut days = Vec::new();
        loop {
            match (regular.peek(), standing.peek()) {
                (Some(&r), Some(&s)) if s < r => days.extend(standing.next().map(|day| (day, ScheduleKind::Standing))),
                (Some(_), _) => days.extend(regular.next().map(|day| (day, ScheduleKind::Regular))),
                (None, Some(_)) => days.extend(standing.next().map(|day| (day, ScheduleKind::Standing))),
                (None, None) => return days,
            }
        }
    }

    /// The ordinal the old count gave: the due days of the schedule from the contract's first to this one, less one.
    pub(crate) fn ordinal_old(&self, kind: ScheduleKind, due: Day) -> Option<u32> {
        let through = Days::new(self.contract.days.first(), due).unwrap_or(Days::on(due));
        let count = self.walked_old(kind, through).into_iter().filter(|day| *day <= due).count();
        count.checked_sub(1).and_then(|index| u32::try_from(index).ok())
    }

    fn due(&self, window: Days) -> Vec<(Day, ScheduleKind)> {
        let mut days: Vec<(Day, ScheduleKind)> = [ScheduleKind::Regular, ScheduleKind::Standing]
            .into_iter()
            .flat_map(|kind| self.owed(kind, window).into_iter().map(move |day| (day, kind)))
            .collect();
        days.sort_by_key(|&(day, kind)| (day, kind == ScheduleKind::Standing));
        days
    }

    pub(crate) fn ordinal(&self, kind: ScheduleKind, due: Day) -> Option<u32> {
        let through = Days::new(self.contract.days.first(), due)?;
        let owed = self.owed(kind, through);
        (owed.last() == Some(&due)).then(|| owed.len() as u32 - 1)
    }

    /// The line the old matching kept: the nearest due day of each schedule within the longest cadence of either, and
    /// the nearer of the two.
    pub(crate) fn keep_old(&self, day: Day) -> Keep {
        self.keep_via(day, Radius::Longest, Walk::Old)
    }

    /// The line the language says a line dated `day` keeps: the nearest due day of each schedule within its own reach.
    pub(crate) fn keep(&self, day: Day) -> Keep {
        self.keep_via(day, Radius::Own, Walk::Reference)
    }

    /// The line a line dated `day` keeps, if a reach is `radius` and the due days are those `walk` finds: both rules
    /// are this one, and the ways they differ are the two arguments.
    fn keep_via(&self, day: Day, radius: Radius, walk: Walk) -> Keep {
        if !self.contract.days.contains(day) {
            return Keep::Out;
        }
        let nearest = |kind| {
            let window = self.search(kind, day, radius)?;
            let near = |due: &Day| ((i64::from(day.0) - i64::from(due.0)).abs(), *due > day);
            let found = match walk {
                Walk::Old => self.walked_old(kind, window),
                Walk::Reference => self.owed(kind, window),
            };
            found.into_iter().min_by_key(|due| near(due)).map(|due| (near(&due).0, due))
        };
        match (nearest(ScheduleKind::Regular), nearest(ScheduleKind::Standing)) {
            (Some((r, regular)), Some((s, standing))) if r == s => Keep::Ambiguous(regular, standing),
            (Some((r, regular)), Some((s, _))) if r < s => Keep::Kept(ScheduleKind::Regular, regular),
            (_, Some((_, standing))) => Keep::Kept(ScheduleKind::Standing, standing),
            (Some((_, regular)), None) => Keep::Kept(ScheduleKind::Regular, regular),
            (None, None) => Keep::Out,
        }
    }

    /// How far from a due day of the schedule a line may be dated and still keep it: its `grace`, else half its cadence.
    /// A month counts 31 days, as a reach always has, and half is rounded down.
    pub(crate) fn reach(&self, kind: ScheduleKind) -> i64 {
        let Some(terms) = self.contract.terms_of(kind) else { return 0 };
        terms.grace.map_or(cadence(terms) / 2, days).clamp(0, i64::from(i32::MAX))
    }

    /// The days a line dated `day` may keep one of, of one schedule: within its reach of it, or, for the old rule, of
    /// the longest cadence of either schedule.
    fn search(&self, kind: ScheduleKind, day: Day, radius: Radius) -> Option<Days> {
        let reach = match radius {
            Radius::Own => self.reach(kind),
            Radius::Longest => {
                let of = |kind| self.contract.terms_of(kind).map_or(0, cadence);
                of(ScheduleKind::Regular).max(of(ScheduleKind::Standing))
            }
        }
        .clamp(0, i64::from(i32::MAX)) as i32;
        Days::new(Day(day.0.saturating_sub(reach)), Day(day.0.saturating_add(reach)))
    }

    /// Whether the schedule is one only a walk can say, with no first day: the old rule walked it from the beginning of
    /// time, and a walk of four billion days is not asked of it here.
    fn rephased(&self, kind: ScheduleKind) -> bool {
        self.contract.terms_of(kind).is_some_and(|declared| self.anchor(declared) != self.contract.days.first())
    }

    /// How the old walk and the reference differ over `window`, for one schedule.
    fn walks(&self, kind: ScheduleKind, window: Days) -> Walks {
        let old = self.walked_old(kind, window);
        let mut once = old.clone();
        once.sort_unstable();
        once.dedup();
        let owed = self.owed(kind, window);
        let in_order = old.windows(2).all(|pair| pair[0] <= pair[1]);
        if once == owed {
            if old.len() > owed.len() || !in_order { Walks::Listing } else { Walks::Same }
        } else if once.iter().all(|day| owed.binary_search(day).is_ok()) {
            Walks::Lost
        } else {
            Walks::Other
        }
    }

    /// Why the line `day` keeps now is not the one the old rule kept: the reach alone says it, or the old walk lost a due
    /// day in reach of it.
    pub(crate) fn keep_cause(&self, day: Day) -> Cause {
        let kinds = [ScheduleKind::Regular, ScheduleKind::Standing];
        if kinds.into_iter().any(|kind| self.rephased(kind)) {
            return Cause::Rephased;
        }
        let new = self.keep(day);
        if self.keep_old(day) == new {
            return Cause::Same;
        }
        if self.keep_via(day, Radius::Own, Walk::Old) == new {
            return Cause::Reach;
        }
        // The old walk loses a day only for the window it is asked: each of the two a line is matched against.
        let lost = kinds.into_iter().any(|kind| {
            let windows = [Radius::Own, Radius::Longest].map(|radius| self.search(kind, day, radius));
            windows.into_iter().flatten().any(|window| self.walks(kind, window) == Walks::Lost)
        });
        if lost { Cause::Lost } else { Cause::Unexplained }
    }

    /// Why the days due in `window` are not what the old walk said: it found a day twice, or it lost one.
    pub(crate) fn due_cause(&self, window: Days) -> Cause {
        let kinds = [ScheduleKind::Regular, ScheduleKind::Standing];
        if kinds.into_iter().any(|kind| self.rephased(kind)) {
            return Cause::Rephased;
        }
        let walks: Vec<Walks> = kinds.into_iter().map(|kind| self.walks(kind, window)).collect();
        let (same, due) = (self.due_old(window) == self.due(window), walks.iter().all(|walk| *walk == Walks::Same));
        match (walks.contains(&Walks::Other), walks.contains(&Walks::Lost), walks.contains(&Walks::Listing)) {
            (true, _, _) => Cause::Unexplained,
            (_, true, _) => Cause::Lost,
            (_, _, true) => Cause::Listing,
            _ if same && due => Cause::Same,
            _ => Cause::Unexplained,
        }
    }

    /// Why the ordinal of `due` is not what the old count said.
    pub(crate) fn ordinal_cause(&self, kind: ScheduleKind, due: Day) -> Cause {
        if self.rephased(kind) {
            return Cause::Rephased;
        }
        if self.ordinal_old(kind, due) == self.ordinal(kind, due) {
            return Cause::Same;
        }
        let through = Days::new(self.contract.days.first(), due).unwrap_or(Days::on(due));
        match self.walks(kind, through) {
            Walks::Listing => Cause::Listing,
            Walks::Lost => Cause::Lost,
            Walks::Same | Walks::Other => Cause::Unexplained,
        }
    }
}

/// How far a line may be from a due day: its schedule's own reach, or, as the old rule had it, the longest cadence of
/// either schedule.
#[derive(Clone, Copy)]
enum Radius {
    Own,
    Longest,
}

/// Which due days a line is matched against: those the old walkers found, or the reference's.
#[derive(Clone, Copy)]
enum Walk {
    Old,
    Reference,
}

/// How the days the old walk found in a window differ from the days owed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Walks {
    Same,
    /// Each owed day, but some twice (`weekly on 15` lands twice in a week of its own) or not in order (`monthly on 1,
    /// monday` lists a period's days as written).
    Listing,
    /// Some owed day is not found: a day that falls after its step and before the window of its stretch.
    Lost,
    /// A day the old walk found that is not owed: nothing says why.
    Other,
}

/// The way the fold's answer differs from the old rule's, if it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cause {
    Same,
    /// The reach: a line the old rule kept from a whole cadence is out of its schedule's own reach, or the reverse.
    Reach,
    /// A day the old rule found twice, or listed out of order: its count and its list are not the owed days'.
    Listing,
    /// A due day the old walk lost.
    Lost,
    /// A walked schedule with no first day: counted from 1970 now, from the beginning of time then. Not compared.
    Rephased,
    Unexplained,
}

impl Cause {
    fn name(self) -> &'static str {
        match self {
            Cause::Same => "the same",
            Cause::Reach => "the reach",
            Cause::Listing => "a day found twice or out of order",
            Cause::Lost => "a day the old walk lost",
            Cause::Rephased => "no first day, not compared",
            Cause::Unexplained => "unexplained",
        }
    }
}

/// A span in days, a month counting 31.
fn days(span: axiom_core::Span) -> i64 {
    i64::from(span.months) * 31 + i64::from(span.days)
}

/// A schedule's cadence in days.
fn cadence(terms: &axiom_model::Terms) -> i64 {
    match terms.every {
        calendar::Cadence::Every(span) => days(span),
        calendar::Cadence::TwiceMonthly => 31,
    }
}

/// Holds the fold's answers to the reference.
pub fn check(book: &Book<'_>, facts: &Facts<'_>, answers: &Answers) -> Tally {
    let reference = Reference { id: facts.id, contract: facts.contract };
    let name = || book.name(facts.contract.name).to_string();
    let mut tally = Tally::default();
    for (window, found) in &answers.due {
        let expected = reference.due(*window);
        tally.new("due days", *found == expected, || {
            format!("{} in {window:?}: {found:?} against {expected:?}", name())
        });
        tally.against_old("due days", reference.due_cause(*window), || format!("{} in {window:?}", name()));
    }
    for (schedule, due, found) in &answers.ordinal {
        let expected = reference.ordinal(*schedule, *due);
        tally.new("ordinal", *found == expected, || {
            format!("{} {schedule:?} {due}: {found:?} against {expected:?}", name())
        });
        tally.against_old("ordinal", reference.ordinal_cause(*schedule, *due), || {
            format!("{} {schedule:?} {due}", name())
        });
    }
    for (day, found) in &answers.keep {
        let expected = reference.keep(*day);
        tally.new("keep", *found == expected, || format!("{} {day}: {found} against {expected}", name()));
        tally.against_old("keep", reference.keep_cause(*day), || format!("{} {day}", name()));
    }
    factors(&reference, answers, &mut tally);
    crate::walk::walk(book, facts, &reference, &mut tally);
    tally
}

/// What can be said of the factors and the recognition windows without a second implementation of them: none for a
/// day that is not owed, and one where nothing escalates or prorates.
fn factors(reference: &Reference<'_>, answers: &Answers, tally: &mut Tally) {
    let contract = reference.contract;
    for (kind, day, found) in &answers.factor {
        let owed = contract.days.contains(*day) && contract.waiver_on(*day).is_none();
        let answered = match found {
            Err(ForecastError::OutsideContract(_)) => !contract.days.contains(*day),
            Err(ForecastError::Waived(_)) => contract.days.contains(*day) && contract.waiver_on(*day).is_some(),
            _ => owed,
        };
        tally.new("factor of a day not owed", answered, || format!("{kind:?} {day}: {found:?}, owed {owed}"));
        let terms = contract.terms_of(*kind).expect("a schedule that was asked");
        if owed && terms.escalation.is_none() && !terms.prorated {
            tally.new("factor of one", *found == Ok(Ratio::ONE), || format!("{kind:?} {day}: {found:?}"));
        }
    }
}
