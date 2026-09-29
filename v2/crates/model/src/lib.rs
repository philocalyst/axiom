//! Syntax trees to a [`Book`]: names resolved, kinds linked, laws compiled and
//! type-checked, transactions elaborated into flows.

pub mod book;
pub mod journal;
pub mod law;

pub use book::*;
pub use journal::*;
pub use law::*;

use axiom_core::Diagnostic;
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
/// systems. Reports every independent problem it finds.
pub fn build<'s>(sources: &[Source<'s>]) -> (Book<'s>, Vec<Diagnostic>) {
    let _ = sources;
    todo!("lane B")
}
