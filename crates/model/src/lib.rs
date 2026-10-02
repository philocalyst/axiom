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
pub mod sync;

mod declare;
mod errors;
mod kinds;
mod laws;
mod lower;
mod names;
#[cfg(test)]
mod names_tests;
mod params;
mod paths;
mod prices;
mod problem;
mod props;
mod purposes;
mod resolve;
mod rules;
mod scope;
mod sources;
mod sync_lower;
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
    let settings = declare::settings(&sites, &mut diags);
    let scopes = declare::scopes(&sites, &systems, &systems_tree, &mut diags);
    let survey = lower::survey(&sites);
    let mut world = declare::declare(&sites, &settings, names, systems_tree, systems, scopes, &survey, &mut diags);
    props::declare(&mut world, &sites, &mut diags);
    world.finish_props();
    params::declare(&mut world, &sites, &mut diags);
    props::system_rates(&mut world, &sites, &mut diags);
    sync_lower::declare(&mut world, &sites, &mut diags);
    laws::declare(&mut world, &sites, &mut diags);
    lower::contracts(&mut world, &sites, &survey, &mut diags);
    laws::register_native(&mut world, &mut diags);
    lower::record(&mut world, &sites, &mut diags);
    // One cause is reported once, however many declarations shared the line.
    let mut seen = Set::default();
    diags.retain(|diagnostic| seen.insert((diagnostic.code.clone(), diagnostic.anchor(), diagnostic.message.clone())));
    (world.book, diags)
}
