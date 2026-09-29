# 05 - A family

Two adults filing jointly, one child, calendar year 2025. Alex (37, salary plus
a March bonus) and Jordan (37, biweekly pay with a July raise) share a house with
a mortgage, a car with a loan, a joint checking account, an HSA, a
dependent-care FSA, two 401(k)s and a 529 plan for Riley (5). Sample data; every
account statement reconciles to the cent. The point is the friction: see
`../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax          root: me (Alex), jordan, riley; joint filing under us/ca
accounts.ax       kinds, places, employers and merchants, codes, a budget
laws.ax           itemized deductions and the child tax credit (year end)
systems/us.ax     a FORK of the shipped `us` with two marked edits (see below)
prices/home.ax    quarterly estimates for the house and the car
plans.ax          escrow payments, tuition, the yearly bonus
journal/2024/12   opening balances, dated 2024-12-31
journal/2025/*    January-December 2025
wishes.ax.txt     not loaded (the loader only reads *.ax)
outputs/          every command, run with --today 2026-01-05
```

## What it models

- **Payroll, twice.** Alex's stub (semi-monthly) splits gross 5,750.00 into a 10%
  401(k) deferral, an HSA deposit and a health premium and a dependent-care FSA
  deposit (all pre-tax), federal and California withholding, FICA, SDI and net
  pay (`...`). Jordan's biweekly stub has a 6% deferral and a 3% match. A 12,000
  bonus is withheld at the flat supplemental rates.
- **Mortgage with escrow.** The 3,174.46 payment is one transaction with three
  legs: principal (reduces `liabilities/mortgage`), interest (an expense) and
  escrow (an asset held by the servicer). Escrow then pays property tax (April,
  November) and insurance (June), and the July escrow analysis raises the
  escrow by 15.00 USD from September.
- **529 plan**: 250 USD a month, a 3,000 USD gift from grandma sent straight to
  the plan, one qualified withdrawal (4,800 USD of kindergarten tuition paid
  from the plan to `expenses/tuition`) and one non-qualified one (1,500 USD to
  checking for a brake job).
- **Daycare** paid partly by the FSA (up to what it holds) and partly from
  checking; after-school care from September.
- **Car loan**, **credit card** paid in full on the 25th, with two purchase
  refunds (`expenses/shopping -> card`) and a cash-back credit.
- **HSA**: medical spending straight from the account, and a 620 USD
  reimbursement to checking marked `!`.
- **One return for two people.** Axiom has one owner per place and tallies keyed
  by owner, so a joint return needs every place that receives pay to belong to
  `me`. Only Jordan's 401(k) is Jordan's, which keeps the 401(k) deferral limit
  per person. The consequence is wrong per-person wages: `axiom tax 2025
  --entity jordan` shows 5,846.36 USD of wages, not 97,440.07.
- **`systems/us.ax`** is a copy of the shipped `us` with two edits marked
  `PROJECT EDIT`: the standard deduction becomes `max(standard, tally(itemized))`
  and `federal-income-tax` subtracts `tally(credits)`. The project has no other
  way to add deductions or credits.

## What the commands should show (hand-verified; today = 2026-01-05)

An independent Python ledger (amortization, payroll, escrow, the 529 pro-rata
relief; `python3 verify-529.py` reruns the 529 part) reproduces every asset, liability and card balance below, and the
tax figures by hand.

`axiom check`: no errors, two waived warnings (the two opening 529 flows exceed
the yearly gift exclusion, and say so even with `!`).

`axiom balance`: assets 1,109,933.88 USD (house 612,400.00, car 25,300.00,
checking 36,838.99, savings 77,329.02, 401(k)s 313,762.82, 529 28,902.11, HSA
13,544.20, escrow 1,440.00, FSA 416.74); liabilities 425,770.63 USD (mortgage
406,692.02, car loan 16,976.91, card 2,101.70); net worth **684,163.25 USD**.
Mortgage interest paid in 2025 is 24,077.32 USD (= 12 x 2,484.46 - 5,736.20 of
principal).

`axiom tax 2025` (as the language computes it): wages 241,593.71 USD, pre-tax
31,100.00 USD, AGI 216,748.00 USD, itemized 48,885.07 USD (mortgage interest
24,077.32 + state and local taxes 20,907.75 + charity 3,900.00), taxable income
167,862.93 USD, income tax 26,757.84 USD (10% to 23,850, 12% to 96,950, 22%
above), credit 2,200.00 USD, withholding 27,594.00 USD, so nothing owed federally
(a refund of 3,036.16 USD is not booked). California: taxable 205,336.00 USD, tax
11,973.52 USD, withheld 11,239.60 USD, owed 733.92 USD. Also owed: 263.18 USD
(401(k) early-withdrawal penalty), 52.35 USD (529) and 124.00 USD (HSA).

Three of those numbers are wrong for reasons the findings explain:

- The 401(k) penalties are Q1's market losses (Alex 2,631.77 USD, Jordan
  1,685.99 USD): a fall in a tax-deferred account is a flow out of it.
- The HSA `!` waived nothing: the 620 USD reimbursement still counts as income
  and owes 124.00 USD, although the law's own doc comment says to mark it `!`.
- Grandma's gift arrives from an income place, so it has no basis and counts as
  earnings: the 529 non-qualified withdrawal shows 523.50 USD of earnings, not
  392.94 USD, and the plan's remaining basis is 18,602.82 USD instead of
  21,023.19 USD.

With those three removed the federal income tax is 26,013.73 USD, not 26,757.84.

`axiom tax 2025` after renaming `laws.ax` to `tax.ax` (see
`outputs/tax-if-laws-file-were-named-tax-ax.txt`): taxable income 185,248.00
USD, income tax 30,582.56 USD, and 2,988.56 USD owed. Nothing else changed.

`axiom available`: 115,608.01 USD "liquid", which counts the servicer's escrow
1,440.00 USD. The rest is listed with what withdrawing it would cost: the two
401(k)s would lose 51,720.06 + 25,801.29 USD to the 10% penalty and tax, the
car "realizes" a deductible-looking loss of 8,600.00 USD.

`axiom forecast`: starts at 327,537.38 USD "liquid net worth" = 115,608.01
liquid + 612,400.00 (house) + 25,300.00 (car) - 425,770.63 (all debts). The house
and car count as liquid because their *places* declare no liquidity, although
`HOME` has 90 days.

The forecast ends 2027-01-05 at 345,238.48 USD with `plans.ax` as written, and at
419,387.24 USD (`outputs/forecast-without-bonus-plan.txt`) without the bonus
plan. The 74,149 USD difference is Alex's salary: a forecast learns recurring
flows from the journal, except for any (from, to) pair a plan already projects,
and the bonus plan reuses the salary's own places, so the whole biweekly paycheck
drops out of "What recurs".
