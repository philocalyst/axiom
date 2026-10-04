# K6b map: a law that fires on a flow that has posted, and derives another

Written before the first code change of lane K6b, from the code at `990ddb5` (K7b merged), and checked against what the code does
(section 12 says how). Paths are in `crates/`; line numbers are those of `990ddb5`. The vocabulary is K6's
([`K6-map.md`](K6-map.md) sections 3 to 5 and 13: `Effect::Derive`, `Rules` keyed by `Watch`, the occurrence host in
`engine/src/occurrence/derive.rs`), K3d's (`engine/recognition.rs`: what a flow counts toward), K5c's (the forecast is the fold)
and K7b's (`engine/histories.rs`: the fold records every balance it changes).

K6's map says the post host needs four things: a transaction identity for a derived flow, a pool the run keeps them in, a guard,
and an order (section 13.4 there). Reading the code shows **the brief is right about the four and wrong about two of the reasons**
(section 0, points 2 and 3), and **wrong about the shape of the owner list** (point 5): a kind's, an entity's and an account's
`on flow` has no table to be looked up in, and `self` in the one example LANGUAGE gives would mean the wrong place.

## 0. What the brief says and what the code does

Eight things in the brief, in K6's map or in its report do not match the code. Each decides something below.

1. **No law fires on a posted flow with a `derive` step today, and none can be written.** `Outcome::Derive` reaches
   `fire.rs:597`, which faults it as `InvalidProgram` ("a law that derives is read when an occurrence is made, and the model keeps it
   out of every table a posted flow fires"). It is kept out by the compiler: `laws/compile/derive.rs:87` refuses a `derive` that is not
   in a contract (`derive-owner`), and `laws/mod.rs:259` (`declare_alsos`) turns every `also` of a kind, entity, purpose or account
   into the warning `also-inert` and makes no law. So there is nothing to unhook from a table: the lane **lowers an `also` under
   every owner into a law**, and routes it like the owner's other laws; the effect handler at `fire.rs:597` becomes the queue.
2. **The "transaction identity" is not a record for the views; it is the identity of the parcels a flow lands.** `RuntimeTxn`
   (`model/journal.rs:156`) is read by `Parcel.txn` (every parcel is stamped with it, `post.rs:556, 709`), by `PartId { origin, ordinal }`
   (an asset part is named by it, `post.rs:546, 625, 689, 742`), by `Book::claim_of` (a claim parcel's payee and due day) and by
   `contract_of` (`post.rs:174`: which contract's laws and totals a flow belongs to). No report reads it (`why`, `register`, `flow`
   read `Flow`, `Cause` and the run's lists). A derived flow needs its own, because it lands parcels and may start a part, and
   `(txn, flow_ordinal)` must be unique: **`RuntimeTxn::Derived(Id<Offspring>)`**, numbered by the fold. A derived flow is not of
   its cause's contract (`contract_of` says `None`): the flow a hand-written line would be is no occurrence either, and the oracle
   holds the two equal.
3. **"A record pool the forecast replays" is not what a forecast does, and an index into a pool is not an identity.** A forecast
   does not replay a record: it is the fold (K5c), so a promised occurrence's flows post through `post_made` (`promising.rs:278`)
   to the one `post`, and whatever `post` derives, it derives. What the forecast needs is that nothing be done twice. The pool is
   not stable across a fork, though: `Record::forked` (`state.rs:158`) starts every list empty, so a pool index of the resumed
   ledger restarts at 0 while the parcels of the checkpoint, which carry the old identities, live on in the world. An identity must
   therefore be a **number the world carries** (`World.offspring`, cloned with every checkpoint and fork, like `Clock.applied`), and
   the pool says where it begins.
4. **A returned flow reverses what it derived, but the fold already knows which flows will be returned.** `Events::state`
   (`events.rs:25`) is known before the fold starts, and `Fact::Settle` (`ledger.rs:476`) posts a returned flow's reversal
   (`post_journal(.., reversed)`, `ledger.rs:496`). Only a flow the plan says is `Returned` needs its derived flows remembered, so the
   memory is a map of **the few flows that will be returned** (`Record::returnable`, kept by `forked`, as `Record::settled` is, because
   a return dated after a forecast's checkpoint reverses what was derived before it), and costs nothing for any other.
5. **There is no table a kind's, an entity's or an account's `on flow` can be looked up in, and `on flow` is refused for them.**
   `laws/mod.rs:333` (`fits`) allows `on flow` for a purpose, an asset, a contract and an asset kind only; `rules.rs:107`
   (`place_watch`) maps `Trigger::Flow` to no row for a place. A purpose's goes in `Watch::Purpose` (`rules.rs:259`), an asset's in
   `Watch::About` (`rules.rs:275`) and both are already fired by `fire_purpose` (`post.rs:155`). The other four owners are a flow
   **at a place** (an account, an account kind, an entity standing at it, an entity kind): a new row, keyed by place and fired
   for either end, `Table::Touching` / `Watch::Touching(place)`. K6's map listed "a place" among the owners and did not
   say it needs its own row.
6. **`self` in LANGUAGE's example is the governed place, and K6 reads it as the flow's own end.** `kind card / also issuer -> self 2% of
   amount #rebate` means the issuer credits the card. K6's `implied_end` (`laws/compile/line.rs:252`) makes `self` and an omitted end
   the same thing, "the end the implying flow has at that position", which is right for a contract and, for a charge `card -> shop`,
   makes the derived flow `issuer -> shop`. The lane gives an end three states (`Stand::Flow`, `Stand::Subject`, `Stand::At(place)`),
   and `self` means the subject in the laws whose `self` is a place or an entity, and stays positional in a contract's.
7. **A carved item cannot be derived after the fact, and neither can a `-` item with no purpose or an added one with none.** K6's
   host reads `Derived::makes_flow()` (`book.rs:784`): a flow of its own, or an item that has a purpose or an owner. A carved item
   (`also 5% #fee`, no sign) shrinks the header, which has moved. The lowering says so for a law that fires on a posted flow
   (`derive-carve`, an error naming the fix: write `+`, `-` or a flow of its own), and K6's contract path is untouched.
8. **A pad and a flow the monitor makes are the run's own flows and cause nothing.** `Motion::pad` (`motion.rs:152`) closes a gap so that
   the books equal the statement; a derived flow after it would break the assertion it was posted to satisfy. `claim_missed`
   (`claims.rs:44`) posts a claim the monitor makes. Both carry `Cause::Time`, which derives nothing. A flow returned derives
   nothing either: its derived flows are reversed, not derived again (section 5).

## 1. The order today, and where the derived flow sits

`Ledger::post` (`post.rs:72`) in the order it does things, and the line of each:

```text
 post(m)                                              a root flow: a journal flow, an occurrence's, a hypothetical one
  ├─ accept_waiver                              :76
  ├─ settle_claims / relieve the claim place    :77   (settle.rs: only a journal flow, Cause::Flow, settles anything)
  ├─ count_leaving                              :80   totals, purposes counted (recognition); fire  on out  at the source
  ├─ relieve · price · arrive                   :82-93   the value moves
  ├─ record_capital_outflow                     :95
  ├─ fire_arrival                               :98   on in at the target ─ NEW: flow laws at the source, then the target
  │     (post.rs:138)                                    ─ the purpose's laws per piece, an asset's ─ the contract's judging laws
  │                                                      ─ on spend ─ always at both ends
  ├─ dispose_sold_asset                         :101
  ├─ record_balances (K7b)                      :103   the histories take every position that moved
  └─ NEW: drain what the laws derived, in the order they derived it; each posts through all of the above
```

**The drain is a loop, not a recursion.** `post` is split in two: `post_flow(m)` is the body above, and `post(m)` is `post_flow(m)`
then `while let Some(next) = queue.next() { post_flow(next) }`. A derived flow is queued **by value** (`Waiting`: the `Flow`, the
law, its parent `Cause`, its `Lineage`) by the effect handler that is now `fire.rs:597`, and `post_flow` of a derived flow queues its
own at the back: first in, first out, so all the flows one flow derives post before any they derive in turn. The stack depth is the
depth of `post_flow`, not of the book. No `post` calls another (the six callers, `grep "\.post("`: `apply_view`, `post_journal` twice,
`post_made`, `claim_missed`, `reconcile`), so there is one queue per ledger and it is empty between facts.

Each derived flow's `post_flow` ends in `record_balances`, so the K7b histories take derived flows as steps without a hook of their
own (the oracle `crates/session/tests/histories.rs` holds them to it).

**The firing slot decides only the order of derived flows.** The laws that derive fire where the owner's other laws do: a purpose's and
an asset's in `fire_purpose` (so, per piece K3d's recognition cuts the flow into: a cash payment of an invoice derives from the part
that counts as `#design`, and a claim's making derives nothing), the new place rows right after `on in`. `skip_internal` applies to the
new rows as to `on in`/`on out`: value that moved around inside what the law governs entered and left nothing.

## 2. Which law watches what

| written under | fires for | row | read by |
|---|---|---|---|
| a purpose | a flow for it or a purpose beneath it, per piece | `Watch::Purpose` (exists) | `fire_purpose` |
| an asset, an asset kind | a flow `of` the asset | `Watch::About` (exists) | `fire_purpose` |
| an account | a flow at it or beneath it, either end | `Watch::Touching(place)` **new** | `fire_touching` |
| an account kind | a flow at a place of the kind or a subkind | the same | |
| an entity | a flow at a place it stands at: a party's own outside place, an owner's places | the same | |
| an entity kind | the same, for the entities of the kind | the same | |
| a contract | an occurrence, **before** it posts | `Watch::Occurrence` (K6) | the occurrence host |

`Touching` has one row per place, keyed like `In`/`Out` (the key spaces lie end to end, `law.rs:803`): the laws of the place and of
its ancestors, of its kind chain, of the entity standing at it and of that entity's kind chain, that say `on flow`, in dependency
order. A law that governs both ends of a flow is found twice and **derives once**: a step derives once for a flow (the queue holds one
entry for each `(parent, step)`), so a flow between two cards pays one cash back, not two. The entity standing at a place is the party
of `Role::Outside(Some(party))`, and the owner (`Place.owner`) of every other place. That is a function of the place, so it is
decided when the rows are built; the engine asks it again only to resolve `self`.

`on flow` becomes valid under every owner a place or an entity can be (`fits`), and for hand-written laws too: `also LINE` stays
sugar for `on flow` / `derive LINE` (K6's invariant), so the hand-written law has to exist beside it. A hand-written `on flow` that
only judges (`warn amount <= 500 USD`) under an account therefore works as a consequence, and is tested.

## 3. The queue, the lineage and the cycle

```text
   flow F  (a journal line, line 12)                      lineage []                     law: card-cash-back  (kind card)
     └─ derives D1  issuer -> card  2% #rebate            lineage [card-cash-back]
          └─ would derive D2: card-cash-back again        STOPPED: it is already in the chain       ← the diagnostic
          └─ derives D2  issuer -> rebates  #fee          lineage [card-cash-back, rebate-fee]      (law rebate-fee, purpose rebate)
               └─ would derive: rebate-fee again          STOPPED
```

`Lineage` is the laws a derived flow descends through, root first: a fixed array of `DEPTH = 8` law ids and a length (36 bytes, `Copy`).
Its only constructor is `Lineage::ROOT`, and its only way to grow is `then(law) -> Result<Lineage, Stopped>`, which says `Cycle` if
the law is already in it and `TooDeep` if it is full. A `Waiting` has a `Lineage` field and cannot be built without one, and the
effect handler can only get the next one from `then`: the bound and the cycle check are the same call, and cannot be forgotten.

What a stop says (`derive-cycle`, an error, once per distinct chain: `Record::cycles: Set<Lineage>`):

```text
error[derive-cycle]: the laws of this book derive each other's flows without end
   ╭─[book.ax:14:3]
12 │ 2026-03-05 checking -> shop 100.00 USD #groceries     ← the flow that started it
   ├─[book.ax:3:3]
 3 │   also issuer -> self 2% of amount #rebate            ← 1. `also` of kind card derives from it
   ├─[book.ax:7:3]
 7 │   also + 5% of amount #fee for lender                 ← 2. `also` of purpose rebate derives from that
   ├─[book.ax:3:3]                                         ← 3. and `also` of kind card would derive from that, again
   = note: each law derives once for a flow and for each flow it derives; the third derivation was not made
   = help: make one of the laws say what it does not apply to: `when from is not card`
```

`derive-depth` is the same for a chain of eight different laws. Both are the engine's (`engine/src/offspring.rs`, one function that
builds them) and not in `explain.rs`, which K5d is changing.

## 4. The record, the identity and the edge

```text
model/journal.rs   RuntimeTxn::Derived(Id<Offspring>)         what a parcel or a part says made it
model/journal.rs   Cause (moved from engine/lib.rs:445)        Flow(Id<Flow>) | Transaction(Id<Txn>) | Applied(u32) | Time | Derived(Id<Offspring>)
model/journal.rs   Offspring { flow: Flow, parent: Cause, law: Id<Law> }          208 bytes: a flow of the run, with its edge
engine/state.rs    World.offspring: u32            how many the fold has made: the next number (cloned with every fork)
engine/state.rs    Record.offspring: Vec<Offspring>      what this ledger has derived, beginning at number `Record.first`
engine/lib.rs      Run.offspring: Box<[Offspring]>       beginning at 0 (a Run is a fold from the first day)
engine/state.rs    Record.returnable: Map<Id<Flow>, Box<[Offspring]>>      kept by forked(): only for a flow the plan says is returned
engine/state.rs    Record.cycles: Set<Lineage>                              kept by forked(): a cycle is said once
```

`Cause` moves to `model` so that the record can be one noun in one crate (`RuntimeTxn` needs the number's type and the edge needs
`Cause`); `engine` re-exports it (`pub use axiom_model::Cause`), so no path in `engine`, `report` or `session` changes. `Cause::Derived(n)`
is the typed edge: `Offspring.parent` is the flow it came from, `Offspring.law` the law that made it (`flow.origin` says it too, as a
reader that has only the flow needs). Following `parent` until it is not `Derived` finds the root; following the laws gives the lineage, so
**the chain is a walk of the record and is never stored** (a `Lineage` is a by-value guard on the way down, and the record is the path
back up). A number is the position in the world's count: `n - Record.first` is the index in this ledger's list, and a number below `first`
(a parcel from before a checkpoint) is "derived before this record began". K7b's walk (`why`) reads `Cause` in two places: `table.rs:257`
(`cause_cell`, the source column of an effect) and `why/line.rs:266` (which flow of a line caused a consequence). Both learn the edge:
the first says "derived by `also` of kind card, from line 12", the second follows `parent` to the line.

The identity the world carries is a number, not a pool index, so **`Checkpoint`/`resume` need one line each** (`World` is cloned whole).
The digest hashes the holdings, which hash `Parcel.txn`, so two folds that derived different flows have different digests.

**What a derived flow is** is built by one function of the model, `Derived::flow_from(header, law, amount)`, which is K6's
`derived_flow` (`occurrence/derive.rs:148`) moved to the data it reads. K6's host calls it, and so does the post host: there is one
way to make a flow of a template, and the oracles hold both. The post host sets what a posted flow has that a template header has not:
`day` (the day it posts), `mode: Actual`, `recognized` (the piece's days) and `txn`, which is not the cause's.

## 5. Returned, pending, written-ahead, hypothetical, forecast

| the cause is | what happens |
|---|---|
| a journal flow, real on its day | derives when it posts, on its day |
| **pending** (`!`) | does not post, so derives nothing. When it is settled (`Fact::Settle`, which lands it, `ledger.rs:476`) it posts on the settlement day and derives then: the derived flow is dated the day it posts, as a hand-written flow after the settlement would be |
| **void** | never posts |
| **returned** (`Settle` after a real flow) | the flow is reversed (as today). Its derived flows are reversed too, **in the order they were posted**, each as the same value moving back; none of them derives. Remembered in `Record::returnable` since the flow's own post; removed at the return |
| written **ahead** of today | posts when the fold reaches its day (through the horizon): derived then. A forecast ledger resumed at today reaches them as any fact |
| a promised occurrence's flow (a forecast) | `post_made` posts it through `post`: derives. The forecast and a kept occurrence are the same fold, so the same derived flows |
| a hypothetical flow (`apply`, `available`'s forks) | derives (`Cause::Applied`): what a withdrawal costs is judged by the same laws |
| a pad or a claim the monitor makes (`Cause::Time`) | derives nothing |
| an `opening` | no law sees it, so none derives |
| a flow that settled claims (a payment from a party) | derives from the pieces it counts (section 1) |

A derived flow settles nothing (`settle.rs:105, 176` read only `Cause::Flow`): a payment a law implied does not pay a party's claims
by itself. That is the one place a hand-written line (which would settle) and a derived flow differ, and the oracle does not generate
claims.

**State of an offspring, for the reports:** `Posted.state` is the root's: `Returned(on)` if the journal flow it descends from is
returned, `Actual` otherwise. It is decided when the flow is derived (the plan knows), so no reader follows the chain to learn it.

## 6. What the readers need

The reports read a journal flow through `Posting { id, flow, posted, settlement }` (`report/history.rs:14`) and `postings(book, run)`.
Eight places call `postings`; `id` is read in two (`why/code.rs:25`, `why/text.rs:19`, both to collect journal flows). The lane does not
change what those eight see: **`postings` stays the journal's**. A second stream, `offspring(book, run)`, yields the derived flows as
`Posting`s whose `source` is `Source::Offspring(id)` (the field `id: Id<Flow>` becomes `source: Source`, and the two readers keep
the journal's), and the views that list flows add it:

| view | what changes |
|---|---|
| `register` of a place, of an entity, of an asset | a derived flow touching it is a row, with its origin in the Note column ("derived by `also` of kind card, from line 12") |
| `flow` (by purpose and by party) | a derived flow counts toward its purpose (`#rebate` is income) |
| `balance`, `check`, `tax`, `budget`, `limits` | nothing: they read the fold's holdings, tallies and readings, which already include it |
| `why LINE` | what the line caused includes what its flows derived and what those caused (the edge is followed to the root); a "Derived flows" section lists them |
| `why` an effect's source cell | says what derived the flow that caused it |
| `claims`, `lots`, `available`, `forecast` | nothing: holdings. The forecast's "Contract occurrences" rows list an occurrence's own flows |

**What is not read and is not in this lane's files:** `explain.rs` (K5d's) walks `book.flows` for the flows that counted toward a limit that
broke, so a derived flow counts toward the limit and is not named among its contributors. Said in the report.

## 7. What the examples and the probe books do

Measured on the baseline binary (`990ddb5`) by `check` of every book in the repository that writes an `also` at the start of a line
(16 files): **no example and no golden or mistake writes a kind's, entity's, purpose's, account's or asset's `also`.** The
ones that write an `also` are contracts' (`05-family`, `11-sam`, `v4-sketch`, `explore-v5/*`) or a contract kind's (`explore-v5/07-relators`,
K6 layer 2, which does not go through the code this lane changes). The four that do are K6's own probe cases,
`docs/v5/measure/diff/cases/g-also-{entity,kind,account,purpose}.ax`, which print `also-inert` (three of them; `g-also-purpose` is an
`ambiguous-purpose` error: `fun` is declared by std). They are inputs of a differential harness, not goldens, and are the only outputs the
lane moves among files the repository keeps. `tests/golden/` and `tests/mistakes/` are expected **byte-identical**; the brief's "except what
a kind-level law now derives" is the empty set, and the report says so with the regeneration.

## 8. Oracle, mutants, timing

**`docs/v5/measure/derived.py`**, in the style of K6's `derives.py`: a seeded generator of projects, each **written three ways**, which
must print the same to every command:

```text
   also      the owner's `also LINE [when E]`, for each owner kind (account, account kind, entity, entity kind, purpose, asset)
   law       the same owner with `law NAME / on flow / [when E] / derive LINE` (the hand-written law the line abbreviates)
   written   a journal with the flows each law derives written after the line that caused them (the generator computes them: it is a
             reference of its own: which laws a flow reaches, in which order, to what amount rounded as the fold rounds)
```

`check` (the footer without the count of flows and laws, which only the journals differ in), `balance`, `flow`, `tax`, `register` of each
account touched (the Note column of a derived row masked), `claims`, `forecast`; and the engine's own dump (`forecasts/main.rs`, extended
to print the derived flows with the rest of what the fold made), so a flow the reports do not list is compared too. The corpus covers
every owner, every shape (`flow` of its own, `+` and `-` with a purpose), a `when` true and false, two laws on one flow, a law that derives
into a flow another watches (chains, in the reference), a cycle (the reference stops where the engine does and the diagnostic is compared
by its code and its labels' lines), flows pending then settled, returned, written ahead, and contracts whose occurrences a kind's law
reaches, kept (lines) and promised (forecast).

**Mutants** (`derived.py mutate`, K6's `mutation.py`): each change of one line of the code this lane writes must be caught by the oracle or
by a named unit test; a survivor is listed with its reason in the report. K6's `derives.py` and `relators.py` and K7b's histories oracle
(`crates/session/tests/histories.rs`) must still hold; `fuzz.py ... diff`, K4b's `splits.py` and K3c's `claims.py` unchanged.

**Timing.** `axiom check` on `bench/` at 100k and 1m flows, fastest of three, the load average beside it, baseline binary
(`990ddb5`, built first, kept in the scratchpad) against the lane's: 100k 0.446 s and 1m 4.430 s at load 0.4 before. The cost a book with
no kind-level `also` pays is the `Touching` rows' two empty-slice lookups per flow and one empty check of the queue; callgrind gives it in
instructions (K7b measured the recorder at +1.1%, K3d's recognition +1.3%, and the bar for this lane is "nothing").

## 9. Which of K6's "not built" this lane builds

K6 left: (a) the post host, (b) `sales-tax` on a party kind, (c) `share` on a journal flow, (d) a `share` for a party as a claim,
(e) `+ N% of amount` as a template item (K5's), (f) Layer 3.

| | |
|---|---|
| **built** | (a): a flow of its own and a `+`/`-` item derived from a posted flow, for a purpose, an asset, an account, an account kind, an entity, an entity kind; the record, the identity, the guard, the return. "Sales tax collected" as the book writes it today (`also + 10.35% #sales-tax-collected for wa-dor` under a purpose) is an item of this kind and works |
| not built | (b) and (c) are allocations: they re-attribute a part of the flow to another owner or to the tax purpose and so reduce what the flow already put in its owner's tally, which a flow derived after it cannot. They stay in the occurrence host (contracts), and a `share` under anything else keeps what K6 said. (d) needs a claim and so needs a Debt tab to hold parcels, which is K3f's; a `share` for a party stays the flow it bears with K6's warning |
| not this lane's | (e) and (f) |

## 10. What Layer 3's `on start` / `on end` would need of this host

The host already takes a derived flow with no flow to cause it if the queue is given a `Waiting` whose parent is not a posted flow: the
parent is a `Cause`, and `Lineage::ROOT` is where a chain starts. What `on start`/`on end` add: **a trigger fired by the timeline** (a row
`Watch::Contract(contract)` read at the first and last due day of a span, from the same heap `Promising` keeps), **an `Occasion` with no
`Motion`** (so `Stand::Flow` has no flow to be positional against: the line must name both ends, which is one more check in `derive()`,
and `Stand::Subject` resolves to the contract's owner), and **a `Cause` that names the span** (`Cause::Time` says "a period ending" and
names no law or day: K7b's missing edge 1, which two words more fix). The drain, the lineage, the record and the reversal need nothing.

## 11. The plan

| step | what | proof |
|---|---|---|
| 0 | this map; the baseline binary, bench corpora and probes in the scratchpad | |
| 1 | model: `Cause` moved, `Offspring`, `RuntimeTxn::Derived`, `Stand`, `Derived::flow_from`; `also` and `on flow` lowered for every owner; `Table::Touching`; `derive-carve`; the K6 warning and its test | model tests, one for each owner kind; goldens and mistakes byte-identical |
| 2 | engine: the queue, `post` split, `Lineage`, the effect handler, the record, `Run.offspring`, the return, the checkpoint number, the diagnostics | engine tests, the histories oracle |
| 3 | report: `Posting::source`, `offspring`, `register`, `flow`, `why`, `cause_cell` | report tests, goldens, the 12-date harness |
| 4 | `derived.py`, the dump, the mutants | the oracle; every mutant killed or listed |
| 5 | LANGUAGE §10; timings; this file's "what was built, measured and not finished" | |

## 12. How this map was checked

- **0.1:** `check` of a hand-written `law` / `on flow` / `derive` under a purpose (`derive-owner`) and under an account (`law-trigger`, "`on flow`
  laws govern purposes, assets, contracts, and asset kinds") on the baseline binary; `laws/mod.rs:256-269`, `fire.rs:595-597`.
- **0.2 and 4:** `grep` of `RuntimeTxn::` (44 non-test uses, in `model/journal.rs`, `model/book.rs`, `engine/{post,lots,motion,ledger,temporal,statement,
  occurrence,explain,claims,assets,assets_runtime,eval}.rs` and `report/{claims,available}.rs`) and of `\.txn\b` and `Cause::` in `engine/src` and `report/src`, each
  read. Three matches on `Cause` list every variant (`post.rs:789` and `800`, `table.rs:257`), so the compiler finds them; the others ask for
  `Cause::Flow` and say `_` for the rest (`explain.rs:169, 192`, `settle.rs`, `why/line.rs:266`, `why/taxline.rs`), and `explain.rs`'s label of a
  violation caused by a derived flow points at the `also` line that derived it (`Flow.loc` is the template's).
- **0.3 and 0.4:** `state.rs:158` (`forked`), `checkpoint.rs`, `events.rs`, `timeline.rs:451-470` (`skip_unreal`: a pending flow is not a fact until it
  settles), `ledger.rs:476` and `496-524`.
- **0.5:** `law.rs:668-705` (`Table`), `rules.rs:107, 166, 259, 275, 302`, `laws/mod.rs:333`; `plan.rs:252` (`inside`), `scope.rs:28`
  (`containing`: an entity subject is the *asset* places its owners hold, so a party's outside place is inside no entity).
- **0.6:** `laws/compile/line.rs:252`, `occurrence/derive.rs:148`, `LANGUAGE.md` §10.
- **0.7, 0.8:** `book.rs:784`, `occurrence/derive.rs:116`; `motion.rs:152`, `claims.rs:44`, `reconcile.rs:133`.
- **7:** a script over every `.ax` in the repository that has a line beginning `also`, `check` through the baseline: 3 `also-inert`, in K6's
  `g-also-*` cases (`scratchpad/k6b/also-inert.txt`).
- **The baseline:** `cargo build --release` at `990ddb5` (the binary is `scratchpad/k6b/baseline-axiom`); `loc.py` 54,314 and `hist.py`
  (2,085 / 732 / 515 / 127 / 6 / 0 / 1 functions in the bins 1-10 / 11-20 / 21-40 / 41-80 / 81-160 / 161-320 / 321+) before any change.

## 13. What was built

Ten commits on `990ddb5` (this file's commit is the last), each building and passing (`axiom-engine`'s `a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`
and `axiom-report`'s `a_context_forecast_keeps_historical_and_same_day_obligations_once` fail before and after; the oracles and the sibling lanes
say nothing of them):

| commit | what |
|---|---|
| `25f8e94` | this map |
| `714f430` | model: `Cause` and `Offspring` (with `RuntimeTxn::Derived`), `Stand`, `Derived::flow_from`, `Table::Touching`, an `also` or `derive` under any owner is a law |
| `c342eaa` | engine: the host (`offspring.rs`: `post` = `post_flow` + `post_brood`), `Lineage`, the record, the return, `Run.offspring` |
| `c84ba1b` | engine: `Offspring.root`, the payee, an asset's `amount` is money |
| `aa72ff8` | report: a derived flow is a posting after the flow it came from (`register`, `flow`, `balance`, `tax`, `why`, the histories) |
| `deecf5b`, `c28ffb7`, `77c1fea` | the cost of a book with no such law, the depth stop's own note, `fits` and `why LAW` back under the length limit |
| `8f5561c` | `derived.py`, its 40 mutants, the probes, `tests/mistakes/105-109`, LANGUAGE section 10 |
| `ac39991` | a test that a claim the monitor makes derives nothing (the one mutant the oracle's books cannot reach) |

**Where the build differs from the sections above** (each was decided by the code, and none changes what the sections promise):

- **The number is the record's, not the world's** (section 0.3, 4): `Record.first_offspring` says where a record begins and `Record::forked` sets it
  to its parent's end, so a number is never reused across a checkpoint, and `World` is untouched. `Run.offspring` begins at 0.
- **`derive-posted`, not `derive-carve`** (0.7): the error covers every item that is part of the flow it comes with, not only a carve (a `+` or `-`
  with no purpose too), and says the fix (`give it a purpose`). A contract keeps carving.
- **A law does not watch what it made** (`Stopped::Own`, said nothing): the card of section 3 and of LANGUAGE credits itself through `issuer -> self`,
  which touches the card, so without it the example was a cycle of one. A cycle is every other chain that comes back (`Stopped::Unbounded`), keyed
  by `(lineage, law)` in `Record.stopped`, not a `Set<Lineage>`.
- **`Offspring.root`** (the first cause back that is no offspring) is stored: `why`, the register and the cycle's "flow that started it" would
  otherwise walk the record for every row.
- **A law's firing order is the order they are written in** (`rank`), not the order of the rows they are found in: `watching_place` fills a row
  in owner order and `Rules::of` then sorts it by rank, so the kind-before-entity order of section 2 is declaration order.
- **A flow of a returned flow's chain is remembered only if the plan says it is returned** (`remember_for_return`), as designed.
- **The flow does not wait for a `begin`**: the chain a flow begins (`Brood::begin`) is set when its first law derives, and `post_brood` returns at
  once for a flow nothing derived from.
- **`law-never-fires`** is a warning for a kind of which nothing exists (and not for a system's laws, which are written for every book that lives
  under it). **No entity stands at no place**: every declared entity has an outside place, so an entity's law is always reached by some row, and
  the orchestrator's example for this warning does not arise in this model. A law that no flow of *this* journal reaches is not said: a forecast's
  contracts may bring one.
- **An asset's `amount`** is not typed in the asset's own unit (a repair of a house is money): found by the oracle's first asset book.
- **A derived flow's `recognized` is its cause's** (a pending flow settled on May 4 derives a flow of April's tallies, as the flow it comes from
  is), which the written book says with `for 2026-04`.

## 14. What was measured

**The oracle** (`docs/v5/measure/derived.py`, 700 lines of Python that work out which law fires for which flow, in what order, a second time from
LANGUAGE and not from the engine): a project is four books (`also`, `law ... on flow ... derive`, no law with the derived lines written after each
line, and `also` with every occurrence kept), a flow of the journal is a line, pending (settled, void or still pending), returned, written ahead of
today, or an occurrence of a contract; every owner kind is an owner (`kind` of an account, of an entity, of an asset; an account; an entity; a
purpose, among them one that inherits its law; an asset), laws derive an item along or back or a flow of their own with `self` or named ends, with
`when` guards on the amount, in chains, in cycles (`derive-cycle`, held to the reference's count) and in ladders of nine and ten laws
(`derive-depth`). It checks the CLI (`check`, `balance`, `flow`, `flow --by party`, `claims`, `tax 2026`, seven registers, `forecast`, `available`),
and the engine's own dump (`forecasts/main.rs`: every flow posted, the derived after today, holdings at each month end, the diagnostics). The fund
`available` draws is slow money, so its hypothetical flow derives and what the laws owe for it is held to the reference's. On the corpora I
kept: 600 projects (seed 7: 22 rejected for a flow of a place to itself or a chain too long to read) through the CLI of the final build, 0 differ, and
through the dump, 0 differ (25,961 flows posted, 4,462 derived after today, 12,656 derived flows written out for the `written` books); the
owners are all used (every kind, account, entity, purpose, asset in the world 22 to 161 times; 107 projects are cycles, 77 ladders, of which
208 projects say `derive-cycle` and 51 `derive-depth`, each held to the reference's count); 442 pending flows stay pending, 897 settle, 446
are void; 1,183 are returned (a third of those after today, so the return is after the forecast's checkpoint); and the forecast's promised
occurrences derive 1,588 flows that `ahead`'s fold derives again, equal on every month end.

**Mutants**: 40 changes of one piece of the code under test (`derived.py mutate`), the oracle's two layers first and the crates' own tests after.
Of the 40: **26 are killed by the engine's dump, 6 by the CLI's views** (the ones that change an order or what a view lists), **5 by a named unit
test** (`self_in_the_law_of_an_asset_is_the_asset_itself`, `the_numbers_of_derived_flows_go_on_after_a_checkpoint_and_the_effects_they_cause_say_which`,
`a_derive_that_cannot_be_made_is_said_where_it_is_written` twice, `a_flow_handed_to_a_ledger_derives_as_a_flow_of_the_journal_does`) and **3 survived**:
(19) *a flow caused by time derives*, which no book of the oracle makes (it writes no claim), and which `the_claim_the_monitor_makes_for_a_missed_occurrence_derives_nothing`
kills, checked by hand against the mutant (two derived flows where one is the kept occurrence's); (32) *a derived flow's `sequence` is its root's*, which
is equivalent while the register's sort is stable, since the postings arrive in order; (39) *`Rules::watches` is always true*, which only
costs, since the lookups it skips find empty rows. One more mutant of the first pass, *an opening derives*, survived because the clause was
dead (no law watches an opening) and was removed from `Motion::derives`; the first pass and the second (the survivors and five more, after the fast
path) are in `scratchpad`, not kept

**Unchanged**: K6's `derives.py` (150 triples: run, dump, compare), `relators.py` (80 cases), K4b's `splits.py` (120 projects, 3,374 commands, and
100 `promises`: 0 differ), `fuzz.py` (2,000 mutants of the examples, `diff`: one output differs, `regress_1607`, a hand-written `on flow` law under
an entity that used to be `law-trigger` and now is a law whose `amount` has no one unit, so `count amount as business-expenses` says
`type-mismatch`), K7b's twelve-date harness over the examples (3,292 files, `diff -r` empty), `diff/compare.sh` (the only outputs that move are
K6's three `g-also-{account,entity,kind}` probes: the `also-inert` warning is gone and the `also` they write is read, so its `#fees`, which no
book declares, is an `unknown-purpose` error), `tests/golden.sh` and `tests/mistakes/run.sh` (byte-identical; 105-109 are new).

**Cost of a book with no such law** (`axiom check`, `bench/100k`, 1,868,129,556 instructions on `990ddb5`, callgrind):

| build | instructions | over the baseline |
|---|---|---|
| first host (two `Touching` lookups, a `begin` and a `post` wrapper per flow) | 1,881,140,905 | +13.0 M, +0.70% |
| `Rules::watches` fast path, `begin` when a law first derives (`c28ffb7`) | 1,873,570,980 | +5.4 M, +0.29% |

That is about 50 instructions for each of the ~100,000 flows (`post_flow` +2.5 M, the `post` wrapper +2.3 M, `enforce` +2.1 M without my touching
its path), not one empty-slice check. Wall time on the same books, three runs and the fastest, load average 2.9 to 3.4 (`time3.sh`): 100k 0.444 s before, 0.412 s after; 1m
4.220 s before, 4.195 s after. Interleaved (`ab.py`, nine and five runs, fastest, then user time): 100k 0.410 against 0.411 s wall and 0.378
against 0.393 s user; 1m 4.335 against 4.346 s wall and 4.069 against 4.158 s user. The difference is inside the noise of a shared machine
except that the user time is 2% to 4% higher in both, which is more than the instruction count says, and which I read as the layout of
`post_flow` and not as work

**Size** (`loc.py`, non-blank non-comment lines, tests and `#[cfg(test)]` excluded; `hist.py` on `crates`): engine 12,242 to 12,492 (+250:
`offspring.rs` is 306 of the +798 lines written, and `offspring_tests.rs` another 380, which `loc.py` leaves out), model 19,081 to 19,269 (+188),
report 6,748 to 6,878 (+130), the rest unchanged: **54,314 to 54,882, +568** (+1.0%). By function length (bins 1-10, 11-20, 21-40, 41-80, 81-160, 161-320,
321+): 2,085 / 732 / 515 / 127 / 6 / 0 / 1 became **2,115 / 740 / 522 / 126 / 6 / 0 / 1**: 43 more functions, 7 of them in 21 to 40 and one fewer in 41 to 80.
Functions over 40 lines: 132 before, 131 after; the one this lane left longer is `Ledger::finish` (50 to 51 lines: `Run.offspring`, one field of
a struct literal; I did not split it, to leave K5d's neighbourhood alone). `fits` went from 60 lines to three functions.

## 15. What is not finished

- **A derived flow settles nothing**, so the one place a hand-written line and a derived flow differ is claims: a payment a law implied does not
  pay a party's claim. The oracle generates no claims.
- **`explain.rs` is K5d's**: the contributors the explanation of a limit that broke names are journal flows (`book.flows`), so a derived flow counts
  toward a limit and is not named among them.
- **The hypothetical flow `available` derives is held to the reference for one place** (the fund into `checking`); other hypothetical flows
  (`Reach` of a 401(k), of a security) go through the same `post` and are checked by `a_flow_handed_to_a_ledger_derives_as_a_flow_of_the_journal_does`.
- **No entity stands at no place** (section 13), so `law-never-fires` does not say a law of an entity that no flow touches.
- **The oracle masks what only a written line can say** (the note a register gives a derived row, where it was written, the count of flows, the
  code of the flow a derived row came from, a sentence about flows recognized over a range); what it masks is listed in `derivation` and `written`.
- **A share for a party, a party kind's `sales-tax`, and a written line replacing the derived one** (LANGUAGE section 10) are not built; the last
  was never this lane's.

## 16. The three places I am least proud of

1. **`derived.py`'s masks.** The strongest claim, that a law and the lines it stands for are the same book, passes through a layer of regular
   expressions that strip the note, the code, the source line and a sentence, and a derived row shows its root's code (a code that names a line
   that has not derived it, and is not unique). The masks are few and named, and nothing they strip is a number, but they are where a difference
   between a derived flow and a written line goes to hide.
2. **`Brood`'s invariants.** The chain a flow begins is set when its first law derives (`derive_later` calls `begin` for a flow no law derived),
   and a derived flow enters its chain from the `Waiting` it was made as (`enter`); a flow that derives nothing never touches either. It is
   cheaper by 8 M instructions and it is subtler than a reset at the start of every post: a second place that posts a root flow and forgets to
   reset the lineage would derive with the last chain's. Only `post` posts a root flow, and the oracle's mutant that drops `begin` is killed.
3. **A derived flow is a posting only through `all_postings`.** Its state is its root's (`Posting::derived` reads `run.posted[root]`), its place
   among the day's postings is `sequence`, and `postings` stays the journal's, so every view that lists flows must know to ask for the one and not
   the other. `register`, `flow`, `balance` and `why` of an asset ask for the one; `why` of a code and of a description (`why/code.rs`, `why/text.rs`) ask for the other and list lines, so `why ^c4` does not list what the card derived from `^c4`, though a register row says it has that code; a next view would be easy to get wrong, and the
   mutant that makes `sequence` give every derived flow its root's number survives, because the sort is stable and the iterator already puts it
   right (an equivalent mutant today, and a fragile one).

## 17. For the merge

K5d is in `occurrence.rs`, `monitor.rs`, `promising.rs`, `reconcile.rs`, `explain.rs`, `promise/*`, `lower/contracts.rs` and `std.ax`; this
lane edits none of them. **Where the two meet** and a conflict is likely: `engine/src/lib.rs` (`Run.offspring`, `Recorded` and the `mod`
lines), `engine/src/state.rs` (`Record` fields and `forked`), `engine/src/ledger.rs` (`Course` in `post_journal` and `post_computed`, `Run`
assembly in `finish`: one line), `engine/src/fire.rs` (`Outcome::Derive` calls `derive_later`; `fault` is `pub(crate)`), `engine/src/motion.rs`
(`Course`, `derives`), `engine/src/occurrence/derive.rs` (calls `Derived::flow_from` in place of K6's `derived_flow`, which moved to the model: 4 lines
added, 32 removed), and `model/src/book.rs`/`journal.rs`/`law.rs`/`rules.rs` where the other lane adds a `Watch` row or a `Shape`. I merge nothing
of K5d's logic: the dependency is that `Cause` moved to the model (a `pub use` keeps every path), and that `Shape::Flow` ends are `Stand`, not
`Option<Id<Place>>` (any test of K5d's that builds a `Shape::Flow` by hand writes `Stand::Flow` or `Stand::At(place)`).
