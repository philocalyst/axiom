# Axiom v4: what remains

> Local continuation, 2026-10-01: this checklist is being completed on
> `cutover/promote-workspace` in the isolated `axiom-local` checkout. The
> historical branch status and benchmark figures below are preserved evidence,
> not results from the new implementation. Current requirements, reviewed
> checkpoints and verification are recorded in
> [CUTOVER.md](../docs/local-rework/CUTOVER.md). Native model and journal wiring,
> integrated execution, corpus migration, root promotion and legacy removal
> remain pending. Completion will be recorded only after the checks pass.

As of 2026-09-30. This is the brief for whoever continues the v4 rework, whether a person or a session of agents. Read it first. Then read:

- [PLAN.md](PLAN.md): history, motives and every decision so far;
- [DESIGN.md](DESIGN.md) and [LANGUAGE.md](LANGUAGE.md): the constitution and the normative spec;
- [`briefs/`](briefs/README.md): the lane briefs, audits and research this document points into. Where a brief and the spec disagree, the spec wins.

---

## 1. Where everything is

Every branch with work on it is pushed.

| remote branch | head | what it holds | state |
|---|---|---|---|
| `claude/great-wozniak-pnqn7x` (**main**) | `c488ad7` | The v3 toolchain end to end; the v4 public types, with the v3 model bridged onto them (lanes I4, C5); the engine restructure (E4a); five v4 ledgers (X5); the spec at its third pass (`5f15e35`) | **Green:** 399 tests, goldens and the mistakes corpus byte-identical |
| `claude/great-wozniak-pnqn7x-v4` (local `v4`) | `455f819` | main at `e643ea5`, plus the v4 syntax (lanes S4 and S5) and the spec decisions of `36972a0` | **Syntax green:** 81 tests, and the whole v4 sketch parses. Model, engine, report and cli **do not build**, because the model still reads the v3 AST. This is expected until lanes M4a and M4b land. |
| `claude/great-wozniak-pnqn7x-report` | `9debdf9` | Lane R4, from main `e643ea5`: phase 1 and part of phase 2 | Green at `9debdf9`. **Not merged.** Its merge of `c488ad7` stopped mid-conflict and was not saved: redo it (§4.2). |
| `claude/great-wozniak-pnqn7x-sync` | `75325d8` | Lane SY, from main `26a396c`: the `sync` crate's pure layer | Green on its own base. **Not merged:** the permission classifier denied merging it, in both directions. **The user decides** (§4.1). |

Not pushed, and not needed:
- **A stash entry, `e4a-wip-plan-2026`.** It is obsolete, since E4a is merged. The lane was denied dropping it, so it was left for the user.
- **Superseded local lane branches.** `worktree-agent-a5e8d45…` is already in `v4`. `worktree-agent-a7fde93…` is in main as rewritten copies.
- **The benchmark projects.** Regenerate them with `python3 bench/gen.py` (seeded and deterministic: see `bench/REPORT.md`). The 100k and 1m scales are the ones every performance number below uses.

The session scratchpad held the lane briefs, audits and research notes. They are copied into [`briefs/`](briefs/README.md), with their paths rewritten to the repo.

## 2. What v4 is, in one paragraph

v4 replaces the chart of accounts with **agents, resources, events and promises** (DESIGN §1). There are no income, expense or equity accounts:
- **Accounts** exist only where money sits.
- **Parties** are the other end of flows, and typed parties imply their **purposes** (a tree rooted at income, spending, capital and transfer).
- **Assets** are identified things whose parts (purchase, improvements) are their basis.
- **Contracts** are promises, with terms that change by **statements** in the journal. **Claims** live on parties.
- Implied costs (sales tax, exchange cost, shares, escrow, matches) are **derived** and visible.
- **Laws are norms**: `unless`, `else` chains, `overrides`, and ranks.
- **Amounts carry units.**
- **Sync** recognizes and reconciles; it never imports blindly.
- **Dates are short** where the file's place says the rest.

`examples/v4-sketch/` is the worked example, and its README gives the target outputs.

## 3. Lane status

