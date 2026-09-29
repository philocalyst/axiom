# 08 - A US citizen who moved to Berlin

Dana works in San Francisco until 2025-06-30, moves to Berlin on 2025-07-01 and is
paid in euros from July. Calendar year 2025, reported in dollars. Accounts in USD,
EUR and GBP, conversions with fees and with spreads, a trip to London paid in
pounds on a US card with a 3% foreign transaction fee, and the tax and
reporting a US citizen abroad owes. Sample data; every statement reconciles to the
cent (an independent Python ledger models all three currencies). The point is the
friction: see `../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax                    root: Dana, `lives us/ca/san-francisco` then `lives us/abroad/de from 2025-07-01`
accounts.ax                 accounts in three currencies, a running-maximum law per foreign account, payees
systems/us/abroad.ax        foreign earned income exclusion (with its stacking tax); a system written for this project
systems/us/abroad/de.ax     German tax counted as foreign tax paid
prices/fx.ax                mid-market EUR and GBP rates on every day the journal uses foreign money
journal/2025/*              January-December 2025
wishes.ax.txt               not loaded (the loader only reads *.ax)
outputs/                    every command, run with --today 2026-01-06
```

## What it models

- **Living under two jurisdictions.** A US citizen abroad remains under `us` and
  is also under Germany. `lives` cannot say two things at once (a second `lives`
  ends the first), so Berlin is spelled `us/abroad/de`, a jurisdiction nested
  *under* `us` so that `us` applies by its path. San Francisco and California
  stop applying on 2025-07-01, which is the point of the dated `lives`.
- **Three currencies.** The German salary (5,400.00 EUR gross, 3,331.30 EUR net,
  Lohnsteuer and social contributions as split legs), the flat's rent and its
  blocked deposit, the savings moved from the US to a German Tagesgeld account,
  and a Wise account in pounds. `axiom balance --value` and `axiom flow` value
  everything in dollars.
- **Conversions with a cost.** Two shapes: Wise-style (`girokonto 900 EUR ->` with
  a fee leg and a USD leg) and bank-style (`us-savings 24_000 USD -> tagesgeld
  20_334.10 EUR`, no fee line: the 1.1% spread is implicit). The mid-market rate
  is in `prices/fx.ax` and the conversions never write a rate, because `@ 1.0860
  USD` is rejected (USD counts two decimals).
- **The London trip.** Hotel and minibar charged in pounds on the US card: the
  USD amount, the GBP expense it bought, and a separate 3% fee flow, all three on
  the card statement. Wise pays the small purchases in pounds, and what is left
  goes back to euros.
- **FBAR.** A US person with foreign accounts worth over 10,000 USD in total at
  any time files FinCEN 114 with each account's highest balance. Each foreign
  account carries a law that keeps a running maximum in a tally (`count max(v -
  tally(x), empty) as x`); a project law at year end adds them up and warns.
- **Foreign earned income exclusion** (`systems/us/abroad.ax`): 37,564.58 USD of
  euro pay, excluded up to 130,000 x 184/365, with the "stacking" tax that
  follows it.

## What the commands should show (today = 2026-01-06)

`axiom check`: one warning, the FBAR reminder (38,927.15 USD summed over four
accounts: 9,444.90 + 24,093.87 + 4,223.52 + 1,164.86). Net worth 110,425.84 USD.

`axiom tax 2025`: wages 100,064.60 USD = 62,500.02 (San Francisco) + 37,564.58
(Berlin, at the rate of each pay day); foreign-earned 37,564.58 all excluded;
AGI 60,271.81 USD; taxable income 44,521.81 USD; income tax 5,104.12 USD; withheld
10,740.00 USD. By hand, the stacking rule adds tax on 82,086.39 USD less tax on
the excluded 37,564.58 USD less tax on the 44,521.81 USD that remains, which is
3,599.63 USD (Axiom: 3,599.64 USD). German tax paid: 6,357.40 USD.

Three things to distrust:

- **California is never computed.** Dana was a resident until June 30 and owes
  California tax on that part of the year, but California's year-end law only runs
  for someone living there on December 31. `ca-withheld` is counted (4,416.00 USD);
  `ca-income-tax` does not exist.
- **Euro spending is taxed as foreign-exchange gain.** 105 small "realized gains"
  make 27.34 USD of short-term gains: every euro that leaves the account is a sale
  of euros bought at another rate.
- **`feie-stacking-tax` is "owed" 3,599.64 USD** although 10,740.00 USD was
  withheld against a total of 8,703.76 USD: the withholding credit is applied
  inside the shipped federal law only, so the extra obligation ignores it.

`axiom available`: 34,164.69 USD. The 7,481.00 EUR in the current account and
20,482.76 EUR in the savings account are "now" liquid, but sit in the second
table: only base-currency money is spendable.

`axiom forecast`: reports the FBAR warning as a problem on 2026-12-31.
