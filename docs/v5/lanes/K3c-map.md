# K3c map: what a claim is in the fold, what settles it, and why an asset's parts are not parcels

Written before the first code change of lane K3c, from the code at `36ead82`, and checked against what the code does (the
last section says how). Paths are in `crates/`; line numbers are those of `36ead82`. The probe books are in
`docs/v5/measure/diff/cases2/` (`claim-*.ax`, `asset-parts.ax`); every output quoted below is the baseline binary's, built
from `36ead82`.

A **claim** is value someone owes, held as a parcel in a *claim place*: a place whose facts say `claim`, or a tab. An
**asset** is an identified thing (`asset condo : rental`) whose unit is held in a place of its own, with a table of its
**parts** (the purchase, each improvement) beside the parcel.

## 0. What the brief says and what the code does

Seven things in the brief do not match the code. The first three decide what the lane can build.

1. **There is no exact-amount shortcut to delete. Nothing in the fold settles a claim by the amount, and a payment from the
   party does not settle it at all.** Searched for it in `engine`, `model`, `report`, `sync` and `cli` (`exact`, `oldest`,
   `settle`, a quantity compared with a flow's): the only matches are unrelated (`sync/world.rs:709` compares an imported
   record to a promise's due; `Closing::Exact` is an assertion's). PROPOSAL §0 says it in one line: "paying a claim does not
   settle it". Shown on `claim-party-flow.ax`: `ann owes me` three claims (300, 200, 300), then `2026-01-20 ann -> checking 300
   USD ^i1`. After it `claims` still lists all three (800.00 USD), `checking` is 1,300.00 and net worth 2,100.00: the payment
   is a flow from *ann's own place* (`Role::Outside`), which is not where the claims are. A tab has no name (K3a section 1: "no
   name finds a tab"), so no flow can be written out of one. What the brief calls "settlement by code" is relief from a claim
   *place* by a written `[^code]` selector, and that works (section 3).
2. **A flow's own codes are a label, not a settlement.** LANGUAGE §3 says "a payment carrying the code of an open claim
   settles it", and `explain::overdue` says "record the payment `for` the claim's code". `claim-flow-code.ax` has three
   claims in a declared `receivable` place and `owed -> checking 200 USD ^i2`: it takes 200 of `^i1` (the oldest), leaves
   `^i2` whole. Only a written selector, `owed[^i3] -> checking 100 USD`, chooses a claim. `Request.selectors` is
   `Motion::select()` (`post.rs:199`) and a flow's codes are never in it.
3. **A debt of the owner is not a parcel, and `claims` does not list it.** `Class::Debt.holds_parcels()` is false
   (`book.rs:231`): what the owner owes is a plain signed balance. `claims::open` lists a debt only if its place is `Debt`
   *and of kind `payable`* (`claims.rs:86`), and a tab's kind is the root `debt` (`declare.rs:107`). So `me owes pge 142.50 USD
   ^b1` is in `balance` (a 192.50 USD liability on `claim-debt-tab.ax`) and `claims` says "Nothing is owed either way". The
   K3a map expected `owed_by_you` to serve tab debts; it serves declared `payable` accounts only.
4. **`ClaimChange` carries a description and nothing else.** `ClaimChange { day, target: Id<Txn>, action: WriteOff,
   description, loc }` (`journal.rs:864`). The lowering refuses lines and any clause but the description
   (`statements.rs:463, 472`: "a full claim write-off only accepts a description"), so the brief's "purpose" is not in the
   statement: LANGUAGE §5's `#bad-debt` and recoverable items are not lowered. The fold half is `Fact::ClaimChange(_) => {}`
   (`ledger.rs:1098`).
5. **`books cash|accrual` is parsed and never read.** `builtin::BOOKS` is written by `props.rs:114` and read by nothing in
   `engine`. A claim made with a purpose is recognized *when it is made* (the flow from the party into the tab is an inflow of
   the owner: `purpose_flow`, `post.rs:169`), which is accrual. That is why a payment from the party cannot simply be added
   to the settlement: under today's recognition it would count a second time (section 6).
