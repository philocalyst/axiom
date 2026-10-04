//! Why a statement of what a loan owes disagrees with its schedule.
//!
//! `31 mortgage = 310_978.17 USD` says what the lender says is owed. The schedule says what the loan's terms and the book's events
//! make of it. When they differ, a difference of cents is rarely noise: it is a payment nobody wrote, a payment that was
//! short, a prepayment nobody recorded, or an extra written into a payment's amount. Each of those makes a difference of an
//! exact size, so each is an integer test against the schedule, and a statement is blamed on one only when **exactly one**
//! explains it to the quantum. Two that tie are not guessed between (they are said to tie), and neither is a difference that none
//! explains.
//!
//! | difference (statement less schedule) | explained by |
//! |---|---|
//! | below nothing | a prepayment nobody wrote, of exactly the difference |
//! | the principal of the payments due by the day that no line keeps | those payments were **missed** |
//! | the principal that the lines that state less than their payment left unpaid | those payments were **short** |
//! | what one line states over its payment, or what all of them do | the schedule took an **extra** (an escrow in the draft) as principal |
//!
//! Interest accrued since the last payment (a payoff quote), a rate nobody wrote and a draw have no exact test and are not
//! candidates: the statement alone cannot say which of them it is.

use axiom_core::{Day, Id, Qty};

use super::Promises;
use super::amortization::{Amortization, Said};
use crate::book::{Book, Contract};

/// The one thing that explains a difference, or that nothing, or more than one thing, does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Cause {
    /// Payments due that no line keeps, each with the principal it would have paid off.
    Missed(Vec<(Day, Qty)>),
    /// Payments a line keeps with less than the payment, each with the principal left unpaid.
    Short(Vec<(Day, Qty)>),
    /// Lines that state more than their payment, each with the excess the schedule took as principal.
    Extra(Vec<(Day, Qty)>),
    /// The schedule owes more than the statement says: a prepayment nobody wrote.
    Prepaid,
    /// More than one of the above explains it to the quantum alike, so none is named.
    Several(Vec<Cause>),
    /// None explains it to the quantum.
    Unknown,
}

/// A statement of what is owed on a day, and what the schedule says, which are not the same.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Disagreement {
    pub day: Day,
    pub stated: Qty,
    pub owed: Qty,
    pub cause: Cause,
}

impl Disagreement {
    /// How much more the statement says is owed than the schedule does.
    pub fn gap(&self) -> Qty {
        self.stated - self.owed
    }
}

impl Promises {
    /// What `stated` being owed on `day` of the loan of `contract` disagrees with, if it does: none for a contract with no
    /// loan, for a day before the loan was made, and for a statement that agrees with the schedule.
    pub fn reconcile(&self, book: &Book<'_>, contract: Id<Contract>, day: Day, stated: Qty) -> Option<Disagreement> {
        let loan = self.loan(contract)?;
        // What the book says of the loan is read off every transaction, so only a statement that disagrees pays for it.
        (loan.open_on(day)? != stated)
            .then(|| loan.disagreement(&Said::of(book, contract, &book.contracts[contract]), day, stated))?
    }
}

impl Amortization<'_> {
    /// How the schedule on `day` disagrees with a statement of `stated`, and why, if it does.
    pub fn disagreement(&self, said: &Said, day: Day, stated: Qty) -> Option<Disagreement> {
        let owed = self.open_on(day)?;
        let gap = stated - owed;
        let cause = match gap {
            gap if gap == Qty::ZERO => return None,
            gap if gap < Qty::ZERO => Cause::Prepaid,
            gap => self.explain(said, day, gap),
        };
        Some(Disagreement { day, stated, owed, cause })
    }

    /// The cause of a statement that says `gap` more is owed than the schedule does, if exactly one explains it.
    fn explain(&self, said: &Said, day: Day, gap: Qty) -> Cause {
        let (mut missed, mut short, mut extra) = (Vec::new(), Vec::new(), Vec::new());
        for payment in self.payments().filter(|payment| (said.begins..=day).contains(&payment.day)) {
            let paid = payment.paid;
            let due = payment.day;
            match said.lines.binary_search_by_key(&due, |&(day, _)| day).ok().map(|at| said.lines[at].1) {
                None => missed.push((due, paid.principal)),
                Some(Some(stated)) if stated < paid.interest + paid.principal => {
                    let principal = (stated - paid.interest).clamp(Qty::ZERO, paid.principal);
                    short.push((due, paid.principal - principal));
                }
                Some(Some(stated)) if stated > paid.interest + paid.principal => {
                    extra.push((due, stated - paid.interest - paid.principal));
                }
                Some(_) => {}
            }
        }
        let sum = |lines: &[(Day, Qty)]| lines.iter().map(|line| line.1).sum::<Qty>();
        let one_line = extra.iter().find(|line| line.1 == gap).map(|&line| vec![line]);
        let holds = [
            (!missed.is_empty() && sum(&missed) == gap).then_some(Cause::Missed(missed)),
            (!short.is_empty() && sum(&short) == gap).then_some(Cause::Short(short)),
            one_line.or_else(|| (!extra.is_empty() && sum(&extra) == gap).then_some(extra)).map(Cause::Extra),
        ];
        let mut found: Vec<_> = holds.into_iter().flatten().collect();
        match found.len() {
            0 => Cause::Unknown,
            1 => found.remove(0),
            _ => Cause::Several(found),
        }
    }
}
