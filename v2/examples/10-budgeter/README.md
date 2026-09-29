# 10 - A budgeter

Jamie, 24, junior designer, single, a room in a shared flat. September 2025 to
2026-02-14, with a question for June: can I afford a 3,000 USD trip? Envelopes
for the month and for the year, money set aside for the car, the insurance and
the trip, subscriptions, an ATM whose amounts were not written down, a check in
the mail, a check that never happened, a deposit that bounced, and a wallet that
does not add up. Sample data; every statement reconciles to the cent (an
independent Python model keeps every balance). The point is the friction: see
`../FINDINGS.md` and `wishes.ax.txt`.

```text
axiom.ax          root: me (single, born 2001, lives us)
accounts.ax       accounts, envelopes (`budget` lines), funds, payees, codes for checks and deposits
plans.ax          the June trip, as plans: flights in March, hotel and spending in June
journal/2025/*    September-December 2025
journal/2026/*    January and the first half of February 2026
wishes.ax.txt     not loaded (the loader only reads *.ax)
outputs/          every command, run with --today 2026-02-14
```

## What it models

- **Envelopes.** `budget 350 USD monthly` on groceries, 150 on dining, 120 on
  transport, fun, subscriptions, clothing, personal care, utilities; and yearly
  envelopes for gifts (500), car insurance (1,200) and medical (400). A budget
  only warns and starts again at zero each period: there is no rollover, so what
  is not monthly gets a fund instead.
- **Sinking funds.** `assets/funds/car-fund` gets 100.00 USD a month; it pays the
  486.25 USD brake job on 2026-01-15 and is left with 113.75 USD. The insurance
  fund (100.00 USD a month, opened with 700.00 USD) pays most of the 1,140.00 USD
  premium on 2025-12-12 (the rest from checking, as a two-source split). The
  trip fund gets 250.00 USD a month. Axiom cannot restrict money to a purpose
  that came from oneself, so each fund has `liquidity 1d`, which is a hack to keep
  it out of "available to spend".
- **Subscriptions**: five a month plus a forgotten one cancelled after two months and a
  139.00 USD yearly one that lands in November and breaks that month's 90.00 USD
  envelope, with a warning on it and on every subscription that follows.
- **Gaps in knowledge.** The ATM amounts are `checking -> wallet ? USD`, solved
  from the checking assertions on the day before and after (80.00 USD three
  times). An unexplained 27.60 USD in October and 14.00 USD in the wallet are
  accepted with `!` and booked from `unknown`, visibly. A 14.20 USD charge with no
  known payee is `checking -> ? 14.20 USD`, and a 20.00 USD gift is `? -> checking`.
- **A pending check**: `checking -> jo (240 USD) #check-1027` on 2025-10-02, settled
  on 2025-10-16. It reduces "available to spend" the day it is written, and counts
  in October's fun envelope the same day. **A check that never happened**:
  `#check-1029` (75.00 USD) is voided on 2026-01-20.
- **A returned deposit**: 350.00 USD deposited on 2025-11-10 (`#deposit-14`), returned
  on 2025-11-14 with a 12.00 USD fee.

## What the commands should show (today = 2026-02-14)

`axiom check`: no errors, nine warnings (October's fun envelope twice, November's
subscriptions five times, dining twice) and two pad notes. Net worth 10,665.04 USD:
checking 6,597.01, wallet 81.56, funds 4,813.75, card owed 827.28.

`axiom available`: 6,678.57 USD = checking + wallet; the funds are listed under "what
it would take to reach the rest" (4,813.75 USD). On 2025-10-05 it subtracts the
pending 240.00 USD: 3,562.98 USD.

`axiom budget 2025-11`: subscriptions 192.46 of 90.00 USD (213.84%), dining 163.17 of
150.00 USD; yearly gifts 460.60 of 500.00 USD, insurance 1,140.00 of 1,200.00 USD.

`axiom forecast --until 2026-07-15`: with the trip plans, liquid net worth on
2026-06-30 is 13,466.01 USD (p10 7,692.23 USD); without them (`outputs/forecast-without-the-trip.txt`)
13,966.01 USD. The trip changes the answer by only the 500.00 USD paid from checking:
the other 2,500.00 USD comes out of the trip fund, which is excluded from liquid net
worth, so "can I afford 3,000 USD?" is answered as if it cost 500.
It projects no problem: the trip fund is out of sight, and so is the question.
