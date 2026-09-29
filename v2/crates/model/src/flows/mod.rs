//! The journal: transactions elaborated into flows, and everything else
//! that is written down about them.
//!
//! Transactions are independent of each other, so runs of them are elaborated
//! on every core against the read-only world, each run into its own dense
//! vectors. Then the runs are laid end to end in day order, declaration order
//! deciding ties, and the book learns which flows touch which places.

mod faults;
mod pairing;
mod records;
mod txn;

use axiom_core::{Diagnostic, Groups, Id, par};
use axiom_syntax::Txn as WrittenTxn;

use self::txn::{Dated, Sink, elaborate};
use crate::book::{Book, Place};
use crate::catalog::{Catalog, Written};
use crate::journal::{Flow, Mode, Prices, Quote, Txn};
use crate::world::World;

pub(crate) fn record<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) {
    records::code_rules(world, catalog, diags);
    let mut quotes = records::written_prices(world, catalog, diags);
    quotes.extend(journal(world, catalog, diags));
    world.book.asserts = records::asserts(world, catalog, diags);
    world.book.events = records::events(world, catalog, diags);
    world.book.syncs = records::syncs(world, catalog);
    world.book.prices = Prices::new(quotes);
    records::plans(world, catalog, diags);
}

/// Elaborates every transaction, orders the flows, and indexes them by place.
/// Returns the prices the exchanges imply.
fn journal<'s>(world: &mut World<'s>, catalog: &Catalog<'_, 's>, diags: &mut Vec<Diagnostic>) -> Vec<Quote> {
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    // Twice as many runs as cores, so that a slow run does not leave cores idle.
    let splits = usize::BITS - cores.saturating_sub(1).leading_zeros() + 1;
    let mut sinks = elaborate_runs(world, &catalog.txns, catalog.layout_free, splits);

    let mut quotes = Vec::new();
    for sink in &mut sinks {
        diags.append(&mut sink.diags);
        quotes.append(&mut sink.quotes);
    }
    let book = &mut world.book;
    if in_day_order(&sinks) {
        for sink in sinks {
            let mut flows = sink.flows.into_iter();
            for txn in sink.txns {
                let own = flows.by_ref().take(txn.len as usize);
                lay_out(book, txn, own);
            }
        }
    } else {
        for (txn, flows) in sorted_by_day(sinks) {
            lay_out(book, txn, flows.into_iter());
        }
    }
    book.touching = touching(book.places.len(), book.flows.iter());
    quotes
}

/// Splits `items` in halves, `splits` times over, and elaborates the pieces
/// concurrently. Small pieces are not worth a thread.
fn elaborate_runs<'s>(world: &World<'s>, items: &[Written<WrittenTxn>], layout_free: bool, splits: u32) -> Vec<Sink> {
    const SEQUENTIAL: usize = 2048;
    if splits == 0 || items.len() <= SEQUENTIAL {
        return vec![elaborate_run(world, items, layout_free)];
    }
    let (left, right) = items.split_at(items.len() / 2);
    let (mut sinks, more) = par::join(
        || elaborate_runs(world, left, layout_free, splits - 1),
        || elaborate_runs(world, right, layout_free, splits - 1),
    );
    sinks.extend(more);
    sinks
}

fn elaborate_run<'s>(world: &World<'s>, items: &[Written<WrittenTxn>], layout_free: bool) -> Sink {
    let mut sink = Sink::with_room_for(items.len());
    for written in items {
        let txn = written.what;
        let dated = Dated { day: txn.date, until: txn.until.unwrap_or(txn.date), mode: Mode::Actual };
        elaborate(world, written.item, dated, &txn.flow, &mut sink);
        if !layout_free {
            let site = written.site;
            sink.diags.extend(site.layout.check_date(site.source.path, written.item));
        }
    }
    sink
}

/// Whether the runs, laid end to end, are already in day order: usually true,
/// since journals are written as time passes.
fn in_day_order(sinks: &[Sink]) -> bool {
    let mut last = None;
    for sink in sinks {
        let (Some(first), Some(end)) = (sink.txns.first(), sink.txns.last()) else { continue };
        if sink.unordered || last.is_some_and(|last| last > first.day) {
            return false;
        }
        last = Some(end.day);
    }
    true
}

/// Every transaction with its flows, in day order; ties keep declaration order.
fn sorted_by_day(sinks: Vec<Sink>) -> Vec<(Txn, Vec<Flow>)> {
    let mut records = Vec::new();
    for sink in sinks {
        let mut flows = sink.flows.into_iter();
        for txn in sink.txns {
            let own = flows.by_ref().take(txn.len as usize).collect();
            records.push((txn, own));
        }
    }
    records.sort_by_key(|(txn, _)| txn.day);
    records
}

/// Appends a transaction and its flows, numbering them from where the book is.
fn lay_out(book: &mut Book, mut txn: Txn, flows: impl Iterator<Item = Flow>) {
    let id = Id::new(book.txns.len() as u32);
    txn.first = Id::new(book.flows.len() as u32);
    for mut flow in flows {
        flow.txn = id;
        book.flows.push(flow);
    }
    book.txns.push(txn);
}

/// Every flow that touches each place, as source or as target, in flow order.
fn touching<'a>(places: usize, flows: impl Iterator<Item = (Id<Flow>, &'a Flow)>) -> Groups<Place, Id<Flow>> {
    let ends = flows.flat_map(|(id, flow)| {
        let target = (flow.to != flow.from).then_some((flow.to, id));
        std::iter::once((flow.from, id)).chain(target)
    });
    Groups::build(places, ends)
}
