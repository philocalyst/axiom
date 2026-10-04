"""Mutants of the one store of an asset's basis, of a wash sale's carry, and of realizing what leaves (U21, U22).

usage: python3 docs/v5/measure/session/mutate.py docs/v5/measure/u/parts_mutants.py [NAME...]

Since U21 a part's basis is written in the part table and in the parcels of its asset's acquisition by one hook each
(`assets.rs`), the parcels through `Holdings::adjust` and `Holdings::carry` (`lots.rs`), which check before they write; since
U22 a sale and an asset's disposal realize through one `realize` (`post.rs`). The engine's tests (the lots tests of adjust and
carry, the asset tests of the part table and of the ledger, the wash-sale tests of `source_tests.rs`) must kill each of these.
"""
LOTS = "crates/engine/src/lots.rs"
ASSETS = "crates/engine/src/assets.rs"
POST = "crates/engine/src/post.rs"
COMMANDS = [["test", "--release", "-p", "axiom-engine", "--lib"]]
KNOWN = []
MUTANTS = [
    # adjust
    ("a-fall-shared-by-quantity", LOTS, "let weight = |lot: &Parcel| if falls { lot.basis } else { lot.qty };",
     "let weight = |lot: &Parcel| lot.qty;"),
    ("a-rise-shared-by-basis", LOTS, "let weight = |lot: &Parcel| if falls { lot.basis } else { lot.qty };",
     "let weight = |lot: &Parcel| lot.basis;"),
    ("a-fall-beyond-what-is-held", LOTS, "(_, basis, _) if falls && magnitude > basis =>", "(_, basis, _) if falls && magnitude > basis + basis =>"),
    ("a-fall-that-rises", LOTS, "lot.basis = if falls { lot.basis - share } else { lot.basis + share };", "lot.basis += share;"),
    ("another-commodity-adjusted", LOTS, "                for slot in self.slots.iter_mut().filter(|slot| slot.unit == unit) {",
     "                for slot in self.slots.iter_mut() {"),
    # carry
    ("a-carry-forgets-what-came-before", LOTS, "let taken = carried.iter().filter(|c| (c.slot, c.at) == (slot, at));",
     "let taken = carried.iter().filter(|_| false);"),
    ("carries-split-front-first", LOTS, "(Reverse(c.slot), Reverse(c.at), c.order)", "(c.slot, c.at, c.order)"),
    ("a-carry-does-not-tack", LOTS, "let held_since = add.held_since.min(lot.held_since);", "let held_since = lot.held_since;"),
    ("a-matched-lot-carried-again", LOTS, "lot.acquired == add.acquired && !lot.wash_matched;", "lot.acquired == add.acquired;"),
    ("a-split-keeps-the-whole-basis", LOTS, "(lot.qty, lot.basis) = (lot.qty - matched.qty, lot.basis - basis);",
     "(lot.qty, lot.basis) = (lot.qty - matched.qty, lot.basis);"),
    # the part table beside the parcels
    ("an-improvement-adds-no-basis-to-the-parcels", ASSETS, "if part.kind == PartKind::Improvement {", "if part.kind == PartKind::Acquisition {"),
    ("consumption-uncapped", ASSETS, "let applied = requested.min(held.basis);", "let applied = requested;"),
    ("consumption-leaves-the-part", ASSETS, "        self.world.assets.part_mut(part).basis -= applied;\n", ""),
    ("a-carry-into-the-part-forgotten", ASSETS,
     "additions.iter().try_for_each(|addition| self.world.assets.add_basis(addition.part, addition.amount))",
     "additions.iter().try_for_each(|_| Ok(()))"),
    # realize
    ("an-asset-sale-realized-as-the-flow", POST, "self.realize(m, (declaration.place, declaration.unit), m.purpose);",
     "self.realize(m, (m.from, m.out.unit), m.purpose);"),
    ("an-asset-sale-at-no-price", POST, "self.scratch.relief.slices.iter_mut().for_each(|slice| slice.worth = shares.take(slice.qty));\n", ""),
    ("plain-money-realizes", POST, "let Slice { lot, origin: Origin::Lot, worth: proceeds, .. } = self.scratch.relief.slices[at] else {",
     "let Slice { lot, worth: proceeds, .. } = self.scratch.relief.slices[at] else {"),
]
