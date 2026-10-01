You are **lane M4a (model: declarations and laws)** of the Axiom v4 rework. First read `v2/briefs/common.md`. It is part of this brief.

The model is rebuilt on the v4 syntax in two lanes: you, then lane M4b, which elaborates the journal (flows, contracts, claims, assets, statements) on what you leave.

**Read first, in full:**
- `v2/LANGUAGE.md` (v4, normative) and `v2/DESIGN.md`;
- `v2/examples/v4-sketch/`, all of it, with its README;
- the v4 AST in `crates/syntax/src/ast.rs` (lanes S4 and S5), and lane S5's notes in `v2/briefs/` (`notes-S5-syntax.md`) if the orchestrator left them;
- the public types in `crates/model/src/{book,journal,law}.rs`, with annotated sources in `v2/briefs/types-v4.rs` and `types-v4b.rs`;
- `v2/briefs/audit-core-syntax-model.md`: findings 1, 2, 3, 5, 6, 7 and 13 are part of your work list.

**You own `v2/crates/model/**`** (declarations, names, kinds, places, owners, purposes, props, laws, budgets, sources), `v2/crates/systems/src/std.ax` (ported to v4), and the model's tests.
- The engine and report lanes are adapting in parallel on main, against the public types. Change a public type only when the spec cannot be met otherwise, and say so in your report.
- `us` and the examples are ported by a later lane. Only `std` loads by default in your tests.
- Goldens are expected to break on this branch: say which, and why.

## The pipeline

