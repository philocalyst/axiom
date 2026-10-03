# Lane K3e: parcels in columns, and one relief that is a ranking

Read [`common.md`](common.md) first. Then [`../DESIGN.md`](../DESIGN.md) §3.1 (`tagless`) and §3.3 (parcels), [`K3c-map.md`](K3c-map.md)
**all of it** (§4 says what asset parts are and why they are not parcels; §0.3 how `exact` relieves), `crates/core/src/{tagless,facts,sparse}.rs`
(the standard for a data structure), and `crates/engine/src/{lots,assets,assets_runtime,settle,claims,post}.rs`: the code
you replace. Your worktree is `/home/user/axiom/.claude/worktrees/lane-k3e`, on branch `claude/great-wozniak-pnqn7x-v5-k3e`.
**This lane starts after K3d and K4c have merged** (K3d changes what a parcel's purpose counts; K4c changes how a flow's
quantity is read), and **runs alone with K7b at most**: it is the engine's one remaining large structure.

**Your crates:** `engine` (`lots.rs`, `assets.rs`, `assets_runtime.rs`, `settle.rs`, `claims.rs`, the parts of `post.rs` and
`ledger.rs` that land and relieve), `core` (`tagless.rs` and a new module if a parcel needs one).

## What is wrong

`lots.rs` is about 1,350 non-test lines (2,015 with its tests), `assets.rs` 871, `assets_runtime.rs` 304: **about 2,500
lines to keep what a position holds and what leaves it**. Read, do not take this brief's word for it:

- A `Slot` is one `(place, commodity)` with a `Vec<Parcel>` oldest first, a cursor over the exhausted ones, a lazily built
  HIFO heap, a sweep threshold, and **three kinds of identity** (`Plain`, `Money`, `Lot`: a tagged union written as a
  Rust `enum` with a hand-written `PartialEq`) that decide when two parcels merge.
- `relieve` is one entry point and **six strategies behind it** (`relieve_in_order`, `take_plain`, `take_exact`, `take_run`,
  `take_priciest`, `take_dearest`, `relieve_scanning`, with `Colour`, `Candidate`, `Shares`, `Selection`, `Ranked`): FIFO,
  LIFO and HIFO each take from an end or a heap, `exact` a lot of the size asked, `prorata` and selectors scan.
- A parcel is held as a `Parcel` struct (what is it? measure it) in a `Vec`, so a scan that wants the **quantity and
  basis of every parcel** (`prorata`, `admitted`, `basis`, a sale's gain) pulls the codes, dates and transaction handles
  with them.
- The identity of a parcel is recomputed at each landing (`identity(parcel, money)`) and compared structurally.
- The three asset-part guards (`PartBasisAdjustment`, `CarryLotAddition`, `CarryLotBatchAdjustment`) stand beside the
  lots: K3c's map §4 says what the smaller cut is, and **this lane decides whether to make it** (a part's basis as
  `cost + carried - consumed` from the adjustments, the guards going).

## What to build (the map decides how much)

1. **Parcels in columns.** A position's parcels as struct-of-arrays: hot columns `qty`, `basis`, an `identity key` (one
   `u64`, below), cold columns `acquired`, `held_since`, `txn`, `codes`, `tied`, `part`. Relief reads the hot columns
   only; reports read the cold ones. Measure with a microbenchmark on 1,000-parcel slots (the way lane C measured
   `postings`).
2. **Identity as a key, not a comparison.** Two parcels merge exactly when their identities are equal. Hash the
   identity's fields **once, at landing**, into the key column, and merging is an integer compare (a collision is checked
   against the cold columns, once). Delete the hand-written `PartialEq`. If a parcel's variant (plain, money, lot) is
   better as one `tagless` payload than a three-variant enum with a different `qty`/`basis` placement, extend
   `core::tagless` for it (the invariant, a `// SAFETY:` argument and a round-trip test per tag; unsafe **only in
   `core`**); if the safe encoding is as fast, use it and say so with the benchmark.
3. **One relief, a ranking.** FIFO, LIFO, HIFO, `exact` and `prorata` are each a **ranking of the parcels plus a way of
   taking** (all of one before the next; pro rata across a tie). Write the relief as `rank` (a function from `Policy` to a
   sort key over the hot columns, `Ord`) and `take` (in rank order until the quantity is met; split the last; share a
   tie by weight), so six strategies and their helper types become two functions and a data table. Keep the cursor, the
   heap and the sweep **only if the benchmark shows the ranking cannot match them** at 100k parcels; otherwise they go.
   The relief **order** of `exact` (K3c: the parcels of one transaction adding up, else the oldest) must stay byte-exact.
4. **Asset parts**, per K3c §4: decide in the map from the code, with the smaller cut costed. If it deletes the three
   guards and `PendingCarry`/`part_slots` and is net negative in lines, build it; if it makes `lots.rs` branch on "is
   this a part" anywhere, do not.
5. **`Holdings` iteration** (`into_sorted`, `within`, the chain of slots) stays as is unless the map shows the columns
   make a simpler one.

## The proof

- **Behaviour byte-identical**: goldens, mistakes, the K3c claims oracle (1,500 books) and its mutants, the K4b splits
  oracle, `fuzz.py ... diff`, the unit tests of `lots.rs` (they move, none weakened). Every policy has a model-based
  test: a naive `Vec<(qty, basis, key)>` implementation of FIFO/LIFO/HIFO/exact/prorata in the test module, compared
  to the real one on random landings and reliefs (seeded, a few thousand cases) including selectors, ties and
  merges; mutate the ranking and the take (swap two keys, round the last share wrongly) and the model must fail.
- **Performance, the reason for the lane**: `axiom check` on `bench/` 100k and 1m, and a **sale-heavy book** (generate one
  with 100k lots in a position and 50k sales, and one with prorata): wall time, user CPU, RSS, callgrind instruction
  counts, three runs each, fastest reported with the load average, against the baseline binary from your starting
  commit. Target: not slower anywhere, faster on the scans, RSS down. If the columns are not at least neutral, say so and
  keep the layout only if it is simpler.
- Size assertions in code: the hot record per parcel (target 24 bytes or fewer), the key.

## Rules of this lane

- Common bar. Functions under 40 lines, no bool parameters (`money: bool` is a `Kind` or goes into the identity),
  no parameter bundles, no `Arc/Mutex/Rc/RefCell`. `unsafe` only in `core`, each block with `// SAFETY:` and a test.
- No test deleted or weakened. Do not change what a parcel **means** or the order in which relief takes: the claims
  oracle and the goldens are the judges.
- Commit per step, each green: columns first (same behaviour, old relief), identity key second, the ranking relief third,
  parts last. A bad step is one `git revert`.

## Step 0: the map

`docs/v5/lanes/K3e-map.md`, committed first: `Parcel`, `Slice`, `Identity`, `Slot`, `Holding` and every field's readers (hot
or cold by count); every strategy of relief with the order it takes and what state it keeps (the cursor, the heap, the
sweep) and why; the six strategies as a table of rank and take; what a landing does; the benchmark of today's code on the
three books above (**measure before you design**); the part guards and the smaller cut, costed.

## Measure

Lines per crate (target: `lots.rs`+`assets*.rs` from about 2,500 to **under 1,500**); the histogram; the benchmark table
before and after; the list of changed outputs (none expected).

## Not in this lane

- A parcel on a debt-class tab (K3d phase C): you build over whatever K3d left. The forecast (K5c). `Disposal` as a
  derivation of the position history (K7b).
