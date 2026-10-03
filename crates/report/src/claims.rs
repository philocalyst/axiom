//! `claims`: what others owe, and what is owed to them, still open on a day.
//!
//! A claim is value someone owes. Owed to you, it is a parcel in a `claim`
//! place, and each parcel is one claim: it remembers the transaction that made
//! it, and the flow of it that paid in has the payee and the `due` day. Owed by
//! you, it is a debt in a `payable` place, which holds a plain balance, so it is
//! told apart by the code on the flows that made and settled it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use axiom_core::{Day, Id, Qty, Sym};
use axiom_engine::{Holding, Options, Plan, Run};
use axiom_model::{Amount, Book, Class, Entity, Flow, Place, RuntimeTxn, Select};

use crate::history::{Posting, journal_ends_by};
use crate::lens::Lens;
use crate::places::path;
use crate::table::{code_labels, doc_headline};
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

/// The holdings on `day`: the run's, unless the journal goes on after it, in
/// which case the ledger is folded up to it.
pub fn holdings_at<'r>(book: &Book, run: &'r Run, day: Day) -> Cow<'r, [Holding]> {
    if journal_ends_by(book, day) {
        return Cow::Borrowed(&run.holdings);
    }
    let plan = Plan::new(book);
    let mut ledger = plan.start(Options { today: day, relaxed: book.relaxed });
    ledger.advance(day);
    Cow::Owned(ledger.holdings().cloned().collect())
}

/// Every claim open on the lens's day, for its owners, given the holdings on
/// that day: soonest due first, what is owed to you before what you owe, and of two claims made and due on one day the
/// one whose place is listed first (see `Book::listing`).
pub fn open<'h>(lens: Lens, run: &Run, holdings: impl IntoIterator<Item = &'h Holding>) -> Vec<Claim> {
    let book = lens.book();
    let claimed = holdings.into_iter().filter(|holding| book.is_claim(holding.place) && lens.owns(holding.place));
    let parcels = claimed.flat_map(|holding| {
        holding.lots.iter().filter_map(move |lot| {
            let left = lens.place_qty(holding.place, lot.qty);
            if left.is_zero() {
                return None;
            }
            let made = lot.txn.source_txn().and_then(|txn| book.paid_into(txn, holding.place));
            Some(Claim {
                mine: true,
                place: holding.place,
                txn: lot.txn,
                left: Amount::new(left, holding.unit),
                made: lot.acquired,
                payee: made.and_then(|flow| flow.payee),
                due: made.and_then(|flow| book.flow_view(flow).detail().due),
            })
        })
    });
    let payable = book.kind("payable").ok();
    let payables = book.places.iter().filter(|&(id, place)| {
        place.class == Class::Debt && lens.owns(id) && payable.is_some_and(|kind| book.is_a(place.kind, kind))
    });
    let mut claims: Vec<Claim> = parcels.chain(payables.flat_map(|(place, _)| owed_by_you(lens, run, place))).collect();
    claims.sort_by_key(|claim| (!claim.mine, claim.due.unwrap_or(Day::MAX), claim.made, book.listing(claim.place)));
    claims
}

/// What is owed through a payable place, netted per code: a flow out of it
/// (a bill) names its debt by its first code, and a flow into it (a payment)
/// settles the debts of the codes it names.
pub(crate) fn owed_by_you(lens: Lens, run: &Run, place: Id<Place>) -> Vec<Claim> {
    let book = lens.book();
    let mut debts: BTreeMap<Sym, Claim> = BTreeMap::new();
    for &id in &book.touching[place] {
        let posting = Posting::at(book, run, id);
        if !posting.is_real_on(lens.day) {
            continue;
        }
        let flow = posting.flow;
        if flow.from == place {
            let Some(code) = book.flow_view(flow).codes().next() else {
                continue;
            };
            let debt = debts.entry(code).or_insert(Claim {
                mine: false,
                place,
                txn: RuntimeTxn::journal(flow.txn).expect("journal flow has a real transaction"),
                left: Amount::zero(flow.out.unit),
                made: flow.day,
                payee: flow.payee,
                due: book.flow_view(flow).detail().due,
            });
            debt.left.qty += posting.out().qty;
        } else if let Some(debt) =
            settled_codes(book, flow).find(|code| debts.contains_key(code)).and_then(|code| debts.get_mut(&code))
        {
            debt.left.qty -= posting.arrive().qty;
        }
    }
    debts
        .into_values()
        .map(|mut debt| {
            debt.left.qty = lens.place_qty(place, debt.left.qty);
            debt
        })
        .filter(|debt| debt.left.qty > Qty::ZERO)
        .collect()
}

/// The codes a flow carries, and those it settles with `for #code`.
fn settled_codes<'a>(book: &'a Book, flow: &'a Flow) -> impl Iterator<Item = Sym> + 'a {
    let view = book.flow_view(flow);
    let selected =
        view.select().iter().filter_map(|select| if let Select::Code(code) = select { Some(*code) } else { None });
    view.codes().chain(selected)
}

/// Builds a claims view from holdings supplied by a shared context ledger.
pub(crate) fn view_from<'h, 's>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    holdings: impl IntoIterator<Item = &'h Holding>,
) -> Report<'s> {
    let at = lens.day;
    let claims = open(lens, run, holdings);
    let (mine, theirs): (Vec<&Claim>, Vec<&Claim>) = claims.iter().partition(|claim| claim.mine);
    let report = Report::new(format!("Claims on {at}")).with(section(lens, "Owed to you", &mine));
    let report = report.with(section(lens, "Owed by you", &theirs));
    match claims.is_empty() {
        true => report.with(Section::note_only(
            "Nothing is owed either way. A flow with `due` into a receivable place makes one.",
        )),
        false => report,
    }
}

/// Claims with what each is, when it was made and how old it is, when it is due
/// and whether it is late, and what they come to.
pub fn section<'s>(lens: Lens<'s, '_, '_, '_>, heading: &'s str, claims: &[&Claim]) -> Section<'s> {
    let (book, at) = (lens.book(), lens.day);
    let columns = ["Counterparty", "What"].map(Column::left).into_iter();
    let columns = columns.chain([Column::right("Left")]).chain(["Made", "Age", "Due", "Status"].map(Column::left));
    let mut section = Section::new(columns).headed(Cell::text(heading));
    let (mut total, mut unpriced) = (Qty::ZERO, 0);
    for claim in claims {
        let txn = claim.txn.source_txn().and_then(|id| book.txns.get(id));
        let what = txn
            .map(|txn| {
                code_labels(book, book.codes[txn.codes].iter().copied())
                    .chain(doc_headline(book, txn.doc).map(Cell::text))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // What it is: its codes and doc, or where it was written when it has neither.
        let what = if what.is_empty() {
            txn.map_or(Cell::Blank, |txn| Cell::Source(txn.loc))
        } else {
            Cell::list(" · ", what)
        };
        let days_left = claim.due.map(|due| due.0 - at.0);
        let status = days_left.map(|days| if days < 0 { format!("overdue {}d", -days) } else { format!("in {days}d") });
        match lens.value(claim.left) {
            Some(qty) => total += qty,
            None => unpriced += 1,
        }
        let cells = [
            Cell::text(claim.counterparty(book)),
            what,
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
