# K5a map: what a promise's schedule is, who asks which occurrence is due, and what a term needs to say

Written before the first code change of lane K5a, from the code at `5649739` (the merge of K3a), and checked against what
the code does (the last section says how). Paths are in `crates/`; line numbers are those of `5649739`.

A **promise** is a contract's flows to come. Today it is `Contract { terms: Option<Timeline<Terms>>, standing:
Option<Timeline<Terms>>, buys, deposit, deposit_holding, loan, matching, ended, .. }`: two timelines of `Terms` (the regular
schedule and a standing `buy`), each stretch of a timeline a whole `Terms` of eighteen fields, and five more features beside
them. "Which occurrence is due, and which one is this line keeping?" is asked of that structure by four pieces of code.

## 0. What the brief says and what the code does

Nine things in the brief do not match the code, and each decides something below.

1. **The fourth implementation does not run.** `sync/promise.rs::keep_paired` is read by `World::statement`
   (sync/world.rs:322) over `World.dues`, and `World.dues` is `Vec::new()` in the only constructor (sync/binding.rs:43: "this
   adapter never reconstructs them"). Only its own tests make a `Due`. It has a window of its own (half a cadence), a different
   algorithm (a global greedy match by distance, then record, then due) and no reader in the product. K5b deletes it; this
   lane does not compare to it (section 3).
2. **The tie-break is not "half a cadence".** `nearest_occurrence` (model/lower/record.rs:1186) looks within one **full**
   cadence of the line (`months × 31 + days`, the largest over every stretch of both schedules of the contract) and takes
   the nearest due day, the earlier of two equally near. `Terms.grace` is lowered (contracts.rs:702) and **read by nothing**:
   `grace 3d` makes no difference (`diff/cases2/promise-grace-ignored.ax`: a line nine days after the due day keeps it).
   LANGUAGE §7 says "within its `grace` (default: half a cadence)": the code implements neither half. In a schedule with no
   gaps the nearest due day is within half a cadence anyway; the full cadence matters at the ends of a contract and across a
   waived stretch (section 3.2).
3. **The ordinal count is O(n) per occurrence, and n is 71 million for a contract with no `from`.** `post_written_occurrence`
   (engine/ledger.rs:1145-1150) counts the due days from `contract.days.first()`. With no `from` that is `Day::MIN`, and every
   kept occurrence walks about 71 million months: `diff/cases2/promise-no-from.ax` (two kept lines) takes 9 s to `check`.
   `loan_payment` (ledger.rs:1689) is a second quadratic: it loops `periods` times, and `instantiate_occurrence` calls it for
   every occurrence of a loan.
4. **The schedule code is about 500 lines, not 450, and a third of it is not in `book.rs`.** `book.rs:678-957` is 280 lines
   (`Contract::occurrences`, `amount_on_schedule`, recognition, `terms_on_schedule`, `covers`, `ratio_pow`, `index_at`,
   `prorated_share`, `covered_span`); `core/calendar.rs:428-645` (`Cadence`, `On`, `Landings`, `due`) is 220 more.
5. **`Reset`, `Prepay` and `asset` of a loan add nothing to any schedule.** `Loan.resets`, `Loan.prepay` and `Loan.asset` are
   written by `contract_loan` and read by nothing. There is **no amortization anywhere**: `Derivation::Interest` and
   `Derivation::Principal` are never constructed, and a loan's payment is the one level amount `loan_payment` computes from
   the principal, the rate and the number of periods. The report's "Loan balance" is the holdings of the debt tab
   (report/contracts.rs:93), which only the journal moves.
6. **`deposit` and `matching` do nothing.** `Contract.deposit` and `.deposit_holding` are set by lowering (which checks the
   amount and the holding, diagnostics included) and read by nothing. `Contract.matching` and the type `Match` are never set
   (`empty_contract` writes `None`, the lowering never changes it).
7. **There are no "loan balances per period" in the old code.** The oracle can compare a loan's payment and how many payments
   fall due; a balance after each payment is new, so it is proven by identities, not by comparison (section 5.4).
8. **A written occurrence is matched against the schedule as lowering has left it so far**, not the final one: lines are
   lowered in `(day, source order)` (record.rs:143) and a waiver or an `ends` painted by an earlier statement is in the
   timeline for the later lines only. A line that keeps a due day which a later statement waives lowers fine and cannot be
   applied (`promise-lowering-order.ax`: `contract-occurrence-materialization ... Waived`). The fold, the forecast and
   the new structure see the final schedule.
9. **`ForecastError` has four variants nothing constructs** (`MissingInput`, `UnresolvedAmount`, `MissingTemplate`,
   `UnsupportedFeature`), and `ForecastFeature` exists only for the last. They are residue (PROPOSAL F1), and the new
   structure carries none of them.

## 1. `Contract`: every field and what reads it

| field | what it is | read by |
|---|---|---|
| `name`, `loc` | identity | everywhere (reports, `why`, diagnostics) |
| `party` | who the promise is with | `resolve.rs:236` (a loan's name as a flow end), record.rs:1108 (`lower_loan_origin`), report `contracts.rs:50`, `why/contract.rs:36` |
| `owner` | whose promise: the owner of the holding | engine `eval.rs:1576`, `plan.rs:242,382`, `post.rs:143`, `scope.rs:41`; model `laws/compile.rs:811,846`, `rules.rs:341`, record.rs:1087; report `contracts.rs:27` |
| `purpose`, `description` | what every promised flow is for | report `contracts.rs:72-75`, `why/contract.rs:38`; `infer.rs:133` for the diagnostic |
| `area` | the denominator of a measured `share` | **nothing after lowering** (`contracts.rs:1559` reads the local, not the field) |
| `days` | `from`..`until`, cut by `ends`: the days anything is expected | `book.rs` (`covers`, `occurrences`, `amount_on_schedule`, `recognition_on_schedule`, `prorated_share`), engine `ledger.rs:373,1145`, report `contracts.rs:34`, `register.rs:319`, `why/line.rs:215`, `statements.rs:638,676` |
| `terms` | the regular schedule, a timeline of `Terms` | `book.rs`, `record.rs:629,1062`, `statements.rs:593`, report `contracts.rs:31`, `register.rs:319`, `why/contract.rs:53` |
| `standing` | a standing `buy`'s schedule, a second timeline | `book.rs`, `record.rs:1068`, `statements.rs:593`, report `why/line.rs:218` |
| `buys` | the commodity a standing `buy` buys | engine `ledger.rs:609,614`; `record.rs:1386` (the unit an occurrence's amount is in). Also stored as `Unknown(unit)` in the group's `arrive` (contracts.rs:716): twice |
| `deposit`, `deposit_holding` | `deposit AMOUNT [into HOLDING]` | **nothing** |
| `loan` | `loan P on D at R over T` | engine `ledger.rs:1690` (`loan_payment`), model `record.rs:624,1055`, `resolve.rs:235`, `statements.rs:187`, report `contracts.rs:87` |
| `matching` | `match 50% of ..` | **nothing, and never set** |
| `ended` | where an `ends` cut `days` | report `why/contract.rs:70` |
| `laws` | the contract's own laws | written (`laws/mod.rs:429`, `contracts.rs:87`); the engine reaches a contract's laws through `Law.owner`, not this |
| `doc` | doc comment | written; no reader found |

`Loan`: `principal`, `on`, `term` are read by `loan_payment` and `lower_loan_origin`; `debt` by `resolve.rs`, `statements.rs`
and report `contracts.rs`; **`asset`, `resets`, `prepay` by nothing**. `Reset` is read by nothing at all.

## 2. `Terms`: the eighteen fields

`Terms` is one stretch of a timeline. `Timeline::paint` is called on one in exactly one place, `waive_contract`
(statements.rs:593): it clones the terms in force that day, sets `state = Waived` and `change`, and paints them over the
waiver's days. **So the stretches of one timeline differ only in `state` and `change`**: everything else of a waived stretch
is a clone of the declaration's, and "a change of terms" (`now 3_050 USD monthly`, LANGUAGE §7) is not implemented. Every
stretch carries its own copy of the template and the program (a `Promised` is 296 bytes).

| group | field | what it says | read by |
|---|---|---|---|
| when | `state` | `Active` or `Waived` | `is_waived` in `book.rs` (4 sites), engine `ledger.rs:379`, report `contracts.rs:67`, `register.rs:352`, `why/contract.rs:57` |
| when | `every` | `Cadence::Every(Span)` or `TwiceMonthly` | `calendar::due` (book.rs:841), `nearest_occurrence`'s radius (record.rs:1197), `loan_payment` (ledger.rs:1692), report `contracts.rs:70`, `forecast.rs:462` |
| when | `on` | the days of the period each step lands on | `calendar::due`, report `contracts.rs:71` |
| when | `anchor` | `days.first()`: where the steps count from (`Day::MIN` with no `from`) | `calendar::due`; the template flow's own day (contracts.rs:825) |
| amount | `template` | one `Promised` (always one) | engine `instantiate_occurrence`, `record.rs:654,1062-1074,1389`, `book.rs::template_covers_flow`, report `contracts.rs`, `why/line.rs` |
| amount | `program` | the nodes of the template's computed amounts | engine `ledger.rs:522` |
| amount | `inputs` | the names an occurrence may state | `record.rs:653`, report `contracts.rs:79`, `forecast.rs:472` |
| amount | `rate` | a loan's yearly rate | engine `loan_payment` |
| amount | `estimate` | `about` | **nothing** |
| conditions | `escalation` | `rising 3% yearly`, `indexed to cpi yearly` | `book.rs::amount_on_schedule` only |
| conditions | `prorated` | the share of the period the contract lives in | `amount_on_schedule` only |
| conditions | `period`, `covers` | the window an occurrence is recognized over | `recognition_period_for_schedule` only |
| conditions | `due` | `due 5d else + 5% #late-fee` | **nothing** (`declare/mentions.rs:101` reads the AST) |
| conditions | `grace` | `grace SPAN` | **nothing** |
| rest | `shares` | `share 20% for studio` | **nothing** (the `Share`s the engine reads are an account's and an asset's) |
| rest | `also` | the `also` lines | **nothing** (the engine reads `Book.also` through `Law`) |
| rest | `change` | the statement that made this stretch | report `register.rs:351`, `why/contract.rs:61` |

