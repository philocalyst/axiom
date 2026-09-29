# 06 - An investor

Sam, 34, a software engineer in Seattle (no state income tax), with a taxable
brokerage, employer stock, an employee stock purchase plan and a little crypto.
September 2024 to December 2025; the 2025 tax year is the one to read. Sample data.
Every bank, cash and holding assertion reconciles, and an independent Python lot
engine (FIFO, LIFO, HIFO and named lots, transfers, an exchange, a split) reproduces
Axiom's realized gains line for line. The project shows what an investor's return
needs from the system: netting of gains and losses across terms, the tax on
investment income, basis that is adjusted after the fact, and lots older than the ledger.

```text
axiom.ax          root: me (single, born 1991, lives us), uses std, us and us/401k
accounts.ax       commodities, brokers, kinds for staking and wash sales, the ESPP envelope
tax.ax            two laws: staking income, and the add-back of a wash sale's disallowed loss
prices/stocks.ax  closing prices on every trading day used and every month end
prices/crypto.ax  BTC and ETH, the same way
journal/2024/09   the opening block (an `opening 2024-09-01` statement), then September 2024
journal/2025/*    January-December 2025
outputs/          every command, run with --today 2026-04-16 unless it says otherwise
```

`tax.ax` is 26 lines. Everything else a return needs is `us`: the lines it counts
(wages, interest, dividends, short- and long-term gains), the netting of the year's
gains and losses, the standard deduction, the brackets with the lower rates for
long-term gains, the 3.8% net investment income tax, and the tax owed less what was
withheld. A project adds only what `us` cannot know: staking rewards are wages-like
income at their value on arrival, and a wash sale's disallowed loss has to be added back.

## What it models

- **Lots older than the ledger.** The `opening 2024-09-01` block carries what Sam
  held before the first flow, each lot with its cost and its day: 30 VTI since
  2020-05-11 (basis 5,100.00 USD), 12 MSFT since 2021-06-15 (3,000.00) and 300 BND since
  2021-08-11 (25,500.00). No law sees an opening, and no year-end law is run for the
  years before the first flow. The old VTI is long-term when it is sold, with no
  flow dated 2020 to fake it.
- **A monthly habit**: 1,500 USD of VTI on the 5th, each purchase its own lot
  (`checking -> fidelity 5.5851 VTI @ 268.57 USD`), and reinvested dividends that add a small lot
  every quarter.
- **Specific lots and a policy.** `fidelity[2024-09-05, 2024-10-05] 10 VTI` picks two
  lots by date (both past a year, so long-term); the plain sale of 8 VTI in December
  lets the account's `select hifo` choose (two short-term lots, the smallest gain).
- **Brokers move in kind.** `fidelity all VXUS -> schwab` and `fidelity all BND -> schwab`
  move every parcel of the fund, with its cost and its day, in one line each.
- **A stock split is a line in the journal.** `2025-05-22 FAST split 2 for 1`: every parcel
  doubles in quantity and keeps its cost and its day. Prices before that day stay in the
  old units, so the April statement (`fidelity = 50.9847 FAST`) and the July one
  (`= 101.9694 FAST`) both reconcile.
- **RSUs**: 60 shares vest each quarter, booked as wages at the market price on the day
  (`northwind -> etrade 60 NWND @ 157.54 USD`), and shares are sold to cover the tax: a sale to a
  `federal-tax` place counts as withholding (8,691.54 USD of it in 2025).
- **The employee stock purchase plan is held money.** Each paycheck sends 1,400.00 USD to
  `espp-cash` marked `for espp`, so the money is tied to the plan: it shows in the balance but not
  in `axiom available`. The purchase is `espp-cash 8_400.01 USD -> etrade 69.3814 NWND for me`,
  which unties it: the money becomes shares Sam owns. The plan buys at 85% of the lower of the
  offering-date and purchase-date prices, so the shares are worth 3,325.45 USD more than they cost.
  That is pay, a second line: a flow from `espp-discount` (a wages place) into the basis of the
  lot, `etrade[2025-06-30].basis 3_325.45 USD`. The lot's cost is the market value, and the
  discount is counted once, as wages.
