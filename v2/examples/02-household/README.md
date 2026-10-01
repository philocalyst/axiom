# A household

One person's finances for the first quarter of 2026: wages and a 401(k), rent,
a card, a brokerage account, a 529 plan, and a restricted scholarship. The sample
person is a 37-year-old designer who rents in San Francisco.

This example keeps the original hand-checked figures while its v4 model and
report support is being integrated. Those figures are targets, not captured
output from the new toolchain yet.

| File | Contents |
|---|---|
| `axiom.ax` | Base currency, systems, and the owner with a San Francisco residence. |
| `accounts.ax` | Commodities, accounts, purposes, parties, budget, and check-code rule. |
| `contracts.ax` | Rent, insurance, variable utilities, payroll, tuition, trip, and bonus terms. |
| `journal/2025/12.ax` | Opening positions and basis at 2025-12-31. |
| `journal/2026/01.ax`–`03.ax` | Transactions and statements for the quarter. |
| `prices/2026.ax` | Dated VTI prices in USD. |

## What the source demonstrates

- **Purpose inference:** transactions with grocers, a utility, and transit use
  party purposes. Explicit `#dining`, `#tuition`, and `#transfer` labels resolve
  cases where inference does not know the purpose.
- **Contracts:** rent, the annual renter policy, variable utility bills,
  paychecks, tuition, a trip, and the bonus have reusable terms in one file.
  The paycheck's remaining amount goes to checking after its deferral and
  withholding legs.
- **Account history:** VTI purchases establish lots; the March sale consumes the
  oldest shares first. The account's prices provide a quarter-end value.
- **Claims and pending payments:** `^check-1041` marks a check that reduces
  availability before it leaves the bank balance, then settles on February 12.
- **Tax-favored accounts:** the 401(k) takes pre-tax deferrals from wages. The
  529 withdrawal paid directly to the school is qualified; the March withdrawal
  to checking is not.
- **Restricted money:** the scholarship is tied to education after it reaches
  checking and is spent on tuition.
- **Assertions and a pad:** statements reconcile the bank balances; the March
  wallet statement explicitly accepts a 14.50 USD difference.

## Original hand-checked targets

At 2026-03-31, the expected positions are 10,332.70 USD in checking, 16,647.05
USD in savings, 43,700.00 USD in the 401(k), 6,700.00 USD in the 529, 63.00 USD
in cash, two VTI shares plus 1.88 USD in the brokerage, and 452.00 USD owed on
the card. At the dated price of 302.55 USD per share, net worth is 77,597.73 USD.

The quarter includes 24,000.00 USD of wages, 147.05 USD of interest, and 1.88
USD of dividends. The employee defers 2,400.00 USD to the 401(k). The March VTI
sale realizes 141.55 USD of short-term gain. The nonqualified 529 withdrawal
has 349.37 USD of earnings and a 34.94 USD additional-tax target. The annual
renter policy recognizes about 148 USD through March, rather than the full 600
USD paid.

The April 15, 2027 return-closing targets from the original example are 11,209.23
USD federal tax and 4,379.11 USD California tax, against payments of 11,440 USD
and 4,849 USD. That implies refunds of 230.77 USD and 469.89 USD. These figures
need an end-to-end v4 report comparison before they can be treated as verified.

## Running after integration

From `v2/`, run `axiom check examples/02-household`, then inspect the balance,
tax, lot, and forecast views. Re-run the report checks after changing a journal
amount or a contract term. The source no longer uses a chart of income and
expense accounts: parties identify who the flow is with, purposes identify
what it is for, and contracts describe recurring commitments.
