# Transactions, open patterns, checked consequences

The ledger is the authored authority. A reusable definition is an ordinary ledger
entry with named holes. There is no authored schema language. Types and field
structure are compiler results inferred from patterns, values, and rules.

## One surface

An ordinary entry:

```text
ledger household
use personal

2026-01-04 buy first_purchase
  account brokerage
  units 10 ABC
  cost 200 USD
```

Its reusable pattern, in package source:

```text
package personal

?date buy ?buy
  account ?account
  units ?units ?asset
  cost ?cost ?currency
  fees 0 ?currency
```

The final header hole captures occurrence identity. It is not another data field.
`@first_purchase` references a concrete occurrence; `@?buy` in a pattern constrains
a reference to the pattern whose occurrence capture is `buy`. It never selects
a particular buy.

The date prefix is generic: `DATE KIND ID` or `?date KIND ?id`. The package's
header capture names the field: `?date` supplies `date`, while `?issued` supplies
`issued`. An undated header plus that explicit field is equivalent. A conflicting
date, or a dated entry whose pattern has no date capture, is an error. No economic
word is a parser production. Raw bytes/spans are retained.

Named IDs may be omitted for unlinked everyday entries. Generated `entry/1` names
follow source entry order, ignoring trivia. They are local occurrences, not
inferred economic identity; explicit IDs support links and corrections involving
insertions/reordering. Equal values never merge occurrences.

## Pattern inference

- `?name` captures a canonical typed value. Pattern structure constrains its shape;
  rules infer output types and check remaining dynamic requirements during evaluation.
- `?amount ?unit` captures an exact quantity. Repeated unit captures must agree.
- `0 ?currency` supplies an omitted exact zero fee in the inferred currency.
  Explicit nonzero fees are allowed: concrete values provide defaults/examples,
  not equality predicates.
- Concrete literals infer a type and supply defaults.
- `@?buy` supplies a reference constraint to the buy pattern, not a default target.
- Lists/records carry ordinary values and holes. An open list `[?event]` permits
  any number of elements; it is not a one-element default. Structured elements
  constrain shape recursively.
- Collect captures from supplied fields before filling omitted fields. Definition
  order must not change inference.
- An underdetermined field remains a named hole. Unrelated entries still check.

Captures are local to each application; they never infer relationships across
occurrences. User-entry holes remain open values. Only a package entry with a
hole in its occurrence position defines a reusable pattern.

No authored `form` or `fact` declarations remain. Internal inferred shapes still
reject unknown fields and wrong types. Unconstrained values retain intrinsic
bounds and reference-existence checks. A literal dangling reference cannot become
a checked output.

## Composable rules

```text
rule purchase_basis
  for b buy
  require (gt (number b.units) 0)
  emit purchase_value
  set purchase (ref b)
  set date b.date
  set amount (add b.cost b.fees)
```

The output needs no declaration; its shape comes from `set` expressions. Several
rules can produce one relation when their fields/types agree. Consumers wait for
all producers. Programs are acyclic. Package dependencies govern visibility;
a model set may pin several dependency closures.

The finite algebra includes exact dimension-aware arithmetic, comparisons,
`if`, `rows`, `filter`, `map`, `all`, `any`, `sum`, `sort`, `contains`,
`concat`, `list`, `pairs`, `at`, fields, and explicit references.
Lifecycle checking is package code over adjacent pairs and transition tables;
there is no lifecycle operator in Rust.

`rows pattern` and `get value field` take static bare names. Other expression text
is quoted; unbound names are errors. Binders are lexical. Values, nesting, and work
are bounded. Exhausted work is Incomplete, never false.

`choose` checks candidate identity AND contents against actual relation rows.
An open selector explores remaining constraints within the bound and reports
viable alternatives. It does not silently choose. `decide ID` with `target`
and `value` resolves a source hole, retaining provenance.

## Authority and history

Authored, candidate, and recognized claims are phases of one result representation.
Rules cannot author observations or feed recognized book results back into facts.
Standard/community patterns use the same compiler; new domains add no Rust enums.

Complete deterministic replay checks exact pinned inputs, all claims, findings,
coverage, and roots. This is a replay verifier, not a separate theorem prover.
Shared read-set witnesses encode full relation dependencies once. Incomplete
relations cannot silently become complete aggregates over their surviving rows.
Success is a claim; findings are unresolved results only.

A World is opaque and created by checking. A View consumes that World and its
matching model, without rechecking a mutable document. Books are pure.
A period filters dated outputs of a revision; it is not as-of reconstruction.
The standard payment model describes final state, not every cash-flow movement.

