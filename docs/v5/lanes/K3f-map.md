# K3f map: a debt is a parcel, and paying a bill settles it

Written before the first code change of lane K3f, from the code at `46a6c07` (the head after K6b), and checked against what the
code does (section 5 says how). Paths are in `crates/`; line numbers are those of `46a6c07`. The vocabulary is K3c's and K3d's
([`K3c-map.md`](K3c-map.md), [`K3d-map.md`](K3d-map.md)): a *claim* is a parcel in a claim place or a tab, a *payment* a flow
that settles it, *recognition* when a claim's purpose counts. This lane is K3d's section 6 and the mirror of K3c's section 10:
what the owner owes a party, held the way what a party owes the owner is.

The probe books are `docs/v5/measure/diff/cases2/claim-debt-tab.ax` and, new, the family named in section 6.

## 0. What the brief says and what the code does

Ten things in the brief or in K3d's section 6 do not match the code. The first three decide the design.

1. **`Sides` needs no change, and the brief's parcels cannot be positive.** K3d §6 has a Debt tab hold positive parcels and gives it
   a `Sides` sign that reads them as liabilities. Checked against every reader of a Debt place (section 1), that breaks the one
   invariant they share: *what a place holds is what flowed into it less what flowed out*, in the sign of the flows. The register
   and `Unpriced` replay postings (a bill is a flow out of the tab: negative); the histories the fold records give `balance --at`
   and `NetWorth` (`balance.rs:86`: assets plus liabilities, liabilities "naturally negative"); `trace.rs:113,146` reads
   `holding.qty()` and `-ledger.balance(to)` for a debt; `why/entity.rs`, `why/place.rs`, `expected.rs:140` sum `holding.qty()`. A
   claim Debt place whose holding is positive owed would be the one place where holdings and postings differ in sign, so each of
   those would carry an exception and the register would show a bill as negative beside a balance that shows it positive. So **a
   debt's parcels are negative quantities**, as a liability's balance is (`Holding.plain` is documented "signed: liabilities...
   hold only this"). `Sides` is untouched: `display_sign(Debt) = -1` already reads a negative parcel as the liability it is, and
   every report that reads a Debt place stays as it is. What is new is only that `lots.rs`'s relief is written for positive
   quantities; section 2.1 says how a debt is relieved in the mirror, in about 25 lines.
2. **The `payable` gate serves declared accounts, not tabs.** `claims.rs:73-77` lists a Debt place if its kind `is_a` `payable`, and
   `std.ax` says `kind payable : debt` with `claim`: so a declared `account bills : payable` (07-landlord's `bills` and `deposits`,
   09-shared's `owed-to-ben` and `owed-to-cleo`) is already a claim place of the Debt class that holds a plain balance, listed by
   `owed_by_you`. A *tab* (`me owes pge`) is not listed, because its kind is the root `debt-claim`, which is not `payable`: that is
   why `claim-debt-tab.ax` says "Nothing is owed either way" while `balance` shows 192.50 USD. Both shapes become parcel places by
   one rule, "a Debt place that says `claim`", and are settled differently: a declared place by a flow *into* it
   (`checking -> owed-to-ben 47.53 USD for #power`, which `owed_by_you` reads as "a flow into it carrying or selecting the code"),
   a tab by a flow from the owner's money to the party's place (`checking -> pge 142.50 USD ^b1`).
3. **A loan's debt and a bill share one tab, and an interest payment to the lender would settle the bill.** A tab is keyed
   `(party, owner, class)` (`declare.rs:397`) and every Debt tab is kind `debt-claim`, which says `claim`. K5d's loan asks for
   `world.tab(party, owner, Class::Debt)` (`contracts.rs:166, 300`), so a mortgage with `bank` and a bill from `bank` are one place,
   and once a Debt tab holds parcels a payment to `bank`'s place (the loan's `#interest` leg, a credit-card payment) would relieve
   it. A loan must stay a plain balance (the schedule of K5d is held to its balance, `loan_balance.rs`). **The model tells a loan
   from a bill by the tab's kind**: a bill is `debt-claim` (a claim the party holds on the owner, and it says `claim`), a loan's tab
   is the root `debt` (a plain balance that says nothing), and a tab is keyed by `(party, owner, kind)`. A card is a declared
   `debt` place (`kind card : debt`), a loan account `kind loan : debt`: neither says `claim`, so neither holds parcels. One test
   (`a_tab_made_while_the_journal_is_lowered_has_rules...`, `model/tests/tabs.rs:72`) asserts `is_claim` of a loan's tab; it is
   edited to say the loan's tab is *not* one (its other assertions stay), because the brief says it is not.
4. **`holds_parcels` has three readers and only one of them wants the claims.** `post.rs:83` (the gate: whether a flow goes through
   relief, pricing and arrival or is two credits), `post.rs:121` (a flow out of a claim place relieves it first) and `ledger.rs:615`
   (`all` takes what the selectors admit, or the plain balance). Only the gate must see a Debt claim place; the other two ask
   whether a place is one whose parcels a flow *takes from*, which a debt's are not (`D -> checking all` is the credit the owner has
   with the party, the plain balance). `Class::holds_parcels` goes; the two keep `class == Asset`; the gate asks the class or the
   place's `claim` (no new byte in `PlaceTraits`: it is four bytes and asserted so).
