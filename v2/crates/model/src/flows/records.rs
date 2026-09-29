//! Everything the journal records besides transactions: code rules, balance
//! assertions, settlement events, prices, plans and syncs.

use axiom_core::diag::closest;
use axiom_core::glob::is_pattern;
use axiom_core::{Day, Diagnostic, Id};

use super::txn::{Dated, Sink, elaborate};
use crate::book::{Amount, CodeRule, CodeScope, SyncSpec};
use crate::catalog::Catalog;
use crate::journal::{Assert, Event, Mode, Plan, Quote, Waive};
use crate::scope::Home;
use crate::survey::{class_of, code_text};
use crate::world::World;

/// `code trip-*` / `on expenses/travel/*`: where codes may appear. A rule from
/// a system the project does not use never applies.
pub(super) fn code_rules<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    for written in &catalog.codes {
        let rule = written.what;
        let mut scopes = Vec::with_capacity(rule.on.len());
        for name in &rule.on {
            if is_pattern(name.text) || class_of(name.text).is_some() {
                scopes.push(CodeScope::Places(world.book.names.intern(name.text)));
                continue;
            }
            match world.kind(written.home(), *name) {
                Ok(kind) => scopes.push(CodeScope::Kind(kind)),
                Err(diagnostic) => diags.push(diagnostic),
            }
        }
        if !world.scopes.of(Home::Project).sees(written.home()) {
            continue;
        }
        let pattern = world.book.names.intern(code_text(rule.pattern.text));
        world.book.codes.push(CodeRule { pattern, on: scopes.into(), loc: written.item.loc });
    }
}

/// `2026-01-31 checking = 7_921.30 USD`
pub(super) fn asserts<'s>(world: &World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) -> Vec<Assert> {
    let mut asserts = Vec::with_capacity(catalog.asserts.len());
    for written in &catalog.asserts {
        let assert = written.what;
        let end = world.end(assert.place.name);
        let stated = match assert.amount.unit {
            Some(unit) => world.amount(assert.amount.num, unit, assert.amount.loc).map(Some),
            None => Ok(None),
        };
        match (end, stated) {
            (Ok(end), Ok(stated)) => asserts.push(Assert {
                day: assert.date,
                place: end.place,
                amount: stated.unwrap_or_else(|| zero_of(world, end.place)),
                pad: assert
                    .waive
                    .map(|waive| Waive { loc: waive.loc, reason: waive.reason.map(|reason| world.sym(reason)) }),
                loc: written.item.loc,
            }),
            (end, stated) => diags.extend(end.err().into_iter().chain(stated.err())),
        }
    }
    asserts.sort_by_key(|assert| assert.day);
    asserts
}

/// `= empty` asserts nothing is left: zero of the account's only commodity,
/// or of the base currency when it holds several.
fn zero_of(world: &World, place: Id<crate::book::Place>) -> Amount {
    match world.book.places[place].holds.as_deref() {
        Some([only]) => Amount::zero(*only),
        _ => Amount::zero(world.book.base),
    }
}

/// `2026-02-06 #check-1041 settled`: the code must mark something.
pub(super) fn events<'s>(world: &World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) -> Vec<Event> {
    let mut events = Vec::with_capacity(catalog.events.len());
    for written in &catalog.events {
        let event = written.what;
        let text = code_text(event.code.text);
        let code = world.sym(text);
        if !world.codes.contains(&code) {
            let known = world.codes.iter().map(|&known| world.book.name(known));
            let mut diagnostic = Diagnostic::error("unknown-code", format!("no transaction is marked `#{text}`"))
                .label(event.code.loc, "nothing carries this code")
                .note("an event names the transaction it changes by its code");
            if let Some(near) = closest(text, known) {
                diagnostic = diagnostic.fix(format!("did you mean `#{near}`?"), event.code.loc, format!("#{near}"));
            }
            diags.push(diagnostic);
            continue;
        }
        events.push(Event { day: event.date, code, state: event.state, loc: written.item.loc });
    }
    events.sort_by_key(|event| event.day);
    events
}

pub(super) fn syncs<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>) -> Vec<SyncSpec> {
    let names = &mut world.book.names;
    catalog
        .syncs
        .iter()
        .map(|written| SyncSpec {
            file: names.intern(written.what.file.text),
            run: names.intern(written.what.run.text),
            loc: written.item.loc,
        })
        .collect()
}

/// `2026-01-02 VTI 280.14 USD`
pub(super) fn written_prices<'s>(
    world: &World<'s>,
    catalog: &Catalog<'_, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Quote> {
    let mut quotes = Vec::with_capacity(catalog.prices.len());
    for written in &catalog.prices {
        let price = written.what;
        let Some(quote_unit) = price.price.unit else {
            diags.push(
                Diagnostic::error("price-unit", "a price is an amount of some commodity")
                    .label(price.price.loc, "which commodity?")
                    .help("as in `2026-01-02 VTI 280.14 USD`"),
            );
            continue;
        };
        let units = (world.commodity(price.unit), world.commodity(quote_unit));
        let rate = price.price.num.to_ratio().filter(|rate| !rate.is_zero());
        match (units, rate) {
            ((Ok(unit), Ok(quote)), Some(rate)) if unit != quote => {
                quotes.push(Quote { unit, quote, day: price.date, rate, implied: false, loc: written.item.loc });
            }
            ((Ok(_), Ok(_)), Some(_)) => diags.push(
                Diagnostic::error("price-self", "a commodity's price in itself is always one")
                    .label(price.unit.loc, "priced in itself")
                    .help("quote it in another commodity"),
            ),
            ((Ok(_), Ok(_)), None) => diags.push(
                Diagnostic::error("price-zero", "a price is more than nothing").label(price.price.loc, "this price"),
            ),
            ((unit, quote), _) => diags.extend(unit.err().into_iter().chain(quote.err())),
        }
    }
    quotes
}

/// `every month on 1 checking -> landlord 2_400 USD`: one occurrence, as flows
/// of mode `Planned`, under a transaction of their own so that every flow's
/// `txn` is valid. That transaction holds no journal flows.
pub(super) fn plans<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    let shared: &World<'s> = world;
    let mut elaborated = Vec::with_capacity(catalog.plans.len());
    for written in &catalog.plans {
        let plan = written.what;
        // Forecasts re-date copies, so this day only has to be a day.
        let day = plan.from.unwrap_or(Day(0));
        let dated = Dated { day, until: day, mode: Mode::Planned };
        let mut sink = Sink::default();
        elaborate(shared, written.item, dated, &plan.flow, &mut sink);
        elaborated.push((written, sink));
    }
    for (written, mut sink) in elaborated {
        diags.append(&mut sink.diags);
        if sink.flows.is_empty() {
            continue;
        }
        let id = Id::new(world.book.txns.len() as u32);
        sink.flows.iter_mut().for_each(|flow| flow.txn = id);
        // The plan's transaction holds no journal flows: its template is not in the book's flows.
        let mut txn = sink.txns.swap_remove(0);
        (txn.first, txn.len) = (Id::new(0), 0);
        world.book.txns.push(txn);
        let plan = written.what;
        world.book.plans.push(Plan {
            every: plan.every,
            on: plan.on,
            from: plan.from,
            until: plan.until,
            template: sink.flows.into(),
            loc: written.item.loc,
        });
    }
}
