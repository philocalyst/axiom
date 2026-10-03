# K6 map: what the five "derive" mechanisms do, what ranks a purpose, what keys a rule, and what projection needs

Written before the first code change of lane K6, from the code at `8e33a9d` (K3b merged), and checked against what the
code does (the last section says how). Paths are in `crates/`; line numbers are those of `8e33a9d`. The vocabulary is
K4a's and K4b's (`model/split.rs`, `model/solve.rs`), K5b's and K5c's (`engine/promising.rs`, `engine/occurrence.rs`) and
K3b's (`model/addresses.rs`: an account's fillers).

The brief says five things "say when a flow of this kind happens, derive another flow or a check", each with a path of
its own, and that one effect, `Derive`, replaces four of them. **Four of the five have no run-time path at all.** That
decides the lane, so it comes first.

## 0. What the brief says and what the code does

Nine things in the brief or in the research do not match the code. Each decides something below.

1. **`also`, `share`, `match` and `sales-tax` are lowered, validated and read by nothing.** They are not four paths to
   merge; they are four records the engine never reads (section 1). A book that writes them gets the diagnostics and
   no flow. Three one-file books in `docs/v5/measure/k6/probes/` show it on the baseline: a lease with
   `also -> escrow 100 USD #contribution` opens no `escrow` and moves no money into it; a party kind's `sales-tax 10%`
   and a contract's `share 60% for studio` change no total, no flow and no tally. `Derivation::{Also, Share, SalesTax,
   PassThrough, PaidFor, Reparation, WriteOff}` are **never constructed**; `Interest` and `Principal` neither. LANGUAGE §10
   describes all of them as derived events. They were live in v4's engine and the cutover dropped the engine half (the
   comment in `laws/mod.rs:253`, "the native group builder applies it to the matching flow", names a builder that no
   longer exists). The brief's "`Derive` replaces the paths" is therefore "`Derive` is the first path": the lane
   **builds** what four mechanisms promise, and **changes** every book that writes one (section 6).
2. **A law owned by a contract never fires either.** `Rules.contracts` (`law.rs:595`, "laws of a contract") is filled
   (`rules.rs:320`) and included in the sets the plan watches (`Rules::all`) and never read to fire: nothing calls
   `fire` with it (`post.rs` fires `on_out`, `on_in`, `always`, `purposes`, `about`, `on_gain` and `on_spend`).
   `contract-law-never-fires.ax` shows a contract's `on flow` law that should warn on three occurrences of 1,000 USD and
   never does, beside a purpose's that warns on the journal flow. "An `also` is a law owned by its contract" is true of
   the data and means nothing until something fires a contract's laws (section 3).
3. **The three inference functions are one pipeline, not three rankings** (section 2). `endpoint_purpose` ranks within
   one end (the party's own purpose, then its kind's `pays`, then its kind's `purpose`); `taken_purpose` is a rewrite
   (an account kind's `takes`), not a source; `infer_for_flow` demands that the two ends and the written purpose
   agree, and **drops the flow** (`make_resolved_flow` returns `None`) when they do not. There is no ranking between
   sources today: any two that name unrelated purposes are an error. LANGUAGE §2 says both "first match wins" and "two
   sources naming unrelated purposes ... are an error"; four model tests pin the error.
4. **`budget` already is a law.** `laws/budget.rs` builds a `Law { trigger: Flow, owner: Purpose, steps: [when, warn
   BudgetTotal <= BudgetLimit] }` (`lower_budget`), and `Law::cap` reads it back as a cap. What is dedicated is its two
   functions, `Func::BudgetTotal` and `Func::BudgetLimit`, the `Budget.terms` timeline they read (dated changes, `carries`,
   `funded from … into`, a limit that is a share of another purpose's total or a computed expression) and
   `engine/budget.rs` (the segments of a dated budget). A plain law cannot say "the limit in force that day, with the
   unspent part of earlier windows carried": that is K2's timelines plus a `carried` total. So the brief's "delete the
   budget functions as far as the laws now carry them" is **nothing**: the laws carry none of them (section 1).
5. **`Rules` has eight keyed tables and one list, and one of the eight is never read** (section 3).
6. **An item cannot be derived after the fact; a leg can.** An item is carved from its header: the header's amount
   shrinks and the item takes it, **along the header's own ends** (`occurrence.rs:push_item` clones the header flow; a
   `-` item swaps the ends). Posted flows are already posted when a law fires on them (`post.rs`: laws fire after the
   value has moved). So of the three things an `also`/`share`/`sales-tax` can be (section 5): a *flow of its own* and a
   `+` or `-` item (a flow along the same or the reversed ends) are derivable by a law that fires on a posted flow;
   a carved item, which `share` and `sales-tax` are (a part of the flow re-attributed to another owner, or to a tax
   purpose), is only derivable while the group is being solved, which is the occurrence's (`solve`).
7. **Contracts have no kind.** The brief's `kind employment : contract` and `contract alex-pay : employment with acme`
   are not a spelling of something that exists: `kind X : contract` is `unknown-kind`, `contract NAME : KIND` is
   `expected-end-of-line` (probe in section 8). Layer 2 adds one sort and one clause of grammar before it adds a
   projection. `as with` is the smaller of the two grammar changes and is not needed (section 7).
8. **"The fold projects" is a lowering step** (section 8). The set of owners and the entity each end names are known when
   a contract is lowered, and nothing the fold knows adds to them.
9. **The brief's line target (about -1,200 for layer 1) cannot be met.** What is deletable is `lower/also.rs` (349 lines
   of code), the contract's `shares` (about 90), `Match` and `Contract.matching` (about 12) and the types that carry
   them (`Also`, `AlsoOn`, `Implied`: about 50): about 500. `Derive` (law IR, compiler, an engine host, the dispatch)
   adds about as much. `laws/budget.rs` (350) and the engine's budget (210 with `budget.rs`) stay (point 4). Section 11
   says what the lane expects to land at.

**The orchestrator's rule for this lane is "if an example's output changes for a reason that is not the purpose
inference, stop and report instead of editing the book".** Point 1 is exactly that: making `also` derive changes
`05-family` (three `also` lines: the mortgage's escrow, two 401(k) matches), and `11-sam` (`also`, `share`,
`sales-tax`; not a golden's input). So the lane is built in two parts: everything that changes no example is on the lane
branch; the step that turns the four sugars on, with the goldens it moves, is a separate pair of commits on a sibling
branch, and the report says so (section 6).

## 1. The five mechanisms: what each says, what it is lowered to, what the engine does per flow

| | written as | lowered to (today's types) | the engine, per flow | lines (non-test) |
|---|---|---|---|---:|
| `also` | `also ITEM \| FLOW [when E]` under a contract, entity, kind or purpose (`syntax/contract.rs:242`, `ast::Also`) | `lower_alsos` (`lower/also.rs`) makes one `Also { on: AlsoOn, what: Implied, when, law, purpose, description, codes, select, detail, waive }` per line in `book.also`, an auxiliary `Law` (no steps) that holds its expressions, and, on a contract, the ids in `Terms.also` | **nothing.** `rules.rs:86` and `laws::register` read `book.also` only to leave the auxiliary law out of the tables; no engine file names `Also`, `Implied` or `Terms.also` | 349 + 40 (`declare_alsos`) |
| `share` | `share 20% for studio` on a contract (also written on a party, purpose, asset in the grammar) | `contracts.rs::shares` (`read_share_line`, `measure`, the 100% total check) fills `Terms.shares: Box<[Share]>`; `Share { rate, entity, measure, loc }` is also what `owner A 60%, B 40%` makes (`Place.shares`, `Asset.owned_by`), which the engine reads (`owners.rs`) | **nothing for a contract's `Terms.shares`.** (A place's owner shares are the plan's ownership composition, a different thing with the same type.) `Purpose.shares` is set to empty and never filled | about 90 |
| `match` | `match 50% of [retirement] up to 6%` | `Match { rate, into, up_to }` on `Contract.matching`, set to `None` in `empty_contract` and nowhere else; the grammar has no `match` keyword left (the employer's match is an `also`: `05-family/contracts.ax:50`) | nothing | 12 |
| `sales-tax` | `sales-tax 8.625%` on a party kind | a fact, `builtin::SALES_TAX` (`props.rs:138`), set on the kind | nothing (`model/tests.rs:267` reads it back) | 1 |
| `budget` | `budget food 900 USD monthly [carries] [funded from A into B]`, `#food now budget …` | `laws/budget.rs`: a `Budget { purpose, starts, terms: Timeline<BudgetTerms>, law }` and a `Law` that warns `BudgetTotal <= BudgetLimit` | **a law.** Fired as any purpose law (`fire_purpose`); `Func::BudgetTotal`/`BudgetLimit` (`eval.rs:1246-1390`, with `budget_history` and `engine/budget.rs`) read the terms timeline | 350 + 210 |

