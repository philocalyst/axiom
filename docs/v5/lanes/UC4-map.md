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

## 6. What C4 delivered (written at its last commit)

Counted as in §0 (quality.py, test modules excluded), from `48519a6` to this branch's last code commit. The v5 head
(`d52032c`, C2 and C3) is merged in; it changed no file of `engine`. **No output of any corpus changed**: at every commit
the goldens and the mistakes regenerate byte for byte, and `diff/` (868 cases), `claims.py` (1,500 books: the dump of every
lot, part, adjustment and carry, and seven reports), `splits.py` (8,413 commands) and `tabs.py` (8,379 commands) say nothing
differs from the baseline's binary (`48519a6` while C4 was built; for the final proof `d52032c`, the merged head, built and
run from private copies, with every corpus and reference made again from it); `fuzz.py`, 200 mutated books at U21 and
1,000 at the end (seed 4): `check`'s exit, output and diagnostics the same for every one (891 of them rejected, the
same way by both), and no panic in either build.

### Lines, planned against delivered

| entry | UNIFY plan | this map (§3) | delivered | what decided the difference |
|---|---:|---:|---:|---|
| U19 one relief | −220 | −85 | **−92** | the cursor, the back and the heap stay (§2); the general path is one gather, one sort, one take |
| U20 identity | −40 | −47 | **−53** | a predicate, not a key (§3); `money: bool` became `Held` across six signatures |
| U21 the part store | −910 | −510 | **−650** | `Part.basis` stays (§3); the guards and the index go whole, and the checks that agreed by construction |
| U22 realize once | −65 | −95 | **−176** | `Slice` holds its `Parcel` (nine fields and two converters go), one builder of an asset part, `Hash` derived |
| U23 settlements | −60 | −45 | **0** | not taken: below |
| dead code (U33, engine) | (C5) | −50 | **−36** | plus `into_states`, `nearest_*`, `part_straight_line`, `calc::straight_line`, `Holdings::rebase`, `Slot::land`, counted in U20/U21 |
| follow-ons | | | **−35** | `asset_measure` for `asset_cost`/`asset_basis`; the carry queue's one key; `update_pending_carry` infallible |
| an asset's deadlines | | | **−18** | `held_parts` read once by `deadline` and `pre_disposal` (−26); `fire.rs::carry_loss` split in three, the allotment pure (+8) |
| a law's frame | | | **−25** | `Frame` reads the record (U23's `Frame::settled`), built once by `Ledger::frame` for three callers |
| **C4** | **−1,295** | **about −830** | **−1,085** | |

`engine` 12,863 → **11,778**; every other crate unchanged (model: one `#[derive(Default)]` on `FlowCodes`). The tree at
`48519a6` was 56,143 and C4 alone takes it to 55,058; with C2 and C3 merged, the v5 head's 55,770 becomes **54,685**:

| crate | `d52032c` (v5 head) | this branch |
|---|---:|---:|
| cli | 2,487 | 2,487 |
| core | 3,532 | 3,532 |
| **engine** | **12,863** | **11,778** |
| model | 18,710 | 18,710 |
| report | 6,886 | 6,886 |
| session | 501 | 501 |
| sync | 4,339 | 4,339 |
| syntax | 6,438 | 6,438 |
| systems | 14 | 14 |
| total | 55,770 | 54,685 |

