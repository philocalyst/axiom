# K3f map: a debt is a parcel, and paying a bill settles it

Written before the first code change of lane K3f, from the code at `46a6c07` (the head after K6b), and checked against what the
code does (section 8 says how). Paths are in `crates/`; line numbers are those of `46a6c07`. The vocabulary is K3c's and K3d's
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
281, 02: 130, 01: 149) so their goldens say mostly the port's errors; what a debt changes in them is section 6 of the report.

## 5. How this map was checked

- Section 0.1: `grep` of `display_sign`, `Sides`, `holding.qty()`, `.balance(` and `Class::Debt` over `crates/`, each hit read.
- Section 0.2: `std.ax` (`kind payable : debt` with `claim`), `owed_by_you`, `tests/golden/*-claims.txt`.
- Section 0.3: `declare.rs:100-125`, `contracts.rs:166,251,300`, `model/tests/tabs.rs`.
- Section 1: every `holds_parcels`, `makes_claim`, `makes_debt`, `owed_by_you`, `claim_of` and `Counts::` hit read.
- Section 4: the baseline binary built from `46a6c07`, run on `claim-debt-tab.ax` and the examples; `sh tests/golden.sh` and
  `sh tests/mistakes/run.sh` reproduce byte for byte at the start commit.
