//! Which records are already written. A record matches what the account
//! already has when the amount is the same and the days are within three: a
//! whole flow, one leg of a split, the total of a batch of flows that share a
//! code, or a derived flow, whichever the book offers as an [`Existing`]. The
//! nearest day is taken first, and a flow is taken at most once, so two
//! identical coffees on one day are two.

use axiom_core::{Day, Map, Qty, Set};

use crate::Record;

/// How far apart the bank's day and the journal's may be.
pub const WINDOW: i32 = 3;

/// How a flow the book has belongs to a batch: flows that share a code, which
/// a bank may show as their total or one by one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Batch {
    #[default]
    Alone,
    /// One of batch `n`. Taking it leaves the total no longer whole.
    Member(u32),
    /// The total of batch `n`. Taking it takes every member.
    Total(u32),
}

/// A flow the book already has on an account: a whole flow, one leg of a split,
/// a derived flow, or the [total](Batch::Total) of a batch.
#[derive(Clone, Copy, Debug)]
pub struct Existing<'a> {
    pub day: Day,
    /// Money into the account is positive.
    pub qty: Qty,
    /// The code that settles this flow, if it is pending and carries one: the
    /// record that posts it is written as `DD ^code settled`.
    pub settle: Option<&'a str>,
    /// The unit, when it is not the one the account's feed is in.
    pub unit: Option<&'a str>,
    pub batch: Batch,
}

impl<'a> Existing<'a> {
    pub fn new(day: Day, qty: Qty) -> Existing<'a> {
        Existing {
            day,
            qty,
            settle: None,
            unit: None,
            batch: Batch::Alone,
        }
    }
}

/// A flow's unit, amount and day, ordered for lookup by them in that order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Slot {
    unit: u16,
    qty: Qty,
    day: Day,
    index: u32,
}

#[derive(Clone, Copy)]
struct Candidate {
    unit: u16,
    qty: Qty,
}

