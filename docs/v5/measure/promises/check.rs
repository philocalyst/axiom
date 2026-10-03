//! The verdict: the compiled promise must say what a walk from the contract's first day says, and the old machinery
//! must say it too, except where it is wrong and the corpus says how.
//!
//! The reference is the plainest reading of the language there is: [`calendar::due`] asked from the first day through a
//! window that begins a month before the one asked (so that a day that falls after its step and before the window is
//! not lost), its days taken once, those in the contract's life, none that is waived. The new structure must equal it
//! on every question. The old one must equal it too, and where it does not, the way it differs must be one of the
//! listed ways, each shown on a book in `docs/v5/lanes/K5a-map.md`:
//!
//! | label     | the old code                                                                           |
//! |-----------|----------------------------------------------------------------------------------------|
//! | `repeats` | yields a day once for each step that lands on it (an `on` for a longer period than the cadence's) |
//! | `lost`    | loses a due day when a window begins after its step (`on last` has no slack)             |
//! | `no-start`| has no recognition window for a contract with no `from`: its template's day is `Day::MIN` |

use std::collections::BTreeMap;

use axiom_core::{Day, Days, Id, calendar};
use axiom_model::{Book, Contract, ScheduleKind};

use crate::ask::{Answers, Facts};
use crate::reading::{Keep, Reading};

/// How one answer of the old code compares with the reference.
pub enum Verdict {
    Same,
    Listed(&'static str),
    Fails(String),
}

/// The verdicts of one contract, by question.
#[derive(Default)]
pub struct Tally {
    agreed: BTreeMap<&'static str, usize>,
    same: BTreeMap<&'static str, usize>,
    listed: BTreeMap<(&'static str, &'static str), usize>,
    pub fails: Vec<String>,
}

impl Tally {
    fn old(&mut self, what: &'static str, verdict: Verdict, context: impl FnOnce() -> String) {
        match verdict {
            Verdict::Same => *self.same.entry(what).or_default() += 1,
            Verdict::Listed(label) => *self.listed.entry((what, label)).or_default() += 1,
            Verdict::Fails(why) => self.fails.push(format!("the old {what} {why}: {}", context())),
        }
    }

