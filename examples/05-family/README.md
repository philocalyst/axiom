# 05 — A family ledger

Alex and Jordan file jointly with one child. They share a home, mortgage, car,
checking and savings accounts, while keeping separate 401(k) limits. Riley has a
529 plan. The 2025 journal keeps the original household transactions, payroll
figures, quarterly investment values, and month-end account assertions.

## Project layout

```text
axiom.ax          family and household members, filing status, state
accounts.ax       native accounts, assets, counterparties, purposes and limits
contracts.ax      post-2025 mortgage, auto, tax, insurance, tuition and payroll terms
prices/home.ax    dated house and vehicle values
journal/2024/12   opening balances and tax bases at 2024-12-31
journal/2025/*    dated native flows and month-end statements
```

The source uses the native v4 model. A journal movement is a dated flow such as
`checking -> lender 2_019.18 USD #mortgage-interest`; the source and destination,
amount, purpose, and optional code are explicit. Opening values are `opening`
statements, and market changes are dated account statements `via market`.

Some v3 chart-account concepts became typed v4 flows while preserving the source
amounts. A payroll stub is represented by a gross employer-to-checking wage flow,
then separate checking-to-destination flows for deferrals, benefits, withholding,
and payroll taxes. Mortgage principal, interest, and escrow are separate flows.
The 2025 transaction history remains in the journal; `contracts.ax` contains the
future agreements, beginning from independently stated 2025-12-31 balances. This
means the source representation changed even where its financial facts did not.

## Independent arithmetic oracle

Run `python3 ../verify/verify05.py` from this directory. It reads native flow and
statement rows directly and does not invoke Axiom or import its parser. It computes
the 529 basis released by each withdrawal, taxable nonqualified earnings, payroll
and tax totals, and asserts the established figures below. The assertions also
check that the source still contains the 2024 state tax payment, the 401(k) and HSA
payroll amounts, and the 529 contributions.

| 2025 fact | Independently calculated value |
|---|---:|
| Gross wages | 247,440.07 USD |
| Pre-tax 401(k), HSA, health premium, and FSA flows | 36,946.36 USD |
| Interest income | 2,479.02 USD |
| Taxable 529 earnings plus HSA reimbursement | 1,012.94 USD |
| Total income / AGI | 213,985.67 USD |
| Mortgage interest | 24,077.32 USD |
| State and local taxes, after the 40,000 USD limit | 20,907.75 USD |
| Charity | 3,900.00 USD |
| Itemized deductions | 48,885.07 USD |
| Taxable income | 165,100.60 USD |
| Federal income tax before child credit | 26,150.13 USD |
| Federal tax after 2,200.00 USD child credit | 23,950.13 USD |
| Federal withholding | 27,594.00 USD |
| Federal refund (negative means refund) | -3,643.87 USD |
| California withholding, excluding 2024 balance payment | 11,239.60 USD |
| California tax balance due | 477.03 USD |
| Taxable nonqualified 529 earnings | 392.94 USD |

The 529 calculation begins with the opening statement's 24,600.00 USD value and
19,850.00 USD basis. The 6,000.00 USD of 2025 contributions increase basis. The
4,800.00 USD tuition payment is qualified; the 1,500.00 USD October transfer to
checking is not, and releases 392.94 USD of earnings. The separate 620.00 USD HSA
reimbursement is included in this historical return calculation because it is an
unqualified distribution in the source example.

The tax calculation uses the displayed 2025 federal and California schedules in
`verify05.py`, rounds each bracket slice to cents, and uses the source's one-child
filing status. It is a reproducible example oracle, not tax advice or an Axiom
runtime result.

## Verification status

The current native model inventory accepts this project with 30 source files, 864
flows, 853 transactions, 9 contracts, 52 laws, and zero model diagnostics. The
independent arithmetic oracle passes. These checks do not establish that every
flow has been folded into the expected runtime tax, purpose, and balance reports.

The journal retains the monthly 250.00 USD checking-to-529 contributions and
2,800.00 USD checking-to-savings transfers from 2025. The current future contracts
do not yet encode those elective owner-to-owner transfers. A combined scheduled
`from account into account` form was rejected by the native model inventory, so
the port does not invent a different direction or duplicate the transfer. Future
projection of those choices remains open until the model has a source-equivalent
form.

Files under `outputs/` and the corresponding old golden snapshots were produced
before the v4 source conversion. They are retained as historical evidence and are
not current expected output. Regenerate and grade those reports after the native
engine integration; do not compare the converted source against those old text
files as if they were v4 results.