| lane | scope | state | brief |
|---|---|---|---|
| I4 | first v4 public types, v3 model bridged | **done**, main `e14bda3` | — |
| C5 | core calendar (`Days`, `Window`, `spread`, `due`), `Timeline`, typed `Run`; units (`Dim`); the public types for terms, promises, measures, `also`, norms, filed returns, sync | **done**, main `c7b352a` and `e643ea5` | — |
| X5 | five ledgers written against v4; twelve proposals | **done**, main `3514632`; folded into the spec at `5f15e35` | — |
| E4a | engine structure: `Plan`/`Ledger`, `LawFacts`, `Verdict`/`Consequence`, `Known`, the v3 bridge in one module, checkpoints | **done**, main `c488ad7` | — |
| S4, S5 | the v4 syntax: statements by verb, amounts and references, contracts, laws, sources, patterns, formats, `axiom_syntax::format` | **done**, `v4` `32826f8` (merged with main at `455f819`) | [notes-S5-syntax.md](briefs/notes-S5-syntax.md) |
| R4 | report and cli: views as data, `--json`, `check --json`, `fmt`, the v4 views | **partial** (§4.2) | [lane-R4-report-cli.md](briefs/lane-R4-report-cli.md) |
| SY | the `sync` crate | **pure layer done, not merged** (§4.1) | [lane-SY-sync.md](briefs/lane-SY-sync.md) |
| M4a | model: declarations, names, kinds, places, purposes, props, laws, units, norms, `also`, budgets, sources, patterns, std in v4 | **not started**. It was launched, then stopped by a container restart before its first commit. | [lane-M4a-model-declarations.md](briefs/lane-M4a-model-declarations.md) |
| M4b | model: the journal (flows, items, statements by verb, references, contracts, claims, assets, shares) | not started; after M4a | [lane-M4b-model-journal.md](briefs/lane-M4b-model-journal.md) |
| E4b | engine semantics: purposes, budgets, props by day, assets and parts, `carry`, parties and tabs, claims, promises as one monitor, `also`, measures, `against`, owner sets | not started; can start now, from main | [lane-E4b-engine-semantics.md](briefs/lane-E4b-engine-semantics.md) |
| E4c | engine: units and conversions, norms (`unless`, `else` chains, ranks), new functions, filed returns, diagnostics that use the whole architecture | not started; after E4b | [lane-E4c-engine-units-norms-diagnostics.md](briefs/lane-E4c-engine-units-norms-diagnostics.md) |
| SY2 | bind sync to the book (§4.7) | not started; after M4a and the SY decision | this document, §4.7 |
| Y4-A, Y4-B | `us` and every example in v4; the mistakes corpus | not started; after M4b, with E4b, E4c and R4 in `v4` | [lane-Y4-systems-examples.md](briefs/lane-Y4-systems-examples.md) |
| integration | `v4` into main; the v3 bridge deleted; goldens, verify scripts, mistakes, benchmarks | not started; last | this document, §4.9 |

## 4. The remaining work, in order

```text
now ──┬─ decide SY (user) ───────────────────────────────┐
      ├─ R4 finish (main) ───────────────┐               │
      ├─ E4b (main) ── E4c (main) ───────┤               │
      └─ M4a (v4) ── M4b (v4) ──┬────────┴── merge main into v4 ── Y4-A ∥ Y4-B ── integration: v4 into main
                                └── SY2 (v4) ─────────────────────┘
```

Three lines can run at once today: R4, E4b and M4a. Merge main into `v4` after each main merge that `v4` needs (R4, E4b, E4c), so the model lanes and Y4 see the real engine and views.

### 4.1 Decision: the sync branch (the user's)

The SY lane's attempt to merge main into its branch was denied twice by the auto-mode permission classifier. The orchestrator's attempt to merge SY into main was denied as the same outcome, and was aborted, leaving main untouched. Nothing in this document works around that.

The user chooses:
- **(a)** Allow the merge. Then merge `claude/great-wozniak-pnqn7x-sync` into main, resolving `cli/src/sync.rs` against C5's `axiom_model::sync::{Fetch, Sink}`: the declared sources are `book.sources`, not `book.syncs`.
- **(b)** Leave it on its branch. SY2 (§4.7) then folds it into `v4` when the model's formats and patterns are real.

What the branch has:
- CSV, and one tagged reader for SGML-OFX, OFX2 and camt.053, driven by declared formats, with diagnostics at the cell.
- A PEG matcher over flat programs, dispatched through a trie of first literals.
- Recognition: longest literal wins, and a tie is an error. `payee:` recognized in turn gives `via`. It also handles code, amount and date captures, and every name known by itself.
- Reconciliation: same amount within three days. It covers legs, batches and derived flows, and a document outranks a bank line.
- Promises kept, claims by code, and gross and fee.
- Writing in day order into the file a day belongs to, with short dates.
- File and param sinks, `--dry` as a diff, and idempotence.
- Commands in parallel with `{since}`, `{today}`, `{units}` and `{year}`.
- `unrecognized()` grouping for `check`.
- `crates/sync/std/formats.ax`, with `format ofx` and `format camt053`.

Tests: 96 unit and 7 end-to-end. The size is 3,032 non-test lines, more than three times its ~900 target.

