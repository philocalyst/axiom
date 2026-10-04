# 08 - A US citizen who moved to Berlin

Dana works in San Francisco until 2025-06-30, moves to Berlin on 2025-07-01 and is
paid in euros from July. Calendar year 2025, reported in dollars. Accounts in USD,
EUR and GBP, conversions with fees and with spreads, a trip to London paid in
pounds on a US card with a 3% foreign transaction fee, and the tax and
reporting a US citizen abroad owes. Sample data; every statement reconciles to the
cent (an independent Python ledger models all three currencies), and
`python3 ../verify/verify08.py` works the return, California's, the foreign
account peaks and the net worth from the journal text, without Axiom.

```text
axiom.ax                    root: Dana, three residences that overlap from July
accounts.ax                 accounts in three currencies, a running-maximum law per foreign account, purposes, payees
tax.ax                      the student loan interest deduction
systems/us/abroad.ax        foreign earned income exclusion (an adjustment) and its stacking tax (other taxes)
systems/de.ax               German tax counted as foreign tax paid
prices/fx.ax                mid-market EUR and GBP rates on every day the journal uses foreign money
journal/2025/*              January-December 2025
journal/2026/01.ax          one flow: the payment Dana makes in January for 2025
outputs/                    every command, run with --today 2026-04-16 unless it says otherwise
```

## What it models

- **Overlapping residences.** `lives us/ca/san-francisco until 2025-06-30`, then, from July 1,
  `lives us/abroad` and `lives de`. A citizen abroad is under United States law and under the
  country's, on the same days, and each system governs on the days its residence covers.
  San Francisco and California stop applying on 2025-07-01, but California owes for the
  months it did: its return runs for 2025 because the residence touched the year.
- **Foreign earned income exclusion**, a law of `us/abroad`: 37,564.56 USD of euro pay,
  excluded up to 130,000 x 184/365 (65,534.25) USD. It counts into `adjustments`, so `us`
  takes it off income, and the tax the "stacking rule" adds counts into `other-taxes`, so
  the payments Dana already made are set against all of it.
- **Foreign currency is cash.** The German salary (5,400.00 EUR gross, 3,331.30 EUR net, Lohnsteuer and social
  contributions between them: lines through a clearing account, below), the flat's rent and its blocked
  deposit, the savings moved from the US to a German Tagesgeld account, and a Wise account in
  pounds are money. A euro spent on groceries is not a sale: `std` spends a currency oldest first
  (`select fifo` on the kind `currency`), with no account saying so, and `us` counts a currency gain only above 200 USD in one disposal
  (IRC §988(e)). `axiom gains` still lists 105 small disposals (a loss of 39.68 USD in all, with 18 more lines of nothing: what
  leaves the clearing account the day it arrives), and the return counts none of them. `axiom available` counts the euros in hand.
- **Pay is the gross and what comes out of it.** A dollar stub is `us-checking <- acme 10_416.67 USD #wages` and then the
  6% deferral into the 401(k) (`#payroll-deferral`, a purpose with the law that counts it as pre-tax and takes it back
  out of income), the withholding (`-> irs`, `-> ftb`) and the payroll taxes (`-> ssa`, `-> edd`), so the wage law counts the
  gross. A euro stub passes through `de-clearing`: `employer-gmbh -> de-clearing 5_400.00 EUR`, the Lohnsteuer to the
  Finanzamt, the contributions to the insurers, and 3,331.30 EUR to the girokonto, which never holds the gross (the
  running maximum below and the currency disposals read what an account holds). The employer is of kind
  `foreign-employer`, which `pays foreign-wages`, so no line says it.
