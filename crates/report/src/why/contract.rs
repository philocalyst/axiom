//! `why CONTRACT`: each change in terms and the occurrences it promised.

use axiom_core::{Id, Qty};
use axiom_engine::Run;
use axiom_model::promise::{Entry, Kind};
use axiom_model::{Amount, Contract};

use super::flows_table;
use crate::history::all_postings;
use crate::lens::Lens;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, contract_id: Id<Contract>) -> Report<'s> {
    let book = lens.book();
    let contract = &book.contracts[contract_id];
    let name = book.name(contract.name);
    if !lens.owns_entity(contract.owner) {
        return Report::new(format!("Why {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[contract.owner].path)
        )));
    }
    // The flows the contract derived, or that its occurrences wrote.
    let derived = all_postings(book, run).filter(|posting| {
        lens.owns(crate::flow::movement_place(lens, posting.flow))
            && crate::register::contract_flow(posting.flow.origin, contract_id)
    });
    let report = Report::new(format!("Why {name}"))
        .with(about_section(lens, contract))
        .with(terms_section(lens, contract))
        .with(promises_section(lens, run, contract_id))
        .with(flows_table(lens, derived, "Derived flows"));
    schedule_section(lens, run, contract_id).into_iter().fold(report, Report::with)
}

fn about_section<'s>(lens: Lens<'s, '_, '_, '_>, contract: &'s Contract) -> Section<'s> {
    let book = lens.book();
    let purpose = contract
        .purpose
        .map_or(Cell::Blank, |purpose| Cell::Purpose(book.name(book.purposes[purpose.value.purpose].name)));
    let mut about =
        Section::new([Column::left("Party"), Column::left("Purpose"), Column::left("Description")]).headed("Contract");
    about.push(Row::new([
        Cell::Name(book.name(book.entities[contract.party].path)),
        purpose,
        contract.description.map_or(Cell::Blank, |text| Cell::text(book.text(text))),
    ]));
    about
}

/// Each change in terms, and the days they were in force.
fn terms_section<'s>(lens: Lens<'s, '_, '_, '_>, contract: &'s Contract) -> Section<'s> {
    let mut terms = Section::new([
        Column::left("From"),
        Column::left("Through"),
        Column::left("State"),
        Column::left("Terms"),
        Column::left("Statement"),
    ])
    .headed("Terms over time");
    let stretches =
        contract.terms.iter().flat_map(|terms| contract.stretches().map(move |(days, waiver)| (days, terms, waiver)));
    for (days, value, waiver) in stretches {
        let active_days = days.intersect(contract.days).unwrap_or(days);
        let templates =
            value.template.iter().map(|flow| crate::contracts::template_flow_cell(lens, flow)).collect::<Vec<_>>();
        let state = match waiver {
            None => Cell::Word("active"),
            Some(_) => Cell::Word("waived"),
        };
        let change = waiver.map_or(Cell::Blank, |change| Cell::Source(change.loc));
        terms.push(Row::new([
            Cell::Day(active_days.first()),
            Cell::Day(active_days.last()),
            state,
            Cell::list_or_blank(" · ", templates),
            change,
        ]));
    }
    if let Some(loc) = contract.ended {
        terms.note(Cell::list(" ", [Cell::Word("Ended"), Cell::Source(loc)]));
    }
    terms
}

/// The occurrences the contract promised, kept, late or missing.
fn promises_section<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, contract_id: Id<Contract>) -> Section<'s> {
    let book = lens.book();
    let mut promises = Section::new([
        Column::left("Due"),
        Column::left("Kept"),
        Column::left("Occurrence"),
        Column::left("State"),
        Column::right("Late by"),
    ])
    .headed("Occurrences");
    let mut kept = 0usize;
    let mut late = 0usize;
    for promise in run.promises.iter().filter(|promise| promise.contract == contract_id) {
        let late_by = promise.late(run.today);
        kept += usize::from(promise.kept.is_some());
        late += usize::from(late_by > 0);
        let txn = promise.kept.map(|(_, txn)| &book.txns[txn]);
        let description = txn
            .and_then(|txn| txn.doc)
            .map_or(Cell::Blank, |doc| Cell::text(crate::table::doc_headline(book, Some(doc)).unwrap_or_default()));
        let state = match (late_by > 0, promise.kept.is_some()) {
            (true, _) => "late",
            (false, true) => "kept",
            (false, false) => "missing",
        };
        promises.push(
            Row::new([
                Cell::Day(promise.due),
                promise.kept.map_or(Cell::Blank, |(day, _)| Cell::Day(day)),
                description,
                Cell::Word(state),
                if late_by > 0 { Cell::text(format!("{late_by} days")) } else { Cell::Blank },
            ])
            .style(if late_by > 0 { Style::Alert } else { Style::Normal }),
        );
    }
    if promises.rows.is_empty() {
        promises.note("No occurrences were expected by the run's horizon.");
    }
    promises.note(format!("{kept} kept; {late} late."));
    promises
}

/// How many payments of a loan's schedule `why` shows either side of today.
const AROUND_TODAY: usize = 6;

/// A loan's schedule around today: what each payment pays of interest and of principal, and what is owed after it, with what
/// became of it. None for a contract that is no loan.
fn schedule_section<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, contract_id: Id<Contract>) -> Option<Section<'s>> {
    let book = lens.book();
    let loan = book.promises.loan(contract_id)?;
    let unit = loan.terms().principal().unit;
    let money = |qty| Cell::amount(book, Amount::new(qty, unit));
    let mut schedule = Section::new([
        Column::left("Due"),
        Column::right("Interest"),
        Column::right("Principal"),
        Column::right("Owed after"),
        Column::left("State"),
    ])
    .headed("Loan schedule");
    let entries = loan.entries();
    let next = entries.partition_point(|entry| entry.day <= run.today);
    for entry in &entries[next.saturating_sub(AROUND_TODAY)..entries.len().min(next + AROUND_TODAY)] {
        let interest = if entry.kind == Kind::Pay { money(entry.paid.interest) } else { Cell::Blank };
        schedule.push(Row::new([
            Cell::Day(entry.day),
            interest,
            money(entry.paid.principal),
            money(entry.paid.open),
            Cell::Word(entry_state(run, contract_id, entry)),
        ]));
    }
    let (ahead, interest) = (
        loan.payments().filter(|payment| payment.day > run.today).count(),
        entries.iter().map(|entry| entry.paid.interest).sum::<Qty>(),
    );
    let payments =
        format!("{} payments, {ahead} of them ahead; interest over the life of the loan", loan.payments().count());
    schedule.note(Cell::list(" ", [Cell::text(payments), money(interest)]));
    Some(schedule)
}

/// What became of an entry of a loan's schedule, in a word.
fn entry_state(run: &Run, contract_id: Id<Contract>, entry: &Entry) -> &'static str {
    let kept = |promise: &&axiom_engine::Promise| promise.contract == contract_id && promise.due == entry.day;
    match (entry.kind, run.promises.iter().find(kept)) {
        (Kind::Prepay, _) => "prepaid",
        (Kind::Pay, Some(promise)) if promise.kept.is_some() => "kept",
        (Kind::Pay, Some(_)) => "missed",
        (Kind::Pay, None) if entry.day > run.today => "ahead",
        (Kind::Pay, None) => "not written",
    }
}