Raw revision identity preserves bytes and parents. World identity excludes ledger
trivia and lineage, but binds normalized values, decisions, outcomes, and models.
Exact package sources, including comments, are pinned in the model identity.

Two durable kinds: self-contained revisions (source, package sources, parent,
certificate) and closes (revision, book, period, report root, prior close).
Canonical decode, hashes, and replay share a validation path. Restatement requires
a direct correction with the same model/book/period. Old closes remain verifiable.
Completeness is scoped to selected inputs, never all real-world events.

## Efficiency

No world sorting per insertion, full relation dependencies copied into every
result, or repeated whole-world checking just to render books. Execution may use
borrowed rows, slots, and typed columns while canonical interchange values stay
unchanged. Fast paths must preserve outcomes, ordering, and proof roots.

The layout lab measures one million synthetic records; this is not a claim of
one-million-row production verification. Safe handles and typed columns already
remove most layout overhead. Unsafe code must earn its cost.

# Axiom rewrite assessment

Yes—a comprehensive rewrite is justified.

But it should not be a “throw everything away and recreate it” rewrite. Axiom already contains excellent laws, algorithms, adversarial tests, and domain knowledge. The rewrite should preserve those assets while replacing the architecture that currently forces them into several competing semantic systems.

The core diagnosis is:

> Axiom has many strong parts, but too many equally plausible centers.

The new system should have exactly one center:

```text
ledger source
  → typed forms
  → canonical claims
  → checked world
  → book interpretations
  → reports, explanations, and immutable closes
```

Everything should either participate in that path or remain explicitly experimental.

The source ledger remains the final authored source of truth. Packages define vocabulary and deterministic economic laws; they do not acquire authority to invent facts. A checked world is a reproducible consequence of the ledger, its pinned model set, and explicit decisions—not a second mutable ledger.

No files were changed for this assessment.

## Executive recommendation

Rewrite Axiom around seven singular concepts:

1. One authoring grammar.
2. One pinned package/model system.
3. One canonical typed claim representation.
4. One phase and outcome model.
5. One independently checkable certificate calculus.
6. One durable repository and object codec.
7. One public workflow from source to close.

Preserve the strongest existing semantic laws, but stop preserving architectural duplication merely because it already has tests.

The rewrite should proceed as a strangler-style vertical replacement:

- Build one small, complete V2 path alongside V1.
- Use the existing engine, reference evaluator, and tests as differential oracles.
- Port laws and domains, not class hierarchies and public APIs.
- Cut over the CLI only once one genuinely representative mixed ledger works end to end.
- Delete old production paths aggressively after parity.

That is materially safer than either endless incremental layering or a blind greenfield rewrite.

---

# Where the repository stands

The current checkout is:

- Branch: `canonical`
- HEAD: `afb3cc5`
- Approximately 71,267 lines of Rust under `src/`
- Approximately 16,512 lines of Rust under `tests/`
- Approximately 87,779 Rust lines overall
- 35 publicly exported modules
- Roughly 784 top-level public declarations
- 637 `#[test]` attributes

Some particularly large components are:

| Component | Approximate size |
|---|---:|
| `engine.rs` | 6,882 lines |
| `ontology.rs` | 4,890 |
| `store.rs` | 4,775 |
| benchmark executable | 4,182 |
| `reference.rs` | 3,586 |
| `package_compiler.rs` | 3,457 |
| `proof.rs` | 3,176 |
| semantic constitution test | 5,326 |

The size is not inherently the problem. The problem is how much of it defines parallel foundations rather than contributing to one product path.

The implementation ledger is commendably honest about the remaining gaps: [IMPLEMENTATION.md](/Users/mileswirht/Downloads/axiom/IMPLEMENTATION.md). The public framing is in [README.md](/Users/mileswirht/Downloads/axiom/README.md). I treated both [confirmed-direction.md](/Users/mileswirht/Downloads/axiom/confirmed-direction.md) and [other.md](/Users/mileswirht/Downloads/axiom/other.md) as design inputs, not authorities.

## Current uncommitted state

The tree currently contains:

- User-owned changes to [other.md](/Users/mileswirht/Downloads/axiom/other.md)
- An interrupted mixed-surface experiment in:
  - [src/parser.rs](/Users/mileswirht/Downloads/axiom/src/parser.rs)
  - [src/workspace.rs](/Users/mileswirht/Downloads/axiom/src/workspace.rs)
  - [tests/workspace_package_forms.rs](/Users/mileswirht/Downloads/axiom/tests/workspace_package_forms.rs)

The experiment compiles under:

```text
cargo check --locked --offline --all-targets
```

