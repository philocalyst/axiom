# K4b map: what resolves a split, where, and whether it is one algorithm

Written before the first code change of lane K4b, from the code at `39b6c8b` (the merge of K4a), and checked against what
the code does (the last section says how). Paths are in `crates/`; line numbers are those of `39b6c8b`. The vocabulary
is K4a's: [`K4a-map.md`](K4a-map.md) and `model/src/split.rs`.

## 0. The verdict: "one function" is half true

The brief names three copies of one algorithm:

| the brief says | what the code has |
|---|---|
| `model/lower/flow.rs` (`lower_items`) and `record.rs` (`lower_split_flow`, `lower_named_flow`) **resolve** a statement's header, legs and items at model time | they **transcribe**. A literal amount is copied into the flow that carries it. Nothing is subtracted, nothing is resolved, no `...` is settled, no item bears on anything. There is no algorithm here to be a copy of |
| `engine/ledger.rs::post_journal` resolves the same statement at fold time, flow by flow | it resolves **no group**. It evaluates a computed amount of *one flow* (and a computed basis), recovers a `=` or `all`, and sums an exchange's cost items. No leg is carved from a header, no remainder is settled, no item takes from anything (section 2) |
| `engine/ledger.rs::materialize_group` resolves a contract's template, with the escalation of the day, and an occurrence's own overrides | this is the one algorithm: header, legs, shares, `...`, items, escalation, an omitted input. It is the only code in the book that subtracts a leg from a header |

