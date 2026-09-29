# Axiom: design

Axiom is a typed, plain-text ledger. It answers one question first, *how much
money is actually available to spend*, and then every harder one the same way:
- what you own, and what you owe;
- what the rules you live under say about each move you make;
- what you will owe the government for it;
- what you have promised, and been promised;
- where it is all heading.

Everything below follows from a handful of concepts that compose. There is no
second engine for taxes, no budgeting subsystem, no liquidity model and no
forecast language. Those are consequences.

```text
owner      whose money it is            me, jordan, family, studio
party      who else                     lumen, trader-joes, irs, jo, VTI, market
account    where money sits             checking (a deposit at chase), visa, fidelity
asset      a thing, with a history      condo, laptop
commodity  what it is counted in        USD, EUR, VTI
purpose    what a flow is for           #groceries, #wages, #improvement of condo
flow       an event: value moving       06 visa -> trader-joes 84.20 USD
contract   a promise of flows           phone with mint: 45 USD monthly, 60% business
claim      what is owed, until paid     jo owes me 400 USD, due 04-01
law        what must hold, and count    deferrals per year <= limit[year]
system     kinds, purposes and laws     us, us/ca, us/401k, written in Axiom
```

## 1. What it rests on

Axiom v3 rebuilt beancount's chart of accounts:
- `income/salary`, `expenses/housing/mortgage-interest`;
- `assets/owed/by-cleo : receivable`;
- `equity/wash-sale` and `house.basis`.

Every meaning had to be parked in an account, and laws hung on accounts. The
notebook asked for the opposite ("an institution issues identity"; "grant
money isn't money, it is a subclass of money"; "you specify accounts only
when the world is ambiguous enough"), and accounting theory has a better
answer.

- **REA** (McCarthy 1982; Geerts & McCarthy):
  - *Resources*, *events* and *agents*, with no debit and credit.
  - Every give is paired with a take (*duality*): labour for wages, dollars
    for a laptop.
  - *Commitments* and *contracts* are promises of future events.
  - A *claim* is only the time between an event and its counterpart.
  - Axiom is REA for one household: owners and parties are agents, money and
    assets are resources, flows are events, and contracts and claims are
    first class.
- **Custody vs rights** (ValueFlows' `transfer-custody` vs
  `transfer-all-rights`). A bank holding your money does not make it the
  bank's: a deposit is a debt the bank owes you. So an account is a
  *position* with an institution, not a category of meaning, and money in no
  account is simply with its owner.
- **Fungible vs identified resources.** Dollars are interchangeable and a
  house is not. Money is parcels that remember their provenance. An asset
  has a history of parts, and its basis is derived from that history.
- **Capital vs revenue expenditure.** What joins a thing (an improvement) is
  capitalized into it. What keeps it as it was (a repair) is spent. Two
  purposes with an object replace every basis flow.
- **Cost allocation.** A mixed-use bill (the phone that is 60% business, the
  office corner of a flat) is shared by a declared rule, once.
- **Haig–Simons income** (consumption + Δ net worth). `flow` and `balance` are
  two views of one fold, and they reconcile.
- **Momentum accounting** (Ijiri). A household's future is mostly its
  promises, so the forecast runs contracts, and `check` compares what was
  promised with what happened.
- **Mental accounting** (Thaler). People budget by what money is *for*, and
  in envelopes. Budgets live on purposes, and envelopes are money held `for`
  someone.

## 2. Flows are the only facts

A journal line records one event: value moved between two ends.

```text
06 visa -> trader-joes 84.20 USD
```

The ends are accounts, owners, parties and assets. `from -> to` balances
every flow by construction, so there is no balancing account to invent. A
transaction is flows written together, with one side split:

```text
15 job
  retirement   276.00 USD
  irs          498.00 USD
  checking     ...
```

A leg between two parties passes through the transaction's owner. `lumen ->
irs 498 USD` is Sam's wages, paid on to the IRS, which is exactly how a tax
reads a paystub.

A flow says why it happened in three registers, and each has one mark:
- **a purpose**, `#groceries`, `#repair of condo`: structured, declared,
  typed, budgeted, counted by laws;
- **a description**, `"food for the routine"`: free words for people, with
  no meaning to the book, which are how a thought is written down before it
  deserves a purpose;
- **a code**, `^inv-12`: the same mark on things that belong together (an
  invoice and its payment, a lot and its sale).

`:` never appears on a flow. It means only "is a kind of", in declarations.

**Purposes are inferred**, first match wins:
1. what is written;
2. the flow's contract;
3. its party's kind (a grocer's purpose is groceries);
4. its commodity's kind (a fund pays dividends);
5. its accounts' kinds (a 401(k) takes pre-tax deferrals from wages).

