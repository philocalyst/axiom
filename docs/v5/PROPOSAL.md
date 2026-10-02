# Axiom v5: seven kernels in place of fifty special cases

A review of `cutover/promote-workspace` at `2c94f28`, read by hand, and a plan to cut its 49,428 lines (58,493 as
written; see §1a) nearly in half by building the cores the theory already names. Paths such as
`crates/model/src/book.rs` are relative to that branch's root, where the workspace no longer lives under `v2/`. The
plan also improves the Rust, and changes the language so that a line says what kind of event it is.

## 0. The short version

- **The branch has the right ideas and the wrong shape.** v4's concepts are sound: agents, positions, parcels,
  promises, laws as norms, behaviours over time. But none of the small cores that `briefs/theory.md` proposed was
  built. In their place, each feature was lowered by its own long procedural path, and the paths copy each other:
  - split resolution is implemented three times;
  - "which occurrence is due" is decided in four places;
  - six mechanisms hold values that change on a day;
  - turning a written literal into an amount is implemented six times.
- **The core of v4 is missing.** Nothing monitors promises:
  - `open_claims` is always empty and `monitor_complete` is always `false`;
  - claim write-offs are a no-op (`Fact::ClaimChange(_) => {}`);
  - paying a claim does not settle it.

  Three of the four failing tests trace to that, or to the second fold driver the forecast had to build
  because of it.
- **The Rust got measurably weaker:**

  | | main `12d5d18` | cutover |
  |---|---|---|
  | mean function length | 11 lines | 21 lines |
  | functions over 80 lines | 3 (2% of function lines) | 94 (30% of function lines) |
  | longest function | 101 lines | 730 lines, 21 parameters |
  | borrowed lifetimes per 1,000 lines | 50 | 38 |

  Twenty-five hand-written rollbacks, option-soup structs and sentinel values replace types.
- **The fix is seven kernels**, one per theory, each small, typed and fast:
  - a taxonomy;
  - behaviours;
  - positions and parcels;
  - one event IR, solved at compile time when it can be;
  - promises as CSL terms reduced by residuation;
  - one norm IR that `also`, `share`, `budget` and `sales-tax` desugar into;
  - facts that views query.

  Projected size, keeping every v4 feature: **about 27,000 lines (−45%) by porting**, and about 24,500 as a
  clean-room floor. §7 lists the levers that would take it to about 20,000, and what each costs. All counts are
  measured under one formatter (§1a); the first version of this proposal counted the tree as written, in two
  styles, and said 58,493 → 31,700.
- **The language change is that the junction becomes the verb.** Every journal line is already `DATE SUBJECT
  VERB …`, except flows, whose verb column is always `->`. In v5:
  - the subject of a money line is always the book's own side;
  - `->` gives and `<-` takes, so income no longer puts a party in the subject column;
  - `@` marks an exchange;
  - legs lead with their own arrow, so a paystub is one transaction that reads left to right.

  Positions belong to the agent that holds them, so the `account` keyword goes. A debt is a promise, not an
  account plus a contract. Direction, not a purpose's root, decides income versus spending. No party has to be
  invented to stand for a category.

## 1. What I read, and how I measured

I read every crate by hand, in pipeline order: core, syntax, model, engine, report, sync, cli, and the embedded
systems. I compared against our last main (`12d5d18`, `v2/crates`). Line counts use `briefs/loc.py` (non-test,
non-comment). The numbers below come from three scripts in [`measure/`](measure/) (`quality.py`, `fnlen.py`
and `hist.py`) and from `grep` counts given inline. Each script takes the two crate directories to compare. Every
claim names its file and line in the cutover tree. The full reading notes, file by file, are in
[`measure/reading-notes.md`](measure/reading-notes.md).

The branch's own stopping point records: 734 tests pass and 4 fail, `cargo fmt --check` differs in 117 files,
the family example reports 141 errors and the landlord example 44.

### 1a. One formatter before any count

The cutover had no `rustfmt.toml` and was written in two styles. `core` and much of `engine` were written by hand
at width 120 with small heuristics off, as main was. The rest was written at rustfmt's default width 100. The same
tree measures:

| formatted as | `loc.py` total |
|---|---:|
| written (two styles) | 58,493 |
| rustfmt defaults | 62,793 |
| width 100, `use_small_heuristics = "Max"` | 55,208 |
| **width 120, `use_small_heuristics = "Max"`** (main's and core's style) | **49,428** |

Formatting alone moves the count by 13,000 lines, so no simplification may be measured across a format change. The
v5 branch starts with `rustfmt.toml` at main's style, applied in a commit of its own (`fc01009`, no token changed).
Every number in this proposal is measured against that commit, with main reformatted the same way (which changes
main by 17 lines). Tables below still name the cutover's line numbers at `2c94f28` where they cite code.

## 2. Where the lines go

The per-concept figures below were counted on the tree as written; under the fixed formatter each is about 15%
smaller.

| concept | where it lives now | lines (approx.) | how many implementations |
|---|---|---:|---|
| Promises: contracts, claims, loans, deposits, occurrences | `book.rs:546-1320`, `lower/contracts.rs`, `record.rs` (`lower_occurrence`, `lower_loan_origin`, `lower_owes`), `ledger.rs` (`instantiate_occurrence`, `materialize_group`), `report/forecast.rs`, `sync/promise.rs` | ~7,000 | 4 places decide which occurrence is due; there is no monitor |
| Lowering one journal line | `lower/record.rs` (4,631), `lower.rs` (733) | ~5,400 | 9 per-verb copies of one skeleton |
| Values that change on a day | `props.rs`, `params.rs`, `values.rs`, `prices.rs`, `Residence`, `Timeline<Terms>`, `Timeline<BudgetTerms>`, `engine/temporal.rs`, `plan.rs:343` | ~3,700 | 6 |
| Declarations and name trees | `declare.rs` (1,789), `kinds.rs`, `purposes.rs`, `paths.rs` | ~2,400 | 3 tree builders, 30 passes over every item |
| Laws, `also`, budgets | `laws/*`, `law.rs`, `lower/also.rs` | ~3,800 | `also` and budgets have their own lowering |
| Posting, parcels, assets | `post.rs`, `lots.rs`, `assets*.rs`, `fire.rs` (`carry`, `consume`) | ~3,800 | asset parts are a second store beside parcels |
| Law evaluation, totals | `eval.rs`, `totals.rs`, `calc.rs`, `facts.rs` | ~2,900 | 4 total-recording paths |
| Views | 40 files in `report` | ~9,000 | each a bespoke fold; 5 views re-run the ledger |
| Sync | `sync` + `model/sync_lower.rs` | ~6,500 | re-derives the book, dates and due promises |
| Diagnostics construction | everywhere | model has 310 sites | no catalog |