| measurement | time |
|---|---|
| CSV, 1M rows | 0.50–0.69 s |
| OFX, 100k records | about 107 ms |
| Recognition, 1M memos × 200 patterns | 268 ms |
| Reconcile, 100k records vs 1M flows | 225–267 ms |
| Full feed, 100k vs a 1M-flow account | 457–541 ms |

### 4.2 R4: finish the report and cli (main)

**Done on `claude/great-wozniak-pnqn7x-report`:**
- One `Priced` tally and `Section::total` (`ea499ef`).
- A `Lens` on a `Context`, with `Sides` and well-known names resolved once (`8fa49c6`).
- Sparse `Snapshots` (`fb8e2ff`).
- Views as data: typed cells, sentences of cells, and XBRL-shaped facts (`81d72ba`).
- `--json` on every view, and `check --json` with fixes as edits.
- `axiom fmt [FILE…] [--check]`, wired end to end but on a stub layout (`dcf9913`).
- `contracts`, late promises in `claims` and `available`, and itemized claims (`85352ce`).
- `flow` by the purpose tree or `--by party`, with measures (`9debdf9`).

**Remaining:**
1. **Redo the merge of main `c488ad7`.** It conflicts in `report/src/{available,claims,forecast,headroom}.rs`. Then:
   - **There must be one `Sides` and one `Known`, not two.** R4 built its own in the report's `Context` (`8fa49c6`), while E4a built `bridge::Sides` and `plan::Known` in the engine. Keep the engine's and have the report read them through the plan. Expose `Sides` from the engine if needed; it is a one-line change.
   - **Fork, don't re-fold.** `available`, `claims::holdings_at` and `forecast/projection` fork from one `Plan::run_with_view(options) -> (Run, Ledger)` instead of folding again. A view that judges a later year calls `ledger.reach(horizon)`.
   - **Adopt `Headroom.bound` and `is_floor(reading)`,** as E4a changed them.
2. **The rest of phase 2,** built on fixtures, as the brief lists:
   - `balance` by owner: accounts by institution, own holdings, assets at price or basis, claims held, and debts including loan contracts' debts.
   - `register` for owners, parties, assets and contracts, with pass-through halves and shares shown once, and documents marked.
   - `why` for a contract, an asset, `#purpose`, `^code`, `"text"`, `FILE:LINE` (provenance and derivations) and any declared thing's changes.
   - `budget` and `limits` on purposes: limits over time, `carries` as a running balance, and a purpose law's subject being the owner.
   - `forecast` from contracts: `due_days`, the terms in force each day, `amount_on(day)` for escalations, loans' split, habits only where no contract covers, and "looks like a contract: declare it?".
   - `available` as the brief defines it.
   - The v4 additions: measures, units such as `USD/MI` shown as written, `also`-derived flows shown once with the flow that implies them, and `tax` with a filed return beside the book and amendments marked.
3. **Wire `axiom fmt` to `axiom_syntax::format(src, &File)`.** That formatter exists only on `v4`, so this lands when `v4` merges, or by merging main into `v4` and wiring it there.
4. **`check` lists unrecognized memos,** grouped, with the `known-as` line that would recognize each, through `axiom_sync::unrecognized`. This waits on §4.1.
5. **Size.** The brief allowed +700 net lines, and the branch is already at +1,011 (report 4,389, cli 2,115). Pay it back where v3 goes: `// v3 bridge:` code, plans in `forecast/expected.rs`, and `.basis` ends in `history.rs`, `register.rs` and `places.rs`.

### 4.3 M4a: model declarations (v4 branch)

Brief: [lane-M4a-model-declarations.md](briefs/lane-M4a-model-declarations.md), including its two revision sections. Read [notes-S5-syntax.md](briefs/notes-S5-syntax.md) for the AST it builds from.

In short:
1. **Delete the v3 model and its bridge** (audit finding 1: `PathRoot`, `v3_root`, plans, `basis_end`, path-root places, positional kind constants), and read the v4 AST directly.
2. **One name lookup,** `Scoped<T>::lookup`, and one `unresolved` builder. A name in two namespaces is an error, with one exception: a contract may share its party's name.
3. **Kinds as `Traits` variants,** with named `KindRoots`.
4. **Places:**
   - accounts with institutions;
   - owners' holdings;
   - parties' outside places;
   - tabs on demand, through one function M4b calls;
   - one place for each asset's unit.
5. **Owners,** households, `owner A 60%, B 40%` (`owned_by`, `Place.shares`), `currency`, `citizen`, and `books cash|accrual`.
6. **The purpose tree** with four roots (income, spending, capital, transfer):
   - `of KIND`, and `business N% for OWNER`;
   - kinds' `purpose`, `pays` and `takes`;
   - one `infer(flow facts) -> Option<Purposed>` in LANGUAGE §3's order, with `Provenance`, where a conflict names both sources;
   - the query M4b needs for "between two owners, a flow needs a transfer purpose".