Its behavior is useful but narrow: validated package-form blocks can coexist syntactically with built-in `buy` and `sell` blocks. The workspace identifies the package form nodes and lets the built-in parser skip those ranges.

That is syntax coexistence, not semantic unification. Package forms still do not become part of `model::Ledger` or affect ordinary sale analysis. I would not merge that patch as the architectural answer. It may provide useful fixtures or a temporary bridge, but it currently reinforces the split between built-in and package semantics.

---

# What works today

Axiom has two real end-to-end paths.

## Built-in sale path

```text
source bytes
  → RawEvidence / stored Evidence
  → source Commit
  → lossless SurfaceFile
  → ParsedLedger
  → model::Ledger
  → engine::Analysis
  → generic proof DAG
  → analysis artifact and child Commit
  → sale Close
```

This is the primary everyday path in [src/workspace.rs](/Users/mileswirht/Downloads/axiom/src/workspace.rs) and [src/engine.rs](/Users/mileswirht/Downloads/axiom/src/engine.rs).

## Package settlement path

```text
source Commit + compiled package artifact
  → generic form elaboration
  → SettlementStateV1 projection
  → dedicated settlement certificate
  → SettlementWorld
  → observation or cash recognition
  → SettlementCloseObject
```

This is a substantive second vertical, with a notably stronger replay discipline in [src/settlement_proof.rs](/Users/mileswirht/Downloads/axiom/src/settlement_proof.rs).

These paths are individually useful. They are not one system.

There is also a third recognition architecture in [src/recognize.rs](/Users/mileswirht/Downloads/axiom/src/recognize.rs). Its generic `AcceptedWorld`, attribute-based facts, recognizers, and close workflow are not the built-in engine world and are not the typed settlement world.

That is the central architectural fracture.

---

# What should absolutely be preserved

A rewrite should not discard the project’s best work.

## Exactness and determinism

The existing emphasis on exact arithmetic, units, canonical encodings, deterministic content hashes, and reproducible results is correct. These should remain non-negotiable.

No floating-point convenience should enter the trusted economic layer.

## Immutable evidence and correction lineage

Raw source bytes, occurrence identity, correction lineage, and the distinction between history and current interpretation are foundational strengths.

A correction should create a new revision and world without erasing the old one. Old closes must continue to reopen and verify.

## Explicit ambiguity

The engine’s refusal to silently guess a lot, payment relationship, or economic identity is essential.

Axiom should remain willing to say:

- there are multiple valid answers;
- required information is missing;
- available facts conflict;
- a result could not be completed within a declared resource bound.

That is far more valuable than merely producing an answer.

## Strong domain laws

The sale/lot, obligation, settlement, satisfaction, return, and recognition logic embodies substantial domain knowledge. Preserve the laws and conformance vectors even if their current type hierarchy is retired.

## Independent checking

The best current architectural pattern is the typed settlement certificate:

- exact source binding;
- exact compiled artifact binding;
- exhaustive ordered coverage;
- independent replay;
- refusal to accept unproven extra material.

The generic rewrite should expand that pattern rather than treating the more permissive generic proof metadata as sufficient authority.

## Adversarial and differential assurance

The heavy, adversarial, metamorphic, and differential tests are valuable. The reference implementation should survive as a test oracle, even if it moves out of the public runtime surface.

---

# The root causes of complexity

The issue is not merely large files. It is conceptual duplication.

## Several representations of authored meaning

Today Axiom contains overlapping representations such as:

- lossless source/CST nodes;
- parser statements and `model::LedgerForm`;
- HIR and package IR;
- elaborated package records;
- `ontology::EventGraph`;
- `semantics::Statement<P>`;
- engine-specific analysis structures;
- `recognize::AcceptedWorld`;
- typed settlement-world facts.

Some separation between syntax, typed form, and canonical semantics is healthy. Having multiple public semantic destinations is not.

The rewrite should have one deliberate sequence:

```text
CST → typed document → canonical claim graph
```

Each phase should have a distinct purpose and no competing sibling representation.

## Several meanings of “accepted”

“Accepted” currently appears in multiple conceptual systems:

- model phase wrappers;
- ontology accepted facts;
- semantics phases and resolutions;
- `recognize::AcceptedWorld`;
- engine recognition results;
- settlement-world recognition.

That makes phase boundaries difficult to reason about and makes accidental authority escalation more likely.

There should be one phase graph:

```text
Authored/Observed
        ↓
    Candidate
        ↓ explicit checked resolution
     Accepted
        ↓ pure interpretation under a book
 Recognized<Book>
```