## 3. Six structural faults

### F1. The theory's cores were never built; special cases were

`theory.md` proposes four unifications: one `Promise` term, one `Timeline<T>`, units as `Dim`, and defeasible norms.
Only `Dim` landed in full.

- **Promises.** `Contract` (`book.rs:546`) is a bag of optional features: `terms`, `standing`, `buys`, `deposit`,
  `deposit_holding`, `loan`, `matching` and `ended`. Each has its own code path.
  - Loans need a hand-written transaction kind, `TxnKind::LoanOrigin` (`record.rs:1700`, 255 lines).
  - Deposits, standing orders and `due … else` are separate fields.
  - `ForecastError::UnsupportedFeature` and the whole `ForecastFeature` enum (`Deadline`, `Shares`, `Also`, `Buy`,
    `Deposit`, `Matching`, `GroupedTemplate`; `book.rs:784`) are never constructed. They are residue of an earlier
    forecast that could not run what the engine runs.
- **Timelines.** `Timeline<T>` exists in core and is good, but it holds only contract terms and budgets. Six other
  mechanisms hold values that change on a day:
  - built-in properties as **static** typed fields (`Kind`, `Place` and `Entity` in `book.rs`);
  - custom properties as `Prop { since }` rows read by a linear scan (`book.rs:436`);
  - params as `ParamRow { since }` read by binary search;
  - `Residence { days }`;
  - prices;
  - sampled temporal histories (`engine/temporal.rs`, plus `sample_temporal` called three times per posting at
    `post.rs:80,104,107`).

  The spec's "everything declared can change" holds for half of the properties.

### F2. Every concept is lowered by its own path

- **One skeleton, nine copies.** `record.rs` has `lower_txn`, `lower_opening`, `lower_occurrence`,
  `lower_loan_origin`, `lower_owes`, `lower_basis`, `lower_value`, `lower_measure` and `lower_filed`. Each repeats the
  same steps:
  1. collect roots;
  2. `compile_template`;
  3. roll back on failure;
  4. `lower_tail`;
  5. resolve ends;
  6. `make_resolved_flow` (14 parameters);
  7. `lower_items` (17 parameters);
  8. `JournalGroup`, then `JournalProgram`;
  9. push `Txn` (11 fields).

  `rollback(world, first, code_start, selector_start, detail_start, program_start)` appears **25 times**: a
  hand-made transaction over five arenas.
- **Split resolution three times.** Header, legs, `...`, carve, add, less and percent-of-header are resolved:
  - statically in `lower_txn`/`lower_items` (model);
  - for occurrences in `materialize_group` (`ledger.rs:541`, 730 lines);
  - for computed journal amounts in `post_journal` (`ledger.rs:1929`).
- **"Which occurrence is due" four times:**
  - `Contract::occurrences` plus `nearest_occurrence` with a radius of `months × 31` (`record.rs:1956`);
  - the occurrence ordinal in `post_written_occurrence`, which counts every due day from the contract's start,
    O(n²) over a contract's life (`ledger.rs:1820`);
  - `contract_forecasts`, which numbers ordinals from 0 inside the forecast window (`forecast.rs:484`), so an
    occurrence's identity differs between history and forecast;
  - `sync/promise.rs keep_paired`.
- **Smaller duplicates:**
  - literal-to-amount six times: `literal_amount`, the `resolve_literal` closure (which swallows diagnostics with
    `.ok()`), `resolve_amount` ×2 and inline ×2 in `laws/mod.rs`;
  - `lower_tail` twice, the contracts copy returning a 6-tuple and silently dropping seven clause kinds;
  - `resolve_object` twice;
  - the kind and purpose tree builders (`kinds.rs`, `purposes.rs`), each with its own `cycles`;
  - calendar arithmetic re-implemented in `book.rs:1155-1306` beside core's `Day::add` and `Window`;
  - syntax's `Folder::of`, heading and short-date logic re-implemented in `sync/write.rs:16-110`;
  - two JSON escapers;
  - the owner-share composition loop twice (`plan.rs:445`, `578`).
- **Thirty full passes.** `for site in sites { for item in &file.items { let ItemKind::X(id) = item.kind else
  continue` occurs 30 times. Main had `collect.rs`, one pass that sorted items into typed buckets. It was deleted.

### F3. Phases that do not trust each other

- **A survey that predicts lowering.** The place tree must freeze before lowering, so two full AST walkers predict
  which claim positions lowering will need: `lower::survey` (Mentions) and `visit_endpoints` (a 14-variant
  `EndpointContext`), with `declare.rs:1158-1225` registering tabs. When the prediction misses, `World::tab()`
  fails with `unregistered-tab`. That is about 500 lines whose only job is to guess.
- **The engine re-validates the model** (`ledger.rs:413-432`): `valid_offsets`, `group.template < len`,
  `legs.len() == leg_quantities.len()`. The model→engine occurrence protocol is a set of implicit indexes into
  source flows.
- **The model re-validates the parser.** "duplicate-waiver-description", "duplicate-end-description",
  "duplicate-claim-writeoff-description", "assertion-tail" and "until-position" cannot fire: `clauses()` rejects
  repeated clauses, and `takes(verb, clause)` rejects clauses a verb does not take.

