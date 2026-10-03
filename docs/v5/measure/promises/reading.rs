//! What a build of the fold says: the questions of the module above, answered through the compiled promise and, where
//! the question is the engine's, through the engine.

use axiom_core::{Day, Days, Id, Ratio};
use axiom_engine::{Options, Plan};
use axiom_model::{Amount, Book, Contract, ForecastError, ScheduleKind};

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

/// What a build says: the compiled promise, and the engine where it is the engine that is asked.
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

/// The fold: the compiled promise the fold, the lowering and the reports ask, and the engine's own materializer.
pub struct Fold<'a, 'b, 's> {
    pub book: &'b Book<'s>,
    pub plan: &'a Plan<'b, 's>,
}

impl Fold<'_, '_, '_> {
    /// The first due day of each schedule on or after `from`, regular first: what the forecast's driver asks.
    fn first_due(&self, id: Id<Contract>, from: Day) -> Vec<(ScheduleKind, Day)> {
        let promises = &self.book.promises;
        let each = [ScheduleKind::Regular, ScheduleKind::Standing].into_iter();
        each.filter_map(|kind| {
            let schedule = promises.schedule(id, kind)?;
            Some((kind, schedule.nth(schedule.before(from))?))
        })
        .collect()
    }

    /// What the fold makes of the first due day of each schedule, as the forecast would: its flows, where they go, the
    /// days each is recognized over.
    pub fn first_occurrence(&self, id: Id<Contract>, from: Day) -> Vec<String> {
        let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
        let mut lines = Vec::new();
        for (schedule, due) in self.first_due(id, from) {
            let mut ledger = self.plan.start(Options { today, relaxed: false });
            let (mut flows, mut details, mut missing) = (Vec::new(), axiom_core::Arena::new(), Vec::new());
            let ordinal = self.book.promises.schedule(id, schedule).and_then(|found| found.ordinal(due)).unwrap_or(0);
            let made = ledger.instantiate_occurrence(id, schedule, due, ordinal, None, &mut flows, &mut details, &mut missing);
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
            lines.push(format!("first {} {} : {shown}", kind(schedule), show(due)));
        }
        lines
    }
}

impl Reading for Fold<'_, '_, '_> {
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
        self.book.contracts[contract].terms_of(schedule)?.template.first()?;
        Some(self.book.promises.schedule(contract, schedule)?.recognized(day))
    }

    /// The engine's own answer: it materializes the first due day, the way the forecast does.
    fn payment(&self, id: Id<Contract>, from: Day) -> Payment {
        let due = self.first_due(id, from).into_iter().min_by_key(|(kind, day)| (*day, *kind == ScheduleKind::Standing));
        let Some((schedule, first)) = due else { return Payment::NoDueDay };
        let today = Day::from_ymd(TODAY.0, TODAY.1, TODAY.2).expect("a date");
        let mut ledger = self.plan.start(Options { today, relaxed: false });
        let (mut flows, mut details, mut missing) = (Vec::new(), axiom_core::Arena::new(), Vec::new());
        let ordinal = self.book.promises.schedule(id, schedule).and_then(|found| found.ordinal(first)).unwrap_or(0);
        let made = ledger.instantiate_occurrence(id, schedule, first, ordinal, None, &mut flows, &mut details, &mut missing);
        match (made, flows.first()) {
            (Ok(_), Some(flow)) => Payment::Pays(first, flow.flow.out),
            (Err(error), _) if format!("{error:?}").contains("UnsupportedLoan") => Payment::Unsupported(first),
            (made, _) => Payment::Other(format!("{} {made:?}", show(first))),
        }
    }
}

/// Two sorted streams of due days as the old merge took them: a standing day only if it is strictly earlier.
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