The exact names can change. The one-way boundary cannot.

In particular:

- Recognition may not alter accepted reality.
- Package rules may propose candidates.
- A package may not invisibly promote its own candidate into accepted truth.
- Explicit decisions must be source-visible, provenance-bearing, and checkable.

## Several proof systems

Axiom currently has:

- the general [src/proof.rs](/Users/mileswirht/Downloads/axiom/src/proof.rs) DAG;
- the stricter `SettlementStateV1Proof`;
- recognition metadata;
- engine-specific semantic validation;
- close-specific verification logic.

The generic proof DAG mixes genuinely typed operations with string-like metadata and domain-specific conventions. This is adequate for explanations and some invariants, but too permissive to serve as the only authority boundary for community-defined economics.

The rewrite needs one typed certificate format whose meaning does not depend on the caller remembering extra validation steps.

## Several close concepts

There are distinct close workflows for:

- built-in sale analysis;
- typed settlement recognition;
- generic recognition.

A close should be one concept:

> An immutable, independently verifiable commitment to a source revision, semantic world, exact model set, selected book, reporting boundary, completeness status, certificate roots, and lineage.

New domains should not require new stored close types.

## Several package systems

There are currently distinct notions of package identity and behavior:

- the small executable FIFO/LIFO policy system;
- package manifests and lock resolution;
- HIR-to-artifact compilation;
- compiled schemas and templates;
- stored policy packages;
- source commits with both package IDs and optional compiled-artifact roots.

These do not yet have one demonstrated equivalence relation.

More importantly, package HIR is generally constructed by Rust callers. The manifest body is hashed, but it is not yet the ordinary source language from which executable economic definitions are compiled. The CLI has no complete compile, lock, pin, check workflow for package authors.

Community extensibility is therefore present as a substantial foundation, but not yet as a simple product experience.

## Superficial incrementality

The custom incremental runtime has meaningful dependency tracking machinery, but several named stages currently cache partition bytes and dependency shapes while the report query still recomputes the whole ledger through the main engine.

Calling nodes “valuation,” “position,” “settlement,” and “recognition” does not make them independent incremental computations.

This is a high-cost abstraction without corresponding product behavior. V2 should begin with correct coarse content-addressed caching and introduce fine-grained queries only where profiling proves value.

## Research islands in the public architecture

Modules for accounts, collaboration, contracts, liquidity, logic, scenarios, semantics, ontology, unification, recognition, and LSP contain interesting work. Much of it is exercised through Rust-level tests and benchmarks rather than through source → workspace → checked result.

Those tests demonstrate foundations, not an integrated user experience.

Research code is not bad. Making every research foundation part of the public kernel is expensive.

---

# The durability gap

This deserves special emphasis.

The current `ObjectStore` in [src/store.rs](/Users/mileswirht/Downloads/axiom/src/store.rs) is an in-memory `BTreeMap`. Objects have canonical bytes and hashes, but the system has no complete decoder/open/save/reload path. The CLI in [src/main.rs](/Users/mileswirht/Downloads/axiom/src/main.rs) creates a fresh workspace for each invocation.

Consequently, “persisted” currently means stored in the current process’s object store. It does not yet mean:

- close the process;
- reopen the repository;
- verify historical source and closes;
- detect corruption;
- continue a correction lineage.

That should be fixed before adding many more in-memory authority object families.

A real ledger needs durable history more urgently than it needs another abstract world representation.

---

# What to take from the two design directions

Neither existing plan should be implemented wholesale.

## From the confirmed direction

Keep:

- immutable evidence;
- exact occurrence and correction identity;
- ambiguity as a first-class result;
- open-world completeness;
- explicit decisions;
- multiple books over shared accepted facts;
- strict phase separation;
- independently checkable proof;
- recognition that matching amounts and dates do not establish identity.

Avoid:

- implementing every logical foundation before one complete product path;
- expanding the number of authoritative intermediate objects;
- allowing the accepted-world layer to become a separately mutable ledger;
- treating every theoretical capability as a V1 kernel requirement.

## From `other.md`

Keep as inspiration:

- one familiar authoring surface;
- progressive enrichment rather than separate “simple” and “advanced” languages;
- content-addressed definitions;
- package-defined economic forms;
- pure, non-mutating book interpretations;
- lawful projections;
- flexible tools may propose, but may not mint authority.

Do not take into the first rewrite:

- a universal “form does everything” abstraction;
- a broad set-theoretic type engine;
- full inductive-family and ornament machinery;
- a general-purpose query or scripting runtime in the trusted core;
- Revo, C ABI, or Zig runtime requirements;
- UI lenses and updatable projections;
- migrations, importers, editors, and collaboration all embedded into the definition of a form.