5. **`claim_target` refuses a bill, and `makes_debt` exists for its wording.** `statements.rs:501` rejects `^b1 waived` with "this
   transaction made a debt of yours, which has nothing to forgive". With a bill a parcel it has: the party forgives what the owner
   owes (K3d's table: "a bill's write-off reverses in accrual"). `Book::makes_claim` learns the debt's end, `makes_debt` and the
   wording go.
6. **Recognition's direction is the claim's, and three places say it as "In".** `Claiming.dir` is `Dir::In` for a settlement and
   `Dir::Out` for a return (`settle.rs:36,125,132`), `Counting::forgiving` says `Dir::Out` (`recognition.rs:101`), `Piece::takes_back`
   is "a claim piece counted `Dir::Out`", and `flow.rs::forgiven_by` negates the quantity. All of them read a claim as income's way.
   What a claim counts in is a function of the tab's class (an Asset tab counts In, a Debt tab Out, and taking it back is the other
   way), so `takes_back` takes the book and `Dir`s are asked of the class once (section 2.5).
7. **`lots` would list every debt, as a negative parcel, and add it to "Unrealized".** `lots.rs:42` walks every owned holding's
   parcels. A liability is not held: the report keeps to the Asset class (one filter).
8. **`deposit` is not Part B of this lane.** STATUS Decision 17 says K3f builds `deposit`, but the brief does not, and lane U's
   budget has K3f at +70 net lines. The grammar reads `deposit` and nothing reads `Contract.deposit` (no flow, no claim on the
   landlord) before and after this lane; said in the report, not built.
9. **A debt the monitor finds missing is not made a claim here.** `plan.rs:285` `claim_tab` says "a debt of the owner is not
   claimed: it is a plain balance". LANGUAGE §7 says "a bill the owner owes" missing is a claim on the owner. With bills parcels it
   could be; it is a decision about the monitor (K5c) and the brief puts the bill's *making* by `owes`, so it stays.
10. **A test calls `owed_by_you` directly.** `report/src/tests.rs:1136,1145` (`a_bill_is_netted_by_its_code_across_the_flows_that_made_and_settled_it`)
    asserts what the function computed from flows. The function is deleted by the brief, so the test is rewritten, with the same
    two facts (700.00 open on 03-31 with its made and due days, 1,200.00 on 03-05), as a book the fold runs and `claims::open`
    reads. No test is deleted.

## 1. Every reader of a Debt-class place, and what each needs of a parcel

A parcel in a Debt claim place is a negative quantity with a transaction, a day (`acquired`), codes and a part id (the line that
made it): the same record as a claim's, which is why the readers that ask a claim place for its parcels need only the sign.

