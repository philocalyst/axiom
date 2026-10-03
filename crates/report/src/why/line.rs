//! `why FILE:LINE`: what is written on one source line, and everything it caused.

use std::collections::BTreeSet;

use axiom_core::{Id, Loc, Sym};
use axiom_engine::{Cause, Run};
use axiom_model::{Action, Amount, Book, Flow, Promised, Provenance, Purposed, Subject, Terms};

use super::event_words;
use crate::flow::{object_name, scoped_movement_qty};
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
    for (text, loc) in (OnLine { lens, run, at }).items(&flows) {
        written.push(Row::new([Cell::text(text), Cell::Source(loc)]));
    }
    if written.rows.is_empty() {
        written.note("Nothing that the book records is written on that line.");
    }

    let mut caused = Section::new([Column::left("What it caused"), Column::left("Law")]);
    for id in flows {
        consequences(lens, run, id, &mut caused);
    }
    Report::new("Why this line").with(written).with(caused.headed("Consequences"))
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
        .filter(|(_, flow)| overlaps(flow.loc, at) && lens.owns(crate::flow::movement_place(lens, flow)))
        .map(|(id, _)| id)
        .collect();
    if !direct.is_empty() {
        return direct;
    }
    let headers = book.txns.values().filter(|txn| overlaps(txn.loc, at));
    headers
        .flat_map(|txn| txn.flows.ids())
        .filter(|&id| lens.owns(crate::flow::movement_place(lens, &book.flows[id])))
        .collect()
}

/// What the book records on one source line, as a lens sees it: each thing a sentence, and where it is written.
struct OnLine<'a> {
    lens: Lens<'a, 'a, 'a, 'a>,
    run: &'a Run,
    at: Loc,
}

