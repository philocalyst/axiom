//! Headroom: what every limit had counted, and what it allowed.
//!
//! The engine records it whenever a `require` or `warn` compares two amounts,
//! so `limits`, `budget` and `why` can say how close anyone is before anything
//! breaks.

use axiom_core::{Day, Id, Map, Qty, Ratio, Set, Severity};
use axiom_engine::Headroom;
use axiom_model::{Amount, BinOp, Book, Law, Op, Period, StepKind, Subject, Window};

use crate::calendar::Periods;
use crate::lens::Lens;

/// Every limit's readings, and for each budget that nothing has reached in a
/// window that touches `from..=to`, a reading of nothing against its written
/// limit: a budget no flow touched is still a budget, wholly unspent. Before
/// the journal begins there is nothing to budget.
pub fn current(lens: Lens, from: Day, to: Day) -> Vec<Headroom> {
    let (book, run) = (lens.book, lens.run);
    let mut readings = run.headroom.clone();
    let Some(begins) = book.flows.as_slice().first().map(|first| first.day) else { return readings };
    let mut read: Set<(Id<Law>, u32, Subject, Day)> =
        readings.iter().map(|reading| (reading.law, reading.step, reading.subject, reading.days.first())).collect();
    let (months, years) = (Periods::covering(Period::Month, from, to), Periods::covering(Period::Year, from, to));
    let rules = book.rules.on_in.values().iter().chain(book.rules.on_out.values());
    for rule in rules {
        let law = &book.laws[rule.law];
        let (Subject::Place(place), Some(cap)) = (rule.subject, law.cap()) else { continue };
        let (step, limit) = (0, cap.limit);
        let warn = matches!(law.steps[0].kind, StepKind::Require { severity: Severity::Warning, .. });
        let windows = match cap.window {
            Window::Month => &months,
            Window::Year => &years,
            Window::Ever => continue,
        };
        for index in 0..windows.len() {
            let days = windows.window(index).days();
            let in_force = rule.days.overlaps(days);
            if in_force && begins <= days.last() && read.insert((rule.law, step, rule.subject, days.first())) {
                let (owner, counted) = (book.places[place].owner, Amount::new(Qty::ZERO, limit.unit));
                let (law, subject, day) = (rule.law, rule.subject, days.last().min(to));
                readings.push(Headroom { law, step, subject, owner, days, counted, limit, day, warn });
            }
        }
    }
    readings
}

/// The latest window of each limit (per law, step and subject) among `readings`.
pub fn latest<'a>(readings: impl Iterator<Item = &'a Headroom>) -> Vec<&'a Headroom> {
    let mut latest: Map<(Id<Law>, u32, Subject), &Headroom> = Map::default();
    for reading in readings {
        let slot = latest.entry((reading.law, reading.step, reading.subject)).or_insert(reading);
        if reading.days.last() > slot.days.last() {
            *slot = reading;
        }
    }
    latest.into_values().collect()
}

/// Whether the limit is a floor (`balance >= empty`): its room is what stands
/// above it, where a cap's room is what is left below it.
pub fn is_floor(book: &Book, reading: &Headroom) -> bool {
    let law = &book.laws[reading.law];
    let StepKind::Require { cond, .. } = law.steps[reading.step as usize].kind else { return false };
    matches!(law.nodes[cond.index()].op, Op::Bin(BinOp::Ge | BinOp::Gt, ..))
}

/// Room left: what the limit allows, less what is counted.
pub fn room(reading: &Headroom) -> Qty {
    reading.limit.qty - reading.counted.qty
}

pub fn is_over(reading: &Headroom) -> bool {
    room(reading).is_negative()
}

/// What share of the limit is counted.
pub fn used(reading: &Headroom) -> Option<Ratio> {
    (reading.counted.unit == reading.limit.unit)
        .then(|| Ratio::new(reading.counted.qty.0.into(), reading.limit.qty.0.into()))
        .flatten()
}
