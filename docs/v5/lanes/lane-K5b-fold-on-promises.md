# Lane K5b: the fold reads the promise, and what the old schedule code did goes

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F2, F4, F5 and §5 K5, and
[`../DESIGN.md`](../DESIGN.md) §3.7. Then the finished maps: `K5a-map.md` (all of it, especially sections 0, 7 and 10:
what the old code gets wrong, shown on books, and what K5a built), `K4b-map.md` sections 10 and 11 (where the fold's
occurrence code lives now: `engine/occurrence.rs`, `statement.rs`, `evaluate.rs`), and LANGUAGE.md §7 (normative). Your
worktree is `/home/user/axiom/.claude/worktrees/lane-k5b`, on branch `claude/great-wozniak-pnqn7x-v5-k5b`.

**Your crates:** `model` (`book.rs`'s `Contract`, `Terms` and the schedule code, `promise/`, `lower/contracts.rs`,
`lower/record.rs`'s matching), `engine` (`occurrence.rs`, and `ledger.rs` where it asks a schedule anything), `sync`
(`promise.rs`, `world.rs`, `binding.rs`). Lane K3c works at the same time on `engine/post.rs`, `lots.rs`, `assets*.rs`,
`fire.rs`, `explain.rs` and `report/claims.rs`: stay out of them.

## What is wrong

K5a built the new structure **beside** the old: `core::Dues`, `model::promise` (`Term`, `Schedule`, `Residual`, `Annuity`,
`Reckoning`), compiled once at the end of `build` into `Book.promises`. Nothing reads it. So today the book carries a
promise twice and the code answers "which occurrence is due, which one does this line keep, what does it come to" with
the old walkers: `Contract::occurrences`, `amount_on_schedule`, `recognition_on_schedule` (`book.rs`, ~280 lines),
`nearest_occurrence` (`record.rs`), the ordinal count and `loan_payment` in the engine (O(n) and O(periods) per
occurrence), `contract_forecasts`' own numbering, and `sync/promise.rs::keep_paired` (dead: nothing builds the `dues` it
reads). Three defects of K5a's section 7 are live in the product until the fold reads the new structure: the ordinal
walk (a contract with no `from` costs 9 s per kept line), the loan that never ends, and a due day lost after a waiver.

The monitor does not exist: the engine builds every `Run` with `monitor_complete: false` and `open_claims: Box::default()`,
and `sync` warns `sync-monitor-incomplete` for every book with a contract.

## What to build

1. **The fold reads `Promises`.** Occurrences, ordinals, due days in a window, the line a day keeps, the amount on a day
   (escalation, proration, covers, recognition window), a loan's payment: all from `Promises` / `Sched` / `Annuity` /
   `Residual`. Then **delete** `Contract::occurrences`, `amount_on_schedule`, `recognition_on_schedule`, the helpers only
   they used, `nearest_occurrence`, `loan_payment`, the ordinal count, `ContractOccurrences`, and `Reckoning`'s duplicate
   of the old arithmetic (it is the arithmetic now, once). K5a counted about 450 lines that go: the lane is measured on
   whether they do.
2. **A promise is stored once.** `Payment` clones the template, program and inputs; `Contract.terms` is a
   `Timeline<Terms>` whose stretches differ only in `state`/`change` (K5a's `alike`, a `debug_assert!`). Make that
   invariant the **type**: a contract holds its declared `Terms` once, its waived days as a `DaySet`, and its `ends`; the
   timeline of whole `Terms` goes, and `Payment` becomes an index into the one place the template lives. An invariant
   that holds only under `debug_assert!` and `--release` is not an invariant. Say in your map what reads `Terms` per
   day today (`terms.at(day)`) and what each reader needs instead.
3. **The monitor.** `Residual` advances by `Every`/`Due`/`Annuity`. The fold walks a `Residual` per stream beside the
   journal: a due day with a kept occurrence settles it, a due day past its grace with none is **missed**. For this lane,
   missed is a diagnostic and a `Run` result (the `overdue` list the report reads), computed by the monitor, **not a
   claim posting**: posting the claim for a missed `Due` is K5c's, once K3c's claim parcels have landed. Set
   `monitor_complete: true` and fill `open_claims` from what the claims are today; delete the `sync-monitor-incomplete`
   warning if it can no longer fire, and the dead `ForecastError` variants and `ForecastFeature` if nothing builds them.
4. **A deadline heap, only if it is the simplest thing.** `Due { after }` creates a deadline: a promise whose due day
   passed with nothing kept is overdue at `due + after`. A sorted pass over the residuals may be simpler than a heap.
   Choose from the code, say why in the map.
5. **`grace`, as LANGUAGE §7 says**: "Each due day is kept by the nearest occurrence within its `grace` (default: half a
   cadence)." The code today reads `grace` nowhere and uses a full cadence. Implement the spec: the reach is `grace`, or
   half the cadence. This **is a behaviour change**: list every golden, mistake and test output it changes, with the
   reason. If an example's output changes, stop and tell me before editing the book.
