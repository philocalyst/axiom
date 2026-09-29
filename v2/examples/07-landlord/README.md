# 07 - A landlord

Jamie, 45, a nurse practitioner who owns one rental house: bought on 2024-12-18
(372,000 USD assessed, 376,850 USD with closing costs, 25% down, a 279,000 USD mortgage at
6.75%) and sold on 2025-12-29 (431,500 USD), with a day job on the side. Sample data.
Every bank, deposit, loan and bill statement reconciles to the cent, and
`python3 ../verify/verify07.py` works the loan, the depreciation, the year's rental
figures, the sale and the 2025 return from the journal text, without Axiom.

```text
axiom.ax           root; a `sync` line that "fetches" the house's price
accounts.ax        kinds (rental-property, tenant), the house, the banks, three payables
plans.ax           paycheck, rent-a, rent-b, mortgage-payment, manager-fee, depreciation
tax.ax             Schedule E and the passive-loss allowance
prices/home.ax     what `scripts/home-prices.sh` prints (an offline stand-in for an API)
scripts/           the sync script
journal/2024/12    the opening balance and the closing day
journal/2025/*     January-December 2025
outputs/           every command, run with --today 2026-04-16 unless it says otherwise
```

What `us` did not have is here in one short file: Schedule E (rents less the expenses under
`expenses/rental`, counted into `total-income`) and the rent that counts when it arrives.
The sale's depreciation recapture is a law of the kind `rental-property`, seven lines,
so it governs every rental the project will ever own.

## What it models

- **The house's basis is a flow.** The closing puts 376,850 USD in the house (price plus
  4,850 of closing costs). The new roof is a flow into it, `bills -> house.basis 14_200 USD`.
  Depreciation is a flow out of it, `house.basis -> expenses/rental/depreciation`: the amount
  is recognized as an expense at the target, the basis falls, the quantity stays 1 HOME.
  So there is no `equity/acc-dep-house`, no second commodity for the roof, and the
  gain the sale realizes is the gain the tax return needs.
- **Depreciation is a plan.** 913.58 USD a month on the building (80% of the price over 27.5
  years), half a month in the first and last months, and the roof's 43.03 USD a month from
  mid-September. The journal writes one word a month, `2025-03-31 depreciation`, and an amount
  in the months that differ.
- **The mortgage is a claim.** `mortgage -> rental-bank 279_000 USD #loan due 2054-12-18` is
  one payable, settled by every payment `for #loan` (principal), with the interest a separate
  leg into an expense. `axiom claims` lists it, and it is gone the day it is paid off.
  The payment is a plan too: 1,809.59 USD on the first, and each month's occurrence writes the
  two legs that changed.
- **Rent is a plan.** Tenant A's `rent-a` runs February to August and tenant B's `rent-b` from
  October; each stops where its reason stops, so the forecast made after the sale does not
  collect rent or pay a mortgage on a house Jamie no longer owns. The journal writes the day
  the money came (`2025-02-03 rent-a`), and June's late fee as a flow of its own.
- **Deposits are tied to the tenant.** `deposits -> deposit-bank 2_400 USD / tenant-a #dep-a
  for tenant-a due 2025-09-30` is a claim on Jamie (a payable) and money that is not
  available to spend. When tenant A leaves, 2,050.00 USD goes back (`for #dep-a`), 350.00 USD
  settles the rest of the claim as income (`income/forfeited-deposits`, rent-income kind), and
  `deposit-bank -> rental-bank 350 USD for me` releases what Jamie keeps. Tenant B's 2,500.00
  USD is credited to the buyer in the closing statement, and the money is released.
- **The loan costs are spread by date.** 3,120.00 USD paid at closing, written
  `2024-12-18..2025-12-29 rental-bank -> amortization 3_120 USD`: recognized evenly per day
  over the 377 days the loan lasted, 115.86 USD in 2024 and 3,004.14 USD in 2025. The 722.30 USD
  of prepaid interest is 2024's.
- **The sale is one closing statement.** `2025-12-29 house 1 HOME -> 404_531.25 USD` with legs:
  the loan payoff (276,282.05 USD `for #loan`, and 1,450.48 USD of interest to the day),
  tenant B's deposit credit to the buyer, and the rest into the rental account. The
  404,531.25 is the price less the seller's costs (5% commission and 1.25% of other costs,
  26,968.75 USD).
- **Recapture.** The sale's gain is measured from the adjusted basis, so `us` counts all of
  it as long-term. `recapture`, a law of `rental-property`, moves as much of it as the
  depreciation taken (`tally(depreciation)`, counted by a law on the expense place) to the
  ordinary-rate line.
- **Repairs versus improvements.** The plumber, the dishwasher, the gutters, the turnover
  paint and the screens are expenses. The roof is an improvement: a bill (a payable due in a month)
  that raises the basis, paid on 2025-10-10.

## What the commands should show (hand-verified)

`python3 ../verify/verify07.py` reads the journal, checks every mortgage occurrence against its
own amortization, every depreciation and management-fee occurrence against its formula, and prints
the figures below. All agree with Axiom to the cent.

