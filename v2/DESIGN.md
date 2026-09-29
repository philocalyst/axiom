# Axiom — design

Axiom is a typed, plain-text ledger. It answers one question first — *how much
money is actually available to spend* — and then every harder one the same way:
what you own, what you owe, what the rules you live under say about each move you
make, what you will owe the government for it, and where it is all heading.

Everything below follows from a handful of concepts that compose. There is no
second engine for taxes, no budgeting subsystem, no liquidity model, no forecast
language. Those are consequences.

```text
place     where value can be          assets/checking, expenses/food, income/salary
entity    who                          me, acme, landlord, irs, nsf-grant
commodity what                         USD, VTI, BTC, HOUSE
kind      what a thing *is*            bank, 401k, 529, grant, stock, person
flow      value moving, once          2026-01-15 acme -> checking 3_900 USD
parcel    value at rest, remembered   7 VTI in brokerage, basis 1_960 USD, since 2026-01-22
law       what must hold              contributions per year <= limit[year]
system    a body of kinds and laws    us, us/ca, us/401k — written in Axiom itself
```

## 1. Flows are the only facts

A journal line records one thing: value moved from one place to another.

```text
2026-01-18 checking -> food 84.20 USD / trader-joes
```

`from -> to` makes every flow balanced by construction — there is nothing to
balance and no balancing account to invent. A *transaction* is a set of flows
written together. One side may be split:

```text
2026-01-15 acme -> 5_200 USD
  retirement      800 USD
  taxes/federal   910 USD
  checking        ...
```

An exchange names what leaves and what arrives; the price is implied, the cost is
exact, and the second place is always the target:

```text
2026-01-22 checking 2_000 USD -> brokerage 7 VTI
```

Flows have a *mode*. Written in parentheses, a flow is **pending** — a check
written but not cashed. It already reduces what you can spend but not what the
bank reports. Plans (`every month …`) are **planned** flows that exist only in
forecasts. Laws produce **owed** flows (taxes, penalties) that are real
obligations but not yet cash. The journal itself never holds anything but authored
facts; nothing is generated into it. A named plan is a template the journal
instantiates in one line (`2026-01-16 paycheck`), so what repeats is written once
and every occurrence is still an authored fact.

A flow has **two times**: the day value moves, and the period it belongs to.
They are usually the same day. An annual insurance premium is paid in January
and recognized over the year (`2026-01-01..2026-12-31`); an estimated tax paid
in January is `for 2025`. Balances and parcels follow the first; tallies,
budgets, window totals and every report about a period follow the second. That
one distinction is spreads, accruals, prepaid costs, depreciation schedules and
"this payment is for last year".

**`for` says what a flow is on account of**: a period (`for 2025`), a claim it
settles (`for #inv-12`), or someone the money is held for (`for car-fund`).

## 2. Parcels remember where value came from

A place does not hold a number. It holds **parcels**: a quantity of a commodity
with a *basis* (value already accounted for — cost for a stock, contributions for
a 529, zero for pre-tax 401k money), the day it was acquired, the transaction that
brought it, and optionally the restricted source it is still tied to.

Parcels whose distinguishing attributes are equal merge. Plain money in a checking
account is one parcel, so the common case costs one integer. **Fungibility is
derived**: two parcels are interchangeable exactly when nothing about them differs.

This one structure is:

- **lots** for securities (FIFO/LIFO/HIFO/pro-rata or `[2024]`, `[#house]` selectors);
- **basis tracking** for tax-advantaged accounts: a 529 withdrawal's earnings
  portion is literally the gain on the parcels leaving, relieved pro-rata;
- **restricted money**: a grant's dollars stay tied to the grant after they land
  in checking, and spending them on the wrong thing is a violation. An envelope
  or a sinking fund is the same thing laid on by the owner (`for car-fund`), and
  a tenant's deposit is money held for the tenant;
- **claims**: an invoice, a loan to a friend, a deposit paid, a reimbursement
  due. A claim is a parcel in a `claim` place that remembers the transaction that
  made it (its counterparty, its age, its `due` day) and is relieved by the flows
  `for` it. Aging, "coming in", and "overdue" are group-bys over parcels.

A parcel's three dimensions can each move. Its quantity moves with flows, and
with a `split`. Its basis moves with flows into or out of `PLACE.basis`: a
capital improvement raises it, and depreciation lowers it and recognizes the
expense. So the balance sheet, the P&L and the tax gain are one double entry. A
revaluation from a `market` place changes quantity and keeps basis, which is an
unrealized gain or loss. None of these is a withdrawal. What basis arriving
value takes is the target kind's rule (`basis zero` for pre-tax money, `basis
cost` otherwise), overridable per flow, never an accident of the route the money
took. Opening balances create parcels with the basis and acquisition day the
statement gives.

The one rule for exchanges: `basis(new) = basis(given) + gain realized`. Taxable
places realize gains on exchange; `deferred` places (401k, IRA, 529) do not, and
realize only when value leaves them. Every tax-advantaged account in the world is
this rule plus a few laws.

## 3. Kinds type everything

Every declared thing has a kind; kinds form trees rooted in `asset`,
`liability`, `income`, `expense`, `equity` (places), `commodity`, and `entity`.
A kind contributes properties (typed), defaults, and laws.

```text
account assets/retirement : 401k
  employer acme
