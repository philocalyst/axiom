# Lane K5a: a promise is a term, and its schedule is computed, not searched

Read [`common.md`](common.md) first. Then [`../PROPOSAL.md`](../PROPOSAL.md) §3 F2 ("which occurrence is due, four
times"), F4 and F5, and §5 K5, and [`../DESIGN.md`](../DESIGN.md) §3.7. Then the finished maps of the lanes before you
(`K4a-map.md`, `K3a-map.md`, `K4b-map.md`): they are the standard yours is held to. Your worktree is
`/home/user/axiom/.claude/worktrees/lane-k5a`, on branch `claude/great-wozniak-pnqn7x-v5-k5a`.

**Your crates:** `model` (`book.rs`'s contract and schedule code, `lower/contracts.rs`), plus a new module, and the
readers you move in `engine`, `report` and `sync`. Lanes K3b/K3c (positions, parcels) may be running: stay out of
`engine/post.rs`, `lots.rs`, `assets*.rs`.

## What is wrong

"Which occurrence of this promise is due, and which one is this line keeping?" is answered **four** times, four ways:

| where | how | cost |
|---|---|---|
| `model/book.rs::Contract::occurrences` and `lower/record.rs::nearest_occurrence` | walks the schedule within a radius of `months × 31` days of the line | per line |
| `engine/ledger.rs::post_written_occurrence` | counts every due day from the contract's start up to this one: its **ordinal** | O(n) per occurrence, **O(n²)** over a contract's life |
| `report/forecast.rs::contract_forecasts` | numbers ordinals from 0 inside the forecast window, so an occurrence's identity differs between history and forecast | a second simulation loop |
| `sync/promise.rs::keep_paired` | greedy nearest within a window | per record |

and a contract is `Contract { terms: Option<Timeline<Terms>>, standing: Option<Timeline<Terms>>, buys, deposit,
deposit_holding, loan, matching, ended, … }`: eight optional features, each with its own schedule code in `book.rs` (about
450 lines of cadence, anniversary, window, escalation and proration arithmetic), plus `Terms` with eighteen fields.

The engine builds every `Run` with `monitor_complete: false` and `open_claims: Box::default()`: **the monitor that would
know what is still owed does not exist**, and two of the three failing tests (a loan forecast that emits five payments
where three are owed; a forecast closing prefix that lacks a year-end tax) are its symptoms.

This lane builds the data structure and the algorithms. It does not yet move the fold onto them (K5b) or delete the
forecast's second driver (K5c): it builds the new thing **beside** the old and proves it agrees.

## What to build

A promise compiled to a term, stored post-order like the expression arena, and a residual cursor over it. The shape is in
PROPOSAL §5 K5 and DESIGN §3.7. Treat those as the destination, not a script: where the code shows a better cut, take it,
and say why. In outline:

```rust
pub enum Term { Done, Pay(Leg), All(Run<Term>), Every { schedule: Id<Schedule>, body: TermId },
                Due { grace: Span, blame: Role, body: TermId, otherwise: TermId }, At { day: DayExpr, body: TermId },
                If { .. }, Let { .. }, Annuity(Id<Loan>), Accrue { .. }, Choose { .. } }
pub struct Schedule { period: Cadence, paid: Option<Cadence>, roll: Roll }
pub struct Residual { term: TermId, next: Day, ordinal: u32, open: Qty }      // a cursor into the plan's terms, never a copy
```

The algorithmic points (the part that must be right):

1. **The due days of a schedule are computed, not searched.** `Schedule::nth(n) -> Day` and its inverse
   `Schedule::index_at_or_after(day) -> u32`, by arithmetic on the cadence (`monthly on 1, 15`, `every 2w`, `quarterly on
   last`, a date list) and a binary search where a cadence is irregular, in O(log n). That replaces the radius walk, the
   O(n) ordinal count, and the forecast's own numbering: an occurrence's **ordinal is its index in its schedule**, the same
   in history and in forecast.
2. **Matching a line to a promise is one sweep.** Given the sorted due days within grace of a dated line, find the one it
   keeps in O(log n), with the same tie-break the code has today (read it: it is half a cadence, and the oldest first when
   two are within it).
3. **A residual advances by `Done`/`Due`/`Every`**, and an annuity's residual is `Done` when its balance is zero (this is what
   fixes the loan forecast).
4. **Amounts are expressions** over the fold's state, evaluated by K4b's `solve`. You do not evaluate any in this lane:
   `Pay` carries K4a's `Promised` group.

## Rules of this lane

- **No behaviour change.** Nothing in the fold, the forecast or sync is switched to the new structure in this lane. It is
  built beside the old and proven equal. Goldens, mistakes and tests unchanged; the same three known failures.
- **The proof is the point.** An oracle compares, over every contract in `examples/` and a generated corpus, the old
  schedule code and the new term walker on: the due days in any window; each occurrence's ordinal; the occurrence a given
  day keeps (`nearest_occurrence`'s answer); `amount_on` (escalation, proration, covers, recognition windows) per day; and
  loan balances per period. **Equal, including equal edge cases** (a window that begins before the contract, a 31st in
  a short month, `on last`, an escalation on an anniversary, a waived term, a `standing` schedule that merges with the
  regular one). Where the old code is wrong (a radius that misses a due day, an overflow near `Day::MIN`), say so, show it
  on a book, and list it: do not reproduce a bug silently, and do not fix one silently.
- No test deleted or weakened.

## Step 0: the map, before any code

Write `docs/v5/lanes/K5a-map.md` and commit it first. It must say, from the code:
1. every field of `Contract` and `Terms`, what reads it (file, function), and which of the eighteen `Terms` fields are the
   *cadence* (when), the *amount* (how much), the *conditions* (due, grace, covers, prorated, escalation) and the *rest*;
2. how `terms` and `standing` merge into one stream of occurrences (`ContractOccurrences`), and what `ScheduleKind` is for;
3. the four implementations of "which occurrence": their inputs, their tie-breaks, their radius, and where they disagree;
4. what `Loan`, `Reset`, `Prepay`, `deposit`, `buys` and `matching` each add to a schedule, in terms of the term constructors:
   a table of "written" to "term", as in PROPOSAL §5 K5, **checked against the code**, not copied from the proposal;
5. which constructors of the proposal's `Term` nothing in today's language needs (`Choose`, `Accrue`, `Let`...) and so
   should not exist yet. A constructor with no source is dead code in waiting.

## Step 1: the oracle

Before the structure: a generator of contracts (`docs/v5/measure/contracts.py`: seeded, deterministic, in the style of
`splits.py` and `tabs.py`), covering every cadence form, `on`, `anchor`, `from`/`until`, grace, `covers`, `prorated`,
escalation by percent and by index, a loan with resets and a prepayment, a deposit, a `buy`, a `standing` schedule beside a
regular one, `ended`. And a dump (like `docs/v5/measure/internals/`) of what the old code says: due days per window,
ordinals, the matched occurrence for a set of probe days, `amount_on` per day. Report how many contracts it generates and
what each form covers. Mutation-test the oracle as K4a did.

## Step 2: `Term`, `Schedule`, `Residual`

The structures, with their `size_of` asserted and their module docs saying why the layout is the one it is. `Schedule`'s
arithmetic is the heart: write it small and prove it against the old code by the oracle.

## Step 3: the compiler

From a lowered `Contract` to its `Term`s, **in the model**, once, at build time. The `Book` gets the terms beside the
contracts. Nothing reads them yet except the oracle's dump.

## Measure

Lines added (this lane adds code: the deletion comes in K5b and K5c, so count what the new code will let go and say so);
`size_of`s; the function-length histogram; the oracle's coverage.

## Not in this lane

- Moving the fold onto residuals, the deadline heap, claims for a missed `Due`: K5b.
- The forecast as the same fold past today, deleting `contract_forecasts`: K5c.
- `sync/promise.rs`: K5b.