6. **`sync/promise.rs`**: delete `keep_paired` and `World.dues`; sync asks the same `Promises` the fold does (or, if it
   has no use for any, say so and delete the module).

## Rules of this lane

- **Behaviour change only where K5a's section 7 lists the old code as wrong (a, b, c, e, g, h, j, k) and for `grace` (d)**
  and the monitor's new diagnostics. Everything else byte-identical: goldens, mistakes, the three known failures (the
  loan-forecast one may now pass: that is the monitor's `Annuity` done-ness; say so).
- K5a's oracle (`docs/v5/measure/contracts.py`, `promises/`) is the proof: its dump asks the **old** code, and the old
  code goes in this lane. So first change the oracle to ask the new fold's answers (through the same questions) against
  K5a's reference, then delete the old code, and keep the reference, which is independent code, as the oracle's judge.
  Keep K5a's mutation harness working on the new code and mutation-test the monitor (every mutant killed or argued
  equivalent).
- A baseline binary from your starting commit; `fuzz.py ... diff` on mutated promise books; `splits.py` oracle and
  K4b's `split_tests.rs` (occurrences are materialized through `solve`).
- No test deleted or weakened; tests that name a deleted function move to its replacement with the same assertions.
- No `unwrap`/`expect` in the new code outside tests except where an invariant is typed; no bool parameters; functions
  under 40 lines; no parameter bundles: if a function needs six things, find out which of them belong together.

## Step 0: the map

`docs/v5/lanes/K5b-map.md`, committed before any code change. For every reader of the old schedule code (file, function):
what it asks, and the `Promises` call that answers it, in a table; what reads `Terms` at a day and what it needs; what
reads `Contract.terms`, `.standing`, `.ended`; where the forecast (`report/forecast/*`) calls the old code (K5c deletes its
driver, but it must keep compiling and pass with the same output: say what it will call); what `overdue` and
`open_claims` are today and what the monitor makes of them.

## Verification

At each commit that touches model or engine: `cargo fmt --all`; `cargo test --workspace --release --no-fail-fast`; the
mistakes corpus; `fuzz.py OLD NEW examples SEED 1000 diff`; K5a's oracle on the 1,500-project corpus; K4b's splits oracle.
`sh tests/golden.sh` at the end of each step. `git diff tests/` empty except for what you listed. Time `axiom check` on
`bench/` at 100k and 1m before and after (`sh bench/run.sh 100k 1m`) and on `diff/cases2/promise-no-from.ax` (9 s before).

## Measure

Lines per crate before and after; the function-length histogram; the types deleted with `size_of`; the target is the
~450 lines of old schedule code, the duplicate `Reckoning`, `Payment`'s clones, the timeline of `Terms` and the dead
sync/forecast residue: **about −900 lines** net of the monitor. Report what you land on, and where the lines were that
you expected.

## Not in this lane

- Posting a claim for a missed `Due`, the forecast as the same fold past today, deleting `contract_forecasts`: K5c.
- Loans' interest and principal, `resets`, `prepay`, `deposit`, `match`, an `asset` for a loan (written, checked, and
  read by nothing): K5d. Do not build them. Do not delete their grammar. List in your map each field of `Contract`/`Loan`
  that is still written and read by nothing after your lane.
- Claims and assets as parcels: K3c.
