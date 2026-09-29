# Axiom v2 — handoff plan

This document lets a fresh session continue this work with no other context. Read
it top to bottom once, then [DESIGN.md](DESIGN.md) and [LANGUAGE.md](LANGUAGE.md).
Those two are the constitution and the normative spec. This file holds the
motives, decisions, current state, the remaining design details, and the
execution plan.

---

## 0. Environment facts

- Repo: `axiom/` (git, branch `canonical`, main branch `main`). **`v2/` and
  `v2-previous/` are untracked.** Commit or copy them before moving machines, or
  they will be lost.
- `v2-previous/` holds the old v2 attempt (S-expression rule language, ~10k
  lines; it did not compile). It is kept only for reference and can be deleted
  once v2 surpasses it.
- The old v1 lives in `axiom/src` (~71k lines) with docs `axiom/confirmed-direction.md`,
  `axiom/other.md`, `axiom/DESIGN.md`, `axiom/IMPLEMENTATION.md`. They are
  inspiration only.
- Toolchain on the original machine: stable rustc 1.98.1 (Homebrew), no nightly
  and no rustup, so **no `std::simd`**. The machine was arm64, 11 cores. The
  crates available offline in `~/.cargo` included `memchr 2.8`, `rayon`,
  `rustc-hash`, `smallvec`, `unicode-width`, `miette`, and
  `codespan-reporting`. There was no `ariadne`. The plan uses only `memchr` as an
  external dependency.
- Build and check: `cd v2 && cargo check --workspace` and
  `cargo test -p axiom-core --release`. The user asked for **no test-writing and
  no clippy** for now; `cargo check` is enough. The existing core unit tests
  exist and pass. Keep them.

## 1. What the user asked for (every requirement)

In the user's words, condensed:

1. Implement in `v2/`, and rework everything there ("it's kind of trash").
2. Be **brutal with the implementors (sub-agents, "lanes")**: accept only the best
   code, and constantly rework abstractions to be stronger and more capable
   while getting simpler.
3. The code should be **a fraction of the current codebase** (71k lines),
   excluding tests and comments. The aim is roughly an order of magnitude drop.
   Line count is not the goal and there must be **no code golfing**. What counts
   is readable, beautiful code built on deeper, smarter abstractions that compose
   and make more things *innate qualities*.
4. The craft bar is benjoffe.com/fast-day-of-week: clever, minimal-work
   algorithms, exhaustively validated.
5. Read `v2/DESIGN.md` (the old one, now in `v2-previous/`) and find an even
   stronger formulation: a "magical", **progressively complex** application with
   **rich constraints** that expresses much more complex financial situations.
   **Error messages must be gorgeous and as helpful as the best compilers**
   (the user referenced "janklang", their benchmark).
6. Keep **all of the handwritten notes** in mind (transcribed in §2).
7. Build towards a **comprehensive budgeting and accounting application** that
   is simple, **fully typed**, runs **forecasts** easily, shows where money is
   and where it may go based on history, and generates **tax reports** and other
   reports from the constraints of the systems you operate in.
8. Capture the nuance of a **401k, college savings (529)** and similar accounts.
   **Catch postings that move money in ways that violate their constraints**, and
   infer those violations in most or all cases.
9. Constraints at **all levels**, down to **country, state, and city**, with the
   **community defining constraints**.
10. **Advanced Rust typing and borrowing tricks**: avoid allocations and stay
    highly efficient, **especially for small monetary values**. It should be
    **deeply parallel**, **SIMD-accelerated**, flexible, and able to handle
    **gaps in knowledge and externalities** (missed transactions).
11. The language must be **gorgeous and simple, with progressive complexity**.
    Stocks, bonds, property, and more should be trackable at full granularity.
12. **No direct internet access**, but users can attach **scripts in any language
    that run on `sync`** (for example, to check house prices). It needs deep
    granularity over time.
13. Read all files of the old attempt. Find a **massively simpler formulation**
    that achieves all the same benefits and more. Do as much as possible at
    compile time and avoid redundant work.
14. **Great crate separation** with excellent boundaries and folder structure.
15. **Use Sonnet lanes for implementation.** Orchestrate, review, and rework.
16. **CLI only** for now. Drive it all the way, exceeding the old code's
    capabilities while being simpler, more robust, and faster.
17. Use stdlib tricks to **avoid `Arc` and `Mutex`**. Build excellent data
    structures.
18. Dependencies like ariadne or miette are acceptable. Workspace crates get
    the same quality bar.

## 2. The handwritten notes (full transcription, 12 pages)

These are paraphrased where the handwriting was unclear. Items marked (?) were
illegible or uncertain.

**The journal.** The general model for input should be concentrated in the
journal, which is the substrate of record. Debits are losses or withdrawals;
credits are additions or deposits. The ultimate aim is to tell you **how much
money is available to spend**. Any intake or outtake happens between two parties,
payer and payee. The only requirement to be either is having a place for money to
go: "if you're a garbage can, you count; the money goes to the trash."
Accounts section the flow of money and add the concept of *location*. Different
locations carry different meaning and semantics.

**Transactions.** A transaction is a matched set of debits from at least one
account and credits to at least one account. It is a *set* because payments are
compositional: you might spread across accounts to maximize future value, or
lock funds in one account. A composite payee is rare, so it is usually written as
a string of transactions.

**Ideas.**
- Transactions have implicit accounts. You can register an entity, and if a known
  entity is the recipient you don't need to write its account, because it is
  derived.
- It should describe complex financial realities. **An uncashed check is a
  "maybe", not "now".** Stocks and other instruments have rich constraints and
  can be functionally liquid. Other assets have rigid assurances of eventual
  liquidity. These matter for forecasting.
- A general **effect system**. The model should cover the breadth of financial
  realities, though some agreements may be extemporaneous.

**Timeline diagram.** A debt asset is gained, constrained by a payback period.
At the end of that period, any action afterwards errors because of the
unfulfilled constraint.

**More ideas.**
- Model risk in general by describing the *properties of transfer*. The system
  works along a dimension of "activity": the value of things is only determined
  over many actions. Inflation or risk can be described as the **cost of action
  rising or falling**.
- A **constraint-based specification of the world's economic systems**: models
  for bonds, 401ks, and so on, maintained by the community. Beancount's few
  constraints are double-entry: an action on one account imposes the inverse on
  another. **Constraints act as narrowing tools, so they stay opt-in.**
- Against a timeline, actions are a subset of time and no two actions happen at
  once. **In a conflict, order of declaration wins** (in loose mode). You can't
  constrain time, but you can constrain actions. Time is contingent on the flow
  of actions, so **constraints about future events apply only once an action is
  filed at or beyond that date**. Constraints are mutable. The way to remove a
  constraint error is to loosen or remove the constraint, **or resolve it to a
  loss**.
- **Commodities are collections of rules defining their use.** They are on the
  same spectrum as financial instruments, and currency is a subset.
- Linking accounts to more than one place should happen at account creation. A
  transaction that represents a particular place must be marked as such. (The
  meaning is ambiguous; possibly joint ownership or locations.)

**Syntax sketch.** `DATE EMPLOYER@income -> Assets/checking +$500`: "they paid,
with this account, to my checking account, at this amount."
- Debts are assigned money. If someone owes you, that is an asset: a marker of
  total value even if not direct currency. Debts you owe are illiquid losses.
- **Liquidity is mobility**: the relationship between the accounts you hold that
  are directly interchangeable. Zero hops to perform the transaction means
  liquid.
