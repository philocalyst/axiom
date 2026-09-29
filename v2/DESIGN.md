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
unit       what is counted              USD, VTI, HR, MI, USD/MI
purpose    what an event is for         #groceries, #wages, #improvement of condo
flow       value moving                 06 visa -> trader-joes 84.20 USD
measure    work done, a thing used      12 me worked 6.5 HR for halcyon
promise    flows committed to           flat: 2,900 USD monthly · halcyon owes studio 3,800 USD
change     terms from a day             07-01 flat now 3_050 USD monthly
law        a norm: what must hold       deferrals per year <= limit[year]
system     kinds, purposes, laws, data  us, us/ca, us/401k, written in Axiom
```

## 1. What it rests on

Axiom v3 rebuilt beancount's chart of accounts:
- `income/salary`, `expenses/housing/mortgage-interest`;
- `assets/owed/by-cleo : receivable`;
- `equity/wash-sale` and `house.basis`.

Every meaning had to be parked in an account, and laws hung on accounts. The
notebook asked for the opposite ("an institution issues identity"; "grant
money isn't money, it is a subclass of money"; "you specify accounts only
when the world is ambiguous enough"). Theory has better answers, and each part
of Axiom takes one.

**Events, resources and agents.**
- **REA** (McCarthy 1982; Geerts & McCarthy 2002, 2006). Resources, events and
  agents, with no debit and credit. Every give is paired with a take (duality):
  labour for wages, dollars for a laptop. *Commitments* are promises of events, a
  *claim* is only the time between an event and its counterpart, and *policies*
  are the rules above both. Axiom is REA for one household: owners and parties are
  agents; money, measures and assets are resources; flows and measures are events;
  promises are commitments; laws are policy.
- **ValueFlows.** Custody vs rights: a bank holding your money does not make it
  the bank's, so an account is a *position* with an institution, not a category of
  meaning. Its actions separate moving a resource (`transfer`, `move`) from using
  one or working (`use`, `work`), which carry an effort quantity and move nothing.
  So hours and miles are measures that amounts are computed from, never money in
  fake accounts.
- **Fungible vs identified resources.** Dollars are interchangeable, a house is
  not. Money is parcels that remember their provenance. An asset is made of parts,
  and its basis is derived from them.
- **Capital vs revenue expenditure.** What joins a thing is capitalized into it;
  what keeps it as it was is spent. Two purposes with an object replace every basis
  flow. What only passes through (a gift, tax withheld, a partner's draw) is a
  fourth kind, `transfer`, beside income, spending and capital.
- **Cost allocation.** A mixed-use bill is shared by a declared rule, once, by a
  percent or by a measure (120 of 1,000 square feet), and a share for someone
  outside the book is what they owe.

**Promises.**
- **Composing contracts** (Peyton Jones, Eber & Seward 2000; Bahr, Berthold &
  Elsman 2015) and **CSL** (Andersen, Hvitved, Henglein). A contract is a small term
  of transfers, schedules, conditions and deadlines, and each obligation names whom
  its breach blames. Monitoring is residuation: each event rewrites the promise
  into what is still owed. So contracts and claims are one thing: a claim is what
  remains of a promise, and a late rent, an unpaid invoice and a friend's loan are
  the same to `check`.
- **ACTUS**, the algorithmic contract types standard. A loan's terms generate
  typed events: interest, principal, prepayment, rate reset, indexation. Axiom's
  loans are the ACTUS annuity with its resets and prepayment effects.
- **Momentum accounting** (Ijiri). A household's future is mostly its promises. A
  rent is a momentum, USD per month, and a change of terms is a force on it; the
  forecast integrates momenta, and `check` compares what was promised with what
  happened.

**Norms.**
- **Defeasible deontic logic** (Nute; Governatori & Rotolo's Formal Contract
  Logic; Catala for tax law). A law says what normally holds; exceptions and
  priorities say when it does not. `require A else B` is an obligation with a
  reparation (FCL's ⊗): a repaired violation is compensated, not simply wrong.
  `unless` is the statute's own exception; `!` and `waived` are the book's; the
  more specific rule wins. Constraints are tightened and loosened in place, and
  the book stays declarative.

**Time and quantity.**
- **Behaviors and events** (Elliott & Hudak; Fowler's Temporal Property and
  Effectivity). Flows and measures are events. Terms, properties, prices, params,
  budgets and balances are behaviors, values over time: a declaration is the first
  value, a `now` statement a step, `until` a switch back. Every rule reads a
  behavior on the day it judges.
- **Units of measure** (Kennedy). Units form a free abelian group: `USD/MI` times
  `MI` is `USD`, and adding EUR to USD without a conversion is an error before
  anything runs. Each owner and system counts in its own currency, and converts
  only at a declared rate.
- **Haig–Simons income** (consumption + Δ net worth). `flow` and `balance` are two
  views of one fold, and they reconcile.
- **Mental accounting** (Thaler). People budget by what money is for, and in
  envelopes. Budgets live on purposes; a funded budget is an envelope.

**Records.**
- **Bitemporal data** (Snodgrass). Valid time is the day a fact holds; record time
  only extends. A book needs record time in one place, a return once filed, so
  `filed` makes a later change to its figures an explicit amendment.
- **ISO 20022 and triple entry** (Grigg). A bank statement is the same events seen
  from another book. Sync reconciles two records of one event; it does not import.
- **The network view of double entry** (Ellerman; Arya et al.). Balances are the
  incidence matrix times the flows: unknown amounts are solvable exactly when they
  form no cycle, and the error names the cycle.

## 2. Lines that are easy to skim

The journal records **flows**, which move value, and **statements**, which say one
thing about one subject on a day. Every line reads `DATE SUBJECT …`, and the word
after the subject says what it is: `->` a flow, `=` a value, `owes` a claim, `now`
a change, `worked`/`used` a measure, a verb an event, and a promise's name alone
that promise kept. `axiom fmt` keeps those columns aligned, so a month reads down
the page, and sync writes in the same style.

```text
01 flat
06 visa -> trader-joes 84.20 USD
07-01 flat now 3_050 USD monthly
12 me worked 6.5 HR for halcyon ^inv-12
31 checking = 8_828.87 USD
```

`from -> to` balances every flow by construction, so there is no balancing account
to invent. A leg between two parties passes through the transaction's owner:
`lumen -> irs 498 USD` is Sam's wages, paid on to the IRS, which is how a tax reads
a paystub. A receipt breaks down by line items: `32.10 USD #groceries` carves out
part of a payment, `+ 4.00 USD #tip` adds to it, `- 60 USD "loyalty"` takes from it.