Per flow, then: the three engine facts about a flow are that it posts (`post.rs`), that laws watching it fire in a fixed
order (`on out` at the source, relief, `on in` at the target, the purpose's laws, `on spend`, `always` at both ends), and
that a *promised* occurrence is made first by `instantiate_occurrence` (`occurrence.rs`) and posted flow by flow
(`promising.rs::post_made`, with `RuntimeFlow`s kept in `Recorded::promised_flows`). `also` and `share` appear in none of
these. Where each differs from "a law that derives":

- `also` is the nearest: it has a trigger (every flow of its owner), a condition (`when`), and what it says is a flow
  or an item. What it lacks is an owner a law can be fired for (point 2) and an effect (point 6).
- `share` and `sales-tax` are *allocations*: the studio's 27.00 of a 45.00 bill is "a flow `#phone` owned by `studio`,
  and Sam's own phone is 18.00" (LANGUAGE §10); the bill as paid is one flow in `register`. In the data model that is a
  carved item whose flow has another **owner** (`Flow.owner`: "who bears it ... a share makes it another") and the
  header's purpose. The sales tax is a carved item `amount × r/(1+r)` with purpose `#sales-tax`. Neither moves money.
- `match` is an `also`.
- `budget` is not "a law that derives"; it is a law that checks. Its desugaring in the brief,
  `on flow warn total(month, carried) <= 900 USD`, is what it already is, minus the two things a plain law cannot say.

## 2. Purpose inference

### 2.1 What each function ranks

`lower/infer.rs`, 121 lines, called by `make_resolved_flow` (every journal flow and every flow of a contract's terms,
`flow.rs:382`) and by the two places a contract's header and its legs are made (`contracts.rs:766`, `:794`).

| function | what it does | what it ranks |
|---|---|---|
| `endpoint_purpose(end, side)` | what **one end** says the flow is for | the party's *own* `#purpose` (`Provenance::Entity`) over its kind's `pays` (source side only) over its kind's `purpose` (`Party`); a commodity's issuer, source side only: its kind's `pays` (`Commodity`) |
| `taken_purpose(destination, source)` | an account kind's `takes B from A`, applied to what the ends said | nothing: it **rewrites** the inferred purpose (`Account`) when the destination is an account whose kind takes it. A flow with a *written* purpose is compared with the rewritten one |
| `infer_for_flow(from, to, written)` | what the flow is for, or `purpose-disagreement` | nothing between ends: if both ends say something and the two are not **related** (one covers the other in the purpose tree, and any explicit `of` object is the same), it is an error; the same test between the written purpose and the inferred one. Result: the written one, else the source end's, else the target's, taken |

`Provenance` (`journal.rs`: `Written, Contract, Entity, Party, Commodity, Account, Derived`) is already LANGUAGE §2's list
in order; nothing compares by it.

### 2.2 Where they disagree on the examples (counted on the baseline, `docs/v5/measure/purposes.py`)

A disagreement is an error, and the flow is **not made** (`make_resolved_flow`: `infer_for_flow(...).ok()?`; the model
tests assert `book.flows.is_empty()`). So the count is the count of flows a book loses.

| example (`check --today 2026-04-16`) | disagreements | the two sources |
|---|---:|---|
| `04-freelancer` | 173 | party kind against written: 77 `software` (`#software`) against `#business-software`, 31 `utility` against `#home-office-cost`, 16 `landlord` against `#home-office-cost`, 16 `insurer` against `#health-premium`, 16 against `#home-office-cost`, 15 `phone-company` against `#business-phone`, 2 more |
| `05-family` | 128 | party against written 51 (`edd`'s `#payroll-tax` against `#state-disability`); account kind against written 51 (`401k` takes `#pre-tax-deferral`, the line says `#contribution`); party kind against written 25 (24 `health-insurer` `#insurance` against `#household-pre-tax`, 1 `employer` `#wages` against `#contribution`); party kind against party kind 1 (`employer` `pays wages` against `health-insurer` `purpose insurance`, on `blue-shield 212.50 USD #pretax-benefit`, which also says a purpose) |
| `07-landlord` | 3 | party kind against written 2 (`utility` against `#rental-utilities`); party against party kind 1 (`irs` `#federal-tax` against `employer` `#wages`) |
| `02-household`, `06-investor` | 1 each | party against party kind (the same `irs` against `employer`) |
| `03-violations` | 1 | party kind against written |
| `11-sam` | 1 | party kind against party kind (`employer` against `health-insurer`) |
| `01`, `08`, `09`, `10` | 0 | |
| **total** | **308** | party kind against written 201, party against written 51, account kind against written 51, party against party kind 3, party kind against party kind 2 |

Of the 308, **306 are a conflict between a more specific source and a less specific one**, which LANGUAGE §2's "first
match wins" settles and the code does not. The 2 left are two party kinds at the same rank. On both of them the
flow also carries a written purpose in `05-family` (so it is settled by rank too); `11-sam`'s is checked when the code is
written.

### 2.3 What the one ranking is

`classify` takes every source a flow has, each with its `Provenance` as its rank, and answers with the best:

1. the written purpose (a flow's own line, or its contract's, `Written` over `Contract`);
2. each end's own purpose, ranked `Entity > Party > Commodity` as `endpoint_purpose` does, source end before target when
   they agree (as `from_purpose.or(to_purpose)` does today, so a pair that agreed keeps its answer);
3. an account kind's `takes`, which **rewrites an inferred purpose** and is never applied to a written one.

The highest rank wins. A source ranked below the winner never contradicts it. **Two sources of the same rank that name
unrelated purposes are still `purpose-disagreement`** (two parties that each say what the flow is for; the same
diagnostic, built by the same function), because rank cannot choose between them. This is the whole behaviour change.
It is one `max` over a small array and one `related` test, where the code today has three functions and two error
checks; `taken_purpose` becomes a step of the ranking, and `PurposeEvidence` is the candidate.

What changes, on a book that has a disagreement today: **the flow is made**, with the winner's purpose, where it was
dropped with an error. No other flow changes: a flow whose sources agreed, or that had one, gets what it got (the
tie-break above is today's). The 306 flows come back to 04, 05, 07, 02, 06, 03 (and 11-sam's), and with them the
balances, tallies, claims and limits that those flows feed. Section 11 lists the files; the report counts them.

**The four model tests that pin the error** (`model/tests/native_records.rs:210, 313, 396, 421`) assert that a written
purpose cannot be overridden by a commodity kind, an entity, a party kind or an account kind, and that the book has no
flows. They are the behaviour the brief changes; they are rewritten to assert what the ranking says, one for each
rank, and a tie is asserted for the error that remains. The commit says so.

## 3. The nine tables of `rules.rs`, and what keys them

`Rules` (`law.rs:581`) is built once (`rules::govern`, after every place exists). Eight tables are `Groups<Key, Rule>`
(compressed rows: a dense id is the row); the ninth is a list.

| table | keyed by | trigger | what fills it (`rules.rs`) | read by |
|---|---|---|---|---|
| `on_in` | place | `In` | `watching_place`: a place's own and ancestors' laws, its kind chain's, its asset's, the residents' (dated by residence) and the project's | `post.rs:99`; `headroom.rs:24`; `why/place.rs` |
| `on_out` | place | `Out` | the same | `post.rs:83`, `:815`; `headroom.rs:24` |
| `on_gain` | place | `Gain` | the same | `post.rs:407`, `:849` |
| `always` | place | `Always` | the same | `post.rs:102-104` |
| `on_spend` | entity | `Spend` | `spending`: a restricted entity's kind chain's laws, then its own | `post.rs:963`, `fire.rs:106` (`permits_spend`), `why/entity.rs` |
| `purposes` | purpose | `Flow` | `purpose_flows`: a purpose's laws for every descendant purpose (placeholder subject: the flow's owner) | `post.rs:119` |
| `about` | place | `Flow` | `asset_flows`: an asset's kind chain's and its own, keyed by the asset's place | `post.rs:122`, `totals.rs:297` |
| `contracts` | contract | `Flow` | `contract_flows`: a contract's own, never by its party | **nothing fires it** (point 2) |
| `timed` | none (a list; `rule` indexes into it) | `Each`, `By` | `timed`: once per subject a dated law governs | `timeline.rs`, `fire.rs:119, 147`, `plan.rs:155`, `closings.rs`, `why/*` |

Five of the eight keyed tables are keyed by a place, so four of them are the same `Groups<Place, Rule>` filled by one
walk and cut by trigger (`rules.rs:96`). The tables differ in two things only: the **key** an occasion is looked up by
(place, entity, purpose, contract) and the **trigger** that fires it. That is a typed pair, so the one index is:

```rust
/// Where a rule is looked up: the kind of occasion and the thing it happens at. A trigger and its key cannot disagree.
pub enum Watch {
    In(Id<Place>), Out(Id<Place>), Gain(Id<Place>), Always(Id<Place>), About(Id<Place>),
    Spend(Id<Entity>), Purpose(Id<Purpose>), Contract(Id<Contract>), Party(Id<Entity>), Timed,
}
```

stored as **one** `Groups<u32, Rule>` whose rows are the eight key spaces laid end to end (a base offset for each, six
bases, dense) and one row for `Timed`. `Rules::at(Watch) -> &[Rule]` is the only read; `Rules::all()` is the whole run
of rules; `repeats` (`plan.rs:425`) and the two watch builders become one loop over rows. `Party` is the one row that is
new: the laws of an entity's kind chain and its own that fire for the flows of a contract with that party (section 5).

## 4. What `Trigger` carries, and what `Derive` needs of the law compiler

`Trigger` (`law.rs:176`) carries nothing for six of its eight variants (`In, Out, Gain, Spend, Flow, Always`); `Each`
carries a period and an optional closing day, `By` a node (the date expression). Only the six are looked up by an
occasion; `Each` and `By` are timed. So the dispatch key is `(Trigger, key)` for the six and nothing for the two, as
section 3 builds it. The compiler's `When` (`laws/vars.rs`) is the same list plus `Deadline` and `Template`, and decides which
variables a law may read (`Var::provided_by`): `amount from to payee flow purpose description` for every flow trigger
(`Payee` not for `Gain`), `date year month self owner` for all.

What `Derive` needs of the typing:

- **`amount` and units.** `Var::Amount` is typed `flow_amount_ty()` (`compile.rs:860`): `Amount(Dim::Of(u))` only when the
  governing place holds exactly one commodity or the owner is an asset; for every contract, kind, entity or purpose law it is
  `Amount(Dim::Any)`, "an amount of some commodity". A derived `7.65% of amount` is then `Dim::Any` and the derived flow's
  unit is the trigger's at run time (`Value::Amount` carries it); a literal says its own (`705 USD`), and a bare number of
  `also` is read in the owner's currency (`also.rs:pending_amount` falls back to `currency(owner)`). The check `Derive`
  adds is that its amount is an `Ty::Amount(_)` (not a number, not `empty`), by the `expression(root, Ty::AMOUNT)` call
  `owe`, `count` and `consume` already make. This is why `warn amount <= 500 USD` is a type error in a contract's law
  (`contract-law-never-fires.ax`, first draft): the mixed units of `Dim::Any` against USD, and it is the same rule here.
- **`self`.** A contract's law has `self: flow` (`Placement { subject: Ty::Flow }`, `laws/mod.rs`); a purpose's has the flow's
  owner; a place's has the place. In a derived line `self` as an **end** (`-> self`, `issuer -> self`) is not an expression
  but "the implying flow's own end" (`also.rs:implied_end`): it is an `Option<Id<Place>>` of `None` in the template, and
  stays so.
- **The ends and the selectors of the source end** (`checking[^invoice] -> self 2 USD`) are resolved names (`world.end`), a
  `Run<Select>` on the template, as `lower_selectors` makes them.
- **`when E`** is the law's `when` step, compiled by the one `condition` call; `unless E` is free.
- **What the effect holds** is small: `Effect::Derive { template: Id<LegTemplate>, amount: NodeId }`, with the template (a
  flow of its own, or a `+`/`-`/carved item; ends; purpose; description; codes; selectors; the `since`/`due`/`for` of its
  `Detail`; a waiver) in an arena of the book, so that `Effect` does not grow (a size test says so). This is `also.rs`'s `Also` minus the
  law that held its expressions: the expressions are the law's own nodes.

## 5. `Derive`: where it is read, and what it can be

An effect is read by the thing that fires the law. Two hosts have what a derived flow needs, and they differ in whether
a group is being solved:

**The occurrence** (`instantiate_occurrence`, `materialize`). The contract's own laws and the laws of its party (its
kind chain and itself) fire **once per occurrence**, after its groups are made: `amount` is the first group's header, the
selectors of the expressions (`[alex-401k]`) read the occurrence's flows (`Context::with_template_flows`, which exists
for this and is used by one test), and what is derived joins the occurrence:

- a *carved item* (`share`, `sales-tax`, `also ITEM`) joins its group's items **before `solve`**: `items_at` gains a
  `Bear` and an `ItemAt` for it, its amount is a `Cut::Share` or an `Expr::Computed` of the law's own node, and the solver
  carves it from the header exactly as it carves a written item (the same `solve(Some(remaining), &draws, &bears,
  Remainder::BeforeItems, &mut env)`); `Says` gains the owner a share re-attributes to;
- a *flow of its own* (`also -> escrow 705 USD`, a match) is a header-only group made after the others, its amount read by
  `Quantity::resolve` against the same `Reads` environment, its ordinal after the occurrence's last.

Both are pushed into the same `Pools` as any flow of the occurrence, so **a forecast (which is `promising.rs`: the same
`post_occurrence`) sees them with no second path**, a kept occurrence's flows include them, and `Recorded::promised_flows`,
`register`, `why` and the oracles of K5b and K5c read them as they read the rest. The amounts are read by
`Quantity::resolve` and `solve`, the one algebra of a group; nothing else evaluates a derived amount.

**A posted flow** (`Ledger::post`, after the laws that fire on it). A law of a place, purpose, asset or entity fires on a
flow that has moved, so what it derives can only be a flow of its own, or a `+`/`-` item (the same ends, or the reversed).
This host needs a `RuntimeTxn` for a derived flow, a record for the views, and a guard against a derived flow firing the
law that made it. It is **not built in this lane** (section 6 says why that is a choice and what it costs): a kind's or a
purpose's `also` (a card's cash back, a processor's fee) stays as inert as it is today, and the model says so with a
note where it is written, not silently.

