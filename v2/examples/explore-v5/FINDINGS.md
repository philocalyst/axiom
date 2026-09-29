# What Axiom v4 still cannot say

Lane X5. Five ledgers, written against `LANGUAGE.md` v4 as a careful user would, with real-looking
numbers, in the sketch's layout. The toolchain is not runnable, so nothing was executed: every finding
is a line in a ledger where the spec could not say what was needed, said it badly, contradicted itself, or
left a person typing what the book should derive. Each ledger marks the line, in a comment, with the id
used below.

| ledger | who | span | journal lines |
|---|---|---|---|
| `01-agency` | a Seattle design agency, LLC 60/40, 30 clients, payroll, 1099, sales tax, three cards | 2026-01..06 | 460 |
| `02-family` | marriage, baby, CA to NY, job change, car, ARM and refinance, claim, HSA, 529, FSA | 2026-01..12 | 274 |
| `03-triplex` | a landlord in one of three units, property manager at 8% | 2026-01..09 | 109 |
| `04-nomad` | US citizen paid in EUR, GBP, USD; Wise, Stripe, PayPal, staking; Austin to Lisbon | 2026-01..08 | 190 |
| `05-budgeter` | zero-based budgeter, envelopes, roommates, cash back, miles, BNPL | 2026-01..07 | 183 |

Marks in the ledgers: `✗` cannot say, `✎` says it badly, `?` unclear or contradictory, `⇄` what sync would
need. `▸` is what Axiom would derive, as in the sketch. One deviation from the brief: a refinanced loan cannot
reset within the year, so 02 has the ARM reset (March) first and the refinance (May) second. Both are used.

## What the spec already says well

Things the ledgers needed and found, so they must survive.
- **Items that carve claims.** `1_050.00 USD for ben` under a rent (05), `39.25 USD for jo` under a dinner:
  a share and its claim in one line, and Venmo settling several claims oldest first.
- **A fee as a template item.** `- 8% #management via elm-pm` in a lease (03) is the whole property manager.
- **Terms that change.** The internet promotion, its extension, the price rise (05), a lease renewal (03), a
  rate reset (02), a free month by `waived`: each one line, on its day.
- **A loan at 0%** is Klarna's four payments (05); `covers 1y` spreads an annual prepayment for the budget.
- **Bills owed with items.** A payroll deposit as `loomfield owes irs … ^eftps-2026-01`, one item per
  employee, settled by the `eftps` occurrence (01).
- **`until` on a statement** for a trip (`me in ch until 07-24`), and `budget dining empty … until` with `!`.
- **The purpose's object** (`#repair of unit-c`, `#wages of maya`) is the general "about" relation.

---

## 01-agency

**(a) Cannot say**
- **01a1** A 60/40 owner. `owner` takes one entity (§4); Theo owns nothing here. A business's tallies also
  never say how they reach its owners' returns (§6: "tallies belong to owners"): no K-1.
- **01a2** A flow between owners: monthly draws (a contract needs a `with` party), Theo's conference on his
  own card `for loomfield`, a household run on the company card `for theo`, the reimbursement (no purpose).
  `#distribution` hangs from no root.
- **01a3** Withheld tax and sales tax collected: money in the bank that is owed to a party. No root purpose,
  and nothing ties the cash. Wrote root-less purposes and `for wa-dor` on items.
- **01a4** A reserve ("30% of what arrived stays here"): a hand transfer of a guessed `about` amount.
- **01a5** A retainer is an invoice, due net 15, not a receipt. Written as a receipt; in accrual books its
  income lands on the payment day, and it is "late" for two weeks every month.
- **01a6** Sales tax collected is an item on 7 of 10 retainer contracts and every invoice. Its rate is
  Seattle's, by date; whether it applies is the client's `place` and `exempt`. Typed each time.
- **01a7** Payroll: 12 lines a month (withholding, both FICAs, FUTA to 7,000 per employee, SUTA, WA Cares),
  all derived from gross, the W-4 and params. The FUTA base is a per-employee running total.
