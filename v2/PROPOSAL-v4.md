# v4 proposal: money, parties and purposes, not accounts

Status: a draft for review. Nothing here is built.

## The problem

v3 rebuilt beancount's chart of accounts. Every meaning had to live in an account:
- **the kind of income:** `income/salary : wages`, `income/rsu : wages`, `income/dividends`;
- **the kind of spending:** `expenses/food/groceries`, `expenses/housing/mortgage-interest : mortgage-interest`;
- **who owes whom:** `assets/owed/by-cleo : receivable`, `liabilities/deposits : payable`;
- **where a book begins, or what nobody explained:** `equity/opening`, `equity/unknown`;
- **the market:** `income/market`;
- **a basis adjustment:** `equity/wash-sale`.

Example 05 declares about 45 accounts for one family. One of its laws even tells the user to send a payment "to this account, which the household owns, to reach the joint return". Laws hang on those accounts, so the phone's business share is a law on `expenses/phone`.

The notes said the opposite: *"We need a better theory of accounts. An institution issues identity."* *"If a fund deposits $100 into checking, it is still money tied to the fund."* *"You specify accounts only when the world is ambiguous enough to need it."* *"Transactions have implicit accounts."*

## The idea

Say what a flow is *by the flow itself*: who it is with, what it is for, and what the money is. A place is named only when it matters where money sits.

Four nouns, one job each:

| noun | what it is | examples | declared |
|---|---|---|---|
| **owner** | who the money belongs to | `me`, `jordan`, `family` | `entity` |
| **holding** | where your money sits, when that matters: a container with an institution and a statement | `checking`, `visa`, `fidelity`, `alex-401k` | `account`, a few per person |
| **party** | everyone you deal with | `acme`, `trader-joes`, `irs`, `riley`, `market` | `entity`, or not at all |
| **purpose** | what a flow is for | `wages`, `groceries`, `rent`, `mortgage-interest`, `dividend` | std ships the tree |

