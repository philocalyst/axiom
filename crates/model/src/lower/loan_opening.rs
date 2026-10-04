//! A loan made before the book began opens its debt with what its terms say is owed when the book begins.
//!
//! `loan 320_000 USD on 2024-02-20 ...` in a book whose first fact is on 2026-01-01 has 22 payments behind it that the book
//! never wrote. Nothing opens its debt tab, so the principal of each payment the book does keep would take the tab below
//! nothing. LANGUAGE §5 says a loan's balance "comes from its terms and needs no opening line", and the schedule can say what
//! it is, so the debt is opened the way an `opening` line opens it: a flow out of the tab against the opening entity, in
//! `Mode::Opening`, with the one difference that the book did not write it (its origin is [`Derivation::Opening`]).
//!
//! # Which loans, and on what day
//!
//! A loan whose debt an `opening` of the book names is the book's to say, whatever the day, and is left alone. A loan the
//! journal originates (`DATE NAME` on the day it was made) was made in the book: a line is a fact, so it is no earlier than the
//! first fact, and `made < first fact` is the whole test. The loan is opened on the day of the book's first fact: **not the day
//! before it**, which would be the first flow of the book, move the first fact and the monitor's start with it, and need the
//! arena of flows (day-sorted, read as such by the fold) to take a flow before the lowering knows what the first fact is. On
//! that day the opening is lowered right after the record that makes it, so the arena stays in order and the opening is
//! followed by what the book says of the day (a payment due on it is the book's, and posts after). A book with no fact at all
//! is begun by its loans, each on the day it was made, as an origination would.
//!
//! # How much
//!
//! What the schedule says is owed before that day ([`Promises::owed_before`]): the lender's terms, which is what a loan is. A
//! payment missed or a prepayment made before the book began is not in them, and a statement of the lender's number says so
//! later (`loan-balance`). Nothing is opened for a loan nothing is owed on.

use axiom_core::{Day, Days, Diagnostic, Id, Loc, Qty, Run, Set};
use axiom_syntax as ast;

use super::flow::{empty_codes, journal_txn};
use super::infer::classify;
use super::staged::Staged;
use crate::book::{Amount, Book, Contract, Loan, Place};
use crate::collect::{Collected, Written};
use crate::declare::World;
use crate::journal::{Derivation, Flow, Infer, Mode, Origin};
use crate::promise::Promises;
use crate::resolve::End;

/// Where the edit that says a loan's debt in the book's own words goes: the least surprising place for a line that is one
/// of the lines of an opening.
pub(super) enum Insertion {
    /// Among the lines of the opening that begins the book: before its first, indented as it is.
    Line { before: Loc, indent: String },
    /// As an opening of its own, before the record that begins the book.
    Opening { before: Loc },
}

impl Insertion {
    /// The edit that says `said` (`NAME AMOUNT`) in an opening dated `day`: where it goes, and what is written there.
    fn edit(&self, day: Day, said: &str) -> (Loc, String) {
        match self {
            Insertion::Line { before, indent } => (*before, format!("{indent}{said}\n")),
            Insertion::Opening { before } => (*before, format!("opening {day}\n  {said}\n\n")),
        }
    }
}

/// When the book begins, as far as a loan made before it is concerned.
#[derive(Clone, Copy)]
enum Begins<'a> {
    /// On `day`, with the record whose lines `at` says where an edit goes.
    With { day: Day, at: &'a Insertion },
    /// It has no fact, so each loan begins it on the day it was made.
    WithTheLoan,
}

impl Begins<'_> {
    /// The day a loan made on `made` is opened, if it is opened.
    fn day(self, made: Day) -> Option<Day> {
        match self {
            Begins::With { day, .. } => (made < day).then_some(day),
            Begins::WithTheLoan => Some(made),
        }
    }
}

/// The loans no opening of the book names, by the day they were made: those that may have to be opened.
pub(super) struct Unopened(Vec<Id<Contract>>);

impl Unopened {
    pub(super) fn of(world: &World<'_>, collected: &Collected<'_, '_>) -> Unopened {
        let book = &world.book;
        let mut loans: Vec<_> = book.contracts.iter().filter(|(_, contract)| contract.loan.is_some()).collect();
        if !loans.is_empty() {
            let (named, originated) = (named_by_openings(collected), originations(collected));
            loans.retain(|(_, contract)| {
                let (name, made) = (book.name(contract.name), contract.loan.map(|loan| loan.on));
                !named.contains(name) && !made.is_some_and(|made| originated.contains(&(name, made)))
            });
            loans.sort_by_key(|(_, contract)| contract.loan.map(|loan| loan.on));
        }
        Unopened(loans.into_iter().map(|(id, _)| id).collect())
    }

    /// Called after each record is lowered: when that one has begun the book, the loans made before it are opened. `at` says
    /// where an edit goes, when the record is the one that began it.
    pub(super) fn after(&mut self, world: &mut World<'_>, at: impl FnOnce() -> Insertion) {
        if self.0.is_empty() {
            return;
        }
        if let Some(day) = world.book.first_fact() {
            self.open(world, Begins::With { day, at: &at() });
        }
    }

