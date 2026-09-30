//! `why FILE:LINE`: what is written on one source line, and everything it caused.

use axiom_core::{Id, Loc};
use axiom_engine::{Cause, Run};
use axiom_model::{Amount, Book, Flow};

use super::event_words;
use crate::history::Posting;
use crate::lens::Lens;
use crate::places::{path, route};
use crate::table::{creditor, gap_words};
use crate::{Cell, Column, Report, Row, Section, Style};

/// Explains the items whose source overlaps `at`.
pub fn line<'s>(lens: Lens<'_, 's>, at: Loc) -> Report<'s> {
    let (book, run) = (lens.book, lens.run);
    let flows = flows_on(book, at);
    let mut written = Section::new([Column::left("On this line"), Column::left("Source")]);
    for (text, loc) in items(book, run, at, &flows) {
        written.push(Row::new([text, Cell::Source(loc)]));
    }
    if written.rows.is_empty() {
        written.note("Nothing that the book records is written on that line.");
    }

    let mut caused = Section::new([Column::left("What it caused"), Column::left("Law")]);
    for id in flows {
        consequences(book, run, id, &mut caused);
    }
    Report::new("Why this line").with(written).with(caused.headed("Consequences"))
}

fn overlaps(a: Loc, b: Loc) -> bool {
    a.file == b.file && a.start < b.end && b.start < a.end
}

/// The flows written on the line. The header of a split transaction holds no
/// flow of its own; its legs do, so it stands for all of them.
fn flows_on(book: &Book, at: Loc) -> Vec<Id<Flow>> {
    let direct: Vec<Id<Flow>> =
        book.flows.iter().filter(|(_, flow)| overlaps(flow.loc, at)).map(|(id, _)| id).collect();
    if !direct.is_empty() {
        return direct;
    }
    let headers = book.txns.values().filter(|txn| overlaps(txn.loc, at));
    headers.flat_map(|txn| txn.flows.ids()).collect()
}

/// Everything whose source overlaps the line, described in a sentence.
fn items<'s>(book: &Book<'s>, run: &Run, at: Loc, flows: &[Id<Flow>]) -> Vec<(Cell<'s>, Loc)> {
    let sentence = |parts: Vec<Cell<'s>>| Cell::Join("", parts);
    let mut items = Vec::new();
    for &id in flows {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        let out = Cell::amount(book, posting.out());
        let amounts =
            if flow.is_exchange() { [out, "for".into(), Cell::amount(book, posting.arrive())].into() } else { out };
        items.push((sentence(vec!["flow: ".into(), route(book, flow), ", ".into(), amounts]), flow.loc));
    }
    for (index, assertion) in book.asserts.iter().enumerate().filter(|(_, assertion)| overlaps(assertion.loc, at)) {
        let gap = run.pads.iter().find(|pad| pad.assert as usize == index).map(|pad| gap_words(book, pad));
        let mut parts = vec![
            "assertion: ".into(),
            Cell::Name(path(book, assertion.place)),
            " = ".into(),
            Cell::amount(book, assertion.amount),
        ];
        parts.extend(gap.map(|words| sentence(vec![", ".into(), words])));
        items.push((sentence(parts), assertion.loc));
    }
    for event in book.events.iter().filter(|event| overlaps(event.loc, at)) {
        let parts = vec!["event: ".into(), Cell::code(book, event.code), " ".into(), event_words(event.state)];
        items.push((sentence(parts), event.loc));
    }
    for quote in book.prices.quotes().iter().filter(|quote| overlaps(quote.loc, at)) {
        let (unit, priced_in) = (&book.commodities[quote.unit], &book.commodities[quote.quote]);
        let text = [
            "price: 1".into(),
            Cell::Name(book.name(unit.symbol)),
            "=".into(),
            Cell::Number(quote.rate),
            Cell::Name(book.name(priced_in.symbol)),
            "on".into(),
            Cell::Day(quote.day),
        ];
        items.push((text.into(), quote.loc));
    }
    let laws = book.laws.values().filter(|law| overlaps(law.loc, at));
    items.extend(laws.map(|law| (["law".into(), Cell::Name(book.name(law.name))].into(), law.loc)));
    items.extend(declarations(book, at));
    items
}

/// Places, entities and commodities declared on the line.
fn declarations<'s>(book: &Book<'s>, at: Loc) -> Vec<(Cell<'s>, Loc)> {
    let places = book.places.values().map(|place| ("place", place.path, place.loc));
    let entities = book.entities.values().map(|entity| ("entity", entity.path, entity.loc));
    let commodities = book.commodities.values().map(|commodity| ("commodity", commodity.symbol, commodity.loc));
    places
        .chain(entities)
        .chain(commodities)
        .filter_map(|(what, name, loc)| {
            let loc = loc.filter(|&loc| overlaps(loc, at))?;
            Some((["declares".into(), what.into(), Cell::Name(book.name(name))].into(), loc))
        })
        .collect()
}

/// What one flow did downstream: gains, obligations, tallies, violations.
fn consequences<'s>(book: &Book<'s>, run: &Run, id: Id<Flow>, section: &mut Section<'s>) {
    let cause = Cause::Flow(id);
    for gain in run.gains.iter().filter(|gain| gain.cause == cause) {
        let mut text = vec![
            "realized a gain of".into(),
            Cell::amount(book, Amount::new(gain.gain(), book.base)),
            "selling".into(),
            Cell::amount(book, Amount::new(gain.qty, gain.unit)),
            "from".into(),
            Cell::Name(path(book, gain.from)),
        ];
        if gain.ambiguous {
            text.push("(no lot policy: FIFO assumed)".into());
        }
        section.push(Row::new([Cell::Join(" ", text), Cell::Blank]));
    }
    for effect in run.effects.iter().filter(|effect| effect.cause == cause) {
        let (name, amount) = (Cell::Name(book.name(effect.name)), Cell::amount(book, effect.amount));
        let text = match effect.owe {
            Some(owed) => vec!["owes".into(), amount, "to".into(), creditor(book, owed), ":".into(), name],
            None => vec!["counts".into(), amount, "as".into(), name],
        };
        section.push(Row::new([Cell::Join(" ", text), Cell::Name(book.name(book.laws[effect.law].name))]));
    }
    for violation in run.violations.iter().filter(|violation| violation.cause == cause) {
        let message = Cell::Said(run.diagnostics[violation.diagnostic as usize].message.clone());
        let style = if violation.waived { Style::Muted } else { Style::Alert };
        section.push(Row::new([message, Cell::Name(book.name(book.laws[violation.law].name))]).style(style));
    }
}
