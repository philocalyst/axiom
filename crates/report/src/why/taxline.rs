//! `why NAME` for a tally or obligation: every effect by that name, and the
//! flows and gains behind them.

use axiom_core::Id;
use axiom_engine::{Cause, Effect, Gain};
use axiom_model::Flow;

use super::{effects_table, flows_table};
use crate::Report;
use crate::gains;
use crate::history::Posting;
use crate::view::View;

pub fn report<'s>(view: View<'s, '_, '_>, name: &str) -> Report<'s> {
    let book = view.book();
    let effects: Vec<&Effect> = view
        .run
        .effects
        .iter()
        .filter(|effect| book.name(effect.name) == name && view.owns_entity(effect.owner))
        .collect();
    let mut flows: Vec<Id<Flow>> =
        effects.iter().filter_map(|effect| crate::history::journal_root(view.run, effect.cause)).collect();
    flows.sort_unstable();
    flows.dedup();
    let sold = view.run.gains.iter().filter(|gain| {
        crate::history::journal_root(view.run, gain.cause).is_some_and(|flow| flows.binary_search(&flow).is_ok())
    });
    let sold: Vec<&Gain> = sold.collect();

    let mut tallied = effects_table(book, view.run, &effects, "Effects");
    if effects.iter().any(|effect| effect.cause == Cause::Time) {
        tallied.note("Effects from a period ending were computed from the tallies counted during it.");
    }
    let gains = gains::section(book, &sold).headed("Gains realized");
    let behind = flows.iter().map(|&id| Posting::at(book, view.run, id));
    Report::new(format!("Why {name}")).with(tallied).with(flows_table(view, behind, "Flows")).with(gains)
}
