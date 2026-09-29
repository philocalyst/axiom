# v4 proposal: agents, resources, events and promises, not a chart of accounts

Status: a draft for review, revised after the first round of feedback. The
worked example is `examples/v4-sketch/`, with a README; read it alongside
this.

## Why

v3 rebuilt beancount's chart of accounts, so every meaning had to be parked
in an account:
- the kind of income: `income/rsu : wages`;
- the kind of spending: `expenses/housing/mortgage-interest`;
- who owes whom: `assets/owed/by-cleo : receivable`;
- a basis adjustment: `equity/wash-sale`, `house.basis`;
- the market: `income/market`.

Laws hung on those accounts. Example 05 declares about 45 accounts for one
family, and one of its laws tells the user to send a payment "to this
account, which the household owns, to reach the joint return".

The notes asked for the opposite:
- *"We need a better theory of accounts. An institution issues identity."*
- *"If a fund deposits $100 into checking, it is still money tied to the
  fund."*
- *"You specify accounts only when the world is ambiguous enough to need it."*
- *"Transactions have implicit accounts."*
- *"Splitting and recurring should share one primitive."*
- *"Price and cost differ: those differences should DEFINITELY be recorded."*

## The theory it rests on

**REA: resources, events, agents** (McCarthy 1982; Geerts & McCarthy's extensions).
- **Theory:** Accounting without debit and credit. Economic *events* move *resources* between *agents*. Every give is paired with a take in a *duality*: I give labour, I take wages; I give dollars, I take a laptop. *Commitments* and *contracts* are promises of future events. A *claim* is nothing but the timing gap between an event and its dual.
- **In Axiom:** Agents are owners and parties. Resources are money and things. Events are flows. Contracts are first class. A claim is what a party owes, or is owed, until the dual event happens.

**Custody vs rights** (ValueFlows, the REA vocabulary for networks: `transfer-custody` vs `transfer-all-rights`).
- **Theory:** An institution holding your money is not the money being theirs. A deposit is a debt the bank owes you; a brokerage keeps securities you own.
- **In Axiom:** An account is a *position* with an institution (`deposit at chase`, `card at chase`, `brokerage`). It is not a category of meaning. Money in no account is with its owner.

**Fungible vs identified resources** (ValueFlows' resource specification vs tracked resource).
- **Theory:** A dollar is interchangeable. A house is not.
- **In Axiom:** A commodity is a specification, with fungible parcels that carry provenance. An `asset` is an identified thing with its own history: cost, improvements, depreciation, disposal.

**Capital vs revenue expenditure.**
- **Theory:** Money spent that joins a thing (an improvement) is capitalized into it. Money spent keeping it as it was (a repair) is an expense.
- **In Axiom:** Purposes can take an object: `: improvement of condo` joins the condo, and `: repair of condo` is spent. `.basis` goes away.

**Cost allocation** (joint and mixed-use costs).
- **Theory:** One bill can serve two owners or two ends: the phone that is 60% business, the flat's office corner.
- **In Axiom:** `business 60% for studio` on a contract, party or purpose derives the studio's share of each flow. The bill itself is still the phone.

**Transformation duality** (REA: consume and produce).
- **Theory:** Depreciation is a resource being used up, not a flow to a party.
- **In Axiom:** Laws may `depreciate` an asset, which consumes its basis and counts the deduction. No flow is written.

**Haig–Simons income** (consumption + Δ net worth).
- **Theory:** Income and wealth must reconcile.
- **In Axiom:** `flow` (by purpose) and `balance` (holdings, assets, claims) are two views of one fold: what arrived, less what was consumed, is what net worth changed by. This holds even for capital purposes, which move value between money and assets.

**Momentum accounting** (Ijiri): income as a rate, and what changes it.
- **Theory:** A household's future is mostly its promises: pay, rent, loans, subscriptions.
- **In Axiom:** The forecast runs contracts, not guesses from history. A contract's amount and cadence *are* the household's momentum. `check` compares promises with what happened: late rent, a missing bill, a cancelled contract.

**Mental accounting** (Thaler).
- **Theory:** People budget by what money is *for*, in envelopes.
- **In Axiom:** Budgets live on purposes, and envelopes are money held `for` someone or something.

## The model

**Agents.**
- **Owners** are you, a spouse, a household, a business you own (`studio`).
- **Parties** are everyone else: employers, shops, people, tax authorities, the market, and funds, which pay dividends.
- Both are `entity`, typed by kind. A party's kind says what dealing with it means:
  - paying a grocer is groceries;
  - money from an employer is wages;
  - money to a tax authority is tax paid for the flow's year.

**Resources.**
- **Money** is parcels of a commodity. Each parcel carries its provenance: owner, custodian, cost, acquisition day, what it is held for, and whose it was.
- **Assets** are identified things (`asset condo : rental-home`). An asset carries its history, and `basis` is derived from it, never written.

**Accounts.**
- An account is a position with an institution: `account visa : card at chase`.
- There are few of them, flat, and each kind says what it is: a deposit, a card, a brokerage, a 401(k), an escrow.
- There are no class roots (`assets/…`, `expenses/…`) and no income, expense or equity accounts.

**Events (flows).**
- The grammar is v3's arrow, and each end is an account, an owner, a party, an asset, or nothing.
- **A purpose** is `: purpose [of THING]`, a kind of flow in a tree std ships. It is inferred, first match wins:
  1. what is written on the flow or leg;
  2. its contract;
  3. its party's kind;
  4. its money's kind;
  5. its accounts' kinds.

  Two sources that disagree are an error that names both. A flow no source classifies is `unclassified`, and `check --strict` asks what it was for.
- **A leg from one party to another passes through the owner.** In a paystub, `irs 498.00 USD` is Sam's wages paid on to the IRS, which is exactly how the tax reads it.

**Contracts (commitments).**
- A contract states once who a promise is with, the amount, the cadence, the purpose, the allocations, what it covers, and when it ends. This covers employment, leases, loans, subscriptions, insurance and standing orders.
- `DATE CONTRACT [AMOUNT]` in the journal records one fulfillment, followed by whatever differed that time.
- A loan's contract derives each payment's interest and principal from its terms.
- The forecast is contracts run forward.
- `check` reports a fulfillment that is late or missing, and `DATE CONTRACT ends` closes a contract.

**Claims.**
- A claim is a party's balance: `checking -> jo 600 USD due 2026-04-01` lends, and `halcyon owes studio 3_800 USD due 30d` invoices.
- A later flow from that party settles its oldest open claim first, unless `for #code` says which.
- A late contract fulfillment is also a claim for as long as it is late.
- There are no receivable or payable accounts.

**Derived events (the implicit).** What a contract, a party, an asset or a law implies without anyone writing it:
- interest inside a loan payment;
- the business share of a bill;
- the sales tax inside a price;
- the exchange cost of a card abroad (what was paid, less what the money was worth that day);
- depreciation;
- a wash sale's basis adjustment;
- an employer match a contract promises.

Each is computed in the run, never written into the journal. `why` explains it, and the editor shows it as a hint.

**Laws** attach to what they are about:
- kinds of accounts (a 401(k)'s limit);
- kinds of parties;
- purposes (budgets, a business share, what counts as wages);
- kinds of assets (depreciation);
- kinds of money;
- owners and systems.

Tallies belong to the taxpayer who owns the flow, so a household's member's deduction reaches the joint return with no routing.

## The surface, v3 to v4

**Removed:**
- class roots and income, expense and equity accounts;
- `.basis` places, and the basis tails on flows;
- `receivable` and `payable`;
- `/ payee` in most places (the party *is* the other end; `/` remains for paying one party through another, such as PayPal);
- `every` plans and named plans, which become contracts.

**Added:**
- `purpose NAME : PARENT` with `of TYPE`;
- `asset NAME : KIND`;
- `contract NAME with PARTY` (terms, allocations, legs), with `DATE CONTRACT [AMOUNT]` and `DATE CONTRACT ends` in the journal;
- `: PURPOSE [of THING]` on flows and legs;
- `PARTY owes OWNER AMOUNT`;
- `business N% for OWNER`;
- `at INSTITUTION` on accounts;
- `budget PURPOSE AMOUNT CADENCE`;
- the law effect `depreciate`.

**Kept:**
- dates, amounts, `@`, `due`, `for`, `#codes`, `!`;
- lot selectors `[…]`;
- `opening`, `split`, assertions with `via`;
- kinds, properties, params, laws and systems.

## The editor

Inference is the point, so it has to be visible. An LSP, which can come later but is designed for now, shows:
- each flow's inferred purpose, and where it came from;
- each contract line's amounts;
- each derived event, attached to the line that caused it.

`axiom why FILE:LINE` prints the same, so every hint in the example is also a command's output.

## What it takes

The engine's core carries over: parcels, relief, realization, tallies, headroom, recognition and the fold.

What changes:
- **Syntax:** `purpose`, `asset`, `contract`, occurrences and `ends`, `owes`, objects, allocations.
- **Model:**
  - three namespaces (accounts, parties, purposes) in place of one path tree;
  - purpose inference with its provenance;
  - contracts compiled into schedules and templates;
  - owners' implicit holdings;
  - parties' claim places.
- **Engine:**
  - laws on purposes, where totals and budgets are over flows of a purpose;
  - derived events from contracts (loan splits, allocations, matches) and from laws (`depreciate`, wash sales);
  - identified assets;
  - late and missing fulfillments.
- **Report:**
  - `flow` by purpose or party;
  - `balance` as holdings, assets and claims;
  - `why ASSET` as its history;
  - `forecast` from contracts.
- **std and `us`:** rewritten on purposes and party kinds.
- **Examples:** all of them, again. `v4-sketch` becomes the first runnable one.

## Open questions

1. **Should derived allocations be flows of their own** (the studio's 27.00 of the phone is a flow `: phone` owned by `studio`), or annotations on the bill? Flows make `flow` and budgets exact. Annotations keep the fold smaller.
2. **Should contract fulfillments be written, or implied** when the statement assertion covers them? Written is honest and the notes' default ("nothing is generated into the journal"). Implied is less typing, and a missing line would then surface as a statement gap.
3. **Should a wash sale be derived by a law** (recommended: it is a tax rule, not a fact), or written?
4. **Is `me` as a holder of money** (cash in hand, the condo) the right default, or should cash need an account?
