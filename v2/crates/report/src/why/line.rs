//! `why FILE:LINE`: what is written on one source line, and everything it caused.

use axiom_core::{Id, Loc};
use axiom_engine::{Cause, Run};
use axiom_model::{Amount, Book, Flow, Object, Provenance};

use super::event_words;
use crate::history::Posting;
use crate::lens::Whose;
use crate::places::{path, route};
use crate::table::{creditor, gap_words};
use crate::{Cell, Column, Report, Row, Section, Style};

/// Explains the items whose source overlaps `at`.
pub fn line<'s>(book: &Book<'s>, run: &Run, whose: &Whose, at: Loc) -> Report<'s> {
    let flows = flows_on(book, at, whose);
    let mut written = Section::new([Column::left("On this line"), Column::left("Source")]);
    for (text, loc) in items(book, run, whose, at, &flows) {
        written.push(Row::new([Cell::text(text), Cell::Source(loc)]));
    }
    if written.rows.is_empty() {
        written.note("Nothing that the book records is written on that line.");
    }

    let mut caused = Section::new([Column::left("What it caused"), Column::left("Law")]);
    for id in flows {
        consequences(book, run, id, &mut caused);
    }
    Report::new("Why this line")
        .with(written)
        .with(caused.headed("Consequences"))
}

fn overlaps(a: Loc, b: Loc) -> bool {
    a.file == b.file && a.start < b.end && b.start < a.end
}

/// The flows written on the line. The header of a split transaction holds no
/// flow of its own; its legs do, so it stands for all of them.
fn flows_on(book: &Book, at: Loc, whose: &Whose) -> Vec<Id<Flow>> {
    let direct: Vec<Id<Flow>> = book
        .flows
        .iter()
        .filter(|(_, flow)| overlaps(flow.loc, at) && whose.includes(flow.owner))
        .map(|(id, _)| id)
        .collect();
    if !direct.is_empty() {
        return direct;
    }
    let headers = book.txns.values().filter(|txn| overlaps(txn.loc, at));
    headers
        .flat_map(|txn| txn.flows.ids())
        .filter(|&id| whose.includes(book.flows[id].owner))
        .collect()
}