- Accounts can hold more than one commodity (gold and silver, …).
- Commodities are linked through a price system, a **price history file**.
- **Equity is value.** Starting a ledger usually sets an initial value.

**"We need a better theory of accounts."**
- **An institution issues identity.** By default all transactions occur against
  your own person (an institution of just you). With others, where money comes
  from isn't always 1:1 with the semantics of its origin institution.
- **If a fund deposits $100 into checking, it is still money tied to the fund**,
  even though the account itself can't enforce that.
- Generalized: **if an account holds money from organizations with conflicting
  semantics, no transaction can be performed without narrowing the ambiguity.**
- Ledger's auto-add feature means each new account creates a typed view of the
  world. You specify accounts only when the world is ambiguous enough to need it.
- **Grant money isn't money; it is a subclass of money dictated by the grant.**
- (?) "Slapdash comments /c". **Don't want the whole meta key/value thing.**

**Basic account categories.** Expenses (where money goes out), Assets (holdings),
Income (input), Equity (value).
- Handle gains on assets by **requiring cost basis on asset entry**. Find an
  ergonomic way to handle immediate taxation (sales, capital gains).
- Commodities can be anything: stocks, cash, Google gems. That makes
  **importers** far more important, since they may import from sources very
  unlike bank statements.
- Ledger records gain explicitly with cost `{$20}` versus price `@ $50`.
- Ledger identifies past transactions by value and date. **We should use only
  date plus tag**: "sell everything from [2026-2027]", "empty all holdings marked
  house: [#house]".
- Better importers mean more data, and more data means more precision.

**Typing.**
- **Typing against money** makes control over balances simpler. If you want a
  commodity's relative cost fixed at construction, encode it in the type.
- **Price and cost differ.** Ledger's cost-basis syntax is unstructured.
  Example: a Shell gas purchase, 11 gallons at a basis of 2.2 (?) but priced @
  2.30. Ledger silently books the difference to capital losses, which "feels
  devious". **Those differences should DEFINITELY be recorded.**
- Ledger's built-in valuation, with Python lookups for "what was BTC worth on
  X", is bad and under-declarative. **Any current valuation can be represented as
  a state machine.** Processing should be **pure and in-language**.

**Ledger critique.**
- When declaring accounts, you can constrain payees (through ranges) and the
  commodities they transact in.
- Ledger has a general-purpose expression language for constraints and math.
  (?) Could we reuse some existing math language here?
- A **global balancing account** is a **bad idea**.
- Aliases and mappings for account names are conceptually weak and break typing.
- Assert versus "check" (warning only): a **relaxed mode where missed constraints
  are warnings is fine, but not by default, and there should be no "check"**.
- Payees inheriting account flexibility (aliases): **disagree; payees should be
  typed all the way down.**
- **Tags have constraints** on where they can be placed.

**Layout and syntax.**
- **Folder structure should be enforced by default, yet be overridable.**
- Declaration order is `AMOUNT CURRENCY`. **No dollar signs.**
- Transactions across dates: not bad, but use the same mechanism as stock
  constraints.
- **Instead of tags, codes.** A regex-like grammar for meta rules would help.
- Cleared/uncleared state on transactions and postings: **dislikes mutability in
  journals** (hard to fully avoid, but discouraged). NEEDS THOUGHT.
- A general **doc comment** system is great, as long as it is separate from
  plain comments.

**Payees.**
- The payee idea is universal. It should be an **optional extension of a
  transaction arm: `100 USD / John`**. The place of money isn't its identity: with
  PayPal, transactions show PayPal rather than the real recipient, so **payees are
  tree-like and nestable**, like accounts.
- Ledger's virtual postings for budgeting are smart, but that notation should be
  tracked, linked, and usually elided.
- The only number needing no currency is 0. **Use a term `EMPTY` instead.**
- Folding assertions into transactions with `=` makes no sense when the
  transaction already balances, but **as a way to compute the balancing amount
  it is good**.
- Marking accounts as adjusted (pad) is bad when it looks like a regular
  transaction. It should be **part of one class of escape hatches**.

**Ordering.**
- Order is only convention in ledger. **Make it law: the second is always the
  target.** Example: "800 USD @ 50.00 for shares".
- **Soft transactions go in parentheses**, e.g. `(1000 AAPL @ 11 $)`, so they
  don't affect the balance.
- The user is confused by double price and double `@@`.
- **Commodity swaps should work against the base currency.**

**Automation.**
- Automated transactions matching a predicate are a **bad idea**. They mix up
  the ledger's job of recording, and recursion or multiplier rules get weird.
  The *association* property of virtual postings is good. Move it to something
  else, like tags.
- **Splitting and recurring should share one primitive with predicting cost
  recovery.** Example: a 2025 purchase of 100 USD stretched across 2026.
- **Defining global tax or tithe** (income with a church tithe, etc.): explicitly
  link the semantics of an account with a system.
- **Simple attributes are defined in-language. More complex ones may use scripts
  (some language like Lean?).**

## 3. What the old code had (from five full-read inventories)

| Area | Old state | Keep as a *law* (not machinery) |
|---|---|---|
| Numbers | BigInt/BigRational everywhere, allocating on every compare | Exact arithmetic; explicit half-even rounding; unit-polymorphic zero (now `empty`) |
| Surface | 9 fixed directives; packages built from Rust structs, no source syntax | Lossless line-oriented parsing |
| Lots | FIFO/LIFO, tie-group ambiguity, conditional gains per candidate; blocked sales consume nothing | Never guess: ambiguous relief lists candidates with gains |
| Settlements | 14 states, rail tables for ach/card/check; only a final `settled` state counts; a bounced payment restores the obligation | Only a settled flow counts; `returned` reverses; nothing is deleted |
| Obligations | Satisfaction network with union-find isolation | Owed effects can be paid down |
| Accounts | Two unused account models; no constraints, no restricted funds, no retirement rules, no tax, no jurisdiction | Everything here is new |
| Liquidity | Standalone DFS Pareto search, unused by the engine | Replaced: liquidity is *derived from laws* |
| Scenario | Recurrence with missing-day policies, horizons; no balance projection | Recurrence clamps to month end |
| Proofs and stores | Proof DAGs, certificates, in-memory CAS, closes, restatements, merges | **Dropped** (git is the history; determinism remains) |
| Diagnostics | Strings, first error only, no carets, no suggestions | Replaced by rich, power-assert-style diagnostics |
| Incremental | Placeholder stages; everything recomputed | Dropped; the design is fast enough to recompute |
| v2 | S-expression rules over `BTreeMap<String,Value>`; no budgeting; did not compile | Lab finding: dense rows and inline numbers win massively |

## 4. The formulation (why it is shaped this way)

The full statement is in [DESIGN.md](DESIGN.md). The decisions, with reasons:

1. **Flows are the only facts.** `from -> to` is balanced by construction, which
   removes balancing, balancing accounts, and "debit/credit" from the user's
   head (notes: two parties, garbage can counts, no global balancing account,
   second is target). A transaction is a set of flows, with one side split
   (1:N or N:1). N:M is an error with a fix.
2. **Modes, not mutable state.** Parenthesized amounts are *pending* ("maybe,
   not now"). `DATE #code settled|void|returned` events change the mode
   append-only, identified by date plus code only (notes). Plans (`every`) are
   *planned*. Law consequences are *owed*. Nothing is ever generated into the
   journal (notes: automated transactions are bad).
3. **Parcels unify lots, basis, and restricted money.** A parcel is
   `(qty, basis, acquired, txn, tied-to)`, and equal parcels merge, so
   fungibility is derived and plain cash is one integer. **Basis means "value
   already accounted or taxed"**, and that one idea covers:
   - stock cost basis;
   - 529 and Roth contributions, where basis = face and earnings have basis 0;
   - pre-tax 401k money, where basis is 0 and a distribution is entirely gain;
   - after-tax contributions.

   Exchange rule: `basis(new) = basis(given) + gain realized`. `deferred` kinds
   don't realize inside and do realize on leaving. Restricted entities
   (`kind grant: restricted`) tie their money (notes: fund money stays tied;
   grant money is a subclass). Relief resolves ties first (which colors a law
   permits), then the lot policy. Only a real ambiguity is an error (notes: no
   transaction without narrowing ambiguity).
4. **Kinds type everything**: places, entities, and commodities, in one tree
   mechanism. Kinds carry typed properties (`has NAME TYPE`), defaults, and
   laws. This covers "commodities are collections of rules", "typed all the way
   down", and "no meta K/V" (unknown properties are errors with a suggestion).
5. **Laws**:
   - a trigger (`on in|out|gain|spend`, `each month|year`, `by EXPR`, `always`);
   - then `when` and `let`;
   - then `require [else EFFECT]`, `warn`, `owe … to E [by D] [as N]`, and
     `count … as N`.

   `else` is "resolve it to a loss" from the notes. Constraints are opt-in
   narrowing: `--relaxed` demotes them and `!` waives one item. Laws apply to a
   place's subtree (budgets on categories). Deadline laws fire when the journal
   reaches the date (notes: time is contingent on actions).
6. **Systems are `.ax` files** (community-maintained), and jurisdictions nest by
   path (`us/ca/san-francisco` inherits `us/ca`, then `us`). Dated `param`
   tables use a latest-≤ year lookup, with name keys and schedules for brackets.
   The Rust core knows nothing about taxes.
7. **Consequences, not features.**
   - Budgets are warn laws.
   - Taxes are tallies plus `each year` laws using `progressive(schedule, x)`.
   - Liquidity and "available to spend" run a hypothetical withdrawal through the
     laws (penalties and taxes, which is the "cost of action" in the notes), then
     add commodity or kind liquidity time.
   - The forecast combines plans, inferred recurrences, owed obligations, and
     growth models, and evaluates laws on the projection.
8. **Gaps in knowledge.**
   - `? USD` amounts are inferred from surrounding assertions (exactly one
     unknown between two assertions).
   - `-> ?` goes to the `unknown` place.
   - A failed assertion shows the gap and the flows since the last pass.
   - `=` with `!` pads explicitly from `unknown` (the one escape-hatch class).
9. **Syntax**, per the notes:
   - `AMOUNT COMMODITY`, no `$`.
   - Lowercase places and entities, uppercase commodities.
   - `#codes` instead of tags.
   - Selectors `[2024]`, `[2024-01..2024-06]`, `[#house]`, `[fifo]` on the source
     place, plus `all`.
   - `empty` instead of 0.
   - `/ payee` on an arm.
   - `///` doc comments, separate from `//` comments.
   - `...` remainder; `= X` target-balance leg.
   - `DATE..DATE` spreads recognition (amortization and cost recovery share this
     primitive with splitting, as the notes ask).
   - Price lines are `DATE UNIT PRICE UNIT`.
   - **Folder layout is a constraint**: `journal/2026/03.ax` may contain only
     March 2026. `layout free` overrides.
10. **Numbers.**
    - `Qty(i64)` counts the commodity's quantum. Precision is per commodity:
      declared, or inferred as the most decimals seen.
    - `Ratio(i64/i64)` holds law numbers and percents.
    - `mul_div` goes through an i128 intermediate with half-even rounding. i64×i64
      always fits in i128, so no bignum is needed and nothing allocates. This is
      the "efficient for small amounts" requirement.
    - A literal is capped at `Qty::LIMIT = 1e17` quanta (diagnosed at elaboration),
      leaving 92× headroom for sums. Overflow panics with a message and is
      practically unreachable.
    - Decision recorded: 18-decimal tokens beyond about 0.1 unit need
      `commodity ETH precision 9`.
11. **Time.** `Day(i32)` since 1970 uses Joffe's algorithms: 4 multiplies for
    `ymd` and an inverse without divisions. The weekday is 1 multiply plus a
    rotation table. `Span{months, days}` is a calendar duration, and ages compare
    exactly. Parsing is SWAR: validate eight digits at once and fold pairs with
    one multiply.
12. **Performance and parallelism.**
    - Parse files in parallel with `std::thread::scope` (`core::par`).
    - Split lines with `memchr` (SIMD).
    - Elaborate transactions in parallel over read-only tables.
    - The inference pass runs in parallel per place.
    - The main fold is sequential by causality.
    - Reports run in parallel per place.
    - Forecast Monte Carlo runs lanes of `[i64; 8]` so it autovectorizes.
    - There is no `Arc` or `Mutex` anywhere.
13. **Dropped:** CAS, proofs, closes, merges, phases, Datalog, unification.

## 5. Current state of `v2/`

```text
v2/
  Cargo.toml         workspace, edition 2024, resolver 3, memchr only external dep
  DESIGN.md          constitution (done)
  LANGUAGE.md        normative language spec (done — treat as the source of truth)
  PLAN.md            this file
  crates/core/       DONE and tested (8 tests pass; Joffe verified over ±2M days)
    num.rs   Qty(i64), Ratio, Dec (literal), mul_div/div_round (half-even), digits8 SWAR, Shown formatting
    day.rs   Day (Joffe ymd/from_ymd/weekday), SWAR parse, add(Span) clamping, since(), Span
    sym.rs   Interner<'s> (borrowed &'s str → Sym u32, zero copy)
    id.rs    Id<T> (typed u32 index, PhantomData<fn()->T>), Arena<T>
    glob.rs  glob(pattern, text) with * ?; covers(root, path) subtree test
    diag.rs  FileId, Loc, Severity, Diagnostic{severity, code, message, labels, notes, help}, closest() did-you-mean (OSA distance)
    hash.rs  FxHasher, Map/Set aliases
    par.rs   map, for_each_mut, join over scoped threads
  crates/{syntax,model,engine,report,systems}/src/lib.rs   placeholders
  crates/cli/src/main.rs                                    placeholder
```

Crate dependency chain: `core ← syntax ← model ← engine ← report ← cli`, plus
`systems` (data only) ← `cli`.

## 6. Interface types to write next (by the orchestrator, not the lanes)

The orchestrator owns every cross-crate type. Lanes implement functions against
these. Write them exactly this way, or improve on them deliberately.

### 6.1 `axiom-syntax` — `ast.rs` (borrowed, zero-copy)

```rust
pub struct Name<'s> { pub text: &'s str, pub loc: Loc }
pub struct File<'s> { pub id: FileId, pub items: Vec<Item<'s>> }
pub struct Item<'s> { pub doc: Option<&'s str> /* raw `///` block */, pub loc: Loc, pub kind: ItemKind<'s> }
pub enum ItemKind<'s> {
    Txn(Txn<'s>), Assert(Assert<'s>), Event(Event<'s>), Price(Price<'s>), Plan(Plan<'s>),
    Decl(Decl<'s>), Code(CodeDecl<'s>), Param(Param<'s>), Law(Law<'s>), Sync(Sync<'s>),
    Use(Name<'s>), System(Name<'s>), Base(Name<'s>), Flag(Name<'s>) /* relaxed | layout free */,
}
pub struct FlowSpec<'s> { pub from: Side<'s>, pub to: Side<'s>, pub price: Option<Amount<'s>>, pub tail: Tail<'s>, pub legs: Vec<Leg<'s>> }
pub struct Txn<'s>  { pub date: Day, pub until: Option<Day>, pub flow: FlowSpec<'s> }
pub struct Plan<'s> { pub every: Span, pub on: Option<On>, pub from: Option<Day>, pub until: Option<Day>, pub flow: FlowSpec<'s> }
pub enum On { MonthDay(u8), YearDay(u8, u8), Weekday(u8) }
pub struct Side<'s> { pub place: Option<PlaceRef<'s>>, pub amount: Option<LegAmount<'s>> }
pub struct PlaceRef<'s> { pub name: Name<'s> /* "?" = unknown */, pub select: Vec<Select<'s>> }
pub enum Select<'s> { Range(Day, Day) /* day, month, year normalize to ranges */, Code(Name<'s>), Policy(Name<'s>) }
pub enum LegAmount<'s> { Fixed(Amount<'s>), Pending(Amount<'s>), Rest, Target(Amount<'s>), All, Unknown(Name<'s> /* unit */) }
pub struct Amount<'s> { pub num: Dec /* signed */, pub unit: Option<Name<'s>> /* None only for `empty` */, pub loc: Loc }
pub struct Leg<'s> { pub doc: Option<&'s str>, pub place: PlaceRef<'s>, pub amount: LegAmount<'s>, pub price: Option<Amount<'s>>, pub tail: Tail<'s>, pub loc: Loc }
pub struct Tail<'s> { pub payee: Option<Name<'s>>, pub codes: Vec<Name<'s>>, pub waive: Option<Waive<'s>> }
pub struct Waive<'s> { pub loc: Loc, pub reason: Option<&'s str> }
pub struct Assert<'s> { pub date: Day, pub place: PlaceRef<'s>, pub amount: Amount<'s>, pub waive: Option<Waive<'s>> }
pub struct Event<'s> { pub date: Day, pub code: Name<'s>, pub state: EventState }  // Settled | Void | Returned
pub struct Price<'s> { pub date: Day, pub unit: Name<'s>, pub price: Amount<'s> }
pub struct Decl<'s> { pub what: DeclKind /* Account|Entity|Commodity|Kind */, pub name: Name<'s>, pub kind: Option<Name<'s>>, pub props: Vec<Prop<'s>>, pub laws: Vec<Law<'s>> }
pub struct Prop<'s> { pub name: Name<'s>, pub args: Vec<Expr<'s>> /* primary exprs, commas skipped */, pub loc: Loc }
pub struct CodeDecl<'s> { pub pattern: Name<'s>, pub on: Vec<Name<'s>> }
pub struct Param<'s> { pub name: Name<'s>, pub rows: Vec<ParamRow<'s>> }
pub struct ParamRow<'s> { pub keys: Vec<Key<'s>> /* Year(i32) | Date(Day) | Name */, pub value: Expr<'s>, pub loc: Loc }
pub struct Law<'s> { pub doc: Option<&'s str>, pub name: Name<'s>, pub trigger: Trigger<'s>, pub steps: Vec<Step<'s>>, pub loc: Loc }
pub enum Trigger<'s> { In, Out, Gain, Spend, Each(Period /* Month|Year */), By(Expr<'s>), Always }
pub enum Step<'s> { When(Expr<'s>), Let(Name<'s>, Expr<'s>), Require { cond: Expr<'s>, otherwise: Option<Effect<'s>>, message: Option<&'s str>, warn: bool }, Effect(Effect<'s>) }
pub enum Effect<'s> { Owe { amount: Expr<'s>, to: Name<'s>, due: Option<Expr<'s>>, name: Option<Name<'s>> }, Count { amount: Expr<'s>, name: Name<'s> } }
pub struct Expr<'s> { pub loc: Loc, pub kind: ExprKind<'s> }
pub enum ExprKind<'s> {
    Num(Dec), Pct(Dec), Amount(Dec, Name<'s>), Date(Day), Span(Span), Str(&'s str), Empty,
    Name(Name<'s>) /* identifiers, paths, globs */, Unit(Name<'s>), Code(Name<'s>),
    Field(Box<Expr<'s>>, Name<'s>), Index(Box<Expr<'s>>, Vec<Expr<'s>>), Call(Name<'s>, Vec<Expr<'s>>),
    Unary(UnOp /* Neg|Not */, Box<Expr<'s>>), Binary(BinOp, Box<Expr<'s>>, Box<Expr<'s>>),
    If(Box<[Expr<'s>; 3]>), Schedule(Vec<(Expr<'s>, Expr<'s>)>),
}
pub enum BinOp { Or, And, Eq, Ne, Lt, Le, Gt, Ge, Is, Add, Sub, Mul, Div }
pub fn doc_lines(raw: &str) -> impl Iterator<Item = &str>   // strips `///` prefixes, zero-alloc
pub fn parse<'s>(file: FileId, src: &'s str) -> (File<'s>, Vec<Diagnostic>)
```

### 6.2 `axiom-model` — `book.rs` (typed, interned, arena-backed)

```rust
pub struct Book<'s> {
    pub names: Interner<'s>,
    pub base: Id<Commodity>, pub me: Id<Entity>, pub unknown: Id<Place>, pub relaxed: bool,
    pub commodities: Arena<Commodity<'s>>, pub entities: Arena<Entity<'s>>, pub places: Arena<Place<'s>>,
    pub kinds: Arena<Kind<'s>>, pub systems: Arena<System<'s>>, pub laws: Arena<Law<'s>>,
    pub params: Arena<Param>, pub schedules: Arena<Schedule>, pub codes: Vec<CodeRule<'s>>,
    pub txns: Arena<Txn<'s>>, pub flows: Vec<Flow> /* sorted by (day, declaration order) */,
    pub asserts: Vec<Assert>, pub events: Vec<Event>, pub prices: Prices, pub plans: Vec<Plan>,
    pub syncs: Vec<SyncSpec<'s>>,
    lookup: Map<&'s str, Lookup> /* every suffix of every path → Unique(id) | Ambiguous */,
}
pub struct Amount { pub qty: Qty, pub unit: Id<Commodity> }                         // 16 bytes, Copy
pub enum Class { Asset, Liability, Income, Expense, Equity }
pub enum Sort { Place, Commodity, Entity }
pub struct Commodity<'s> { pub symbol: &'s str, pub kind: Id<Kind>, pub scale: u8, pub liquidity: Span, pub growth: Option<Ratio>, pub props: Props, pub loc: Option<Loc> }
pub struct Entity<'s> { pub path: &'s str, pub parent: Option<Id<Entity>>, pub kind: Id<Kind>, pub via: Option<Id<Place>>, pub lives: Vec<(Day, Id<System>)>, pub props: Props, pub loc: Option<Loc> }
pub struct Place<'s> { pub path: &'s str, pub parent: Option<Id<Place>>, pub class: Class, pub kind: Id<Kind>, pub owner: Id<Entity>, pub holds: Option<Box<[Id<Commodity>]>>, pub select: Option<Policy>, pub liquidity: Option<Span>, pub opened: Option<Day>, pub closed: Option<Day>, pub laws: Vec<Id<Law>>, pub props: Props, pub loc: Option<Loc> }
pub struct Kind<'s> { pub name: &'s str, pub sort: Sort, pub parent: Option<Id<Kind>>, pub class: Option<Class>, pub system: Option<Id<System>>, pub restricted: bool, pub deferred: bool, pub select: Option<Policy>, pub liquidity: Option<Span>, pub has: Vec<(Sym, Ty)>, pub props: Props, pub laws: Vec<Id<Law>>, pub loc: Option<Loc> }
pub struct System<'s> { pub path: &'s str, pub parent: Option<Id<System>>, pub laws: Vec<Id<Law>>, pub loc: Loc }
pub type Props = Vec<(Sym, Value)>;
#[derive(Clone, Copy)] pub enum Value { Empty, Bool(bool), Num(Ratio), Amount(Amount), Day(Day), Span(Span), Text(Sym), Name(Sym), Place(Id<Place>), Entity(Id<Entity>), Kind(Id<Kind>), Unit(Id<Commodity>), Schedule(Id<Schedule>) }
pub enum Ty { Amount, Num, Bool, Day, Span, Text, Name, Place, Entity, Kind, Unit, Schedule, Empty }
pub struct Schedule { pub unit: Id<Commodity>, pub brackets: Vec<(Qty /* threshold */, Ratio /* rate */)> }
pub struct Param { pub name: Sym, pub system: Option<Id<System>>, pub rows: Vec<(Option<i32> /* year */, Box<[Sym]> /* name keys */, Value)> }
pub enum Policy { Fifo, Lifo, Hifo, Prorata }
pub struct Flow { pub day: Day, pub until: Day, pub from: Id<Place>, pub to: Id<Place>, pub out: Amount, pub arrive: Amount, pub mode: Mode /* Actual|Pending|Planned */, pub infer: Infer /* Known|Target|Unknown */, pub txn: Id<Txn<'static>> /* use Id<TxnInfo> */, pub select: Option<Box<[Select]>>, pub codes: Box<[Sym]>, pub loc: Loc }
pub struct Txn<'s> { pub loc: Loc, pub doc: Option<&'s str>, pub day: Day, pub payee: Option<Id<Entity>>, pub codes: Box<[Sym]>, pub waive: Option<(Loc, Option<&'s str>)> }
pub struct Assert { pub day: Day, pub place: Id<Place>, pub amount: Amount, pub pad: bool, pub loc: Loc }
pub struct Event { pub day: Day, pub code: Sym, pub state: EventState, pub loc: Loc }
pub struct Prices { /* sorted Vec<(unit, quote, day, Ratio)>; fn rate(unit, quote, day) -> Option<Ratio>: direct, inverse, via base */ }
// Compiled laws: flat node arena so the engine can record every subexpression's value (power-assert).
pub struct Law<'s> { pub name: &'s str, pub doc: Option<&'s str>, pub owner: LawOwner /* Kind|Place|System */, pub trigger: Trigger, pub steps: Vec<Step<'s>>, pub nodes: Vec<Node>, pub loc: Loc }
pub struct ExprId(u32);
pub struct Node { pub op: Op, pub ty: Ty, pub loc: Loc }
pub enum Op { Const(Value), Var(Var), Local(u16), Field(ExprId, Field), Param(Id<Param>, Box<[ExprId]>), Call(Func, Box<[ExprId]>), Neg(ExprId), Not(ExprId), Bin(BinOp, ExprId, ExprId), If(ExprId, ExprId, ExprId), Is(ExprId, Test) }
pub enum Var { Amount, From, To, Payee, Date, Year, Month, SelfRef, Owner, Gain, Proceeds, Basis, Held, Balance, Remaining }
pub enum Func { Total(Dir, Window), Tally(Sym), Min, Max, Abs, Progressive, Value(Id<Commodity>), Date }
pub enum Field { Balance, Owner, Age, Kind, Prop(Sym) }
pub enum Test { Kind(Id<Kind>), Place(Id<Place>), Entity(Id<Entity>), Glob(&'static str /* use Sym */), Code(Sym) }
pub fn build<'s>(files: &[File<'s>]) -> (Book<'s>, Vec<Diagnostic>)   // files = project + embedded systems
impl Book { pub fn place(&self, text: &str) -> Result<Id<Place>, Vec<Id<Place>>>; pub fn value(&self, a: Amount, unit, day) -> Option<Amount>; ... }
```

Model responsibilities, in order:
1. Collect systems and resolve `use`. Jurisdictions auto-use their ancestors, and
   `lives` implies `use`.
2. Declare kinds (root kinds are built in, the rest come from used systems plus
   the project), then link parents and inherit.
3. Declare commodities. Precision is declared, or the maximum `Dec::places()`
   seen across all amounts in that unit (a pre-scan). Unknown commodities
   auto-declare as kind `commodity`.
4. Declare entities (`me` is implicit) and places. A full-path reference under a
   class root auto-opens. Build the suffix `lookup` map.
5. Type-check props against built-ins plus `has`, and compile laws (resolve
   names, infer `Ty`, reject unknown vars and functions with did-you-mean).
   Expand `budget X monthly` into a warn law.
6. Elaborate transactions (in parallel over read-only tables via `core::par`)
   into flows, following the pairing rules in LANGUAGE.md §2. Resolve `...`
   remainders statically. Check price/amount disagreement, `holds`,
   `opened`/`closed`, code placement rules, and the layout rules (§8 of
   LANGUAGE.md, using the file path, which the CLI passes in).
7. Sort flows by `(day, declaration order)`: a stable sort, so declaration order
   wins, per the notes.

### 6.3 `axiom-engine` — `run.rs`

```rust
pub struct Run { pub now: Day, pub posted: Vec<Posted> /* parallel to book.flows */, pub gains: Vec<Gain>, pub effects: Vec<Effect>, pub pads: Vec<Pad>, pub holdings: Vec<Holding>, pub diags: Vec<Diagnostic> }
pub struct Posted { pub out: Qty, pub arrive: Qty, pub state: State /* Actual | Pending | Void | Settled(Day) | Returned(Day) */ }
pub struct Parcel { pub qty: Qty, pub basis: Qty /* base-currency quanta */, pub acquired: Day, pub txn: u32, pub tied: Option<Id<Entity>> }
pub struct Holding { pub place: Id<Place>, pub unit: Id<Commodity>, pub parcels: Vec<Parcel>, pub deficit: Qty }
pub struct Gain { pub flow: u32, pub place: Id<Place>, pub to: Id<Place>, pub unit: Id<Commodity>, pub qty: Qty, pub basis: Qty, pub proceeds: Qty, pub costs: Qty, pub acquired: Day, pub day: Day, pub ambiguous: bool }
pub struct Effect { pub law: Id<Law>, pub entity: Id<Entity>, pub year: i32, pub day: Day, pub name: Sym, pub amount: Amount, pub owe: Option<(Id<Entity> /* to */, Day /* due */)>, pub cause: Cause /* Flow(u32) | Gain(u32) | Period */ }
pub fn run(book: &Book) -> Run
pub fn simulate(book: &Book, planned: &[Flow]) -> Run   // same fold with planned flows mixed in (forecast)
```

The engine algorithm:
1. **Inference pass**, in parallel per place: prefix sums of known quantities.
   Solve `?` amounts (exactly one unknown between two assertions, else a
   diagnostic listing them). Resolve `=` target legs once the prior balance is
   known (sequential per place). Apply events (settled, void, returned) to flow
   states.
2. **Main fold** over flows in order. Before each day, fire `by` and `each`
   triggers whose dates have passed. For each flow:
   - **Relief** at `from` (asset places only): pick tied colors, then apply the
     lot policy (selector > place > kind chain), ambiguity → error listing
     candidates and the gain each would realize, with a FIFO fallback so
     quantities stay consistent.
   - **Realization**: on a commodity change, on leaving the owner's asset
     places, or on deferred → non-deferred. Emit `Gain` rows.
   - **Arrival** parcels: basis rules in LANGUAGE.md §7.
   - **Totals**: maintain `(place-subtree, unit, year|month) → (in, out)` by
     adding to every ancestor.
   - **Laws**, in this order: place laws (own plus ancestors), kind-chain laws,
     residents' system laws, tied-entity `on spend` laws.
3. **Law evaluation** walks the node arena and stores each value in a scratch
   `Vec<Value>`. When a `require` fails, build the diagnostic:
   - the primary label on the flow's source;
   - secondary labels on each law subexpression, showing its value;
   - the law's doc comment as a note;
   - help: if the comparison involves `amount` or `total`, suggest the bound
     `amount − over`.

   Waived (`!`) or relaxed errors demote to warnings. `else` turns a failure into
   an effect.
4. **Assertions**: compare at end of day. On mismatch, give the difference and
   list the flows since the last pass. With `!`, emit a pad flow from `unknown`.

### 6.4 `axiom-report`

Reports return data (`Report { title, sections: Vec<Section { heading, table: Table, notes } > }`
with typed cells: amount, date, text, and delta). The CLI renders them.

- **balance**: per place and commodity, at a date, optionally valued at market,
  optionally with monthly columns. Classes are signed for display, with net worth
  and totals.
- **register**: a place's flows with a running balance.
- **flow**: an income statement by period. Spread flows are recognized linearly
  per day. Realized gains are shown under `income/gains` as a derived line
  marked `≈`.
- **available**:
  - liquid now = plain liquid places minus pending outflows minus tied money;
  - then each other place with its liquidity span and the net it would yield if
    drawn today, found by simulating a withdrawal flow through the laws.
- **budget**: every warn law with `total(in, month)`, spent versus limit.
- **tax**: tallies and owed effects per system for a year and entity. Each line
  can be traced to its causes (`why`).
- **lots**: parcels with basis and unrealized gain from the latest price.
- **forecast**:
  - plans;
  - inferred recurrences: group actual flows by (from, to, payee), need ≥3
    occurrences, a cadence from the median gap (7, 14, ~30.4, ~91, or ~365 days
    with small MAD), the median amount, and the last occurrence within 1.5
    cadences;
  - owed effects not yet paid;
  - growth models.

  Take the deterministic path through `engine::simulate` so laws run on it.
  Monte Carlo bands (p10, p50, p90) on liquid net worth come from bootstrapped
  monthly residual spending per top-level expense category, with 1000 paths in
  lanes of 8 using a seeded xorshift. Report projected law violations and
  overdrafts by date.
- **why TARGET**: for a place, its balance composition and recent flows; for
  `#code`, the linked flows and events; for a law, where it applies and its
  recent firings; for a tax line, the contributing flows and gains.

### 6.5 `axiom-cli`

- **Loading.** Find the root `axiom.ax` by walking up the directories. Load every
  `.ax` file under it, in parallel. Add the embedded systems from
  `axiom-systems` (a `pub static SYSTEMS: &[(&str, &str)]` built with
  `include_str!`); project `systems/` overrides by path.
- **Diagnostic renderer** (own implementation, no deps):
  - header `error[code]: message` in bold red (warning yellow, note blue);
  - `╭─[path:line:col]`;
  - a gutter with line numbers;
  - primary `^^^^` or `────` underlines with labels, and multiple labels per line
    laid out with `├──` / `╰──` connectors;
  - labels in other files (the law source in a system) as a second snippet;
  - `= note:` and `= help:` lines, and fix-it edits shown as a `+` line;
  - colors only when stdout is a TTY and `NO_COLOR` is unset;
  - unicode width taken as char count (ASCII-dominant).
- **Commands**: as in LANGUAGE.md §9. `check` prints diagnostics, then a summary
  line: `✓ 1,284 flows · 23 places · 4 laws enforced · net worth 184,220.13 USD`.
- **sync**: for each `sync FILE / run CMD`, spawn `sh -c CMD` (all in parallel)
  with a 60-second timeout. Parse stdout as Axiom with `axiom_syntax::parse`. If
  it has no errors, write the file atomically (tmp file + rename). Report per
  file.
- **Exit codes**: 0 clean, 1 errors present, 2 usage error.

### 6.6 `axiom-systems` (Axiom source, embedded)

Files: `std.ax`, `us.ax`, `us/401k.ax`, `us/ira.ax` (traditional and Roth
kinds), `us/529.ax`, `us/hsa.ax`, `us/ca.ax`, `us/ny.ax`, `us/ny/nyc.ax`,
`us/ca/san-francisco.ax`.

- **`std`**:
  - place kinds: `cash bank broker property vehicle credit-card loan mortgage
    wages interest dividends gains education`;
  - commodity kinds: `currency security stock fund bond crypto real-estate good`;
  - entity kinds: `person (has born date, filing name) org employer government
    grant (restricted; has purpose kind, until date)`;
  - common ISO currencies with precision (JPY 0, KRW 0, BHD/KWD 3);
  - `bank` gets a warn law `always balance >= empty`;
  - `loan` gets `has rate percent, maturity date` and the law
    `by self.maturity require balance == empty`;
  - `grant` gets `on spend require to is self.purpose` and
    `by self.until require remaining == empty`;
  - `broker` gets `select fifo`.
- **`us`**:
  - entity `irs : government`;
  - params for 2024–2026 (**verify every number against the IRS before
    shipping; mark them as community data**):
    - ordinary brackets for single and joint (2026 single: 0 10% | 12,400 12% |
      50,400 22% | 105,700 24% | 201,775 32% | 256,225 35% | 640,600 37%);
    - standard deduction (2025: 15,750 single and 31,500 joint after the 2025
      law; 2026: 16,100 and 32,200);
    - long-term capital-gains brackets (2025 single: 0% to 48,350, 15% to
      533,400, then 20%; 2026 single: 0% to 49,450, 15% to 545,500);
  - laws that count wages, interest, dividends, short-term gains, and long-term
    gains (`on gain`, `held > 1y`, not in a `deferred` place);
  - an `each year` law that owes federal income tax, due April 15 of the next
    year, via `progressive`.
- **`us/401k`**:
  - `kind 401k : asset`, `deferred`, `has employer entity`;
  - params `limit` (2025: 23,500; 2026: 24,500) and `catch-up` (2025: 7,500;
    2026: 8,000; ages 60–63 in 2025+: 11,250);
  - laws:
    - deferral limit: `on in when from is wages require total(in, year) <=
      limit[year] + (if owner.age >= 50y then catch-up[year] else empty)`;
    - early withdrawal: `on gain when not to is 401k|ira require owner.age >=
      59y6m else owe 10% * gain to irs as early-withdrawal-penalty`;
    - distributions count as ordinary income;
    - RMD at age 73 (warn).
- **`us/ira`**: traditional and Roth limits (2025: 7,000; 2026: 7,500;
  catch-up 1,000/1,100). For Roth, qualified distributions (age 59½ and a
  5-year span) are tax-free, and otherwise earnings (gain) are taxable plus 10%.
- **`us/529`**: `kind 529 : asset`, `deferred`, `select prorata`,
  `has beneficiary entity`. Non-qualified withdrawals (`on gain when not to is
  education|529`) count the gain as ordinary income and owe 10% of it. Warn when
  contributions exceed the gift exclusion (19,000 per year in 2025–2026).
- **`us/hsa`**: limits (2026: 4,400 self, 8,750 family), and qualified-medical
  checks.
- **`us/ca`**: state brackets (sample), and state wages from federal tallies
  through ancestor lookup.
- **`us/ca/san-francisco`**: an illustrative city rule, so the three-level
  jurisdiction chain is demonstrated.

### 6.7 Examples (`v2/examples/`)

- `01-first-steps.ax`: three lines, no declarations beyond one account.
- `02-household/`: a project folder with `axiom.ax`, `accounts.ax`,
  `journal/2026/01.ax` … `03.ax`, `prices/2026.ax`, and `plans.ax`. It should
  exercise:
  - a paycheck split with a 401k deferral and `...`;
  - rent via an entity;
  - a credit card and its payment;
  - a budget;
  - a pending check with a `settled` event;
  - a brokerage buy and a partial sale with FIFO;
  - an assertion with an inferred `?` ATM amount;
  - a 529 contribution and a non-qualified withdrawal that fires the penalty;
  - a grant with restricted money;
  - a `!` pad;
  - a spread insurance premium;
  - plans for the forecast.
- `03-violations.ax`: a 401k over-contribution, an early withdrawal, an
  ambiguous lot without a policy, a typo'd property, an unknown place (with a
  did-you-mean), and a price/amount disagreement. Each should produce a
  gorgeous diagnostic.

## 7. Execution plan (orchestration)

1. **The orchestrator writes all interface types** (§6.1–6.5 signatures and
   stubs using `todo!()`). The workspace must `cargo check` clean.
2. **Lanes** (Sonnet sub-agents; run them in parallel, each in its own scratch
   copy of `v2/`, touching only its own crate; the orchestrator copies accepted
   crates back):
   - A: syntax (lexer with memchr and a Pratt expression parser);
   - B: model;
   - C: engine;
   - D: systems plus examples (the `.ax` sources);
   - E: CLI plus the renderer plus sync;
   - F: report plus forecast.

