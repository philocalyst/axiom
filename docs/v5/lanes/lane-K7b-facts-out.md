# Lane K7b: the run remembers its balances, a view is a pivot over them, and `why` is a walk

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §5 K7, [`../DESIGN.md`](../DESIGN.md) §3.8 to
§3.10 (the trail, facts out, the session), `K7a-map.md` (the `Session` this lane makes fast: its list of every place a
view re-folds or re-plans is **your work list**), `K3d-map.md` (where a posting's counted amount is decided: the one rule
your pivots read) and `K4c-map.md` (the flows' columns). Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k7b`, on branch `claude/great-wozniak-pnqn7x-v5-k7b`. **This lane starts after
K7a and K3d have merged** (K5c and K4c are welcome first, and the map says what each leaves you).

**Your crates:** `report` (everything but `forecast/`: K5c's), `engine` (`run`'s recording of balances, `facts.rs`),
`core` (`sparse.rs` gains what a window query needs). Lane K6 works in `model` and `eval.rs`: stay out of them.

## What is wrong

`report` is 8,995 lines for fourteen views, and each view is a bespoke fold over the result of a fold:

| view | what it does that the run already did | where |
|---|---|---|
| `balance --at`, `--monthly`, `register` | `Snapshots::replay` rebuilds balances per day from the postings | `history.rs` (430) |
| `claims --at`, `overdue --at` | `holdings_at` builds `Plan::new(book)` and **folds the book again** up to the day | `claims.rs` |
| `available` | forks the run **twice** | `available.rs` (387) |
| `forecast` | resumes a ledger (K5c: the fold continues; whatever is left of `projection.rs` goes) | `forecast/` |
| `flow`, `tax`, `budget`, `tally` | each walks the postings with its own filter and its own period cut | `flow.rs` (693), `tax.rs`, `budget.rs`, `balance.rs` |
| `why X` | one page per target kind (place, purpose, entity, contract, law, asset, line, code, system) | `why/` (1,900) |

and the facts a client wants (a balance on a day, a tally over a month, "what made this posting and what did it make") are
all derivable from the one stream the fold already walks. A GUI that scrubs a date slider or an MCP tool that asks
`balance --at` forty times pays a fold per question.

## What to build

**A. Position histories are steppers recorded by the fold.** At the one place a position's balance changes, the run
appends a `(day, balance)` row per position and commodity. Columnar and flat, in the style of `core::facts`: `days`,
`balances`, row offsets per position; reads are `Steps` over a borrowed slice, nothing to chase. Then `balance --at`,
`--monthly`, `register`, `holdings_at` and `claims --at` read `at(day)` and **the replay and the re-plan are deleted**.
`peak` and `low` over a window (FBAR's maximum balance) read a **sparse table** (`core::sparse`: O(n log n) to build on
first use, O(1) per query), with no sampling. Measure the memory cost (rows × bytes) at `bench/` 1m **before** committing:
if the histories cost more than 15% RSS built eagerly, build a position's history **on first use** from the postings
(the cheaper cut), and say which and why.

**B. A view is a pivot.** One function, `pivot(run, Dimension rows, Period cols, Measure, Filter) -> Table`, over the
posting stream, where `Dimension` is `Position | Purpose | Party | Kind | Period`, `Measure` is `Balance | Flow | Counted`
(the amount that counts, from K3d's one rule) and `Filter` is a small typed predicate. `balance`, `flow`, `tally`,
`tax`'s income lines and `budget`'s spent column become instantiations (a `Query` is a `Pivot` plus a shape for the
renderer), and the period cut (`--monthly`, `--by quarter`, `--year`) exists **once**. **The delete list is the proof**:
name every bespoke fold and its lines that go; a view that does not fit (`available`, `headroom`, `limits`: they are
about the future) stays and says why.

**C. `why` is a provenance walk.** The model already carries `Origin` and `Derivation`; the fold's postings carry an
`Origin { event, rule, promise }` edge (extend only what is missing, and say what). `why X` for any target is the walk of
those edges: **what made this** (posting → leg → event → statement → line, the rule or promise that derived it) and **what
this made** (the postings that read it: a claim's settlements, a law's derived flows). One `Target` enum, one walker, one
formatter: the nine pages become nine small selectors over the walk. Every line of every golden `why` stays
byte-identical unless the walk is *more* right (list each, with the book that shows it).

**D. Deltas, in the map only.** DESIGN §3.9: a linear view (balances, tallies, flows by purpose, claims) patches itself from
the delta between a checkpoint and the re-fold (Z-sets). **Do not build it**; say in the map what `Session::apply` needs of
your steppers and pivots (a pivot is linear in the postings: `Δview = view(Δpostings)`), and what a pivot over a
non-linear measure (a running balance's `peak`) needs instead.

## The proof

- **Every view, every example, every date**: a differential harness (extend `docs/v5/measure/diff/` and `fuzz.py`): for each
  example and 200 fuzz books, each of the views the lane touches at 12 dates (the days of the postings and the days
  between), `--monthly`, `--by`, json and text: **byte-identical to the baseline** binary built at your starting commit. A
  difference is a bug unless the baseline was wrong, in which case it is listed with the book and the reason.
- Goldens and mistakes byte-identical. The stepper has an oracle of its own: a naive replay in the test (the old
  `Snapshots::replay`, kept **in the test module only**) against the recorded history on every fuzz book; mutate the
  recording (drop a row, merge wrongly, off-by-one on the day) and the oracle must fail.
- Performance, **three runs, fastest reported, with the load average**: `axiom balance --at` and `--monthly` on `bench/`
  100k and 1m (target: at least 5× faster than the replay on 1m), `axiom check` not slower, RSS reported.

## Rules of this lane

- Common bar: functions under 40 lines, no bool parameters, no parameter bundles, no `unsafe` (a column of
  `(Day, Qty)` needs none; if you think it does, say why in the map first).
- The fold records the history **beside** the postings in one pass; a second pass over the postings to build the histories
  is the replay again with a new name.
- No test deleted or weakened; a test that names a deleted function moves to the one that replaces it.
- No Arc/Mutex/Rc/RefCell: a lazily built history is a `OnceCell` **only if** the borrow checker cannot express it as a
  `&mut self` build step in `Session`; say which in the map.

## Step 0: the map

`docs/v5/lanes/K7b-map.md`, committed first: each view's fold, its lines, what it reads and what it recomputes (the K7a
list, checked against the code); the stepper's layout with sizes and its memory at 100k and 1m measured on a scratch
build; the pivot's four types and the table that says which view is which instantiation (with the ones that are not,
and why); the provenance edges the fold records today and the ones missing; the delete list by file with line counts;
what K5c, K4c and K3d left you; the delta design for §D.

## Measure

Lines per crate (the target is `report` ≤ 5,500 from 8,995, and a stretch of 4,300); the histogram; the deleted files
and functions by name; the timings above; the list of changed outputs (should be none).

## Not in this lane

- The `Session` surface (K7a, merged first), the forecast (K5c), claim recognition (K3d), the flows' columns (K4c), an
  incremental `apply` (a later lane over the trail).