| reader | where | today | what changes |
|---|---|---|---|
| the fold's two credits | `post.rs:83-103`, `relieve_balance :318`, `arrive_balance :532` | a flow between non-asset places is two credits; a Debt place is a signed balance | the gate asks `claim` too; `relieve_balance` makes the parcel of a bill, `arrive_balance` is told what the payment settled |
| `all` | `ledger.rs:615` | `holds_parcels()` is `Asset`: admitted parcels, else `plain.max(0)` | `class == Asset`: unchanged |
| claim place source | `post.rs:121` | `claim && holds_parcels()` | `claim && class == Asset`: unchanged |
| settlement of a payment | `settle.rs` `payment_of`, `Traits.claims` | a flow out of a party's place into an owner's money; the table lists Asset tabs | a flow out of an owner's money into the party's place, and a flow into a Debt claim place; the table lists both classes |
| write-off | `engine/claims.rs` `write_off`, `forgive` | the line paid into a claim place; value goes back to where it came from | the line paid out of a Debt claim place; value goes back to where it went, in the sign of the place |
| recognition | `recognition.rs`, `plan.rs:206` `makes_claim`, `post.rs:240`, `flow.rs` | a claim is made by value from outside into a claim place; counted In | a bill is made by value out of a Debt claim place to outside; counted Out; direction from the tab's class |
| `claims` | `report/claims.rs:54-77, 85-120` | parcels of Asset claims; debts of `payable` places rebuilt by `owed_by_you` from flows touching the place | `is_claim` and the class say which side; the left of a debt is its parcel in the display sign; `owed_by_you`, `settled_codes`, the gate and `Class`/`BTreeMap` imports go |
| `available` | `report/available.rs:92-98`, `due_soon` | counts `claims::open`'s debts due within 30 days | unchanged: it reads `Claim`s, so a paid bill leaves and an unpaid one in the next 30 days comes in |
| `balance`, `NetWorth`, `why`, `register`, `Unpriced` | `balance.rs`, `balances.rs`, `why/*`, `register.rs` | flow-signed holdings and postings | unchanged (section 0.1) |
| the forecast | `trace.rs:113,146`, `expected.rs:140` | `holding.qty()`, `-ledger.balance(to)` for a debt | unchanged; a payment into a debt is bounded by the place's balance, which is the owed amount |
| `lots` | `report/lots.rs:42` | every owned holding's parcels | Asset class only |
| `open(^code)` in a law | `eval.rs:1414` | claim places' parcels, whatever their sign | reads each parcel in the place's display sign, so an open bill is positive |
| overdue, monitor | `ledger.rs:350`, `monitor.rs:206` | `lot.qty > 0` of a claim place | unchanged: a debt's negative parcel is excluded by the filter that was already there, so no `overdue` is said of a bill (it never was) |
| `claim_target` | `statements.rs:501`, `said.rs:199,205` | refuses a debt | `makes_claim` knows both ends; `makes_debt` and its wording go |
| tabs | `declare.rs:100,397`, `contracts.rs:166,251,300`, `record.rs:1351` | keyed `(party, owner, class)`, kind by class | keyed `(party, owner, kind)`; a loan asks for the root `debt` |

## 2. The design

### 2.1 A debt's parcels, and relief in the mirror

`lots.rs` is written for what is held: every comparison, `min`, `take` and `Exact` assumes a lot of positive quantity, and "a
negative quantity never lives in a lot" (`lib.rs:392`). A debt's lots are negative. Rewriting relief for both signs would touch
`take_run`, `take_exact`, `relieve_scanning`, `gather` and `allocate`, in the file K3e is about to restructure. The slot is
instead *mirrored* around the one call that needs positive quantities:

```text
Slot { owes: bool, .. }
fn mirror(&mut self)        negate plain, qty and every lot's qty and basis; drop the HIFO heap
fn owe(&mut self, parcel)   what is owed, held as a liability: sets `owes`, lands the parcel negated
fn relieve(&mut self, req)  if owes { mirror; relieve; mirror }
fn admitted(&self, ..)      if owes: the same sum in the other sign
```

Mirrored, what is owed is what is held, and relief of it is relief of a claim: by a written selector, then the codes of the flow,
then the exact amount, then the oldest (`Policy::Exact`, which a claim place has by default), and "relief takes more than the place
holds: the shortfall is taken from plain, so a holding can go negative" becomes, mirrored back, *an overpayment is a positive
plain balance*, the credit the owner has with the party, with no case for it. `Slot::qty` and `recorded` are negated twice, so the
histories see one change. Plain-only slots (the hot path) pay one `bool` test in `relieve`.

### 2.2 Made

A flow *out of* a Debt claim place makes a bill: in `relieve_balance`, which is already "a source that holds no parcels, a debt or
the outside, only a balance", a claim place is not credited but *owes*: one parcel of `m.out.qty`, acquired on the flow's `since`
or day, the transaction and codes of the flow, `part` = the line that made it (`PartId { origin, ordinal }`, which recognition and
the write-off read for the purpose), basis zero. The value in flight is the fresh slice it always was.