- There are no income, expense or equity accounts. A place is always yours.
- What a flow means comes from its purpose. The purpose is written `: groceries`, or follows from the party's kind (a grocer's purpose is groceries).
- What money *is* comes from the money: its commodity, what it cost, when it was acquired, who it is tied to. That is how v3's parcels already work.
- Laws attach to what they are about:
  - kinds of holdings (a 401(k)'s limit);
  - kinds of parties (a gift to a charity is itemized);
  - purposes (a budget on food, the phone's business share);
  - kinds of money (currency relieves FIFO);
  - owners and systems.

## Before and after, from the real examples

**A paycheck (05).** The withholding legs are payments to the tax agencies, not accounts named after them:

```text
// v3: accounts for every destination
2025-03-15 acme -> 5_750.00 USD
  alex-401k           575.00 USD
  health-premium      212.50 USD
  taxes/federal       692.00 USD
  taxes/state         285.00 USD
  taxes/payroll       388.56 USD
  taxes/sdi            60.95 USD
  joint-checking      ...

// v4: who got the money, and what for
2025-03-15 acme -> 5_750.00 USD                // acme is an employer: wages
  alex-401k           575.00 USD
  blue-shield         212.50 USD : premium      // pre-tax by its purpose
  irs                 692.00 USD                // paid to the IRS: withholding for 2025
  ftb                 285.00 USD
  ssa                 388.56 USD
  edd                  60.95 USD : sdi
  joint-checking      ...
```

A leg that goes from one party to another passes through the transaction's owner. Acme pays Alex 692.00 of wages, and Alex pays the IRS. Nothing needs an account for that.

**Everyday spending (05).** The party is the other end, and its kind gives the purpose:

```text
// v3
2025-03-17 card -> groceries 210.70 USD / costco
2025-03-22 card -> kids 109.55 USD / target
2025-03-20 card -> dining 28.69 USD

// v4
2025-03-17 card -> costco 210.70 USD               // costco : grocer
2025-03-22 card -> target 109.55 USD : kids        // a store sells everything: say what it was
2025-03-20 card -> 28.69 USD : dining              // nobody worth naming
```

**The phone (your example).** The law sits on the purpose or on the party, which is what a person means by "my phone is 60% business":

```text
entity mint-mobile : phone-company                 // its payments are `: phone`

purpose phone : utilities
  /// The phone is 60% for the business, so only that share is deductible.
  ///
  /// To fix: change the share here if the business use changes.
  law business-share
    count amount * 60% as business-expenses
```

**Owing and being owed (09, 04).** A party's balance is what they owe you, or you owe them. There are no receivable or payable accounts:

```text
// v3: a receivable account per debtor, and `for #code` to settle
account assets/owed/by-cleo : receivable
2025-03-01 checking -> 3_150 USD / landlord
  rent          2_100 USD
  by-cleo       1_050 USD due 2025-03-08
2025-03-08 cleo -> checking 1_050 USD for #rent-march

// v4: Cleo owes her third; her payment settles it
2025-03-01 checking -> landlord 3_150 USD : rent
  cleo          1_050 USD due 2025-03-08           // lent: Cleo owes it
2025-03-08 cleo -> checking 1_050 USD               // settles what Cleo owes, oldest first
```

An invoice is the same thing, the other way round:

```text
2026-03-01 acme owes 4_800 USD #inv-12 due 30d : design
2026-04-02 acme -> checking 3_000 USD for #inv-12
```

**Things you own (07).** A house is not an account holding `1 HOME`. It is a thing the owner holds. Its basis moves with improvements and depreciation, addressed by what it is:

```text
// v3
account assets/rental/house : rental-property
  holds HOME
2025-09-15 rental-checking -> house.basis 14_200 USD / summit-roofing
2025-01-31 house.basis -> depreciation 456.79 USD

// v4
2023-06-15 checking 540_000 USD -> 1 HOME / seller     // HOME arrives with me
2025-09-15 checking -> summit-roofing 14_200 USD : improvement of HOME
2025-01-31 HOME.basis -> 456.79 USD : depreciation
```

**Stocks (06).** A share is money of another commodity at a broker. A dividend comes from the fund, and ESPP and wash-sale adjustments are basis, not equity accounts:

```text
// v3
account income/dividends : dividends
account equity/wash-sale : wash-adjustment
2025-03-27 dividends -> fidelity 12.40 USD / vanguard
2025-06-10 wash-sale -> fidelity[2025-06-10].basis 1_978.60 USD

// v4
2025-03-27 VTI -> fidelity 12.40 USD                  // a fund pays: a dividend
2025-06-10 fidelity[2025-06-10].basis + 1_978.60 USD : wash-sale
```

**Where a book begins, and what nobody explained.** There is no `equity`:

```text
opening 2025-01-01                   // a state, not a flow from equity
  checking   12_000 USD
  fidelity   40 VTI basis 8_000 USD since 2022-03-01
  1 HOME basis 540_000 USD since 2023-06-15

2025-06-30 cash = 90 USD !           // a gap, accepted: its own kind of fact
2025-12-31 alex-401k = 105_000 USD via market   // the market is a party
```

## What each part gains

- **Authoring.**
  - 05 needs about 11 accounts instead of 45.
  - A new shop needs no declaration. A name you declare once (`entity costco : grocer`) classifies every payment to it.
  - Adding a holding never makes old lines ambiguous (F31), because parties and holdings are different namespaces.
- **Tax.** `us` counts wages from employers, withholding as payments to tax agencies, and itemized deductions by purpose (`mortgage-interest`, `property-tax`, a gift to a `charity`).
  - Tallies follow the owner of the flow. A household member's deduction therefore reaches the joint return with no "send it to this account" (B7).
- **Budgets:** `purpose food` with `budget 1_300 USD monthly`. A budget is about what money was for, not where it went.
- **Claims:** one mechanism, a party's tab, for invoices, IOUs, deposits, reimbursements and shared bills. `claims` lists the parties who owe you or are owed.
- **Reports.**
  - `balance` is holdings by owner, plus things owned, plus claims.
  - `flow` is purposes (by default) or parties (`--by party`).
  - Neither shows an equity line or a pile of expense accounts.

## How it maps onto what is built

The engine hardly changes. The model compiles v4 into what the engine already folds:
- **Holdings** are asset and debt places. Flat names; the kind says which.
- **Owners** get an implicit holding each ("with me") for things kept without an institution (`HOME`, cash in hand, a car). Money a party-to-party leg passes through the owner also goes through it.
- **Parties** get an implicit outside place each. It carries claim parcels when a flow says `due` (or `X owes`), and otherwise realizes like v3's expense and income places.
- **A purpose** is a kind of flow: a new sort of `kind`, written after `:` like every other kind.
  - Laws declared under a purpose fire on flows of it and its sub-purposes.
  - `total(in, month)` under a purpose sums its flows.
  - Budgets become laws on purposes.
- **Openings, gaps and the market** keep their v3 machinery, with built-in parties in place of the equity and income accounts.

What changes most:
- the model's name resolution (holdings, parties and purposes are separate namespaces, so suffix ambiguity mostly disappears);
- the flow report (by purpose);
- `balance` (no class roots);
- std and `us` (rewritten on purposes and party kinds);
- the examples, rewritten again, which is the proof.

## The choices I am least sure of

1. **The word for a holding.** I kept `account`, because a bank or brokerage account is one. Another option is `place`, or `holding` to break with beancount completely.
2. **Purposes as a tree in std**, which projects extend, like the kinds of places today. The alternative is no purposes at all, only party kinds and codes. That is simpler, but "Target, 109.55, for the kids" then has nowhere to go.
3. **A party's payment settles its open claims, oldest first, by default.** That is natural for "Riley paid me back". It is wrong if Riley also pays you for something else, and then you write `for #code` or `: purpose`.
4. **Party-to-party legs pass through the owner.** This makes a paycheck's withholding "my wages, paid on to the IRS", which is exactly the tax reading. I know of no case where it is wrong, but it is the one new semantic rule.
