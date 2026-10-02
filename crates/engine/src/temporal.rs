//! Sparse histories for the law functions that inspect a value over time.
//!
//! The plan names only the expressions used by `peak`, `low`, and `days`.
//! A ledger records their values at state boundaries; it never snapshots the
//! holdings or clones a `World` for historical reads.

use std::hash::{Hash, Hasher};

use axiom_core::{Day, Id, Map};
use axiom_model::{Entity, Fault, Func, Law, NodeId, Subject, Value};

use crate::assets::PartId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Key {
    pub law: Id<Law>,
    pub subject: Subject,
    pub owner: Id<Entity>,
    pub call: NodeId,
    /// Asset laws run once for each acquired or improved part. Keeping this in
    /// the history identity prevents their cost/basis readings from merging.
    pub part: Option<PartId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub day: Day,
    pub value: Value,
}

impl Hash for Sample {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.day.hash(state);
        hash_value(self.value, state);
    }
}

/// Keys are inserted in `Plan.temporal` order, so entries and their vectors
/// have deterministic hash order across forks and platforms.
#[derive(Clone, Default)]
pub(crate) struct History {
    entries: Vec<(Key, Vec<Sample>)>,
    index: Map<Key, usize>,
}

impl History {
    pub fn record(&mut self, key: Key, day: Day, value: Value) {
        let at = if let Some(&at) = self.index.get(&key) {
            at
        } else {
            let at = self.entries.len();
            self.entries.push((key, Vec::new()));
            self.index.insert(key, at);
            at
        };
        let samples = &mut self.entries[at].1;
        if samples.last().is_some_and(|last| last.value == value) {
            return;
        }
        samples.push(Sample { day, value });
    }

    pub fn get(&self, key: Key) -> &[Sample] {
        self.index.get(&key).map_or(&[], |&at| &self.entries[at].1)
    }
}

impl Hash for History {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entries.hash(state);
    }
}

fn hash_value<H: Hasher>(value: Value, state: &mut H) {
    std::mem::discriminant(&value).hash(state);
    match value {
        Value::Empty | Value::Flow => {}
        Value::Bool(value) => value.hash(state),
        Value::Num(value) => value.hash(state),
        Value::Amount(value) => value.hash(state),
        Value::Day(value) => value.hash(state),
        Value::Span(value) => value.hash(state),
        Value::Text(value) => value.hash(state),
        Value::Name(value) | Value::Code(value) | Value::Glob(value) => value.hash(state),
        Value::Place(value) => value.hash(state),
        Value::Entity(value) => value.hash(state),
        Value::Kind(value) => value.hash(state),
        Value::Unit(value) => value.hash(state),
        Value::Purpose(purpose, object) => {
            purpose.hash(state);
            object.hash(state);
        }
        Value::Asset(value) => value.hash(state),
        Value::Schedule(value) => value.hash(state),
        Value::Fault(fault) => hash_fault(fault, state),
    }
}

fn hash_fault<H: Hasher>(fault: Fault, state: &mut H) {
    std::mem::discriminant(&fault).hash(state);
    match fault {
        Fault::InvalidProgram | Fault::DivideByZero | Fault::Overflow => {}
        Fault::NoPrice { unit, quote } => {
            unit.hash(state);
            quote.hash(state);
        }
        Fault::UnitMismatch { found, expected } => {
            found.hash(state);
            expected.hash(state);
        }
        Fault::Unset(name) => name.hash(state),
        Fault::MissingInput(input) => input.hash(state),
        Fault::NoRow(param) => param.hash(state),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Query {
    pub key: Key,
    pub func: Func,
    pub root: NodeId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_model::Amount;

    #[test]
    fn history_keeps_same_day_extrema_and_hashes_them() {
        let day = Day::from_ymd(2026, 2, 5).unwrap();
        let unit = Id::new(0);
        let key = Key {
            law: Id::new(0),
            subject: Subject::Entity(Id::new(0)),
            owner: Id::new(0),
            call: NodeId(4),
            part: None,
        };
        let mut history = History::default();
        history.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(100), unit)));
        history.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(200), unit)));
        history.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(150), unit)));
        history.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(150), unit)));

        let samples = history.get(key);
        assert_eq!(samples.len(), 3, "same-day changes are observable, repeats are compacted");
        assert_eq!(samples[0].value, Value::Amount(Amount::new(axiom_core::Qty(100), unit)));
        assert_eq!(samples[1].value, Value::Amount(Amount::new(axiom_core::Qty(200), unit)));
        assert_eq!(samples[2].value, Value::Amount(Amount::new(axiom_core::Qty(150), unit)));

        let changed = {
            let mut other = History::default();
            other.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(100), unit)));
            other.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(200), unit)));
            other.record(key, day, Value::Amount(Amount::new(axiom_core::Qty(149), unit)));
            other
        };
        let hash = |history: &History| {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            history.hash(&mut state);
            state.finish()
        };
        assert_ne!(hash(&history), hash(&changed), "checkpoint state includes every retained extreme");
    }

    #[test]
    fn part_temporal_histories_do_not_merge_asset_costs() {
        let day = Day::from_ymd(2026, 2, 5).unwrap();
        let unit = Id::new(0);
        let origin = axiom_model::RuntimeTxn::Adjustment { place: Id::new(0), day };
        let key = Key {
            law: Id::new(0),
            subject: Subject::Asset(Id::new(0)),
            owner: Id::new(0),
            call: NodeId(4),
            part: None,
        };
        let first = PartId { origin, ordinal: 0 };
        let second = PartId { origin, ordinal: 1 };
        let mut history = History::default();
        history.record(Key { part: Some(first), ..key }, day, Value::Amount(Amount::new(axiom_core::Qty(100), unit)));
        history.record(Key { part: Some(second), ..key }, day, Value::Amount(Amount::new(axiom_core::Qty(200), unit)));

        assert_eq!(history.get(Key { part: Some(first), ..key }).len(), 1);
        assert_eq!(history.get(Key { part: Some(second), ..key }).len(), 1);
        assert_eq!(history.entries.len(), 2);
    }
}
