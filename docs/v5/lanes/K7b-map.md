# K7b map: what each view re-does, what the fold can record, and which views are a pivot

Written from the code at `3f17468` (K3d merged) before any code of the lane is committed. Paths are in `crates/`; line counts are
non-test lines by `briefs/loc.py` (`report` is 7,027 of them; the brief's 8,995 is `wc -l` with tests and comments). The sources are
[`lane-K7b-facts-out.md`](lane-K7b-facts-out.md), PROPOSAL §5 K7, DESIGN §3.8 to §3.10, [`K7a-map.md`](K7a-map.md) §3 and §17,
[`K3d-map.md`](K3d-map.md) §3.2, [`K5c-map.md`](K5c-map.md), and every file named below.

**What was done before this map was written, and why.** The measurements in §2 need the recorder, so a first version of it was
written to take them: `engine/src/histories.rs`, the hooks in `lots.rs`, `ledger.rs`, `post.rs`, `state.rs`, and `report`'s switch
from the replay to the histories (`balances.rs`, `balance.rs`, `lens.rs`, `history.rs`). That code was **uncommitted** when this map
was committed, and went in as separate commits after it (the engine half, then the report half), with the debugging line that
printed the sizes taken out. Every number marked *(scratch)* is from it. The baseline binary (`3f17468`) is built and kept.
**Sections 0 to 10 are the map as approved; §11 onwards says what was built, where it differs from them, and what it measured.**

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
   columns: K3e) and why it is not this lane's. **`ledger_at` and `available`'s forks stay.** `holdings_at` was the free-function path
   (`report::report`), which exists for `Past::Journal` in `forecast/`, which this lane was told to stay out of: so it stayed too (§6)
   *until the orchestrator allowed the three-line edit that removes `Past::Journal`; the path is deleted (§11)*.
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
| flow → how it came to be | `Flow.origin: Origin::{Written, Derived(Derivation), Occurrence(Id<Contract>)}` (model) | `Derivation` names the contract (`Interest`, `Principal`, `Claim`, `Otherwise`, `Refund`), the asset (`Disposal`), the law (`Reparation`), the `also` line (`Also`), the kind (`SalesTax`), the sharer (`Share`) or the party (`PaidFor`), **never the flow that caused it** *(corrected after the build: the first version of this row left the last four out)* |
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

---

*Sections 0 to 10 are the map as approved before the lane's code was committed (with three corrections marked in place). What follows
was written when the lane was done: what was built, where it differs from the map, where the baseline was wrong, what it measured,
what is not finished, and what the orchestrator may decide for K7c.*

## 11. What was built, against §10

