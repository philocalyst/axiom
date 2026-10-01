# 07 - A landlord

Jamie is a nurse practitioner with one rental house. She bought it on 2024-12-18 for
$376,850 including closing costs, borrowed $279,000 at 6.75%, and sold it on 2025-12-29
for $431,500. The source uses the v4 model: identified assets, parties, purposes,
contracts, and dated journal statements.

## Source files

- `axiom.ax` imports `std`, `us`, and `us/rental`.
- `accounts.ax` declares the three cash accounts, loan/bill/deposit claims, native
  expense purposes, and counterparties. It has no income, expense, or equity chart.
- `assets.ax` declares the house as a `rental-home`. It records $75,370 of nondepreciable
  land and a 2025-01-15 in-service date. The depreciable building basis is $301,480.
  `us/rental` treats the September roof as a separate improvement part.
- `contracts.ax` carries the wage, lease, and mortgage terms. Journal contract
  occurrences retain the dates and amounts that differed from the standing terms.
- `tax.ax` adds Schedule E, the documented passive-loss allowance simplification, and
  depreciation recapture. `us/rental` supplies the standard rental income, cost, and
  depreciation laws.
- `journal/` contains the original bank balances, counterparties, purpose-tagged flows,
  loan statements, and sale allocations. The journal no longer repeats depreciation
  as a manually typed flow; the independent verifier derives it from the asset parts.
- `prices/home.ax` retains the quarter-end home value observations in native price form.
- `outputs/` are historical v3 captures. They are not evidence of v4 runtime behavior.

## Independent arithmetic check

Run `python3 ../verify/verify07.py`. The verifier reads contract rates and amounts,
asset basis/land/service data, the roof invoice and sale, and dated native flows. It
independently recomputes the amortization, cumulative depreciation, rental totals,
sale, tax arithmetic, and every monthly cash/claim statement. It imports no Axiom
model or engine code.

| 2025 result | Independently computed |
|---|---:|
| Mortgage payment | $1,809.59 |
| Mortgage interest, scheduled payments | $17,187.54 |
| Interest accrued through payoff | $1,450.48 |
| Principal paid off on 2025-12-29 | $276,282.05 |
| Building depreciation, 11 months | $10,049.33 |
| Roof depreciation, 3 months | $129.09 |
| Total depreciation | $10,178.42 |
| Loan costs recognized | $3,004.14 |
| Rental income: rent, late fee, retained deposit | $24,725.00 |
| Rental expenses | $42,065.18 |
| Schedule E net | -$17,340.18 |
| Adjusted basis at sale | $380,871.58 |
| Amount realized after seller costs | $404,531.25 |
| Gain | $23,659.67 |
| Depreciation recapture / long-term gain | $10,178.42 / $13,481.25 |
| Wages / total income | $96,000.00 / $78,659.82 |
| AGI / taxable income | $102,319.49 / $86,569.49 |
| Federal tax / withholding | $13,015.59 / $14,580.00 |
| Amount owed (negative is refund) | -$1,564.41 |
| Checking after sale and settlement | $173,866.60 |

The depreciation convention is global cumulative boundary rounding: each period's
recovery is the rounded cumulative amount through its end less the rounded cumulative
amount through the prior boundary. It yields $10,049.33 for the building and $129.09
for the roof. This is six cents below the old independently rounded monthly slices;
the basis, gain, and recapture figures above use the cumulative convention.

The tax estimate deliberately omits the passive-loss phase-out above $100,000 of
income, the qualified business income deduction, and state tax. It assumes the full
$25,000 active-participation allowance and taxes recapture as ordinary income below
the 25% recapture ceiling.

## Cutover status

The source parses with no syntax diagnostics. The current model inventory still reports
44 `statement-lowering` errors on contract occurrences and the dated invoice/claim
records; these are implementation gaps in native record lowering, not numbers waived
from the example. The native engine has not yet been run against this source, so no
runtime or report parity is claimed. The independent arithmetic and month-end statement
checks pass, but the old files under `outputs/` must be regenerated after contract,
claim, asset-disposal, and tax execution are implemented.
