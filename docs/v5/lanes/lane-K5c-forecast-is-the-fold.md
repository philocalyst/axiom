# Lane K5c: the forecast is the fold past today, and a missed due day is a claim

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F4 and F5 ("two fold drivers",
"the monitor does not exist"), §5 K5, and [`../DESIGN.md`](../DESIGN.md) §3.7 to §3.8 (the trail). Then the maps of the
lanes before you: `K5a-map.md`, `K5b-map.md` (what the fold now reads), `K3c-map.md` (claims are parcels, settlement is
relief). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k5c`, on branch
`claude/great-wozniak-pnqn7x-v5-k5c`.

**Your crates:** `report` (`forecast.rs`, `forecast/`), `engine` (the fold's horizon and its monitor), `sync` where it
reads the monitor. Lane K7 may work at the same time on the report's views, `history.rs`, `projection.rs`'s second
replay and `why/*`: stay out of those, and say in your map which functions the two of you both call.

## What is wrong

The forecast is a **second fold driver**. `report/forecast.rs::contract_forecasts` (160 lines) builds a throwaway
`Ledger` with `today` set to the horizon, materializes each contract's occurrences past today itself, numbers them from 0
inside the window (so an occurrence's identity differs between history and forecast), pushes the flows through
`project_runtime` / `project_runtime_from` (`forecast/projection.rs`, 326 lines: a **third** replay of the book), and reads
the result back. Every rule the fold knows (laws, claims, tallies, the tax lines) has to be re-run there by a second
mechanism, and the two disagree: two of the three known failures are its symptoms:

- `a_context_forecast_keeps_historical_and_same_day_obligations_once`: a forecast's closing prefix lacks a year-end tax
  the history has;
- `native_loan_forecast_stops_after_the_typed_principal_is_repaid` (K5b may have fixed it: check).

And nothing makes a due day that passed with nothing kept into a claim. K5b made the monitor and its `overdue`; the claim
is the missing half.

## What to build

1. **The forecast is the same fold, run past today.** `Plan` has `resume(checkpoint, options)`; K5b's monitor walks a
   `Residual` per stream. The forecast is: fold to `today`, take the state there, **continue the fold with each stream's
   residual producing its occurrences to the horizon**, read the run. The occurrences are the promised ones; the fold posts
   them as it posts a kept one. No second numbering: an ordinal is its index in the schedule, history and forecast alike.
   Delete `contract_forecasts`, `project_runtime`, `project_runtime_from` and `forecast/projection.rs` (or what of it
   `available` still needs: map it, do not guess), and the `RuntimeFlow` paths that existed only for them.
2. **Whatever mutates the fold's state for a hypothetical is undone by the type, not by cloning**: if `core::trail` fits
   (a mark at `today`, fold past, read, undo; a `Fork<'r>` guard that holds `&mut` and undoes on drop) use it; if making
   the fold's state `Trailed` is a larger lane than this one, **stop at the map** and say what the smaller cut is (a
   checkpoint clone at `today`, as `resume` does now). The map decides; do not let the trail eat the lane.
3. **A due day past its `due ... else` deadline with nothing kept is a claim** on whoever the promise blames: posted by the
   monitor through the claim path K3c built, as a parcel, so that `claims`, `check` and the forecast's "owed" section all
   see it, and a later kept occurrence settles it (relief). `monitor_complete: true` was K5b's.
4. **The habit forecast stays what it is**: `expected.rs`, `variable.rs`, `recurrence.rs`, `bands.rs` (inferred recurrences
   and bootstrapped bands) are not promises. They feed the fold's horizon as source flows, as they do now. Say in the map
   whether they should move out (PROPOSAL §7 lever: ~400 lines, no p10/p50/p90 bands): that is a decision for the user,
   so describe it and do not decide it.

## Rules of this lane

- **No behaviour change except** the two failing tests (they pass or the map says why not), the due-day claims (new), and
  forecast rows whose ordinal or identity changed because history and forecast now number alike: list every changed
  golden, mistake and test output with the reason. An example's forecast golden that changes for another reason stops the
  lane: tell me.
- A baseline binary from the starting commit; `fuzz.py ... diff`; the promise oracle (`contracts.py`) and K4b's splits
  oracle; a generator that compares **the forecast to the fold**: for a book whose `today` is moved later, the forecast
  from the earlier today and the history from the later one must agree on every occurrence they share (the strongest
  test the new structure allows; build it, mutation-test it).
- No test deleted or weakened; the two failing tests' assertions stay as written.
- Common bar: no bool parameters, functions under 40 lines, no parameter bundles.

## Step 0: the map

`docs/v5/lanes/K5c-map.md`, committed before any code: every caller of `contract_forecasts` and of the three projection
functions, and what each needs of the result; what `available` and `history` need from `projection.rs` (K7's territory:
name each function by file and line); how the horizon is decided today and how the fold would take it; what state a
checkpoint holds; what `Options.today` means in the fold and in the forecast, and whether a second `today` (the report's
vs the horizon's) is needed.

## Verification

As K5b's, plus the forecast-versus-fold oracle above, and `sh tests/golden.sh` at the end of each step. Time `axiom
forecast` on `examples/` before and after: a slowdown is a finding.

## Measure

Lines per crate; the histogram; deleted with the file names. Target **about −900 lines** (`contract_forecasts` 160, the
projection 326, `RuntimeFlow`/`RuntimeDetail` paths, the forecast's own `Run` plumbing, `ForecastError` and friends if
K5b left them).

## Not in this lane

- Steppers, pivots and provenance `why`: K7. Loan amortization, `deposit`, `resets`, `prepay`, `match`: K5d.
