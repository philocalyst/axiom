# UC4 map: where value rests and how it leaves (checkpoint C4 of UNIFY: U19 to U23)

Written before the first code change of C4, from the code at `48519a6` (the v5 head after C1), and checked against what the
code does. Paths are in `crates/engine/src/`. Counts are code lines as `docs/v5/measure/quality.py` counts them (non-blank,
non-comment, test modules excluded), per item. The baseline binary is `48519a6`'s, built release and kept outside git.

## 0. Where C4 starts

`engine` 12,863 of 56,143. The three files the checkpoint is about: `lots.rs` 1,135, `assets.rs` 519, `assets_runtime.rs`
159: **1,813**. Around them: the asset arms of `post.rs` (about 420 of its 980), `fire.rs::carry` (149) and the asset deadlines
(86), `settle.rs` 184, `claims.rs` 97.

## 1. What each file holds today

### `lots.rs` (1,135)

| what | items (lines) | total |
|---|---|---:|
| identity: when two parcels merge, and when a choice is ambiguous | `Identity` 12, its `PartialEq` 18, `identity()` 21, `same_codes` 7, `empty_codes` 4 | 62 |
| the value in flight | `Slice` 15, `Origin` 5, `Slice::new` 16, `::parcel` 13, `::fresh` 5 | 54 |
| what relief is asked | `Request` 12, `::of` 16, `Colour` 9, `::colour` 8, `::allows` 3, `::possible` 11, `Selection` 4 + 17, `carries` 3 | 83 |
| what relief found | `Relief` 9, `Source` 5, `Candidate` 10 + 7 | 31 |
| the slot | `Slot` 11, `Deref` 8, `new` 14, `live` 3, `is_tied` 3, `credit` 4, `land` 3 (tests only), `land_with_codes` 26, `insert` 11, `revalue` 6, `owe` 4, `restore` 6, `mirror` 8, `carries` 3, `admitted` 13, `rebase` 29, `scale` 11, `basis` 4, `sweep_ends` 14, `tidy` 5 | 186 |
| relief: six strategies | `relieve` 8, `relieve_held` 28, `relieve_in_order` 32, `take_plain` 7, `take_exact` 20, `take_run` 24, `take_priciest` 10, `take_dearest` 17, `relieve_scanning` 35, `gather` 23, `take` 34, `whole_claims` 9, `by_policy` 8, `basis_per_unit` 3, `interchangeable` 3, `allocate` 16, `Ranked` 21 | 298 |
| shares | `Shares` 22 | 22 |
| holdings | `Holdings` 7, `Hash` 8, `new` 4, `chain` 4, `get` 3, `entry` 17, `moved` 7, `positions` 4, `qty` 3, `credit` 3, `of` 3, `iter` 3, `within` 3, `relieve` 5, `rebase` 18 (dead), `scale` 7, `tidy` 5, `into_sorted` 8 | 112 |
| the second store of an asset part's basis, and wash-sale carries | `part_slots` (in `Holdings`), `index_part_slot` 10, `slot_index` 12, `part_basis` 15, `adjust_part_basis` 4 (tests only), `prepare_part_basis_adjustment` 51, `PartBasisAdjustment` 7 + 34, `CarryLotAddition` 7, `ParcelCarryChange` 9, `CarryLotBatchAdjustment` 4 + 33, `prepare_part_carry_additions` 87 | 273 |
| uses, constants | | 14 |

### `assets.rs` (519)

| what | items (lines) | total |
|---|---|---:|
| the part table's types | `PartId` 5, `EventKey` 5, `DisposalBoundary` 13, `PartKind` 5, `Part` 10, `Disposal` 6, `PendingCarry` 14, `AssetState` 5, `Assets` 5 + `Hash` 7, `AssetError` 17 | 92 |
| readers | `AssetState::{new, parts, part_count, cost, basis, total_cost, total_basis, measure, total, in_service, property_applies, held_at}` 46; `Assets::{new, from_book, asset, iter, part}` 21; `into_states` 3 (dead), `into_run_parts` 3 | 73 |
| the carry queue | `pending_carries` 3, `expire_carries_through` 3, `within_carry_window` 3, `enqueue_carry` 26, `pending_carry` 3, `update_pending_carry` 16, `shift` 11 | 65 |
| parts and disposal | `add_part` 9, `validate_part` 26, `dispose` 21 | 56 |
| the guards that keep the part table and the parcels in step | `consume` 3 (tests only), `prepare_consumption` 29, `carry` 8 (tests only), `prepare_carry` 34, `prepare_basis_additions` 30, `Consumption` 7, `ConsumptionGuard` 19 (`before` dead), `CarryUpdate` 5, `CarryGuard` 16 (`before` dead), `AssetBasisChange` 6, `AssetBasisBatchGuard` 11 | 168 |
| nothing calls it | `nearest_acquisition` 19, `nearest_from` 41 | 60 |
| uses | | 4 |

