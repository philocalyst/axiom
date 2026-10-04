//! A loan's schedule: what is owed after every payment and prepayment the book states, worked out once.
//!
//! [`Annuity::step`] is what one event does. This module is the one place that calls it for a loan, over the events a book
//! says in the order the days put them, so that everything that asks of a loan (the amount an occurrence posts, the balance
//! on a statement's day, whether another payment is owed, a report) reads one answer and not a state each of them walked.
//!
//! # Why a table, and in this order
//!
//! A loan's interest depends on the order its events happen in: a payment on the 1st is of the rate before a reset on the
//! 2nd and of the balance before a prepayment on the 1st. The fold meets a loan's facts in a different order than that: a
//! line is kept before its due day or after it, a miss is found a reach later, a statement asks for the balance of a day
//! whose payments the monitor has not yet stepped. A loan walked as the fold meets it would depend on how it was met. A loan
//! walked **here** depends on the book alone, so a line kept early, a miss found late, a forecast from today and a statement
//! on any day agree.
//!
//! The events, in order of day and, on one day, of rank:
//!
//! 0. a reset (`resets EVERY from DATE to INDEX + MARGIN`: the index read that day),
//! 1. a rate a statement says (`DATE LOAN now at 6.25%`),
//! 2. a payment (each owed day of the loan's schedule after the day it was made),
//! 3. a prepayment: a flow into the loan's debt tab that no occurrence made, and what an occurrence line that states its own
//!    amount pays over the payment of its due day.
//!
//! So a rate that changes on a due day is that day's rate, and a prepayment on a due day comes after that day's payment. A
//! prepayment between due days takes effect at once (the next payment's interest is on what is then owed: interest has no
//! day count here). What a loan is paid off by ends the walk, and an event after it is ignored.
//!
//! An [`Entry`] is 32 bytes and every loan's entries are one run of one pool of the [`Promises`](super::Promises), as every
//! variable-length thing of a promise is; the schedule is a `Run<Entry>` in the [`Annuity`]. A loan of 360 payments is 11
//! KB, a book with no loan has none, and none of it is walked again.

use axiom_core::{Day, Id, Qty, Ratio, Span};

use super::annuity::{Annuity, Event, Paid};
use crate::book::{Book, Contract, ForecastError, Loan, ScheduleKind};
use crate::journal::{Flow, Infer, Mode, Txn, WrittenOccurrence};
use crate::split::Expr;

/// What an entry of the schedule is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A scheduled payment.
    Pay,
    /// Principal paid beyond it.
    Prepay,
}

/// One payment or prepayment of a loan, on the day it falls: what it paid and what is owed after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub day: Day,
    pub kind: Kind,
    pub paid: Paid,
}

const _: () = assert!(size_of::<Entry>() <= 32);

/// Where a schedule stopped for want of something the book does not say, and what it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Halt {
    pub day: Day,
    pub error: ForecastError,
}

/// What the book says happens to one loan besides its payments falling due, each in the order of its day.
#[derive(Default, Debug)]
pub struct Said {
    /// A rate a statement says, from a day.
    pub rates: Vec<(Day, Ratio)>,
    /// Principal paid by a flow into the debt tab.
    pub prepaid: Vec<(Day, Qty)>,
    /// The lines that keep a due day, by due day, in order, each with the amount it states if it states one.
    pub lines: Vec<(Day, Option<Qty>)>,
    /// The first day of the book. A payment due before it is not the book's: no line can keep it (a line is a fact, and this
    /// is the first), so it is not one that was missed.
    pub begins: Day,
}

impl Said {
    /// What the book says of the loan of `contract`: the rates its statements say, the flows into its debt tab that are not
    /// its own payments, and the lines that keep its due days.
    pub(super) fn of(book: &Book<'_>, id: Id<Contract>, contract: &Contract) -> Said {
        let Some(loan) = contract.loan else { return Said::default() };
        let begins = book.first_fact().unwrap_or(Day::MIN);
        Said {
            prepaid: Said::paid_into(book, id, &loan),
            lines: Said::kept(book, id, &loan),
            begins,
            ..Said::rated(contract)
        }
    }

    /// What is said of the loan of `contract` while the journal is being lowered, before any fact of it: only the rates
    /// its statements have said so far.
    pub(super) fn rated(contract: &Contract) -> Said {
        let rates = contract.rates.iter().map(|change| (change.day, change.rate)).collect();
        Said { rates, ..Said::default() }
    }