Those could eventually be valuable. They are currently additional trusted and operational boundaries, not demonstrated simplifications.

The best synthesis is much smaller:

> One fixed grammar, package-defined typed schemas and total rules, one canonical claim graph, and a small replay checker.

---

# The target user experience

The authoring surface should feel like one language forever.

A representative file might look like:

```text
ledger household

use personal
use us-tax

buy buy/one
  date 2026-01-04
  quantity 10 ABC
  into brokerage
  cost 200 USD
  fee 1 USD

sell sell/one
  date 2026-09-20
  quantity 10 ABC
  from brokerage
  proceeds 500 USD
  lot ?lot

invoice invoice/17
  debtor customer/17
  creditor vendor/17
  amount 100 USD
  due 2026-10-01

payment payment/17
  from customer/17
  to vendor/17
  amount 100 USD
  state issued at 2026-09-01
  state settled at 2026-09-03

satisfy allocation/17
  obligation invoice/17
  settlement payment/17
  amount 100 USD

decide tax.us/sell-one
  select sell/one.lot = buy/one
```

Underneath, `buy`, `sell`, `invoice`, `payment`, and `satisfy` are names or aliases exported by pinned model packages. They are not new parser productions.

The grammar can normalize this to a uniform internal form such as:

```text
entry buy/one : investing.acquisition
  ...
```

The important property is that package installation does not install a parser. It installs definitions understood by the same parser and compiler.

## Progressive disclosure

For most people:

```text
use personal
buy ...
sell ...
check
```

For more sophisticated users:

- qualify form names;
- inspect the exact model lock;
- use typed holes;
- author explicit decisions;
- request different books;
- inspect claims and proof steps;
- define a community package.

The basic surface does not change when sophistication increases.

## Uniform outcomes

Every check and query should return the same small result family:

```text
Proven(value, certificate)
Alternatives(options)
Missing(requirements)
Conflict(conflicting_claims)
Incomplete(resource_limit)
```

No subsystem should invent another mixture of `Option`, special enums, error strings, and partially recognized state.

Diagnostics can render these outcomes differently, but the semantic result stays uniform.

---

# The target semantic architecture

## 1. One lossless grammar

The parser should know:

- document and block structure;
- identifiers;
- field syntax;
- literals;
- lists and records;
- typed holes;
- package qualification;
- source spans and trivia.

It should not know the economic meaning of `buy`, `invoice`, or `payment`.

Package schemas provide that meaning after parsing.

This removes the current built-in parser versus generic form-elaborator split.

## 2. One package and model-set identity

A model package should contain separate declarations under one identity:

- form/record schemas;
- canonical field names and aliases;
- bounded pure lowering rules;
- derived predicates;
- book interpretations;
- conformance examples;
- optional syntactic aliases that desugar through the fixed grammar.

The model lock determines one exact transitive closure. That closure receives a `ModelSetId`, and every checked revision, world, certificate, and close binds to it.

Packages must be:

- deterministic;
- content-addressed;
- total or explicitly resource-bounded;
- free of I/O, clocks, network access, mutation, and hidden global state;
- unable to mint accepted facts without a kernel-authorized rule.

Built-in forms should eventually be standard packages compiled by the same compiler as community forms. Otherwise there will always be a privileged second language.

## 3. One canonical typed claim graph

Avoid both extremes:

- a giant Rust enum with a variant for every future community domain;
- a generic string-to-string attribute map.

A claim should instead reference a content-addressed predicate definition:

```text
Claim {
  predicate: DefinitionId,
  arguments: CanonicalTypedValues,
  phase: Phase,
  provenance: Provenance
}
```

The predicate definition specifies the exact argument schema. The kernel validates the values against it.

Core values need only include things such as:

- exact numbers;
- quantities with units;
- dates and times;
- stable IDs and references;
- booleans and text;
- lists and records;
- typed holes.

Core relations should remain small:

- equality;
- exact arithmetic;
- unit compatibility;
- temporal ordering;
- membership;
- explicit reference and identity;
- conservation or balance laws.

Economic meaning lives in model packages. The kernel only needs the primitives required to check their rules.

## 4. One identity model

The current rewrite is an opportunity to separate several identities that are presently too easily conflated.

| Identity | Meaning |
|---|---|
| `RevisionId` | Exact authored bytes, including comments and formatting |
| `DefinitionId` | One content-addressed package definition |
| `ModelSetId` | Exact locked transitive package closure |
| `OccurrenceId` | Stable authored occurrence across correction lineage |
| `ValueId` | Canonical normalized typed value, where useful |
| `ClaimId` | Canonical proposition plus appropriate provenance |
| `WorldId` | Checked claims, decisions, and model set |
| `ViewId` | World plus book/interpretation policy |
| `CloseId` | Immutable report/period commitment |