/// Everything whose source overlaps the line, described in a sentence.
fn items(
    book: &Book,
    run: &Run,
    whose: &Whose,
    at: Loc,
    flows: &[Id<Flow>],
) -> Vec<(String, Loc)> {
    let mut items = Vec::new();
    for &id in flows {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        let amounts = if flow.is_exchange() {
            format!(
                "{} for {}",
                book.show(posting.out()),
                book.show(posting.arrive())
            )
        } else {
            book.show(posting.out()).to_string()
        };
        let purpose = flow.purpose.map_or_else(String::new, |purposed| {
            let object = purposed.of.map_or_else(String::new, |object| match object {
                Object::Asset(id) => format!(" of {}", book.name(book.assets[id].name)),
                Object::Place(id) => format!(" of {}", book.name(book.places[id].path)),
                Object::Entity(id) => format!(" of {}", book.name(book.entities[id].path)),
            });
            let source = match purposed.source {
                Provenance::Written => "written".to_string(),
                Provenance::Contract(id) => {
                    format!("contract {}", book.name(book.contracts[id].name))
                }
                Provenance::Entity(id) => format!("party {}", book.name(book.entities[id].path)),
                Provenance::Party(id) => format!("party kind {}", book.name(book.kinds[id].name)),
                Provenance::Commodity(id) => {
                    format!("commodity kind {}", book.name(book.kinds[id].name))
                }
                Provenance::Account(id) => {
                    format!("account kind {}", book.name(book.kinds[id].name))
                }
                Provenance::Derived => "derived".to_string(),
            };
            format!(
                " for #{}{object} ({source})",
                book.name(book.purposes[purposed.purpose].name)
            )
        });
        let codes = book
            .flow_view(flow)
            .codes()
            .map(|code| format!(" ^{}", book.name(code)))
            .collect::<String>();
        items.push((
            format!("flow: {}, {amounts}{purpose}{codes}", route(book, flow)),
            flow.loc,
        ));
    }
    for (index, assertion) in book
        .asserts
        .iter()
        .enumerate()
        .filter(|(_, assertion)| overlaps(assertion.loc, at))
    {
        let gap = run
            .pads
            .iter()
            .find(|pad| pad.assert as usize == index)
            .map(|pad| gap_words(book, pad));
        let gap = gap.map_or(String::new(), |words| format!(", {words}"));
        items.push((
            format!(
                "assertion: {} = {}{gap}",
                path(book, assertion.place),
                book.show(assertion.amount)
            ),
            assertion.loc,
        ));
    }
    for event in book.events.iter().filter(|event| overlaps(event.loc, at)) {
        items.push((
            format!(
                "event: ^{} {}",
                book.name(event.code),
                event_words(event.state)
            ),
            event.loc,
        ));
    }
    for quote in book
        .prices
        .quotes()
        .iter()
        .filter(|quote| overlaps(quote.loc, at))
    {
        let (unit, priced_in) = (
            &book.commodities[quote.unit],
            &book.commodities[quote.quote],
        );
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
    items.extend(
        book.measures
            .iter()
            .filter(|measure| overlaps(measure.loc, at) && whose.includes(measure.owner))
            .map(|measure| {
                let action = match measure.action {
                    axiom_model::Action::Work => "worked",
                    axiom_model::Action::Use => "used",
                };
                let subject = match measure.subject {
                    axiom_model::Subject::Entity(id) => book.name(book.entities[id].path),
                    axiom_model::Subject::Place(id) => book.name(book.places[id].path),
                    axiom_model::Subject::Asset(id) => book.name(book.assets[id].name),
                };
                let purpose = measure.purpose.map_or_else(String::new, |purpose| {
                    format!(" for #{}", book.name(book.purposes[purpose.purpose].name))
                });
                (
                    format!(
                        "measure: {subject} {action} {}{purpose}",
                        book.show(measure.quantity)
                    ),
                    measure.loc,
                )
            }),
    );
    items.extend(
        book.filed
            .iter()
            .filter(|filed| overlaps(filed.loc, at) && whose.includes(filed.owner))
            .map(|filed| {
                (
                    format!(
                        "filed: {} for {} by {}",
                        filed.year,
                        book.name(book.systems[filed.system].path),
                        book.name(book.entities[filed.owner].path)
                    ),
                    filed.loc,
                )
            }),
    );
    items.extend(
        book.readings
            .iter()
            .filter(|reading| overlaps(reading.loc, at))
            .map(|reading| {
                (
                    format!(
                        "reading: ^{} = {}",
                        book.name(reading.code),
                        book.show(reading.amount)
                    ),
                    reading.loc,
                )
            }),
    );
    items.extend(declarations(book, at));
    items
}

/// Places, entities and commodities declared on the line.
fn declarations(book: &Book, at: Loc) -> Vec<(String, Loc)> {
    let places = book
        .places
        .values()
        .map(|place| ("place", place.path, place.loc));
    let entities = book
        .entities
        .values()
        .map(|entity| ("entity", entity.path, entity.loc));
    let commodities = book
        .commodities
        .values()
        .map(|commodity| ("commodity", commodity.symbol, commodity.loc));
    let purposes = book
        .purposes
        .values()
        .map(|purpose| ("purpose", purpose.name, purpose.loc));
    let contracts = book
        .contracts
        .values()
        .map(|contract| ("contract", contract.name, Some(contract.loc)));
    let assets = book
        .assets
        .values()
        .map(|asset| ("asset", asset.name, Some(asset.loc)));
    places
        .chain(entities)
        .chain(commodities)
        .chain(purposes)
        .chain(contracts)
        .chain(assets)
        .filter_map(|(what, name, loc)| {
            loc.filter(|&loc| overlaps(loc, at)).map(|loc| {
                let sigil = if what == "purpose" { "#" } else { "" };
                (format!("declares {what} {sigil}{}", book.name(name)), loc)
            })
        })
        .collect()
}

/// What one flow did downstream: gains, obligations, tallies, violations.
fn consequences<'s>(book: &Book<'s>, run: &Run, id: Id<Flow>, section: &mut Section<'s>) {
    let cause = Cause::Flow(id);
    for gain in run.gains.iter().filter(|gain| gain.cause == cause) {
        let ambiguity = if gain.ambiguous {
            " (no lot policy: FIFO assumed)"
        } else {
            ""
        };
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
        let text = match effect.owed() {
            Some(owed) => format!(
                "owes {} to {}: {name}",
                book.show(effect.amount),
                creditor(book, owed)
            ),
            None => format!("counts {} as {name}", book.show(effect.amount)),
        };
        section.push(Row::new([
            Cell::text(text),
            Cell::text(book.name(book.laws[effect.law].name)),
        ]));
    }
    for violation in run
        .violations
        .iter()
        .filter(|violation| violation.cause == cause)
    {
        let message = run.diagnostics[violation.diagnostic as usize]
            .message
            .clone();
        let style = if violation.verdict.is_waived() {
            Style::Muted
        } else {
            Style::Alert
        };
        section.push(
            Row::new([
                Cell::text(message),
                Cell::text(book.name(book.laws[violation.law].name)),
            ])
            .style(style),
        );
    }
}
