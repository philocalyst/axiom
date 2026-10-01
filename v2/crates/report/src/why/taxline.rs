//! `why NAME` for a tally or obligation: every effect by that name, and the
//! flows and gains behind them.

use axiom_core::Id;
use axiom_engine::{Cause, Effect, Gain, Run};
use axiom_model::Flow;

use crate::lens::Lens;
use super::{effects_table, flows_table};
use crate::Report;
use crate::gains;

pub fn report<'s>(lens: Lens<'s, '_, '_, '_>, run: &Run, name: &str) -> Report<'s> {
    let book = lens.book();
    let effects: Vec<&Effect> = run
        .effects
        .iter()
        .filter(|effect| book.name(effect.name) == name && lens.owns_entity(effect.owner))
        .collect();
    let mut flows: Vec<Id<Flow>> = effects
        .iter()
        .filter_map(|effect| if let Cause::Flow(flow) = effect.cause { Some(flow) } else { None })
        .collect();
    flows.sort_unstable();
    flows.dedup();
    let sold =
        run.gains.iter().filter(|gain| matches!(gain.cause, Cause::Flow(flow) if flows.binary_search(&flow).is_ok()));
    let sold: Vec<&Gain> = sold.collect();

    let mut tallied = effects_table(book, &effects, "Effects");
    if effects.iter().any(|effect| effect.cause == Cause::Time) {
        tallied.note("Effects from a period ending were computed from the tallies counted during it.");
    }
    let gains = gains::section(book, &sold).headed("Gains realized");
    Report::new(format!("Why {name}")).with(tallied).with(flows_table(book, run, &flows, "Flows behind it")).with(gains)
}
