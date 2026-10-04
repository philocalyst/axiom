# 09 - A shared life

Alex, 28, bartender, shares a flat with two roommates, lent a friend money, keeps
cash tips in a wallet, is waiting for a work expense to be paid back, went away for a
weekend with three friends, and is treasurer of a garden collective that holds a
restricted grant. March 2025 to January 2026. Sample data; every bank, card, cash and
claim statement reconciles to the cent (an independent Python model keeps every claim),
and `python3 ../verify/verify09.py` works the claims, the return, the grant and the
net worth from the journal text, without Axiom.

```text
axiom.ax          root: me (single, born 1996, lives us), green-table (an org that lives nowhere)
accounts.ax       the friend kind, accounts, purposes (the garden's among them), the grant, payees, codes
journal/2025/*    March-December 2025
journal/2026/01   January: the roommates pay December's bills
outputs/          every command, run with --today 2026-04-16 unless it says otherwise
```

## What it models

- **People owe Alex as claims.** A claim is a tab on the person: `ben owes me 1_050.00 USD #rent due 2025-03-08
  ^rent-2025-03` moves no money, and `axiom claims` lists it with its age and due day. Rent is 3,150.00 USD paid
  by Alex to the landlord (`checking -> landlord 3_150.00 USD #rent`), and under it Ben's and Cleo's thirds are two
  claims, so Alex's third is the rent and the others are claims on them. A payment carries the claim's code, `checking <- ben
  1_050.00 USD ^rent-2025-03`, and settles it, in part or in full: Ben pays half of June and the rest on July 8,
  Cleo is eleven days late in August and 50.00 USD short in November. The 50.00 USD stays open and overdue, and
  `check` says so. A claim with a purpose (`#rent`) counts as that purpose when it is paid, so what a roommate pays
  takes it off Alex's rent.
- **The small shared bills are one code a month.** Internet, and groceries on the card, are split three ways
  with `^shared-2025-03` on each claim; at month end Ben and Cleo each pay with one flow that carries that
  code, which settles the month's claims oldest first.
- **What Alex owes is a claim too.** Ben's name is on the electricity: `me owes ben 29.47 USD #utilities due
  2025-04-11 ^power-2025-03` is a bill of his, and Alex pays it with its code (`checking -> ben 29.47 USD
  ^power-2025-03`); the electricity is spending on the day it is paid. Cleo's Costco runs are the same. Nothing nets:
  money moves each way, and `claims` lists both directions.
- **A loan to a friend**: 2,000.00 USD on 2025-04-10, `checking -> riley 2_000 USD #loan` (a transfer, not spending) and
  `riley owes me 2_000 USD due 2025-12-15 ^loan-riley` (250.00 USD a month from May 15, so the last payment falls due
  then). Riley pays 250.00 USD in May, June, July and August, 500.00 USD in November (two months missed) and 250.00 USD
  in December, each with `^loan-riley`. At Christmas 200.00 USD is forgiven as a gift: Riley's 200.00 USD settles that
  part of the claim and Alex gives it straight back, `checking -> riley 200 USD #gifts`, so no money moves in the end.
  50.00 USD is left, overdue. (A claim can be written off only whole, `^loan-riley waived`, not in part.)
- **A weekend away, split four ways.** The cabin, gas and food, 1,182.80 USD on the card, one flow and three claims:
  Alex's 295.70 USD share is the trip, Ben, Cleo and Riley each owe 295.70 USD by September 15
  (`^trip-2025-08`). Riley also owes the loan, and a payment carrying `^trip-2025-08` settles the trip and
  not the loan. Cleo pays 200.00 USD on September 5 and 95.70 USD on October 3.
- **Claims are on people, not on places.** The first version of the book gave each person a `receivable` account
  (`by-ben`, `by-cleo`) and a statement of it at every month end; a claim is a tab on its party now, a tab has no name
  a statement can be written about, and the book's reconciliation of claims is `axiom claims`, the README table
  below, and `verify09.py`.
- **A work expense.** 138.50 USD for an apron and a tablet stand, on the card on 2025-06-02, is not Alex's
  expense: `visa -> ? 138.50 USD #job-supplies` and `lantern owes me 138.50 USD #job-supplies due 2025-07-15
  ^expense-2025-06`, a claim on the employer that takes the spending back. When the Lantern pays on July 11
  (`#reimbursement ^expense-2025-06`) it settles the claim: it is not wages (the employer's payments are wages unless
  the line says what this one is for), and not income.
- **Cash tips**: counted every Sunday, `wallet <- ? 163.14 USD #tips` (`purpose tips : wages`, so the tax law counts
  them), paid into the bank monthly, and one `wallet = X !` that accepts a 23.00 USD gap, visibly (booked from `unknown`).
- **A pay stub is the gross wages and what comes out of it**: `checking <- lantern 1_440.25 USD #wages`, then the
  federal withholding to the IRS and the payroll tax to the SSA (`entity ssa #payroll-tax`), so the wage law counts the
  1,440.25 USD and the withholding is what the return pays.
