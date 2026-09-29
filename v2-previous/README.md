# Axiom V2

One source ledger, package-defined economics, reproducible books, and durable
history. V2 is a standalone implementation; it does not call the V1 engine in
production. V1 is used only as a differential test oracle.

```sh
cargo run --offline --manifest-path v2/Cargo.toml -- check v2/examples/clean_ledger.axm
cargo run --offline --manifest-path v2/Cargo.toml -- view v2/examples/clean_ledger.axm tax
cargo run --offline --manifest-path v2/Cargo.toml -- why v2/examples/clean_ledger.axm sale_1
```

Start with dated entries. Types, shared units, and defaults follow from the
reusable entries in the imported package:

```text
ledger household
use personal

2026-01-04 buy first_purchase
  account brokerage
  units 10 ABC
  cost 200 USD

2026-09-20 sell first_sale
  account brokerage
  units 4 ABC
  proceeds 120 USD
  lot @first_purchase
```

There are no schema declarations. The unit identifies the asset or currency once;
an omitted fee is inferred as zero in the entry's currency. Arithmetic remains exact,
including fractional quantities and basis. References are explicit: matching
amounts, dates, or parties never invent an identity or payment allocation.

Write `lot ?lot` to leave a decision open. Checking reports the admissible
alternatives, missing requirements, conflicts, or an exhausted work budget.
An unresolved row does not erase unrelated checked results. A source decision
resolves a particular hole without overwriting the original evidence:

```text
decide identify_first_lot
  target first_sale.lot
  value @first_purchase
```

## Economics belongs in packages

[personal.axm](models/personal.axm) defines purchases, sales, invoices, payments,
allocations, and cash/tax/accrual books. It uses the exact same parser and compiler
as [community_rewards.axm](models/community_rewards.axm):

```sh
cargo run --offline --manifest-path v2/Cargo.toml -- \
  view v2/examples/payroll.axm payroll_book --model v2/models/community_rewards.axm
```

The kernel has no investment, invoice, payment, or payroll enum. A package contains
ordinary entries with named holes, plus finite rules. The open occurrence makes
an entry reusable. For example:

```text
package bonus

?date award ?award
  amount ?amount ?currency

rule recognize_award
  for a award
  require (gt (number a.amount) 0)
  emit recognized_award
  set date a.date
  set amount a.amount
  book rewards

book rewards
  include recognized_award
```

`for`, `let`, `choose`, `require`, and `emit` compose with exact arithmetic,
typed comparisons, explicit references, `rows`, `filter`, `map`, `all`, `any`,
`sum`, `sort`, `list`, `pairs`, and `at`. Several rules can contribute to the
same relation. Dependencies are acyclic and declared by package imports.
Payment lifecycle validation is ordinary package code over adjacent history
pairs; it has no privileged core operator.

Books interpret the same checked world. Package rules may derive consequences;
they cannot author observations, change the ledger, or accept an unchosen branch.
Checking establishes consequences under the pinned package definitions. It does
not certify that a community package's economic or legal policy is appropriate.

## Reopenable history

```sh
# Copy the revision ID printed by this command.
cargo run --offline --manifest-path v2/Cargo.toml -- \
  commit v2/examples/clean_ledger.axm --repo /tmp/axiom-v2-demo

# Substitute that ID for REVISION below.
cargo run --offline --manifest-path v2/Cargo.toml -- \
  close REVISION tax --repo /tmp/axiom-v2-demo --from 2026-01-01 --to 2026-12-31

cargo run --offline --manifest-path v2/Cargo.toml -- verify --repo /tmp/axiom-v2-demo
```

`commit --parent REVISION` creates a correction. `restate CLOSE NEW_REVISION`
preserves the book and period and requires a direct correction under the same
model. Old source and closes remain independently reopenable. `show CLOSE`
replays and displays a close. `history --repo PATH` lists the stored revision and
close addresses. `--partial` explicitly records unresolved results;
it never turns them into a complete close.

Each revision is a self-contained immutable object holding exact source, pinned
package sources, parent, and replay certificate. Closes store their revision,
book, period, report root, and optional prior close. There is one canonical codec
and one CAS insertion/read path. Writes use synced temporary files and atomic
no-replace insertion. Reads verify canonical bytes, hashes, dependencies, and
semantic replay. A corrupt existing object is never overwritten.

A period is an inclusive filter over dated results of the exact revision. It is
**not an as-of reconstruction** that pretends later evidence was unknown. The
payment example reports the final state of an authored history, not every bank
movement in that history. A transaction cash-flow package would need explicit
movement occurrences. Close completeness means no unresolved result in the
selected source/model/book scope; it does not assert all real-world events are
known. Content addresses detect missing referenced objects; detecting deletion
of an otherwise unreferenced tip requires retaining its address externally.

## What makes the implementation smaller

- One fixed block parser serves ledger and package source.
- Reusable entry patterns infer typed records; a finite algebra replaces
  domain-specific engines.
- Successful claims are results; only unresolved cases need separate findings.
- Shared read sets certify complete relation dependencies once, rather than
  copying every input into every derived claim.
- An opaque checked world feeds one book-view implementation. Reporting does not
  re-run source checking or maintain another accepted ledger.
- Two durable object kinds cover history and closes.

Certificates currently use complete deterministic replay. They bind exact source,
models, ordered coverage, claims, outcomes, and semantic roots. This is an
independent verification entry point, **not a separate small proof algorithm**.
The same bounded interpreter is the trusted semantic implementation. Source
comments change the raw revision while retaining the semantic world. Package
source bytes are deliberately pinned exactly, including package comments.

## Verification and performance

```sh
cargo test --locked --offline --manifest-path v2/Cargo.toml --all-targets
cargo clippy --locked --offline --manifest-path v2/Cargo.toml --all-targets -- -D warnings
cargo test --locked --offline --manifest-path v2/lab/Cargo.toml
```

The tests cover mixed ledgers, explicit ambiguity and resolution, partial-lot
arithmetic against V1's reference evaluator, community packages, phase boundaries,
forged certificates, corruption, correction lineage, and real CLI process restarts.

[The layout lab](lab/RESULTS.md) compares map-backed rows, dense layouts, exact
numeric arenas, and typed columns on up to one million synthetic facts. Its
smallest layout uses 22.6× less requested live heap than its map baseline. Safe
handles and typed columns produce the improvement; production code uses no
unsafe blocks. These measurements concern storage/query prototypes, not a
million-row production checker. The production expression evaluator now borrows
relation rows and lexical bindings. A measured 100,000-row query used 212× fewer
allocations and ran 7.8× faster than its owning reference path, with the same result
and remaining work budget; see [the reproduction notes](lab/PRODUCTION.md).
Canonical storage still owns values. The checker has explicit source/work bounds,
and correlated relational scans can remain quadratic. The columnar layouts are
prototypes, not a claim that production storage has already migrated to SoA.

The V1 CLI remains available. V2 has a new grammar and object format; this is
not a compatibility reader for all V1 syntax or a port of its research modules.
The first package supports explicit single-lot partial disposals. General lot
splitting, FIFO/LIFO inventory planning, automatic migration, and arbitrary
recursive rules are not implemented by this package.
