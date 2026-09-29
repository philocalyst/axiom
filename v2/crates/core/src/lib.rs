//! The vocabulary every other crate speaks: exact quantities, calendar days,
//! interned names, typed ids, pre-ordered trees, grouped tables, source
//! locations, diagnostics, and scoped parallelism.

pub mod day;
pub mod diag;
pub mod glob;
pub mod groups;
pub mod hash;
pub mod id;
pub mod num;
pub mod par;
pub mod sym;
pub mod tree;

pub use day::{Day, Span};
pub use diag::{Diagnostic, FileId, Loc, Severity};
pub use groups::Groups;
pub use hash::{Map, Set};
pub use id::{Arena, Id};
pub use num::{Dec, Qty, Ratio};
pub use sym::{Interner, Sym};
pub use tree::Tree;
