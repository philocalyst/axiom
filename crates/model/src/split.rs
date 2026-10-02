//! One vocabulary for a split: what a header, its legs and the items under them say.
//!
//! A statement that moves value says a header (one end and an amount), legs (the other ends, each with a
//! quantity) and items (signed amounts carved out of, added to or taken off the header). A contract promises
//! the same thing for every occurrence, a journal transaction says it once, and a written occurrence says
//! what it changes of its promise. Each used to have types of its own; these are the words they share.
//! (`docs/v5/lanes/K4a-map.md` is the map of what was where.)
//!
//! # A quantity, and what a leg may be besides
//!
//! How much moves on one side is a [`Quantity`]: an amount (written, or computed when the flow lands),
//! pending, a target balance, unknown, `all`, or what the contract's own rule says. Every one of them means
//! something on a header's side and on a leg's, and the fold resolves each the same way.
//!
//! A leg may also be a [`Part`] of its header that no side can be: a share of it (`6%`), or what the others
//! leave (`...`). They need the group to resolve, so they are only the legs', and the type says so: a header's
//! side is a `Quantity`, a leg is a `Part`, and there is no state for the fold to find invalid. Two forms
//! that lowering resolves do not survive to the fold at all: `Whole` (an opening line's one unit of an asset)
//! and a statement's bare percentage (a type error).

use axiom_core::{Id, Ratio};

use crate::book::{Amount, Commodity};
use crate::law::NodeId;

/// An amount as written: its literal, or the node of an expression program that computes it when the flow
/// lands. A computed amount has no literal: whatever stands in for it (the zero a flow carries meanwhile) is
/// the flow's, not the amount's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expr {
    Literal(Amount),
    Computed(NodeId),
}

impl Expr {
    /// The node that computes it, if it is computed.
    pub fn root(self) -> Option<NodeId> {
        match self {
            Expr::Literal(_) => None,
            Expr::Computed(root) => Some(root),
        }
    }

    /// What a flow carries for it until the fold has read it: the literal, or zero in `unit`.
    pub fn stand_in(self, unit: Id<Commodity>) -> Amount {
        match self {
            Expr::Literal(amount) => amount,
            Expr::Computed(_) => Amount::zero(unit),
        }
    }
}

/// How much one side of a flow moves, as written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quantity {
    /// `84.20 USD`, or an amount computed when the flow lands.
    Amount(Expr),
    /// `(350 USD)`: written, but not yet real, like an uncashed check.
    Pending(Expr),
    /// `= 5_000 USD`: whatever makes this end's place hold that after the flow.
    Target(Expr),
    /// `? USD`: solved from the balance assertions around it.
    Unknown(Id<Commodity>),
    /// `all`, or `all VXUS`: everything the selected parcels at the source hold, of that unit if one is named.
    All(Option<Id<Commodity>>),
    /// What the contract's own rule says it is: a loan's payment.
    Derived,
}

impl Quantity {
    /// The node that computes its amount, if it is computed.
    pub fn root(self) -> Option<NodeId> {
        match self {
            Quantity::Amount(expr) | Quantity::Pending(expr) | Quantity::Target(expr) => expr.root(),
            Quantity::Unknown(_) | Quantity::All(_) | Quantity::Derived => None,
        }
    }
}

/// What a leg takes of its header's side.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// An amount of its own: whatever a header's side may be.
    Of(Quantity),
    /// `6%`: that share of the header's side as it was resolved, before any leg or item took from it.
    Share(Ratio),
    /// `...`: what the others leave of it.
    Rest,
}

impl Part {
    /// The node that computes its amount, if it is computed.
    pub fn root(self) -> Option<NodeId> {
        match self {
            Part::Of(quantity) => quantity.root(),
            Part::Share(_) | Part::Rest => None,
        }
    }
}
