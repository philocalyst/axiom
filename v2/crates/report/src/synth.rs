//! Flows the journal never wrote: planned ones, and hypothetical ones.
//!
//! They borrow their transaction and source line from a real flow, so any
//! diagnostic they cause points somewhere meaningful.

use axiom_core::{Day, Days, Id};
use axiom_model::{Amount, Flow, Infer, Mode, Origin, Place};

/// A `Planned` flow on `day`, as `template` says it: the same ends, payee,
/// purpose, description and detail (what it is `for`, the basis it takes, a
/// basis end), and its recognition period and due day moved along with the
/// day, so a plan that pays `for 2026` each January means 2027 the next time,
/// and an invoice a plan sends falls due a month after each one. It carries no
/// codes: those link real events.
pub fn planned(template: &Flow, day: Day, out: Amount, arrive: Amount) -> Flow {
    let shift = day.0 - template.day.0;
    Flow {
        recognized: template.recognized.moved(shift),
        payee: template.payee,
        purpose: template.purpose,
        description: template.description,
        select: template.select.clone(),
        detail: template.detail.as_ref().map(|detail| Box::new(detail.moved(shift))),
        ..hypothetical(template, day, template.from, template.to, out, arrive)
    }
}

/// A made-up flow on `day` from one place to another, saying nothing about
/// payee, parcels or period: what a withdrawal would be if it happened today.
/// It takes only the transaction and the source line of `borrowing`.
pub fn hypothetical(borrowing: &Flow, day: Day, from: Id<Place>, to: Id<Place>, out: Amount, arrive: Amount) -> Flow {
    Flow {
        day,
        recognized: Days::on(day),
        from,
        to,
        out,
        arrive,
        mode: Mode::Planned,
        infer: Infer::Known,
        txn: borrowing.txn,
        payee: None,
        // Keep hypothetical effects in the borrowed flow's owner scope.
        owner: borrowing.owner,
        purpose: None,
        description: None,
        origin: Origin::Written,
        select: Box::default(),
        codes: Box::default(),
        loc: borrowing.loc,
        waive: None,
        detail: None,
    }
}
