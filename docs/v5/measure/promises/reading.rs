//! What a build of the contract machinery says: the questions of the module above, answered by the old code, and,
//! with the `new` feature, by the compiled promise.

use axiom_core::{Day, Days, Id, Ratio};
use axiom_engine::{Options, Plan};
use axiom_model::{Amount, Book, Contract, ForecastError, ScheduleKind, nearest_occurrence};

use crate::{TODAY, kind, show};

/// Which due day a line keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keep {
    Out,
    Kept(ScheduleKind, Day),
    Ambiguous(Day, Day),
}

impl std::fmt::Display for Keep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Keep::Out => f.write_str("none"),
            Keep::Kept(schedule, due) => write!(f, "{} {}", kind(*schedule), show(*due)),
            Keep::Ambiguous(regular, standing) => write!(f, "ambiguous {} {}", show(*regular), show(*standing)),
        }
    }
}

/// What the fold makes of the first due day of a loan.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Payment {
    NoDueDay,
    Pays(Day, Amount),
    /// The loan has no payment for its cadence.
    Unsupported(Day),
    Other(String),
}

/// What a build of the machinery says. The old code is one reading; the compiled promise is another.
pub trait Reading {
    /// The days due in `window`, both schedules merged, regular before standing on a day.
    fn due(&self, contract: Id<Contract>, window: Days) -> Vec<(Day, ScheduleKind)>;
    /// The number of due days of the schedule from the contract's first day to `due`, less one.
    fn ordinal(&self, contract: Id<Contract>, schedule: ScheduleKind, due: Day) -> Option<u32>;
    /// The line `day` keeps.
    fn keep(&self, contract: Id<Contract>, day: Day) -> Keep;
    /// The factor of `day` for a schedule, or the error that says why there is none.
    fn factor(&self, contract: Id<Contract>, schedule: ScheduleKind, day: Day) -> Result<Ratio, ForecastError>;
    /// The window `day`'s occurrence is recognized over, or the error; none if the schedule has no template.
    fn recognized(
        &self,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Option<Result<Days, ForecastError>>;
    /// What the fold makes of the first due day on or after `from` of a loan: the payment, or the error.
    fn payment(&self, contract: Id<Contract>, from: Day) -> Payment;
}

/// The contract machinery as it is: `Contract::occurrences`, `nearest_occurrence`, `amount_on_schedule`, the engine.
pub struct Old<'a, 'b, 's> {
    pub book: &'b Book<'s>,
    pub plan: &'a Plan<'b, 's>,
}

impl Old<'_, '_, '_> {
    /// What the fold makes of the first due day of each schedule, as the forecast would: its flows, where they go, the
    /// days each is recognized over. (The compiled promise does not make flows, so this is asked of the old code only.)
    pub fn first_occurrence(&self, id: Id<Contract>, from: Day) -> Vec<String> {
        let contract = &self.book.contracts[id];
        let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
        let mut lines = Vec::new();
        for schedule in [ScheduleKind::Regular, ScheduleKind::Standing] {
            if contract.terms_on_schedule(schedule, Day::MIN).is_none() {
                continue;
            }
            let first = contract.occurrences(Days::new(from, Day::MAX).expect("days")).find(|o| o.schedule == schedule);
            let Some(first) = first else { continue };
            let mut ledger = self.plan.start(Options { today, relaxed: false });
            let (mut flows, mut details, mut missing) = (Vec::new(), axiom_core::Arena::new(), Vec::new());
            let made =
                ledger.instantiate_occurrence(id, schedule, first.day, 0, None, &mut flows, &mut details, &mut missing);
            let said = |flow: &axiom_model::RuntimeFlow| {
                format!(
                    "{:?} -> {:?} recognized {}",
                    flow.flow.out,
                    flow.flow.arrive,
                    crate::span(flow.flow.recognized)
                )
            };
            let shown = match made {
                Ok(_) => flows.iter().map(said).collect::<Vec<_>>().join("; "),
                Err(error) => format!("{error:?}"),
            };
            lines.push(format!("first {} {} : {shown}", kind(schedule), show(first.day)));
        }
        lines
    }
}

