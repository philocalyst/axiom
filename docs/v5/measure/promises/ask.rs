//! The questions put to a contract, in an order that does not depend on any answer.

use axiom_core::{Day, Days, Id, Ratio, Span};
use axiom_model::{Contract, ForecastError, ScheduleKind};

use crate::reading::{Keep, Payment, Reading};
use crate::{kind, show, span};

/// A contract and what is asked of it.
pub struct Facts<'a> {
    pub id: Id<Contract>,
    pub contract: &'a Contract,
    pub slow: bool,
}

/// What a reading said to every question.
#[derive(Clone, PartialEq, Debug)]
pub struct Answers {
    pub due: Vec<(Days, Vec<(Day, ScheduleKind)>)>,
    pub keep: Vec<(Day, Keep)>,
    pub ordinal: Vec<(ScheduleKind, Day, Option<u32>)>,
    pub factor: Vec<(ScheduleKind, Day, Result<Ratio, ForecastError>)>,
    pub recognized: Vec<(ScheduleKind, Day, Option<Result<Days, ForecastError>>)>,
    pub payment: Option<Payment>,
}

/// The day a contract with no `from` is read around: its days come from `Day::MIN`.
const REFERENCE: (i32, u32, u32) = (2026, 1, 1);

impl Facts<'_> {
    /// Which schedules the contract has.
    pub fn schedules(&self) -> Vec<ScheduleKind> {
        let mut schedules = Vec::new();
        if self.contract.terms.is_some() {
            schedules.push(ScheduleKind::Regular);
        }
        if self.contract.standing.is_some() {
            schedules.push(ScheduleKind::Standing);
        }
        schedules
    }

    /// What the contract is, as the book holds it. The lines are the ones the dump of the old code made (one `stretch`
    /// line for each schedule, and a `terms` line), so that the two dumps can be compared.
    pub fn shape(&self) -> Vec<String> {
        let contract = self.contract;
        let mut lines = vec![format!(
            "days {} ended {} loan {} buys {} deposit {}",
            span(contract.days),
            contract.ended.is_some(),
            contract.loan.is_some(),
            contract.buys.is_some(),
            contract.deposit.is_some()
        )];
        for schedule in self.schedules() {
            let terms = contract.terms_of(schedule).expect("a schedule that exists");
            for (days, waiver) in contract.waived.within(Days::ALWAYS) {
                let state = if waiver.is_some() { "waived" } else { "active" };
                lines.push(format!("stretch {} {} {state}", kind(schedule), span(days)));
            }
            lines.push(format!(
                "terms {} every {:?} on {:?} anchor {} estimate {} grace {:?} escalation {:?} prorated {} period {:?} covers {:?}",
                kind(schedule),
                terms.every,
                terms.on,
                show(contract.days.first()),
                terms.estimate,
                terms.grace,
                terms.escalation,
                terms.prorated,
                terms.period,
                terms.covers,
            ));
        }
        lines
    }

    /// The contract's first and last day, or, for an unbounded end, a day to read around.
    pub fn life(&self) -> (Day, Day) {
        let reference = Day::from_ymd(REFERENCE.0, REFERENCE.1, REFERENCE.2).expect("a date");
        let first = if self.contract.days.first() == Day::MIN { reference } else { self.contract.days.first() };
        let last =
            if self.contract.days.last() == Day::MAX { first.add(Span::months(60)) } else { self.contract.days.last() };
        (first, last)
    }

    /// The stretches of both schedules, as days: the waivers' stretches, once for each schedule the contract has.
    pub fn stretches(&self) -> Vec<Days> {
        let all: Vec<Days> = self.contract.waived.within(Days::ALWAYS).map(|(days, _)| days).collect();
        self.schedules().iter().flat_map(|_| all.iter().copied()).collect()
    }

    /// Windows to ask the due days of: around the first day, the last, each change, and far from all of them.
    pub fn windows(&self) -> Vec<Days> {
        let (first, last) = self.life();
        let near = |center: Day, before: i32, after: i32| {
            Days::new(Day(center.0.saturating_sub(before)), Day(center.0.saturating_add(after)))
        };
        let mut windows = vec![
            near(first, 40, 40),
            near(first, 0, 400),
            near(first, -100, 101),
            near(first, -1000, 1200),
            near(first, 3650, -1),
            near(last, 70, 70),
            near(last, -1, 400),
            near(first, 0, 0),
            near(last, 0, 0),
            near(first, 0, 12_000),
        ];
        for days in self.stretches() {
            for edge in [days.first(), days.last()] {
                if edge != Day::MIN && edge != Day::MAX {
                    windows.push(near(edge, 3, 3));
                    windows.push(near(edge, 0, 20));
                }
            }
        }
        if self.contract.days.first() == Day::MIN {
            windows.push(Days::new(Day::MIN, Day(i32::MIN + 400)));
            windows.push(Days::new(Day(-400_000), Day(-399_000)));
        }
        if self.contract.days.last() == Day::MAX {
            windows.push(Days::new(Day(i32::MAX - 400), Day::MAX));
        }
        let mut windows: Vec<Days> = windows.into_iter().flatten().collect();
        windows.dedup();
        windows
    }

    /// The days a line may be dated on: every day around the first, around each change and the last, and a spread.
    pub fn probes(&self) -> Vec<Day> {
        let (first, last) = self.life();
        let mut days: Vec<i64> = (-3..=60).map(|offset| i64::from(first.0) + offset).collect();
        let edges = self.stretches().into_iter().flat_map(|days| [days.first(), days.last()]).chain([last]);
        for edge in edges.filter(|&edge| edge != Day::MIN && edge != Day::MAX) {
            days.extend((-6..=6).map(|offset| i64::from(edge.0) + offset));
        }
        let (low, high) = (i64::from(first.0), i64::from(last.0).max(i64::from(first.0) + 1));
        let spread = (high - low).clamp(1, 4000);
        let mut state = 0x9e3779b97f4a7c15u64 ^ (low as u64);
        for _ in 0..60 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            days.push(low + (state >> 33) as i64 % spread);
        }
        days.sort_unstable();
        days.dedup();
        days.into_iter().filter_map(|day| i32::try_from(day).ok().map(Day)).collect()
    }

    /// The first dozen due days of each schedule, as `reading` says them, a few later ones: the days whose ordinal is
    /// asked. None for a contract with no `from` unless `slow`: its count starts at `Day::MIN`.
    pub fn ordinal_days(&self, reading: &impl Reading) -> Vec<(ScheduleKind, Day)> {
        self.ordinal_days_by(|window| reading.due(self.id, window))
    }

    /// As [`ordinal_days`](Facts::ordinal_days), from whatever says the days due in a window.
    pub fn ordinal_days_by(&self, due_in: impl Fn(Days) -> Vec<(Day, ScheduleKind)>) -> Vec<(ScheduleKind, Day)> {
        let unbounded = self.contract.days.first() == Day::MIN;
        let (first, _) = self.life();
        let mut asked = Vec::new();
        for schedule in self.schedules() {
            let window = Days::new(first, Day(first.0.saturating_add(3000))).expect("days");
            let due = due_in(window).into_iter().filter(|(_, s)| *s == schedule).map(|(day, _)| day);
            let due: Vec<Day> = due.collect();
            let picks = due.iter().copied().take(12).chain(due.iter().copied().skip(12).step_by(37));
            let limit = if unbounded { usize::from(self.slow) } else { 40 };
            asked.extend(picks.take(limit).map(|day| (schedule, day)));
        }
        asked
    }

    /// The days the factor and the recognition window are asked on: about six years from just before the first day.
    pub fn days_to_read(&self) -> Vec<Day> {
        let (first, _) = self.life();
        (-3..2200).map(|offset| Day(first.0.saturating_add(offset))).collect()
    }

    /// The questions the old walkers were asked, answered by the rebuilt old rule (`Reference::due_old` and the others):
    /// the due days, the lines kept and the ordinals, over the same windows, probes and days as the fold was asked.
    pub fn old_rule(&self, reference: &crate::check::Reference<'_>) -> Answers {
        let ordinals = self.ordinal_days_by(|window| reference.due_old(window));
        let due = self.windows().into_iter().map(|window| (window, reference.due_old(window))).collect();
        let keep = self.probes().into_iter().map(|day| (day, reference.keep_old(day))).collect();
        let ordinal = ordinals.iter().map(|&(schedule, day)| (schedule, day, reference.ordinal_old(schedule, day))).collect();
        Answers { due, keep, ordinal, factor: Vec::new(), recognized: Vec::new(), payment: None }
    }

    /// Every question, put to `reading`. `ordinals` are the days whose ordinal is asked: the same for every reading.
    pub fn ask(&self, reading: &impl Reading, ordinals: &[(ScheduleKind, Day)]) -> Answers {
        let due = self.windows().into_iter().map(|window| (window, reading.due(self.id, window))).collect();
        let keep = self.probes().into_iter().map(|day| (day, reading.keep(self.id, day))).collect();
        let ordinal =
            ordinals.iter().map(|&(schedule, day)| (schedule, day, reading.ordinal(self.id, schedule, day))).collect();
        let days = self.days_to_read();
        let (mut factor, mut recognized) = (Vec::new(), Vec::new());
        for schedule in self.schedules() {
            factor.extend(days.iter().map(|&day| (schedule, day, reading.factor(self.id, schedule, day))));
            recognized.extend(days.iter().map(|&day| (schedule, day, reading.recognized(self.id, schedule, day))));
        }
        let payment = self.contract.loan.is_some().then(|| reading.payment(self.id, self.life().0));
        Answers { due, keep, ordinal, factor, recognized, payment }
    }
}