### 2.3 Relieved

`settle_claims` (`settle.rs`) is the one place a payment is known to settle. It grows a mirror of `payment_of`:

- **a flow into a Debt claim place** (`checking -> owed-to-ben for #power`): the place is relieved of `min(open, flow)` by the order
  above, and the target is credited the rest, as the place of a party is debited `out - settled` today;
- **a flow from an owner's money to the party's place** (`checking -> pge`, `Role::Outside(Some(party))`) where the owner has a Debt
  claim tab with that party (`Traits::tab_of(party, owner, Class::Debt)`): the tab is relieved the same way and the party's place is
  credited `arrive - settled`.

Both are `Reaches`/`Settlement` as they are for a party's payment, and `Record::settled` holds what was taken, so a returned
payment reopens it. Two things make the mirror cost no new rule: the party's place is adjusted by the *direction the claim counts
in* (a claim counted In is adjusted at the source, `paid()`; counted Out at the target, which is what `returned_claims` already
does for an Asset claim returned), and a payment is a payment only in the *forward* course (a returned bill payment is a flow
out of the party's place, which an Asset claim on that party would otherwise take for a payment).

### 2.4 Written off

`^b1 waived` (the party forgives the bill): `claim_target` accepts it; `write_off` forgives the parcels of the transaction's
lines at the Debt claim place the line paid *out of*, and gives the value back to where it went (the party's place), in the sign of
the place (`Sides::display`): the party was credited the bill and no longer is. The relief is `Fifo` over `Select::Txn`, as for a
claim, and relieves in the mirror. `Run.written_off` rows are positive, as the slices are.

### 2.5 Recognition

K3d's rule with the direction `Out`, from one function of the tab's class (`counted(class)`: Asset `In`, Debt `Out`):

| the flow is | cash | accrual |
|---|---|---|
| a bill (value out of a Debt claim place to outside) with a purpose | nothing | its purpose, as spending, whole, when made |
| a payment that settled bills | the bills' purposes, as spending, for what they were; its own for the rest | its own for the rest, nothing for what settled |
| a payment returned | the same, the other way | the same |
| a bill forgiven | nothing to reverse | the bill's purpose, the other way (less spending) |

`Dealing::Making` is asked of `makes_claim(from, to)` in the fold (`plan.rs:206`) and in the readers (`recognition.rs:105`); both
learn the Debt end. `Claiming.dir` is `counted(class)` for a settlement and the other way for a return; `Counting::forgiving` takes
the class. `Piece::takes_back` is "counted against its claim's way", which needs the book.

### 2.6 A loan and a card stay plain

Said in section 0.3: a loan's tab is the root `debt` (no `claim`), a card and a loan account are declared `debt` kinds (no
`claim`), a bill is `debt-claim` (`claim`), and a declared `payable` place says `claim` itself. `holds_parcels` is then "an Asset, or
a place that says `claim`", read from the place's four-byte traits.

## 3. The plan

| step | what | proof |
|---|---|---|
| 0 | this map | |
| 1 | acceptance tests, ignored: a bill paid in full, in parts, by code, returned, forgiven; a declared payable; a loan beside a bill; recognition cash and accrual | they fail |
| 2 | model: a tab is keyed by kind, a loan's tab is `debt`; `makes_claim` knows the Debt end; `claim_of` finds the end a place is paid out of | model tests; goldens unmoved |
| 3 | engine: `Slot::owe`, the mirror; the gate; `relieve_balance`/`arrive_balance`; `settle.rs`'s mirror; write-off; recognition | tests un-ignored; the probe books |
| 4 | report: `claims` lists by `is_claim` and the class, `owed_by_you` and the gate go; `flow`; `lots`; `open(^code)` | tests; sweep of every example |
| 5 | the oracle: `claims.py` learns debts (the owner's side), its mutants | killed or named |
| 6 | goldens: what a debt changes in the examples | listed with the reason |

## 4. Measured at `46a6c07`

`claim-debt-tab.ax` (1,000 in checking; `me owes pge 142.50 USD ^b1`, `me owes pge 50 USD ^b2`, `checking -> pge 142.50 USD ^b1`),
today: `claims` "Nothing is owed either way"; `balance` checking 857.50, liabilities 192.50 (the two bills, the paid one not netted),
net worth 665.00; `available` 857.50 (the bills are not counted at all); `check` "net worth 665.00 USD". After this lane it must say:
liabilities 50.00, net worth 807.50, `claims` one row "Owed by you" of `^b2` 50.00 USD, `available` 857.50 less nothing while
`^b2` is past due and not within 30 days.

Of the examples, the ones that write a debt of the owner are 07-landlord (`me owes summit-roofing`, and the declared `bills` and
`deposits` payables), 09-shared (`owed-to-ben`, `owed-to-cleo`), 10-budgeter (bills as contracts), `explore-v5/01-agency` (`loomfield
owes irs ...`, 20-odd taxes owed) and `02-family` (`me owes nyu-langone`); every one of them is mid-port (07: 7 errors, 09: 419, 10:
281, 02: 130, 01: 149) so their goldens say mostly the port's errors; what a debt changes in them is section 7.

## 5. How this map was checked

- Section 0.1: `grep` of `display_sign`, `Sides`, `holding.qty()`, `.balance(` and `Class::Debt` over `crates/`, each hit read.
- Section 0.2: `std.ax` (`kind payable : debt` with `claim`), `owed_by_you`, `tests/golden/*-claims.txt`.
- Section 0.3: `declare.rs:100-125`, `contracts.rs:166,251,300`, `model/tests/tabs.rs`.
- Section 1: every `holds_parcels`, `makes_claim`, `makes_debt`, `owed_by_you`, `claim_of` and `Counts::` hit read.
- Section 4: the baseline binary built from `46a6c07`, run on `claim-debt-tab.ax` and the examples; `sh tests/golden.sh` and
  `sh tests/mistakes/run.sh` reproduce byte for byte at the start commit.

## 6. What was built

| commit | what |
|---|---|
| `745488a` | this map |
| `e0c7da2` | acceptance tests of a bill as a parcel the owner owes (ignored until it is one) |
| `94d2595` | the model learns a bill (`claim_made_in`, a tab keyed by kind), the engine holds it (`Slot::owe`, relief in the mirror, `relieve_balance`, `settle.rs`, write-off, recognition), the report reads it (`claims::open`); `owed_by_you`, the `payable` gate, `makes_debt` and `Class::holds_parcels` go |
| `3e6d318` | the claims oracle holds what the owner owes to the references; the nine probe books `debt-*.ax` |
| `9792b35` | the table of tabs is a type of its own (`Tab`), and the mutants that read it say what it says now |
| `75d6139` | a payment into a declared place that is returned opens its bill again; a credit note into it is its spending refunded |
| `15e00ab` | a bill's age is the day it was made; a payment to an owner's account settles what is owed to that owner; `flow` counts a bill of a purpose that passes through |
| `d307736` | what is paid to a lender settles the bill from it and not the loan |
| `495fb2f` | golden: `07-landlord-balance`, the roof's invoice is settled by its payment |
| `bd7d60b` | a law reads what is open of a bill as what is owed (`open(^b1)`) |
| `21fcd54` | `Piece::takes_back` says it in one expression; the mutant that reads it follows |
| `0592e23` | what a debt added to every flow costs a book with no debt is nothing (section 7, timings) |

The design is section 2 as written, with four things the code taught on the way:

- **Who is a party's payment into a declared place?** A flow into a Debt place that says `claim` is a payment of what it holds whatever
  its source (`paid_into_debt`), forward or returned, so that a bill that is returned is relieved again and not made twice. A flow
  from the owner's money to a *party's place* that has a bill tab is a payment (`paid_to_party`), forward only, and only from an
  owner's own money (not from a place that says `claim`: that flow is a claim being settled by the party's side).
- **Both ends of a settlement are credited by the claim's direction.** `credit_party` adjusts the party's place by `-settled`
  where the claim is the owner's debt (`Dir::Out`) and `Claiming::paid()` adjusts the source where it is a party's claim on the
  owner (`Dir::In`); a returned payment is the other way, through the same two. No case for "debt" or "claim" beyond `claim_dir`.
- **A bill is told from a loan by the kind of its tab**, as section 0.3 said: `debt-claim` (a bill, says `claim`), `debt` (a loan,
  says nothing), `claim` (what a party owes). `World::tab` takes the kind; the class is derived (`claim` is an Asset tab, the
  others Debt).
- **`holds_parcels` is `engine::post::holds_parcels(m)`**: an end is an Asset, or a Debt place that says `claim`. The traits are read
  only for a Debt end, so an asset or an outside end costs what it did (section 7, timings).

The probe books (`docs/v5/measure/diff/cases2/`): `debt-paid-in-full`, `debt-paid-in-parts`, `debt-paid-by-code`, `debt-returned`,
`debt-forgiven`, `debt-recognition-cash`, `debt-recognition-accrual`, `debt-payable-place`, `debt-card-and-loan`, beside
`claim-debt-tab` (`me owes pge 142.50 ^b1`, `50 ^b2`, a payment of `^b1`): after the lane `claims` says one row "Owed by you" of
`^b2` 50.00 USD, `balance` liabilities 50.00 and net worth 807.50 (665.00 before), `available` 857.50.

## 7. Measured

All with release builds; the baseline binary is `46a6c07`'s.

**Goldens and mistakes.** `sh tests/golden.sh` regenerates every file byte for byte except `07-landlord-balance.txt`: the roof's invoice
(`me owes summit-roofing 14,200.00 USD`) is paid by `checking -> summit-roofing`, and the old golden counted it twice, as the balance
of `summit-roofing` (14,200.00 USD) and as a liability, beside the checking that paid it. Now the invoice is settled: the place is
gone from the table, liabilities 14,200.00 -> 0.00, net worth 154,966.60 -> 169,166.60. `10-budgeter` has no debt tab (its bills
are contracts), so its golden does not move. `sh tests/mistakes/run.sh` regenerates every `.out` byte for byte.

**The sweep** (every example through ten reports on two days, base against now). Nine files differ, all in two examples that write
a debt of the owner: `01-agency` (`irs` balance 11,893.50 -> 84.00 USD on 2026-04-16 because the taxes paid are settled, liabilities
11,785.40 -> -24.10, net worth -20,082.10 -> -8,272.60; `available` gains "Due within 30 days" for the bills still open;
`claims` lists them) and `07-landlord` (the roof, as above; `flow`'s "Unclassified" row loses the 14,200.00 USD it counted twice,
290,482.05 -> 276,282.05). Nothing else moved.

**`diff/run.sh` + `compare.sh`** (valid projects through ten commands): the nine `debt-*.ax` books and `claim-debt-tab.ax` differ
(they are the lane's), and so does `tab-implied-parties.ax`, whose `me owes zorb 12 USD due 20d` is now listed as "Owed by you"
(12.00 USD, overdue 97d) where `claims` said nothing. No other case differs.

**The oracle** (`claims.py`, 600 books: 107 of the family `debts`, with 150 bills paid, 139 forgiven, 150 payments that name a
bill, 98 into a payable, 47 of them naming a bill, 28 of them returned, 29 payments of a bill returned, 59 loans): held to the
references the build says 0 fail of 600; the baseline held to the same references fails 107 (every one of them a `debts` book);
`compare` of the baseline against the build: 493 same, 107 differ as the references say, 0 differ and should not, 0 should differ
and do not.

**Mutants.** 94 one-line mutants of the code of K3c, K3d and this lane, each built and run against the oracle and then the unit
tests of the crates it touches. This lane's: 43 in the first run (32 killed by the oracle, 5 by named tests, 6 survived); the six
were each answered: 70 (a bill returned owes again) by a generator that returns payments into a payable and a unit test; 77 (a
payment into a debt place replaces what it counts of itself) by the credit-note test; 82 (a bill settled takes back what it counted)
by the test of a returned bill's spending; 84 by the transfer-purpose `flow` test; 88 (a loan's tab is settled by what is paid to
its lender) by the loan-and-bill test; 93 and 95 (what is open of a bill is negative) by `a_law_reads_what_is_open_of_a_bill`. Two
mutants the first list had (79, 81) were removed: 79 was a redundant clause (the code was simplified, not the mutant), 81 an
argument the code no longer has. Final: every mutant killed by the oracle or by a test named in `claims.py`; none survives.

**`fuzz.py ... diff`** (1,000 mutants of the examples, seed 5): panics old = new = 0, output differs 0, regressions 0.
**`splits.py all`**: 300 projects, 8,413 commands, 0 projects differ.

**Tests.** `cargo test --workspace --release`: 1,336 pass, 21 ignored (none this lane's), 2 fail: the two of lane D
(`a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`,
`a_context_forecast_keeps_historical_and_same_day_obligations_once`), failing as at `46a6c07`. The lane adds 23 tests in
`engine/src/debt_tests.rs`, tests in `source_tests.rs`, `recognition_tests.rs`, `claim_tests.rs`, `model/tests/tabs.rs` and
`report/src/tests.rs`. Three existing tests change because the brief changes what they assert, none deleted or loosened:
`a_debt_of_the_owners_can_be_written_off_by_the_party` (it said a bill has nothing to forgive; it now forgives it), the loan
assertion of `model/tests/tabs.rs` (a loan's tab is not a claim), and `owed_by_you`'s report test (rewritten to run the fold, with
the same two facts).

**Lines** (`quality.py crates`, non-test code): 53,939 -> 54,009 (+70): engine 12,715 -> 12,838 (+123), model 18,370 -> 18,367
(-3), report 6,936 -> 6,886 (-50), the others 0. `owed_by_you`, the `payable` gate, `Book::makes_debt` and `Class::holds_parcels` are
what the report loses. No function this lane adds is over 40 lines; three it touches were over 40 at `46a6c07` and are one line longer
(`claim_ends` 69 -> 70, `lots::view_from` 53 -> 54) or the same (`available::from_ledger`).

**Timings** (`axiom check` on the bench, three runs interleaved with the baseline, the fastest of each, load average in
brackets; the machine was shared with other lanes): 100k base 0.462 s [5.4], now 0.458 s [5.4]; 1m base 4.772 s [5.4], now 4.441 s
[5.8]. More runs, alternating the order, at lower load (best of 15 at 100k, best of 7 at 1m): 100k 0.423 -> 0.417 s (-1.4%), 1m
4.629 -> 4.547 s (-1.8%). Instructions, which load does not move (`valgrind --tool=callgrind`, 100k): 1,877.6M -> 1,867.9M (-0.5%).
They were +0.4% with the gates as first written: the gate read a debt end's traits and the settlement the party's tab on every flow,
and the code index asked every flow if it makes a claim, which its own transaction needs to learn only when it has a code; the three
are cheaper now (commit `0592e23`). The bench has no claims and no bills, so what it pays is the check itself.

## 8. Not finished, and what the brief got wrong

- **`deposit` (STATUS Decision 17, Part B) is not built**: the brief does not say so (section 0, point 8).
- **A bill the monitor finds missing is not claimed** (point 9): `claim_tab` is still the party's side only.
- **`check` has no overdue for a bill**: `claims` lists one "overdue", `check` does not warn of it (the K5c monitor's business).
- **The interest leg of a loan settles the oldest bill from the same lender** (section 9, the first).
- The brief's `Sides` change, positive parcels, and `holds_parcels` "a fact of the place" are wrong in the three ways of section 0
  (points 1, 3 and 4); the brief's `10-budgeter` golden does not move (it has no debt tab).

## 9. What I am least sure of

1. **A payment to a party who is both lender and biller.** A flow from the owner's money to a place with a bill tab is a payment of
   the bills, oldest first (`paid_to_party`). A loan's schedule pays `bank` too (its interest and principal legs are flows to
   `bank`'s place); with a bill from `bank` open, the schedule's legs settle it before it is paid on its own. The loan's balance
   stays right (its tab is the root `debt`, no claim, and `loan_balance.rs` holds), but the bill is closed by a payment that was not
   for it. Telling which flows of a contract are the loan's would need the contract's name on the flow, which is K5d's.
2. **Negative parcels and the mirror.** `Slot::owes` and `mirror()` make a debt's relief the claim's, with the shortfall the other
   way (an overpayment is a credit with the party); it reuses every rule of `lots.rs` and costs about 25 lines, but a reader of
   `lots.rs` meets a slot that is negative for one kind of place. The alternative (relief written for both signs) would have
   touched K3e's file; I would revisit it when K3e has landed.
3. **Recognition's direction is a function of the class** (`claim_dir`), which means an Asset tab and a Debt tab are the whole
   vocabulary: a third class of tab would need a third arm, and the four sites that read it are one function each, held by mutants.