### F4. Two fold drivers, and views that fold again

- `report/forecast.rs:425 contract_forecasts` is a second simulation loop. It merges habit flows with scheduled
  occurrences, advances a resumed ledger, calls `instantiate_occurrence` and applies the results.
  `projection.rs` then replays every flow into another ledger.
- `claims.rs:52 holdings_at` builds a fresh `Plan::new(book)` and folds the whole book for any `--at` date.
- `history.rs` replays postings to rebuild daily balances.
- `available` forks twice, and `context.rs` runs and resumes the ledger.

The engine has one fold; the product has five.

### F5. The monitor does not exist, and the failing tests are its symptoms

- `ledger.rs:1731` builds every `Run` with `open_claims: Box::default()` and `monitor_complete: false`.
- `ledger.rs:1767` handles a claim write-off with `Fact::ClaimChange(_) => {}`.
- `record.promises` only ever receives kept occurrences, with `waived: false`.
- Settlement by code, then exact amount, then oldest (LANGUAGE §7) is unimplemented.

What does work is quietly the right model. Claims are **lots in claim positions**, `finish()` reports
`explain::overdue` from lots with `qty > 0`, and the claims view reads those lots.

| failing test | cause | kernel that removes it |
|---|---|---|
| loan forecast emits five payments, not three | occurrences are scheduled whatever the state; nothing knows the debt is paid | K5: the residual of an annuity is `Done` at zero balance |
| forecast closing prefix lacks the 10 USD year-end tax | the forecast is a second driver that resumes the prefix differently | K5/K7: the forecast is the same fold continued past today |
| residence-only books never start their yearly schedule | the schedule starts from flows; behaviours are sampled, not integrated | K2: schedules start from the first step of any behaviour |
| prorata place realizes 0 basis | BasisZero funding special-cased in pricing | K3: one rule for arriving basis per position kind (a decision, see §10) |

### F6. The chart of accounts came back in disguise

- `std.ax` declares `kind receivable : asset claim` and `kind payable : debt claim`. DESIGN §4 says "There are no
  receivable or payable accounts".
- Purposes are rooted by P&L side, so one thing appears twice: `interest : spending` and `interest-income :
  income`; `insurance` and `claim-payout`. `rent : home : spending` makes rent *received* a refund of spending,
  unless a `pays` line intervenes.
- `05-family` invents eight parties that are categories (`restaurant`, `interest-source`, `household-store`,
  `kids-market`, `trip-vendor`, `gifts`, `rewards`, `payroll-office`), used by 68 flows. It also declares
  `commodity HOME : real-estate` beside `asset house : home`.
- A debt is three things at once: `account mortgage : mortgage at lender`, plus `contract mortgage-payment with
  lender` carrying `loan …`, plus a debt tab (`contract_endpoints`).
- Account, agent and contract namespaces collide. Sam's example had to rename `fidelity` to `fidelity-brokerage`
  and `phone` to `phone-bill` (`11-sam/accounts.ax`, `contracts.ax:33`).

## 4. The Rust

### Measured against main

| metric | main `12d5d18` | cutover | |
|---|---:|---:|---|
| non-test lines | 22,375 | 46,584* | ×2.1 |
| functions per 1,000 lines | 70.7 | 50.9 | |
| mean function length | 11.1 | 17.3 | |
| function lines in fns > 80 lines | 2% | 25% | 68 functions |
| function lines in fns > 160 lines | 0% | 9% | 13 functions, 3,467 lines |
| borrowed lifetimes per 1,000 lines | 49.8 | 45.0 | −10% |
| `.clone()` per 1,000 lines | 2.1 | 2.4 | |

\* `quality.py`'s count, which also excludes fixtures. `loc.py` says 49,428. Both trees are formatted alike (§1a).
The first version of this table compared main as formatted against the cutover as written and overstated every
gap: ×2.5, 30% and 15%, and −24% for lifetimes.

The longest functions are:
- `materialize_group` (600 lines, 21 parameters);
- `lower_occurrence` (495);
- `sync::plan` (281);
- `lower_txn` (271);
- `post_journal` (262);
- `lower_alsos` (238);
- `lower_owes` (228);
- `purposes::declare_sites` (210).

### Twelve patterns that make it weaker

1. **Monster functions with positional parameter lists:** `materialize_group` takes 21 parameters,
   `lower_items` 17, `make_flow` 14. No context struct carries what they share.
2. **Hand-written transactions.** The 25 `rollback(..)` calls should be one RAII guard, so that the borrow checker
   makes forgetting impossible.
3. **Option soup instead of enums:**
   - `Contract` (8 optional features);
   - `Detail` (8 `Option`s plus a `Detail::NONE` sentinel, merged field by field in `merge_detail_pool` and
     `ledger.rs:2491`);
   - `Place` (19 fields, constructed by struct literal three times at `declare.rs:1405-1497`);
   - sync's `Facts` (11 `Option`s);
   - a `Tail` with `valid: bool`.
4. **Sentinels instead of types:**
   - `Day::MIN` as a contract's storage anchor causes the overflow handling in `ledger.rs:563` and
     `calendar.rs:447-500`;
   - `TEMPLATE_TXN` needs a `JournalTxn` newtype plus `expect`s;
   - `RuntimeRange` re-implements core's `Run<T>`.
5. **Clones where a borrow belongs:** `lower_occurrence` clones the terms' `inputs` and `template`
   (`record.rs:1097-1098`), and `materialize_group` clones the template `Flow` for every occurrence. Both are
   immutable plan data the fold could borrow.
6. **Recomputation:**
   - O(n²) occurrence ordinals;
   - `Plan::new` for every `--at` view;
   - the forecast simulated twice;
   - `sample_temporal` three times per posting;
   - `node_doc` scanning every item to find one contract's doc (`contracts.rs:1311`).