7. **Properties:**
   - built in `Vec`s and frozen once;
   - kind defaults applied by reference;
   - rows that carry `since`, with `set(thing, prop, days, value, change)` for M4b;
   - where a line was written kept inline, as `At<T>`.
8. **Laws:**
   - triggers spelled once (`Moment`);
   - the IR as an `Arena<Node>` whose poisoned node is `ty: None`;
   - `Require` with a `Severity` and an `otherwise` chain;
   - one `FUNCS` table;
   - `on flow` (implicit in a purpose), `consume`, `carry`, `Var::Purpose` and `Var::Description`, `purpose is P of X`, the asset fields, `straight-line`, `total(#P, window)`, `days`, `peak`, `low`, `open`, and `up to`;
   - governance by purposes (ancestors included), assets and asset kinds, and contracts;
   - `overrides`;
   - `Law.rank` by specificity (thing > kind > parent kind; project > child system > parent system), where two of equal rank is an error. This replaces file shadowing.
9. **Units** (`Ty::Amount(Dim)`), checked bottom-up:
   - params and properties carry units;
   - `amount` is the subject's commodity;
   - `+`, `-` and comparisons need equal dimensions;
   - `*` and `/` use `Dim::mul` and `div`;
   - `value(x, U [at POLICY])` is the only conversion;
   - a mismatch names both units and gives the conversion as the fix.
10. **`also`** compiled and indexed by `AlsoOn`, for the engine to evaluate.
11. **Budgets:** `budget PURPOSE LIMIT period [carries] [funded from H into H]`, and `Limit::Share`.
12. **Sources and formats.** The brief's revision 2 fixes the shape:
    - `Format { shape: Rows | Tagged { records }, specs: [Spec { field, places, layout: Option<DateLayout>, rule: None | Flipped | Sign { place, into } | Is }], categories }`;
    - `DateLayout` goes in `axiom_core::calendar`;
    - every declaration error is caught at build time.

    Drop `Csv` and `Sink::Feed`'s `csv`. **Patterns** are lowered to flat `Op` programs in the exact encoding the brief gives (`A / B / C` is `Choice A Choice B C`), so the matcher can run them as written.
13. **Dates by convention** (LANGUAGE §11): delete every date-versus-path check. Mistakes cases 93, 96 and 99 become non-errors; leave a note for Y4-B.
14. **`std` in v4:**
    - the purpose tree, with transfer purposes;
    - party kinds with `purpose` and `pays`;
    - account, asset and commodity kinds, including the measure units;
    - `format ofx` and `format camt053`;
    - envelopes and grants;
    - `escrow` and `match` as purposes, since contracts write them as `also` lines.

**Also, from C5:**
- delete `Contract.matching` and `Match`;
- make waiving an explicit state on `Terms`: `is_waived()` today is `template.is_empty()`, which reads a loan with no template as waived.

**Verify:**
- the sketch's declarations build with std and a minimal `us` stub;
- the refusal table: a name in two namespaces, a purpose that looks like a code, a missing object kind, a budget on an unknown purpose, a malformed format, and `known-as` on a kind.

**Handoff:** write `briefs/notes-M4a.md`, the handoff M4b's brief expects: the functions M4b calls, the `World` it extends, and what is stubbed.

### 4.4 M4b: the journal (v4 branch, after M4a)

Brief: [lane-M4b-model-journal.md](briefs/lane-M4b-model-journal.md), with its revision section.

In short:
1. **Flows:**
   - ends across namespaces; a loan contract as an end (its debt);
   - an asset written as an end is an error whose fix is `#purchase of`;
   - `flow.owner`;
   - party-to-party legs split through the owner (`PassThrough`);
   - `via`;
   - the `for` table (period, envelope, party arriving, owner release, `PaidFor` leaving, another owner);
   - `due`;
   - `against`.
2. **Line items:**
   - carved items become their own flows;
   - `+` items are additional flows;
   - `-` items are counter-flows, or shrink the header;
   - `N% of` is rounded so the total is exact;
   - a trade-in disposes of the asset;
   - an item in another unit is its own exchange.
3. **Purposes:** call `infer` and record provenance; two sources that disagree are an error.
4. **References:** `^code` and `NAME`, with selectors (`[#purpose]`, `[END]`, `[^code]`, `[UNIT]`, `[DATE]`), resolved in dependency order. A forward reference or a cycle is an error. Record `Detail.reckoned` so `why` shows the arithmetic.
5. **Derivations:**
   - shares, by percent, fraction, or measure against `area`;
   - `part of` assets divided by area;
   - owner sets;
   - sales tax, which stays in cost on capital;
   - exchange cost;
   - one book-wide codes table, with each flow holding a `Run<Sym>` into it (audit finding 11).
