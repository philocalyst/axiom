# 05 - A family

Two adults filing jointly, one child, calendar year 2025. Alex (37, salary plus a
March bonus) and Jordan (37, biweekly pay with a July raise) share a house with a
mortgage, a car with a loan, a joint checking account, an HSA, a dependent-care
FSA, two 401(k)s and a 529 plan for Riley (5). Sample data; every account
statement reconciles to the cent. The project shows a household as one entity: it
files one return for both of them, and each keeps what is personal.

```text
axiom.ax          root: the household `family` (joint, us/ca, one child), Alex (`me`), Jordan, Riley
accounts.ax       kinds, places, employers and merchants, codes, a budget
plans.ax          escrow payments, tuition, the yearly bonus
prices/home.ax    quarterly estimates for the house and the car
journal/2024/12   the opening balances, dated 2024-12-31
journal/2025/*    January-December 2025
outputs/          every command, run with --today 2026-04-16 unless it says otherwise
```

There is no `laws.ax` and no fork of `us`. The old project needed both: a copy of
the `us.ax` with two hand edits, and a file whose name decided a tax bill.
Now `us` counts itemized deductions by the kind of place they arrive at
(`mortgage-interest`, `property-tax`, `charity`), takes pre-tax pay out of income by
the kind of place it goes to (`tax-deferred`, `pretax-benefit`), counts the child
credit from `children 1`, and owes the total tax less the payments. What is left
for the project is two five-line laws that count California SDI and last year's
state balance into `salt`, and the dependent-care FSA's own cap.

## What it models

- **A household.** `entity family : household` files `joint`, lives in `us/ca` and
  has a child; `me` (Alex) and `jordan` are its members. Everything either of them
  earns counts into the household's tallies: `axiom tax 2025` prints one return, and
  `--for jordan` prints only what is personal to Jordan.
- **Each spouse's own 401(k) limit.** Alex has deferred 15,000.00 USD of 23,500.00
  (room 8,500.00); Jordan 5,846.36 USD (room 17,653.64). The limits count into each
  person's tally, so neither borrows the other's room.
- **Payroll, twice.** Alex's stub (semi-monthly) splits gross 5,750.00 into a 10%
  401(k) deferral, an HSA deposit, a health premium and a dependent-care FSA deposit
  (all pre-tax), federal and California withholding, FICA, SDI and net pay (`...`).
  Jordan's biweekly stub has a 6% deferral and a 3% match. A 12,000 bonus is
  withheld at the flat supplemental rates.
- **Mortgage with escrow.** The 3,174.46 payment is one transaction with three legs:
  principal (reduces `liabilities/mortgage`), interest (an expense of kind
  `mortgage-interest`, so it is itemized) and escrow (an asset of kind `escrow`: the
  servicer's, held, not spendable). Escrow pays property tax and insurance, and the
  July escrow analysis raises the monthly escrow by 15.00 USD from September.
- **The house is an opening statement** with its cost and its day:
  `house  1 HOME  basis 540_000 USD  since 2023-06-15`. It is a `home`: a loss on it
  is not deductible, and 500,000 USD of a gain on it is excluded when a joint return
  sells it (held two years or more).
- **A 529 for Riley**, opened with `basis 19_850 USD since 2019-01-01`. Grandma's
  3,000 USD gift arrives from an income place, and a 529 takes what comes in at cost,
  so it is basis. The growth is a quarterly statement `via market`: a revaluation.
  One qualified withdrawal (4,800 USD of tuition, straight from the plan) and one
  that is not (1,500 USD to checking for a brake job).
- **Market losses are not withdrawals.** March's fall in both 401(k)s (2,631.77 and
  1,685.99 USD) is an assertion `via market`. No distribution, no penalty.
- **Daycare** paid partly by the FSA (up to what it holds) and partly from checking;
  after-school care from September.
- **HSA**: medical spending straight from the account, and a 620 USD reimbursement of
  May's dentist bill to checking, marked `!` with the reason.
- **Car loan**, **credit card** paid in full on the 25th, with two purchase refunds
  and a cash-back credit.

## What the commands should show (hand-verified)

`python3 ../verify/verify05.py` reads the journal text, without Axiom, and works the
return, the 529's pro-rata earnings and the limits. The figures agree with
`axiom tax 2025` to the cent.

