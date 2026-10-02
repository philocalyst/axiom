# Lane K0b: groundwork outside the model, and the performance baseline

Read [`common.md`](common.md) first; it is part of this brief. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k0b`, on branch `claude/great-wozniak-pnqn7x-v5-k0b`.

**Your crates:** `engine`, `report`, `sync` and `cli`, plus small additions to `core` and `syntax` named below.
Lane K0a works on `model` at the same time, so do not edit `model`. If something here needs a model change, write
it down for the report instead.

**The rule of this lane:** no behaviour changes. Every passing test still passes, the four known failures still
fail the same way, and `sh tests/golden.sh` and `sh tests/mistakes/run.sh` leave `git diff tests/` empty.

**Do not polish code that a later lane deletes:**
- `materialize_group`, `instantiate_occurrence` and `post_written_occurrence` in `engine/ledger.rs` (K5 deletes the
  materializer);
- `post_journal` (K4 replaces it with `solve`);
- `report/forecast.rs::contract_forecasts` and `projection.rs` (K5: the forecast becomes the fold);
- `report/claims.rs::holdings_at` and `history.rs` (K7).

Leave them as they are.

Work in this order. Each commit builds and passes.

## 1. Measure first

Before you change anything, record the performance baseline:
- `sh bench/run.sh 10k 100k 1m` (set `AXIOM_BENCH_DIR` to a directory under `/tmp`, never inside the repository);
- `sh bench/profile.sh 100k`, which writes callgrind and massif summaries to `bench/perf/`.

Keep the tables. You will re-run both at the end, and the report must show before and after.

Another lane builds on the same four cores while you work, so wall-clock times are noisy. Decide with callgrind's
instruction counts, which are deterministic, and treat wall times as a check that runs three times and keeps the
fastest.

## 2. Owner composition, once

`engine/plan.rs` has `entity_owners` and `place_owners`. Each composes ownership ratios through the owner graph
with the same overflow-checked loop. `entity_owners` builds a `Vec<Vec<(Id<Entity>, Ratio, Loc)>>` first.
- Write one composition: edges collected into `Groups` (counting sort, as `core::groups` does), then one walk that
  multiplies shares along paths and reports a cycle or an overflow once.
- Both callers use it.
- No `Vec<Vec<_>>`.

## 3. One JSON string writer

`cli/commands.rs::json_string` duplicates `report/json.rs`'s `string`/`escaped`. Make report's the one (it is the
crate that owns JSON output), make it `pub`, and have cli call it. Both must escape identically today. Check that,
and if they differ, the report path's behaviour wins only if no golden or test changes; otherwise keep both and say
why.

## 4. Sync's dates come from syntax

`sync/write.rs` re-implements syntax's `Folder::of` (`Context::of_path`), date headings and short-date completion
(`Context::shorten`/`complete`/`named`, about 120 lines). Call syntax's versions. If syntax lacks one, add it to
`syntax` beside `Folder` as a small pub function (with a unit test) rather than keeping sync's copy. The output must
be byte-identical: sync writes into users' files.

## 5. Sync applies its own changes

`cli/commands.rs` holds sync's file application: `apply_changes` and the symlink guard
`canonical_existing_ancestor`, about 250 lines. That logic belongs in `sync`, so that an MCP server or a GUI can
apply a sync plan without the CLI.
- Move it into `sync` behind a small typed API: `apply(root, &[Change]) -> Vec<Diagnostic>`, or better, a type that
  makes "planned" and "applied" different states.
- The CLI calls it and only renders.
- Keep every diagnostic byte-identical.

## 6. Engine and report types that re-implement core

- **`RuntimeRange`** re-implements `core::Run<T>`. Replace it if the replacement stays inside your crates. If the
  type crosses into `model`, note it for the report instead.
- Grep your crates for `Vec<Vec<`. There are hits in `sync/peg.rs`, `sync/tagged.rs`, `sync/csv.rs`,
  `sync/planner.rs` and `cli/render/findings.rs`. Replace each with `Groups` or a flat vector plus offsets where the
  inner vectors are built once and read many times. Leave it where the nesting is genuinely ragged and short-lived,
  and say so.

## 7. Split the long survivors

These functions stay in v5 and are too long:
- `sync/planner.rs::plan` (281 lines);
- `report/flow.rs::purpose_view`;
- `report/why/line.rs::items`;
- `sync/format.rs::record`;
- the longest survivors `python3 docs/v5/measure/fnlen.py crates` shows in your crates, excluding the
  "do not polish" list above.

Make each a short driver over well-named steps. Where a step needs more than five inputs, use a borrowed context
struct. Do not change the order of any output.

## 8. Where SIMD would pay

From the callgrind profile, name the three hottest leaf loops in your crates and in `syntax`/`core`, with their
instruction share. Typical candidates are line splitting, number parsing, the totals prefix sums and the timeline
merge. For each, say whether a `fearless_simd` 1.0 kernel could plausibly reach 1.3×, and why or why not.

Implement **at most one**, only if it measures at least 1.3× on its path and moves end-to-end `check` at 1m by
at least 3%.
- Keep a scalar path, and dispatch once at the top.
- Add `fearless_simd = "1"` to the workspace dependencies only if you keep a kernel.

A well-argued "none pays yet" is an acceptable answer.

## Your report additionally includes

- The bench table and the callgrind top-10, before and after.
- The SIMD analysis from step 8.
- Anything you found that needs a model change, for the orchestrator to route to K0a or a kernel lane.