Five of the eighteen are read by nothing (`estimate`, `due`, `grace`, `shares`, `also`), and the two that decide when a
promise is *missed* (`due`, `grace`) are two of them: F5's "the monitor does not exist", seen from the data. The structure
this lane builds carries `due` and `grace` so that K5b has a place to read them; it does not give them a meaning.

## 3. Which occurrence: how the code asks, four times

### 3.1 The due days: `Contract::occurrences` and `calendar::due`

`Contract::occurrences(within)` (book.rs:709) is `occurrences_for(terms)` and `occurrences_for(standing)` merged by day:
`ContractOccurrences` (book.rs:885) takes the standing one only when it is **strictly** earlier, so on equal days the regular
one comes first. `ScheduleKind` (`Regular`, `Standing`) is what an occurrence names so that two schedules of one contract
are not confused: it is part of the key `RuntimeTxn::ContractOccurrence { contract, schedule, day, ordinal }`, of
`Txn.contract_schedule`, of `WrittenOccurrence.schedule` and of `Promise.schedule`. A schedule is a stream of due days; the
kind says which stream.

`occurrences_for` walks the stretches of the timeline that meet the window, skips a waived one, and for each calls
`calendar::due(every, on, anchor, stretch ∩ window)`. Because every stretch has the same `every`, `on` and `anchor`, the days
it yields are **independent of the window and of how the stretches cut it**: they are

