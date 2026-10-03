//! The existing journal state the statement reconciler borrows. This adapter
//! projects typed model/engine records; it does not infer promises or claims.

use std::ops::Range;

use axiom_core::{Diagnostic, Id, Map, Qty};
use axiom_engine::{Posted, Run, State};
use axiom_model::{Book, Commodity, Flow, Place, Role};

use crate::Unit;
use crate::recognize::Recognizer;
use crate::reconcile::{Batch, Existing};
use crate::world::{Account, World};
use crate::write::Layout;

/// Why the book and the run cannot be read as one, boxed so that the result of every step stays small.
type Mismatch = Box<Diagnostic>;

/// Build the part of the sync world that is directly backed by the canonical
/// book and run. Contract occurrences and open claims are the engine monitor's
/// (`Run::promises`, `Run::open_claims`); this adapter does not reconstruct them.
pub(crate) fn world<'b, 's>(
    book: &'b Book<'s>,
    run: &'b Run,
    project_paths: &[&str],
) -> Result<World<'b, 's>, Diagnostic> {
    if run.posted.len() != book.flows.len() {
        return Err(Diagnostic::error(
            "sync-run-book-mismatch",
            "the run does not contain one posted state for every book flow",
        ));
    }
    Ok(World {
        book,
        recognizer: Recognizer::new(book),
        layout: Layout::new(project_paths.iter().copied()),
        accounts: accounts(book, run).map_err(|mismatch| *mismatch)?,
        units: book
            .commodities
            .iter()
            .map(|(_, unit)| Unit { name: book.name(unit.symbol), scale: unit.scale })
            .collect(),
        claims: Map::default(),
    })
}

/// Each account of the book, with the flows the run has posted on it and the days it is asserted.
fn accounts<'s>(book: &Book<'s>, run: &Run) -> Result<Map<&'s str, Account<'s>>, Mismatch> {
    let places = book.places.iter().filter(|(_, place)| matches!(place.role, Role::Account { .. }));
    places.map(|(id, place)| Ok((book.name(place.path), account(book, run, id)?))).collect()
}

fn account<'s>(book: &Book<'s>, run: &Run, place: Id<Place>) -> Result<Account<'s>, Mismatch> {
    let (mut ids, mut flows) = (Vec::new(), Vec::new());
    for &id in book.touching[place].iter() {
        if let Some(flow) = existing(book, run, place, id)? {
            ids.push(id);
            flows.push(flow);
        }
    }
    assign_batches(book, place, &ids, &mut flows)?;
    let asserted = book.asserts.iter().filter(|assertion| assertion.place == place).map(|assertion| assertion.day);
    Ok(Account { flows, asserted: asserted.collect() })
}

/// What the account has of a flow, if the run has posted it: a void or planned flow, and one that moves
/// nothing on the account, is not there.
fn existing<'s>(book: &Book<'s>, run: &Run, place: Id<Place>, id: Id<Flow>) -> Result<Option<Existing<'s>>, Mismatch> {
    let (flow, posted) = (&book.flows[id], run.posted[id.index()]);
    if matches!(posted.state, State::Void | State::Planned) {
        return Ok(None);
    }
    let (qty, unit) = account_side(id, place, flow, posted)?;
    if qty.is_zero() {
        return Ok(None);
    }
    let pending = posted.state == State::Pending;
    let settle = if pending { book.flow(id).codes().next().map(|code| book.name(code)) } else { None };
    let unit = Some(book.name(book.commodities[unit].symbol));
    Ok(Some(Existing { day: flow.day, qty, settle, unit, batch: Batch::Alone }))
}

/// A bank may show a coded split as its one total or as its separate flows.
/// Only group flows from the same written transaction and account commodity,
/// and only when every member carries a common typed code.
fn assign_batches<'s>(
    book: &Book<'s>,
    place: Id<Place>,
    flow_ids: &[Id<Flow>],
    existing: &mut Vec<Existing<'s>>,
) -> Result<(), Mismatch> {
    let mut totals = Vec::new();
    let mut start = 0;
    for transaction in flow_ids.chunk_by(|a, b| book.flows[*a].txn == book.flows[*b].txn) {
        let range = start..start + transaction.len();
        start = range.end;
        for (unit, members) in batches_in(book, place, flow_ids, range) {
            let batch = u32::try_from(totals.len())
                .map_err(|_| Diagnostic::error("sync-batch-limit", "too many coded flow batches"))?;
            let mut total = 0i128;
            for &at in &members {
                existing[at].batch = Batch::Member(batch);
                total += i128::from(existing[at].qty.0);
            }
            let total = i64::try_from(total).map_err(|_| {
                Diagnostic::error("sync-batch-overflow", "the coded flow total is outside the supported quantity range")
            })?;
            totals.push(Existing {
                day: existing[members[0]].day,
                qty: Qty(total),
                settle: None,
                unit: Some(book.name(book.commodities[unit].symbol)),
                batch: Batch::Total(batch),
            });
        }
    }
    existing.extend(totals);
    Ok(())
}

/// The batches of one transaction, which is the flows at `transaction` of `flow_ids`: for each commodity the
/// account moves in it, the positions of the flows of that commodity, when there are two or more and one code is
/// on all of them. A transaction of one flow has none, and costs nothing to say so.
fn batches_in(
    book: &Book<'_>,
    place: Id<Place>,
    flow_ids: &[Id<Flow>],
    transaction: Range<usize>,
) -> Vec<(Id<Commodity>, Vec<usize>)> {
    if transaction.len() < 2 {
        return Vec::new();
    }
    let unit_at = |at: usize| unit_on(&book.flows[flow_ids[at]], place);
    let (mut units, mut batches) = (Vec::new(), Vec::new());
    for first in transaction.clone() {
        let unit = unit_at(first);
        if units.contains(&unit) {
            continue;
        }
        units.push(unit);
        let members: Vec<usize> = (first..transaction.end).filter(|&at| unit_at(at) == unit).collect();
        if members.len() >= 2 && share_a_code(book, flow_ids, &members) {
            batches.push((unit, members));
        }
    }
    batches
}

/// Whether one code is on every one of the flows at `members` of `flow_ids`.
fn share_a_code(book: &Book<'_>, flow_ids: &[Id<Flow>], members: &[usize]) -> bool {
    let codes = |at: usize| book.flow(flow_ids[at]).codes();
    codes(members[0]).any(|code| members[1..].iter().all(|&at| codes(at).any(|other| other == code)))
}

fn unit_on(flow: &Flow, place: Id<Place>) -> Id<Commodity> {
    if flow.from == place { flow.out.unit } else { flow.arrive.unit }
}

/// What the flow moves on the place it touches, and in which commodity.
fn account_side(id: Id<Flow>, place: Id<Place>, flow: &Flow, posted: Posted) -> Result<(Qty, Id<Commodity>), Mismatch> {
    if flow.from == place {
        Ok((-posted.out, flow.out.unit))
    } else if flow.to == place {
        Ok((posted.arrive, flow.arrive.unit))
    } else {
        Err(Box::new(Diagnostic::error(
            "sync-flow-index",
            format!("the book says flow #{} touches an account that is not one of its ends", id.index()),
        )))
    }
}