Disagreement is an error. So most lines say only who and how much, and an
editor shows the rest as hints, each with where it came from.

A flow has two times: the day value moves, and the period it belongs to (`for
2025`, or a contract that `covers the year`). Balances read the day. Tallies,
budgets and taxes read the period. A flow in parentheses is pending (a check
written but not cashed). A date says only what its file's place does not: in
`journal/2026/01.ax`, `15 job` is enough.

The journal holds nothing but authored facts. What a book implies is derived
and shown, never generated into it (§5).

## 3. Money remembers; things have histories

Money is held as parcels: quantity, basis, acquisition day, the transaction
that made it, and whom it is held for.
- Parcels that agree merge, so a bank balance of plain dollars is one parcel.
- Money from a restricted party (a grant), or held `for` an envelope or a
  tenant, stays tied and is not available to spend.
- Relief chooses which parcels leave: ties, then policy. Differing parcels
  with no policy are ambiguous, and that is an error that lists each choice
  and the gain it would realize.
- Realization happens when parcels change commodity, leave the owners, or
  leave a tax-deferred account.

An asset is one identified thing made of parts: its acquisition, and each
improvement. Laws of its kind run per part, so a new water heater
depreciates on its own schedule. `consume` lowers a part's basis. Selling the
asset relieves every part. `why condo` is the asset's whole history: what it
cost, what improved it, what it has consumed, and what it earned and cost
along the way.

## 4. Promises: contracts and claims

A contract states once who a promise is with, the amount and schedule, the
purpose, the allocations, what each payment covers, and when it ends. It
covers employment, leases, loans, subscriptions, insurance and standing
orders. From one declaration:
- the journal records each occurrence in a line (`08 phone`), plus what
  differed that time;
- `check` reports an occurrence that is due and not written: a late rent is a
  claim on the tenant, and a missing bill is one the owner owes;
- the forecast is the contracts run forward through the laws;
- a loan's contract derives each payment's interest and principal, and its
  balance, which a statement assertion checks.

A claim is the time between an event and its counterpart:
- a loan to a friend (`checking -> jo 600 USD due 04-01`);
- an invoice (`halcyon owes studio 3_800 USD ^inv-12`);
- a deposit held;
- a late occurrence.

A claim lives on the party. Their next payment settles it, the one its code
names or the oldest. There are no receivable or payable accounts.

## 5. The implicit, made visible

Much of what money means is never written on a receipt, and Axiom derives it:

| derived | from |
|---|---|
| interest and principal in a loan payment | the contract's terms |
| the business share of a bill | `business 60% for studio` |
| sales tax inside a price | the store's `sales-tax` |
| what an exchange rate cost | the price that day |
| depreciation | `consume` in a law |
| a wash sale | `carry` in a law |
| an employer match | the contract |
| recognition of a prepaid bill | `covers` |

Each derived event names the line and the declaration it came from. `why`
explains it, `check` counts on it, and an editor shows it as a hint. A share
is a flow of its own, borne by its owner, so budgets and the tax are exact
while `register` shows the bill once, as paid.

## 6. Laws are what must hold

```text
/// Elective deferrals are capped per calendar year (IRC §402(g)).
law deferral-limit
  on in
  when purpose is pre-tax-deferral
  count amount as elective-deferrals
  require tally(elective-deferrals) <= limit[year] + extra
```

A law has a trigger, `when` filters, `let` bindings and consequences:
- `require`, which is an error unless `else` prices it;
- `warn`;
- `owe`;
- `count`, which feeds a tally, and tax lines are tallies;
- `consume`;
- `carry`.

Laws attach to what they are about:
- kinds of accounts, parties, assets and commodities;
- purposes (a budget on food, the tax meaning of wages);
- individual things;
- systems.

Constraints narrow: nothing is enforced until something declares it. There
is no warning-only directive. `--relaxed` demotes violations, and `!` waives
one item, visibly.

- **Laws are a dataflow graph.** Writers of a tally run before its readers,
  so a project extends a system by counting into the lines it reads, and no
  file name changes a tax bill.
- **Every limit knows its headroom.** A comparison of amounts records both
  sides, so "22,100 USD of 401(k) room left" and "food is 94% of budget" are
  there before anything breaks.