> `D = { landed days of step b, b >= 0 } ∩ [anchor, contract.days.last()] \ waived days`

where step `b` is `anchor + b·every` (months first, clamped to the month's end, counted from the anchor, never from the
previous step; then days), and a step lands on each day of `on` in the month, year or week it is in (`on` empty: on itself).
Checked against the code: `first_cadence_at_or_after` and the `forward_landing`/`backward_landing` slack in `due` only choose
where to start walking; the final `filter` is the set above.

That is a set, and the code does not always yield a set: **when `on` names a longer period than `every` steps by, one landed
day is yielded once per step** (`weekly on 15`, `daily on monday`, `every 3d on monday`, `monthly on 04-15`):
`diff/cases2/promise-coarse-on.ax`, `forecast` lists the 15th five times in January. The parser takes these (`days_of_period`
checks nothing against the cadence). The doc comment of `due` says "a day two of them land on is due once" for the days of
one step; across steps it is not. Section 6 lists it; the new structure's due days are the set.

**And a window can lose a due day.** `due` starts at the first step on or after its window, less a slack for the days a
step lands before or after itself (`forward_landing`). `on last` lands up to 30 days after its step and the slack for it is 0
(calendar.rs:630: `(On::Weekday(_) | On::Last, true) | (On::Last, false) => 0`, where `(On::Last, false)` is the forward
slack). A window that begins after the step and before the month's end therefore misses that month's last day, and the
windows `occurrences` asks are cut by waived stretches: `monthly on last from 2026-01-10` waived 03-01 to 03-15 has no due day
on 2026-03-31 (`promise-last-after-waiver.ax`, `forecast` skips March). The set above is what a walk from the anchor finds, and
this is where a walk from a window disagrees with it.