- **Crypto**: bitcoin bought twice, swapped for ether inside the exchange (with its 0.6% in the
  rate), 6 ETH moved to a wallet with a network fee paid in ETH, monthly staking rewards taxed at
  their value on arrival (`tax.ax`), a long-term BTC sale and a short-term ETH sale.
- **A wash sale.** UNH is sold at a loss on 2025-05-27 and ten shares are bought back on
  2025-06-10. Axiom does not see the 30-day window, so the owner states it: the disallowed 1,978.60 USD
  is a flow from a place of kind `wash-adjustment` into the basis of the new lot,
  `fidelity[2025-06-10].basis`. `us` counted the whole loss when the shares were sold; `disallow-wash-loss`
  in `tax.ax` counts back what the flow moved.
- **Tax-loss harvesting**: VNQ is sold at a loss on 2025-12-16 and SCHH bought the same day.
- **Netting across terms.** A 200 BND lot bought in 2021 is sold at a loss on 2025-08-19:
  long-term, and larger than the year's other long-term results, so it also reduces the short-term
  gains. `us` nets short against long before it taxes anything.

## What the commands should show (hand-verified)

`python3 ../verify/verify06.py` reads the journal text, without Axiom, and works the return
and the net worth; the realized gains come from `oracle06.json`, the lot engine's own books.
Every line agrees with Axiom to the cent.