6. **Contracts:**
   - `Timeline<Terms>` painted from statements: new terms, `waived`, properties, `until`, `ends`, and `^code` changes;
   - inputs bound by `NAME = AMOUNT`;
   - templates with expressions;
   - `for last …`, `covers`, `prorated`;
   - escalations, with `Contract::amount_on(day)`;
   - `due SPAN else ITEM`, and `grace`;
   - `deposit`;
   - loans with `resets` and `prepay shortens|recasts`, interest and principal derived given every earlier occurrence, and a loan that began before the book opening at the schedule's balance;
   - pro-rata refunds when a `covers` promise ends early.

   Escrow and match are `also` lines now, which the engine evaluates.
7. **Claims:** `owes` both ways, with items; the opening's `X owes Y`; `^code now due`; `^code waived` as a write-off; recognition in accrual books.
8. **Assets:** purchase, parts, sale with its costs, `basis … since`, and `ends`.
9. **Statements by verb:**
   - `=` values (balances, prices, readings);
   - `worked` and `used` as measures;
   - `now` changes (terms, properties, budgets, `^code now due/until`, claim amendments by items);
   - `settled`, `void` and `returned`;
   - `split`, `basis`, and `filed`;
   - a subject that cannot take its verb is an error listing the verbs it takes.

**Verify:** the sketch builds and gives the README's figures:
- January interest 1,527.88 and principal 365.02;
- the phone share 27.00;
- sales tax 138.09;
- exchange cost 0.39;
- March rent 2,918.60, with its 18.60 `#utilities` item;
- the gym at 120.00 under the promotion extended to 06-30;
- figma waived in March;
- the itemized invoice settled on 02-26;
- every claim.

Plus the journal refusal table.

**Performance:** load and build at 1M flows, with a v4 converter or generator kept in `bench/`. It may regress at most 10% from `bench/REPORT.md`.

**Size:** the whole model at or below 7,500 lines (it is 7,584 on main today).

### 4.5 E4b: engine semantics (main, from `c488ad7`)

Brief: [lane-E4b-engine-semantics.md](briefs/lane-E4b-engine-semantics.md). Build every semantic on hand-made books in `fixture.rs` and `tests.rs`; v3 books and goldens stay byte-identical.

1. **Flows with an owner and a purpose.** Tallies key by owner. `Var::Purpose` and `Var::Description` evaluate. `is` tests the purpose tree, including `of`, and `total(#P, window)` reads a purpose's total.
2. **Purpose laws** (`Trigger::Flow`) fire per posted flow with a purpose, with the owner as subject. Asset laws fire through `of`. Purpose totals are signed in the purpose's own direction, and only the purposes a law reads are watched.
3. **Budgets:**
   - `limits: Timeline<Limit>` gives the limit in force on the window's first day;
   - `Limit::Share` is a share of another purpose's total;
   - `carries` compares running totals;
   - the `Cap` shortcut extends to budgets.
4. **Properties by day:** `prop(props, name, day)`.
5. **Assets:**
   - parts carried beside the asset's single parcel (no second store);
   - `consume` lowers basis, never below zero, and records `Adjustment::Consumed`;
   - timed laws of an asset kind run per part;
   - `.cost`, `.basis` and `.in-service` per part;
   - a sale relieves the sum of the parts' basis, less the sale's costs;
   - `Derivation::Disposal`;
   - `Run.assets` is filled.
6. **`carry … to UNIT within SPAN`:** a wash sale, with the holding period carried over and `Adjustment::Carried`.
7. **Parties, tabs and the market.** Remove every `Class::Income`, `Expense` and `Equity` assumption.
8. **Claims settle** first by the codes a flow names, then by the exact amount, then oldest first. `PaidFor` and `WriteOff` are posted; a write-off in accrual books reverses what was recognized.
9. **Promises as one monitor** (LANGUAGE §7, residuation):
   - contract due days and claim due days are one structure with a deadline, someone to blame, and what settles it;
   - `grace`;
   - `due … else ITEM` adds `Derivation::Otherwise`;
   - `warning[late]`; kept-late occurrences are recorded;
   - a missing income promise is a claim;
   - `Run.promises` is filled, and forks carry it.
10. **`also`, evaluated per flow:** from its contract, party, kind chain and purpose chain, with `when` and amounts on the law evaluator and the flow in context. The result is `Derivation::Also`, and a written line with the same ends and purpose replaces it.
11. **Measures** are events: purpose laws fire on them, `total` counts them in their unit, and `on in`/`on out` never fire.
12. **`against`:** a refund takes its original's purpose and object, stops unused recognition, and lowers a part's cost.
13. **Owner sets:** a business's tallies reach its owners in their `owned_by` shares.

