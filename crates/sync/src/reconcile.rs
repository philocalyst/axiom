//! Which records are already written. A record matches what the account
//! already has when the amount is the same and the days are within three: a
//! whole flow, one leg of a split, the total of a batch of flows that share a
//! code, or a derived flow, whichever the book offers as an [`Existing`]. The
//! nearest day is taken first, and a flow is taken at most once, so two
//! identical coffees on one day are two.

use axiom_core::{Day, Map, Qty, Set};

use crate::Record;
use crate::recognize::Reading;

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
        Existing { day, qty, settle: None, unit: None, batch: Batch::Alone }
    }
}

/// A flow's unit, amount and day, ordered for lookup by them in that order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Slot {
    unit: usize,
    qty: Qty,
    day: Day,
    index: usize,
}

/// What a record could be a flow of: an amount in a unit.
#[derive(Clone, Copy)]
struct Candidate {
    unit: usize,
    qty: Qty,
}

/// For each record, the index in `existing` of the flow it is. Exact days go
/// first, so that a record never takes the flow another one is on top of.
#[cfg(test)]
fn reconcile<'a>(records: &'a [Record<'_>], existing: &'a [Existing<'a>], default: &'a str) -> Vec<Option<usize>> {
    reconcile_by(records.len(), |at| &records[at], existing, default)
}

/// Reconciles the record half of paired memo/recognition rows without
/// materializing a second vector of records.
pub(crate) fn reconcile_paired<'a, 't: 'a, 's>(
    rows: &'a [(Record<'t>, Reading<'s>)],
    existing: &'a [Existing<'a>],
    default: &'a str,
) -> Vec<Option<usize>> {
    reconcile_by(rows.len(), |at| &rows[at].0, existing, default)
}

fn reconcile_by<'a, 't: 'a>(
    len: usize,
    record_at: impl Fn(usize) -> &'a Record<'t> + Copy,
    existing: &'a [Existing<'a>],
    default: &'a str,
) -> Vec<Option<usize>> {
    let mut matched = vec![None; len];
    let days = (0..len).map(|at| record_at(at).day);
    let (Some(first), Some(last)) = (days.clone().min(), days.max()) else {
        return matched;
    };
    let mut units = UnitIds { default, known: Vec::new() };
    let candidates = candidates(len, record_at, &mut units);
    let mut flows = Flows::near(existing, first..=last, &candidates, &mut units);
    let mut order: Vec<usize> = (0..len).collect();
    order.sort_by_key(|&at| record_at(at).day);
    for radius in [0, WINDOW] {
        for &at in &order {
            if matched[at].is_none() {
                matched[at] = flows.take_nearest(&candidates[at], record_at(at).day, radius);
            }
        }
    }
    matched
}

/// Units as small numbers, so that comparing them costs nothing: the account's
/// own unit, which a flow or a record may also name outright, is 0, and the
/// others count up from 1 in the order they are first seen, in any case.
struct UnitIds<'a> {
    default: &'a str,
    known: Vec<&'a str>,
}

impl<'a> UnitIds<'a> {
    fn id(&mut self, name: Option<&'a str>) -> usize {
        let Some(name) = name.filter(|name| !name.eq_ignore_ascii_case(self.default)) else {
            return 0;
        };
        let at = self.known.iter().position(|known| known.eq_ignore_ascii_case(name)).unwrap_or_else(|| {
            self.known.push(name);
            self.known.len() - 1
        });
        1 + at
    }
}

/// What each record could be: its own amount, and the bank's original amount where that is another.
fn candidates<'a, 't: 'a>(
    len: usize,
    record_at: impl Fn(usize) -> &'a Record<'t>,
    units: &mut UnitIds<'a>,
) -> Vec<[Option<Candidate>; 2]> {
    (0..len)
        .map(|at| {
            let record = record_at(at);
            let primary = Candidate { unit: units.id(record.facts().currency.as_deref()), qty: record.qty };
            let original = record
                .facts()
                .original
                .as_ref()
                .map(|original| Candidate { unit: units.id(Some(original.unit.as_ref())), qty: original.qty });
            [Some(primary), original.filter(|other| (other.unit, other.qty) != (primary.unit, primary.qty))]
        })
        .collect()
}

