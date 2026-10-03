# Lane K5d: a loan is a state machine with four inputs, and its payment says what is interest and what is principal

Read [`common.md`](common.md) first. Then LANGUAGE.md §7 ("Loans follow the ACTUS annuity" and the `deposit` paragraph),
[`K5a-map.md`](K5a-map.md) §0 item 5, §7 and §10 (what a loan is today and what K5a built: `Annuity`, `Paid`, `Residual`'s
fifth field `began`), [`K5b-map.md`](K5b-map.md) §6 (the monitor walks a `Residual` per stream), `K5c-map.md` (the forecast
is the fold), `K4b-map.md` §10 (a split is a `Group`, solved by `solve`) and `K3c-map.md` §0.5 and §6 (what a parcel on a
claim tab is). Your worktree is `/home/user/axiom/.claude/worktrees/lane-k5d`, on branch
`claude/great-wozniak-pnqn7x-v5-k5d`. **This lane starts after K5c has merged** (the forecast and the fold then walk one
`Residual`); the deposit half also wants K3d's phase C (a debt is a parcel). Say in the map which half you build.

**Your crates:** `model` (`promise/annuity.rs`, `promise/residual.rs`, `book.rs`'s `Loan`/`Reset`/`Prepay`,
`lower/contracts.rs`' loan and deposit clauses, `lower/record.rs`'s `lower_loan_origin`), `engine` (`occurrence.rs`,
`monitor.rs`, `ledger.rs` where a flow to a loan is posted), `report` (`contracts.rs`, `why/contract.rs`). Lane K6 touches
`laws/` and `lower/also.rs` (the employer's `match` is a K6 `Derive`: **not yours**, leave `Match` alone).

## What is wrong

Everything LANGUAGE §7 promises of a loan except the level payment is **written, checked and read by nothing**
(STATUS, "What the grammar accepts and the engine does nothing with"):

- a payment is one lump to the debt tab: `#interest` and `#principal` do not exist as flows, `Derivation::Interest` and
  `Principal` are never constructed, so a mortgage's interest cannot be deducted, budgeted or seen;
- `resets EVERY from DATE to PARAM + PERCENT [cap] [life]`, `prepay shortens | recasts` and `for ASSET` are lowered into
  `Loan` and read by nobody;
- a rate `now` changed does not refigure the payment over what is left;
- `31 mortgage = 310_978.17 USD` (§7: "its balance on any day, which a value on the contract's name checks") checks the
  debt tab's holdings, which only the journal moves, so the schedule's own balance is never compared with the bank's;
- `deposit AMOUNT [into HOLDING]` is validated and then nothing reads `Contract.deposit`.

## What to build

**A. The loan is one step function.** K5a's `Annuity` is the arithmetic of a *fixed* loan. Make the loan's state a value and
its life a fold over four inputs:

```text
State  { open: Qty, rate: Ratio, payment: Qty, remaining: u32 }          // 40 bytes or fewer: say what you measured
Event  { Pay(day), Prepay(day, Qty), Reset(day, Ratio), Rate(day, Ratio) }
step: (State, Event) -> (State, Paid { interest, principal, open })     // pure, total, no allocation
```

- `Pay`: interest `open * rate` rounded half to even (K5a's fixed point: the cents must be the ones the books have always
  had), principal `payment - interest`, the last payment clears `open` exactly.
- `Prepay` with `Shortens`: `open` falls, `payment` stays, `remaining` is **recomputed** (the number of payments that the
  same payment needs: closed-form, `n = -ln(1 - open*r/payment) / ln(1+r)` is not exact in fixed point: **find the exact
  integer formulation** and say how you know it agrees with stepping to zero; a binary search on the annuity factor is
  acceptable if the map shows it exact); with `Recasts`: `remaining` stays, `payment` is refigured.
- `Reset`: `rate = clamp(index + margin, previous ± cap, initial ± life)`; the payment is refigured over `remaining`
  from `open`; `Rate` (a `now ... at R%` change) is the same step with no clamp.
- The **fold, the monitor and the forecast call the one `step`**; `Residual` for an `Annuity` term carries a `State` (the
  fifth field `began` becomes part of it) and the forecast's loan lines come from the same walk. A second implementation
  anywhere (the report's, the forecast's, a test's) is a defect in the lane.
- A payment **is a split**: it is a `Group` whose header is the payment and whose legs are `interest` (to the outside,
  `#interest`, `of ASSET` when `for` names one) and `principal` (to the debt tab), solved by K4b's `solve` with the
  principal as the remainder. So `flow`, `tax`, `budget` and `why` see `#interest` and `#principal` with no special case,
  and a payment written by hand with its own legs is checked against the schedule's.
- A flow to the loan's contract that is not a scheduled payment is a **prepayment**: say in the map exactly what is one
  (on a due day, the excess over the scheduled payment; on any other day, all of it) and what the fold does with a flow
  from the loan (a draw).
- `value mortgage = X` becomes a **reconciliation**: the schedule's `open` on that day against the statement. A
  difference is a diagnostic naming both numbers and the day, **and the likely cause** if one candidate explains it (a
  missed payment, an unrecorded prepayment of exactly the difference, an escrow). This is the feature users will love;
  build it well and show it on a book.

**B. `deposit`** (only if K3d's phase C has landed; otherwise stop at the map with the design). `deposit 2_900 USD into
escrow` is a flow at the contract's first occurrence and a **parcel on a claim tab**: the owner paying it holds a claim on
the party (returned at the end by a `Term::At` promise, so a deposit not returned is **overdue** like any promise), and a
deposit **to** the owner is a debt to the tenant held in the named holding. Both settle by the one relief order
(`exact`/`code`/`oldest`). `Term::At` is the variant K5a built and nothing yet uses: use it or delete it.

**C. `for ASSET` and the tax line.** Interest `of` the asset, so that a report that asks for the interest on the condo
finds it. If a mortgage-interest deduction exists in `tax` (check `report/tax.rs`), say whether it now reads `#interest`
and what the example moves; if it does not, do not invent one: list it in the report as a decision.

## The proof

- **An oracle**, in `docs/v5/measure/` in the style of `contracts.rs`/`promises/` (K5a's reference): an independent
  Python (or Rust-free) ACTUS ANN with the same fixed-point rule, seeded generators for loans of every cadence, with
  prepayments (both modes), rate changes, resets with caps, a final payment, a loan paid off early, a payment missed,
  compared on **every payment's interest, principal and open balance** and on the monitor's overdue day. Mutation-test
  it (the K5a/K5b standard: every mutant of `step` killed by the oracle or a named unit test).
- Behaviour: goldens and mistakes **change only where a loan appears** (`02`, `07`, `10`?; measure which); each moved
  line comes with its reason (a payment now says `#interest` and `#principal`; totals are unchanged). **The sum of the
  legs equals the old payment on every payment of every example** (assert it in a test): if it does not, stop.
- `fuzz.py ... diff` over books with no loan: byte-identical. The three known failures remain only as the STATUS lists them.
- Performance: `axiom check` on `bench/` 100k and 1m: no slowdown (a loan's `step` is a handful of integer ops).

## Rules of this lane

- Common bar. In particular: `Event` and `Prepay` are enums; no bool parameters; functions under 40 lines; no parameter
  bundles (`step` takes a state and an event, not six numbers); no `unsafe` (a `State` is plain data; if it wants a
  tagged payload, K4c's `tagless` is the place and not this lane).
- The exact-integer arithmetic is the hard part: do it in `core::num`'s fixed point, reuse `Ratio`, and say in the module
  doc what is exact and what is rounded and **where**, with a test per rounding site.
- No test deleted or weakened; `native_loan_forecast_stops_after_the_typed_principal_is_repaid` must keep passing.

## Step 0: the map

`docs/v5/lanes/K5d-map.md`, committed first: today's loan code path end to end (lowering, `Annuity`, `Residual`, the
fold's occurrence, the forecast, the reports), with line numbers; the `State`/`Event` types and their sizes; the exact
integer formulation of "how many payments does this payment need" with its proof sketch; what a prepayment is; the
reconciliation's candidate causes; the examples a loan touches and what each prints today; what half you will build
(A only, A and B) and why.

## Measure

Lines per crate (this lane **adds** about +350; say what it deleted: the lump payment, `Derivation::Interest`/`Principal`
if they go); the histogram; the oracle's counts; the list of changed outputs with reasons.

## Not in this lane

- The employer's `match` and `share`: K6 (`Derive`). Prorata, claim recognition: K3d / the user's decisions. Escrow as a
  separate account: it is an `also` line and stays one.