So there is **one** copy of the algebra of a group (`materialize_group`, 324 lines, with `quantity`, `expression`, `leg`,
`compute` and `subtract_parent` under it), **two** evaluators of "what does one computed amount come to" that differ in
what they read the amount against (`compute` and `journal_expression`, with three hand-written matches on the value and seven
calls of `journal_expression_fault` in `post_journal` beside them), and **three** transcriptions of "what does one written quantity say, as a flow carries it"
(`resolve_quantity` in the model, `Ledger::quantity` in the engine, `template_quantity` in the model's contracts).

And the statement side differs from the promise side in more than K4a's map's section 3 says. Four things the statement
path does **not** do, each of which the promise path does (section 4 shows each on a book):

1. A statement's legs are not subtracted from anything. A leg of `checking 3_150 USD -> / shop 1_050 USD / ...` is a flow
   of its own with `out = 0` and `arrive = 1_050`; the source end is **never debited**. Net worth rises by the sum of the
   legs. (This is wider than K4a's "the leg amount goes on the arrive side": it is every split statement, not only a
   non-conserving one.)
2. `...` posts zero, and the header's total is read by nothing (the unit of the legs, when they name none, aside).
   `checking -> 999999 USD` and `checking -> 300 USD` over the same legs give byte-identical output.
3. A statement's items do not carve its header (LANGUAGE §3 says they do). A `Carve` item posts beside the whole header; a
   `Less` item with no purpose makes no flow and so does nothing at all.
4. A header amount written **after** the arrow of a split (`checking -> 300 USD`) is dropped by `lower_split_flow`, which
   reads the source's own side only.

None of this is for K4b to fix (the rules of the lane: behaviour is preserved), and all of it is behaviour the proof must
see unchanged. What it means for "one function" is: the statement and the promise do not differ in a step of one algorithm;
they differ in having the algorithm at all.

### The smallest honest unification

1. **`solve` is the algebra of one group, written once** (model, `solve.rs`): a header's remaining amounts, legs that take a
   quantity, a share or the rest, items that carve, add or take off. It is `materialize_group`'s algebra, moved out of the
   engine and made pure over an `Env` (what an expression comes to, in the phase that asks). The fold calls it for every
   promised and kept occurrence. The model calls it with `LiteralEnv` **to check**, never to build a flow: that is the
   static conservation check, "`solve`'s linear algebra used to check instead of solve" (DESIGN §3.5).
2. **`Quantity::resolve` is "what one written quantity comes to", written once**, over the same `Env`. The model's
   `resolve_quantity` (its second half), the engine's `quantity`, `expression` and `leg`, and the model's three
   `stand_in` matches in `contracts.rs` are one function. With `LiteralEnv` a computed amount is *later*: it keeps
   the zero a flow carries meanwhile, and the fold evaluates it. That is the constant folding there is to do.
3. **One evaluator in the engine**: `compute` and `journal_expression` build the same motion, occasion and context and
   differ only in what they are given and in what they do with a fault (section 3.2). They become one `evaluate` over a `Reading`, behind `FoldEnv`; `post_journal`'s
   three matches (seven reports of a fault) become one.
4. **`post_journal` keeps its flow-level logic** (patch the computed amount into the flow the model already shaped, refresh
   a computed `=`, add up an exchange's cost). It resolves no split, so it does not call `solve`; it calls the evaluator.
   The brief's table is wrong in this cell and the report says so.

What this does **not** do, and why the brief's lines target (about -1,500) is out of reach: nothing is a copy of
`materialize_group`, so there is nothing to delete but `materialize_group`'s own plumbing (about 150 lines) and the
duplicated evaluators and matches (about 150). `solve` and `Quantity::resolve` are new code in the model (about 250 lines,
with tests). The net is a few hundred lines, not 1,500. The gain is that the algebra is testable alone, that the fold's
three ways to evaluate an amount are one, and that a split that cannot conserve is an error in the book rather than money
that appears.

### What the brief says that the code does not, one by one

| the brief | the code |
|---|---|
| `Env::held(place, unit)` for `all` and `=` | `all` and `=` are resolved when the flow **lands** (`amounts`, ledger.rs:1498), after the legs before it have landed, and a reversal must undo exactly what was done (`record.resolved`). A group solved in one go with `held` would read the balances from before the first leg: a different amount for any group with two legs on one place. So `solve` leaves `all` and `=` as the markers they are (`Infer::All`, `Infer::Target`) with the placeholder they carry, and `Env` has no `held` |
| `eval(root, scale)` | an expression is read **against a flow** (`self`, `amount`, `from`, `to`, the payee, the purpose, the codes), and for an item that flow is the header as the legs and the earlier items have left it. So `Env::amount` takes the site and the header's remaining amounts, and returns what the node comes to, scaled as the site says |
| `solve(group, env) -> Result<Solved, Fault>` | the error is the engine's `TemplateError::Expression { fault, loc }`, whose `loc` is the node's, the flow's or the item's by where it arose (section 3.3). So an `Env` has its own failure type and `solve` passes it through beside its own (a unit mismatch, an overflow, two remainders on one side) |
| the group is a `Group<H, F, I>` | an occurrence's group is a **template** with a **written override** over it: the legs are the template's, each replaced by the written leg that goes to the same end, then the written legs that replace none. `solve` takes the effective legs and items (what the group *is* for this occurrence), which the wrapper makes. A template's item says whether it makes a flow by its purpose; a written item by the purpose of the flow it names: `Item<I>` cannot say it for both, so the wrapper does |
| the model "stores the solved amounts" | it has nothing to store: every literal amount is already in the flow that carries it, and a statement's leg is not a function of the others (section 0). What the model can add is a check |
| for nearly every transaction "the fold then does no solving at all, only posting" | it already does none: a transaction with no program takes the straight path in `post_journal` (ledger.rs:1262), and `post_journal` is 1.35% of `check` at 100k (24M of 1.81G instructions, `bench/perf/callgrind-self-100k.txt`) |

## 1. What each reads and makes, in K4a's vocabulary

### 1.1 `materialize_group` (ledger.rs:495-818), for a promise and a kept occurrence

Reads:

- the template, a `Promised`: `header: Header<Flow> { flow, out: Quantity, arrive: Quantity }`, `side`, `legs: [Leg<Flow> { flow, part: Part }]`,
  `items: [Item<Says> { sign, amount: Expr, loc, flow: Says }]`;
- the written override, a `Made` (`Option`): `legs: [Leg<u32> { flow: offset, part }]`, its own `side`, `items: [Item<Option<u32>>]`, with
  the occurrence transaction's flows (`source_flows`) for the offsets;
- the occurrence: `ratio` (the escalation, and the proration, of the day), `amount_override`, the `OccurrenceTail`, the
  inputs, the due day and the day it was written, the occurrence's own `Program` (the template's is `terms.program`).

Makes: pushes `RuntimeFlow`s: the header (as the legs and items have left it), the legs, the items that make a flow, each
stamped. Or omits the whole group (a header side or the occurrence's amount reads an unbound input), or fails the whole
occurrence with a `TemplateError` (`instantiate_occurrence` truncates what it pushed).

### 1.2 `post_journal` (ledger.rs:1245-1493), for a statement's flow

Reads one flow `source = book.flows[id]` of a transaction, and, if it has them, `program.roots_of(offset)`
(`FlowExpressions { out, arrive, basis }`) and `program.group`. Of the group it reads only: whether this flow is the header
(`Heading::Flow(offset)`), whether it is an item (`item.flow == Some(offset)`), and the items' `amount` and `sign` for an
exchange's cost. It never reads a leg, a `side`, a `total`, or a `Part`.

Makes: one `Motion` posted, with the amounts `amounts()` settled; or a diagnostic and nothing posted.

### 1.3 the lowering (`flow.rs`, `record.rs`), for a statement

Reads the AST (`ast::Flow`: `from`/`to` with an optional end and amount each, `body.legs`, `body.items`, a tail) and the
world. Makes: `Flow`s in `book.flows` (with the literal, or the zero a computed amount stands in with, as `out` and
`arrive`), `FlowExpressions` roots, and **one** `Made` group stored in `Program.group`. `resolve_quantity`
(flow.rs:161-211) is the only place that turns a written quantity into `{ amount, infer, mode, part }`.

## 2. The steps, in order, in each

A dash is "does not". Section 3 says which differences are behaviour.

| step | the lowering (`lower_*`) | `post_journal` | `materialize_group` |
|---|---|---|---|
| **header: amounts of the two sides** | `make_flow`: `resolve_quantity` per side (flow.rs:230); `stated_amounts` mirrors a one-sided transfer, and rejects two literal amounts that differ (`flow-amount-mismatch`); `apply_price` | – (the flow is the header). A computed `out` or `arrive` is evaluated, set on the flow, mirrored to the other side if it is not an exchange; a computed basis is evaluated | `quantity` per side (822), each scaled by `ratio`; `set_quantity` (1579); mirrored to the other side if it is not an exchange and one side is computed (590-606); the occurrence's amount replaces the group 0's (`apply_occurrence_amount`, 1609); `bought_quantity` for a standing `buy` |
| **a missing input** | – (a leg named as an input is *bound* at lowering, `lower_occurrence`) | – (a statement has no inputs) | `Fault::MissingInput(i)` from the evaluator: the input is noted in `missing`, and the part is **omitted**: a header side omits the group (570-577); a leg is not emitted and not carved (`ResolvedLeg::Omitted`); an item is skipped |
| **escalation** | – | – | the template's literals and computed amounts are scaled by `ratio` (`scale_template_amount`, 1575); a written leg or item is read with `Ratio::ONE`; a share is of the **scaled** header |
| **header's `Percent`** | – (a bare share in a statement is a type error, K4a 1.3) | – | `Part::Share(rate)`: the header's side **as resolved, before any leg carved it** (`leg`, 886-889), scaled by `rate` |
| **each leg's quantity** | `lower_leg` (record.rs:392): `resolve_quantity` against the unit of the total or the base; the amount is placed on `side.other()`, zero on `side` | – (a leg is a flow like any other: its computed amount, `=`, `all` as above) | `leg` (874): `Part::Of(q)` through `quantity`, `Share`, or `Rest` (taken out for later). Template legs first, each replaced by the written leg to the same end; then written legs that replace none |
| **two `...` on one side** | – (the parser rejects a second in one body: `two-remainders`) | – | `InvalidTemplate { loc: the second's }`, at the point the second is read (665, 698). Reachable only by an occurrence whose written `...` replaces a template leg that is not the template's own `...` |
| **subtract the legs** | – | – | `subtract_parent` (1591) for every explicit leg, in order, on the leg's `side`: the parent's side, and the opposite side too when it has the same unit (an exchange keeps its own). `UnitMismatch` or `Overflow`, `loc` the leg's |
| **resolve `...`** | – (a `Rest` leg's amount is the zero `resolve_quantity` gives it) | – (posts `0`) | after every explicit leg **even if written first**: the header's remaining amount on the leg's side, subtracted (716-724); the leg's mode is the occurrence's (`stamp.mode`), its infer `Known` |
| **each item's sign** | `lower_items` (flow.rs:457): `Carve`, `Add`, `Less` copied; an item that says something its parent does not (`says_something`) makes a flow, `Less` with the ends swapped | a flow like any other. A `Less` item with a spending purpose on an exchange header is also **cost evidence**: summed into the header's `detail.cost`. A `Less` item of an asset sale reduces its proceeds (`asset_sale_less_items`, post.rs:949) | `Carve`, and `Less` **with no purpose**, subtract their amount from the header; `Add`, and `Less` with a purpose, do not. An item with a purpose makes a flow (`Less` swaps its ends) whose amount is its own. Template items first (their literals scaled), then written items |
| **a computed item's `amount`** | – | the item's flow, with `amount` the flow's own out or arrive | the header **as it stands** after the legs and the earlier items |
| **`all`** | `Infer::All`, amount zero | `amounts` → `everything` (1515): what the selected parcels hold, **when the flow lands**; cached in `record.resolved` so a return undoes it | `Infer::All`, zero. Resolved the same way when the flow lands **in the forecast** (`apply_runtime`); **not** for a kept occurrence, which posts `Amounts::written` (`post_written_occurrence`, 1206) |
| **`=`** | `Infer::Target { end, balance }` with the literal balance | `resolve_target` (1530), when it lands, cached | as `all`. And the leg's **balance** is carved from the header as if it were an amount (the stand-in is the balance): section 4.4 |
| **exchange cost** | – | the header aggregates its cost items' converted amounts into `detail.cost` (1384-1475); a cost item's amount is evaluated once, by whichever lands first, and cached | – |
| **`?`** | `Infer::Unknown`; solved before the fold by `infer::solve` from balance assertions (`plan.amounts`), only for a flow that is not an occurrence's | as written, or the plan's | `Infer::Unknown`, zero: **never solved** for an occurrence's flow |

## 3. Where they differ, as behaviour and as accident

### 3.1 The group

Every row of section 2 where `materialize_group` does something and the other two do not is the same statement: the
promise's group is a *header with legs that take from it*; the statement's group is a *list of flows that were written
together*. Whether that is behaviour or accident:

| difference | verdict |
|---|---|
| a statement's leg is not subtracted from a header; the source is never debited | **accident**: the language says "their total is the header amount" (LANGUAGE §3, Split flows), and a split that raises net worth is not what anyone wrote |
| `...` posts zero in a statement | **accident**, the same |
| a statement's header total is read by nothing | **accident**: `Heading::Source { total }` is built, stored and read by tests alone |
| a header amount written after the arrow is dropped | **accident** (`lower_split_flow` reads the source's own side only) |
| a statement's items do not carve its header | **accident**: LANGUAGE §3, Line items |
| a promise's `Target` leg carves its target **balance** from the header | **accident** (section 4.4) |
| a kept occurrence posts `all` and `=` legs as written (zero, or the balance), the forecast resolves them | **accident**: the same flow, two postings |
| the template's leg modes survive a kept occurrence: a leg is `Planned`, the header and the remainder `Actual` | **accident** (`flow.mode = value.mode` at 735-738 overwrites the stamp the line before it set) |
| escalation scales the template and not the written override | **behaviour**: an occurrence says what it says |
| a share is of the scaled header | **behaviour** (and rounding makes it observable) |
| an occurrence can make two `...` on one side, and the fold fails on the first due day | **accident**: the model knows the template when it lowers the occurrence and does not look |

Every accident above is **preserved** by this lane. The statement-side ones are not the lane's to fix (the rules), and the
promise-side ones are what the differential proof is for: a unification that quietly repaired one would be the very
behaviour change the proof exists to catch. The report lists them and says what each fix would be.

### 3.2 The two evaluators (`compute`, ledger.rs:900, and `journal_expression`, 2224)

Both build a `Motion` and an `Occasion` for a flow and run `program_expression`. They differ in:

| | `compute` (promise, occurrence) | `journal_expression` (statement) |
|---|---|---|
| the flow's day | `at.due` (the day the occurrence is due) | `flow.day` |
| `amount` in an expression | `Value::Empty` (`occasion.amount` is `None`) | the flow's out, or its arrive when out is zero |
| cause, ordinal | `Applied(ordinal)`, the ordinal passed | `Flow(id)`, the flow's offset in its transaction |
| inputs | the occurrence's | `book.txn_inputs(txn)` |
| program | `terms.program` or the occurrence's | the transaction's |
| result | scaled by `at.scale` | as is |
| `MissingInput` | noted, `None` (the part is omitted) | a diagnostic, the flow is not posted |
| any other fault | `TemplateError::Expression { fault, loc: node.loc }` aborts the occurrence | `explain::journal_expression_fault(book, flow, program, root, fault, day)`, the flow is not posted |
| a value that is no amount | `InvalidProgram` | `InvalidProgram` |

All **behaviour**, each of them a thing the reader of an expression can see (`amount` in an item of a statement is the
statement's; in a promise's item it is empty). They are inputs of one `evaluate`, not two functions. The same reading
of `post_journal`'s three matches on `Value` (1285-1314, 1336-1376, 1419-1447: seven reports of a fault between them) is
one match.

### 3.3 Where an error points

`TemplateError::Expression { fault, loc }` takes its `loc` from: the node (`compute`: a fault, or a scale that overflows
a computed amount), the flow (`expression`: a literal scaled to overflow, a leg or the header), the item (a template
item's literal scaled to overflow; a carve that overflows or mismatches), the leg (a carve that mismatches), the group's
header flow (`apply_occurrence_amount`). `InvalidTemplate { loc }` takes the second `...`'s flow, or the template's header
flow for an index or ordinal that overflows. The proof requires equal errors **including the `loc`**, so `Env`'s failure
carries the `loc` it was raised at, and `solve`'s own failures name the site that raised them.

### 3.4 Order matters in three places

1. The header's two sides are read `out` then `arrive`; a missing input in `out` omits the group only after `arrive` has
   been read, so a fault in `arrive` wins.
2. A leg is evaluated, then the second `...` is looked for, **then the next leg is evaluated**: the error for two `...`
   comes before a fault in a later leg.
3. All carving, then all `...`, then the emission; and an item's computed amount is read **after** the carving of the
   items before it. `solve` keeps these orders.

The flows are pushed between the steps today (the header before the legs, each item as it is read). A push can fail only
by a day that overflows when moved (`push_occurrence_flow`, 967), and every flow the materializer pushes has a real day or
`Day::MIN`, which is special-cased; so no input can make a push fail before a later step fails differently. The proof
(section 6) covers this by running both on every occurrence a generated book has, not by this argument alone.

## 4. Four things found on the way, shown on a book

Each was run against the binary of `39b6c8b` (`--today 2026-04-01`, the `splits.py` prelude).

### 4.1 A split statement creates money

```text
2026-03-01 checking 3_150 USD ->
  shop 1_050 USD
  acme 1_050 USD
  savings 1_050 USD
```

Net worth before the line 219,300.00 USD; after it, 220,350.00 (checking untouched at 200,000.00, savings 6,050.00).
Written as three plain transfers, the same book ends at 217,200.00. The line adds 3,150.00 USD to net worth.
`lower_leg` puts each leg's amount on `arrive` and zero on `out` (`(out, arrive) = (zero, amount)` for a source that is
`from`), and `Ledger::post` debits `from` by `out` and credits `to` by `arrive` (post.rs:90-91).

### 4.2 The total, and `...`, are inert

`checking 300 USD -> / shop 100 USD / savings ...` and the same with `999999 USD` print the same `check`, `balance` and
`register`. The `...` leg posts `0.00 USD`. A total written after the arrow (`checking -> 300 USD`) is not even read.

### 4.3 A statement's items are not carved

```text
2026-03-03 checking -> acme 100 USD #household
  60 USD #fun
  - 5 USD
```

posts `-100.00` (#household) and `-60.00` (#fun) from `checking`: 160.00 left, where LANGUAGE §3 says 40.00 stays
`#household`. The `- 5 USD` makes no flow (it says nothing its parent does not) and bears on nothing.

### 4.4 A `Target` leg of a promise carves its balance

```text
contract plan1 with shop
  1000 USD monthly on 5 from checking
  savings 200 USD
  reserve = 90000 USD
  bonus ...
```

materializes the header as `0`, `savings 200.00`, `reserve 90,000.00` (infer `Target`, mode `Planned`) and `bonus -89,200.00`
(mode `Actual`): the `=` leg's stand-in, its balance, is carved from the header like an amount, so `...` is
1000 - 200 - 90000. The kept occurrence posts the leg as written, 90,000.00 USD moved. The overdraft warning in the
book is this.

## 5. What `solve` takes, and what each difference becomes

```text
Env      what an expression comes to, in the phase that asks: `amount(site, header, expr)`, `payment(site)`.
         `Said::{Amount(a), Missing, Later}`: LiteralEnv says `Later` for a computed amount, the fold never does.
Site     `Header(side)`, `Leg(i)`, `Item(j)`: which flow an expression is read against.
Remaining  a header's two amounts (out, arrive) and what is taken from them (`take(side, amount)`, with the unit
         and overflow faults `subtract_parent` raises).
Asked    one effective leg: its part, the side it takes from, the unit of that side.
Take     one effective item: its sign, its amount, whether it takes from the header (Carve, or Less with no purpose).
Solved   what is left of the header; for each leg `Omitted`, `Value(Resolved)` or `Rest(amount)`; for each item its amount
         or `None` if omitted; and whether every amount is exact.
Resolved `{ amount, infer, mode: Option<Mode>, exact }`: what a quantity comes to. `mode` is `Some(Pending)` for a pending
         amount and `None` for "the flow's own"; `exact` is false for `=`, `all`, `?` (their amount is the balance's, not
         the quantity's) and for any amount that is `Later`.
```

How each difference of section 3 is carried:

| difference | what it is in the one function |
|---|---|
| escalation | not in `solve`: `Env::amount` scales a literal and a computed amount by the site's scale. `Share` is `header.of(side) * rate` with the `Remaining` the wrapper handed in, which is already scaled |
| written vs template reading | `Site` says which; the engine's `Env` keeps a table by site (flow, program, scale, ordinal) |
| `mode` of a leg | `Resolved.mode`: `Some(Pending)` or `None`; the wrapper puts the flow's own mode where `None` is, and the occurrence's where `Rest` is |
| a missing input | `Said::Missing`; `solve` omits the part and goes on |
| the statement's lack of a group algebra | **not a parameter**: `post_journal` does not call `solve`. The model calls `solve` for a statement only to check (`LiteralEnv`), with the header's total when it states one and the remainder rule below |
| what a remainder is | the check says: a promise's header **keeps** what the legs leave (its own flow posts it, so only a negative one is wrong); a statement's legs **are** the total (so a remainder that no `...` takes is wrong). This is the group's header: `Header::flow` or `Heading::Flow` keep it, `Heading::Source` does not |

## 6. The static check, what it will catch, and how it is kept honest

A split is checked **only if every amount of it is exact** (`Solved.exact`): a literal, a share, `...`. A computed amount, `=`,
`all`, `?` or an input says "the fold decides", and the model says nothing.

- **statement with a total** (`Heading::Source { total: Some(Amount(Literal)) }`): the legs, in the total's unit, add to the total, or
  one is `...` and the others do not exceed it. A leg in another commodity than the total's is a split that cannot conserve
  (nothing balances it).
- **named flow with items** (`Heading::Flow`): the items that take from the header (carve, `Less` with no purpose) do not take
  more than it has.
- **promise** (`Promised`): the legs and the items that take from the header do not take more than it has, and each leg is in
  the header's unit (the fold fails on a mismatch today, `UnitMismatch` from `subtract_parent`, on the first due day).

What it does not do: judge an occurrence (its legs are read against a template and a day's ratio), judge a header amount
written after the arrow (the model does not keep it: 0, item 4), or judge the items of a split.

How many books of `examples/`, the engine's and model's tests, and the `splits.py` corpus it catches is measured by the commit
that adds it, and listed in the report. The commit that adds it is separate from every other, so that the oracle can show that
**every difference between the old and the new binary is a book the check rejects**, and no other.

## 7. The plan

| step | what | proof |
|---|---|---|
| 1 | `model/solve.rs`: `Env`, `LiteralEnv`, `Quantity::resolve`, `solve`; unit tests of each part | unit tests; a property test against an independent model of K4a's rules on 200,000+ generated groups, equal results and errors |
| 2 | the engine: `FoldEnv`, one `evaluate`; `materialize_group` beside the old, tested against it **in one process** on every occurrence of 20,000+ generated books (`splits.py gen`) | equal flows, details, missing inputs and errors, counted by form |
| 3 | switch `instantiate_occurrence` to the new; delete the old and its test; `post_journal` on the one evaluator | splits oracle against the baseline binary: 0 differences; the `internals` dump: 0; `cargo test`; goldens |
| 4 | the model: `Quantity::resolve` for `resolve_quantity` and the three `stand_in`s | oracle, `internals`, tests, goldens |
| 5 | the static check, one commit | the oracle: each difference is a book the check rejects; every test and example listed |

## 8. How this map was checked

- Section 4 is four books run through `/tmp/axiom-k4b-base`, a release build of `39b6c8b`: 4.1's three registers and net
  worths, 4.2's `md5sum` of `balance` for the two totals and `register savings` for the `...` leg, 4.3's registers and
  `flow`, 4.4 by `check` (the overdraft) and by the `internals` dump of the same book, which prints every flow the kept
  occurrence and the forecast made (`out`, `arrive`, `mode`, `infer`).
- The table of section 2 is read from the code at the lines it cites. The claim that `post_journal` reads no leg,
  `side`, `total` or `Part` is the K4a map's (section 2) and was re-read: the group is touched at 1253-1261 and 1402-1408
  and in `asset_sale_less_items` (post.rs:956-960), and only for the header, the items' `flow`, `sign`, `amount` and `loc`.
- The claim that `examples/` does not exercise the engine: `check` of `02`, `04`-`08`, `11` stops at
  `purpose-disagreement`, `08` and `09` at `unknown-kind`, `10` at `chart-account`, so their goldens are mostly a record of
  errors. The oracle (`splits.py`, 2,000 books, 55,722 commands) and the engine's tests are what guard a change here.
- `cargo test --workspace --release --no-fail-fast` at `39b6c8b`: the three known failures
  (`a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`,
  `a_context_forecast_keeps_historical_and_same_day_obligations_once`,
  `native_loan_forecast_stops_after_the_typed_principal_is_repaid`).

## 9. Addendum: the scope the orchestrator gave after this map

The orchestrator checked section 4.1 on the integration binary and found it true, and that LANGUAGE.md §3 (normative) says the
opposite of what a split statement does: "their total is the header amount, or the sum of the legs", `...` is the remainder,
and items are "carved out of the header's amount". No example in `examples/` uses a split **statement** (every `...` there is
in a contract), so no golden depends on the wrong behaviour. The lane's rule "behaviour is preserved" is therefore **lifted
for the statement path** and kept for the promise path:

- A split statement and a statement's items mean what §3 says, **through the same `solve`** the promise path uses. The model
  solves a literal group (constant folding: the flows it makes carry the solved amounts, and the fold only posts). The fold
  solves the rest with its own `Env` when the group's first flow lands (computed amounts, `=`, `all`).
- The promise path, and everything `splits.py` writes for it, stays byte for byte. Section 3.1's accidents on the promise
  side (4.4's `=` leg that carves its balance, the leg modes, `all` and `=` posted as written by a kept occurrence) are
  preserved; 4.4 is decided in the report with the question for the user.
- Acceptance tests come first, written from §3's own worked examples (`14 visa -> target 120.00 USD #household` with 32.10
  and 12.00 items, 75.90 left) and from 4.1 to 4.3 as written; they are committed ignored, shown failing on the old code,
  and un-ignored by the commit that makes them pass.
- Every difference the oracle shows between the baseline and the new binary on a statement is classified (4.1 a leg did not
  debit its source, 4.2 the total and `...` were inert or a total after the arrow was dropped, 4.3 an item did not carve, or
  the new static check rejects the split). A difference that is none of these is a bug.
- The line target is secondary. Correctness, and one algorithm, come first.

Two things the statement path needs that this map's sections 2 and 5 did not plan for, found while reading for it:

1. **The `amount` an item's expression reads is the item's own stand-in zero** (`journal_expression` reads the item flow's
   out or arrive): `25% of amount` under a statement header posts `0.00 USD` today (`/tmp` probe: `checking -> shop 200 USD
   #household` over `25% of amount #fun` posts `#fun` 0.00 and `#household` 200.00). §3 says an item carves the header's amount,
   so `amount` in an item of a statement is the header's. This is a fifth class of difference, reported as such.
2. **`6%` as an item's amount does not compile today** (`type-mismatch: expected an amount, but this is a number`), and it is
   the second worked example of §3 (`- 6% #selling-costs`). A percentage of the header is a quantity the group has (`Part::Share`
   for a leg); for an item it is not built. Reported; built if the work allows.

## 10. What was built, and where it departs from sections 2, 5 and 9

`crates/model/src/solve.rs` is the algebra of one group: `solve(header, legs, items, settles, env)`. It reads no flow and makes none.
`Quantity::resolve` is the one reading of a written quantity (an amount, a remainder, a share, `all`, `=`, `?`) against an `Env`;
`Quantity::stand_in` is what it comes to with no book. `crates/model/src/balance.rs` is what the model does with `solve` for a
statement: it solves a group whose every amount is written out (`settle`), puts what each part came to into the flows (constant
folding, `put`) and says what cannot add up (the static check, `split-imbalance`). `crates/engine/src/occurrence.rs` makes a promised
or kept occurrence from `solve`; `statement.rs` solves a statement's open group when its first flow lands; `evaluate.rs` is the one
place an expression is read against a flow (`Lent::value`, `Evaluating`).

Where it differs from the plan above:

- **`solve` has a header that may have no amount, and a place for the remainder.** A statement's header may state no total
  (`Option<Remaining>`: nothing to carve, a remainder of nothing), and a promise's remainder is settled before its items while a
  split's is settled after them (`Remainder`): a promise's header is a flow of its own and keeps what the items leave, a split has no
  header flow.
- **`Env` has `lands`, which section 0 said it would not.** Section 0 argued that `all` and `=` mean the balance at the moment their
  flow lands, so that a group solved in one go reads the balance from before its first leg. That is true, and a remainder needs every
  other leg known, so a statement's open group *is* solved in one go, at the first of its flows to land; an `=` or `all` leg, and an
  `all` header, read the book then. A promise's `Env` reads the book for one thing, an `=` leg's account, because the gap to a balance
  is what the leg moves (section 9, 4.4); its `all` and `?` stay the markers they were, with the zero they carry.
- **`Item.amount` is a `Cut`** (`Of(Expr)` or `Share(Ratio)`), so that `- 6%` is an item (section 9, point 2). `solve` settles a
  share itself, of the header as it was given, as it does a leg's. A promise's items are still only `Of`.
- **post_journal calls no `solve`.** It reads one flow's computed amounts through `amount_of`, which is `evaluate.rs`'s reading; the
  group of an open statement is solved in `statement.rs` before its first flow posts, and `Record::resolved` says what each flow came
  to. Section 2's table said `post_journal` was a third copy of the algorithm; it is a reader of one flow, and the copies were
  `lower_*` (model), `materialize_group` (engine, promises) and the expression reading around `post_journal`.
- **The model's solve for a statement is `balance::settle`.** `lower_split_flow`, `lower_named_flow` and `lower_owes` call it; what it
  cannot close stays open (`Program.open`) and the fold solves it. `Record::unsolved` remembers a group the fold could not solve, and
  is hashed into checkpoints.
- **The model's three readings of a written quantity are `Quantity::resolve(LiteralEnv)` and `stand_in`**, not three matches.
- **A leg in another commodity than the total is the exchange of the remainder, and the solver is not told so.** The model makes
  such a leg's flow with its written amount on its own side and zero in the total's commodity on the source's, and `Statement::asked`
  asks the solver for a remainder where a leg's flow is an exchange; `balance::moved` puts the answer on the source's side alone.
  That keeps `solve` as it was (a leg in another commodity is still a `UnitMismatch` for a promise) and needs no new `Drawn`.
- **`post_journal` is two functions** (`post_journal` finds what a flow is, `post_computed` reads its amounts, basis and cost),
  and `lower_owes` is three (`claim_ends`, `claim_program`, the group), because the lane made both longer.

What was not done: the line target. `ledger.rs` lost 1,001 non-test lines, and `occurrence.rs`, `statement.rs`, `evaluate.rs`,
`solve.rs` and `balance.rs` hold what they became, with their tests, so the lane is longer than it found the code (section 11.5).
Whether an item is an exchange's cost (`is_exchange_cost` in `statement.rs`) is still decided by the fold and not at model time. A
`?` leg beside a `...` leg is `cannot-infer` for the fold's solver, as it is opaque to `infer`. A fee leg of an exchange split is not
a cost of the exchange (the README of example 08 says it is: 9,500.00 USD of basis, not 9,444.90); it is a payment of its own.

## 11. What the statement path found, and what it changed

### 11.1 The premise of section 9 was wrong about the examples

Section 9 says "no example in `examples/` uses a split statement". Five do: `04-freelancer` (8, Stripe fee legs), `06-investor`
(16, paystubs with `...`), `08-expat` (38: paystubs, Wise-style conversions with a leg in another commodity, a fee leg),
`09-shared` (63: card bills split between owners, paystubs) and `10-budgeter` (13 paystubs). (A statement is a dated line with an
arrow whose indented lines are legs; a script over `journal/` counts them.) So the goldens do change, and for the reason the lane
was given: 17 of 60 golden files (04: 2, 08: 6, 09: 3, 10: 6) and 2 of 100 mistakes (`34-split-short`, `35-split-over`).

| files | what changed | why |
|---|---|---|
| `04-freelancer-{tax,available}` | gross receipts 132,900.00 to 158,672.40 (35 to 42 sources), business expenses 6,320.31 to 6,847.91 (19 to 26), and everything that reads them | the seven `fernhill -> 3_100 USD` bills, with `business-checking 3_009.80` and `stripe 90.20 #business-fees` as legs, are flows from the party now: the income and the fee are recognized, where a leg's flow had no amount on its source side |
| `08-expat-*` (6) | balances, net worth, the assertion gaps | the paystub's `us-checking ...` receives the net pay it never did; the Wise conversions (`girokonto 900 EUR ->` with a fee leg and a USD leg) post as an exchange at the implied rate instead of creating dollars |
| `09-shared-{balance,available,tax}` | checking 1,031.39 to 25,881.80, wages 6,232.01 to 31,082.42 (44 to 66 sources) | the paystubs' `...` to checking |
| `10-budgeter-*` (6) | checking 679.92 to 13,230.57; one `cannot-infer` and one `unchecked` note go (the assertions the amounts were solved from now hold different balances) | the paystubs' `checking ...` |
| `34-split-short`, `35-split-over` | one error is added: `split-imbalance`, at the legs | these are the mistakes their first comment says: legs that add to 4,800 of a 5,000 total with no remainder, legs that take more than the total. The baseline found 3 other errors and not this one |

One book shows what the paystub is: `studio -> 2_307.69 USD` with `fed 226.00`, `payroll-office 176.54`, `insurer 60.00` and `checking ...`
over a checking account of 1,000.00 leaves it at 1,000.00 on the baseline (the net pay never arrives) and at 2,845.15 now:
2,307.69 - 226.00 - 176.54 - 60.00 = 1,845.15, which is §3's "...".

What these goldens do **not** show is that the new numbers are right: every example still carries 36 to 461 errors (v3 syntax:
`/ party`, `income/` accounts), the statements that fail are dropped, and the READMEs' hand-verified figures are matched by neither
build (10-budgeter on 2026-02-14: checking 6,597.01 by hand, 679.92 before, 13,230.57 now; net worth 10,653.04, 9,699.07, 22,615.12).
The new numbers are further from the READMEs than the old, with the dropped statements between. Section 11.3 is what shows the
semantics right: a split is the plain transfers it says, on 5,000 generated books.

The goldens are regenerated and committed on their own, in `tests: goldens after K4b` (the orchestrator asked to review that diff
alone): the integration branch at 92e80c1 reproduces its committed goldens and mistakes byte for byte, so the 19 files are the statement
path's and no other lane's. `git revert` of that commit puts the old ones back.

### 11.2 Every behaviour change

LANGUAGE.md line numbers (§3 is lines 130-290). "Ask" says whether the user should be asked.

| # | before | after | spec | ask |
|---|---|---|---|---|
| 1 | a split's legs did not debit the source: `checking 100 USD -> acme 60 USD, shop 40 USD` left checking at 10,000.00; legs to the owner's own accounts (`savings 60`, `reserve 40`) created 100.00 | the source is debited by the legs: 9,900.00; nothing is created | 184-185 | no |
| 2 | `...` was a zero flow | it is what the other legs leave: `reserve ...` of `100 - 60` is 40.00 | 186 | no |
| 3 | a total after the arrow (`checking -> 100 USD` over legs) was dropped; a header with no total posted its legs and not their sum | the total is read from either side; with none, the sum of the legs is it | 132-135, 185 | no |
| 4 | an item was paid on top of the header: §3's own example (`120.00` with `32.10` and `12.00` items) debited 164.10 | an item is carved from it, 75.90 keeps the header's purpose; `+` comes on top, `-` is taken off | 197-201, 211-214 | no |
| 5 | `- 6%` did not compile (`type-mismatch`) | it is 6% of the header: 37.62 of 627.00 | 200, 214 | no |
| 6 | `2% of amount` in a statement's item read the item's own flow and posted 0.00 USD | `amount` is the header's amount in the item's commodity: 12.00 of 600.00 | 197-206 | no |
| 7 | the items of a split went between the source and the first leg, reversed for a split that arrives (`- 5 USD #fees` credited the source 5 on top of the legs) | between the source and the remainder leg, in the legs' direction: source 9,905.00 | 203-204 | no |
| 8 | a statement's `= AMOUNT` leg moved its gap and the header was not debited by the rest | the leg moves the gap, `...` takes what is left: `checking 1_000 USD -> reserve = 5_040 USD, shop ...` debits checking 1,000.00 | 186-187 | no |
| 9 | `all` as a leg moved everything its source held whatever the total; `all` as a header was ignored (`reserve all -> savings 60 USD, checking ...` posted 60 and nothing to checking) | a leg's `all` is everything the source holds when the split lands, and counts against the total; a header's is the total, so `checking ...` receives the 4,940.00 | 132-135, 186 | no |
| 10 | a split that does not add up posted what it said: legs over or short of the total, items over the header, a leg in a commodity nothing balances | `split-imbalance`, said at the header and the legs with what each takes, and nothing posted. Judged only when every amount is known (a computed one is judged by the fold). Not judged when a leg failed to read: that error is the one | 184-186, 219 | no |
| 11 | a leg in another commodity than the total (`girokonto 900 EUR ->` with `fx-fees 4.77 EUR`, `us-checking 1_027.63 USD`) was a flow with nothing out and the dollars in: currency created (net worth +96,000.00 on a probe) | the exchange of what the other legs leave, at the implied rate (out 895.23 EUR, in 1,027.63 USD); with `...` or a second such leg it is an error that says two legs want the remainder, with nothing left it is an error | 217-221; README of example 08 | **yes**: the README says the fee leg is a cost of the exchange (basis 9,500.00 and not 9,444.90); that is not done, it is a payment |
| 12 | a promise's `= AMOUNT` leg carved the balance itself as if it were the amount (`savings = 5_030 USD` of a 100.00 promise moved 5,030.00 into savings, and the remainder was -4,930) | it moves the gap to the balance (30.00 from savings at 5,000), and the header's remainder is what is left (70.00); a kept occurrence posts what it made, so it moves the gap as well | 186-187 | **yes**: the other reading is that an `=` leg is not carved from the header at all (the header pays its end in full and the leg moves its own gap on top) |
| 13 | `savings ? USD` beside `...` was `cannot-infer`, nothing posted | still `cannot-infer`; the remainder takes the whole total meanwhile, so the books are not consistent until the error is fixed | none | no, a limitation |

Nothing else on the promise path changes: 25,000 generated books with 70,589 promises and 1,000,279 forecast occurrences, 4,794
differ and every one has an `=` leg; 2,000 books on the whole CLI, 448 differ and every one has an `=` leg (11.3).

### 11.3 How it was checked

- **A split is the plain transfers it says.** `splits.py equiv`: 5,000 generated splits (from and to a source, a named header with
  items, totals before, after or none, `...` first or last, amounts, computed legs, `=` legs, `- 6%` shares, `% of amount`, a leg in
  another commodity with one or two fee legs) each run on the new build against the plain transfers LANGUAGE §3 says it is on the
  baseline, which gets plain transfers right: balance, flow and net worth the same, 5,000 of 5,000 and none with an error. Mutants of
  the code it checks (`/tmp/k4b/mutants.txt`) are each killed by it; the 8 mutants of the exchange leg, the `all` header and the
  lost-leg guard are each killed by `split_tests.rs`, one survives that is a word of a message.
- **The acceptance tests** are `crates/engine/src/split_tests.rs`: 25, from §3's worked examples and from 1-11 above. The first 19
  were written before the code and failed on the baseline; the six after it (an `all` header, a leg in another commodity three ways,
  and a leg that failed to read) were written with it, and a mutant of each is killed by them.
- **The statement-path oracle, per recipe** (600 books each, `splits.py gen ... recipe:NAME`, new against baseline on `check`,
  `balance` and `flow`): transfer 0 differ, claims 0, basis 0 (the recipes with no split); `unknown` 105 differ (0 without a `?` leg),
  `items_under_header` 355 (0 without a carved or computed item), `exchange` 61 (0 without a computed cost item), `split` 588 (the
  recipe is splits, so every project is explained by it; the 12 that do not differ have a header with no total and legs that are all computed fractions, pending amounts, `=` targets or `...`: nothing the baseline left unposted). **0 differences are unclassified** at that grain. A finer rule, by the forms each statement holds (a literal leg, `...` under a
  total, a computed or pending leg under a total, a carved item), agrees with 591 of the 600 `split` books; of the 9 it does not
  predict, 2 differ because an item sits at the remainder leg and no longer at the first (row 7), and 7 are headers with no total
  whose legs are pending, `=`, `all` or `...`, which the baseline posted right already or does not count as money.
- **The static check** over the 2,000 books of `splits.py gen statements`: 488 raise `split-imbalance` (524 errors: 308 legs short
  of the total, 216 over it), the baseline none. No book of `examples/` raises it, and the 2 mistakes that do are the ones that are
  meant to.
- **The promise path.** `internals` (every promise kept or missed, every flow materialized, the debug text of every posted flow,
  every due day to the end of 2027): 0 differences before the `=` fix on 2,000 books, and after it 393 of 2,000 (4,794 of 25,000), all
  with a promise `=` leg; the whole CLI (63,201 commands): 448 of 2,000, all with an `=` leg, none without. An `=` leg of a kept
  occurrence is in them.
- **The tests**: `cargo test --workspace --release`: 955 passed, 3 failed, 18 ignored; the 3 are the failures of the integration
  branch (`a_prorata_place_realizes_only_the_lots_share_and_deferrals_merge_into_one_lot`,
  `a_context_forecast_keeps_historical_and_same_day_obligations_once`,
  `native_loan_forecast_stops_after_the_typed_principal_is_repaid`).

### 11.4 What the oracles cannot see

- The books of `splits.py` use few commodities and every account is a bank; a leg that exchanges into a parcel-holding account
  (cost basis, lots) is posted as the fold posts any exchange, and nothing says that is what the user means.
- `is_exchange_cost`: a split's fee leg is not a cost of its exchange (11.2, row 11).
- A promise's `all` and `?` are still the markers they were; a kept occurrence posts them as written (zero).

### 11.5 Lines

Non-test Rust lines, `briefs/loc.py`: 49,943 before and 50,801 after (+858): model +651 (`solve.rs` 285, `balance.rs` 191,
`lower/record.rs` +70, `problem.rs` +53, `split.rs` +27, `flow.rs` +16, `journal.rs` +11), engine +206 (`occurrence.rs` 815,
`statement.rs` 309, `evaluate.rs` 70, `ledger.rs` -1,004). Functions over 80 lines: 16 before, 12 after (`post_journal` 249 to 29, with `post_computed` of 60 beside it;
`materialize_group` 324 to 65; `instantiate_occurrence` 135 to 47; `lower_owes` 140 to 75).
The brief's -1,500 was out of reach: the three copies were not three copies of one algorithm (section 0), the promise path
(`materialize_group`) moved to `occurrence.rs` whole with its tests, the statement path did not exist in the fold and had to be built,
and the static check and the acceptance tests are the lane's too.
