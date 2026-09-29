//! Headroom: what every limit had counted, and what it allowed.
//!
//! The engine records it whenever a `require` or `warn` compares two amounts,
//! so `limits`, `budget` and `why` can say how close anyone is before anything
//! breaks.

use axiom_core::day::days_in_month;
use axiom_core::{Day, Id, Map, Qty, Ratio, Set};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, BinOp, Book, Func, Law, Op, Period, StepKind, Subject, Value, Window};

use crate::calendar::Periods;

/// Every limit's readings, and for each budget that nothing has reached in a
/// window that touches `from..=to`, a reading of nothing against its written
/// limit: a budget no flow touched is still a budget, wholly unspent. Before
/// the journal begins there is nothing to budget.
pub fn current(book: &Book, run: &Run, from: Day, to: Day) -> Vec<Headroom> {
    let mut readings = run.headroom.clone();
    let Some(begins) = book.flows.as_slice().first().map(|first| first.day) else { return readings };
    let mut read: Set<(Id<Law>, u32, Subject, Day)> =
        readings.iter().map(|reading| (reading.law, reading.step, reading.subject, reading.from)).collect();
    let (months, years) = (Periods::covering(Period::Month, from, to), Periods::covering(Period::Year, from, to));
    let rules = book.rules.on_in.values().iter().chain(book.rules.on_out.values());
    for rule in rules {
        let (Subject::Place(place), Some((step, window, limit))) = (rule.subject, budget(&book.laws[rule.law])) else {
            continue;
        };
        let windows = match window {
            Window::Month => &months,
            Window::Year => &years,
            Window::Ever => continue,
        };
        for index in 0..windows.len() {
            let (start, until) = (windows.start(index), windows.end(index));
            let in_force = rule.from <= until && start <= rule.until;
            if in_force && begins <= until && read.insert((rule.law, step, rule.subject, start)) {
                let (owner, counted) = (book.places[place].owner, Amount::new(Qty::ZERO, limit.unit));
                let (law, subject, day) = (rule.law, rule.subject, until.min(to));
                readings.push(Headroom {
                    law,
                    step,
                    subject,
                    owner,
                    from: start,
                    until,
                    counted,
                    limit,
                    day,
                    warn: true,
                });
            }
        }
    }
    readings
}

/// A budget's step: `warn total(DIR, month|year) <= LIMIT` with the limit
/// written out, as `budget 500 USD monthly` makes it.
fn budget(law: &Law) -> Option<(u32, Window, Amount)> {
    law.steps.iter().enumerate().find_map(|(index, step)| {
        let StepKind::Require { cond, warn: true, .. } = step.kind else { return None };
        let Op::Bin(BinOp::Le | BinOp::Lt, total, limit) = law.nodes[cond.index()].op else { return None };
        match (&law.nodes[total.index()].op, &law.nodes[limit.index()].op) {
            (Op::Call(Func::Total(_, window), _), Op::Const(Value::Amount(limit))) => {
                Some((index as u32, *window, *limit))
            }
            _ => None,
        }
    })
}

/// The latest window of each limit (per law, step and subject) among `readings`.
pub fn latest<'a>(readings: impl Iterator<Item = &'a Headroom>) -> Vec<&'a Headroom> {
    let mut latest: Map<(Id<Law>, u32, Subject), &Headroom> = Map::default();
    for reading in readings {
        let slot = latest.entry((reading.law, reading.step, reading.subject)).or_insert(reading);
        if reading.until > slot.until {
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

/// The calendar month or year the reading's window is exactly, if it is one.
pub fn period(reading: &Headroom) -> Option<Period> {
    let (year, month, _) = reading.from.ymd();
    let window = |first: u32, last: u32| {
        let (from, until) = (Day::from_ymd(year, first, 1), Day::from_ymd(year, last, days_in_month(year, last)));
        (from, until) == (Some(reading.from), Some(reading.until))
    };
    if window(month, month) {
        Some(Period::Month)
    } else if window(1, 12) {
        Some(Period::Year)
    } else {
        None
    }
}

/// `2026-03`, `2026`, `on 2026-03-31`, `ever`, or the range itself.
pub fn window_words(reading: &Headroom) -> String {
    let (year, month, _) = reading.from.ymd();
    match period(reading) {
        Some(Period::Month) => format!("{year:04}-{month:02}"),
        Some(Period::Year) => format!("{year:04}"),
        None if reading.from == reading.until => format!("on {}", reading.from),
        None if (reading.from.0, reading.until.0) == (i32::MIN, i32::MAX) => "ever".to_string(),
        None => format!("{}..{}", reading.from, reading.until),
    }
}