- **Conversions with a cost.** Two shapes: Wise-style (`girokonto -> 900.00 EUR` with a fee leg to `wise-fees` and
  a USD leg) and bank-style, which states its own rate: `us-savings -> tagesgeld 20_334.11
  EUR @ 1.180283 USD` (the dollars out follow from it, 24,000.00). The mid-market rate is in
  `prices/fx.ax`, and the difference is the implicit 1.1% spread. A fee leg to a party that carries `#fees` (a spending
  purpose) is a cost of the exchange: the 55.10 USD of the first Wise transfer is part of what the euros
  cost, so their basis is 9,500.00 USD and not 9,444.90 (`axiom lots girokonto --today 2025-06-28`).
- **The London trip.** Hotel and minibar charged in pounds on the US card at the network's rate
  (`visa -> london-hotel 640 GBP @ 1.336734375 USD`, the 855.51 USD on the statement), and a separate 3% fee flow to the
  card's issuer (`entity card-issuer #fees`), all on the card statement. Wise pays the small purchases in pounds, and what is left goes
  back to euros.
- **Payments for another year.** In April 2025 Dana pays the balance due on the 2024 returns,
  `us-checking -> irs 640 USD for 2024` and 215 USD to the FTB (`#prior-year-state-tax for 2024`). The money leaves the
  account that day and is not a payment toward 2025. In January 2026 Dana works out the year's tax,
  stacking rule included, and pays 1,200.00 USD `for 2025`; the 2025 return, which closes on April
  15, counts it.
- **Student loan interest**, a law of the project counting into `adjustments`: the servicer's
  statement splits each payment, the principal goes into the loan and the interest leg to the servicer, which carries
  `#student-loan-interest`.
- **FBAR.** A US person with foreign accounts worth over 10,000 USD in total at any time files
  FinCEN 114 with each account's highest balance. Each foreign account carries a law that keeps a
  running maximum in a tally (`count max(v - value(tally(x), USD), empty) as x`); a project law at year end adds
  them up and warns.

## What the commands should show (hand-verified)

`python3 ../verify/verify08.py` prints the figures below. Every one agrees with Axiom to the cent.

| line | by hand | Axiom |
|---|---|---|
| wages: San Francisco 62,500.02 + Berlin at each pay day's rate 37,564.56 | 100,064.58 | 100,064.58 |
| pre-tax 401(k) | 3,750.00 | 3,750.00 |
| interest, US and German, gross | 1,494.44 | 1,494.44 |
| total income | 97,809.02 | 97,809.02 |
| student loan interest, 12 payments, under the 2,500.00 limit | 1,333.42 | 1,333.42 |
| foreign earned income excluded (pay 37,564.56, cap 65,534.25) | 37,564.56 | 37,564.56 |
| AGI: 97,809.02 - 1,333.42 - 37,564.56 | 58,911.04 | 58,911.04 |
| deductions: the standard 15,750.00 beats itemized (state tax 2,820.00) | 15,750.00 | 15,750.00 |
| taxable income | 43,161.04 | 43,161.04 |
| income tax | 4,940.82 | 4,940.82 |
| stacking tax: tax on 80,725.60, less tax on 37,564.56, less tax on 43,161.04 | 3,463.56 | 3,463.56 |
| total tax | 8,404.38 | 8,404.38 |
| payments: withholding 6 x 1,180.00 + the January payment 1,200.00 (not the 640.00 for 2024) | 8,280.00 | 8,280.00 |
| **federal, owed** | 124.38 | 124.38 |
| California taxable income (AGI less 5,706.00) | 53,205.04 | 53,205.04 |
| California tax, withheld 2,820.00 (not the 215.00 for 2024) | 1,727.19 | 1,727.19 |
| **California, owed (negative = refund)** | -1,092.81 | -1,092.81 |
| German tax paid, in dollars | 6,357.40 | 6,357.40 |
| FBAR: girokonto 9,444.90 + tagesgeld 24,093.88 + kaution 4,223.52 + wise 1,164.86 | 38,927.16 | 38,927.16 |
| 401(k) deferral limit: 3,750.00 of 23,500.00 | room 19,750.00 | room 19,750.00 |
| net worth on 2026-04-16 | 113,626.85 | 113,626.85 |