The shape carries over:
1. a survey;
2. declarations, properties, params, laws and governance;
3. parallel elaboration of the journal (M4b's);
4. laying out.

Delete the v3 model as you go (finding 1): `PathRoot`, `v3_root`, plans, `basis_end`, via-resolution, path-root places, and the positional kind constants. There is no bridge on this branch: read the v4 AST directly.

1. **Names** (finding 3, generalized). One lookup: `Scoped<T>::lookup(names, scope, text) -> Result<Id<T>, Miss<T>>`, with qualifier and nearest-name ranking in one place, and one `unresolved` diagnostic builder. It serves kinds, params, entities, accounts, assets, purposes, contracts and laws.
   - A name that means two things across namespaces is an error at the later declaration, naming both.
   - The one exception: a contract may share its party's name. `contract netflix` is with the entity `netflix`.
   - Parties undeclared in the journal are allowed. They are untyped, and get an `Outside` place on first use. A near miss of a declared party or account is an error with the suggestion.
   - A purpose name that looks like a code (it has digits, or matches a `code` glob) suggests `^name`.
2. **Kinds** (finding 5).
   - `Kind { name, system, traits: Traits, has, props, laws, doc, loc }`, where `Traits` is one of `Account(..)`, `Thing`, `Commodity { pays }` or `Entity(PartyKind { purpose, sales_tax, shares })`.
   - Named `KindRoots { asset, debt, thing, commodity, entity }`: no positional indexes.
   - `Sort`, `Target::of` and `PropTable::family` collapse onto the variant.
3. **Places.**
   - An account's class comes from its kind's root: `asset` or `debt`.
   - `at` gives `Role::Account { institution }`.
   - Every owner gets a `Holding` place, and every party an `Outside` place. `Role::Outside` now always names an entity: `?`, the opening and the market are built-in entities.
   - Tabs are made on demand by M4b, through one function you provide.
   - Every asset gets a place for its unit.
4. **Owners:**
   - `me`;
   - household members;
   - entities with `owner`;
   - the owners of accounts and assets.
   
   Every other entity is a party. A client `of` an owner makes its payments that owner's.
5. **Purposes.**
   - Build the tree: the roots, std's nodes, and the project's.
   - Handle `of KIND`, and `business N% for OWNER` on purposes.
   - A party's own `#purpose`.
   - Kinds' `purpose`, `pays` and `takes`.
   - Provide M4b one function, `infer(flow facts) -> Option<Purposed>`, implementing LANGUAGE §2's order with `Provenance`, including `Provenance::Entity`, and reporting a conflict naming both sources.
6. **Properties** (findings 6 and 7).
   - Build them in `Vec`s and freeze them once.
   - Kind defaults apply by reference, not by copying.
   - Rows carry `since` (`Day::MIN` for declarations). M4b adds dated rows from statements through one function you provide, `set(thing, prop, days, value, change)`.
   - Where a property line was written is kept inline as `At<T>`, not in a string-keyed side table.
   - `known-as` fills `known_as`.
7. **Laws** (findings 2 and 13).
   - Triggers are spelled once: a `Moment` view of `Trigger`, with rule tables indexed by it.
   - The law IR's nodes are an `Arena<Node>`. A poisoned node is `ty: None`, not a parallel `Vec<bool>`.
   - `Require` carries a `Severity`, not `warn: bool`.
   - One `FUNCS` table drives the names, arities, roles and types of functions.
   - Compile:
     - `on flow` (implicit inside a purpose);
     - `consume` and `carry`;
     - `Var::Purpose` and `Var::Description`;
     - `purpose is P of X`;
     - the asset fields (`.cost .basis .in-service .parts`);
     - `straight-line(...)`, a pure function giving this period's share;
     - `total(#P, window)`.
   - Govern laws in purposes (`Rules.purposes`, ancestors included, in dependency order), in assets and asset kinds (`Rules.about` and `timed`), and in contracts.
8. **Budgets.** `budget PURPOSE LIMIT period [carries]` gives a `Budget`, whose `limits` start as a timeline with the declared limit, plus its law (`warn total(window) <= limit`) with `Law::budget` set.
   - `N% of #P` gives `Limit::Share`.
   - M4b paints later limits from statements.
9. **Sources.** `sync NAME` gives a `Source`.
   - A NAME that is an account gives a `Sink::Feed`.
   - `into PATH` gives `Sink::File`, and `into param NAME` gives `Sink::Param`.
   - A sync with neither gives `Sink::Journal`.
   - `csv` arguments decode into `Csv`, `Money` and `Column`, with a diagnostic for each malformed argument.
10. **Dates and files** (LANGUAGE §10). The loader gives `parse` each file's `Folder`, and headings refine it (the syntax does that).
    - Delete every check that a file's dates agree with its path, and the `layout free` handling.
    - Only `systems/` is special.
    - The mistakes cases for misfiled dates (93, 96, 99) become non-errors: leave a note for the corpus lane.
11. **std in v4.** Rewrite `crates/systems/src/std.ax`:
    - **the purpose tree:**
      - under spending: food (groceries, dining), home (rent, utilities, household), phone, transport, health, fun, insurance (premium), interest, fees, closing-costs, selling-costs, gifts, tax-paid, sales-tax, exchange-cost;
      - under income: wages, interest, dividend, rent, gifts;
      - under capital: purchase, improvement, sale;
      - repair.
    - **party kinds with their purposes:** grocer, restaurant, landlord, phone-company, utility, employer, tax-authority, insurer, lender, store, contractor, tenant, client, fund, gym, streaming, software.
    - **account kinds:** deposit, card, brokerage, cash, escrow, loan.
    - **asset kinds:** property, vehicle, equipment, personal-use.
    - **commodity kinds** with `pays`.
    - **envelopes and grants.**
    
    `examples/v4-sketch/std-sketch.ax` shows the intended shape.

## Verify

- The sketch's declarations build with std, plus a minimal `us` stub in its `systems/` folder naming only what it uses: `irs`, `ftb`, `ssa`, `edd`, `foreign-tax`, `401k`, `rental-home`, and the purposes `payroll-tax`, `premium` and `business-income`.
- M4b builds the journal. Until then, test with the journal files excluded, or with the elaboration stubbed to nothing, and say which.
- Small `.ax` tests for each feature above, and the declaration part of the refusal table, carried over from v3 and extended with v4's refusals:
  - a name in two namespaces;
  - an unknown purpose that looks like a code;
  - a purpose missing its required object kind;
  - a budget on an unknown purpose;
  - a malformed `csv`;
  - `known-as` on a kind (it belongs on things).
- **Size:** the model is about 9,300 lines on this branch, v3 included. When M4b is done, the whole model should be at or below 7,000. Your half should leave the declaration side clearly smaller than it is.

## Where you work

Work in `/home/user/axiom/.claude/worktrees/lane-v4`, on branch `v4`. Start every shell command with `cd /home/user/axiom/.claude/worktrees/lane-v4`. Commit in steps, each with its tests. Your report must end with a precise handoff for M4b:
- the functions it calls (tabs, infer, set, lookups);
- the `World` state it extends;
- what is stubbed.

## Revision: the spec at 5f15e35 and types-v4c

This brief predates the spec's third pass. Read LANGUAGE.md at 5f15e35 or later in full, with `theory.md` and `v2/examples/explore-v5/FINDINGS.md` for why. Where this brief and the spec differ, the spec wins. On your side, add:

- **Kinds.**
  - Party kinds carry `purpose` (money to them) and `pays` (money from them).
  - Commodity kinds rooted at `measure` (`HR`, `MI`, `KWH`, `SQFT`) are never held: an account that `holds` one is an error.
  - Assets may be `part of` another.
  - Measures such as `area` are declared with `has NAME UNIT`.
- **Entities.**
  - `owner A 60%, B 40%` gives `owned_by` shares, and on accounts, `Place.shares`.
  - `currency`, resolved as own, else the residence system's, else base.
  - `citizen`, and `books cash|accrual`.
  - Every entity and account is `known-as` its own name. Compile `known-as` PEG patterns, `pattern NAME = …` and `code … known-as` into `Pattern` programs.
- **Purposes.** Four roots: add `transfer`. Between two owners, a flow needs a transfer purpose; that check is M4b's, but provide the query.
- **Units** (LANGUAGE §8). `Ty::Amount(Dim)`, checked bottom-up, with no inference:
  - literals, params (`param NAME UNIT`) and properties carry units;
  - `amount` is the subject's commodity when `holds` names one, else `Dim::Any`;
  - `+`, `-` and comparisons need equal dims;
  - `*` and `/` use `Dim::mul`/`div`;
  - a tally takes its owner's currency;
  - `value(x, U [at POLICY])` is the only conversion.
  
  A mismatch is an error naming both units, with the conversion as the fix. Systems' `currency` and `rates` lines fill `System`.
- **Norms.**
  - `unless EXPR` becomes a step.
  - `require … else E else F` is a chain of reparations.
  - `law NAME overrides NAME`.
  - Among laws of one name governing one subject, the most specific applies: thing > kind > parent kind; project > child system > parent system. Compute `Law.rank`. Two of equal rank are an error naming both. This replaces file shadowing in `systems/`.
- **`also`.** Compile `also ITEM | FLOW [when EXPR]` on contracts, entities, kinds and purposes into `Also`, with its expressions in a law node arena and no steps, and index them by `AlsoOn`. The engine evaluates them per flow (lane E4b): you only compile and index.
- **Budgets:** `funded from H into H`.
- **Functions:** `days(COND, window)`, `peak(x, window)`, `low(x, window)`, `open(^code)`, and `x up to y` as min, with their types.
- **Sources:**
  - `read`/`run`;
  - `format` declarations (CSV columns with every `Field`, and tagged records with paths);
  - category mappings;
  - `into`.
- **std** gains:
  - `format ofx` and `format camt053`;
  - the measure units;
  - the transfer purposes: gift-received, distribution, contribution, reimbursement, loan, withholding, sales-tax-collected, rebate;
  - kinds with `pays`: insurer (claim-payout), processor (payout), card-issuer (rebate);
  - kinds envelope, tax-authority and plan-administrator;
  - `escrow` and `match` purposes: contracts now write escrow and matches as `also` lines.

**Size:** the model must still end, with M4b, at or below 7,500 lines (raised from 7,000 for units, norms and `also`). Pay for it with the audit's deletions.

**Notes from lane C5, which applied types-v4c.** Take these three decisions:
- The sync types live in `axiom_model::sync`, and `Money` is gone: `Csv` has `columns`. Remove `Sink::Feed`'s `csv`, since `Source.format` says it.
- Delete `Contract.matching` and `Match`: a match is an `also` line.
- `Terms::is_waived()` is `template.is_empty()`, which reads a loan with no template as waived. Make waiving an explicit flag or state on `Terms`.

## Revision 2: the syntax is S5's, and the shape of formats and patterns

The v4 branch is at **455f819**: S5's syntax plus main e643ea5 (lane C5's types). **Read `notes-S5-syntax.md`** for the AST you build from. The model does not build on this branch yet: you rebuild it.