| line | by hand | Axiom |
|---|---|---|
| mortgage payment (279,000 at 6.75%, 360 months) | 1,809.59 | 1,809.59 (every leg matches) |
| interest paid in 2025: eleven payments 17,187.54 + payoff accrual 1,450.48 | 18,638.02 | 18,638.02 |
| payoff principal on 2025-12-29 | 276,282.05 | 276,282.05 |
| depreciation: 301,480.00 building, half months in January and December, the roof from September | 10,178.48 | 10,178.48 |
| loan costs recognized in 2025 (3,120 x 363/377, 2024 took 115.86) | 3,004.14 | 3,004.14 |
| rental income: rent 24,300 + late fee 75 + kept deposit 350 | 24,725.00 | 24,725.00 |
| rental expenses (interest, depreciation, property tax 4,380.00, amortization, repairs 2,051.00, management 1,950.00, insurance 1,560.00, utilities 154.60, advertising 149.00) | 42,065.24 | 42,065.24 |
| Schedule E net (allowed in full, under the 25,000 allowance) | -17,340.24 | -17,340.24 |
| adjusted basis: 376,850 + 14,200 roof - 10,178.48 | 380,871.52 | 380,871.52 |
| gain: 404,531.25 - 380,871.52 | 23,659.73 | 23,659.73 |
| of which depreciation recaptured, taxed as ordinary income | 10,178.48 | 10,178.48 |
| of which long-term | 13,481.25 | 13,481.25 |
| wages | 96,000.00 | 96,000.00 |
| total income = wages + Schedule E | 78,659.76 | 78,659.76 |
| AGI = total income + 10,178.48 + 13,481.25 | 102,319.49 | 102,319.49 |
| taxable income (15,750.00 standard deduction) | 86,569.49 | 86,569.49 |
| income tax (single brackets on 73,088.24 of ordinary income; 15% on the 13,481.25) | 13,015.59 | 13,015.59 |
| net investment income tax (AGI is under 200,000) | 0.00 | 0.00 |
| payments: federal withholding, 12 x 1,215.00 | 14,580.00 | 14,580.00 |
| **federal, owed (negative = refund)** | -1,564.41 | -1,564.41 |
| net worth on 2026-04-16 (all of it in checking) | 173,866.60 | 173,866.60 |

The return closes on 2026-04-15 with a **refund of 1,564.41 USD**. Run with `--today 2026-01-06`
(`outputs/tax-2025-before-closing.txt`) the return has not closed: `axiom tax` shows the rent, the
expenses and the recapture, and `axiom forecast` (`outputs/forecast.txt`, run that day) lists the refund among the
obligations coming due. The 2024 figures are a stub: the ledger opens on 2024-12-18, and a 2024 return
would show only a net rental loss of 838.16 USD (the prepaid interest and 14 days of loan costs).

`axiom check`: no diagnostics. 182 flows, 28 places, 15 laws, net worth 173,866.60 USD, all of it in
checking (the rental account, the deposit account, the house, the loan, the bill and the deposits are
all at zero).

`axiom flow --by year` shows "realized gains" of 23,659.73 USD for the sale, the same figure the return
taxes: the depreciation has already lowered the basis. `axiom gains 2025`: one row, proceeds 404,531.25,
basis 380,871.52, gain 23,659.73, long.

`axiom available --today 2025-06-01`: 37,098.35 USD to spend. Money in hand is 39,498.35 (checking
35,189.60, the rental account 1,908.75, the deposit account 2,400.00) less 2,400.00 held for tenant-a.
The loan costs are no longer an asset that counts as cash. `axiom claims --today 2025-09-20` lists two claims
against Jamie: the roofer's invoice (14,200.00 USD, due 2025-10-15) and the mortgage (277,040.01 USD, due
2054-12-18). Today it lists nothing.

## Findings this project now demonstrates as fixed

F01 (basis is a flow: the roof and the depreciation move the house's basis, and the sale realizes the gain from
it; `flow` and `tax` now agree, where before they disagreed by the depreciation), F06 (a mortgage, a bill and two
deposits are claims: `claims`, `available` and `tax` agree, and the 350.00 USD Jamie kept is released with
`for me` instead of staying tied to the tenant for ever), F13 (plans end with `until`: the forecast made after the
sale projects no rent, no mortgage and no depreciation, and no overdraft), F18 (the loan costs are spread by date;
depreciation is one plan, not twelve memo flows between `equity/acc-dep-*` places), F19 (Schedule E counts into
`total-income`, and a refund is booked, negative), F25 (the sale and its payoff are one closing statement with
legs), F10 (the deposit is held, not spendable).

## Still open

- **Selling costs cannot reduce the amount realized.** LANGUAGE says an expense leg of an exchange is a cost of
  it, but `gains` ignores it: a 26,968.75 USD leg into an expense would be deducted and the gain would stay at
  proceeds less basis. The closing statement here is written net (404,531.25 USD), and the price of 431,500 USD
  is in a comment and in `prices/home.ax`, not in the journal.
- **A recognition range cannot be cut short.** The loan costs should have been written for 30 years and
  accelerated at payoff. Written that way, the rest would never be deducted; this journal, written after the
  sale, gives the range the loan's actual life.
- **`balance --value` counts basis flows as money.** At 2025-08-31 it shows the house at 398,148.15 USD instead
  of the 405,000.00 USD price, the depreciation taken (6,851.85) taken off; after the roof it adds 14,200.00. And
  `axiom register house` lists the basis flows as dollar amounts with a dollar running balance. Both need to
  skip `PLACE.basis` flows.
- **A place that `holds HOME` refuses basis flows.** `assets/rental/house` cannot say `holds HOME` (the basis
  flows are dollars: `not-held`), so the account is not restricted to its commodity.
- **Same-day order still matters.** The half month of depreciation on the day of sale must be journalled before
  the sale: the recapture law reads what has been counted so far.
- **Amortization is typed.** The split of each mortgage payment is computed outside and written on the
  occurrence; a `payable` has no rate to check it against.
- **`available` values the house without its mortgage.** It lists 388,500.00 USD net at 2025-06-01, with the
  277,785.33 USD claim in `claims` but not netted.