6. **Every claim parcel carries an asset-part id.** `arrive` gives `fresh_part = PartId { origin: txn, ordinal }` to every
   parcel that is not money and not a transfer (`post.rs:418-424`), and a claim place is not money (`scope.rs:49`). So each
   claim writes an entry in `Holdings.part_slots` and walks the queue of pending wash-sale carries
   (`match_pending_carries`, `post.rs:820`). Nothing reads the id of a claim. It is also part of the parcel's identity
   (`lots.rs:105`), so two flows of one transaction (an itemized invoice: a header and an item) are two parcels because
   their ordinals differ; dropping the id would merge them and change `lots` and `claims`. Left as it is.
7. **A claim placed by a transfer of money is not a claim of its own.** `claim-receivable-merge.ax`: `checking -> owed 300
   USD due ... ^i1` (and two more) moves parcels of the money `checking` held, whose identity is the market arrival's. `claims`
   shows one parcel of 600.00 USD, made 2026-01-01, "What: c.ax:7". Only value that arrives from outside (`ann -> owed`,
   `ann owes me`) is a parcel of its own. This is the chart of accounts coming back (PROPOSAL F6); the tab path does not
   have it.

## 1. A claim, from its making to its end

### 1.1 Creation

| step | where | what |
|---|---|---|
| the statement | `lower/record.rs:1242` `lower_owes` (`X owes Y`, in the journal or an opening) | key `(party, owner, class)`: `Asset` when the creditor is an owner, `Debt` when the debtor is, else `Asset`. `world.tab(..)` finds or makes the tab (`declare.rs:96`): a root place with `role: Tab(party)`, `kind: asset|debt` (the root kinds), `owner`. Makes one `Flow` from the party's place to the tab (`Asset`) or the tab to the party's place (`Debt`), a `Txn` with the statement's codes |
| a flow with `due` into a declared place | `lower/tail.rs:182` | the `Detail.due` of the flow; the place says it is a claim by its kind (`kind receivable : asset claim`, `std.ax:30`) |
| the fold | `post.rs:395` `arrive`, `lots.rs:364` `land_with_codes` | the slice that arrives lands as a parcel: `qty`, `basis` (what the flow fetched), `acquired` (`since` or the day), `txn` (the flow's), `codes` (`m.code_runs`), `part` (section 0.6). Parcels that agree on the identity (`lots.rs:105`: acquired, held_since, wash flag, txn, tie, part) and on their code sets merge |
| what says it is a claim | `said.rs:168` `is_claim`: the fact `claim` **or `Role::Tab`**; read once into `PlaceTraits.claim` (`traits.rs:51`) | a claim place is not money (`scope.rs:49`): its parcels are told apart by the purchase, not by basis per unit |

Aging and blame are not stored. **Aging is the parcel's `acquired`**: `claims.rs:80` (`made: lot.acquired`, the `Age` column
is `at.since(claim.made)`, `claims.rs:198`) and `explain.rs:928` `overdue` (`today.since(lot.acquired)`). The due day and the
payee (the one blamed) are read from the flow that made the parcel: `Book::paid_into(lot.txn, place)` (`book.rs:1689`) gives that
flow, whose `payee` and `detail().due` are the claim's. So a parcel is a claim by its `txn` and its place; nothing is added.

### 1.2 Settlement: what exists

Relief at a claim place is the ordinary relief of `lots.rs`, because a claim place is `Class::Asset` (`post.rs:190`):

| rule | where | in force |
|---|---|---|
| a written `[^code]` | `Select::Code` (`journal.rs:577`), `Selection::admits` (`lots.rs:782`): the parcels whose `codes` (header or local) contain it; a filter | yes (`owed[^i3] -> checking 100 USD` on `claim-flow-code.ax` takes it from `^i3`) |
| a written `[date]`, `[unit]`, `[#purpose]`, `[end]` | the same | yes |
| a flow's own codes | nowhere | **no** (section 0.2) |
| exact amount | nowhere | **no** |
| the place's policy, then the commodity's | `post.rs:201`: `place.select.or(unit_select)`; `currency` kinds say `select fifo` (`std.ax:46`), so a USD claim is FIFO; a claim of any other commodity has no policy and is **ambiguous** (`Relief.ambiguous`, `lots.rs:495`) | yes |
| FIFO | `relieve_in_order` (`lots.rs:492`), `take_run` | yes |

