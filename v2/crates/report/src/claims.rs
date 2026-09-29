//! `claims`: what others owe, and what is owed to them, still open on a day.
//!
//! A claim is value someone owes. Owed to you, it is a parcel in a `claim`
//! place, and each parcel is one claim: it remembers the transaction that made
//! it. Owed by you, it is a debt in a `payable` place, which holds a plain
//! balance, so it is told apart by the code on the flows that made and settled it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use axiom_core::{Day, Diagnostic, Id, Qty, Sym};
use axiom_engine::{Holding, Ledger, Options, Run};
use axiom_model::{Amount, Book, Class, Entity, Flow, Place, Select, Txn};

use crate::history::{Posting, journal_ends_by};
use crate::lens::{Lens, Whose};
use crate::places::path;
use crate::table::{code_labels, doc_headline};
use crate::{Cell, Column, Report, Row, Section, Style};

/// One open claim.
pub struct Claim {
    /// Owed to you, or owed by you.
    pub mine: bool,
    pub place: Id<Place>,
    /// The transaction that made it: its codes, doc, source line and payee.
    pub txn: Id<Txn>,
    pub left: Amount,
    pub made: Day,
    pub due: Option<Day>,
}

impl Claim {
    /// Who owes it, or is owed: the payee, else the place the claim sits in.
    pub fn counterparty<'s>(&self, book: &Book<'s>) -> &'s str {
        match book.txns[self.txn].payee {
            Some(payee) => book.name(book.entities[payee].path),
            None => path(book, self.place),
        }
    }

    pub fn with(&self, book: &Book, entity: Id<Entity>) -> bool {
        book.txns[self.txn].payee == Some(entity)
    }
}

/// The holdings on `day`: the run's, unless the journal goes on after it, in
/// which case the ledger is folded up to it.
pub fn holdings_at<'r>(book: &Book, run: &'r Run, day: Day) -> Cow<'r, [Holding]> {
    if journal_ends_by(book, day) {
        return Cow::Borrowed(&run.holdings);
    }
    let mut ledger = Ledger::new(book, Options { today: day, relaxed: book.relaxed });
    ledger.advance(day);
    Cow::Owned(ledger.holdings().cloned().collect())
}

/// Every claim open on the lens's day, for its owners, given the holdings on
/// that day.
pub fn open<'h>(lens: Lens, run: &Run, holdings: impl IntoIterator<Item = &'h Holding>) -> Vec<Claim> {
    let book = lens.book;
    let mut claims = Vec::new();
    for holding in holdings.into_iter().filter(|holding| book.places[holding.place].claim && lens.owns(holding.place)) {
        for lot in &holding.lots {
            let due = book.txns[lot.txn].due;
            claims.push(Claim {
                mine: true,
                place: holding.place,
                txn: lot.txn,
                left: Amount::new(lot.qty, holding.unit),
                made: lot.acquired,
                due,
            });
        }
    }
    let payable = book.kind("payable").ok();
    let payables = book.places.iter().filter(|&(id, place)| {
        place.class == Class::Liability && lens.owns(id) && payable.is_some_and(|kind| book.is_a(place.kind, kind))
    });
    for (place, _) in payables {
        claims.extend(owed_by_you(lens, run, place));
    }
    claims.sort_by_key(|claim| (!claim.mine, claim.due.unwrap_or(Day(i32::MAX)), claim.made));
    claims
}

