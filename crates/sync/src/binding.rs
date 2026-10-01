//! The existing journal state the statement reconciler borrows. This adapter
//! projects typed model/engine records; it does not infer promises or claims.

use axiom_core::{Diagnostic, Id, Map, Qty};
use axiom_engine::{Run, State};
use axiom_model::{Book, Commodity, Flow, Place, Role};

use crate::reconcile::{Batch, Existing};
use crate::world::{Account, World};
use crate::promise::Due;
use crate::recognize::Recognizer;
use crate::write::Layout;
use crate::Unit;

/// Build the part of the sync world that is directly backed by the canonical
/// book and run. Contract occurrences and open claims are intentionally left
/// to the engine's typed monitor API; this adapter never reconstructs them.
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

    let units: Vec<Unit<'s>> = book
        .commodities
        .iter()
        .map(|(_, unit)| Unit {
            name: book.name(unit.symbol),
            scale: unit.scale,
        })
        .collect();
    let mut accounts: Map<&'s str, Account<'s>> = Map::default();
    for (place_id, place) in book.places.iter() {
        if !matches!(place.role, Role::Account { .. }) {
            continue;
        }
        let name = book.name(place.path);
        let account = accounts.entry(name).or_default();
        let mut flow_ids = Vec::new();
        for &flow_id in book.touching[place_id].iter() {
            let flow = &book.flows[flow_id];
            let posted = run.posted[flow_id.index()];
            if matches!(posted.state, State::Void | State::Planned) {
                continue;
            }
            let (qty, unit) = account_side(flow_id, place_id, flow, posted.out, posted.arrive)?;
            if qty.is_zero() {
                continue;
            }
            let view = book.flow(flow_id);
            let settle = if posted.state == State::Pending {
                view.codes().next().map(|code| book.name(code))
            } else {
                None
            };
            account.flows.push(Existing {
                day: flow.day,
                qty,
                settle,
                unit: Some(book.name(book.commodities[unit].symbol)),
                batch: Batch::Alone,
            });
            flow_ids.push(flow_id);
        }
        assign_batches(book, place_id, &flow_ids, &mut account.flows)?;
        account.asserted.extend(
            book.asserts
                .iter()
                .filter(|assertion| assertion.place == place_id)
                .map(|assertion| assertion.day),
        );
    }

    Ok(World {
        book,
        recognizer: Recognizer::new(book),
        layout: Layout::new(project_paths.iter().copied()),
        accounts,
        units,
        dues: Vec::<Due<'s>>::new(),
        claims: Map::default(),
    })
}

/// A bank may show a coded split as its one total or as its separate flows.
/// Only group flows from the same written transaction and account commodity,
/// and only when every member carries a common typed code.
fn assign_batches<'s>(
    book: &Book<'s>,
    place: Id<Place>,
    flow_ids: &[Id<Flow>],
    existing: &mut Vec<Existing<'s>>,
) -> Result<(), Diagnostic> {
    let mut txn_start = 0;
    let mut next_batch = 0usize;
    let mut units = Vec::<Id<Commodity>>::new();
    let mut members = Vec::<usize>::new();
    let mut totals = Vec::new();
    while txn_start < flow_ids.len() {
        let txn = book.flows[flow_ids[txn_start]].txn;
        let mut txn_end = txn_start + 1;
        while txn_end < flow_ids.len() && book.flows[flow_ids[txn_end]].txn == txn {
            txn_end += 1;
        }
        units.clear();
        for first in txn_start..txn_end {
            let flow = &book.flows[flow_ids[first]];
            let unit = unit_on(flow, place);
            if units.contains(&unit) {
                continue;
            }
            units.push(unit);
            members.clear();
            for at in first..txn_end {
                if unit_on(&book.flows[flow_ids[at]], place) == unit {
                    members.push(at);
                }
            }
            if members.len() < 2 {
                continue;
            }
            let first_codes = book.flow(flow_ids[members[0]]).codes();
            let shares_code = first_codes.into_iter().any(|code| {
                members[1..]
                    .iter()
                    .all(|&at| book.flow(flow_ids[at]).codes().any(|other| other == code))
            });
            if !shares_code {
                continue;
            }
            let batch = u32::try_from(next_batch).map_err(|_| {
                Diagnostic::error("sync-batch-limit", "too many coded flow batches")
            })?;
            next_batch += 1;
            let mut total = 0i128;
            for &at in &members {
                existing[at].batch = Batch::Member(batch);
                total += existing[at].qty.0 as i128;
            }
            let total = i64::try_from(total).map_err(|_| {
                Diagnostic::error(
                    "sync-batch-overflow",
                    "the coded flow total is outside the supported quantity range",
                )
            })?;
            let day = existing[members[0]].day;
            totals.push(Existing {
                day,
                qty: Qty(total),
                settle: None,
                unit: Some(book.name(book.commodities[unit].symbol)),
                batch: Batch::Total(batch),
            });
        }
        txn_start = txn_end;
    }
    existing.extend(totals);
    Ok(())
}

fn unit_on(flow: &Flow, place: Id<Place>) -> Id<Commodity> {
    if flow.from == place {
        flow.out.unit
    } else {
        flow.arrive.unit
    }
}

fn account_side(
    flow_id: Id<axiom_model::Flow>,
    place: Id<Place>,
    flow: &Flow,
    out: Qty,
    arrive: Qty,
) -> Result<(Qty, Id<axiom_model::Commodity>), Diagnostic> {
    if flow.from == place {
        Ok((-out, flow.out.unit))
    } else if flow.to == place {
        Ok((arrive, flow.arrive.unit))
    } else {
        Err(Diagnostic::error(
            "sync-flow-index",
            format!(
                "the book says flow #{} touches an account that is not one of its ends",
                flow_id.index()
            ),
        ))
    }
}
