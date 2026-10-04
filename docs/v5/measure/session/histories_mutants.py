"""Mutants of the recording of position histories: each changes one thing the fold or the reader does, and the oracles must fail.

usage: python3 docs/v5/measure/session/mutate.py docs/v5/measure/session/histories_mutants.py [NAME...]

The tests that are to kill them are the engine's own (`histories_tests`: the history held to the fold a day at a time, on
books that settle, return, split, pad and claim) and the oracles of `crates/session/tests/histories.rs`, which hold every
example and probe book to the fold and to the replay. Each mutant is a drop of a step, a wrong merge, an off-by-one on a day,
a hook that is missed, or a recording made at the wrong moment.
"""
ENGINE = "crates/engine/src/"
REPORT = "crates/report/src/"
COMMANDS = [
    ["test", "--release", "-p", "axiom-engine", "--lib", "histories"],
    ["test", "--release", "-p", "axiom-session", "--test", "histories"],
    ["test", "--release", "-p", "axiom-report", "--lib"],
]
# Fail on the integration branch with no mutant at all (STATUS, "Waiting on you" 10): they kill nothing.
KNOWN = ["a_context_forecast_keeps_historical_and_same_day_obligations_once", "a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot"]
MUTANTS = [
    # the steps of a position
    ("same-day-step-dropped", ENGINE + "histories.rs", "Some((was, _)) if was == day => Step::Replace,", "Some((was, _)) if was == day => Step::Skip,"),
    ("neighbouring-days-merged", ENGINE + "histories.rs", "Some((was, _)) if was == day => Step::Replace,", "Some((was, _)) if was.0 / 2 == day.0 / 2 => Step::Replace,"),
    ("a-changed-balance-is-skipped", ENGINE + "histories.rs", "Some((_, before)) if before == held => Step::Skip,", "Some((_, before)) if before.0 / 2 == held.0 / 2 => Step::Skip,"),
    ("replace-adds", ENGINE + "histories.rs", "Step::Replace => self.balances[to - 1] = held,", "Step::Replace => self.balances[to - 1] += held,"),
    ("replace-writes-the-wrong-step", ENGINE + "histories.rs", "Step::Replace => self.balances[to - 1] = held,", "Step::Replace => self.balances[to.saturating_sub(2)] = held,"),
    ("counts-a-replacement-as-a-step", ENGINE + "histories.rs", "== Step::New);", "!= Step::Skip);"),
    # the reader of a position
    ("day-off-by-one", ENGINE + "histories.rs", "self.days.partition_point(|&from| from <= day)", "self.days.partition_point(|&from| from < day)"),
    ("before-the-first-step-it-holds-the-first", ENGINE + "histories.rs", "0 => Qty::ZERO,", "0 => self.balances.first().copied().unwrap_or_default(),"),
    ("positions-in-the-wrong-order", ENGINE + "histories.rs", "order.sort_unstable_by_key(|&at| keys[at]);", "order.sort_unstable_by_key(|&at| std::cmp::Reverse(keys[at]));"),
    ("places-start-one-early", ENGINE + "histories.rs", "starts[position.place.index() + 1] += 1;", "starts[position.place.index()] += 1;"),
    ("a-subtree-takes-one-more-position", ENGINE + "histories.rs", "Run::of(at(first)..at(past))", "Run::of(at(first)..at(past) + 1)"),
    ("extremes-ignore-the-window-start", ENGINE + "histories.rs", "sparse::peak_within(self.days, &self.peaks, window)", "sparse::peak_within(self.days, &self.peaks, Days::new(Day::MIN, window.last()).unwrap())"),
    # what the fold writes
    ("an-entry-is-not-noted", ENGINE + "lots.rs", "        self.touched.push(at);\n        &mut self.slots[at as usize]", "        &mut self.slots[at as usize]"),
    ("a-split-is-not-noted", ENGINE + "lots.rs", "            self.touched.push(at as u32);\n", ""),
    ("a-change-is-not-a-change", ENGINE + "lots.rs", "(std::mem::replace(&mut slot.recorded, slot.qty) != slot.qty).then_some((at, slot.qty))", "(std::mem::replace(&mut slot.recorded, slot.qty) == slot.qty).then_some((at, slot.qty))"),
    ("recorded-before-the-fact", ENGINE + "ledger.rs", "        self.take_fact(moment);\n        self.record_balances(moment.day);", "        self.record_balances(moment.day);\n        self.take_fact(moment);"),
    ("fact-recorded-a-day-early", ENGINE + "ledger.rs", "        self.record_balances(moment.day);\n    }", "        self.record_balances(moment.day.add_days(-1));\n    }"),
    ("post-records-a-day-late", ENGINE + "post.rs", "        self.record_balances(m.day);", "        self.record_balances(m.day.add_days(1));"),
    ("post-does-not-record", ENGINE + "post.rs", "        self.record_balances(m.day);\n", ""),
    ("slots-mislabelled", ENGINE + "lots.rs", "self.slots.iter().map(at).collect()", "self.slots.iter().rev().map(at).collect()"),
    # what a view reads of them
    ("balances-a-day-early", REPORT + "balances.rs", "histories.at(id, day)", "histories.at(id, day.add_days(-1))"),
    ("balances-unscaled", REPORT + "balances.rs", "lens.place_qty(at.place, histories.at(id, day))", "histories.at(id, day)"),
    ("balances-subtree-is-empty", REPORT + "balances.rs", "self.histories.beneath(place, book.places.end(place))", "self.histories.beneath(place, place)"),
    ("balances-column-offset", REPORT + "balances.rs", "&self.cells[column * self.histories.len()..]", "&self.cells[..]"),
    ("unpriced-excludes-the-first-day", REPORT + "balance.rs", "days.partition_point(|&day| day < start)", "days.partition_point(|&day| day <= start)"),
    ("unpriced-counts-the-day-it-ended", REPORT + "balance.rs", ".is_some_and(|&day| day < past)", ".is_some_and(|&day| day <= past)"),
    ("unpriced-ignores-the-sheet", REPORT + "balance.rs", "lens.owns(place) && !on_balance_sheet(book.places[place].class)", "lens.owns(place)"),
    ("unpriced-ignores-prices", REPORT + "balance.rs", "&& on_its_day.value(moved).is_none())", "&& true)"),
]
