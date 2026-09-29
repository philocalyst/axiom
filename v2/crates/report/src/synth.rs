//! Flows the journal never wrote: planned ones, and hypothetical ones.
//!
//! They borrow their transaction and source line from a real flow, so any
//! diagnostic they cause points somewhere meaningful.

use axiom_core::{Day, Id};
use axiom_model::{Amount, Flow, Infer, Mode, Place, Recognition};

/// A `Planned` flow on `day`, as `template` says it: the same ends, payee and
/// terms (what it is `for`, the basis it takes, a basis end), and its
/// recognition period moved along with the day, so a plan that pays `for 2026`
/// each January means 2027 the next time. It carries no codes: those link real
/// events.
pub fn planned(template: &Flow, day: Day, out: Amount, arrive: Amount) -> Flow {
    let shift = day.0 - template.day.0;
    let recognized = Recognition {
        from: template.recognized.from.add_days(shift),
        until: template.recognized.until.add_days(shift),
    };
    Flow {
        recognized,
        payee: template.payee,
        select: template.select.clone(),
        terms: template.terms.clone(),
        ..hypothetical(template, day, template.from, template.to, out, arrive)
    }
}

/// A made-up flow on `day` from one place to another, saying nothing about
/// payee, parcels or period: what a withdrawal would be if it happened today.
/// It takes only the transaction and the source line of `borrowing`.
pub fn hypothetical(borrowing: &Flow, day: Day, from: Id<Place>, to: Id<Place>, out: Amount, arrive: Amount) -> Flow {
    Flow {
        day,
        recognized: Recognition::on(day),
        from,
        to,
        out,
        arrive,
        mode: Mode::Planned,
        infer: Infer::Known,
        txn: borrowing.txn,
        payee: None,
        select: Box::default(),
        codes: Box::default(),
        loc: borrowing.loc,
        waive: None,
        terms: None,
    }
}