The three files: `lots.rs` 1,135 → **774**, `assets.rs` + `assets_runtime.rs` 678 → **315** (one file), together
1,813 → **1,089**. Around them: `post.rs` 980 → 818, `fire.rs` 667 → 563, `eval.rs` 1,414 → 1,379, `explain.rs` 826 → 808,
`lib.rs` 411 → 399, `settle.rs` 184 → 173, `motion.rs` 204 → 194, `calc.rs` 326 → 316, `ledger.rs` 555 → 553, `claims.rs`
97 → 96, `scope.rs` 28 → 32. (U19's commit message says −90; the tree at that commit counts −92.)

Function lengths in `engine` (`fnlen.py`, `hist.py`): 794 functions, mean 13.7 lines, three over 80 (`fire.rs::carry` 152,
`dispose_sold_asset` 109, `prepare_part_carry_additions` 87) → 759 functions, mean 13.0, **none over 80**; the longest is
now `totals.rs::record` (66), which C4 did not touch. `clippy`: 267 → **253** at `48519a6`, and 58 → **44** against the merged
head (C2 took most of the rest); none new: the same fourteen went, thirteen dead-code warnings and one of too many
arguments, with what they were about.

### Tests

1,382 → **1,384** pass, 21 ignored, none fail. Moved to what replaced what they tested: the lots tests of the part store to
`Holdings::adjust` and `Holdings::carry`; `carry_updates_only_the_selected_part_and_reports_unreceived_loss` to
`carry_updates_only_the_acquisition_carried_into` (the unreceived rest of a loss is `carry_loss`'s, queued as a pending carry,
held by `wash_sale_carries_a_loss_into_a_later_replacement_lot`); `consumption_is_part_specific_…` from the asset table to the
ledger, where the cap now is; `acquisition_land_and_improvement_service_are_independent` from `part_straight_line` to
`calc.rs` against the function the law calls; `plain_identity_is_reflexive_…` to `plain_value_is_one_candidate_…`; the
engine's property test that no two held lots are alike to `Parcel::is_like`. Strengthened: the ambiguity test (selectors and
ties, so the general path's rule is held), the adjustment test (a parcel of the part in another commodity, which must not
move), the sale test (where its gain is from). New: `a_fall_in_basis_is_shared_by_basis_and_a_rise_by_quantity`,
`carries_into_one_lot_split_it_in_turn_and_the_last_takes_the_rest`, `carries_into_lots_of_two_days_split_each_in_its_place`,
`carry_quantity_preflight_is_atomic_…` (two additions, the second too big: nothing is written),
`a_loss_goes_into_no_more_shares_than_were_sold_or_bought_and_no_more_than_the_loss`. Deleted with the dead code they alone
called: `acquisition_search_uses_owner_unit_window_and_deterministic_nearest_tie` (`nearest_from`) and
`part_schedule_rejects_land_above_acquisition_cost` (`part_straight_line`'s guard; the law's straight line is tested in
`calc.rs`).