   Each lane brief contains:
   - LANGUAGE.md, DESIGN.md, and the relevant part of §6;
   - the code-quality rules (below);
   - "keep your crate compiling; don't edit shared types; request changes in
     your final report".
3. **Review brutally.**
   - Read every file. Reject clones where a borrow works, `String` where
     `&'s str` or `Sym` works, stringly-typed dispatch, needless `Box`/`Vec`,
     duplicated logic, and `Rc`/`Arc`/`RefCell`/`Mutex`.
   - Reject functions over about 60 lines without reason, and comments that
     narrate.
   - Rework abstractions when two lanes needed the same helper.
   - Iterate with each lane (SendMessage to continue it) until the code is the
     best version.
4. **Integrate**:
   - `cargo check --workspace`;
   - run the CLI on the examples;
   - fix the semantics until every example behaves as described;
   - look at the rendered diagnostics and polish them until they are beautiful.
5. **Measure the size**: count non-comment, non-test lines per crate. The
   targets below are directional, not quotas (§8).
6. **Measure performance**: generate a 1M-flow synthetic journal and time
   `axiom check` (release). Parse plus model plus engine should take well under
   a second on 11 cores.

### Code-quality rules for every lane

- Readable first: descriptive names, small functions, early returns, match on
  enums, no macros unless they remove real repetition, and no golfing.
- Borrow the source (`&'s str`), intern names (`Sym`), and index with typed
  `Id<T>`. Nothing allocates per number or per date.
- Use `core::par` for parallelism. No `Arc`, `Mutex`, `Rc`, or `RefCell`.
- Errors are `Diagnostic`s with labels, notes, and help. Never `panic!` on user
  input. Report as many independent errors as possible (recover per item).
- Comments explain *why*, not what. Doc comments on public items.
- Stable Rust only. External dependencies: `memchr` only (discuss others first).

## 8. Size budget (directional)

| crate | target lines (no tests/comments) |
|---|---|
| core | ~700 (done: roughly this) |
| syntax | ~1,300 |
| model | ~1,600 |
| engine | ~1,500 |
| report | ~1,100 |
| cli | ~1,000 |
| **total Rust** | **~7,000** versus 71,000 in v1 |
| systems (`.ax`) | as needed; it is data |

## 9. Open questions and interpretations made

- "Linking accounts to more than one place at creation": interpreted as joint
  ownership. Deferred; each place has a single `owner`. Ask the user.
- "Folder structure enforced by default": implemented as both layout date rules
  and class roots for undeclared paths.
- The meaning of "soft transactions" (`(1000 AAPL @ 11 $)`) was unclear;
  interpreted as pending/maybe. Settlement goes through `#code settled`.
- "Some kind of language like Lean" for complex attributes: out of scope now.
  `sync` scripts cover external computation, and laws cover in-language rules.
- Tax figures are illustrative and must be verified. Present them as
  community-maintained data.
- Weekday uses Joffe's MIT `get_weekday_32unix`. The date algorithms are BSL-1.0
  and attributed in `day.rs`.

## 10. Decisions made while writing the interfaces

The interface types in `crates/*/src` are now the contract; §6 above is the
earlier sketch. Where they differ, the code wins. These are the rules the
lanes share.

**Data structures.**
- Every hierarchy (places, entities, kinds, systems) is a `core::Tree`: ids
  are assigned in pre-order, so a subtree is the id range `id..end(id)`.
  `covers(a, b)` is two comparisons; `lineage(id)` walks up. The model sorts
  inputs by path segment before building so siblings come out alphabetical.
- `core::Groups<K, V>` is a compressed-row table (one flat vector plus offsets,
  built by a stable counting sort). Governance (`Rules`) and `Book::touching`
  (flows per place) use it.
- Model types carry no lifetimes: names are `Sym`, docs are `Sym`. Only
  `Book<'s>` does, through its interner.
- Expressions are post-order arenas in both the AST (`Exprs`, per file) and
  compiled laws (`Law::nodes`, per law), one compiled node per source node.
  Every node stores the first node of its subtree, so a step's expression is
  the range `first..=root`. Type checking and evaluation are single forward
  scans; the engine keeps every node's value, which is exactly what the
  power-assert diagnostic prints.
- Evaluation is total. Faults (missing price, unset property, missing param
  row, divide by zero, overflow) are `Value::Fault` and propagate; `if`,
  `and`, `or` select among already-computed values, so a fault in an unused
  branch is never seen. A fault reaching a step is reported as its own error.

**Semantics.**
- `?` as a place is the built-in `equity/unknown`. Reports call it
  "unexplained": value into it is unexplained spending, out of it unexplained
  income.
- Balances are inflow minus outflow. `Class::display_sign` flips income,
  liabilities and equity for display.
- `total(dir, window)` is kept in the **base currency**, valued on the flow's
  day, per place, and added to every ancestor. A flow whose both ends lie in
  the subject's subtree does not count as entering or leaving it, and does not
  fire the subject's `on in`/`on out` laws.
- Tallies are keyed by `(owner, year, name)`: one namespace per person-year,
  so a tally is a line on that person's year. Systems name their lines by
  convention (`wages`, `agi`, `federal-withheld`, `ca-withheld`, …).
  `Effect::system` still records which system counted a line, for grouping.
  Per-person limits are tallies, not window totals: `count amount as
  elective-deferrals` then `require tally(elective-deferrals) <= limit[year]`
  is exactly IRC §402(g) (across every plan a person holds, employer match
  excluded), which `total(in, year)` on one account cannot express.
- `year` and `month` are available in every law context, from the context day.
- Param keys: a year `2026` means "since 2026-01-01"; a date key means since
  that day. A lookup with a number `N` asks for N-01-01; with a date, that day.
  The latest row at or before wins, among rows whose name keys match.
- **Plain money** is base currency, in an asset place, with basis equal to its
  face and no tie. It is one signed integer (`Holding::plain`) and it never
  realizes: its gain is zero by definition, so it produces no `Gain` and fires
  no `on gain` law. Everything else is a `Parcel` in `Holding::lots`.
- **Merging is interchangeability.** Lots merge exactly when they are
  interchangeable: for the base currency, the same tie and the same basis per
  unit (so a 401k's zero-basis deferrals are one lot); for other commodities,
  the same `(acquired, txn, tied)` (each purchase stays its own lot).
- **Relief** at an asset place (plain money is a candidate too): (1) apply
  selectors; (2) order by colors: if the flow is one a tied entity's `on
  spend` laws permit, its tied parcels go
  first, otherwise plain and untied parcels go first and tied ones last; (3)
  apply the policy (selector, else place, else kind chain). Candidates are
  *interchangeable* when they agree on basis per unit and tie, and, for
  non-base commodities, on acquired day. Only relief among
  non-interchangeable candidates with no policy is ambiguous: it is an error
  listing each candidate and the gain it would realize, and FIFO is used so
  everything downstream stays consistent.
- **Realization** happens when parcels change commodity, leave the owner's
  asset places (to an expense, income, equity, liability, `?`, or another
  entity's place), or leave a `deferred` place for a non-deferred one. It never
  happens between two deferred places or within one owner's taxable asset
  places. Exchange rule: `basis(new) = basis(given) + gain realized`.
- **Arrival basis**: from income, equity or `?`, face value in base (valued on
  the day), or zero if the target is `deferred`; with `@ P`, `P × qty`. Tied
  to the source entity when its kind is `restricted` (the source place's
  owner, or the payee).
- Law firing order for a flow: `on out` rules at `from`, relief and `on gain`
  per relieved parcel, arrival, `on in` rules at `to`, `on spend` rules of any
  tie that left, then `always` rules at both ends.
- Governance is precomputed in `Rules` and dated by residence (`lives X from
  D`), so the engine filters by `from..=until` and never searches.
- `budget 500 USD monthly` on an account compiles to a place law
  `on in` + `warn total(in, month) <= 500 USD`, named `budget`, with every node
  located at the property.
- Assertions are written in the place's display sign: `visa = 1_234.56 USD`
  means 1,234.56 is owed.
- A project may declare `entity me : person` to set the built-in `me`'s kind
  and properties.

## 11. The v3 rework (in progress)

Two exploration lanes produced the direction:
- `examples/FINDINGS.md`: seven realistic ledgers (examples 04–10) and 36
  findings, with "the five deepest problems".
- `tests/mistakes/REPORT.md`: 99 graded mistakes, the ideal diagnostic for
  each, and a diagnostic style guide (§14).
- `bench/REPORT.md`: benchmarks from 10k to 5M flows, pathologies and hotspots.

`LANGUAGE.md` and `DESIGN.md` were rewritten to answer them (commit 2009dda).
The new concepts, each chosen because it dissolves several findings:

| concept | dissolves |
|---|---|
| a flow's recognition period (`DATE..DATE`, `for PERIOD`), and `each year closing MM-DD` | F07, F18, F26 |
| `for #code` settles, `due` makes a claim, `claim` kinds keep parcels apart | F06, F15, F32, F35 |
| `for ENTITY` ties money (envelopes, deposits); `for` the owner unties | F16, F10 |
| basis as a dimension: `basis zero\|cost` kinds, `basis` tails, `PLACE.basis` flows | F01, F24 |
| `split`, `opening`, assertion `via PLACE`, `market` revaluations | F02, F03, F17 |
| households (`member`), overlapping residences with ends | F05, F09, F21 |
| law order by tally dataflow; headroom on every comparison | F08, F19, constraint surfacing |
| named plans instantiated by the journal | authoring, F11, F13 |
| aliases, multi-entity decls, `on in from X`, `all UNIT`, closing-statement headers | F25, F31, F33 |

Deliberately *not* done: self-posting plans (the notes: nothing is generated
into the journal; a named plan makes each occurrence a one-line fact instead),
expressions in flow amounts, import rules (a `sync` script's job), loans that
write their own flows.

**Waves.** The orchestrator wrote the cross-crate types with defaults that
keep today's behaviour (commits 2009dda and the next), so every lane starts
green.
1. **Wave 1, in parallel.**
   - syntax: compact AST, new surface, SWAR, in-file parallel parse;
   - engine: performance, and the new semantics against the new `Book` fields;
   - report: the new views, and the forecast and available fixes;
   - cli: the diagnostic style, new commands, parallel loading.

   Engine, report and cli merge as they finish. Syntax waits for the model.
2. **Wave 2.**
   - model: rebuilt on the new AST, filling every new field;
   - then systems and examples: `std`/`us` fixed and extended, and examples
     04–10 rewritten with the new features, their numbers verified.
3. **Integration.** Goldens, benchmarks against `bench/REPORT.md`, and the
   mistakes corpus regraded.

Targets: every example and golden keeps working or improves (each diff
justified); 1M flows `check` well under 1 s on 4 cores; no quadratic path;
total Rust (non-test, non-comment) down from 17.1k toward 11k while the
language grows.

### Wave 1, as it landed

| lane | merged | lines (target) | what it bought |
|---|---|---|---|
| cli | a2a883c | 1,750 (1,500) | one report per cause, the reader's file first, new commands, parallel loading |
| report | b0e0517 | 3,650 (2,900) | `Lens` (whose, worth, liquidity), limits, claims, gains, `--for`, forecast and available rebuilt |
| engine | 617677b | 3,844 (3,200) | the v3 fold, relief without scanning (400k trades: 13 s → 0.9 s), headroom, per-window reporting |
| syntax | on `lane-m` (169cbd5) | 3,074 (2,300) | 40-byte items, per-piece tables, parallel parse (64 MB: 0.58 → 0.20 s), the whole v3 surface |

Every lane missed its size target, by the new semantics and the diagnostics
the style guide asks for. Each overrun was reviewed and accepted: nothing was
found that a better formulation would remove without dropping a feature.

Syntax cannot land on main until the model is rebuilt on its AST. The model
lane works on branch `lane-m`, which is main plus lane S, and merges main as
the other lanes land.

### Decisions taken while merging

- **A deferred kind with no written `basis` has `Basis::Zero`.** This is the
  old engine's rule, now in the model. The systems will say `basis zero`
  explicitly, and the default stays for projects that declare their own
  deferred kinds.
- **A violated law is reported once per subject and window, as LANGUAGE says.**
  Three missed months are three warnings, each naming its month; they are no
  longer folded into "2 more like this", which hid which months.
- **The priced headline:** "34.94 USD owed to irs as nonqualified-529-penalty,
  due 2026-03-25". A waived one says "waived: … would be owed …", never "owed".
- **Power-assert:** a comparison shows its two sides. Any other condition
  shows each fact it read once, as a whole (`owner.age`, not `owner` and then
  its age), and never the booleans that combine them.
- **Headroom is the engine's.** The report no longer re-reads flows. A budget
  that no flow reached in the current window reads as nothing spent against
  its written limit.
- **`limits` lists limits, not invariants.** A floor of nothing
  (`balance >= empty`) is left out; a real floor (a minimum payment) stays.
- **`gains` lists what realized something.** Money leaving at its own basis
  (a grant spent, a deposit returned) is not a disposal.
- **`available` treats a priced violation as a cost, not a block.**
