# 07 - A landlord

One rental house, bought on 2024-12-18 (372,000 USD, 25% down, a 279,000 USD
mortgage at 6.75%) and sold on 2025-12-29 (431,500 USD), with a day job on the
side. Sample data. Every bank, deposit and mortgage statement reconciles, and an
independent Python check (`python3 verify.py`) reproduces the expenses, income and the gain by hand.
The point is the friction: see `../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax           root; a `sync` line that "fetches" the house's price
accounts.ax        kinds (depreciable, deposit-holder), the house, the roof, tenants
tax.ax             Schedule E and the passive-loss allowance
prices/home.ax     what `scripts/home-prices.sh` prints (an offline stand-in for an API)
scripts/           the sync script
journal/2024/12    closing day
journal/2025/*     January-December 2025
wishes.ax.txt      not loaded (the loader only reads *.ax)
outputs/           every command, run with --today 2026-01-06
```

## What it models

- **Closing.** One day, several flows: the lender funds 279,000 USD, the house is
  bought `@ 376_850 USD` (price plus 4,850 USD of closing costs that belong in
  basis), 3,120 USD of loan costs become a prepaid asset amortized 8.67 USD a month,
  and 722.30 USD of prepaid interest is an expense (of 2024).
- **Tenants.** Tenant A: 2,400 USD a month Feb-Aug, a security deposit of
  2,400 USD kept in its own account, a late payment in June with a 75 USD fee, and
  a move-out on 2025-09-12 with 350 USD kept for damage. One vacant month
  (September: no rent, the landlord pays the utilities, a new listing and 1,150 USD
  of turnover repairs). Tenant B: 2,500 USD a month from October, deposit 2,500 USD.
  Each tenant's deposit is a liability (`liabilities/deposits/tenant-a`) and a tie: money
  from a `deposit-holder` entity stays tied to it, so `available` does not spend it.
  Rent is written without a payee and marked `#lease-a` instead, because rent from
  the same entity would be tied too.
- **Repairs versus improvements.** The plumber, the dishwasher, the gutters, the
  turnover paint and the screens are expenses. The new roof (14,200 USD) is not:
  it is its own asset, `assets/rental/roof`, its own commodity (`ROOF`) and its own
  depreciation.
- **Depreciation.** Booked as monthly flows `equity/acc-dep-house -> expenses/rental/depreciation`,
  no cash, 913.58 USD a month on the building (80% of the price over 27.5 years,
  half a month in January and in December, the mid-month convention).
  What piles up in `equity/acc-dep-*` is the accumulated depreciation, which the
  sale reads (`kind depreciable`'s `recapture` law).
- **The sale.** The closing statement nets 26,968.75 USD of costs out of the price,
  so the house and roof sell for 404,531.25 USD together, split by hand (roof at its
  adjusted basis, 14,070.90 USD). The mortgage is paid off (276,282.05 USD plus
  1,450.48 USD of December interest), the remaining loan costs are deducted
  (3,024.63 USD), and tenant B's deposit goes to the buyer.

## What the commands should show (today = 2026-01-06)

`axiom check`: no diagnostics, 197 flows, net worth 173,866.60 USD, all of it in
checking (the rental account, the deposit account, the house and the loan are
all at zero).

`axiom tax 2025`: wages 96,000.00 USD; rental income 24,725.00 USD (rent 24,300 +
late fee 75 + kept deposit 350); rental expenses 42,181.10 USD (interest 18,638.02,
depreciation 10,178.48, property tax 4,380.00, amortization 3,120.00, repairs
2,051.00, management 1,950.00, insurance 1,560.00, utilities 154.60, advertising
149.00); net rental loss -17,456.10 USD, allowed in full under the 25,000 USD
allowance; long-term gain 13,610.35 USD; recapture 10,049.38 USD, taxed as ordinary
income (the roof's 129.10 USD recapture nets against its own 129.10 USD loss);
AGI 102,203.63 USD; taxable income 86,453.63 USD; income tax 12,981.07 USD; withheld
14,580.00 USD (a refund of 1,598.93 USD is not booked). By hand, the sale's tax gain
is 23,659.73 USD = 13,610.35 + 10,049.38.

`axiom flow` shows "realized gains" of 13,481.25 USD for the sale, the gain from
cost, not the 23,659.73 USD taxed: depreciation reduces the profit of the year
and never reduces the parcel's basis, so the books and the tax disagree by the
depreciation.

`axiom available --at 2025-06-01`: 40,175.00 USD, of which the loan costs
(3,076.65 USD of prepaid asset) count as spendable cash, and 2,400.00 USD is tied to
tenant A. The house is listed at 388,500.00 USD net, with no mortgage against it
and no cost of selling.

`axiom lots`, `axiom available` (today): 350.00 USD is still "tied to tenant-a" in
checking, four months after the deposit was settled and its liability reached zero.

`axiom forecast` (today): still projects rent, mortgage payments, depreciation and
management fees for a house that was sold, and reports the rental account
"overdrawn" on 2026-02-01.