| commit | step | what |
|---|---|---|
| `e5248e4` | 0 | this map |
| `83df805` | A1 | `engine/histories.rs` (`Histories`, `Steps`, `Extremes`, `Changes`), the hooks (`Slot.recorded`, `Holdings.touched`, `moved()`), `Ledger::record_balances` at the end of `post` and of `step`, `Run::histories`; the engine's own oracle (`histories_tests.rs`: a ledger advanced a day at a time against the history, on books that settle, return, split, pad and claim) |
| `9bf577d` | A2 | `report/balances.rs`: `Balances` replaces `Snapshots`; `history.rs` 326 to 72 lines; `unpriced_flows` |
| `fe8f6b4` | A3 | `crates/session/tests/histories.rs` (the fold oracle and the replay oracle on every example and probe book), `dates.py`, `fuzzbooks.py`, `histories_mutants.py`, `crates/session/examples/scrub.rs` |
| `f6f3ad4` | | the free-function path deleted (`report`, `report_with_sources`, `views`, `holdings_at`, `available::view_with_lens`, `Context::new`, `Past::Journal`) |
| `47401bc` | B | `report/pivot.rs`: `Pivot<K>` over a flat `Grid`; the party table, the purpose table and `why #purpose`'s totals are instantiations |
| `bbffce9` | C | `why::Target` (what `why X` asked about, resolved once), `consequences` in one pass |
| `ee13f10`, `708e313` | | measure: the monitor-claim test, mutants that fail only a known test are not killed, `fuzzcmds.py` asks for the pivot's views and every kind of `why` |
| `e9c09c9` | | what a value could not price is asked of the run once (`Unpriced`, `Folded::unpriced`: `--value` a read), `why/asset.rs` split in four (it was 106 lines, the only function over 80 in `report`), `Target::named`'s error tail its own function, a returned flow and a priced flow in the unpriced-count test |
| `0d28182` | | `Context::report`'s claims and lots arms are methods, `Run::histories` says a resumed fold records from its checkpoint, the session test pins which owner is told of what |
| `6e859a7` | | measure: the mutants of the unpriced list, `climutate.py` and `report_mutants.py` (mutants of the pivot and of `why` held to the baseline's outputs through a built CLI) |
| (the commit that carries this section) | | this section, `K7b-baseline-wrong.tsv` |

**Where it differs from the map, and why.**

- **§6, the free path stays: it did not.** The orchestrator allowed the edit in `forecast.rs` and `forecast/trace.rs` that removes
  `Past::Journal` (`Past` is `{ at: &Checkpoint, effects: &[Effect] }`; `coming_due` and `select_due_effects` take slices; three
  `trace.rs` tests start from `plan.start(options).checkpoint()`), and with it `report`, `report_with_sources`, `views`,
  `holdings_at`, `available::view_with_lens`, `journal_ends_by` and `Context::new` went. `forecast/` needed nothing else from
  the path: `holdings_at` had no caller left.
- **§4, no cause index.** `consequences` does one pass over gains, effects and violations for all the flows of a page, ranks each by
  the position of the flow it names and sorts by (rank, kind). That removed the scan per flow without an index, and a second structure
  to keep in step with the fold. `Cause::Time` still names no period (missing 1 stays missing).
- **§4, C is smaller than "one walk".** `Target` and `consequences` are built; "the claims a payment settled" is not (no page shows it).
  `why/*` is 1,177 + 279 lines of which most are sections about their own target (§4 said so: -100 at best). The size C came to is +8
  lines (a `Target` enum with its resolver costs what `identify` and `target_with_lens` cost).
- **§3, the pivot is not in `forecast`.** `forecast/variable.rs::purpose_history` still calls `for_each_counted` and keeps its own
  rows: `forecast/` was out of bounds.
- **§7, the harness counts.** `docs/v5/measure/diff/run.sh` writes 552 outputs, not 468 (K3d's probes), and `allcmds.sh` 796 commands (1,592 files: an output and an error stream each; K7a's "1,592 outputs" is the files).
- **§0.7, the unpriced count is a list asked once, not a function.** `unpriced_flows` scanned every posting for each `--value`
  (2.6 ms of a question's 2.7 ms at 100k, 43 of 44 ms at 1m, measured by taking it out: §13), which made `--value` 2.1 to 2.8 times
  the baseline and not a read. The ends it counts do not depend on whose money is asked of or on the day asked (a price is the book's), so
  `Unpriced::of` finds them once, by the first `--value` a `Folded` is asked for, and `Unpriced::standing` counts those a view
  needs (a handful, not a million postings). `Folded` holds it in a `OnceLock`. Every harness of §12 was run on the build that has it.
  It costs 16 lines and a test that the first asker's owner is not every asker's
  (`what_a_value_could_not_price_does_not_depend_on_whose_books_asked_first`), and two mutants.
- **No `STATUS.md` edit:** the orchestrator writes "STATUS after K7b" (the history of `docs/v5/STATUS.md` says so); the numbers are here.

**Where the lines went** (non-test, `briefs/loc.py`; each row is the commit that made the change):

| step | engine | report | whole tree |
|---|---|---|---|
| baseline `3f17468` | 11,828 | 7,027 | 53,604 |
| A1 the recorder | +218 | 0 | +218 |
| A2 `Balances` for `Snapshots` | 0 | -211 | -211 |
| the free path | 0 | -123 | -123 |
| B the pivot | 0 | +11 | +11 |
| C `Target` and one pass | 0 | +8 | +8 |
| the asset split and `named` | 0 | +18 | +18 |
| the unpriced count asked once of a `Folded` (`Unpriced`, §13) | 0 | +16 | +16 |
| **now** | **12,046** | **6,746** | **53,541** (-63) |

**Honest accounting against the target.** The brief asked for `report` at 5,500 or less and the orchestrator accepted about 6,500. It
is **6,746**: 1,246 over the target and 246 over what was accepted. `report` is -281 from the baseline and the engine +218, so
**the tree shrank by 63 lines**, not by the 1,500 the target asked of `report`. A does not delete a line by itself (the recorder is
`engine`'s 218; what it removed was `report`'s 211): the lines moved to the layer that has the facts, as §6 said they would. 5,500
needed outputs to change (§14 prices each such change; all of them together come to about -315).

Deleted files: none. New non-test files: `engine/histories.rs` (342 lines with its tests, 182 without), `report/balances.rs` (30),
`report/pivot.rs` (102). Deleted functions: 38 definitions in 36 names, of which the longest were `Snapshots::replay` (116 lines),
`why::identify` (44), `why::target_with_lens` (36), `history::final_state` (15), `history::keep` (14), `flow::moved`/`push_rows` (13 each), and
`history::{accumulate, empty, held, pair_index, subtree, split, of, cell, change, column_from, days, storage_shape, add_assign, journal_ends_by}`,
`available::view_with_lens`, `claims::holdings_at`, `Context::new`, `lens::{governs, subject_qty}`, `forecast::effects`,
`why::explain_with_lens`, and six warnings with them (`report` built with 6 dead-code and unused-import warnings and builds with none;
`engine` and `cli` keep their 14 and 1, which are not this lane's).

**Function lengths** (`docs/v5/measure/hist.py`, whole `crates/`; baseline in brackets): 1 to 10 lines 2,058 (2,040), 11 to 20 707
(700), 21 to 40 514 (513), **41 to 80 127 (129)**, **81 to 160 six (eight)**, 161 to 320 none, 321 and over one (the model's `lower_occurrence`,
not this lane's). The two over 80 that went are `Snapshots::replay` (116) and `why/asset::report` (106, split here); the six left are the
engine's `fire::carry`, `post::dispose_sold_asset` and `lots::prepare_part_carry_additions`, and the model's and the CLI's. In `report`
alone: 81 to 160 none (two), 41 to 80 twenty (twenty-two). Functions this lane touched that are over 40: `ledger::finish` 50 (was 48:
28 lines are the one struct literal of `Run`), `balance::view_with_lens` 42 (was 41), `balance::push_place` 56 (was 59). Nothing the
lane wrote is over 40 (the longest are `why/line::consequences`, 40, `asset::part_row`, `Target::named` and `histories::assemble`, about 32 each).

## 12. Where the baseline was wrong

**`balance` at a day the run has not ended on, `--monthly`, `--value`, and `--today D`, said what the flows said, and not what the
fold held.** Each row of [`K7b-baseline-wrong.tsv`](K7b-baseline-wrong.tsv) is one (book, position): the first day on which the baseline's
number and the fold's differ, the two numbers, how many of the days asked differ, and the cause. There are 177 of them, in 21 books (and one note, below).
**In every one the fold is right**: `Ledger::balance` advanced a day at a time (`histories_tests.rs`, and the same oracle over every
example and probe book in `crates/session/tests/histories.rs`) says the fold's number to the unit on every day of every book, and the
baseline's own `balance` (no `--at`) says it too on the last day, so the baseline contradicted itself. Three causes:

| cause | rows | what the baseline did |
|---|---|---|
| a kept occurrence's flows | 94 | `Snapshots::replay` added up the flows the journal wrote; an occurrence a line kept is a promise whose template flows the fold posts and the journal does not hold, so `balance --at` did not count them (and counted the placeholder line's zeros) |
| a claim settled, forgiven, or made by the monitor | 70 | a payment from a party relieves the owner's tab and a write-off takes a claim's parcels out: neither is a flow end, so the replay kept the tab at its full amount (`recognition-writeoff-lines.ax`: `ann 3,300.00 USD` on 2026-02-20 where `claims --at` said 2,300.00 open on the same day) |
| an asset counted twice | 13 | an asset opened by an `opening` line is a flow end and a holding: the replay counted both (`05-family` `crv` and `house`: 2 where the book holds 1) |

The first differing day of each book (the full list is the tsv):

| book | day | position | baseline | fold (right) |
|---|---|---|---|---|
| 02-household | 2026-01-01 | `checking` | 9,200.00 USD | 5,750.00 USD |
| 04-freelancer | 2025-02-12 | `brightwave` | -9,600.00 USD | -6,400.00 USD |
| 05-family | 2024-12-31 | `crv` | 2 | 1 |
| 07-landlord | 2025-02-01 | `lender` | 3,842.30 USD | 5,651.89 USD |
| 11-sam | 2026-01-01 | `checking` | 8,412.55 USD | 5,969.63 USD |
| v4-sketch | 2026-01-01 | `checking` | 8,412.55 USD | 3,619.63 USD |
| explore-v5/01-agency | 2026-01-01 | `card-me` | 0.00 USD | -84.00 USD |
| explore-v5/02-family | 2026-01-01 | `car1` | 2 | 1 |
| explore-v5/03-triplex | 2026-01-01 | `checking` | 23,140.62 USD | 21,073.17 USD |
| explore-v5/04-nomad | 2026-01-02 | `kraken` | 0 | -0.001435 ETH |
| explore-v5/05-budgeter | 2026-01-01 | `card` | -486.20 USD | -531.20 USD |
| explore-v5/06-family-addresses | 2024-12-31 | `crv` | 2 | 1 |
| probes `claim-party-flow`, `claim-recognition`, `claim-writeoff`, `recognition-{accrual,cash,writeoff-lines}`, `split-payment` | e.g. 2026-01-20 | `ann` or `fernhill` | e.g. -1,100 | -800 (`claim-party-flow`) |
| probes `stmts-ok` | 2026-06-01 | `boat` | 2 | 1 |
| probes `promise-no-from` | 2026-02-01 | `checking` | 20,000 | 17,100 |

**One more output changed, and it is the same cause:** `balance --value --at 2024-07-01` on `06-investor` said
"3 flows have no price on their day and are not counted in the value." and now says nothing. The baseline counted, for a column on a
split day, flows that had not happened yet; the count is now flows that stood on a day asked.

**What the differential harness found, as it was run.** `dates.py` (every view that reads a past day, twelve days of every example,
text and `--json`) and `whys.py` (every kind of `why` target of every example, text and JSON, with and without `--for`) leave only the
rows above: every differing output is a `balance` form of a book in the tsv, and no `register`, `claims`, `lots`, `flow` or `why`
output differs. The command fuzz (`fuzzcmds.py`, 300 mutants x 27 commands) found 33 mutants that differ, all `balance --value --json`
on a mutant of `05-family`, and in all of them only the cells `crv` and `house` (2 where the new binary says 1: the asset counted
twice); nothing else on the line moves.

## 13. Measured

The machine is shared (load average 2.4 to 8 while these ran, in each run's header in `scratchpad/k7b`); every comparison alternates the
baseline binary (`3f17468`) and this one and takes the fastest.

**In-process, which is what the 5x of the brief is about** (`crates/session/examples/scrub.rs`: one project opened and folded once,
then the same view asked of forty days spread over the book; the one file is built against both commits; fastest of 5 sweeps at 100k
and of 3 at 1m, over 3 and 2 alternating rounds). Two ways of asking: a `Context` that keeps its plan (the view alone), and a `Session`,
which builds a `Plan` for each answer (K7a):

| a question, over 40 days | 100k baseline | 100k now | | 1m baseline | 1m now | |
|---|---|---|---|---|---|---|
| **the view alone**: `balance --at` | 6.24 ms | 0.053 ms | **117x** | 78.2 ms | 0.48 ms | **164x** |
| `balance --at --value` | 6.99 ms | 0.063 ms | **112x** | 93.4 ms | 0.49 ms | **189x** |
| `balance --monthly --at` | 6.38 ms | 0.42 ms | **15x** | 93.3 ms | 3.03 ms | **31x** |
| `register --to` (a statement, §0.3) | 23 µs | 26 µs | same | 51 µs | 59 µs | same |
| `claims --at` (a re-fold, §5), 8 days | 12.4 ms | 13.5 ms | same | 142 ms | 152 ms | same |
| **through a `Session`**: `balance --at` | 21.4 ms | 14.1 ms | 1.5x | 254 ms | 160 ms | 1.6x |
| `balance --at --value` | 23.6 ms | 15.6 ms | 1.5x | 275 ms | 161 ms | 1.7x |
| `balance --monthly --at` | 22.3 ms | 15.6 ms | 1.4x | 271 ms | 161 ms | 1.7x |
| a `Plan`, built for each answer | 14.7 ms | 14.6 ms | | 173 ms | 161 ms | |

The brief's 5x is true of the view, by 15 to 190 times, and false of the product: through a `Session` it is 1.4 to 1.7 times, because
the plan is 14.6 ms of a 14.1 ms answer (the answers are the plan and a view that costs nothing). §15 says what lifts it. The
`claims --at` row is within the noise of the load, and a little above it (+9% and +7%, in the direction a recorder that a re-fold does
not read would cost); it is the one number here I would re-measure on a quiet machine. **`balance --value` was 2.1x to 2.8x until its
count of unpriced flows stopped being a scan of every posting** (a build with the scan taken out measured 0.06 to 0.1 ms at 100k and 0.54 ms at 1m: the scan
was all of it; §11).

**One-shot, the CLI** (`ab.py`: interleaved, wall and user, fastest and median of 11 runs at 100k and of 5 at 1m). A command folds
whatever it asks, so a view that is free saves the CLI nothing and the recorder's cost is what shows:

| | 100k, baseline / now (fastest) | 1m, baseline / now (fastest, median) |
|---|---|---|
| `check` | 0.411 / 0.410 s | 4.12 / 4.30 s (4.46 / 4.56) |
| `balance --at` | 0.429 / 0.437 s | |
| `balance --monthly` | 0.436 / 0.464 s | 4.51 / 4.56 s (4.54 / 4.60) |
| `balance --value --at` | 0.404 / 0.428 s | 4.38 / 4.39 s (4.63 / 4.61) |
| `register p1-checking`, `flow`, `claims` | 0.408 / 0.417, 0.431 / 0.430, 0.414 / 0.421 s | |

Wall time on this machine is inside the noise (a `check` at 1m was +4.3% fastest and +2.1% median; `check` at 100k was -0.2%). The
instructions are not: **callgrind, `check`: +1.11% at 100k (1,898.8M to 1,919.9M) and +1.10% at 1m (17,567.8M to 17,761.3M)**, the cost
of the recorder (a push for each slot handed out, a drain for each fact, 16 bytes a step, the counting sort at the end). Instructions
of the views that read it: `balance --monthly` -1.3%, `balance --at` -0.8%, `balance --value --at` -0.3% at 100k.

**Memory** (`bench/timeit.py`, peak RSS of `check`, fastest of 3): 100k 80,348 to 80,680 KB (+0.4%), 1m 680,564 to 680,424 KB (-0.02%).
The histories are 1.4 MB at 100k and 11.9 MB at 1m (§2: 759 positions and 113,538 steps; 2,236 positions and 990,112 steps), under
the peak that the model's build and the fold make, which is why the peak does not move.

**Proof.**

- **The oracles.** Exact: a ledger advanced a day at a time against the history (`engine/histories_tests.rs`: six books that settle,
  return, split, pad and claim, and the claim the monitor makes), and the same on every example and probe book
  (`crates/session/tests/histories.rs`: 52 projects (every example and probe book), 11,922 days, 170 positions the replay gets wrong and the classifier names). Naive: the old replay, rebuilt in the test and classified by cause, which is
  where §12's 177 rows come from. Generated: 200 projects mutated from the examples (`fuzzbooks.py`), 127,442 days, 628 positions the replay gets wrong and the classifier names, none unexplained.
- **The tests.** `cargo test --workspace --release --no-fail-fast`: **1,162 passed, 2 failed, 21 ignored**; the two are
  `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` (engine) and
  `a_context_forecast_keeps_historical_and_same_day_obligations_once` (report), which fail identically at `3f17468` (checked in a
  build of it). 19 tests were added, 3 renamed from `snapshots_*` to `balances_*`, and 4 deleted with the code they tested (the shape of
  `Snapshots`' storage, the free-function report twice, the agreement of the final-state shortcut with the replay).
  Clippy on `engine`, `report` and `session`: 130 warnings before, 124 after; five of the new ones are the one lint that fires on
  every function returning `Result<_, Diagnostic>` (136 in the tree) in `tests.rs` helpers and `Target::of`'s chain.
- **The differential harness against the baseline binary**, with this build: `allcmds.sh` 796 commands: **20 differ**, every one
  `balance --at`, `--monthly` or `--value` (text and JSON) of `02-household`, `04-freelancer`, `05-family`, `07-landlord` or
  `11-sam`; `diff/run.sh` 552 outputs: **1 differs** (`stmts-ok.balance`, `boat` 2 for 1); `dates.py` 6,928 commands over 53
  projects: **614 differ**, every one a `balance`, `--value`, `--monthly`, `--for` or `--today` of a book of §12's table and none
  of `claims`, `lots`, `available`, `flow` or `register`; `whys.py` 14,568 commands (every kind of target of 51 projects, text and
  JSON, with and without `--for`): **0 differ**; the 60 goldens and 213 mistakes regenerate with no `git diff`; the command fuzz
  (`fuzzcmds.py`, 700 mutants of the examples x 27 commands, two seeds): 82 differ, all `balance --value --json` of a mutant of
  `05-family` and in each only the cells `crv` and `house` (2 for 1).
- **Mutants of the recorder and of the balance reads** (`histories_mutants.py`, 30): 28 first: all killed, but three of them (`post-does-not-record`,
  `unpriced-counts-the-day-it-ended`, `unpriced-ignores-prices`) only by a test that fails at `3f17468` with no mutant at all, which kills
  nothing: that was the finding of the run, and `mutate.py` now ignores the known failures. Tests were added (a claim the monitor makes;
  a returned flow and a flow in a priced commodity in the unpriced test) and the three re-run: each killed by the new test. When
  the unpriced count became a list (§11) its six mutants were re-run: **five killed, one survived**, `unpriced-ignores-the-owner`
  (the count of a place is not filtered by whose it is), because the session test held only that an answer agrees with itself;
  it now says who is told of what, and fails with the mutant (applied by hand to the tree, run, restored). **No mutant of the 30
  survives.** The whole sweep was not re-run after the last test edits: the three mutants of the first group and the six of the
  second were, each against the tree it was written for.
- **Mutants of the pivot and of `why`, killed by the harness and not by the unit tests** (`report_mutants.py`, 18, each built into
  a CLI and held to the outputs above by `climutate.py`): 12 of the 18 were run (the pivot and the two flow
  tables): **11 killed, every one by `allcmds.sh`** (the flow views of the examples), **1 survived**,
  `pivot-a-zero-amount-moves-its-row` (a purpose whose flows all came to nothing shown as a row of zeros): no example or probe has a
  flow of nothing under a purpose with no other flow. `a_purpose_whose_flows_came_to_nothing_has_no_row` was added and fails with the
  mutant (by hand). **The six mutants of `why` are written and not run** (the order of what a line caused, the quotes of a
  description, an entity that stands for its place, the owner scope of a description): the sweep was stopped for time. `whys.py` holds
  every page byte for byte on 14,568 commands, and how sensitive it is to these six is not measured.


## 14. Candidate output unifications, for K7c to decide

The lane was told not to change bytes to reach 5,500, so none of these is done. Each is a change of what the CLI prints; the lines are
**estimates from the functions that would go, read from the code and not built** (a built one is -20% to +30% of its estimate; the
pivot was estimated at -100 and came to +11 for B). "Goldens" is the files of `tests/golden` (60) that hold the layout; "other" is
what `allcmds.sh` (796 commands), `whys.py` and `dates.py` would show.

| | change | lines | goldens that change | other outputs |
|---|---|---|---|---|
| U1 | `balance --value` without the note "N flows have no price on their day and are not counted in the value." | **-45**: `Unpriced` 27 lines (`of`, `standing`), its place in `Folded` and `Context::report` 6, `Posting::standing` 10, the note 4 (it was -30 before the count became a list: §11 spent 16 lines to make `--value` a read, and this takes them back) | **none** (no golden holds the note; `household-value.txt` has no unpriced flow) | 8 of the 796 commands `allcmds.sh` runs (it writes 1,592 files, an output and an error stream each): `balance --value` of `05-family`, `06-investor`, `08-expat`, `11-sam`, text and JSON |
| U2 | the register of an entity, of an asset and of a contract is the list of the postings that touch it, in the place register's columns; the terms and promises that the contract register mixes in are left to `why contract:`, which has "Terms over time" and "Occurrences" | **about -130**: three row builders (`entity_flow_row` 30, `asset_flow_row` 19, `contract_flow_row` 11) become one, `terms_row` 20, `promise_row` 12, `basis_row` 11, `dated_register` 14, `purpose_cell` 3, less about 20 for the one that stays | **none** (`household-register.txt` is `register checking`, a place; no golden registers an entity, an asset or a contract) | every `register entity:/asset:/contract:`; none in `allcmds.sh` |
| U3 | `why asset:`'s "Flows about it" and `why contract:`'s "Derived flows" are `flows_table`, the table `why ^code`, `why "text"` and a tax line already share (Date, Flow, Amount, State, From; the most recent are shown and the rest counted) | **about -45**: `asset::about` 33 and `contract::derived_section` 25, less the selection they keep | **none** (`household-why-*` are a law, a place and a purpose) | `why asset:` and `why contract:` |
| U4 | `why #purpose`'s Limits, Headroom and Budgets are the rows of `limits` and `budget` | **about -60**: `purpose::budget_section` 52, `budget_limit` 9, `headroom_row` 10, `limits_section` 29, less what they call | **none** (the only golden with these sections is `household-budget.txt`, which stays if `budget` is not touched) | `why #purpose` |
| U5 | `flow --by party` as the periods table with the party under each purpose root, and not a table of its own | **about -40**: `view_by_party_with_lens` 20, `push_root` 29, `label`, `table`, `row`, `net_row`, less the shared rows | **none** (`household-flow.txt` is by period) | `flow --by party`: 22 of the 796 commands (11 books, text and JSON) |
| U6 | one `why` layout: a facts table, then Flows, then Consequences, for every kind of target | **not estimated**: the pages' own sections (composition and parcels of a place, a law's facts, an asset's parts, a contract's terms and occurrences, a purpose's budgets) are information, not layout; what goes is their different headings and column orders | `household-why-place.txt`, `household-why-law.txt`, `household-why-code.txt` | every `why` |

All of U1 to U5 come to about **-320**: `report` would be about 6,425, still 925 over 5,500. **No combination of byte-neutral or output
changes in `report` alone reaches 5,500.** What would: parcels in columns (K3e), so that `claims --at`, `lots --at` and `available --at`
read columns and not a re-fold, which takes `Context::ledger_at`, `holdings`'s plumbing and `available`'s forks (about -100 in `report`,
more in `engine`), and a decision on `json.rs` (450 lines of typed cells for the same tables). Said now so it is not found later.

Two more that are not about output and are not this lane's: (1) `claims::owed_by_you` (44 lines with `settled_codes`) walks
`book.touching` per payable place because a payable is still a plain balance, not a claim place (K3d left it); once it is one, `open`'s
first branch reads it and the second goes. (2) `Standing` (§13): a `Plan` for each answer is the floor of a session's scrubbing, and
`Lens::plan()` is why it cannot be lifted without touching `eval.rs`.

## 15. What is not finished

- **Claims, lots and available at a day still re-fold** (`Context::ledger_at`). The lane's name is "facts out, the views stop
  re-folding": `balance` in every form no longer does (and `summary`); `claims --at`, `lots --at` and `available --at` do. §5 says why
  (state at a day is parcels, not balances) and what removes it (parcels in columns, K3e; or month-end checkpoints a `Session` makes
  on a second past query).
- **A session still builds a `Plan` for each answer** (13.6 ms at 100k, 179 ms at 1m): the floor of in-session scrubbing (§13).
  The lever is a `Standing` (owners, sides and what is known, the part of a `Plan` that `Lens` reads) that a `Plan` derefs to, so that a
  `balance` lens needs no plan. It is not built because `Lens::plan()` is read by forecast, available, flow and register and K6's
  `eval.rs` reads `plan.sides` and `plan.known`; it is a change across both.
- **`Steps::extremes` has no product consumer**: a peak or a low is asked by no view yet (FBAR is a law of a system, not a view). It
  is built, tested against a scan and mutated, and nothing calls it.
- **The deltas (§9) are a design**, as the brief said, and are not built.
- **No cause index and no `Cause::Time { law, period }`** (§11, §4 missing 1 and 2): a period-end effect's page stops at "a period
  ending".
- **`forecast/variable.rs::purpose_history` keeps its own rows** over `for_each_counted` (out of bounds).
- **The first `balance --value` of a run scans every posting once** (2.6 ms at 100k, 43 ms at 1m) for the unpriced note, and every one
  after reads the list it made (§11).
- **A hand-built `Run` in the tests** gets its histories from a naive replay of its journal (`tests.rs::replayed`): an oracle of its own
  kind, the one place a test's `Histories` are not the fold's.

## 16. The three places I am least proud of

1. **The session floor.** The lane made the view of a balance a read (§13: `balance --at` 0.053 ms in a context that keeps its plan,
   117 times the baseline's 6.24 ms at 100k, 164 times at 1m), and a client scrubbing through a `Session` gets **1.5 times** (21.4 to
   14.1 ms), not five: `Session::query` builds a `Plan` for each answer and the plan alone is 14.6 ms. The measurement says exactly
   where the time went; the lever is named and not built, because it crosses K6's `eval.rs`. The 5x of the brief is true of the view and
   false of the product.
2. **A deletes nothing, and the target is missed by 1,246 lines.** The tree is -63, `report` is 6,746 against 5,500, and the one
   thing the recorder was built to serve with a peak or a low (`extremes`) is called by nobody. I said in the map that the lines would
   move to the layer that has the facts; they did, and the 5,500 was not a number this lane could reach without changing bytes (§14
   prices it: about -320 with all of them). The lane's half-built claim is that "the views stop re-folding": three of them still do.
3. **A cache whose correctness is one fact, and the oracle's classifier.** `Folded` now holds a `OnceLock<Unpriced>` that the first
   `balance --value` fills with the flow ends nothing prices, for whoever asked; it is right because `Lens::value` reads the book's
   prices and not whose money the lens is about, and nothing enforces that but a test (`what_a_value_could_not_price_does_not_depend_on_whose_books_asked_first`)
   and a mutant. If `value` ever reads `whose`, the first asker's answer is every asker's. And the replay oracle's `cause()` is a
   classifier I wrote to explain the baseline's differences: it sorts the 177 rows of §12 into three names, and a wrong name would
   be noticed by nothing (the exact oracle, the ledger advanced a day at a time, does not depend on it, so it cannot hide a wrong
   balance; but §12's "cause" column is my reading of each).