This creates an important simplification:

- Proofs and closes can bind both the raw `RevisionId` and semantic `WorldId`.
- Adding a comment produces a new raw revision.
- If semantic meaning is unchanged, semantic caches and world results can be reused.
- Audit history remains exact without making whitespace invalidate every downstream semantic object.

That distinction is both more rigorous and more efficient.

## 5. One checked-world boundary

The checked world should be a reproducible manifest, not a copied mutable fact database.

Conceptually:

```text
WorldId =
  hash(
    RevisionId,
    ModelSetId,
    normalized authored claims,
    explicit decisions,
    verified derived claims
  )
```

It should be impossible to insert an accepted claim directly into storage without a source occurrence or replayable certificate path.

Importers and AI tools may produce draft forms or proposed decisions. They remain outside the trusted computing base until their output enters the ledger and is checked normally.

## 6. Books as pure views

A book should not own or mutate facts.

A book is a pure interpretation:

```text
ViewId = interpret(WorldId, BookDefinitionId, parameters)
```

Cash, accrual, tax, management, and observation views can therefore disagree without duplicating the source ledger.

The current top-level singular `book` assumption should disappear from semantic authority. A user may configure a convenient default view, but the same world must support many books.

## 7. One certificate calculus

The general certificate model can be compact:

```text
Step {
  rule: RuleId,
  inputs: [ClaimId],
  output: ClaimId,
  witness: TypedWitness
}

Bundle {
  roots: [ClaimId],
  steps: [Step]
}
```

The independent checker:

1. Loads the exact content-addressed rule definition.
2. Validates all input and output schemas.
3. Replays the total rule against the witness.
4. Checks coverage and ordering where the rule requires them.
5. Confirms that all roots are derivable.
6. Rejects unused or unbound authoritative material where completeness matters.

Search, optimization, heuristics, and even external solvers may be used to discover an answer. They must emit a certificate checked by this smaller kernel.

Human explanation is rendered from the certificate. Explanation text is never itself authority.

## 8. One durable object model

The durable repository needs only a small set of generic object families:

- blobs;
- package definitions and model-set manifests;
- source revisions;
- certificate bundles;
- checked-world manifests;
- closes.

Evidence, decisions, purchases, payments, and settlements should normally be typed ledger forms and claims—not new top-level storage kinds.

Every object needs:

- canonical encoder;
- canonical decoder;
- version tag;
- structural validation;
- content-hash validation;
- referential validation;
- corruption tests;
- reopen tests.

Insertion, lookup, repository reopening, and whole-store verification must use the same validator.

The physical backend could be immutable blob files with a small transactional index, SQLite-backed storage, or another local CAS. The exact choice matters less than having a real decoder and restart-safe repository.

---

# Simplicity laws for the rewrite

These are the constraints that keep the system simple as it becomes more powerful.

1. A new economic domain adds no parser production.
2. A new form adds no Rust enum variant to the kernel.
3. A new book adds no new world or close type.
4. A new package uses the same compiler as the standard library.
5. A new authoritative result must be expressible through the same certificate calculus.
6. A new source block follows the same diagnostics and typed-hole behavior.
7. A package can derive candidates but cannot silently create accepted truth.
8. Equal amounts, dates, and parties never imply identity or satisfaction.
9. Recognition is pure and cannot modify the accepted world.
10. Every authoritative object survives process restart and independent verification.
11. Warm and clean evaluation produce identical semantic IDs and certificates.
12. A second accepted-world type, package identity, or close hierarchy is a design failure, not an expedient extension.

These laws are more important than a target line count.

---

# What to keep, rewrite, and retire