The federal return closes on 2026-04-15 owing **124.38 USD** and California's with a **refund of 1,092.81
USD**. Run with `--today 2026-01-06` (`outputs/tax-2025-before-closing.txt`) the returns have not
closed: `axiom tax` shows what was counted and `axiom forecast` (`outputs/forecast.txt`, run that day) lists
both among the obligations coming due, the refund as a negative amount.

`axiom check`: no errors, and one warning, the FBAR reminder (38,927.16 USD summed over four accounts).
The summary reads 322 flows, 48 places, 26 laws enforced and net worth 113,626.85 USD.

`axiom balance --value`: assets 141,094.15 USD (bank 37,365.69, Germany 37,128.46, the 401(k) 66,600.00),
liabilities 27,467.30 USD (student loan 27,413.82, card 53.48): net worth 113,626.85 USD.

`axiom available`: 70,259.47 USD to spend: checking and savings (18,805.75 and 18,559.94 USD), and now the euros in the current account and
the Tagesgeld (8,799.90 and 24,093.88 USD). The blocked deposit (4,234.68 USD, 700 days) and the 401(k) are
in the second table, the 401(k) with its 6,660.00 USD early-withdrawal penalty.

`axiom limits 2025`: the FBAR total at 389.27% of its 10,000.00 USD threshold, the groceries budget in
December at 54.11%, and the 401(k) deferral limit.

## Findings this project now demonstrates as fixed

F09 (two residences that overlap, and a part-year residence that files: California's return is computed,
1,727.19 USD against 2,820.00 withheld, where before it never existed), F14 (a foreign currency is
cash: no lot to choose, no capital gain per coffee, the euros are in `available`, and a conversion can state
its own rate), F07 (a payment says which year it is `for`, in both directions), F19 (the exclusion is an
adjustment and the stacking tax counts into `other-taxes`, so the payments are set against it: the old
project owed 3,599.64 USD of stacking tax as a separate obligation that ignored the 10,740.00 USD of
withholding), F10 (`available` counts the euros), F17 (an opening statement).

## Still open

- **The exclusion's qualifying days are a param.** Laws cannot count days, so `days-abroad` (184) is written
  in `systems/us/abroad.ax`.
- **California taxes the whole federal AGI.** `us/ca` starts from `agi` and does not prorate to the part of
  the year Dana lived there, and California does not allow the exclusion: Dana's real bill would be a
  little higher. The simplification is written in `us/ca`.
- **The foreign account report is a running maximum by hand.** One law per account, the tally used as a memory
  cell, and the account names spelled out in the year-end law: a `max over the year` aggregate is still not
  expressible.
- **A conversion's cost is not derived.** The 1.1% spread and the Wise fee are implicit or typed; no report
  says what converting cost against the mid-market rate in `prices/fx.ax`.
- **`gains` lists currency disposals.** It shows every euro spent as a disposal with a gain, although the return
  counts none of them, and the euros that leave the clearing account the day they arrive as 18 disposals of nothing.
- **A pay stub written as a split counts only what lands.** `me <- acme 10_416.67 USD` with `->` legs for the 401(k), the
  withholding and the rest counts as wages what reached the account (7,219.80 USD of the stub), not the gross, so the stubs are
  written as the gross and what comes out of it, in euros through a clearing account.
- **A two-amount line the upgrade cannot place is written with the price that gives its amount.** The minibar,
  `visa 77.75 USD -> london-hotel 58.20 GBP`, has no exact price (77.75 / 58.20 does not terminate), so it says
  `@ 1.335911 USD`, which rounds to the 77.75 USD of the statement.
- **`available` prices a withdrawal against the year so far.** It folds the journal to the day and runs the laws on
  to the day the return closes, but knows no pay to come: the 401(k) shows the tax its withdrawal would add to the
  income counted until then, as well as the early-withdrawal penalty.
