//! Syntax trees to a [`Book`]: names resolved, kinds linked, laws compiled and
//! type-checked, transactions elaborated into flows.
//!
//! | module      | job                                                          |
//! |-------------|--------------------------------------------------------------|
//! | `sources`   | which files are systems, the tree of systems, folder layout  |
//! | `collect`   | declarations sorted out of the items; what needs every source |
//! | `declare`   | kinds, commodities, entities and places come to exist        |
//! | `props`     | property lines, read once and applied down the kind chain    |
//! | `params`    | dated tables                                                 |
//! | `laws`      | laws compiled and typed, and the order they run in           |
//! | `rules`     | which laws watch which place, households and residences      |
//! | `flows`     | the journal elaborated, in parallel, into flows              |

pub mod book;
pub mod journal;
pub mod law;

mod collect;
mod cx;
mod declare;
mod errors;
mod flows;
mod kinds;
mod laws;
mod layout;
mod names;
mod params;
mod paths;
mod prices;
mod props;
mod resolve;
mod rules;
mod scope;
mod sources;
#[cfg(test)]
mod tests;
mod values;

pub use book::*;
pub use journal::*;
pub use law::*;

use axiom_core::{Diagnostic, Interner, Set};
use axiom_syntax::File;

/// One parsed source.
pub struct Source<'s> {
    /// Relative to the project root (`journal/2026/03.ax`), or the system path
    /// for embedded systems (`us/401k.ax`). Layout rules read it.
    pub path: &'s str,
    pub file: File<'s>,
    /// Shipped with Axiom rather than written in the project.
    pub embedded: bool,
}

/// Builds the book from every source: the project's files and the embedded
/// systems. Reports every independent problem it finds, each once, at the
/// declaration the user can change.
pub fn build<'s>(sources: &[Source<'s>]) -> (Book<'s>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let mut names = Interner::default();
    let (sites, systems_tree, systems) = sources::arrange(sources, &mut names, &mut diags);
    let surveyed = collect::survey(&sites, &mut names);
    diags.extend(surveyed.diags);
    let entries = surveyed.entries;

    let settings = declare::settings(&entries, &mut diags);
    let scopes = declare::scopes(&entries, &systems, &systems_tree, &mut diags);
    let declare = (&surveyed.units, &surveyed.paths, &settings);
    let mut world =
        declare::declare(&entries, declare.0, declare.1, declare.2, names, systems_tree, systems, scopes, &mut diags);
    let budgets = props::apply(&mut world, &entries, &mut diags);
    params::declare(&mut world, &entries, &mut diags);
    laws::declare(&mut world, &entries, budgets, &mut diags);
    let rank = laws::rank(&world.book, &mut diags);
    rules::govern(&mut world.book, &rank);
    flows::record(&mut world, &sites, &entries, &surveyed.journal, surveyed.txns, settings.layout_free, &mut diags);
    if !settings.layout_free {
        layout::check(&sites, &mut diags);
    }
    // One cause is reported once, however many declarations shared the line.
    let mut seen = Set::default();
    diags.retain(|diagnostic| seen.insert((diagnostic.code.clone(), diagnostic.anchor(), diagnostic.message.clone())));
    (world.book, diags)
}
