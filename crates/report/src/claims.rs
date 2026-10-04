//! `claims`: what others owe, and what is owed to them, still open on a day.
//!
//! A claim is value someone owes. Owed to you, it is a parcel in a `claim`
//! place, and each parcel is one claim: it remembers the transaction that made
//! it, and the flow of it that paid in has the payee and the `due` day. Owed by
//! you, it is a parcel of a debt place (a tab of the bills you owe, or an account
//! that says `claim`), negative as a liability is, made by the flow that paid out
//! of it and settled the same way.

use axiom_core::{Day, Id, Qty};
use axiom_engine::Holding;
use axiom_model::{Amount, Book, Class, Entity, Place, RuntimeTxn};

use crate::places::path;
use crate::table::{code_labels, doc_headline};
use crate::view::View;
use crate::{Cell, Column, Report, Row, Section, Style};

/// One open claim.
pub struct Claim {
    /// Owed to you, or owed by you.
    pub mine: bool,
    pub place: Id<Place>,
    /// The transaction that made it: its codes, doc and source line.
    pub txn: RuntimeTxn,
    pub left: Amount,
    pub made: Day,
    /// Who owes it, or is owed: the payee of the flow that made it.
    pub payee: Option<Id<Entity>>,
    pub due: Option<Day>,
}

impl Claim {
    /// Who owes it, or is owed: the payee, else the place the claim sits in.
    pub fn counterparty<'s>(&self, book: &'s Book<'_>) -> &'s str {
        match self.payee {
            Some(payee) => book.name(book.entities[payee].path),
            None => path(book, self.place),
        }
    }

    pub fn with(&self, entity: Id<Entity>) -> bool {
        self.payee == Some(entity)
    }
}

/// Every claim open on the view's day, for its owners, given the holdings on that day: soonest due first, what is owed to
/// you before what you owe, and of two claims made and due on one day the one whose place is listed first (see
/// `Book::listing`). What is owed by you is a parcel of a debt place, negative as a liability is, and is read in the sign the
/// place is shown in.
pub fn open<'h>(view: View, holdings: impl IntoIterator<Item = &'h Holding>) -> Vec<Claim> {
    let book = view.book();
    let claimed = holdings.into_iter().filter(|holding| book.is_claim(holding.place) && view.owns(holding.place));
    let parcels = claimed.flat_map(|holding| {
        holding.lots.iter().filter_map(move |lot| {
            let left = view.plan().sides().display(holding.place, view.place_qty(holding.place, lot.qty));
            if left.is_zero() {
                return None;
            }
            let made = book.claim_of(lot.txn, holding.place);
            Some(Claim {
                mine: book.places[holding.place].class == Class::Asset,
                place: holding.place,
                txn: lot.txn,
                left: Amount::new(left, holding.unit),
                made: lot.acquired,
                payee: made.and_then(|claim| claim.payee),
                due: made.and_then(|claim| claim.due),
            })
        })
    });
    let mut claims: Vec<Claim> = parcels.collect();
    claims.sort_by_key(|claim| (!claim.mine, claim.due.unwrap_or(Day::MAX), claim.made, book.listing(claim.place)));
    claims
}

/// Builds a claims view from holdings supplied by a shared context ledger.
pub(crate) fn view_from<'h, 's>(view: View<'s, '_, '_>, holdings: impl IntoIterator<Item = &'h Holding>) -> Report<'s> {
    let at = view.day;
    let claims = open(view, holdings);
    let (mine, theirs): (Vec<&Claim>, Vec<&Claim>) = claims.iter().partition(|claim| claim.mine);
    let report = Report::new(format!("Claims on {at}")).with(section(view, "Owed to you", &mine));
    let report = report.with(section(view, "Owed by you", &theirs));
    match claims.is_empty() {
        true => report.with(Section::note_only(
            "Nothing is owed either way. A flow with `due` into a receivable place makes one.",
        )),
        false => report,
    }
}

/// What a claim is: its codes and doc, or where it was written when it has neither (a claim the monitor made says the
/// contract's line).
fn what_of<'s>(book: &'s Book<'_>, claim: &Claim) -> Cell<'s> {
    let txn = claim.txn.source_txn().and_then(|id| book.txns.get(id));
    let named: Vec<_> = txn
        .map(|txn| {
            code_labels(book, book.codes[txn.codes].iter().copied())
                .chain(doc_headline(book, txn.doc).map(Cell::text))
                .collect()
        })
        .unwrap_or_default();
    if !named.is_empty() {
        return Cell::list(" · ", named);
    }
    let wrote = txn.map(|txn| txn.loc).or_else(|| book.claim_of(claim.txn, claim.place).map(|made| made.loc));
    wrote.map_or(Cell::Blank, Cell::Source)
}

/// Claims with what each is, when it was made and how old it is, when it is due
/// and whether it is late, and what they come to.
pub fn section<'s>(view: View<'s, '_, '_>, heading: &'s str, claims: &[&Claim]) -> Section<'s> {
    let (book, at) = (view.book(), view.day);
    let columns = ["Counterparty", "What"].map(Column::left).into_iter();
    let columns = columns.chain([Column::right("Left")]).chain(["Made", "Age", "Due", "Status"].map(Column::left));
    let mut section = Section::new(columns).headed(Cell::text(heading));
    let (mut total, mut unpriced) = (Qty::ZERO, 0);
    for claim in claims {
        let days_left = claim.due.map(|due| due.0 - at.0);
        let status = days_left.map(|days| if days < 0 { format!("overdue {}d", -days) } else { format!("in {days}d") });
        match view.value(claim.left) {
            Some(qty) => total += qty,
            None => unpriced += 1,
        }
        let cells = [
            Cell::text(claim.counterparty(book)),
            what_of(book, claim),
            Cell::amount(book, claim.left),
            Cell::Day(claim.made),
            Cell::text(at.since(claim.made).to_string()),
            claim.due.map_or(Cell::Blank, Cell::Day),
            status.map_or(Cell::Blank, Cell::text),
        ];
        section.push(Row::new(cells).style(if days_left.is_some_and(|days| days < 0) {
            Style::Alert
        } else {
            Style::Normal
        }));
    }
    if !claims.is_empty() {
        section.push(Row::padded([Cell::text("Total"), Cell::Blank, Cell::base(book, total)], 7).style(Style::Total));
    }
    if unpriced > 0 {
        section.note(format!("{unpriced} claims have no price and are left out of the total."));
    }
    section
}