/// Consecutive days with the same answer, as ranges: `2026-01-01..2026-01-31 : Ok(..)`.
struct Runs(Vec<(Day, Day, String)>);

impl FromIterator<(Day, String)> for Runs {
    fn from_iter<I: IntoIterator<Item = (Day, String)>>(items: I) -> Runs {
        let mut runs: Vec<(Day, Day, String)> = Vec::new();
        for (day, answer) in items {
            match runs.last_mut() {
                Some((_, last, said)) if *said == answer && last.0.checked_add(1) == Some(day.0) => *last = day,
                _ => runs.push((day, day, answer)),
            }
        }
        Runs(runs)
    }
}

impl Runs {
    fn lines(&self, name: &str) -> Vec<String> {
        let range = |from: Day, to: Day| span(Days::new(from, to).expect("ordered"));
        self.0.iter().map(|(from, to, answer)| format!("{name} {} : {answer}", range(*from, *to))).collect()
    }
}

/// An answer that names the day it was asked on names it as `the day`, so that the same answer on two days is one.
fn said(day: Day, answer: String) -> String {
    answer.replace(&format!("Day({})", day.0), "the day")
}

/// The days of a window as the dump prints them: all of them if there are few, else a count and a checksum.
fn due_line(window: Days, found: &[(Day, ScheduleKind)]) -> String {
    let shown = |(day, schedule): &(Day, ScheduleKind)| format!("{}/{}", show(*day), &kind(*schedule)[..1]);
    if found.len() <= 40 {
        return format!("due {} : {}", span(window), found.iter().map(shown).collect::<Vec<_>>().join(" "));
    }
    let sum = found.iter().fold(0xcbf29ce484222325u64, |sum, (day, schedule)| {
        (sum ^ (day.0 as u32 as u64) ^ ((*schedule == ScheduleKind::Standing) as u64) << 33).wrapping_mul(0x100000001b3)
    });
    format!(
        "due {} : n {} first {} last {} sum {sum:016x}",
        span(window),
        found.len(),
        shown(&found[0]),
        shown(&found[found.len() - 1])
    )
}