7. **Validation across boundaries** that types would make unnecessary (F3).
8. **Dead code:**
   - `let _ = loc;` after computing `loc` (`record.rs:3669`);
   - both branches of an `if` identical (`record.rs:4474-4478`);
   - the unused `_survey` and `_party` parameters;
   - `monitor_complete: false`;
   - six unreachable diagnostics.
9. **No diagnostic catalog:** the model builds 310 diagnostics by hand. Many are near-duplicates, such as
   `CodeIndex::resolve` and `resolve_claim`, which differ only in wording (`record.rs:135-210`).
10. **Thirty passes over the items** where one bucketing pass belongs.
11. **Repeated boilerplate:**
    - `Word { text: x.0, loc: file.loc(x.0) }` appears 105 times in the model, where `file.word(x)` belongs;
    - eight `let x_pairs = x; let x = Groups::build(…); drop(x_pairs);` blocks (`rules.rs:136-159`).
12. **Linear scans where an index or an interval map belongs:**
    - `prop()` (`book.rs:436`);
    - `index_at` (`book.rs:1196`), which duplicates `Param::row`;
    - `put()`, which rebuilds a boxed slice on every property write (`props.rs:243`).

### What good looks like is already in the tree

- `engine/lots.rs`: a lazy `BinaryHeap<Ranked>` for HIFO that compares basis per unit by cross-multiplication,
  colour-ordered tie relief, a plain-money fast path, prorata allocation in exact shares, ambiguity explained
  lazily, and a test that the ordered and scanning paths agree.
- `core`: typed `Id`/`Arena`/`Run`, a pre-order `Tree` where a subtree is an id range, `Groups` built by counting
  sort, an `Interner` that borrows `&'s str`, exact `Qty`/`Ratio`, SWAR digit parsing, Joffe's calendar, scoped
  `par`.
- `syntax`: a borrowed AST of flat per-piece tables behind `Ref<T>`/`Many<T>` (a 24-bit local index plus the
  piece), a post-order expression arena, and parallel parsing of large files cut at item boundaries.
- `engine/totals.rs` block prefix sums, `infer.rs`'s per-place `?` solver, the `timeline.rs` stream merge, the law
  compiler's post-order IR, and `explain.rs`'s balance-failure suspects (swapped digits, a doubled flow, a sign
  slip).

v5 keeps all of these verbatim. They are what the kernels are made of.

## 5. The design: seven kernels

Each kernel takes one theory, absorbs a list of today's modules, has a small typed shape and one good algorithm.
The fold is the only place where time passes.

```text
           sources ──parse──► AST ──collect (one pass)──► Declarations ──elaborate──► Book
                                                           │  K1 taxonomies          │  K4 events (solved IR)
                                                           │  K2 behaviours          │  K5 promise terms
                                                           │  K3 positions           │  K6 rules (desugared)
                                                                                     ▼
 journal events ─┐                                                  Plan (immutable, Sync)
 promise monitor ┼─► timeline ─► post(leg) ─► positions & parcels ─► rules fire ─► derived legs ─┐
 deadlines ──────┘        ▲                                                                      │
                          └──────────────────────────────────────────────────────────────────────┘
                                         ▼
                        K7 facts: postings, position histories, tallies, effects, residuals
                                         ▼
                        views = queries · forecast = the same fold past today · why = provenance walk
```

### K1. Taxonomy: one scoped tree builder

**Theory:** pre-order trees (core already) and lexical scope by system.
**Absorbs:** `kinds::declare_sites`, `purposes::declare_sites`, `paths::build` and both `cycles`, about 700 lines.

```rust
/// A tree of names written `NAME : PARENT`: kinds, purposes, systems. Roots are built in; parents resolve
/// in the declaring file's scope with a suggestion when they do not; a cycle is reported once and cut.
pub struct Taxonomy<T> {
    pub tree: Tree<T>,
    pub index: Scoped<T>,
}

pub trait Node: Sized {
    const NOUN: &'static str;
    fn root(name: Sym) -> Self;
    fn declared(at: Declared<'_, '_>, names: &mut Interner<'_>) -> Self;
}
```

**Budget:** about 250 lines.

### K2. Behaviours: every value that changes on a day is a `Timeline`

**Theory:** FRP `stepper`/`switcher` (Elliott and Hudak) and Fowler's Temporal Property and Effectivity:
statements are steps, and `until` switches back.
**Absorbs:**
- the typed static fields in `Kind`/`Place`/`Entity`, `props.rs`'s `Assign` enum and its five `set` impls;
- `Prop { since }`, `ParamRow { since }`, `Residence`, `Prices`;
- the budget and terms timelines;
- `engine/temporal.rs`, `sample_temporal*` and `plan.rs:343 temporal_queries`.

About 3,700 lines in all.

```rust
/// A property's id and the type of its value, both known at compile time: `SELECT: Key<Policy>`.
pub struct Key<V> { id: KeyId, of: PhantomData<fn() -> V> }

/// Every declared value that can change: built-in and `has` properties, terms, budgets, params, prices,
/// residence. One timeline per holder and key; a kind's timeline is the default of its things.
pub struct Behaviours { values: Map<(Holder, KeyId), Timeline<Value>> }

impl Behaviours {
    pub fn at<V: FromValue>(&self, key: Key<V>, holder: Holder, day: Day) -> Option<V>;
}

impl<T> Timeline<T> {
    /// The days in `within` on which `holds` is true: `days(self.lives is foreign, 2026)` exactly, by
    /// summing step lengths, with no sampling.
    pub fn days_where(&self, within: Days, holds: impl Fn(&T) -> bool) -> u32;
}
```

**Algorithm:**
- Behaviours are interval maps, and `days()` integrates them exactly.
- A position's balance history (K7) is a stepper as well. `peak`/`low` over a window use a sparse table, which
  answers range max and min in O(1) after an O(n log n) build. That covers FBAR peaks with no sampling.
- A yearly schedule starts from the first step of any behaviour, which fixes the residence test.

**Budget:** about 900 lines.

### K3. Positions and parcels: custody and claims are one store

