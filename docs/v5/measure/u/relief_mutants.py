"""Mutants of relief: a wrong rank, a wrong take, a wrong walk, a wrong merge. The relief tests must kill every one.

usage: python3 docs/v5/measure/session/mutate.py docs/v5/measure/u/relief_mutants.py [NAME...]

`crates/engine/src/lots/tests/relief_model.rs` holds relief (`lots.rs`) to a naive model on seeded random holdings; the
unit tests of `lots.rs` hold what the model does not look at (ambiguity, the heap after a change, merges by code). Since
U19 relief is one ranking (`Candidate::rank`, whose last key `Ranked` is also the heap's order) and one take (`share`), with
the cursor, the back and the heap finding the first candidates of FIFO, LIFO and HIFO where nothing is tied. These are the
ways it can be wrong: an order of policy inverted, a tie broken the other way, a colour taken out of turn, plain money in the
wrong place, a claim of the wrong size, the last pro rata share or a part of a basis rounded wrongly, the walk that skips or
trusts what it should not, an ambiguity said of one candidate, two parcels merged that differ.
"""
LOTS = "crates/engine/src/lots.rs"
COMMANDS = [["test", "--release", "-p", "axiom-engine", "--lib", "lots::"]]
KNOWN = []
MUTANTS = [
    # the rank
    ("hifo-cheapest-first", LOTS,
     "let by_unit = (self.basis as i128 * other.qty as i128).cmp(&(other.basis as i128 * self.qty as i128));",
     "let by_unit = (other.basis as i128 * self.qty as i128).cmp(&(self.basis as i128 * other.qty as i128));"),
    ("hifo-ties-newest-first", LOTS, "by_unit.then(other.at.cmp(&self.at))", "by_unit.then(self.at.cmp(&other.at))"),
    ("ranked-lifo-is-fifo", LOTS, "Some(Policy::Lifo) => (false, Ranked { at: -at, ..Ranked::SAME }),",
     "Some(Policy::Lifo) => (false, Ranked { at, ..Ranked::SAME }),"),
    ("ranked-hifo-ties-newest-first", LOTS, "Ranked { basis: self.basis.0, qty: self.qty.0, at }),",
     "Ranked { basis: self.basis.0, qty: self.qty.0, at: -at }),"),
    ("ranked-hifo-plain-last", LOTS, "Some(Policy::Hifo) => (plain && !req.money,", "Some(Policy::Hifo) => (false,"),
    ("exact-claims-last", LOTS, "(!plain && self.claim == req.exact,", "(!plain && self.claim != req.exact,"),
    ("exact-takes-a-bigger-claim", LOTS, "(!plain && self.claim == req.exact,", "(!plain && self.claim >= req.exact,"),
    ("exact-plain-is-a-claim", LOTS, "(!plain && self.claim == req.exact,", "(self.claim == req.exact,"),
    ("permitted-before-own", LOTS, "    Own,\n    /// Tied to an entity whose laws permit the flow.\n    Permitted,",
     "    Permitted,\n    /// Tied to an entity whose laws permit the flow.\n    Own,"),
    # the take
    ("lifo-takes-plain-first", LOTS, "let mut left = if lifo { req.need } else { self.take_plain(req.need, req, out) };",
     "let mut left = self.take_plain(req.need, req, out);"),
    ("last-share-rounded-down", LOTS, "(true, _) => self.total,", "(true, _) => self.total - Qty(i64::from(!self.total.is_zero())),"),
    ("a-part-of-a-basis-rounded-wrongly", LOTS, 'lot.basis.share(qty, lot.qty).expect("a part of a basis fits")',
     'lot.basis.share(qty, lot.qty + Qty(1)).expect("a part of a basis fits")'),
    ("prorata-is-in-order", LOTS, "    if policy == Some(Policy::Prorata) {\n        let mut shares",
     "    if policy == Some(Policy::Lifo) {\n        let mut shares"),
    ("a-claim-is-every-colour", LOTS, "*held.entry((c.txn, colour(c))).or_default() += c.qty;",
     "*held.entry((c.txn, Colour::Free)).or_default() += c.qty;\n        let _ = colour(c);"),
    # the walk
    ("ties-walked-in-order", LOTS, "        let in_order = !selection.constrains()\n            && !self.is_tied()\n",
     "        let in_order = !selection.constrains()\n"),
    ("the-heap-trusts-a-stale-top", LOTS, ".is_some_and(|lot| (lot.basis.0, lot.qty.0) == (top.basis, top.qty));",
     ".is_some_and(|lot| !lot.qty.is_zero());"),
    ("a-change-the-heap-is-not-told", LOTS, "            ranked.push(Ranked::of(at, &self.holding.lots[at]));\n", ""),
    ("the-cursor-passes-a-live-lot", LOTS, ".iter().take_while(|lot| lot.qty.is_zero()).count();",
     ".iter().take_while(|lot| lot.qty.is_zero()).count().max(usize::from(self.first + 1 < lots.len()));"),
    ("one-candidate-is-ambiguous", LOTS, "if take < total && policy.is_none() && group.len() > 1 {",
     "if take < total && policy.is_none() {"),
    # the merge
    ("money-merges-by-basis-not-per-unit", LOTS,
     "a == b && ap == bp && aw == bw && ab.0 as i128 * bq.0 as i128 == bb.0 as i128 * aq.0 as i128",
     "a == b && ap == bp && aw == bw && ab == bb"),
    ("a-merge-drops-the-basis", LOTS, "            lots[at].basis += parcel.basis;\n", ""),
    ("ranges-select-the-rest", LOTS, "ranges.any(|days| days.contains(lot.acquired))", "ranges.any(|days| !days.contains(lot.acquired))"),
]
