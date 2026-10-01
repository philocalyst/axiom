//! `why FILE:LINE`: what is written on one source line, and everything it caused.

use std::collections::BTreeSet;

use axiom_core::{Id, Loc};
use axiom_engine::{Cause, Run};
use axiom_model::{Amount, Book, Flow, Object, Provenance, TemplateFlow, Terms};

use super::event_words;
use crate::history::Posting;
use crate::lens::Lens;
use crate::places::{path, route};
use crate::table::{creditor, gap_words};
use crate::{Cell, Column, Report, Row, Section, Style};

/// Explains the items whose source overlaps `at`.
pub fn line<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, at: Loc) -> Report<'s> {
    let book = lens.book();
    let flows = flows_on(book, at, lens);
    let mut written = Section::new([Column::left("On this line"), Column::left("Source")]);
    for (text, loc) in items(book, run, lens, at, &flows) {
        written.push(Row::new([Cell::text(text), Cell::Source(loc)]));
    }
    if written.rows.is_empty() {
        written.note("Nothing that the book records is written on that line.");
    }

    let mut caused = Section::new([Column::left("What it caused"), Column::left("Law")]);
    for id in flows {
        consequences(lens, run, id, &mut caused);
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
fn flows_on(book: &Book, at: Loc, lens: Lens<'_, '_, '_, '_>) -> Vec<Id<Flow>> {
    let direct: Vec<Id<Flow>> = book
        .flows
        .iter()
        .filter(|(_, flow)| overlaps(flow.loc, at) && lens.owns_entity(flow.owner))
        .map(|(id, _)| id)
        .collect();
    if !direct.is_empty() {
        return direct;
    }
    let headers = book.txns.values().filter(|txn| overlaps(txn.loc, at));
    headers
        .flat_map(|txn| txn.flows.ids())
        .filter(|&id| lens.owns_entity(book.flows[id].owner))
        .collect()
}

/// Everything whose source overlaps the line, described in a sentence.
fn items(
    book: &Book,
    run: &Run,
    lens: Lens<'_, '_, '_, '_>,
    at: Loc,
    flows: &[Id<Flow>],
) -> Vec<(String, Loc)> {
    let mut items = Vec::new();
    let visible_codes = scoped_codes(book, lens);
    for &id in flows {
        let posting = Posting::at(book, run, id);
        let flow = posting.flow;
        let out = posting.out();
        let out = Amount::new(lens.entity_qty(flow.owner, out.qty), out.unit);
        let arrive = posting.arrive();
        let arrive = Amount::new(lens.entity_qty(flow.owner, arrive.qty), arrive.unit);
        let amounts = if flow.is_exchange() {
            format!("{} for {}", book.show(out), book.show(arrive))
        } else {
            book.show(out).to_string()
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
        .filter(|(_, assertion)| overlaps(assertion.loc, at) && lens.owns(assertion.place))
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
                book.show(Amount::new(
                    lens.place_qty(assertion.place, assertion.amount.qty),
                    assertion.amount.unit,
                ))
            ),
            assertion.loc,
        ));
    }
    for event in book.events.iter().filter(|event| {
        overlaps(event.loc, at) && (lens.whose.is_everyone() || visible_codes.contains(&event.code))
    }) {
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
            .map(|(_, measure)| measure)
            .filter(|measure| overlaps(measure.loc, at) && lens.owns_entity(measure.owner))
            .map(|measure| {
                let action = match measure.action {
                    axiom_model::Action::Work => "worked",
                    axiom_model::Action::Use => "used",
                };
                let subject = match measure.subject {
                    axiom_model::Subject::Entity(id) => book.name(book.entities[id].path),
                    axiom_model::Subject::Place(id) => book.name(book.places[id].path),
                    axiom_model::Subject::Asset(id) => book.name(book.assets[id].name),
                    axiom_model::Subject::Contract(id) => book.name(book.contracts[id].name),
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
            .filter(|filed| overlaps(filed.loc, at) && lens.owns_entity(filed.owner))
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
            .filter(|reading| {
                overlaps(reading.loc, at)
                    && (lens.whose.is_everyone() || visible_codes.contains(&reading.code))
            })
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

/// Codes named by data visible in this owner scope. Events and readings have
/// no owner of their own, so an owner-scoped source query only exposes them
/// when a visible flow, measure or contract refers to their code.
pub(super) fn scoped_codes(book: &Book, lens: Lens<'_, '_, '_, '_>) -> BTreeSet<axiom_core::Sym> {
    let mut codes = BTreeSet::new();
    for (_, flow) in book
        .flows
        .iter()
        .filter(|(_, flow)| lens.owns_entity(flow.owner))
    {
        codes.extend(book.flow_view(flow).codes());
    }
    for (_, measure) in book
        .measures
        .iter()
        .filter(|(_, measure)| lens.owns_entity(measure.owner))
    {
        codes.extend(measure.codes.iter().copied());
    }
    for (_, contract) in book
        .contracts
        .iter()
        .filter(|(_, contract)| lens.owns_entity(contract.owner))
    {
        for (_, terms) in contract
            .terms
            .iter()
            .flat_map(|terms| terms.within(contract.days))
        {
            add_terms_codes(book, terms, &mut codes);
        }
        if let Some(standing) = &contract.standing {
            for (_, terms) in standing.within(contract.days) {
                add_terms_codes(book, terms, &mut codes);
            }
        }
    }
    codes
}

fn add_terms_codes(book: &Book, terms: &Terms, codes: &mut BTreeSet<axiom_core::Sym>) {
    for template in &terms.template {
        add_template_codes(book, template, codes);
    }
}

fn add_template_codes(book: &Book, template: &TemplateFlow, codes: &mut BTreeSet<axiom_core::Sym>) {
    codes.extend(book.flow_view(&template.flow).codes());
    for leg in &template.legs {
        codes.extend(book.flow_view(&leg.flow).codes());
    }
    for item in &template.items {
        codes.extend(book.codes[item.codes].iter().copied());
    }
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
fn consequences<'s>(
    lens: Lens<'s, '_, '_, '_>,
    run: &Run,
    id: Id<Flow>,
    section: &mut Section<'s>,
) {
    let book = lens.book();
    let cause = Cause::Flow(id);
    for gain in run
        .gains
        .iter()
        .filter(|gain| gain.cause == cause && lens.owns(gain.from))
    {
        let realized = lens.place_qty(gain.from, gain.gain());
        let ambiguity = if gain.ambiguous {
            " (no lot policy: FIFO assumed)"
        } else {
            ""
        };
        let text = format!(
            "realized a gain of {} selling {} from {}{ambiguity}",
            book.show(Amount::new(realized, book.base)),
            book.show(Amount::new(gain.qty, gain.unit)),
            path(book, gain.from)
        );
        section.push(Row::new([Cell::text(text), Cell::Blank]));
    }
    for effect in run
        .effects
        .iter()
        .filter(|effect| effect.cause == cause && lens.owns_entity(effect.owner))
    {
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
        .filter(|violation| violation.cause == cause && lens.governs(violation.subject))
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
