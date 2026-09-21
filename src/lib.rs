//! Axiom's first executable semantic slice.
//!
//! The source ledger is canonical. Parsing, resolution, recognition, journal
//! projection, and explanations are pure views over it.

pub mod engine;
pub mod exact;
pub mod model;
pub mod package;
pub mod parser;
pub mod proof;
pub mod render;

pub use engine::{Analysis, analyze};
pub use parser::parse_ledger;