What `Derive` cannot be, said once: **it is not an allocation after the fact.** A `share` or a `sales-tax` that fires on a
posted flow cannot reduce what the flow already put in its owner's tally. They are derivable only in the occurrence
host, which is where the corpus uses them (`11-sam`'s `share` is on contracts; its `sales-tax` is on a party kind whose
contract's party is of that kind).

## 6. What turning the sugars on does to the examples, and the decision

`also` on a contract becomes a law owned by that contract, `share` a law with a carved item, `sales-tax` a law of a party kind,
`match` is deleted (nothing sets it), and the occurrence host fires them. The books that write them:

| book | what it writes | what changes if it derives |
|---|---|---|
| `05-family` (golden inputs `05-family-*`) | `also -> escrow 705 USD #contribution` on the mortgage; two 401(k) matches, `also acme -> alex-401k 40% of ([alex-401k] up to 10% of amount)`, `also bluefin -> jordan-401k 50% of (… 6% …)` | the escrow gains 705.00 USD a month and `joint-checking` pays it; the two 401(k)s gain the employer's match; `balance`, `available`, `limits`, `claims`, `tax`, the forecast |
| `11-sam` (not a golden's input) | `also lumen -> retirement 50% of ([retirement] up to 6% of amount) #match`, `also -> escrow 410 USD #escrow`, `share 120 SQFT for studio`, `share 60% for studio`, a party kind's `sales-tax 8.625%` | the same, and the studio's shares and the tax |
| `v4-sketch`, `explore-v5/*` | the same lines | not goldens |
| `docs/v5/measure/diff/cases/g-also-*` | `also + 5%` on an entity, an account, a kind, a purpose | none (the post host is not built: section 5) |

The orchestrator's rule says to stop and report when an example moves for a reason other than the purpose inference. It
does not say what to do when the change is the point of the lane, so **this lane does the safe half and keeps the other
half apart**: the lane branch has everything that moves no example (the ranking, which moves examples for the reason the
brief gives; the one index; `Derive` in the law IR, the compiler, the occurrence host and the dispatch of a contract's
laws, exercised by laws written with `derive` and by the generator of section 11; the relator kinds), and the step that
routes `also`, `share`, `sales-tax` and `match` through `Derive`, deletes `lower/also.rs` and regenerates the goldens it moves, is **the
last two commits, on a sibling branch** (`claude/great-wozniak-pnqn7x-v5-k6-live`), which the report names. `git merge` of that
branch is the decision; nothing on the lane branch depends on it.

## 7. `with`, `for`, and what `as with` would change

Today a contract's two ends are its **party** (`with PARTY`, else the entity of the contract's own name:
`contract_party`) and its **owner** (the owner of the place its schedule is paid from or into: `contract_owner`). The
header of a template is `holding ↔ party's place` (`HeaderCx`: `from`/`to` by the schedule's `from`|`into`); a leg names
the end it goes to and takes from the header's source (`template_legs`: "the source end of the scheduled header is kept and that
portion is sent to the named endpoint"); an item is a flow along the header's own ends. A contract's `for` is a loan's
`for ASSET` (`contract_loan`) and a clause of a tail (`for PARTY`: `detail.hold`, the money tied to whom: `post.rs:450`);
neither names a leg's end.

So an end of a leg is one of three things today: the **holding** (the owner's side, written once on the schedule line),
the **party's** outside place (`with`), or a **named place or entity** on the leg line. A relator's role is none of these: it
is a slot of the contract's kind, filled by a word (`employee alex`, `employer acme`), and an end written as a role means
"whoever fills it".

`as with` (DESIGN §2.3, `has employer agent as with`) says **which slot the contract's `with` fills**, so that `with acme` and
`employer acme` are one word. Without it a relator kind needs the word twice (`with acme`, and a line `employer acme`), and
a check that they are the same entity. That is one clause of grammar on `has` (`syntax/decl.rs:88`) and one field of `Slot`
(`slots.rs`), and it is lane L's (K3b §7). This lane does not build it: Layer 2 writes both words and checks they agree, and the
map of section 9 says what `as with` removes.

## 8. Projection: a lowering step, decided from the code

*Question: does the fold project a relator's legs onto a book, or does the model specialise them to it?*

What projection needs: **the set of the book's owners**, **the entity each role is filled by**, and **the place an end
resolves to**. An end that is a book's owner is that owner's position (the schedule's holding, when the holding is the
owner's); any other end is an entity's outside place; a leg with both ends outside touches nothing.

- The owners are known at declaration: `Entities.holds` (`declare/parties.rs:229`) is true for `me` and every entity named as
  an owner (`owner X`, a share, an address filler). `Role::Holding(entity)` marks the place each owner has. Nothing the journal or the
  fold does adds an owner.
- The fillers are known when the contract is lowered: a slot is a fact (`core::facts`) of the contract's name, filled by a
  property line, checked once by `fill.rs`.
- The places are known: `entities[e].place` is the outside place of a party and the holding of an owner; the schedule line names
  the owner's account.
- The fold has none of this in a form it can use. It sees places and `Rule`s, and a `Flow` with two outside ends is a flow
  like any other (it moves value between two outside balances); dropping it would be a filter per flow, per occurrence, per
  forecast, on data that cannot change.
- `Terms` is stored once (K5b) and the occurrence **clones the template per occurrence**: a leg that projection would drop
  and the fold drops would be cloned and solved for each occurrence first.

So projection is the model's: when a contract of a relator kind is lowered, the kind's legs are **specialised to the
book**, the legs with no owner at either end are dropped, and what is left is an ordinary `Terms.template` and ordinary
derive laws. The fold, the monitor, the forecast and every report see a contract like any other. **One kind, two books** is
two different specialisations of the same legs by two different sets of owners: no new fold, no new state.

What needs more than that and is not built: an owner end whose position is not the schedule's holding (a plan's account,
the escrow of a mortgage): that is `part` and `joins` (Layer 3). Layer 2 says `relator-position` (a diagnostic) when a leg
has an owner end that the schedule does not name.

## 9. Layer 2: the spelling, with what exists

The brief's spelling cannot be written today (point 7), and its arrow-led legs are lane L's. This is the spelling Layer 2
builds, in today's grammar:

```text
kind employment : contract                      // a kind of the new sort `contract`
  has employee person                           // slots: what the existing `has` says
  has employer employer
  also employer -> irs 7.65% of amount #payroll-tax        // the kind's `also`: roles as ends, once

contract alex-pay : employment with acme        // new: `: KIND` after the name
  employee alex                                 // new: a line that fills a slot (the existing property-line form)
  employer acme                                 // `as with` makes this line (and its check) unnecessary
  5_750 USD twice monthly on 15, last into joint-checking #wages
  irs 692 USD #federal-tax
  …
```

The grammar this adds: **`kind NAME : contract`** (a sort and its root: `kind_roots.contract`), **`contract NAME : KIND [with
PARTY]`**, and in a contract a **slot line** (`NAME VALUE`, told from a leg by having no amount), where K12 already
reads a slot line on a place or an entity. The kind's body takes the lines a contract's body takes, with an end that is a
role word. The projection of section 8 happens in `lower_contract`: each leg and item of the kind is specialised to the
contract's fillers and the book's owners, and the result is **added to the contract's template and to its derive laws**, so that
`also employer -> irs …` is a derive law of the contract in the employer's book (both ends owners' or an outside
place: one of them the holding) and nothing in the household's (both ends outside). Acceptance (the brief's): a copy of
`examples/05-family`'s paycheck and of `07-landlord`'s lease and management in this spelling, in `examples/explore-v5/`, that
`check`s to the same balances, tallies and claims as the originals in the household's book and, in a second book that makes
`acme` an owner, the legs of the employer's own half; `docs/v5/measure/relators.py` is the oracle (section 11).

What the kind **cannot** carry in this lane, because it is not a relator's leg: per-contract withholding amounts (they
differ per paycheck: `irs 692 USD`), a plan's match (`40% up to 10%` is the *plan's*, and differs per employer: `joins`,
Layer 3). The copies keep those lines on the contract and move what is the same on every paycheck of the kind.