    /// What was paid into the loan's debt tab by a flow of the journal: all of it is principal. (An occurrence's flows are made when
    /// it is posted and are in no book, so none of them is counted twice.)
    /// A tab is one for the lender and the owner, so two loans of the same pair share it, and the earlier takes what is paid.
    fn paid_into(book: &Book<'_>, id: Id<Contract>, loan: &Loan) -> Vec<(Day, Qty)> {
        let first = book.contracts.iter().find(|(_, other)| other.loan.is_some_and(|other| other.debt == loan.debt));
        let paid_in = |flow: &&Flow| {
            flow.to == loan.debt
                && flow.from != loan.debt
                && flow.mode == Mode::Actual
                && flow.infer == Infer::Known
                && flow.arrive.unit == loan.principal.unit
        };
        let flows = book.touching[loan.debt].iter().map(|&flow| &book.flows[flow]);
        let own = first.is_some_and(|(first, _)| first == id);
        flows.filter(|_| own).filter(paid_in).map(|flow| (flow.day, flow.arrive.qty)).collect()
    }

    /// The lines of the journal that keep a due day of the loan, and what each states of its amount if it is a number in the
    /// loan's commodity.
    fn kept(book: &Book<'_>, id: Id<Contract>, loan: &Loan) -> Vec<(Day, Option<Qty>)> {
        let of_the_loan = |txn: &&Txn| txn.contract == Some(id) && txn.contract_schedule == Some(ScheduleKind::Regular);
        let written =
            book.txns.values().filter(of_the_loan).filter_map(|txn| book.written_occurrences.get(txn.occurrence?));
        let amount = |written: &WrittenOccurrence| match written.amount {
            Some(Expr::Literal(amount)) if amount.unit == loan.principal.unit => Some(amount.qty),
            _ => None,
        };
        let mut lines: Vec<_> = written.map(|written| (written.due, amount(written))).collect();
        lines.sort_by_key(|&(due, _)| due);
        lines.dedup_by_key(|&mut (due, _)| due);
        lines
    }

    /// What the line that keeps `due` states it pays, if it states a number.
    pub fn stated(&self, due: Day) -> Option<Qty> {
        let at = self.lines.binary_search_by_key(&due, |&(day, _)| day).ok()?;
        self.lines[at].1
    }

    /// What the line that keeps `due` pays over `paid`, if it states an amount and it is more than the payment.
    fn over(&self, due: Day, paid: Paid) -> Option<Qty> {
        let extra = self.stated(due)? - paid.interest - paid.principal;
        (extra > Qty::ZERO).then_some(extra)
    }
}

/// The order events fall in on one day.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Reset,
    Rate,
    Pay,
    Prepay,
}

impl Rank {
    /// What an event of this rank is in the schedule, if it is something paid.
    fn entry(self) -> Option<Kind> {
        match self {
            Rank::Pay => Some(Kind::Pay),
            Rank::Prepay => Some(Kind::Prepay),
            Rank::Reset | Rank::Rate => None,
        }
    }
}

/// What falls on a day: an event, or a reset, which is the event the index makes of what it reads then.
#[derive(Clone, Copy)]
enum Input {
    Known(Event),
    Index,
}

/// One input, and the day and rank it falls on.
#[derive(Clone, Copy)]
struct Fall {
    day: Day,
    rank: Rank,
    input: Input,
}

/// What a walk made: the schedule and, if the events could not be followed to the end, why not.
pub(super) struct Walked {
    pub entries: Vec<Entry>,
    pub halted: Option<Halt>,
}

