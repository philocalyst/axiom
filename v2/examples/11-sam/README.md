# Sam: a sketch of the v4 surface

The v4 parser accepts this source. Model and engine support for every declaration and
law shown is still being integrated, so the report excerpts below remain targets rather
than captured output. `../verify/verify11.py` checks the listed arithmetic independently.
The situation: a salaried designer with a side studio, a rented flat with an
office corner, a rented-out condo with a mortgage, a brokerage with a standing order, a
401(k), and a friend who borrows money.

Every `// ▸` comment is what Axiom would infer or derive, which an editor shows as a hint.
None of it is typed.

| file | what it shows |
|---|---|
| `axiom.ax` | the owners: Sam, and the studio he owns |
| `parties.ax` | everyone Sam deals with, typed by what dealing with them means |
| `accounts.ax` | the six places money sits; there are no income, expense or equity accounts |
| `assets.ax` | the condo and the laptop, each an identified thing with a history |
| `contracts.ax` | everything recurring or scheduled, stated once |
| `budgets.ax` | budgets on what money is for |
| `journal/2026/*.ax` | three months: mostly contract names, plus the one-offs and the changes |
| `prices/2026.ax` | the dated EUR/USD quote used to value the Paris conversion |
| `sync.ax` | where the journal comes from: the bank's and card's exports, prices |
| `std-sketch.ax` | where the hints come from: purposes, party kinds, allocations, `us/rental` |

## What the journal no longer says

- **What most flows are for.**
  - `visa -> trader-joes 84.20 USD` is groceries because Trader Joe's is a grocer.
  - A paycheck is wages because Lumen is an employer.
  - `irs 498.00 USD` in a paystub is federal tax paid for 2026, because the IRS is a tax authority and the paycheck is dated 2026.
  - `VTI -> fidelity 198.12 USD` is a dividend because VTI is a fund.
- **Anything a contract already says.**
  - `2026-01-01 flat` is 2,900 USD of rent from checking, 348 of it the studio's office: 120 of the flat's 1,000 square feet.
  - `2026-01-08 phone` is 45 USD from the card, 27 of it the studio's.
  - `2026-01-01 mortgage` is 2,302.92 USD, split by the loan's own terms into 1,529.66 of interest on the condo, 363.26 of principal and 410.00 of escrow.
  - A paystub restates only its amounts.
- **What changed, only once, on the day it changed.**
  - `27 gym now 120 USD monthly until 04-30 ^promo` halves the gym for a while, and `28 ^promo now until 06-30` extends it.
  - The flat's contract says it takes a share of the building's water when there is a bill (`input water`, `+ 12% of water`); March's `01 flat` states `water = 155.00 USD`, and the 18.60 follows.
  - `03 figma waived` releases one month: nothing is late, and nothing is owed.
- **Which flows are the same flow.** The bank's export and a line typed from a receipt reconcile into one; a record that keeps a contract is written as its occurrence (`sync.ax`).
- **Anything implicit in a price.**
  - The 138.09 USD of sales tax inside the laptop's 1,739.13, which is part of what the laptop cost (`#purchase of laptop`).
  - The 0.39 USD the card's exchange rate cost in Paris.
  - The 2.49 USD of foreign tax taken from VXUS's dividend. That one is written, because it is a withholding like a paystub's, and becomes a credit.
- **Any basis.**
  - The condo's basis is what it cost, plus the water heater (`#improvement of condo`), less the depreciation `us/rental` derives each month.
  - The leak under the sink (`#repair of condo`) is spent instead.
  - The wash sale moves 18.86 USD of loss into the February lot's basis. The `us` wash-sale law derives it from the sale and the standing order 15 days later.

## What `check` says on 2026-03-05

```text
warning[late]: dana's rent for March is 4 days late: 2,350.00 USD owed since 2026-03-01
   ╭─[contracts.ax:58:3]
58 │   2_350 USD monthly on 1 into checking #rent of condo
   │   ────────────────────┬────────────────────
   │                       ╰── the lease expects it on the 1st
   = note: February's came on the 3rd
   = help: record it when it arrives: `05 lease` in journal/2026/03.ax

note[wash-sale]: 18.86 USD of loss on VTI sold 2026-02-05 is disallowed: VTI was bought again on 2026-02-20
   ╭─[journal/2026/02.ax:22:1]
   │  …
   = note: the loss joins the 2026-02-20 lot's basis (500.00 → 518.86), which is held since 2026-01-20

✓ 3 months · 14 contracts kept 41 times
```

## What `check` says when a statement disagrees

If February's rent had not been written, the bank's balance would not match, and the
book knows why:

```text
error[assertion]: checking holds 14,431.34 USD, not 11,531.34 USD
   ╭─[journal/2026/02.ax:60:1]
   │
60 │ 28 checking   = 11_531.34 USD
   │ ──────────────┬─────────────
   │               ╰── 2,900.00 USD less than the book holds
   │
   ├─[contracts.ax:26:3]
   │
26 │   2_900 USD monthly on 1 from checking
   │   ────────────────┬─────────────────
   │                   ╰── due 2026-02-01, and not written
   │
   = since 01-31, when checking held 8,828.85:
       02-01  mortgage       -2,302.92     6,525.95
       02-02  bay-plumbing   -1,480.00     5,045.97   #improvement of condo
       …
       02-28  job            +3,054.70    14,431.34
   = note: the difference is exactly the flat's rent for February, due 02-01, which the journal does not record
   = help: record it: `01 flat` in journal/2026/02.ax

✗ 1 error
```

## What `why condo` shows

```text
condo, a rental home of Sam's, bought 2024-02-20

  Cost                                  402,000.00   the purchase (land 120,000.00)
  Improvements   2026-02-02 water heater  1,480.00   journal/2026/02.ax:9
  Depreciation   2024–2025              -18,372.73   us/rental: 27.5 years, mid-month, from 2024-03-01
                 2026 so far             -2,570.37   (6.73 of it the water heater's)
  Basis                                 382,536.90

  Earns          lease with dana          2,350.00 a month   rental income 7,050.00 in 2026
  Costs          interest of condo (mortgage)   4,583.63 through March 2026
                 insurance of condo             95.00 a month
                 repair of condo  2026-02-14    240.00
  Owed on it     mortgage, rocket       311,345.99
```

## What `flow` shows for February, by purpose

```text
Income                        15,350.00
  wages (lumen, gross)         9,200.00
  rent of condo (dana)         2,350.00
  design (halcyon, #inv…)      3,800.00
Spending
  rent (greystar)              2,900.00   of which the studio's 348.00
  food                           160.22   of 900 budgeted
  phone (mint)                    45.00   of which the studio's 27.00
  interest of condo            1,527.88
  repair of condo                240.00
  …
Capital
  improvement of condo         1,480.00   joined the condo
```

The mortgage, depreciation, sales-tax, exchange-cost, payroll and wash-sale figures
are checked independently by `../verify/verify11.py`. The report excerpts remain targets
until the model and engine can run this project and their output is checked against those
inputs.