impl Answers {
    /// The answers as text, one line to a window, a probe, an ordinal, and one to each run of days with one answer.
    pub fn lines(&self, ordinals_skipped: &[ScheduleKind]) -> Vec<String> {
        let mut lines: Vec<String> = self.due.iter().map(|(window, found)| due_line(*window, found)).collect();
        lines.extend(self.keep.iter().map(|&(day, keep)| (day, keep.to_string())).collect::<Runs>().lines("keep"));
        for (schedule, day, ordinal) in &self.ordinal {
            lines.push(format!("ordinal {} {} : {ordinal:?}", kind(*schedule), show(*day)));
        }
        lines.extend(
            ordinals_skipped.iter().map(|schedule| format!("ordinal {} skipped: no first day", kind(*schedule))),
        );
        for schedule in [ScheduleKind::Regular, ScheduleKind::Standing] {
            let factors = self.factor.iter().filter(|(s, ..)| *s == schedule);
            let factors = factors.map(|(_, day, answer)| (*day, said(*day, format!("{answer:?}")))).collect::<Runs>();
            lines.extend(factors.lines(&format!("factor {}", kind(schedule))));
            let windows = self.recognized.iter().filter(|(s, ..)| *s == schedule);
            let windows = windows.map(|(_, day, answer)| (*day, recognition(*day, answer))).collect::<Runs>();
            lines.extend(windows.lines(&format!("recog {}", kind(schedule))));
        }
        lines.extend(self.payment.iter().map(|payment| format!("payment {payment:?}")));
        lines
    }
}

fn recognition(day: Day, answer: &Option<Result<Days, ForecastError>>) -> String {
    match answer {
        // A window that starts on the day it is asked of is the same answer on every day, said that way.
        Some(Ok(days)) if days.first() == day => format!("Ok(from the day, {} days)", days.len()),
        Some(found) => said(day, format!("{:?}", found.map(span))),
        None => "no template".to_string(),
    }
}