`Day::MIN` as an anchor is a number, not a missing value: `due` counts steps from it (`schedules_fast_forward_from_day_min..`
in the tests), so a contract with no `from` has a phase decided by `Day::MIN`'s day of month and weekday when it has no `on`.

### 3.2 The four answers

| | `nearest_occurrence` (record.rs:1186) | ordinal in `post_written_occurrence` (ledger.rs:1112) | `contract_forecasts` (report/forecast.rs:371) | `keep_paired` (sync/promise.rs:35) |
|---|---|---|---|---|
| asked by | `lower_occurrence`, once per `DATE NAME` line, in lowering | the fold, once per kept occurrence | the forecast, once per contract | `World::statement`, per import: **never has a due** |
| input | the contract as lowered so far, a day | the final contract, the due day the line kept | the final contract, `today+1..until` | records, parties, `Due { day, qty, window }` |
| what it finds | the due day this line keeps and its `Terms`; or none; or "equally near two schedules" | **ordinal**: the number of due days of its schedule from `contract.days.first()` to this one, less one | the due days in the window, each numbered from 0 | for each record the due it keeps |
| how | `occurrences(day ± radius)`, keeps the minimum of `(distance, is_future)` per schedule | `occurrences(first..due)`, filtered and counted | `occurrences(window).enumerate()`, **both schedules merged**, written ones counted then dropped | all pairs within the window, sorted by `(distance, record, due)`, taken greedily |
| tie | the **earlier** due day when equally near; regular against standing equally near is an error | none | none | the smaller record, then the smaller due |
| radius / window | `max` over every stretch of both schedules of `months·31 + days` (`TwiceMonthly`: 31); a **full** cadence | the whole contract so far | the forecast window | `Due.window`, "half a cadence", set by a caller that does not exist |
| cost | O(window) per line, window about two cadences | **O(n)** per occurrence, O(n²) over a contract; 71 million per occurrence with no `from` | O(window) | O(records × dues) |
| numbering | none | **per schedule, from the contract's start** | **per window, both schedules together, from today** | none |

Where they disagree, each shown by a case or by reading:

- **Numbering.** An occurrence has one ordinal in history (`n`th of its own schedule since the contract began) and another
  in the forecast (`m`th of both schedules since `today`). `RuntimeTxn`'s key includes the ordinal, so the identity of "the
  rent due on 2026-07-01" differs between a fold that kept it and a forecast that predicted it. The ordinal is read by
  no report: it is seen by the key and by `Promise.ordinal` in a dump.
- **The radius is shared.** It is the largest cadence over the stretches of both schedules (record.rs:1194-1205), so a
  daily standing `buy` is matched with the reach of the monthly payment beside it.