    fn new(&mut self, what: &'static str, agrees: bool, context: impl FnOnce() -> String) {
        if agrees {
            *self.agreed.entry(what).or_default() += 1;
        } else {
            self.fails.push(format!("the new {what} is not the reference's: {}", context()));
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.agreed.iter().map(|(what, n)| format!("check new {what} {n}")).collect();
        lines.extend(self.same.iter().map(|(what, n)| format!("check old {what} same {n}")));
        lines.extend(self.listed.iter().map(|((what, label), n)| format!("check old {what} listed {label} {n}")));
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
    fn owed(&self, kind: ScheduleKind, window: Days) -> Vec<Day> {
        let timeline = match kind {
            ScheduleKind::Regular => self.contract.terms.as_ref(),
            ScheduleKind::Standing => self.contract.standing.as_ref(),
        };
        let (Some(timeline), Some(window)) = (timeline, window.intersect(self.contract.days)) else {
            return Vec::new();
        };
        let declared = timeline.at(Day::MIN);
        let early = Day(window.first().0.saturating_sub(45).max(declared.anchor.0));
        let wide = Days::new(early.min(window.first()), window.last()).expect("days");
        let walked = calendar::due(declared.every, &declared.on, declared.anchor, wide);
        let mut days: Vec<Day> = walked.filter(|day| window.contains(*day) && !timeline.at(*day).is_waived()).collect();
        days.sort_unstable();
        days.dedup();
        days
    }

    fn due(&self, window: Days) -> Vec<(Day, ScheduleKind)> {
        let mut days: Vec<(Day, ScheduleKind)> = [ScheduleKind::Regular, ScheduleKind::Standing]
            .into_iter()
            .flat_map(|kind| self.owed(kind, window).into_iter().map(move |day| (day, kind)))
            .collect();
        days.sort_by_key(|&(day, kind)| (day, kind == ScheduleKind::Standing));
        days
    }

    fn ordinal(&self, kind: ScheduleKind, due: Day) -> Option<u32> {
        let through = Days::new(self.contract.days.first(), due)?;
        let owed = self.owed(kind, through);
        (owed.last() == Some(&due)).then(|| owed.len() as u32 - 1)
    }

    /// How far from a due day a line may be: the longest cadence, a month counted as 31 days.
    fn reach(&self) -> i64 {
        let stretches = [&self.contract.terms, &self.contract.standing].into_iter().flatten();
        let cadence = |every| match every {
            calendar::Cadence::Every(span) => i64::from(span.months) * 31 + i64::from(span.days),
            calendar::Cadence::TwiceMonthly => 31,
        };
        stretches
            .flat_map(|timeline| timeline.within(Days::ALWAYS))
            .map(|(_, terms)| cadence(terms.every))
            .max()
            .unwrap_or(0)
    }

    /// The days a line dated `day` may keep one of: within the reach of it.
    fn search(&self, day: Day) -> Option<Days> {
        let reach = self.reach().clamp(0, i64::from(i32::MAX)) as i32;
        Days::new(Day(day.0.saturating_sub(reach)), Day(day.0.saturating_add(reach)))
    }

    fn keep(&self, day: Day) -> Keep {
        let Some(window) = self.search(day).filter(|_| self.contract.days.contains(day)) else { return Keep::Out };
        let nearest = |kind| {
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

/// Why a stream of days is not the reference's, if it is one of the ways the old code differs.
fn explain(old: &[(Day, ScheduleKind)], reference: &[(Day, ScheduleKind)]) -> Verdict {
    if old == reference {
        return Verdict::Same;
    }
    let mut once: Vec<(Day, ScheduleKind)> = old.to_vec();
    once.sort_by_key(|&(day, kind)| (day, kind == ScheduleKind::Standing));
    once.dedup();
    let repeats = once.len() != old.len() || old.windows(2).any(|pair| pair[0].0 > pair[1].0);
    let inside = once.iter().all(|day| reference.contains(day));
    match (repeats, inside, once == reference) {
        (true, _, true) => Verdict::Listed("repeats"),
        (false, true, false) => Verdict::Listed("lost"),
        (true, true, false) => Verdict::Listed("repeats+lost"),
        _ => Verdict::Fails(format!("has {old:?}, the reference {reference:?}")),
    }
}

/// Holds the new answers to the reference and the old to the reference and its listed exceptions. `reading` is the old
/// machinery, asked again for the days an old answer was counted over.
pub fn check(book: &Book<'_>, facts: &Facts<'_>, reading: &impl Reading, old: &Answers, new: &Answers) -> Tally {
    let reference = Reference { id: facts.id, contract: facts.contract };
    let name = || book.name(facts.contract.name).to_string();
    let mut tally = Tally::default();
    for ((window, was), (_, is)) in old.due.iter().zip(&new.due) {
        let expected = reference.due(*window);
        tally.new("due days", *is == expected, || format!("{} in {window:?}: {is:?} against {expected:?}", name()));
        tally.old("due days", explain(was, &expected), || format!("{} in {window:?}", name()));
    }
    for ((schedule, due, was), (_, _, is)) in old.ordinal.iter().zip(&new.ordinal) {
        let expected = reference.ordinal(*schedule, *due);
        tally.new("ordinal", *is == expected, || format!("{} {schedule:?} {due}: {is:?} against {expected:?}", name()));
        let through = Days::new(facts.contract.days.first(), *due).expect("a due day is in the life");
        let verdict = match *was == expected {
            true => Verdict::Same,
            false => explain(&reading.due(facts.id, through), &reference.due(through)),
        };
        tally.old("ordinal", verdict, || format!("{} {schedule:?} {due}: {was:?} against {expected:?}", name()));
    }
    for ((day, was), (_, is)) in old.keep.iter().zip(&new.keep) {
        let expected = reference.keep(*day);
        tally.new("keep", *is == expected, || format!("{} {day}: {is} against {expected}", name()));
        let verdict = match (*was == expected, reference.search(*day)) {
            (true, _) => Verdict::Same,
            (false, Some(window)) => match explain(&reading.due(facts.id, window), &reference.due(window)) {
                Verdict::Same => Verdict::Fails("keeps another day though its due days are the reference's".into()),
                explained => explained,
            },
            (false, None) => Verdict::Fails("keeps a day where none can be".into()),
        };
        tally.old("keep", verdict, || format!("{} {day}: {was} against {expected}", name()));
    }
    tally.new("factor", old.factor == new.factor, || format!("{}: the factor of a day differs", name()));
    let unbounded = facts.contract.days.first() == Day::MIN;
    for ((schedule, day, was), (_, _, is)) in old.recognized.iter().zip(&new.recognized) {
        let said = is == was;
        let verdict = if said {
            Verdict::Same
        } else if unbounded {
            Verdict::Listed("no-start")
        } else {
            Verdict::Fails("differs".into())
        };
        tally.old("recognition", verdict, || format!("{} {schedule:?} {day}: {was:?} against {is:?}", name()));
    }
    tally.new("payment", old.payment == new.payment, || {
        format!("{}: {:?} against {:?}", name(), old.payment, new.payment)
    });
    tally
}