/// The flows of the book that a record could be, ordered by unit, amount and
/// day so that finding one is a binary search, and which of them are taken.
struct Flows<'a> {
    existing: &'a [Existing<'a>],
    slots: Vec<Slot>,
    taken: Vec<bool>,
    /// The slots of each batch: what is taken along with a flow.
    batches: Map<u32, Vec<usize>>,
}

impl<'a> Flows<'a> {
    /// A book is far longer than a statement, and most of it is of other
    /// amounts: only the flows near the records, of an amount one of them has.
    fn near(
        existing: &'a [Existing<'a>],
        days: std::ops::RangeInclusive<Day>,
        candidates: &[[Option<Candidate>; 2]],
        units: &mut UnitIds<'a>,
    ) -> Flows<'a> {
        let wanted: Set<(usize, Qty)> =
            candidates.iter().flatten().flatten().map(|candidate| (candidate.unit, candidate.qty)).collect();
        let (first, last) = (days.start().add_days(-WINDOW), days.end().add_days(WINDOW));
        let mut slots: Vec<Slot> = existing
            .iter()
            .enumerate()
            .filter(|(_, flow)| (first..=last).contains(&flow.day))
            .map(|(index, flow)| Slot { unit: units.id(flow.unit), qty: flow.qty, day: flow.day, index })
            .filter(|slot| wanted.contains(&(slot.unit, slot.qty)))
            .collect();
        slots.sort_unstable();
        let mut batches: Map<u32, Vec<usize>> = Map::default();
        for (at, slot) in slots.iter().enumerate() {
            if let Batch::Member(n) | Batch::Total(n) = existing[slot.index].batch {
                batches.entry(n).or_default().push(at);
            }
        }
        Flows { existing, taken: vec![false; slots.len()], slots, batches }
    }

    /// Takes the flow nearest the day of a record, within `radius` days, that is one of its candidates and not yet taken.
    fn take_nearest(&mut self, candidates: &[Option<Candidate>; 2], day: Day, radius: i32) -> Option<usize> {
        let (slot, index) = candidates.iter().flatten().find_map(|candidate| self.nearest(candidate, day, radius))?;
        self.take(slot, index);
        Some(index)
    }

    /// The free flow of this amount nearest `day`, within `radius` days: its slot, and its index in the book.
    fn nearest(&self, candidate: &Candidate, day: Day, radius: i32) -> Option<(usize, usize)> {
        let key = |slot: &Slot| (slot.unit, slot.qty, slot.day);
        let from =
            self.slots.partition_point(|slot| key(slot) < (candidate.unit, candidate.qty, day.add_days(-radius)));
        self.slots[from..]
            .iter()
            .enumerate()
            .take_while(|(_, slot)| {
                (slot.unit, slot.qty) == (candidate.unit, candidate.qty) && slot.day <= day.add_days(radius)
            })
            .filter(|(offset, _)| !self.taken[from + offset])
            .min_by_key(|(_, slot)| (slot.day.0 - day.0).abs())
            .map(|(offset, slot)| (from + offset, slot.index))
    }

    /// A total takes every member; a member leaves the total no longer whole.
    fn take(&mut self, slot: usize, index: usize) {
        self.taken[slot] = true;
        let (Batch::Member(n) | Batch::Total(n)) = self.existing[index].batch else {
            return;
        };
        let total = matches!(self.existing[index].batch, Batch::Total(_));
        for &other in &self.batches[&n] {
            let is_total = matches!(self.existing[self.slots[other].index].batch, Batch::Total(_));
            self.taken[other] |= total || is_total;
        }
    }
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
        Existing { batch, ..flow(day, cents) }
    }

    #[test]
    fn same_amount_within_three_days_is_written() {
        let flows = [flow(10, -450), flow(10, -500), flow(20, -450)];
        let records = [record(13, -450), record(14, -450), record(10, -501), record(20, 450)];
        assert_eq!(reconcile(&records, &flows, "USD"), [Some(0), None, None, None]);
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
        assert_eq!(reconcile(&[record(1, -450), record(2, -450)], &flows, "USD"), [None, Some(0)]);
        let flows = [flow(4, -450), flow(2, -450)];
        assert_eq!(reconcile(&[record(3, -450)], &flows, "USD"), [Some(1)], "a tie goes to the earlier day");
    }

    #[test]
    fn the_answer_does_not_depend_on_the_order_of_the_export() {
        let flows: Vec<_> = (0..20).map(|day| flow(day, -450)).collect();
        let records: Vec<_> = (0..20).map(|day| record(day + 1, -450)).collect();
        let forward = reconcile(&records, &flows, "USD");
        let mut backward: Vec<_> = reconcile(&records.iter().rev().cloned().collect::<Vec<_>>(), &flows, "USD");
        backward.reverse();
        assert_eq!(forward, backward);
        assert_eq!(forward.iter().flatten().count(), 19, "the last record has no flow left within three days");
    }

    #[test]
    fn a_batch_is_matched_as_its_total_or_member_by_member_and_never_both() {
        // A payroll run of 1,000 and 500 that the bank may show as 1,500.
        let parts = |total: bool| {
            let mut all = vec![part(5, 100_000, Batch::Member(1)), part(5, 50_000, Batch::Member(1))];
            if total {
                all.push(part(5, 150_000, Batch::Total(1)));
            }
            all
        };
        let flows = parts(true);
        assert_eq!(reconcile(&[record(6, 150_000)], &flows, "USD"), [Some(2)], "the total");
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
        assert_eq!(reconcile(&[record(6, 150_000)], &parts(false), "USD"), [None], "a total nobody offered");
    }

    #[test]
    fn a_unit_is_part_of_the_amount() {
        let eur = Existing { unit: Some("EUR"), ..flow(5, -450) };
        let euros = Record {
            facts: Some(Box::new(crate::Facts { currency: Some("EUR".into()), ..Default::default() })),
            ..record(5, -450)
        };
        assert_eq!(reconcile(&[record(5, -450)], &[eur], "USD"), [None], "dollars are not euros");
        assert_eq!(reconcile(&[euros.clone()], &[flow(5, -450)], "USD"), [None]);
        assert_eq!(reconcile(&[euros], &[eur], "USD"), [Some(0)]);
    }

    #[test]
    fn unit_indices_do_not_alias_after_u16_range() {
        let units: Vec<String> = (0..=65_536).map(|at| format!("U{at}")).collect();
        let records: Vec<Record<'_>> = units
            .iter()
            .map(|unit| Record {
                facts: Some(Box::new(crate::Facts { currency: Some(unit.as_str().into()), ..Default::default() })),
                ..record(5, -450)
            })
            .collect();
        let existing = [Existing { unit: Some(units.last().unwrap()), ..flow(5, -450) }];

        let matched = reconcile(&records, &existing, "USD");
        assert!(matched[..matched.len() - 1].iter().all(Option::is_none));
        assert_eq!(matched.last(), Some(&Some(0)));
    }

    #[test]
    fn original_currency_capture_reconciles_a_foreign_unit_without_making_another_flow() {
        let record = Record {
            facts: Some(Box::new(crate::Facts {
                original: Some(crate::Original { qty: Qty(-4_500), unit: "CHF".into() }),
                ..Default::default()
            })),
            ..record(5, -4_200)
        };
        let foreign = Existing { unit: Some("CHF"), ..flow(5, -4_500) };
        assert_eq!(reconcile(&[record], &[foreign], "EUR"), [Some(0)]);
    }

    #[test]
    #[ignore = "a timing, alone: cargo test -p axiom-sync --release -- --ignored --test-threads=1"]
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
        let matched = reconcile(&records, &flows, "USD");
        let took = started.elapsed();
        let found = matched.iter().flatten().count();
        eprintln!("reconciled {} records against {} flows in {took:?}: {found} written", records.len(), flows.len());
        assert!(found >= 25_000 - 2_000, "{found}");
        assert!(took.as_millis() < 1000, "{took:?}");
        let mut seen = matched.iter().flatten().collect::<Vec<_>>();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), found, "a flow is taken at most once");
        // What a person syncs: a month of records against the same book.
        let month: Vec<_> = (0..300).map(|_| record(1_000 + next(30) as i32, -(next(50_000) as i64) - 1)).collect();
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