Mutants (`docs/v5/measure/session/mutate.py`): `u/relief_mutants.py`, rewritten against the ranking, 25 mutants, **all
killed** (22 at U19; at U20 two rewritten and three added for the merge; rerun on the final tree, 24 of the 25 killed and none surviving when this was written, the last, `ranges-select-the-rest`, still running); `u/parts_mutants.py` (new: adjust,
carry, the part table, realize) **17 of 17 killed**, four of them only after the tests above were added (a fall shared by
quantity, an adjustment written into another commodity, carries split front to back, a sale's gain recorded from the
flow's place). `claims.py`'s own mutants of K3c's and K3d's code: eleven were written against the functions C4 replaced
(`take_exact`, the ordered path's claim runs, `land_with_codes`, `Slice`'s fields) and no longer applied; each is moved to
the code that now does what it broke (the rank's `exact` key, `whole_claims`, `owe`, `admitted`, `restore`, the write-off's
parcel). `claims.py mutate` itself cannot run on the v5 head: its first step requires the unmutated tree to pass the
new-rule verdict, and `d52032c` fails it on some projects (p0008, p0010, p0013, p0022, p0026, …: what a flow says it
counted toward a purpose), a reference that drifted from the model before C4 and is the same for both builds. So the eleven
were run with `mutate.py` against the tests of `engine` and `report`: 10 of the 11 killed and none surviving when this was written (the last, `a payment that is returned lands the bill positive`, still running). (One more, `a loan is a bill` in
`model`, stopped applying before C4, when `lower/contracts.rs` changed; it is not C4's.)

### Measured (`check`, release; base `d52032c` and this branch, both built here)

Wall: three rounds, each running every book once on each binary, base first; the fastest of three; load average 4.1 to 4.8
on 4 shared cores (two other lanes building). Instructions: callgrind, one run each.

| book | wall s, base → C4 | user s | peak RSS MB | instructions, base → C4 |
|---|---:|---:|---:|---:|
| relief FIFO (100k lots, 50k sales) | 0.731 → **0.541** (−26%) | 0.605 → 0.427 | 127 → **105** | 3,138M → **2,431M** (−22.5%) |
| relief LIFO | 0.762 → **0.576** (−24%) | 0.678 → 0.477 | 140 → **112** | 3,143M → **2,435M** (−22.5%) |
| relief HIFO | 0.767 → **0.569** (−26%) | 0.674 → 0.431 | 129 → **112** | 3,188M → **2,494M** (−21.8%) |
| relief pro rata (2k lots, 1k sales) | 0.284 → **0.232** (−18%) | 0.238 → 0.178 | 63 → 63 | 1,377M → **1,312M** (−4.7%) |
| bench 100k | 0.496 → 0.451 | 0.478 → 0.430 | 80 → 80 | 1,899M → **1,895M** (−0.2%) |
| bench 1m | 5.121 → 4.883 | 4.757 → 4.589 | 673 → 673 | 17,522M → **17,488M** (−0.2%) |

On the sale-heavy books the gain is landing (U20: a field predicate with the transaction first, where a three-variant
identity was rebuilt for every lot of the day, 17% of the instructions) and the part index U21 removed (a map insert per
landing, 1.8%, and the memory it held). On the full books relief and the part store are a small part of the work: their
instructions move by a few tenths of a percent, so the wall times' −5 to −9% there are the shared machine, not C4.

### What the code showed the plan got wrong

1. **U21: a part's basis cannot be read from its parcels.** An acquisition's basis is what its parcels landed with, which is
   not `cost − consumed + carried` when a flow states a basis, and an improvement has no parcel. `Part.basis` stays; what
   was redundant was the sum and the guards that compared it before every write. Hence −650, not −910.
2. **U19: a ranking cannot be the only way to find what leaves.** Sorting every lot per sale is 105.8 s against 0.69 s on the
   FIFO book; the key is one (`Candidate::rank`, also the heap's order), the take is one (`share`), and FIFO, LIFO and HIFO
   on an untied, unselected holding find their first candidates by the cursor, the back and the heap.
3. **U20: identity is a predicate, not a key.** Money's identity is its basis per unit, which relief and rebasing change after
   landing; a stored key would be rewritten on each change. The predicate with the transaction first made landing cheap
   enough that columns (K3e item 1) have nothing left to win: not built (§2).
4. **U22 was larger than planned** (−176 against −65): the plan counted the `Parcel` literals and `realize`, not `Slice`'s copy of
   `Parcel`'s fields with its two converters, the hand-written `Hash`, or the three builders of an asset part.
5. **U23 is two records of two things.** `Record::settled` is the fold's state: a returned payment takes its entry back to reopen
   the claims, a fork (`Record::forked`) keeps it while it starts every record empty, the record's hash of what the rest of a
   fold depends on covers it, and `explain`'s `Frame` reads it to decide what a flow counted. `Record::settlements`
   is the history: it also holds what a flow out of a claim place relieved (never in `settled`) and keeps a returned flow's
   entry, sorted once for `report::history`. One cannot be the other, nor an index into it: a fork's `settled` outlives the
   history it was written beside. The second clone the plan counted is one line, and it is the price of the two. What U23
   could take in `engine` was `Frame`'s copy of the record's fields, taken (−25, above). The report half (`forgiven_by`,
   `Counting::forgiving`) is outside this lane, and carrying what was taken back in `Run` would move about as many lines
   into `engine` as it takes from `report`. The claim rule skipped at the call site is a speed item: `settle_claims` is
   inlined into `post_flow`, so the profile does not separate it, and it was not measured.

### Behaviour that one rule decides (no corpus reaches any of them)

- With no policy, a relief is ambiguous when the colour it cuts holds more than one candidate (§4.1).
- `exact`'s claim is every lot of its transaction and colour, wherever they sit (§4.2).
- Where one part's parcels sit in several slots, a fall or rise in basis is shared in slot order, not the order the part
  reached them; a wash sale into shares of one purchase moved between two accounts that both held the commodity is the only
  way to see it.
- The `asset-state` errors that said the parcels' sum and the part table's disagreed cannot be raised: each hook computes its
  amounts from the parcels it then writes, checks before it writes (`adjust`, `carry`), and no baseline output of any corpus
  carries one.

### Not taken, and the biggest left in `engine`

- **The wash-sale window, matched twice** (in C4's own area): a loss goes into shares bought before the sale
  (`carry_loss` → `bought_within` → `allot`) and into shares bought after it (`match_pending_carries` over the
  `PendingCarry` queue), two allotment loops over the same rule. One matching of losses against purchases within the
  window, run at the sale and at each purchase, is about −50.
- **The biggest un-taken unification in `engine`: U30, a balance over time from the run's histories** — `eval.rs`'s
  `day_count` 53, `extreme` 50, `temporal` 41, `sample_temporal` 18 and `sample_temporal_query` 19, `temporal.rs` 100, and
  `sample_temporal` called after the fold's writes at twenty sites (`post.rs` 9, `ledger.rs` 7, `fire.rs` 4): about −200 as
  UNIFY counts it, with a temporal oracle to write first.

### Left undone, and found on the way

- U23 (above). The relief and part mutants were run at the commits that changed what they test; on the final tree the relief mutants and the ported claims mutants were still finishing their last one each when this record was written (above).
- `Slot::tidy` does not recount `ties` when it drops a tied parcel of no quantity that was inserted; no path inserts one
  today, so nothing reads a wrong count, but the count can only be trusted while that holds.
- `splits.py`'s `quiet()` takes v4 syntax out of the new side's output only, so the tool reports 222 projects differing
  between a binary and itself; C4's comparisons of splits and tabs are of raw bytes (`rawdiff.py`, kept with the lane's
  scratch files), which is what the brief asks.
- `claims.py compare` says "301 should differ and do not" of any two equal builds: its references for the old rules
  name projects that differ between v4 and v5, not between two v5 builds. Not a C4 difference.
- `asset_sale_less_items`'s arms in `post.rs` read alike and may merge; not looked at further.
