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
accounts.ax       kinds (friend, garden), accounts, entities, codes
journal/2025/*    March-December 2025
journal/2026/01   January: the roommates pay December's bills
outputs/          every command, run with --today 2026-04-16 unless it says otherwise
```

## What it models

- **People owe Alex as claims.** One `receivable` place per person (`assets/owed/by-ben`, `by-cleo`,
  `by-riley`), and each claim is its own parcel, kept apart by the flow that made it. Rent is 3,150.00 USD
  paid by Alex: one flow with three legs, `#rent-2025-03 due 2025-03-08`, Alex's third an expense
  and Ben's and Cleo's thirds claims on them. A payment is `for` the code, `ben -> checking 1_050 USD for
  #rent-2025-03`, and settles that claim, in part or in full: Ben pays half of June and the rest on July 8,
  Cleo is eleven days late in August and 50.00 USD short in November. The 50.00 USD stays open and
  overdue, and `check` says so.
- **The small shared bills are one code a month.** Internet, and groceries on the card, are split three
  ways with `#shared-2025-03` on each; at month end Ben and Cleo each pay with one flow `for` that
  code, which settles the month's claims oldest first.
- **What Alex owes is a claim too.** Ben's name is on the electricity: `owed-to-ben -> utilities 29.47
  USD #power-2025-03 due ...` is a payable, and Alex pays it `for` its code. Cleo's Costco runs are the
  same. Nothing nets: money moves each way, and `claims` lists both directions.
- **A loan to a friend**: 2,000.00 USD on 2025-04-10, `#loan-riley due 2025-12-15` (250.00 USD a month from
  May 15, so the last payment falls due then). Riley pays 250.00 USD in May, June, July and August, 500.00 USD
  in November (two months missed) and 250.00 USD in December, each `for #loan-riley`, and 200.00 USD is
  forgiven at Christmas as a gift: `riley -> gifts 200 USD for #loan-riley` settles that part of the
  claim without money moving. 50.00 USD is left, overdue.
- **A weekend away, split four ways.** The cabin, gas and food, 1,182.80 USD on the card, one flow with legs:
  Alex's 295.70 USD share is an expense, Ben, Cleo and Riley each owe 295.70 USD by September 15
  (`#trip-2025-08`). Riley also owes the loan, and a payment `for #trip-2025-08` settles the trip and
  not the loan. Cleo pays 200.00 USD on September 5 and 95.70 USD on October 3.
- **A work expense.** 138.50 USD for an apron and a tablet stand, on the card on 2025-06-02, is not Alex's
  expense: it is `visa -> lantern-owes 138.50 USD #expense-2025-06 due 2025-07-15`, a claim on the employer. When
  the Lantern pays on July 11 (`for #expense-2025-06`) it settles the claim: it is not wages, and not income.
- **Cash tips**: counted every Sunday into `income/tips` (kind `wages`, so the tax law counts them), paid
  into the bank monthly, and one `wallet = X !` that accepts a 23.00 USD gap, visibly (`equity/unknown`).
- **The collective.** `owner green-table` puts the bank account, cash box, donations, grant and garden
  expenses in the collective's name, in Alex's ledger. The Riverfront Foundation's 6,000.00 USD
  (`entity riverfront : grant`, `purpose garden`, `until 2025-10-31`) lands tied to the foundation; seven
  garden purchases from the grant (5,942.75 USD) spend it; pizza for volunteers (not a garden
  supply) and the summer's water are paid from the untied donations, because relief goes to untied money
  first; and on 2025-10-28 the 57.25 USD that is left is returned to the foundation. The `deadline` law finds
  nothing left on 2025-10-31.

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
claims (Cleo's 50.00 USD and Riley's 50.00 USD), and the note for the 23.00 USD wallet gap. 496 flows, 40 places,
11 laws, net worth 22,917.53 USD.

`axiom claims --today 2025-09-20` shows the trip, the month's small bills and the loan owed to Alex, and September's
electricity owed by Alex, each with its age and due day; `outputs/claims.txt` shows the two that are still open
today. `axiom available --today 2025-05-01`: 7,657.33 USD to spend (money in hand 10,815.98 USD less 3,158.65
USD held for the foundation), and 4,100.00 USD *coming in*, listed below and not counted.

## Findings this project now demonstrates as fixed

F06 (a roommate's third, a loan, a reimbursement, a trip share and a bill in someone else's name are claims:
each is its own parcel with an age and a due day, `claims` lists both directions, `check` warns on the overdue
ones, and a payment says which it settles with `for #code`), F10 (`available` no longer counts the 2,000.00 USD lent
and the 2,100.00 USD the roommates owed as money to spend: they are *coming in*), F15 in part (`!` accepts the wallet
gap and is reported), F27 in part (Riley's loan needs no monthly law that fires in April, the month it is made: the
claim has a due day and `check` reports it when it is missed).

## Still open

- **No owner scope in the reports.** `available` counts the collective's bank account and cash box
  (2,763.61 USD) as Alex's to spend, and `flow` puts the grant's 5,942.75 USD and 6,220.94 USD of garden spending
  in Alex's income and expenses. Only the grant's unspent money is held back.
- **The forecast still projects Riley's 250.00 USD a month** after 50.00 USD is left: a recurrence learned from
  history knows nothing about the claim's balance, and adds 3,000.00 USD to the year's committed money.
- **A claim made by a leg does not name its debtor.** `axiom claims` shows `assets/owed/by-cleo`, not Cleo, for a
  leg of a split: the counterparty is the header's payee or the place. A header payee (`/ landlord`) would name
  the landlord as the debtor of all three claims, so the shared flows have no payee on the header and the landlord
  is on Alex's leg. `due` can only be on the header.
- **A schedule is one due day.** The loan is repaid 250.00 USD a month, but the claim has one due date, the last
  payment's; nothing says that May's payment was late.
- **A place that ends in an entity's name is an ambiguity, and the entity wins.** `assets/owed/lantern` next to
  `entity lantern` (payroll) is an `ambiguous-name` error, and every `lantern -> ...` means the entity, so the
  receivable places are `by-ben`, `by-cleo`, `by-lantern`.
- **Thirds are hand-rounded.** 69.99 USD is 23.33 + 23.33 + 23.33, and any extra cent (73.54 USD is 24.52 + 24.51 + 24.51) is on Alex's
  leg; the language does no splitting.
- **`axiom balance --at DATE` panics before an accepted gap.** The wallet's `!` on 2025-08-31 is a pad, and a balance at
  an earlier day stops in `crates/report/src/history.rs:220` (index out of bounds), so there is no
  mid-year balance output here.
