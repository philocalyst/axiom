# 10 - A budgeter

Jamie, 24, junior designer, single, a room in a shared flat. September 2025 to
2026-02-14, with a question for June: can I afford a 3,000 USD trip? Envelopes
for the month and for the year, money set aside for the emergency fund, the car, the
insurance and the trip, subscriptions, an ATM whose amounts were not written down, a check
in the mail, a check that never happened, a deposit that bounced, and a wallet that does
not add up. Sample data; every statement reconciles to the cent (an independent Python
model keeps every balance), and `python3 ../verify/verify10.py` works the balances, the
envelopes, the budgets and the 2025 return from the journal text, without Axiom.

```text
axiom.ax          root: me (single, born 2001, lives us)
accounts.ax       accounts, the four funds as envelopes, purposes and budgets, payees, codes for checks and deposits
plans.ax          the June trip, as contracts: flights in March, hotel and spending in June
journal/2025/*    September-December 2025
journal/2026/*    January and the first half of February 2026
outputs/          every command, run with --today 2026-02-14
```

## What it models

- **Budgets.** A budget is a limit on a purpose: `budget groceries 350 USD monthly`, 150 on dining, 120 on
  transport, fun, 90 on subscriptions, 80 on clothing, 40 on personal care, 100 on utilities; and yearly
  envelopes for gifts (500), car insurance (1,200) and medical (400). There are no expense accounts: a
  merchant is a party that carries the purpose of what it sells (`entity kroger #groceries`), and a
  payment to no one in particular says its purpose itself (`visa -> ? 21.14 USD #dining`). A budget only
  warns and starts again at zero each period, and `axiom limits` and `axiom budget 2025` (or `2026`, or a
  month) show every envelope: spent, limit, room left, per month and for the year. One warning per breach.
- **Funds are envelopes.** One savings account holds them all, and each deposit is tied to its envelope
  with `for`: `checking -> savings 100 USD for car-fund`. The tied money is not available to
  spend, and `axiom available` lists what is held for each: 113.75 USD for the car, 3,000.00 for
  emergencies, 200.00 for insurance and 1,500.00 for the trip. The old project gave each fund
  `liquidity 1d` to keep it out of "available to spend", a hack that also priced it as if it took a day to
  reach. The car fund pays the 486.25 USD brake job on 2026-01-15 (`car-fund -> kwik-brakes`, the envelope as the
  source, so the payment takes the parcels tied to it) and is left with 113.75 USD; the
  insurance fund pays 1,100.00 USD of the 1,140.00 USD premium on 2025-12-12 and checking the
  other 40.00 (`me -> progressive 1_140 USD` with a leg from each source); the trip fund gets 250.00 USD a month.
- **Yearly bills are spread where they belong.** The premium is paid in December `for 2026`, so the
  yearly envelope of 2026 sees it (1,140.00 USD, then a 12.00 USD policy fee: 1,152.00 of 1,200.00 USD,
  room 48.00), and 2025's sees nothing. The envelope counts the premium on its own, 1,140.00 of 1,200.00 USD,
  before the fee or any other flow lands in 2026. The 139.00 USD Prime membership, paid on November 3, is written
  `2025-11-03..2026-11-02` and recognized a little each day: the subscriptions envelope sees 10.66 USD of it
  in November (28 days of 365), not a 139.00 USD spike that broke the month and warned five times.
- **Gaps in knowledge.** The ATM amounts are `checking -> wallet ? USD`, solved from the checking
  assertions on the day before and after (80.00 USD three times). An unexplained 27.60 USD in October and
  14.00 USD in the wallet are accepted with `!` and booked from `unknown`, visibly. A 14.20 USD charge with no
  known payee is `checking -> ? 14.20 USD`, and a 20.00 USD gift is `checking <- ? 20 USD`.
- **A pending check**: `checking -> jo (240 USD) ^check-1027` on 2025-10-02, settled on 2025-10-16
  (`^check-1027 settled`). It reduces "available to spend" the day it is written, and counts in October's
  fun envelope the same day. **A check that never happened**: `^check-1029` (75.00 USD) is voided on
  2026-01-20 (`^check-1029 void`).
- **A returned deposit**: 350.00 USD deposited on 2025-11-10 (`^deposit-14`), returned on 2025-11-14
  (`^deposit-14 returned`) with a 12.00 USD fee.
- **A pre-tax premium.** A stub is the gross pay (`checking <- studio 2_307.69 USD #wages`) and what comes
  out of it: 226.00 USD of income tax to the IRS, 176.54 USD to the SSA, and the 60.00 USD health premium,
  written `#pretax-premium`. That purpose is of the kind `pretax-benefit`, and its law counts the premium as
  pre-tax and takes it back out of income, so `us` taxes 20,229.21 USD, not 20,769.21.