- **Waived stretches are not candidates, and they are not distances either.** A line inside a waived stretch keeps the
  nearest due day of the others if that is within the radius, else none (`promise-waived-reach.ax`: 47 days off, "outside
  every contract schedule's grace window").
- **Lowering order.** `nearest_occurrence` runs while the timeline is being painted; the fold and the forecast run on the
  painted one (section 0.8).
- **The contract's first day.** `nearest_occurrence` refuses a line outside `contract.days` before looking at a due day.
  The ordinal's count and the forecast's window are clipped by the same days, so they agree on this.

## 4. What the fold needs of a schedule

Read from `instantiate_occurrence` (ledger.rs:359), `post_written_occurrence` and `contract_forecasts`; nothing else in
`engine`, `report` or `sync` asks a contract's schedule anything but `contracts`/`why` (next due, terms by day, the stretches):

| question | asked as | answer |
|---|---|---|
| the due days in a window, in order | `occurrences(window)` | `ContractOccurrence { day, schedule, terms }` |
| its ordinal | count (history), `enumerate` (forecast) | `u32` |
| the line's occurrence | `nearest_occurrence` | `(schedule, due, terms)` |
| is it waived, is it in the contract | `terms_on_schedule`, `days.contains`, `is_waived` | `Waived(day)`, `OutsideContract(day)` |
| the factor of the day | `amount_on_schedule(book, schedule, day)` | `Ratio`, or a `ForecastError` |
| the window it is recognized over | `recognition_on_schedule(template, schedule, day)` | `Days`, or an error |
| what is paid | `terms.template`, `terms.program`, `terms.inputs`, `terms.rate`, `contract.buys`, `contract.loan` | |
| the next due day, for the `contracts` view | `occurrences(today..).find(not kept)` | `Day` |
| the stretches, for `why` and `register` | `terms.within(days)` | `(Days, &Terms)` |
| does a contract cover this habit flow (`covers`) | `Contract::covers(template, day)` | `ContractCoverage`, for `forecast/expected.rs:70`: the second driver, K5c's |

Everything on the first six rows is a function of the schedule's constants and the day: no state of the book but the
parameters an `indexed` escalation reads. That is what lets it be computed.

## 5. Written to term, checked against the code

PROPOSAL §5 K5 has a table. This is the same table for what today's language says and what the code does with it.

| written | lowered to | term | evidence |
|---|---|---|---|
| `AMOUNT CADENCE [on D] (from \| into) HOLDING` | `Terms` with `template`, `every`, `on`, `anchor` | `Every { schedule, body: Pay }` | contracts.rs:673 |
| `buy UNIT for AMOUNT CADENCE ..` | the `standing` timeline; `Contract.buys`; the group's `arrive: Unknown(unit)` | a second `Every`, with a `Pay` whose group has the exchange header | contracts.rs:716, 220 |
| both lines | two timelines | `All[Every, Every]` | `ContractOccurrences` |
| `from` / `until` / `X ends` | `Contract.days`; `Terms.anchor = days.first()` | the schedule's `anchor` and `until` | contracts.rs:167, statements.rs:676 |
| `X waived [until D]` | a waived stretch painted over the days | the schedule's **holes** | statements.rs:593 |
| `due SPAN [else ITEM]` | `Terms.due` (read by nothing) | `Due { grace: SPAN, blame, body: Pay, otherwise: Pay(item) }` | contracts.rs:694 |
| `grace SPAN` | `Terms.grace` (read by nothing) | carried, with the matching's reach, for K5b | contracts.rs:702 |
| `rising`, `indexed` | `Terms.escalation` | the schedule's factor of the day | book.rs:743 |
| `prorated`, `for last ..`, `covers ..` | `Terms.prorated`, `period`, `covers` | the schedule's factor and recognition window | book.rs:761, 801 |
| `input NAME`, `NAME = V` | `Terms.inputs`; the written occurrence's bound inputs | in the `Pay`'s group; K4b's | record.rs:653 |
| `about` | `Terms.estimate` (read by nothing) | dropped | |
| `loan P on D at R over T` | `Contract.loan`; `Terms.rate`; the header's side `Derived` | `Annuity` in the body of an `Every`; the payment and the number of payments computed once | ledger.rs:1689 |
| `resets`, `prepay` | `Loan.resets`, `Loan.prepay` (read by nothing) | dropped; K5b decides what a prepayment does to the residual's `open` | |
| `deposit` | `Contract.deposit` (read by nothing) | nothing: there is no behaviour to compile | |
| `D NAME` that is the loan's `on` | `lower_loan_origin`: a flow from the debt tab to the funding account | nothing: the fold makes it only when it is written, and nothing predicts it | record.rs:1031 |
| `share`, `also` | `Terms.shares` (nothing), `Terms.also` (nothing); `Book.also` | not terms: laws (K6) | |
| `match` | never set | nothing | |

Three consequences:

1. **A loan is not a schedule of interest and principal.** What there is to compile is a level payment and a number of
   payments. `Annuity` is the schedule's body plus the arithmetic that says when the debt is paid: its payment (computed
   once, with the engine's own fixed-point loop so that the cents agree), the number of payments `ceil(term / step)`
   (`TwiceMonthly`: `term.months × 2`; a step that is neither whole months nor whole days has none) and a balance after
   each payment, which is new. A balance after the last payment is zero: that is what ends the loan forecast
   (`native_loan_forecast_stops_after_the_typed_principal_is_repaid`, one of the three known failures).
2. **A waiver is a hole in the day set, not a change of terms.** It removes due days and never changes what one pays.
3. **Escalation, proration and recognition are not amounts.** They read the day, the contract's life and (for `indexed`) a
   parameter, never a flow or the state of the fold, so they belong to the schedule beside the due days; K4b's `solve` is
   given the factor, as it is today (`Reading.scale`).

## 6. The constructors today's language needs

| constructor | needed? | why |
|---|---|---|
| `Done` | yes | what is left when nothing is |
| `Pay` | yes | the template's group (`Promised`) |
| `All` | yes | a contract with a regular and a standing schedule |
| `Every` | yes | the schedule |
| `Due` | yes | `due .. else`: lowered, validated, carried; no reader yet |
| `Annuity` | yes | `loan`: the one place a promise has state (a balance) |
| `At` | **no** | its only sources are `deposit` (nothing reads it) and a loan's origination (nothing predicts it) |
| `If` | **no** | no promise has a condition; `also .. when` is a law |
| `Let` | **no** | `indexed` and a loan's rate are read as the day's factor, `resets` is unread |
| `Accrue` | **no** | no interest is accrued anywhere |
| `Choose` | **no** | there are no options |

Six constructors, then, and the schedule's `paid` and `roll` (PROPOSAL §5 K5: the pay period and the pay date, DESIGN A6) are
not built either: no language word says them.

## 7. Where the old code is wrong

The oracle reports each of these as listed, not as a failure; the new structure's answer is stated.

| # | the old code | shown on | the new structure |
|---|---|---|---|
| a | one due day yielded once per step when `on` names a longer period than the cadence | `promise-coarse-on.ax` | the due days are the set: once |
| b | the ordinal is O(n) (71 million with no `from`) | `promise-no-from.ax` | O(log n) |
| c | the forecast numbers from the window and merges the schedules; history numbers per schedule | section 3.2 | the index in the schedule, in both |
| d | `grace` is read by nothing; the reach is a full cadence, not "half a cadence" | `promise-grace-ignored.ax` | the reach is the old one; `grace` is carried |
| e | matching sees the timeline as lowering has painted it so far | `promise-lowering-order.ax` | the final schedule |
| f | a line in a waived stretch is matched 47 days away or not at all | `promise-waived-reach.ax` | as old (reach), on the final schedule |
| g | `loan_payment` is O(periods) per occurrence; the loan forecast does not stop | `source_tests.rs:852` | the payment once; the loan is done after its last payment |
| h | recognition of a due day is `Days::on(anchor).moved(day - anchor)`, which overflows for an anchor of `Day::MIN` (any due day from 1970 on) and which the engine papers over (ledger.rs:540) | read; `promise-no-from.ax` goes through it | `Days::on(day)` |
| i | an escalating contract with no `from` is `ForecastError::Overflow` (a ratio compounded over 5.9 million years) | `promise-rising-no-from.ax`, `forecast` | the same error; a contract that escalates needs a start (a language question) |
| j | a due day is lost when a window begins after its step and before the month's end (`on last`, slack 0) | `promise-last-after-waiver.ax` | the day is due |

## 8. What this decides

1. **A `Schedule` is a set of days with a rank.** `nth(n)` and `index_at_or_after(day)` are inverses on `D` of 3.1. The
   arithmetic is pure calendar and lives in `core` beside `Cadence` and `On`, where `due` is today. Shapes:
   - *plain* (`on` empty): step `b` is `anchor ⊕ b·every`, strictly increasing, so `rank` is a binary search over `b` and
     `nth` is one addition. Exact for every `Span`, `Day::MIN` included;
   - *tiled* (`on` names days of one kind of period and every step falls in a period of its own, no two days of one step
     clamping to the same day): the days of step `b` are a block that is entirely before the next step's, so
     `rank = b·k + (days of the block before day)` less the days of the first block that precede the anchor. Binary search
     over `b` with an O(k) probe;
   - *walked* (the rest: a longer `on` than the cadence, two clamping days such as `on 30, last`, days of two kinds): the old
     iterator with the duplicates removed. They are O(n), the oracle covers them as sets, and the report counts them.
2. **Holes and the contract's life are part of the schedule.** `skips`: sorted disjoint ranges with the number of due days
   each swallows, in one flat arena (`core::dayset` has the form); `until`: the last day. `nth` and `rank` then give the
   index among the days that are owed, which is what an ordinal is.
3. **The factor and the recognition window of a day are the schedule's.** `Escalation`, `Coverage` and `Relative` are
   Terms' own types; the arithmetic of `amount_on_schedule` and `recognition_on_schedule` moves to the new module with the same
   errors, and the old copy stays until K5b.
4. **Matching is `keep(day)`:** the due days either side of the line, the nearer, the earlier on a tie, within the contract's
   reach; the contract's reach is the old radius. Two schedules are compared by distance and equal distance is ambiguous.
5. **A `Residual` is `{ term, next, ordinal, open }`** and advances by `advance(promise)`: `Every` moves `next` to
   `nth(ordinal + 1)`, `Due` is overdue at `next + grace`, an `Annuity` is `Done` when no payment is left.
6. **The compile runs once, at the end of `build`, over the final contracts**, and the `Book` keeps the terms beside
   the contracts: the stretches of today's `Terms` differ only in `state` and `change` (section 2), so one compile per
   contract is exact, and it `debug_assert!`s that.
7. **No behaviour moves.** `Contract::occurrences`, `nearest_occurrence`, `amount_on` and the engine's count stay and are
   what the oracle compares to; K5b moves the fold, K5c the forecast, and then they go.

## 9. How this map was checked

- Reads of each field: `grep` of every field name over `model`, `engine`, `report`, `sync` and `cli`, tests excluded, and a
  scratch copy of the tree with every field of `Contract`, `Terms`, `Loan`, `Reset`, `Deadline`, `Share` and `Input` made
  `pub(crate)`: `cargo check -p axiom-model` warns that only `matching` is never read inside `model`, and the
  workspace's errors list every other crate's reads (24 in `engine`).
- Findings on books: `docs/v5/measure/diff/cases2/promise-*.ax`, run through the baseline binary (`check`, `contracts`,
  `forecast`) at `5649739`: `grace` ignored (kept nine days late), the lowering order (`could not materialize this
  occurrence: Forecast(Waived(..))`), the waived stretch (`contract-occurrence-date`), no `from` (9 s for two lines), coarse
  `on` (nine rows of `a` on one day in a forecast to 2026-02-20).
- The four implementations and the numbering: read in full (record.rs:1186-1238, ledger.rs:1112-1232, forecast.rs:330-484,
  sync/promise.rs, binding.rs, world.rs:322).
- The set `D`: `core/calendar.rs::due` read in full, and its tests; the oracle of Step 1 checks it on generated contracts.