**Constraints:**
- no regression from E4a's numbers (§6);
- v3 books pay nothing;
- at most +900 lines.

**Needs from the model:** list them in the report for M4a and M4b.

### 4.6 E4c: units, norms and diagnostics (main, after E4b)

Brief: [lane-E4c-engine-units-norms-diagnostics.md](briefs/lane-E4c-engine-units-norms-diagnostics.md).

- **Units at run time:**
  - `value(x, U [at POLICY])` at the owner's system's `rates` (`spot`, or `param NAME`), recording the rate used;
  - tallies and gains in the owner's currency;
  - a missing rate is a `Missing` fault reported once.
- **Norms:**
  - `unless`;
  - `require A else B else C`, where a repaired violation is a note;
  - respect `Law.rank`;
  - `days`, `peak`, `low` and `open`.
- **Filed returns:** `warning[amended]` with each changed line and the flows that changed it.
- **Diagnostics** (LANGUAGE §12; beat [limabean.md](briefs/limabean.md)):
  - A failed assertion shows its window as a table: day, other end, amount, running balance, purpose.
  - The suspect row is marked. `Suspect` grows variants, ranked by how exactly each explains the gap:
    - a contract occurrence due and not written, with a related label on the schedule line and the fix `01 flat`;
    - a claim settled off the book;
    - a derived flow written again by hand;
    - a pending flow that settled;
    - a flow dated just past the assertion.
  - Derived flows label the declaration that derived them.
  - Late promises offer both fixes ("record it", and "`waived`").
  - Budgets label their top contributors and offer both fixes (an amendment, or `!`).
  - `note[terms]` fires after three occurrences in a row off by the same amount.
  - Unsolvable `?` amounts name their cycle.
  - Run-time unit errors name both units.

**Constraints:**
- no regression from E4b;
- at most +700 lines;
- the mistakes corpus is regenerated, with two before/after cases in the report.

### 4.7 SY2: bind sync to the book (v4, after M4a and §4.1)

Nobody has briefed this yet. What it must do:

1. **One representation.** The model's `sync::{Format, Spec, Column, Rule, Field, Pattern, Op, CharClass, Capture}` (from M4a) are the only declaration types. The sync crate depends on `axiom-model` and deletes its own copies:
   - `Field`, `Format`, `Spec`, `Place`, `Rule` and `Shape` in `format.rs`;
   - its `Pattern`, `Op` and `CharClass`;
   - its text compiler for patterns (`peg.rs`'s `Parser`), since the syntax parses patterns and the model lowers them;
   - `Format::check`, since the model's builder reports declaration errors.

   What stays in the crate is runtime:
   - the readers (CSV, tagged, dates through `core::DateLayout`, amounts);
   - the matcher over `&[Op]`, and the `Recognizer` compiled from the book;
   - `Record` and `Facts`;
   - reconciliation, promises, the world pipeline, writing, sinks, the diff, commands, the session, and `unrecognized`.

   Expect the crate to shrink by several hundred lines.
2. **The adapter.** One `bind` module, reading `&Book` and `&Run`. This table is from the crate's docs:

   | the book says | the crate takes |
   |---|---|
   | entities, accounts and their `known_as` (never `me`) | `Known` (`account` for a place) |
   | named patterns | `Patterns` |
   | `CodeRule.known_as` | codes |
   | flows on an account, each leg of a split, derived flows | one `Existing` each, in its unit |
   | a batch of flows sharing a code | `Batch` members and total |
   | due occurrences within grace, minus kept promises | `Due` |
   | open claims with a code | `World.claims` |
   | the commodities held | `World.units` |
   | `book.sources` | `Source`, `Feed`, `Format`, `Sink` |

3. **Writing goes through `axiom_syntax::format`,** the house style, instead of the crate's own single-spaced rendering.
4. **`crates/sync/std/formats.ax` moves into `crates/systems/src/std.ax`.** Its camt053 gains `memo AddtlNtryInf, RmtInf/Ustrd`, which the spec's version lacks: add that to LANGUAGE §14.
5. **`check` lists unrecognized memos** (with R4, §4.2 item 4).
6. **Decide the spec questions the lane raised:**
   - Is there a code prefix in a format's `code` declaration? OFX `CHECKNUM` is written `^1041` today, not `^check-1041`.
   - A memo that carries an original amount and currency (for example "APPLE STORE … CHF 3,290.00") needs an `original:` capture.
   - `via` in a format is implemented as the named party, with the memo's party as the go-between. Confirm it.
   - Tagged formats have no statement-level balance: should §14 give them one?

**Size:** bring the crate toward its brief's ~900 lines. A realistic floor after the unification is about 2,000.

### 4.8 Y4-A and Y4-B: systems and examples (v4, after M4b)

Brief: [lane-Y4-systems-examples.md](briefs/lane-Y4-systems-examples.md). They write Axiom, not Rust; toolchain bugs go in the report as minimal repros, and every workaround is marked `// WORKAROUND:`.

**Y4-A:**
- `us` and its subsystems (`401k`, `ira`, `hsa`, `529`, `ca`, `ny`, `nyc`, `san-francisco`) in v4, with the new `us/rental`: `rental-home`, depreciation by `straight-line` and `consume` per part, and a wash-sale law with `carry`.
- Every v3 return figure is unchanged for the same facts.
- Examples 01–05 rewritten in v4, with every verify script agreeing.
- The sketch becomes the runnable `examples/11-sam`, checked by `verify11.py`, with its illustrative figures corrected to the truth.

**Y4-B:**
- Examples 06–10 in v4.
- The mistakes corpus: cases converted to v4, and cases v4 makes impossible replaced with v4's own mistakes: a purpose typo, disagreeing purpose sources, a short date with no context, `#code` for `^code`, a party-to-party leg, an unknown contract, a missed promise, `.basis`.
- Outputs regenerated and regraded in a new `REPORT.md` section.

Also rewrite the X5 ledgers in `examples/explore-v5/` against the S5 syntax, or retire them into the numbered examples. They predate it and no longer parse: they use `csv` lines in sync, `DATE thing prop value` without `now`, `from X into Y` schedules and bare price lines.

### 4.9 Integration: v4 into main (last)

1. Merge main into `v4` one last time, then `v4` into main with `--no-ff`.
2. **Delete every `// v3 bridge:`:**
   - the engine's `bridge.rs` (`V3`, `Sides` from path roots, `change_basis`), `Moves::Basis` and the `unreachable!("{V3}")` arms;
   - the report's v3 seams;
   - any `PathRoot` or `v3_root` left.

   Audit finding 9 estimates about 130 lines in the engine and report, besides the model's.
3. **Goldens:** regenerate them with `sh tests/golden.sh` and justify every diff.
4. **Checks that must agree or pass:**
   - every `examples/verify/verifyNN.py` agrees with its README;
   - `sh tests/mistakes/run.sh` is regenerated and regraded;
   - `cargo test --workspace --release` is green.
5. **Benchmarks** (`bench/run.sh`, `bench/REPORT.md`):
   - `check` at 1M flows under 1 s;
   - no regression beyond noise from §6's numbers;
   - peak RSS reported.
6. **Update PLAN.md:** §12's lane table, sizes and decisions, and close task "Integration: goldens, benchmarks, mistakes regrade".

### 4.10 After v4: open, not assigned

- **`axiom lsp`.** Purpose provenance as inlay hints (the user asked for this: "if we make an LSP you could see it as a hint"), hovers from `why`, and code actions from each diagnostic's fixes. `check --json` already carries fixes as edits.
- **A diagnostics pass over the v4 mistakes corpus** once real books build.
- **The formatter's scope.** `axiom fmt` lays out flows, statements, legs and items. Declarations, contracts, laws and opening claim lines are left verbatim.
- **Findings still open** (`examples/FINDINGS.md`):
  - F21: owner scope in reports;
  - F23: same-day flow order changes what laws see (07's half month of depreciation before the sale);
  - F28: a running maximum, days abroad. `peak` and `days` in E4c should close it.
  - Partly done:
    - F10 (`available` does not net a mortgage);
    - F13 (a forecast past a loan's balance);
    - F15 (a reimbursement counted as income: `against` should close it);
    - F26 (budget rollover: `carries` closes it);
    - F27 (calendar windows: C5's `Window` may close it).
- **B13:** cutting a recognition range short.
- **P9: a parallel fold of independent owners.** E4a analysed it and declined:
  - the Amdahl ceiling is about −19% on 4 cores, and about −12% after merge costs;
  - it helps only books with many independent owners;
  - it needs over 300 lines and risks byte-identical output.
- **Smaller `Flow` and `Txn`.** Page faults are about a quarter of CPU at 1M. The model's test holds `Flow` at 200 bytes or less; M4b's codes table and `Run<Sym>` help.
- **Engine leftovers from E4a:**
  - a per-commodity price memo, which needs a public `rescale` or rate-taking converter from the model;
  - a widened `total(…, kind)` still scans every place per evaluation, though no shipped system uses one.
- **Loose ends from S5, C5 and SY:**
  - S5 left a few clippy style lints.
  - `run` and `into` raw lines now end at a `//` after a blank. This is new behaviour: document it in §14.
  - `Severity::Note` in a `require` is meaningless: restrict it to Error and Warning.
  - `Implied::Flow { from, to }` holds `Option<Id<Place>>`.
  - SY noted that a statement-level balance for tagged formats is undone.

## 5. The size budget: needs a decision

`briefs/common.md` sets a cap of **24,000** non-test lines for the whole workspace at the end of v4, counted with `python3 briefs/loc.py v2`.

**Now, per branch:**

| crate | main `c488ad7` | on its branch |
|---|---|---|
| cli | 1,759 | 2,115 (R4), 1,784 (SY) |
| core | 1,491 | — |
| engine | 4,643 | — |
| model | 7,584 | — |
| report | 3,741 | 4,389 (R4) |
| syntax | 3,097 | 5,308 (v4) |
| sync | — | 3,032 (SY) |
| systems | 12 | — |
| **total** | **22,327** | |

**Projection** if everything lands as briefed:

| item | lines |
|---|---|
| main today | 22,327 |
| syntax | +2,211 |
| report and cli | about +1,000 |
| sync | +3,050 |
| E4b | +900 |
| E4c | +700 |
| the model to its 7,500 target | −84 |
| the v3 bridge in engine and report | −130 |
| **total** | **about 30,000** |

That is about 6,000 over the cap. The cuts on the table:
- **SY2's unification:** −500 to −1,000.
- **The report's v3 seams:** −200 to −400.
- **A tighter formatter:** `style.rs` is 514 lines.
- **Holding E4b and E4c to their caps.**

Even with all of them, the likely end is about 27,000. That is still about 38% of v1's 71k, and it covers far more. **The user should either raise the cap or name what to drop.** Every lane so far missed its size target, and every overrun was reviewed and accepted as features and diagnostics rather than padding.

## 6. Performance baselines (do not regress)

**Main `c488ad7`, on the generated bench projects:**

| measurement | result |
|---|---|
| `check`, 100k: instructions (callgrind Ir) | 912–917 M, of which the engine is 229.3 M |
| `check`, 1M: wall | about 0.745 s best of 7 (single runs 0.74–0.93 s on a shared box) |
| `check`, 1M: user CPU | 1.07 s |
| `check`, 1M: peak RSS | 650–730 MB |
| `available`, 1M: user CPU | 1.32–1.41 s |

**The v4 syntax, `32826f8`:**

| input | Ir | change against S4 |
|---|---|---|
| plain slice, 15.8 MB | 604.9 M | +3.8% |
| enriched slice | 676.2 M | +4.9% |
| statements only, 300k lines | 473.3 M | +11.5% |

**SIMD:** `fearless_simd` 1.0 was measured twice (S5's name runs, SY's CSV delimiters). It did not pay either time, so it is not a dependency. The only dependency is `memchr`.

**Sync:** see §4.1.

## 7. Constraints that hold for every lane

These come from the user's own words (PLAN §1) and `briefs/common.md`:
- **Dependencies:** `memchr` only; `fearless_simd` 1.0 only where measured to pay. Stable Rust. No `Arc`, `Mutex`, `Rc` or `RefCell`. Concurrency goes through `core::par`, with immutable phase outputs asserted `Sync`.
- **Readability:** no code golf, and fewer lines only from better abstractions. Invariants live in types; facts that never change are computed once; nothing allocates per flow on a hot path.
- **Views are data;** text and JSON are renderers.
- **Diagnostics state the fact in the book's words,** with labels at the causes and fixes as edits. Never panic on user input.
- **The spec wins.** A change to a public type is made only when the spec cannot be met otherwise, and the report says so.

## 8. How the work has been run

- **Orchestration:** one orchestrating session writes briefs and reviews every line. Implementation lanes are sub-agents, each in its own git worktree, briefed by `briefs/common.md` plus a lane brief.
- **Merging:**
  - Lanes merge into main with `--no-ff`, after `cargo test --workspace --release`, `sh tests/golden.sh` and `sh tests/mistakes/run.sh`, and a push.
  - `v4` is a separate branch because the model rebuild breaks everything above it until Y4. Main is merged into `v4` as needed.
- **Commits:** each ends with the committing harness's own attribution lines. No model is named anywhere else.
- **Container restarts** stopped long lanes three times. Every lane brief says to commit in small green steps, and a restarted lane resumes from its worktree's `git diff`.
- **The auto-mode permission classifier** denied the SY merges (§4.1) and E4a's dropping of its own stash entry. Neither was worked around: they are the user's calls.

## 9. Definition of done for v4

- main holds the v4 toolchain with no `// v3 bridge:` left.
- Every example builds and `check` is clean apart from what it teaches.
- Every `verifyNN.py` agrees.
- `examples/11-sam` reproduces the sketch's README.
- The mistakes corpus is regraded.
- `check` at 1M flows is under 1 s.
- The size is at or under the cap the user settles (§5).
- PLAN.md is current.
