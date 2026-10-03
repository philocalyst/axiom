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

use std::collections::BTreeMap;

use axiom_core::{Day, Days, Id, Ratio, calendar};
use axiom_model::{Book, Contract, ForecastError, ScheduleKind};

use crate::ask::{Answers, Facts};
use crate::reading::Keep;

/// The verdicts of one contract, by question.
#[derive(Default)]
pub struct Tally {
    agreed: BTreeMap<&'static str, usize>,
    pub fails: Vec<String>,
}

impl Tally {
    pub(crate) fn new(&mut self, what: &'static str, agrees: bool, context: impl FnOnce() -> String) {
        if agrees {
            *self.agreed.entry(what).or_default() += 1;
        } else {
            self.fails.push(format!("the fold's {what} is not the reference's: {}", context()));
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
        let (Some(declared), Some(window)) = (self.contract.terms_of(kind), window.intersect(self.contract.days)) else {
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

    /// The line the old matching kept: the nearest due day of each schedule within the longest cadence of either, and
    /// the nearer of the two.
    pub(crate) fn keep_old(&self, day: Day) -> Keep {
        if !self.contract.days.contains(day) {
            return Keep::Out;
        }
        let days = |span: axiom_core::Span| i64::from(span.months) * 31 + i64::from(span.days);
        let cadence = |kind| {
            self.contract.terms_of(kind).map_or(0, |terms| match terms.every {
                calendar::Cadence::Every(span) => days(span),
                calendar::Cadence::TwiceMonthly => 31,
            })
        };
        let radius = cadence(ScheduleKind::Regular).max(cadence(ScheduleKind::Standing)).clamp(0, i64::from(i32::MAX)) as i32;
        let Some(search) = Days::new(Day(day.0.saturating_sub(radius)), Day(day.0.saturating_add(radius))) else {
            return Keep::Out;
        };
        let nearest = |kind| {
            let near = |due: &Day| ((i64::from(day.0) - i64::from(due.0)).abs(), *due > day);
            self.walked_old(kind, search).into_iter().min_by_key(|due| near(due)).map(|due| (near(&due).0, due))
        };
        match (nearest(ScheduleKind::Regular), nearest(ScheduleKind::Standing)) {
            (Some((r, regular)), Some((s, standing))) if r == s => Keep::Ambiguous(regular, standing),
            (Some((r, regular)), Some((s, _))) if r < s => Keep::Kept(ScheduleKind::Regular, regular),
            (_, Some((_, standing))) => Keep::Kept(ScheduleKind::Standing, standing),
            (Some((_, regular)), None) => Keep::Kept(ScheduleKind::Regular, regular),
            (None, None) => Keep::Out,
        }
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

    /// How far from a due day of the schedule a line may be dated and still keep it: its `grace`, else half its cadence.
    /// A month counts 31 days, as a reach always has, and half is rounded down.
    pub(crate) fn reach(&self, kind: ScheduleKind) -> i64 {
        let days = |span: axiom_core::Span| i64::from(span.months) * 31 + i64::from(span.days);
        let Some(terms) = self.contract.terms_of(kind) else { return 0 };
        let cadence = match terms.every {
            calendar::Cadence::Every(span) => days(span),
            calendar::Cadence::TwiceMonthly => 31,
        };
        terms.grace.map_or(cadence / 2, days).clamp(0, i64::from(i32::MAX))
    }

    /// The days a line dated `day` may keep one of, of one schedule: within its reach of it.
    fn search(&self, kind: ScheduleKind, day: Day) -> Option<Days> {
        let reach = self.reach(kind) as i32;
        Days::new(Day(day.0.saturating_sub(reach)), Day(day.0.saturating_add(reach)))
    }

    pub(crate) fn keep(&self, day: Day) -> Keep {
        if !self.contract.days.contains(day) {
            return Keep::Out;
        }
        let nearest = |kind| {
            let window = self.search(kind, day)?;
            let near = |due: &Day| ((i64::from(day.0) - i64::from(due.0)).abs(), *due > day);
            self.owed(kind, window).into_iter().min_by_key(|due| near(due)).map(|due| (near(&due).0, due))
        };
        match (nearest(ScheduleKind::Regular), nearest(ScheduleKind::Standing)) {
            (Some((r, regular)), Some((s, standing))) if r == s => Keep::Ambiguous(regular, standing),
            (Some((r, regular)), Some((s, _))) if r < s => Keep::Kept(ScheduleKind::Regular, regular),
            (_, Some((_, standing))) => Keep::Kept(ScheduleKind::Standing, standing),
            (Some((_, regular)), None) => Keep::Kept(ScheduleKind::Regular, regular),
            (None, None) => Keep::Out,
        }
    }
}

/// Holds the fold's answers to the reference.
pub fn check(book: &Book<'_>, facts: &Facts<'_>, answers: &Answers) -> Tally {
    let reference = Reference { id: facts.id, contract: facts.contract };
    let name = || book.name(facts.contract.name).to_string();
    let mut tally = Tally::default();
    for (window, found) in &answers.due {
        let expected = reference.due(*window);
        tally.new("due days", *found == expected, || format!("{} in {window:?}: {found:?} against {expected:?}", name()));
    }
    for (schedule, due, found) in &answers.ordinal {
        let expected = reference.ordinal(*schedule, *due);
        tally.new("ordinal", *found == expected, || format!("{} {schedule:?} {due}: {found:?} against {expected:?}", name()));
    }
    for (day, found) in &answers.keep {
        let expected = reference.keep(*day);
        tally.new("keep", *found == expected, || format!("{} {day}: {found} against {expected}", name()));
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
