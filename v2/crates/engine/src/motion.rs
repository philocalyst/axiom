//! A flow as the fold sees it: oriented, with its quantities solved, on the
//! day it takes effect.
//!
//! Journal flows, applied flows and reversals (a returned deposit runs its flow
//! backwards) all become a `Motion`, so exactly one code path moves value.

use axiom_core::{Day, Id, Loc, Qty, Sym};
use axiom_model::{Amount, Entity, Flow, Place, Select, Txn, Waive};

use crate::Cause;

/// What leaves and what arrives, once every `?`, `=` and `all` is solved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Amounts {
    pub out: Qty,
    pub arrive: Qty,
}

impl Amounts {
    /// The quantities as written (zero where the source said `?`).
    pub fn written(flow: &Flow) -> Amounts {
        Amounts { out: flow.out.qty, arrive: flow.arrive.qty }
    }
}

pub(crate) struct Motion<'f> {
    pub cause: Cause,
    pub day: Day,
    pub from: Id<Place>,
    pub to: Id<Place>,
    pub out: Amount,
    pub arrive: Amount,
    pub txn: Id<Txn>,
    pub payee: Option<Id<Entity>>,
    pub select: &'f [Select],
    pub codes: &'f [Sym],
    pub waive: Option<Waive>,
    pub loc: Loc,
}

impl<'f> Motion<'f> {
    pub fn new(flow: &'f Flow, cause: Cause, day: Day, amounts: Amounts) -> Motion<'f> {
        Motion {
            cause,
            day,
            from: flow.from,
            to: flow.to,
            out: Amount::new(amounts.out, flow.out.unit),
            arrive: Amount::new(amounts.arrive, flow.arrive.unit),
            txn: flow.txn,
            payee: flow.payee,
            select: &flow.select,
            codes: &flow.codes,
            waive: flow.waive,
            loc: flow.loc,
        }
    }

    /// The same value moving back: what arrived leaves, and comes home. The
    /// original's lot selectors chose parcels at the other end and mean
    /// nothing here.
    pub fn reversed(&self) -> Motion<'f> {
        Motion { from: self.to, to: self.from, out: self.arrive, arrive: self.out, select: &[], ..*self }
    }

    pub fn is_exchange(&self) -> bool {
        self.out.unit != self.arrive.unit
    }
}
