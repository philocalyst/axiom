# 04 - A freelance designer

A solo brand designer's books for tax year 2025, plus January 2026 (the fourth
estimated payment, a SEP-IRA top-up, invoices still open). Sample data, written
the way a careful person would keep it. The point of the project is the friction:
see `../FINDINGS.md`, and `wishes.ax.txt` for what this file wished it could say.

```text
axiom.ax          root: base currency, `me` (single, born 1989, lives us)
accounts.ax       kinds, places, clients and vendors as entities, codes, budgets
tax.ax            self-employment tax, SEP/QBI/health adjustments, 1040-ES dates
plans.ax          2026 estimated payments and the next SEP top-up
prices/prices.ax  the IRS mileage rate, as the price of one MI
journal/2025/*    January-December 2025
journal/2026/01   January 2026
wishes.ax.txt     not loaded (the loader only reads *.ax)
outputs/          every command, run with --today 2026-02-10
```

## What it models

- **Five clients, invoices sent then paid late.** Brightwave's monthly retainer
  (paid 30-45 days after each invoice), Fernhill (Stripe, fee netted from the
  deposit), Orbit Labs (one invoice paid in two wires, then a 65-day payer),
  Northpeak (one invoice still open on 2026-01-31), and Delta Rugs, which never
  pays and is written off on 2025-12-15. Each client is an entity whose `via`
  place is its own receivable account; `#inv-2025-xxx` links invoice, payments
  and write-off.
- **Cash-method tax on accrual books.** `income/design` is recognized when
  invoiced (see `flow`), but the tax laws count a receipt only when money leaves
  a receivable (`kind receivable`'s `count-receipts` law). The write-off is an
  expense outside `expenses/business`, so it is not deductible.
- **Business card** paid in full on the 20th for the prior month; annual plans
  (insurance, Dropbox, Squarespace) are spread with `DATE..DATE`.
- **Deductions as laws on places**: subscriptions and equipment (100%), client
  meals (50%), the home office (15% of rent, utilities, internet, renters
  insurance), the phone (60%), miles as `MI` valued at the IRS rate, health
  premiums (an adjustment, not a business expense).
- **Self-employment tax and QBI**, `tax.ax`. Half of SE tax, the SEP-IRA
  deduction (limited to 20% of net earnings after half of SE tax) and health
  premiums come off income; QBI is 20% of what is left, limited to 20% of taxable
  income before it.
- **Four estimated payments** from a tax vault to the IRS on 04-14, 06-23 (a week
  late: due Monday 06-16), 09-15 and 2026-01-14, and a `by` law per due date
  that checks the running total against the safe harbor.

## What the commands should show (hand-verified; today = 2026-02-10)

Independent check in Python (`python3 verify.py` prints these): receipts
74,800.00 USD in 2025 (27 payment legs), deductible expenses 13,742.54 USD,
net profit 61,057.46 USD, SE tax 8,627.14 USD, half 4,313.57 USD, adjusted
income 45,799.89 USD, QBI 6,068.00 USD, taxable income 24,272.01 USD, income tax
2,674.14 USD, total federal tax 11,301.28 USD.

`axiom check`: no errors; 7 warnings: five budget warnings (Dropbox's annual
fee lands in one month and every later software charge that month warns again),
the second installment short on 2025-06-16 (correct), and the fourth installment
short on 2026-01-15 (**wrong**: see finding "tallies are read in the year of the
day").

`axiom tax 2025` prints 2,711.13 USD of income tax and 38.54 USD of
early-withdrawal penalty, **not** the 2,674.14 USD above: Q1's SEP-IRA market
*loss* of 385.35 USD is a flow out of a tax-deferred place, and the SEP kind's
distribution law treats it as a withdrawal. Owed 4,176.81 USD in total, against
4,101.28 USD by hand, and against 1,701.28 USD if the Q4 payment made on
2026-01-14 counted for 2025.

`axiom balance`: net worth 64,421.58 USD; bank 29,380.98 USD (business
14,548.43, personal 4,411.60, tax vault 10,420.95), receivables 5,800.00 USD
(Brightwave 3,200, Northpeak 2,600), SEP-IRA 29,353.79 USD, card 113.19 USD owed.
Every month-end bank, card and quarterly SEP statement reconciles.

`axiom available`: 29,380.98 USD liquid because the receivable kind has
`liquidity 45d`; without that line the two open invoices count as spendable cash
(35,180.98 USD), and 10,420.95 USD of the liquid total is the tax vault with
4,101.28 USD of tax already known to be owed.

`axiom lots`: the SEP-IRA shows only 21,353.79 USD at zero basis; the 8,000 USD
contributed from checking has face-value basis, so a withdrawal would be taxed
on too little.