```

That one line is enough for Axiom to enforce the deferral limit, flag an early
withdrawal and price its penalty, count distributions as ordinary income, and exempt
rollovers — because `401k` is a kind in the `us/401k` system, and those are its
laws. Properties are typed by the kind that declares them. `benificiary` is an
error with a suggestion, not a new metadata key.

Names are typed too. Lowercase identifiers are places and entities, uppercase are
commodities, `#codes` mark and link transactions, dates are dates. A place may be
written by its shortest unique suffix: `checking` means `assets/bank/checking`
when nothing else ends that way. An entity used where a place is expected resolves
to its `via` place and becomes the payee. You name exactly as much as the world
needs to disambiguate.

## 4. Laws are what must hold

```text
/// Elective deferrals are capped per calendar year (IRC §402(g)).
law deferral-limit
  on in
  when from is wages
  require total(in, year) <= limit[year] + catch-up
```

A law has a trigger (`on in`, `on out`, `on gain`, `on spend`, `each year`,
`by <date>`, `always`), optional `when` filters and `let` bindings, and then
consequences:

- `require e` — an error when false.
- `require e else owe x to irs as penalty` — the violation has a *price*, and the
  law resolves it to a loss instead of failing.
- `warn e` — a budget, or a soft limit.
- `count x as wages` — a tally for reports (tax lines are tallies).
- `owe x to irs by date as federal-tax` — an obligation that the forecast and
  the tax report see.

Laws attach to kinds, to individual places, and to systems. They apply to a place's
whole subtree, so a budget on `expenses/food` covers `expenses/food/groceries`.
Constraints narrow: nothing is enforced until something declares it, and once
declared, it is an error unless you relax it explicitly. There is no warning-only
`check` directive. `--relaxed` demotes, and `!` waives a single item.

**Laws are a dataflow graph.** The model sees what each law reads (`tally(x)`)
and writes (`count … as x`) and runs writers before readers, like a spreadsheet.
So a project extends a system by counting into the lines it reads (an itemized
deduction, a credit), and never by copying it, and no file name changes a tax
bill.

**Every limit knows its headroom.** A `require` or `warn` that compares two
amounts records both sides whenever it runs, per subject and window. "22,100 USD
of 401(k) room left this year" and "the food budget is 94% used" are those
records, available before anything breaks.

Time is a flow of actions. A law about the future (a loan's payoff date, a grant's
deadline) applies once the journal reaches that date. Before then it is a pending
obligation the forecast can see. A law about a period can wait for the period to
close (`each year closing 04-15`), so what is recognized `for` that period after
it ends still counts.

**Who** is a set, not a line. A household is an entity its members belong to.
It owns joint places, lives somewhere, files, and is governed as one: its
members' paychecks count into one return. A member keeps what is personal: a
401(k)'s limit and an age-based penalty are that person's. Residences may
overlap and end, so a citizen abroad and a part-year resident are each taxed for
what applies.

## 5. Systems are written in Axiom

`us`, `us/ca`, `us/ca/san-francisco`, `us/401k`, `us/529` are plain `.ax` files
that declare kinds, dated parameters, entities (`irs`), and laws. A jurisdiction
path inherits its ancestors. An entity that `lives us/ca/san-francisco` is governed
by all three. The standard library ships embedded, and a project's `systems/`
folder can add or override. The community maintains tax brackets and contribution
limits as data. The Rust core knows nothing about any of it.

Dated parameters are tables keyed by year (step lookup, latest ≤ year) and names:

```text
param limit
  2025 23_500 USD
  2026 24_500 USD
param ordinary
  2026 single 0 USD 10% | 12_400 USD 12% | 50_400 USD 22% | 105_700 USD 24% | ...
```

## 6. Consequences, not features

- **Balances** fold flows over parcels.
- **Available to spend** is cash in every currency, minus pending outflows,
  restricted and earmarked parcels, and obligations falling due. Claims are
  "coming in", never "to spend". It also shows what each illiquid place would
  yield if drawn today,
  computed by running a hypothetical withdrawal through the laws. That
  withdrawal pays the 401k's 10% penalty and ordinary tax, and it settles in the
  commodity's liquidity time. **Liquidity is derived from law**, which is exactly
  the notebook's "cost of action".
- **Budgets** are `warn` laws on categories, and **limits** are their headroom:
  one table of every cap a person lives under, with what is counted and what is
  left.
