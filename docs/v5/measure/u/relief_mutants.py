"""Mutants of relief: a wrong rank, a wrong take, a wrong merge. The relief model test must kill every one.

usage: python3 docs/v5/measure/session/mutate.py docs/v5/measure/u/relief_mutants.py [NAME...]

`crates/engine/src/lots/tests/relief_model.rs` holds today's relief (`lots.rs`: the cursor, the heap, `exact`'s runs,
the scan) to a naive model on seeded random holdings. These are the ways a relief can be wrong that lane U's ranking
relief (U19, U20) could introduce: an order of policy inverted, a tie broken the other way, a colour taken out of turn,
the last share of a pro rata relief rounded wrongly, a part of a basis rounded wrongly, a claim of the wrong size, two
parcels merged that differ. When U19 replaces the strategies with `rank` and `take`, the same list is rewritten against
the new code (the same eleven ways of being wrong), and the model must still kill all of them.
"""
LOTS = "crates/engine/src/lots.rs"
COMMANDS = [["test", "-p", "axiom-engine", "--lib", "relief_model"]]
KNOWN = []
MUTANTS = [
    # the rank
    ("hifo-cheapest-first", LOTS,
     "let by_unit = (self.basis as i128 * other.qty as i128).cmp(&(other.basis as i128 * self.qty as i128));",
     "let by_unit = (other.basis as i128 * self.qty as i128).cmp(&(self.basis as i128 * other.qty as i128));"),
    ("hifo-ties-newest-first", LOTS, "by_unit.then(other.at.cmp(&self.at))", "by_unit.then(self.at.cmp(&other.at))"),
    ("scanned-lifo-is-fifo", LOTS, "Some(Policy::Lifo) => b.source.cmp(&a.source),", "Some(Policy::Lifo) => a.source.cmp(&b.source),"),
    ("scanned-hifo-ties-newest-first", LOTS, "basis_per_unit(b, a).then(a.source.cmp(&b.source))",
     "basis_per_unit(b, a).then(b.source.cmp(&a.source))"),
    ("exact-claims-last", LOTS, "(b.claim == exact).cmp(&(a.claim == exact))", "(a.claim == exact).cmp(&(b.claim == exact))"),
    ("permitted-before-own", LOTS, "const ALL: [Colour; 4] = [Colour::Own, Colour::Permitted, Colour::Free, Colour::Refused];",
     "const ALL: [Colour; 4] = [Colour::Permitted, Colour::Own, Colour::Free, Colour::Refused];"),
    # the take
    ("lifo-takes-plain-first", LOTS, "if plain_here && !lifo {", "if plain_here {"),
    ("exact-takes-a-bigger-claim", LOTS, "if held == req.exact {", "if held >= req.exact {"),
    ("last-share-rounded-down", LOTS, "(true, _) => self.total,", "(true, _) => self.total - Qty(i64::from(!self.total.is_zero())),"),
    ("a-part-of-a-basis-rounded-wrongly", LOTS, 'lot.basis.share(qty, lot.qty).expect("a part of a basis fits")',
     'lot.basis.share(qty, lot.qty + Qty(1)).expect("a part of a basis fits")'),
    ("prorata-is-in-order", LOTS, "allocate(group, take, policy == Some(Policy::Prorata), &mut plan);",
     "allocate(group, take, policy == Some(Policy::Lifo), &mut plan);"),
    # the merge
    ("money-merges-by-basis-not-per-unit", LOTS,
     "a == b && ap == bp && aw == bw && ab.0 as i128 * bq.0 as i128 == bb.0 as i128 * aq.0 as i128",
     "a == b && ap == bp && aw == bw && ab == bb"),
    ("a-merge-drops-the-basis", LOTS, "            lots[at].basis += parcel.basis;\n", ""),
    ("ranges-select-the-rest", LOTS, "ranges.any(|days| days.contains(lot.acquired))", "ranges.any(|days| !days.contains(lot.acquired))"),
]