A line says why in three registers, each with one mark:
- **a purpose**, `#groceries`, `#repair of condo`: structured, typed, budgeted,
  counted by laws;
- **a description**, `"food for the routine"`: free words, which are how a thought
  is written down before it deserves a purpose;
- **a code**, `^inv-12`: a name for a fact, so other facts can refer to it.

**Amounts refer to what they come from.** `+ 12% of ^bldg-water` keeps the
building's bill and the share visible; `^inv-12[HR] @ 150 USD/HR` bills the hours
the code names; `1/3 of ^pge-jan` is a roommate's third. Selectors (`[#purpose]`,
`[END]`, `[UNIT]`, `[DATE]`) narrow any reference to its parts, as they narrow an
account to its lots. A flow `against ^code` refunds or reimburses the one the code
names.

**Purposes are inferred**, first match wins: what is written, the promise, the
party, the commodity, the accounts. Disagreement is an error. So most lines say only
who and how much, and an editor shows the rest as hints, each with where it came
from. The journal holds nothing but authored facts: what a book implies is derived
and shown, never generated into it.

A flow has two times: the day value moves, and the period it belongs to. A date
says only what its context does not: in `journal/2026/01.ax`, or below a `2026-01`
heading, `15 job` is enough. Folders are a convention the tools follow, never a law
they enforce.

## 3. Money remembers; things have histories; measures count

Money is held as parcels: quantity, basis, acquisition day, the transaction that
made it, and whom it is held for.
- Parcels that agree merge, so a bank balance of plain dollars is one parcel.
- Money from a restricted party (a grant), or held `for` an envelope or a tenant,
  stays tied and is not available to spend.
- Relief chooses which parcels leave: ties, then policy. Differing parcels with no
  policy are ambiguous, and that is an error listing each choice and its gain.
- Realization happens when parcels change commodity, leave the owners, or leave a
  tax-deferred account.