/// For each record, the index in `existing` of the flow it is. Exact days go
/// first, so that a record never takes the flow another one is on top of.
pub fn reconcile(records: &[Record], existing: &[Existing], default: &str) -> Vec<Option<usize>> {
    let mut matched = vec![None; records.len()];
    let days = records.iter().map(|record| record.day);
    let (Some(first), Some(last)) = (days.clone().min(), days.max()) else {
        return matched;
    };
    // A book is far longer than a statement: only the flows near it can match.
    let near = first.add_days(-WINDOW)..=last.add_days(WINDOW);
    // Units are compared as small numbers; the account's own, which a flow or a
    // record may also name outright, is 0.
    let mut units: Vec<String> = Vec::new();
    let mut key = |name: Option<&str>| match name.filter(|name| !name.eq_ignore_ascii_case(default))
    {
        None => 0,
        Some(name) => {
            let name = name.to_ascii_lowercase();
            let known = units.iter().position(|known| *known == name);
            1 + known.unwrap_or_else(|| {
                units.push(name);
                units.len() - 1
            }) as u16
        }
    };
    let candidates: Vec<[Option<Candidate>; 2]> = records
        .iter()
        .map(|record| {
            let primary = Candidate {
                unit: key(record.facts().currency.as_deref()),
                qty: record.qty,
            };
            let original = record.facts().original.as_ref().map(|original| Candidate {
                unit: key(Some(original.unit.as_ref())),
                qty: original.qty,
            });
            [
                Some(primary),
                original.filter(|other| (other.unit, other.qty) != (primary.unit, primary.qty)),
            ]
        })
        .collect();
    // Only a flow of an amount some record has can be one: most of a book is not.
    let wanted: Set<(u16, Qty)> = candidates
        .iter()
        .flatten()
        .flatten()
        .map(|candidate| (candidate.unit, candidate.qty))
        .collect();
    let mut slots: Vec<Slot> = existing
        .iter()
        .enumerate()
        .filter(|(_, flow)| near.contains(&flow.day))
        .map(|(index, flow)| Slot {
            unit: key(flow.unit),
            qty: flow.qty,
            day: flow.day,
            index: index as u32,
        })
        .filter(|slot| wanted.contains(&(slot.unit, slot.qty)))
        .collect();
    slots.sort_unstable();
    let batch_of = |slot: &Slot| existing[slot.index as usize].batch;
    // The slots of each batch: what is taken along with a flow.
    let mut batches: Map<u32, Vec<usize>> = Map::default();
    for (at, slot) in slots.iter().enumerate() {
        if let Batch::Member(n) | Batch::Total(n) = batch_of(slot) {
            batches.entry(n).or_default().push(at);
        }
    }
    let mut taken = vec![false; slots.len()];
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&at| records[at].day);
    for radius in [0, WINDOW] {
        for &at in &order {
            if matched[at].is_some() {
                continue;
            }
            let record = &records[at];
            let key = |slot: &Slot| (slot.unit, slot.qty, slot.day);
            let mut nearest = None;
            for candidate in candidates[at].iter().flatten() {
                let from = slots.partition_point(|slot| {
                    key(slot) < (candidate.unit, candidate.qty, record.day.add_days(-radius))
                });
                nearest = slots[from..]
                    .iter()
                    .enumerate()
                    .take_while(|(_, slot)| {
                        (slot.unit, slot.qty) == (candidate.unit, candidate.qty)
                            && slot.day <= record.day.add_days(radius)
                    })
                    .filter(|(offset, _)| !taken[from + offset])
                    .min_by_key(|(_, slot)| (slot.day.0 - record.day.0).abs())
                    .map(|(offset, slot)| (from + offset, slot.index));
                if nearest.is_some() {
                    break;
                }
            }
            let Some((slot, index)) = nearest else {
                continue;
            };
            taken[slot] = true;
            matched[at] = Some(index as usize);
            // A total takes every member; a member leaves the total no longer whole.
            let (Batch::Member(n) | Batch::Total(n)) = existing[index as usize].batch else {
                continue;
            };
            let total = matches!(existing[index as usize].batch, Batch::Total(_));
            for &other in &batches[&n] {
                taken[other] |= total || matches!(batch_of(&slots[other]), Batch::Total(_));
            }
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn record(day: i32, cents: i64) -> Record<'static> {
        Record::new(Day(day), Qty(cents), "")
    }

    fn flow(day: i32, cents: i64) -> Existing<'static> {
        Existing::new(Day(day), Qty(cents))
    }

    fn part(day: i32, cents: i64, batch: Batch) -> Existing<'static> {
        Existing {
            batch,
            ..flow(day, cents)
        }
    }

    #[test]
    fn same_amount_within_three_days_is_written() {
        let flows = [flow(10, -450), flow(10, -500), flow(20, -450)];
        let records = [
            record(13, -450),
            record(14, -450),
            record(10, -501),
            record(20, 450),
        ];
        assert_eq!(
            reconcile(&records, &flows, "USD"),
            [Some(0), None, None, None]
        );
    }

    #[test]
    fn two_identical_coffees_are_two() {
        let one = [flow(5, -450)];
        let two = [flow(5, -450), flow(6, -450)];
        let coffees = [record(5, -450), record(5, -450)];
        assert_eq!(reconcile(&coffees, &one, "USD"), [Some(0), None]);
        assert_eq!(reconcile(&coffees, &two, "USD"), [Some(0), Some(1)]);
    }

    #[test]
    fn the_exact_day_goes_before_the_nearest_and_the_earlier_before_the_later() {
        // The record on the 1st would take the flow on the 2nd, which the record on the 2nd is on top of.
        let flows = [flow(2, -450)];
        assert_eq!(
            reconcile(&[record(1, -450), record(2, -450)], &flows, "USD"),
            [None, Some(0)]
        );
        let flows = [flow(4, -450), flow(2, -450)];
        assert_eq!(
            reconcile(&[record(3, -450)], &flows, "USD"),
            [Some(1)],
            "a tie goes to the earlier day"
        );
    }

    #[test]
    fn the_answer_does_not_depend_on_the_order_of_the_export() {
        let flows: Vec<_> = (0..20).map(|day| flow(day, -450)).collect();
        let records: Vec<_> = (0..20).map(|day| record(day + 1, -450)).collect();
        let forward = reconcile(&records, &flows, "USD");
        let mut backward: Vec<_> = reconcile(
            &records.iter().rev().cloned().collect::<Vec<_>>(),
            &flows,
            "USD",
        );
        backward.reverse();
        assert_eq!(forward, backward);
        assert_eq!(
            forward.iter().flatten().count(),
            19,
            "the last record has no flow left within three days"
        );
    }

    #[test]
    fn a_batch_is_matched_as_its_total_or_member_by_member_and_never_both() {
        // A payroll run of 1,000 and 500 that the bank may show as 1,500.
        let parts = |total: bool| {
            let mut all = vec![
                part(5, 100_000, Batch::Member(1)),
                part(5, 50_000, Batch::Member(1)),
            ];
            if total {
                all.push(part(5, 150_000, Batch::Total(1)));
            }
            all
        };
        let flows = parts(true);
        assert_eq!(
            reconcile(&[record(6, 150_000)], &flows, "USD"),
            [Some(2)],
            "the total"
        );
        assert_eq!(
            reconcile(&[record(6, 150_000), record(6, 100_000)], &flows, "USD"),
            [Some(2), None],
            "members went with it"
        );
        assert_eq!(
            reconcile(&[record(6, 100_000), record(6, 50_000)], &flows, "USD"),
            [Some(0), Some(1)],
            "one by one"
        );
        assert_eq!(
            reconcile(&[record(6, 100_000), record(6, 150_000)], &flows, "USD"),
            [Some(0), None],
            "no longer whole"
        );
        assert_eq!(
            reconcile(&[record(6, 150_000)], &parts(false), "USD"),
            [None],
            "a total nobody offered"
        );
    }

    #[test]
    fn a_unit_is_part_of_the_amount() {
        let eur = Existing {
            unit: Some("EUR"),
            ..flow(5, -450)
        };
        let euros = Record {
            facts: Some(Box::new(crate::Facts {
                currency: Some("EUR".into()),
                ..Default::default()
            })),
            ..record(5, -450)
        };
        assert_eq!(
            reconcile(&[record(5, -450)], &[eur], "USD"),
            [None],
            "dollars are not euros"
        );
        assert_eq!(reconcile(&[euros.clone()], &[flow(5, -450)], "USD"), [None]);
        assert_eq!(reconcile(&[euros], &[eur], "USD"), [Some(0)]);
    }

    #[test]
    fn original_currency_capture_reconciles_a_foreign_unit_without_making_another_flow() {
        let record = Record {
            facts: Some(Box::new(crate::Facts {
                original: Some(crate::Original {
                    qty: Qty(-4_500),
                    unit: "CHF".into(),
                }),
                ..Default::default()
            })),
            ..record(5, -4_200)
        };
        let foreign = Existing {
            unit: Some("CHF"),
            ..flow(5, -4_500)
        };
        assert_eq!(reconcile(&[record], &[foreign], "EUR"), [Some(0)]);
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
    fn a_hundred_thousand_records_against_a_million_flows() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = |bound: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let flows: Vec<_> = (0..1_000_000)
            .map(|_| flow(next(3650) as i32, -(next(50_000) as i64) - 1))
            .collect();
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
        let matched = reconcile(&records, &flows, "USD");
        let took = started.elapsed();
        let found = matched.iter().flatten().count();
        eprintln!(
            "reconciled {} records against {} flows in {took:?}: {found} written",
            records.len(),
            flows.len()
        );
        assert!(found >= 25_000 - 2_000, "{found}");
        assert!(took.as_millis() < 1000, "{took:?}");
        let mut seen = matched.iter().flatten().collect::<Vec<_>>();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), found, "a flow is taken at most once");
        // What a person syncs: a month of records against the same book.
        let month: Vec<_> = (0..300)
            .map(|_| record(1_000 + next(30) as i32, -(next(50_000) as i64) - 1))
            .collect();
        let started = Instant::now();
        let matched = reconcile(&month, &flows, "USD");
        eprintln!(
            "reconciled a month of {} records against {} flows in {:?}",
            month.len(),
            flows.len(),
            started.elapsed()
        );
        assert_eq!(matched.len(), month.len());
    }
}