/// What is owed through a payable place, netted per code: a flow out of it
/// (a bill) names its debt by its first code, and a flow into it (a payment)
/// settles the debts of the codes it names.
pub(crate) fn owed_by_you(lens: Lens, run: &Run, place: Id<Place>) -> Vec<Claim> {
    let book = lens.book;
    let mut debts: BTreeMap<Sym, Claim> = BTreeMap::new();
    for &id in &book.touching[place] {
        let posting = Posting::at(book, run, id);
        if !posting.is_real_on(lens.day) {
            continue;
        }
        let flow = posting.flow;
        if flow.from == place {
            let Some(&code) = flow.codes.first() else { continue };
            let due = book.txns[flow.txn].due;
            let debt = debts.entry(code).or_insert(Claim {
                mine: false,
                place,
                txn: flow.txn,
                left: Amount::zero(flow.out.unit),
                made: flow.day,
                due,
            });
            debt.left.qty += posting.out().qty;
        } else if let Some(debt) =
            settled_codes(flow).find(|code| debts.contains_key(code)).and_then(|code| debts.get_mut(&code))
        {
            debt.left.qty -= posting.arrive().qty;
        }
    }
    debts.into_values().filter(|debt| debt.left.qty > Qty::ZERO).collect()
}

/// The codes a flow carries, and those it settles with `for #code`.
fn settled_codes(flow: &Flow) -> impl Iterator<Item = Sym> + '_ {
    let selected =
        flow.select.iter().filter_map(|select| if let Select::Code(code) = select { Some(*code) } else { None });
    flow.codes.iter().copied().chain(selected)
}

pub fn view<'s>(book: &Book<'s>, run: &Run, whose: &Whose, at: Option<Day>) -> Result<Report<'s>, Diagnostic> {
    let at = at.unwrap_or(run.today);
    let lens = Lens::new(book, whose, at);
    let holdings = holdings_at(book, run, at);
    let claims = open(lens, run, holdings.iter());
    let (mine, theirs): (Vec<&Claim>, Vec<&Claim>) = claims.iter().partition(|claim| claim.mine);
    let owed_to_you = section(lens, "Owed to you", &mine);
    let owed_by_you = section(lens, "Owed by you", &theirs);
    let mut report = Report::new(format!("Claims on {at}")).with(owed_to_you).with(owed_by_you);
    if claims.is_empty() {
        report = report.with(Section::note_only(
            "Nothing is owed either way. A flow with `due` into a receivable place makes one.",
        ));
    }
    Ok(report)
}

pub fn section<'s>(lens: Lens<'_, 's>, heading: &str, claims: &[&Claim]) -> Section<'s> {
    let (book, at) = (lens.book, lens.day);
    let columns = [
        Column::left("Counterparty"),
        Column::left("What"),
        Column::right("Left"),
        Column::left("Made"),
        Column::left("Age"),
        Column::left("Due"),
        Column::left("Status"),
        Column::left("From"),
    ];
    let mut section = Section::new(columns).headed(heading);
    let (mut total, mut unpriced) = (Qty::ZERO, 0);
    for claim in claims {
        let txn = &book.txns[claim.txn];
        let what: Vec<String> =
            code_labels(book, &txn.codes).chain(doc_headline(book, txn.doc).map(str::to_string)).collect();
        let status = claim.due.map(|due| match due.0 - at.0 {
            late if late < 0 => format!("overdue {}d", -late),
            soon => format!("in {soon}d"),
        });
        let overdue = claim.due.is_some_and(|due| due < at);
        match lens.value(claim.left) {
            Some(qty) => total += qty,
            None => unpriced += 1,
        }
        let cells = [
            Cell::text(claim.counterparty(book)),
            Cell::text(what.join(" · ")),
            Cell::amount(book, claim.left),
            Cell::Day(claim.made),
            Cell::text(at.since(claim.made).to_string()),
            claim.due.map_or(Cell::Blank, Cell::Day),
            status.map_or(Cell::Blank, Cell::text),
            Cell::Source(txn.loc),
        ];
        section.push(Row::new(cells).style(if overdue { Style::Alert } else { Style::Normal }));
    }
    if !claims.is_empty() {
        section.push(Row::padded([Cell::text("Total"), Cell::Blank, Cell::base(book, total)], 8).style(Style::Total));
    }
    if unpriced > 0 {
        section.note(format!("{unpriced} claims have no price and are left out of the total."));
    }
    section
}
