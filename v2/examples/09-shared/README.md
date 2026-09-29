# 09 - A shared life

Alex, 28, bartender, shares a flat with two roommates, lent a friend money, keeps
cash tips in a wallet, is waiting for a work expense to be paid back, and is
treasurer of a garden collective that holds a restricted grant. March to December
2025. Sample data; every bank, card, cash and IOU statement reconciles to the cent
(an independent Python model splits the bills and keeps the IOUs). The point is the
friction: see `../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax          root: me (single, born 1996, lives us), green-table (an org that lives nowhere)
accounts.ax       kinds (iou, personal-loan, garden, reimbursement), accounts, entities, codes
journal/2025/*    March-December 2025
wishes.ax.txt     not loaded (the loader only reads *.ax)
outputs/          every command, run with --today 2026-01-06
```

## What it models

- **Roommates.** Rent is 3,150.00 USD, paid by Alex; each third is either an expense
  (mine) or what Ben and Cleo owe me (`assets/roommates/ben`, one place per
  person, netted). One transaction with three legs writes it. Ben pays the
  electricity: my third is what I owe him, so it reduces his balance and can push
  it below zero (an asset that goes negative is a debt). A payment `ben -> checking`
  reduces what he owes. Ben pays half of June and catches up in July, Cleo is two
  weeks late in August and 50.00 USD short in November; the month-end assertions
  are the numbers the split app shows.
- **A loan to a friend**: 2,000.00 USD on 2025-04-10, 250.00 USD a month expected. The
  `personal-loan` kind has a monthly `warn` law that fires when less than `expected`
  has come in while something is still owed: it fires in September and October
  (nothing paid), and once in April, the month of the loan (a false alarm: the law
  cannot say "starting next month"). Riley pays 500.00 USD in November, and 200.00
  USD is forgiven at Christmas as a gift.
- **Cash tips**: counted every Sunday into `income/tips` (kind `wages`, so the tax
  law counts them), paid into the bank monthly, and one `wallet = X !` that
  accepts a 23.00 USD gap, visibly (`equity/unknown`).
- **A reimbursable expense**: 138.50 USD for an apron and a tablet stand, on the
  card on 2025-06-02, with a pending inflow `lantern-reimbursement -> checking
  (138.50 USD) #reimb-2025-06` settled on 2025-07-11. The source is a place of kind
  `reimbursement`, not `wages`, so it is not income for tax.
- **The collective.** `owner green-table` puts the bank account, cash box, donations,
  grant and garden expenses in the collective's name, in Alex's ledger. The
  Riverfront Foundation's 6,000.00 USD (`entity riverfront : grant`, `purpose
  garden`, `until 2025-10-31`) lands tied to the foundation; six garden purchases
  and a final 250.00 USD of compost in October spend it; pizza for volunteers
  (not a garden supply) is paid from the untied donations, because relief goes to
  untied money first; and on 2025-10-28 the 57.25 USD that is left is returned to
  the foundation. The `deadline` law finds nothing left on 2025-10-31.

## What the commands should show (today = 2026-01-06)

`axiom check`: no errors. Three warnings, all from the loan (2025-04-30, 09-30,
10-31), and the note for the 23.00 USD wallet gap. Net worth is the sum of Alex's
and the collective's accounts (see below).

`axiom tax 2025`: wages 40,192.89 USD = 31,082.42 (pay and card tips) + 9,110.47 (cash tips);
taxable income 24,442.89 USD; income tax 2,694.65 USD (10% to 11,925, 12% above);
withheld 3,854.21 USD, so nothing owed (a refund of 1,159.56 USD is not booked). The
reimbursement, the grant, the donations and the loan are not income.

`axiom balance assets/roommates/*`: Ben owes 37.21 USD, Cleo 216.34 USD (what the
November and December bills left unsettled, including the 50.00 USD she was short on
November's rent). `axiom register
roommates/ben` is the statement.

Three things to distrust:

- **`axiom available` counts the collective's money and the loan and the IOUs as
  mine**: 23,718.59 USD "liquid" includes the collective's account (2,654.61 USD)
  and cash box (109.00 USD), and on 2025-05-01 it included the 2,000.00 USD lent to Riley
  and 2,100.00 USD owed by the roommates. Only the grant's 3,158.65 USD is
  subtracted, as "tied".
- **`axiom flow` and `axiom balance` have no owner filter**: 5,942.75 USD of grant
  income and 6,220.94 USD of garden spending are in Alex's income and expenses.
- **`axiom forecast` invents a deficit**: it sees Alex pay 3,150.00 USD of rent
  every month but not the roommates' payments back (irregular dates and amounts),
  and warns that checking is overdrawn by 7,062.57 USD on 2026-08-03. It also keeps
  projecting Riley's 250.00 USD a month past the 50.00 USD that is left ("assets/loans/riley
  is overdrawn, down to -2,950.00 USD").