    /// Called when the journal is lowered: a book that has no fact is begun by its loans.
    pub(super) fn finish(&mut self, world: &mut World<'_>) {
        self.open(world, Begins::WithTheLoan);
    }

    fn open(&mut self, world: &mut World<'_>, begins: Begins<'_>) {
        for id in std::mem::take(&mut self.0) {
            open_loan(world, id, begins);
        }
    }
}

/// The names the lines of the book's openings give a place: a contract's name is its debt tab when it is a loan's.
fn named_by_openings<'s>(collected: &Collected<'_, 's>) -> Set<&'s str> {
    let lines = |opening: &Written<'_, 's, ast::Opening<'s>>| {
        opening.file()[opening.node.lines].iter().map(|leg| leg.end.name.0).collect::<Vec<_>>()
    };
    collected.openings.iter().flat_map(lines).collect()
}

/// The lines of the journal that are `DATE NAME`: the name of a contract, and the day. One on the day a loan was made is its
/// origination, which the book has written (and when it is rejected, said it began then): the loan is not opened by its terms.
fn originations<'s>(collected: &Collected<'_, 's>) -> Set<(&'s str, Day)> {
    let line = |written: &Written<'_, 's, ast::Statement<'s>>| match (written.node.subject, &written.node.verb) {
        (ast::Subject::Name(name), ast::Verb::Occurrence(_)) => Some((name.0, written.node.date)),
        _ => None,
    };
    collected.statements.iter().filter_map(line).collect()
}

/// Opens the debt of one loan, if it was made before the book began and its terms say any of it is owed.
fn open_loan(world: &mut World<'_>, id: Id<Contract>, begins: Begins<'_>) {
    let book = &world.book;
    let contract = &book.contracts[id];
    let Some(loan) = contract.loan else { return };
    let Some(day) = begins.day(loan.on) else { return };
    let owed = Promises::owed_before(book, contract, day).filter(|&owed| owed > Qty::ZERO);
    let (Some(owed), Some(opening)) = (owed, book.entities[book.roots.opening].place) else { return };
    let owed = Amount::new(owed, loan.principal.unit);
    let note = note(book, contract, loan, owed, day, begins);
    if push_opening(world, id, day, owed, opening).is_some() {
        world.diags.push(note);
    }
}

/// The flow out of the debt tab, and the transaction that owns it, as an `opening` line makes them. None, with the problem
/// said, when what the flow is for cannot be told.
fn push_opening(world: &mut World<'_>, id: Id<Contract>, day: Day, owed: Amount, opening: Id<Place>) -> Option<()> {
    let contract = &world.book.contracts[id];
    let (debt, loc) = (contract.loan?.debt, contract.loc);
    let owner = world.book.places[debt].owner;
    let mut staged = Staged::open(world);
    let txn = Id::new(staged.book.txns.len() as u32);
    let end = |place| End { place, entity: None };
    let purpose = classify(&mut staged, end(debt), end(opening), None, loc).ok()?;
    let codes = empty_codes(&staged);
    staged.book.flows.push(Flow {
        day,
        recognized: Days::on(day),
        from: debt,
        to: opening,
        out: owed,
        arrive: owed,
        mode: Mode::Opening,
        infer: Infer::Known,
        txn,
        payee: None,
        owner,
        purpose,
        description: None,
        origin: Origin::Derived(Derivation::Opening(id)),
        select: Run::new(Id::new(0), 0),
        header_codes: codes,
        codes,
        loc,
        waive: None,
        detail: None,
    });
    let record = journal_txn(&staged, day, loc);
    staged.book.txns.push(record);
    staged.commit();
    Some(())
}

/// What `check` says of an opening nobody wrote: the loan, the day, the amount and why, and the line that says it
/// another way.
fn note(book: &Book<'_>, contract: &Contract, loan: Loan, owed: Amount, day: Day, begins: Begins<'_>) -> Diagnostic {
    let (name, money) = (book.name(contract.name), book.show(owed));
    let note = Diagnostic::info("loan-opening", format!("`{name}` opens owing {money}, what its terms say on {day}"));
    match begins {
        Begins::With { at, .. } => {
            let (loc, line) = at.edit(day, &format!("{name} {}", money.to_string().replace(',', "_")));
            note.label(contract.loc, format!("made on {}, before the book begins", loan.on))
                .note("the payments due since are not in the book, so the debt opens after them")
                .fix("if the lender says another number, write it in an opening: that replaces this", loc, line)
        }
        Begins::WithTheLoan => note
            .label(contract.loc, "the book has no fact yet, so it begins with this loan")
            .note("the payments due since are waited for, and reported as missed until a line keeps each")
            .help("write the lines that kept them, or begin the book later with an opening that says what is owed"),
    }
}