So today the order is: written selector (a filter), then the policy, which for a USD claim is oldest first. Where relief takes more
than the place holds, the shortfall is taken from plain value and "a holding can go negative" (`lots.rs:454-457`).

### 1.3 Write-off

| step | where | what |
|---|---|---|
| grammar | `syntax` `^CODE waived ["why"]` | |
| lowering | `lower/statements.rs:454` `lower_claim_change`, `:485` `claim_target`: the code resolves to one transaction (`CodeUse::ClaimWaiver`), which must have a flow into or out of a **`Role::Tab`** place (`:495`) and not be dated after the statement | `Book.claim_changes: Vec<ClaimChange>` (`book.rs:120`), in the order the statements are lowered, i.e. `(day, source order)` |
| ordering | `timeline.rs:56`, stream `Stream::ClaimChange` (`:237-259`, `:385`); `start` and `last_fact` count it (`:118`, `:133`) | `Fact::ClaimChange(index)` |
| the fold | `ledger.rs:1098` | `{}`. Nothing is done: `claim-writeoff.ax` (`^i1 waived` on 2026-02-15) still lists `^i1` at 300.00 USD, overdue 28d, and `balance` 500.00 USD in `ann`; `why ^i1` knows the flow and not the waiver |

What `Fact::ClaimChange(u32)` carries is an index into `claim_changes`: the day, the target transaction, the action (only
`WriteOff`: "forgive the remaining amount of the referenced claim"), the description and the line. No amount, no place and no
purpose.

### 1.4 What reads a claim

| reader | where | asks |
|---|---|---|
| `claims` | `report/claims.rs:67` `open` | `book.is_claim(place)` (the fact **or the role**), then each lot: `lens.place_qty`, `paid_into` for payee and due. Debts: `place.class == Debt && is_a(place.kind, payable)` (`:86`) then `owed_by_you` (`:95`) |
| `owed_by_you` | `claims.rs:95` | the flows touching the place (`Book::touching`), per first code: a flow *out of* the place opens a debt of its amount, a flow *into* it carrying (or selecting, `settled_codes`) that code reduces it. It rebuilds from flows what a parcel holds for an asset: the Debt class holds a plain balance |
| `finish` | `ledger.rs:1030` | every slot of a claim place (`traits.place(p).claim`), each lot with `qty > 0`, to `explain::overdue` |
| `open(^code)` in a law | `eval.rs:1414` | claim places, lots whose codes contain it |
| `lots`, `balance`, `available` | `report` | the holdings; `available` counts claims through `claims::open` |
| `claim_target` | `statements.rs:495` | `Role::Tab` on a flow end |

## 2. The order of a day, and why `ClaimChange` is after the movements

`timeline.rs:3-12`: `Split`, `Settle` (a pending flow lands, or an actual one is returned), `Source` (journal flows and written
occurrences, by transaction then flow), **`ClaimChange`**, `Assert`, `Deadline`. The order is `#[derive(Ord)]` over `Fact`'s
variants, so it is the declaration order (`timeline.rs:50-63`), and `source_transactions_interleave_by_transaction_before_claim_changes`
(`:466`) keeps it.

Why a write-off comes after the day's movements, from what each neighbour needs:

- **After `Source`.** A write-off forgives what is *still open*. A payment or a claim dated the same day has to be applied (a
  claim written on the day it is forgiven exists, and the lowering allows it: `date < source.day` is the only refusal); a
  settlement of the day relieves first, so the write-off sees the rest.
- **Before `Assert`.** A balance assertion of the day sees the holdings after the write-off.
- **Before `Deadline`.** A month or year that closes on the day (a law that counts or limits) sees it.

Nothing in this lane changes the order.

## 3. The lot's fields

`Parcel` (`lib.rs:348`, 9 fields), set at arrival (`post.rs:435`), read by relief (`lots.rs`):

