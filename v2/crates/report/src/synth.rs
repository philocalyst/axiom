//! Flows the journal never wrote: planned ones, and hypothetical ones.
//!
//! They borrow their transaction and source line from a real flow, so any
//! diagnostic they cause points somewhere meaningful.

use axiom_core::Day;
use axiom_model::{Amount, Flow, Infer, Mode};

/// A `Planned` flow on `day`, shaped like `template` (same ends and payee)
/// but with its own amounts. It carries no codes: those link real events.
pub fn planned(template: &Flow, day: Day, out: Amount, arrive: Amount) -> Flow {
    Flow {
        day,
        until: day,
        from: template.from,
        to: template.to,
        out,
        arrive,
        mode: Mode::Planned,
        infer: Infer::Known,
        txn: template.txn,
        payee: template.payee,
        select: template.select.clone(),
        codes: Box::default(),
        loc: template.loc,
        waive: None,
    }
}