| line | by hand | Axiom |
|---|---|---|
| wages: salary 168,000.00 + RSU vests 39,507.00 + ESPP discount 3,325.45 | 210,832.45 | 210,832.45 |
| pre-tax: 401(k) deferrals | 23,499.96 | 23,499.96 |
| interest | 5,866.24 | 5,866.24 |
| dividends (cash, and reinvested at the day's price) | 1,200.31 | 1,200.31 |
| staking rewards, at their value on arrival | 263.40 | 263.40 |
| total income (wages less pre-tax, plus the three above) | 194,662.44 | 194,662.44 |
| short-term: realized 4,450.44 + the wash sale's 1,978.60 added back | 6,429.04 | 6,429.04 |
| long-term: realized | -584.67 | -584.67 |
| after netting: short 5,844.37, long 0, loss deduction 0 | | |
| AGI | 200,506.81 | 200,506.81 |
| deductions: the 15,750.00 standard | 15,750.00 | 15,750.00 |
| taxable income | 184,756.81 | 184,756.81 |
| income tax (single brackets, each slice rounded to the cent) | 37,188.63 | 37,188.63 |
| net investment income: 5,866.24 + 1,200.31 + 5,844.37 | 12,910.92 | 12,910.92 |
| net investment income tax: 3.8% of the lesser of 12,910.92 and AGI over 200,000 (506.81) | 19.26 | 19.26 |
| total tax | 37,207.89 | 37,207.89 |
| payments: withheld 31,800.00 from pay + 8,691.54 from shares sold to cover | 40,491.54 | 40,491.54 |
| **federal, owed (negative = refund)** | -3,283.65 | -3,283.65 |
| 401(k) deferral limit: 23,499.96 of 23,500.00 | room 0.04 | room 0.04 |
| net worth on 2026-04-16 | 469,860.96 | 469,860.96 |

The federal return closes on 2026-04-15 with a **refund of 3,283.65 USD**: the tax is 37,207.89 USD and
40,491.54 USD was withheld. Run with `--today 2026-01-06` (`outputs/tax-2025-before-closing.txt`) the return has not
closed: `axiom tax` shows only what was counted, and `axiom forecast` (`outputs/forecast.txt`, run that day)
lists it among the obligations coming due, the refund as a negative amount. The return of 2024 is not
printed: the ledger starts in September 2024, and a partial year's refund would mean nothing.

The year's long-term loss (584.67 USD) is smaller than its short-term gain, so netting lowers the short-term
figure and nothing more: no loss deduction, and no loss to carry into 2026. The wash sale adds back
1,978.60 USD that `us` had already deducted when the UNH shares were sold. Without `disallow-wash-loss` the
return, worked the same way by hand, would have AGI 198,528.21, taxable income 182,778.21, income tax
36,713.77, no net investment income tax (AGI is under 200,000) and a refund of 3,777.77: 494.12 USD too much
(474.86 of income tax at 24% and 19.26 of the 3.8% tax).

`axiom check`: no errors, no warnings. The summary reads 331 flows, 25 places, 17 laws enforced and net worth
469,860.96 USD.

`axiom balance` (plain): cash and 401(k) 297,655.00 USD (checking 84,554.08, savings 83,100.79, ESPP cash 8,400.00,
brokerage cash 3,228.01 and 638.84, the 401(k) 117,733.28), plus the holdings in their own commodities.
Valued at the last price on file, they are worth 172,205.96 USD: net worth **469,860.96 USD**, the same figure
the check prints. There are no liabilities.

`axiom limits 2025`: the 401(k) deferral limit, 23,499.96 of 23,500.00 USD, room 0.04 USD, 100% used.

`axiom available`: 171,521.72 USD to spend. Money in hand is 179,921.72; the 8,400.00 USD of the plan
is listed under *held for espp* and taken off. Every holding is listed with the time it takes to reach and, for
the 401(k), the 10% early-withdrawal penalty (11,773.33 USD) and the federal tax on the withdrawal.

`axiom gains 2025`: 18 short-term rows (proceeds 62,075.84, basis 57,625.40, gain 4,450.44) and 5 long-term rows
(23,869.54, 24,454.21, -584.67). The UNH loss of -2,949.54 is in the short-term rows at its full amount; the
`disallow-wash-loss` law is what adds 1,978.60 back (`axiom why disallow-wash-loss` shows it running once).

`axiom lots fidelity`: the 30 VTI of 2020 with the basis the opening states, one VTI lot per month, the FAST parcel
(50.9847 shares bought before the split, 60 sold in September, 41.9694 left) with its 2024-12-10 date, the
VXUS lots bought at Fidelity after the older ones moved to Schwab, and the wash-sale lot of UNH: 10 shares at
302.21 (3,022.10) plus the 1,978.60 adjustment, basis 5,000.70 USD.

## Findings this project now demonstrates as fixed

F01 (basis is stated, not inferred from the route: the ESPP discount and the wash adjustment are flows into a lot's
basis, and the openings carry their cost), F02 (the split is a journal line: parcels double, cost and day kept, prices before
it stay in the old units), F04 (gains and losses net across terms; the net investment income tax is counted from the
same lines), F10 (the plan's contributions are held `for espp` until the purchase is `for me`), F17 (three lots older
than the ledger, in an opening block, with no laws run for the years before the first flow), F19 (`us` has slots
for deductions, credits, payments and a total: a refund is booked, negative), F25 (`all VXUS`, and an opening that
states cost and day).

## Still open

- **The wash-sale window is not seen.** Axiom has no law that looks 30 days either side of a sale, so the owner
  writes the adjustment. The holding period does not tack either: the replacement UNH lot is dated 2025-06-10, so
  it shows as short-term even though the disallowed loss belongs to a lot bought in January.
- **A purchase with two sources still takes two lines.** The ESPP purchase is the cash the plan held plus, as
  a second line, the discount into the basis; a single transaction cannot have a cash leg and a basis leg.
- **`available` prices a withdrawal against the year so far.** It folds the journal to the day and runs the laws on
  to the day the return closes, but knows no pay to come. The journal ends in 2025, so 2026 has no income yet: the
  7,397.22 USD of unrealized gain in the employer stock lies under the standard deduction and costs nothing in the
  table, and the 401(k) shows the tax the withdrawal itself would owe as well as the penalty.
- **A market fall is not shown.** The 401(k) here has only contributions and interest; the market
  revaluation case (`via market` in an account that penalizes withdrawals) is shown in `05-family`.
