"""Mutants of the pivot and of `why`, for `climutate.py`: each changes one thing a table counts, sorts or says, and a harness that holds
the CLI to the baseline's outputs (`allcmds.sh`, `dates.py`, `whys.py`) must tell it from the original.

usage: python3 docs/v5/measure/session/climutate.py docs/v5/measure/session/report_mutants.py REFERENCE-DIR [NAME...]
"""
R = "crates/report/src/"
MUTANTS = [
    # the pivot
    ("pivot-spread-is-never-set", R + "pivot.rs", "self.spread |= counted.recognized.last() > counted.day;", "self.spread |= false;"),
    ("pivot-an-unpriced-amount-is-priced", R + "pivot.rs", "self.unpriced += usize::from(counted.amount.is_none());", "self.unpriced += usize::from(counted.amount.is_some());"),
    ("pivot-a-zero-amount-moves-its-row", R + "pivot.rs", "let moved = !amount.is_zero() && periods.overlapping", "let moved = periods.overlapping"),
    ("pivot-moved-does-not-roll-up", R + "pivot.rs", "        self.moved[target] |= self.moved[source];\n", ""),
    ("pivot-roll-up-subtracts", R + "pivot.rs", "self.cells[into * self.width + period] += value;", "self.cells[into * self.width + period] -= value;"),
    ("pivot-spreads-past-the-cutoff", R + "pivot.rs", "window.last().min(cutoff)", "window.last()"),
    # what the tables key and sort by
    ("party-is-the-other-end-of-the-flow", R + "flow.rs", "let party = flow.payee.map_or(Party::Place(other), Party::Entity);", "let party = Party::Place(other);"),
    ("party-smallest-first", R + "flow.rs", "(-magnitude, party.label(book))", "(magnitude, party.label(book))"),
    ("purpose-skips-the-object-it-is-of", R + "flow.rs", "                    add(Tally::Of(purpose, object));\n", ""),
    ("purpose-does-not-roll-up", R + "flow.rs", "                self.0.roll_up(Tally::Purpose(purpose), Tally::Purpose(parent));\n", ""),
    ("purpose-rolls-up-from-the-top", R + "flow.rs", "for purpose in (0..book.purposes.len()).rev()", "for purpose in (0..book.purposes.len())"),
    ("objects-in-the-wrong-order", R + "flow.rs", "left.cmp(&right).then_with(", "right.cmp(&left).then_with("),
    # why
    ("consequences-by-kind-before-flow", R + "why/line.rs", "told.sort_by_key(|&(at, kind, _)| (at, kind));", "told.sort_by_key(|&(at, kind, _)| (kind, at));"),
    ("consequences-forget-the-flow-they-follow", R + "why/line.rs", "told.sort_by_key(|&(at, kind, _)| (at, kind));", "told.sort_by_key(|&(_, kind, _)| kind);"),
    ("consequences-of-time-are-of-the-first-flow", R + "why/line.rs", "        Cause::Flow(id) => rank.get(&id).copied(),\n        _ => None,", "        Cause::Flow(id) => rank.get(&id).copied(),\n        _ => Some(0),"),
    ("a-quoted-description-keeps-its-quotes", R + "why.rs", ".and_then(|text| text.strip_suffix('\"')).unwrap_or(text);", ".and_then(|text| text.strip_suffix('\"')).map(|_| text).unwrap_or(text);"),
    ("an-entity-that-stands-for-its-place-is-a-place", R + "why.rs", "Some(entity) if book.entities[entity].place == Some(place) => Target::Entity(entity),", "Some(_) if false => unreachable!(),"),
    ("a-description-is-found-in-every-owners-books", R + "why/text.rs", "lens.owns(crate::flow::movement_place(lens, posting.flow))\n            && ", ""),
]
