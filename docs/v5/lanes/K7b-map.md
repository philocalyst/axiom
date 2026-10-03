# K7b map: what each view re-does, what the fold can record, and which views are a pivot

Written from the code at `3f17468` (K3d merged) before any code of the lane is committed. Paths are in `crates/`; line counts are
non-test lines by `briefs/loc.py` (`report` is 7,027 of them; the brief's 8,995 is `wc -l` with tests and comments). The sources are
[`lane-K7b-facts-out.md`](lane-K7b-facts-out.md), PROPOSAL §5 K7, DESIGN §3.8 to §3.10, [`K7a-map.md`](K7a-map.md) §3 and §17,
[`K3d-map.md`](K3d-map.md) §3.2, [`K5c-map.md`](K5c-map.md), and every file named below.

**What was done before this map was written, and why.** The measurements in §2 need the recorder, so a first version of it was
written to take them: `engine/src/histories.rs`, the hooks in `lots.rs`, `ledger.rs`, `post.rs`, `state.rs`, and `report`'s switch
from the replay to the histories (`balances.rs`, `balance.rs`, `lens.rs`, `history.rs`). That code is **uncommitted** while this map is
committed, and it goes in as separate commits after it (the engine half, then the report half), with the debugging line that printed
the sizes taken out. Every number marked *(scratch)* is from it. The baseline binary (`3f17468`) is built and kept.

## 0. Where the brief does not match the code

Eleven things, each of which decides something below.

1. **There is no "one place a position's balance changes".** `Holdings` (`engine/lots.rs`, 2,013 lines) is mutated from `post.rs` (ten
   sites: relief, `credit`, `land_with_codes`, the asset parcel), `settle.rs`, `claims.rs` (a write-off relieves one place and credits
   another), `ledger.rs` (a split scales every slot of a commodity), and `reconcile.rs` (a pad is posted as a flow). What there is is a
   **choke point**: every change to a slot's quantity goes through `Holdings::entry` (the only `&mut Slot`) or `Holdings::scale`, and
   `Slot::qty` is the balance ("plain and every lot, kept in step: nobody sums lots"). So the recorder notes the slots those two hand
   out and, when a fact has been done (`Ledger::step`, and the end of `post`, which `apply`, an occurrence, a claim and a pad also
   reach), writes one step for each slot whose `qty` is not what it last wrote. §2.
2. **`Snapshots::replay` is not the fold's balance, and the difference is a baseline bug.** The replay (`history.rs:206`, 116 lines)
   adds up *flows*, so it cannot see what the fold does to a place that is not a flow's end: a payment from a party relieves the
   owner's **tab** (K3c/K3d), a write-off takes parcels out of a claim place, a split scales lot by lot. The final-state shortcut
   (`journal_ends_by`, one day at or after the last fact) reads `run.holdings` and is right; every other day is the replay.
   Shown on the baseline, `docs/v5/measure/diff/cases2/recognition-writeoff-lines.ax` (an invoice of 3,300.00 to `ann`, 1,000.00 paid
   on 2026-02-10): `balance --at 2026-02-20` says `ann 3,300.00 USD` and `claims --at 2026-02-20` on the same day says 2,300.00 open;
   `balance --at 2026-03-20` (after the write-off) says `ann` is gone. A history recorded from the holdings makes the three agree, so
   **outputs will differ from the baseline exactly where the baseline contradicted its own final state**; each such book is listed
   in the report with the reason. Nothing else may differ (the oracle in §7 holds the two to each other on every book where the
   replay is right, and names the books where it is not).
3. **`register` cannot read `at(day)`.** Its running balance is a sum of *steps* (one per flow, in the journal's order, several a
   day) in which a flow counts if it is **real on the cutoff** (`is_real_on(window.cutoff)`), whatever day it was written for: a flow
   pending on its day and settled before the cutoff moves the balance on its own date. A history says what was held at the end of each
   day, in the days the flows stood. They differ for pending and returned flows and for any day with two flows, so a register that
   read steppers would change bytes. It stays a statement (a list of dated steps), not a pivot; what it can share is the `Posting`
   stream (it already does). The brief's "register reads `at(day)`" is not built, and this is why.
4. **`holdings_at`, `claims --at`, `lots --at` and `available --at` need parcels, not balances.** `claims::open` reads each parcel of a
   claim place (`lot.txn`, `lot.acquired`, `lot.qty`), `lots` reads basis and tie, `available` forks a ledger and applies a
   hypothetical withdrawal to it. A `(day, qty)` column per position cannot say which claim is still open. The re-fold they pay today
   (`Context::ledger_at`: +0.12 s at 100k, +0.25 s at 1m for `claims`; `available --at <past>`: +12.3 s at 1m, K7a-map §3) is the cost of
   *state at a day*, and a stepper of quantities is not state. §5 says what would remove it (month-end checkpoints, or parcels in
   columns: K3e) and why it is not this lane's. **`ledger_at` and `available`'s forks stay.** `holdings_at` is the free-function path
   (`report::report`), which exists for `Past::Journal` in `forecast/`, which this lane is told to stay out of: so it stays too (§6).
5. **`tax` and `budget` do not read postings.** `tax.rs` reads `Run.effects` and `budget.rs` reads `Run.headroom`: what the fold's laws
   counted (K3d-map §0.1). A pivot over postings would *recompute* what the fold already decided, which is the opposite of facts out.
   They are not instantiations, and the delete list does not count them.
6. **The period cut already exists once.** `calendar::Periods` (42 lines) is used by `flow`, `balance --monthly` (`column_days`),
   `budget` and `why #purpose`; `Window` is the model's. There is no second cut to delete.
7. **`Held.booked` and `Snapshots::valued` are dead weight, except one count.** `booked` is read only by `Basket::value` for a place
   that is not on the balance sheet, and `balance` shows balance-sheet places only; `valued` exists to fill it. What survives in the
   output is `snapshots.unpriced` (the note "N flows have no price on their day…" of `balance --value`): the count of flow ends in
   places that are not on the balance sheet (income, expense) whose commodity has no rate. It is one small function over the postings, kept
   (`unpriced_flows`), and it is the only thing in the lane that depends on `valued`.
8. **`core::sparse` already has what a window query needs** (`peak_within`, `low_within`, `steps_in_effect`). The lane's `core`
   change is none. No product view asks for a peak or a low yet (FBAR is a law of a system, not a view), so the sparse tables are
   built by whoever asks, for the position they ask about (`Steps::extremes`), with tests and the oracle, and nothing builds them
   otherwise. No lazy cell is needed: there is no shared state to fill, so §0 of the brief's `OnceCell` question is answered "not
   needed".
9. **`Run.posted` is made after the fold**, from the record (`ledger.rs::posted`, a parallel map over flows). "Beside the postings" is
   therefore beside the fold's posting step, which is where the recorder is.
10. **A hand-built `Run` exists in the tests** (`report/src/tests.rs:620`, the household). It needs histories too; §7 says how.
11. **There is no `K4c-map.md`** (K4c is not merged; `Flow` is still 192 bytes). Pivots reach flows only through `Posting` (`history.rs`,
    80 lines), so K4c's columns change that one file.

## 1. Every view: its fold, its lines, what it reads and what it re-does

Costs are K7a-map §3's (100k / 1m flows) unless this lane measured.

| view | lines | what it walks | what it recomputes that the run already has |
|---|---|---|---|
| `balance` (`balance.rs`) | 294 | `Snapshots::of` (history.rs): two passes over every flow (pairs, then cells), `accumulate`, splits, a scaling pass | **a balance per day, from the flows**: +0.02 to 0.05 s / +0.04 to 0.37 s; wrong where a claim was settled (§0.2) |
| `balance --monthly`, `--value` | (same) | the same, once, for 12 columns; `--value` also prices each basket | the same; `booked` per flow end for a value nothing shows (§0.7) |
| `summary` / `check` (`lib.rs:339`) | 245 | `NetWorth::of` on `Snapshots::of([today])`: the final-state shortcut, plus a `Plan::new` | the plan (K7a-map §14); the holdings are already the answer |
| `register` (`register.rs`) | 544 | `book.touching[place]` postings and `run.pads`, sorted; entity, asset and contract registers filter **all** `book.flows` | a running balance per step, as of the cutoff (§0.3); three registers that are one scan each |
| `claims` (`claims.rs`) | 180 | `holdings_at` or `Context::ledger_at`: a fold to the day; `owed_by_you` re-walks `book.touching[place]` per payable place, netting by code | the parcels of a day (§0.4); a debt's balance per code |
| `lots`, `available` | 116 / 322 | `ledger_at`; `available` forks the ledger once for the baseline and **once per slow holding per owner** | state at a day; the forks are hypotheticals, so they stay |
| `flow` (`flow.rs`) | 624 | `for_each_counted` over **all** postings real on the cutoff (K3d's `Counting::pieces`), `first_activity` scans `book.flows` once or twice more to find the first period, `forgiven_by` scans `run.written_off` | nothing the fold has as a column; four walkers (`PurposeTotals::of`+`add` 50 lines, `PartyTotals::of` 24, `why/purpose.rs::totals` 40, `forecast/variable.rs::purpose_history`) each keep their own rows over the one walk |
| `tax` (`tax.rs`) | 184 | `run.effects` filtered by owner and year, merged into lines | nothing: the fold's tallies |
| `budget` (`budget.rs`) | 210 | `run.headroom`, a scan per (budget, owner, month) | nothing: the fold's headroom readings |
| `limits`, `headroom`, `gains`, `contracts` | 79, 78, 102, 209 | `run.headroom`, `run.gains`, `book.contracts` and `run.promises` | nothing |
| `forecast` | 378 + 5 files | the fold continued past today (K5c); `variable.rs` walks postings for habits | not this lane's |
| `why` (nine pages, `why.rs` 279 + `why/*` 1,177) | 1,456 | each page filters `run.effects`, `run.gains`, `run.violations`, `run.promises`, `book.flows`, `book.touching` by its own key | `line.rs::consequences` scans three run vectors **per flow** of a line; every page re-derives "what this made" with its own filter |

What is *shared* already and stays: `lens.rs` (whose, worth, liquidity), `table.rs` (cells), `calendar.rs`, `history.rs`'s `Posting`, and
`flow::for_each_counted`, which is K3d's one rule read once.

## 2. The stepper: layout, sizes, measured memory

`engine/histories.rs`. A **position** is a place's holding of one commodity (a `Slot` of `Holdings`).

```text
Position       { place: Id<Place>, unit: Id<Commodity> }                       8 bytes
Histories      { positions: Vec<Position>   sorted by place, then commodity    8 bytes a position
                 starts: Vec<u32>           rows of position p: starts[p]..[p+1]  4 bytes a position
                 days: Vec<Day>             the day each step begins            4 bytes a step
                 balances: Vec<Qty>         what is held from it                8 bytes a step
                 places: Vec<u32>           positions of place i: [i]..[i+1]    4 bytes a place }
Steps<'a>      { days: &[Day], balances: &[Qty] }                              32 bytes, borrowed
Extremes<'a>   { days, Sparse<Qty, Max>, Sparse<Qty, Min> }                    built by the asker
Change (the fold's log row) { slot: u32, day: Day, balance: Qty }              16 bytes, dropped after `finish`
```

Columns, not `Vec<(Day, Qty)>`: the search for "the last step not after the day" touches only `days` (4 bytes a probe, 16 probes a cache
line), and the balance is read once. Places are in pre-order, so the positions beneath a place are **one run** (`beneath`), and a
subtree's balance is a slice sum. Same-day steps collapse to the last (what stands when the day ends), a step that changes nothing is
left out, and before its first step a position holds nothing. The frozen layout is `Groups`' counting sort with two columns.

**What the recorder adds to the fold:** `Slot.recorded: Qty` (8 bytes a slot), `Holdings.touched: Vec<u32>` (slots handed out since the
last write), one `moved()` drain per fact (empty for a fact that touches nothing), one 16-byte `Change` per step, and `Record::balances`
(forks forget it, as they forget every record). The cost of a fold that does not need a history is a push per `entry`.

**Measured *(scratch)*, `bench/` generated books, release, load average 8 (a shared machine):**

| | 100k flows | 1m flows |
|---|---|---|
| positions | 759 | 2,236 |
| steps recorded | 113,538 (1.18 per flow) | 990,112 (1.01 per flow) |
| the columns (12 bytes a step) | 1.4 MB | 11.9 MB |
| the log while folding (16 bytes a step) | 1.8 MB | 15.8 MB |
| freeze (counting sort of the log) | 3.9 ms | 45 ms |
| peak RSS of `check`, baseline → scratch | 80,548 → 80,600 KB (+0.06%) | 680,100 → 680,308 KB (+0.03%) |
| `check` wall, fastest of 3, baseline → scratch | 0.511 → 0.473 s | 4.925 → 5.032 s (+2.2%, inside the noise of this load; interleaved A/B and callgrind instructions are reported at the end) |

Steps are about one a flow because a flow moves two positions, but the far end of most flows is an income or expense place that
many flows reach in one day, and a day's steps collapse. **Decision: eager, in the fold.** The brief's line is 15% of RSS; this is
0.03%. A lazily built history would be the replay again with a new name, so it is not built.

## 3. The pivot, and which view is which instantiation

What is common to the views that read postings is **a walk, a row key, a column key and a grid**. The brief's
`pivot(run, Dimension, Period, Measure, Filter)` has five dimensions and three measures that nothing asks for in most
combinations; orthogonal enums would make `(Purpose, Balance)` representable and meaningless, which is the thing the common bar says
not to do. So the pivot is a pair of typed instantiations over one `Grid`, and each combination that exists is a type that cannot be
misused:

```text
Balances  { histories, days, cells }              Position × days × Balance     a read of Histories (balances.rs, scratch)
Counted<K>{ rows: Map<K, row>, grid, unpriced }   K × periods × Counted         for_each_counted, spread over the days recognized
            K = purpose rows (purpose, of an object, no purpose by description)  | party rows (root, party)
```

| view | rows | columns | measure | |
|---|---|---|---|---|
| `balance`, `--monthly`, `--value` (the value is a *reading* of a basket, priced whole, so it is applied after the sum) | position, summed up the place tree | days (the one day, or 12 month ends) | Balance | **`Balances`** |
| net worth (`balance`, `summary`) | the roots of Asset and Debt | the same | Balance, valued | `Balances` |
| `flow` by purpose and period | purpose tree, objects, no purpose | `Periods` | Counted | **`Counted`** |
| `flow --by party` | (root, party) | month `Periods` | Counted | **`Counted`** |
| `why #purpose`: activity and largest parties | party, one window | the year | Counted, purpose subtree | **`Counted`** |
| `forecast`'s habit history | purpose, spending | month | Counted | reads `for_each_counted` today; **not touched** (`forecast/`) |
| `tax` | owner, system, name | the year | the fold's tallies | **no**: reads `run.effects` (§0.5) |
| `budget` | purpose, owner, window | month or year | the fold's headroom | **no**: reads `run.headroom` (§0.5) |
| `register` | a place's flows, one row each | the journal's days | a running balance as of the cutoff | **no**: a statement (§0.3) |
| `claims`, `lots`, `available` | parcels | a day | state at a day | **no**: parcels (§0.4); `available`, `headroom`, `limits`, `forecast` are about the future or about readings |
| `gains`, `contracts` | disposals, promises | | | no: records of the fold, shown as they are |

`Dimension::Kind` and `Period`-as-a-row have no consumer, and a type for a consumer that does not exist is dead code: not built, and
the report says so. `Measure::Flow` (what moved, without recognition) has the same fate; `Counted` is K3d's one rule.

**Honest size of B:** the grid, the walk over `for_each_counted`, `spread_over`, the unpriced count and the "active" flag are written
four times today (§1); one pivot replaces `PurposeTotals::of`/`add`, `PartyTotals::of` and `why/purpose.rs::totals`. That is about
-100 lines and the layouts (the objects cursor, the unclassified rows, the roots of a party report) stay, because their text is
the goldens.

## 4. Provenance: the edges the fold records today, and the ones that are missing

What a `why` page can follow today, by where the edge lives:

| edge | where | notes |
|---|---|---|
| flow → the statement and its line | `Flow.txn → Txn { loc, doc, codes, flows }`, `Flow.loc` | `book.txns[flow.txn].flows` is the consecutive run of its legs and items |
| flow → how it came to be | `Flow.origin: Origin::{Written, Derived(Derivation), Occurrence(Id<Contract>)}` (model) | `Derivation` names the contract (`Interest`, `Principal`, `Claim`, `Otherwise`, `Refund`) or the asset (`Disposal`), **not the flow or the rule that caused it** |
| flow → who said its purpose | `Flow.purpose: Purposed { purpose, of, source: Provenance }` | `Provenance::{Written, Contract, Entity, Party, Commodity, Account, Derived}` |
| flow → what it settled | `Run.settlements: [(Id<Flow>, Settlement)]` (sorted by flow) | a returned payment's is kept (K3d) |
| flow → what it caused | `Effect.cause`, `Gain.cause`, `Violation.cause`: `Cause::{Flow(id), Transaction(txn), Applied(n), Time}` | **only as a scan**: no index by cause; `why LINE` does three scans of `run` per flow |
| effect → the law | `Effect.law`, `Violation.law`, `Headroom.law/step/subject` | |
| claim → write-off | `WriteOff { change, claim: Id<Flow> }`, `Book::claim_changes` | |
| occurrence → kept | `Promise.kept: (Day, Id<Txn>)`, `Promise.flows` range | |
| assertion → pad | `Pad.assert` | |

**Missing**, and what each costs:

1. **`Cause::Time` names no period or deadline**, so "what made this" for a period-end effect stops at "a period ending".
   The fold knows (`Fact::Deadline(rule, period)`); `Cause::Time { law, period }` is two words more per effect.
2. **The inverse edge, flow → consequences, as an index.** A sorted view of `effects`, `gains` and `violations` by cause is built in one
   pass over them (they are already in fold order, so the causes are nearly sorted); it turns `consequences` from O(flows × records)
   into O(flows × log + k).
3. **Claim → the payments that settled it**: derivable from `Settlement.parcels[..].txn`, not stored by claim. The `claims` page
   does not show it today; a walker over settlements gives it.
4. **A derived flow's cause and rule**: PROPOSAL's `Origin { event, rule, promise }` does not exist; `Derivation` lacks them. They
   arrive with K6b (the post host), which derives flows. This lane builds the walk over what exists and does not widen `Origin`
   in `model`, which K6 is in.
5. **Occurrence flows are not in `Run.posted`** (they are `run.promised_flows`), so a walk from a kept occurrence's flows goes through
   `Promise.flows`, not through postings.

**What C is, then:** one `Target` (what `why X` asked about, resolved once by `identify`), one **walk** that yields, for a target, the
*postings it touches*, the *records its postings caused* (through the cause index) and the *claims they settled*, and one formatter for
the four tables every page ends in (flows, effects, gains, violations; `flows_table`, `effects_table`, `gains::section` exist
already and are shared). The nine pages become nine selectors plus the sections that are about the target alone (composition and
parcels of a place, a law's facts, an asset's parts, a contract's terms), which are text the goldens fix and which stay. Every golden
`why` stays byte-identical. **The honest size is -100 lines at best**, not half of 1,456: most of `why/*` is those sections.

## 5. What would remove the re-fold of `claims --at`, `lots --at` and `available --at`, and why it is not this lane's

The state at a past day is parcels. Two ways to have it without folding the journal from its start:

- **Month-end checkpoints** (DESIGN §3.8's marks, by clone): `Folded` keeps a `Checkpoint` at each month end and `ledger_at(day)` resumes
  the nearest one and folds forward at most a month. A clone of the world is 6 ms at 1m (K7a-map §3), 144 of them are 0.8 s and their
  memory, so they can only be built **on a second past query** (a session's, not a one-shot command's), by one more fold that keeps them. That
  is a policy ("when is a session asked often enough?") that belongs to `Session`, not to the views, and it does nothing for the CLI.
- **Parcels in columns** (K3e): each parcel's open quantity becomes a stepper like a position's, and a claim's amount at a day is a read.

Both are later lanes. This lane's A serves `balance` (every form), `summary` and the extremes; `Context::ledger_at` stays, with
this section as its reason.

## 6. The delete list, by file *(scratch numbers where built; estimates marked ~)*

Non-test lines (`loc.py`):

| file | now | after | what goes |
|---|---|---|---|
| `report/history.rs` | 326 | **80** | `Snapshots` and `replay` (116 lines, the longest function in `report`), `final_state`, `empty`, `accumulate`, `split`, `keep`, `cell`, `change`, `column_from`, `held`, `pair_index`, `subtree`, `Held` |
| `report/balances.rs` | 0 | 29 | the read of the histories |
| `report/balance.rs` | 294 | 310 | `unpriced_flows` (+16): the one thing `valued` was for |
| `report/lens.rs` | 208 | 206 | `Held`, `booked`, the class branch of `Basket::value` |
| `engine/histories.rs` + hooks | 0 | 182 + ~38 | the recorder (`Histories`, `Steps`, `Extremes`, `Changes`), the hook in `Holdings`, `Ledger::record_balances`, `Run::histories` |
| `report/flow.rs`, `why/purpose.rs` | 624, 223 | ~ -100 | `PartyTotals::of`, `PurposeTotals::of`/`add`, `purpose::totals` into one pivot |
| `report/why.rs`, `why/*` | 1,456 | ~ -100 | the cause index and the shared selectors (§4) |
| `report/lib.rs`, `claims.rs`, `context.rs`, `available.rs` | | **0** | `report`, `report_with_sources`, `views`, `holdings_at`, `available::view_with_lens`, `Past::Journal` (K7a's list, about 90 lines) **stay**: `Past::Journal` is in `forecast.rs`, which this lane is told to stay out of, and the free path constructs it. A three-line edit there removes all of it: the orchestrator's call |

**Against the target, said now so it is not found later:** `report` goes from 7,027 to about **6,500** with all of the above (A: -203
now in `report`, +220 in `engine`, so **the tree does not shrink for A**: the lines move to the layer that has the facts), against the
brief's 5,500 and a stretch of 4,300. 5,500 is -1,500 of 7,027; what is identified is about -500. The rest would have to be output:
one `why` layout, one register for places, entities, assets and contracts, a `balance` with no `--value` note. Those change bytes,
which this lane is told not to do. The honest accounting is in the report.

## 7. How it is held to the baseline

- **The differential harness.** `docs/v5/measure/diff/run.sh` (468 files) and `docs/v5/measure/session/allcmds.sh` (1,592) gain, for each
  of `balance`, `balance --value`, `balance --monthly`, `register`, `claims`, `lots`, `flow` (each `--by`), `why`: 12 dates per
  example (the days of the postings and the days between), text and `--json`, `--for`; `fuzzcmds.py` gains the same dates; `fuzz.py … diff`
  runs on 2,000 mutants. Baseline: `scratchpad/k7b/baseline-axiom`, built from `3f17468`. A difference is a bug unless the baseline
  was wrong (§0.2), in which case it is listed with the book and the reason.
- **The oracle.** The naive replay (today's `Snapshots::replay`, reduced to raw balances, **kept in a test module only**) is held
  against `Run::histories` on every example, every probe book, and every fuzz book (`docs/v5/measure/session/fuzzbooks.py` writes
  N mutated projects; an ignored test reads them), at every day a posting stands on and the days between. The invariant that needs
  no replay: for every position, `at(Day::MAX)` is the quantity `run.holdings` holds. Where replay and history differ, the book is
  printed with the day and the position, and each is classified (a settled claim, a write-off, a lot-by-lot split) or is a bug.
  The household fixture's `Run` gets its histories from the same naive replay of its hand-built journal.
- **Mutants of the recording:** drop a step, merge steps of two days, off-by-one on the day, record before the fact instead of after,
  skip `scale`, skip the write-off hook, collapse the wrong way; each must fail the oracle (`docs/v5/measure/session/histories_mutants.py`,
  in the style of K7a's `mutants.py`). Survivors are listed with the reason.
- **Timing:** `balance --at`, `--monthly`, `register`, `check` at 100k and 1m, three runs, fastest, load average, interleaved with the
  baseline (`ab.py`); and **in-process**, because the CLI pays the fold either way and the 5x target is about the *view*: a session
  that has folded once answers `balance --at` forty times (`crates/session/examples/scrub.rs`, built against both commits). RSS from
  `bench/timeit.py`.

## 8. What K5c, K4c and K3d left, and what the lane builds on

- **K3d:** a posting's counted amount is decided once (`engine/recognition.rs`, `Counting::pieces`), read by the fold and by
  `flow::for_each_counted`. The `Counted` pivot is that walk with a grid behind it; it **does not recompute the amount**.
  K3d's own list of what it did not finish is a list of what the pivot inherits: a transfer purpose's reversal counts as `-volume`,
  `AccrualAt::Due` has no test.
- **K5c:** the forecast is the fold; `forecast/` is not touched. `forecast/variable.rs::purpose_history` calls `for_each_counted` and
  `forecast/trace.rs` builds a `Basket` of what a ledger holds: the lane changes `Basket`'s `add` to take a `Qty` (the `Held` wrapper
  was always `booked: ZERO` there), a **one-line edit in `trace.rs` and its import**, which is the only contact with that directory.
- **K4c:** not merged, no map. `Posting::at` is the one place `Flow` and `Posted` are read together.
- **K7a:** `Session`, `Folded`, `Context::over`. The histories live in `Run`, so they are kept by `Folded` with no new field, and
  `Session::query` reads them without a plan for `balance` once the views stop needing one (they still do for `lens`, which reads
  ownership: a plan per answer remains, K7a-map §14).

## 9. Deltas (§D): design only, not built

DESIGN §3.9: a linear view patches itself from the delta between a checkpoint and the re-fold.

- **A pivot over postings is linear in the postings**: `Δview = view(Δpostings)`, with a posting added counting +1 and a removed one -1
  (a Z-set of postings). `Counted` is linear in the grid it fills, *except* that `MovementShares::split` carries a rounding boundary per
  (place, commodity) across postings under an owner scope, so for `--for` the delta must start from the cumulative at the checkpoint day, a
  `Map<(Place, Unit), Qty>` a checkpoint would have to keep. Under `everyone` it is linear as it stands.
- **What `Session::apply` needs of the steppers:** (1) the fold's `Changes` log kept after `finish` (16 bytes a step; it is
  chronological, so the steps of day D and after are a suffix found by one `partition_point` on its days); (2) slot numbers that are
  stable across an edit of day D (slots are numbered in the order the fold made them: a prefix is stable, and a re-fold from the
  checkpoint at or before D re-makes the same ones, then new ones); (3) a checkpoint at or before D to re-fold from (§5); (4) a re-freeze,
  45 ms at 1m, or a patch of each position's columns from the first step on or after D (`partition_point`, truncate, append the re-fold's
  steps).
- **A non-linear measure: a running balance's `peak`.** An edit of amount `a` on day D shifts every step from D on by `a` when it adds a
  flow and changes nothing else, so for a window wholly after D the new peak is the old peak `+ a`, and for a window that straddles D it
  is `max(peak over the part before D, peak over the part after D + a)`: **two O(1) sparse-table queries over the old table**. An edit
  that adds or removes steps (a flow on a day with none) moves the table's cells after it, so its suffix is rebuilt, O(m log m) for the
  m steps after D, or the table becomes a segment tree with lazy addition (O(log n) per query, per edit). A pivot over `Counted` has no
  such issue, and `Balances` is a read, so it needs nothing but the patched columns.
- **Claims, lots, available:** state, not a stream; they are re-asked of the re-folded ledger (§5).

## 10. The plan, and what each step ends with

| step | what | proof |
|---|---|---|
| 0 | this map | |
| A1 | `engine`: the recorder, `Run::histories`; unit tests (`Histories`, the extremes against a scan) and the engine's own cases: a pending flow that settles, a return, a split, a pad, a write-off, an asset sale | `at(Day::MAX)` equals the holdings on every engine test book |
| A2 | `report`: `Balances` replaces `Snapshots`; the tests that named it move to it; the household's histories; `unpriced_flows` | goldens and mistakes byte-identical; the 12-date harness; the oracle; **the list of books where the baseline was wrong** |
| A3 | the oracle and its mutants; timings and RSS; `scrub` | |
| B | `Counted` and the purpose and party instantiations; `why #purpose` | harness; goldens |
| C | the cause index, `Target`, the walk, the four shared tables | `why` goldens byte-identical; harness over `why` of every place, purpose, entity, law and line of every example |
| D | §9, in this file | |
| end | this file's "what was built, measured, not finished, and the three places I am least proud of" | |
