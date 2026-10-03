# K5b map: who still asks the old schedule code, what the fold reads instead, and what a monitor has to say

Written before the first code change of lane K5b, from the code at `456b2dd` (K5a, K3a, K4a and K4b merged), and checked
against what the code does (the last section says how). Paths are in `crates/`; line numbers are those of `456b2dd`.
The vocabulary is K5a's: [`K5a-map.md`](K5a-map.md), `model/src/promise.rs` and `core/src/dues.rs`.

K5a compiled every contract to a term beside the old structure, at the end of `build`, and nothing but its oracle reads it.
So today a contract is held twice (`Contract.terms`/`.standing`, two timelines of eighteen-field `Terms`, and `Book.promises`,
whose `Payment` clones the template, the program and the inputs of the first stretch), and "which due day, which ordinal,
what factor" is answered by the old walkers. This lane moves every reader to the compiled structure, deletes the walkers,
makes the invariant K5a only asserted a type, and builds the monitor.

## 0. What the brief says and what the code does

Eight things in the brief or in K5a's map do not match the code, and each decides something below.

1. **`Terms.anchor` is always `contract.days.first()`, and `Terms.grace`, `due`, `period`, `covers`, `prorated`,
   `escalation`, `shares`, `also`, `rate` and `inputs` are the contract's, not the schedule's.** `lower_terms` (contracts.rs:672)
   reads them from `node.props` and `TermsCx` and writes the same value into the regular and the standing `Terms`. What
   differs between a contract's two schedules is only `every`, `on`, `template`, `program` and `estimate`. K5a asserted the
   anchor (`debug_assert_eq!`, promise.rs:245). This lane does not split `Terms` (that is a lane of its own); it drops what
   is derived (`anchor`, `state`, `change`) and keeps one `Terms` per schedule, once.
2. **Both timelines are always painted alike.** `waive_contract` (statements.rs:593) paints the regular and the standing
   timeline over the same days with the same `Change`. One waiver set per contract is therefore exact, not one per schedule.
3. **The matching happens while the book is being lowered, so it cannot ask a compiled `Promises`.** `lower_occurrence`
   (record.rs:680) needs the schedule kind and the `Terms` of the line it lowers *before* the statement list is finished,
   and `Promises::compile` runs after the last waiver and ending. It needs only the kind, because `Terms` no longer differ by
   day; the due day is stored (`WrittenOccurrence.due`) and read back by the fold. What it asks is a one-contract compile
   of the schedule as lowering has left it (`Promises::alone`): the same arithmetic, over the waivers and endings of the lines
   before it. That keeps K5a's section 7e where it is (a line is matched against the schedule so far); what changes is that the fold
   now *checks* the match against the final schedule (the ordinal of a due day that is not owed is `None`: section 4).
4. **`grace` is read nowhere, and the old reach is not "a cadence" either**: it is the *largest* cadence over every stretch of
   *both* schedules (`nearest_occurrence`, record.rs:1246), so a daily standing `buy` is matched with a monthly payment's
   reach. LANGUAGE §7 says "within its `grace` (default: half a cadence)", of the due day's own schedule. Section 5.
