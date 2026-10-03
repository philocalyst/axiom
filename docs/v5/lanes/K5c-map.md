# K5c map: how the forecast folds today, what the fold has to learn to go past today, and what it leaves

Written before the first code change of lane K5c, from the code at `c8c1695` (K5b merged), and checked against what the code
does (the last section says how). Paths are in `crates/`; line numbers are those of `c8c1695`. The vocabulary is K5a's and K5b's:
[`K5a-map.md`](K5a-map.md), [`K5b-map.md`](K5b-map.md) (§11 to §13), `engine/src/monitor.rs`, `model/src/promise.rs`.

Today a forecast is **two ledgers and a merge**. `report/forecast.rs::contract_forecasts` (157 lines) resumes the view's checkpoint,
walks the days that `Promises::expected` gives it beside the habit flows, asks `Ledger::instantiate_occurrence` for each occurrence
and applies it to that ledger to learn the next one's amounts; it hands every flow back, and `forecast/projection.rs` resumes the
checkpoint **again**, applies every flow in day order and reads the position at each month end. Nothing in `engine` knows that a
promise comes due after `today`; the report decides it. This lane moves that knowledge into the fold, so the report only reads.

## 0. What the brief says and what the code does

Ten things in the brief or in K5b's map do not match the code, and each decides something below.

1. **`contract_forecasts` is not a second materializer; it is a second *driver*.** The amounts are `instantiate_occurrence`'s (K4b),
   the due days and the ordinals are `Promises::expected`'s (K5b, so the "numbers from 0 inside the window" the brief describes
   is already gone: an ordinal is its index in the schedule, in history and in the forecast, and
   `RuntimeTxn::ContractOccurrence`'s equality ignores `source`, so a kept occurrence and the one a forecast expects are the same
   key). What is left of the 157 lines is the loop: merge two sorted sources by day, advance the ledger, apply, keep a row.
2. **The two ledgers can disagree, and the report cannot tell.** `contract_forecasts`'s ledger takes the habit flows as they are and
   never caps them; the second ledger runs each flow through `within_means` first. An amount that reads a balance (`10% of
   [checking]`, an `=` leg, a cash `all`) is computed against the first ledger's state and posted in the second. In the
   checkpoint-less path (`report()` of `lib.rs:241`, every report test) each of the two folds the whole journal, so a forecast folds
   the book three times (the run, and one for each ledger).
3. **`project` and `project_from` (`projection.rs:39, 77`) have no caller outside their own tests** (`projection.rs:280-322`). The
   production callers are `project_runtime` (from `view_with`, `forecast.rs:92`) and `project_runtime_from` (`forecast.rs:90`).
