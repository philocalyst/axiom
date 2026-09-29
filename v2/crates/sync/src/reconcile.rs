//! Which records are already written. A record matches a flow the account
//! already has when the amount is the same and the days are within three; the
//! nearest day is taken first, and a flow is taken at most once, so two
//! identical coffees on one day are two.

use axiom_core::{Day, Qty};

use crate::Record;

/// How far apart the bank's day and the journal's may be.
pub const WINDOW: i32 = 3;

/// A flow the book already has on an account.
#[derive(Clone, Copy, Debug)]
pub struct Existing<'a> {
    pub day: Day,
    /// Money into the account is positive.
    pub qty: Qty,
    /// The code that settles this flow, if it is pending and carries one: the
    /// record that posts it is written as `DD ^code settled`.
    pub settle: Option<&'a str>,
}

/// A flow's amount and day, ordered for lookup by amount, then day.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Slot {
    qty: Qty,
    day: Day,
    index: u32,
}

/// For each record, the index in `existing` of the flow it is. Exact days go
/// first, so that a record never takes the flow another one is on top of.
pub fn reconcile(records: &[Record], existing: &[Existing]) -> Vec<Option<usize>> {
    let mut matched = vec![None; records.len()];
    let days = records.iter().map(|record| record.day);
    let (Some(first), Some(last)) = (days.clone().min(), days.max()) else { return matched };
    // A book is far longer than a statement: only the flows near it can match.
    let near = first.add_days(-WINDOW)..=last.add_days(WINDOW);
    let mut slots: Vec<Slot> = existing
        .iter()
        .enumerate()
        .filter(|(_, flow)| near.contains(&flow.day))
        .map(|(index, flow)| Slot { qty: flow.qty, day: flow.day, index: index as u32 })
        .collect();
    slots.sort_unstable();
    let mut taken = vec![false; slots.len()];
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&at| records[at].day);
    for radius in [0, WINDOW] {
        for &at in &order {
            if matched[at].is_some() {
                continue;
            }
            let record = &records[at];
            let from = slots.partition_point(|slot| (slot.qty, slot.day) < (record.qty, record.day.add_days(-radius)));
            let nearest = slots[from..]
                .iter()
                .enumerate()
                .take_while(|(_, slot)| slot.qty == record.qty && slot.day <= record.day.add_days(radius))
                .filter(|(offset, _)| !taken[from + offset])
                .min_by_key(|(_, slot)| (slot.day.0 - record.day.0).abs())
                .map(|(offset, slot)| (from + offset, slot.index));
            if let Some((slot, index)) = nearest {
                taken[slot] = true;
                matched[at] = Some(index as usize);
            }
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use axiom_core::Loc;

    use super::*;

    fn record(day: i32, cents: i64) -> Record<'static> {
        Record { day: Day(day), qty: Qty(cents), memo: "".into(), balance: None, pending: false, at: Loc::default() }
    }

    fn flow(day: i32, cents: i64) -> Existing<'static> {
        Existing { day: Day(day), qty: Qty(cents), settle: None }
    }

    #[test]
    fn same_amount_within_three_days_is_written() {
        let flows = [flow(10, -450), flow(10, -500), flow(20, -450)];
        let records = [record(13, -450), record(14, -450), record(10, -501), record(20, 450)];
        assert_eq!(reconcile(&records, &flows), [Some(0), None, None, None]);
    }

    #[test]
    fn two_identical_coffees_are_two() {
        let one = [flow(5, -450)];
        let two = [flow(5, -450), flow(6, -450)];
        let coffees = [record(5, -450), record(5, -450)];
        assert_eq!(reconcile(&coffees, &one), [Some(0), None]);
        assert_eq!(reconcile(&coffees, &two), [Some(0), Some(1)]);
    }

    #[test]
    fn the_exact_day_goes_before_the_nearest_and_the_earlier_before_the_later() {
        // The record on the 1st would take the flow on the 2nd, which the record on the 2nd is on top of.
        let flows = [flow(2, -450)];
        assert_eq!(reconcile(&[record(1, -450), record(2, -450)], &flows), [None, Some(0)]);
        let flows = [flow(4, -450), flow(2, -450)];
        assert_eq!(reconcile(&[record(3, -450)], &flows), [Some(1)], "a tie goes to the earlier day");
    }

    #[test]
    fn the_answer_does_not_depend_on_the_order_of_the_export() {
        let flows: Vec<_> = (0..20).map(|day| flow(day, -450)).collect();
        let records: Vec<_> = (0..20).map(|day| record(day + 1, -450)).collect();
        let forward = reconcile(&records, &flows);
        let mut backward: Vec<_> = reconcile(&records.iter().rev().cloned().collect::<Vec<_>>(), &flows);
        backward.reverse();
        assert_eq!(forward, backward);
        assert_eq!(forward.iter().flatten().count(), 19, "the last record has no flow left within three days");
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore = "timings are for release builds")]
    fn a_hundred_thousand_records_against_a_million_flows() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let flows: Vec<_> = (0..1_000_000).map(|_| flow(next(3650) as i32, -(next(50_000) as i64) - 1)).collect();
        // A quarter are on the book (a few days off), the rest are new.
        let records: Vec<_> = (0..100_000)
            .map(|at| match at % 4 {
                0 => {
                    let known = &flows[next(1_000_000) as usize];
                    record(known.day.0 + next(4) as i32 - 1, known.qty.0)
                }
                _ => record(next(3650) as i32, -(next(50_000) as i64) - 1),
            })
            .collect();
        let started = Instant::now();
        let matched = reconcile(&records, &flows);
        let took = started.elapsed();
        let found = matched.iter().flatten().count();
        eprintln!("reconciled {} records against {} flows in {took:?}: {found} written", records.len(), flows.len());
        assert!(found >= 25_000 - 2_000, "{found}");
        assert!(took.as_millis() < 1000, "{took:?}");
        let mut seen = matched.iter().flatten().collect::<Vec<_>>();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), found, "a flow is taken at most once");
    }
}
