# A household

One person's money for the first quarter of 2026: a salary with a 401(k), rent,
a credit card, a brokerage account, a 529 plan, a scholarship, and the small
mess that real books have. Everything is sample data.

The person is a 37-year-old designer, single, renting in San Francisco. The
`lives us/ca/san-francisco` line in `axiom.ax` is what brings the federal,
California and San Francisco laws into force.

```text
axiom.ax                the root: base currency, the systems in use, me
accounts.ax             commodities, accounts, people and companies, a code rule
journal/2025/12.ax      opening balances, dated 2025-12-31 (an `opening` block: statements, not flows)
journal/2026/01.ax      January
journal/2026/02.ax      February
journal/2026/03.ax      March
prices/2026.ax          market prices, nothing else (the folder says so)
plans.ax                what has not happened yet, for the forecast
```

The folders are constraints: `journal/2026/02.ax` may hold February 2026 and
nothing else, and `prices/` may hold only prices. Add `layout free` to
`axiom.ax` to switch that off.

## What each part shows

| Feature | Where | What to look for |
|---|---|---|
| Paycheck split | `01.ax`, `02.ax`, `03.ax` | One source, four targets, and `...` for the remainder, which lands in checking. The 800 USD `retirement` leg is a pre-tax 401(k) deferral. |
| Rent through an entity | every month | `checking -> landlord` names no rent account. `landlord` stands for its `via` place, `expenses/rent`, and is the payee. |
| Credit card | `01.ax`, `02.ax` | Spending goes onto `visa`, and a later payment moves money from checking. `visa = empty` proves each statement was paid in full. |
| Budget | `accounts.ax` | `budget 650 USD monthly` on `expenses/food` covers `groceries` and `dining`. February goes over. |
| Pending check | `02.ax` | `(350 USD) #check-1041` counts against what can be spent on 2026-02-05, but not against the bank's balance, until `#check-1041 settled` on 2026-02-12. The 02-08 assertion is the bank's number. |
| Brokerage, FIFO | `01.ax`, `02.ax`, `03.ax` | Two lots at different costs, then a sale of 8 shares. A `broker` account sells the oldest first, so no lot has to be named. |
| Inferred ATM amount | `02.ax` | `checking -> cash ? USD` between two checking assertions. The one unknown must be 100 USD. |
| 401(k) | `accounts.ax` | Kind `401k`: capped each year, and counted as income when it comes out. By the March paycheck 2,400 USD of the 24,500 USD limit is used. |
| 529 plan | `02.ax`, `03.ax` | A qualified withdrawal straight into `tuition`, then one that is not: the earnings in it are income, and owe 10%. |
| Grant | `accounts.ax`, `02.ax` | The scholarship's 1,200 USD lands in checking but stays tied to it. The tuition payment spends it, which is what it is for. |
| `!` pad | `03.ax` | `cash = 63.00 USD !` accepts a 14.50 USD gap and books it as unexplained, visibly. |
| Spread premium | `01.ax` | `2026-01-01..2026-12-31` pays 600 USD once and recognizes it day by day. |
| Plans | `plans.ax` | A trip, fall tuition, a bonus and a renewal, for the forecast. |

## What the commands should show

These follow from the files by hand. The engine may round a cent differently.

### `axiom check`

No errors. Three things are reported, none of them a failure:

- **A budget warning** on 2026-02-25. Food this month is 681.00 USD against a
  budget of 650.00 USD, over by 31.00 USD. It crosses the line on the last
  grocery run.
- **A priced violation** on 2026-03-25, the 529 withdrawal to checking. It did
  not pay for school, so `nonqualified-penalty` resolves it to a loss: about
  34.94 USD owed to the IRS, which is 10% of 349.37 USD in earnings. The same
  earnings count as income.
- **A waived gap** on 2026-03-31. The wallet should hold 77.50 USD and holds
  63.00 USD. The 14.50 USD difference is booked from `unknown`, and reported
  because it was waived and never hidden.

The 529 withdrawal into `tuition` on 2026-02-20 reports nothing: it is
qualified, and the 290.11 USD of earnings in it are not taxed. All 21
assertions reconcile, and no account overdraws. The summary line reads
about `67 flows`, and net worth is about 77,597.73 USD.

### `axiom balance`

At 2026-03-31, the assets are:

```text
assets
  bank/checking     10,332.70 USD
  bank/savings      16,647.05 USD
  brokerage          2 VTI, 1.88 USD
  cash                  63.00 USD
  college            6,700.00 USD
  retirement        43,700.00 USD
liabilities
  visa                 452.00 USD
```

`axiom balance --value` shows the two shares at 302.55 USD each, 605.10 USD.
Income in the quarter is 24,000.00 USD from `salary`, 147.05 USD of interest,
1.88 USD of dividends and the scholarship's 1,200.00 USD. `expenses/insurance`
shows about 148 USD, the 90 days of the year that have passed, not the 600 USD
that was paid.

`axiom lots brokerage` shows one lot left: 2 VTI acquired 2026-02-18, basis
582.70 USD, unrealized gain 22.40 USD.

### `axiom tax 2026`

The tallies for the year so far, under `us`:

```text
wages               24,000.00 USD
total-income        22,098.30 USD    the sum of the lines below it, less what was deferred
pretax               2,400.00 USD    the 401(k) deferrals, taken back out of income
payments             2,640.00 USD    withheld from pay, credited against the year's tax
interest               147.05 USD
short-term-gains       141.55 USD    the March sale: 128.80 from January's lot, 12.75 from February's
distributions          349.37 USD    the earnings in the non-qualified 529 withdrawal
dividends                1.88 USD
```

The short-term gains join `total-income` at the year's end, when `agi` is
figured: 22,098.30 + 141.55 = 22,239.85 USD. California's `ca-withheld` is
1,110.00 USD. The 529 penalty of about 34.94 USD is owed as
`nonqualified-529-penalty`. The scholarship does not appear in the tallies: no
law counts it, because scholarship money spent on tuition is not taxable income
(IRC §117).

The `federal-income-tax` and `ca-income-tax` obligations are worked out when
the year ends, and `axiom forecast` projects them. On these figures, with the
plans, the year comes to 11,209.23 USD of federal tax and 4,379.11 USD of
California tax. More was withheld in both cases (11,440 USD and 4,849 USD), so
the forecast lists two refunds due 2027-04-15: -230.77 USD federal and -469.89
USD California. A refund is an obligation with a negative amount.

### `axiom forecast`

It runs the journal forward with three kinds of input:

- what repeats in the journal, found by cadence and amount: the rent on the
  1st, the paycheck and its withholding, the 529 and savings transfers, and
  the utility bill;
- the plans in `plans.ax`: the July trip, fall tuition in August and the 529
  payment beside it, the year-end bonus, and the renewal on 2027-01-01;
- what the laws say is owed.

The same laws run on the projection, so the 401(k) deferral limit is checked
against the projected paychecks: 800 USD a month and 400 USD from the bonus
comes to 10,000 USD for the year, well inside 24,500 USD.

Try changing all three `retirement 800 USD` legs to `2_400 USD` and run the
forecast again. The projected deferrals should pass the 24,500 USD limit with
the November paycheck, and the forecast should say so before it happens.
