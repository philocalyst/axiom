# 04 — A freelance designer

A single-member design studio's books for tax year 2025, carried through the
2026-04-15 return close. This source now uses the v4 model: parties and purposes
classify each flow, claims represent invoices and bills, and measurements record
business mileage. `accounts.ax` holds accounts, kinds, purposes, parties, and
classification laws; `tax.ax` supplies the sole-proprietor tax laws; `plans.ax`
contains the three 2026 estimated-payment contracts; `journal/` retains each
source event and statement; `prices/prices.ax` declares the IRS mileage rate.

## Preserved independent tax calculation

Run `python3 ../verify/verify04.py`. The verifier reads the current v4 journal,
claims, purpose tags, measures, and price rows directly. It does not parse with
Axiom or copy report output. Its arithmetic reproduces the established 2025
figures:

| Return line | Independently calculated |
| --- | ---: |
| Gross receipts from 20 client payments | $74,800.00 |
| Business expenses, including fees and annual-cost accruals | $13,471.01 |
| Net profit | $61,328.99 |
| Health premiums | $4,944.00 |
| Interest | $290.12 |
| SEP contributions for 2025, including the January 2026 payment | $8,000.00 |
| Self-employment tax | $8,665.51 |
| Adjustments | $17,276.76 |
| AGI | $44,342.35 |
| QBI deduction | $5,718.47 |
| Taxable income | $22,873.88 |
| Income tax | $2,506.37 |
| Federal payments credited to 2025 | $11,100.00 |
| Balance due | $71.88 |

The oracle derives taxable expenses from the typed source purposes. Date-range
charges (insurance, Dropbox, and hosting) are allocated across the coverage
period; the Adobe annual plan is tagged for 2026. Business mileage is valued at
the price in force on each measure date. The 15% home-office share, 60% phone
share, and 50% client-meal allowance are explicit laws in the project.

The journal preserves all amounts, dates, statement assertions, source parties,
invoice identifiers, and payment legs. V3 chart categories were represented as
v4 purposes and typed payees. Client invoices are claims; settlements carry the
same `^inv-*` code. Stripe receipts retain gross income and separately tag the
processor fee. Estimated payments retain their tax year even when paid in 2026.

## Verification status

`outputs/` contains historical v3 snapshots and should not be treated as current
v4 CLI evidence. A root-native model inventory currently reports 449 flows, 439
transactions, 3 contracts, and 60 laws. One claim-waiver event still needs its
native statement-lowering path; the source uses the documented `^claim waived`
form, and the root inventory reports that blocker explicitly. Runtime fold,
report, and golden-output parity have not yet been verified for this project.