- **Taxes**: jurisdictions' laws tally flows and gains, then `each year` laws
  compute what is owed from the tallies with `progressive(schedule, x)`. The tax
  report is those tallies and obligations, each explainable back to flows.
- **Claims**: what others owe you and what you owe them, aged, with due days;
  `check` warns when one is overdue.
- **Forecast**: planned flows, recurring flows inferred from history (cadence
  and amount), claims on their due days, owed obligations, and commodity growth
  models run forward. The same laws are evaluated on the projection, so a
  contribution that *will* exceed its limit in November or a checking account
  that *will* overdraw is reported before it happens. Variable spending is
  bootstrapped from history into bands.
- **Gaps in knowledge**: `?` amounts are inferred from later balance assertions.
  `->` to `?` goes to `unknown`, the garbage can that counts. An assertion that
  does not reconcile names the gap and its likeliest cause. `!` accepts it as an
  explicit externality, never silently, and `via PLACE` says where it went (a
  statement's market value, `via market`).

## 7. Evaluation

```text
bytes ──parse──▶ AST ──model──▶ Book ──engine──▶ Run ──report──▶ views
 (SWAR,       (borrowed,     (typed,      (timeline fold:   (balances, tax,
  per file,    zero-copy)     interned,    parcels, laws,    forecast, why)
  parallel)                   columnar)    inference)
```

- **Parse**: files are read and parsed in parallel with `std::thread::scope`,
  and a large file is cut at item boundaries so its pieces parse on every core.
  The AST is flat: small items whose variable parts (legs, codes, selectors,
  properties, expressions) live in per-file arenas addressed by typed ranges, so
  a line costs tens of bytes, not hundreds. Lines split with SIMD `memchr`, and
  dates, digit runs and names are scanned eight bytes at a time (SWAR).
- **Model**: names are interned into typed arenas (`Id<Place>`, `Id<Law>` …).
  Kinds are linked, laws are type-checked, and transactions are elaborated into
  flows. Resolution runs on read-only tables, so transaction elaboration is
  parallel.
- **Engine**: an inference pass solves `?` amounts, `...` remainders, and
  `=` targets per place, in parallel across places. Then a single ordered fold
  moves parcels, realizes gains, evaluates laws, and records effects. The fold is
  sequential because causality is. Everything around it is not.
- **Numbers**: an amount is an `i64` count of its commodity's quantum (USD 0.01,
  JPY 1, BTC 1e-8), so arithmetic within a commodity is plain integer
  arithmetic. Every scaling operation — price conversion, pro-rata relief,
  percentages — is one `mul_div` through an exact `i128` intermediate with
  explicit banker's rounding. There are no floats in the ledger and no
  allocations for any number.
- **Dates**: `Day(i32)` since 1970-01-01. Conversion to and from the civil
  calendar uses Ben Joffe's multiplication-only algorithms, and the weekday is a
  single multiply. Recurrences and ages are calendar spans (`59y6m`) that carry
  months and days separately.
- **Diagnostics** speak accounting. A failed law shows the offending flow in the
  journal, the law's source in its system, and the value of every subexpression
  under it, power-assert style. It then suggests the bound that would satisfy it.
  The law's doc comment is the explanation.

## 8. Crates

```text
crates/
  core     numbers, dates, interning, globs, typed ids, trees, groups, diagnostics, par
  syntax   lexer + parser → borrowed AST              (depends: core)
  model    AST → Book: resolution, kinds, laws, flows  (core, syntax)
  engine   Book → Run: timeline, parcels, laws, gains  (core, model)
  report   views: balance, register, flow, available,  (core, model, engine)
           budget, tax, lots, forecast, why
  systems  the standard library of systems, embedded   (no deps)
  cli      `axiom`: commands, rendering, sync scripts  (all)
```

Each boundary is a phase with one output type: `File`, `Book`, `Run`, `Report`.
Nothing downstream mutates anything upstream. No crate uses `Arc` or `Mutex`.
Parallel stages borrow immutable inputs through scoped threads and return owned
results.

## 9. Sync without the network

`sync` declarations name a file and a command in any language. `axiom sync` runs
the commands in parallel and parses their stdout as Axiom. It writes each file
only if the output is valid. Prices, appraisals, and statements arrive this way.
The core stays pure and reproducible from the files alone. Git is the history.

## 10. What was deliberately dropped from earlier attempts

Content-addressed object stores, proof DAGs and certificates, closes and
restatements, phase-indexed wrappers, a Datalog engine, a unifier, and a generic
S-expression rule language have all been removed. They guaranteed properties
nobody asked for at a cost of ~80k lines, and never delivered budgeting, taxes,
constraints on accounts, or forecasting. Determinism, exactness, "never guess an
ambiguous lot", "only a settled payment counts", and "a correction is a new fact"
survive as properties of the design rather than machinery around it.
