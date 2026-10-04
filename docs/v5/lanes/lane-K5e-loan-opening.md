# Lane K5e: a loan that began before the book opens its debt with what its schedule says is owed

Read [`common.md`](common.md) first. Then [`K5d-map.md`](K5d-map.md) **all of it** (§0.1: a payment's principal now leaves the debt
tab; §13 and the report in STATUS: "the three places I am least proud of", number 1 is yours), LANGUAGE §5 and §7 (a loan's
balance "comes from its terms"; an `opening` block; the origination line `DATE NAME`), `crates/model/src/promise/amortization.rs`
(the schedule: `open_on(day)`), `crates/model/src/lower/record.rs` (`lower_loan_origin`, `opening` lowering) and
`crates/engine/src/{ledger,reconcile,loan_balance}.rs`. Your worktree is `/home/user/axiom/.claude/worktrees/lane-k5e`, on branch
`claude/great-wozniak-pnqn7x-v5-k5e`. **Small lane: a design decision, a few hundred lines at most.**

## What is wrong

K5d made a loan's payment two flows: `#principal` to the loan's debt tab and `#interest` to the lender. That is right when the
book holds the loan's whole life or its origination. **A book that begins after the loan was made** (the common case: a person
starts keeping books in 2026 with a mortgage from 2024) writes `loan 320_000 USD on 2024-02-20 at 5.875% over 30y` and nothing
opens the debt tab, so the tab starts at zero and the principal legs take it **negative**: `11-sam` and `v4-sketch` print
liabilities of -334.42 and -460.94 USD. The books' own comments say what is owed (`11-sam`: "312,441.12 owed" is exactly the
schedule's balance after 22 payments). Before K5d the same books printed a payment as pure expense and never touched the tab, so
the balance sheet was incomplete in a quieter way.

## What to build

A loan whose `on` day is **before the book's first fact** (the day the monitor starts its streams; find the one definition and
reuse it) and which has **no origination line and no `opening` line for its debt tab** **opens that tab on the day before the
first fact with the schedule's balance on that day**, against the opening-balances equity, exactly as an `opening` block would
(`Origin::Derived`, the same kind of flow the model already makes for an `opening`). Decide in the map, from the code:

- what "before the book's first fact" is when the book has no journal yet (a book of only declarations): the loan then opens on
  its own `on` day (that is the origination K5d reads: say how the two cases join);
- what the opening's size is when payments were **missed or prepaid before the book began** (the schedule is the lender's
  terms; K5d's `Cause::Short` papers over a short payment): the schedule's balance on that day is the answer, and a statement the
  book writes later reconciles it (K5d's `loan-balance` error, which now has a real balance to hold);
- an `opening` block that names the debt tab **wins** (the book says what it says: a user with a statement writes the number),
  and an origination line wins; neither is overridden, and the map says what happens when both an `opening` and the implied
  one would exist (the implied one is not made);
- the one diagnostic: when the implied opening is made, `check` says so once per loan, as a note naming the loan, the day and the
  amount, with the line that overrides it (`opening DATE / LOAN -X USD`). A note, not a warning: it is the language working.

## The proof

`11-sam` and `v4-sketch` print positive liabilities equal to the schedule's balance (check `11-sam`'s own comment: 312,441.12),
and their `loan-balance` statements (if any) agree; `07-landlord` (origination line), `05-family` (loans begin 2026-01-01, the
journal ends 2025) and `explore-v5/03-triplex` (`from 2022-10-01`, statements agree to the cent) are **byte-identical** except
the note where it applies; K5d's `loans.py` oracle gains books whose first fact is after the loan's day (statements,
prepayments, resets, missed payments, with and without an `opening`), compared to its independent reference, 0 disagreements,
mutation-tested (every mutant killed by the oracle or a named test); goldens and mistakes byte-identical except what the
examples above print; a mistake book for each diagnostic; `fuzz.py ... diff`; `axiom check` on `bench/` 100k and 1m, no loans:
no slowdown.

## Rules

Common bar. No unsafe, no bool parameters, functions under 40 lines, no parameter bundles. Add as little as possible: **this lane
must not grow `engine/loan_balance.rs`** (K5d's own list says it is 191 lines of prose in code; do not add to it). No test
deleted or weakened.

## Step 0: the map

`docs/v5/lanes/K5e-map.md`, committed first: the book's first-fact day (one definition, where it lives), how an `opening` block
is lowered today and what flow it makes, which examples have a loan that predates the book (measure on a scratch build: `11-sam`,
`v4-sketch`, maybe more), and the decisions above.

## Not in this lane

`deposit` (K3f), the mortgage-interest deduction (the user's decision), `loan_balance.rs`'s structure (K12b).
