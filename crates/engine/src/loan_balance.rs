//! A statement of what a loan owes, held to the loan's schedule.
//!
//! `31 mortgage = 310_978.17 USD` says what the lender says is owed, and it is an assertion on the loan's debt tab like any
//! other: the book's flows must add up to it. A loan also has a schedule, what its terms and every payment, prepayment
//! and rate the book states make of it (`axiom_model::promise::Amortization`), and a statement is held to that too. The two
//! checks say different things. The tab's is that the **flows** add up to the statement. The schedule's is that the **terms**
//! do, so that the next payment's interest and principal are the lender's; a loan whose payments are all written down and
//! whose schedule is wrong (a rate the lender changed, a payment that was missed) agrees with the bank in the book and
//! disagrees in the schedule.
//!
//! Where both disagree by the same amount the book is consistent with the schedule and the statement is what differs, so the
//! loan's diagnostic is the one said (it names the likely cause, which the tab's generic "a flow is missing" cannot). Where
//! only one disagrees, that one is said. A statement that accepts its gap (`!`, `via`) is not held to the schedule: it has
//! said it does not add up.

use axiom_core::{Day, Diagnostic, Id, Loc, Qty};
use axiom_model::promise::{Cause, Disagreement, Entry, Kind};
use axiom_model::{Amount, Assert, Book, Contract, Gap, Role};

use crate::ledger::Ledger;
use crate::show;

impl Ledger<'_, '_, '_> {
    /// How the statement of `assert` disagrees with the schedule of the loan whose debt tab it is about, if it does and the
    /// statement has not accepted its gap.
    pub(crate) fn schedule_disagrees(&self, assert: &Assert) -> Option<(Id<Contract>, Disagreement)> {
        let book = self.plan.book;
        if !matches!(assert.gap, Gap::Refused) || !matches!(book.places[assert.place].role, Role::Tab(_)) {
            return None;
        }
        let loan_of = |(id, contract): (Id<Contract>, &Contract)| {
            contract.loan.filter(|loan| loan.debt == assert.place).map(|loan| (id, loan))
        };
        let (contract, loan) = book.contracts.iter().find_map(loan_of)?;
        let found = book.promises.reconcile(book, contract, assert.day, assert.amount.qty);
        found.filter(|_| assert.amount.unit == loan.principal.unit).map(|found| (contract, found))
    }
}

/// The diagnostic a statement that the schedule contradicts makes: both numbers, the day, the payments around it and the
/// cause that explains it exactly, with the edit that mends it where there is one.
pub(crate) fn disagreement(book: &Book, assert: &Assert, contract: Id<Contract>, found: &Disagreement) -> Diagnostic {
    let story = Story { book, assert, contract, found };
    let (more, gap) = if found.gap() > Qty::ZERO { ("more", found.gap()) } else { ("less", -found.gap()) };
    let headline = format!(
        "`{}` owes {} on {} by its schedule, and the statement says {}",
        story.name(),
        story.money(found.owed),
        found.day,
        story.money(found.stated)
    );
    let d = Diagnostic::error("loan-balance", headline)
        .label(assert.loc, format!("{} {more} than the schedule", story.money(gap)));
    let d = last_entries(book, contract, found.day).into_iter().fold(d, |d, note| d.note(note));
    story.cause(d)
}

/// What a note about a statement needs to say: the book's words for the loan and its money, and where an edit goes.
struct Story<'a, 'b> {
    book: &'a Book<'b>,
    assert: &'a Assert,
    contract: Id<Contract>,
    found: &'a Disagreement,
}

impl Story<'_, '_> {
    fn name(&self) -> &str {
        self.book.name(self.book.contracts[self.contract].name)
    }

    fn money(&self, qty: Qty) -> String {
        self.book.show(Amount::new(qty, self.assert.amount.unit)).to_string()
    }

    /// A line of the journal, to be written just before the statement.
    fn before(&self, line: String) -> (Loc, String) {
        let at = self.assert.loc;
        (Loc::new(at.file, at.start, at.start), format!("{line}\n"))
    }

    /// What explains the difference, said in the book's words and mended by an edit where one mends it.
    fn cause(&self, d: Diagnostic) -> Diagnostic {
        match &self.found.cause {
            Cause::Missed(days) => self.missed(d, days),
            Cause::Short(days) => self.short(d, days),
            Cause::Extra(days) => self.extra(d, days),
            Cause::Prepaid => self.prepaid(d),
            Cause::Several(tied) => self.several(d, tied),
            Cause::Unknown => self.unknown(d),
        }
    }

    fn missed(&self, d: Diagnostic, days: &[(Day, Qty)]) -> Diagnostic {
        let many = days.len() > 1;
        let named = |(due, principal): &(Day, Qty)| {
            if many { format!("{due} ({})", self.money(*principal)) } else { due.to_string() }
        };
        let d = d.note(format!(
            "likely cause: a payment was missed: the {} due {} would have paid off exactly the {} of principal that is the difference, and no line of the journal keeps {}",
            if many { "payments" } else { "payment" },
            list(days.iter().map(named)),
            self.money(self.found.gap()),
            if many { "them" } else { "it" }
        ));
        let name = self.name();
        days.iter().fold(d, |d, &(due, _)| {
            let (at, kept) = self.before(format!("{due} {name}"));
            let (_, waived) = self.before(format!("{due} {name} waived"));
            d.fix(format!("write the payment of {due}, if it was made"), at, kept).fix(
                format!("or say the payment of {due} was not owed"),
                at,
                waived,
            )
        })
    }