**Theory:** REA agents and ValueFlows custody vs rights. A bank deposit *is* the bank's debt to you, so an account,
a claim, a debt and an envelope are all one owner's standing with one agent. Parcels with provenance cover
fungible and identified resources alike.
**Absorbs:**
- `Role::{Account, Holding, Outside, Issuer, Tab, Asset}` plus `contract_endpoints`;
- the survey and tab prediction (~500 lines);
- `OpenClaim`;
- `assets.rs`, `assets_runtime.rs` and the asset-part code in `post.rs` (~1,000 lines).

```rust
/// Where value sits: an owner's standing with an agent. `checking` is chase's debt to Sam; `card` Sam's to
/// chase; an invoice halcyon's to the studio; cash Sam's standing with himself.
pub struct Position {
    pub owner: Id<Entity>,
    pub with: Id<Entity>,
    pub name: Option<Sym>,
    pub kind: Id<Kind>,
    pub class: Class,
}
```

What changes:
- **Positions are created during elaboration in an append-only arena**, so no survey is needed: the declarations
  are frozen and borrowed, and positions grow beside them.
- **A claim is a parcel** in a position with the debtor. **Settling is relief**:
  1. the `[^code]` selector, which already exists in `lots.rs`;
  2. a new `Exact` policy;
  3. FIFO.
- **Aging is the parcel's `acquired` day; blame is the class.**
- **An asset is a position holding one parcel per part.** `Parcel.part` already exists.
  - `consume` (depreciation) is `rebase`;
  - `carry` (a wash sale) is `rebase` on the matched lot;
  - a sale relieves every part.

  That deletes the parallel `AssetState` store.

**Budget:** about 600 lines, on top of `lots.rs` (kept).

### K4. Events: one IR, solved at compile time when it can be

**Theory:** REA events and ValueFlows actions, and Ellerman's network view: a transaction's unknowns form a forest.
**Absorbs:**
- the nine lowering paths and the three split resolvers;
- `Txn`, `Flow`, `Detail`, `FlowView`, `RuntimeFlow`, `RuntimeTxn`, `JournalProgram`, `JournalGroup`,
  `JournalItem`, `WrittenOccurrence`, `WrittenGroup`, `TemplateFlow`, `TemplateLeg`, `TemplateItem`, `Also`,
  `Measure`, `Assert`, `Split`, `Event`, `EndEvent`, `ClaimChange`, `Reading` and `Filed`.

That is 24 types in nine stores.

```rust
pub struct Event { pub day: Day, pub verb: Verb, pub legs: Run<Leg>, pub tail: Id<Tail>, pub at: Loc }

pub enum Verb {
    Move, Owe { due: Day }, Measure(Effort), Value(Gap), Change(Id<Change>),
    Waive(Days), End, Settle(Settlement), Split(Ratio), File(i32), Keep(Id<Promise>),
}

pub struct Leg { pub from: Id<Position>, pub to: Id<Position>, pub qty: Q, pub why: Purposed, pub codes: Run<Sym> }

/// A written quantity. Literal transactions (nearly all of them) are solved by the model; the rest by the
/// fold, with the same function.
pub enum Q { Known(Amount), Expr(NodeId), Rest, Target(Amount), Unknown(Id<Commodity>), All(Option<Id<Commodity>>) }

pub trait Env { fn eval(&self, node: NodeId) -> Result<Amount, Fault>; fn held(&self, p: Id<Position>, u: Id<Commodity>) -> Qty; }

/// Header, legs, `...`, carve/add/less items, percent of the header: resolved once.
pub fn solve(legs: &mut [LegIr], env: &impl Env) -> Result<(), Fault>;
```

**Elaboration through one guard:**

```rust
/// Staged writes to the book's arenas. Dropped without `commit`, everything it added is truncated away.
pub struct Staged<'j> { journal: &'j mut Journal, marks: Marks, committed: bool }
impl Drop for Staged<'_> { fn drop(&mut self) { if !self.committed { self.journal.truncate(self.marks) } } }
```

**Algorithm:** **constant folding**. `solve` with a `LiteralEnv` runs at model time and leaves `Known` amounts. The
fold calls it only for transactions whose expressions read state, so a computed amount is never handled twice.

**Budget:** model elaboration about 1,300 lines (from 5,400); the engine solver about 250.

### K5. Promises: CSL terms, reduced by residuation in the fold

**Theory:** Peyton Jones, Eber and Seward; Bahr et al.'s seven-constructor core; CSL's blame and deadlines;
FCL's ⊗; ACTUS ANN/NAM/PP/RR.
**Absorbs:**
- `Contract`'s eight optional features and `book.rs`'s schedule and escalation logic (~450 lines);
- most of `lower/contracts.rs`;
- `lower_occurrence`, `lower_loan_origin` and `lower_owes`;
- `instantiate_occurrence` and `materialize_group`;
- `contract_forecasts`, `nearest_occurrence` and `sync/promise.rs`.

```rust
/// A promise, stored post-order like expressions: children before parents, a subtree a range.
pub enum Term {
    Done,
    Pay(Id<LegTemplate>),                                               // one flow; amounts are expressions
    All(Range<TermId>),                                                 // and
    Every { schedule: Schedule, body: TermId },                         // the only recursion is a calendar
    Due { grace: Span, blame: Id<Entity>, body: TermId, otherwise: TermId }, // obligation ⊗ reparation
    If { cond: NodeId, then: TermId, otherwise: TermId },
    Let { name: Sym, value: NodeId, body: TermId },                     // fix an index that day: reset, CPI
    Annuity(Id<Loan>),                                                  // ACTUS ANN: interest and principal per period
}

/// What is still owed of one promise: a cursor into the plan's terms, never a copy of them.
#[derive(Clone, Copy)]
pub struct Residual { term: TermId, next: Day, ordinal: u32, open: Qty }
```

