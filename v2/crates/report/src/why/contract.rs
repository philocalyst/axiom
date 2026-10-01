//! `why CONTRACT`: each change in terms and the occurrences it promised.

use axiom_engine::Run;
use axiom_model::{Book, Contract, Derivation, Origin, TermsState};

use crate::lens::Whose;
use crate::places::route;
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn report<'s>(
    book: &'s Book<'_>,
    run: &Run,
    whose: &Whose,
    contract_id: axiom_core::Id<Contract>,
) -> Report<'s> {
    let contract = &book.contracts[contract_id];
    let name = book.name(contract.name);
    if !whose.includes(contract.owner) {
        return Report::new(format!("Why {name}")).with(Section::note_only(format!(
            "{name} belongs to {}, whose money this is not.",
            book.name(book.entities[contract.owner].path)
        )));
    }
    let mut terms = Section::new([
        Column::left("From"),
        Column::left("Through"),
        Column::left("State"),
        Column::left("Terms"),
        Column::left("Statement"),
    ])
    .headed("Terms over time");
    for (days, value) in contract.terms.within(contract.days) {
        let active_days = days.intersect(contract.days).unwrap_or(days);
        let templates = value
            .template
            .iter()
            .map(|flow| crate::contracts::template_flow_cell(book, flow))
            .collect::<Vec<_>>();
        let state = match value.state {
            TermsState::Active => Cell::Word("active"),
            TermsState::Waived => Cell::Word("waived"),
        };
        let change = value
            .change
            .map_or(Cell::Blank, |change| Cell::Source(change.loc));
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
    for promise in run
        .promises
        .iter()
        .filter(|promise| promise.contract == contract_id)
    {
        let late_by = promise.late(run.today);
        kept += usize::from(promise.kept.is_some());
        late += usize::from(late_by > 0);
        let txn = promise.kept.map(|(_, txn)| &book.txns[txn]);
        let description = txn.and_then(|txn| txn.doc).map_or(Cell::Blank, |doc| {
            Cell::text(crate::table::doc_headline(book, Some(doc)).unwrap_or_default())
        });
        promises.push(
            Row::new([
                Cell::Day(promise.due),
                promise.kept.map_or(Cell::Blank, |(day, _)| Cell::Day(day)),
                description,
                Cell::Word(if late_by > 0 {
                    "late"
                } else if promise.kept.is_some() {
                    "kept"
                } else {
                    "missing"
                }),
                if late_by > 0 {
                    Cell::text(format!("{late_by} days"))
                } else {
                    Cell::Blank
                },
            ])
            .style(if late_by > 0 {
                Style::Alert
            } else {
                Style::Normal
            }),
        );
    }
    if promises.rows.is_empty() {
        promises.note("No occurrences were expected by the run's horizon.");
    }
    promises.note(format!("{kept} kept; {late} late."));

    let mut derived = Section::new([
        Column::left("Date"),
        Column::left("What it derived"),
        Column::left("Flow"),
        Column::left("From"),
    ])
    .headed("Derived flows");
    for flow in book.flows.values().filter(|flow| match flow.origin {
        Origin::Occurrence(id) => id == contract_id,
        Origin::Derived(
            Derivation::Interest(id)
            | Derivation::Principal(id)
            | Derivation::Claim(id)
            | Derivation::Otherwise(id)
            | Derivation::Refund(id),
        ) => id == contract_id,
        _ => false,
    }) {
        let origin = match flow.origin {
            Origin::Occurrence(_) => "occurrence",
            Origin::Derived(Derivation::Interest(_)) => "interest",
            Origin::Derived(Derivation::Principal(_)) => "principal",
            Origin::Derived(Derivation::Claim(_)) => "claim",
            Origin::Derived(Derivation::Otherwise(_)) => "late fee",
            Origin::Derived(Derivation::Refund(_)) => "refund",
            _ => "derived",
        };
        derived.push(Row::new([
            Cell::Day(flow.day),
            Cell::Word(origin),
            Cell::text(route(book, flow)),
            Cell::Source(flow.loc),
        ]));
    }
    if derived.rows.is_empty() {
        derived.note("No flow from this contract appears in the book.");
    }

    let purpose = contract.purpose.map_or(Cell::Blank, |purpose| {
        Cell::Purpose(book.name(book.purposes[purpose.value.purpose].name))
    });
    let mut about = Section::new([
        Column::left("Party"),
        Column::left("Purpose"),
        Column::left("Description"),
    ])
    .headed("Contract");
    about.push(Row::new([
        Cell::Name(book.name(book.entities[contract.party].path)),
        purpose,
        contract
            .description
            .map_or(Cell::Blank, |text| Cell::text(book.text(text))),
    ]));
    Report::new(format!("Why {name}"))
        .with(about)
        .with(terms)
        .with(promises)
        .with(derived)
}