**Formats.** `Shape::Tagged.fields: (Field, Box<[Sym]>)` and `Csv` cannot carry what §14 declares. Replace `Csv` and `Shape` with this shape, so that one declaration reads either kind of export:

```rust
pub struct Format { pub name: Sym, pub shape: Shape, pub specs: Box<[Spec]>, pub categories: Box<[(Sym, Id<Purpose>)]>, pub loc: Loc }
pub enum Shape { Rows, Tagged { records: Sym } }
/// One line of a format: a field, where it is, and how to read it.
pub struct Spec { pub field: Field, pub places: Box<[Column]>, pub layout: Option<DateLayout>, pub rule: Rule, pub loc: Loc }
pub enum Column { Header(Sym), Index(u16), Path(Sym) }          // Path: `BookgDt/Dt`, or a tag at any depth
pub enum Rule { None, Flipped, Sign { place: Column, into: Sym }, Is(Sym) }
```

- Only `memo` may name several places (`memo NAME, MEMO`: what they say in turn).
- `Flipped` and `Sign` go on `amount`, and `Is` on `pending` (`pending Sts PDNG`).
- A csv format names headers or indexes, from 1. A tagged format names paths.
- `date` needs a layout or reads ISO.
- `debit` and `credit` go together.
- There must be an `amount`, or `debit` and `credit`, or `gross`.