- **The collective.** `owner green-table` puts the bank account and cash box in the collective's name, in Alex's ledger.
  The Riverfront Foundation's 6,000.00 USD (`entity riverfront : grant`, `grant-purpose garden`, `until 2025-10-31`)
  lands tied to the foundation; the garden is a purpose with a child for each kind of supply (`#soil`, `#seeds`,
  `#lumber`, `#irrigation`, `#tools`, `#fencing`, `#water`), each supplier carries its own
  (`entity bayview-soil #soil`), and seven garden purchases from the grant (5,942.75 USD) spend it; pizza for
  volunteers (`#volunteer-food`, not a garden supply) and the summer's water are paid from the untied donations,
  because relief goes to untied money first; and on 2025-10-28 the 57.25 USD that is left is returned to the foundation.
  The `deadline` law finds nothing left on 2025-10-31.

## What the commands should show (hand-verified)

`python3 ../verify/verify09.py` replays every flow and every claim and prints the figures below. Each
agrees with Axiom.

| line | by hand | Axiom |
|---|---|---|
| claims open on 2025-05-01: May's rent, Ben 1,050.00 and Cleo 1,050.00 (due 05-08), Riley's loan 2,000.00 | 4,100.00 | 4,100.00 |
| claims open on 2026-04-16: Cleo, November's rent, 50.00 (overdue 159 days); Riley, the loan, 50.00 (overdue 122 days) | 100.00 | 100.00 |
| owed by Ben, Cleo, Riley and the Lantern on 2026-04-16 | 0.00, 50.00, 50.00, 0.00 | 0.00, 50.00, 50.00, 0.00 |
| wages: pay and card tips 31,082.42 + cash tips 9,110.47 | 40,192.89 | 40,192.89 |
| taxable income (15,750.00 standard deduction) | 24,442.89 | 24,442.89 |
| income tax (10% to 11,925.00, 12% above) | 2,694.65 | 2,694.65 |
| federal withholding | 3,854.21 | 3,854.21 |
| **federal, owed (negative = refund)** | -1,159.56 | -1,159.56 |
| the grant: spent 5,942.75, returned 57.25, left | 0.00 | 0.00 |
| the collective's bank and cash box | 2,763.61 | 2,763.61 |
| net worth on 2026-04-16 (the wallet's 23.00 gap accepted) | 22,917.53 | 22,917.53 |

The return closes on 2026-04-15 with a **refund of 1,159.56 USD**; the reimbursement, the grant, the donations,
the loan and the roommates' claims are not income. `axiom check`: no errors, two warnings, both overdue
claims (Cleo's 50.00 USD and Riley's 50.00 USD), and the note for the 23.00 USD wallet gap. 499 flows, 45 places,
11 laws enforced, net worth 22,917.53 USD.

`axiom claims --today 2025-09-20` shows the trip, the month's small bills and the loan owed to Alex, and September's
electricity owed by Alex, each with its age and due day; `outputs/claims.txt` shows the two that are still open
today. `axiom available --today 2025-05-01`: 7,657.33 USD to spend (money in hand 10,815.98 USD less 3,158.65
USD held for the foundation), and 4,100.00 USD *coming in*, listed below and not counted.

## Findings this project now demonstrates as fixed

F06 (a roommate's third, a loan, a reimbursement, a trip share and a bill in someone else's name are claims:
each is its own parcel with an age and a due day, `claims` lists both directions, `check` warns on the overdue
ones, and a payment says which it settles with its `^code`), F10 (`available` no longer counts the 2,000.00 USD lent
and the 2,100.00 USD the roommates owed as money to spend: they are *coming in*), F15 in part (`!` accepts the wallet
gap and is reported), F27 in part (Riley's loan needs no monthly law that fires in April, the month it is made: the
claim has a due day and `check` reports it when it is missed).

## Still open

- **No owner scope in the reports.** `available` counts the collective's bank account and cash box
  (2,763.61 USD) as Alex's to spend, and `flow` puts the grant's 5,942.75 USD and 6,220.94 USD of garden spending
  in Alex's income and spending. Only the grant's unspent money is held back.
- **The forecast reads recent history only.** On 2026-04-16, three months after the last line, nothing recurs and the
  forecast is flat; on 2026-01-06 it projects Riley's 250.00 USD a month (found 8 times) though 50.00 USD is left: a
  recurrence learned from history knows nothing about the claim's balance.
- **A schedule is one due day.** The loan is repaid 250.00 USD a month, but the claim has one due date, the last
  payment's; nothing says that May's payment was late.
- **A claim is written off whole or not at all.** Forgiving 200.00 USD of Riley's 250.00 USD is written as a payment and a
  gift of the same money; `waived` takes the whole 250.00 USD and refuses a purpose or a recoverable part.
- **No statement for a claim.** A balance can be asserted of an account, an asset, a code or a commodity, not of what
  someone owes: the month-end `by-cleo = 95.70 USD` statements of the first version have nothing to be written about.
- **A claim on the employer counts at the claim.** The Lantern's payments are wages by its kind, so the claim for
  the apron says `#job-supplies` and the payment says `#reimbursement`; left to the party, either would count 138.50 USD
  of wages.
- **Thirds are hand-rounded.** 69.99 USD is 23.33 each, and any extra cent (73.54 USD is 24.52 for Alex and 24.51 for each of
  the others) is Alex's: the claims say what each roommate owes and the rest of the line is Alex's; the language does
  no splitting.