- **Tallies belong to owners.** A household is governed as one, so a
  member's deduction reaches the joint return without routing anything.

## 7. Systems are written in Axiom

`us`, `us/ca`, `us/401k` and `us/rental` are plain `.ax` files. They declare:
- kinds (`employer`, `tax-authority`, `401k`, `rental-home`);
- purposes (`wages`, `tax-paid`, `mortgage-interest`);
- entities (`irs`);
- dated parameters;
- laws.

A jurisdiction inherits its ancestors. A project's `systems/` may add or
override. The Rust core knows nothing about taxes: the IRS's meaning, "money
to it is tax paid for the flow's year", is a line of `us`.

## 8. Consequences, not features

- **Balances**: what the owners hold (accounts and assets) and what is owed
  either way.
- **Available to spend**:
  - It is cash, less what is pending, held for others, or falling due.
  - What each other holding would yield comes from running a withdrawal
    through the laws. Liquidity is derived from law.
- **Budgets** are warnings on purposes, and **limits** are every cap's
  headroom.
- **Taxes** are laws that tally purposes and gains, and closing laws that
  figure the return from them.
- **Contracts** show what is promised and kept, what is late, and what is
  next.
- **The forecast** runs contracts, claims and obligations forward through
  the same laws, with bands for the spending no contract covers.
- **Gaps**: `?` amounts are inferred from assertions. A failed assertion
  names its likeliest cause. `!` accepts a gap as unexplained, and `via
  market` says it was the market.

## 9. Evaluation

```text
bytes ──parse──▶ AST ──model──▶ Book ──engine──▶ Run ──report──▶ views
 (per file and    (flat,          (typed, purposes    (timeline fold:   (balance, flow,
  per piece,       borrowed)       inferred, contracts  parcels, assets,  contracts, tax,
  in parallel)                     compiled)            laws, derived)    forecast, why)
```

- **Parse**: files, and pieces of large files, parse in parallel into a flat
  AST of small items and per-piece tables.
- **Model**:
  - Names are interned into typed arenas.
  - Purposes are inferred, and each carries its provenance.
  - Contracts compile into schedules and templates, and occurrences into
    flows.
  - Laws are type-checked and ordered by the tallies they read and write.
- **Engine**:
  - `?` amounts are solved per account, in parallel.
  - One ordered fold moves parcels, keeps assets' parts, derives what
    contracts and laws imply, realizes gains, evaluates laws, and records
    claims and late promises.
  - The fold is sequential because causality is.
- **Numbers**: an `i64` count of the commodity's quantum. Every scaling is one
  `mul_div` through `i128` with banker's rounding. There are no floats.
- **Dates**: `Day(i32)`, with Joffe's multiplication-only calendar
  conversions.
- **Diagnostics** speak accounting. They state the fact, point into the
  reader's file, show the facts a law read, and give the fix as an edit.

## 10. Crates

```text
crates/
  core     numbers, dates, interning, typed ids, trees, groups, diagnostics, par
  syntax   lexer + parser → borrowed AST
  model    AST → Book: names, kinds, purposes, contracts, laws, flows
  engine   Book → Run: timeline, parcels, assets, laws, derived events, claims
  report   balance, register, flow, available, budget, limits, claims,
           contracts, tax, gains, lots, forecast, why
  systems  std and the jurisdictions, embedded
  cli      `axiom`: commands, rendering, sync
```

Each boundary is one output type: `File`, `Book`, `Run`, `Report`. No crate
uses `Arc`, `Mutex`, `Rc` or `RefCell`. The only dependency is `memchr`.

## 11. Sync without the network

`sync FILE` names a command whose stdout is Axiom. `axiom sync` runs the
commands in parallel and writes each file only if it parses. Prices,
appraisals and statements arrive this way. The core stays pure and
reproducible.

## 12. What was deliberately dropped

- **From earlier attempts:** content-addressed stores, proof DAGs, closes,
  a Datalog engine, a unifier and an S-expression rule language, about 80k
  lines that never delivered budgets, taxes or forecasts.
- **From v3:** the chart of accounts:
  - income, expense and equity accounts;
  - `receivable` and `payable`;
  - `PLACE.basis`;
  - plans (contracts replace them);
  - `:` on flows.

What survives are properties, not machinery:
- determinism and exactness;
- "never guess an ambiguous lot";
- "a correction is a new fact";
- "nothing written is silently reinterpreted".