An asset is one identified thing made of parts: its acquisition, and each
improvement. It never moves like money: flows are *about* it, through their
purpose's object (`#purchase of laptop`, `#improvement of condo`, `#sale of
condo`). A building's units are its parts too, and a bill `of` the building is
divided among them by area. Laws of its kind run per part, `consume` lowers a
part's basis, and a sale relieves every part. `why condo` is the whole history.

Measures are quantities of work and use: `HR`, `MI`, `KWH`, `SQFT`. They are never
held, and they are what shares and rates act on: the studio's share of the flat is
`120 SQFT` of its `1_000 SQFT`, the car's business use is the miles written `for
studio`, an invoice is the hours its code names at the contract's rate.

## 4. Promises

A promise states once who it is with, the amounts and schedule, the purpose, the
shares, what each payment covers, what always comes with it, when it is due and
what a delay costs. A contract promises flows on a schedule; a claim promises one.
Both are one term in a small algebra of transfers, schedules, conditions and
deadlines, and the engine monitors them the same way:
- the journal records each occurrence in a line (`08 phone`), plus what differed;
- whatever is due past its grace and not written is a claim on whoever owes it,
  late, with its blame: a missing rent on the tenant, a missing bill on the owner;
- a deadline's `else` adds its consequence (a late fee) when it passes;
- the forecast unfolds every promise forward through the laws;
- a loan derives each payment's interest and principal, its resets and
  prepayments, and its balance, which a statement checks.

**Terms change.** A promise is rarely kept exactly as made. The rent goes up; the
landlord adds a share of the water one quarter and waives December; a gym runs a
promotion, then extends it; a mortgage's rate resets; a credit note takes a line
off an invoice. Each is one statement on the day it happens, for good or `until` a
day, and a code lets a later statement extend or end it. The same holds for every
property and budget, so the book says what was true on each day, and every rule
reads it as it stood then.

There are no receivable or payable accounts. A claim lives on the party, and the
next payment between them settles it: the ones its codes name, else the one of
exactly its amount, else the oldest. Paying for someone (`for lumen`) is a claim on
them, not your spending.

## 5. The implicit, made visible

Much of what money means is never written on a receipt. Axiom derives it: a loan
payment's interest and principal, a bill's shares, the sales tax inside a price,
what an exchange rate cost, depreciation, a wash sale, an employer's match, a
card's cash back, a processor's fee, a prepaid bill's recognition, an escalation, a
pro-rata refund, a late fee. Most are declared once with `also`, on the promise,
party, kind or purpose they come with: the same five places a purpose is inferred
from. Each derived event names the line and the declaration it came from; `why`
explains it, `check` counts on it, an editor shows it as a hint, and a written line
saying the same thing replaces it.

## 6. Laws are norms

```text
/// Elective deferrals are capped per calendar year (IRC §402(g)).
law deferral-limit
  on in
  when purpose is pre-tax-deferral
  count amount as elective-deferrals
  require tally(elective-deferrals) <= limit[year] + extra
    else owe excess to plan as corrective-distribution by date(year + 1, 4, 15)
    else count excess as wages
```

A law has a trigger, filters (`when`, `unless`), bindings, and consequences:
`require` (with its reparations), `warn`, `owe`, `count` (tax lines are tallies),
`consume`, `carry`. Laws attach to what they are about: kinds, purposes, individual
things, promises, systems. Nothing is enforced until something declares it.
`--relaxed` demotes violations, `!` waives one item visibly, and the more specific
of two laws wins.

- **Laws are a dataflow graph.** Writers of a tally run before its readers, so a
  project extends a system by counting into the lines it reads, and no file name
  changes a tax bill.
- **Every limit knows its headroom.** A comparison of amounts records both sides,
  so "22,100 USD of 401(k) room left" and "food is 94% of budget" are there before
  anything breaks.
- **Tallies belong to owners**, and count in their currency. A household is
  governed as one; a business's tallies reach its owners in their shares.
- **Units are checked before anything runs.** A law that mixes currencies, or
  miles with dollars, without a declared conversion does not compile.

## 7. Systems are written in Axiom

`us`, `us/ca`, `us/401k` and `us/rental` are plain `.ax` files, and so is a
community system under `systems/`. They declare kinds, purposes, entities (`irs`),
dated parameters with units, laws, their currency and rate policy, `also` lines
(payroll taxes, sales tax collected), record formats, and the sources their data
comes from (the IRS's yearly exchange rates, an index for rent escalations). A
jurisdiction inherits its ancestors. The Rust core knows nothing about taxes or
banks: the IRS's meaning, "money to it is tax paid for the flow's year", is a line
of `us`, and OFX is a declaration in std.

## 8. Consequences, not features

- **Balances**: what the owners hold (accounts and assets) and what is owed either
  way.
- **Available to spend**: cash, less what is pending, held for others, or falling
  due. What each other holding would yield comes from running a withdrawal through
  the laws: liquidity is derived from law.
- **Budgets** are norms on purposes, and **limits** are every cap's headroom. A
  budget can be loosened for a while, tightened (a dry January is a budget of
  `empty`), tied to another purpose (`10% of #income`), let carry, so an overspent
  month is a loss the next one takes, or funded, so it has money behind it.
- **Taxes** are laws that tally purposes, measures and gains, and closing laws that
  figure the return. A filed return is kept, and later changes to it are shown as
  amendments.
- **Promises** show what is promised and kept, what is late and whom it blames, and
  what is next.
- **The forecast** runs promises, claims and obligations forward through the same
  laws, with bands for the spending no promise covers.
- **Gaps**: `?` amounts are inferred from values. A failed balance names its
  likeliest cause, including the promise that explains it.

## 9. Evaluation