| field | made by | read by |
|---|---|---|
| `qty`, `basis` | the slice; `Shares` split a flow over slices | everything; relief takes `basis.share(qty, lot.qty)` |
| `acquired` | `since` or the flow's day | order of the lots (`insert`, `partition_point`), `Select::Range`, identity, **aging** |
| `held_since` | `since`; a wash-sale carry tacks an earlier day | `Realized.held` |
| `wash_matched` | a carry (`CarryLotBatchAdjustment::apply`) | identity; the candidates of `fire.rs::carry` |
| `txn` | the flow's `RuntimeTxn` | identity; `claims::open` and `explain::overdue` (the flow that made the claim), ambiguity listing |
| `part` | an acquisition part's id, or `{txn, flow ordinal}` for any other parcel that is not money (section 0.6) | identity; `Holdings.part_slots`; `fire.rs::carry`; `Realized.part` |
| `codes` | `m.code_runs` of the flow | `Selection::admits`; `open(^code)`; `same_codes` for merging |
| `tied` | a restricted party or `for` | relief colours (`Colour`), `on spend` |

## 4. The asset store, field by field

The question: is each field (a) already a field of a parcel, (b) derivable, or (c) state a parcel would have to grow?

`Parcel` has no field for any of the (c) rows below, and the first row is why the question has an answer.

### 4.0 Why it cannot be one parcel per part: an improvement has no quantity

An asset's unit is `Qty(1)`: one quantum, indivisible. An improvement "adds basis without another unit"
(`an_improvement_adds_basis_without_changing_the_asset_quantity`, `source_tests.rs:814`, asserts the holding is 1). On
`asset-parts.ax` (a purchase and two improvements) `lots` shows one parcel, `1 condo`, and `why condo` three parts:

```text
Parts
  Part          Acquired            Cost       Basis   Consumed  From
  #purchase     2025-01-02  1,000.00 USD  970.00 USD  30.00 USD  asset.ax:17
  #improvement  2025-02-15    120.00 USD  100.00 USD  20.00 USD  asset.ax:18
  #improvement  2025-03-15     60.00 USD   50.00 USD  10.00 USD  asset.ax:19
```

A parcel per part would make the two improvements parcels of quantity **zero** and basis 100 and 50, and `lots.rs` is built on
the opposite: quantity zero is exhausted. `Slot::insert` counts a lot of quantity zero as `dead` (`lots.rs:397`);
`sweep_ends` pops it off the end and `tidy` removes it (`:713`, `:729`); `live()` and every relief loop skip it
(`lot.qty.is_zero()`); a sale takes by quantity, so selling the one unit takes the purchase's parcel and **leaves the
improvements' basis behind, swept away unseen**. Making "a sale relieves every part" true needs the relief code to know that a
zero-quantity lot with a part id belongs to the unit being sold: `lots.rs` would branch on "is this a part", which the brief
names as the reason to stop. Giving the parts a share of the unit instead (the purchase 1/2, an improvement 1/2) cannot
be: the unit is one quantum, the report prices `Amount::new(Qty(1), unit)` (`why/asset.rs:25`), and `an_improvement_adds…`
asserts 1.

### 4.1 The fields

