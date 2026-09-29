//! A source ledger, deterministic models, and independently replayable history.

mod check;
pub mod diagnostic;
mod expr;
mod infer;
mod model;
mod project;
mod repository;
mod syntax;
mod value;

use serde::{Deserialize, Serialize};
use std::fmt;

pub use check::*;
pub use diagnostic::{Diagnostic, IssueKind, Label, Source, render_diagnostics};
pub use model::{Decision, Expr, Model, Row, TypedDocument};
pub use project::*;
pub use syntax::{Block, Document, Field, Span};
pub use value::{Date, Number, Value};

/// A versioned, domain-separated content address. Construction validates its text.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    pub fn digest(domain: &str, bytes: &[u8]) -> Self {
        let mut hash = blake3::Hasher::new_derive_key("axiom.v2.content.v1");
        hash.update(&(domain.len() as u64).to_le_bytes());
        hash.update(domain.as_bytes());
        hash.update(bytes);
        Self(hash.finalize().to_hex().to_string())
    }

    pub fn of(domain: &str, value: &impl Serialize) -> Self {
        Self::digest(domain, &canonical(value))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl std::str::FromStr for Id {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
            Ok(Self(value.to_owned()))
        } else {
            Err("expected a lowercase 64-character content address".into())
        }
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self,D::Error> {
        String::deserialize(decoder)?.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

pub(crate) fn canonical(value: &impl Serialize) -> Vec<u8> {
    // Normalize struct field order too, so typed and generic decoding agree.
    let ordered = serde_json::to_value(value).expect("semantic values are always JSON encodable");
    serde_json::to_vec(&ordered).expect("semantic values are always JSON encodable")
}