Each of these is a diagnostic at the line, at build time, not when sync runs. Drop `Sink::Feed`'s `csv`.

`DateLayout` is new in `axiom_core::calendar`. It is a compiled `"MM/DD/YYYY"`-style layout: fields `YYYY YY MM M DD D` and literal separators, with `read(&str) -> Option<Day>`, `Display`, and `swapped()` (day and month exchanged, for the help "if the day comes first…"). Keep it small. You may add it to core.

**Patterns.** Lower S5's `Pattern { choices }` AST into `sync::Pattern { name, program: Box<[Op]>, loc }`, with this encoding exactly, because the matcher will run it as written:
- Ops run in order. `Repeat { min, max, len }` and `Capture { name, len }` take the next `len` ops as their body.
- `Choice { len }`: the next `len` ops are one way, and the ops after them, to the end of the enclosing body, are the others. `A / B / C` is `Choice(len A) A Choice(len B) B C`.
- A group that is a choice is wrapped in `Repeat { 1, Some(1), len }`, so that what follows the group is not taken as one more way.
- `Call(Id<Pattern>)` runs a named pattern whole. Named patterns may be declared in any order. A cycle, an unknown name or a clash is an error at the name, with "did you mean".
- `Literal(Sym)` is stored uppercase. Captures: `payee`, `code`, `amount`, `date`; any other name is `Named`.
- Nesting deeper than 32 is an error.
- Every entity and account is known by its own name too (hyphens as spaces): compile that as a literal pattern.

Test the lowering on the sketch's and §14's patterns by asserting the op lists.
