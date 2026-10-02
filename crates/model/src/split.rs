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
//!
//! # A group, in the phase of a record's life it is in
//!
//! A [`Group`] is a header, the legs that take from it and the items under it. The same shape is read at three
//! points, and what differs there is where the flows are and what an item is:
//!
//! - [`Promised`] is a contract's template. Its flows are values in the template (they are in no arena, and the
//!   fold clones them for each occurrence), its header is always a flow, and an item is a delta over its parent
//!   flow as the fold has it, which an occurrence's own tail may already have changed: [`Says`].
//! - [`Made`] is what a record (a transaction, or an occurrence that keeps a promise) made. Its flows are in the
//!   book's flow arena, so the group names them by their offset in the record's, an item that says something its
//!   parent does not is a flow already made, and the header is either a flow of its own or only the end it names.
//!
//! A phase is three type parameters, because those are the three things that differ; nothing else is optional
//! for either. The `side` is the group's: it was stored on every leg and every item of a promise, and every item of
//! a record, and was the same on each.

use axiom_core::{Id, Loc, Ratio, Run, Sym};

use crate::book::{Amount, Commodity, Entity, Place, Text};
use crate::journal::{Flow, Purposed, Select, Waive};
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

/// Which quantity of the parent transfer a leg or item supplies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowSide {
    Out,
    Arrive,
}

impl FlowSide {
    /// The other side.
    pub fn other(self) -> FlowSide {
        match self {
            FlowSide::Out => FlowSide::Arrive,
            FlowSide::Arrive => FlowSide::Out,
        }
    }

    /// The end of the flow whose quantity this side is.
    pub fn end(self) -> crate::journal::End {
        match self {
            FlowSide::Out => crate::journal::End::From,
            FlowSide::Arrive => crate::journal::End::To,
        }
    }
}

/// How a line item bears on the flow it is under (LANGUAGE §3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign {
    /// Carved out of the header's amount.
    Carve,
    /// Comes on top of it.
    Add,
    /// Taken off it.
    Less,
}

/// A header, the legs that take from it, and the items under it. `H` is the header, `F` how a leg names its
/// flow, `I` what an item says of the flow it makes: [`Promised`] and [`Made`] are the two phases.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Group<H, F, I> {
    pub header: H,
    /// Which side of the header the legs and items take from. For a promise it is the side that its legs'
    /// destinations are on; for a record, the side the end its header names is on.
    pub side: FlowSide,
    /// The legs in source order, each with what it takes.
    pub legs: Box<[Leg<F>]>,
    /// The items in source order.
    pub items: Box<[Item<I>]>,
}

/// One leg: its flow, and what it takes of its header.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Leg<F> {
    pub flow: F,
    pub part: Part,
}

/// One item, a signed amount carved out of, added to or taken off the header. It makes a flow of its own when it
/// says something its parent does not: `I` is how.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item<I> {
    pub sign: Sign,
    pub amount: Expr,
    pub loc: Loc,
    pub flow: I,
}

/// A promise's header: a flow of its own, which the legs and items take from, and what it says moves on each
/// side (an exchange may say two units; a standing `buy`'s `arrive` is unknown). `txn` of the flow is
/// [`TEMPLATE_TXN`](crate::journal::TEMPLATE_TXN) until an occurrence is made.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header<F> {
    pub flow: F,
    pub out: Quantity,
    pub arrive: Quantity,
}

/// What an item of a promise says of the flow it makes, on top of its parent, which the fold has by then: its
/// purpose (an item with none makes no flow, and only takes its amount off), and the description, codes,
/// selectors and waiver that it adds to the parent's or replaces.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Says {
    pub purpose: Option<Purposed>,
    pub description: Option<Text>,
    pub codes: Run<Sym>,
    pub select: Run<Select>,
    pub waive: Option<Waive>,
}

/// A resolved end, retained when a split's header is only the end it names, so that its relation to the legs
/// stays explicit. `entity` records that the written end named an entity, whose place the flow uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Endpoint {
    pub place: Id<Place>,
    pub entity: Option<Id<Entity>>,
}

/// What the header of a record's group is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Heading {
    /// A flow of its own, the record's `n`th, which its items take from.
    Flow(u32),
    /// Only the end it names, which the legs share, and what it says of their total, when it says anything.
    Source { end: Endpoint, total: Option<Quantity> },
}

/// A contract's template: its flows are values, and its items are what they say over their parent.
pub type Promised = Group<Header<Flow>, Flow, Says>;

/// What a record made: its flows are the record's own, named by offset, and an item is the flow it made, if any.
pub type Made = Group<Heading, u32, Option<u32>>;
