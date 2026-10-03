# K3d map: what a purpose counts toward, who counts it, and what a payment settles

Written before the first code change of lane K3d, from the code at `368e5e8` (the merge of K7a), and checked against what the
code does (section 8 says how). Paths are in `crates/`; line numbers are those of `368e5e8`. The vocabulary is K3c's
([`K3c-map.md`](K3c-map.md) sections 0, 6, 10, 11): a *claim* is a parcel in a claim place or a tab, a *payment* a flow out of a
party's place that settles it. The probe books are in `docs/v5/measure/diff/cases2/` (`claim-recognition.ax`,
`claim-debt-tab.ax`, `claim-party-flow.ax`) and, new, `split-payment.ax` and its friends (section 7).

## 0. What the brief says and what the code does

Six things in the brief do not match the code. The first three decide the design.

1. **`budget` and `tax` recompute nothing.** The brief (and K3c's section 11.3) say `flow`, `budget` and `tax` "read each flow's
   posting and recompute what a purpose gets". Only `flow` does, with two siblings (`why #purpose` and the forecast's habit
   history). `budget.rs` reads `Run.headroom` (the readings the fold's own laws made) and `tax.rs` reads `Run.effects` (what the
   fold's own laws counted); both are *downstream* of the fold's counting, so one rule in the fold gives them the rule with no edit
   of theirs. The readers that do recompute are listed in section 1: three in `report`, one in `engine` (`explain`), and a dead
   one (`Lens::purpose_direction`, no caller).
2. **The cash default moves one example in the goldens, not five.** The brief says 04-freelancer, 07-landlord, 08-expat,
   09-shared and 11-sam write a claim with a purpose. 04 and 11-sam do (an itemized invoice whose items say `#design`); 07's is
   a *debt* (`me owes summit-roofing`, phase C); 09's are `due` flows from an owner's own account into a declared place, with no
   party at the other end; 08 writes none (its `owes` and `due` are prose). Measured (section 5), what moves in the goldens and in
   every report the fold feeds is **04-freelancer only**; 11-sam has no golden and no law that reads `#design`, so only its `flow`
   will move (it reads the postings, not the fold). 04-freelancer moves to the README's own figure: **gross receipts of 2025
   are 74,800.00 USD from 27 sources** (the README's table: "Gross receipts from 20 client payments, $74,800.00", and
   `outputs/tax-2025.txt` says the same 27 sources). At the start commit the golden says 158,672.40 from 42 sources: every
   invoice counted when made and again when paid.
3. **The fee leg is not a leg to the owner, but a payment's legs are one transaction.** K3c settles "by flow" and a leg to
   `stripe` is not a flow to the owner. The brief says the fold must know a leg's flow is part of one payment through `Made`'s
   group or K4b's `Heading::Source`. It needs neither: every flow a statement made, split legs and items alike, is in
   `Book::txns[flow.txn].flows`, a run of consecutive flow ids, and a split's legs share their `from` place. `Heading` is the
   solver's, and the fold has already used it (the group is solved when its first flow lands; `Record::resolved` says what each leg
   came to) by the time a leg is posted. Section 2.
