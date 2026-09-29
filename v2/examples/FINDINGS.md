# Axiom v2 in the hands of real ledgers: findings

Lane G1, explorer and critic. Seven ledgers of the kind a demanding user actually
keeps, written against the v2 language as it stands, run through every command,
and checked by hand or by an independent model. This file lists everywhere the
language or the model **cannot represent** something real, **gets ugly** for no
reason, **computes something wrong**, or **explains itself poorly**, ranked by
how much it hurts, with a proposed fix that stays inside Axiom's philosophy
(flows are the only facts; parcels remember provenance; kinds type everything;
laws are what must hold; systems are written in Axiom; progressive complexity).

Nothing under `v2/crates/`, the docs or examples 01 to 03 was touched. No bug was
fixed; each is reported with a repro.

**The headline.** The language is close: all seven ledgers could be written and
every one reconciles to the cent. What could not be written honestly is anything
that *happens to a holding after it arrives* (splits, basis, depreciation),
anything *owed* (invoices, IOUs, deposits, loans, earmarks, reimbursements),
anything with *more than one owner or one residence*, and anything whose *period
differs from its date*. Around those gaps sit silent wrong numbers in the shipped
`us` system and in `forecast` and `available` (F03, F04, F08, F10 to F13), several
of which are fixable in a few lines of Axiom today.

## Status after the v3 rework

Examples 04 to 10 were rewritten against v3 and the systems (`std`, `us` and what is under it) with them; each
example's README says which items it now demonstrates as fixed and which are still open, with the numbers
checked by an independent script in `examples/verify/`. The text below this section is the original report
(v2, before the rework): its repros no longer run as written, and its `wishes.ax.txt` files are gone.