| Area | Recommendation |
|---|---|
| Exact arithmetic and unit laws | Keep concepts and tests; choose one canonical representation |
| Lossless lexer/CST | Keep the strong source-preservation core |
| Evidence and correction laws | Keep; consolidate the two evidence schemas |
| Content hashes and canonical encoding | Keep; add decoding, versions, and durable verification |
| Sale/lot algorithms | Port behind canonical claims and package definitions |
| Obligation/settlement laws | Port; use explicit relationships only |
| Settlement replay certificate | Generalize its discipline into the shared checker |
| Reference evaluator | Keep as test support and differential oracle |
| Heavy/adversarial fixtures | Keep and route through the new public workflow |
| `surface` + `parser` + `model` | Rewrite into one syntax-to-typed-document compiler |
| HIR/IR/package elaboration | Consolidate into the same compiler pipeline |
| Package, package-lock, compiler, stored policy types | Replace with one model-package and lock system |
| Engine-specific and settlement-specific worlds | Replace with canonical claims and one checked world |
| Generic proof and special settlement proof | Replace with a typed generic certificate plus domain rules |
| Sale close, settlement close, generic close | Replace with one close |
| Store and workspace | Rewrite around a durable repository and small project API |
| Custom placeholder incremental stages | Remove; reintroduce only as real typed queries |
| `recognize::AcceptedWorld` | Quarantine; do not promote it into storage authority |
| Unused phase wrappers | Retire once useful laws are represented canonically |
| Ontology trait-object graph | Keep only as a derived/query view if still useful |
| Accounts, contracts, liquidity, scenarios, collaboration | Move to packages, experimental code, or test support until integrated |
| Logic and unification foundations | Keep outside the kernel unless a real vertical requires them |
| LSP foundations | Rebuild over the actual compiler rather than a parallel HIR |
| Giant benchmark response schema | Replace with a small stable measurement record |

Large modules should not be deleted solely for being large. They should lose production authority unless they fit the single path.

---

# Suggested code organization

Do not begin by manufacturing many crates. First establish private boundaries in one crate and allow the boundaries to prove themselves.

A minimal conceptual structure is:

```text
syntax/       fixed lossless grammar and source diagnostics
model/        canonical values, definitions, claims, phases, IDs
compiler/     package compilation and source elaboration
check/        rule replay and certificate verification
world/        resolution and checked-world construction
view/         pure book interpretations
repository/   codecs, durable CAS, lineage, closes
project/      small public orchestration API
cli/          check, why, view, close, package commands
models/       standard economic packages as data/source
```

Once stable, the kernel can become a tiny no-I/O crate:

```text
axiom-kernel   canonical values, IDs, codecs, rule checker
axiom          compiler, project, repository, views
axiom-cli      commands and rendering
```

The public Rust surface should be closer to:

```text
Project
Revision
CheckResult
World
View
Close
Diagnostic
TypedId
```

It should not expose every intermediate theory as a peer public subsystem.

A reduction from roughly 71,000 production lines toward 20,000–30,000 integrated Rust lines plus declarative standard packages appears plausible, but it should be treated as a directional expectation rather than a quota.

---

# Rewrite sequence

## Phase 0: Freeze and classify

Before V2 code:

- Record current V0 source-to-result behavior.
- Separate source-level product tests from Rust-level foundation tests.
- Preserve reference and conformance fixtures.
- Record canonical encodings that have a real compatibility requirement.
- Decide whether the current mixed-surface patch is archived as an experiment or reduced to reusable tests.

Exit condition: we know which behavior is contractual and which code is merely exploratory.

## Phase 1: Write the V2 constitution

Write a short, sharp architecture document defining:

- ledger authority;
- phases;
- identity;
- explicit relationship rules;
- package powers and prohibitions;
- books;
- outcomes;
- proof and completeness;
- correction lineage.

Also write the representative ledger before implementing the compiler. If the architecture cannot express the flagship file simply, the abstractions are wrong.

Exit condition: the product can be explained without referring to current module names.

## Phase 2: Implement the kernel and repository

Implement:

- canonical typed values;
- predicate and rule definitions;
- the identity model;
- versioned encoding and decoding;
- certificate replay;
- durable storage;
- reopen and corruption tests.

Do not implement the whole economic universe.

Exit condition: a tiny hand-constructed claim world can be stored, closed, reopened in another process, and independently verified.

## Phase 3: Implement the single compiler

Implement:

- fixed block grammar;
- lossless CST;
- package source parser;
- model-set lock;
- schema elaboration;
- typed holes;
- canonical claim lowering.

Port a few built-in forms as standard packages. FIFO and LIFO should become ordinary package definitions or package rules, not an unrelated executable package family.

Exit condition: a community package can add a typed form without modifying Rust parser or kernel code.

## Phase 4: Build one flagship vertical

The fixture should include:

- an investment acquisition;
- a disposal with ambiguous lot selection;
- an obligation;
- a payment with issued, settled, returned, and re-presented states;
- an explicit satisfaction allocation;
- a source correction;
- cash and tax/accrual books;
- typed holes;
- one conflict;
- one ambiguity that does not block unrelated facts.

It must support:

- `check`;
- `why`;
- at least two views;
- an immutable close;
- restart and reopen;
- correction lineage;
- proof replay.