## 10. Layer 3: stop, and what a run would need

`on start` / `on end`, `part` and `joins` (ASSOCIATIONS §5.1-5.2) are not built. What a run would need, so that the next brief can
be written:

- **A relator's span as an event.** `on end` needs the fold to know when a contract ends: today `Contract.days` (cut by `ends`)
  and the monitor's `Residual`; an `on start`/`on end` is a `Trigger` with a contract as its key (`Watch::Contract`) fired
  by the same heap the monitor and `Promising` keep, on the first and last due day of the span. The derived flow is posted by
  the occurrence host's path with no occurrence to carry it: it needs `RuntimeTxn` for a derived flow and a `Recorded` list
  (the post host of section 5), so **the post host is Layer 3's prerequisite**.
- **`part`.** A position that exists with the relator needs a place made per instance at lowering (`World::tab` is the model of
  it: a place made by the first claim that asks), its owner and `with` inherited from the whole by slot name, and a name that
  addresses it (`K3b`'s `Addresses` already takes a path of fillers). The `relator-position` diagnostic of section 8 is the
  place where a leg's end names a part.
- **`joins`.** A membership is a thing with its own kind, slots, position and laws, made by a `joins` line of an employment: a
  second contract kind whose `with` is an employment, so K12's `has … some|many` and the same lowering. Whether it is a stored
  thing or a projection of (employment, plan) is the open question of ASSOCIATIONS §10 item 6, and it should be answered with a
  book written in the Layer 2 spelling, not before.

## 11. The plan, what proves each step, and where the lines go

| step | what | proof |
|---|---|---|
| 0 | this map, `docs/v5/measure/purposes.py`, the three probes | |
| 1 | `classify` (`lower/infer.rs`): one ranking, the tie as the one error; the four tests rewritten; the goldens that move | `purposes.py` before and after (308 to the ties); each moved golden with its count of flows; `fuzz.py`, K0a's harness, `splits.py`: the differences are the dropped flows and nothing else |
| 2 | one dispatch index: `Rules` as `Groups<u32, Rule>` with `Watch`; every reader moves; `Contract` and `Party` rows exist and nothing fires them yet | byte-identical goldens, mistakes and tests; `check` on `bench/` 100k and 1m, three runs, before and after |
| 3 | `Effect::Derive` in the law IR, the written `derive` step, its typing, the occurrence host (a contract's and its party's rules fire per occurrence, items before `solve`, flows after) | `docs/v5/measure/derives.py`: a seeded generator of contracts, each *sugar form beside its hand-written law*, equal on `check`, `flow`, `balance`, `tax`; mutation-tested |
| 4 | the sugars through `Derive`, `lower/also.rs`, `Share`, `Match` deleted, goldens regenerated | **the sibling branch** (section 6) |
| 5 | Layer 2: `kind : contract`, `contract : KIND`, slot lines in contracts, specialisation in `lower_contract` | `relators.py`, the explore-v5 copies, both books |

**Where the lines go** (non-test, `briefs/loc.py`; baseline 52,827: cli 2,507, core 3,476, engine 11,448, model 18,542, report
6,968, sync 4,277, syntax 5,595, systems 14). Step 1 deletes about 40 (`taken_purpose` and a check) and adds about 40. Step 2 deletes
about 60 (eight constructors, `per_place`, two builders) and adds a `Watch` and one build. Step 3 adds the effect, a typed
step, the host and the dispatch of a contract's rules: about +300. Step 4 deletes `also.rs` (349), `declare_alsos` (40), `shares` and
`read_share_line` (90), `Match`/`matching` (12), `Also`/`AlsoOn`/`Implied` (50) and adds the lowering of an `also` into a law
(about 120): about -420. Layer 2 adds, and removes lines of the examples. **The lane lands near +100 to +300 for layer 1,
against a target of -1,200**, and the report says why: the sugars were not copies of anything (point 1), and `budget` is
a law already (point 4).

## 12. How this map was checked

- **Sections 0.1 and 1, the sugars:** `grep` of every name (`Also`, `Implied`, `AlsoOn`, `.also`, `Terms.shares`, `Match`,
  `matching`, `SALES_TAX`, every `Derivation::` variant) over `crates/`, tests aside; the engine has no reference to any
  of them. The three probes of `docs/v5/measure/k6/probes/` on the baseline binary (`8e33a9d`, built in the worktree).
- **0.2, 3:** `grep` of `rules.` over `crates/` and a probe: a contract's `on flow` law that warns above 500 USD, three occurrences of
  1,000 USD: no warning.
- **2:** `infer.rs` read in full; `purposes.py` over the examples on the baseline (308); the four model tests read; LANGUAGE
  §2 lines 224-238.
- **0.4:** `laws/budget.rs`, `eval.rs:1246-1390`, `engine/budget.rs`, `laws/mod.rs:253`.
- **0.7, 8:** a book with `kind employment : contract` and `contract pay : employment with acme` through the baseline: `unknown-kind` and
  `expected-end-of-line`; `Entities.holds`, `Role`, `contracts.rs:128-160, 727-800`, `occurrence.rs:push_item`, `legs_at`.
- **Lines:** `briefs/loc.py` and `hist.py` on the tree at `8e33a9d`; per-file counts by the same rule.
- **The baseline:** `cargo test --workspace --release --no-fail-fast` at `8e33a9d`, before any change, is the number the report compares with.