| # | Status | Where |
|---|---|---|
| F01 basis by route | fixed | 05 (a gift at cost by the kind), 06 (ESPP and wash adjustments as basis flows), 07 (roof, depreciation) |
| F02 stock split | fixed | 06 (`FAST split 2 for 1`) |
| F03 market loss taxed | fixed | 05 (`via market`) |
| F04 losses do not net | fixed | 06 (netting across terms, NIIT) |
| F05 one owner, per-place tallies | fixed for households | 05 (one joint return, each spouse's own limit) |
| F06 claims | fixed | 04 (invoices), 07 (mortgage, deposits, bills), 09 (roommates, loan, trip, reimbursement) |
| F07 payment for a year | fixed | 04 (estimates), 08 (`for 2024` and `for 2025`) |
| F08 law order | fixed | 04, 05 (dataflow order; no fork of `us`) |
| F09 part-year and overlapping residence | fixed | 08 (California files for the part year) |
| F10 `available` | partly | held, coming in and foreign cash are right (05, 06, 07, 08, 09); the tax on money drawn and a mortgage against a house are not netted |
| F11 plan deletes a recurrence | fixed | 05 |
| F12 forecast liquid net worth | fixed | 05 |
| F13 forecast past its reason | partly | 07 (`until`); 09 still projects a loan repayment past the balance |
| F14 foreign currency | fixed | 08 (cash, no lots, no gain under 200 USD, `@` states a rate) |
| F15 `!` and priced violations | fixed for the price | 05 (the reimbursement still counts as income) |
| F16 self-imposed restrictions | fixed | 10 (envelopes) |
| F17 opening balances | fixed | 05, 06, 08 (`opening` with `basis` and `since`) |
| F18 spreads (prepaid, annual, depreciation) | fixed | 04, 07, 10 (`for PERIOD`, ranges, basis flows) |
| F19 slots and totals in `us` | fixed | every project (`adjustments`, `itemized`, `credits`, `payments`, booked refunds) |
| F21 owner scope in reports | open | 09 (the collective's money is in `available` and `flow`) |
| F23 same-day order | open | 07 (the half month of depreciation before the sale) |
| F24 personal-use property | fixed | 05 (loss not deductible, §121) |
| F25 closing statement, `all X` | fixed | 06 (`all VXUS`), 07 (the sale) |
| F26 budgets | partly | 10 (one warning per breach, the yearly bill spread); no rollover |
| F27 windowed laws | partly | 09 (a loan needs no monthly law); still no calendar windows |
| F28 running maximum, days abroad | open | 08 |

## How this was done

| Project | Who and when | What it stresses | Journal lines | Checked against |
|---|---|---|---|---|
| `04-freelancer` | Sole proprietor consultant, Jan 2025 to Feb 2026 | Cash-method tax on accrual books, invoices, SE tax, SEP-IRA, QBI, 1040-ES installments, mileage | 555 | Python model of the tax return and every statement |
| `05-family` | Two earners and a child in California, Dec 2024 to 2025 | Joint return, mortgage and escrow, two 401(k)s, HSA, dependent-care FSA, 529, itemized deductions, child credit | 1,163 | Python ledger (amortization, payroll, escrow, 529 pro-rata), tax by hand |
| `06-investor` | Sep 2024 to 2025 | Monthly DCA, DRIP, RSUs with sell-to-cover, ESPP, ACATS transfer, specific-lot and HIFO sales, crypto swap and staking, wash sale, tax-loss harvest | 548 | Independent lot engine: gains match to the cent |
| `07-landlord` | One rental house, Dec 2024 to 2025 | Mortgage, depreciation, a roof replaced, security deposits, a sale with recapture, `sync` of a price file | 333 | `check07.py`: basis, depreciation, recapture |
| `08-expat` | US citizen moves San Francisco to Berlin, 2025 | EUR accounts, FX, FEIE with stacking tax, FBAR, part-year residence | 473 | Hand: stacking tax 3,599.63 vs Axiom 3,599.64 |
| `09-shared` | Roommates, a friend's loan, tip income, a nonprofit collective with a restricted grant | IOUs, reimbursements, restricted money, other people's money in your books | 708 | Hand and Python |
| `10-budgeter` | 24-year-old on envelopes, Sep 2025 to 2026-02-14 | Budgets, funds, ATM amounts inferred by `?`, checks in the mail, a bounced deposit, "can I afford June?" | 349 | Python model keeps every balance |

About 4,100 journal lines and 2,300 lines of accounts, laws, plans and prices in
all. Every ledger reconciles to the cent, every assertion passes, and the
numbers quoted below were reproduced from the saved outputs in each project's
`outputs/`. Each project has a `README.md` (what it models, what each command
should show), a `wishes.ax.txt` (what the language could not say, written as the
syntax I wished for; not loaded because the loader reads only `*.ax`), and
`outputs/<command>.txt` for `check`, `balance`, `balance --value`, `register`,
`flow`, `available`, `budget`, `tax`, `lots`, `forecast` and `why` on a law, a
place and a code.

The journals were produced by throwaway Python generators that also modelled
every balance and wrote the statement assertions, so Axiom's `check` is a second,
independent reconciliation; those generators are wired to scratch paths and are not
committed. Three small hand-check scripts are: `04-freelancer/verify.py` (the
return), `05-family/verify-529.py` (the 529 pro-rata) and `07-landlord/verify.py`
(mortgage, expenses, recapture). Each prints the figures its README quotes.

Small probes (a few lines each) that isolate a finding are reproduced inside it.
Every repro below was run against `v2/target/release/axiom`.

**Nothing panicked, nothing hung, nothing was slow.** A synthetic ledger of
47,711 flows over 60 years of weekday trades ran `check`, `balance`, `lots`,
`available`, `tax`, `flow`, `register`, `forecast` and `why` in 0.09 to 0.23 s
each. There is no `slow` finding and no `crash/panic` finding.

### Reading guide

- **Severity.** S1: a headline number is silently wrong, or a common real
  situation cannot be written and has no honest workaround. S2: wrong or
  misleading in one report, or a hack a user would not find. S3: friction and
  confusing output. S4: polish.
- **Fix lives in.** *core* needs a new concept in the engine; *system* is a
  change to a system written in Axiom (`us`, `std`); *report* is a change to a
  command; *diagnostic* is wording and locations.
- Quoted output is verbatim; long outputs are trimmed with `...`.

## What worked, and must survive the rework

- **`?` inference and its diagnostics.** ATM amounts solved from assertions on
  the day before and after (`10-budgeter`); the explanation of *why* a second
  unknown cannot be solved is the best diagnostic in the tool.
- **Tallies as memory.** `count max(v - tally(x), empty) as x` is a running maximum
  in one line; the FBAR law in `08-expat` is built on it.
- **Pending mode.** `(4_800 USD) #inv-1` settled by `#inv-1`, voided by `void`:
  cash-method taxes count on settlement, and a write-off is a void. Elegant.
- **Restricted entities and relief order.** A grant spent on pizza is paid from
  the untied money first; the return of the unspent 57.25 needs no waiver.
- **An `org` with no `lives`** has no tax laws: a nonprofit collective is free.
- **Commodities for non-money things.** Mileage as `MI` valued by
  `value(amount, USD)` in a law; the roof as its own commodity.
- **`sync FILE / run CMD`** wrote 6 price lines identically on every run.
- **Diagnostics that print the fix line** (`13 + account assets/house : broker`)
  and the `us/ira` contribution-limit explanation with its statute citation.
- **Performance.** See above.

## Index

| # | Sev | Category | Finding | Fix lives in |
|---|---|---|---|---|
| F01 | S1 | cannot-represent, wrong-result | Cost basis is decided by the route the money took | core |
| F02 | S1 | cannot-represent | A stock split cannot be written | core |
| F03 | S1 | wrong-result | A market loss in a tax-deferred account is taxed as a withdrawal | system, core |
| F04 | S1 | wrong-result | Capital losses do not net against long-term gains | system |
| F05 | S1 | cannot-represent, wrong-result | A place has one owner, and tallies follow the receiving place | core |
| F06 | S1 | cannot-represent, missing-report | Money owed, lent, deposited, invoiced or reimbursed has no home | core |
| F07 | S1 | wrong-result | A payment cannot say which tax year it is for | core |
| F08 | S1 | wrong-result | Law order is implicit, differs by trigger, and depends on file names | core |
| F09 | S1 | cannot-represent, wrong-result | Residence is one sequential timeline; a part-year residence never files | core, system |
| F10 | S1 | wrong-result | `available` counts money that is not yours, not money, or spoken for | report, kinds |
| F11 | S1 | wrong-result | A plan silently deletes the recurrence it resembles | report |
| F12 | S1 | wrong-result | Forecast "liquid net worth" counts the house and the securities | report |
| F13 | S1 | wrong-result | Forecast projects past the end of its reason, and misses standing orders | report |
| F14 | S2 | wrong-result | Foreign currency is treated as an investment | system, core |
| F15 | S2 | wrong-result, bad-diagnostic | `!` does not waive priced violations; useless waivers go unflagged | core |
| F16 | S2 | cannot-represent | Restrictions cannot be self-imposed and never lift | core |
| F17 | S2 | cannot-represent | Opening balances are dated flows; history before the first param row errors | core |
| F18 | S2 | cannot-represent | Depreciation, prepaid and annual bills are spread only in `flow` | core |
| F19 | S2 | ugly/verbose, wrong-result | `us` has no slot for deductions or credits, and no total | system |
| F20 | S2 | wrong-result | Valuation is decided report by report | core |
| F21 | S2 | missing-report | No owner scope in any report | report |
| F22 | S2 | bad-diagnostic | Reports refuse to run on any error and do not say how to override | diagnostic |
| F23 | S2 | wrong-result | Same-day flows: declaration order changes what laws see | core |
| F24 | S2 | wrong-result | Personal-use property: deductible losses, no home-sale exclusion | system |
| F25 | S2 | ugly/verbose | A closing statement, a two-source purchase and `all X` cannot be written | core (syntax) |
| F26 | S3 | confusing-output | Budgets re-warn on every flow, ignore spreads, roll nothing over | core, report |
| F27 | S3 | cannot-represent | Laws cannot be windowed or scheduled by the calendar | core |
| F28 | S3 | ugly/verbose | Memory and spans: running max by tally hack; days abroad by hand | core |
| F29 | S3 | bad-diagnostic | `?` failures cascade into false assertion errors with "-0.00" | diagnostic |
| F30 | S3 | bad-diagnostic | `ambiguous-law` lists identical names and no locations | diagnostic |
| F31 | S3 | bad-diagnostic | Adding an account breaks lines written long before | core, diagnostic |
| F32 | S3 | ugly/verbose | Code rules: one glob, one role, header codes rejected | core |
| F33 | S3 | ugly/verbose | Every payee needs an entity; restricted payees force omissions | core |
| F34 | S3 | confusing-output | Register, `lots` and `why` output gaps | report |
| F35 | S3 | missing-report | Reports a real user asks for and does not get | report |
| F36 | S4 | mixed | Small things (list) | mixed |

---

## S1: silently wrong numbers, or a common situation with no honest way to write it

### F01. Cost basis is decided by the route the money took

**Category:** cannot-represent, wrong-result. **Fix lives in:** core (a basis
dimension on parcels and an arrival rule on kinds). **Seen in:** 04, 05, 06, 07.

**Repro**

```text
use us/ira
use us/529
account assets/ira : traditional-ira
account assets/education/riley-529 : 529-plan

2025-03-10 checking -> ira 6_000 USD                 // a deducted contribution: its basis should be 0
2025-05-16 gifts -> riley-529 3_000 USD / grandma    // an after-tax gift: its basis should be 3_000
```

**What happened.** A parcel's basis is its value on arrival *unless it came from
an income place, in which case it is zero*. The user has no say. Both lines
above get the wrong basis, in opposite directions. `axiom lots`:

```text
  Place                           Quantity     Basis  Acquired    Held          Value    Unrealized  Term
  assets/education/riley-529  3,000.00 USD  0.00 USD  2025-05-16  7m15d  3,000.00 USD  3,000.00 USD  short
  Total                                     0.00 USD                     3,000.00 USD  3,000.00 USD
```

The gift is 3,000 of pure "gain" with no basis, and the IRA does not appear at all
because its 6,000 sits at face basis, so a withdrawal is taxed on nothing.

`05-family`: grandma's real 3,000 USD gift arrives the same way, so the 529's
non-qualified withdrawal reports 523.50 USD of earnings (correct: 392.94) and the
plan is left with 18,602.82 USD of basis (correct: 21,023.19; run
`05-family/verify-529.py`). In `04-freelancer` the
SEP-IRA holds 29,353.79 USD but `lots`
lists only the 21,353.79 that arrived from income at zero basis; the 8,000.00
deducted contribution made from checking sits at face basis, so a withdrawal is
taxed on 21,353.79 instead of 29,353.79 and `available` prices the 10% penalty at
2,135.38 instead of 2,935.38.

The same root, elsewhere:

- **Wash sale** (`06`): the disallowed loss 1,978.60 is parked in
  `equity/wash-sale`, released as a second funding leg of the replacement buy,
  and added back by a second law. `balance` then shows `wash-sale 1,978.60 USD`
  as if equity had been created. Nothing checks the 30 days, and the holding
  period does not tack.
- **Improvements and depreciation** (`07`): the basis of a house cannot change,
  so the new roof is a second commodity `ROOF` and depreciation is a hand-written
  memo flow `equity/acc-dep-house -> expenses/rental/depreciation` every month.
  `flow` then reports a realized gain of **13,481.25** while the tax laws count
  **13,610.35** long-term plus **10,049.38** recapture (23,659.73); depreciation
  is an expense in the P&L that never reduces the asset.
- **ESPP** (`06`): a discounted purchase is two transactions (cash, then
  `income/espp-discount`) because a share purchase cannot have two funding
  commodities (F25).
- **Exchange spread** (`06`): a BTC to ETH swap values the proceeds at BTC's
  market price; the fee hidden in the rate is capitalized into the ETH basis and
  the gain is overstated. `LANGUAGE.md` says Axiom never books a difference
  silently.
- **Opening date** (`05`, F17): a lot's acquisition date is the day the opening
  balance was written.

**What should happen.** Basis is what the user put in. The route is a default,
never a rule, and every later change to a parcel's basis is a flow one can `why`.

**Proposal (core; the rest is Axiom).** Treat basis as a second dimension of the
same double entry, not a hidden field.

1. *Arrival rule on the kind.* `kind sep-ira` says `arrives basis zero`;
   `kind 529-plan` says `arrives basis cost`; a flow can override with a tail
   (`... basis 0`). Default stays "cost, except from income places".
2. *`PLACE[basis]` is that dimension as a place* (the selector syntax
   `broker[2024-02-01]` already exists). Capitalizing is a flow into it
   (`checking -> house[basis] 14_200 USD / summit-roofing`), depreciating is a
   flow out of it (`house[basis] -> expenses/rental/depreciation 456.79 USD`),
   so the P&L, the asset and the tax gain agree. The roof needs no commodity.
3. *Wash sale, recapture and section 121 are laws that read basis.* A law may
   only observe, so the wash adjustment is a **derived flow**, labelled with the
   `≈` `flow` already uses for realized gains ("the journal does not state
   them"), listed by `why` and undoable by writing the flow yourself.
4. *Exchange cost is derived the same way:* a two-priced flow reports the
   difference between the two market values as a fee, in `flow`.

`wishes.ax.txt` in `05-family`, `06-investor` and `07-landlord` spell out the
syntax.

### F02. A stock split cannot be written

**Category:** cannot-represent. **Fix lives in:** core (a flow mode). **Seen in:** 06.

**Repro**

```text
2024-03-01 checking -> broker 100 ACME @ 50 USD
2025-02-01 broker 100 ACME -> broker 200 ACME          // 2-for-1
2025-02-02 equity/split -> broker 100 ACME @ 0 USD     // the other way to say it
```

**What happened**

```text
error[amounts-differ]: the two sides state different amounts
19 │ 2025-02-01 broker 100 ACME -> broker 200 ACME
   │                            ─┬
   │                             ╰── 100.00 ACME leaves, but 200.00 ACME arrives
   = note: a transfer keeps its amount: value is never created or lost between the two places

error[price-zero]: a price is more than nothing
20 │ 2025-02-02 equity/split -> broker 100 ACME @ 0 USD
```

Both refusals are right about what they were asked. There is nothing to write
instead. `06-investor` states every lot already split-adjusted, which breaks the
statement assertions dated before the split and loses the price the trade was
executed at. Reverse splits, spin-offs, mergers and stock dividends have the same
problem.

**What should happen.** Every lot keeps its basis and acquisition date; only
the quantity scales.

**Proposal (core).** A *restate* mode on a flow, the way `(pending)` is a mode: a
flow whose two sides are the same place and commodity, value conserved and
quantity scaled, `split broker ACME 1:2` as its surface. A spin-off is a restate
into two commodities with a basis allocation (`... basis 90%/10%`). It is a fact
in the journal (`why` shows it), so nothing is edited and no history is lost.

### F03. A market loss in a tax-deferred account is taxed as a withdrawal

**Category:** wrong-result. **Fix lives in:** system (`us/401k`, `us/ira`) plus a
small core idea (a mark). **Seen in:** 04, 05.

**Repro**

```text
use us/401k
account assets/k401 : 401k
account income/growth
2025-01-10 acme -> k401 6_000 USD       // payroll deferral: zero basis
2025-03-31 k401 -> growth 400 USD       // the market fell 400
```

**What happened**

```text
Taxes 2025 for me
    us/401k
      pretax              6,000.00 USD  axiom.ax:20
      agi                   400.00 USD  3 sources
      distributions         400.00 USD  axiom.ax:22
Owed
    us/401k
      early-withdrawal-penalty  irs  2025-03-31  40.00 USD  axiom.ax:22
```

and `axiom flow` books it back as income:

```text
    growth                      -400.00 USD
    realized gains ≈             400.00 USD
  Net                 0.00 USD     0.00 USD ...
```

A loss shows as a gain of the same size, and Net is zero. In `05-family` the two
Q1 losses (2,631.77 and 1,685.99 USD) became distributions with 263.18 and
168.60 USD of penalties, and federal tax was overstated by 744.11 USD. In
`04-freelancer` the SEP-IRA's Q1 loss of 385.35 caused a 38.54 penalty.

**Why.** The only way to write a fall in value is a flow from the account to an
income place, and that is indistinguishable from paying money out to a
non-deferred place.

**What should happen.** No distribution, no penalty, and `flow` shows the loss as
a loss.

**Proposal.** (a) System: the distribution and penalty laws add
`when not to is market`, where `market` is a kind for the counterpart of growth
and loss (`income/growth : market`, in `std`). (b) Core, because users will
otherwise write it wrong: a *mark*, the assertion that adopts a statement
value: `2025-03-31 k401 = 24_600 USD via income/growth` posts the difference,
in either direction, to the named place, instead of making the user work out
a flow. `!` is the same idea with `equity/unknown`. (c) `flow` must not derive
realized gains from a flow to a `market` place.

### F04. Capital losses do not net against long-term gains

**Category:** wrong-result. **Fix lives in:** system (`us.ax`). **Seen in:** 06.

**Repro**

```text
// single filer, 2025
2025-01-15 income/salary -> checking 150_000 USD
2024-02-01 checking -> broker 100 AAA @ 100 USD
2025-03-01 checking -> broker 100 BBB @ 200 USD
2025-06-01 AAA 180 USD
2025-06-01 BBB 140 USD
2025-06-02 broker 100 AAA -> checking 18_000 USD    // long-term gain   +8,000
2025-06-02 broker 100 BBB -> checking 14_000 USD    // short-term loss  -6,000
```

**What happened**

```text
    wages             150,000.00 USD
    agi               152,000.00 USD  3 sources
    long-term-gains     8,000.00 USD
    short-term-gains   -6,000.00 USD
    taxable-income    136,250.00 USD  period end
    income-tax         24,827.00 USD  period end
```

`federal-income-tax` defines `ordinary-income = agi - long-term-gains`, so the
6,000 short-term loss reduces *ordinary* income (saving 24%) while the whole 8,000
long-term gain is taxed at 15%. The loss first offsets the gain: net long-term is
2,000, ordinary income is 134,250, and the tax is **25,367.00**. Axiom
understates by 540.00 USD (6,000 x (24% - 15%)). The docs admit the 3,000 cap on
net losses is not modelled; the netting is a different bug.

**Proposal (system, two lines in `us.ax`, no new concept).** Verified on a copy
of the shipped `us.ax` in a scratch project (`systems/us.ax` overriding the
embedded one): it gives 25,367.00 USD.

```text
  let lt-net = max(tally(long-term-gains) + min(tally(short-term-gains), empty), empty)
  let ordinary-income = tally(agi) - lt-net          // was: tally(agi) - tally(long-term-gains)
```

A loss first offsets the other term; only what remains is ordinary. The 3,000
USD cap on deducting a net loss, with the excess carried forward, needs
`tally(x, year - 1)` (F07).

### F05. A place has one owner, and tallies follow the receiving place

**Category:** cannot-represent, wrong-result. **Fix lives in:** core (owner sets
and attribution) with a household kind in `std`. **Seen in:** 05, 09.

**Repro**

```text
entity me : person
  filing joint
entity jordan : person
  filing joint
account assets/bank/joint : bank
  owner me, jordan
```

**What happened**

```text
error[property-argument]: `owner` takes no more arguments here
16 │   owner me, spouse
   │             ───┬──
   │                └── unexpected
```

So a joint account is owned by one of them. Worse, a tally is keyed by the owner
of the *receiving* place, so a paycheck is split by where it lands. Two paychecks
(6,000 and 4,000 gross) with Jordan's net pay going into the joint account owned
by `me`:

```text
Taxes 2025 for me
    wages                 9,200.00 USD  4 sources       (6,000 hers + 3,200 of Jordan's net)
    federal-withheld        700.00 USD
```

Jordan's tallies hold the other 800 (the 401(k) leg and the withholding). If each
spouse says `filing joint`, each gets a tax computed on their own fragment with
joint brackets. In `05-family` I gave every place to `me` except Jordan's 401(k)
(limits and age are per person): the joint return comes out right, and every
per-person figure is wrong ("Jordan's wages" are 5,846.36 instead of
97,440.07). The same wall appears in `09-shared`: a collective's account and the
roommates' shared costs cannot belong to two people.

**What should happen.** A paycheck is its earner's wages wherever the net pay
lands; a couple's return sums both; a joint account has two owners.

**Proposal (core).**

1. `owner a, b` (equal shares) or `owner a 60%, b 40%`: a place has an owner *set*.
2. *Attribution.* A flow counts for the owner of the **income-side** place when
   there is one (`entity bluefin : employer, employs jordan` or the owner of
   `income/jordan-salary`), else for the owner of the source place; never for the
   place it happens to land in.
3. `kind household : entity` in `std` (`members alex, jordan`, `filing joint`,
   `lives us/ca`). A tax law that runs `each year` on a household reads
   `tally(agi)` summed over its members; a per-person limit reads
   `tally(x, member)`. The household is written in Axiom; only owner sets and
   attribution need the engine.

Details in `05-family/wishes.ax.txt` (1) and `09-shared/wishes.ax.txt` (2).

### F06. Money owed, lent, deposited, invoiced or reimbursed has no home

**Category:** cannot-represent, missing-report. **Fix lives in:** core (parcel
terms and a settlement link); reports and laws follow. **Seen in:** 04, 07, 09, 10.

**Repro**

```text
2025-03-01 acme -> checking (4_800 USD) #inv-1      // invoice sent, net 30
```

`axiom balance`, `axiom available` and `axiom forecast` on 2025-04-15 all show
1,000.00 USD (the opening balance) and nothing else. Only `why` can find it:

```text
Why #inv-1
  2025-03-01  income/consulting → assets/bank/checking  4,800.00 USD  pending  axiom.ax:19
```

`axiom check` on 2026-04-15, 13 months later, prints
`✓ 2 flows · 5 places · 2 laws enforced · net worth 1,000.00 USD`. There is no
due date, no aging, no partial payment, and an invoice that will never be paid
looks the same as one due tomorrow.

The workaround in `04-freelancer` (a `kind receivable` place with a law on
`out` that counts cash-method receipts) works but is a convention each user must
invent, and `available` counts the receivable as spendable unless you add
`liquidity 45d`. The same thing again, each with its own workaround:

- **Roommate IOUs** (`09`): an asset place per roommate that goes negative with no
  warning, shown in `balance` as `ben 37.21` with no hint of who owes whom.
- **A loan to a friend** (`09`): `loans/riley` plus a law
  `warn total(out, month) >= self.expected` that also fires in the month the
  loan was made, because a law has no start date.
- **Reimbursements** (`09`): the code rule that marks the expense refuses the
  pending reimbursement (`code-placement`), so each event needs two codes (F32).
- **A security deposit** (`07`): a liability place per tenant, a second place
  `repay`, a restricted entity whose tie never lifts (F16).
- **A grant** (`09`): an entity with `purpose` and `until`, whose status is not
  reported anywhere.
- **A check in the mail** (`10`): `checking -> jo (240 USD) #check-1027` is a
  pending flow, invisible except in `available`, which subtracts it.

**What should happen.** Each of these is one thing: *someone owes someone an
amount, for a reason, by a date*. It should be on the balance sheet as such, on
the calendar, in `available` as "coming in" or "owed", aged, settleable in
pieces, and impossible to forget.

**Proposal (core, small).** A **claim** is a parcel in a place of kind `claim`
carrying *terms*: counterparty, `due`, optionally a `purpose`. It is created by a
flow and extinguished by a flow that says what it settles: `for #inv-1`.

```text
2025-03-01 acme -> receivables 4_800 USD due 2025-03-31 #inv-1
2025-04-20 receivables -> checking 3_000 USD for #inv-1         // partial
2025-05-09 receivables -> checking 1_800 USD for #inv-1         // settled
```

Everything else is a law or a report: cash-method tax counts *on settlement*
(a law on `for`, which is today's `on out` workaround made honest); `check`
warns on `due` passed; `available` lists claims under "coming in" and never in
"spend"; forecast puts them on their due dates; an aging report is a group-by;
a pending flow is a claim that has not yet reached a place; a void is a write-off.
`for` is also what lets an HSA reimbursement point at the dentist's bill (F15)
and a check settle a pending flow. `04-freelancer/wishes.ax.txt` (1),
`07-landlord/wishes.ax.txt` (2) and `09-shared/wishes.ax.txt` (1, 3) draw it.

### F07. A payment cannot say which tax year it is for

**Category:** wrong-result. **Fix lives in:** core (a `for` attribution on a flow;
explicit-year `tally`). **Seen in:** 04, 05.

**Repro**

```text
2025-06-01 income/client -> checking 60_000 USD
2026-01-15 checking -> taxes/federal 3_000 USD      // the fourth 1040-ES installment for 2025
```

**What happened.** `axiom tax 2025` owes the whole 5,071.50 USD; the January
payment is counted in 2026:

```text
Taxes 2026 for me
    federal-withheld  3,000.00 USD  axiom.ax:17
```

In `04-freelancer` the total owed for 2025 is 4,176.81 USD (4,101.28 without
F03's artifact) where it should be 1,701.28 USD once the 2,400 USD paid on
2026-01-14 counts for 2025 (`verify.py`), and the `by 2026-01-15` law "fourth installment" reads the 2026
tally and warns that the fourth installment is short when it has just been paid
("estimated tax: fourth installment short"). The same shape: an IRA contribution
made in February for the previous year; a state refund received in the next year.

**What should happen.** A flow has a date it moved and a period it belongs to; the
tally follows the second.

**Proposal (core).** A `for 2025` tail on a flow, defaulting to its own year:
`2026-01-15 checking -> taxes/federal 3_000 USD for 2025`. `tally(x)` reads the
law's own period; `tally(x, 2025)` reads another. It is the same "second time"
that a spread already is (`..`): the flow's *recognition period* (F18).

### F08. Law order is implicit, differs by trigger, and depends on file names

**Category:** wrong-result (silent). **Fix lives in:** core (dependency order over
tallies). **Seen in:** 04, 05, 06, 07.

**Repro** (three files, one flow):

```text
// a-reader.ax                           // b-adjuster.ax
law reader                               law adjuster
  each year                                each year
  warn tally(extra) >= 1_000 USD           count 1_000 USD as extra
       "the adjustment was not counted yet"
```

**What happened.** `axiom check` warns "the adjustment was not counted yet".
Rename `b-adjuster.ax` to `0-adjuster.ax` and it passes. The engine runs `each`
and `by` laws in declaration order, project files sorted by path and before the
embedded systems; `on in` and `on out` laws run systems first, then the project.
None of this is in `LANGUAGE.md`.

`05-family` on real money. The itemized-deduction and credit laws must run before
`us` reads `agi`. With the project laws in `laws.ax`:

```text
    taxable-income    167,862.93 USD        income-tax   26,757.84 USD        owed: none
```

Rename the same file to `tax.ax` (now after `systems/us.ax`) and nothing else
changes:

```text
    taxable-income    185,248.00 USD        income-tax   30,582.56 USD        owed 2,988.56 USD
```

3,824.72 USD of tax from a file name, with no message. (`outputs/tax-if-laws-file-were-named-tax-ax.txt`.)

The order also has no hooks. `us` reads `standard-deduction` directly, so
itemizing, QBI, the self-employment credit, the child credit and the stacking tax
can only be added by relying on the order above and by hacking tallies
(`count adjusted - qbi as agi`, `count -se-credit as federal-withheld`), or by
**forking the whole 173-line `us.ax`** (`05-family/systems/us.ax` does, with
`// PROJECT EDIT` markers; its parameter tables go stale the day a new year is
published).

**What should happen.** A law that reads `tally(x)` runs after every law that
writes `x` in that period. A project adds a deduction by writing to a slot the
system declares.

**Proposal (core).** The engine already sees, syntactically, what each law reads
(`tally(x)`) and writes (`count ... as x`). Order laws by that dependency graph
per period, like a spreadsheet; report a cycle as an error naming both laws;
break ties by declaration order (documented). Systems then declare *slots*
(`deductions`, `credits`, `adjustments` for `us`) that project laws count into,
and F19 disappears. No `before`/`after` keywords are needed.

### F09. Residence is one sequential timeline; a part-year residence never files

**Category:** cannot-represent, wrong-result. **Fix lives in:** core (residence
periods and concurrent memberships), system (`us/ca`, `us/abroad`). **Seen in:** 08.

**Repro**

```text
use us
use us/ca
entity me : person
  filing single
  lives us/ca
  lives us from 2025-07-01              // moves out of California on 1 July
2025-03-15 income/salary -> checking 60_000 USD
```

**What happened.** `axiom tax 2025` shows the federal lines only:

```text
  us
    wages           60,000.00 USD  axiom.ax:16
    taxable-income  44,250.00 USD  period end
    income-tax       5,071.50 USD  period end
```

Delete the `lives us from ...` line (never leave California) and the same ledger
adds `ca-taxable-income 54,294.00` and `ca-income-tax 1,792.53 USD`, owed to the
FTB. `us/ca`'s `state-income-tax` is dated to the residence, which ended
2025-06-30; it fires at December 31, so it **never fires** for a residence that
ended earlier. California tax on half a year of California wages is not computed,
and nothing says so. In `08-expat` the same thing happens for
`us/ca/san-francisco`, whose laws also show up in `why girokonto` as still
governing after the move (F34).

Separately, a second `lives` *ends* the first. An American in Berlin is a US
taxpayer everywhere and a German resident: two concurrent things, spelled
today as `lives us/abroad/de` with the FEIE as a system *nested under `us`*
(`08-expat/systems/us/abroad.ax`). The regime elected (FEIE, or not) is a filing
choice, not a place of residence.

**What should happen.** A part-year resident gets a part-year return; a citizen
abroad keeps `us` and adds `de`.

**Proposal.** (core) residences are periods that may overlap, and an `each year`
law fires for every year in which its residence was in force for any day, with
`period` (the residence's days in the year) available; (core) `citizen us` is
separate from `lives`. (system) `us/ca` prorates or, as a first step, simply fires
at the end of each residence with `period end` marked. `elects feie` is a switch a
law reads.

### F10. `available` counts money that is not yours, is not money, or is spoken for

**Category:** wrong-result. **Fix lives in:** report, with kinds declaring what
they are. **Seen in:** every project.

The headline of `axiom available` is "Available to spend". Each project found
something in it that could not be spent:

| Project | Counted as spendable | Why it is wrong |
|---|---|---|
| 04 | Two open invoices 5,800.00 (until I added `liquidity 45d` to the kind); the tax vault 10,420.95 while 4,101.28 of tax is known to be owed | a claim; an obligation |
| 05 | Escrow 1,440.00 (`Money in liquid places`) | the servicer's money |
| 06 | `espp-cash` 8,400.00 | held by the plan for the next purchase |
| 07 | `prepaid/loan-costs` 3,076.65 (an asset that is a cost paid early) | not money |
| 08 | Nothing in EUR: girokonto 7,481.00 EUR and tagesgeld 20,482.76 EUR are `now` liquid but absent from "Available to spend 34,164.69"; only base-currency plain money counts | foreign cash is cash (F14) |
| 09 | The collective's checking 2,654.61 and cash 109.00; Riley's loan 50.00; roommate IOUs 253.55 | another owner's money; claims |
| 10 | Every fund set aside for a trip, the car or the insurance, unless you hack `liquidity 1d` on it | earmarks (F16) |

The hypothetical part ("what it would take to reach the rest") has its own
problems:

- **Costs are computed as if the year ended today, on the year so far.** On
  2026-01-06 the new year is empty, so `06-investor` shows NWND (295.5686 shares)
  as `Value 56,453.60 USD  Costs (blank)  Net 56,453.60 USD` although the 7,397.21 USD
  short-term gain it would realize is taxed at the owner's marginal 24%: the
  year so far has no income to stack it on.
- Holdings are costed one by one, so BTC's -742.13 short-term and ETH's +1,277.45
  do not net (F04).
- `05-family`: the house appears as `Value 612,400.00 USD ... Net 611,232.62 USD`.
  The 406,692.02 USD mortgage secured on it and any selling cost are not part
  of "what it would take": the net is about three times what a sale would leave
  in hand.
- A hypothetical withdrawal of *cash* runs the year-end laws for the person, and
  they answer with nonsense. `08-expat`, `available --at 2025-07-15`:

  ```text
  assets/de/girokonto (2,754.35 EUR)     now   3,215.15 USD    2.48 USD   3,212.67 USD
    counts 24.83 USD as agi
    feie-stacking-tax, owed to irs by 2026-04-15                2.48 USD
    realizes a gain of 3.59 USD
  assets/de/tagesgeld (20,334.10 EUR)    now  23,735.99 USD  -26.41 USD  23,762.40 USD
    counts -357.88 USD as fbar-tagesgeld
    counts -357.88 USD as fbar-aggregate
  ```

  Spending euros costs 2.48 USD of stacking tax, and a *negative* cost of
  26.41 USD for the other account (the memory-cell law of F28 fires on a
  hypothetical). It also says `counts 24.83 USD as agi` and
  `realizes a gain of 3.59 USD` for the same withdrawal.
- Every hypothetical prints the same 3 to 12 `counts X as tally` lines; the
  useful number (cost) is buried.

**What should happen.** "Available" is: what is in hand and free of obligations,
in every currency, after what falls due within the horizon; "coming in" (claims by
due date) and "reach the rest" separate.

**Proposal (report; kinds say what they are).** Liquid is not a property the
report guesses; it is declared once per *holding* (place kind x commodity kind x
owner scope) and shared with `forecast` and `balance` (F20). Add to `std`:
`kind escrow`, `kind claim` (F06), `kind prepaid`, `restricted` for earmarks (F16).
`available` then reads: money in hand (all cash commodities, converted), minus
restricted and earmarked, minus `owe` obligations falling due before a horizon
(default 30 days), "coming in" from claims, and a "reach the rest" that computes
costs on the projected full year (`--year-end`) rather than the year so far.

### F11. A plan silently deletes the recurrence it resembles

**Category:** wrong-result. **Fix lives in:** report (forecast). **Seen in:** 05,
and in the shipped golden.

**Repro** (`05-family/plans.ax`):

```text
/// Alex's bonus, withheld the same way as last year.
every year on 03-14 from 2026-03-14 acme -> 12_000 USD
  alex-401k       1_200 USD
  taxes/federal   2_640 USD
  ...
  joint-checking    ...
```

(`acme` pays `income/alex-salary` into `joint-checking` every two weeks in the
journal.)

**What happened.** The forecast learns recurring flows from history, *except any
`(from, to)` pair a plan already projects*. The bonus uses the salary's places, so
the whole biweekly paycheck drops out of "What recurs":

| Liquid net worth on 2027-01-05 | Committed |
|---|---|
| with the bonus plan | 345,238.48 USD |
| without it (`outputs/forecast-without-bonus-plan.txt`) | 419,387.24 USD |

74,149 USD of Alex's salary vanished, and the only line of "What recurs" for
`income/alex-salary → assets/bank/joint-checking (acme)` is the plan's
`yearly 5,870.40 USD 2026-03-14 plan`. The shipped golden for `02-household`
(`tests/golden/household-forecast.txt`) has the same defect: its "What recurs"
lists the bonus, `income/salary → assets/bank/checking (acme)  yearly  2,005.00 USD
2026-12-15  plan`, and **no paycheck**; liquid net worth falls to -20,587.31 USD by
2027-03-31 and "Problems ahead" reports
`assets/bank/checking is overdrawn, down to -37,892.30 USD` on 2026-06-05.

**What should happen.** A plan adds to what history predicts; it replaces only if
it says so.

**Proposal (report).** Key suppression on the *recurrence* (same pair, same
cadence class, and an amount within a tolerance), not the pair; add
`replaces "salary"` to a plan for the case where it should, and print a note in
`What recurs` ("plan X also matches recurring flow Y: both are projected").

### F12. Forecast "liquid net worth" counts the house and the securities

**Category:** wrong-result. **Fix lives in:** report (forecast). **Seen in:** 05, 06.

`05-family` on 2026-01-05 starts the forecast at `327,537.38 USD` "liquid net
worth". That is 115,608.01 liquid + 612,400.00 (house) + 25,300.00 (car) -
425,770.63 (every debt, including the 406,692.02 mortgage). The house and car count
as liquid because forecast asks only whether the *place* declares liquidity;
`available` also looks at the commodity (`HOME` has `liquidity 90d`) and gets it
right. `06-investor` starts at 313,139.12 USD, which is its net worth
(430,872.40) less only the retirement accounts (117,733.28): the stock, the crypto
and the ESPP cash all count as liquid, because a `broker` place declares nothing,
while `available` lists the stock and crypto under "reach the rest" and offers
164,334.90 to spend. Liquid net worth thus depends on which command you ask.

**What should happen.** One definition of liquid (F20) and, in the forecast, a
label that says what is counted: "liquid assets less debts due within N days".
Counting the full mortgage as a debt against cash that excludes the house is a
category error.

**Proposal.** Share the holding-liquidity function with `available` (F10, F20),
and split debts into "due within the horizon" and the rest, or show
"net of liabilities secured by excluded assets".

### F13. Forecast projects past the end of its reason, and misses standing orders

**Category:** wrong-result. **Fix lives in:** report (forecast). **Seen in:** 06, 07,
08, 09.

- **After the end.** `07-landlord` sold the house and paid off the mortgage on
  2025-12-29. On 2026-01-06 the forecast keeps projecting rent, mortgage,
  depreciation and loan-cost amortization:

  ```text
  Problems ahead
    2026-01-31  assets/prepaid/loan-costs is overdrawn, down to -104.04 USD
    2026-02-01  assets/bank/rental-checking is overdrawn, down to -1,809.59 USD
  ```

- **Beyond the balance.** `09-shared` projects Riley's 250.00 repayment every
  month against a remaining loan of 50.00:
  `2026-01-15  assets/loans/riley is overdrawn, down to -2,950.00 USD`.
- **Phantom deficit.** Checking pays the landlord 1,050.00 USD a month for each
  roommate's share and that is a recurrence (`checking → roommates/ben (landlord)
  monthly 1,050.00 USD`); the roommates' repayments come on irregular dates and
  are not, so the forecast only ever sees the outflow:
  `2026-08-03  assets/bank/checking is overdrawn, down to -7,062.57 USD`.
- **Standing orders that vary.** A monthly 1,500 USD index-fund purchase buys a
  different number of shares each month; a monthly EUR to USD conversion returns a
  different USD amount. Neither is detected as recurring, because the amount on
  one side varies (`06`: no VTI purchase in "What recurs"; `08`: only the
  `girokonto → fx-fees` leg of the conversion is listed, not the conversion). The
  forecast then forgets the biggest monthly outflow.
- **Different things averaged.** Tenant A pays 2,400 and tenant B 2,500; the
  forecast projects 2,400 (`07`).
- **"Committed" cannot be opened.** `05`'s "What recurs" is 42 lines because
  every leg of every paycheck is its own row, and there is no way to see the
  projected flows of one place the way `register` shows the real ones.

**What should happen.** A recurrence ends when its source place or claim ends; a
projected flow is capped by what the source can give; the report should let you
audit any line ("Committed" is a sum you cannot open).

**Proposal (report).** (1) Recurrence keys on the *side that does not vary*
(the cash leg for a conversion or a purchase); (2) stop projecting when the
source/target is closed (zero balance and no inflow for N periods, or a claim
settled, F06); (3) clamp claims and loans at their balance; (4) group a
transaction's legs into one row; (5) `axiom forecast --register PLACE` lists the
projected flows for one place, the way `register` lists the real ones.

---

## S2: wrong or misleading in one report, or a hack a user would not find

### F14. Foreign currency is treated as an investment

**Category:** wrong-result, ugly/verbose. **Fix lives in:** system (`us`, `std`) and
core (a conversion is a flow with a derived cost). **Seen in:** 08, and in the
FX-shaped part of 06 (a crypto swap).

**Repro**

```text
account assets/de/giro : bank
  holds EUR
2025-07-05 EUR 1.0860 USD
2025-07-05 income/salary -> giro 1_000 EUR
2025-08-05 EUR 1.1000 USD
2025-08-05 income/salary -> giro 1_000 EUR
2025-08-10 giro 300 EUR -> groceries               // a week's shopping in Berlin
```

**What happened** (three separate things):

1. The grocery run is an error, and the error calls it a sale:

   ```text
   error[ambiguous-lots]: ambiguous lot: assets/de/giro holds 2 lots that differ and no policy applies
   16 │ 2025-07-05 income/salary -> giro 1_000 EUR
      │   ╰── acquired 2025-07-05: 1,000.00 EUR, basis 1,086.00 USD; from it alone the gain is 4.20 USD
   ...
      = help: name the lot, `assets/de/giro[2025-07-05]`, or a policy, `assets/de/giro[fifo]` ... or give the account `select fifo`
   ```

   The `bank` kind has no lot policy, so *every* spend of euros from two
   deposits at different rates is an error until each foreign account says
   `select fifo`.
2. Once it runs, every euro spent is a disposal of a lot with a USD basis.
   `08-expat`, `axiom tax 2025`: `short-term-gains 27.34 USD  105 sources`; the
   FX spread of each conversion appears as a capital loss; `axiom lots girokonto`
   lists three EUR parcels by acquisition date. For a US person some of this is
   real (section 988) and most of it is not: personal transactions under 200 USD
   are exempt, and a conversion's spread is a cost, not a loss.
3. `available` does not count euros as cash: girokonto 7,481.00 EUR and tagesgeld
   20,482.76 EUR are `now` liquid yet absent from "Available to spend 34,164.69
   USD" (F10); and a hypothetical withdrawal of them prints stacking tax
   (F10, `available --at 2025-07-15`).

Also: `@ 1.0860 USD` on a flow is rejected (`amount-precision`, "USD counts 2
decimal places", with a help line that suggests changing USD globally) while
`EUR 1.0860 USD` on a price line is accepted; a conversion `giro 500 EUR ->
checking 543 USD @ 1.0860 USD` therefore cannot state its own rate.

**What should happen.** A currency is fungible cash: no lots to choose, no gain
per coffee. Converting is a flow whose *cost* is derived and reported (the
difference between the amount received and the market value of what was given,
shown in `flow` as an FX cost); a law may *choose* to tax a currency gain.

**Proposal.** (system) `commodity kind currency` has `select fifo` by default
and a `us` law does not count `on gain` when the disposed commodity is a
currency, except through an explicit `section-988` law with the 200 USD
exemption; (core) a conversion flow `giro 500 EUR -> checking 543 USD` derives
`fx cost = 500 x price(EUR) - 543` on the day, like the `≈` realized gain, and
`flow` and `budget` can show it (`08-expat/wishes.ax.txt`, 4 and 7);
(report) `available` and `balance --value` treat all `currency` holdings
as cash; (core, small) price precision is a property of the price line, so an
`@` price may carry the decimals a price line does.

### F15. `!` does not waive priced violations, and useless waivers go unflagged

**Category:** wrong-result, bad-diagnostic. **Fix lives in:** core, plus one
diagnostic. **Seen in:** 05, 07, 09.

**Repro** (`05-family/journal/2025/11.ax`; `us/hsa.ax` says "keep the withdrawal
and mark it `!` with the reason"):

```text
2025-11-05 hsa -> joint-checking 620 USD ! "reimburses 2025-05-09 dentist, receipt kept"
```

**What happened.** `LANGUAGE.md`: "`!` waives every law violation raised by this
transaction". The HSA law prices the violation with `else owe`. In `axiom tax 2025`:

```text
    us/hsa
      nonqualified-hsa-penalty  irs  2025-11-05    124.00 USD  journal/2025/11.ax:15
```

The 124.00 USD penalty is still owed, and the 620 USD is still counted as a
distribution and as income (`count gain as distributions`), because a `count` is
not a violation. The reimbursement of a real medical bill is both taxed as
income and penalized.

The opposite failure: a waiver that waives nothing is accepted silently.
`07-landlord`: `deposit-account -> rental-checking 350 USD ... ! "kept for damage"`
fires no law (the tie travels with money that stays in the owner's accounts, F16),
so the `!` is dead; `09-shared`: the grant return needed no `!` and I first
wrote one. And a waived warning is printed in full on every run (by design:
"reported, never hidden"): `05-family`'s `axiom check` prints the whole
"529 contributions over the annual gift exclusion" diagnostic, "waived here", for
the two opening 529 balances (one of which, 4,750 USD, is "growth since 2019" and
not a contribution at all: F17, F27), forever.

**What should happen.** A waiver waives what it says. A waiver that waives
nothing warns. And the situation the waiver was for (paying yourself back for a
bill already paid) should not need a waiver at all.

**Proposal.** (core) `!` covers priced (`else owe`) violations too, and
`axiom tax` lists them under "waived" with the reason, so they are never hidden;
(diagnostic) `warning[unused-waiver]`; (core, because it removes the need) the
settlement link of F06: `hsa -> joint-checking 620 USD for #dentist-2025-05-09`,
which lets the HSA law say `when not for is medical` and the count law say the
same. A link is a fact; a waiver is an exception.

### F16. Restrictions cannot be self-imposed, and never lift

**Category:** cannot-represent. **Fix lives in:** core (a restriction is a claim
with a purpose, F06). **Seen in:** 07, 09, 10.

**Repro 1** (`10-budgeter`, reduced to a probe): money set aside for the car.

```text
kind envelope : entity
  restricted
  has purpose kind
  law purpose
    on spend
    require to is self.purpose "envelope money spent on something else"
entity car-envelope : envelope
  purpose car-cost

2025-09-06 checking -> car-fund 100 USD / car-envelope
2025-10-10 car-fund -> dining 20 USD                  // should warn
2025-10-20 car-fund -> repair 150 USD                 // should not
```

`axiom check` is silent about the dining line, and `available` counts the fund:

```text
  Money in liquid places   1,830.00 USD
    assets/bank/checking   1,800.00 USD
    assets/funds/car-fund     30.00 USD
```

A tie is created only when money arrives from an outside restricted entity;
naming the envelope as payee of a transfer between my own accounts creates none.
So a sinking fund, an emergency fund, a tax vault, a trip fund, a "this is the
rent" envelope are all inexpressible. `10-budgeter` marks its funds
`liquidity 1d` to get them out of `available`, which is a lie about liquidity.

**Repro 2** (`07-landlord`): a security deposit.

```text
2025-02-01 liabilities/deposits/tenant-a -> deposit-account 2_400 USD / tenant-a #lease-a
...
2025-09-12 deposit-account -> deposits/tenant-a 2_050 USD / tenant-a #lease-a     // returned
2025-09-12 income/forfeited-deposits -> deposits/tenant-a 350 USD / tenant-a #lease-a
2025-09-12 deposit-account -> rental-checking 350 USD / tenant-a #lease-a ! "kept for damage"
```

The lease is over and the liability is zero, and four months later `available`
still says:

```text
  Tied to restricted entities     -350.00 USD
    tied to tenant-a              -350.00 USD
  Available to spend          173,516.60 USD
```

The 350.00 USD of money the landlord kept is tied to the tenant forever; there is
no operation that releases a tie.

**What should happen.** A restriction has a holder, a purpose, and an end. It
can be laid on by the owner. Its end is an event: settlement of the claim behind
it (F06).

**Proposal (core, dissolved by F06).** "Restricted money" is a claim held
by the owner *against themselves for a purpose*, or by a third party (a
tenant, a grantor). A `for #code` settlement releases the tie; `earmark car-cost`
on a transfer creates one; `available` subtracts open earmarks.

### F17. Opening balances are dated flows, and history before the first parameter row is an error

**Category:** cannot-represent. **Fix lives in:** core (an `opening` statement) and
system (older rows). **Seen in:** 05, 06.

**Repro**

```text
use us
commodity HOME : real-estate
account assets/house : property
2023-06-15 equity/opening -> house 1 HOME @ 540_000 USD     // bought in June 2023
2025-02-01 equity/opening -> checking 10_000 USD
```

**What happened.** The ledger does not check:

```text
error[no-param-row]: cannot check `federal-income-tax`: `standard-deduction` has no row for 2023-12-31
   │ law federal-income-tax
   │            ╰── checked for me on 2023-12-31
   = help: add a row to `param standard-deduction` that starts on or before that day
```

(three times: `standard-deduction`, `ordinary`, `ordinary`). A year-end law
ran for 2023 because a flow happened in 2023, though nothing there is taxable.
The shipped parameters start in 2024, so **no ledger can begin before 2024**, and
neither can a lot be older than 2024. `06-investor` starts in September 2024
and `05-family` restates its opening on 2024-12-31 for this reason.

Opening balances written as flows on the last day of the prior year cause three
more problems in `05-family`: (a) they trigger that prior year's laws (so 2024
needs its own parameter rows); (b) the two 529 opening balances (24,600 USD)
exceed the gift exclusion and warn, even with `!`; (c) the house's acquisition
date is 2024-12-31, not June 2023, so its holding period and section 121
history start on the day the user began keeping books.

**What should happen.** Starting balances are statements, not flows: they carry
a basis and a date, they trigger no laws, and they may be older than the
parameters.

**Proposal.**

```text
opening 2024-12-31
  assets/education/riley-529  = 24_600 USD  basis 19_850 USD
  assets/house                = 1 HOME      basis 540_000 USD  since 2023-06-15
  liabilities/mortgage        = 412_428.22 USD
```

(core) `opening` creates parcels directly with `basis` and `since`; laws do not
see them as flows. (system) a param lookup before its first row uses that row and
prints a note, or year-end laws simply skip a year with no flows in scope. Ship
2020 to 2023 rows: the numbers exist.

### F18. Depreciation, prepaid and annual bills are spread only in `flow`

**Category:** cannot-represent. **Fix lives in:** core (a flow's second time, F07;
scheduled flows). **Seen in:** 04, 07, 10.

**Repro**

```text
account expenses/business/software
  budget 130 USD monthly
2025-02-12..2026-02-11 business-card -> software 119.88 USD / dropbox   // annual plan
2025-02-14 business-card -> software 15 USD / figma
2025-02-22 business-card -> software 21 USD / freshbooks
```

(from `04-freelancer/journal/2025/02.ax`, where February already holds 77.19 USD
of other software.) `..` recognizes the 119.88 over the year in `flow`, but every
other reader sees the payment on the first day. `axiom check`:

```text
warning[law]: law `budget` does not hold for expenses/business/software
 21 │ 2025-02-12..2026-02-11 business-card -> software 119.88 USD / dropbox
110 │   budget 130 USD monthly
    │              ╰── 197.07 USD
    = help: at most 52.81 USD more can go in this month

warning[law]: law `budget` does not hold for expenses/business/software
 27 │ 2025-02-14 business-card -> software 15 USD / figma
    = help: nothing more can go in this month: it is already 67.07 USD over

warning[law]: law `budget` does not hold for expenses/business/software
 35 │ 2025-02-22 business-card -> software 21 USD / freshbooks
    = help: nothing more can go in this month: it is already 82.07 USD over
```

Three warnings for one over-spend that is not one: the annual plan costs 9.99 a
month. Laws (`total(out, month)`), `budget`, `tax` and `available` all read
payment dates. The same in `07-landlord`: depreciation is twelve hand-typed
memo flows a year (`equity/acc-dep-house -> expenses/rental/depreciation`), plus
a half-month in the month of sale, whose order relative to the recapture law
mattered (F23); and loan costs are amortized by hand. `10-budgeter` wants a
139 USD Prime subscription in November to be 11.58 in each month's envelope.

**What should happen.** A flow has a date it moves and a period it belongs to.
Everything that reasons about a period reads the second.

**Proposal.** (core) `total(out, month)`, `tally`, `budget` and `tax` read the
*recognition* of a flow (the spread `..` already defines) rather than its
payment; (core) a **self-posting plan**: a `plan` whose date has passed *is* the
flow unless an actual flow with the same code replaces it, so a depreciation
schedule, a subscription or a standing order is written once:

```text
plan every month from 2025-01-15  house[basis] -> expenses/rental/depreciation
  amount house.basis / 27.5y / 12
```

A later real flow with the same code replaces it and `axiom check` says which
plans posted themselves. This is what the standing orders of F13, the
depreciation of F01, the amortization of a loan and the accruals of an
accountant have in common.

### F19. `us` has no slot for deductions or credits, and no total

**Category:** ugly/verbose, wrong-result. **Fix lives in:** system (`us.ax`), and
F08. **Seen in:** 04, 05, 08.

`us` computes the standard deduction and nothing else. Every project that
itemizes, takes QBI, pays SE tax, claims a credit, or excludes foreign income
hacks the same tallies:

- `04-freelancer/tax.ax`: `count adjusted - qbi as agi` (a deduction as negative
  income) and `count -se-credit as federal-withheld` (SE tax paid out of the
  withholding, so `us` does not credit it twice).
- `05-family/systems/us.ax`: a full fork with `// PROJECT EDIT` lines for
  itemized deductions and the child credit.
- `08-expat/systems/us/abroad.ax`: FEIE and the stacking tax as a second
  `owe`.

And obligations are per law, so the report has no total tax and no refund.
`08-expat`: 10,740.00 USD withheld, income tax 5,104.12 and stacking tax 3,599.64
add to 8,703.76, a **refund of 2,036.24**; the report says
`feie-stacking-tax  irs  2026-04-15  3,599.64 USD` owed (the stacking `owe` does not
see the withholding), and `05-family`'s "refund of 3,036.16 USD is not booked".
`us.ax` itself says so ("A refund ... is not booked").

**What should happen.** A project adds a deduction by counting it into a slot
the system reads, and one report gives the year's tax: liability, credits,
payments, and what is owed or refunded.

**Proposal (system).** With F08's dependency order, declare in `us.ax` the slots
`adjustments`, `deductions` (standard by default, `max` with itemized),
`credits`, `payments`; `federal-income-tax` reads them and owes the net; `tax`
prints "Total tax, paid, owed or refunded" per jurisdiction. The stacking tax,
QBI, SE tax and child credit become five-line laws that count into slots.

### F20. Valuation is decided report by report

**Category:** wrong-result. **Fix lives in:** core (one valuation lens). **Seen in:** 04, 06, 08.

Four different rules for "what is 100 EUR worth":

- `balance --value` values holdings of non-base commodity **at the latest
  price**, including *expense* places: `04-freelancer`'s mileage `880 MI` is
  638.00 USD in `balance --value` but 617.10 USD by the law and in `flow`
  (flow-date prices); `06-investor` values 0.0021 ETH of network fees at 6.51 vs
  3.65 USD at the day they were paid. An expense is a past event and should not
  be revalued.
- `flow` uses flow-date prices.
- Laws use `value(amount, USD)` at the day's price, where **a same-day flow's
  implied price overrides the price line and an `@`**: `06-investor` counts an
  RSU vest as 11,112.01 USD wages, not 11,112.00, because a sell-to-cover the
  same day implies 3,294.71 / 17.79 shares; the DRIP dividend is 123.46 by the
  law and 121.59 by the basis of the shares it bought.
- `axiom balance --value` reports assets 430,872.39 USD against net worth
  430,872.40: a cent of rounding between two sums of the same holdings.

**What should happen.** One valuation, chosen by the report's question (as-of
date for a balance sheet; flow date for a P&L), stated in the header, and price
lines beating implied prices.

**Proposal (core).** A *lens* `(as-of, price-date, owner-scope)` passed to every
report and to `value()` in laws (`value(amount, USD, on flow)` vs
`on today`); an expense place or an equity place is never revalued;
`price` lines outrank implied prices, which are a fallback with a note; sums are
computed from unrounded values and rounded once.

### F21. No owner scope in any report

**Category:** missing-report. **Fix lives in:** report. **Seen in:** 05, 09.

`balance`, `flow`, `register`, `available`, `budget` and `forecast` take no
`--for`. `tax` takes `--entity`. In `09-shared` the collective's grant income
(5,942.75 USD) and garden spending (6,220.94 USD) land in "my" `flow`; the
collective's 2,654.61 USD in "my" `available` (F10). In `05-family`,
`tax --entity jordan` shows one person's return, and nothing else can ask about
one person or about the couple. Any household of two, any person who helps a
relative or runs a small club, has this.

**Proposal.** `--for ENTITY` on every report, default "everything I own"; the
entity kind decides what counts as "mine" (owner sets, F05); an `org` may be
reported on its own.

### F22. Reports refuse to run on any error and do not say how to override

**Category:** bad-diagnostic. **Fix lives in:** diagnostic. **Seen in:** 05, 06, 07.

Any error, even one wrong assertion in 2025-03, aborts `balance`, `flow` and
`tax` for the whole ledger. There is a `--relaxed` flag that prints the report
after the errors and exits 1, and the error output never mentions it. A user
with 400 correct flows and one typo sees only errors. `check`'s own summary line
(`✗ 5 errors`) is the natural place for "`--relaxed` shows the reports anyway".

### F23. Same-day flows: declaration order changes what laws see

**Category:** wrong-result. **Fix lives in:** core (document, then make explicit).
**Seen in:** 07.

**Repro** (`07-landlord/journal/2025/12.ax`): the sale-day depreciation.

```text
2025-12-29 acc-dep-house -> depreciation 456.79 USD     // half a month, before the sale
2025-12-29 -> rental-checking 404_531.25 USD            // the sale; `recapture` reads
  house               1 HOME                            //   self.depreciation.balance
```

With the sale first, the recapture law read the depreciation balance before the
half-month was booked and understated recapture by 478.31 USD; nothing said so.
Flows on the same date run in declaration order, laws read the running balance,
and a user reordering lines inside a day changes a tax figure.

**Proposal.** Either laws that read a balance on `on gain` see the end-of-day
balance (documented and consistent with assertions, which are "end of day"), or
flows on one date may be ordered by `then`. The first is the smaller change and
matches balance assertions.

### F24. Personal-use property: deductible losses, no home-sale exclusion

**Category:** wrong-result. **Fix lives in:** system (`us.ax`, `std`). **Seen in:** 05.

`count-long-term-gains` fires `on gain` for every asset, so a car sold at a loss
lowers `agi`. `05-family`'s `available` says of the 25,300 USD Honda CR-V:

```text
  assets/vehicle/crv (1 CRV)   30d   25,300.00 USD   25,300.00 USD
    counts -8,600.00 USD as agi
    counts -8,600.00 USD as long-term-gains
```

Losses on personal-use property are not deductible. And the house: a 72,400
USD gain on the primary residence is counted in full (`counts 72,400.00 USD as
long-term-gains`); the 250,000/500,000 USD exclusion of section 121 does not
exist. Both are two-line laws on a `kind personal-use` and an `on gain` law
that reads `owner.lived-in` for two of the last five years, but `us` has none.

**Proposal (system).** `kind personal-use : asset` in `std`; `count-...-gains` skip
losses when `from is personal-use`; a `home-sale-exclusion` law with a `param` by
filing status (needs F01 to make the adjusted basis honest).

### F25. A closing statement, a two-source purchase and `all X` cannot be written

**Category:** ugly/verbose. **Fix lives in:** core (syntax). **Seen in:** 06, 07.

Three things a real document shows on one page:

```text
2025-06-01 house 1 HOME -> 431_500 USD          // one price, allocated by the language
  joint     406_500 USD
  closing    25_000 USD
```

```text
error[split-total]: a split's total is stated once
20 │ 2025-06-01 house 1 HOME -> 431_500 USD
   │                            ─────┬─────
   │                                 ╰── a second amount
   = help: state the total on one side; the legs carry the rest
```

`07-landlord`'s sale is written as *net proceeds* (404,531.25 USD) with source
legs for house and roof, then the payoff and costs as separate flows; nothing in
the journal says the sale price was 431,500 USD.

```text
2025-05-01 broker/a all VXUS -> broker/b
error[expected-arrow]: expected `->`, found `VXUS`
```

`all` cannot name a commodity, so moving one holding out of a mixed account needs
a hand-typed quantity (`06-investor`'s ACATS transfer).

An ESPP purchase (cash plus a discount) is one purchase with two funding sources
and needs two transactions (`split-price`: "several legs are in another
commodity"); a trade with a fee and both sides named is `many-to-many`
(`LANGUAGE.md` sends the user to write two transactions). `select hifo` plus a
selector of two lots plus a quantity picks by policy *inside* the selection, so
"x from lot A and y from lot B" is two flows.

**Proposal (core, syntax).** Say what the document says: a header that states the
price once, and legs that allocate it.

```text
2025-06-01 sell house 1 HOME for 431_500 USD
  selling-costs  25_000 USD
  mortgage       276_282.05 USD
  ...            checking
```

(the header states the price; the legs are allocations; the remainder is the
last leg), `all VXUS` in a source, and multi-source exchanges with a priced
role per leg. All of these are the existing pairing rules made total, not new
concepts.

---

## S3: friction and confusing output

### F26. Budgets re-warn on every flow, ignore spreads, and roll nothing over

**Category:** confusing-output. **Fix lives in:** core (budget semantics), report.
**Seen in:** 04, 10.

- **One breach, many warnings.** Once a month is over budget, *every* later flow
  into the envelope warns again with a fresh "already X over" (F18 shows three in a
  row). `10-budgeter`'s 139 USD Prime renewal in November breaks the 90 USD
  subscriptions envelope, and the four ordinary subscription charges after it warn
  as well: five warnings for one event, in a `check` that prints 175 lines for
  nine warnings.
- **No spread.** The envelope sees the payment day, not the recognition of an
  annual plan (F18).
- **No rollover, no sinking.** `budget 150 USD monthly` restarts at zero every
  month; what is not monthly needs a fund and F16's hack. There is no
  `rollover`.
- **No yearly view.** `axiom budget 2025` is an error, although the yearly
  envelopes (gifts, insurance, medical) are the point of a yearly view:

  ```text
  error: expected a month like 2026-03, not `2025`
  ```
- **A budget is also what a law counts.** `05-family`'s `axiom budget 2025-05`
  shows the gift-exclusion law as a row:
  `assets/education/riley-529  gift-exclusion  2025  10,602.11 USD` (no limit),
  which is contributions 3,000 + a gift 3,000 + investment growth 4,602.11 (F27).

**Proposal.** A budget warns once per period, at the flow that crosses it
("envelope broken by X, over by Y"), and is silent until the next period;
`axiom budget` states the level. It reads recognition (F18), accepts `rollover`,
and `axiom budget YEAR` lists twelve months.

### F27. Laws cannot be windowed, scheduled by the calendar, or asked about a subset

**Category:** cannot-represent. **Fix lives in:** core (law windows, calendar
schedules, filtered totals). **Seen in:** 04, 05, 09.

1. **No start date.** `09-shared`'s loan law (`each month`, `when self.balance >
   empty`, `warn total(out, month) >= self.expected`) fires for the month the
   loan was made:

   ```text
   warning[law]: no repayment this month
   17 │   law repayment
      │         ╰── checked for assets/loans/riley on 2025-04-30
   ```

   The loan was made on 2025-04-10; the first repayment is due in May.
2. **One law per due date.** 1040-ES has four installments; each is a separate
   `by DATE` law per year (`04-freelancer/tax.ax`), because a project law's
   `by` expression has no `year`, and June 15 falling on a Sunday means the
   author writes `by 2025-06-16` and a comment. A user who forgets 2026 gets no
   check. `plan every year on 06-15` cannot move to the next business day.
3. **`total()` cannot be filtered.** A law's `when` chooses which flows *fire* it,
   not what `total(in, year)` sums. The 529 gift-exclusion warning sums *every*
   inflow: contributions, gifts, and growth (`05-family`'s opening "growth since
   2019" of 4,750 USD is counted against the exclusion, F15, F17).

**Proposal (core).** `law ... from DATE` / `until DATE` / `from self.since`
(a window in which the law applies); `by every year on 04-15 next-business-day`;
and `total(in, year, from is contribution)`: a filter in the query, so the counted
set is a query over flows (a fact), not a side effect of what fired.

### F28. Memory and spans: a running maximum by tally trick, days abroad by hand

**Category:** ugly/verbose. **Fix lives in:** core (small): tally aggregators;
`days()`. **Seen in:** 08.

FBAR needs the *highest balance during the year*, per account. It can be written,
and it is a pleasant surprise that it can:

```text
account assets/de/girokonto : bank
  holds EUR
  law fbar-max
    always
    let v = value(balance, USD)
    count max(v - tally(fbar-girokonto), empty) as fbar-girokonto     // running max via memory cell
```

but each account has its own copy of the law (four laws named `fbar-max`, so
`why fbar-max` is F30's dead end) with its own tally name, the year-end
aggregate lists them by hand, the value is the day's rate rather than Treasury's
year-end rate, and only the *first change of the year* sets a max, so a balance
carried in from January 1 that never changes is never counted. `available`
also runs it on hypothetical withdrawals (F10: `counts -357.88 USD as
fbar-tagesgeld`).

The physical presence test needs the number of days abroad. A span cannot become
a number, so `days-abroad` is a hand-fed `param` (184 for 2025).

**Proposal (core, small).** A tally has an aggregator: `count v as max fbar` /
`sum` / `last`, and `always`-laws read the balance *at the start of the period*
too; `days(residence, year)` returns a number of days from the residence periods
of F09; then FBAR is one law on the kind `foreign-account` and
`days-abroad` is derived.

### F29. `?` failures cascade into false assertion errors with "-0.00 USD"

**Category:** bad-diagnostic. **Fix lives in:** diagnostic. **Seen in:** 10.

`?` inference is the best thing in the tool, and its own diagnostics are good.
When it cannot solve, the *next* errors are wrong (`checking`, `wallet` and
`dining` declared):

```text
2025-09-01 equity/opening -> checking 1_000 USD
2025-09-02 checking = 1_000 USD
2025-09-03 checking -> wallet ? USD
2025-09-04 checking -> dining ? USD
2025-09-05 checking = 800 USD          // two unknowns between two assertions
```

```text
error[cannot-infer]: cannot infer the amount of this flow
10 │ 2025-09-03 checking -> wallet ? USD          ╰── this amount is unknown
11 │ 2025-09-04 checking -> dining ? USD          ╰── also unknown
   = note: more than one amount is unknown between two assertions on assets/bank/checking; one
           equation cannot solve them all
error[cannot-infer]: cannot infer the amount of this flow      (the same, for line 11)

error[assertion]: assets/bank/checking holds 1,000.00 USD, not 800.00 USD
10 │ 2025-09-03 checking -> wallet ? USD
   │                  ╰── -0.00 USD to assets/cash/wallet
11 │ 2025-09-04 checking -> dining ? USD
   │                  ╰── -0.00 USD to expenses/dining
12 │ 2025-09-05 checking = 800 USD
   │               ╰── 200.00 USD too much: the ledger holds more than this
   = help: if the gap is a genuine externality (a missed transaction), accept it explicitly
12 + 2025-09-05 checking = 800 USD !

✗ 3 errors
```

The assertion cannot be judged until the unknowns are known, yet it is reported as
failing: the unknowns were treated as 0 and printed as "-0.00 USD". Two real errors become three, the third one false, with a bogus
"200.00 USD too much" and a fix line that suggests accepting the gap with `!`,
which would bury the real problem. Add `2025-09-06 checking -> wallet ? USD` and
`2025-09-07 checking = 850 USD` (a `?` that would have to run backwards, since
checking went *up*) and `2025-09-08 checking -> fun ? USD` (no assertion after it)
and the probe prints six errors, two of them assertion failures that are artifacts.

**Proposal.** An unsolved `?` poisons the assertions it depends on: they are
reported once ("not checked: it depends on the unknown at line 12") and never
suggest `!`.

### F30. `ambiguous-law` lists identical names and no locations; the way out is undocumented

**Category:** bad-diagnostic. **Fix lives in:** diagnostic. **Seen in:** 05, 08, 10.

```text
$ axiom why budget
error[ambiguous-law]: `budget` could be `budget`, `budget`, `budget`, `budget`, `budget`,
`budget`, `budget`, `budget`, `budget`, `budget`, `budget`
  = help: write more of the law's path
```

The help is a dead end. With `use us/ca`:

```text
$ axiom why state-income-tax
error[ambiguous-law]: `state-income-tax` could be `state-income-tax`, `state-income-tax`
  = help: write more of the law's path
$ axiom why us/ca/state-income-tax
error[unknown-target]: no place, #code, law or tax line named `us/ca/state-income-tax`
  = help: did you mean `state-income-tax`?
```

The second error sends the user back to the first. The only form that works is
`axiom why us/ca.ax:46` (file and line), which neither error mentions and which
requires knowing where an embedded system lives. `why fbar-max` (`08`) and
`why contribution-limit` (`05`) say the same with 4 and 3 identical names.
`axiom why us` (a system) is `unknown-target` too: a user cannot ask what `us`
is doing to their year, and `axiom why inv-2025-b01` without the `#` (`07`:
`why lease-a`) offers no "did you mean `#lease-a`?".

**Proposal.** List every candidate with its location and a command to paste:

```text
  `state-income-tax` is defined in 2 places:
    us/ca.ax:46      axiom why us/ca.ax:46
    us/ny.ax:44      axiom why us/ny.ax:44
```

and let `why SYSTEM` print the system's laws and what each counted for you.

### F31. Adding an account breaks lines written long before

**Category:** bad-diagnostic, ugly/verbose. **Fix lives in:** core (name
resolution) and diagnostic. **Seen in:** 04, 05.

A place may be written as "any unique suffix of its path". So, in a ledger with
`assets/business`, adding `account expenses/business` turns every old
`checking -> business 100 USD` into an error. In `04-freelancer` this hit
every line that said `business` at once (I renamed the accounts). `axiom check`:

```text
error[ambiguous-place]: `business` could be either of these accounts
15 │ account assets/business
16 │ account expenses/business
21 │ 2025-01-03 checking -> business 100 USD
   = help: write `assets/business` for `assets/business`
21 + 2025-01-03 checking -> assets/business 100 USD
   = help: write `expenses/business` for `expenses/business`
```

The `help` line repeats the same name on both sides of "for". The error appears at
each use, not at the declaration that caused it; `05-family` hit it again with
`interest` (`income/interest` vs `expenses/auto/interest`). Action at a
distance is the price of short names.

**Proposal.** (diagnostic) One error at the *new declaration*: "declaring
`expenses/business` makes `business` ambiguous in 412 places"; (core) `account
expenses/business as biz` aliases, and an exact-leaf match on a *declared alias*
beats suffix matching; (core, optional) when one candidate is the only legal one
for the flow's sides (an expense cannot be a source of a payment from checking)
say so instead of guessing.

### F32. Code rules: one glob, one role, header codes rejected

**Category:** ugly/verbose. **Fix lives in:** core (small). **Seen in:** 04, 05, 07, 09.

- `code NAME on GLOB` takes one glob: `on income/* liabilities/deposits/*` is
  `expected-end-of-line` (`07`).
- The rule that gives a code its meaning also forbids it on the other half of the
  same event (`09`, reproduced):

  ```text
  code expense-*
    on expenses/work-supplies
  2025-02-03 checking -> work-supplies 80 USD #expense-1
  2025-02-03 employer -> checking (80 USD) #expense-1        // the reimbursement due
  error[code-placement]: `#expense-1` may not mark this flow
     = note: it may only mark flows touching `expenses/work-supplies`
  ```

  Without the rule the two flows share the code and `#expense-1 settled` works. With
  it, each event needs two codes (`#expense-2025-06`, `#reimb-2025-06`, and a second
  rule, `09-shared/accounts.ax:206-212`) and a naming convention to keep them
  paired.
- A code on a multi-leg transaction's header is rejected if some legs do not touch
  the rule's place (`05`: `code mortgage-* on mortgage`), so the code goes on the
  principal leg only and `why #mortgage-2025-05` shows only that leg.
- `code inv-* on receivable` prevents tagging the tax-vault sweep with the invoice
  it was made for (`04`), which is correct and leaves no way to link cause and
  effect.

**Proposal.** A code is a tag; a *settlement* is `for #code` (F06). Tags may be
placed on any flow the rule lists (several places or kinds), a header code applies
to all legs, and "should not mark this" is a note, not an error. The relation
between two flows is the `for`, not the code's shape.

### F33. Every payee needs an entity, and a restricted payee forces omissions

**Category:** ugly/verbose. **Fix lives in:** core (syntax). **Seen in:** 05, 07, 10.

`/ trader-joes` must name a declared entity, so each shop is two lines:
`entity trader-joes : org` and `via expenses/groceries`. `10-budgeter` declares 26
payees (52 lines) before its first purchase, and in a real bank feed a new shop
appears weekly. Worse, an `on spend` law on a restricted entity makes *naming*
the entity a semantic act: `07-landlord` leaves the tenant off the rent flows
(`#lease-a` instead) because a tenant `/ tenant-a` would tie the rent money to the
deposit rules, and needs two entities per tenant (the person and the deposit
holder), plus a second `repay` property because a law cannot read the entity's
own `via` (the checker answers that an entity has no `via`).

**Proposal.** Many-to-one payee tables (`payee groceries: aldi kroger
trader-joes`), an undeclared payee as a warning with the declaration as the fix
line (the tool already prints fix lines), and a separate word for "this party is
restricted" from "this is who I paid".

### F34. Register, `lots` and `why` leave things out

**Category:** confusing-output. **Fix lives in:** report. **Seen in:** 04, 05, 07, 08.

- `register` of a liability prints the raw signed balance
  (`-279,000.00 USD`, mortgage in `07`) while `balance` and assertions use the
  display sign.
- `register` of a commodity place shows `1 HOME` with no cost, value or basis;
  `With` names the counterpart account, not the payee.
- `register`'s codes appear to the left of their `Note` header
  (`04-freelancer/outputs/register-business-checking.txt`).
- `lots` hides parcels at face basis: the SEP-IRA's `lots` totals 21,353.79 USD
  of a 29,353.79 USD account (F01), so the report's total does not match the
  balance.
- `why girokonto` lists the San Francisco transfer-tax law as governing after the
  residence ended: the laws table is not date-aware (F09).
- `why <law>` says "Ran 1 times".
- `available`'s hypotheticals repeat 3 to 12 `counts X as tally` lines per holding
  (F10).

### F35. Reports a real user asks for and does not get

**Category:** missing-report. **Fix lives in:** report. **Seen in:** all.

| Report | Who wanted it | What they did instead |
|---|---|---|
| `gains YEAR`: each disposal with acquired, sold, proceeds, basis, gain, term (Form 8949) | 06 investor | `why short-term-gains` lists gains by source line, without proceeds, basis, or dates |
| `claims`: aging of everything owed to or by me, with due dates | 04, 07, 09 | nothing: F06 |
| `afford AMOUNT on DATE`: with and without, funds it would use, lowest balance on the way | 10 | two forecasts, two files and a subtraction (the trip changes June liquid net worth by 500, not 3,000: 13,966.01 vs 13,466.01) |
| `gaps`: every `?`, `!` pad and unknown, with a total | 10 | grep the journal for `!` and `unknown` |
| `grants` / restricted status | 09 | `why riverfront` shows places, not terms or the deadline |
| `rent roll` / per-counterparty statement | 07 | `why #lease-a`, one code at a time |
| FX cost | 08 | derived by hand (F14) |
| Total tax, paid, owed or refunded | 04, 05, 08 | F19 |
| `lots --at DATE`; `budget YEAR`; `flow --for` | 04, 07, 10 | `error: axiom lots has no option --at` |
| `forecast --register PLACE` | 05, 07 | reading 42 lines of "What recurs" |

---

## S4: small things

**F36.** (Each is one line of repro and one line of fix.)

- **Blames the wrong line.** `commodity EUR : currency` in a user file, with
  `std.ax` declaring it too: the diagnostic marks `std.ax:164` "declared again
  here" and the user's line "first declared here". The user cannot edit `std.ax`;
  the error should point at their line and say "`std` already declares `EUR`".
- **`unset-property` triplicate.** `lives us` without `filing` prints the same
  error three times, dated `2024-12-31`, for a ledger with no income at all
  (the first year-end law found any flow).
- **`self` means two things.** In `each`/`by` laws it is the governed thing; in a
  top-level law it is the resident entity. `06-investor`'s wash-sale law used
  `self is wash-adjustment` (tests the *person*) instead of `from is`; a bare
  `self` on the right of `is` is `unknown-name`.
- **Zero tallies vanish.** `axiom tax 2025` with 10,000 USD of wages shows no
  `taxable-income` or `income-tax` line: a zero is indistinguishable from "the law
  did not run".
- **Empty column.** `flow --by year` shows a 2024 column with `Net 0.00` when the
  only 2024 flow is an opening balance.
- **`@` precision.** `@ 1.0860 USD` is `amount-precision` ("USD counts 2 decimal
  places") and the help suggests changing USD everywhere; a price line takes 4
  decimals (F14).
- **Sign-less balances.** `balance assets/roommates` prints `ben 37.21` with no
  hint that Ben owes me; an asset place going negative (I owe Ben) is accepted with
  no kind warning (F06).
- **`Ran N times`.** `why <law>` on a law that ran once prints "Ran 1 times".

---

## The five deepest problems

Most of the 36 findings are symptoms of five structural gaps. Each has a formulation that
dissolves several at once and keeps to the philosophy: flows remain the only facts,
parcels remember provenance, kinds type everything, systems stay written in Axiom.
Each needs one small addition to the engine; almost everything else becomes a
law, a kind or a report.

### 1. A parcel cannot change after it arrives

**Dissolves:** F01, F02, F03, F14, F17, F24, half of F18 (and the workarounds
`equity/wash-sale`, `ROOF`, `equity/acc-dep-*`, restated lots, `two-transaction
ESPP`, `zero-basis 529 gift`).

**Formulation.** Depreciation, improvements, wash sales, ESPP discounts,
deducted contributions, gifts, splits, spin-offs, opening balances, exchange
spreads and FX costs are all **flows in a holding's quantity or basis dimension**.
A parcel carries three things the ledger reasons about (how many, what it cost,
since when). Today the first arrives with the flow and the other two are derived
from *the route it took*, and nothing can change any of them afterwards. Make each
dimension something a flow can move, and every one of the workarounds is a flow
between a parcel and the P&L, or between two parcels, that `why` can explain and
that `flow` and `tax` see the same way.

```text
2025-05-22 fidelity split FAST 1:2                                       // quantity: restate flow
2025-09-15 rental-checking -> house[basis] 14_200 USD / summit-roofing   // basis: capitalize
2025-01-31 house[basis] -> expenses/rental/depreciation 456.79 USD       // basis: depreciate
kind sep-ira : tax-deferred                                              // arrival: a kind default,
  arrives basis 0                                                        //   overridable per flow
2025-05-16 gifts -> riley-529 3_000 USD basis 3_000 USD
```

What the block uses is the engine's part (a restate mode, `PLACE[basis]` as the
place of a dimension, `arrives basis`, an `opening` statement); what follows from
it is Axiom: the wash-sale law that *derives* an adjustment flow (shown with the same
`≈` realized gains use), recapture as a read of `house[basis]`, the section 121
exclusion, the section 988 exemption. The asset, the P&L and the tax gain agree
because they are the same double entry.

### 2. Obligations and restrictions are conventions, not things

**Dissolves:** F06, F10 (half), F13 (half), F15, F16, F32, F35 (aging, grants,
afford).

**Formulation. Receivables, IOUs, deposits, loans, reimbursements, grants, checks
in the mail, escrow, prepaid costs, sinking funds, envelopes and ties to a
restricted entity are all one thing: a claim** (an amount held or owed, between
two parties, for a purpose, by a date), **settled by a flow that says it is `for`
that claim.** A pending flow is a claim that has not reached a place yet; a void is
a write-off; a *restriction* is a claim held against oneself or a third party, and
it ends when the claim does. Five of the seven projects invented a private
convention for this (a `receivable` kind with a law on `out`; an asset place per
roommate; a `repay place` and a restricted entity; a fund with `liquidity 1d`; a
`#reimb-*` code convention), and none of them shows up in `balance`, `available`
or `forecast` as what it is.

```text
2025-03-01 acme -> receivables 4_800 USD due 2025-03-31 #inv-1           // an invoice
2025-04-20 receivables -> checking 4_800 USD for #inv-1                  // its payment (partial is fine)
2025-02-01 tenant-a -> deposit-account 2_400 USD owes tenant-a #lease-a  // a deposit I hold
2025-04-10 checking -> loans/riley 2_000 USD due 250 USD monthly #loan   // a loan
2025-09-06 checking -> car-fund 100 USD earmark car-cost                 // an envelope
2025-11-05 hsa -> checking 620 USD for #dentist-2025-05-09               // a reimbursement
```

The engine learns *terms on a parcel* (counterparty, due, purpose) and the
`for` link. Laws and reports do the rest: cash-method income counts on the `for`
flow; `check` warns on a due date passed; `available` subtracts what I owe and
lists what is coming in; `forecast` puts claims on their due dates and stops when
they end; `axiom claims` is a group-by.

### 3. "Who" is a single value on a single line

**Dissolves:** F05, F09, F21, half of F10, F14's `citizen` half, the `us/abroad/de`
nesting.

**Formulation.** Owner, residence and filing unit are treated as one value each,
in sequence. Real ledgers have *sets and overlaps*: a joint account has two owners;
a household files one return for two people; a citizen abroad is a US taxpayer,
a German resident and an FEIE elector at once; a part-year resident owes two
states; the collective's account, the servicer's escrow and the roommate's IOU are
in the file but are not mine. **A party is a set of persons with shares, a person
is a set of dated memberships that may overlap, and a flow is attributed to the
person who earned it, not to the account it landed in.**

```text
entity household : household
  members alex, jordan
  filing joint
account assets/bank/joint : bank
  owner alex 50%, jordan 50%
entity bluefin : employer
  employs jordan                        // its pay is Jordan's wherever it lands
entity me : person
  citizen us
  lives us/ca until 2025-06-30
  lives de from 2025-07-01
  elects feie
```

with `--for ENTITY` on every report. Owner sets and attribution are engine;
`household`, `citizen`, regimes and the state laws stay in `std` and `us`.

### 4. Time has one axis

**Dissolves:** F07, F18, F26, F27, F28, half of F13 and of F09.

**Formulation. Depreciation, spreads, accruals, prepaid costs, subscriptions,
standing orders, loan amortization, "this payment is for last year" and a
part-year residence are all a flow with a period of its own, or a plan that
posts itself.** A flow has a date on which money moved and a period it belongs to;
`..` already defines the second for `flow` and nothing else reads it. Tallies,
budgets, law windows, `total(out, month)`, tax years and residences should all
read the period; and what happens on a schedule should be written once, as a plan
that becomes the flow when its day passes unless a real flow with the same code
replaces it.

```text
2026-01-15 checking -> taxes/federal 3_000 USD for 2025          // moved 2026, belongs to 2025
2025-02-12..2026-02-11 card -> software 119.88 USD               // recognized over a year: budgets see it too
plan every month from 2025-01-15 post house[basis] -> expenses/rental/depreciation
  amount house.basis / 27.5y / 12
law repayment
  each month from self.since                                     // a window
```

The period and the self-posting plan are engine; depreciation tables, safe-harbor
schedules, amortization and the FBAR max are Axiom on top of them.

### 5. Laws are a list, and every report has its own idea of value and liquid

**Dissolves:** F08, F19, F20, F23, F12, half of F10, and the forks of `us`.

**Formulation.** The engine already sees what each law reads (`tally(x)`) and
writes (`count ... as x`), yet runs laws in a hand-made order that depends on the
trigger and on file names, so a system can be extended only by copying it. And
each report re-answers "what is liquid", "what price", "what year", "whose": four
answers for a euro in F20, two for a house in F10 and F12. **Laws are a dataflow
graph over tallies with named slots; reports share one lens (as-of, price date,
owner scope, what counts as cash) and one tax position (liability, credits,
payments, owed or refunded).**

```text
system us
  slot deductions                        // standard by default; itemizing counts into it
  slot credits
  slot payments
law itemized                             // a project file; the file's name no longer matters
  each year
  count itemized as deductions           // the engine runs this before federal-income-tax reads it
```

```text
axiom available --for household --lens flow-date
```

Dependency order and slots are engine (a spreadsheet's order over `count` and
`tally`, a cycle being an error naming both laws); the lens is one function shared
by every command; `us` becomes shorter, because itemizing, QBI, the SE credit, the
child credit and the stacking tax are five-line laws that count into slots rather
than tallies hacked into `agi` and `federal-withheld`.

### An order of attack

The claim and parcel-dimension work (1 and 2) unlock the most: they remove a
workaround from every one of the seven projects and turn four of the largest
silent errors into something the user can state. The system, report and wording
fixes need no engine work and can go first: **F04** (two lines in `us.ax`), **F03** (a
`market` kind and one `when`), **F24**, ship 2020 to 2023 parameter rows (**F17**),
**F30** and **F22** (wording), and **F11** (suppress on recurrence, not on pair).

---

## Authoring friction

Seven ledgers, 4,100 journal lines, 2,300 lines of accounts, laws, plans and
prices. Roughly where the time went, and what a first-time author would trip over.

### What was tedious

- **The journal is written by a generator.** I wrote a Python generator for every
  project, because nothing in Axiom says "the same thing every two weeks". A
  paycheck is one 6 to 9 leg statement (gross, 401(k), health, federal, state,
  payroll, dependent-care FSA, net) and there are 26 a year; a mortgage payment
  is a three-leg flow (escrow, interest, principal) whose split changes every
  month and must be computed by the user; a credit-card statement is 30 flows.
  `plan` exists, but a plan is a forecast and is never the flow; and `sync ... run`
  can regenerate a file from a script (`07-landlord` uses it for prices), but there
  is no rule table that turns a bank export into flows. A new user with a CSV has
  no path.
- **Payee ceremony.** 26 `entity ... via ...` pairs before the first grocery
  purchase in `10-budgeter` (F33).
- **Amortization by hand.** Interest, principal, escrow and the payoff in
  `07-landlord`, computed in Python and typed in; the loan's `rate` property is
  read by nothing, so a wrong split is never questioned.
- **Statements are the checks, and they are typed too.** Every month-end
  `checking = X USD` is a copy of a number from a bank statement; that is the
  point, and it is also why one wrong flow makes every later assertion fail, and
  F29 makes it worse.
- **Names.** Every new account risks an ambiguity in old lines (F31); every
  system law is reachable only by the file and line (F30); every account of a
  foreign currency needs `holds EUR` and `select fifo` (F14).
- **Laws for things the language should know.** An FBAR law per account (F28),
  one estimated-tax law per due date (F27), a year-end law that hacks `agi`
  (F19), a forked `us` (F08). The cliff from `use us` to "I need itemized
  deductions" is a 173-line copy: progressive complexity stops at the first real
  tax return.
- **Reading the output.** `available` prints 3 to 12 `counts X as tally` lines per
  holding, `forecast` prints 42 rows of "What recurs", and there is no way to ask
  for the one number wanted (F10, F13, F35).

### What a first-time author gets wrong first, in order of likelihood

1. **Adds an account and breaks old lines** (F31): `business` becomes ambiguous.
2. **`lives us` with no `filing`**: three identical, confusing `unset-property`
   errors dated `2024-12-31` for a ledger with no income (F36).
3. **Spends euros from a plain `bank` account** and is told about "ambiguous lots"
   for a grocery run (F14).
4. **Puts a top-level `on in` law over a transfer between their own accounts and
   nothing happens.** Project and jurisdiction laws do not fire for flows between
   the owner's own asset places; the law must live in the kind or place. Silent,
   and documented only in `PLAN.md`.
5. **Names a law file `tax.ax`** and gets a different tax bill (F08).
6. **Writes `self` in a top-level law** and tests the person, not the thing (F36).
7. **Expects `!` to waive a priced violation** because the shipped `us/hsa.ax` says
   to (F15).
8. **Pays estimated tax in January** and sees the previous year's tax unpaid (F07).
9. **Enters an invoice as pending** and expects it on the balance sheet (F06).
10. **Tries to write a split** (F02), a **closing statement** (F25) or **`all X`**
    with a commodity (F25).
11. **Writes a market loss as a flow to `income/growth`** and receives a penalty
    (F03).
12. **Puts an `@` rate with four decimals on a USD amount** (F14, F36).
13. **Starts the ledger in 2023** and every command says `no-param-row` (F17).
14. **Gives a joint account to one spouse** and finds the other spouse's paycheck
    split between two tallies (F05).
15. **Puts a code rule on one place** and cannot put the code on the pending
    reimbursement (F32).

### What a cleverer surface would look like

None of these is a new concept; each is the existing pairing and law machinery
with a shorter way to write what a user means.

```text
// 1. A paycheck is a template, and a standing order is a plan that posts itself.
stub acme                                  // written once
  gross 6_000 USD
  401k 600, hsa 250, health 212.50
  federal 700, state 300, payroll 459
  net -> joint-checking
plan every 2 weeks from 2025-01-03 post stub acme

// 2. A loan writes its own flows; the statement only has to agree.
loan mortgage 279_000 USD 6.125% 30y from 2024-12-18 escrow 690 USD
2025-02-01 checking pays mortgage        // splits interest, principal and escrow; warns if the bank disagrees

// 3. Payees are a table, not a ceremony.
payee groceries: aldi kroger trader-joes
payee subscriptions: netflix spotify apple

// 4. A sale reads like the closing statement.
sell house 1 HOME for 431_500 USD on 2025-12-29
  selling-costs 26_968.75 USD
  pays mortgage
  ... rental-checking

// 5. A bank export has rules, and unmatched lines are visible.
import "export.csv" as checking
  when payee ~ "TRADER JOE" -> groceries / trader-joes
  otherwise -> ? (listed by `axiom gaps`)

// 6. Aliases beat suffix luck.
account expenses/business as biz

// 7. A statement can adopt a value.
2025-03-31 k401 = 24_600 USD via income/growth

// 8. Ownership reads like the sentence.
account assets/bank/joint : bank
  owner alex 50%, jordan 50%
```

`stub`, `loan`, `payee` and `import` are macros over what exists (a flow with legs,
a law on a kind, an `entity ... via`, `sync ... run`). The rule that keeps them
inside the philosophy: *each expands to journal lines the user can see with
`axiom expand`*, so the flows stay the only facts.