| written | term |
|---|---|
| `contract flat … 2_900 USD monthly on 1 from checking` | `Every(monthly on 1, Due(grace, me, Pay(rent), Done))` |
| `due 5d else + 5% #late-fee` | `Due { otherwise: Pay(fee) }` |
| `halcyon owes studio 3_800 USD due 30d` | `Due(30d, halcyon, Pay(3_800), Done)`; the open amount is a parcel (K3) |
| `deposit 2_350 USD` | `All[Pay(in, for dana), At(end, Due(30d, me, Pay(back), Done))]` |
| `loan 320_000 USD … over 30y` | `All[Pay(principal in), Every(monthly, Let(rate, Annuity))]`; a prepayment reduces the residual's `open` |
| `buy VTI for 500 USD monthly` | `Every(monthly, Pay(500 USD → VTI))` |

**Algorithm:**
1. Deadlines sit in a binary heap merged into the timeline.
2. An event matches its promise by a sweep over the sorted due days within grace, in O(n + m), replacing
   `nearest_occurrence` per line.
3. An occurrence's ordinal is computed arithmetically from the schedule, by binary search on the cadence, in
   O(log n).
4. Passing a `Due` with nothing kept creates a claim parcel against the blamed party (K3) and starts `otherwise`.
5. An annuity's residual is `Done` when its balance is zero, which fixes the loan forecast.
6. **The forecast is the same fold run past `today`**: `Pay` terms that no event kept emit planned legs. That
   deletes the second driver and fixes the closing-prefix failure.
7. `claims`, `contracts` and `check` read the residuals.

**Budget:** the compiler about 500 lines, the monitor about 800.

### K6. Norms: one rule IR; everything else is sugar

