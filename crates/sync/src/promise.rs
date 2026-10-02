//! Promises kept. A record from a contract's party, near an occurrence that is
//! due and not yet written, is that occurrence: the journal says `01 flat`
//! and not the flow the contract already spells out.

use axiom_core::{Day, Map, Qty};

use crate::Record;
use crate::recognize::Reading;

/// An occurrence of a contract that is due and not written, as it touches one
/// account.
#[derive(Clone, Debug)]
pub struct Due<'a> {
    pub contract: &'a str,
    pub party: &'a str,
    pub account: &'a str,
    pub day: Day,
    /// What the contract moves on the account; money into it is positive.
    pub qty: Qty,
    /// How far from `day` a record may be and still keep it: half a cadence.
    pub window: i32,
}

/// For each record whose `parties` entry names one, the occurrence it keeps:
/// the nearest first, each occurrence kept once, and only by money going the
/// way the contract says.
#[cfg(test)]
fn keep(records: &[Record], parties: &[Option<&str>], dues: &[Due]) -> Vec<Option<usize>> {
    keep_by(records.len(), parties, dues, |at| (records[at].day, records[at].qty))
}

/// Keeps promises directly from the shared record/recognition buffer.
pub(crate) fn keep_paired<'t, 's>(
    rows: &[(Record<'t>, Reading<'s>)],
    parties: &[Option<&str>],
    dues: &[Due],
) -> Vec<Option<usize>> {
    keep_by(rows.len(), parties, dues, |at| (rows[at].0.day, rows[at].0.qty))
}

fn keep_by(
    len: usize,
    parties: &[Option<&str>],
    dues: &[Due],
    record_at: impl Fn(usize) -> (Day, Qty),
) -> Vec<Option<usize>> {
    let mut by_party: Map<&str, Vec<usize>> = Map::default();
    for (at, due) in dues.iter().enumerate() {
        by_party.entry(due.party).or_default().push(at);
    }
    let mut pairs = Vec::new();
    for (at, party) in parties.iter().enumerate() {
        for &due_at in party.and_then(|party| by_party.get(party)).into_iter().flatten() {
            let (day, qty) = record_at(at);
            let due = &dues[due_at];
            let apart = (day.0 - due.day.0).abs();
            if apart <= due.window && qty.is_negative() == due.qty.is_negative() {
                pairs.push((apart, at, due_at));
            }
        }
    }
    pairs.sort_unstable();
    let (mut kept, mut used) = (vec![None; len], vec![false; dues.len()]);
    for (_, at, due_at) in pairs {
        if kept[at].is_none() && !used[due_at] {
            (kept[at], used[due_at]) = (Some(due_at), true);
        }
    }
    kept
}

#[cfg(test)]
mod tests {

    use super::*;

    fn record(day: i32, cents: i64) -> Record<'static> {
        Record::new(Day(day), Qty(cents), "")
    }

    fn due(party: &'static str, day: i32, cents: i64) -> Due<'static> {
        Due { contract: "flat", party, account: "checking", day: Day(day), qty: Qty(cents), window: 15 }
    }

    #[test]
    fn a_record_within_half_a_cadence_keeps_the_nearest_occurrence() {
        let dues = [due("greystar", 1, -290_000), due("greystar", 31, -290_000)];
        let parties = [Some("greystar"), Some("greystar"), Some("greystar")];
        let records = [record(3, -290_000), record(28, -290_000), record(60, -290_000)];
        assert_eq!(keep(&records, &parties, &dues), [Some(0), Some(1), None]);
    }

    #[test]
    fn each_occurrence_is_kept_once_and_only_by_money_going_the_right_way() {
        let dues = [due("greystar", 1, -290_000)];
        let parties = [Some("greystar"), Some("greystar"), Some("greystar"), Some("lumen")];
        let records = [record(2, -290_000), record(3, -290_000), record(1, 290_000), record(1, -290_000)];
        assert_eq!(keep(&records, &parties, &dues), [Some(0), None, None, None]);
    }

    #[test]
    fn a_different_amount_still_keeps_it_and_a_stranger_does_not() {
        let dues = [due("greystar", 1, -290_000)];
        let records = [record(1, -291_860), record(1, -290_000)];
        assert_eq!(keep(&records, &[Some("greystar"), None], &dues), [Some(0), None]);
    }
}
