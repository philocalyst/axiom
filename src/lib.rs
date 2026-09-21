//! Axiom's first executable semantic slice.
//!
//! The source ledger is canonical. Parsing, resolution, recognition, journal
//! projection, and explanations are pure views over it.

pub mod engine;
pub mod evidence;
pub mod exact;
pub mod explain;
pub mod incremental;
pub mod ir;
pub mod liquidity;
pub mod logic;
pub mod model;
pub mod ontology;
pub mod package;
pub mod parser;
pub mod proof;
pub mod recognize;
pub mod reference;
pub mod render;
pub mod scenario;
pub mod semantics;
pub mod store;
pub mod surface;
pub mod time;
pub mod unify;
pub mod units;
pub mod workspace;

// `Analysis` is the stable result type returned by the workspace boundary.
// The parser and engine entry points themselves stay crate-private so an
// external caller cannot accidentally evaluate an uncommitted ledger.
pub use engine::Analysis;