**Theory:** defeasible deontic logic (Nute; Governatori and Rotolo's FCL; Catala) and lex specialis, posterior and
superior.
**Absorbs:**
- `laws/budget.rs` (521), `lower_alsos` (280), `lower/also.rs` (150);
- contract `shares` (170), `Match`, the sales-tax special path;
- the budget functions in `eval.rs` (~150);
- the nine dispatch tables of `rules.rs`.

| sugar | desugars to |
|---|---|
| `also ITEM \| FLOW when E` on X | a law owned by X: `on flow when E derive ITEM \| FLOW` |
| `share 20% for studio` | `on flow derive 20% of amount for studio` |
| `sales-tax 8.625%` on a party kind | `on out derive item amount × 8.625/108.625 #sales-tax` |
| `budget food 900 USD monthly carries` | `on flow warn total(month, carried) <= 900 USD` |
| `match 50% of [retirement] up to 6%` | an `also` |

The only new effect is `Derive(LegTemplate)`. **Purpose inference uses the same ranking as laws**: written,
promise, party, party kind, commodity kind, account kind. It becomes a `classify` default with a rank, replacing
`endpoint_purpose`, `taken_purpose` and `infer_for_flow`. **Dispatch** is one `Groups<(Trigger, Key), Rule>`.

**Budget:** desugaring about 300 lines; laws/compile about 1,400 (kept, minus the budget paths).

### K7. Facts: views are queries; `why` is a provenance walk

**Theory:** the XBRL Open Information Model (concept, entity, period, unit, value; instant vs duration) and
Haig–Simons (flow and balance are two views of one fold).
**Absorbs:**
- `history.rs`'s replay, `holdings_at`'s re-plan, `available`'s double fork;
- `projection.rs`'s second replay;
- most of each view's bespoke fold;
- about half of `why/*` (1,900 lines).

What changes:
- **Position histories are steppers in the `Run`**, so `balance --at`, `--monthly` and `register` read
  `Timeline::at`.
- **Every fact carries its origin.** `Origin { event: Id<Event>, rule: Option<Id<Law>>, promise: Option<Id<Promise>> }`
  already half-exists as `Origin`/`Derivation`/`Provenance`. `why X` walks those edges: what made this, and what
  this made.
- **A view is a pivot:** rows by a dimension (position tree, purpose tree, party, period), columns by period, a
  measure, and a filter.

**Budget:** report about 4,300 lines (from 9,000).

## 6. The language: the junction says what happens

### 6.1 One line grammar, where the junction is the action

Every journal line is already `DATE SUBJECT VERB …`. A reader skims the verb column (LANGUAGE §2), but for 2,387 of
the example lines (flows) that column always says `->`. v5 makes flows follow the rule:

| line | REA / ValueFlows action | reads |
|---|---|---|
| `checking -> trader-joes 84.20 USD` | transfer out (give) | checking pays Trader Joe's |
| `checking <- acme 5_750 USD #wages` | transfer in (take) | checking receives from Acme |
| `fidelity <- 7 VTI @ 285.70 USD` | exchange: buy | fidelity buys 7 VTI; the money leaves fidelity |
| `fidelity -> 1.62 VTI @ 297.00 USD` | exchange: sell | the proceeds stay at fidelity |
| `halcyon owes studio 3_800 USD due 30d` | commitment | unchanged |
| `checking = 8_828.87 USD` | observation | unchanged |
| `me worked 6.5 HR for halcyon` | work (effort) | unchanged |
| `flat` | a promise kept | unchanged |
| indented `-> irs 692 USD`, `<- acme 40 USD` | a leg of the same event | the arrow is explicit |
| indented `+ 4 USD #tip`, `- 60 USD`, `32 USD #gifts` | items | unchanged |

**Rule:** the subject of a money line is always the book's own side: an account, an owner, a position. The
subject column becomes the register, which is also how sync writes a feed. Three things follow:
- `<-` exists so that income stops putting a party in the subject column. 314 example lines flip.
- **Legs lead with their arrow.** Today a leg and an item differ only by whether an end is named. LANGUAGE §3's
  "a leg between two parties passes through the owner" stops being a rule and becomes what the syntax says.
- **`@` marks an exchange**, with the market by default or with a party when one is named. That retires the
  amount-on-both-sides form (`checking 2_000 USD -> fidelity 7 VTI`) and the dangling one-ended arrow
  (`20 lumen 9_200 USD ->`, finding 02b5).

Every other primitive is unchanged. This is the smallest alphabet that makes the verb column say what happened.

### 6.2 Positions belong to the agent that holds them; a debt is a promise

```text
entity chase : bank
  checking : deposit
  savings  : deposit
  card     : credit-card
    in full monthly on 25 from checking           // a card is a promise too: the forecast sees its bill
entity fidelity : broker
  brokerage
  retirement : 401k
    employer lumen
entity rocket : lender
  mortgage : loan 320_000 USD on 2024-02-20 at 5.875% over 30y for condo
    monthly on 1 from checking
    also -> escrow 410 USD
  escrow : escrow
```

What this removes:
- the `account` keyword, `at PARTY` and `entity chase : org`;
- **one debt written twice** (an account plus a contract);
- `kind receivable` and `kind payable`, since a claim is a position with any party;
- the name collisions: `fidelity` is the agent, `brokerage` is what it holds for you, and `fidelity/brokerage`
  works when a short name is ambiguous.

### 6.3 Purposes say what for; direction says income or spending

Purpose roots drop `income` and `spending`. The engine already derives direction (`lib.rs purpose_direction`);
the roots only duplicate it.
- `#interest` paid to a lender is spending; received from a bank it is income. One purpose replaces `interest`
  and `interest-income`.
- `#rent` to a landlord is spending; from a tenant it is income. No `pays` line is needed for it.
- **The party still decides refund versus income**, as today: money *from* a party whose kind's `purpose` is
  `groceries` is a refund of groceries.
- `capital` stays a property of the purposes that join an asset (`purchase`, `improvement`). `transfer` stays a
  property of the purposes that move value between owners.

### 6.4 No party has to be invented to stand for a category

- **The counterparty becomes optional** when a purpose or description says why:
  `06 card -> 94.21 USD #dining`.
- **It defaults to the position's agent** when one exists: `31 savings <- 170.70 USD #interest` is from chase.

In `05-family`, 68 flows stop needing `restaurant`, `interest-source`, `gifts`, `rewards`, `household-store`,
`kids-market` and `trip-vendor`.

### 6.5 Before and after, on a real month

`examples/05-family/journal/2025/03.ax`, the bonus day, today (14 lines):

```text
2025-03-14 acme -> joint-checking 12_000.00 USD #wages
2025-03-14 joint-checking -> alex-401k 1_200.00 USD #household-deferral // 10% pre-tax deferral
2025-03-14 joint-checking -> irs 2_640.00 USD #federal-tax
2025-03-14 joint-checking -> ftb 1_227.60 USD #state-tax
2025-03-14 joint-checking -> ssa 918.00 USD #payroll-tax // Social Security + Medicare
2025-03-14 joint-checking -> edd 144.00 USD #state-disability // CA SDI
2025-03-14 acme -> alex-401k 480 USD #contribution
2025-03-14 bluefin -> joint-checking 3_692.31 USD #wages
2025-03-14 joint-checking -> jordan-401k 221.54 USD #household-deferral // 6% deferral
2025-03-14 joint-checking -> irs 312.00 USD #federal-tax
2025-03-14 joint-checking -> ftb 118.00 USD #state-tax
2025-03-14 joint-checking -> ssa 282.46 USD #payroll-tax // Social Security + Medicare
2025-03-14 joint-checking -> edd 44.31 USD #state-disability // CA SDI
2025-03-14 bluefin -> jordan-401k 110.77 USD #contribution
```

v5, the same facts written out. Each paystub becomes one event that reads left to right: gross in, where it went,
the rest to checking.

```text
/// Annual bonus, withheld at the flat supplemental rates (22% federal, 10.23% California).
14 me     <- acme     12_000.00 USD #wages
     -> alex-401k      1_200.00 USD #deferral
     -> irs            2_640.00 USD
     -> ftb            1_227.60 USD
     -> ssa              918.00 USD
     -> edd              144.00 USD
     -> joint-checking ...
14 jordan <- bluefin   3_692.31 USD #wages
     -> jordan-401k      221.54 USD #deferral
     -> irs              312.00 USD
     -> ftb              118.00 USD
     -> ssa              282.46 USD
     -> edd               44.31 USD
     -> joint-checking ...
14 alex-401k   <- acme      480.00 USD #match
14 jordan-401k <- bluefin   110.77 USD #match
```

Declared once as an `also` on the pay contract, as the family's 2026 contracts already do, the two match lines are
derived. With the v4-intended promises (the example starts its contracts in 2026 and writes 2025 out), March's 87
entry lines become 48:

```text
2025-03

01 mortgage                                 // ▸ 3,174.46: principal 469.85 · interest 2,014.61 · escrow 690.00
02 card -> netflix               17.99 USD
03 card -> costco               158.77 USD
03 joint-checking -> st-annes-parish 200.00 USD #charity
05 car-loan                                 // ▸ 602.41: principal 511.80 · interest 90.61
05 dcfsa -> little-sprouts      416.66 USD #childcare
06 card -> 94.21 USD #dining                // no `restaurant` party
14 alex-bonus                               // ▸ 12,000.00 gross · net 5,870.40
14 jordan-pay
15 alex-pay
25 card                                     // ▸ February's statement, 2,308.35, paid in full
28 jordan-pay
31 alex-pay
31 joint-savings <- 170.70 USD #interest    // from chase
31 joint-checking = 23_072.46 USD
…
```

Across the year, the family journal writes 51 paystubs out line by line. They take about 430 of its 983 entry
lines, and as promises they become 51 lines.

### 6.6 What the parser gains

Flows and statements become one production, `DATE SUBJECT VERB …` with a verb table:
- `transaction()` and `statement()` merge;
- `flow.rs` and `journal.rs` fold into `statement.rs`;
- contract lines lead with a keyword or an arrow, so `contract.rs:at_schedule`'s four lexer-cloning lookaheads go.

The formatter aligns one set of columns. `axiom fmt --upgrade` rewrites v4 books mechanically from the AST
(flip party-subject lines to `<-`, put arrows on legs, nest accounts under their institution), so no example has
to be ported by hand.

## 7. The line budget

| crate | now | v5 | mechanism |
|---|---:|---:|---|
| core | 1,825 | 1,500 | drop the `Day::MIN` defensive calendar code (−150); `Timeline::days_where`, sparse table (+80) |
| syntax | 5,481 | 4,350 | one line grammar; a formatter on AST locations; v3 migration hints retired (−150) |
| model | 17,034 | 6,950 | `collect` (one pass) · K1 · K2 · K3 lazily · K4 with `Staged` · K5 compiler · K6 desugar · diagnostic catalog |
| engine | 11,009 | 6,550 | no materializer; the monitor (+800); asset parts are parcels; one totals path; no sampling |
| report | 7,059 | 3,400 | the forecast is the fold; views are pivots; `why` walks provenance; one JSON writer |
| sync | 4,375 | 2,550 | reads `Run` positions and residuals; dates from `syntax`; file application moves here from cli |
| cli | 2,631 | 1,850 | sync application leaves; one renderer for views |
| **total** | **49,428** | **~27,000** | **−45%** |

The v5 column was first estimated against the tree as written. It is restated here per crate by the same ratio the
formatter applied to that crate, which assumes the ported code keeps its density; the new kernels were sketched in
this style already, so the restatement is if anything generous.

The floor is about 24,500. That is a clean-room estimate of this feature set at this diagnostic quality, kernel by
kernel. It comes in below the table because a rewrite also loses the slack that a port keeps.

**Reaching about 20,000** needs one or more of these. Each is a product decision, not an engineering one:

| lever | saves | costs |
|---|---:|---|
| count `sync`'s importers (csv, tagged, peg, format: ~1,500 lines) as a separate crate outside the core budget | ~2,500 | none at run time; sync becomes a plugin crate |
| habit forecasting (`expected.rs`, `variable.rs`, `recurrence.rs`, `bands.rs`) moves out, so the forecast is promises only | ~400 | no p10/p50/p90 bands |
| `why` keeps only provenance walks and drops the bespoke per-target pages | ~600 | plainer `why` output |
| a diagnostic catalog in syntax as well | ~350 | none |

## 8. Migration: eight lanes, each ending green

The order keeps every test and golden passing at each merge. The lanes interleave with the four known failures,
so they get fixed by construction rather than patched.

| lane | does | removes | exit |
|---|---|---|---|
| **K0 groundwork** | restore `collect`; the `Staged` guard; the `Problem` catalog; `file.word()`; delete dead code and duplicate helpers; calendar to core; sync dates to syntax | ~3,000 | no behaviour change; goldens byte-identical |
| **K1+K2 behaviours** | `Taxonomy<T>`; `Behaviours` with typed keys; params, prices, residence and budgets on timelines; `days_where`; delete sampling | ~3,500 | fixes `temporal_days_count…` |
| **K3 positions** | positions created lazily; delete the survey; claims as parcels; settle = relief (code → exact → FIFO); asset parts as parcels | ~3,000 | ClaimChange and settlement work; a decision on prorata basis (§10) |
| **K4 one event IR** | `Event`/`Leg`/`Q`; `solve` shared by model and engine; delete templates and the occurrence diff | ~4,000 | the family and landlord occurrence errors |
| **K5 promises** | term compiler; residuation monitor; forecast = the fold past today; delete `contract_forecasts` and the materializer | ~4,000 | fixes both forecast failures; `monitor_complete` goes away |
| **K6 norms** | desugar `also`, `share`, `sales-tax`, `budget`, `match`; ranked purpose defaults; one dispatch index | ~2,000 | purpose-disagreement counts in family and landlord |
| **K7 facts** | position steppers in `Run`; views as pivots; provenance `why`; one JSON writer | ~4,000 | goldens regraded once, with each change justified |
| **L language** | the junction, positions under agents, debts as promises, direction-derived purposes, optional counterparty; `fmt --upgrade` | ~700, plus smaller examples | every example upgraded mechanically |

K0 can start immediately and in parallel with nothing else. The remaining lanes run in two waves: K1/K2 and K3
together, then K4 and K5 together. K6, K7 and L can overlap once K4 has landed.

## 9. What must not be lost

- Every behaviour pinned by the 734 passing tests, the 60 goldens and the 125 mistakes. The four failing
  assertions stay as written.
- `lots.rs` relief semantics and its ambiguity diagnostics; `totals.rs` prefix sums; `infer.rs`; `explain.rs`'s
  balance-failure suspects; the law compiler's typing and units.
- Borrowed AST and borrowed view cells; `Plan` immutable and `Sync`; no `Arc`, `Mutex`, `Rc` or `RefCell`; stable
  Rust; `memchr` as the only dependency.
- The provenance the model already carries (`Origin`, `Derivation`, `Provenance`). K7 generalizes it, not replaces it.
- Performance: the 1M-line check stays under a second. Constant folding takes literal transactions out of the
  fold's expression work, and residuation replaces an O(n²) ordinal count.

## 10. Decisions for you

1. **The budget.** Approve about 27,000 (under the fixed formatter) as the v5 ceiling, or name which §7 levers to
   pull for about 20,000.
2. **The junction.** `->` / `<-` / `@` as in §6.1, or word verbs (`pays`, `gets`, `buys`, `sells`). I recommend the
   arrows: they keep `->`, stay one token wide, and read left to right with the subject first.
3. **Positions under agents** (§6.2), which replace the `account` keyword. I recommend yes, because it removes the
   namespace collisions and the double-declared debts.
4. **Prorata basis** (the fourth failing test): either `basis zero` places keep after-tax contributions' basis
   when it is stated, or the old fixture gains an explicit `basis`. This is the one failure that needs a
   semantic decision, not a kernel.
5. **Where this lands.** The cutover branch is `cutover/promote-workspace`, which I have not pushed to. Say whether
   v5 lanes branch from it, and whether I may push there.