- **01a8** `for last month` (the excise return), and estimates whose fourth installment (01-15) is for the
  year before. A contract has no `for`; each occurrence is typed with its period.
- **01a9** Amending a claim: a credit note (wrote a claim *against* me, then `waived` after payment), a late
  fee (typed from the terms), a write-off (no `#bad-debt`, and 388.12 of tax in it is recoverable).
- **01a10** Billing a cost paid for a client: `for halcyon` makes a claim of 1,840; the invoice's item repeats
  it, and halcyon owes it twice.
- **01a11** An opening claim has no items, so nothing says how much of it is sales tax.

**(b) Says it badly**
- **01b1** A cost re-billed at cost is income *and* spending; `#reimbursed-costs : income` is a lie.
- **01b2** `merchant` has no purpose: 21 of the 33 one-off card charges carry a written one.
- **01b3** A list declaration shares every property, so 30 clients need thirteen declarations.
- **01b5** `quarterly on 04-30` fixes the day at the 30th (07-30, not 07-31); there is no "last".
- **01b6** `yearly on 04-15, 06-15, 09-15, 01-15` reads as one date repeated (§5 lists only days).
- **01b7** One purpose per party kind: premiums out and claim payouts in share `insurance`.

**(c) Unclear or contradictory**
- **01c1** `books cash|accrual` is in §7's prose and not in §4's property table.
- **01c3** `#wages of maya`: §2 and §8 show `of` for assets only. It is used with an entity throughout.
- **01c4** `kind processor … purpose fees`: what is money *from* Stripe? §2's rule makes it a refund of fees.
- **01c5** `covers the month`: §5 writes `covers the year`, the sketch reads it as twelve months from the day.
- **01c6** A Stripe payment with `- fee` items settles the invoice by the header (2,648.40) or by what
  arrives (2,571.30)? §7 says "the flow".

**(d) Sync would need**
- **01d1** The name as the default `known-as`: 27 of 30 clients would need no line.
- **01d2** One export, three cardholders: a column that routes a row to an account.
- **01d3** Stripe's payout as one document: gross, fee and payout, printed as flows.
- **01d4** The invoicing export must print amendments (credit note, fee, write-off), not only `owes`.
- **01d5** The payroll run against a bank line: two ACH lines, one flow of eleven legs.
- **01d6** A retainer paid 18 days after the 1st is nearer next month's due day (§5 half a cadence).

## 02-family

**(a) Cannot say**
- **02a1** A dependent-care FSA and its rule (limit, use it or lose it): no system, no account kind.
- **02a2** A household whose members join in June but whose return is for the year. Tallies were counted for
  each alone until then; `filing` and `children` are "read on the day" (§3), not at the year's end.
- **02a3** Withholding legs from salary and W-4. A raise, a move or an election restates six by hand.
- **02a4** Proration: the first paycheck (09-15), the first month of rent (17 of 31 days), daycare (21 of 30).
  An amount on the occurrence does not rescale constant legs; a `from` mid-month with `on 1` skips the month.
- **02a5** Interest follows the use of a loan's proceeds: 20,000 of the refinance is cash out spent on a
  wedding, and `loan … for home` is true of 460,039.99 of it.
- **02a6** Replacing an account (the joint one on 06-13) across every contract that names it: four statements.
- **02a7** A terms statement with legs (drop `ftb`, add `dtf`, add `dcfsa`): §5 gives legs to occurrences only.
- **02a8** Owner to owner: `jamie-checking -> me-checking`, and funding the joint account (an owner that is a
  household). What is it: a gift, a loan, a contribution?
- **02a9** A trade-in: proceeds that never arrive as money (`- 11_000.00 USD #sale of car1`), and a sales tax
  on the price less the trade-in, which the derived tax would compute on the whole price.
- **02a10** Wedding gifts: not income, not spending; no root purpose fits.
- **02a11** An HSA distribution reimbursing an expense paid out of pocket in May. Nothing links the two, so no
  law can say "qualified".