5. **`overdue` is not a `Run` field; it is an iterator in `finish()`** (ledger.rs:311) over the lots of claim places,
   turned into `overdue` warnings by `explain::overdue` (K3c's file). `open_claims` is `Box::default()`, `monitor_complete` is
   `false`, and `Run.promises` holds only occurrences a journal line kept (`kept: Some`, `waived: false`). Nothing the
   reports call "missing" or "late" can happen: `Promise::late` and `why contract`'s "missing" state are written for a
   monitor that does not exist.
6. **`keep_paired` is dead, and so is everything of sync's that reads `dues`.** `World.dues` is `Vec::new()`
   (binding.rs:43), so `Statement.dues`, `Statement.kept`, `parties`, the `(None, Some(due), _)` arm of `World::line` and
   `occurrence` (world.rs) can never run. Making them live would make sync write `01 flat` where it writes a flow today, a
   behaviour change the brief does not list, so the lane deletes them (section 7).
7. **A contract with no `from` starts at `Day::MIN`, and a monitor that walked from its first due day would make 71 million
   missed occurrences.** The monitor starts a stream at the book's own first day (section 6.2): with ordinals by arithmetic that
   is one binary search.
8. **`Promise.waived` is never true, and cannot be**: a waived day is a hole in the schedule, not a due day that is waived
   (K5a map 5, consequence 2). The field stays (it is the API's), always `false`.

## 1. Every reader of the old schedule code, and the call that answers it

| # | where | what it asks | the call that answers it |
|---|---|---|---|
| 1 | `engine/ledger.rs:425-430` `post_written_occurrence` | the ordinal of a kept occurrence: a count of every due day from the contract's first (O(n); 9 s for a contract with no `from`) | `promises.schedule(contract, kind)?.ordinal(due)`, O(log n); `None` is "not part of its schedule" |
| 2 | `engine/occurrence.rs:208` `instantiate_occurrence` | the factor of the day: `amount_on_schedule` | `Sched::factor(book, due)` |
| 3 | `engine/occurrence.rs:289` `materialize_group` | the window the occurrence is recognized over, and the `Overflow` paper-over for `template.day == Day::MIN` (ledger 540, K5a 7h) | `Sched::recognized(due)`: `Days::on(due)` where nothing says otherwise |
| 4 | `engine/occurrence.rs:539` `find` | the terms in force on the due day, and whether they are waived | `contract.terms_of(kind)`, and `Sched::owed(due)` (the error is `OutsideContract` or `Waived`, as `amount_on_schedule` said it) |
| 5 | `engine/occurrence.rs:720,910` `Env::payment`, `loan_payment` | a loan's level payment: a loop of `periods` fixed-point multiplications **per occurrence** (K5a 7g) | `Promises::annuity(contract)`'s `Annuity::payment()`, worked out once at compile |
| 6 | `model/lower/record.rs:680,1239` `lower_occurrence`, `nearest_occurrence` | the schedule kind and due day a line keeps; the `Terms` for its inputs, template and unit | `Promises::alone(contract)` then `Promise::keep(day)`; `contract.terms_of(kind)` |
| 7 | `model/lower/record.rs:1112-1125` `lower_loan_origin` | the template of the loan's first schedule on the day it was made | `contract.terms_of(Regular)` else `Standing` (the terms are the same on every day) |
| 8 | `report/contracts.rs:31,36` `contracts` | the terms today (waived or not), the next due day not kept | `contract.terms`, `contract.waiver_on(today)`; `Promises::schedule(..).days(window)` merged for both kinds, the first not in `run.promises` as kept |
| 9 | `report/register.rs:319` `contract_register` | each stretch of the terms over the contract's life, with the statement that made it | `contract.stretches()`: the waivers' stretches over `days`, each `(Days, Option<&Change>)`, with the one `Terms` |
| 10 | `report/why/contract.rs:53,70` | the same stretches; `ended` | the same; `contract.ended` is unchanged |
| 11 | `report/why/line.rs:215-221` `scoped_codes` | the codes of every template of every stretch of both schedules | the codes of the one `Terms` of each schedule |
| 12 | `report/forecast.rs:375` `contract_forecasts` | the due days in `today+1..until`, numbered from 0 in the window, both schedules merged | `Promises::expected(contract, kind, window)`: `(ordinal, day)` pairs, the schedule's own index, a loan from its first payment to its last. **K5c deletes the driver; until then it calls this** |
| 13 | `report/forecast.rs:461,470` | the cadence and the inputs of the terms on the day | `contract.terms_of(kind)` |
| 14 | `report/forecast/expected.rs:70` `covered_by_contract` | `Contract::covers(template, day)`: does a contract cover a habit flow on a day | the same method, now reading one `Terms` and the waivers (its nearest-stretch search goes: every stretch has the same template). K5c's |
| 15 | `sync/promise.rs`, `world.rs`, `binding.rs` | what `dues` would be, if anything built them | nothing builds them: deleted (section 7) |
| 16 | `docs/v5/measure/promises/` (K5a's dump), `internals/main.rs` (K4b's) | the oracle: every question, and `Contract::occurrences` for the forecast's way | the oracle asks the compiled structure and the fold (section 9) |
| 17 | `model/tests/native_records.rs`, `tests/promises.rs`, `tests/tabs.rs`; `engine/src/tests.rs:196`, `report/forecast/expected.rs:264,289` | `contract.terms.as_ref().unwrap().at(day)`, `due_days`, a `Contract` literal | the one `Terms`; `Promises`; the literal with `waived` |

Nothing in `cli` or `systems` reads the old code.

## 2. What reads `Terms` at a day, what each needs, and `Contract.terms`, `.standing`, `.ended`

`Timeline::at` and `within` are called on a contract's `Terms` in exactly these places, and in none of them does the answer
depend on the day except through `state` and `change`:

| reader | what it takes from `terms.at(day)` | what it needs instead |
|---|---|---|
| `Contract::occurrences`, `amount_on_schedule`, `recognition_on_schedule`, `terms_on_schedule` (book.rs) | `state`, `every`, `on`, `anchor`, `escalation`, `prorated`, `period`, `covers` | deleted; the compiled `Schedule` and `Reckoning` hold them |
| `nearest_occurrence` (record.rs:1247) | every `every`, for the radius | `Schedule::reach` |
| `lower_occurrence` (record.rs:680) | `inputs`, `template`, `template.first()` for the unit | `terms_of(kind)` |
| `lower_loan_origin` (record.rs:1112) | `template` on `loan.on` | `terms_of` |
| `waive_contract` (statements.rs:593) | clones `terms.at(day)` to set `state` and `change` | `contract.waived.paint(days, Some(change))` |
| `Promises::every`, `skips_of`, `reach` (promise.rs) | `state`, `change` (the alike assert), `every`, `on`, `anchor`, `grace`, `due`, `template`, `program`, `inputs`, `rate` | the declared `Terms` and `contract.waived` |
| `contracts` view (contracts.rs:31) | the terms today and whether they are waived | `terms`, `waiver_on(today)` |
| `contract_register`, `why contract` | each stretch: days, state, `change`, the template | `stretches()` |
| `scoped_codes` (why/line.rs:215) | every stretch's template | the one `Terms` |
| `contract_forecasts` (forecast.rs:461,470) | `every`, `inputs` | `terms_of(kind)` |
| `Contract::covers` (book.rs:690) | the templates of the stretches, the nearest active one across a waiver | `terms_of`, `waiver_on` |
| the tests | `.at(day).template/.program/.state/.change/.grace/.anchor` | the same fields of the one `Terms`; `waiver_on(day)` for state and change |

`Contract.standing` is read where `terms` is, and by `lower_occurrence`/`nearest_occurrence`/`covers`/the forecast/`scoped_codes`;
it is the second schedule of the same contract, and stays as a field. `Contract.ended` (`DATE NAME ends`, the statement's
`Loc`) is read by `why contract` (a note) and the oracle; `contract.days` (the cut itself) is read by everything. Both stay.

## 3. The type

```rust
pub struct Contract {
    // … party, owner, purpose, description, area …
    pub days: Days,                              // from … until, cut by `ends`
    pub terms: Option<Terms>,                    // the regular schedule, as declared, once
    pub standing: Option<Terms>,                 // a standing `buy`, as declared, once
    pub waived: Timeline<Option<Change>>,        // None: promised; Some: the statement that waived it
    // … buys, deposit, loan, matching, ended …
}
```

`Terms` loses `state`, `anchor` and `change`; `TermsState` and `Terms::is_waived` go. A contract can no longer hold a
stretch whose template differs from the declaration's, because it holds no stretches of terms: K5a's `alike` and the two
`debug_assert!`s of `Promises::every` are gone with the thing they asserted. The waivers stay a `Timeline` of the `Change`
that made each stretch (not a bare `DaySet`) because `why contract` and the register print the statement of each stretch, and
adjacent waivers by different statements are different stretches there and one hole in the schedule: `waived_days()` is the
`DaySet`, and the compile reads that.

`Payment` goes: `Term::Pay(ScheduleKind)` says that each due day of this stream pays the template of the contract's
`terms_of(kind)`; the template, the program and the inputs live in that one place. `Deadline` is not cloned either:
`Term::Due { after, body }` carries the span, and the `else` item stays in `Terms.due`, where K5c finds it. `blame` leaves the
term: it is a function of the template's side (`Contract::owes`), needed by a diagnostic and by K5c's claim, and stored by neither.

## 4. The ordinal and the final schedule

A kept occurrence's identity is `RuntimeTxn::ContractOccurrence { contract, schedule, day, ordinal, source }`. Its ordinal is
now `Sched::ordinal(due)`: the index among the days that are *owed*, counted from the contract's first day, the same in the
journal and in a forecast (K5a 7c). A written occurrence whose `due` is not owed in the final schedule (a later statement waived
it, or ended the contract before it: `promise-lowering-order.ax`) has no ordinal, and the fold says so
(`contract-occurrence-source`: "not part of its contract schedule") where it said `contract-occurrence-materialization ... Waived`.
That is K5a 7e's one visible change; it is listed with the outputs in section 11.

## 5. `grace`, as LANGUAGE §7 says

"Each due day is kept by the nearest occurrence within its `grace` (default: half a cadence)." Today: `grace` is lowered and
read by nothing, and the reach is the contract's longest cadence. After: the reach of a schedule is its `grace` if the
contract has one, else half its own cadence. Both measure a span the way the radius did, **a month as 31 days**, and half is
rounded down (a distance is whole days: monthly reaches 15, weekly 3, daily 0, `twice monthly` 15). The reach is a field of
the compiled `Schedule`, per schedule, so a regular payment and a standing `buy` are matched by their own. Matching stays the
line's choice of the nearest owed day (the earlier on a tie), as the old code and K5a's `keep` have it.

It changes: every line that was written more than a reach from its due day (kept before because the radius was a whole cadence);
every `grace` that was ignored; every contract whose two schedules had different cadences. The books it changes are listed in
section 11, with every golden, mistake and test output.

## 6. The monitor

### 6.1 What there is, and what it makes

| | today | after |
|---|---|---|
| `Run.promises` | the kept occurrences, in journal order | the kept, as now, and the **missed**: `kept: None`, in the order the fold found them |
| `Run.open_claims` | always empty | one per open parcel of a claim place (the lots `overdue` already walks) |
| `Run.monitor_complete` | `false` | `true` |
| `overdue` (diagnostic) | an iterator over claim lots, one warning per overdue lot | the same, and one `missed-occurrence` warning per contract that has missed due days |
| `sync-monitor-incomplete` | warns for every book with a contract | cannot fire: deleted with `PlanOutcome.incomplete` |

### 6.2 What it does

A **missed** due day is one that no line kept and that can no longer be kept: its day, plus its schedule's reach, has passed
(LANGUAGE §7: "past its grace with no occurrence"). A stream's state is a `Residual`: the due day it waits for, its ordinal,
and for a loan what is still owed. The fold keeps one per stream in `World` (a dense `Vec`, cloned with the world, hashed into
a checkpoint's digest), and a min-heap of `(day it is missed, stream)`:

- **settle**: a written occurrence of ordinal `k` of a stream settles it up to `k`: the due days before `k` that were never kept
  are missed now (a later line cannot keep them: the nearest due day is monotone in the line's day and the fold takes lines in
  day order), and the cursor moves to `k + 1`.
- **miss**: the heap's earliest entry that is still the stream's current one, on the first day it is past its reach, makes a
  `Promise` with `kept: None` and advances the residual (a loan's `open` moves by `Annuity::pay`; a loan that is paid off is done).
- **start**: a stream starts at the first due day on or after **the book's first day** (the earliest of its flows, openings,
  assertions, written occurrences, settlements and splits), with the ordinal `Sched::before(day)` gives; a due day before the book
  begins is not the book's to miss, and a contract with no `from` does not walk from `Day::MIN`.

The monitor never touches holdings in this lane: a miss is a record, not a posting (K5c posts the claim, once K3c's parcels
are in). It is merged with the timeline by day in the one loop that consumes facts (`Ledger::advance_through`); a miss on a
day comes before that day's facts, because a line dated that day is already out of reach. A forecast ledger (`resume`, `start`)
runs the same loop to its horizon, so it records the misses of the window it is driven over in a record nothing reads: a
24-byte `Promise` each, never a diagnostic (diagnostics are made by `finish()`, which no forecast ledger calls). K5c turns that
window into the fold's own.

**Heap or sorted pass?** A heap. The next miss changes only when a residual moves (a settle or a miss), and `advance_through`
asks for it before every fact, so the question is a peek, O(1); a scan over the streams would be O(streams) per fact, and a
sorted vector cannot be moved cheaply. An entry whose stream has moved is stale and is dropped when it reaches the top (lazy
deletion), which keeps the update one push.

### 6.3 What the deadline is for

`due SPAN else ITEM` gives a deadline `after` its due day and what is added when it passes. LANGUAGE separates three things:
kept (within grace), missing (past grace: a claim) and the deadline (when `else` takes effect). The monitor's event is the
second. `Term::Due { after, body }` and `Residual::deadline` carry the third for K5c, which posts `else` and the claim; in
this lane nothing but the oracle and a test reads them, and the map says so (section 8) rather than pretend.

### 6.4 The diagnostic

One warning per contract with missed due days, at the contract's declaration, because a book whose journal ends a month before
today can miss forty of them and forty warnings would be the book's whole output: "`mortgage` was due 4 times with nothing
written within its reach: 2026-01-01, 2026-02-01, 2026-03-01, 2026-04-01", the latest first in the note, and the fix as an edit
(`2026-04-01 mortgage`, or `2026-04-01 mortgage waived`). `check` prints warnings, so an example whose contracts are not
written down to `today` gains them (section 11).

## 7. sync

`sync/promise.rs` (`Due`, `keep`, `keep_paired`, `keep_by`, three tests), `World.dues`, `Statement.dues`, `Statement.kept`,
`parties`, the `(None, Some(due), _)` arm of `World::line`, `occurrence()` and the `kept` argument of `exchanges` read a list
nothing makes. Sync has no use for `Promises` today (it would need the amount a contract moves on an account, which only
the fold computes), and writing `01 flat` for a bank record is a feature, not a deletion. The module and everything that only it
fed are deleted; the three tests of `keep` go with `keep` (the brief: "or, if it has no use for any, say so and delete the
module"). `PlanOutcome.incomplete`, `monitor_gaps` and the assertion of `e2e.rs:192` that the field is empty go with the
warning that can no longer fire.

## 8. Written and read by nothing after this lane

`Contract.area`, `.deposit`, `.deposit_holding`, `.matching` (never set), `.doc`; `Loan.asset`, `.resets`, `.prepay` and the type
`Reset`; `Terms.estimate`, `.shares`, `.also`; `Deadline.otherwise` (read by K5c) and `Term::Due`/`Residual::deadline` (read by
a test and the oracle). K5d owns the loan's and the deposit's; the grammar stays.

## 9. The plan, and what proves each step

| step | what | proof |
|---|---|---|
| 0 | this map | |
| 1 | the oracle asks the compiled structure and the fold, against the reference, with the reference rewritten to the spec (grace, per-schedule reach, the monitor); a **frozen dump of the old code** is made from the baseline | the verdict on 1,500 projects; the frozen dump's lines compared by kind |
| 2 | `Contract` holds its terms once and its waivers as a timeline; `Promises` reads them; `Payment`, `Deadline` clones, `alike`, the asserts go | every test, the goldens byte for byte, the oracle |
| 3 | the fold reads `Promises` (ordinal, factor, recognition, payment, terms); `loan_payment`, the count, `Contract::occurrences` and its helpers go | the oracle, `fuzz.py ... diff`, K4b's splits oracle, goldens, mistakes |
| 4 | lowering and the reports read it; `nearest_occurrence`, `ForecastFeature` and the dead `ForecastError` go; `grace` is read | the goldens and tests whose output `grace` changes, listed |
| 5 | the monitor; sync's dead path | mutants of the monitor, killed or argued; the three known failures |

## 10. How this map was checked

- Readers: `grep` of every name of the old code over `crates/` (tests included) and over `docs/v5/measure/`, at `456b2dd`.
- Section 0.1: `lower_terms` read in full, and the oracle's `equal-stretches` tally (K5a: true for 1,500 projects).
- Section 0.5: `finish()`, `Run` and `OpenClaim` read; `grep` of `open_claims`, `monitor_complete`, `overdue`.
- Baseline behaviour on books (`target/release/axiom` of `456b2dd`, kept as the baseline binary): `diff/cases2/promise-grace-ignored.ax`
  (`contracts`: "1 occurrences, 9 days" late, a line nine days late keeps its due day under `grace 3d`), `promise-no-from.ax`.

## 11. What was built, where it departs from sections 0 to 10, and every output that changed

Written after the code, from the code at the tip of the lane and the runs listed in section 12. Sections 0 to 10 stay as they
were written; this section is what is different from them.

### 11.1 What was built

| step | commit subject | what it did |
|---|---|---|
| 0 | `docs/v5/lanes: K5b-map ...` | sections 0 to 10 |
| 2, 3, 4 | `model, engine, report, sync: a contract holds its terms once, and the fold, the lowering and the reports read the compiled promise` | `Contract.terms`/`.standing` are one `Terms` each, `waived` a `Timeline<Option<Change>>`; `Promises` reads them; the ordinal, the factor, the recognition window, the payment, the matching and the reports ask `Promises`; `Contract::occurrences` and its helpers, `ContractOccurrences`, `loan_payment`, the ordinal count, `nearest_occurrence`, `ForecastFeature` and four `ForecastError` variants are deleted; `grace` is read; sync's `promise.rs` and `World.dues` are deleted |
| 5 | `engine: the monitor walks a residual per stream beside the journal, and a due day nothing kept is missed` | `engine/src/monitor.rs`; `World.monitor`; `Run.open_claims` and `monitor_complete: true`; the `missed-occurrence` warning; `sync-monitor-incomplete` deleted |
| 1 | `core, docs/v5/measure: a walked schedule with no first day begins in 1970 ...` | `Dues` for a walked schedule with no first day; the oracle against the reference, the rebuilt old rule and the frozen dump |
| | `tests: goldens after K5b` | the seven goldens of section 11.3 |
| | the commits after it | `Contract::covers` inlined (the forecast's per-flow loop: 132M instructions out of line at 100k flows); `kept_by` (one function says what a line keeps and the terms it is made from, so the `expect` in `lower_occurrence` is gone and the function is 23 lines shorter); the monitor's two `expect`s in non-test code gone; a test for the order in which misses are recorded; the verdict's causes (section 12) |

The order of the plan changed once: the oracle (step 1) was finished after the code it judges (steps 2 to 5), because the first
reference had to be rewritten to the spec (grace, per-schedule reach) before it could say anything about the second.

### 11.2 Where it departs from the map

1. **`blame` did not leave the term.** `Term::Due` carries `blame: Blame` (`Party` or `Owner`, one byte, a function of the
   template's side, `Blame::of(&contract)` says who). Section 3 had it leave; K5a's test asserts it and K5c's claim reads it, and a
   byte beside a `Span` keeps the term at 16 bytes. The template, the program and the inputs are held once, as section 3 says.
2. **`Promises::alone(&Contract)`** compiles one contract by itself, for the match the lowering does while the book is still
   being written (section 0.3 said "a one-contract compile"; this is its name). The fold checks the match against the final
   schedule (`Sched::ordinal` is `None` for a day that is not owed), so the lowering's answer is provisional and the fold's is not.
   `Promises::expected(contract, kind, window)` and `Promises::loan(contract)` are the two calls the forecast and
   `instantiate_occurrence` make, so that neither knows what an `Annuity` or a `Sched` is.
3. **A walked schedule with no first day counts from 1970-01-01** (`Dues::new`). Section 0.7 solved the monitor's walk from
   `Day::MIN` (start at the book's first day); it did not solve `Dues::walked`, which collects its days from the anchor, and
   a `weekly on 15` or `every 45d` with no `from` has no first day: the counting of its ordinals, its residual and the
   monitor's first day collected four billion of them (the oracle's out-of-memory, then `promise-no-from.ax` at 8.9 s).
   The phase of such a schedule used to be that of `Day::MIN`, which means nothing; it is now that of day zero, which also means
   nothing, and is at least a day. It changes the due days of six of the oracle's 1,500 projects (`first` differs, section 12),
   of no example and of no golden. `Dues::days` also clips a window that begins before the anchor.
4. **The reach of a month is 31 days and half is rounded down** (section 5 said so); the reach is a field of the compiled
   `Schedule`, and a `twice monthly` schedule has 15.
5. **The monitor does not read `Term::Due`'s `after`.** Section 6.3 said so; it is repeated here because nothing reads that
   field but the oracle and one test, and K5c will have to read it or delete it.
6. **The matching is still provisional in one way section 0.3 hoped to remove**: a line is matched against the schedule as the
   statements before it left it, not the final one. A later waiver can make a line that was matched to a due day that is no
   longer owed; the fold then says `contract-occurrence-source` (11.3). The brief's "the fold reads the promise" holds for
   everything the fold computes; what the lowering *chooses* is still decided one statement at a time.

### 11.3 Every output that changed, and why

**Goldens** (`git diff 456b2dd -- tests/`, seven files, all of them `05-family` and `07-landlord`; no mistakes golden, no other
example):

* `05-family-check.txt`: two `warning[missed-occurrence]` (`mortgage-payment`, due 2026-01-01, 02-01, 03-01; `car-payment`, due
  2026-01-05, 02-05, 03-05) and the footer's `2 warnings`. The journal of the example ends on 2025-12-31 and its contracts run on, so,
  on `--today 2026-04-16`, three due days of each have passed their reach with nothing written. **Not the grace.**
* `07-landlord-{check,balance,available,limits,claims,tax}.txt`: two of the changes are the monitor: `home-loan` (the house was
  sold on 2025-12-29 and the loan paid off, and the contract has no `until`: due 2026-01-01, 02-01, 03-01) and `manager-fee`
  (nothing on 2025-10-05: the book has no October line). The rest is **the grace**: `2025-12-29 manager-fee 200.00 USD` is a second
  fee line 24 days after the due day 2025-12-05 (the contract ends on 2025-12-31, so it has no due day on 2026-01-05); the old reach was
  the contract's longest cadence, a month, 31 days, so it kept 12-05 a second time; LANGUAGE §7's reach is half of the cadence of the schedule's
  own, 15 days, so it keeps nothing: `contract-occurrence-date` (the "(7 more)" of the paycheck error is "(8 more)"), the line
  is not posted, the 12-31 assertion on `rental-bank` fails by 200.00 USD (a new `error[assertion]`: 46 errors where there were 44), and
  the reports move with the 200.00 USD that did not leave `rental-bank`: `balance` (`rental-bank` 154.60 to 354.60 USD), `available`
  (money in hand 95,045.20 to 95,245.20 USD) and `tax` (`rental-net` 13,184.52 to 13,384.52 USD, `agi` 50,915.10 to 51,115.10 USD);
  `limits` and `claims` gain only the diagnostics.
  **The book was not edited.** To keep the example as it was, the contract would say `grace 30d`, or the line would be dated 2025-12-05 or
  moved to a different contract. That is the orchestrator's decision.

**Tests whose assertion changed** (none deleted without its replacement):

* `model/tests/native_records.rs`: the book of the input-binding test wrote `2026-02-05 flat` under `grace 3d`, four days from the due day
  `02-01`, which nothing read; it is `2026-02-04 flat` now (inside the grace) and its assertion on the day the occurrence lands follows.
  The rest of that file is ported to one `Terms` and `waiver_on`, with the same assertions.
* `model/tests/promises.rs`: `a_grace_is_carried_though_nothing_reads_it` is `the_reach_of_a_schedule_is_its_grace_or_half_its_own_cadence`;
  the assertion that "the old walk loses it" is gone with the old walk; three new tests
  (`a_line_beyond_the_reach_of_its_due_day_keeps_nothing`, `a_loan_expects_its_payments_and_no_more`,
  `a_contract_alone_is_asked_as_it_stands`).
* `engine` and `report`: `monitor_complete` is `true` (the brief says so); `forecast_materializes_contracts_even_when_the_run_monitor_is_incomplete`
  is `forecast_materializes_contracts_beside_a_run_whose_monitor_is_complete`.
* `sync`: the three tests of `keep` (`a_record_within_half_a_cadence_keeps_the_nearest_occurrence`, `each_occurrence_is_kept_once_and_only_by_money_going_the_right_way`,
  `a_different_amount_still_keeps_it_and_a_stranger_does_not`) go with `sync/promise.rs` (the brief allows it); `e2e.rs` no longer asserts
  the `sync-monitor-incomplete` field that cannot be set.
* `native_loan_forecast_stops_after_the_typed_principal_is_repaid` (K5c's name for it in the known failures) **now passes**: a loan expects
  its payments and no more.
* Failing as before and not mine: `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot` (K3c),
  `a_context_forecast_keeps_historical_and_same_day_obligations_once` (K5c).

**The K0a harness** (`docs/v5/measure/diff/compare.sh`, 384 outputs): 68 differ and 316 do not. 28 differ only by the new
`missed-occurrence` warning (and its line in the footer) in books that declare a contract and write nothing; 17 are the `contracts` view
(a `Late` cell: "6 occurrences, 629 days", and, for `promise-last-after-waiver`, the next due day 2026-06-30 where the old walk, which lost
that day, said 2026-07-31); 11 are `promise-grace-ignored` (a line 9 days from its due day under `grace 3d` is now an error: the
`grace` is read), 11 are `promise-lowering-order` (a line matched to a due day that a later waiver took out: `contract-occurrence-source`,
"this occurrence is not part of its contract schedule", where it was `Forecast(Waived(Day(20513)))`: K5a 7e's one visible change) and
`stmts-ok.check` (three warnings). Nothing else.

**The oracle's old-rule differences** (section 12): the reach (35,287 probe days of 339,756), a day the old walk lost (880 probe
days, 404 due windows, 183 ordinals), a day it found twice or out of order (904 due windows, 1,398 ordinals), and a walk with no first
day (not compared). Recognition: 359 answers of `Err(Overflow)` for a contract with no first day are now `Ok`. Factors and payments: **no
difference in 1,500 projects**. `diagnostics`: `contract-occurrence-date` +199, `ambiguous-contract-occurrence` -6 (the reach).

**The fuzz** (`fuzz.py OLD NEW examples 2 1000 diff missed-occurrence`): no panic in either build; 153 mutants differ, 151 from `07-landlord`
(the grace line above, mutated around) and 2 from `04-freelancer` (one more `missed-occurrence`, hidden behind the display limit and
so not removed by the filter, in the count of "N more diagnostics not shown").

### 11.4 Written and read by nothing after the lane

Section 8's list, plus: `Term::Due`'s `after` and `blame` and `Residual::deadline` (K5c); `Promise.waived` (always `false`, section 0.8).

## 12. How it was checked

Every number here is from the code at the tip of the lane. The scratch files are not in the repository; the commands are.

**The oracle** (`docs/v5/measure/contracts.py`, `promises/`): `contracts.py gen DIR 1500 7` writes 1,500 projects, 2,329
contracts of 77 forms (`contracts.py cover`).

* `check` (the fold against the reference, `calendar::due` walked from the first day, days taken once, waived ones out, the nearest
  within the reach LANGUAGE §7 says): **0 failures**. It asked 45,181 due windows, 30,926 ordinals, 339,756 probe days, every one of
  the 1,855 occurrences the journals kept (line and ordinal) and the missed set of 2,492 streams (a missed day is owed, after the book
  began, kept by no line, and past its reach at the horizon or before a day that was kept; no other is missed), a residual walked
  against the schedule at 85,858 points (728 ends, 2,467 loan, 3,213 open, 121 repaid), 5,551,560 factors of a day that is not owed
  and 3,210,975 factors of one.
* The old rule is rebuilt from `calendar::due` as the old walkers asked it, and `reproduce` holds the rebuild to the frozen dump of the
  tree before the lane: **1,500 projects, 0 kinds of line not reproduced**. The verdict then gives every answer the fold gives
  otherwise a cause, and fails on one with none: probe days that keep a different line than the old rule did: the reach 35,287, a due
  day the old walk lost 880, no first day 4,048 (not compared), the same 299,541; due windows: found twice or out of order 904, lost 404,
  no first day 621, the same 43,252; ordinals: twice or out of order 1,398, lost 183, the same 29,345. **0 unexplained.** (An earlier
  python `classify` read only the windows the dump asked and left 637 days unexplained; it is deleted.)
* The frozen dump against the new one (`compare`): **factor** and **payment** lines differ in **no** project; `recog` in 156 (359
  answers, all `Err(Overflow)` for a contract with no first day, now `Ok`); `first` in 6 (the walked schedule with no first day, 11.2);
  `due`, `keep`, `ordinal` as above; `diagnostics` in 151 (`contract-occurrence-date` +199, `ambiguous-contract-occurrence` -6).

**Mutation** (`contracts.py mutate`, 55 mutants of `core/dues.rs`, `model/promise.rs` and `promise/*`, `model/book.rs`,
`engine/monitor.rs`, `ledger.rs`, `timeline.rs`; each built into the dump and caught by the verdict, by a factor, recognition window
or payment that is not the fold's (the pinned lines of `dump.new.txt`) or by the crate's own tests):
**55 mutants: 48 killed by the verdict, 5 by the pinned lines, 2 by the tests, 0 survived.** The 12 of the monitor (a miss a day early or
late, a stale heap entry, a kept day missed or waited for again, a line that settles the stream after its own or nothing, a stream
never missed, nothing missed after the last fact, a book that begins on its last day): 11 by the verdict and one, the order in which
misses are recorded (a miss found after the facts of the day it falls on instead of before them), by `a_miss_is_recorded_before_the_facts_of_a_later_day_and_not_after_them`:
the corpus cannot tell the order of two records, and the first sweep said so. The five by the pinned lines are the escalation
(a rise of 3% a fall, a year early), the squaring of a power, a proration share a day too long and `for last month` as the day: the
factors and windows no second implementation judges any more. The other test-killed mutant is the day after a hole (`the day after a hole is inside it`):
a boundary the 1,500 projects do not reach (it is the count of due days up to the last day of a hole, which is a day nothing is owed on), caught by `tests/promises.rs`. The sweep also found one **equivalent** mutant, which is
now deleted code: `Annuity::owed_after` ended with `if payments >= periods { 0 } else { open }`, and the last payment of a loan
is what is left, so the loop already ends at zero. Two mutants named code that has since changed shape and were rewritten; one's
replacement did not build and was fixed (`a grace is not read`, `a kept line settles nothing`).

**Other differential runs**, against the baseline binary built from `456b2dd`:

| run | result |
|---|---|
| `fuzz.py OLD NEW examples SEED 1000 diff missed-occurrence`, seeds 2 and 3 | no panic in either build; 153 and 149 mutants differ: 151 and 148 from `07-landlord` (its grace line, mutated around), 2 and 1 from `04-freelancer` (one more `missed-occurrence`, hidden behind "N more diagnostics not shown", which the filter cannot see) |
| `splits.py run`, 800 projects of statements, 21,147 commands | 0 differ |
| `splits.py run`, 600 projects of promises, 18,852 commands | `balance`, `flow`, `register`, `lots`, `gains`, `claims` and `forecast` identical in all; `check`, `contracts` (and `--json`) and `why contract` differ only by the monitor's output (the warning, the `Late` cell, the late rows) |
| `splits.py equiv`, 300 splits | 0 not the plain transfers they say |
| `diff/compare.sh` (K0a, 384 outputs) | 68 differ, all as listed in 11.3 |
| `sh tests/golden.sh` | the seven files of 11.3 and no other; `sh tests/mistakes/run.sh` exit 0, `git diff tests/mistakes` empty |
| `cargo test --workspace --release --no-fail-fast` | 993 pass, 2 fail (the two of 11.3, not mine), 19 ignored; the baseline was 981 pass, 3 fail, 19 ignored |

**Size** (`python3 briefs/loc.py .`, non-blank non-comment lines outside test modules), before to after: `cli` 2,507 to 2,507,
`core` 3,461 to 3,465, `engine` 10,846 to 10,994, `model` 17,916 to 17,637, `report` 7,066 to 7,075, `sync` 4,368 to 4,277,
`syntax` 5,589, `systems` 14; **total 51,767 to 51,558 (-209)**, against the brief's target of about -900. `git diff --shortstat`
over `crates/`: 1,036 insertions and 1,042 deletions, of which `monitor.rs` is 177 lines of code (238 with its comments) and 185 of tests.
The deletions are `book.rs` (324 lines of the walkers, the helpers and the three types), `promise.rs` and `schedule.rs` (the `Payment`,
the clone, `alike`), `sync/promise.rs` (107, the whole module), `occurrence.rs` (87), `record.rs` (74: `nearest_occurrence`), `world.rs`,
`planner.rs`, `commands.rs`. The function-length histogram (`hist.py crates`) before to after: 1-10 lines 1,895 to 1,906, 11-20
663 to 662, 21-40 481 to 480, 41-80 137 to 135, 81-160 11 to 11, over 320: 1 (`lower_occurrence`, 426 lines) to 1 (403).

**Time** (`axiom check` and `forecast`, baseline binary against new, the fastest of three alternated runs on an otherwise quiet
machine; instructions from callgrind, which do not move with the machine's load):

| | baseline | new |
|---|---|---|
| `check diff/cases2/promise-no-from.ax` | 8.911 s, 79,147M instructions | **0.008 s, 6.8M instructions** |
| `check` bench 100k | 0.416 s, 1,837.5M | 0.416 s, 1,842.0M (+0.25%) |
| `forecast` bench 100k | 0.466 s, 2,090.5M | 0.467 s, 2,089.3M |
| `check` bench 1m | 4.638 s, 16,951.8M | 4.450 s, 17,011.9M (+0.35%) |
| `forecast` bench 1m | 5.389 s, 22,070.6M | 5.490 s, 21,697.0M |

The first forecast of the lane cost 18% more instructions at 100k: `Contract::covers`, asked 4.6M times, was out of line. It is inlined.

## 13. What is not finished, and the three places I am least proud of

**Not finished, said plainly.**

* The matching is still done at lowering time on a one-contract compile (11.2, 6). `lower_occurrence` is 403 lines and `post_written_occurrence`
  121; both are older than the lane and I did not take them apart. `finish` (45) and `Plan::new` (41) are over the 40 lines by what
  the lane added.
* The monitor records a miss and warns; it posts no claim (K5c) and reads no deadline: `Term::Due`'s `after` and `Residual::deadline` are read by
  a test and the oracle only.
* Loans: no interest or principal as flows, no resets, no prepay (K5d). The forecast of a loan stops after its payments.
* The old rule is not rebuilt for a walked schedule with no first day (4,048 probe days, 621 windows): a walk from the beginning of time is four
  billion days. What they do now is compared to the reference only.
* The `Late` cell of `contracts` and the `why contract` rows are the monitor's records read by code written for a monitor that did not exist;
  they work and are tested by the golden and the splits runs, not by a unit test of their own.

**The three places I am least proud of.**

1. **The grace changes two examples and I did not decide it.** `07-landlord` loses a line (`2025-12-29 manager-fee 200.00 USD`) and gains an
   assertion error, and `07-landlord` and `05-family` gain warnings. LANGUAGE §7 says half a cadence, the old code said a whole one; I read the
   language and listed the books, and left them alone as told. The right fix is a decision about the example (`grace 30d`, or another date), not about the code.
2. **The 1970 anchor.** A walked schedule with no `from` counts from day zero because counting from `Day::MIN` collects four billion days. The
   phase changes (six of 1,500 projects), the old one meant nothing and the new one means nothing too; it is a number picked to make a
   walk finite, written down in `Dues::new` and in 11.2, and the oracle does not compare it.
3. **The monitor starts at the book's first fact.** A due day before it is not the book's to miss, which is right for a contract with no `from`
   and a heuristic for one that began before the journal did: it could say nothing about a due day the book never saw. It is also why
   `home-loan` of `07-landlord` warns three times for 2026 after the house was sold: the contract has no end. That is a book that needs an
   `until`, and a monitor that cannot tell it from a book that forgot a payment.
