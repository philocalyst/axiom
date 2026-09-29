//! What every declaring stage needs at hand.

use axiom_core::{Diagnostic, Interner, Tree};

use crate::book::System;
use crate::scope::Scopes;

pub(crate) struct Cx<'a, 's> {
    pub names: &'a mut Interner<'s>,
    pub systems: &'a Tree<System>,
    pub scopes: &'a Scopes,
    pub diags: &'a mut Vec<Diagnostic>,
}
