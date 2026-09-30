//! `why LAW`: where it applies, what it says, how often it ran, what it caused.

use axiom_core::Id;
use axiom_model::{Book, Law, Owner};

use super::{effects_table, recent};
use crate::lens::Lens;
use crate::table::{cause_cell, doc_headline, doc_lines};
use crate::{Cell, Column, Report, Row, Section, Style};

pub fn report<'s>(lens: Lens<'_, 's>, id: Id<Law>) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let law = &book.laws[id];
    let violations: Vec<_> = run.violations.iter().filter(|violation| violation.law == id).collect();
    let effects: Vec<_> = run.effects.iter().filter(|effect| effect.law == id).collect();

    let mut about = Section::new([Column::left("Law"), Column::left(Cell::Name(book.name(law.name)))]);
    let ran = run.checks.get(id.index()).copied().unwrap_or(0) as usize;
    let recorded = Cell::list(", ", [Cell::Count(violations.len(), "violation"), Cell::Count(effects.len(), "effect")]);
    for (what, cell) in [
        ("When", Cell::Trigger(law.trigger)),
        ("Governs", governs(book, law.owner)),
        ("Written", Cell::Source(law.loc)),
        ("Ran", Cell::Count(ran, "time")),
        ("Recorded", recorded),
    ] {
        about.push(Row::new([what.into(), cell]));
    }
    for line in law.doc.iter().flat_map(|&doc| doc_lines(book.name(doc))) {
        about.note(Cell::Text(line));
    }

    let mut broken = Section::new([Column::left("Date"), Column::left("Violation"), Column::left("From")])
        .headed("Recent violations");
    for violation in recent(&violations).0 {
        let message = &run.diagnostics[violation.diagnostic as usize].message;
        let style = if violation.waived { Style::Muted } else { Style::Alert };
        let cells = [Cell::Day(violation.day), Cell::headline(message), cause_cell(book, violation.cause)];
        broken.push(Row::new(cells).style(style));
    }
    let caused = effects_table(book, &effects, "Recent effects");
    Report::new(["Why".into(), Cell::Name(book.name(law.name))]).with(about).with(broken).with(caused)
}

/// Several laws answer to one name, in different systems or files: each one
/// with where it is written, so the reader can ask about the one meant.
pub fn which<'s>(book: &Book<'s>, candidates: &[Id<Law>]) -> Report<'s> {
    let name = book.name(book.laws[candidates[0]].name);
    let columns = [
        Column::left("Law"),
        Column::left("System"),
        Column::left("Written"),
        Column::left("Governs"),
        Column::left("Explains"),
    ];
    let mut section = Section::new(columns);
    for &id in candidates {
        let law = &book.laws[id];
        let system = law.system.map_or("project".into(), |system| Cell::Name(book.name(book.systems[system].path)));
        let explains = doc_headline(book, law.doc).unwrap_or(Cell::Blank);
        let cells =
            [Cell::Name(book.name(law.name)), system, Cell::Source(law.loc), governs(book, law.owner), explains];
        section.push(Row::new(cells));
    }
    section.note("Ask about the one you mean with `axiom why FILE:LINE`, using the location in Written.");
    let title = [
        Cell::Join("", vec!["`".into(), Cell::Name(name), "`".into()]),
        "is written in".into(),
        Cell::Count(candidates.len(), "place"),
    ];
    Report::new(title).with(section)
}

/// Who a law governs, as the sentence that explains it.
fn governs<'s>(book: &Book<'s>, owner: Owner) -> Cell<'s> {
    match owner {
        Owner::Kind(kind) => ["every".into(), Cell::Name(book.name(book.kinds[kind].name))].into(),
        Owner::Place(place) => {
            [Cell::Name(book.name(book.places[place].path)), "and everything beneath it".into()].into()
        }
        Owner::Entity(entity) => Cell::Name(book.name(book.entities[entity].path)),
        Owner::Purpose(purpose) => {
            ["every flow of".into(), Cell::Name(book.name(book.purposes[purpose].name)), ", and beneath it".into()]
                .into()
        }
        Owner::Asset(asset) => Cell::Name(book.name(book.assets[asset].name)),
        Owner::Contract(contract) => {
            ["every flow of".into(), Cell::Name(book.name(book.contracts[contract].name))].into()
        }
        Owner::System(system) => [
            "everyone living under".into(),
            Cell::Name(book.name(book.systems[system].path)),
            ", and all they own".into(),
        ]
        .into(),
        Owner::Book => "everything in this book".into(),
    }
}
