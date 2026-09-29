//! Headroom: what every limit had counted, and what it allowed.
//!
//! The engine records it whenever a `require` or `warn` compares two amounts,
//! so `limits`, `budget` and `why` can say how close anyone is before anything
//! breaks. A run that recorded none still has its budgets, which are laws of
//! one shape, and those are read from the flows.

use std::borrow::Cow;

use axiom_core::day::days_in_month;
use axiom_core::{Day, Id, Map, Qty, Ratio};
use axiom_engine::{Headroom, Run};
use axiom_model::{Amount, BinOp, Book, Dir, Func, Law, Op, Owner, Period, Place, StepKind, Subject, Value, Window};

use crate::history::Posting;
use crate::lens::{Lens, Whose};

/// Every limit's last reading, in the window that contains `at` if the run
/// recorded none.
pub fn readings<'r>(book: &Book, run: &'r Run, at: Day) -> Cow<'r, [Headroom]> {
    if run.headroom.is_empty() { Cow::Owned(implied(book, run, at)) } else { Cow::Borrowed(&run.headroom) }
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

/// The laws of `budget`'s shape, read from the flows of the window holding
/// `at`: a run with no headroom has still seen these totals.
fn implied(book: &Book, run: &Run, at: Day) -> Vec<Headroom> {
    let everyone = Whose::default();
    let lens = Lens::new(book, &everyone, at);
    let mut found = Vec::new();
    for (id, law) in book.laws.iter() {
        let Some((step, dir, window, limit)) = budget_shape(law) else { continue };
        let (from, until) = match window {
            Window::Month => (at.month_start(), at.month_end()),
            Window::Year => (at.year_start(), at.year_end()),
            Window::Ever => (Day(i32::MIN), Day(i32::MAX)),
        };
        for subject in subjects(book, law.owner) {
            let counted = crossing(lens, run, subject, dir, from, until.min(run.today));
            found.push(Headroom {
                law: id,
                step,
                subject: Subject::Place(subject),
                owner: book.places[subject].owner,
                from,
                until,
                counted: Amount::new(counted, book.base),
                limit,
                day: until.min(run.today),
                warn: true,
            });
        }
    }
    found
}

/// The places a law's total covers, when it is written under a place or a kind.
fn subjects(book: &Book, owner: Owner) -> Vec<Id<Place>> {
    match owner {
        Owner::Place(place) => vec![place],
        Owner::Kind(kind) => {
            book.places.iter().filter(|(_, place)| book.is_a(place.kind, kind)).map(|(id, _)| id).collect()
        }
        Owner::Entity(_) | Owner::System(_) | Owner::Book => Vec::new(),
    }
}

/// `warn total(dir, window) <= limit` (or `limit >= total(…)`) with a written
/// limit: the step's index, and what it bounds.
fn budget_shape(law: &Law) -> Option<(u32, Dir, Window, Amount)> {
    law.steps.iter().enumerate().find_map(|(index, step)| {
        let StepKind::Require { cond, warn: true, .. } = step.kind else { return None };
        let Op::Bin(op, left, right) = &law.nodes[cond.index()].op else { return None };
        let (total, limit) = match op {
            BinOp::Le | BinOp::Lt => (left, right),
            BinOp::Ge | BinOp::Gt => (right, left),
            _ => return None,
        };
        let (Op::Call(Func::Total(dir, window), _), Op::Const(Value::Amount(limit))) =
            (&law.nodes[total.index()].op, &law.nodes[limit.index()].op)
        else {
            return None;
        };
        Some((index as u32, *dir, *window, *limit))
    })
}

/// What crossed the boundary of `subject`'s subtree from `from` to `until`,
/// in the base currency: a flow inside the subtree is neither in nor out.
fn crossing(lens: Lens, run: &Run, subject: Id<Place>, dir: Dir, from: Day, until: Day) -> Qty {
    let book = lens.book;
    let mut total = Qty::ZERO;
    for place in book.places.subtree(subject) {
        for &id in &book.touching[place] {
            let posting = Posting::at(book, run, id);
            let flow = posting.flow;
            let (near, far, value) = match dir {
                Dir::In => (flow.to, flow.from, posting.arrive_in_base(lens)),
                Dir::Out => (flow.from, flow.to, posting.out_in_base(lens)),
            };
            let counts = near == place && !book.places.covers(subject, far) && posting.is_real_on(until);
            if counts && (from..=until).contains(&flow.day) {
                total += value.unwrap_or_default();
            }
        }
    }
    total
}