/// Walks the loan over every event the book states. `dues` are the owed days after the loan was made, as many as it has
/// payments, in order; `index` says what an index reads on a day, which a reset needs.
pub(super) fn walk(
    annuity: &Annuity,
    dues: &[Day],
    said: &Said,
    index: impl Fn(Day) -> Result<Ratio, ForecastError>,
) -> Walked {
    let mut falls = falls(annuity, dues, said);
    falls.sort_by_key(|fall| (fall.day, fall.rank));
    let (mut state, mut entries) = (annuity.start(), Vec::new());
    for fall in falls.into_iter().filter(|fall| fall.day >= annuity.begins()) {
        if state.open == Qty::ZERO {
            break;
        }
        let event = match fall.input {
            Input::Known(event) => event,
            Input::Index => match index(fall.day) {
                Ok(value) => Event::Reset(value),
                Err(error) => return Walked { entries, halted: Some(Halt { day: fall.day, error }) },
            },
        };
        let (next, paid) = annuity.step(state, event);
        state = next;
        entries.extend(fall.rank.entry().map(|kind| Entry { day: fall.day, kind, paid }));
        // A line that states more than the payment pays the difference off the principal, after the payment.
        let extra = said.over(fall.day, paid).filter(|_| fall.rank == Rank::Pay && state.open > Qty::ZERO);
        if let Some(extra) = extra {
            let (next, paid) = annuity.step(state, Event::Prepay(extra));
            state = next;
            entries.push(Entry { day: fall.day, kind: Kind::Prepay, paid });
        }
    }
    Walked { entries, halted: None }
}

/// Every event of the walk, unordered: the payments, what the book says, and the resets until the last payment.
fn falls(annuity: &Annuity, dues: &[Day], said: &Said) -> Vec<Fall> {
    let mut falls = Vec::with_capacity(dues.len() + said.rates.len() + said.prepaid.len());
    let known = |day, rank, event| Fall { day, rank, input: Input::Known(event) };
    falls.extend(dues.iter().map(|&due| known(due, Rank::Pay, Event::Pay)));
    falls.extend(said.rates.iter().map(|&(day, rate)| known(day, Rank::Rate, Event::Rate(rate))));
    falls.extend(said.prepaid.iter().map(|&(day, amount)| known(day, Rank::Prepay, Event::Prepay(amount))));
    if let (Some(resets), Some(&last)) = (annuity.resets(), dues.last()) {
        let days = (0..).map_while(|number| resets.from.checked_add(times(resets.every, number)));
        falls.extend(days.take_while(|&day| day <= last).map(|day| Fall {
            day,
            rank: Rank::Reset,
            input: Input::Index,
        }));
    }
    falls
}

/// A span, `number` times: each reset is counted from the first, never from the one before, as the schedule counts its days.
fn times(span: Span, number: i32) -> Span {
    Span { months: span.months.saturating_mul(number), days: span.days.saturating_mul(number) }
}

/// A loan's schedule as a reader sees it: the terms, and what the book says happens to it.
#[derive(Clone, Copy)]
pub struct Amortization<'p> {
    annuity: &'p Annuity,
    entries: &'p [Entry],
}

impl<'p> Amortization<'p> {
    pub(super) fn new(annuity: &'p Annuity, pool: &'p [Entry]) -> Amortization<'p> {
        let entries = annuity.entries().get(pool).unwrap_or_default();
        Amortization { annuity, entries }
    }

    pub fn terms(&self) -> &'p Annuity {
        self.annuity
    }

    /// Every payment and prepayment, in order.
    pub fn entries(&self) -> &'p [Entry] {
        self.entries
    }

    /// What the payment due on `due` pays, or why the schedule has none: the day is not one of its payments, or it stopped
    /// before it.
    pub fn paid_on(&self, due: Day) -> Result<Paid, ForecastError> {
        if let Some(halt) = self.annuity.halted().filter(|halt| due >= halt.day) {
            return Err(halt.error);
        }
        let from = self.entries.partition_point(|entry| entry.day < due);
        let payment =
            self.entries[from..].iter().take_while(|entry| entry.day == due).find(|entry| entry.kind == Kind::Pay);
        payment.map(|entry| entry.paid).ok_or(ForecastError::UnsupportedLoan(due))
    }

    /// What is owed on `day`, after every payment and prepayment on or before it; none before the loan was made.
    pub fn open_on(&self, day: Day) -> Option<Qty> {
        let principal = self.annuity.principal().qty;
        (day >= self.annuity.begins()).then(|| {
            let last = self.entries.partition_point(|entry| entry.day <= day).checked_sub(1);
            last.map_or(principal, |last| self.entries[last].paid.open)
        })
    }

    /// What is owed when `day` begins: after everything before it, and all of it on the day the loan was made or before.
    pub fn owed_before(&self, day: Day) -> Qty {
        self.open_on(day.add_days(-1)).unwrap_or(self.annuity.principal().qty)
    }

    /// The payments, in order, each with the day it is due.
    pub fn payments(&self) -> impl Iterator<Item = &'p Entry> {
        self.entries.iter().filter(|entry| entry.kind == Kind::Pay)
    }
}
