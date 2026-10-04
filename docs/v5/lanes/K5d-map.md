# K5d map: what a loan is today, what its life is as one fold, and where the brief's design meets the code

Written before the first code change of lane K5d, from the code at `17da806` (the merge of K6), and checked against what the
code does (the last section says how). Paths are in `crates/`; line numbers are those of `17da806`. The vocabulary is K5a's,
K5b's and K5c's ([`K5a-map.md`](K5a-map.md), [`K5b-map.md`](K5b-map.md), [`K5c-map.md`](K5c-map.md)): `Annuity`, `Residual`, the
monitor, `Promising`; K4b's `solve`; K3c's and K3d's claims. The baseline binary is built from `17da806`
(`scratchpad/k5d/baseline-axiom`).

## 0. What the brief says and what the code does

Eleven things in the brief do not match the code. Items 1 to 5 decide the design; the rest are facts the plan needs.

1. **A payment is not "one lump to the debt tab". It is one lump to the lender, and the debt tab is touched by nothing a loan
   promises.** `template_header` (lower/contracts.rs:739-772) makes a loan's header `from` the owner's holding to the *party's
   place* (outside), with `Quantity::Derived` (contracts.rs:828, solve.rs:189) for its amount; `Reads::payment`
   (engine/occurrence.rs:721-726) answers it with the level payment. The debt tab (`Loan.debt`, made by `world.tab`,
   contracts.rs:309) moves only by the journal: the origination (`lower_loan_origin`, record.rs:1117-1262: tab to the funding
   account on the loan's day) and a flow written to the contract's name (`special_end`, resolve.rs:247-258: a loan's name
   *is* its tab). Shown on a book (`scratchpad/k5d/p1/a`): a 320,000 loan, two kept payments of 1,892.92 and a 10,000 flow to
   `mortgage`: `checking` pays 3,785.84 and the debt tab reads 310,000.00, the 10,000 and nothing of the payments.
   **`examples/07-landlord` is that bug at scale**: its twelve kept payments and its eleven month-end statements
   (`2025-02-28 home-loan = 278_759.79 USD`, hand-checked, the comment lines say so) all fail today, `lender holds
   279,000.00 USD, not 278,759.79 USD`, eleven of its eighteen errors (golden `07-landlord-check.txt`). So moving the
   principal to the tab is not a re-labelling that leaves totals alone: **liabilities fall and net worth rises by the
   principal of every payment** the fold keeps or the forecast promises, where today the whole payment is an expense that
   leaves the debt as it was.
2. **`Term::At` was not built by K5a.** The brief says K5a built it and nothing uses it. K5a's map (§6) says it is not needed
   and the code agrees: `Term` is `Done`, `Pay`, `All`, `Every`, `Due`, `Annuity` (model/promise.rs:50-64). Nothing to use or
   delete. Said again in §10, where it decides half B.
3. **The loan cannot be a fold of four events walked at fold time by three cursors, because there are two cursors, they
   start at different ordinals, and an occurrence is kept before it falls due as often as after.** A loan is walked today
   in `Residual::advance` (promise/residual.rs:83-95) by `Monitor` (engine/monitor.rs:90-164: settles on a kept line, misses
   after the reach) and by `Promising` (engine/promising.rs:56-117: starts at `today + 1`), each with its own
   `open`. The amount a kept or promised occurrence posts is *neither*: it is the constant `Annuity::payment()`, read by
   `Reads::payment` for any day. A rate change on 03-10, a payment due 03-01 that the monitor finds missed on 03-17 (reach 15
   days, K5b §5) and a prepayment on 03-12 are met by the fold in the order 03-10, 03-12, 03-17: a fold that steps `Pay` when
   it learns of it applies the 03-01 payment after the rate and the prepayment. A line dated 02-28 keeps the 03-01 payment and
   is posted before anything dated 03-01 is known. And `value LOAN = X` on 03-05 asks for the balance **after the payment
   due 03-01**, which the monitor has not stepped yet. §5 is the design that does not have these failures: the loan is walked
   **once, when the book is compiled, over the events the book states, in schedule order**, and every reader (the fold's
   amounts, the monitor, `Promising`, the reconciliation, the reports) reads that walk. There is still exactly one `step` and
   one place that calls it. `Residual` stays a cursor and loses what it has to do with the loan (§5).
4. **A rate `now` changed is not lowered.** `2029-03-01 mortgage now at 6.25%` (LANGUAGE §5, §7) parses
   (`Change::Property`, syntax/ast.rs:865) and is dropped: `lower_statement` ignores it (record.rs:657: "lowered by
   props::declare") and `props::property_changes` (props.rs:677-695) finds no property `at` of a contract and says nothing, so on
   a contract called `home-loan` the line has **no effect and no diagnostic**. On a contract called `mortgage` (the name of
   LANGUAGE's own example) it is `error[unknown-property]: `at` is not a property of kind `mortgage``, because std.ax:28
   declares a *kind* `mortgage` and the statement's subject finds the kind first (props.rs:996-1005). Both shown on the
   baseline (`scratchpad/k5d/p1/c`).
5. **There is no `principal` purpose.** `std.ax:78` has `purpose interest : spending` and nothing for the principal. `#interest of
   condo` is already a valid flow (`flow` prints `spending > interest > of condo`: shown), so the interest leg needs no
   new purpose; `#principal` needs `purpose principal : transfer` in std.ax (no example, mistake or probe declares one:
   grepped), a transfer between the owner's holding and the owner's own debt tab.
6. **`Derivation::Interest(contract)` and `Principal(contract)` are never constructed (journal.rs:455-456), and every reader
   already knows them**: `report/register.rs:384-385,407-408` and `report/why/contract.rs:138-139` label them and
   `contract_flow` (register.rs:402-415) counts them as the contract's own. Nothing needs teaching; the flows need making.
7. **`Residual` does not need `began` and `open` once the loan is a table**; and the loan ends when the table does, which also ends
   K5b's third "least proud" place for loans: `home-loan` of `07-landlord` warns `missed-occurrence` for 2026-01-01, 02-01 and
   03-01 after the house was sold and the loan paid off on 2025-12-29 (golden). A loan whose balance is zero has no more
   payments; the warning goes.
8. **A "flow to the loan's contract" is a flow into the debt tab.** The brief's "on a due day the excess over the scheduled payment; on
   any other day all of it" has two readings of "flow": a plain journal flow (`checking -> home-loan 20_000 USD`) and an
   occurrence line that states its own amount (`2026-03-01 home-loan 3_487.48 USD`, which replaces the scheduled amount this
   once, LANGUAGE §7). Both exist today and both are prepayments in the sense of §6.
9. **`Loan` and `Reset` are `Copy` and carry no list**, so rate changes written as statements (`now at`) are a new field of
   `Contract`, beside `waived` (the other thing statements do to a contract), not of `Loan`.
10. **The examples with a loan are `05-family`, `07-landlord`, `11-sam`, `v4-sketch` and `explore-v5/02-family`** (grepped). Not
    `02-household` and not `10-budgeter`. Only `05` and `07` are goldens. `05-family`'s journal ends 2025-12-31 and its loans
    start 2026-01-01, so on the golden day (2026-04-16) no occurrence is kept or forecast and only the monitor sees them.
    `explore-v5/02-family` has the one ARM (`Priya's 5/1 ARM`, written for a reset) the corpus holds.
11. **The mortgage-interest deduction exists and does not read `#interest`.** `systems/src/us.ax:103,264-272`: `purpose
    mortgage-interest : spending` and `law itemize-interest-and-gifts ... when purpose is #mortgage-interest | #charity`.
    `05-family`'s hand-written journal books its mortgage interest as `#mortgage-interest` and its car loan's as `#interest`.
    Part C is therefore a decision, not a build (§10).

## 1. A loan today, end to end

| stage | where | what it does |
|---|---|---|
| grammar | syntax: `loan AMOUNT on DATE at RATE over SPAN [for ASSET]` with nested `resets`, `prepay` | parsed into `Prop`s; nothing here changes |
| lowering | `lower/contracts.rs:155-166` `loan_endpoint` | the debt tab asked for before any template is lowered, so a template may name a loan declared later |
| | `:281-311` `contract_loan`, `:322-446` `loan_fields`, `loan_principal`, `loan_asset`, `:449-492` `loan_prepay`, `:494-662` `loan_resets` and its four helpers | `Loan { principal, on, term, asset, debt, resets, prepay }` (book.rs:725-739), `Reset` (741-751), `Prepay` (753-761): 380 lines of validation. `Loan.asset`, `.resets` and `.prepay` are **read by nothing** after this |
| | `:828` `schedule_amount` | a schedule that writes no amount has the header quantity `Quantity::Derived` and a zero stand-in in the *base* currency |
| | `:720-734` `lower_terms` | `Terms.rate = loan_rate` (the yearly rate); one `Promised` with the header and none of the loan's legs |
| | `record.rs:733`, `:1117-1262` `lower_loan_origin` | a line `DATE NAME` on the loan's own day is the origination: one flow from the debt tab to the owner holding the schedule pays from (146 lines, the lane's longest function of its own) |
| promise | `promise.rs:287-301` `body` | `Term::Annuity { annuity, body }` around `Term::Pay`, from `Annuity::new(loan, every, terms.rate)` |
| | `promise/annuity.rs` (230 lines with tests) | `Annuity { begins, principal, payment, periods, rate }` (48 bytes): the level payment by the engine's 18-place loop (`payment_factor`, :112-126), `pay(open, index)` (interest `open.scale(rate)`, the last payment is `open + interest`), `owed_after(n)` by stepping. `Paid { interest, principal, open }` is returned and used by `advance` and `owed_after` only |
| | `promise/residual.rs:15-96` | `Residual { term, next, ordinal, began, open }` (24 bytes); `starting_at` steps `owed_after` to find `open`; `advance` steps `pay`, and a loan with `open == 0` is done |
| fold: kept and promised occurrence | `engine/occurrence.rs:721-726` `Reads::payment` | `book.promises.loan(contract).payment()`, the constant level payment, for every due day, **whatever was prepaid or reset**; one flow to the party |
| fold: monitor | `engine/monitor.rs:122-163` | a miss and a kept line advance the stream's `Residual`; a loan is done when `open` is zero |
| fold: forecast | `engine/promising.rs:68-117`, `report/forecast.rs:322-366` | a second `Residual` from `today + 1`; the row's amount is the sum of the occurrence's flows |
| journal flow to the contract | `resolve.rs:247-258` | `checking -> home-loan 20_000 USD` is a flow into the debt tab: a prepayment today in effect, and unread by any schedule |
| assertion | `lower/statements.rs:206-261` `lower_value`, `engine/reconcile.rs:28-94`, `engine/explain.rs:782-846` | `home-loan = X` is an assertion on the debt tab's holding; a gap is `assertion` ("lender holds A, not X", generic causes: a transposition, twice a flow) |
| reports | `report/contracts.rs:96-111` `loan_balance` | "Loan balance" is the debt tab's holding, `:162` the header quantity reads "derived by contract rule"; `register.rs:384-415`, `why/contract.rs:138-139` label interest and principal and are fed none |

Size today (non-test, `loc.py`, `17da806`): cli 2,338, core 3,482, engine 12,024, model 19,081, report 7,029, session 500, sync
4,277, syntax 5,632, systems 14: **54,377**. Functions over 80 lines: 9, of which `lower_loan_origin` (146) is this lane's.

## 2. What the arithmetic is, and what it rounds

The rule is K5a's, which is the engine's before it, which is what the books of the examples were written with
(`07-landlord`'s comment lines: "independently checked from the contract terms"). Reproduced independently
(`scratchpad/k5d/py/ann0.py`, Python integers, no floats): 279,000.00 at 6.75% over 360 months gives a payment of 1,809.59 and,
after the first eleven payments, 278,759.79, 278,518.22, 278,275.29, 278,031.00, 277,785.33, 277,538.28, ... **the example's own
statements, to the cent.** So the rule needs no change and the schedule can be checked against twelve statements of a real
book before any oracle runs.

What is exact and what is rounded, and where (each rounding site gets a test and the module's doc says so):

1. **The yearly rate to the period's rate** is exact: `Ratio` (i64 over i64): `months/12`, `days/365`, `/24` for twice monthly.
2. **The annuity factor** `r / (1 - (1 + r)^-n)` is fixed point at 18 places, half to even **at every multiplication of the loop**
   (a power by squaring would round differently and move cents the books have always had). One site: `payment_factor`.
3. **The payment** is `principal.scale(factor)`: one half-to-even rounding to the commodity's quantum. A *recast* is the same call over
   `open` and the payments left, so a recast at the loan's start reproduces its payment exactly (a property test).
4. **Interest** is `open.scale(period rate)`: one half-to-even rounding, once per payment.
5. **The last payment** is what is left (`open + interest`): exact, nothing is rounded. Principal is `payment - interest` clamped to
   `[0, open]`: exact.
6. **A reset's new rate** is `index + margin` held to `previous ± cap` and to `initial ± life`: exact `Ratio` arithmetic, no rounding.
7. **The payments a payment needs after a shortening prepayment** (§3) is the same recurrence as 4 and 5: no new rounding.

A `Qty` is an `i64` count of quanta, an interest is at most `open × rate`, and a loan whose principal is at most `Qty::LIMIT`
(10^17) at a period rate of at most 100% never leaves `i64` (`open + interest ≤ 2·10^17`): validated once, where the `Annuity` is
made, so that `step` has nothing to check and no `Option` to return.

## 3. How many payments does this payment need

After a prepayment that **shortens** the loan the payment stays and the loan ends sooner. LANGUAGE does not say how much sooner; the
brief says "the number of payments the same payment needs" and that `n = -ln(1 - open·r/payment) / ln(1+r)` is not exact in fixed point.

**The exact integer formulation is the loan's own recurrence.** With `f(open) = open + round(open·r) − payment`, a payment covers
what is owed when `open + round(open·r) ≤ payment`, and `n(open, payment, r)` is the first k at which that holds for `f^k(open)`.
There is no other definition that agrees with stepping to zero, because stepping *is* the loan: the number of payments is a
property of the rounded trajectory, not of the real-valued one.

Proof sketch of what the lane relies on:

* **Monotone.** `round` is monotone and `f(x) − f(y) = (x − y) + (round(xr) − round(yr)) ≥ x − y > 0` for `x > y`: the trajectory of a
  smaller balance stays at or below that of a larger one at every step. So `n` never grows with a prepayment, and **`n(open', ...) ≤` the
  payments that were left before it**. The walk is bounded by a number the state already holds and needs no cap of its own.
* **Terminates and agrees.** The walk stops at the first payment that clears; the loan stepped from the new state then pays `payment` for
  `n − 1` payments and clears exactly on the `n`th, which is the same recurrence run again. That is the property test: `step` applied
  `remaining` times from any state reaches `open = 0` on the last and not before.
* **The last payment is never more than the level one** (it is chosen as the first that fits), where the last payment of an unprepaid
  loan is what rounding left and can exceed it by cents.

**Why not the closed form or a binary search on the factor.** Measured (`scratchpad/k5d/py/needs.py`, `needs2.py`: the same rounding rule in
Python integers). Prepayments of random size at random points of 1,200 random loans (6 terms, rates 0.01% to 12%): the factor search
(smallest `n` whose level payment `principal.scale(factor(r, n))` is at most the payment) and the float `ln` formula agree with stepping in all
1,200. At the boundary, with 4,000 loans each given the balance that `m` more payments of the payment *almost* retire, ±3 cents (28,000
cases): **the factor search is one payment off in 10,004 (36%) and the float formula in 4,558 (16%)**, always by one. The off-by-one
leaves a remainder of up to a whole payment that the loan's "the last payment is what is left" rule then adds to the last payment, which is
nearly twice the payment. The recurrence is O(remaining) (360 steps of one `mul_div` for a 30-year monthly loan, about 10 µs) and is run
once per shortening prepayment, a thing a person types.

**A recast** (`prepay recasts`) keeps the payments left and refigures the payment: `open.scale(payment_factor(r, remaining))`, exactly the
call that made the first payment.

## 4. The state, the events, the step

```text
State { open: Qty, rate: Ratio, payment: Qty, remaining: u32 }     8 + 16 + 8 + 4 = 36, aligned to 40 bytes (asserted)
Event { Pay, Prepay(Qty), Reset(Ratio), Rate(Ratio) }              24 bytes, never stored
Paid  { interest, principal, open }                                 24 bytes
Annuity::step(&self, State, Event) -> (State, Paid)                 pure, total, no allocation
```

Where it departs from the brief:

* **The events carry no day.** The arithmetic of a payment depends on the balance and the rate, never on the day: interest is a period's, not
  a day count (K5a's rule: `months/12`, `days/365`). The day belongs to whoever orders the events (§5), and a `Pay` on 03-01 and on 03-03
  step identically. A day in the event would be a field `step` never reads.
* **`step` is a method of the loan's terms (`Annuity`)**, not a function of two values, because a `Reset` needs the margin, the caps and the
  initial rate, `Rate` and `Reset` need the period fraction of a year, and `Prepay` needs the mode (`Shortens` or `Recasts`). None of that
  can live in a 40-byte state. It takes the state and the event, as the brief says; the terms are what it is a method of.
* **`Reset` and `Rate` are two events because only one of them is clamped**, as the brief says. `Reset(index)` is the index's value and
  the step computes `index + margin` held by the caps; `Rate(yearly)` is a statement's number, taken as it is. Both refigure the payment over what is left.
* **Events are valid by construction at the boundary:** a rate above 100% a year is held at 100% by `step` (so that the 18-place loop
  cannot leave `i128`: checked once for the loan's longest term at the highest rate, where the `Annuity` is made, and refused there with the
  existing `UnsupportedLoan`).

## 5. Where the walk lives, and why

`Annuity::new` gives the terms. **`Promises::compile`** (run once, at the end of `build`, after the journal is lowered and `Book.touching` is
built) gives each loan **its schedule**: the entries of one walk of `step` over the events the book states, in order, in one flat pool of the
`Promises` (`Entry { day, kind, paid }`, 32 bytes, a `Run<Entry>` in the `Annuity`, as every variable-length thing of a promise is).

The events, merged by `(day, rank)`:

| rank | event | where it comes from |
|---|---|---|
| 0 | `Reset(index)` | the loan's `resets EVERY from DATE to PARAM + PERCENT`: `from`, `from + EVERY`, ... (each counted from `from`, never from the one before, as the schedule counts), the index read from the book's params on that day (`index_on`, promise/reckon.rs:126) |
| 1 | `Rate(yearly)` | a statement `DATE LOAN now at PERCENT`, a new `Contract.rates`, lowered in `(day, source order)` |
| 2 | `Pay` | each due day of the regular schedule after the day the loan was made (`Sched::days`, so a waiver is a hole, not a payment), as many as `remaining` says |
| 3 | `Prepay(amount)` | a flow into the debt tab (`Book.touching`) that no occurrence of the contract made and that is `Actual`; the excess of an occurrence line's literal amount over the payment of its due day (§6) |

A rate that changes on a due day applies to that day's payment; a prepayment on a due day comes after that day's payment. A prepayment between
due days applies at once: the next payment's interest is of the reduced balance (K5a's rule has no day count, and a statement taken the day
after a prepayment shows it). The walk stops when the balance is zero; an event after the payoff is ignored.

**Why a table and not three cursors with a `State` each**, beyond §0.3:

* The reader of the loan is **one function over one array**: the amount a kept or promised occurrence posts (`paid_on(due)`), the balance on any day
  (`open_on(day)`, a binary search), whether a loan has another payment (`pays(ordinal)`), the reconciliation, `why`. The fold, the monitor and the
  forecast cannot disagree because there is one answer, not three states that should be equal.
* A rate change, a prepayment or a payment kept early or late changes **a table entry, not the order in which the fold met things**: the schedule is a
  function of the book alone, so a kept line dated before its due day, a miss found after the reach, the forecast from today and the balance
  on a statement day all agree, and the oracle compares **every entry**.
* The engine files `ledger.rs`, `state.rs`, `post.rs` and `lots.rs` are lane K7b's this week; a loan that lived in the fold's world would be edited in them.
  This design touches `occurrence.rs`, `monitor.rs`, `promising.rs`, `reconcile.rs` and `explain.rs` only.
* Cost: nothing for a book with no loan (`compile` finds none and walks no flow); for a book with loans, one pass over the flows touching each debt tab and one
  walk of at most the loan's payments. The 100k and 1m benches hold no loan.

What the walk cannot see, said: a prepayment that is a *promised* flow of another contract (an `also -> home-loan 500 USD` extra principal) or a hypothetical flow applied
to a ledger (`Ledger::apply`); the debt tab moves and the schedule does not, and the reconciliation names it. A flow later returned or voided (`^code returned`) is still
counted: the schedule is a function of what is written. A tab shared by two loans (the same lender and owner: one tab per `(party, owner, class)`, K3a) takes its
prepayments for the earlier declared.

`Residual` is `{ term, next, ordinal }` after this lane (12 bytes where it was 24): the cursor over the schedule's days, which says it is done when the loan has
no payment at that ordinal. `Annuity::pays(ordinal)` is the only question the monitor and `Promising` ask of the loan.

## 6. What a prepayment is

A prepayment is anything that pays principal beyond the scheduled payment of its due day:

1. **a flow into the debt tab** (written `checking -> home-loan 20_000 USD`, the loan's name being its tab) that is not an occurrence's own, on **any day, all of it**,
   including a due day: the due day's payment is the occurrence (a line `DATE home-loan`, or the promise) and this flow is more;
2. **an occurrence line that states its own amount** (`2026-03-01 home-loan 3_487.48 USD`): the occurrence posts that amount, its interest is the schedule's
   and its principal is what the amount leaves, so the flows are right whatever the amount is; **on a due day, the excess over the scheduled payment**
   is the prepayment the schedule takes, dated that due day, after the payment. A line that states **less** than the scheduled payment pays interest first and
   changes nothing in the schedule: the schedule is what the loan says, the difference is what the reconciliation finds (§8).

A prepayment is a literal amount, in the loan's commodity, on or after the day the loan was made. An amount larger than what is owed pays the loan off. What a flow *from* the
debt tab is (a draw) is the origination's shape: it moves the tab and is not an input of the schedule, which has four inputs and none of them borrows.

Mode: `Shortens` (the default) keeps the payment and recomputes `remaining` (§3); `Recasts` keeps `remaining` and refigures the payment.

## 7. A payment is a split

A loan's template (lowered where `schedule_amount` finds no written amount and the schedule is `from` a holding) is a promise's group, which K4b's
`solve` already knows how to solve:

| | flow | amount | purpose |
|---|---|---|---|
| header | owner's holding → the **debt tab** | `Quantity::Derived`: the schedule's payment of the due day | `#principal` |
| leg | the same source → the lender's place | `Part::Of(Quantity::Interest)`: the schedule's interest of the due day | `#interest`, `of ASSET` when `for` names one |

The header keeps what the leg leaves: **principal is the remainder**, so the legs add up to the payment by construction, on every payment, the last one included.
A leg of nothing (a 0% loan) is omitted, as a leg that reads an unbound input is. Written legs and items under the schedule keep their meaning in the same group
(a leg carves the payment; `+ 410 USD #escrow` is an escrow on top). `Quantity::Derived` keeps its two other roles (a statement's header with no total, record.rs:1422).
`Env` gains `interest` beside `payment`; both are answered from the schedule by the due day of the occurrence, and a leg's amount is held to what the header has left, so an
occurrence line of less than the interest never makes a negative flow.

Flows are made with `Origin::Derived(Derivation::Principal(contract))` and `Interest(contract)`: the registers already label them (§0.6).

Brief check: **the sum of the interest and principal legs equals the old payment on every payment of every example** (a test): for a payment the schedule
makes, `interest + principal` is the payment, and the old payment is the level payment for all but the last, so the assertion is "the new payment is the old one"
for every payment of an unprepaid, unreset loan, and the last payment of the old walk (`open + interest`) is the old code's `Annuity::pay` result, which the new walk
shares. Where the schedule differs from the level payment (prepaid, reset, rate-changed) there is no old payment to equal: no example has one.

## 8. The reconciliation

`value LOAN = X` is an assertion on the debt tab and is checked as it is today (`T` against `X`). **New: when the place is a loan's debt tab, the statement is also held to the
schedule** (`S`, its balance on that day: `open_on(day)`): a difference is `error[loan-balance]` that names the statement, the schedule, the day and the difference, the payments
of the schedule around it, and **the likely cause when exactly one candidate explains the difference to the cent**. Candidates, all exact integer tests against the table:

| candidate | holds when | the diagnostic says | the fix, as an edit |
|---|---|---|---|
| **a payment was missed** | `X − S` equals the principal of the last `k ≥ 1` payments due on or before the day, and no line of the journal keeps at least one of them | "exactly the principal of the payment due 2026-02-01 (478.17 USD), which no line keeps" | `2026-02-01 home-loan`, or `2026-02-01 home-loan waived` |
| **an unrecorded prepayment** | `S − X > 0` | "exactly the 5,000.00 USD of a prepayment nobody wrote" | `DATE ACCOUNT -> home-loan 5_000.00 USD` |
| **an extra was counted as a prepayment** (an escrow in the draft) | `X − S` equals the excess of one occurrence line of the loan over its payment (or the sum of them) | "the 2026-02-01 line states 410.00 USD over the payment, and the schedule took it as principal; if it paid an escrow, write it as `also -> escrow 410 USD #escrow` and the line without its amount" | the line without its amount |

When none holds, or two do, the diagnostic names the numbers and the nearest payments and says no single cause explains it (no guess). A `!` or `via` on the assertion says the
gap is accepted and silences the schedule's check, as it does the tab's. A statement before the loan was made, or after it was paid off, has no schedule balance of its own: it is
compared with zero.

Not a candidate, said: an interest accrued since the last payment (a payoff quote, which has no exact test), a rate change nobody wrote (it changes the payment and the interest of every
payment after, and the statement alone cannot say which), a draw (the schedule has no input that borrows).

## 9. The examples, and what each will move

Measured on the baseline (`scratchpad/k5d/base-out/`) and to be measured again after the lane (§13 of this map, written last):

| example | loan | today | after |
|---|---|---|---|
| `05-family` | `mortgage-payment` 406,692.02 at 5.875% over 27y6m from 2025-12-31; `car-payment` 16,976.91 at 4.9% over 30m | golden day 2026-04-16: no occurrence kept or forecast; the monitor warns (`due 3 times`) for both | goldens byte-identical (the monitor says the same); `forecast` and `contracts` move (interest and principal, a loan balance); `forecast.txt` is not a golden |
| `07-landlord` | `home-loan` 279,000.00 at 6.75% over 30y, origination 2024-12-18, twelve payments kept, a payoff flow of 276,282.05 on 2025-12-29 | 18 errors, 3 warnings: 11 are `lender holds 279,000.00 USD, not ...` | those 11 errors and the `home-loan` warning go (the schedule and the book agree with every statement); balance sheet and tax lines move with the principal that now leaves the debt |
| `11-sam`, `v4-sketch` | `320_000 USD at 5.875% over 30y for condo` | not goldens | read, not compared |
| `explore-v5/02-family` | the 5/1 ARM with `resets`, three more loans | not goldens | the first real `resets` book |

`02-household` and `10-budgeter`, which the brief guessed, have no loan. No mistake book has one.

## 10. Which half, and why

**A is built; C is a decision; B stops at the map.**

* **A** entire: the step, the schedule, the payment as a split, resets, `Rate`, prepay both ways, `for ASSET`, the reconciliation, the reports.
* **C** (`for ASSET` and the tax line): `#interest of ASSET` is built with A. The mortgage-interest deduction **exists** (§0.11) and **reads `#mortgage-interest`**, not
  `#interest`. Making the loan's interest count toward it would be a choice of the user's (a loan `for house` writes `#mortgage-interest`? `purpose mortgage-interest : interest`
  in us.ax so that both count? the deduction reads `#interest of ASSET` for an asset of kind home?), and the examples' own journals disagree (`05-family`: `#mortgage-interest` for the
  house, `#interest` for the car). Not invented; listed in the report as a decision.
* **B** (`deposit`): built only if it can be done without debts as parcels. It cannot be done whole: a deposit the owner pays (`from` a holding: a claim on the party, in an *Asset* tab, which K3c
  supports) needs the first-occurrence flow, `Term::At` for its return at the contract's end (an overdue promise like any other, so a new term constructor, a one-shot stream in the monitor and in
  `Promising`) and its own oracle; a deposit the owner receives (`into`: a debt to the tenant, held in the named holding) is a *Debt* tab, a plain balance whose settlement by the relief order is K3f's.
  Building the first half now would put two mechanisms where one belongs. The design for both, for the lane after K3f:
  `deposit AMOUNT [into HOLDING]` is one `Term::At { day, body }` per direction, a flow at the contract's first occurrence between the holding and a tab of the pair, and a return flow at the contract's last day
  that the monitor waits for (so a deposit not returned is `missed-occurrence`, and an Asset tab's parcel is settled by the same `exact`/`code`/`oldest`); K3f makes the second tab hold parcels so the order is one.

## 11. The plan, and what proves each step

| step | what | proof |
|---|---|---|
| 0 | this map; the baseline binary | |
| 1 | the oracle before the code: `docs/v5/measure/loans.py` (an independent ACTUS ANN in Python integers with K5a's fixed-point rule; seeded generators for loans of every cadence, prepayments in both modes, rate changes, resets with caps, a final payment, a loan paid off early, a missed payment; the reference answer for every payment's interest, principal and open balance, and for the monitor's overdue day) | it reproduces `07-landlord`'s statements; its own tests |
| 2 | `model`: `State`, `Event`, `Annuity::step`, the number of payments a payment needs; `purpose principal`; tests per rounding site | unit tests; the property tests of §3 (stepping `remaining` times ends at zero; a recast at the start is the first payment) |
| 3 | `model`: `Contract.rates` and `now at`; the schedule (`Promises::compile`); `Residual` loses `began`/`open`; the template's split; `Quantity::Interest` | model tests; the oracle's dump of the schedule against the reference |
| 4 | `engine`: the occurrence reads the schedule; the monitor and `Promising` walk the cursor | engine tests; the oracle's flows and monitor lines |
| 5 | `engine`: the reconciliation | tests for each cause and for none; the oracle's statements |
| 6 | `report`: `contracts`, `why contract`; goldens in their own commit | `git diff tests/` listed line by line |
| 7 | the oracle's mutation sweep; `fuzz.py ... diff`, `splits.py`, `claims.py`; timings | mutants killed by the oracle or a named test; zero differences on books with no loan |
| 8 | the map's last section; LANGUAGE §7 | |

## 12. How this map was checked

* Every reader of the loan types (`grep` of `Annuity`, `Residual::open`, `Loan`, `Reset`, `Prepay`, `.loan` over `crates/` and `docs/v5/measure/`) at `17da806`.
* The loan path read in full: lower/contracts.rs:155-166, 281-662, 690-735, 739-831; record.rs:710-736, 1117-1262; promise.rs and promise/{annuity,residual,reckon,schedule}.rs; engine/{occurrence,
  monitor,promising,reconcile}.rs and explain.rs:782-846; report/{contracts,register,why/contract}.rs.
* §0.1, §0.4 on the baseline binary: `scratchpad/k5d/p1/{a,b,c}`; §0.1's 07-landlord from `tests/golden/07-landlord-check.txt`; §2 and §3 in Python integers (`scratchpad/k5d/py`).
* The baseline: `cargo build --release` at `17da806`, copied to `scratchpad/k5d/baseline-axiom`; `loc.py`, `hist.py` and `fnlen.py` of the same tree.

## 13. What was built, what the map got wrong, and what moved

Written last, from the tree and the runs. Commits, in order: the map, the oracle before any code, the code (model, engine, report in one: they do not build apart),
the goldens of `07-landlord` with every moved line explained, the mistake books, LANGUAGE §7, the oracle's harness and the K5b walk, and three small ones after the sweep found two
cases no test named.

### 13.1 What the code is

* `model/promise/annuity.rs`: `Annuity` (what a loan is), `State` (40 bytes: `open`, `rate`, `payment`, `remaining`), `Event` (`Pay`, `Prepay(Qty)`, `Reset(Ratio)`, `Rate(Ratio)`), `Paid`,
  and `Annuity::step(State, Event) -> (State, Paid)`: pure, total (the boundary is checked once in `Annuity::new`), allocation-free. Its four rounding sites are named in the module's doc and each has a test.
* `model/promise/amortization.rs`: the schedule. One walk (`walk`) over every event the book states, in the order of day and rank (reset, rate, payment, prepayment), into a flat pool of 32-byte `Entry`
  (`Promises.entries`); `Amortization` is the reader (`paid_on`, `open_on`, `payments`). It is walked when the promises compile and never again.
* `model/promise/causes.rs`: `Promises::reconcile`: the statement against the schedule, and the cause named only when exactly one candidate explains the gap to the quantum.
* `model/promise/residual.rs` is the 12-byte cursor it was, minus `began` and `open`; it asks the schedule whether a payment is owed at its ordinal.
* `model/book.rs` `Contract.rates` and `lower/statements.rs` `DATE LOAN now at 6.25%`; `split.rs` and `solve.rs` `Quantity::Interest` (`Env::interest`); `lower/contracts.rs` `split_loan_payment`.
* `engine/occurrence.rs` reads the schedule for the payment and its interest; `engine/loan_balance.rs` is the diagnostic; `engine/reconcile.rs` asks it. `report/contracts.rs` and `report/why/contract.rs` show it.
* `std.ax` gains `purpose principal : transfer` **at the end of the file** (a line anywhere else moves every golden that quotes std). It is now a **built-in name**: a book that declares its own
  `principal` purpose collides with it, as `claim` did for kinds.

### 13.2 Where the code differs from this map

* §4 said `State` lives in `Residual`; it does not. The schedule is a table (§5, approved) and the `Residual` stays a cursor. `Event` carries no day (the arithmetic is day-free), and `step` is a method of
  `Annuity`, which holds the terms it needs, not a free function of three arguments.
* **§7 said the legs add up to the old payment "on every payment, the last one included". That is wrong for the last payment of an unprepaid loan.** The old engine paid the level payment again
  and left a few cents of rounding on the debt; the new last payment is what is left (`open + interest`), as ACTUS has it and as §2 item 5 already said. The legs add up to the old payment on every
  other payment, and `crates/session/tests/loans.rs` asserts it on every payment but the last of the 12 loans of the examples (2,286 payments, before any reset or rate a book says). The last payments differ by:
  `05-family` mortgage -3.23 USD (2053), car -0.02 (2028); `11-sam` and `v4-sketch` mortgage +0.80 (2054); `explore-v5/02-family` mortgage-2 +0.71, car-loan -0.31; `03-triplex` +0.60.
  No output of any example reaches those dates. Stopped and reported, as the brief says.
* §8's candidates are four and a tie: `Cause::{Missed, Short, Extra, Prepaid}`, plus `Several` (two explain it alike) and `Unknown` (none does). `Short` was a candidate the map did not list: a line that
  states less than its payment leaves principal unpaid in the book that the schedule paid.
* A line that states **more** than its payment is a prepayment of the difference in the schedule (the map's "excess"), for a literal amount in the loan's commodity; a computed or pending amount is not read.

### 13.3 What changed in the outputs, and why

Goldens: only `tests/golden/07-landlord-{check,balance,available,limits,claims,tax}.txt`; the commit message of that commit lists every line. No other golden moved. No mistake book moved (seven are new).

| example (day) | before | after | why |
|---|---|---|---|
| `07-landlord` check (2026-04-16) | 18 errors, 3 warnings | 7 errors, 2 warnings | the 11 statements of `home-loan` that said `lender holds 279,000.00 USD, not ...` agree with the schedule to the cent (all of 2025: 278,759.79 ... 276,282.05); the `home-loan` warning (`due 3 times ... last on 2026-03-01`) is gone, the loan having been paid off on 2025-12-29 |
| `07-landlord` balance | liabilities 16,917.95; net worth 152,248.65 | liabilities 14,200.00; net worth 154,966.60 | the debt tab falls by the principal of each payment and not by nothing; the interest is an expense |
| `07-landlord` tax 2025 | rental-expenses 11,495.08 (21 sources), rental-net 13,229.92, total-income 87,305.92, total-tax 16,315.89, owed 1,735.89 | 28,682.62 (32), -3,957.62, 70,118.38, 12,534.62, -2,045.38 (a refund) | the loan is `for house`, so each payment's interest is `#interest of house`, which `rental.ax`'s `rental-expenses` law already counted; it was never posted. 17,187.54 USD over the 11 scheduled payments: the figure `examples/07-landlord/README.md`'s independent verifier gives |
| `05-family` | check and balance identical (46,463.25 net worth; liabilities 425,770.63) | | no payment is kept before 2026-04-16 |
| `05-family` `contracts` | blank loan balance | 404,691.86 (mortgage), 14,831.43 (car); the terms read "the loan's payment; split ... the loan's interest" | the schedule's balance on the day |
| `05-family` `forecast` | | every month's balances move (the interest leaves, the principal pays the debt) | the split |
| `11-sam`, `v4-sketch` | liabilities 760.71 / 634.19 | -334.42 / -460.94 | **the loan has no origination and no opening**: the debt tab starts at zero and the principal legs take it below. See 13.5 |
| `explore-v5/03-triplex` | 69 errors | 73 | four `loan-balance` errors: the contract says `first payment 2022-10-01` in a comment and has no `from`, so the schedule's first payment is 2022-09-01 and every statement is one payment's principal off (511.39 on 2026-01-31). With `from 2022-10-01` added the four statements agree to the cent with the author's own hand arithmetic and the book has 66 errors. Not edited (it is not mine); the one-line fix is verified |
| `explore-v5/02-family`, `05-budgeter`, `06-family-addresses` | | check and balance identical | |

Not goldens, not compared: the forecast view and the `why contract` page of every loan (a "Loan schedule" section is new).

### 13.4 Numbers

| what | result |
|---|---|
| non-test Rust lines (`briefs/loc.py`) | 54,377 to 55,102: **+725**, not the +350 the plan said. `engine` 12,024 to 12,233 (+209), `model` 19,081 to 19,549 (+468), `report` 7,029 to 7,077 (+48), the others unchanged |
| where the 725 are | `engine/loan_balance.rs` +191 (the diagnostic: six causes, each said in the book's words with its edit), `model/promise/amortization.rs` +187, `annuity.rs` +115 (rewritten: 91 to 206), `causes.rs` +75, `report/why/contract.rs` +47 (the "Loan schedule" section, which the plan listed and the brief did not), `lower/statements.rs` +37 (`now at`), `promise.rs` +34, small edits +40 |
| what it deleted | **13 lines** (`Residual`'s `began`/`open`). The old payment path was one `Annuity` of 91 lines and a cursor; there was no second implementation of the loan to remove, and the lump's flow became the split's header. The lane added a mechanism where there was a constant |
| function lengths (`hist.py`) | 1-10 lines 2,066 to 2,111 functions; 11-20: 723 to 731; 21-40: 514 to 523; 41-80: 128 to 128; 81-160: 8 to 8; over 320: 1 to 1. No function over 40 lines was added or lengthened (`cause` of the diagnostic was 60 and is six functions; `schedule_section` 47 and is 30; `lower_terms` 48 and is 45) |
| the oracle (`loans.py`) | an independent ACTUS annuity in Python integers; seeded books of one loan over eight cadences (monthly, quarterly, every 6m, every 12m, twice monthly, every 2w, every 45d, every 10d), prepayments in both modes, rate changes, resets with cap and life, lines that state more or less than their payment, missed payments, loans paid off early, statements of every cause. **4,500 projects (1,500 of seed 7 and 3,000 of seed 11), 0 disagreements**. The 1,500 of seed 7 ask 105,356 payments' interest, principal and balance, 3,642 prepayments, 54,175 month-end balances, 28,017 posted splits, 13,568 forecast splits, 3,805 missed days, 1,345 `assertion` and 1,704 `loan-balance` diagnostics, each cause asked (`extra` 265, `missed` 315, `short` 45, `prepayment` 363, `none` 716) |
| the K5b promise oracle | ported to read the schedule (`promises/walk.rs`); 800 projects, 0 failures |
| `fuzz.py diff`, books with no loan | 5,000 mutants of the examples without a loan (two seeds): 0 differ between the baseline binary and this one, 0 panics. 3,000 mutants of all the examples: 0 panics |
| `splits.py run` | 1,000 projects, 27,691 commands: 74 projects differ, **all 74 have a loan** (79 projects have one; 5 are unmoved: no payment kept, no view shows it) |
| `claims.py` | 600 projects dumped by the baseline build and this one (the CLI's reports and the dump's account of every parcel, gain, part and diagnostic): all 600 byte-identical, the one with a loan (paid in kind) included. (The harness's "should differ and do not" column compares a build before K3c with one after it, and both of these are after.) |
| `axiom check`, three runs, the fastest | `bench/` generated at 100k (100,030 flows) and 1m, no loan in either; the baseline binary and this one interleaved, three runs each, fastest: **100k 0.358 s and 0.362 s; 1m 3.811 s and 3.912 s** (peak RSS 665 MB both). Nine more interleaved runs on 1m: fastest 3.906 s baseline, 3.827 s new; median 4.245 s and 4.107 s. Load average 2.2 to 2.8 (another lane was building); the runs differ by their noise (3%) and not by a direction. No slowdown |
| tests | `cargo test --workspace --release`: only the two known failures (`a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`, `a_context_forecast_keeps_historical_and_same_day_obligations_once`); `native_loan_forecast_stops_after_the_typed_principal_is_repaid` passes; `tests/mistakes/` and `tests/golden.sh` regenerate to the committed files |

### 13.5 The mutation sweep, and what was not killed

`loans.py mutate` copies the tree, changes one line of it by one text at a time, builds the dump of the mutant, and asks the oracle (the engine's lines against the reference's, on the first 300 books) and, if the oracle does
not see it, the tests of the model and the engine (an unoptimized build; the four tests that fail unoptimized with no mutant at all are left out: an overflow in the solver property test's reference at `solve.rs`, the two known failures and a kind-totals test, none of them this lane's). 80 mutants are written (a 81st was dropped with the code it changed, below) and every text is checked to occur once.
The sweep was stopped, as agreed, after **the step's (38: the payment, the four rounding sites, a prepayment in both modes, a rate, a reset, the terms) and the schedule's (24: the order of the events of a day, the walk, what a line that
states an amount adds, the table's readers, the first payment and the payments owed)**: 62 mutants.

| outcome | mutants |
|---|---|
| killed by the oracle | 39 |
| killed by a unit test that names it | 14 (`a_payment_that_does_not_cover_the_interest_pays_no_principal_and_owes_no_more`, `a_prepayment_is_held_to_what_is_owed_and_paying_it_all_ends_the_loan`, `a_rate_refigures_the_payment_over_what_is_left_and_is_held_between_nothing_and_everything`, `how_many_payments_follows_the_cadence`, `the_factor_is_fixed_point_at_eighteen_places_rounded_at_every_step`, and others) |
| survived both | 9, of which 7 are now killed by a test written for each (below), 1 is equivalent, 1 was code that does nothing |

The 7: `the_factor_rounds_the_rate_and_every_step_of_its_loop_and_truncates_neither` (the loop's and the rate's truncation: the oracle cannot see a rounding at the 18th place that changes no cent in a
million payments), `a_rate_said_on_the_day_of_a_reset_is_the_one_the_payment_of_that_day_is_at`, `a_rate_said_on_the_day_the_loan_was_made_is_the_rate_from_its_first_payment`,
`a_reset_on_the_day_of_the_last_payment_is_the_rate_of_that_payment`, and `a_line_that_states_exactly_its_payment_or_another_commodity_prepays_nothing` (two mutants). Two more tests, for a payment that clears what is owed before
the last and for the number of payments a balance needs, came from the first, interrupted run of the sweep and kill three mutants between them.

**Not killed, and why.** (1) `if state.open == Qty::ZERO { continue }` for `break` in `walk`: **equivalent**; once nothing is owed the condition holds for every later event, so `continue` skips them all as `break` does (it
is slower, by the events). (2) The filter `txn.occurrence.is_none()` in `Said::paid_into`: **no flow can fail it** (an occurrence's flows are made when it is posted and are in no book); the filter is deleted, and its mutant with it.

**Not run**: the 19 of the causes (9), the occurrence's reading of the schedule (3), the split in the lowering (4), `reconcile` (1) and `loan_balance` (2). They are in `MUTANTS` and each text is checked against the tree. What stands behind
that code instead is the oracle's diagnostics (5 causes asked 1,704 times) and the seven mistake books.

The sweep's first copy of the tree kept the files' old times, and cargo built the second run of it with the first run's last mutant; `mutation.py` now copies with new times (found when the unmutated tree "failed its own oracle").

### 13.6 What is not finished, and the decisions that are not mine

* **Part B (`deposit`) stays at §10.** It needs K3f (a deposit the owner receives is a debt tab, whose settlement is the relief order) and a one-shot term. Nothing of it is built.
* **Part C: the mortgage-interest deduction.** `#interest of ASSET` is posted (07-landlord's tax moved by it), and `rental.ax`'s `rental-expenses` already counted it. `us.ax`'s itemized
  deduction reads `#mortgage-interest`, which no loan writes. The options, not chosen: (a) a loan `for` an asset of a home kind writes `#mortgage-interest` instead of `#interest`: the rental law would
  then stop counting it unless `rental.ax` is changed too; (b) `purpose mortgage-interest : interest` in `us.ax`, so that one posting counts for both, and the loan says which it is; (c) the deduction reads
  `#interest of ASSET` for an asset of a home kind, as the rental law does, and the shares of the asset (`03-triplex`: 64.06% rentals) decide how much of it is personal. The examples do not agree
  with one another (`05-family`'s own journal books the house's interest as `#mortgage-interest` and the car's as `#interest`).
* **A loan that predates the book has no opening balance.** LANGUAGE §5 says a loan's balance "comes from its terms and needs no opening line", and the schedule can now say it
  (`11-sam`'s own comment says `312,441.12 owed to rocket, from its terms`; the schedule's balance after 22 payments is exactly that). Nothing opens the debt tab with it: a loan made before
  the book began, with no `DATE loan` line, starts at zero, and its principal legs take the tab below it (`11-sam` -334.42, `v4-sketch` -460.94 liabilities). It is a design question (what is the
  opening day of a loan?) and the code is in `ledger.rs`/`state.rs`, outside this lane's files.
* **Statements that follow another convention.** `03-triplex` is four `loan-balance` errors by one missing `from` (13.3). The first payment of a loan is the first owed day after it was made (K5a's
  rule); real mortgages skip a month, the language has `from` for it, and nothing warns when a comment says otherwise.
* The causes and the fold's mutants (`causes.rs`, `occurrence.rs`, the split in `lower/contracts.rs`, `reconcile.rs`, `loan_balance.rs`: 18 mutants) are written and validated against the tree and were
  **not run**: the sweep was stopped after the step's and the schedule's, as agreed. Their unit tests (the model's `loans.rs`, the engine's `loan_tests.rs`) and the seven mistake books name each cause.

### 13.7 The three places I am least proud of

1. **The debt tab of a loan that predates the book** (13.6): the lane makes `Liabilities` negative in two examples, which is correct arithmetic on an incomplete book and not a thing I should have left for the next lane to find. The schedule knows
   the balance; nothing uses it to open the tab.
2. **`engine/loan_balance.rs` and the dump that reads it.** 191 lines of prose in code, and the oracle learns the cause from the *words* of a note (`CAUSES` in `loans/main.rs`): a test of the wording is a
   test of the oracle's phrases. `Run` should carry the structured disagreement and the report should word it. It is also the largest part of the +725 lines the plan did not expect.
3. **A short payment is the book's and not the schedule's.** A line that states less than its payment changes nothing in the schedule (the loan is paid as the lender's terms say), while the debt tab holds
   only the principal the line paid; the two disagree by the shortfall, and `Cause::Short` is how the diagnostic reconciles them. It is a defensible rule that makes the schedule a statement of the
   terms rather than of what happened, which is also why `contracts` shows 404,691.86 for a mortgage with no payment kept. The cost of reading the book is also there: `Said::of` reads every transaction, once per loan at compile and once per
   statement that disagrees.