## What the commands should show (hand-verified)

`python3 ../verify/verify10.py` replays the journal, solves the blank ATM amounts from the statements and prints the
figures below. Every one agrees with Axiom.

| line | by hand | Axiom |
|---|---|---|
| checking | 6,597.01 | 6,597.01 |
| wallet (after the 14.00 gap accepted) | 81.56 | 81.56 |
| savings: emergency 3,000.00 + car 113.75 + insurance 200.00 + trip 1,500.00 | 4,813.75 | 4,813.75 |
| card owed | 839.28 | 839.28 |
| **net worth on 2026-02-14** | 10,653.04 | 10,653.04 |
| the ATM, three times, solved from the statements | 80.00 | 80.00 |
| the premium: from the fund 1,100.00 + from checking | 40.00 | 40.00 |
| October, fun: gym 29.00 + the pending check 240.00 + 28.01 + 26.01 (limit 120.00) | 323.02 | 323.02 |
| November, dining (limit 150.00) | 163.17 | 163.17 |
| December, dining (limit 150.00) | 160.00 | 160.00 |
| November, subscriptions: 53.46 of the monthly ones + 10.66 of the membership | 64.12 | 64.12 |
| 2025, gifts (185.60 + 200.00 + the 75.00 online donation; the voided check is not counted) | 460.60 | 460.60 |
| 2026, insurance: 1,140.00 recognized `for 2026` + the 12.00 fee (limit 1,200.00) | 1,152.00 | 1,152.00 |
| 2025 wages: 9 stubs of 2,307.69 | 20,769.21 | 20,769.21 |
| pre-tax health premium, 9 x 60.00 | 540.00 | 540.00 |
| taxable income (15,750.00 standard deduction) | 4,479.21 | 4,479.21 |
| income tax (10%) | 447.92 | 447.92 |
| withholding, 9 x 226.00 | 2,034.00 | 2,034.00 |
| **federal, owed (negative = refund)** | -1,586.08 | -1,586.08 |

`axiom check`: no errors, three warnings (October's fun envelope, November's and December's dining) and two pad
notes. 251 flows, 47 places, 15 laws enforced, net worth 10,653.04 USD.

`axiom available`: 6,678.57 USD (checking and wallet); the 4,813.75 USD in savings is *held for* the
four envelopes and taken off. On 2025-10-05 it also subtracts the pending 240.00 USD: 3,562.98 USD.

`axiom budget 2025-11`: dining 163.17 of 150.00 USD (108.78%); yearly gifts 460.60 of 500.00 USD. `axiom budget 2026` and
`axiom limits 2026` show the year's envelopes as of today: insurance 1,152.00 of 1,200.00 USD.

`axiom forecast --until 2026-07-15`: on 2026-06-30 the Net worth column is 18,457.76 USD with the trip contracts and
21,457.76 USD without them (empty `plans.ax` and run it again): the whole 3,000.00 USD. The Committed
column, which leaves out what is held for envelopes, is 17,730.09 against 18,230.09 USD: 500.00 USD of it (see
below), and no problem is projected.

## Findings this project now demonstrates as fixed

F16 (money set aside by oneself: an envelope entity and `for` tie it, `available` lists what is held for each, and no
account needs `liquidity 1d`), F18 (annual bills spread over the period they cover, by `for 2026` and by a range), F26
in part (a warning per breach, not one per flow after it, and the yearly bill no longer breaks the month), F19
(the health premium is pre-tax through its purpose's kind, and the refund is booked, negative).

## Still open

- **"Can I afford 3,000 USD in June?" is still two forecasts and a subtraction.** The Net worth column moves by the
  whole trip and Committed by 500.00 USD: the forecast lets the trip fund pay 2,500.00 USD although it holds
  1,500.00 USD (it does not project the monthly deposits into the funds), so only what checking pays is
  taken from Committed. With 350.00 USD from the fund and 1,500.00 USD from checking in June, Committed moves by
  1,500.00 USD, which is what the trip written as plans moved it by. The forecast also no longer finds the
  dining spending a recurring flow (`visa -> ? #dining`, 24.50 USD a week, was found 27 times when dining was an
  account): it names no party to recur with, so the Net worth column is higher by that 24.50 USD a week, 490.00 USD by
  the end of June.
- **No rollover.** A budget starts at zero each month, so a bill that is not monthly still needs its own envelope.
- **The gaps are mixed in with the rest.** The 14.20 USD out and the 20.00 USD in are flows with `?`, like every
  `#dining` charge (`axiom register 'entity:?'`), and the 27.60 and 14.00 USD accepted with `!` are two pad
  notes of `check`; no view lists the four together.
