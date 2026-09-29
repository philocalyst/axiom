# 04 - A freelance designer

A solo brand designer's books for tax year 2025, carried on to 2026-04-16, the
day after the return for 2025 closes. Sample data, written the way a careful
person keeps it. The project shows what a sole proprietor needs that `us` does
not ship, and how little of it there is once the language can say who owes what,
which year a payment is for, and what is set aside.

```text
axiom.ax          root: base currency, `me` (single, born 1989, lives us)
accounts.ax       kinds, places, clients and vendors as entities, the vault, one code rule
tax.ax            profit, self-employment tax, QBI, and the 1040-ES checks
plans.ax          the 2026 estimated payments
prices/prices.ax  the IRS mileage rate, as the price of one MI
journal/2025/*    January-December 2025
journal/2026/*    January to April 14, 2026
outputs/          every command, run with --today 2026-04-16
```

## What it shows

- **Invoices are claims.** `design -> brightwave 3_200 USD #inv-2025-b01 due 30d`
  puts 3,200 USD into `assets/receivable` (a `receivable` kind from `std`), owed
  by Brightwave within 30 days. `brightwave -> business-checking 3_200 USD for
  #inv-2025-b01` settles it. Orbit Labs' 5,500 USD invoice is paid in two halves
  by wire, each `for #inv-2025-o1`; Stripe's fee comes off a payment as a leg.
  Delta Rugs never pays, and the claim ends in a write-off to `bad-debt`.
  `axiom claims` ages what is open, and `check` warns on what is past due.
- **Cash method on claims.** The receivable account's one law counts a receipt
  when money *leaves* the claim, so an invoice is income when paid, and a
  write-off is not a receipt.
- **Estimated tax `for 2025`, paid in January.** The fourth installment leaves the
  vault on 2026-01-14 with `for 2025`, and so does 1,500 USD paid on 2026-04-10.
  The return closes on 2026-04-15, and what the journal recognizes for 2025 by
  then counts: 11,100 USD, not the 9,600 USD paid inside the year.