4. **`available` needs nothing of `projection.rs`.** It starts or forks its own ledger (`available.rs:42`, forks at `:72` and `:292`) and applies one flow
   through `Ledger::apply_runtime` with `RuntimeTxn::Adjustment` and the run's `runtime_details` (`available.rs:285-293`). `history.rs`
   (K7's) neither. What `projection.rs` holds besides the driver is not a replay of the book but **reading**: `Trace`, the loop over
   the month ends, `in_hand_or_owed`, `grown`, `compound`, `within_means`, `note_overdrafts` (about 110 of its 170 lines of code).
   They stay, in a file that says what they are; the driver in them (`project_runtime`, `project_runtime_from`, and the dead `project`
   and `project_from`) goes. `Trace` still needs what a fold continued past today leaves to read (§6.4).
5. **`a_context_forecast_keeps_historical_and_same_day_obligations_once` fails for a reason that is not in the forecast.** Its book says
   `each year closing 12-31`, and LANGUAGE §8 says "the year closes on that day of the next" (`Closing::day_for`, model `law.rs:202`,
   `closings.rs`: "A year is judged in the next one"): the 2026 year closes on **2027-12-31**. The test asserts a `year-end-tax` of
   10.00 USD due 2027-01-15 in a forecast to 2027-03-01, which the fold cannot make, because the law has not run by then
   (`axiom why year-end-tax --today 2026-12-31`: "Ran 0 times"; `tax 2026` says "the return closes on 2027-12-31"). The same book with
   `each year` (no `closing`) gives exactly the three asserted rows from the **baseline** forecast, so the closing prefix is not the
   cause: both paths already agree (the test's first assertion, `shared == old`, passes), and what they both lack is the law's year
   end. **It cannot pass as written without changing the book or the language**, and the brief forbids editing its assertions.
   This lane adds a sibling test whose book says `each year` and asserts the same three rows (what the test means), leaves
   the original as it is, and says so in the report. §8 shows the run.
6. **The loan test passes since K5b** (`Promises::expected` stops a loan after its payments); K5c keeps it passing by the residual
   (an `Annuity` is done when `open == 0`).
7. **A due day that is on or before `today` and not yet missed is neither history nor forecast today.** The forecast's window is
   `today+1..until`, so an occurrence due yesterday and still within its reach is not forecast, and is not a record either:
   it waits for a line or for its reach to pass. The fold continued past today does exactly what a fold run to a later day would
   do with it: the monitor misses it when its reach passes, and a line already written for it keeps it. That is also what makes
   "the forecast from an earlier today and the history from a later one agree" true of it (§5).
8. **The forecast is filtered by `--for`.** `contract_forecasts` skips a contract whose owner the lens does not own
   (`forecast.rs:372`): the laws, balances and tallies of a `--for me` forecast never see another owner's contracts. The fold keeps that:
   promising takes the contracts it is asked for.
9. **Overdrafts are noted after every applied flow, contracts' included** (`note_overdrafts`, `projection.rs:138`, called for
   `contract_flows` too, since they are in `flows`), and `native_contract_terms_project_paychecks_once_and_preserve_overdrafts`
   asserts "overdrawn" from a *contract* flow. A fold that posts the contract's flows by itself must let the report see each one,
   not only the position at the day's end (an intraday dip is a different first day and lowest balance). §6.3.
10. **`Term::Due.after`, `Residual::deadline` and `Promises::deadline_of` are read by a test and the oracle only** (K5b §11.4). The
    claim item decides whether they are read or deleted (§9).

## 1. Every caller of what goes, and what it needs of the result

| what goes | callers | what each needs |
|---|---|---|
| `contract_forecasts` (`forecast.rs:325`, 157 lines) | `view_with` (`forecast.rs:83`) only | `(contract_flows, runtime_details, rows, issues)`: the flows to apply to the second ledger, the rows of "Contract occurrences", and the issues (an occurrence that could not be made; a required input that is missing) |
| `project_runtime` (`projection.rs:53`) | `view_with` (`forecast.rs:92`), the no-checkpoint path | a `Trace` from a ledger that folds the whole journal |
| `project_runtime_from` (`projection.rs:101`) | `view_with` (`forecast.rs:90`) | a `Trace` from the checkpoint paired with the run |
| `project`, `project_from` (`projection.rs:39, 77`) | three tests of `projection.rs` | the same, for flows with no runtime wrapper |
| `Promises::expected`, `payments`, `of_life` (`model/promise.rs:230-258`) | `contract_forecasts` (`forecast.rs:376`), `model/tests/promises.rs:305,309`, the scratch dump `docs/v5/measure/internals/main.rs:55` | the days a stream owes in a window with their ordinals, a loan's from its first payment to its last. A `Residual` says the same one step at a time and is what the monitor already walks |
| `RuntimeFlow::source`, `source_at` (`model/journal.rs:129,135`) | `forecast.rs:80` (the habit flows) | a habit flow as a runtime flow with the journal's transaction and no detail; `Ledger::apply(&Flow)` takes exactly that without the wrapper |
| `instantiate_occurrence` as `pub` | `contract_forecasts`, the dump | the fold calls it; nothing outside `engine` needs to |
| `ForecastError` (`model/book.rs:599`) | `TemplateError::Forecast`; stays: it is what a day's factor can fail with |  |

What `Trace` reads of the ledger (`ledger.recorded()`): `effects` (obligations coming due: `coming_due`), `violations` and `diagnostics`
(problems ahead), and `holdings()` and `balance()` for the positions. The rows need the flows each occurrence made.

## 2. What `available` and `history` use of `projection.rs`, by file and line

| consumer | uses | of `projection.rs` |
|---|---|---|
| `report/available.rs:42` (`view_with_lens`) | `lens.plan().start(..)`, `advance_to_closing` | nothing |
| `report/available.rs:72` (`from_ledger`) | `ledger.fork()`, `advance(horizon)` | nothing |
| `report/available.rs:256-300` (`Reach::of`) | `RuntimeFlow { txn: RuntimeTxn::Adjustment .. }`, `fork.apply_runtime(&runtime, runtime_details)`, `run.runtime_details` | nothing: `apply_runtime`, `RuntimeFlow`, `RuntimeDetail` and `Run.runtime_details` **stay** for it |
| `report/context.rs:96-104` (`forecast`) | `forecast::view_from(&plan, &checkpoint, &run, effects, lens, relaxed, until, paths)` | the entry point; its signature does not change |
| `report/context.rs:127` (`ledger_at`), `report/claims.rs:52` (`holdings_at`), `report/history.rs:207` (`replay`) | their own `plan.start`/`resume` and folds | nothing (K7's: not touched) |

The functions this lane and K7 both call, so that the second to merge knows: `lens.rs`'s `Lens::{owns, owns_entity, place_qty, value,
liquidity, free, known, plan}` and `Basket`; `history.rs`'s `Held` and `postings` (`expected.rs`, `variable.rs`, `trace.rs`);
`flow.rs`'s `scoped_movement_qty`, `movement_in_base_with`, `MovementShares`, `movement_place` (`forecast.rs`, `variable.rs`);
`places.rs`'s `path` and `route`; `claims.rs`'s `Claim`/`open`. This lane **does not edit** any of them.

## 3. How the horizon is decided today, and how the fold takes it

`view_with` (`forecast.rs:73`): `until = until.unwrap_or_else(|| default_horizon(book, today)).max(today)`, where the default is a
year ahead or the day a return closes if that is within four months after it (`closings::next_after`). Both ledgers are given
`Options { today: until }` (`forecast.rs:355`, `projection.rs:113`), and `Ledger::resumed` makes the fold's horizon
`plan.horizon(until)` = `max(until, the journal's last fact)` (`plan.rs:262`): deadlines and period ends fire up to it and no further
(`advance_through` clamps its limit to `Moment::end_of(horizon)`, `ledger.rs:361`).

The fold takes it the same way. `Ledger::reach(day)` (`ledger.rs:198`) already lengthens the horizon of a ledger that stands on
`today` ("as they would had the ledger been started for a later `today`"), and nothing but one engine test calls it. A forecast
ledger is made with the run's own `Options { today }` and **reaches** to `until`; a promised occurrence is a moment of the fold
like any other and `advance_through` does not take one past the horizon.

## 4. What a checkpoint holds

`Checkpoint { day, phase, applied, temporal_through, world, record, digest }` (`checkpoint.rs`): the **world** (the monitor's
residuals and miss heap, `Holdings`, `Totals`, `Tallies`, `Assets`, the temporal history) and a **forked record** (what must not be
reported again: `resolved`, `computed_basis`, `unsolved`, `settled`, `checkpoints`, `failing`, `reported`, `ambiguous`, `missing`;
the gains, effects, violations, diagnostics, promises and their flow pools start empty, so a ledger resumed from it records only what
it causes). `resume` re-seeks the timeline by binary search (`Timeline::before_closings(plan, day)`), so no stale index is kept.
The view's checkpoint stands on `today` **before** that day's closings, so a resumed ledger can still take a hypothetical flow
before them, and `ledger.advance(today)` then closes the day. `monitor.waiting` is in it, so the forecast ledger knows, for every
stream, the oldest due day nothing has kept or missed yet.

## 5. `Options.today` in the fold and in the forecast, and whether a second `today` is needed

In the fold, `options.today` is **the day the fold is run to**: it sets the horizon, is `Run.today`, is the day `finish()` judges
what is overdue by, and (only for a book with no fact at all) is where the monitor starts to watch. In the forecast it has been
the *horizon* (`until`), and the report's `today` is the day the checkpoint stands on. **No second `today` is needed**: the ledger
already holds both, `clock.day` (where it stands) and the horizon (how far it may go). The forecast ledger takes the run's
`Options { today, relaxed }` and `reach(until)`; that makes the checkpoint-less path equal to the fold that produced the run
(`watch_from(today)` is then the same day in both, where `Options { today: until }` made a book with no fact watch from the
horizon).

## 6. What the fold learns

### 6.1 A promise is promised

A stream's residual says what is still *owed*: the oldest due day no line has kept and the monitor has not missed. A forecast needs
a different cursor over the same schedule: **what is still to be promised after today**, which may be a due day ahead of the
residual (a stream with a long `grace` is waiting on March while April is the next day to promise). So the ledger holds, beside
the monitor, a `Promising`: for every stream asked for, a `Residual::starting_at(promises, every, today + 1)` and a min-heap of
`(due day, stream)`, one entry for each stream, ordered as the old merge ordered them (day, contract, regular before standing).
The fold takes it as it takes a deadline of the timeline: `advance_through` compares the next promised day with the timeline's next
moment, and a promised occurrence has the moment `Moment::after_flows(due)`: after every journal flow of its day, before that day's
claim changes, assertions and closings, where a line that kept it would have been (and where `apply` has always placed a
hypothetical flow). When it falls due the fold

1. skips it if the journal wrote a line for that due day (a **sorted table of the written `(stream, due)` pairs**, built once when
   promising starts: the old `written` set, `forecast.rs:340`, but kept by the ledger and not rebuilt by the report);
2. asks `instantiate_occurrence` for its flows, which reads the ledger as it stands, as a line's occurrence does;
3. posts them through the one function that posts a kept occurrence's flows (K5b's `post_written_occurrence` split in two: what a
   line names, and what an occurrence posts), with `Amounts::written` as a kept one has, not `apply`'s re-resolution;
4. tells the monitor that ordinal is kept (`settle`), which misses what nothing kept before it, as a line would;
5. records a `Planned { contract, schedule, ordinal, due, made }` (the flows' range in the record's pool, or the `TemplateError`
   that stopped it) and advances the stream's residual: a loan's is done when `open == 0`, which is the loan test's answer.

The occurrence's identity is `RuntimeTxn::ContractOccurrence { contract, schedule, due, ordinal, source: None }`, which equals a kept
one's. Nothing is numbered by the forecast.

### 6.2 What is not promised

An occurrence due on or before `today`, a waived day, a day after the contract's life (`Sched::nth` knows all three), one the journal
has written (above), and any occurrence of a contract the caller did not ask for.

### 6.3 What the report reads, and why it can step

The report keeps its loop over the month ends. A forecast flow of a habit is `Ledger::apply(&Flow)` (the wrapper was only
`RuntimeFlow::source`). To let the report see each promised occurrence as it posts (§0.9) and the balance a habit flow is judged
against (`within_means`), the engine offers **one step**: `promise_through(day) -> Option<Planned>`, which takes the next promised
occurrence due on or before `day` (the journal's facts before it first) and says what it was. `advance` and `advance_to_closing`
take the same occurrences without saying, through the same function, so the two ways cannot disagree. The loop of the report is
then: for each habit flow, take every occurrence due by its day, judge the flow, apply it; at each month end, take the rest, close
the day, read the positions. A watcher type parameter threaded through `advance_through` was the other way; it touches `fold_through`, `apply_view`, `conclude` and so every
caller of `advance`, for what one report reads, and it is not built.

### 6.4 The result

`Recorded` (what a ledger holds that is its own) gains `planned` and the pools its ranges index, so a forecast reads
`ledger.recorded()` as it always has and the rows, the issues and the "required input is missing" notes come from `Planned`
(`made: Err(error)` is `"{contract} could not be forecast: {error:?}"` as before). `Trace` keeps `ledger`, `liquid`, `worth` and
`overdrafts`.

## 7. The trail (brief item 2): not in this lane, and the smaller cut

`core::trail` is built (`Trailed<T: Copy, L>`, `Fork`) and nothing in the engine uses it. Making the fold's state `Trailed` is not
this lane's size. What a fork would undo is `Holdings` (`Vec<Slot>`, each with a `Vec<Parcel>`, a lazy HIFO heap and a
`part_slots` map: 2,015 lines of `lots.rs`), `Totals` (a `Vec<Windows>` and three `Map`s of `Windows` or `History`, which hold
`Vec`s: 1,011 lines), `Tallies` and `Assets` (`Map`s and `Vec<Part>`: 871 + 304 lines), the temporal history, the monitor and the
record's maps, called from 82 places in `engine/src` (34 methods of the six containers) and from every `post`, `fire` and `lots` path. `Trailed` is a dense array of
`Copy` cells with a log of `(index, old)`: none of these is one, and K3's parcels-in-columns (DESIGN §3.6) and the steppers of K7 are
what would make them so. **The smaller cut is what `resume` does**: clone the world once at `today` and fold the hypothetical on the
clone. It is one clone where the forecast made two (and three in the checkpoint-less path): at 100k flows a whole `forecast` costs
50 ms beyond `check`. The type that keeps a hypothetical from escaping already exists: a `Ledger` is an owned value and `Run` is
produced by `finish()`, so a forecast cannot write to the run; the guard (`Fork<'r>`) would add an `undo` that nothing here needs
until a session edits and re-asks. Nothing is built for it.

## 8. The failing test, shown

Baseline binary, the test's book (`systems/context-return.ax` and `axiom.ax` as in `source_tests.rs:1095-1133`), `--today 2026-12-31
--until 2027-03-01 --paths 1`:

| the law says | "Obligations coming due" |
|---|---|
| `each year closing 12-31` (the test) | 2027-02-15 historical-fee 5.00; 2027-03-01 pad-fee 2.00 (what the test's `left` is) |
| `each year` | **2027-01-15 year-end-tax 10.00**; 2027-02-15 historical-fee 5.00; 2027-03-01 pad-fee 2.00 (the test's `right`, exactly) |

## 9. A missed due day is a claim: what the claim path supports

LANGUAGE §7: a due day past its `grace` with nothing kept is missing, "a claim on whoever owes it (a rent the party owes, a bill the
owner owes)"; `due SPAN else ITEM` is a deadline after the due day and what is added when it passes; "the occurrence, when written,
settles what is missing". K3c built a claim as a parcel in a **tab**, settled by relief; K3c's map (§0.3, §6, §11) and K3d's brief
say what that supports. Read against what a monitor-made claim needs:

| needs | K3c gives | |
|---|---|---|
| a place to hold it | a tab exists only if a statement (`X owes Y`) asked for one (`World::tab`, lowering) | a contract with a `due` would have to ask for one when it is lowered: a model change, small |
| the owner is owed (`Blame::Party`: rent received, an invoice stream) | an `Asset`-class tab holds parcels; a later payment from the party relieves it by code, exact amount, oldest (`settle.rs`), with no new code: a kept occurrence is such a payment | supported |
| the owner owes (`Blame::Owner`: rent paid, a bill) | a `Debt`-class tab holds a plain balance (`Class::holds_parcels` is false) and a payment to the party **does not settle it** (K3c §0.3, §10; K3d phase C) | **not supported**: posting it would count the debt and the payment both |
| what a reader says of a parcel | `claims::open`, `explain::overdue` and `monitor::claim` read the payee and the due day from the **journal flow** that made the parcel (`Book::paid_into(txn.source_txn()?, place)`); a parcel the monitor makes has no journal flow | three readers, in K3c's and K3d's files, would learn a second source |
| recognition | a flow into a tab with a purpose counts when made (accrual) and again when paid; `books cash\|accrual` is read by nothing | K3d's: **posted without recognizing anything more or less than K3c's own claims do, and said so** |

The decision is taken **last, by building the narrowest thing** (§10, step 6) and keeping it only if it is clean: a claim on
the party, for a contract with `due`, posted when the deadline `due.after` passes with nothing kept, through the flow a claim already
is. If it is, `Term::Due.after` and `Residual::deadline` are read and the map says it. If it is not, they are deleted and the map
says why. The owner's debts wait for K3d phase C in either case.

## 10. The habit forecast, for the user to decide (brief item 4)

`expected.rs` (105 lines of code, with `Origin`, `Expectation`, `covered_by_contract`, `has_ended`), `variable.rs` (77: bootstrapped
monthly spending by category), `recurrence.rs` (86: rhythms detected in history) and `bands.rs` (100: the Monte Carlo of p10/p50/p90):
**368 lines**, plus what only they feed: in `forecast.rs` `simulate` (25), `expected_section` (54), the `p10 p50 p90` columns of
`outlook_section` and the second note of `method_notes`, `SEED` and `MIN_HISTORY_MONTHS`; `synth.rs::planned` (they alone make
planned flows); and in `model/book.rs` `Contract::covers`, `ContractCoverage` and the two helpers behind them (about 45 lines), which
exist only to stop a habit from echoing a contract's flow. About **550 lines in all**. They are not promises: they are inferred from the
journal, they are a different kind of claim ("this has happened often"), and they feed the fold as source flows (`Ledger::apply`),
exactly as they do now. **If they moved out**, the forecast would be promises only: no "What recurs", no bands, no habit echo to
suppress, and the loop of §6.3 would apply no flow at all (`within_means` and `note_overdrafts` for habit flows go too). The outlook
of a book whose spending is not in a contract would be flat. If they stayed but moved to a crate of their own (`forecast`'s
statistics, as PROPOSAL §7's first lever treats `sync`'s importers), the report would take them as a list of flows to apply, which is
the interface this lane leaves. **Not decided here.**

## 11. The plan, and what proves each step

| step | what | proof |
|---|---|---|
| 0 | this map | |
| 1 | the oracle, before any code: a book's `forecast` from an earlier `today` and the same book with those occurrences written, from a later one | (a) CLI layer, any build: net worth at every month end of the forecast equals `balance --at` of the written book, and the rows of "Contract occurrences" equal what the written book kept; run on the **baseline** to count where the second driver disagrees with the fold |
| 2 | the engine: `Promising`, `Planned`, `promise`, `promise_through`, `Recorded`, the split of `post_written_occurrence`; `instantiate_occurrence` private | (b) engine layer (`docs/v5/measure/forecasts/`): holdings at every month end, the effects after `today`, and every occurrence's identity and flows, the forecast ledger against the written book's history; tests; mutants of the new code, each caught |
| 3 | the report reads: `view_with` takes the fold; delete `contract_forecasts`, `project_runtime*`, `project`, `project_from`, `Promises::expected`, `RuntimeFlow::source*` | tests, goldens (`household-forecast` is the only forecast golden), the `fcrun.sh` forecasts of every example on nine days, `fuzz.py ... diff`, K4b's splits oracle (it runs `forecast`), K5b's promise oracle, timings on `examples/` and `bench/` 100k |
| 4 | the sibling test of §0.5; the dump `internals/main.rs` reads `Planned` | |
| 5 | the claim | the narrowest cut of §9, kept or dropped; K3c's claims oracle, mistakes |
| 6 | this map's last sections: what was built, every output that changed, the numbers | |

## 12. How this map was checked

- Callers: `grep` of every name in §1 over `crates/` and `docs/v5/measure/`, tests included, at `c8c1695`.
- §0.1 and §0.2: `forecast.rs:325-481` and `projection.rs` read in full; the second ledger's use of `within_means` (`projection.rs:134`).
- §0.5 and §8: the test's book run through the baseline binary (`target/release/axiom` of `c8c1695`) with and without `closing 12-31`,
  `why year-end-tax`, `tax 2026` on three days, and `cargo test` at `c8c1695`: **1,031 passed, 2 failed, 19 ignored**; the failures are
  `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` (the user's) and the one above.
- §3, §4, §5: `view_with`, `Ledger::resumed`, `Plan::horizon`, `Checkpoint`, `Record::forked`, `fold_to_view_and_effects_prefix` read in
  full.
- §7: the containers of the fold's state counted by `grep` of `world.*` calls in `engine/src` (82 uses of 34 methods on
  `holdings`, `totals`, `tallies`, `assets`, `temporal`, `monitor`), and the line counts of the files that hold them.
- §10: `loc.py`'s rule (non-blank, non-comment, outside `#[cfg(test)]`) over the four files and the functions named.
- The forecast of every example on nine days (`fcrun.sh`: `02-household` to `11-sam`, `today` 2026-02-14, 04-16 and 09-30, the default
  horizon and 2028-06-30) from the baseline binary is kept to compare the new build against (§11, step 3).