    fn short(&self, d: Diagnostic, days: &[(Day, Qty)]) -> Diagnostic {
        let lines = list(days.iter().map(|(due, principal)| format!("the line of {due} ({})", self.money(*principal))));
        d.note(format!(
            "likely cause: a payment was short: {} is exactly the principal that {lines} left unpaid",
            self.money(self.found.gap())
        ))
        .help("write what was paid with the rest, or what the lender took of it, as the amount of the line")
    }

    fn extra(&self, d: Diagnostic, days: &[(Day, Qty)]) -> Diagnostic {
        let lines = list(days.iter().map(|(due, extra)| format!("the line of {due} ({})", self.money(*extra))));
        d.note(format!(
            "likely cause: an extra was counted as principal: {lines} states more than its payment, and the schedule took {} as principal",
            if days.len() == 1 { "it" } else { "them" }
        ))
        .help("if it paid an escrow, write it as `also -> escrow AMOUNT #escrow` on the contract and the line without its amount")
    }

    fn prepaid(&self, d: Diagnostic) -> Diagnostic {
        let (assert, found) = (self.assert, self.found);
        let amount = self.book.show(Amount::new(-found.gap(), assert.amount.unit)).to_string().replace(',', "_");
        let from = paid_from(self.book, self.contract);
        let (at, line) = self.before(format!("{} {from} -> {} {amount}", found.day, self.name()));
        d.note(format!(
            "likely cause: a prepayment nobody wrote: {} is exactly what the schedule owes beyond the statement",
            self.money(-found.gap())
        ))
        .fix("record the prepayment, from the account it was paid from", at, line)
    }

    fn several(&self, d: Diagnostic, tied: &[Cause]) -> Diagnostic {
        let (found, name) = (self.found, self.name());
        d.note(format!(
            "no cause is named: {} explain the {} alike, so the statement alone cannot say which",
            list(tied.iter().map(|cause| self.about(cause))),
            self.money(found.gap())
        ))
        .help(format!(
            "write what happened: the payment, or `{} {name} now at RATE` if the lender changed the rate",
            found.day
        ))
    }

    fn unknown(&self, d: Diagnostic) -> Diagnostic {
        let (found, name) = (self.found, self.name());
        d.note(format!(
            "no cause explains it: {} is not the principal of the payments no line keeps, of the payments that were short, or of an extra, and the statement does not say less is owed",
            self.money(found.gap())
        ))
        .help(format!("if the lender changed the rate, write `{} {name} now at RATE`", found.day))
    }

    /// One cause, in a clause: what it says happened.
    fn about(&self, cause: &Cause) -> String {
        let lines = |days: &[(Day, Qty)]| list(days.iter().map(|(due, qty)| format!("{due} ({})", self.money(*qty))));
        match cause {
            Cause::Missed(days) if days.len() > 1 => format!("the payments due {} that no line keeps", lines(days)),
            Cause::Missed(days) => format!("the payment due {} that no line keeps", lines(days)),
            Cause::Short(days) if days.len() > 1 => {
                format!("the lines of {} that pay less than their payment", lines(days))
            }
            Cause::Short(days) => format!("the line of {} that pays less than its payment", lines(days)),
            Cause::Extra(days) if days.len() > 1 => {
                format!("the lines of {} that state more than their payment", lines(days))
            }
            Cause::Extra(days) => format!("the line of {} that states more than its payment", lines(days)),
            Cause::Prepaid => "a prepayment nobody wrote".to_owned(),
            Cause::Several(_) | Cause::Unknown => String::new(),
        }
    }
}

/// What the schedule did last on or before `day`, and the payment before it if that was a prepayment, so that the reader sees
/// the number the statement is held to and how it was reached.
fn last_entries(book: &Book, contract: Id<Contract>, day: Day) -> Vec<String> {
    let Some(loan) = book.promises.loan(contract) else { return Vec::new() };
    let unit = loan.terms().principal().unit;
    let money = |qty: Qty| book.show(Amount::new(qty, unit)).to_string();
    let entries: Vec<_> = loan.entries().iter().take_while(|entry| entry.day <= day).collect();
    let said = |entry: &&Entry| match entry.kind {
        Kind::Pay => format!(
            "the schedule's payment of {} paid {} of interest and {} of principal and left {} owed",
            entry.day,
            money(entry.paid.interest),
            money(entry.paid.principal),
            money(entry.paid.open)
        ),
        Kind::Prepay => format!(
            "the schedule's prepayment of {} paid off {} of principal and left {} owed",
            entry.day,
            money(entry.paid.principal),
            money(entry.paid.open)
        ),
    };
    let from = match entries.as_slice() {
        [.., before, last] if last.kind == Kind::Prepay => entries.len() - 2 + usize::from(before.kind == Kind::Prepay),
        _ => entries.len().saturating_sub(1),
    };
    entries[from..].iter().map(said).collect()
}

/// The account a loan is paid from: the holding of its schedule.
fn paid_from<'a>(book: &Book<'a>, contract: Id<Contract>) -> &'a str {
    let from = book.contracts[contract]
        .terms
        .as_ref()
        .and_then(|terms| terms.template.first())
        .map(|group| group.header.flow.from);
    from.map_or("ACCOUNT", |place| show::place(book, place))
}

/// `a`, `a and b`, `a, b and c`.
fn list(items: impl Iterator<Item = String>) -> String {
    let items: Vec<_> = items.collect();
    match items.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}
