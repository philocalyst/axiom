//! `why LAW`: where it applies, what it says, how often it ran, what it caused.

use axiom_core::Id;
use axiom_engine::Run;
use axiom_model::{Book, Law, Owner};

use super::trigger_words;
use crate::table::{cause_cell, creditor, doc_headline, doc_lines, headline, plural};
use crate::{Cell, Column, Report, Row, Section, Style};

/// How many of the latest violations and effects to list.
const RECENT: usize = 8;

pub fn report<'s>(book: &Book<'s>, run: &Run, id: Id<Law>) -> Report<'s> {
    let law = &book.laws[id];
    let violations: Vec<_> = run.violations.iter().filter(|violation| violation.law == id).collect();
    let effects: Vec<_> = run.effects.iter().filter(|effect| effect.law == id).collect();

    let mut about = Section::new([Column::left("Law"), Column::left(book.name(law.name).to_string())]);
    let ran = run.checks.get(id.index()).copied().unwrap_or(0) as usize;
    about.push(Row::new([Cell::text("When"), Cell::text(trigger_words(law.trigger))]));
    about.push(Row::new([Cell::text("Governs"), Cell::text(governs(book, law.owner))]));
    about.push(Row::new([Cell::text("Written"), Cell::Source(law.loc)]));
    about.push(Row::new([Cell::text("Ran"), Cell::text(plural(ran, "time"))]));
    let recorded = format!("{}, {}", plural(violations.len(), "violation"), plural(effects.len(), "effect"));
    about.push(Row::new([Cell::text("Recorded"), Cell::text(recorded)]));
    for line in law.doc.iter().flat_map(|&doc| doc_lines(book.name(doc))) {
        about.note(line);
    }

    let mut broken = Section::new([Column::left("Date"), Column::left("Violation"), Column::left("From")])
        .headed("Recent violations");
    for violation in violations.iter().rev().take(RECENT).rev() {
        let message = &run.diagnostics[violation.diagnostic as usize].message;
        let style = if violation.waived { Style::Muted } else { Style::Alert };
        let cells =
            [Cell::Day(violation.day), Cell::text(headline(message).to_string()), cause_cell(book, violation.cause)];
        broken.push(Row::new(cells).style(style));
    }

    let columns = [
        Column::left("Date"),
        Column::left("Effect"),
        Column::right("Amount"),
        Column::left("Owed to"),
        Column::left("From"),
    ];
    let mut caused = Section::new(columns).headed("Recent effects");
    for effect in effects.iter().rev().take(RECENT).rev() {
        let owed = effect.owe.map_or(Cell::Blank, |owed| Cell::text(creditor(book, owed)));
        let cells = [
            Cell::Day(effect.day),
            Cell::text(book.name(effect.name)),
            Cell::amount(book, effect.amount),
            owed,
            cause_cell(book, effect.cause),
        ];
        caused.push(Row::new(cells));
    }

    Report::new(format!("Why {}", book.name(law.name))).with(about).with(broken).with(caused)
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
        let system = law.system.map_or("project", |system| book.name(book.systems[system].path));
        let explains = doc_headline(book, law.doc).map_or(Cell::Blank, Cell::text);
        let cells = [
            Cell::text(book.name(law.name)),
            Cell::text(system),
            Cell::Source(law.loc),
            Cell::text(governs(book, law.owner)),
            explains,
        ];
        section.push(Row::new(cells));
    }
    section.note("Ask about the one you mean with `axiom why FILE:LINE`, using the location in Written.");
    Report::new(format!("`{name}` is written in {}", plural(candidates.len(), "place"))).with(section)
}

/// Who a law governs, as the sentence that explains it.
fn governs<'s>(book: &Book<'s>, owner: Owner) -> String {
    match owner {
        Owner::Kind(kind) => format!("every {}", book.name(book.kinds[kind].name)),
        Owner::Place(place) => format!("{} and everything beneath it", book.name(book.places[place].path)),
        Owner::Entity(entity) => book.name(book.entities[entity].path).to_string(),
        Owner::System(system) => {
            format!("everyone living under {}, and all they own", book.name(book.systems[system].path))
        }
        Owner::Book => "everything in this book".to_string(),
    }
}