### `assets_runtime.rs` (159)

`part_straight_line` 20 (dead; it is `calc::straight_line`'s only caller, also dead), `AssetPartAddition` 16, `anchor_and_total` 5,
and the `Ledger` hooks: `validate_asset_part` 3 (a forwarder), `add_asset_part` 18, `consume_asset_part` 22, `carry_asset_basis`
24 (tests only), `dispose_asset` 9 (a forwarder), `carry_basis_to_parts` 35; uses 6.

## 2. Measured at `48519a6` (release, three runs, fastest; load average 2.7 to 4.5 on 4 cores, shared)

| book | wall s | user s | RSS MB | instructions |
|---|---:|---:|---:|---:|
| relief FIFO (100k lots, 50k sales) | 0.710 | 0.604 | 131 | 3,140M |
| relief LIFO | 0.762 | 0.648 | 142 | 3,144M |
| relief HIFO | 0.791 | 0.650 | 132 | 3,191M |
| relief pro rata (2k lots, 1k sales) | 0.242 | 0.190 | 65 | 1,374M |
| bench 100k | 0.497 | 0.420 | 82 | |

Where the instructions go (callgrind, inclusive): on the sale-heavy books **relief is 2.5% (FIFO) to 4% (HIFO)**; landing a
parcel is **25%**, of which `Identity::eq` alone is **17%** (528M): each purchase compares its freshly built identity, field by
field through a three-variant enum, with every lot of its day (about 33 on this book). `Holdings::index_part_slot` is 1.8%
(a map insert per landing of a security, for the part store). On pro rata, relief is 42% (gather 10%, `take` 17%,
`Selection::admits` 3%).

**The plain ranking cannot replace the cursor, the back walk and the heap.** The same binary with every relief sent through
the scanning path (gather every candidate, sort by colour and policy, take: what a ranking without an index is) runs the FIFO
book in **105.8 s against 0.69 s**: each of 50,000 sales sorts 100,000 lots. So the order a policy takes in is one ranking, and
for FIFO, LIFO and HIFO its first candidates are produced by the cursor, a walk from the back and the heap; everything else is
sorted.

**Parcels in columns (K3e item 1): not built.** The measurement says why: relief is a few percent of the books it is meant for,
and the cost that is there (landing) is a comparison, which a cheaper identity removes without a second layout. A column
store would also make `Holding` (the boundary type `report` reads through `Ledger::holdings()` and `Run.holdings`) a copy
assembled on demand, which moves code into `report` and adds an allocation per view. If U20 and U19 leave the books as fast
as or faster than the baseline, columns are neither neutral-and-simpler nor needed.

## 3. The entries, counted strictly

A line counts as deleted only if it will not exist afterwards; a line rewritten counts as deleted and added.

### U20: identity is a predicate over the parcel's fields, and money is a kind

*Goes:* `Identity` (12), its hand-written `PartialEq` (18), `identity()` (21), `same_codes` (7), `Candidate.identity`,
`interchangeable` (3), `Slot::land` (3, tests only). *Becomes:* `Parcel::is_like(other, held, codes)`: the same tie, part,
wash mark and code set, and then the same purchase (day, holding-period start, transaction) or, for money, the same basis
per unit, compared field by field, cheapest first. `money: bool`, a parameter of six functions and a field of `Request`, is
`Held::{Money, Lots}`, which `scope::is_money` becomes. *Why not a hashed key:* a key must be stored per lot (a column or a
field of the public `Parcel`), and money's identity is its basis per unit, which relief and rebasing change after landing, so
the key would be recomputed on every change; the field comparison with the transaction or the tie first costs a few
instructions per lot and needs no storage. *Lines:* −65 / +18, **−47**. *Behaviour:* none (§4 for ambiguity). *Proof:* the
relief model test (its merges), the lots unit tests, `claims.py`; landing's instructions on the FIFO book.

### U19: one relief, a ranking and a take

*Goes:* `relieve_in_order`, `take_plain`'s two call sites, `take_exact`, `take_run`, `take_priciest`, `take_dearest`,
`relieve_scanning`, `by_policy`, `basis_per_unit`, `allocate`, `Request::possible`, `Colour::ALL`, the per-colour walk of the
ordered path, `relieve_held`'s plain fast path (the ordered take covers it). *Becomes:*

- **one ranking**: `Candidate::rank(policy, request)`, a sort key `(Colour, lead, Reverse<Ranked>)`: the colour (the spender's
  own, then permitted, then untied, then refused), then whatever the policy puts first (for `exact` the claims of the size
  asked; for HIFO outside money, plain value), then `Ranked` (basis per unit, then position; FIFO, pro rata and none rank every
  candidate the same per unit, so position decides; LIFO negates the position). The HIFO heap orders by the same `Ranked`;
- **one take**: `share(group, take, how)`, all of each in rank order, or pro rata by weight across a colour;
- **two ways to find the first candidates in rank order**: an untied holding with no selector, relieved FIFO, LIFO, HIFO
  (outside money) or by no policy, takes from the cursor, the back, or the heap (`next`), touching only what it takes; every
  other relief (selectors, ties, `exact`, pro rata, HIFO in money) gathers and sorts by the ranking.

*Lines:* −215 / +130, **−85**. *Behaviour:* none on any book; §4 lists the two edge cases where the two paths disagreed
today and one rule is chosen. *Proof:* the relief model test (3,000 holdings) and its mutants rewritten against `rank` and
`share`; `the_ordered_paths_agree_with_scanning`; `claims.py` (every relief order of claims, bills and lots); `splits.py`; the
sale-heavy and pro rata books and `bench/` 100k and 1m not slower.

### U21: the guards go; the parcels and the part table are written together, once

*What the code says that the plan did not.* The plan's "a part's basis is read from its parcels; `Part` loses `basis`; a
part's own basis is `cost − consumed + carried`" does not hold. An acquisition's basis is what its parcels were landed with,
which is not its cost when the flow states a basis and is not an opening (`capital_cost` against `price`), so `cost −
consumed + carried` would change the acquisition's basis, what depreciation may consume of it, and `why ASSET`. And an
improvement has no parcel at all (K3c §4.0). The part table's per-part basis is real information; what is redundant is the
*sum*, which the anchor's parcels also hold, and the machinery that checks the two sums agree before every write.

*Goes:* `Holdings.part_slots`, `index_part_slot`, `slot_index`, `part_basis`, `adjust_part_basis`,
`prepare_part_basis_adjustment`, `PartBasisAdjustment`, `ParcelCarryChange`, `CarryLotBatchAdjustment`,
`prepare_part_carry_additions` (`lots.rs`, 263); `prepare_consumption`/`ConsumptionGuard`, `carry`/`prepare_carry`/`CarryGuard`/
`CarryUpdate` (tests only), `prepare_basis_additions`/`AssetBasisChange`/`AssetBasisBatchGuard`, `into_states`, `AssetState::
{cost, basis, measure}` (tests only), `nearest_acquisition`/`nearest_from` (dead) (`assets.rs`, about 220); `AssetPartAddition`,
`anchor_and_total`, the forwarders, `carry_asset_basis` (tests only), `part_straight_line` and `calc::straight_line` (dead)
(`assets_runtime.rs` and `calc.rs`, about 125); the eight `holdings.part_basis(anchor) != total => ParcelBasisMismatch` checks
and the `index_part_slot` calls (`post.rs`, about 35). *Becomes:* `Holdings::adjust(unit, part, delta)` (a part's parcels,
wherever they are, take `delta` of basis, by basis when it falls and by quantity when it rises; checked before it writes) and
`Holdings::carry(unit, addition)` (a wash sale's carry into the matched shares, split where a parcel is matched in part); in
`assets.rs`, `consume` and `add_basis`, each one checked write; the `Ledger` hooks write the parcels and then the part. The
parcels of a part are found by walking the slots of its commodity (what `scale` does), so nothing indexes them.
`assets_runtime.rs` is folded into `assets.rs`. *Lines:* −640 / +130, **−510**. *Behaviour:* none on any book: the
`asset-state` errors that said the two sums disagreed, or that a carry found fewer shares than its own scan had just counted,
cannot be raised by any path a book reaches (each caller computes its amounts from the very parcels it then writes). *Proof:*
goldens with assets (`why ASSET`, `gains`, `lots`, depreciation in `tax`), `claims.py` (its `assets` and `lots` families), the
engine's asset and wash-sale tests (moved to the new functions where they called the deleted ones), and a transition check run
once on the branch before the old store goes: after every asset operation of the corpus, the anchor's parcels hold the part
table's sum.

### U22: realizing what leaves, once; a parcel made once

*Goes:* `dispose_sold_asset`'s own construction of `Gain`, `Realized` and the `on gain` occasion; the nine-field `Parcel`
literals of `arrive`, `add_acquisition_part`, `Slot::rebase` and `owe`; `Slice`'s copy of `Parcel`'s nine fields with
`Slice::new` and `Slice::parcel`; the hand-written `Hash` of `Parcel`, which hashes exactly what a derived one does;
`Realized.acquired` (written, never read). *Becomes:* `realize(m, from, unit, purpose)` over the slices whose `worth` is set (a
sale sets it from its shares of the proceeds); `Parcel::new(qty, basis, (day, txn))` with the struct-update syntax for what
differs; `Slice { lot: Parcel, origin, worth, carried }`. *Lines:* −120 / +25, **−95**. *Behaviour:* none. *Proof:* goldens
(`gains`), `claims.py`, the engine's asset tests.

### U23: what a flow settled, once; what a write-off took back, read

*Goes:* the second `Settlement` clone per payment (`Record::settled` beside `Record::settlements`), and the report's
`forgiven_by` with its `Counting::forgiving` call, which redo what `claims.rs::take_back` did in the fold. Decided after U21,
from what the code allows: the map of settlements is the fold's state (a returned payment reopens what it settled, a fork
keeps it), and the list is its history (a reader of a day before the return sees the flow as it was), so they may stay two
records of two things with one owner each; what can go is the duplicated clone and the report's re-derivation. *Lines:*
engine about −10, report about −35. *Behaviour:* none. *Proof:* `claims.py`, goldens (`claims`, `flow`).

### Dead code the K merges left, in engine (U33's engine part, taken here)

`explain::basis_shortfall` 18, `Motion::from_view` 10, `Context::for_purpose` 4, `Holdings::rebase` 18 (and `Assets::into_states`,
`nearest_*`, `part_straight_line`, `calc::straight_line`, counted under U21). Tests that tested only dead code are listed in the
report with what replaced them. **About −50.**

### The plan against this map

| entry | UNIFY plan | revised (C1 report) | this map |
|---|---:|---:|---:|
| U19 | −220 | | −85 |
| U20 | −40 | | −47 |
| U21 | −910 | | −510 |
| U22 | −65 | | −95 |
| U23 | −60 | | −45 |
| dead code (U33, engine) | (C5) | | −50 |
| **C4** | **−1,295** | **−900** | **about −830** |

## 4. What one relief decides where today's two paths disagree

Both are cases no corpus produces; the model test, `claims.py` and the goldens will say if one does.

1. **Ambiguity.** With no policy, the ordered path calls a relief ambiguous when more than one candidate is held and it takes
   less than all of them; the scanning path, when the candidates of the colour it splits are not all the same by identity.
   They differ only for two parcels that are the same by identity and were not merged: two lots of one purchase with
   different codes, or money whose basis per unit two separate reliefs rounded into agreement. U19 takes the ordered path's
   rule for both ("more than one candidate"): a parcel that was not merged into another can be told from it, so choosing
   between them is a choice.
2. **`exact` and a claim's lines.** The ordered path finds a claim as a *run* of consecutive lots of one transaction; the
   scanning path as *all* the lots of that transaction (and colour) wherever they are. They agree whenever a transaction's
   lots are consecutive, which they are unless a returned payment puts back one line of a partly settled invoice after another
   claim of the same day. U19 takes the scanning path's (the claim is the transaction's lots: K3c §9.1).

## 5. Order of the commits

1. this map; 2. U20; 3. U19; 4. U21 (the transition check first, then the guards, then the part store); 5. U22; 6. U23;
7. the dead code; 8. the record (this map's section 6, written at the end with what was delivered).