impl OnLine<'_> {
    /// Everything whose source overlaps the line, described in a sentence.
    fn items(&self, flows: &[Id<Flow>]) -> Vec<(String, Loc)> {
        let book = self.lens.book();
        let codes = scoped_codes(book, self.lens);
        let mut items: Vec<_> = flows.iter().map(|&id| self.flow(id)).collect();
        items.extend(self.assertions());
        items.extend(self.events(&codes));
        items.extend(self.prices());
        items.extend(self.laws());
        items.extend(self.measures());
        items.extend(self.filed());
        items.extend(self.readings(&codes));
        items.extend(declarations(book, self.at));
        items
    }

    fn flow(&self, id: Id<Flow>) -> (String, Loc) {
        let (book, lens) = (self.lens.book(), self.lens);
        let posting = Posting::at(book, self.run, id);
        let flow = posting.flow;
        let scoped = |amount: Amount| Amount::new(scoped_movement_qty(lens, flow, amount.qty), amount.unit);
        let (out, arrive) = (scoped(posting.out()), scoped(posting.arrive()));
        let amounts = if flow.is_exchange() {
            format!("{} for {}", book.show(out), book.show(arrive))
        } else {
            book.show(out).to_string()
        };
        let purpose = flow.purpose.map_or_else(String::new, |purposed| purpose_words(book, purposed));
        let codes = book.flow_view(flow).codes().map(|code| format!(" ^{}", book.name(code))).collect::<String>();
        (format!("flow: {}, {amounts}{purpose}{codes}", route(book, flow)), flow.loc)
    }

    fn assertions(&self) -> impl Iterator<Item = (String, Loc)> {
        let (book, lens, run, at) = (self.lens.book(), self.lens, self.run, self.at);
        let on_line = book.asserts.iter().enumerate();
        on_line.filter(move |(_, assertion)| overlaps(assertion.loc, at) && lens.owns(assertion.place)).map(
            move |(index, assertion)| {
                let gap = run.pads.iter().find(|pad| pad.assert as usize == index).map(|pad| gap_words(book, pad));
                let gap = gap.map_or(String::new(), |words| format!(", {words}"));
                let held = Amount::new(lens.place_qty(assertion.place, assertion.amount.qty), assertion.amount.unit);
                (format!("assertion: {} = {}{gap}", path(book, assertion.place), book.show(held)), assertion.loc)
            },
        )
    }

    fn events(&self, codes: &BTreeSet<Sym>) -> impl Iterator<Item = (String, Loc)> {
        let (book, at, visible) = (self.lens.book(), self.at, self.sees_every_code(codes));
        let on_line = book.events.iter().filter(move |event| overlaps(event.loc, at) && visible(event.code));
        on_line.map(move |event| (format!("event: ^{} {}", book.name(event.code), event_words(event.state)), event.loc))
    }

    fn prices(&self) -> impl Iterator<Item = (String, Loc)> {
        let (book, at) = (self.lens.book(), self.at);
        book.prices.quotes().iter().filter(move |quote| overlaps(quote.loc, at)).map(move |quote| {
            let (unit, priced_in) = (&book.commodities[quote.unit], &book.commodities[quote.quote]);
            let text = format!(
                "price: 1 {} = {} {} on {}",
                book.name(unit.symbol),
                quote.rate,
                book.name(priced_in.symbol),
                quote.day
            );
            (text, quote.loc)
        })
    }

    fn laws(&self) -> impl Iterator<Item = (String, Loc)> {
        let (book, at) = (self.lens.book(), self.at);
        book.laws
            .values()
            .filter(move |law| overlaps(law.loc, at))
            .map(move |law| (format!("law {}", book.name(law.name)), law.loc))
    }

    fn measures(&self) -> impl Iterator<Item = (String, Loc)> {
        let (book, lens, at) = (self.lens.book(), self.lens, self.at);
        let on_line =
            book.measures.values().filter(move |measure| overlaps(measure.loc, at) && lens.owns_entity(measure.owner));
        on_line.map(move |measure| {
            let action = match measure.action {
                Action::Work => "worked",
                Action::Use => "used",
            };
            let subject = match measure.subject {
                Subject::Entity(id) => book.name(book.entities[id].path),
                Subject::Place(id) => book.name(book.places[id].path),
                Subject::Asset(id) => book.name(book.assets[id].name),
                Subject::Contract(id) => book.name(book.contracts[id].name),
            };
            let purpose = measure.purpose.map_or_else(String::new, |purpose| {
                format!(" for #{}", book.name(book.purposes[purpose.purpose].name))
            });
            (format!("measure: {subject} {action} {}{purpose}", book.show(measure.quantity)), measure.loc)
        })
    }

    fn filed(&self) -> impl Iterator<Item = (String, Loc)> {
        let (book, lens, at) = (self.lens.book(), self.lens, self.at);
        let on_line = book.filed.iter().filter(move |filed| overlaps(filed.loc, at) && lens.owns_entity(filed.owner));
        on_line.map(move |filed| {
            let (system, owner) =
                (book.name(book.systems[filed.system].path), book.name(book.entities[filed.owner].path));
            (format!("filed: {} for {system} by {owner}", filed.year), filed.loc)
        })
    }

    fn readings(&self, codes: &BTreeSet<Sym>) -> impl Iterator<Item = (String, Loc)> {
        let (book, at, visible) = (self.lens.book(), self.at, self.sees_every_code(codes));
        let on_line = book.readings.iter().filter(move |reading| overlaps(reading.loc, at) && visible(reading.code));
        on_line.map(move |reading| {
            (format!("reading: ^{} = {}", book.name(reading.code), book.show(reading.amount)), reading.loc)
        })
    }

    /// Events and readings have no owner, so an owner's view shows those whose code something it owns refers to.
    fn sees_every_code<'c>(&self, codes: &'c BTreeSet<Sym>) -> impl Fn(Sym) -> bool + 'c {
        let everyone = self.lens.whose.is_everyone();
        move |code| everyone || codes.contains(&code)
    }
}

/// A flow's purpose in words: what for, of what, and who said so.
fn purpose_words(book: &Book, purposed: Purposed) -> String {
    let object = purposed.of.map_or_else(String::new, |object| format!(" of {}", object_name(book, object)));
    let source = match purposed.source {
        Provenance::Written => "written".to_string(),
        Provenance::Contract(id) => format!("contract {}", book.name(book.contracts[id].name)),
        Provenance::Entity(id) => format!("party {}", book.name(book.entities[id].path)),
        Provenance::Party(id) => format!("party kind {}", book.name(book.kinds[id].name)),
        Provenance::Commodity(id) => format!("commodity kind {}", book.name(book.kinds[id].name)),
        Provenance::Account(id) => format!("account kind {}", book.name(book.kinds[id].name)),
        Provenance::Derived => "derived".to_string(),
    };
    format!(" for #{}{object} ({source})", book.name(book.purposes[purposed.purpose].name))
}

