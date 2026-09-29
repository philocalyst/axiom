# 06 - An investor

A software engineer in Seattle (no state income tax) with a taxable brokerage,
employer stock, an employee stock purchase plan and some crypto. September 2024
to December 2025, tax year 2025 the one to read. Sample data. Every bank, cash and
holding assertion reconciles, and an independent Python lot engine (FIFO, LIFO,
HIFO and named lots, transfers, exchanges) reproduces Axiom's realized gains
line for line. The point is the friction: see `../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax          root: me (single, born 1991, lives us)
accounts.ax       commodities, brokers, kinds for staking and wash sales
tax.ax            staking income; a wash-sale add-back
prices/stocks.ax  closing prices on every trading day used and every month end
prices/crypto.ax  BTC and ETH, the same way
journal/2024/*    September-December 2024 (the ledger cannot start earlier: see below)
journal/2025/*    January-December 2025
wishes.ax.txt     not loaded (the loader only reads *.ax)
outputs/          every command, run with --today 2026-01-06
```

The ledger starts in September 2024 because the shipped `us` system has no tax
figures before 2024, and a ledger with a lot dated 2023 fails with
`no-param-row ... 2023-12-31` (see finding "history before the first
tax year"). So every lot here is younger than 16 months.

## What it models

- **A monthly habit**: 1,500 USD of VTI on the 5th, sixteen lots, each bought
  `@` the day's price. Reinvested dividends (`income/dividends -> fidelity 0.0649 VTI @ price`)
  add a small lot every quarter.
- **Specific lots and a policy.** `fidelity[2024-09-05, 2024-10-05] 10 VTI` picks
  two lots by date (both past a year, so long-term); the plain sale of 8 VTI in
  December lets the account's `select hifo` choose (two short-term lots, the
  smallest gain). Naming two lots and a quantity that does not exhaust them does not
  mean "from each in turn": the account's policy chooses among the named lots,
  here highest cost first, and the report shows it.
- **A transfer between brokers**: 38.2029 VXUS and 33.4448 BND move from
  Fidelity to Schwab in kind (`fidelity 38.2029 VXUS -> schwab`). Basis and
  acquisition dates arrive unchanged (see `axiom lots schwab`).
- **RSUs**: 60 units vest each quarter, booked as wages at the market price on
  the day (`northwind -> etrade 60 NWND @ price`), and shares are sold to cover
  federal tax and FICA (a sale to a `federal-tax` place counts as withholding).
- **ESPP**: two purchases at 85% of the lower of the offering-date and
  purchase-date prices, funded from `espp-cash` and from `income/espp-discount`
  (wages), so the lot's basis is the market value. It takes two transactions,
  because one transaction cannot have two sources and a share purchase.
- **Crypto**: bitcoin bought twice, swapped for ether (with the exchange's 0.6%
  inside the rate), 6 ETH moved to a wallet with a network fee paid in ETH, monthly
  staking rewards taxed at their value on arrival, a long-term BTC sale and a
  short-term ETH sale.
- **A wash sale, by hand**: UNH lot sold at a loss on 2025-05-27, ten shares
  bought back on 2025-06-10. Axiom cannot see the window, so the 1,978.60 USD
  loss on those ten shares is parked in `equity/wash-sale` and released into the
  replacement's basis; `tax.ax` adds it back to short-term gains. The
  replacement's holding period is not carried over.
- **Tax-loss harvesting**: VNQ sold on 2025-12-16 at a loss, SCHH bought the same
  day.
- **The stock split** is not in the journal. FAST split 2-for-1 on 2025-05-22 and
  Axiom has no way to say so. The lots are entered *already split-adjusted*
  (100 shares of Fastenal bought 2024-12-10 at the halved price), so a
  statement dated before the split cannot be asserted.

## What the commands should show (today = 2026-01-06)

`axiom check`: no diagnostics. 328 flows, net worth 430,872.40 USD.

`axiom tax 2025`: wages 210,832.45 USD (salary 168,000 + RSU 39,507.00 + ESPP
discount 3,325.45), pre-tax 23,499.96, interest 5,846.29, dividends 467.60, staking
263.40, short-term gains 6,429.04 (4,450.44 realized + 1,978.60 wash-sale add-back),
long-term gains 1,771.33, AGI 202,110.15, taxable income 186,360.15, income tax
37,414.02, withheld 40,491.54, nothing owed (a 3,077.52 refund is not booked).
Checked by hand: ordinary income 184,588.82 taxed on the 2025 single brackets is
37,148.32, and the long-term gain adds 15% = 265.70.

`axiom lots fidelity`: 28 lots (19 of VTI). `axiom lots schwab`: VXUS and BND with their
original 2024-09-18 dates. `axiom lots etrade`: five NWND lots (four vests and the
June ESPP purchase; the November 2024 vest lot and the December ESPP lot were sold
in full). `axiom lots wallet`: the staked ether, one lot per reward.

Two numbers to distrust:

- `axiom available` lists what liquidating each holding would cost, computed "as if
  the year ended today", that is, on top of an empty 2026. So the 7,397.21 USD
  short-term gain on the employer stock, worth about 1,775 USD at this person's 24%
  bracket, costs nothing in the report.
- `axiom forecast`'s "liquid net worth" (313,139.12 USD) counts every security
  at market as liquid, and `axiom available` (164,334.90 USD) counts the
  employee stock plan's 8,400.00 USD, which cannot be touched until the
  purchase date, as spendable cash.
