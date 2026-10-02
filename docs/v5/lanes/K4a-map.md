# K4a map: the two families of a split, where each is built and read

Written before the first code change of lane K4a, from the code at `589b9b5`, and checked against what the code does
(the last section says how). Paths are in `crates/`; line numbers are those of `589b9b5`.

A statement that moves value says a **header** (one end, an amount), **legs** (the other ends, each with a quantity)
and **items** (signed amounts carved out of, added to or taken off the header). Today three things say it, and
each is read by its own code:

| | a *promise*: a contract's template | a *statement*: a journal transaction | an *occurrence*: a statement that keeps a promise |
|---|---|---|---|
| the whole | `TemplateFlow` in `Terms.template` | `JournalGroup` in `JournalProgram.groups` | `WrittenGroup` in `WrittenOccurrence.groups` |
| expressions | `Terms.program: TemplateProgram` | `Txn.program: JournalProgram` | `WrittenOccurrence.program: JournalProgram` |
| engine | `materialize_group` | `post_journal` | `materialize_group`, over the template |

## 1. Quantities

### 1.1 `TemplateQuantity` (book.rs:696)

Built only in `model/lower/contracts.rs`, and only for a promise:

| where | function | what it can be |
|---|---|---|
| a header side, `Header.out` and `.arrive` | `template_header` through `schedule_amount` and `template_amount` (contracts.rs:684, 757, 877) | `Amount(None)` for a literal, `Amount(Some(root))` for a computed amount, `Derived` when the schedule has no payment (a loan's); `.arrive` is `Unknown(unit)` instead, for a standing `buy` |
| a split leg, `TemplateLeg.quantity` | `template_legs` through `template_quantity` (contracts.rs:723, 809) | everything but `Derived`: `Amount`, `Pending`, `Target` (each with or without a root), `Percent`, `Unknown`, `All`, `Rest` |

A literal amount lives in the `Flow` beside the quantity (`flow.out` or `flow.arrive`), and `Amount(None)` says "take it
from there": the quantity is not self-contained. `as_quantity(amount, kind: u8)` (contracts.rs:865) retags an
`Amount` as `Pending` or `Target` by a number.

Read in `engine/ledger.rs`: `template_quantity` (1069, header sides and every leg that is not `Percent` or `Rest`),
`has_computed_quantity` (1903), and the leg loop of `materialize_group` (707), which handles `Rest` and `Percent`
itself. In `report/contracts.rs`: `template_quantity` (147), which prints every variant. `Whole` is built (contracts.rs:861)
and read (ledger.rs:1132, contracts.rs:172) and never occurs: see 1.3.

### 1.2 `JournalQuantity` (journal.rs:713)

Built in `model/lower/flow.rs::resolve_quantity` (149) for every quantity a record writes, and carried out of it in
`ResolvedQuantity.group`. It is **stored** in three places, and used from only some of them:

| stored in | by | what it can be | read by |
|---|---|---|---|
| `JournalGroup.leg_quantities`, parallel to `legs` | `lower_split_flow` (record.rs:355), once per split leg; `lower_occurrence` (record.rs:988), once per leg an occurrence replaces or adds | `Amount`, `Pending`, `Target` (each with its root, and the amount that is zero when there is a root), `Unknown`, `All`, `Rest` | the engine, for an occurrence's legs only (`written_quantity`, ledger.rs:1147); for a split statement by tests alone |
| `JournalGroup.total`, the split header's own side | `lower_split_flow`; `lower_owes` (record.rs:1428) | those, and `Derived` (a claim written by its items alone) | tests alone (`source_tests.rs:1043`) |
| `WrittenGroup.out`, `.arrive` | `lower_occurrence` (record.rs:1020) | **`None`, always** | the engine (`materialize_group` 532, 567, 604-613), on a branch that cannot run |

`Whole` is built for an opening line (`lower_opening_leg`, record.rs:551) and its `group` is dropped there: nothing
stores it. `Derived` is stored only as `total`.

### 1.3 Which of `Percent`, `Rest`, `Whole`, `Derived`, `Unknown`, `All` can reach the engine's occurrence path

The engine's occurrence path is `Ledger::instantiate_occurrence`, used for a kept occurrence and for the forecast.

| form | made by | reaches the engine as | resolved by lowering? |
|---|---|---|---|
| `Percent(rate)` | `template_quantity`, for a literal `6%` leg of a promise (contracts.rs:834) | a promise's leg; resolved in the leg loop against the header's resolved side, before any carve | no. A bare percent leg of a *statement* is a type error (`shop 30%`: "expected an amount, but this is a number"), so it never reaches a `JournalQuantity` |
| `Rest` | `...` on a leg: promise (contracts.rs:860) and statement or occurrence (flow.rs:193) | a leg of a promise, or a leg of a written occurrence; resolved after every carve (ledger.rs:871) | no. For a *statement* the engine never resolves it: the leg posts as zero (see 3) |
| `Whole` | `basis` in an opening (`Scope::Opening`, syntax/flow.rs:186); the arms in contracts.rs:861 and ledger.rs:1132 | never: a promise's leg is parsed `Scope::Undated` and an occurrence's `Scope::Statement`, which cannot say it. `lower_occurrence` has a diagnostic for it (record.rs:883) that cannot fire | yes: the opening makes one unit of the asset (`Mode::Opening`) and keeps no quantity |
| `Derived` | `schedule_amount` when a schedule has no payment (contracts.rs:774); `lower_owes` (record.rs:1428) | a promise's header side only; the loan's payment, from `loan_payment` | no: it depends on the terms in force on the due day |
| `Unknown(unit)` | `? USD` on a header side or a leg; a standing `buy`'s `arrive` (contracts.rs:716) | a promise's header (`buy`) or leg, or an occurrence's leg; becomes `Infer::Unknown` and is solved by the plan from balance assertions | no |
| `All(unit)` | `all` or `all UNIT` | a promise's or an occurrence's leg; becomes `Infer::All`, resolved when the flow lands | no |

`Pending` and `Target` are not in the question, and reach the engine from the same places as `Amount`.

The map of where a variant can be, from the builders (a dash is "cannot"):

| | promise header | promise leg | statement header (total) | statement leg | occurrence leg |
|---|---|---|---|---|---|
| `Amount` | yes | yes | yes | yes | yes |
| `Pending` | – | yes | yes | yes | yes |
| `Target` | – | yes | – (legs only) | yes | yes |
| `Percent` | – | yes | – | – | – |
| `Unknown` | `arrive` of a `buy` | yes | yes | yes | yes |
| `All` | – | yes | yes | yes | yes |
| `Rest` | – | yes | – | yes | yes |
| `Whole` | – | – | – | – | – |
| `Derived` | yes | – | `lower_owes` only | – | – |

### 1.4 What differs between the two functions that read them

`ledger.rs:1069` `template_quantity` (77 lines) and `:1147` `written_quantity` (70 lines) are one match with five
differences, all of them where the quantity lives, none of them in what it means:

1. A literal: the template's is the flow's side, **scaled by `ratio`** (the escalation of the day); the written one is
   the variant's own amount and is **not** scaled. A computed root is scaled by `ratio` in a template and by
   `Ratio::ONE` (the identity: `Qty::scale` is `x * 1 / 1`) in a written one.
2. Which program the root points into: `terms.program`, or the occurrence's `JournalProgram.program`.
3. `Rest`: an error in `template_quantity` (its callers take `Rest` out first), a value `ResolvedLeg::Rest` in the other.
4. `Percent`: an error in `template_quantity` (taken out first); not a variant of the other.
5. `Target`'s balance: the scaled amount's, in both.

The errors of 3 and 4 are the only `Err(InvalidTemplate)` that a quantity can raise, and no builder in 1.1 and 1.2 can
make either reachable.

## 2. Groups

| | `TemplateFlow` | `JournalGroup` | `WrittenGroup` |
|---|---|---|---|
| defined | book.rs:654 | journal.rs:726 | journal.rs:646 |
| the header | `flow: Flow` (a value), `out`, `arrive: TemplateQuantity` | `header: Option<u32>` (an offset into the transaction's flows) or none, with `source: JournalEnd` and `total: Option<JournalQuantity>` | none (`header` is `None`); `source`, no `total`; `out`, `arrive`: always `None` |
| legs | `Box<[TemplateLeg { flow: Flow, side, quantity }]>` | `legs: Box<[u32]>` and, parallel, `leg_quantities` | the same, as a `JournalGroup` in `group`, with `template: u32` saying which template group it replaces |
| items | `Box<[TemplateItem]>`, 11 fields | `Box<[JournalItem]>`, 6 fields | the same |
| built | `lower_terms` (contracts.rs:629), one per schedule: `Terms.template` is always a box of **one** | `lower_named_flow` (record.rs:301, only when items follow), `lower_split_flow` (355, always), `lower_owes` (1285). A statement has **at most one** group | `lower_occurrence` (record.rs:622), from `OccurrenceGroupDraft`: at most one, for template 0 |
| read | engine `materialize_group` (ledger.rs:468), report `contracts.rs`, `why/line.rs`, `book.rs::template_covers_flow`, `lower_occurrence` (matching legs), `lower_loan_origin` | engine `post_journal` (1581), `is_exchange_cost` (2569), `post.rs::asset_sale_less_items` (944) | engine `materialize_group` |

What the engine reads of a statement's `JournalGroup` is small. `post_journal` and `asset_sale_less_items` read
`header` (to know whether this flow is a group's header), and from `items`: `flow` (an offset), `sign`, `parent`, `amount`,
`loc`. **Nothing reads `source`, `side`, `total`, `legs` or `leg_quantities` of a statement** except tests
(`source_tests.rs:1038`, `native_records.rs:1190-1276`); they are the typed split, kept for the K4b that will solve it.
For a written occurrence the engine reads `legs`, `leg_quantities`, `side` and `items` in full, and never `header` (always
`None`) or `source`.

### 2.1 What is different, really

- **Where the flows are.** A promise's flows are values in the template: they are not in `Book::flows` and the engine
  clones them. A statement's and an occurrence's flows are in `Book::flows` (an occurrence's as metadata overlays, which the
  timeline skips: `timeline.rs:365`), and the group names them by offset into `Txn.flows`.
- **What an item is.** A promise's item is a *delta* over its parent flow as the fold has it (purpose, description, codes,
  selectors, waiver; its `detail` is always `None`), applied after the occurrence's own tail has changed the parent.
  A statement's or an occurrence's item is a *flow already made* (`flow: Option<u32>`), when it says something its
  parent does not. They cannot be one thing without a behaviour change: a prebuilt item flow would not inherit the
  occurrence tail's `payee` and `description`, which the template item does through the runtime parent
  (ledger.rs:926, 965-984).
- **The header.** A promise's header is a flow of its own, the remainder after the legs. A statement's split header names
  one end and states a total (`total`), and the remainder is a `...` leg. A named-ends statement with items is the first
  kind: `header: Some(offset)`.

### 2.2 What is not different, though it is stored twice

- `TemplateLeg.side` is `header.side` for every leg of a template; `JournalItem.side` and `TemplateItem.side` are the group's
  `side`, for every item (flow.rs:503, contracts.rs:913, record.rs:1001). The `side` belongs to the group.
  (Its meaning differs between a promise and a statement; see 3.)
- `TemplateItemParent::Leg(u16)` is **never constructed**. Both builders write `Header` (flow.rs:502, contracts.rs:913),
  and the syntax has no item under a leg. The engine's `leg_positions` (ledger.rs:887, 911-925, 994-1003) exists for it.
- `WrittenOccurrence.groups[i].template` is always `0` (a `Terms` has one template) and the engine re-validates it
  (ledger.rs:360-376: a template index in range, no repeated index, `legs.len() == leg_quantities.len()`, every offset in range).
- `WrittenOccurrence.program.flow_roots` (record.rs:975) is written and never read: the engine posts an occurrence through
  `materialize_group`, which reads only `.program` (the nodes), and `post_journal` reads `Txn.program`, which an
  occurrence transaction does not have (`record.rs:1053`).

## 3. A fact the unification must not change

A statement's split does not mean what a promise's does, and the lowering and the engine agree on neither. Nothing here
is for K4a to fix; each is behaviour the oracle must see unchanged.

- A statement's `...` leg is zero: `checking 100 USD -> / shop ... / savings 30 USD` posts `shop` and `savings` as
  `+0.00` and `+30.00` from a source that loses `0`.
- A statement split's leg puts its amount on the side that is not the source's (`lower_leg`: `(out, arrive) = (zero, amount)`
  when the source is `from`), so the source is not debited. A promise's leg is subtracted from the header.
- `side` is the *source's* side for a statement (`SplitEnd.side`, record.rs:362) and the side the legs *carve* for a promise
  and an occurrence (`header.side`, `template_side`), which are the opposite ends.

## 4. Programs

| | `TemplateProgram` | `JournalProgram` |
|---|---|---|
| defined | book.rs:647: `nodes: Arena<Node>` | journal.rs:685: `program: TemplateProgram`, `flow_roots`, `groups` |
| held by | `Terms.program`; `Book.assertion_programs: Arena<TemplateProgram>` (`Assert.computed`) | `Book.journal_programs`, by `Txn.program` and `WrittenOccurrence.program` |
| made by | `laws::compile_template` (compile.rs:254), `compile_budget_limit` | `record.rs` (6 sites), `statements.rs:761`, always with the `TemplateProgram` of `compile_template` |

`FlowExpressions { flow, out, arrive, basis }` is the sparse root of each flow of a statement that has a computed
quantity or basis, sorted by `flow` (an offset); it is read by `post_journal` (ledger.rs:1592, `partition_point`) and by
`infer.rs::computed_amount` (263, `partition_point`). A *group's* quantities hold the same roots for the flows that are
legs, in `leg_quantities`: nothing joins the two. Flows that are in no group (an opening line, a basis statement, a loan's
origination, a transaction of one named flow with no items) have only `FlowExpressions`.

`Txn.program` is `None` for a transaction with no computed amount or basis and no group. `lower_split_flow` always makes a
group, so every split statement has a program.

## 5. What this decides

1. **One `Quantity`, and a `Part` for what a leg may be besides.** The forms that need the group to resolve
   (`Percent`, `Rest`) are the only forms a header side cannot be, and they are exactly the ones whose resolution the leg
   loop already does by itself. A header side is a `Quantity` (`Amount`, `Pending`, `Target`, `Unknown`, `All`, `Derived`:
   every one has a meaning on a side); a leg is a `Part` (a `Quantity`, a share of the header's side, or the rest).
   `Whole` is resolved by lowering and has no variant. `WrittenGroup.out` and `.arrive` go: they are never `Some`.
2. **One expression is `Literal` or `Computed`**, held in the quantity, so a template quantity carries its literal.
3. **A `Group` over a phase**: header, legs with their quantities as values, items; for a promise the flows are values, for a
   statement they are offsets. The item's flow is a *delta* in the first and an offset in the second: that is the real
   difference, and the phase says it. `side` is the group's. `Leg(u16)` goes, and with it `leg_positions`.
4. **One `Program`**: the expression arena and the sparse per-flow roots; the groups leave it.
5. **One function resolves a quantity**, taking the program and the scale (`ratio` or `ONE`) as what they are.

## 6. How this map was checked

`docs/v5/measure/splits.py gen DIR 2000 1` writes 2,000 books that use every form above. The same books were run
(`check`, `forecast`, `contracts`) through a binary built from `589b9b5` plus an `eprintln!` at each builder and at each
engine arm of 1.1, 1.2 and 2: a scratch copy, not committed. Projects that reached each arm, of 2,000:

| arm | projects |
|---|---|
| promise header sides (built) | `Amount(literal)` 598, `Amount(computed)` 189, `Derived` 175, `arrive = Unknown` 114; no other form |
| promise legs (built and read) | `Amount(literal)` 264, `Amount(computed)` 136, `Pending` 154, `Target` 148, `All` 171, `Rest` 157, `Percent` 152; `Unknown` and `Whole`: none (`Unknown` is not in the generator for a promise's leg, which would need a balance to solve it) |
| promise items (built) | all `parent = Header`: `Add` computed 169, `Add` literal 127, `Carve` 99, `Less` 99 |
| occurrence legs (`leg_quantities` of a `WrittenGroup`) | `Amount(literal)` 144, `Amount(computed)` 33, `Pending` 21, `Target` 24, `All` 28, `Rest` 50 |
| `WrittenGroup.out`, `.arrive` | `None` in all 560 groups made (each built by three commands), and `written=false` at every one of the 16,048 header resolutions |
| statement groups | named flow with items 768, split from the source `Out` 520 and `Arrive` 440 (a total of `Amount(literal)` in 299, none in 661), `owes` with items 109 (`total = Derived`) |
| statement split legs | `Amount(literal)` 797, `Amount(computed)` 202, `Pending` 226, `Target` 107, `All` 127, `Rest` 447, `Unknown` 35 |
| the engine's `Percent` arm | 141; the two `Err(InvalidTemplate)` arms for `Percent` and `Rest` in `template_quantity`: 0 |

So the table in 1.3 is what the code does, with one cell the generator does not exercise: a promise's `Unknown` leg.