```text
bytes ─parse→ AST ─model→ Book ─plan→ Plan ─fold→ Run ─views→ data ─render→ text · JSON · editor · GUI
```

- **Parse**: files, and pieces of large files, parse in parallel into a flat AST of
  small items and per-piece tables. Byte scanning (lexing, CSV, patterns) uses SIMD
  through `memchr` and `fearless_simd` where it measurably pays.
- **Model**: names are interned into typed arenas; purposes are inferred, each with
  its provenance; promises compile into term timelines; laws are type- and
  unit-checked and ordered by the tallies they read and write.
- **Plan**: everything decided before the fold, once: solved unknowns, each law's
  static facts, rule tables. It is immutable and shared by reference across
  threads.
- **Fold**: a `Ledger` borrows the plan and folds the timeline: parcels, assets,
  promises, laws, derived events, claims. The fold is sequential because causality
  is; owners whose books never touch fold in parallel. A fork costs the state, not
  the history, so `available` and the forecast try withdrawals and futures side by
  side, in parallel.
- **Views** are data (sections, rows, typed cells, facts), not text. The CLI
  renders them as text or JSON; an editor or a GUI is another renderer over the
  same data.
- **Incremental**: month-end checkpoints of the fold let an edit dated in March
  refold from February's, and stop early when a later checkpoint comes out the
  same.
- **Numbers**: an `i64` count of the unit's quantum; every scaling is one `mul_div`
  through `i128` with banker's rounding. There are no floats.
- **Dates**: `Day(i32)`, with Joffe's multiplication-only calendar conversions.
- **Diagnostics** speak accounting and use everything the book knows. They state
  the fact, point at the causes (the rule, the promise, the flows a tally counted),
  show the facts a law read, and give the fix as an edit. A failed balance is
  explained by what the book expected.

## 10. Crates

```text
crates/
  core     numbers, dates, calendar, timelines, units, interning, ids, trees, groups, diagnostics, par
  syntax   lexer + parser → borrowed AST; the formatter
  model    AST → Book: names, kinds, purposes, promises, laws, flows, measures
  engine   Book → Plan → Run: timeline, parcels, assets, promises, laws, derived events
  report   views as data: balance, register, flow, available, budget, limits,
           claims, contracts, tax, gains, lots, forecast, why
  sync     sources, formats, patterns, reconciliation, writing
  systems  std and the jurisdictions, embedded
  cli      `axiom`: commands, text and JSON rendering
```

Each boundary is one output type: `File`, `Book`, `Plan`, `Run`, views. Each is
immutable once built and `Sync`, so any interface can hold and share them. No crate
uses `Arc`, `Mutex`, `Rc` or `RefCell`. The dependencies are `memchr` and
`fearless_simd`.

## 11. Sync without the network

A book at any scale is mostly written by machines: a bank's export, an invoicing
system, a price feed, a statistics office. Sources, formats and recognition are
declared in the book; a script is the fallback, never the rule.

- **Formats are declarations.** CSV columns, OFX and ISO 20022 records are named
  in Axiom (std ships the common ones), so a new bank is a few lines, not code.
- **Recognition is identity again.** Every party is known by its own name; one a
  bank spells otherwise says so as a parsing expression (`known-as "PAYPAL *"
  payee:rest`), and a code says how its codes appear. Structured fields (a
  remittance reference, an ultimate creditor) win over memos.
- **Reconciliation, not import.** A record matching what the account already saw
  (a flow, a leg, a code's batch, a derived flow) is left alone: a receipt typed by
  hand and the bank's line are one flow, and a transfer seen by two feeds is
  written once.
- **Promises absorb the routine.** A record that keeps a contract is written as its
  occurrence (`01 flat`), so the journal stays as short as a person would write it.
- **Only facts are added.** Nothing written is changed; new lines go into the file
  their day belongs to, in the house style. Prices, rates and parameters arrive the
  same way, and systems declare the sources their own data comes from.
- **Queries go the other way.** Every view prints JSON, so the scripts that feed
  the book can read it too.

## 12. What was deliberately dropped

- **From earlier attempts:** content-addressed stores, proof DAGs, closes, a
  Datalog engine, a unifier and an S-expression rule language, about 80k lines that
  never delivered budgets, taxes or forecasts.
- **From v3:** the chart of accounts:
  - income, expense and equity accounts;
  - `receivable` and `payable`;
  - `PLACE.basis`, and assets as flow ends;
  - plans (promises replace them);
  - `:` and `/ PARTY` on flows;
  - fake commodities for miles and hours, and prices standing in for rates;
  - special cases for escrow and matches (`also` says them);
  - layouts that constrain dates;
  - file shadowing as the way to override a law.

What survives are properties, not machinery:
- determinism and exactness;
- "never guess an ambiguous lot";
- "a correction is a new fact";
- "nothing written is silently reinterpreted".