- **Slots, not hacks.** `tax.ax` is four laws. Profit counts into `total-income`;
  self-employment tax counts into `other-taxes` and, halved, into `adjustments`;
  the qualified business income deduction counts into `deductions`; health
  premiums and SEP-IRA deposits (the kind's own law) count into `adjustments`.
  `us` reads them when the return closes, whatever the files are called.
- **The vault is an envelope.** A fifth of every payment goes to `tax-vault` with
  `for vault`, an entity of kind `envelope` (`std`): the money is tied to it, so
  `available` subtracts it, and only estimated tax comes out.
- **Prepaid and annual costs land in the year they cover.** Adobe's annual plan,
  bought 2025-12-19, is `for 2026`. Insurance, Dropbox and the site are
  `DATE..DATE` spreads. Budgets, `flow`, and the return read the recognition, so
  the software budget sees a twelfth of Dropbox each month instead of a false
  overspend in February.
- **Market moves are revaluations.** The SEP-IRA's quarterly statement is an
  assertion `via market`: the gap is a loss or a gain, never a withdrawal, so the
  10% early-withdrawal penalty does not fire.
- **A late installment, checked.** Three `by` laws test 2025's first three
  installments; the fourth, one law for every year, closes on January 15 and reads
  the year that just ended.
- **Openings are statements.** `opening 2025-01-01` in `journal/2025/01.ax`.

## What the commands should show (hand-verified; today = 2026-04-16)

`python3 ../verify/verify04.py` reads the journal text, without Axiom, and works
the 2025 return from the invoices and expenses. Each figure below was computed
that way and then compared with the output.

| line of the return | by hand | `axiom tax 2025` |
|---|---|---|
| gross receipts, 20 payments (27 with the fee legs) | 74,800.00 | 74,800.00 |
| business expenses | 13,471.01 | 13,471.01 |
| net profit | 61,328.99 | 61,328.99 |
| self-employment tax (92.35% of net; 12.4% to the wage base, 2.9%) | 8,665.51 | 8,665.51 |
| its deductible half | 4,332.76 | 4,332.76 |
| adjustments: SEP 8,000.00 + health 4,944.00 + half SE tax | 17,276.76 | 17,276.76 |
| total income (net profit and 290.12 interest) | 61,619.11 | 61,619.11 |
| AGI | 44,342.35 | 44,342.35 |
| QBI deduction: 20% of 28,592.35, the smaller of attributable income 44,052.23 and taxable income before it | 5,718.47 | in `deductions` |
| deductions (standard 15,750.00 + QBI) | 21,468.47 | 21,468.47 |
| taxable income | 22,873.88 | 22,873.88 |
| income tax (each bracket rounded to the cent) | 2,506.37 | 2,506.37 |
| total tax (income tax + SE tax) | 11,171.88 | 11,171.88 |
| payments for 2025 (9,600.00 + 1,500.00) | 11,100.00 | 11,100.00 |
| owed on 2026-04-15 | 71.88 | 71.88 |

Run with `--today 2026-02-10` instead (`outputs/tax-2025-before-closing.txt`), the
return has not closed: `axiom tax` shows what was counted and the payments so
far, and says the return closes on 2026-04-15 instead of showing a tax, because the
laws that figure it wait for that day.

`axiom check`: no errors, three warnings, each one taught: Northpeak's 2,600.00 USD
invoice is 106 days past due, Orbit Labs' 6,000.00 USD invoice is 7 days past
due, and the second 2025 installment was short on 2025-06-16 (2,400.00 USD paid
of 4,600.00 USD, correct: it was paid a week late). The fourth installment is not
short: the payment `for 2025` on 2026-01-14 counts on January 15. Every month-end
bank, card and quarterly SEP statement reconciles; the generator that wrote the
journal kept a second ledger to compute them. The summary reads 470 flows, and net
worth is 70,222.47 USD.

`axiom limits 2025`: the SEP-IRA takes 20% of net profit less half of SE tax:
`(61,328.99 - 4,332.76) x 20% = 11,399.25 USD`, of which 8,000.00 USD was used, room
3,399.25 USD (the 2,000.00 USD paid on 2026-01-28 `for 2025` counts). The four
installments show as floors: 2,400.00 of 2,300.00, 2,400.00 of 4,600.00 (short),
7,200.00 of 6,900.00, 9,600.00 of 9,200.00.

`axiom claims`: 11,800.00 USD is owed to me: 2,600.00 (Northpeak, overdue), 6,000.00
(Orbit Labs, overdue) and 3,200.00 (Brightwave's April retainer, due in 15 days).
`--at 2025-09-30` shows the same book in September, with Delta Rugs 90 days late.

`axiom available`: 28,969.11 USD in hand, of which 8,856.11 USD is the vault, held
for `vault`, so 20,113.00 USD (business checking 15,324.46 + personal 4,788.54)
is available; the 11,800.00 USD of claims is "coming in", never to spend; the
SEP-IRA (29,586.06 USD) is reached only by drawing it down.

`axiom balance`: assets 70,355.17 USD (bank 28,969.11, receivables 11,800.00, SEP-IRA
29,586.06), the card 132.70 USD owed. `axiom lots` lists the whole SEP-IRA at zero
basis, 21,586.06 USD of parcels plus 8,000.00 USD of plain money that came from
checking: the kind (`basis zero`) decides what arrives, not the route it took.

## Findings this project now demonstrates as fixed

F01 (a SEP deposit from checking has zero basis, as its kind says), F03 (market
losses are revaluations, no penalty), F06 (invoices are claims: aging, partial
payment, write-off, coming in), F07 (payments `for` a year, a return that closes
on April 15), F08 (no file order: `tax.ax` may be called anything), F10 (claims are
coming in, the vault is held), F16 (a self-imposed restriction, the envelope), F17
(the opening is a statement), F18 (prepaid and annual costs recognized over the
period they cover), F19 (slots for adjustments, deductions, other taxes; a refund
would be negative, no forked `us`), F25 (a payment with a fee leg, entities
declared several at a time), F26 (a yearly `budget 2025` view; one warning per
window).

## Still open

- **One law per due date.** `by` names one day and `closing` fires once a year, so
  Q1-Q3 of each year is one law each. 2025's are written; 2026's are not, and
  nothing warns that they are missing (F27).
- **Plans drop `for`.** A plan's `for 2026` is not carried into the forecast, so
  the January installment is planned in December (`plans.ax`).
- **Cash or accrual is not a property of the person.** The cash method is a law on
  the receivable account.