4. **A payment out of a claim place is also a settlement.** K3c's first block settles in a declared claim place (`owed ->
   checking 200 USD`): it relieves `owed`'s parcels, and it is an internal transfer, so today it counts nothing. Under cash
   books that would make a claim in a declared place *never* income (the making counts nothing, the settlement counts
   nothing). The brief says "payment from the party"; the rule has to say "the claims a flow relieved", wherever they were.
5. **`Books::Accrual`'s doc and LANGUAGE §6 say "when it is due"; §7 says "when invoiced"; and §7 itself says both.** §7's
   contract paragraph (line 627) says "in accrual books an occurrence is income or spending on its due day", while its claims
   paragraph (line 668) says "income when invoiced". Built as the brief says, the claims paragraph (section 3.4); the
   contract reading is the same enum with another variant.
6. **`claims.rs` of the engine says "nothing reads `books` yet" and K5c's claim has no purpose.** That is still true of
   K5c's claim (`Ledger::claim_missed` sets `header.flow.purpose = None`) and this lane leaves it (section 3.6).

## 1. What a posting's purpose counts toward, and who counts it

A flow that has a purpose counts toward it: in a window total some law reads (`purpose total`, a budget's `spent`), as an
occasion for the laws of the purpose (`on flow`), and in four reports. Every place, with what it recomputes:

| reader | where | what it asks of the flow | what it recomputes |
|---|---|---|---|
| tallies of laws and budgets | `engine/post.rs:128` `count`, `:171` `purpose_flow`, then `Totals::record_purpose` (`totals.rs:703`) | the flow's purpose, `m.recognized`, ends | the **direction** (`purpose_direction(source_owned, target_owned, root)`, `lib.rs:113`) and the **amount** (`m.out` or `m.arrive` whole, in base currency on the flow's day) |
| purpose laws (`on flow`) | `post.rs:115` `fire_purpose` | `m.purpose`, `m.out` | the amount a law sees (`m.out`, whole); `tax.rs` is these laws' effects |
| `purpose total`, budgets | `eval.rs:1216` `purpose_total`, `Totals::read_purpose*` (`totals.rs:740`) | nothing: they read what `record_purpose` stored | nothing |
| `why` of a limit that broke | `explain.rs:245` `purpose_contributors`, `:268` `counts_toward` | every flow of the book | direction by shares (`purpose_direction(owns(from), owns(to), root)`) and amount from `plan.amounts`, whole, converted at the flow's day |
| `flow` by purpose and by party | `report/flow.rs:268` `PurposeTotals::of`/`add`, `:151` `PartyTotals::of`, through `movement_in_base_with` (`:586`) | every posting real on the day, owned | the **sign by root** (income `+`, spending and capital `-`, transfer `abs`), the end the amount is read at (`arrive` if from outside, else `out`), the ownership share, the price |
| `why #purpose` | `report/why/purpose.rs:197` `totals` | the same | the same loop, again |
| habit spending of the forecast | `report/forecast/variable.rs:37` `purpose_history` | the same, spending only | the same loop, a third time |
| a direction nobody asks | `report/lens.rs:171` `Lens::purpose_direction` | | dead: no caller. `flow.rs:553` `movement_in_base` is dead too |
| `budget`, `limits`, `headroom`, `tax` | `report/budget.rs`, `limits.rs`, `headroom.rs`, `tax.rs` | nothing: `Run.headroom` and `Run.effects` | nothing (section 0.1) |
| `available`, the forecast's ledger | `report/available.rs`, `forecast.rs` | they fork a ledger and `apply` flows | nothing: the fold's `post` counts them (so they get the rule for free) |
| asset capital | `post.rs:683` `capital_asset_direction`, `fire_purpose`'s `about` rules | | an asset's parts: K3c section 4, not touched |

So a posting's purpose counts toward four things (a tally, a law, a report row, a habit), asked in **seven places that each
decide direction and amount for themselves, whole**. A claim made with a purpose is counted when it is made (it is a flow into a
tab from outside, so direction `In`), and its payment, a flow from outside into the owner's money with the same purpose, is
counted again. There is nowhere that knows "this flow is the settling of a claim"; `settle_claims` knows it, and tells nobody.

## 2. How the legs of a split are one payment (phase A)

A statement is one `Txn`; its flows (the legs of a split, a header's items) are `Txn.flows`, consecutive ids. A split's legs all
leave the header's source place. A payment out of `fernhill -> 3_100 USD` with legs `business-checking 3_009.80 USD` and
`stripe 90.20 USD #business-fees` is two flows out of fernhill's place, to the owner's money and to a third party.

**A payment** is the flows of one statement out of the same party's place, in one commodity, real on the day, that pay an owner
or pay someone else *beside a flow that pays an owner*. "On the owner's behalf" is therefore a fact of the statement (the same
header pays the owner), not of a leg's purpose: a leg that has a purpose of its own (the processor's fee) is the owner's cost,
and a leg that has none is the same payment. A leg to a third party in a statement that pays no owner's money, and a flow
`ann -> bob 50 USD` that is a statement of its own, settle **nothing**: a party paying someone else settles nothing of ours.
The owner a third-party leg is paid on behalf of is the one the statement pays first.

**What it settles.** Each leg settles in its turn, as K3c settled a flow: the tab is relieved of `min(open, leg)`, in the
order a code, then the exact amount, then the oldest, and the party's place is debited `leg - settled`. Two things change:

- **A third-party leg settles too** (it pays what the claim was, to someone the owner owes the fee), so the 3,100 invoice is
  settled by 3,009.80 and 90.20 and the owner is out 90.20, the fee.
- **"Exactly the flow's" is judged on what the party still pays from this leg on**, in all. Leg one of the invoice of 3,100 asks
  for the claim whose open amount is 3,100 (not 3,009.80, which no claim is, so the oldest would have been chosen); it takes
  3,009.80 of it and leaves 90.20; leg two asks for 90.20 and finds that remainder. A statement with one flow asks exactly what it
  asked before. This is `Request.exact`, beside `Request.need`; `need` is still how much to take.

