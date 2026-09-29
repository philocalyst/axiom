//! `why FILE:LINE`: what is written on one source line, and everything it caused.

use axiom_core::{Id, Loc};
use axiom_engine::{Cause, Run};
use axiom_model::{Amount, Book, Flow};

use super::event_words;
use crate::history::Posting;
use crate::places::{path, route};
use crate::table::creditor;
use crate::{Cell, Column, Report, Row, Section, Style};

/// Explains the items whose source overlaps `at`.
pub fn line<'s>(book: &Book<'s>, run: &Run, at: Loc) -> Report<'s> {
    let flows = flows_on(book, at);
    let mut written = Section::new([Column::left("On this line"), Column::left("Source")]);
    for (text, loc) in items(book, run, at, &flows) {
        written.push(Row::new([Cell::text(text), Cell::Source(loc)]));
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
    headers.flat_map(|txn| (0..txn.len).map(move |leg| Id::new(txn.first.index() as u32 + leg))).collect()
}

/// Everything whose source overlaps the line, described in a sentence.
fn items(book: &Book, run: &Run, at: Loc, flows: &[Id<Flow>]) -> Vec<(String, Loc)> {
    let mut items = Vec::new();
    for &id in flows {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        let amounts = if flow.is_exchange() {
            format!("{} for {}", book.show(posting.out()), book.show(posting.arrive()))
        } else {
            book.show(posting.out()).to_string()
        };
        items.push((format!("flow: {}, {amounts}", route(book, flow)), flow.loc));
    }
    for (index, assertion) in book.asserts.iter().enumerate().filter(|(_, assertion)| overlaps(assertion.loc, at)) {
        let padded = run.pads.iter().any(|pad| pad.assert as usize == index);
        let gap = if padded { ", gap accepted with !" } else { "" };
        items.push((
            format!("assertion: {} = {}{gap}", path(book, assertion.place), book.show(assertion.amount)),
            assertion.loc,
        ));
    }
    for event in book.events.iter().filter(|event| overlaps(event.loc, at)) {
        let code = book.name(event.code).trim_start_matches('#');
        items.push((format!("event: #{code} {}", event_words(event.state)), event.loc));
    }
    for quote in book.prices.quotes().iter().filter(|quote| overlaps(quote.loc, at)) {
        let (unit, priced_in) = (&book.commodities[quote.unit], &book.commodities[quote.quote]);
        let text = format!(
            "price: 1 {} = {} {} on {}",
            book.name(unit.symbol),
            quote.rate,
            book.name(priced_in.symbol),
            quote.day
        );
        items.push((text, quote.loc));
    }
    items.extend(
        book.laws
            .values()
            .filter(|law| overlaps(law.loc, at))
            .map(|law| (format!("law {}", book.name(law.name)), law.loc)),
    );
    items.extend(declarations(book, at));
    items
}

/// Places, entities and commodities declared on the line.
fn declarations(book: &Book, at: Loc) -> Vec<(String, Loc)> {
    let places = book.places.values().map(|place| ("place", place.path, place.loc));
    let entities = book.entities.values().map(|entity| ("entity", entity.path, entity.loc));
    let commodities = book.commodities.values().map(|commodity| ("commodity", commodity.symbol, commodity.loc));
    places
        .chain(entities)
        .chain(commodities)
        .filter_map(|(what, name, loc)| {
            loc.filter(|&loc| overlaps(loc, at)).map(|loc| (format!("declares {what} {}", book.name(name)), loc))
        })
        .collect()
}

/// What one flow did downstream: gains, obligations, tallies, violations.
fn consequences<'s>(book: &Book<'s>, run: &Run, id: Id<Flow>, section: &mut Section<'s>) {
    let cause = Cause::Flow(id);
    for gain in run.gains.iter().filter(|gain| gain.cause == cause) {
        let ambiguity = if gain.ambiguous { " (no lot policy: FIFO assumed)" } else { "" };
        let text = format!(
            "realized a gain of {} selling {} from {}{ambiguity}",
            book.show(Amount::new(gain.gain(), book.base)),
            book.show(Amount::new(gain.qty, gain.unit)),
            path(book, gain.from)
        );
        section.push(Row::new([Cell::text(text), Cell::Blank]));
    }
    for effect in run.effects.iter().filter(|effect| effect.cause == cause) {
        let name = book.name(effect.name);
        let text = match effect.owe {
            Some(owed) => format!("owes {} to {}: {name}", book.show(effect.amount), creditor(book, owed)),
            None => format!("counts {} as {name}", book.show(effect.amount)),
        };
        section.push(Row::new([Cell::text(text), Cell::text(book.name(book.laws[effect.law].name))]));
    }
    for violation in run.violations.iter().filter(|violation| violation.cause == cause) {
        let message = run.diagnostics[violation.diagnostic as usize].message.clone();
        let style = if violation.waived { Style::Muted } else { Style::Alert };
        section
            .push(Row::new([Cell::text(message), Cell::text(book.name(book.laws[violation.law].name))]).style(style));
    }
}
