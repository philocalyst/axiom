//! Syntax trees to a [`Book`]: names resolved, kinds linked, laws compiled and
//! type-checked, transactions elaborated into flows.

pub mod book;
pub mod journal;
pub mod law;

mod args;
mod catalog;
mod commodities;
mod cx;
mod entities;
mod errors;
mod flows;
mod kinds;
mod laws;
mod layout;
mod names;
mod params;
mod paths;
mod places;
mod prices;
mod props;
mod read;
mod resolve;
mod rules;
mod scope;
mod survey;
mod systems;
#[cfg(test)]
mod tests;
mod values;
mod world;

pub use book::*;
pub use journal::*;
pub use law::*;

use axiom_core::{Diagnostic, Interner};
use axiom_syntax::File;

use crate::catalog::Site;
use crate::read::Read;
use crate::systems::Systems;
use crate::world::World;

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
/// systems. Reports every independent problem it finds.
pub fn build<'s>(sources: &[Source<'s>]) -> (Book<'s>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let mut names = Interner::default();
    let sources = systems::arrange(sources, &mut diags);
    let (tree, systems, homes) = Systems::declare(&sources, &mut names);
    let sites: Vec<Site> = sources
        .iter()
        .zip(homes)
        .map(|(&source, home)| Site { home, source, layout: layout::Layout::of(source.path) })
        .collect();
    let Read { catalog, survey, diags: read_diags } = read::read(&sites);
    diags.extend(read_diags);
    let facts = survey.intern(&mut names);
    let scopes = systems::scopes(&catalog, &systems, &tree, &mut diags);
    let mut world = World::declare(names, facts, &catalog, tree, systems, scopes, &mut diags);
    props::apply(&mut world, &catalog, &mut diags);
    params::declare(&mut world, &catalog, &mut diags);
    laws::declare(&mut world, &catalog, &mut diags);
    rules::govern(&mut world.book);
    flows::record(&mut world, &catalog, &mut diags);
    layout::check(&catalog, &sites, &mut diags);
    (world.book, diags)
}