| line | by hand | Axiom |
|---|---|---|
| wages (Alex 150,000.00, Jordan 97,440.07) | 247,440.07 | 247,440.07 |
| pre-tax: 401(k)s, HSA payroll, health premium, FSA | 36,946.36 | 36,946.36 |
| interest | 2,479.02 | 2,479.02 |
| distributions: 529 earnings 392.94 + HSA reimbursement 620.00 | 1,012.94 | 1,012.94 |
| total income = AGI | 213,985.67 | 213,985.67 |
| itemized: mortgage interest 24,077.32 + state and local taxes 20,907.75 + charity 3,900.00 | 48,885.07 | 48,885.07 |
| deductions (itemized beats the 31,500.00 standard) | 48,885.07 | 48,885.07 |
| taxable income | 165,100.60 | 165,100.60 |
| income tax (joint brackets, each rounded to the cent) | 26,150.13 | 26,150.13 |
| child credit | 2,200.00 | 2,200.00 |
| total tax | 23,950.13 | 23,950.13 |
| payments: federal withholding | 27,594.00 | 27,594.00 |
| **federal, owed (negative = refund)** | -3,643.87 | -3,643.87 |
| California taxable income (AGI less 11,412.00) | 202,573.67 | 202,573.67 |
| California tax, withheld 11,239.60, owed | 477.03 | 477.03 |
| 529 penalty: 10% of 392.94 of earnings, owed with the return | 39.29 | 39.29 |

State and local taxes are 11,239.60 withheld + 2,776.15 SDI + 412.00 paid for last
year + 6,480.00 property tax from escrow, under the 40,000 USD cap.

The federal return closes on 2026-04-15 with a **refund of 3,643.87 USD**: the tax is
23,950.13 USD and 27,594.00 USD was withheld. Run with `--today 2026-01-05`
(`outputs/tax-2025-before-closing.txt`) the return has not closed: `axiom tax` shows
what was counted, and `axiom forecast` (`outputs/forecast.txt`, run that day) lists it
among the obligations coming due, the refund as a negative amount, next to California's
477.03 USD.

`axiom check`: no errors, and only what the project teaches: two priced lines, the
529 withdrawal (20.67 and 18.62 USD, one per parcel relieved, 10% of its 206.72 and
186.22 USD of earnings, owed by 2026-04-15) and one waived line, the HSA
reimbursement (`!` waives its 124.00 USD penalty too). The summary reads 864 flows
and net worth 684,163.25 USD.

`axiom balance --value`: assets 1,109,933.88 USD (house 612,400.00, car 25,300.00,
checking 36,838.99, savings 77,329.02, 401(k)s 313,762.82, 529 28,902.11, HSA
13,544.20, escrow 1,440.00, FSA 416.74); liabilities 425,770.63 USD (mortgage
406,692.02, car loan 16,976.91, card 2,101.70); net worth **684,163.25 USD**. Every
month-end assertion reconciles; the generator that wrote the journal kept a second
ledger (amortization, payroll, escrow, the market statements) to compute them.

`axiom limits 2025`: the two 401(k) limits above; the HSA at 7,000.00 of 8,550.00
(family; payroll 6,000.00 and the employer's 1,000.00), room 1,550.00; the FSA at
5,000.00 of 5,000.00, room 0; the 529 at 6,000.00 of the 19,000.00 gift exclusion
(twelve 250.00 deposits and the gift; the market's growth is not a contribution),
room 13,000.00.

`axiom available`: 114,168.01 USD to spend (checking and savings). The escrow
(1,440.00 USD, 365 days) is held, the house and the car are slow, and the 401(k)s,
529 and HSA list their penalties. The tax on money drawn is not in those costs: see
below.

## Findings this project now demonstrates as fixed

F01 (basis is the kind's rule: the gift is basis, the 401(k) and HSA money has none),
F03 (market losses are revaluations: the 263.18 + 168.60 USD of penalties are gone),
F05 (one household, one return, per-person limits), F08 (no file order, no fork:
the old 178-line copy of `us.ax` and the 46-line `laws.ax` are gone), F10 (escrow is held; forecast
liquid net worth no longer counts the house), F11 (the bonus plan no longer deletes
the paycheck: both are projected), F17 (the openings are statements: no 2024 laws, no
gift-exclusion warning at 24,600 USD), F19 (a slot for each thing a project adds; a
refund is booked, negative), F24 (the car's 8,600 USD loss is not deductible; the house
sale is excluded), F27 (the gift-exclusion law counts contributions into a tally, not
every inflow), F15 in part (`!` waives the HSA's priced penalty).

## Still open

- **The reimbursement still counts as income.** `!` waives a priced violation, but
  the 620.00 USD is also counted as a distribution, and a `count` is not a violation:
  AGI is 620.00 USD too high (about 190 USD of tax). A reimbursement of a bill already
  paid needs the `for #code` link to reach the law (F15).
- **Tax on money drawn is missing from `available`'s costs.** The fork stops at the
  end of the year and the return closes on April 15 of the next: the 401(k)s show the
  10% penalty and not the income tax. The forecast likewise shows the return only when
  its window reaches April 15.
- **The house is worth 612,400.00 USD in `available`, not net of its mortgage.**
- **`gains` lists qualified spending** (the FSA's care, the HSA's medical bills, the
  529's tuition) as withdrawals with a gain; they are not counted as income.