- **02a12** Vesting: 2,940 of unvested match forfeited on leaving is a schedule, not an event.
- **02a13** Negative items in an `owes` statement (the hospital's insurance adjustments): §2 shows them for flows.
- **02a14** A plan that pays a claim only up to its balance (the FSA): a claim capped, a remainder forgiven.

**(b) Says it badly**
- **02b2** `state-farm : insurer`: premium out, claim payout in (7,940 had to be `#repair of car2` by hand).
- **02b3** `sales-tax` on the store: the same Target is in Oakland and then Manhattan, and sells food.
- **02b4** `yearly on 04-10, 12-10` again, and each installment covers the half-year that ends on it.
- **02b5** A split with only a source ends in a dangling arrow: `20 lumen 9_200 USD ->`.

**(c) Unclear or contradictory**
- **02c1** `lives` "may overlap" (§4) and a later statement "overrides" (§3): does the move end California?
- **02c2** `ssa`, `edd`, `nyc-dof`, `ny-pfl`: which are tax authorities and which purposes, is std's business.
- **02c3** `deposit` is "a claim the party holds, and money held for it": for a tenant it is the opposite
  direction, and paid before the lease's `from`.
- **02c4** An occurrence with a tail (`01 daycare for benefit-admin ^dc-1201`): §5 gives `DATE NAME [AMOUNT]`.
- **02c5** `owe` (§6) and claims (§7): does a refund settle a negative `owe`, and for which year?
- **02c6** A year-closing law of a system an owner has left (`us/ca` on 2027-04-15): governed by whom, and how
  is the part-year income divided?

**(d) Sync would need**
- **02d1** A refund memo has no year; only the return that produced it does.
- **02d2** Paystub PDFs: print `15 job-jamie` when the stub is the contract's, its legs when it is not.
- **02d3** The refinance: the bank shows one wire, 12,722.25, which is one *leg* of a flow.
- **02d4** "REIMBURSEMENT SMILE DENTAL 2026-05-12": the memo picks an earlier *flow*, not a party.
- **02d5** The FSA portal's claim numbers, for claims the book wrote as `for benefit-admin`.

## 03-triplex

**(a) Cannot say**
- **03a1** A building of units. What was needed: `share 1_050 sqft` on a part, and a bill `of` the whole divided
  by weight. Wrote: units as *owners* (so `business N%` can carry a share), a share typed twice (32.81%,
  31.25%) on the building, the price split by area by hand in the opening, and the boiler split by items.
- **03a3** A late fee is a term of the lease (5% after 5 days): `check` says late, the fee is typed.
- **03a4** Utilities re-billed: 32.81% and 31.25% of each quarterly bill, a claim on each tenant, typed as items.
- **03a5** A vacancy is a contract's absence. Nothing reports it, and the forecast loses the unit's rent.
- **03a6** Deposit deductions: `- 320.00 USD #cleaning` under the deposit's return. Whose income is it, for what,
  and does it release the tie? (Wrote it; it reads well and means nothing yet.)
- **03a7** Prorating the first month (05-15, 17 of 31 days) by hand.

**(b) Says it badly**
- **03b1** A purchase whose header and items carry the same purpose (`#improvement of unit-c`).
- **03b2** `yearly on 01-31, 07-31`, `quarterly on 02-10` (the anchor month is in the day).

**(c) Unclear or contradictory**
- **03c1** Two `business` lines on one asset: §4's table gives one `PERCENT for OWNER`.
- **03c2** How do `rental-b`'s tallies reach Marta's return? §6 says tallies belong to owners.
- **03c3** `covers 1y` or `covers the year` for a policy that starts 06-01?
- **03c4** `deposit` held where? The manager's trust, and §5 names no holding.

**(d) Sync would need**
- **03d1** The manager's CSV has a unit column and a category: `object "Unit"` on a `csv`.
- **03d2** Utility PDFs: the script cannot read the units' areas to print the tenants' shares.
- **03d3** Tenant ledgers (late fee posted, reimbursement billed) as `owes` statements with items.

## 04-nomad

**(a) Cannot say**
- **04a1** FEIE, foreign tax credit and FBAR (`us/abroad`) and Portugal (`pt`): systems, not language.
- **04a2** The physical presence test: `days(me.in is foreign, window)`, a window the book picks, judged at
  its end (2027-02-28): the 2026 return must be forecast. (First exploration F28, still open.)
- **04a3** FBAR: a `foreign-account` kind, and the highest balance of the year: a running maximum.
- **04a4** Four installments that are not a cadence, the last for the year before (= 01a8).
- **04a5** Stripe's schedule is conditional (card country, currency): items are per-flow literals.
- **04a6** A sprint invoiced `for 2026-02-16..2026-03-15`, half of it earned in Austin and half in Lisbon.
  Foreign earned income is by the day the work was done.
- **04a7** A chargeback, and its reversal: income reversed, the invoice open again, the 15.00 fee real.
- **04a8** The VAT refund at the border lowers the laptop's cost by 272.90 and costs 75.39 of fee.

**(b) Says it badly**
- **04b1** A citizen's obligation is written `lives us`, kept true for the whole book.
- **04b2** A stablecoin is a `crypto` that is money: every swap into it is a disposal of cents.
- **04b3** VAT as a store's `sales-tax`: nothing says it is recoverable.

**(c) Unclear or contradictory**
- **04c1** `lives us` and `lives pt` overlap (§4); does "a later statement overrides" (§3) apply?
- **04c2** `purpose fees` on a processor, both ways (= 01c4).
- **04c3** The basis of an arrival with no cost in a non-base commodity: a reward, USDC paid by a client.
- **04c4** Gas paid in ETH on a swap USDC to ETH: an item in a commodity other than the header's.
- **04c5** A tenant's deposit, two ways: `deposit` on a contract, or a flow with `due`.

**(d) Sync would need**
- **04d1** The card memo carries the store's price and currency: `original`, and a recognizer for it.
- **04d2** Wise: a `currency` column, an `id` that ties the two rows of a conversion, a `fee`.
- **04d3** PayPal: `gross`, `fee` and net on every row.
- **04d4** Staking rows and the price of the day; the wallet's explorer export with gas.

## 05-budgeter

**(a) Cannot say**
- **05a1** A rule of an account kind that derives flows: 2% cash back on everything; 1 mile a dollar, 2 on
  Delta. Wrote the rule as a comment and the rebate by hand. `#rebate` hangs from no root.
- **05a2** Miles realize nothing: redeeming 25,000 for a 312.00 flight is not a gain (basis 0, `on gain`).
- **05a3** "Every dollar assigned": a law over the sum of budget limits. A budget is a `warn`, not data.
- **05a4** A 100.00 gift card bought for 90.00: the 10.00 has nowhere to be written.
- **05a5** A pro rata refund of a prepaid year (NYT, 67.12): recognition should stop on that day.

**(b) Says it badly**
- **05b1** A sinking fund twice: a `carries` budget and legs into an envelope, nothing says they agree.
- **05b2** Moving 40 between budgets for a month: two statements that must sum to zero.
- **05b3** The internet's declaration says 85 and the promotion overrides it from day one.

**(c) Unclear or contradictory**
- **05c1** `kind envelope` is assumed: §4 says `restricted`, §8 says envelopes, neither declares them.
- **05c2** Does §13.2 reconcile a record with a *derived* flow (the rebate, the miles)?

**(d) Sync would need**
- **05d1** The card's `Category` column as an offered purpose for a merchant nobody is known as.
- **05d2** Venmo: `From`/`To` as the party, `Funding Source` as the account, `Note` as the description.
- **05d3** A miles earn matched to the charge that caused it.

---

## The twelve proposals

Ranked by how many ledgers need each. Every one follows DESIGN §1 to §5, and none brings back an account
for meaning: identity stays in the money, the transactions and the promises.

### 1. A code is a document (5 ledgers: 01, 02, 03, 04, 05)
```text
12 ^inv-2026-0002 credit 993.15 USD "credit note CN-0002"        // amends: what is owed falls
  - 900.00 USD #design
  - 93.15 USD #sales-tax-collected for wa-dor
13 ^inv-2026-0006 + 79.45 USD #late-fees "1.5% after 03-12"      // amends: it grows
18 ^inv-2026-0009 waived #bad-debt "Sable Hotels, chapter 7"     // forgiven, and why
10 nyt -> card 67.12 USD against ^nyt-2026                       // refunds it in proportion
08 hsa -> jamie-checking 620.00 USD against ^bill-0512           // pays it again: reimburses
contract halcyon-retainer with halcyon
  4_500 USD monthly on 1 invoiced due 15d                        // each occurrence is `halcyon owes … due 15d`
```
A code marks a document (an invoice, a bill, a purchase, a plan claim), and everything about it carries the
code. A statement on it with items amends its open claim, item by item, so tax and fee reverse exactly;
`waived` takes a purpose. A flow `against` it is about the earlier flow: in the opposite direction it refunds
it proportionally (recognition stops on that day, an asset part's cost falls, a dispute and its reversal are
two such flows); in the same direction from another source it reimburses it, and laws read `against.purpose`.
An `invoiced` contract states its occurrence as a claim, settled by the payment (§7). An obligation a law
`owe`s is a claim on the owner with the law as its document: payments to that party for that year settle it,
a refund settles the negative one. Principle: §4 (a claim is the time between an event and its counterpart),
§2 (a code is the same mark on things that belong together). Replaces: `waived` after a payment, typed late
fees, the half-cadence match for billed contracts, and `owe` as a second, unsettled kind of debt.
Closes 01a5 01a9 01a10 01c6 01d6 02a11 02a13 02c5 03a6 04a7 04a8 05a5.

### 2. `also`: implied flows and items, from the five places a purpose comes from (5 ledgers)
```text
purpose design
  when to.place is us/wa and not to.exempt
  also + 10.35% #sales-tax-collected for wa-dor             // an item on every flow of the purpose
kind processor
  also - 2.9% + 0.30 USD #fees                              // two items on every flow through it
kind card
  also issuer -> self 2% of amount #rebate                  // a flow of its own, into the account
contract job-jamie
  irs   withholding(gross - retirement - premium, self.filing)   // a leg is an expression
  also  pinnacle -> hsa 100 USD #hsa-contribution
```
`also` declares an item (`+` or `-`) or a flow that every matching flow carries, in a contract, party, kind or
purpose: the same five places that give a flow its purpose (§2). It is how `match`, `escrow` and a party
kind's `sales-tax` already work (§9): derived, named, shown by `why` and as a hint, reconciled with the record
that shows the same thing, overridden by writing the line. Amounts are expressions over the flow (`amount`,
`gross`, `date`), the properties of its parties and accounts, params, and totals (`total(#wages of employee,
year)` is FUTA's wage base). `when` guards it as in a law. Rates are params of a system (`us/wa`, `us/ny`)
that sync keeps current, so a move changes them and no party is edited. A late fee is `also` on a lease or a
client: `also + 5% #late-fees after 5d` is a claim that grows. Principle: §5, §7. Replaces: `match`,
`sales-tax`, restated paystubs, hand-computed fees.
Closes 01a6 01a7 02a3 02a9 02b3 03a3 04a5 04b3 05a1.

### 3. Periods relative to the day (4 ledgers: 01, 02, 03, 04)
```text
contract wa-excise with wa-dor
  about 6_100 USD monthly on 25 from operating for last month
contract est-me with irs
  9_600 USD installments 04-15, 06-15, 09-15, 01-15 from my-checking for the tax year
contract halcyon-retainer with halcyon
  4_500 USD monthly on 1 into operating covers the month     // a calendar unit, not thirty days
contract lease-c2 with tenant-c2
  2_050 USD monthly on 1 into pm-trust from 2026-05-15 prorated
15 est-me 8_800 USD for 2025                                 // an occurrence may carry a flow's tail
```
`for` takes a relative period (`last month`, `last quarter`, `the year before`); `the tax year` is the year an
installment schedule belongs to, so the January payment is the previous year's. `covers the
month|quarter|year` is the calendar unit containing the due day; `covers 1y` is twelve months from it.
`installments` are dates in a year, moved to the next business day. `prorated` makes the occurrence that starts
or ends inside a period a share of it by days, rescaling percent legs and constant legs alike. Principle: §2
(a flow has two times), §4 (a contract states it once). Replaces: `for` typed on every occurrence, restated
first months, list-of-dates guesses.
Closes 01a8 01b5 01b6 01c5 02a4 02b4 03a7 03b2 03c3 04a4.

### 4. Reconcile by document, not by amount (4 ledgers: 01, 02, 04, 05)
```text
sync paypal-bal
  csv   date "Date", gross "Gross", fee "Fee", amount "Net", currency "Currency", id "Transaction ID"
  group id                                    // rows that share an id are one flow (a conversion)
sync venmo
  csv   date "Datetime", amount "Amount (total)", party "To", memo "Note"
  route "Funding Source": "Venmo balance" is venmo, "Checking*" is checking
```
§13.2 matches a record to a flow of the same amount. A record matches (a) a whole flow by the amount its
account saw, (b) one *leg* of a split flow (the refinance's wire), (c) the sum of the flows that share a code
(a payroll batch), or (d) it is one of several records that make one flow (two ACH lines). `csv` gains `gross`
and `fee` (the fee is a `- fee #fees via PARTY` item), `currency`, `id`, `party`, `route`, `original` (amount
and currency in the memo). A document export (`run … prints Axiom`) outranks a bank row for the same money:
the row is its shadow, and derived flows are reconciled too. Principle: DESIGN §11 (reconciliation, not
import). Replaces: one sync per cardholder, scripts that pre-net a payout.
Closes 01d2 01d3 01d5 02d3 02d4 04d2 04d3 05c2 05d2.

### 5. Shares (4 ledgers: 01, 02, 03, 05)
```text
entity loomfield : llc
  owner me 60%, theo 40%                            // the business's tallies count for them in these shares
asset triplex : building
  part unit-a share 1_150 sqft
  part unit-b share 1_050 sqft owner rental-b       // a bill `of triplex` is divided by weight
contract rent with pine-property
  split among me, ben, cleo                         // a party's share is a claim: `for ben`
```
`owner A 60%, B 40%` gives a thing an owner set. A business's `count` reaches each owner's own line in those
shares (a partnership's K-1). `share W` weights the parts of a thing; a flow about the whole is divided among
the parts, each share a flow of its own borne by the part's owner: §9's `business`, generalized from an owner
to a thing or a party (a party's share is a claim, which is how a re-billed utility reaches a tenant). A share
paid from another owner's account is a claim of the payer on the bearer, settled by a flow between the two
owners whose purpose is `#reimbursement`, `#distribution`, `#contribution` or `#loan`; a flow between owners
with none of them asks which. Principle: §1 (cost allocation: a declared rule, once). Replaces: `owner me` and
a comment, units as owners, hand-split items, `business` repeated on every contract.
Closes 01a1 01a2 02a8 03a1 03a4 03c1 03c2.

### 6. A fourth root, `transfer`: what passes through (4 ledgers: 01, 02, 03, 05)
```text
purpose transfer                                    // beside income, spending and capital
purpose gift-received : transfer
purpose distribution, contribution : transfer
purpose sales-tax-collected, withholding : transfer
purpose rebate : transfer
31 loomfield owes irs 694.00 USD due 02-15 #withholding ^eftps-2026-01    // ▸ 694.00 of the bank's cash is now tied for irs
```
Some value is neither earned, consumed nor built: gifts received, distributions, tax withheld or collected,
deposits, loan proceeds, rebates. `transfer` is shown as itself, counted by no budget and by no income or
spending line, and read by `tax` where a system names it. An open claim the owner owes with a `transfer`
purpose ties that much cash for its creditor, as a `deposit` does; paying the creditor releases it, and
`available` subtracts it. Principle: §1 (custody is not rights), §3 (money remembers whom it is held for).
Replaces: root-less purposes and hand-filled `reserve` accounts. Closes 01a3 01b1 02a10 03a6 05a1.

### 7. Purposes by direction (4 ledgers: 01, 02, 04, 05)
```text
kind insurer
  purpose insurance                        // money to it
  pays claim-payout                        // money from it: §4's `pays` today is for commodity kinds only
kind processor
  pays payout
kind card-issuer
  pays rebate
```
`pays NAME` on a party kind names what money *from* its parties is for; without it the opposite direction is a
refund of `purpose` (§2), as now. Principle: §2 (purposes are inferred from the party's kind; direction was
the missing half). Replaces: written `#repair of car2` on insurer payouts and a `purpose fees` that lies.
Closes 01b7 01c4 02b2 04c2.

### 8. Recognition defaults for sync (4 ledgers: 01, 03, 04, 05)
```text
entity ashgrove : client                   // ▸ known-as "ASHGROVE*" by default: the name, hyphens as spaces, any case
sync card
  csv   date "Transaction Date" "MM/DD/YYYY", amount "Amount" flipped, memo "Description", category "Category"
  category "Groceries" is #groceries, "Food & Drink" is #dining
sync pm-trust
  csv   date "Date", amount "Amount", memo "Description", object "Unit"     // the purpose's object, from a column
```
A name is its own `known-as` unless it says otherwise; `category` offers a purpose for a merchant nobody is
known as; `object` maps a column to the purpose's object; `check` still groups what nothing recognized.
Principle: §11 (recognition is identity again). Replaces: 27 of the 30 client `known-as` lines in 01 and a
memo parse. Closes 01d1 03d1 04d1 05d1.

### 9. A change that replaces (3 ledgers: 02, 04, 05)
```text
2026-06-13 me-checking moves to joint-checking            // every contract and law that names it
2026-08-16 job-jamie 4_400 USD twice monthly              // a terms statement may carry legs: `empty` drops one
  ftb empty
  dtf 190.40 USD
2026-08-15 me moves us/ca/san-francisco to us/ny/nyc       // ends one residence, starts the other
2026-03-24 budget move 40 USD from fun to groceries until 2026-03-31
```
Four "this replaces that" changes have no one-statement form, and each is written as several statements that
must agree. A statement that names both ends of a change is one fact. `lives` stays overlapping for a citizen
abroad (`lives us` and `lives pt`), and `moves` is how one ends. Principle: §4 (terms change: one statement on
the day). Closes 02a6 02a7 02c1 04c1 05b2 05b3.

### 10. Where the body was, as a weight (2 ledgers: 02, 04)
```text
law feie
  each year closing 04-15
  let days  = days(self.in is foreign, 2026-03-01..2027-02-28)      // a window the book picks
  let share = fraction(self.in is foreign)                           // over the days this flow is recognized
  count amount * share as foreign-earned-income
  count peak(value(balance, USD)) as fbar-high                       // the extreme a value reached in the window
```
`days(P is X, window)` counts the days a property held a value; `fraction(P is X)`, inside a law on a flow, is
the share of the flow's recognition days on which it held (a sprint `for 2026-02-16..2026-03-15` is half
Lisbon's); `peak` and `low` give the extreme of an expression over a window. A year-closing law reads a
property as of the period's last day (`at close`), an `on` law as of the day, and a household's line at
close is the sum of its members' lines as members that day, whenever they joined. A statement dated in the
future is a plan, and a test not yet complete says "qualifies if he stays to 2027-02-28". Principle: §3, §4.
Closes 02a2 02c6 04a2 04a3 04a6.

### 11. Funded budgets: an envelope is a budget with money behind it (2 ledgers: 01, 05)
```text
budget car 100 USD monthly carries funded from checking into savings
budget reserve 30% of #income monthly funded from operating into reserve
```
A `funded` budget derives a transfer from the first holding to the second each month (or each income), tied
`for` an envelope of the same name; spending the purpose draws the tied money first, and `carries` is its
balance. Principle: §1 (mental accounting), §8. Replaces: legs into envelopes on every paycheck, a second
declaration of the same 50 USD, and `reserve` accounts filled by hand. Closes 01a4 05a3 05b1.

### 12. Value that arrives without cash (2 ledgers: 02, 05)
```text
14 -> bayside-honda 45_046.25 USD #purchase of car2
  - 11_000.00 USD #sale of car1 "trade-in"                  // a credit that disposes of car1 at 11,000.00
20 card -> gc-target 90.00 USD worth 100.00 USD "Target card at Costco"    // the 10.00 is a `#discount`
```
`worth AMOUNT` says the result of a flow is worth more than its cost; the difference is a `#discount`, a
reduction of cost, so spending is 90 and the card holds 100. An item `- X #sale of ASSET` in a purchase
disposes of the asset at X without money arriving, and a derived tax excludes it. Principle: §3, and "Axiom
never books a difference silently" (§2, Pairing). Closes 02a9 05a4.

---

## Spec repairs: no new syntax, only a sentence

1. **`for` has four readings**, decided by direction and by the word after it: held for an envelope
   (arrival), paid on behalf of a party (departure), a period, and an unstated one for an owner (`for
   loomfield`, `for theo`). Write the table.
2. `lives` "may overlap" (§4) and "a later statement overrides" (§3): say `lives` is the exception (02c1, 04c1).
   A citizen is taxed by `us` wherever he lives: give systems a `citizen` property, so `lives us` stops being
   a lie (04b1).
3. `books cash|accrual` belongs in §4's table (01c1).
4. A purpose's object may be any declared thing, not only an asset: an employee, a unit, a trip (01c3).
5. `covers the year`: calendar or twelve months from the day (01c5, 03c3)?
6. An occurrence line may carry a tail (02c4); `waived` may carry a purpose and items (01a9).
7. `owe` obligations and claims: are they one thing, and does `claims` list them (02c5)?
8. Items under a leg, and items in a one-ended split: which two ends do they sit between (03b1)?
9. Reconciliation matches derived flows too (05c2), and settles by the header, not by the net (01c6).
10. `kind envelope` and the party kinds `tax-authority`, `plan-administrator`, `insurer` belong in std
    (05c1, 02c2).
11. `deposit` names its direction and its holding; a flow with `due` is the same claim (02c3, 03c4, 04c5).
12. An item in a commodity other than the header's is a second exchange (04c4); an arrival with no cost in a
    non-base commodity takes the market price as basis (04c3).
13. Negative items belong in an `owes` statement (02a13). `via` is three things: an intermediary, `market`,
    and a gap's counterparty; say so.
14. Year-closing laws fire for every owner who lived in the system in the year, with the days (02c6).

## Not proposed

- **Accounts for meaning.** Withholding, sales tax collected, tenant deposits, retained earnings and a
  partner's capital all invite a liability or equity account. Each is here a claim, a tie or a share.
- **A payroll engine.** Withholding tables belong in `us/payroll` as params and `also` lines (proposal 2).
- **Systems.** `us/fsa`, `us/abroad`, `pt`, `us/partnership`, `us/co` are content, not language.
- **A per-employee tally.** `total(#wages of employee, year)` answers it; tallies stay the owners'.
- **Interest tracing** (02a5), **vesting** (02a12), **plan balances** (02a14): real, single-ledger, and
  better as laws in a system once `against` (proposal 1) can say what a distribution reimburses.