impl Reading for Old<'_, '_, '_> {
    fn due(&self, contract: Id<Contract>, window: Days) -> Vec<(Day, ScheduleKind)> {
        self.book.contracts[contract]
            .occurrences(window)
            .map(|occurrence| (occurrence.day, occurrence.schedule))
            .collect()
    }

    /// The engine's own count (`post_written_occurrence`), made of the same `Contract::occurrences`.
    fn ordinal(&self, contract: Id<Contract>, schedule: ScheduleKind, due: Day) -> Option<u32> {
        let contract = &self.book.contracts[contract];
        let through = Days::new(contract.days.first(), due).unwrap_or(Days::on(due));
        let count = contract
            .occurrences(through)
            .filter(|occurrence| occurrence.schedule == schedule && occurrence.day <= due)
            .count();
        count.checked_sub(1).and_then(|index| u32::try_from(index).ok())
    }

    fn keep(&self, contract: Id<Contract>, day: Day) -> Keep {
        match nearest_occurrence(&self.book.contracts[contract], day) {
            Ok(Some((schedule, due, _))) => Keep::Kept(schedule, due),
            Ok(None) => Keep::Out,
            Err((regular, standing)) => Keep::Ambiguous(regular, standing),
        }
    }

    fn factor(&self, contract: Id<Contract>, schedule: ScheduleKind, day: Day) -> Result<Ratio, ForecastError> {
        self.book.contracts[contract].amount_on_schedule(self.book, schedule, day)
    }

    fn recognized(
        &self,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Option<Result<Days, ForecastError>> {
        let contract = &self.book.contracts[contract];
        let terms = contract.terms_on_schedule(schedule, day)?;
        let template = terms.template.first()?;
        Some(contract.recognition_on_schedule(&template.header.flow, schedule, day))
    }

    fn payment(&self, id: Id<Contract>, from: Day) -> Payment {
        let contract = &self.book.contracts[id];
        let Some(first) = contract.occurrences(Days::new(from, Day::MAX).expect("days")).next() else {
            return Payment::NoDueDay;
        };
        let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
        let mut ledger = self.plan.start(Options { today, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), axiom_core::Arena::new(), Vec::new());
        let made = ledger.instantiate_occurrence(
            id,
            first.schedule,
            first.day,
            0,
            None,
            &mut flows,
            &mut details,
            &mut missing,
        );
        match (made, flows.first()) {
            (Ok(_), Some(flow)) => Payment::Pays(first.day, flow.flow.out),
            (Err(error), _) if format!("{error:?}").contains("UnsupportedLoan") => Payment::Unsupported(first.day),
            (made, _) => Payment::Other(format!("{} {made:?}", show(first.day))),
        }
    }
}

/// The compiled promise: `Promises`, the arithmetic of `Dues`, the term walker.
#[cfg(feature = "new")]
pub struct New<'b, 's> {
    pub book: &'b Book<'s>,
}

#[cfg(feature = "new")]
impl Reading for New<'_, '_> {
    fn due(&self, contract: Id<Contract>, window: Days) -> Vec<(Day, ScheduleKind)> {
        let days = |kind| {
            let schedule = self.book.promises.schedule(contract, kind);
            schedule.map(|schedule| schedule.days(window).collect::<Vec<_>>()).unwrap_or_default()
        };
        merged(&days(ScheduleKind::Regular), &days(ScheduleKind::Standing))
    }

    fn ordinal(&self, contract: Id<Contract>, schedule: ScheduleKind, due: Day) -> Option<u32> {
        self.book.promises.schedule(contract, schedule)?.ordinal(due)
    }

    fn keep(&self, contract: Id<Contract>, day: Day) -> Keep {
        use axiom_model::promise::Keep as Kept;
        match self.book.promises.keep(contract, day) {
            Kept::Outside => Keep::Out,
            Kept::Kept { schedule, due } => Keep::Kept(schedule, due),
            Kept::Ambiguous { regular, standing } => Keep::Ambiguous(regular, standing),
        }
    }

    fn factor(&self, contract: Id<Contract>, schedule: ScheduleKind, day: Day) -> Result<Ratio, ForecastError> {
        match self.book.promises.schedule(contract, schedule) {
            Some(schedule) => schedule.factor(self.book, day),
            None => Err(ForecastError::OutsideContract(day)),
        }
    }

    fn recognized(
        &self,
        contract: Id<Contract>,
        schedule: ScheduleKind,
        day: Day,
    ) -> Option<Result<Days, ForecastError>> {
        Some(self.book.promises.schedule(contract, schedule)?.recognized(day))
    }

    fn payment(&self, id: Id<Contract>, from: Day) -> Payment {
        let promises = &self.book.promises;
        let first = |kind| promises.schedule(id, kind).and_then(|schedule| schedule.nth(schedule.before(from)));
        let day = match (first(ScheduleKind::Regular), first(ScheduleKind::Standing)) {
            (Some(regular), Some(standing)) if standing < regular => standing,
            (Some(regular), _) => regular,
            (None, Some(standing)) => standing,
            (None, None) => return Payment::NoDueDay,
        };
        let stream = promises.of(id).regular;
        let body = stream.and_then(|stream| match promises.term(stream.every) {
            axiom_model::promise::Term::Every { body, .. } => Some(body),
            _ => None,
        });
        match body.and_then(|body| promises.annuity_of(body)) {
            Some(annuity) => Payment::Pays(day, promises.annuity(annuity).payment()),
            None => Payment::Unsupported(day),
        }
    }
}

/// Two sorted streams of due days as `Contract::occurrences` merges them: a standing day only if it is strictly earlier.
#[cfg(feature = "new")]
fn merged(regular: &[Day], standing: &[Day]) -> Vec<(Day, ScheduleKind)> {
    let (mut regular, mut standing) = (regular.iter().peekable(), standing.iter().peekable());
    let mut days = Vec::new();
    loop {
        match (regular.peek(), standing.peek()) {
            (Some(&&r), Some(&&s)) if s < r => days.extend(standing.next().map(|&day| (day, ScheduleKind::Standing))),
            (Some(_), _) => days.extend(regular.next().map(|&day| (day, ScheduleKind::Regular))),
            (None, Some(_)) => days.extend(standing.next().map(|&day| (day, ScheduleKind::Standing))),
            (None, None) => return days,
        }
    }
}