The settlement of each leg is its own entry in `Record::settled`, so a returned leg reopens exactly what it settled (K3c's
`reopen_claims`, unchanged), and a split returned by its code reopens all its legs. A leg that is pending lands by itself and
is a payment of the legs that are real on its day.

**Cost.** `payment_of` begins with `traits.owes(from)`: a binary search over the sorted `(party place, owner, tab)` table that K3c
built, so a flow out of a party that owes nobody (almost all of them) does nothing more than it did. Only a party with a tab
makes the statement's flows be looked at.

## 3. The rule (phase B)

### 3.1 What is counted, when

LANGUAGE §7: "A claim's purpose is its recognition: an invoice is income when invoiced in accrual books, when settled in cash
books." The rule, for the owner whose books these are, as a function of what a flow *is* to the claims:

| the flow is | cash | accrual |
|---|---|---|
| ordinary | its own purpose, whole | the same |
| the making of a claim with a purpose (value between a party and a claim place) | **nothing** | its own purpose, whole, when made |
| a payment that settled claims (value into the owner's money) | the claims' purposes for what they were, and its own for the rest | its own for the rest, and **nothing** for what settled |
| a leg that settled claims but does not arrive (a third party's, or out of a claim place) | its own, whole, **and** the claims' | its own, whole |
| a payment returned | the same, reversed | the same, reversed |
| a write-off of what was open | nothing to reverse | the claim's purpose, the amount forgiven, on the write-off day, reversed |

Three choices in the table, said out loud: (a) **a claim with no purpose has no recognition**: an unpurposed claim counts as it
did (in `Unclassified`), and what pays it counts as it did; (b) **the fee leg counts in full**: a leg that does not arrive is
the owner's cost, not the claim's recognition, so 04-freelancer's `#business-fees` stay what they were (6,847.91 USD of expenses,
26 sources, unchanged) while the `#design` of the claim they were part of is counted by the claim's own parcels; (c) **the laws of
a purpose see what the purpose counts**: `cash-receipts` fires on the claim's `#design` at the payment, with the settled amount, and
not at the invoice. A write-off's reversal is a window total only: a law cannot subtract what it counted (`count amount as
gross-receipts` adds), so no law fires on it, and the tallies of a law keep what it counted. Said in the report.

### 3.2 One function, asked by the fold and by the reports

`engine/recognition.rs`: `Counting { books, purpose, day, recognized, due, dealing }.pieces(book, &mut Vec<Piece>)`, with
`Dealing::{Ordinary, Making, Settling { settlement, dir, moved }}`. A `Piece` is a part of a flow that counts toward one purpose:
its purpose, its share (`Whole` or a quantity), the days it is recognized over, and whether it counts with the flow's own ends or
as the claim it settled (at the tab that held it, in the direction the claim counted in). A flow that settled nothing and made no
claim is one piece that says `Whole`, so an ordinary flow costs its reader what it cost. The fold asks it as it posts (`count` and
`fire_purpose` read the same pieces) and the reports ask it of the run: so `Run` gains the settlements of each journal flow
(`Run.settlements`, by flow, sorted), which is what `Record::settled` knows but forgets when a payment is returned.

What a claim was made *for* is the purpose of the line that made its parcel: `Parcel.part` is `PartId { origin, ordinal }` and
`Book::txn_flow(origin, ordinal)` is that line. A parcel that no line made (the monitor's claim, K5c) has none.

### 3.3 The readers delete their copies

`PurposeTotals::add`, `PartyTotals::of`, `why/purpose.rs::totals` and `forecast/variable.rs::purpose_history` each walk the
postings, filter the real and owned ones, price the movement and spread it over its days. They become one walk
(`flow::for_each_counted`) that yields what each posting counts of each purpose, priced and signed by the purpose's root, and
each keeps only what it does with it. `Lens::purpose_direction` and `movement_in_base` go (dead). In `engine`, `purpose_flow`
and `explain`'s `counts_toward` read the pieces. This is where lines are deleted; section 8 says how many, honestly.

### 3.4 When accrual counts (the user's decision)

§7 (claims) says "when invoiced", that is, when the claim is made. The doc of `Books::Accrual` and §6 say "when it is due" (and
§7's contract paragraph agrees with them). The lane builds **when made**, as the brief says (normative, simpler), as
`enum AccrualAt { Made, Due }` read in one function (`Counting::made`, the days a claim made in accrual books is recognized
over): `Due` is `Days::on(due)` for the same claim, the machinery for a value recognized ahead of its day being the fold's already
(`Totals::reached`). The doc of `Books::Accrual` is corrected to say "when it is made" in the commit that builds it. **Not hidden:
the two documents disagree, and the lane took §7's claims paragraph.**

### 3.5 The default

`Books::Cash` is the default the enum already says. The code commit builds with `Books::Accrual` as the default (so its
goldens show the double count gone and nothing more) and the flip to cash is one line in its own commit, with its goldens in
another (section 5), so it can be taken or dropped.

### 3.6 What it does not do, and why

- **K5c's claim keeps no purpose.** `claim_missed` clears the occurrence's purpose, so a rent the tenant owes counts when it is
  paid in cash and accrual books alike. With recognition built the purpose could be kept (one line, and the K5c test that says
  "a claim the monitor made has no purpose for a law to count" would then say the opposite), and in accrual books it would
  count on the day the miss is found, which is not the contract's due day (§7 line 627). That is the contract reading of 3.4,
  and the user's.
- **A claim in another commodity than the payment's** is an exchange and settles nothing (K3c).
- **Unpurposed claims double count in `Unclassified`** (the making and the payment are both boundary crossings, as before).

## 4. A write-off in accrual books

`Fact::ClaimChange` relieves the parcels the target transaction made (K3c section 9.3) and records a `WriteOff` row for each. The
reversal needs, per row, the purpose of the line that made it (for an itemized claim, each line its own): `WriteOff` gains the
flow (`flow: Id<Flow>`; `forgive` already holds it). The fold counts the reversal in the tallies (`record_purpose`, direction
opposite to the claim's, on the write-off day, valued then) and the reports count it as a posting of the claim's purpose with no
flow of its own (`Run.written_off` with `Book::claim_changes[change].day`).

## 5. Measured: what the cash default moves

Prototype of phases A and B on `368e5e8` (the fold's half: the code of the commits that follow, built three ways), `sh
tests/golden.sh` into three directories and `diff -rq` against the committed goldens; and every example project (01 to 11,
`explore-v5/*`, `v4-sketch`) through `check balance available limits claims flow budget lots contracts tax` on two days and
`flow --by party`, against the start commit's binary. The reports that read postings (`flow`, `why #purpose`, the forecast's
habits) are measured again when the reports learn the rule (step B3), and this section says what they do:

| build | goldens that differ from `368e5e8` | examples whose fold-fed reports differ |
|---|---|---|
| A: a payment settles by what the party pays in all | `04-freelancer-{check,balance,available,claims}` | 04 only |
| A and B (fold), accrual default | the same four, and `04-freelancer-tax` | 04 only |
| A and B (fold), **cash default** | the same five (`available` and `tax` differ from the accrual build) | 04 only |

The numbers for 04-freelancer (tax 2025 on 2026-04-16 unless said):

| | start | A | A+B accrual | A+B cash |
|---|---|---|---|---|
| gross receipts | 158,672.40 (42 sources) | the same | 84,400.00 (22) | **74,800.00 (27)** |
| total income | 152,114.61 | the same | 77,842.21 | 68,242.21 |
| federal income tax owed | 25,970.50 | the same | 4,286.14 | 2,073.22 |
| SEP-IRA `available`, driven by federal tax | 3,166.80 | 3,166.80 | 3,008.46 | 2,919.24 |
| `claims`, total | 12,467.10 (10 rows) | 11,800.00 (3 rows) | the same | the same |
| net worth, 2026-04-16 | 113,287.25 | 112,620.15 | the same | the same |

A removes the seven invoices that showed the processor's fee as their remainder (`fernhill still owes 130.80 USD, 396 days past its
due day` for an invoice paid in full), and the 667.10 USD of fee that stands in the party places is no longer an asset of the owner
(net worth 113,287.25 to 112,620.15: the fees are what the owner paid). Cash gives the README's figure and 27 sources: 20 client
payments, and the 7 fee-leg parts of the payments written as splits. `business-expenses` is 6,847.91 (26 sources) in every build.

The prototype also runs the existing tests: with the accrual default, `cargo test --workspace --release`: only the two failures
of the integration branch (the prorata one and the year-end forecast one).

### 5.1 Measured again, with the readers and the cash default built

Every example project through `check balance available limits claims flow budget lots contracts tax` on 2026-04-16 and 2026-12-31
and `flow --by party`, then `forecast` and `why #design|#consulting|#retainer`, the start commit's binary against the last
commit's. What differs, and why (the books of an example decide: `01-agency` says `books accrual`, the others say nothing and are
cash):

| example | what differs | why |
|---|---|---|
| `04-freelancer` | `check balance available claims lots tax flow flow --by party`, `why #design`, the forecast's net worth | the table above; `flow` counts each invoice once, when it is paid (cash) |
| `11-sam` | `flow`, `flow --by party`, `why #design` | one 3,800.00 USD invoice counted when made and again when paid; now once, when paid |
| `v4-sketch` | `flow`, `flow --by party`, `why #design` | a 3,800.00 USD invoice that is not paid by 2026-12-31 counts nothing in cash books |
| `explore-v5/01-agency` (accrual) | `flow`, `flow --by party`, `why #design` | the payments of invoices no longer count as `Unclassified` (120,660.65 to 33,689.54 USD), and the two write-offs take back what the claims counted: `design` -589.50 (the credit note `CN-0002`) and -3,750.00 (`inv-2026-0009`), and `sales-tax-collected` takes back its 403.65 and 388.12 |
| `explore-v5/03-triplex` | `flow`, `flow --by party` | the late fee of 92.50 USD counts when it is paid, as `late-fees`, and not as `Unclassified` |
| `explore-v5/04-nomad` | `flow`, `flow --by party`, `why #consulting` | the payments of `#consulting` invoices no longer count as `Unclassified`; the same month counts `consulting` as before |

Nothing else of any example differs: not `check`, `balance`, `available`, `limits`, `claims`, `lots`, `contracts`, `budget` or
`tax` of any but 04, and not the forecast of any but 04's net worth (113,287.25 to 112,620.15 USD, the fees). The 304 of 2,000
mutants of the examples on which `fuzz.py ... diff` finds a different `check` are all of 04.

The probe books of `docs/v5/measure/diff/cases2/` through both binaries: `split-payment` (`check balance available claims lots
flow`: the invoice is settled, 90.20 USD of fee no longer stands as an asset, and `flow` counts 3,100.00 of `design` and not
6,109.80), `claim-recognition` (`check`: one `recognized` warning, on the payment, and not two; `flow` 300.00 and not 600.00),
`claim-flow-code.flow` (300.00 and not 800.00), `recognition-cash.flow` and `recognition-accrual.flow`: all expected by the rule.
`recognition-writeoff-lines.ax` (added with the fixes: an itemized claim of 3,000.00 `#design` and 300.00 `#tax-collected`, paid 1,000.00 and the
rest forgiven, in accrual books) says what the write-off takes back of each line: `design` 3,000.00 then -2,000.00, `tax-collected`
300.00 then -300.00, where the start commit takes back nothing.

## 6. Debts as parcels (phase C): the design, not built yet

What `Class::holds_parcels` gates: `post.rs:95` (a flow whose ends hold no parcels credits and debits balances and relieves
nothing), `ledger.rs:597` (`all` reads the parcels' admitted quantity or the plain balance), and, by sign, `Sides::of` (a place's
display sign comes from its class, `Debt = -1`). Everything that reads a `Debt` place reads a **signed plain balance**: the
balance sheet (`balance.rs:86`), `owed_by_you` (`claims.rs:95`, which rebuilds a debt from the flows touching the place, by code),
`trace.rs:120,153` (what a forecast may pay into a debt), `available.rs` (debts as going out) and the `payable` gate of
`claims.rs:88`. A *tab* of the Debt class (`kind debt-claim`, which says `claim`) is the only Debt place that should hold parcels:
credit cards and loans are balances that are not claims on a party.

The design, if the lane gets to it: `holds_parcels` becomes a fact of the place (`class == Asset || claim`, no new byte in
`PlaceTraits`); a Debt tab holds positive parcels of what is owed, made by the flow *out of* the tab (`me owes pge`), and a flow from the
owner's money to the party **relieves them** by the same `exact`/`code`/`oldest` order (`settle.rs` mirrored: `tab_of(party, owner,
Class::Debt)`), so a paid bill is settled and `available` stops counting it twice (`claim-debt-tab.ax`: 857.50 to 665.00 today);
`Sides` gives a tab the sign that makes its parcels read as liabilities; `claims` lists a debt by `is_claim` and its class, and
`owed_by_you`, the `payable` gate and `Book::makes_debt` go. Recognition mirrors section 3 with the direction `Out`.

## 7. The plan, and what proves each step

| step | what | proof |
|---|---|---|
| 0 | this map | |
| A1 | acceptance tests, ignored (04's fee leg, a returned split, a third party, exactness on the total) | they fail on the start commit |
| A2 | `Request::of`/`exact`, `settle.rs`, `post.rs` settles first; tests un-ignored; the 04 goldens in their own commit | `claims.py` extended with split payments and mutation-tested; `splits.py`; sweep: 04 only |
| B1 | acceptance tests, ignored (cash, accrual, partial, itemized, returned, a law, a budget, a write-off, a claim place) | they fail |
| B2 | `books` read once (`EntityTraits.books`), `recognition.rs`, the fold counts pieces, `Run.settlements`; accrual default | tests; the oracle gains recognition; mutants |
| B3 | the reports read pieces (`for_each_counted`); write-off reversal; `explain` | tests; the sweep's `flow` and `flow --by party` on every example; `fuzz.py ... diff` |
| B4 | the default flips to cash, one line; its goldens in another commit | goldens: 04 only |
| C | debts as parcels | only if A and B land inside budget; else this section is the design |

`docs/v5/measure/diff/cases2/` gains `split-payment.ax` (the fee leg), `split-third-party.ax` (a payment to someone else),
`recognition-cash.ax`, `recognition-accrual.ax` (with a partial payment and a write-off) and `recognition-claim-place.ax`, each
through the start commit's binary and the final one.

## 8. How this map was checked

- Section 1: `grep` of `purpose_direction`, `record_purpose`, `reads_purpose`, `movement_in_base`, `movement_place`, `Posting` and
  `purpose` over `crates/`, each hit read; `budget.rs`, `tax.rs`, `limits.rs`, `headroom.rs` read for what they take from a flow
  (nothing: `Run.headroom`, `Run.effects`).
- Section 2: `claim-party-flow.ax` and a probe of the fee leg (`split-payment.ax`) through the baseline binary: the leg to `stripe`
  is a flow `Outside` to `Outside`, which `post` sends down the branch of places that hold no parcels, where K3c's settlement was
  never asked.
- Section 5: a prototype of A and B built and swept as the table says; the baseline goldens reproduce byte for byte at `368e5e8`
  (`sh tests/golden.sh`, `sh tests/mistakes/run.sh`: no diff).
- 04-freelancer's README and `outputs/tax-2025.txt` for the 74,800.00 USD and 27 sources.
- The two failing tests of the integration branch and the baseline's counts are K3c's and K5c's: the prorata one and the
  year-end forecast one.

## 9. What was built

The commits on `claude/great-wozniak-pnqn7x-v5-k3d`, from `368e5e8`, in the order the plan of section 7 says (the numbers
are non-test lines by `briefs/loc.py`, excluding `#[cfg(test)]` and `*_tests.rs`):

| commit | what | engine | report | model | total |
|---|---|---|---|---|---|
| `368e5e8` | the start | 11,448 | 7,000 | 18,542 | 53,190 |
| `44b64ba` | this map | | | | |
| `3582ef1` | acceptance tests of a split payment, ignored | | | | |
| `490cbfc` | **A** a payment settles by what the party pays in all (`settle.rs`, `Request::exact`) | 11,495 | | | 53,237 |
| `9718f7a` | goldens: `04-freelancer-{check,balance,available,claims}` | | | | |
| `dd2f5ce` | acceptance tests of recognition, ignored; the probe books; LANGUAGE §7 says what a payment of several legs is | | | | |
| `7ed085c` | **B** the rule (`recognition.rs`), the fold counts pieces, `Run.settlements`; accrual default | 11,741 | | 18,549 | 53,490 |
| `e9ccc34` | goldens: `04-freelancer-{available,tax}` | | | | |
| `d8f2fd1` | the claims oracle: split payments and recognition, with references written from §7 | | | | |
| `1dce5f5` | the readers (`flow`, `flow --by party`, `why #purpose`, the forecast's habits) ask the rule; the write-off's reversal | 11,784 | 7,024 | | 53,557 |
| `5da3aee` | **the default of `books` is `cash`**: one attribute | | | | |
| `e8b2de9` | goldens: `04-freelancer-{available,tax}` again | | | | |
| `2f522b4` | a write-off takes back each line of an itemized claim for its own purpose (found by the oracle) | 11,790 | | | 53,563 |
| `d4f6db2` | a claim taken back is not more of a purpose that passes through (found in `01-agency`) | | 7,027 | | |
| `bac034f` | a flow that settles and makes no claim costs the rule almost nothing | 11,810 | | | 53,586 |
| `42c1d77` | the mutants of the rule | | | | |
| `66ace96` | `post` is steps of its own | 11,828 | 7,027 | 18,549 | 53,604 |

**Net: +414 non-test lines (engine +380, report +27, model +7), and 610 lines of tests** (`payment_tests.rs`, `recognition_tests.rs`
in `engine` and in `report`), plus the oracle (`claims.py`) and six probe books. The honest accounting of section 3.3's promise,
"the readers delete their copies": it did, and it did not make `report` smaller. Four walks over the postings (`PurposeTotals::add`,
`PartyTotals::of`, `why/purpose.rs::totals`, `forecast/variable.rs::purpose_history`) and the dead `purpose_direction` of `Lens` went (-32
lines in `lens.rs`, `why/purpose.rs` and `variable.rs`), and the one walk that replaced them, `flow::for_each_counted` with the
write-off's `forgiven_by` and the priced signed amount, is +55 in `flow.rs`: the reports now do three things they did not (settled
claims, claims taken back, a payment's fee leg), and the net of `report` is +27. The rule itself is `recognition.rs`, 137 lines.
Where the lines are: `settle.rs` +81 (A: the legs of a payment are one; B: returns and claim places), `recognition.rs` +137, `post.rs`
+31, `fire.rs` +21 and `explain.rs` +20 (the limit's explanation names what the rule counted), `claims.rs` +19 (the write-off per
line).

Function lengths (`hist.py`, all of `crates/`, tests excluded): 42,777 lines in functions at the start, 43,073 now. 1-10 lines: 2,023
fns to 2,039; 11-20: 691 to 698; 21-40: 504 to 513; 41-80: 131 to 128; 81-160: 8 to 8; over 160: 1 to 1. The lane added no function
over 40 lines, made `finish` of `ledger.rs` two lines longer (46 to 48, the sorted settlements), and shortened `post` from 42 to 32 (it had
grown to 54 on the way and was split: `accept_waiver`, `deal_with_claims`,
`count_leaving`, `fire_arrival`); no new `bool` parameter (`Relief::{Pending, Done}` says whether the claim place was relieved
before the flow was counted).

## 10. What was measured

- **Timings.** `axiom check -C bench/{100k,1m}` (the seeded generator, no claims in it, so this is the cost of the rule on a
  book that does not use it), the start commit's binary against the last commit's, four runs each, interleaved, the fastest of the
  four, on a shared machine with a load average of 5 to 6 (K6 builds beside this lane): 100k flows 0.478 s before and 0.494 s after (+3%), 1m flows 4.426 s before and 4.487 s after (+1.4%), the medians of
  the four equal within the noise, peak RSS unchanged (80.6 MB and 680 MB). Instructions are steadier than the clock on a
  loaded machine (`valgrind --tool=callgrind`, `check` of 100k flows): **1,870.8 million before, 1,895.9 million after (+1.3%)**. The first build of the readers cost +2.7%; the explanation of a limit (`purpose_contributors`) builds pieces for
  every flow of the book, and now passes over the flows of other days and other purposes before it does.
- **The claims oracle** (`docs/v5/measure/claims.py`, 600 projects of seed 7, references written from §7): the build at the last
  commit, held to the references of the new rules in cash books: **0 failures of 600**; against the start commit's build: 276
  identical, 324 different as the references say, 0 different where they say nothing, 0 identical where they say a difference. The
  `flow` report was added to what the oracle checks after the reader commit; it found the bug of `2f522b4` (26 failures, 13 of
  them a column the oracle read wrongly and 13 the bug) and the oracle itself was wrong twice (the `Total` column of `flow` is blank
  when it adds up to nothing; its prediction of when two builds must differ left out the payments that no longer count as
  `Unclassified`).
- **Mutants** (`claims.py mutate`, each built and held to the oracle, then to the tests): at the last commit, 46 mutants of the
  code of lanes K3c and K3d: 43 killed (30 by the oracle and 13 by the tests) and 3 survived. A's 18 that were not rewritten since
  (the exact policy, the codes of a payment, a leg to a third party, what a party still pays, the lines of an invoice) are all
  killed, among them the two that survived the first run (a leg that is not real yet is paid, and the legs of two parties are
  a payment), which are now `payment_tests.rs`'s two last tests. The 28 that are new or rewritten for B and the fixes (when a
  claim counts and in which books, what a payment replaces, what a write-off takes back and for which line, what the readers are
  told, what a transfer purpose's reversal is): 25 killed, 3 survived, and the survivors are equivalent as far as anything
  counts: a returned payment's entry left in `Record::settled` (it is read once), a flow out of a claim place that `reaches` the
  owner (an internal transfer counts nothing either way), and a flow from an owned place into a claim place that is called a claim
  (the same). The reversal of a write-off is a window total that no table of the oracle shows, so the tests, not the oracle, kill
  the mutants of it; that is a gap of the oracle and not of the tests.
- **`fuzz.py ... diff`**, 2,000 mutants of the examples (seed 11): no panics; 304 differ in `check`, all of `04-freelancer` (the
  fee legs: the overdue warnings and the net worth); the 1,696 others are identical.
- **`splits.py`** (K4b's differential over 300 projects, seed 5): the 272 with no claim in them are identical in every command;
  of the 28 with claims, 10 are identical and 18 differ in `flow` or the forecast: a claim with a purpose that is not paid counts
  nothing in cash books (and no habit of spending for the forecast), and one that is paid counts when it is.
- **The probe books** (`diff/compare.sh` over 156 mistakes and 32 valid projects): 11 files of 5 books differ, all of them the
  books of this lane (section 5.1); the mistakes (`cases/`) are identical.
- **The tests**: `cargo test --workspace --release`: all pass but the two that fail at `368e5e8` as well (the prorata one and the
  year-end forecast one). `cargo clippy --workspace --all-targets`: 93 warnings before and after, none in a line of this lane.
  Goldens: five files of `04-freelancer`, in three commits of their own.

## 11. What is not finished

- **Phase C, debts as parcels, is not built.** A and B took the budget, and C touches `Sides`, `owed_by_you`, the `payable` gate and
  `makes_debt`, which K6 (norms and relators) is beside. Section 6 is the design; `claim-debt-tab.ax` is the probe (`available`
  857.50 to 665.00 USD once a bill is settled by its payment).
- **A limit that broke does not name a payment out of a *claim place* that settled the claim** in cash books (`explain.rs` reads
  `Record::settled`, which holds the payments from a party's tab; a flow out of a claim place is in `Record::settlements` only,
  because a return of it must not reopen anything). Its own purpose, if it has one, is named.
- **K5c's claim has no purpose** (`claims.rs`, `claim_missed`: `header.flow.purpose = None`). Giving it the occurrence's is one line
  (drop that assignment) and one K5c test (`claim_tests.rs`'s "a claim the monitor made has no purpose for a law to count" says the
  opposite then). What accrual books would then count: the occurrence, whole, over the occurrence's own recognition days, as of
  the day the miss is found (the day the claim is made, never before its due day), and a write-off takes it back that day; cash
  books count it when the party pays it, on the payment's days. That is the contract reading of §7 line 627 (the due day) only
  as far as the miss is found on it.
- **`AccrualAt::Due` is a variant nothing exercises**: it is `Days::on(due)` for the claim made, in one function
  (`Counting::made`), with no test and no oracle; it will need both the day someone takes the contract reading of §7.
- **A transfer purpose's reversal** counts as `-volume` in `flow`, though `flow` shows a transfer as the volume of what passed in
  either direction; a payment returned that counted a transfer claim is the same. Neither is in the oracle (it checks `design` and
  `retail`, both income).
- **No law fires on a write-off's reversal** (a law cannot subtract what it counted); the totals and the reports follow it, the
  tallies of a law do not. Said in LANGUAGE §7.
- The two failing tests of the integration branch are still failing; they fail at `368e5e8`.

## 12. The three places I am least proud of

1. **Three records of what a flow settled**: `Record::settled` (a map, so a return can open the claims), `Record::settlements` (a
   log, sorted into `Run.settlements` for the readers) and `Frame::settled` (explain's view of the first). They disagree about
   exactly one kind of flow (out of a claim place), which is the gap of section 11, and a fourth reader would have to choose.
   The right shape is a settlement recorded on the flow once (`Posted` could carry it), and a return that reads it back.
2. **The write-off's reversal is written twice**: the fold (`claims.rs::take_back`, which only moves a window total) and the
   reader (`flow.rs::forgiven_by`, which rebuilds the same pieces from `Run.written_off`). Both go through `Counting::forgiving`,
   but nothing makes them agree except the oracle, and it found a bug in exactly that seam (`2f522b4`). `Run` should carry what
   was taken back, as it carries what was settled.
3. **The rule does not make the readers smaller, and costs a flow +1.3%**: section 3.3 promised deletion and delivered one walk
   that is +55 lines where the four it replaced were not, and `report` is +27 net. The rule is asked of every flow with a purpose,
   in a book that makes no claim too, and what it answers, one piece that says "all of it", is built each time (`Counting::pieces`,
   `makes_claim`, and `record_purpose` is no longer inlined). A flow with no purpose and no claim skips it; one with a purpose
   and no claim could, if the fold knew that the book has no claim place at all.
