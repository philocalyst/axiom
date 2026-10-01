//! The existing journal state the statement reconciler borrows. This adapter
//! projects typed model/engine records; it does not infer promises or claims.

use axiom_core::{Diagnostic, Id, Map, Qty};
use axiom_engine::{Run, State};
use axiom_model::{Book, Flow, Place, Role, Select};

use crate::reconcile::{Batch, Existing};
use crate::world::{Account, Unit, World};
use crate::{Due, Recognizer};

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
            let settle = if flow.mode == axiom_model::Mode::Pending {
                view.select().iter().find_map(|selector| match selector {
                    Select::Code(code) => Some(book.name(*code)),
                    _ => None,
                })
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
        }
        account.asserted.extend(
            book.asserts
                .iter()
                .filter(|assertion| assertion.place == place_id)
                .map(|assertion| assertion.day),
        );
    }

    Ok(World {
        book,
        run,
        recognizer: Recognizer::new(book),
        layout: crate::Layout::new(project_paths.iter().copied()),
        accounts,
        units,
        dues: Vec::<Due<'s>>::new(),
        claims: Map::default(),
    })
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