Exit condition: one ledger demonstrates the whole architecture.

## Phase 5: Differential migration

For domains already handled by V1:

- run V1 and V2 against the same fixtures;
- compare normalized outcomes;
- compare exact amounts and selected lots;
- compare proof-law coverage;
- document intentional semantic differences.

Only port algorithms that remain necessary. Do not port public wrappers merely for API parity.

Exit condition: V2 matches or deliberately improves the required V1 behavior.

## Phase 6: Honest incrementality

Begin with coarse immutable caches:

```text
parse(RevisionId)
type_form(NodeId, ModelSetId)
lower(FormId)
derive(RuleId, InputClaimIds)
world(RevisionId, ModelSetId)
view(WorldId, BookId)
```

Split them only after profiling demonstrates recomputation cost.

Required invariant:

```text
clean result == warm result
clean certificate root == warm certificate root
```

Exit condition: cached execution is semantically indistinguishable from clean execution.

## Phase 7: Cut over and delete

Once the flagship path and migrated domains are stable:

- make V2 the default CLI path;
- retain a narrowly scoped V1 compatibility reader only where real external artifacts require it;
- stop writing V1 objects;
- remove duplicate worlds, closes, package representations, and fake incremental stages;
- shrink the public module surface.

Deletion is part of the rewrite, not optional cleanup for later.

---

# The decisive acceptance test

The rewrite is successful when a single user-authored mixed ledger can demonstrate all of this:

- The same source contains investing, billing, payment, and satisfaction forms.
- At least one domain comes from a community-style package.
- The kernel required no Rust change for that new domain.
- A lot ambiguity produces explicit alternatives.
- Unrelated valid facts remain usable despite that ambiguity.
- A typed hole reports exactly what information is missing.
- Payment and obligation are linked only through an explicit satisfaction occurrence.
- Identical amount/date/party combinations do not create implicit identity.
- Cash and tax views differ without duplicating or mutating the accepted world.
- `why` traces a result to source spans, package definitions, decisions, and certificate steps.
- A correction creates a new revision, world, and close.
- The old revision and close still reopen and verify.
- The repository survives process restart.
- Corruption is detected.
- Clean and cached runs produce identical world, view, and certificate roots.
- The CLI, library API, and editor diagnostics consume the same compiler and outcomes.

Until that works, broad theoretical expansion should wait.

---

# Explicit non-goals for the first rewrite

To keep the project ambitious without making it boundless, V2 should initially exclude:

- a general set-theoretic type system;
- inductive families and ornaments;
- arbitrary package scripting;
- Revo or a second execution runtime in the trusted core;
- distributed evaluation;
- higher-order theorem proving;
- million-proposition scale claims without measurements;
- updatable report lenses;
- automatic semantic identity inference;
- broad collaboration cryptography;
- sophisticated UI-specific projection machinery;
- every research module currently present.

These are not permanently rejected. They must earn entry through a concrete user need and the one canonical path.

---

# Risks and controls

## Rewrite drift

Risk: V2 becomes another research branch while V1 continues to grow.

Control: one flagship source file and one executable end-to-end gate drive every phase.

## Excessive generalization

Risk: the package language becomes a new programming language and solver platform.

Control: schemas, total bounded rules, pure interpretations, and typed witnesses only. Add kernel primitives reluctantly.

## Lost domain correctness

Risk: rewriting architecture discards subtle sale and settlement behavior.

Control: differential evaluation, existing reference fixtures, and certificate conformance vectors.

## Compatibility paralysis

Risk: every internal V1 hash or object shape becomes permanent.

Control: distinguish actual published formats from internal transient objects. V1 currently lacks durable decoding, so theoretical internal compatibility should not force pervasive branches. Preserve explicitly documented artifacts and supply deliberate migration lineage.

## A second “temporary” architecture

Risk: a bridge object or alternate accepted bundle becomes permanent.

Control: any temporary layer gets an explicit removal gate. No second world, proof, or close system is accepted as the V2 endpoint.

---

# Bottom line

Axiom should be rewritten.

The rewrite should not aim to preserve today’s module architecture. It should preserve today’s best laws:

- exactness;
- provenance;
- immutable history;
- explicit ambiguity;
- explicit identity;
- deterministic packages;
- independently replayable proof;
- many books over one accepted reality.

The simplest powerful version of Axiom is not a tiny feature set. It is a system where every feature composes through the same few concepts:

```text
one ledger
one grammar
one model set
one claim graph
one checker
one durable history
many community-defined economic systems
many pure books
```

That is the ambitious direction: not merely fewer lines, but making entire categories of future complexity impossible.