| store | field | what it is | verdict |
|---|---|---|---|
| `Part` (`assets.rs:64`) | `id: PartId { origin: RuntimeTxn, ordinal }` | the part's key | **(a)** for the acquisition: it is `Parcel.part` of the anchor parcel. An improvement has no parcel, so its key exists only here |
| | `flow: Option<Id<Flow>>` | the written flow, for `why` | **(b)** for a journal flow (from `id.origin` and the ordinal); `None` for an occurrence or an applied flow, where the cause is gone once the fold has moved on |
| | `kind: Acquisition | Improvement` | which part is the unit's own | **(b)**: the first part of an asset is the acquisition. But "first" is a position in the table that this field exists to keep |
| | `recorded: EventKey { day, sequence: u64 }` | where in the day's order it entered: `held_at`, the disposal boundary, `dispose`'s check | **(c)**: the `sequence` is the ledger's event order (`post.rs:675`: a flow's id, a transaction's index shifted with the ordinal, or the applied counter). A parcel has `acquired` (a day) and no order within it |
| | `day: Day` | the acquisition or in-service day | **(a)** for the acquisition (`Parcel.acquired`, which can be earlier than `recorded.day`: an opening's `since`); **(c)** for an improvement, which has no parcel |
| | `cost: Qty` | the original cost, never reduced | **(c)**: a parcel has `basis`, which falls with consumption. It is not derivable from it (a stated `basis` differs from the price paid; `capital_cost`, `post.rs:650`) |
| | `basis: Qty` | cost less consumption plus what a carry added | **(a)** for the sum over parts: the anchor parcel holds the *total* (`add_improvement_part` adds the improvement's basis into the anchor, `assets_runtime.rs:90`). **(c)** per part: depreciation is capped by each part's own remaining basis (`prepare_consumption`: `applied = min(requested, part.basis)`) and `why` lists it |
| `Disposal` (`assets.rs:83`) | `txn`, `flow`, `boundary: After(EventKey) | Close(Day)` | that the unit was sold, by what, and where in the order | **(c)**, three fields. After a sale the parcel is gone (relieved), so nothing in `Holdings` says it was held. `held_at` (laws run for a part only while the asset is held), `report/history.rs:231` (the asset's span in a replay) and `why asset` read it. A history of the position (K7) would derive it; there is none yet |
| `PendingCarry` (`assets.rs:93`) | 11 fields: `law`, `from`, `cause`, `owner`, `unit`, `sold`, `held_since`, `within`, `quantity`, `amount`, `codes` | a disallowed loss waiting for a replacement acquisition inside the window | **(c)**, and **not an asset's**. `fresh_part` gives every non-money, non-transfer parcel a part id (`post.rs:418`), and `fire.rs::carry` matches securities lots, so the queue belongs to the lots of any commodity. A deferred loss is not value held: there is no parcel to hold it |
| `AssetState` (`assets.rs:111`) | `asset`, `parts: Vec<Part>`, `disposed` | | `asset` is its index; the other two are the rows above |
| `Assets` | `part_index`, `pending_carries` | a derived index; the queue | derived; the queue as above |

Count of **(c)**: `recorded.sequence`, `cost`, the per-part basis, `kind` for the improvements, the three of `Disposal`, and the
eleven of `PendingCarry` (which are not the asset's). That is more than a few, and the first one (a part is a parcel only if a
parcel may have no quantity) is not a field.

### 4.2 What the two stores cost today

The parts table and the parcels keep one fact twice and check it at every operation: `anchor_and_total` and
`holdings.part_basis(anchor) != total` guard `add_asset_part`, `consume_asset_part`, `carry_asset_basis` and
`carry_basis_to_parts` (`assets_runtime.rs:61-206`), each followed by two RAII guards and a committed pair
(`ConsumptionGuard`, `CarryGuard`, `AssetBasisBatchGuard`, `PartBasisAdjustment`, `CarryLotBatchAdjustment`). Non-test lines:
`assets.rs` 519, `assets_runtime.rs` 159, the part code of `lots.rs` about 230 (`part_slots`, `PartBasisAdjustment`,
`CarryLot*`, `prepare_part_*`), the asset arms of `post.rs` about 330, `fire.rs` (`consume`, `carry`, `deadline`,
`pre_disposal`) about 250, the readers in `eval.rs` about 100 and in `report` about 70. The proposal's "about 1,000 lines go"
counts the first two and a part of the third; the behaviour of the rest (a law consumes each part by its own remaining
basis, a wash sale matches by quantity and day, a sale is judged after the day's consumption) does not go, it moves.

### 4.3 The verdict

**No.** Not "a few fields": the table's real content is basis per part with no quantity, and a parcel cannot say it without
`lots.rs` branching on whether a lot is a part. The lane does only claims (section 7) and leaves `assets.rs`,
`assets_runtime.rs` and the asset arms of `post.rs`, `fire.rs` and `eval.rs` as they are. Asset parts: no change.

The smaller cut, for a later lane: (1) a part's current basis is `cost + carried - consumed`, which `Run.adjustments` already
records per part (`AdjustmentKind::Consumed`, `Carried`), so `Part.basis` and the three guards could go if a part's remaining
basis is cached beside its cost; (2) `PendingCarry` and `Holdings.part_slots` belong to the lots (they serve securities), not
to `Assets`; (3) `Disposal` is a derivation of the position's history, which K7 builds. None of the three needs a parcel per
part, and (1) changes the arithmetic that caps a depreciation, so it needs the asset oracle (investor and landlord) and a lane.

## 5. The prorata failure: arriving basis per kind of place

The failing assertion is `held.plain == 5,188.24`, got 0 (`tests.rs:1110`). The sale's arithmetic is right: the lines after it
(`held.lots.len() == 1`, the lot's 1,811.76 left, the one gain of 388.24) are the ones the test expects. What fails is the
**arrival** of the 6,300.00 USD that `checking -> retirement` moves into a `basis zero` place that is not deferral money. The
STATUS file calls the question "whether a prorata sale carries basis per unit or by exact share"; it is not about the sale.

The rule is one line, `post.rs:278` (added by `fc58a78`, "Handle basis-zero HSA funding"):

```text
(None, _, _, _) if unbased && !from.deferred => Qty::ZERO     // before the arms that keep a parcel's basis
```

Whatever arrives in a `basis zero` place from a source that is not `deferred` has no basis unless the flow states one. The
test's `retirement` is deferred and `basis zero`, `checking` is neither, so the 6,300.00 of after-tax money arrives with no
basis, is not plain money (basis equal to face), and merges with the zero-basis deferrals into one lot of 8,500.00. The HSA
test (`hsa_basis_zero_contribution_and_against_medical_reimbursement`) wants exactly this for `checking -> hsa`, so the two
tests ask opposite things of the same flow.

**The two readings.** LANGUAGE §9 says: "Arrival from a party or `?` creates a parcel ... An account kind that says `basis zero`
gives none (pre-tax deferrals)" and "Transfers between an owner's holdings move parcels unchanged".

| | R1: a transfer from a taxed place into a `basis zero` place is a contribution with no basis | R2: a transfer moves the parcels with their basis; `basis zero` is for what arrives from a party |
|---|---|---|
| the rule | today's `post.rs:278`, kept | delete it: the arms below it carry `slice.basis` when the parcels keep their identity |
| after-tax money (a nondeductible IRA contribution) | must say `basis AMOUNT` on the flow (`(Some(basis), ..)` already wins) | arrives with its face value as basis and is plain money, unasked |
| a pre-tax HSA or 401k contribution written as a transfer from checking | no basis, taxable when withdrawn, as the HSA test says | would keep its face value as basis: it has to be written as arriving from a party (`payroll -> hsa`) or with `basis 0 USD` |
| the failing test | its fixture gains a stated basis on flow 2 (a test edit) | passes as written |
| the HSA test | passes as written | **fails**: the withdrawal realizes nothing, no distribution, no penalty |
| REMAINING.md | "Native IRA rules require explicit basis for nondeductible contributions" | contradicts it |

Measured on a scratch copy with the line removed (R2), `cargo test --workspace --release`: the prorata test passes and the HSA
test fails, and nothing else changes. Of the 60 golden files, **4 change**: `04-freelancer-available` (September's federal
estimate 3,166.79 to 2,206.79 USD), `05-family-available` (the early-withdrawal and HSA penalties: alex-401k 19,313.34 to 17,815.31,
HSA 2,708.84 to 1,759.11, jordan-401k 12,062.94 to 11,478.90), `05-family-check` (the HSA reimbursement's `!` becomes an `unused-waiver`
warning, because the penalty it waived no longer arises) and `05-family-tax` (distributions 5,596.20 to 392.94 USD from 14
sources to 2, total income 223,668.93 to 218,465.67).

**This is a decision for the user, and the lane does not take it.** R1 costs a test edit and keeps the HSA behaviour; R2 follows
LANGUAGE §9's text and moves the penalties and the taxable income of two examples. Either way the engine's prorata relief,
which this lane does not touch, is correct.

## 6. What settlement by a party's flow would need

LANGUAGE §7 says a later flow *between* the debtor and the owner settles open claims: those its codes name, else the one
exactly the flow's, else the oldest, and what remains is an ordinary flow. The fold has the first and last as relief and none
of the connection from a party's flow to the tab (section 0.1). Building it is three things, and the lane builds none:

1. **A flow from the party must relieve the tab for the part it settles and be an ordinary flow for the rest.** That is one flow
   posted as two movements, and a flow that is *returned* (`Fact::Settle` with `State::Returned`) must undo both. `Record::resolved`
   remembers one `Amounts` per flow.
2. **Recognition has to know which of the two the owner's books say.** A claim made with a purpose is income when it is made
   today (section 0.5). If the payment is also a flow from the party, income counts twice; if the settled part is an internal
   transfer, the payment is not income, which is the accrual reading. The cash reading is the opposite: the claim's making
   counts nothing and the payment counts. `books` chooses, and nothing reads it.
3. **A debt is a plain balance** (section 0.3), so "the claim whose open amount is exactly the flow's" has no parcels to
   compare for what the owner owes.

The smallest design is a `Plan` index from `(party place, owner)` to the tab, a settling movement made in `Ledger::post` before
the ordinary one, and `books` read once into the traits. It is a decision (cash or accrual) before it is a lane.

## 7. What the lane builds

Claims, in this order, each behind acceptance tests written first and ignored until the code lands:

1. **`Exact` is a relief policy.** `Policy::Exact` (the syntax's table `Policy::WORDS` gains `exact`, `Coded` gains the word):
   the parcel whose quantity is exactly what is asked, if there is one, else the oldest. `relieve_in_order` takes it before
   the run; `relieve_scanning` sorts it first (`by_policy`, with the need). A claim place's default policy is `Exact`
   (`Traits`: `place.select` unset on a claim place is `Exact`, before the commodity's), so the order in §7 is: a selector's
   code, then exact, then the oldest. A claim place is no longer "ambiguous" when it holds differing parcels: a rule chooses.
2. **A flow's own codes name claims at a claim place.** In `relieve`, for a claim source with no written code or date
   selector, each code of the flow that some parcel of the slot carries is a `Select::Code` for this request. A code that
   names nothing there is a label, as before.
3. **Write-off is relief.** `Fact::ClaimChange` relieves every parcel the target transaction made, at every claim place its
   flows touch, and the value leaves the owner for the party's place (conserved). It is recorded in `Run.written_off` with the
   statement's description; `claims`, `check` (`explain::overdue`) and `balance` see it because they read the parcels. A
   write-off that finds nothing open says so (a warning, as an unused `!` does). The target must be a claim that is a
   parcel: `claim_target` refuses one that made a debt of the owner's (a plain balance, section 0.3), which today it
   accepts and does nothing with.
4. **The readers ask the place.** A tab is given a built-in kind that says `claim`, so `said.rs::is_claim` is the fact and
   `claim_target` asks `is_claim` of the place; `claims.rs` lists a debt by `is_claim` and its class and no longer by the
   `payable` kind. The last line changes what `claims` lists for tab debts (section 0.3), and the report lists it.

Not built, and said: settlement by a party's flow (section 6); reversing what a written-off claim recognized (needs `books`);
a purpose or items on a write-off (the statement does not carry them); assets (section 4).

## 8. How this map was checked

- Section 0.1 to 0.3 and 0.7, 1.2 and 1.3: the books in `docs/v5/measure/diff/cases2/claim-*.ax`, each through the baseline
  binary's `claims`, `lots`, `balance`, `check` and `why`.
- Section 4: `asset-parts.ax` through `lots` and `why condo`; the lines of `lots.rs`, `post.rs`, `assets_runtime.rs` and
  `fire.rs` that the verdicts cite, read in full; every call site of `world.assets`, `Part`, `PartId` and `PendingCarry` found by
  `grep` over the workspace (the readers in `eval.rs`, `report/history.rs` and `report/why/asset.rs` are the ones above).
- Section 5: a scratch copy of the tree with the special case removed, and the workspace's tests and goldens run on it.
- `cargo test --workspace --release --no-fail-fast` at `36ead82`: 928 passed, 3 failed, 19 ignored; the three failures are the
  known ones.