/// Codes named by data visible in this owner scope. Events and readings have
/// no owner of their own, so an owner-scoped source query only exposes them
/// when a visible flow, measure or contract refers to their code.
pub(super) fn scoped_codes(book: &Book, lens: Lens<'_, '_, '_, '_>) -> BTreeSet<axiom_core::Sym> {
    let mut codes = BTreeSet::new();
    for (_, flow) in book.flows.iter().filter(|(_, flow)| lens.owns(crate::flow::movement_place(lens, flow))) {
        codes.extend(book.flow_view(flow).codes());
    }
    for (_, measure) in book.measures.iter().filter(|(_, measure)| lens.owns_entity(measure.owner)) {
        codes.extend(measure.codes.iter().copied());
    }
    for (_, contract) in book.contracts.iter().filter(|(_, contract)| lens.owns_entity(contract.owner)) {
        for terms in [&contract.terms, &contract.standing].into_iter().flatten() {
            add_terms_codes(book, terms, &mut codes);
        }
    }
    codes
}

fn add_terms_codes(book: &Book, terms: &Terms, codes: &mut BTreeSet<axiom_core::Sym>) {
    for template in &terms.template {
        add_template_codes(book, template, codes);
    }
}

fn add_template_codes(book: &Book, template: &Promised, codes: &mut BTreeSet<axiom_core::Sym>) {
    codes.extend(book.flow_view(&template.header.flow).codes());
    for leg in &template.legs {
        codes.extend(book.flow_view(&leg.flow).codes());
    }
    for item in &template.items {
        codes.extend(book.codes[item.flow.codes].iter().copied());
    }
}

/// Places, entities and commodities declared on the line.
fn declarations(book: &Book, at: Loc) -> Vec<(String, Loc)> {
    let places = book.places.values().map(|place| ("place", place.path, place.loc));
    let entities = book.entities.values().map(|entity| ("entity", entity.path, entity.loc));
    let commodities = book.commodities.values().map(|commodity| ("commodity", commodity.symbol, commodity.loc));
    let purposes = book.purposes.values().map(|purpose| ("purpose", purpose.name, purpose.loc));
    let contracts = book.contracts.values().map(|contract| ("contract", contract.name, Some(contract.loc)));
    let assets = book.assets.values().map(|asset| ("asset", asset.name, Some(asset.loc)));
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
fn consequences<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, id: Id<Flow>, section: &mut Section<'s>) {
    let book = lens.book();
    let cause = Cause::Flow(id);
    for gain in run.gains.iter().filter(|gain| gain.cause == cause && lens.owns(gain.from)) {
        let realized = lens.place_qty(gain.from, gain.gain());
        let ambiguity = if gain.ambiguous { " (no lot policy: FIFO assumed)" } else { "" };
        let text = format!(
            "realized a gain of {} selling {} from {}{ambiguity}",
            book.show(Amount::new(realized, book.base)),
            book.show(Amount::new(gain.qty, gain.unit)),
            path(book, gain.from)
        );
        section.push(Row::new([Cell::text(text), Cell::Blank]));
    }
    for effect in run.effects.iter().filter(|effect| effect.cause == cause && lens.owns_entity(effect.owner)) {
        let name = book.name(effect.name);
        let text = match effect.owed() {
            Some(owed) => format!("owes {} to {}: {name}", book.show(effect.amount), creditor(book, owed)),
            None => format!("counts {} as {name}", book.show(effect.amount)),
        };
        section.push(Row::new([Cell::text(text), Cell::text(book.name(book.laws[effect.law].name))]));
    }
    for violation in
        run.violations.iter().filter(|violation| violation.cause == cause && lens.governs(violation.subject))
    {
        let message = run.diagnostics[violation.diagnostic as usize].message.clone();
        let style = if violation.verdict.is_waived() { Style::Muted } else { Style::Alert };
        section
            .push(Row::new([Cell::text(message), Cell::text(book.name(book.laws[violation.law].name))]).style(style));
    }
}
