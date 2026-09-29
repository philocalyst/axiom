//! The vocabulary every other crate speaks: exact quantities, calendar days
//! and the ranges, months and schedules built on them, values that change on
//! days, interned names, typed ids, pre-ordered trees, grouped tables, source
//! locations, diagnostics, and scoped parallelism.

pub mod calendar;
pub mod day;
pub mod diag;
pub mod glob;
pub mod groups;
pub mod hash;
pub mod id;
pub mod num;
pub mod par;
pub mod sym;
pub mod timeline;
pub mod tree;

pub use calendar::{Cadence, Days, On, Period, due, spread};
pub use day::{Day, Span};
pub use diag::{Diagnostic, Disposition, FileId, Loc, Severity};
pub use groups::Groups;
pub use hash::{Map, Set};
pub use id::{Arena, Id, Run};
pub use num::{Dec, Qty, Ratio};
pub use sym::{Interner, Sym};
pub use timeline::Timeline;
pub use tree::Tree;
