//! Syntax trees to a [`Book`]: names resolved, kinds linked, laws compiled and
//! type-checked, transactions elaborated into flows.
//!
//! | module       | job                                                         |
//! |--------------|-------------------------------------------------------------|
//! | `sources`    | which files are systems, the tree of systems, folder layout |
//! | `collect`    | every item of every source, sorted into typed buckets once  |
//! | `declare`    | kinds, commodities, entities and places come to exist       |
//! | `taxonomy`   | the trees of `NAME : PARENT` names: kinds and purposes      |
//! | `slots`      | what the things of a kind have: ranges, counts and weights  |
//! | `builtin`    | the language's own slots, as typed keys of the facts        |
//! | `holders`    | the things a book says things about, numbered for the facts |
//! | `said`       | what a book says of a thing: a slot's value on a day        |
//! | `fill`       | what a line gives a slot: range, count and weights, checked |
//! | `props`      | property lines, read once and applied down the kind chain   |
//! | `params`     | dated tables                                                |
//! | `laws`       | laws compiled and typed, and the order they run in          |
//! | `rules`      | which laws watch which place, households and residences     |
//! | `lower`      | the journal and the contracts elaborated into flows         |
//! | `sync_lower` | patterns, formats, code rules and sources of `sync`         |
//! | `problem`    | the diagnostics that come in families, each worded once     |

pub mod book;
pub mod journal;
pub mod law;
pub mod sync;

pub mod builtin;
mod collect;
mod declare;
mod errors;
mod fill;
mod holders;
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
mod said;
mod scope;
mod slots;
mod sources;
mod sync_lower;
mod taxonomy;
#[cfg(test)]
mod tests;
mod values;

pub use book::*;
pub use holders::{Holder, HolderIndex};
pub use journal::*;
pub use law::*;
pub use slots::{Mult, Range, Schema, Slot, View, Weight};

use axiom_core::{Diagnostic, Interner, Set};
use axiom_syntax::File;

use crate::collect::Collected;

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
    let collected = Collected::of(&sites);
    let settings = declare::settings(&collected, &mut diags);
    let scopes = declare::scopes(&collected, &systems, &systems_tree, &mut diags);
    let survey = lower::survey(&sites);
    let said = declare::Said { sites: &sites, collected: &collected, survey: &survey };
    let systems = declare::Systems { tree: systems_tree, index: systems, scopes };
    let mut world = declare::declare(said, &settings, names, systems, &mut diags);
    slots::declare(&mut world, &collected, &mut diags);
    props::declare(&mut world, &collected, &mut diags);
    world.freeze_facts();
    params::declare(&mut world, &collected, &mut diags);
    props::system_rates(&mut world, &collected, &mut diags);
    sync_lower::declare(&mut world, &sites, &collected, &mut diags);
    laws::declare(&mut world, &sites, &mut diags);
    lower::contracts(&mut world, &collected, &mut diags);
    laws::register_native(&mut world, &mut diags);
    lower::record(&mut world, &collected, &mut diags);
    // `end` statements say more of places, once the rest is lowered.
    world.freeze_facts();
    // One cause is reported once, however many declarations shared the line.
    let mut seen = Set::default();
    diags.retain(|diagnostic| seen.insert((diagnostic.code.clone(), diagnostic.anchor(), diagnostic.message.clone())));
    (world.book, diags)
}
